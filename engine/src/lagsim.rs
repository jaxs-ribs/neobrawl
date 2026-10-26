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
use crate::shared::math::V3;
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

/// The real wire: the page moves packets between this queue and a WebRTC
/// data channel (unreliable, unordered: GGRS resends what matters itself).
#[derive(Default)]
pub struct JsWire {
    pub inbox: Vec<Vec<u8>>,
    pub outbox: Vec<Vec<u8>>,
}

struct JsSocket {
    remote: usize,
    wire: Rc<RefCell<JsWire>>,
}

impl NonBlockingSocket<usize> for JsSocket {
    fn send_to(&mut self, msg: &Message, _addr: &usize) {
        if let Ok(b) = bincode::serialize(msg) {
            self.wire.borrow_mut().outbox.push(b);
        }
    }
    fn receive_all_messages(&mut self) -> Vec<(usize, Message)> {
        let inbox = std::mem::take(&mut self.wire.borrow_mut().inbox);
        inbox.iter().filter_map(|b| bincode::deserialize(b).ok()).map(|m| (self.remote, m)).collect()
    }
}

/// The other machine, when it's the bot in the lag lab: a bare session
/// carrying the bot's inputs (its mind lives outside the rollback).
struct Bot {
    sess: P2PSession<Cfg>,
    wire: Rc<RefCell<Wire>>,
    mind: Wanderer,
}

/// A rollback session from your machine's side: you are handle `me`, the
/// other player is `1 - me`, either the lab bot or a real person.
pub struct LagSim {
    sess: P2PSession<Cfg>,
    pub me: usize,
    bot: Option<Bot>,
    pub js: Option<Rc<RefCell<JsWire>>>,
    pub delay: usize,
    online: bool,
    /// audio already played per frame, so a re-simulated frame doesn't
    /// play its sounds twice (a sound first heard in a re-simulation, i.e.
    /// a remote action we only just learned about, plays then, late)
    heard: BTreeMap<i32, Vec<String>>,
    pub cues: Vec<(&'static str, f32, f32)>,
    pub sounds: Vec<(String, f32)>,
    /// what happened on the link, for the page: "synced", "interrupted",
    /// "resumed", "disconnected", "desync"
    pub events: Vec<&'static str>,
    skip: u32,
    win: Window,
    /// rollback corrections being eased in on screen (node -> offset), and
    /// where each mover was last tick (to tell motion from correction)
    smooth: crate::det::HashMap<usize, V3>,
    last_pos: crate::det::HashMap<usize, V3>,
}

/// How much of a correction is still drawn after each tick (~100 ms to
/// settle), and how big a jump counts as a teleport (snapped, not eased).
const SMOOTH_KEEP: f32 = 0.55;
const SMOOTH_SNAP: f32 = 4.0;

fn builder(delay: usize, me: usize, desync: bool) -> SessionBuilder<Cfg> {
    let other = 1 - me;
    SessionBuilder::<Cfg>::new()
        .with_num_players(2)
        // both ends play with the same delay, as two real players would
        .with_input_delay(delay)
        .with_max_prediction_window(12)
        .with_desync_detection_mode(if desync { DesyncDetection::On { interval: 10 } } else { DesyncDetection::Off })
        .with_disconnect_timeout(Duration::from_millis(5000))
        .with_disconnect_notify_delay(Duration::from_millis(600))
        .add_player(PlayerType::Local, me)
        .unwrap()
        .add_player(PlayerType::Remote(other), other)
        .unwrap()
}

impl LagSim {
    /// The lag lab: you (handle 0) against the bot behind a simulated link.
    pub fn new(chaos: Chaos, delay: usize, mind: Wanderer, seed: u64) -> LagSim {
        let wire = Rc::new(RefCell::new(Wire { chaos, rng: seed | 1, queue: Vec::new() }));
        let sess = builder(delay, 0, false).start_p2p_session(Socket { me: 0, wire: wire.clone() }).unwrap();
        let bot_sess = builder(delay, 1, false).start_p2p_session(Socket { me: 1, wire: wire.clone() }).unwrap();
        let mut s = LagSim::bare(sess, 0, delay, false);
        s.bot = Some(Bot { sess: bot_sess, wire, mind });
        s
    }

