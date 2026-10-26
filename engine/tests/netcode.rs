//! The netcode harness. GGRS (a Rust GGPO) drives the real game:
//! - SyncTestSession: no network; every frame it rolls back and
//!   re-simulates the last `check_distance` frames and compares checksums.
//!   Any non-determinism fails it on the frame it appears.
//! Inputs come from scripted "players" that read the game state and act
//! like people (fetch boxes, circle, charge and throw, dash), so the run
//! covers pickups, throws, kills, respawns, debris and dashes.

mod common;
use common::*;
use ggrs::{Config, GgrsRequest, PlayerType, SessionBuilder};
use neobrawl::game::{v2, Game};
use neobrawl::net::NetInput;

struct Cfg;
impl Config for Cfg {
    type Input = NetInput;
    type State = Game;
    type Address = usize;
}

/// A scripted, human-ish player for one slot.
struct Brain {
    rng: u64,
    charge_for: u32,
    charged: u32,
    heading: f32,
    turn_at: u64,
}

impl Brain {
    fn new(seed: u64) -> Brain {
        Brain { rng: seed | 1, charge_for: 30, charged: 0, heading: 0.0, turn_at: 0 }
    }
    fn r(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng % 10_000) as f32 / 10_000.0
    }
    fn input(&mut self, g: &Game, me_tag: &str, them_tag: &str) -> NetInput {
        let me = g.players.iter().find(|p| &*g.w.nodes[p.node].tag == me_tag).unwrap();
        let them = g.players.iter().find(|p| &*g.w.nodes[p.node].tag == them_tag).unwrap();
        let mp = g.w.position(me.node);
        let tp = g.w.position(them.node);
        if mp.x == -666.0 {
            return NetInput::default();
        }
        let dash = self.r() < 0.012;
        if g.frame >= self.turn_at {
            self.heading += (self.r() - 0.5) * 2.5;
            self.turn_at = g.frame + 10 + (self.r() * 40.0) as u64;
        }
        if me.picked_box.is_none() {
            self.charged = 0;
            // run for the nearest box, wandering a little
            let target = g.boxes.iter().filter(|b| g.w.active_in_hierarchy(b.node) && b.carried_by.is_none() && g.w.nodes[b.node].collider.as_ref().map(|c| c.enabled).unwrap_or(false)).map(|b| g.w.position(b.node)).min_by(|a, b| (*a - mp).length().partial_cmp(&(*b - mp).length()).unwrap());
            let (mx, my) = match target {
                Some(t) => {
                    let d = t - mp;
                    let l = d.length().max(1e-3);
                    (d.x / l + self.heading.cos() * 0.3, d.y / l + self.heading.sin() * 0.3)
                }
                None => (self.heading.cos(), self.heading.sin()),
            };
            return NetInput::new(v2(mx, my), v2(0.0, 0.0), false, dash);
        }
        // armed: circle them, charge for the distance, lead them a little,
        // and let go down a clear lane (with some human error)
        let tv = them.cmd;
        let d = tp - mp;
        let l = d.length().max(1e-3);
        let lead = l / 45.0;
        let d = neobrawl::shared::math::v3(d.x + tv.x * 8.0 * lead, d.y + tv.y * 8.0 * lead, 0.0);
        let l = d.length().max(1e-3);
        let (ax, ay) = (d.x / l, d.y / l);
        let err = (self.r() - 0.5) * 0.16;
        let (s, c) = err.sin_cos();
        let aim = v2(ax * c - ay * s, ax * s + ay * c);
        self.charged += 1;
        let dir = neobrawl::shared::math::v3(aim.x, aim.y, 0.0);
        let side = neobrawl::shared::math::v3(-aim.y, aim.x, 0.0);
        let clear = [-0.45f32, 0.0, 0.45].iter().all(|&o| g.phys.raycast(&g.w, mp + dir * 0.6 + side * o, dir, (l - 0.6).max(0.1), 1 | (1 << 8) | (1 << 11)).is_none());
        let want = (l * 7.0 + 12.0).min(90.0) as u32;
        let release = tp.x != -666.0 && self.charged >= want.min(self.charge_for.max(want)) && clear && l < 10.0;
        if release {
            self.charged = 0;
            self.charge_for = 15 + (self.r() * 40.0) as u32;
        }
        NetInput::new(v2(-ay * 0.8 + self.heading.cos() * 0.2, ax * 0.8 + self.heading.sin() * 0.2), aim, !release, dash)
    }
}

