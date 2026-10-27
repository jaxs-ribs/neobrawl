//! The anthill: you against a colony. Ants are small, weak souls that see
//! boxes as food. Workers can barely shift a box alone; a few pushing in
//! unison walk it home, where it goes onto a stacker (a count, capped) and
//! feeds the brood: every stored box is one more ant of that stacker's
//! kind. Soldiers pick boxes up and throw them at you, and at first one hit
//! ends you. A box shattering splashes: every ant near it dies. When a
//! stacker's kind is wiped out (none left to hatch, none alive), the stack
//! overflows and every box it held spills back out, and you harden: each
//! overflow is one more hit you can take.

use crate::motion::{hash, hf, Kind};
use crate::shared::math::{v3, Quat, V3};

use super::{Game, DT};

/// The tuning, kept together so tests can try other numbers.
#[derive(Clone, Debug)]
pub struct AntCfg {
    /// a stacker holds at most this many boxes
    pub cap: u32,
    /// ants each stacker can hatch before any food comes in, per kind
    pub brood: [u32; 2],
    /// ants of a stacker's kind out at once
    pub out: [u32; 2],
    /// seconds between hatchings
    pub hatch: f32,
    /// box speed for n workers in unison: k * n^2, capped
    pub push_k: f32,
    pub push_max: f32,
    /// splash radius of a shattering box
    pub splash: f32,
    /// soldiers: throw speed, wind-up and rest (s), and reach
    pub throw_power: f32,
    pub windup: f32,
    pub rest: f32,
    pub reach: f32,
    /// seconds before you're back after a hit
    pub back_in: f32,
    /// soldiers stay in for this long at the start
    pub soldiers_after: f32,
}

impl Default for AntCfg {
    fn default() -> Self {
        AntCfg { cap: 60, brood: [12, 4], out: [4, 2], hatch: 1.4, push_k: 0.1, push_max: 1.1, splash: 1.8, throw_power: 40.0, windup: 0.9, rest: 4.0, reach: 8.0, back_in: 1.6, soldiers_after: 8.0 }
    }
}

pub const WORKER: u8 = 0;
pub const SOLDIER: u8 = 1;
/// walking pace per kind (you walk at 8)
const PACE: [f32; 2] = [4.2, 5.0];
/// how close a worker must stand to its slot to count as pushing
const SLOT_NEAR: f32 = 0.45;
/// a box this close to a stacker is home
const HOME: f32 = 1.3;
/// soul size per kind: workers small, soldiers nearer yours
const ANT_SCALE: [f32; 2] = [0.55, 0.8];