    /// Online: you are handle `me`, packets go through `js`.
    pub fn online(me: usize, delay: usize) -> LagSim {
        let js = Rc::new(RefCell::new(JsWire::default()));
        let sess = builder(delay, me, true).start_p2p_session(JsSocket { remote: 1 - me, wire: js.clone() }).unwrap();
        let mut s = LagSim::bare(sess, me, delay, true);
        s.js = Some(js);
        s
    }

    fn bare(sess: P2PSession<Cfg>, me: usize, delay: usize, online: bool) -> LagSim {
        LagSim { sess, me, bot: None, js: None, delay, online, heard: BTreeMap::new(), cues: Vec::new(), sounds: Vec::new(), events: Vec::new(), skip: 0, win: Window::default(), smooth: Default::default(), last_pos: Default::default() }
    }

    pub fn set_chaos(&mut self, chaos: Chaos) {
        if let Some(b) = &self.bot {
            b.wire.borrow_mut().chaos = chaos;
        }
    }

    pub fn mind(&mut self) -> Option<&mut Wanderer> {
        self.bot.as_mut().map(|b| &mut b.mind)
    }

    pub fn take_mind(&mut self) -> Option<Wanderer> {
        self.bot.take().map(|b| b.mind)
    }

    pub fn set_mind(&mut self, m: Wanderer) {
        if let Some(b) = self.bot.as_mut() {
            b.mind = m;
        }
    }

    /// Both ends shook hands and frames are flowing.
    pub fn running(&self) -> bool {
        self.sess.current_state() == SessionState::Running && self.bot.as_ref().map(|b| b.sess.current_state() == SessionState::Running).unwrap_or(true)
    }

