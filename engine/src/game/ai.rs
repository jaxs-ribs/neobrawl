//! A wanderer: an opponent that doesn't fight, it just likes to travel.
//! It picks destinations it can actually reach (clear for its width),
//! steers around obstacles with a fan of feelers, sometimes drifts over
//! to orbit you at a respectful distance, and dashes now and then on a
//! long open run. It drives a player through the same `Direct` input a
//! human would, so snake mode, dashes and bodies all just work.

use crate::motion::{hash, hf};
use crate::shared::math::{v3, V3};

use super::{Direct, Game, V2};

#[derive(Clone, Debug, Default)]
pub struct Wanderer {
    pub on: bool,
    /// 0 = wanders only; 1-3 = fights (easy, normal, hard)
    pub level: u8,
    /// counters for tuning: throws, dodges, pre-emptive dashes, ticks armed, ticks lane blocked
    pub stats: [u32; 5],
    fight: Fight,
    target: Option<V3>,
    /// ticks spent on the current target / making no progress
    age: u32,
    stuck: u32,
    last: V3,
    orbit: bool,
    orbit_sign: f32,
    rest: u32,
    /// steering, refreshed every third tick (raycasts aren't free)
    dir: V3,
    /// the god's memory (level 5)
    pub(super) god: super::god::Mind,
}

const MASK: u32 = 1 | (1 << 8) | (1 << crate::physics::LAYER_BOX);

/// The fighter's memory.
#[derive(Clone, Debug, Default)]
struct Fight {
    /// what it saw of each other soul, a few ticks late (reaction time)
    seen: std::collections::VecDeque<Vec<(usize, V3)>>,
    strafe: f32,
    strafe_until: u64,
    /// the charge it is aiming for (ticks) and the shot it is lining up
    want_charge: f32,
    aim: V3,
    hold_aim: u32,
    releasing: u32,
    dodge: V3,
    dodge_until: u64,
    /// ticks the lane has been blocked, and a spot with a clear shot
    blocked: u32,
    spot: Option<V3>,
    /// human reaction to danger: each thrown box is only "seen" once its
    /// own reaction time has passed (box node -> tick it becomes visible)
    noticed: crate::det::HashMap<usize, u64>,
    /// since when their loaded aim has been on us
    lined_since: Option<u64>,
    /// a dash broke its tracking: no shots until then, aim held on the old line
    lost_until: u64,
    // a human rhythm between actions:
    /// the current movement plan and when it gets reconsidered
    plan: V3,
    plan_until: u64,
    /// when it got its box (it takes a beat before charging)
    carry_since: Option<u64>,
    /// ticks its aim has been steady (it must settle before releasing)
    settled: u32,
    /// follow-through after a throw: no box-hunting, just drifting
    recover_until: u64,
    /// this shot's aim error (drawn once per box, like a person's miss)
    err: Option<f32>,
    // showcase (level 4): a tempo of phases, and the opponent's last throw
    /// 0 probe (feel out, hold the shot), 1 bait (dance in their lane, dash
    /// out of their loaded aim), 2 press (close in and throw)
    tempo: u8,
    tempo_until: u64,
    enemy_threw_at: u64,
    enemy_had_box: bool,
    swung: bool,
}

/// How well it reads a charging opponent: per-tick juke hazard at full
/// charge, and the chance a juke really changes direction (else a feint).
fn read(l: u8) -> (f32, f32) {
    match l {
        1 => (0.015, 0.5),
        2 => (0.03, 0.65),
        3 => (0.05, 0.75),
        _ => (0.065, 0.8),
    }
}

struct Level {
    react: usize,
    /// median reaction to a throw, in ticks (human: ~200-300 ms)
    flinch: f32,
    aim_err: f32,
    dodge_p: f32,
    range: f32,
    /// ticks: how often plans change, the beat between grabbing a box and
    /// charging, how long the aim must sit still before a release, and the
    /// follow-through after a throw
    cadence: u64,
    beat: u64,
    settle: u32,
    recover: u64,
    /// aim pursuit: max turn (rad per tick) and the tracking error that
    /// counts as "on target"
    turn: f32,
    tol: f32,
}

