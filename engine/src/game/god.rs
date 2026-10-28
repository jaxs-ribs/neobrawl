//! The god (AI level 5): a showcase fighter with superhuman reactions that
//! plays for the highlight reel. It simulates every throw it could make
//! (straight, banked off walls, through a dash) and only lets go when the
//! box arrives before the target can get out of the way; it reads every
//! box in the air and steps or dashes off its line at the last instant;
//! and it plays named strategies, announced to the play log as intents:
//!
//! - counter: an incoming box met in the air by its own throw
//! - bank: a shot off a wall into someone who can't see it coming
//! - corner: dash round an obstacle and throw on the tick the lane opens
//! - rush: dash in and throw from point blank
//! - bait: stand in their lane, dodge the throw on foot, punish them empty
//! - punish: they just threw; close in while they have nothing
//! - chain: a kill gave the dash back; go straight for the next one
//! - matrix: a dodge left to the very last instant
//!
//! It plays like a very good human, not a machine: it sees the other soul
//! 120 ms late (and carries it forward), notices a throw after ~180 ms and
//! a box off a wall after ~280 ms, turns its aim at a hand's speed and lets
//! go only once it has settled, misses by a hair now and then, jukes out of
//! a loaded aim before the throw comes, and holds a movement plan for a
//! beat instead of twitching every tick.

use crate::motion::{hash, hf};
use crate::shared::math::{v3, V3};

use super::ai::Solids;
use super::plays::Kind;
use super::{Direct, Game, V2, DT};

/// A hit: the box passes this close to the soul's centre (soul 0.25 + box).
const HIT_R: f32 = 0.6;
/// Box drag: speed falls 5 per unit travelled.
const DRAG: f32 = 5.0;
/// Seen after this many ticks: a direct throw, a box off a wall.
const SEE_DIRECT: f32 = 9.0;
const SEE_BANK: f32 = 14.0;
/// the other soul is seen this many ticks late
const LAG: usize = 6;
/// the aim's top turn (rad per tick) and how close counts as on target
const TURN: f32 = 0.2;
const ON_AIM: f32 = 0.05;
const WALK: f32 = 8.0;
const DASH_V: f32 = super::players::DASH_SPEED;
const DASH_LEN: f32 = DASH_V * super::players::DASH_TICKS as f32 * DT;