    /// Exchange packets and events; true once both ends are running.
    pub fn poll(&mut self) -> bool {
        self.sess.poll_remote_clients();
        if let Some(b) = self.bot.as_mut() {
            b.sess.poll_remote_clients();
            for _ in b.sess.events() {}
        }
        for ev in self.sess.events() {
            match ev {
                GgrsEvent::WaitRecommendation { skip_frames } => self.skip += skip_frames,
                GgrsEvent::Synchronized { .. } => self.events.push("synced"),
                GgrsEvent::NetworkInterrupted { .. } => self.events.push("interrupted"),
                GgrsEvent::NetworkResumed { .. } => self.events.push("resumed"),
                GgrsEvent::Disconnected { .. } => self.events.push("disconnected"),
                GgrsEvent::DesyncDetected { .. } => self.events.push("desync"),
                _ => {}
            }
        }
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
        if self.bot.is_some() {
            let input = self.decide(g);
            let b = self.bot.as_mut().unwrap();
            if b.sess.add_local_input(1, input).is_ok() {
                if let Ok(reqs) = b.sess.advance_frame() {
                    for r in reqs {
                        if let GgrsRequest::SaveGameState { cell, frame } = r {
                            cell.save(frame, None, None);
                        }
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
        self.sess.add_local_input(self.me, local).ok();
        let before = movers(g);
        let mut rolled = false;
        match self.sess.advance_frame() {
            Ok(reqs) => {
                let now = self.sess.current_frame();
                let mut at = now;
                for r in reqs {
                    match r {
                        GgrsRequest::SaveGameState { cell, frame } => {
                            // checksums only where desync detection looks
                            let sum = if self.online && frame % 10 == 0 { Some(g.checksum()) } else { None };
                            cell.save(frame, Some(g.clone()), sum)
                        }
                        GgrsRequest::LoadGameState { cell, frame } => {
                            if let Some(s) = cell.load() {
                                *g = s;
                            }
                            let depth = (now - frame).max(0) as u32;
                            rolled = true;
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
        self.ease(g, &before, rolled);
        let ms = t0.elapsed().as_secs_f32() * 1000.0;
        self.win.cost_ms += ms;
        self.win.worst_ms = self.win.worst_ms.max(ms);
    }

    /// A rollback moved things to where they really are: draw them from
    /// where they were shown (a remote throw leaves the thrower's hand, a
    /// remote soul glides back on course) and let them catch up.
    fn ease(&mut self, g: &mut Game, before: &[(usize, V3)], rolled: bool) {
        for v in self.smooth.values_mut() {
            *v = *v * SMOOTH_KEEP;
        }
        self.smooth.retain(|_, v| v.length() > 0.01);
        if rolled {
            let now: crate::det::HashMap<usize, V3> = movers(g).into_iter().collect();
            for &(n, p0) in before {
                let (Some(&p1), Some(&prev)) = (now.get(&n), self.last_pos.get(&n)) else { continue };
                if p0.x == -666.0 || p1.x == -666.0 || prev.x == -666.0 {
                    continue;
                }
                // where it would have gone at the pace it was going
                let expected = p0 + (p0 - prev);
                let c = p1 - expected;
                if c.length() > SMOOTH_SNAP {
                    self.smooth.remove(&n);
                } else if c.length() > 0.03 {
                    let s = self.smooth.entry(n).or_insert(V3::ZERO);
                    *s = *s - c;
                }
            }
        }
        self.last_pos = movers(g).into_iter().collect();
        g.net_smooth = self.smooth.clone();
    }

    fn decide(&mut self, g: &mut Game) -> NetInput {
        // run the bot on the live game: ai_step only touches `ai` and
        // `input.direct`, both put back before the next simulated frame
        let b = self.bot.as_mut().unwrap();
        let keep_ai = g.ai.clone();
        let keep_in = g.input.direct;
        b.mind.on = true;
        g.ai[1] = std::mem::take(&mut b.mind);
        g.ai[0].on = false;
        let d = g.ai_decide()[1];
        b.mind = std::mem::take(&mut g.ai[1]);
        g.ai = keep_ai;
        g.input.direct = keep_in;
        NetInput::new(d.mv, d.aim.normalized(), d.aiming, d.dash)
    }

    fn collect_audio(&mut self, g: &mut Game, frame: i32) {
        let mut seen = self.heard.remove(&frame).unwrap_or_default();
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
        self.heard.insert(frame, seen);
    }

    /// [measured ping ms, rollbacks/s, mean depth, deepest, re-simulated
    /// ticks/s, stalls/s, frames ahead, mean tick ms, worst tick ms, delay]
    pub fn take_stats(&mut self) -> Vec<f32> {
        let w = std::mem::take(&mut self.win);
        let secs = (w.ticks.max(1) as f32) * crate::game::DT;
        let ping = self.sess.network_stats(1 - self.me).map(|s| s.ping as f32).unwrap_or(-1.0);
        vec![
            ping,
            w.rollbacks as f32 / secs,
            if w.rollbacks > 0 { w.depth_sum as f32 / w.rollbacks as f32 } else { 0.0 },
            w.deepest as f32,
            w.resim as f32 / secs,
            w.stalls as f32 / secs,
            self.sess.frames_ahead() as f32,
            w.cost_ms / w.ticks.max(1) as f32,
            w.worst_ms,
            self.delay as f32,
        ]
    }
}

/// Souls and boxes in play, with their positions.
fn movers(g: &Game) -> Vec<(usize, V3)> {
    let mut out: Vec<(usize, V3)> = g.players.iter().map(|p| p.node).chain(g.boxes.iter().map(|b| b.node)).filter(|&n| g.w.active_in_hierarchy(n)).map(|n| (n, g.w.position(n))).collect();
    out.sort_by_key(|x| x.0);
    out
}

/// Your controls as one tick of wire input: cursor aim becomes a world
/// direction from your soul (screen pixels never cross the wire).
pub fn local_input(g: &Game, slot: usize, d: Direct) -> NetInput {
    let mut aim = d.aim;
    if d.use_cursor {
        if let Some(k) = (0..g.players.len()).find(|&k| g.direct_slot_pub(&g.players[k]) == Some(slot)) {
            if let Some(c) = g.cursor_world(k) {
                let me = g.w.position(g.players[k].node);
                aim = crate::game::v2(c.x - me.x, c.y - me.y);
            }
        }
    }
    NetInput::new(if d.active { d.mv } else { crate::game::V2::ZERO }, aim.normalized(), d.active && d.aiming, d.active && d.dash)
}
