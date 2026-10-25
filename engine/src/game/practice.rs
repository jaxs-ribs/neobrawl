//! Target practice: a cleared arena, a field of boxes in the middle, and
//! souls that only ever try to get out of the way, each kept to its own
//! zone (some roomy, some tight). Hit one and it shatters into darts that
//! fly home to you; it condenses back in its zone a moment later.

use std::collections::HashMap;

use crate::motion::{hash, hf, Kind};
use crate::shared::math::{v3, Quat, V3};

use super::{Game, DT};

/// Respawn delay (s) and how many zones (one target each).
const BACK_IN: f32 = 0.45;
const ZONES: usize = 6;
/// A target's calm pace, its dash, and how long it takes to see a throw.
const WALK: f32 = 5.0;
const DASH: f32 = 17.0;
const FLINCH: u64 = 12;

#[derive(Clone, Debug, Default)]
pub struct Target {
    pub node: usize,
    pub zone: V3,
    pub radius: f32,
    vel: V3,
    goal: V3,
    goal_until: u64,
    dodge: V3,
    dodge_until: u64,
    dash_t: u32,
    dash_cd: u32,
    noticed: HashMap<usize, u64>,
    lined_since: Option<u64>,
    pub dead_until: Option<u64>,
}

#[derive(Clone, Debug, Default)]
pub struct Practice {
    pub on: bool,
    pub targets: Vec<Target>,
    pub hits: u32,
    pub streak: u32,
    last_hit: u64,
    pub center: V3,
}

impl Game {
    pub fn practice_target(&self, node: usize) -> Option<usize> {
        self.practice.targets.iter().position(|t| t.node == node)
    }

    /// Turn the running match into target practice (call once the match is up).
    pub fn start_practice(&mut self) -> bool {
        if self.practice.on {
            return true;
        }
        let Some(map) = ["Map1", "Map2", "Map3", "Map4"].iter().filter_map(|n| self.w.find(n)).find(|&m| self.w.active_in_hierarchy(m)) else { return false };
        let p1 = self.players.iter().find(|p| self.w.nodes[p.node].tag == "PlayerOneScreen1").map(|p| p.node);
        let p2 = self.players.iter().position(|p| self.w.nodes[p.node].tag == "PlayerOneScreen2");
        let (Some(p1), Some(k2)) = (p1, p2) else { return false };
        let p2 = self.players[k2].node;
        // the arena's bounds from its big outer walls
        let mut sub = Vec::new();
        self.w.descendants(map, &mut sub);
        let big: Vec<V3> = sub.iter().filter(|&&n| self.w.nodes[n].name.starts_with("Wall Big")).map(|&n| self.w.position(n)).collect();
        if big.len() < 4 {
            return false;
        }
        let (lo, hi) = big.iter().fold((v3(f32::MAX, f32::MAX, 0.0), v3(f32::MIN, f32::MIN, 0.0)), |(lo, hi), p| (v3(lo.x.min(p.x), lo.y.min(p.y), 0.0), v3(hi.x.max(p.x), hi.y.max(p.y), 0.0)));
        let z = self.w.position(p1).z;
        let c0 = v3((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0, z);
        let (hx, hy) = ((hi.x - lo.x) / 2.0 - 0.9, (hi.y - lo.y) / 2.0 - 0.9);
        // every box comes out of its group to the map itself; everything
        // else inside (the lens walls and their groups) goes
        let box_nodes: Vec<usize> = self.boxes.iter().map(|b| b.node).filter(|n| sub.contains(n)).collect();
        for &b in &box_nodes {
            if self.w.nodes[b].parent != Some(map) {
                self.w.reparent(b, map);
            }
        }
        for c in self.w.nodes[map].children.clone() {
            let n = &self.w.nodes[c].name;
            if n.starts_with("Wall Big") || n == "SpawnPoints" || box_nodes.contains(&c) {
                continue;
            }
            self.w.set_active(c, false);
        }
        // the box field: a grid in the middle; respawning boxes come back here
        let cols = (box_nodes.len() as f32).sqrt().ceil() as usize;
        let gap = 1.0;
        let half = (cols as f32 - 1.0) * gap / 2.0;
        for (k, &b) in box_nodes.iter().enumerate() {
            let p = c0 + v3((k % cols) as f32 * gap - half, (k / cols) as f32 * gap - half, 0.0);
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
                self.boxes[bi].initial_rot = self.w.rotation(b);
            }
            self.phys.forget(b);
        }
        // you start just below the field; the opponent leaves
        self.w.set_position(p1, c0 + v3(0.0, -(half + 2.0), 0.0));
        self.phys.forget(p1);
        self.ai[1].on = false;
        self.w.set_active(p2, false);
        // zones round the field, roomy to tight, kept inside the walls
        let mut radii = [1.3f32, 1.8, 2.3, 2.8, 3.3, 3.9];
        for k in (1..radii.len()).rev() {
            let j = hash(k as u32 * 7919 + self.frame as u32) as usize % (k + 1);
            radii.swap(k, j);
        }
        let anchor = self.players[k2].aimer_anchor;
        let skip: Vec<usize> = anchor.into_iter().collect();
        let mut targets = Vec::new();
        for (k, &r) in radii.iter().enumerate().take(ZONES) {
            let a = k as f32 / ZONES as f32 * std::f32::consts::TAU + 0.35 + hf(k as u32 * 31) * 0.2;
            let dir = v3(a.cos(), a.sin(), 0.0);
            let room = ((hx - r) / dir.x.abs().max(1e-3)).min((hy - r) / dir.y.abs().max(1e-3));
            let d = (half + r + 1.6).min(room).max(0.0);
            let zone = c0 + dir * d;
            let node = self.w.clone_subtree(p2, &skip);
            self.w.nodes[node].name = format!("PracticeTarget{k}");
            self.w.nodes[node].tag = "Untagged".into();
            self.w.set_active(node, true);
            self.w.set_position(node, zone);
            if let Some(body) = self.w.nodes[node].body.as_mut() {
                body.velocity = V3::ZERO;
            }
            targets.push(Target { node, zone, radius: r, goal: zone, ..Default::default() });
        }
        self.practice = super::practice::Practice { on: true, targets, hits: 0, streak: 0, last_hit: 0, center: c0 };
        for k in 0..self.practice.targets.len() {
            let n = self.practice.targets[k].node;
            self.fx_respawn(n);
        }
        true
    }