fn level(l: u8) -> Level {
    match l {
        1 => Level { react: 16, flinch: 17.0, aim_err: 0.22, dodge_p: 0.35, range: 5.5, cadence: 11, beat: 15, settle: 22, recover: 24, turn: 0.045, tol: 0.12 },
        2 => Level { react: 12, flinch: 13.0, aim_err: 0.11, dodge_p: 0.6, range: 5.0, cadence: 8, beat: 11, settle: 16, recover: 18, turn: 0.065, tol: 0.08 },
        3 => Level { react: 9, flinch: 10.0, aim_err: 0.045, dodge_p: 0.8, range: 4.6, cadence: 5, beat: 7, settle: 11, recover: 14, turn: 0.09, tol: 0.05 },
        // showcase: still human-limited, at the very top end
        _ => Level { react: 6, flinch: 8.0, aim_err: 0.03, dodge_p: 0.9, range: 5.0, cadence: 4, beat: 4, settle: 7, recover: 9, turn: 0.14, tol: 0.045 },
    }
}

/// Box physics: power 1.11 per tick of charge (min 20, max 100), drag 5,
/// so a box covers d in -ln(1 - 5d/v0)/5 s and reaches at most v0/5.
fn power_for(ticks: f32) -> f32 {
    (ticks * 1.11).max(20.0).min(100.0)
}
fn flight_time(d: f32, v0: f32) -> Option<f32> {
    let k = 1.0 - 5.0 * d / v0;
    (k > 0.08).then(|| -k.ln() / 5.0)
}

/// The arena's solid shapes, gathered once per tick: (shape, centre, radius).
/// (shape, centre x, centre y, bounding radius, is a box)
pub(super) struct Solids(pub(super) Vec<(crate::physics::Geo, f32, f32, f32, bool)>);

impl Solids {
    /// Is the corridor from `from` to `to`, `half` wide each side, clear?
    pub(super) fn clear(&self, from: V3, to: V3, half: f32) -> bool {
        let d = to - from;
        let l = (d.x * d.x + d.y * d.y).sqrt();
        if l < 1e-3 {
            return true;
        }
        let dir = [d.x / l, d.y / l];
        let side = [-dir[1], dir[0]];
        for (g, cx, cy, r, _) in &self.0 {
            // corridor vs bounding circle first
            let (ox, oy) = (cx - from.x, cy - from.y);
            let along = (ox * dir[0] + oy * dir[1]).clamp(0.0, l);
            let (px, py) = (from.x + dir[0] * along - cx, from.y + dir[1] * along - cy);
            if (px * px + py * py).sqrt() > r + half {
                continue;
            }
            for o in [-half, 0.0, half] {
                let start = [from.x + side[0] * o, from.y + side[1] * o];
                if crate::physics::ray_geo(start, dir, g).map(|t| t <= l).unwrap_or(false) {
                    return false;
                }
            }
        }
        true
    }
}

impl Game {
    pub(super) fn solids(&self) -> Solids {
        let w = &self.w;
        let mut out = Vec::new();
        for i in 0..w.nodes.len() {
            let Some(c) = w.nodes[i].collider.as_ref() else { continue };
            if !c.enabled || c.trigger || MASK & (1 << w.nodes[i].layer) == 0 || !w.active_in_hierarchy(i) {
                continue;
            }
            let Some(g) = crate::physics::geometry(w, i) else { continue };
            let (cx, cy, r) = match &g {
                crate::physics::Geo::Circle { c, r } => (c[0], c[1], *r),
                crate::physics::Geo::Obb { c, h, .. } => (c[0], c[1], (h[0] * h[0] + h[1] * h[1]).sqrt()),
                crate::physics::Geo::Poly { n, p } => {
                    let n = *n as usize;
                    let (sx, sy) = p[..n].iter().fold((0.0, 0.0), |a, q| (a.0 + q[0], a.1 + q[1]));
                    let (cx, cy) = (sx / n as f32, sy / n as f32);
                    let r = p[..n].iter().map(|q| ((q[0] - cx).powi(2) + (q[1] - cy).powi(2)).sqrt()).fold(0.0f32, f32::max);
                    (cx, cy, r)
                }
            };
            out.push((g, cx, cy, r, w.nodes[i].layer == crate::physics::LAYER_BOX));
        }
        Solids(out)
    }

