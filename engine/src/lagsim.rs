//! Feel-testing the netcode alone: the match runs through a real GGRS
//! rollback session exactly as it would online, with the bot as the remote
//! player behind a simulated network (latency, jitter, loss). Your machine
//! is peer 0 and owns the only copy of the game; peer 1 is the "other
//! machine": a bare session that carries the bot's inputs over the wire.
//! What you see - delayed own input, the remote soul predicted then
//! corrected, sounds landing late - is what an online player would get.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use ggrs::{Config, DesyncDetection, GgrsEvent, GgrsRequest, Message, NonBlockingSocket, P2PSession, PlayerType, SessionBuilder, SessionState};
use instant::{Duration, Instant};

use crate::game::{ai::Wanderer, Direct, Game};
use crate::net::NetInput;

pub struct Cfg;
impl Config for Cfg {
    type Input = NetInput;
    type State = Game;
    type Address = usize;
}

/// One direction of a simulated link.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chaos {
    pub one_way_ms: f32,
    pub jitter_ms: f32,
    pub loss: f32,
}

struct Wire {
    chaos: Chaos,
    rng: u64,
    /// (deliver at, from, to, msg); a packet never overtakes one sent before
    /// it on the same link beyond what jitter allows (UDP reorders, a little)
    queue: Vec<(Instant, usize, usize, Message)>,
}

impl Wire {
    fn r(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng % 1_000_000) as f32 / 1_000_000.0
    }
}

struct Socket {
    me: usize,
    wire: Rc<RefCell<Wire>>,
}

impl NonBlockingSocket<usize> for Socket {
    fn send_to(&mut self, msg: &Message, addr: &usize) {
        let mut w = self.wire.borrow_mut();
        if w.r() < w.chaos.loss {
            return;
        }
        let delay = (w.chaos.one_way_ms + (w.r() * 2.0 - 1.0) * w.chaos.jitter_ms).max(0.0);
        let at = Instant::now() + Duration::from_micros((delay * 1000.0) as u64);
        let (me, to) = (self.me, *addr);
        w.queue.push((at, me, to, msg.clone()));
    }
    fn receive_all_messages(&mut self) -> Vec<(usize, Message)> {
        let now = Instant::now();
        let mut w = self.wire.borrow_mut();
        let mut out = Vec::new();
        w.queue.retain(|(at, from, to, m)| {
            if *to == self.me && *at <= now {
                out.push((*from, m.clone()));
                false
            } else {
                true
            }
        });
        out
    }
}

/// Rolling counters for the readout (reset by `take_stats`).
#[derive(Default, Clone, Copy)]
struct Window {
    ticks: u32,
    rollbacks: u32,
    depth_sum: u32,
    deepest: u32,
    resim: u32,
    stalls: u32,
    cost_ms: f32,
    worst_ms: f32,
}

pub struct LagSim {
    sess: [P2PSession<Cfg>; 2],
    wire: Rc<RefCell<Wire>>,
    pub delay: usize,
    /// the bot's mind lives on "the other machine", outside the rollback
    pub mind: Wanderer,
    /// audio already played per frame, so a re-simulated frame doesn't
    /// play its sounds twice (a sound first heard in a re-simulation, i.e.
    /// a remote action we only just learned about, plays then, late)
    heard: BTreeMap<i32, Vec<String>>,
    pub cues: Vec<(&'static str, f32, f32)>,
    pub sounds: Vec<(String, f32)>,
    skip: u32,
    win: Window,
}

impl LagSim {
    pub fn new(chaos: Chaos, delay: usize, mind: Wanderer, seed: u64) -> LagSim {
        let wire = Rc::new(RefCell::new(Wire { chaos, rng: seed | 1, queue: Vec::new() }));
        let mk = |me: usize| {
            let other = 1 - me;
            SessionBuilder::<Cfg>::new()
                .with_num_players(2)
                // both ends play with the same delay, as two real players would
                .with_input_delay(delay)
                .with_max_prediction_window(12)
                .with_desync_detection_mode(DesyncDetection::Off)
                .add_player(PlayerType::Local, me)
                .unwrap()
                .add_player(PlayerType::Remote(other), other)
                .unwrap()
                .start_p2p_session(Socket { me, wire: wire.clone() })
                .unwrap()
        };
        let sess = [mk(0), mk(1)];
        LagSim { sess, wire, delay, mind, heard: BTreeMap::new(), cues: Vec::new(), sounds: Vec::new(), skip: 0, win: Window::default() }
    }

    pub fn set_chaos(&mut self, chaos: Chaos) {
        self.wire.borrow_mut().chaos = chaos;
    }

    pub fn chaos(&self) -> Chaos {
        self.wire.borrow().chaos
    }

    /// Both ends shook hands and frames are flowing.
    pub fn running(&self) -> bool {
        self.sess.iter().all(|s| s.current_state() == SessionState::Running)
    }

    /// Exchange packets and events; true once both ends are running.
    pub fn poll(&mut self) -> bool {
        for s in self.sess.iter_mut() {
            s.poll_remote_clients();
        }
        for ev in self.sess[0].events() {
            if let GgrsEvent::WaitRecommendation { skip_frames } = ev {
                self.skip += skip_frames;
            }
        }
        for _ in self.sess[1].events() {}
        self.running()
    }