fn match_game(seed: u64) -> Game {
    let mut e = engine(r#"{"FirstLaunch":1}"#, seed);
    run(&mut e, 40);
    tap(&mut e, "StartingScreenButton");
    run(&mut e, 70);
    tap(&mut e, "MapMenuGameStart");
    run(&mut e, 100);
    e.set_desktop(true);
    e.game().clone()
}

fn handle(game: &mut Game, requests: Vec<GgrsRequest<Cfg>>) -> (u32, u32) {
    let (mut loads, mut advances) = (0, 0);
    for r in requests {
        match r {
            GgrsRequest::SaveGameState { cell, frame } => {
                let sum = game.checksum();
                cell.save(frame, Some(game.clone()), Some(sum));
            }
            GgrsRequest::LoadGameState { cell, .. } => {
                *game = cell.load().expect("a saved state");
                loads += 1;
            }
            GgrsRequest::AdvanceFrame { inputs } => {
                let i: Vec<NetInput> = inputs.iter().map(|(i, _)| *i).collect();
                game.apply_net_inputs(&i);
                game.frame();
                advances += 1;
            }
        }
    }
    (loads, advances)
}

/// The determinism gate: forced rollbacks every frame, checksums compared.
#[test]
fn synctest_the_real_game_stays_deterministic_under_rollback() {
    let mut game = match_game(5);
    let mut sess = SessionBuilder::<Cfg>::new().with_num_players(2).with_check_distance(7).with_input_delay(2).start_synctest_session().unwrap();
    // the brains: the AI fighters, deciding on a copy of the current state
    // (their memory carried between frames); their choices go through GGRS
    let mut minds = game.ai.clone();
    for (k, m) in minds.iter_mut().enumerate() {
        m.on = true;
        m.level = 4;
        let _ = k;
    }
    let start = game.frame;
    let t0 = std::time::Instant::now();
    let (mut loads, mut advances) = (0u32, 0u32);
    let (mut kills, mut shatters) = (0u32, 0u32);
    let mut alive = [true; 2];
    let mut raving = 0usize;
    for _ in 0..2000 {
        let mut brain = game.clone();
        brain.ai = minds.clone();
        let d = brain.ai_decide();
        minds = brain.ai.clone();
        let net = |d: neobrawl::game::Direct| NetInput::new(d.mv, d.aim, d.aiming, d.dash);
        let (i1, i2) = (net(d[0]), net(d[1]));
        sess.add_local_input(0, i1).unwrap();
        sess.add_local_input(1, i2).unwrap();
        match sess.advance_frame() {
            Ok(reqs) => {
                let (l, a) = handle(&mut game, reqs);
                loads += l;
                advances += a;
            }
            Err(e) => panic!("desync at game frame {}: {e:?}", game.frame),
        }
        // coverage on the timeline as it stands after each frame
        for (k, tag) in ["PlayerOneScreen1", "PlayerOneScreen2"].iter().enumerate() {
            let p = game.players.iter().find(|p| &*game.w.nodes[p.node].tag == *tag).unwrap();
            let a = game.w.position(p.node).x != -666.0;
            if alive[k] && !a {
                kills += 1;
            }
            alive[k] = a;
        }
        let now_raving = game.boxes.iter().filter(|b| b.raving).count();
        if now_raving > raving {
            shatters += (now_raving - raving) as u32;
        }
        raving = now_raving;
    }
    // every thrown box that struck something (souls, boxes, walls)
    let impacts: u32 = game.throw_ends.values().map(|v| v[0] + v[1] + v[2]).sum();
    let _ = shatters;
    eprintln!("coverage: {kills} kills, {impacts} thrown-box impacts");
    assert!(kills >= 3 && impacts >= 10, "the run must exercise kills ({kills}) and impacts ({impacts})");
    let s = state_of(&game);
    eprintln!("{} frames, {loads} rollbacks, {advances} simulated ticks in {:.1}s; score {s}", game.frame - start, t0.elapsed().as_secs_f32());
}

fn state_of(g: &Game) -> String {
    g.debug_state()["score"].to_string()
}

// ---------------------------------------------------------------- p2p ----
// Two real GGRS peer sessions, each with its own copy of the game and its own
// brain for its own player, talking over an in-memory socket that injects
// latency, jitter, loss, duplication and reordering (wall-clock based: GGRS
// keeps time itself, so this runs in real time at 50 Hz).

use ggrs::{DesyncDetection, GgrsEvent, Message, NonBlockingSocket, SessionState};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug)]
struct Chaos {
    one_way_ms: f32,
    jitter_ms: f32,
    loss: f32,
    dup: f32,
}