    /// Each tick, before physics: the targets decide where to go.
    pub(super) fn practice_step(&mut self) {
        if !self.practice.on {
            return;
        }
        let t = self.frame;
        let me = self.players.iter().find(|p| self.w.nodes[p.node].tag == "PlayerOneScreen1").map(|p| (p.node, p.touch[1], p.charge_timer));
        let mut targets = std::mem::take(&mut self.practice.targets);
        for (k, g) in targets.iter_mut().enumerate() {
            let seed = (t as u32).wrapping_mul(2654435761) ^ (k as u32 * 977 + 3);
            // dead: back in a moment, somewhere in its zone
            if let Some(until) = g.dead_until {
                if t < until {
                    continue;
                }
                g.dead_until = None;
                self.w.set_active(g.node, true);
                let a = (hf(seed) * 0.5 + 0.5) * std::f32::consts::TAU;
                let r = g.radius * (hf(seed ^ 7) * 0.5 + 0.5).sqrt() * 0.8;
                let p = g.zone + v3(a.cos(), a.sin(), 0.0) * r;
                self.w.set_position(g.node, p);
                self.phys.forget(g.node);
                g.vel = V3::ZERO;
                g.goal = p;
                self.fx_respawn(g.node);
                continue;
            }
            let pos = self.w.position(g.node);
            if g.dash_cd > 0 {
                g.dash_cd -= 1;
            }
            // reading the shooter: a charged aim held on it makes it juke
            if let Some((pn, aim, charge)) = me {
                let pp = self.w.position(pn);
                let to = pos - pp;
                let l = (to.x * to.x + to.y * to.y).sqrt().max(1e-3);
                let lined = (aim.x * to.x + aim.y * to.y) / l;
                if charge > 12.0 && lined > 0.94 {
                    g.lined_since.get_or_insert(t);
                } else {
                    g.lined_since = None;
                }
                if let Some(since) = g.lined_since {
                    let held = ((t - since) as f32 / 25.0).min(2.0);
                    let p = 0.04 * (charge / 90.0) * (0.5 + held);
                    if t >= g.dodge_until && (hash(seed ^ 0x11) % 10000) as f32 / 10000.0 < p {
                        let side = if hash(seed ^ 0x12) & 1 == 0 { 1.0 } else { -1.0 };
                        g.dodge = v3(-to.y / l, to.x / l, 0.0) * side;
                        g.dodge_until = t + 8 + (hash(seed ^ 0x13) % 6) as u64;
                        if charge > 60.0 && g.dash_cd == 0 && hash(seed ^ 0x14) % 3 == 0 {
                            g.dash_t = 6;
                            g.dash_cd = 70;
                        }
                    }
                }
            }
            // boxes in flight, seen after a human reaction time
            for b in &self.boxes {
                let Some(body) = self.w.nodes[b.node].body.as_ref() else { continue };
                let v = body.velocity;
                let sp2 = v.x * v.x + v.y * v.y;
                if sp2 < 100.0 || !self.w.active_in_hierarchy(b.node) {
                    g.noticed.remove(&b.node);
                    continue;
                }
                let seen = *g.noticed.entry(b.node).or_insert_with(|| t + FLINCH + (hash(seed ^ b.node as u32) % 8) as u64);
                if t < seen || t < g.dodge_until {
                    continue;
                }
                let r = pos - self.w.position(b.node);
                let tc = (r.x * v.x + r.y * v.y) / sp2;
                if !(0.0..0.6).contains(&tc) {
                    continue;
                }
                let miss = r - v * tc;
                if miss.length() < 1.0 && hash(seed ^ (b.node as u32 * 5)) % 4 != 0 {
                    let sp = sp2.sqrt();
                    let perp = v3(-v.y / sp, v.x / sp, 0.0);
                    g.dodge = if perp.x * miss.x + perp.y * miss.y >= 0.0 { perp } else { perp * -1.0 };
                    g.dodge_until = t + 10;
                    if tc < 0.3 && g.dash_cd == 0 {
                        g.dash_t = 6;
                        g.dash_cd = 70;
                    }
                }
            }
            // otherwise: amble about the zone
            if t >= g.goal_until || (g.goal - pos).length() < 0.3 {
                let a = (hf(seed ^ 0x21) * 0.5 + 0.5) * std::f32::consts::TAU;
                let r = g.radius * (hf(seed ^ 0x22) * 0.5 + 0.5).sqrt() * 0.85;
                g.goal = g.zone + v3(a.cos(), a.sin(), 0.0) * r;
                g.goal_until = t + 25 + (hash(seed ^ 0x23) % 60) as u64;
            }
            let mut want = if t < g.dodge_until {
                g.dodge
            } else {
                let d = g.goal - pos;
                let l = d.length();
                if l > 1e-3 { d * (1.0 / l) * (l / 1.2).min(1.0) } else { V3::ZERO }
            };
            // the zone's edge pulls it back in
            let out = pos - g.zone;
            let ol = (out.x * out.x + out.y * out.y).sqrt();
            if ol > g.radius {
                want = want + out * (-(ol - g.radius) * 1.5 / ol.max(1e-3));
            }
            let speed = if g.dash_t > 0 {
                g.dash_t -= 1;
                DASH
            } else {
                WALK
            };
            let wl = want.length();
            let target_v = if wl > 1e-3 { want * (speed * wl.min(1.0) / wl) } else { V3::ZERO };
            let k = if g.dash_t > 0 { 1.0 } else { 1.0 - (-DT / 0.06f32).exp() };
            g.vel = g.vel + (target_v - g.vel) * k;
            if let Some(body) = self.w.nodes[g.node].body.as_mut() {
                body.velocity = v3(g.vel.x, g.vel.y, 0.0);
            }
        }
        self.practice.targets = targets;
    }