    /// One 50 Hz tick once running. `local` is your input this tick.
    pub fn tick(&mut self, g: &mut Game, local: NetInput) {
        let t0 = Instant::now();
        if !self.poll() {
            return;
        }
        // the other machine: the bot reads the game and decides, and its
        // input goes out over the wire (its session needs no game state)
        let bot = self.decide(g);
        if self.sess[1].add_local_input(1, bot).is_ok() {
            if let Ok(reqs) = self.sess[1].advance_frame() {
                for r in reqs {
                    if let GgrsRequest::SaveGameState { cell, frame } = r {
                        cell.save(frame, None, None);
                    }
                }
            }
        }
        self.win.ticks += 1;
        if self.skip > 0 {
            // we're ahead of the other end: hold a tick so it catches up
            self.skip -= 1;
            self.win.stalls += 1;
            return;
        }
        self.sess[0].add_local_input(0, local).ok();
        match self.sess[0].advance_frame() {
            Ok(reqs) => {
                let now = self.sess[0].current_frame();
                let mut at = now;
                for r in reqs {
                    match r {
                        GgrsRequest::SaveGameState { cell, frame } => cell.save(frame, Some(g.clone()), None),
                        GgrsRequest::LoadGameState { cell, frame } => {
                            if let Some(s) = cell.load() {
                                *g = s;
                            }
                            let depth = (now - frame).max(0) as u32;
                            self.win.rollbacks += 1;
                            self.win.depth_sum += depth;
                            self.win.deepest = self.win.deepest.max(depth);
                            at = frame;
                        }
                        GgrsRequest::AdvanceFrame { inputs } => {
                            let i: Vec<NetInput> = inputs.iter().map(|(i, _)| *i).collect();
                            g.apply_net_inputs(&i);
                            g.resim = at + 1 < now;
                            g.frame();
                            g.resim = false;
                            at += 1;
                            if at <= now {
                                self.win.resim += 1;
                            }
                            self.collect_audio(g, at);
                        }
                    }
                }
                let keep = now - 120;
                self.heard.retain(|&f, _| f >= keep);
            }
            Err(_) => self.win.stalls += 1, // too far ahead of the remote's inputs
        }
        let ms = t0.elapsed().as_secs_f32() * 1000.0;
        self.win.cost_ms += ms;
        self.win.worst_ms = self.win.worst_ms.max(ms);
    }

    fn decide(&mut self, g: &mut Game) -> NetInput {
        // run the bot on the live game: ai_step only touches `ai` and
        // `input.direct`, both put back before the next simulated frame
        let keep_ai = g.ai.clone();
        let keep_in = g.input.direct;
        self.mind.on = true;
        g.ai[1] = std::mem::take(&mut self.mind);
        g.ai[0].on = false;
        let d = g.ai_decide()[1];
        self.mind = std::mem::take(&mut g.ai[1]);
        g.ai = keep_ai;
        g.input.direct = keep_in;
        NetInput::new(d.mv, d.aim.normalized(), d.aiming, d.dash)
    }

    fn collect_audio(&mut self, g: &mut Game, frame: i32) {
        let heard = self.heard.entry(frame).or_default();
        let mut seen = std::mem::take(heard);
        let mut was = seen.clone();
        for c in g.cues.drain(..) {
            let key = format!("c{}", c.0);
            if let Some(i) = was.iter().position(|k| *k == key) {
                was.swap_remove(i);
            } else {
                seen.push(key);
                self.cues.push(c);
            }
        }
        for s in g.sounds.drain(..) {
            let key = format!("s{}", s.0);
            if let Some(i) = was.iter().position(|k| *k == key) {
                was.swap_remove(i);
            } else {
                seen.push(key);
                self.sounds.push(s);
            }
        }
        *self.heard.get_mut(&frame).unwrap() = seen;
    }

    /// [measured ping ms, rollbacks/s, mean depth, deepest, re-simulated
    /// ticks/s, stalls/s, frames ahead, mean tick ms, worst tick ms, delay]
    pub fn take_stats(&mut self) -> Vec<f32> {
        let w = std::mem::take(&mut self.win);
        let secs = (w.ticks.max(1) as f32) * crate::game::DT;
        let ping = self.sess[0].network_stats(1).map(|s| s.ping as f32).unwrap_or(-1.0);
        vec![
            ping,
            w.rollbacks as f32 / secs,
            if w.rollbacks > 0 { w.depth_sum as f32 / w.rollbacks as f32 } else { 0.0 },
            w.deepest as f32,
            w.resim as f32 / secs,
            w.stalls as f32 / secs,
            self.sess[0].frames_ahead() as f32,
            w.cost_ms / w.ticks.max(1) as f32,
            w.worst_ms,
            self.delay as f32,
        ]
    }
}

/// Your controls as one tick of wire input: cursor aim becomes a world
/// direction from your soul (screen pixels never cross the wire).
pub fn local_input(g: &Game, d: Direct) -> NetInput {
    let mut aim = d.aim;
    if d.use_cursor {
        if let Some(k) = (0..g.players.len()).find(|&k| g.direct_slot_pub(&g.players[k]) == Some(0)) {
            if let Some(c) = g.cursor_world(k) {
                let me = g.w.position(g.players[k].node);
                aim = crate::game::v2(c.x - me.x, c.y - me.y);
            }
        }
    }
    NetInput::new(if d.active { d.mv } else { crate::game::V2::ZERO }, aim.normalized(), d.active && d.aiming, d.active && d.dash)
}