    /// Netcode tests: run only the AI's decision for both slots on the
    /// current state and return what it would press (nothing is simulated).
    pub fn ai_decide(&mut self) -> [Direct; 2] {
        self.ai_step();
        self.input.direct
    }

    /// Run the wanderers before the tick consumes input.
    pub(super) fn ai_step(&mut self) {
        if !self.ai.iter().any(|a| a.on) {
            return;
        }
        let solids = self.solids();
        for slot in 0..2 {
            if !self.ai[slot].on {
                continue;
            }
            let Some(k) = (0..self.players.len()).find(|&k| self.direct_slot_pub(&self.players[k]) == Some(slot)) else { continue };
            let node = self.players[k].node;
            let me = self.w.position(node);
            if me.x == -666.0 || !self.w.active_in_hierarchy(node) {
                self.input.direct[slot] = Direct { active: true, ..Default::default() };
                continue;
            }
            let others: Vec<V3> = (0..self.players.len()).filter(|&o| o != k).map(|o| self.w.position(self.players[o].node)).filter(|p| p.x != -666.0).collect();
            let seed = (self.frame as u32).wrapping_mul(2654435761) ^ (slot as u32 * 7919);
            let carrying = self.players[k].picked_box.is_some();
            if self.ai[slot].level >= 5 {
                let mut w = std::mem::take(&mut self.ai[slot]);
                let d = self.god_step(slot, k, &mut w.god, &solids);
                self.input.direct[slot] = d;
                self.ai[slot] = w;
                continue;
            }
            if self.ai[slot].level > 0 {
                let mut w = std::mem::take(&mut self.ai[slot]);
                let d = self.fight_step(slot, k, &mut w, &solids);
                self.input.direct[slot] = d;
                self.ai[slot] = w;
                continue;
            }
            let mut w = std::mem::take(&mut self.ai[slot]);
            w.age += 1;
            // progress check
            let moved = (me - w.last).length();
            w.last = me;
            w.stuck = if moved < 0.02 { w.stuck + 1 } else { 0 };
            if w.rest > 0 {
                w.rest -= 1;
            }
            // reached, stale or stuck: choose somewhere new
            let reached = w.target.map(|t| (t - me).length() < 0.7).unwrap_or(true);
            if reached || w.age > 250 || w.stuck > 20 {
                if reached && w.target.is_some() && hash(seed ^ 3) % 3 == 0 {
                    w.rest = 10 + hash(seed ^ 4) % 30; // a breath before moving on
                }
                w.target = None;
                w.age = 0;
                w.stuck = 0;
                w.orbit = !others.is_empty() && hash(seed ^ 11) % 4 == 0;
                w.orbit_sign = if hash(seed ^ 12) & 1 == 0 { 1.0 } else { -1.0 };
                // sample candidates; prefer far, reachable, and not on top of anyone
                let mut best: Option<(f32, V3)> = None;
                for c in 0..24u32 {
                    let s = hash(seed ^ (c * 977 + 5));
                    let a = (hf(s) * 0.5 + 0.5) * std::f32::consts::TAU;
                    let r = 2.5 + (hf(s ^ 9) * 0.5 + 0.5) * 6.5;
                    let p = me + v3(a.cos() * r, a.sin() * r, 0.0);
                    if !solids.clear(me, p, 0.32) {
                        continue;
                    }
                    let crowd = others.iter().map(|o| (*o - p).length()).fold(f32::MAX, f32::min);
                    let score = r + crowd.min(4.0) * 0.6 + hf(s ^ 13) * 1.5;
                    if best.map(|b| score > b.0).unwrap_or(true) {
                        best = Some((score, p));
                    }
                }
                w.target = best.map(|b| b.1);
            }
            // where to head: the target, or a slow orbit around the nearest soul
            let goal = if w.orbit {
                others
                    .iter()
                    .min_by(|a, b| (**a - me).length().partial_cmp(&(**b - me).length()).unwrap())
                    .map(|o| {
                        let d = me - *o;
                        let l = (d.x * d.x + d.y * d.y).sqrt().max(1e-3);
                        let radial = v3(d.x / l, d.y / l, 0.0);
                        let tangent = v3(-radial.y, radial.x, 0.0) * w.orbit_sign;
                        // hold ~4 units off and slide round
                        me + tangent * 2.0 + radial * ((4.0 - l) * 0.8)
                    })
                    .unwrap_or(me)
            } else {
                w.target.unwrap_or(me)
            };
            let to = goal - me;
            let l = (to.x * to.x + to.y * to.y).sqrt();
            let want = if l > 1e-3 { v3(to.x / l, to.y / l, 0.0) } else { V3::ZERO };
            // feelers: of a fan of headings, take the clear one nearest the goal
            let mut dir = w.dir;
            if want == V3::ZERO {
                dir = V3::ZERO;
            } else if self.frame % 3 == 0 || dir == V3::ZERO {
                dir = V3::ZERO;
                let base = want.y.atan2(want.x);
                for off in [0.0f32, 0.35, -0.35, 0.7, -0.7, 1.1, -1.1, 1.6, -1.6, 2.3, -2.3] {
                    let a = base + off;
                    let d = v3(a.cos(), a.sin(), 0.0);
                    if solids.clear(me, me + d * 1.1, 0.28) {
                        dir = d;
                        break;
                    }
                }
            }
            w.dir = dir;
            // a dash now and then on a long, open run
            let dash = !carrying
                && dir != V3::ZERO
                && l > 4.0
                && self.players[k].dash_cd == 0
                && hash(seed ^ 77) % 140 == 0
                && solids.clear(me, me + dir * 3.2, 0.3);
            let mv = if w.rest > 0 { V2::ZERO } else { V2 { x: dir.x, y: dir.y } };
            self.input.direct[slot] = Direct { active: true, mv, aim: V2::ZERO, aiming: false, use_cursor: false, dash };
            self.ai[slot] = w;
        }
    }
}