#[derive(Clone, Debug, Default)]
pub struct Ant {
    pub node: usize,
    pub kind: u8,
    pub stacker: usize,
    pub dead: bool,
    vel: V3,
    /// workers: the box being brought home
    pub target: Option<usize>,
    /// soldiers: the box in hand, and when the throw leaves
    pub carry: Option<usize>,
    pub wind_until: Option<u64>,
    pub aim: V3,
    rest_until: u64,
    wander: V3,
    wander_until: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Stacker {
    pub pos: V3,
    pub kind: u8,
    /// box nodes stored here (hidden until the overflow)
    pub boxes: Vec<usize>,
    /// ants still to hatch
    pub brood: u32,
    pub hatched: u32,
    next_hatch: u64,
    pub overflowed: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Anthill {
    pub on: bool,
    pub cfg: AntCfg,
    pub ants: Vec<Ant>,
    pub stackers: Vec<Stacker>,
    pub colony: V3,
    pub start: V3,
    pub lo: V3,
    pub hi: V3,
    /// hits you can still take before one ends you
    pub shield: u32,
    pub deaths: u32,
    pub kills: u32,
    pub delivered: u32,
    pub dead_until: Option<u64>,
    pub won_at: Option<u64>,
    /// (box, workers pushing it) last tick
    pub pushers: Vec<(usize, u8)>,
    pub started_at: u64,
    /// recycled ant souls
    spare: Vec<usize>,
    /// where the living ants stood last tick (they keep apart)
    near: Vec<V3>,
    template: usize,
}

impl Game {
    pub fn ant_of(&self, node: usize) -> Option<usize> {
        if !self.anthill.on {
            return None;
        }
        self.anthill.ants.iter().position(|a| a.node == node && !a.dead)
    }

    fn me(&self) -> Option<(usize, usize)> {
        let k = self.players.iter().position(|p| &*self.w.nodes[p.node].tag == "PlayerOneScreen1")?;
        Some((k, self.players[k].node))
    }

    /// Turn the running match into the anthill (call once the match is up).
    pub fn start_anthill(&mut self, cfg: AntCfg) -> bool {
        if self.anthill.on {
            return true;
        }
        let Some(map) = ["Map1", "Map2", "Map3", "Map4"].iter().filter_map(|n| self.w.find(n)).find(|&m| self.w.active_in_hierarchy(m)) else { return false };
        let Some((_, p1)) = self.me() else { return false };
        let Some(k2) = self.players.iter().position(|p| &*self.w.nodes[p.node].tag == "PlayerOneScreen2") else { return false };
        let p2 = self.players[k2].node;
        let mut sub = Vec::new();
        self.w.descendants(map, &mut sub);
        let big: Vec<V3> = sub.iter().filter(|&&n| self.w.nodes[n].name.starts_with("Wall Big")).map(|&n| self.w.position(n)).collect();
        if big.len() < 4 {
            return false;
        }
        let (lo, hi) = big.iter().fold((v3(f32::MAX, f32::MAX, 0.0), v3(f32::MIN, f32::MIN, 0.0)), |(lo, hi), p| (v3(lo.x.min(p.x), lo.y.min(p.y), 0.0), v3(hi.x.max(p.x), hi.y.max(p.y), 0.0)));
        let z = self.w.position(p1).z;
        let c0 = v3((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0, z);
        let (hx, hy) = ((hi.x - lo.x) / 2.0 - 1.0, (hi.y - lo.y) / 2.0 - 1.0);
        // an open floor: the outer walls and the boxes stay, the rest goes
        let box_nodes: Vec<usize> = self.boxes.iter().map(|b| b.node).filter(|n| sub.contains(n)).collect();
        for &b in &box_nodes {
            if self.w.nodes[b].parent != Some(map) {
                self.w.reparent(b, map);
            }
        }
        for c in self.w.nodes[map].children.clone() {
            let n = &self.w.nodes[c].name;
            if n.starts_with("Wall Big") || &**n == "SpawnPoints" || box_nodes.contains(&c) {
                continue;
            }
            self.w.set_active(c, false);
        }
        // the colony along the far wall, you at the near one
        let wide = hx >= hy;
        let (colony, start) = if wide { (c0 + v3(hx - 1.6, 0.0, 0.0), c0 + v3(-hx + 1.2, 0.0, 0.0)) } else { (c0 + v3(0.0, hy - 1.6, 0.0), c0 + v3(0.0, -hy + 1.2, 0.0)) };
        let across = if wide { v3(0.0, 1.0, 0.0) } else { v3(1.0, 0.0, 0.0) };
        // stackers in a row: soldier in the middle, workers either side
        let kinds = [WORKER, SOLDIER, WORKER];
        let stackers: Vec<Stacker> = kinds
            .iter()
            .enumerate()
            .map(|(i, &kind)| Stacker { pos: colony + across * ((i as f32 - 1.0) * 2.6), kind, brood: cfg.brood[kind as usize], next_hatch: self.frame + 30 + i as u64 * 20 + if kind == SOLDIER { super::ticks(cfg.soldiers_after) as u64 } else { 0 }, ..Default::default() })
            .collect();
        // the boxes scattered over the floor, clear of the colony and of you
        let seed = self.frame as u32;
        for (k, &b) in box_nodes.iter().enumerate() {
            let mut p = c0;
            for t in 0..40u32 {
                let s = hash(seed ^ (k as u32 * 7919 + t * 31));
                p = c0 + v3(hf(s) * hx, hf(s ^ 0x5bd1) * hy, 0.0);
                let clear = (p - colony).length() > 4.2 && (p - start).length() > 2.5 && box_nodes[..k].iter().all(|&o| (self.w.position(o) - p).length() > 1.0);
                if clear {
                    break;
                }
            }
            let bz = self.w.position(b).z;
            let p = v3(p.x, p.y, bz);
            self.w.set_position(b, p);
            self.w.set_rotation(b, Quat::IDENTITY);
            if let Some(body) = self.w.nodes[b].body.as_mut() {
                body.velocity = V3::ZERO;
                body.spin = 0.0;
            }
            if let Some(&bi) = self.box_of.get(&b) {
                self.boxes[bi].initial_pos = p;
                self.boxes[bi].initial_rot = Quat::IDENTITY;
            }
            self.phys.forget(b);
        }
        self.w.set_position(p1, start);
        self.phys.forget(p1);
        self.ai[1].on = false;
        self.w.set_active(p2, false);
        self.anthill = Anthill { on: true, cfg, stackers, colony, start, lo, hi, template: p2, started_at: self.frame, ..Default::default() };
        true
    }

    /// A fresh ant soul from the second player's, small.
    fn ant_soul(&mut self, at: V3, kind: u8) -> usize {
        let node = match self.anthill.spare.pop() {
            Some(n) => n,
            None => {
                let k2 = self.players.iter().position(|p| p.node == self.anthill.template);
                let skip: Vec<usize> = k2.and_then(|k| self.players[k].aimer_anchor).into_iter().collect();
                let n = self.w.clone_subtree(self.anthill.template, &skip);
                self.w.nodes[n].name = "Ant".into();
                self.w.nodes[n].tag = "Untagged".into();
                n
            }
        };
        self.w.nodes[node].lscale = self.w.nodes[self.anthill.template].lscale * ANT_SCALE[kind as usize];
        self.w.set_active(node, true);
        self.w.set_position(node, at);
        if let Some(body) = self.w.nodes[node].body.as_mut() {
            body.velocity = V3::ZERO;
        }
        self.phys.forget(node);
        node
    }

    fn box_free(&self, bi: usize) -> bool {
        let carried: Vec<usize> = self.anthill.ants.iter().filter_map(|a| a.carry).collect();
        self.box_free_of(bi, &carried)
    }

    fn box_free_of(&self, bi: usize, carried: &[usize]) -> bool {
        let b = &self.boxes[bi];
        let n = b.node;
        self.w.active_in_hierarchy(n)
            && !b.raving
            && !self.is_being_thrown(bi)
            && self.w.nodes[n].collider.as_ref().map(|c| c.enabled).unwrap_or(false)
            && !self.players.iter().any(|p| p.picked_box == Some(n))
            && !carried.contains(&n)
    }

    /// Each tick, before physics: hatching, feeding, fighting, overflowing.
    pub(super) fn anthill_step(&mut self) {
        if !self.anthill.on {
            return;
        }
        let t = self.frame;
        let cfg = self.anthill.cfg.clone();
        let me = self.me();
        // you, back after a hit
        if let (Some(until), Some((_, p1))) = (self.anthill.dead_until, me) {
            if t >= until {
                self.anthill.dead_until = None;
                let s = self.anthill.start;
                self.w.set_position(p1, s);
                self.phys.forget(p1);
                self.fx_respawn(p1);
            }
        }
        let you = me.map(|(_, n)| self.w.position(n)).filter(|p| p.x != -666.0);
        // hatching
        for s in 0..self.anthill.stackers.len() {
            let st = &self.anthill.stackers[s];
            if st.overflowed || st.brood == 0 || t < st.next_hatch {
                continue;
            }
            let kind = st.kind;
            let out = self.anthill.ants.iter().filter(|a| !a.dead && a.stacker == s).count() as u32;
            if out >= cfg.out[kind as usize] {
                continue;
            }
            let at = st.pos + v3(hf(t as u32 ^ s as u32 * 13) * 0.6, hf(t as u32 * 3 + 1) * 0.6, 0.0);
            let node = self.ant_soul(at, kind);
            self.fx_respawn(node);
            let st = &mut self.anthill.stackers[s];
            st.brood -= 1;
            st.hatched += 1;
            st.next_hatch = t + super::ticks(cfg.hatch) as u64;
            self.anthill.ants.push(Ant { node, kind, stacker: s, ..Default::default() });
        }
        // workers pick boxes, crowding onto ones others already push
        let homes: Vec<V3> = self.anthill.stackers.iter().filter(|s| !s.overflowed && (s.boxes.len() as u32) < cfg.cap).map(|s| s.pos).collect();
        let nearest_home = |p: V3| homes.iter().copied().min_by(|a, b| (*a - p).length().partial_cmp(&(*b - p).length()).unwrap());
        let mut ants = std::mem::take(&mut self.anthill.ants);
        let mut carried: Vec<usize> = ants.iter().filter(|a| !a.dead).filter_map(|a| a.carry).collect();
        let mut claim: Vec<u32> = vec![0; self.boxes.len()];
        for a in ants.iter().filter(|a| !a.dead) {
            if let Some(bi) = a.target {
                claim[bi] += 1;
            }
        }
        // pushers per box this tick
        let mut pushing: Vec<Vec<usize>> = vec![Vec::new(); self.boxes.len()];
        for (i, a) in ants.iter_mut().enumerate() {
            if a.dead {
                continue;
            }
            let pos = self.w.position(a.node);
            let seed = (t as u32).wrapping_mul(2654435761) ^ (a.node as u32 * 977);
            let mut want = V3::ZERO;
            let mut pace = PACE[a.kind as usize];
            if a.kind == WORKER {
                if a.target.map(|bi| !self.box_free_of(bi, &carried)).unwrap_or(false) {
                    if let Some(bi) = a.target.take() {
                        claim[bi] = claim[bi].saturating_sub(1);
                    }
                }
                if a.target.is_none() && !homes.is_empty() {
                    let pick = (0..self.boxes.len())
                        .filter(|&bi| claim[bi] < 3 && self.box_free_of(bi, &carried))
                        .filter(|&bi| nearest_home(self.w.position(self.boxes[bi].node)).map(|h| (h - self.w.position(self.boxes[bi].node)).length() > HOME).unwrap_or(false))
                        .map(|bi| (bi, (self.w.position(self.boxes[bi].node) - pos).length() - 3.0 * claim[bi].min(3) as f32))
                        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
                    if let Some((bi, _)) = pick {
                        a.target = Some(bi);
                        claim[bi] += 1;
                    }
                }
                if let Some(bi) = a.target {
                    let bp = self.w.position(self.boxes[bi].node);
                    let home = nearest_home(bp).unwrap_or(self.anthill.colony);
                    let dir = nz(home - bp);
                    // a slot behind the box, spread across its back by who you are
                    let side = v3(-dir.y, dir.x, 0.0) * (((i % 4) as f32 - 1.5) * 0.22);
                    let slot = bp - dir * 0.78 + side;
                    let d = slot - pos;
                    if d.length() < SLOT_NEAR {
                        pushing[bi].push(i);
                        want = dir;
                        pace = cfg.push_max;
                    } else {
                        // round the box, not through it
                        let to_box = bp - pos;
                        let tl = to_box.length();
                        let ahead = to_box.x * dir.x + to_box.y * dir.y < 0.0;
                        let detour = if ahead && tl < 1.3 { v3(-dir.y, dir.x, 0.0) * if (side.x * dir.y - side.y * dir.x) > 0.0 { -1.0 } else { 1.0 } } else { V3::ZERO };
                        want = nz(nz(d) + detour) * (d.length() / 0.8).min(1.0);
                    }
                } else {
                    // nothing to fetch: mill about the colony
                    if t >= a.wander_until {
                        let c = self.anthill.colony;
                        a.wander = c + v3(hf(seed) * 3.0, hf(seed ^ 9) * 3.0, 0.0);
                        a.wander_until = t + 60 + (hash(seed) % 80) as u64;
                    }
                    let d = a.wander - pos;
                    want = nz(d) * (d.length() / 1.0).min(1.0) * 0.5;
                }
            } else if let Some(you) = you {
                // soldiers: fetch a box, close in, wind up in plain sight, throw
                if let Some(b) = a.carry {
                    self.w.set_position(b, v3(pos.x, pos.y, self.w.position(b).z));
                    let to = you - pos;
                    let l = to.length();
                    if let Some(at) = a.wind_until {
                        // the wind-up tracks you, slower as it nears the release
                        let left = (at.saturating_sub(t)) as f32 / super::ticks(cfg.windup) as f32;
                        let aim = nz(to);
                        a.aim = nz(a.aim + (aim - a.aim) * (0.25 * left).max(0.02));
                        if t >= at {
                            self.ant_throw(a, b, cfg.throw_power);
                        }
                    } else if l < cfg.reach && t >= a.rest_until {
                        a.wind_until = Some(t + super::ticks(cfg.windup) as u64);
                        a.aim = nz(to);
                    } else {
                        // keep a fighting distance
                        let w = if l > cfg.reach * 0.8 { nz(to) } else if l < 4.0 { nz(to) * -1.0 } else { nz(v3(-to.y, to.x, 0.0)) * if hash(a.node as u32 ^ (t / 100) as u32) & 1 == 0 { 1.0 } else { -1.0 } * 0.6 };
                        want = w;
                    }
                } else {
                    let pick = (0..self.boxes.len()).filter(|&bi| self.box_free_of(bi, &carried)).map(|bi| (bi, (self.w.position(self.boxes[bi].node) - pos).length())).min_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
                    if let Some((bi, l)) = pick {
                        let bp = self.w.position(self.boxes[bi].node);
                        if l < 0.75 {
                            let b = self.boxes[bi].node;
                            a.carry = Some(b);
                            carried.push(b);
                            self.boxes[bi].carried_by = Some(a.node);
                            if let Some(c) = self.w.nodes[b].collider.as_mut() {
                                c.enabled = false;
                            }
                            self.phys.set_ignore(b, a.node, true);
                        } else {
                            want = nz(bp - pos);
                        }
                    } else {
                        want = nz(self.anthill.colony - pos) * 0.4;
                    }
                }
            }
            // ants keep a little apart
            for b in &self.anthill.near {
                let d = pos - *b;
                let l = d.length();
                if l > 1e-3 && l < 0.7 {
                    want = want + d * (0.6 * (0.7 - l) / l);
                }
            }
            let wl = want.length();
            let target_v = if wl > 1e-3 { want * (pace * wl.min(1.0) / wl) } else { V3::ZERO };
            let k = 1.0 - (-DT / 0.08f32).exp();
            a.vel = a.vel + (target_v - a.vel) * k;
            if a.wind_until.is_some() {
                a.vel = V3::ZERO;
            }
            if let Some(body) = self.w.nodes[a.node].body.as_mut() {
                body.velocity = v3(a.vel.x, a.vel.y, 0.0);
            }
        }
        // boxes move with their pushers: one barely, a few in unison
        self.anthill.pushers = pushing.iter().enumerate().filter(|(_, w)| !w.is_empty()).map(|(b, w)| (b, w.len() as u8)).collect();
        for (bi, who) in pushing.iter().enumerate() {
            if who.is_empty() {
                continue;
            }
            let n = who.len() as f32;
            let bp = self.w.position(self.boxes[bi].node);
            let home = nearest_home(bp).unwrap_or(self.anthill.colony);
            let dir = nz(home - bp);
            let sp = (cfg.push_k * n * n).min(cfg.push_max);
            let node = self.boxes[bi].node;
            if let Some(body) = self.w.nodes[node].body.as_mut() {
                body.velocity = dir * sp;
            }
        }
        self.anthill.ants = ants;
        self.anthill.near = self.anthill.ants.iter().filter(|a| !a.dead).map(|a| self.w.position(a.node)).collect();
        // home: the box goes onto the nearest stacker
        for bi in 0..self.boxes.len() {
            if !self.box_free(bi) {
                continue;
            }
            let bp = self.w.position(self.boxes[bi].node);
            let Some(s) = self.anthill.stackers.iter().enumerate().filter(|(_, s)| !s.overflowed && (s.boxes.len() as u32) < cfg.cap).min_by(|a, b| (a.1.pos - bp).length().partial_cmp(&(b.1.pos - bp).length()).unwrap()).map(|(s, _)| s) else { continue };
            if (self.anthill.stackers[s].pos - bp).length() > HOME {
                continue;
            }
            let n = self.boxes[bi].node;
            self.w.set_active(n, false);
            self.phys.forget(n);
            let st = &mut self.anthill.stackers[s];
            st.boxes.push(n);
            st.brood += 1;
            self.anthill.delivered += 1;
            let at = st.pos;
            self.motion.ring(false, at, 0.3, 1.4, 0.04, 0.35, [0.1, 0.1, 0.12, 1.0]);
            self.cue("gather", at, 0.4);
            for a in self.anthill.ants.iter_mut() {
                if a.target == Some(bi) {
                    a.target = None;
                }
            }
        }
        // a kind wiped out: its stack spills
        for s in 0..self.anthill.stackers.len() {
            let st = &self.anthill.stackers[s];
            if st.overflowed || st.brood > 0 || st.hatched == 0 || self.anthill.ants.iter().any(|a| !a.dead && a.stacker == s) {
                continue;
            }
            self.anthill_overflow(s);
        }
        if self.anthill.won_at.is_none() && self.anthill.stackers.iter().all(|s| s.overflowed) {
            self.anthill.won_at = Some(t);
            let c = self.anthill.colony;
            self.cue("home", c, 8.0);
        }
    }

    fn ant_throw(&mut self, a: &mut Ant, b: usize, power: f32) {
        let angle = a.aim;
        a.carry = None;
        a.wind_until = None;
        a.rest_until = self.frame + super::ticks(self.anthill.cfg.rest) as u64;
        if let Some(c) = self.w.nodes[b].collider.as_mut() {
            c.enabled = true;
        }
        if let Some(body) = self.w.nodes[b].body.as_mut() {
            body.velocity = v3(angle.x * power, angle.y * power, 0.0);
        }
        self.tasks.push(super::Task::ThrowRotation { node: b, i: 0, n: (power / 3.0).ceil() as i32 });
        self.tasks.push(super::Task::Unignore { a: b, b: a.node, wait: super::ticks(0.5) });
        let woosh = self.menu.woosh.clone();
        self.play(&woosh, 0.6);
        if let Some(&bi) = self.box_of.get(&b) {
            let x = &mut self.boxes[bi];
            x.being_thrown = true;
            x.being_thrown_pickup = true;
            x.bounces = 0;
            x.spent = false;
            x.carried_by = Some(a.node);
            x.thrown_charge = power / 1.11;
            self.tasks.push(super::Task::ResetBeingThrown { b: bi, stage: 0, wait: 3 });
        }
    }

    fn anthill_overflow(&mut self, s: usize) {
        let st = &mut self.anthill.stackers[s];
        st.overflowed = true;
        let at = st.pos;
        let boxes = std::mem::take(&mut st.boxes);
        let n = boxes.len().max(1) as f32;
        for (k, b) in boxes.into_iter().enumerate() {
            let a = k as f32 / n * std::f32::consts::TAU + hf(k as u32 * 77) * 0.3;
            let r = 0.9 + (k % 3) as f32 * 0.55;
            let p = at + v3(a.cos(), a.sin(), 0.0) * r;
            let bz = self.w.position(b).z;
            self.w.set_active(b, true);
            self.w.set_position(b, v3(p.x, p.y, bz));
            if let Some(body) = self.w.nodes[b].body.as_mut() {
                body.velocity = v3(a.cos(), a.sin(), 0.0) * 4.0;
            }
            if let Some(&bi) = self.box_of.get(&b) {
                self.boxes[bi].initial_pos = v3(p.x, p.y, bz);
                self.boxes[bi].carried_by = None;
            }
            self.phys.forget(b);
        }
        self.anthill.shield += 1;
        let col = [0.1, 0.1, 0.12, 1.0];
        self.motion.ring(false, at, 0.4, 4.0, 0.06, 0.6, col);
        self.motion.add_shake(0.25);
        self.cue("home", at, 5.0);
    }

    /// A box shattered at `at`: every ant close by goes with it.
    /// Only your boxes fell soldiers (workers are fair game for any).
    pub(super) fn anthill_splash(&mut self, at: V3, hit: V3, thrower: Option<usize>) {
        if !self.anthill.on {
            return;
        }
        let r = self.anthill.cfg.splash;
        let yours = thrower.map(|t| self.player_of.contains_key(&t)).unwrap_or(false);
        let near: Vec<usize> = self.anthill.ants.iter().filter(|a| !a.dead && (yours || a.kind != SOLDIER) && (self.w.position(a.node) - at).length() < r).map(|a| a.node).collect();
        for n in near {
            self.anthill_ant_hit(n, hit);
        }
    }

    /// An ant is struck (a box, or a splash).
    pub(super) fn anthill_ant_hit(&mut self, node: usize, hit: V3) {
        let Some(k) = self.ant_of(node) else { return };
        if std::env::var("NB_TRACE_ANTS").is_ok() {
            eprintln!("ant {} kind {} dies at frame {}: {}", node, self.anthill.ants[k].kind, self.frame, std::backtrace::Backtrace::force_capture().to_string().lines().filter(|l| l.contains("neobrawl::game")).take(4).collect::<Vec<_>>().join(" | "));
        }
        let p = self.w.position(node);
        let col = self.soul_color(node);
        let a = if hit.length() > 1e-3 { hit.y.atan2(hit.x) } else { 0.0 };
        self.motion.push(Kind::Blob { c: p, a, r: 0.15 }, false, 0.0, 0.12, col);
        self.motion.burst(false, p, 8, 7.0, 0.04, 0.8, col, None);
        self.motion.add_shake(0.05);
        self.cue("fling", p, 0.4);
        // what it held drops where it fell
        let ant = &mut self.anthill.ants[k];
        ant.dead = true;
        ant.target = None;
        ant.wind_until = None;
        if let Some(b) = ant.carry.take() {
            if let Some(c) = self.w.nodes[b].collider.as_mut() {
                c.enabled = true;
            }
            if let Some(&bi) = self.box_of.get(&b) {
                self.boxes[bi].carried_by = None;
            }
            self.phys.set_ignore(b, node, false);
        }
        self.anthill.kills += 1;
        if let Some(body) = self.w.nodes[node].body.as_mut() {
            body.velocity = V3::ZERO;
        }
        self.w.set_active(node, false);
        self.phys.forget(node);
        self.anthill.spare.push(node);
    }

    /// A thrown box reached you. True if you took it (shielded or not).
    pub(super) fn anthill_you_hit(&mut self, k: usize, hit: V3) {
        let node = self.players[k].node;
        if self.anthill.dead_until.is_some() {
            return;
        }
        let p = self.w.position(node);
        let col = self.soul_color(node);
        if self.anthill.shield > 0 {
            self.anthill.shield -= 1;
            self.motion.ring(false, p, 0.4, 2.2, 0.06, 0.45, col);
            self.motion.add_shake(0.15);
            self.cue("bounce", p, 1.0);
            return;
        }
        self.drop_box(k);
        self.kill_impulse = hit;
        self.fx_kill(node, node);
        self.anthill.deaths += 1;
        self.anthill.dead_until = Some(self.frame + super::ticks(self.anthill.cfg.back_in) as u64);
        self.w.set_position(node, v3(-666.0, -666.0, -666.0));
        self.phys.forget(node);
    }
}

/// Unit length in the plane (zero stays zero).
fn nz(v: V3) -> V3 {
    let l = (v.x * v.x + v.y * v.y).sqrt();
    if l > 1e-5 { v3(v.x / l, v.y / l, 0.0) } else { V3::ZERO }
}

impl Game {
    #[doc(hidden)]
    pub fn anthill_splash_pub(&mut self, at: V3) {
        let me = self.me().map(|m| m.1);
        self.anthill_splash(at, V3::ZERO, me);
    }
}