struct Wire {
    chaos: Chaos,
    rng: u64,
    queue: Vec<(Instant, usize, usize, Message)>, // (deliver at, from, to, msg)
    sent: u32,
    dropped: u32,
}

impl Wire {
    fn r(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng % 1_000_000) as f32 / 1_000_000.0
    }
}

struct ChaosSocket {
    me: usize,
    wire: Rc<RefCell<Wire>>,
}

impl NonBlockingSocket<usize> for ChaosSocket {
    fn send_to(&mut self, msg: &Message, addr: &usize) {
        let mut w = self.wire.borrow_mut();
        w.sent += 1;
        if w.r() < w.chaos.loss {
            w.dropped += 1;
            return;
        }
        let copies = if w.r() < w.chaos.dup { 2 } else { 1 };
        for _ in 0..copies {
            let delay = (w.chaos.one_way_ms + (w.r() * 2.0 - 1.0) * w.chaos.jitter_ms).max(0.0);
            let at = Instant::now() + Duration::from_micros((delay * 1000.0) as u64);
            let (me, to) = (self.me, *addr);
            w.queue.push((at, me, to, msg.clone()));
        }
    }
    fn receive_all_messages(&mut self) -> Vec<(usize, Message)> {
        let now = Instant::now();
        let mut w = self.wire.borrow_mut();
        let mut out = Vec::new();
        let mut keep = Vec::new();
        for (at, from, to, m) in w.queue.drain(..) {
            if to == self.me && at <= now {
                out.push((from, m));
            } else {
                keep.push((at, from, to, m));
            }
        }
        w.queue = keep;
        out
    }
}

struct PeerReport {
    frames: i32,
    rollbacks: u32,
    deepest: i32,
    resim: u32,
    desyncs: u32,
    stalls: u32,
    ping: u128,
}

fn run_p2p(chaos: Chaos, frames: u32, delay: usize) -> (PeerReport, PeerReport, u32) {
    let base = match_game(9);
    let wire = Rc::new(RefCell::new(Wire { chaos, rng: 0x9e3779b97f4a7c15, queue: Vec::new(), sent: 0, dropped: 0 }));
    let mk = |me: usize| {
        let other = 1 - me;
        SessionBuilder::<Cfg>::new()
            .with_num_players(2)
            .with_input_delay(delay)
            .with_max_prediction_window(10)
            .with_desync_detection_mode(DesyncDetection::On { interval: 10 })
            .add_player(PlayerType::Local, me)
            .unwrap()
            .add_player(PlayerType::Remote(other), other)
            .unwrap()
            .start_p2p_session(ChaosSocket { me, wire: wire.clone() })
            .unwrap()
    };
    let mut sess = [mk(0), mk(1)];
    let mut games = [base.clone(), base.clone()];
    let mut minds = [base.ai.clone(), base.ai.clone()];
    for m in minds.iter_mut() {
        for a in m.iter_mut() {
            a.on = true;
            a.level = 4;
        }
    }
    let mut sums: [BTreeMap<i32, u128>; 2] = [BTreeMap::new(), BTreeMap::new()];
    let mut rep = [0, 1].map(|_| PeerReport { frames: 0, rollbacks: 0, deepest: 0, resim: 0, desyncs: 0, stalls: 0, ping: 0 });
    let mut skip = [0u32; 2];
    let tick = Duration::from_millis(20);
    let mut next = Instant::now();
    let deadline = Instant::now() + Duration::from_secs(frames as u64 / 50 + 30);
    while (rep[0].frames < frames as i32 || rep[1].frames < frames as i32) && Instant::now() < deadline {
        for k in 0..2 {
            sess[k].poll_remote_clients();
            for ev in sess[k].events() {
                match ev {
                    GgrsEvent::DesyncDetected { frame, .. } => {
                        rep[k].desyncs += 1;
                        eprintln!("peer {k}: DESYNC at frame {frame}");
                    }
                    GgrsEvent::WaitRecommendation { skip_frames } => skip[k] += skip_frames,
                    _ => {}
                }
            }
            if sess[k].current_state() != SessionState::Running || rep[k].frames >= frames as i32 {
                continue;
            }
            if skip[k] > 0 {
                skip[k] -= 1;
                rep[k].stalls += 1;
                continue;
            }
            // this peer's brain decides for its own player only
            let mut brain = games[k].clone();
            brain.ai = minds[k].clone();
            let d = brain.ai_decide();
            minds[k] = brain.ai.clone();
            let input = NetInput::new(d[k].mv, d[k].aim, d[k].aiming, d[k].dash);
            sess[k].add_local_input(k, input).unwrap();
            match sess[k].advance_frame() {
                Ok(reqs) => {
                    let now_frame = sess[k].current_frame();
                    for r in reqs {
                        match r {
                            GgrsRequest::SaveGameState { cell, frame } => {
                                let sum = games[k].checksum();
                                sums[k].insert(frame, sum);
                                cell.save(frame, Some(games[k].clone()), Some(sum));
                            }
                            GgrsRequest::LoadGameState { cell, frame } => {
                                games[k] = cell.load().unwrap();
                                rep[k].rollbacks += 1;
                                rep[k].deepest = rep[k].deepest.max(now_frame - frame);
                            }
                            GgrsRequest::AdvanceFrame { inputs } => {
                                let i: Vec<NetInput> = inputs.iter().map(|(i, _)| *i).collect();
                                games[k].apply_net_inputs(&i);
                                games[k].frame();
                                rep[k].resim += 1;
                            }
                        }
                    }
                    rep[k].frames = sess[k].current_frame();
                }
                Err(ggrs::GgrsError::PredictionThreshold) => rep[k].stalls += 1,
                Err(e) => panic!("peer {k}: {e:?}"),
            }
        }
        next += tick;
        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        } else {
            next = now;
        }
    }
    for k in 0..2 {
        rep[k].ping = sess[k].network_stats(1 - k).map(|s| s.ping).unwrap_or(0);
    }
    // every confirmed frame must hash the same on both peers
    let confirmed = sess[0].confirmed_frame().min(sess[1].confirmed_frame());
    let mut mismatched = 0;
    for (f, a) in sums[0].range(..=confirmed) {
        if let Some(b) = sums[1].get(f) {
            if a != b {
                mismatched += 1;
            }
        }
    }
    let w = wire.borrow();
    eprintln!(
        "{chaos:?} delay {delay}: confirmed to {confirmed}; packets {} sent, {} dropped; checksum mismatches {mismatched}",
        w.sent, w.dropped
    );
    let [a, b] = rep;
    (a, b, mismatched)
}