impl Game {
    /// The fighter: get a box, hold a range while slipping sideways out of
    /// your aim, charge just enough for the distance, lead the shot, throw
    /// only down a clear lane - and get out of the way of anything coming.
    fn fight_step(&mut self, slot: usize, k: usize, w: &mut Wanderer, solids: &Solids) -> Direct {
        let lv = level(w.level);
        let node = self.players[k].node;
        let me = self.w.position(node);
        let t = self.frame;
        let seed = (t as u32).wrapping_mul(2654435761) ^ (slot as u32 * 7919 + 13);
        // perception, delayed by the reaction time
        let now: Vec<(usize, V3)> = (0..self.players.len())
            .filter(|&o| o != k && !self.players[o].playback && self.w.active_in_hierarchy(self.players[o].node))
            .map(|o| (o, self.w.position(self.players[o].node)))
            .filter(|(_, p)| p.x != -666.0)
            .collect();
        w.fight.seen.push_back(now);
        while w.fight.seen.len() > lv.react + 2 {
            w.fight.seen.pop_front();
        }
        let old = w.fight.seen.front().cloned().unwrap_or_default();
        let span = 3.min(w.fight.seen.len().saturating_sub(1)).max(1);
        let later = w.fight.seen.get(span).cloned().unwrap_or_default();
        let lag = (w.fight.seen.len().saturating_sub(1)) as f32 * super::DT;
        // the enemy as perceived: velocity from the delayed track (3-tick
        // average), and position carried forward across the reaction delay
        // to where it must be now
        let mut burst = false;
        let enemy = old.first().copied().map(|(o, p)| {
            let v = later.iter().find(|(q, _)| *q == o).map(|(_, p2)| (*p2 - p) * (1.0 / (span as f32 * super::DT))).unwrap_or(V3::ZERO);
            let sp = v.length();
            // people extrapolate a walk, not a dash: a burst breaks tracking
            burst = sp > 11.0;
            let v = if sp > 8.5 { v * (8.5 / sp) } else { v };
            let v = if sp > 30.0 { V3::ZERO } else { v };
            (o, p + v * lag, v)
        });
        if burst && t >= w.fight.lost_until {
            let h = hash(seed ^ 0x9191);
            let u = (hf(h) * 0.5 + 0.5).max(0.02);
            w.fight.lost_until = t + (lv.flinch * (1.0 + (-u.ln()).min(2.5) * 0.4)) as u64;
        }
        let lost = t < w.fight.lost_until;
        let carrying = self.players[k].picked_box.is_some();
        let charge = self.players[k].charge_timer;
        let mut mv = V3::ZERO;
        let mut aiming = false;
        let mut aim = V2::ZERO;
        let mut dash = false;
        let mut final_approach = false;

        // 1) danger: thrown boxes about to pass close, and aim lines on us
        if t >= w.fight.dodge_until {
            for b in &self.boxes {
                if b.carried_by == Some(node) {
                    continue;
                }
                let Some(body) = self.w.nodes[b.node].body.as_ref() else { continue };
                let v = body.velocity;
                let sp2 = v.x * v.x + v.y * v.y;
                if sp2 < 100.0 || !self.w.active_in_hierarchy(b.node) {
                    w.fight.noticed.remove(&b.node);
                    continue;
                }
                // a human needs a moment to see it: a reaction time per throw,
                // right-skewed like real ones (mostly near the median, sometimes slow)
                let seen_at = *w.fight.noticed.entry(b.node).or_insert_with(|| {
                    let h = hash(seed ^ (b.node as u32).wrapping_mul(97));
                    let u = (hf(h) * 0.5 + 0.5).max(0.02);
                    let jitter = (-u.ln()).min(3.0) * lv.flinch * 0.35; // exponential tail
                    t + (lv.flinch * 0.8 + jitter) as u64
                });
                if t < seen_at {
                    continue;
                }
                let r = me - self.w.position(b.node);
                let tc = (r.x * v.x + r.y * v.y) / sp2;
                if !(0.0..0.7).contains(&tc) {
                    continue;
                }
                let miss = r - v * tc;
                let md = (miss.x * miss.x + miss.y * miss.y).sqrt();
                if md < 1.1 && (hash(seed ^ b.node as u32) % 1000) as f32 / 1000.0 < lv.dodge_p {
                    // step off the line, away from where it will pass
                    let sp = sp2.sqrt();
                    let perp = v3(-v.y / sp, v.x / sp, 0.0);
                    let side = if perp.x * miss.x + perp.y * miss.y >= 0.0 { perp } else { perp * -1.0 };
                    w.fight.dodge = side;
                    w.fight.dodge_until = t + 10;
                    w.stats[1] += 1;
                    if tc < 0.35 && self.players[k].dash_cd == 0 {
                        dash = true;
                    }
                }
            }
        }
        if t < w.fight.dodge_until {
            mv = w.fight.dodge;
        }

        if let Some((eo, ep, ev)) = enemy {
            let to = ep - me;
            let dist = (to.x * to.x + to.y * to.y).sqrt().max(1e-3);
            let dir = to * (1.0 / dist);
            // their aim on us while they charge: slip sideways off it
            let ea = self.players[eo].touch[1];
            let their_charge = self.players[eo].charge_timer;
            let lined = ea.x * -dir.x + ea.y * -dir.y;
            let on_us = their_charge > 8.0 && lined > 0.9;
            let armed = self.players[eo].picked_box.is_some();
            // a loaded shot dead on us: get off the line before it comes
            // (close throws arrive faster than anyone can sidestep)
            // ...but only once it has been on us for a human beat
            if on_us && lined > 0.97 {
                w.fight.lined_since.get_or_insert(t);
            } else {
                w.fight.lined_since = None;
            }
            let lined_long = w.fight.lined_since.map(|s| t - s >= lv.flinch as u64).unwrap_or(false);
            // anticipation, not reflexes: the longer and fuller you hold a shot
            // on it, the likelier it jukes before you let go - sometimes the
            // wrong way, sometimes just a feint - so you can read it and bait it
            if on_us && their_charge > 15.0 && t >= w.fight.dodge_until {
                let (hazard, honest) = read(w.level);
                let held = w.fight.lined_since.map(|s| (t - s) as f32 / 25.0).unwrap_or(0.0).min(2.0);
                let p = hazard * (their_charge / 90.0) * (0.5 + held);
                if (hash(seed ^ 0x5151) % 10000) as f32 / 10000.0 < p {
                    if (hash(seed ^ 0x6161) % 1000) as f32 / 1000.0 < honest {
                        w.fight.strafe = -w.fight.strafe;
                    }
                    w.fight.dodge = v3(-dir.y, dir.x, 0.0) * w.fight.strafe;
                    w.fight.dodge_until = t + 7 + (hash(seed ^ 0x7171) % 6) as u64;
                    w.fight.strafe_until = t + 20;
                    mv = w.fight.dodge;
                    if their_charge > 60.0 && self.players[k].dash_cd == 0 && hash(seed ^ 0x8181) % 3 == 0 {
                        dash = true;
                    }
                    w.stats[2] += 1;
                }
            }
            if w.level >= 2 && lined_long && their_charge > 30.0 && self.players[k].dash_cd == 0 && t >= w.fight.dodge_until && hash(seed ^ 41) % 3 == 0 {
                w.fight.dodge = v3(-dir.y, dir.x, 0.0) * w.fight.strafe;
                w.fight.dodge_until = t + 8;
                dash = true;
                mv = w.fight.dodge;
                w.stats[2] += 1;
            }
            // strafe sign flips on an uneven clock
            if t >= w.fight.strafe_until {
                w.fight.strafe = if hash(seed ^ 5) & 1 == 0 { 1.0 } else { -1.0 };
                let gap = 12.0 + ((hf(seed ^ 9) * 0.5 + 0.5).max(0.02)).powf(-0.7) * 10.0;
                w.fight.strafe_until = t + gap.min(90.0) as u64;
            }
            // showcase tempo: probe, bait and press in turn, never the same twice
            let show = w.level >= 4;
            let enemy_box = self.players[eo].picked_box.is_some();
            if w.fight.enemy_had_box && !enemy_box {
                w.fight.enemy_threw_at = t; // they just let go: a window opens
            }
            w.fight.enemy_had_box = enemy_box;
            let punish = show && t.saturating_sub(w.fight.enemy_threw_at) < 30 && w.fight.enemy_threw_at > 0;
            if show && t >= w.fight.tempo_until {
                let r = (hash(seed ^ 0x4141) % 100) as u8;
                let next = if r < 38 { 2 } else if r < 70 { 0 } else { 1 };
                w.fight.tempo = if next == w.fight.tempo { (next + 1) % 3 } else { next };
                w.fight.tempo_until = t + 60 + (hash(seed ^ 0x4242) % 90) as u64;
            }
            if show && w.fight.tempo == 1 && t + 30 < w.fight.strafe_until {
                // baiting: erratic, short strafes in their lane
                w.fight.strafe_until = t + 6 + (hash(seed ^ 0x4343) % 10) as u64;
            }
            let tangent = v3(-dir.y, dir.x, 0.0) * w.fight.strafe;

            if carrying {
                // they're loaded and lined up on us and we're not ready: get out
                // of the line rather than race them to the shot
                let threatened = on_us && their_charge > 45.0 && charge < w.fight.want_charge * 0.9;
                if threatened && mv == V3::ZERO {
                    mv = tangent * 1.4 - dir * 0.3;
                }
                // 2) ranging and the shot: a little further off while they're armed
                let range = if show {
                    if punish {
                        3.4
                    } else {
                        [6.6, 4.4, 3.6][w.fight.tempo as usize]
                    }
                } else if armed {
                    lv.range + 0.8
                } else {
                    lv.range - 0.6
                };
                // a fresh box with the dash refilled: swing out to a flank angle
                if show && !w.fight.swung && self.players[k].dash_cd == 0 && w.fight.tempo == 2 && mv == V3::ZERO {
                    w.fight.swung = true;
                    mv = tangent;
                    dash = true;
                }
                let radial = dir * ((dist - range) * 0.6).clamp(-1.0, 1.0);
                // a blocked lane for a moment: go find a spot with a clear shot
                if w.fight.blocked > 8 && (w.fight.spot.is_none() || t % 12 == 0) {
                    let mut best: Option<(f32, V3)> = None;
                    for c in 0..12u32 {
                        let a = c as f32 / 12.0 * std::f32::consts::TAU + hf(seed ^ c) * 0.2;
                        let p = me + v3(a.cos(), a.sin(), 0.0) * (1.2 + (c % 3) as f32 * 0.9);
                        if !solids.clear(me, p, 0.26) {
                            continue;
                        }
                        let d = (ep - p).length();
                        if !solids.clear(p + (ep - p) * (0.5 / d.max(0.5)), ep, 0.45) {
                            continue;
                        }
                        let cost = (p - me).length() + (d - range).abs() * 0.7;
                        if best.map(|b| cost < b.0).unwrap_or(true) {
                            best = Some((cost, p));
                        }
                    }
                    w.fight.spot = best.map(|b| b.1);
                }
                if mv == V3::ZERO {
                    mv = match w.fight.spot {
                        Some(sp) if w.fight.blocked > 8 && (sp - me).length() > 0.3 => {
                            let d = sp - me;
                            d * (1.0 / d.length().max(1e-3)) + if on_us { tangent * 0.6 } else { V3::ZERO }
                        }
                        _ => tangent * if on_us { 1.3 } else { 0.75 } + radial,
                    };
                }
                // how hard: enough to still be lethal past their far side
                let need = power_for(0.0).max((5.0 * (dist + 2.0) / 0.9).min(100.0));
                w.fight.want_charge = (need / 1.11).clamp(14.0, 90.0);
                // lead: where they will be when the box arrives
                let v0 = power_for(w.fight.want_charge);
                let mut aim_at = ep;
                for _ in 0..3 {
                    let d = (aim_at - me).length();
                    let Some(tt) = flight_time(d, v0) else { break };
                    aim_at = ep + ev * tt.min(0.8);
                }
                let mut ad = aim_at - me;
                let al = (ad.x * ad.x + ad.y * ad.y).sqrt().max(1e-3);
                ad = ad * (1.0 / al);
                // human-ish error: one miss per shot, not a tremor
                let err = *w.fight.err.get_or_insert(hf(seed ^ 77) * lv.aim_err);
                let (s, c) = err.sin_cos();
                ad = v3(ad.x * c - ad.y * s, ad.x * s + ad.y * c, 0.0);
                let lane = solids.clear(me + ad * 0.5, me + ad * dist.min(9.0), 0.45);
                // a beat after grabbing the box before it starts to charge
                let since = t - *w.fight.carry_since.get_or_insert(t);
                aiming = since >= lv.beat;
                // the aim pursues the target at a human turn rate, and it only
                // lets go once the tracking error has stayed small for a moment
                // (while a dash has broken its tracking, it stays on the old line)
                if w.fight.hold_aim == 0 && !lost {
                    if w.fight.aim == V3::ZERO {
                        w.fight.aim = ad;
                    }
                    let cur = w.fight.aim.y.atan2(w.fight.aim.x);
                    let mut d = ad.y.atan2(ad.x) - cur;
                    d = d.sin().atan2(d.cos());
                    let na = cur + d.clamp(-lv.turn, lv.turn);
                    w.fight.aim = v3(na.cos(), na.sin(), 0.0);
                    let off = (d.abs() - lv.turn).max(0.0);
                    if off <= lv.tol {
                        w.fight.settled += 1;
                    } else {
                        w.fight.settled = 0;
                    }
                }
                aim = V2 { x: w.fight.aim.x, y: w.fight.aim.y };
                w.stats[3] += 1;
                if !lane {
                    w.stats[4] += 1;
                    w.fight.blocked += 1;
                } else {
                    w.fight.blocked = 0;
                    w.fight.spot = None;
                }
                // probing holds the loaded shot (feeling them out) unless a window opens
                let holding = show && w.fight.tempo == 0 && !punish && dist > 4.0;
                let settle_need = if punish { 3 } else { lv.settle };
                let ready = !lost && !holding && aiming && w.fight.settled >= settle_need && charge >= w.fight.want_charge && lane && flight_time(dist, power_for(charge)).is_some();
                if ready && w.fight.hold_aim == 0 {
                    w.fight.hold_aim = 3;
                }
                if w.fight.hold_aim > 0 {
                    w.fight.hold_aim -= 1;
                    if w.fight.hold_aim == 0 {
                        aiming = false; // release
                        w.stats[0] += 1;
                        w.fight.recover_until = t + lv.recover + hash(seed ^ 0x3131) as u64 % 8;
                        w.fight.carry_since = None;
                        w.fight.settled = 0;
                        w.fight.err = None;
                        w.fight.swung = false;
                    }
                }
            } else if t < w.fight.recover_until {
                // follow-through after a throw: a moment of drifting - an opening
                w.fight.carry_since = None;
                if mv == V3::ZERO {
                    mv = tangent * 0.45;
                }
            } else {
                // 3) get a box: the nearest reachable one, not straight into their aim
                w.fight.hold_aim = 0;
                w.fight.carry_since = None;
                let mut best: Option<(f32, V3)> = None;
                for b in &self.boxes {
                    let n = b.node;
                    if !self.w.active_in_hierarchy(n) || b.carried_by.is_some() || self.w.nodes[n].collider.as_ref().map(|c| !c.enabled).unwrap_or(true) {
                        continue;
                    }
                    let bp = self.w.position(n);
                    let d = (bp - me).length();
                    if d > 14.0 {
                        continue;
                    }
                    let toward_enemy = (bp - ep).length();
                    let cost = d + if self.players[eo].charge_timer > 8.0 && toward_enemy < dist { 3.0 } else { 0.0 };
                    if best.map(|x| cost < x.0).unwrap_or(true) && solids.clear(me, bp - (bp - me) * (0.5 / d.max(0.5)), 0.25) {
                        best = Some((cost, bp));
                    }
                }
                if mv == V3::ZERO {
                    mv = match best {
                        Some((_, bp)) => {
                            let d = bp - me;
                            let l = (d.x * d.x + d.y * d.y).sqrt().max(1e-3);
                            final_approach = l < 1.6;
                            d * (1.0 / l) + if on_us { tangent * 0.8 } else { V3::ZERO }
                        }
                        None => tangent + dir * 0.3,
                    };
                    // a dash toward a far box when the dash is up (a pickup refills it)
                    if let Some((c, _)) = best {
                        if c > 5.0 && self.players[k].dash_cd == 0 && hash(seed ^ 31) % 25 == 0 {
                            dash = true;
                        }
                    }
                }
            }
        }
        // plans change a few times a second, not every tick (dodges, already
        // paced by their own reaction times, cut in)
        let dodging = t < w.fight.dodge_until;
        if !dodging && t < w.fight.plan_until && !final_approach {
            mv = w.fight.plan;
        } else {
            w.fight.plan = mv;
            w.fight.plan_until = t + lv.cadence + hash(seed ^ 0x2121) as u64 % (lv.cadence / 2 + 1);
        }
        // steer the move around obstacles with the wanderer's feelers
        let ml = (mv.x * mv.x + mv.y * mv.y).sqrt();
        // (not on the last stretch to a box: the box itself would read as a wall)
        if ml > 1e-3 && !final_approach {
            let want = mv * (1.0 / ml);
            let base = want.y.atan2(want.x);
            let mut out = V3::ZERO;
            for off in [0.0f32, 0.35, -0.35, 0.7, -0.7, 1.1, -1.1, 1.6, -1.6] {
                let a = base + off;
                let d = v3(a.cos(), a.sin(), 0.0);
                if solids.clear(me, me + d * 0.9, 0.26) {
                    out = d;
                    break;
                }
            }
            mv = out;
        }
        Direct { active: true, mv: V2 { x: mv.x, y: mv.y }, aim, aiming, use_cursor: false, dash }
    }
}