#[derive(Clone, Debug, Default)]
pub struct Mind {
    /// the shot being held for release: aim and ticks to go
    shot: Option<(V3, u32, &'static str)>,
    /// what it's doing, and until when
    plan: &'static str,
    plan_until: u64,
    /// a dodge in progress
    dodge: V3,
    dodge_until: u64,
    /// a dash it took to open a lane (corner / rush): shoot off its end
    dashing_for: &'static str,
    /// the enemy as last seen: position, velocity estimate
    last_enemy: Option<V3>,
    enemy_v: V3,
    enemy_had_box: bool,
    enemy_threw_at: u64,
    /// when the enemy last came back from a death (no shooting the newly born)
    enemy_gone: bool,
    enemy_back_at: u64,
    strafe: f32,
    strafe_until: u64,
    /// last intent logged (no repeats every tick)
    said: &'static str,
    said_at: u64,
    /// what it has seen of the other soul (newest last), for the late view
    seen: std::collections::VecDeque<V3>,
    /// where its aim points now, the shot it is lining up, and for how long it's been on
    aim_cur: V3,
    lining: Option<(V3, &'static str)>,
    settled: u32,
    /// a movement plan held for a beat
    plan_mv: V3,
    plan_mv_until: u64,
}

/// One straight stretch of a box's flight: from a to b, times since release.
#[derive(Clone, Copy, Debug)]
struct Seg {
    a: V3,
    b: V3,
    t0: f32,
    t1: f32,
    bounces: u8,
}

/// Time to travel `d` starting at speed `v` (speed falls DRAG per unit).
fn time_over(d: f32, v: f32) -> f32 {
    let k = (1.0 - DRAG * d / v).max(1e-4);
    -k.ln() / DRAG
}

impl Solids {
    /// First thing a box `half` wide hits going from `o` along `d`: distance,
    /// surface normal (facing back at the box), and whether it's a box.
    fn hit(&self, o: V3, d: V3, max: f32, half: f32) -> Option<(f32, V3, bool)> {
        let side = [-d.y, d.x];
        let mut best: Option<(f32, usize, f32)> = None;
        for (i, (g, cx, cy, r, _)) in self.0.iter().enumerate() {
            let (ox, oy) = (cx - o.x, cy - o.y);
            let along = (ox * d.x + oy * d.y).clamp(0.0, max);
            let (px, py) = (o.x + d.x * along - cx, o.y + d.y * along - cy);
            if px * px + py * py > (r + half) * (r + half) {
                continue;
            }
            for off in [-half, 0.0, half] {
                let s = [o.x + side[0] * off, o.y + side[1] * off];
                if let Some(t) = crate::physics::ray_geo(s, [d.x, d.y], g) {
                    if t <= max && best.map(|b| t < b.0).unwrap_or(true) {
                        best = Some((t, i, off));
                    }
                }
            }
        }
        let (t, i, off) = best?;
        // the surface's normal from two neighbouring rays
        let g = &self.0[i].0;
        let probe = |e: f32| {
            let s = [o.x + side[0] * (off + e), o.y + side[1] * (off + e)];
            crate::physics::ray_geo(s, [d.x, d.y], g).map(|t| v3(s[0] + d.x * t, s[1] + d.y * t, 0.0))
        };
        let n = match (probe(-0.04), probe(0.04)) {
            (Some(p), Some(q)) => {
                let tan = q - p;
                let l = tan.length().max(1e-6);
                let n = v3(-tan.y / l, tan.x / l, 0.0);
                if n.x * d.x + n.y * d.y > 0.0 {
                    n * -1.0
                } else {
                    n
                }
            }
            _ => d * -1.0,
        };
        Some((t, n, self.0[i].4))
    }

    /// A box's flight from `from` along `dir` at speed `v0`, bouncing like
    /// the game bounces, while it can still kill.
    fn flight(&self, from: V3, dir: V3, v0: f32) -> Vec<Seg> {
        let (mut p, mut d, mut v, mut t) = (from, dir, v0, 0.0f32);
        let mut out = Vec::new();
        for bounces in 0..=super::boxes::BOUNCES_LIVE {
            // lethal while young (0.56 s) or fast (> ~10)
            let fast_for = ((v - 10.0) / DRAG).max(0.0);
            let young_for = if t < 0.56 { v / DRAG * (1.0 - (-(0.56 - t) * DRAG).exp()) } else { 0.0 };
            let reach = fast_for.max(young_for).min(v / DRAG * 0.98);
            if reach < 0.05 {
                break;
            }
            match self.hit(p + d * 0.05, d, reach, 0.3) {
                Some((l, n, is_box)) => {
                    let q = p + d * l;
                    let t1 = t + time_over(l, v);
                    out.push(Seg { a: p, b: q, t0: t, t1, bounces });
                    let vh = v - DRAG * l;
                    let into = -(d.x * n.x + d.y * n.y);
                    if is_box || (into > super::boxes::SHATTER_SQUARE && into * vh > super::boxes::SHATTER_NORMAL_SPEED) {
                        break;
                    }
                    let r = d - n * (2.0 * (d.x * n.x + d.y * n.y));
                    d = r * (1.0 / r.length().max(1e-6));
                    v = vh * super::boxes::BOUNCE_KEEP;
                    p = q + n * 0.02;
                    t = t1;
                    if v < 12.0 {
                        break;
                    }
                }
                None => {
                    let q = p + d * reach;
                    out.push(Seg { a: p, b: q, t0: t, t1: t + time_over(reach, v), bounces });
                    break;
                }
            }
        }
        out
    }
}

/// Where a flight passes a soul moving from `p` at `vel`: the first time it
/// comes within HIT_R, and on which bounce.
fn meets(path: &[Seg], p: V3, vel: V3, delay: f32) -> Option<(f32, u8, f32)> {
    for s in path {
        let n = 10;
        for i in 0..=n {
            let u = i as f32 / n as f32;
            let tt = s.t0 + (s.t1 - s.t0) * u;
            // the box slows along the segment: position by distance, close
            // enough sampled by time for a hit test
            let bp = s.a + (s.b - s.a) * u;
            let sp = p + vel * (tt + delay);
            let dd = ((bp.x - sp.x).powi(2) + (bp.y - sp.y).powi(2)).sqrt();
            if dd < HIT_R {
                return Some((tt, s.bounces, dd));
            }
        }
    }
    None
}

impl Game {
    fn say(&mut self, k: usize, m: &mut Mind, what: &'static str) {
        if m.said == what && self.frame < m.said_at + 25 {
            return;
        }
        m.said = what;
        m.said_at = self.frame;
        let at = self.w.position(self.players[k].node);
        self.log_play(Kind::Intent, Some(k), None, at, 0, 0.0, 0.0, what);
    }

    /// How long a soul at `p` needs to get out of a box's way: noticing it,
    /// then stepping (or dashing, if it has one) clear of the hit circle.
    fn escape_time(&self, k: usize, banked: bool) -> f32 {
        let see = if banked { SEE_BANK } else { SEE_DIRECT } * DT;
        let walk = HIT_R / WALK;
        let dash = if self.players[k].dash_cd == 0 { HIT_R / DASH_V + DT } else { f32::MAX };
        see + walk.min(dash)
    }

    /// The best throw from `from` (released `lead` s from now) at soul `ek`
    /// moving at `ev`: aim, arrival time, bounces, and its margin (arrival
    /// minus their escape: negative means they can't get away).
    fn best_shot(&self, solids: &Solids, from: V3, v0: f32, ek: usize, ep: V3, ev: V3, lead: f32, seed: u32) -> Option<(V3, f32, u8, f32)> {
        let mut best: Option<(V3, f32, u8, f32)> = None;
        let direct = ep + ev * lead - from;
        let base = direct.y.atan2(direct.x);
        let n = 72;
        for i in 0..=n {
            // the straight line first, then a fan all round
            let a = if i == 0 { base } else { base + (i as f32 / n as f32) * std::f32::consts::TAU + hf(seed ^ i as u32) * 0.02 };
            let dir = v3(a.cos(), a.sin(), 0.0);
            let path = solids.flight(from, dir, v0);
            let Some((ta, bounces, miss)) = meets(&path, ep, ev, lead) else { continue };
            // a bank: they only see it after the bounce
            let seen_from = if bounces > 0 { path.iter().find(|s| s.bounces == bounces).map(|s| s.t0).unwrap_or(0.0) } else { 0.0 };
            let margin = (ta - seen_from) - self.escape_time(ek, bounces > 0) + miss * 0.05;
            if best.map(|b| margin < b.3).unwrap_or(true) {
                best = Some((dir, ta, bounces, margin));
            }
        }
        best
    }

    pub(super) fn god_step(&mut self, slot: usize, k: usize, m: &mut Mind, solids: &Solids) -> Direct {
        // to walk, only walls count: a box in the way is just picked up
        let walls = Solids(solids.0.iter().filter(|s| !s.4).cloned().collect());
        let d = self.god_think(slot, k, m, solids, &walls);
        let me = self.w.position(self.players[k].node);
        self.god_out(me, v3(d.mv.x, d.mv.y, 0.0), d.aim, d.aiming, d.dash, &walls)
    }

    fn god_think(&mut self, slot: usize, k: usize, m: &mut Mind, solids: &Solids, walls: &Solids) -> Direct {
        let t = self.frame;
        let seed = (t as u32).wrapping_mul(2654435761) ^ (slot as u32 * 7919 + 101) ^ self.ai_salt;
        let node = self.players[k].node;
        let me = self.w.position(node);
        let carrying = self.players[k].picked_box.is_some();
        let charge = self.players[k].charge_timer;
        let dash_ready = self.players[k].dash_cd == 0;
        let mut mv = V3::ZERO;
        let mut aim = V2::ZERO;
        let mut aiming = carrying;
        let mut dash = false;

        // the enemy: perfect information, velocity from the last tick
        let ek = (0..self.players.len()).find(|&o| o != k && !self.players[o].playback && !self.players[o].single && self.w.position(self.players[o].node).x != -666.0);
        let enemy_now = ek.map(|o| (o, self.w.position(self.players[o].node)));
        // seen late: where it was LAG ticks ago, carried forward at the speed it had then
        if let Some((_, p)) = enemy_now {
            m.seen.push_back(p);
            while m.seen.len() > LAG + 4 {
                m.seen.pop_front();
            }
        } else {
            m.seen.clear();
        }
        let enemy = enemy_now.map(|(o, p)| {
            let n = m.seen.len();
            if n < LAG + 4 {
                return (o, p);
            }
            let (old, older) = (m.seen[n - 1 - LAG], m.seen[n - 4 - LAG]);
            let v = (old - older) * (1.0 / (3.0 * DT));
            let v = if v.length() > 12.0 { v * (12.0 / v.length()) } else { v };
            (o, old + v * (LAG as f32 * DT))
        });
        if let Some((_, ep)) = enemy {
            if let Some(lp) = m.last_enemy {
                let v = (ep - lp) * (1.0 / DT);
                m.enemy_v = if v.length() > 40.0 { V3::ZERO } else { v };
            }
            m.last_enemy = Some(ep);
            if m.enemy_gone {
                m.enemy_gone = false;
                m.enemy_back_at = t;
                m.last_enemy = None;
            }
        } else {
            m.enemy_gone = true;
            m.last_enemy = None;
            m.enemy_v = V3::ZERO;
        }
        // their throw just left: a window
        let enemy_box = ek.map(|o| self.players[o].picked_box.is_some()).unwrap_or(false);
        if m.enemy_had_box && !enemy_box {
            m.enemy_threw_at = t;
        }
        m.enemy_had_box = enemy_box;
        // a kill gave the dash back: chain
        let kills = self.plays.iter().rev().take(40).filter(|p| p.kind == Kind::Refill && p.who == k as i32 && p.t + 2 >= t).count() as u32;
        if kills > 0 && m.plan != "chain" {
            m.plan = "chain";
            m.plan_until = t + 75;
            self.say(k, m, "chain");
        }

        // 1) danger: every box in the air that would hit us where we stand
        let mut threat: Option<(f32, V3, V3, usize)> = None; // (time to hit, box pos, box vel, box index)
        for (bi, b) in self.boxes.iter().enumerate() {
            if b.carried_by == Some(node) || !self.is_being_thrown(bi) || !self.w.active_in_hierarchy(b.node) {
                continue;
            }
            let Some(body) = self.w.nodes[b.node].body.as_ref() else { continue };
            let v = body.velocity;
            let sp = v.length();
            if sp < 8.0 {
                continue;
            }
            let bp = self.w.position(b.node);
            // seen only a beat after the throw, or after its last bounce
            let since = (t.saturating_sub(b.thrown_at)) as f32;
            let need = if b.bounces > 0 { SEE_BANK } else { SEE_DIRECT };
            if b.bounces == 0 && since < need {
                continue;
            }
            if b.bounces > 0 {
                let last_bounce = self.plays.iter().rev().find(|p| p.kind == Kind::Bounce && p.throw == b.throw_id).map(|p| p.t).unwrap_or(0);
                if !self.recording || (t.saturating_sub(last_bounce) as f32) < need {
                    if self.recording {
                        continue;
                    }
                }
            }
            let path = solids.flight(bp, v * (1.0 / sp), sp);
            if let Some((ta, _, _)) = meets(&path, me, V3::ZERO, 0.0) {
                if threat.map(|x| ta < x.0).unwrap_or(true) {
                    threat = Some((ta, bp, v, bi));
                }
            }
        }
        if let Some((ta, bp, bv, _bi)) = threat {
            // counter: meet it in the air with our own box
            if carrying && charge >= 12.0 && m.shot.is_none() {
                let v0 = (charge * 1.11).clamp(20.0, 100.0);
                let bs = bv.length();
                let bd = bv * (1.0 / bs);
                let mut found = None;
                for i in 1..=24 {
                    let tm = i as f32 * 0.012 + 2.0 * DT;
                    // where their box will be, and can ours be there then
                    let travelled = bs / DRAG * (1.0 - (-DRAG * tm).exp());
                    let target = bp + bd * travelled;
                    let off = target - me;
                    let dist = off.length();
                    let ours = v0 / DRAG * (1.0 - (-DRAG * (tm - 2.0 * DT)).exp());
                    if (ours - dist).abs() < 0.35 && dist > 0.8 && solids.clear(me, target, 0.2) {
                        found = Some(off * (1.0 / dist));
                        break;
                    }
                }
                if let Some(d) = found {
                    m.shot = Some((d, 2, "counter"));
                    self.say(k, m, "counter");
                }
            }
            if m.shot.is_none() || m.shot.map(|s| s.2) != Some("counter") {
                // step off its line: the side it isn't on, on foot if there's
                // time, else a dash left to the last instant
                let bd = bv * (1.0 / bv.length().max(1e-3));
                let perp = v3(-bd.y, bd.x, 0.0);
                let rel = me - bp;
                let side = if perp.x * rel.x + perp.y * rel.y >= 0.0 { perp } else { perp * -1.0 };
                let side = if walls.clear(me, me + side * 1.2, 0.25) { side } else { side * -1.0 };
                let walk_ok = ta > HIT_R / WALK + DT;
                if walk_ok {
                    m.dodge = side;
                    m.dodge_until = t + 4;
                } else if dash_ready {
                    m.dodge = side;
                    m.dodge_until = t + 4;
                    dash = true;
                    self.say(k, m, "matrix");
                }
            }
        }
        // a loaded aim on us: juke before it comes (a read, not a reflex;
        // sometimes a feint, sometimes the same way twice)
        if let Some((eo, ep)) = enemy {
            let p = &self.players[eo];
            let to_me = me - ep;
            let l = to_me.length().max(1e-3);
            let lined = (p.touch[1].x * to_me.x + p.touch[1].y * to_me.y) / l;
            if p.picked_box.is_some() && p.charge_timer > 20.0 && lined > 0.94 && t >= m.dodge_until && hf(seed ^ 0xa1) * 0.5 + 0.5 < 0.07 * (p.charge_timer / 90.0) {
                let side = v3(-to_me.y / l, to_me.x / l, 0.0) * if hash(seed ^ 0xa2) & 1 == 0 { 1.0 } else { -1.0 };
                let side = if walls.clear(me, me + side * 1.2, 0.25) { side } else { side * -1.0 };
                m.dodge = side;
                m.dodge_until = t + 6 + (hash(seed ^ 0xa3) % 6) as u64;
                if p.charge_timer > 60.0 && dash_ready && hash(seed ^ 0xa4) % 4 == 0 {
                    dash = true;
                }
                self.say(k, m, "juke");
            }
        }
        if t < m.dodge_until {
            mv = m.dodge;
        }

        // lining up: the aim turns at a hand's speed; it goes once it has settled
        if let Some((goal, why)) = m.lining {
            if !carrying {
                m.lining = None;
            } else {
                let cur = m.aim_cur.y.atan2(m.aim_cur.x);
                let mut d = goal.y.atan2(goal.x) - cur;
                d = d.sin().atan2(d.cos());
                let na = cur + d.clamp(-TURN, TURN);
                m.aim_cur = v3(na.cos(), na.sin(), 0.0);
                m.settled = if d.abs() < ON_AIM { m.settled + 1 } else { 0 };
                if m.settled >= 2 {
                    m.lining = None;
                    m.shot = Some((m.aim_cur, 1, why));
                } else {
                    return Self::pack(mv, V2 { x: m.aim_cur.x, y: m.aim_cur.y }, true, dash);
                }
            }
        }

        // 2) a held shot: aim stays put, then it goes
        if let Some((d, left, why)) = m.shot {
            aim = V2 { x: d.x, y: d.y };
            aiming = true;
            if left == 0 {
                aiming = false;
                self.throw_note.insert(k, why);
                m.shot = None;
                m.dashing_for = "";
                let _ = why;
            } else {
                m.shot = Some((d, left - 1, why));
            }
            return Self::pack(mv, aim, aiming, dash);
        }

        let Some((eo, ep)) = enemy else {
            return Self::pack(mv, aim, false, dash);
        };
        let ev = m.enemy_v;
        let to = ep - me;
        let dist = to.length().max(1e-3);
        let dir = to * (1.0 / dist);
        if t >= m.strafe_until {
            m.strafe = if hash(seed ^ 5) & 1 == 0 { 1.0 } else { -1.0 };
            m.strafe_until = t + 14 + (hash(seed ^ 6) % 30) as u64;
        }
        let tangent = v3(-dir.y, dir.x, 0.0) * m.strafe;

        if carrying {
            // the blade follows them while it charges, at a hand's speed
            if m.aim_cur == V3::ZERO {
                m.aim_cur = dir;
            }
            let cur = m.aim_cur.y.atan2(m.aim_cur.x);
            let mut dd = dir.y.atan2(dir.x) - cur;
            dd = dd.sin().atan2(dd.cos());
            let na = cur + dd.clamp(-TURN, TURN);
            m.aim_cur = v3(na.cos(), na.sin(), 0.0);
            aim = V2 { x: m.aim_cur.x, y: m.aim_cur.y };
            let v0 = (charge * 1.11).clamp(20.0, 100.0);
            // where we'll be at release (2 ticks of aim hold)
            let my_v = if self.players[k].dash_t > 0 { v3(self.players[k].dash_dir.x, self.players[k].dash_dir.y, 0.0) * DASH_V } else { V3::ZERO };
            let from = me + my_v * (2.0 * DT);
            let fresh = t < m.enemy_back_at + 50;
            if charge >= 10.0 && !fresh {
                if let Some((d, _ta, bounces, margin)) = self.best_shot(solids, from, v0, eo, ep, ev, 2.0 * DT, seed) {
                    // let go when they can't get away; at full charge take a
                    // near thing now and then (pressure)
                    let full = charge >= 85.0;
                    let take = margin < 0.0 || (full && margin < 0.05 && hash(seed ^ 0x77) % 6 == 0);
                    if take {
                        let why = if !m.dashing_for.is_empty() {
                            m.dashing_for
                        } else if bounces > 0 {
                            "bank"
                        } else if t.saturating_sub(m.enemy_threw_at) < 40 && m.enemy_threw_at > 0 {
                            "punish"
                        } else if m.plan == "chain" {
                            "chain"
                        } else if dist < 3.0 {
                            "rush"
                        } else {
                            "snipe"
                        };
                        self.say(k, m, why);
                        // a hair off, like a hand
                        let err = hf(seed ^ 0x5e) * 0.035;
                        let (sn, cs) = err.sin_cos();
                        m.lining = Some((v3(d.x * cs - d.y * sn, d.x * sn + d.y * cs, 0.0), why));
                        m.settled = 0;
                        return Self::pack(mv, aim, true, dash);
                    }
                }
            }
            // a dash opens a lane: corner (round something) or rush (in close)
            if dash_ready && charge >= 25.0 && mv == V3::ZERO && !fresh && t % 3 == (slot as u64) {
                let mut best: Option<(f32, V3, &'static str)> = None;
                for i in 0..16 {
                    let a = i as f32 / 16.0 * std::f32::consts::TAU;
                    let dd = v3(a.cos(), a.sin(), 0.0);
                    if !walls.clear(me, me + dd * DASH_LEN, 0.27) {
                        continue;
                    }
                    let end = me + dd * DASH_LEN;
                    let blocked_now = !solids.clear(me, ep, 0.3);
                    if let Some((_, _, _, margin)) = self.best_shot(solids, end, v0, eo, ep, ev, 8.0 * DT, seed ^ i) {
                        let why = if blocked_now { "corner" } else if (end - ep).length() < 3.0 { "rush" } else { continue };
                        if margin < 0.0 && best.map(|b| margin < b.0).unwrap_or(true) {
                            best = Some((margin, dd, why));
                        }
                    }
                }
                if let Some((_, dd, why)) = best {
                    mv = dd;
                    dash = true;
                    m.dashing_for = why;
                    self.say(k, m, why);
                }
            }
            if m.dashing_for != "" && self.players[k].dash_t == 0 && !dash {
                m.dashing_for = "";
            }
            if mv == V3::ZERO {
                // bait while they're armed and we can dodge on foot; press otherwise
                let range = if enemy_box && dash_ready { 6.5 } else if m.plan == "chain" { 2.5 } else { 4.0 };
                if enemy_box && dash_ready && dist < 7.5 {
                    self.say(k, m, "bait");
                }
                let radial = dir * ((dist - range) * 0.7).clamp(-1.0, 1.0);
                mv = tangent * 0.8 + radial;
            }
        } else {
            // get a box: the nearest one we can reach, not through their lane
            let mut best: Option<(f32, V3)> = None;
            for b in &self.boxes {
                let n = b.node;
                if !self.w.active_in_hierarchy(n) || b.carried_by.is_some() || self.w.nodes[n].collider.as_ref().map(|c| !c.enabled).unwrap_or(true) {
                    continue;
                }
                let bp = self.w.position(n);
                let d = (bp - me).length();
                let risky = if enemy_box { ((bp - ep).length() < dist) as i32 as f32 * 2.0 } else { 0.0 };
                let cost = d + risky;
                if best.map(|x| cost < x.0).unwrap_or(true) && walls.clear(me, bp, 0.24) {
                    best = Some((cost, bp));
                }
            }
            if mv == V3::ZERO {
                if let Some((c, bp)) = best {
                    let d = bp - me;
                    mv = d * (1.0 / d.length().max(1e-3));
                    // a dash to a far box only when they can't punish it
                    if c > 4.5 && dash_ready && (!enemy_box || m.plan == "chain") {
                        dash = true;
                    }
                } else {
                    mv = tangent;
                }
            }
        }
        if m.plan != "" && t >= m.plan_until {
            m.plan = "";
        }
        // a plan held for a beat, unless something urgent cut in
        if t < m.dodge_until || dash {
            m.plan_mv_until = 0;
        } else if t < m.plan_mv_until {
            mv = m.plan_mv;
        } else {
            m.plan_mv = mv;
            m.plan_mv_until = t + 4 + (hash(seed ^ 0xb1) % 5) as u64;
        }
        if !carrying {
            m.aim_cur = V3::ZERO;
        }
        Self::pack(mv, aim, aiming, dash)
    }

    fn pack(mv: V3, aim: V2, aiming: bool, dash: bool) -> Direct {
        Direct { active: true, mv: V2 { x: mv.x, y: mv.y }, aim, aiming, use_cursor: false, dash }
    }

    /// Steer the move round obstacles and hand it over as input.
    fn god_out(&self, me: V3, mv: V3, aim: V2, aiming: bool, dash: bool, solids: &Solids) -> Direct {
        let ml = mv.length();
        let mut out = mv;
        if ml > 1e-3 {
            let want = mv * (1.0 / ml);
            let base = want.y.atan2(want.x);
            out = want;
            for off in [0.0f32, 0.3, -0.3, 0.65, -0.65, 1.0, -1.0, 1.5, -1.5, 2.2, -2.2] {
                let a = base + off;
                let d = v3(a.cos(), a.sin(), 0.0);
                if solids.clear(me, me + d * 0.8, 0.26) {
                    out = d;
                    break;
                }
            }
        }
        Direct { active: true, mv: V2 { x: out.x, y: out.y }, aim, aiming, use_cursor: false, dash }
    }
}