fn report(r: &PeerReport, k: usize) {
    eprintln!(
        "  peer {k}: {} frames, {} rollbacks ({:.0}% of frames), deepest {} frames, {} re-simulated ticks ({:.2} per frame), {} stalls, ping {} ms, desyncs {}",
        r.frames, r.rollbacks, r.rollbacks as f32 * 100.0 / r.frames.max(1) as f32, r.deepest, r.resim, r.resim as f32 / r.frames.max(1) as f32, r.stalls, r.ping, r.desyncs
    );
}

/// Two peers over a 120 ms round trip with jitter, loss and duplication:
/// no desyncs, every confirmed frame identical, rollbacks shallow.
#[test]
fn p2p_over_a_bad_network_never_desyncs() {
    let chaos = Chaos { one_way_ms: 60.0, jitter_ms: 10.0, loss: 0.03, dup: 0.01 };
    let (a, b, mismatched) = run_p2p(chaos, 600, 2);
    report(&a, 0);
    report(&b, 1);
    assert_eq!(a.desyncs + b.desyncs, 0, "desync detected");
    assert_eq!(mismatched, 0, "confirmed frames differ between peers");
    assert!(a.frames >= 600 && b.frames >= 600, "both ran the whole match");
    // at 120 ms RTT and 2 frames of delay, corrections should stay short
    assert!(a.deepest <= 8 && b.deepest <= 8, "rollbacks too deep: {} / {}", a.deepest, b.deepest);
}

/// The soak: a sweep of network conditions (run with --ignored).
#[test]
#[ignore]
fn p2p_soak_network_sweep() {
    for (rtt, jitter, loss) in [(50.0, 5.0, 0.01), (100.0, 10.0, 0.02), (150.0, 15.0, 0.03), (200.0, 25.0, 0.05), (80.0, 40.0, 0.08)] {
        let chaos = Chaos { one_way_ms: rtt / 2.0, jitter_ms: jitter, loss, dup: 0.01 };
        let (a, b, mismatched) = run_p2p(chaos, 1500, 2);
        report(&a, 0);
        report(&b, 1);
        assert_eq!(a.desyncs + b.desyncs + mismatched, 0);
    }
}