    /// A thrown box struck a target.
    pub(super) fn practice_hit(&mut self, node: usize, hit: V3) {
        let Some(k) = self.practice_target(node) else { return };
        if self.practice.targets[k].dead_until.is_some() {
            return;
        }
        let t = self.frame;
        let p = self.w.position(node);
        let col = self.soul_color(node);
        let shooter = self.players.iter().find(|p| self.w.nodes[p.node].tag == "PlayerOneScreen1").map(|p| p.node);
        // the same beats as a kill, lighter: squash, ring, darts home to you
        let a = if hit.length() > 1e-3 { hit.y.atan2(hit.x) } else { 0.0 };
        self.motion.push(Kind::Blob { c: p, a, r: 0.22 }, false, 0.0, 0.14, col);
        self.motion.ring(false, p, 0.3, 1.8, 0.05, 0.4, col);
        self.motion.burst(false, p, 12, 9.0, 0.05, 1.1, col, shooter);
        self.motion.add_shake(0.08);
        self.motion.hitstop = 1;
        self.motion.last_action = self.motion.now;
        self.practice.streak = if t - self.practice.last_hit < 150 { self.practice.streak + 1 } else { 1 };
        self.practice.last_hit = t;
        self.practice.hits += 1;
        self.cue("fling", p, 0.6);
        let n = self.practice.streak.min(8) as f32;
        self.cue("home", p, n);
        let g = &mut self.practice.targets[k];
        g.dead_until = Some(t + super::ticks(BACK_IN) as u64);
        g.noticed.clear();
        g.dodge_until = 0;
        // gone until it condenses back in: out of physics and out of sight
        if let Some(body) = self.w.nodes[node].body.as_mut() {
            body.velocity = V3::ZERO;
        }
        self.w.set_active(node, false);
        self.phys.forget(node);
    }
}
