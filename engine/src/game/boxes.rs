//! BoxState (+ its debris and LineRendererManager).

use crate::shared::math::{v3, Quat, V3};

use super::{fbool, has_script, ticks, Game, Task};
use crate::scene::FREEZE_ALL;

#[derive(Clone, Debug, Default)]
pub struct BoxS {
    pub node: usize,
    pub being_thrown: bool,
    pub being_thrown_pickup: bool,
    pub prev_vel: V3,
    pub cur_vel: V3,
    pub initial_pos: V3,
    pub initial_rot: Quat,
    pub debris_parent: Option<usize>,
    pub debris: Vec<usize>,
    pub debris_pos: Vec<V3>,
    pub debris_rot: Vec<Quat>,
    pub lines: Vec<usize>,
    pub raving: bool,
    pub rave_timer: i32,
    pub carried_by: Option<usize>,
    pub ended_triggered: bool,
    pub menu: bool,
    pub intro: bool,
    pub reflected: V3,
    pub started: bool,
    /// wall bounces since the throw; after BOUNCES_LIVE it's harmless
    pub bounces: u8,
    pub spent: bool,
    /// charge it was thrown with (tuning: jab vs loaded kills)
    pub thrown_charge: f32,
}

/// A wall hit shatters the box only if it comes in nearly square
/// (|cos| to the wall normal above this) and fast into the wall.
pub const SHATTER_SQUARE: f32 = 0.85;
pub const SHATTER_NORMAL_SPEED: f32 = 24.0;
/// Speed kept per bounce, and bounces before a box goes harmless.
pub const BOUNCE_KEEP: f32 = 0.7;
pub const BOUNCES_LIVE: u8 = 3;

impl Game {
    pub(super) fn collect_boxes(&mut self) {
        for i in 0..self.w.nodes.len() {
            if !has_script(&self.w, i, "BoxState") {
                continue;
            }
            let w = &self.w;
            let debris_parent = w.nodes[i].children.get(1).copied();
            let debris: Vec<usize> = debris_parent.map(|d| w.nodes[d].children.clone()).unwrap_or_default();
            let lines = w.nodes[i].children.get(2).map(|l| w.nodes[*l].children.clone()).unwrap_or_default();
            let b = BoxS {
                node: i,
                debris_parent,
                debris_pos: debris.iter().map(|&d| w.nodes[d].lpos).collect(),
                debris_rot: debris.iter().map(|&d| w.rotation(d)).collect(),
                debris,
                lines,
                menu: fbool(w, i, "BoxState", "menu"),
                intro: fbool(w, i, "BoxState", "intro"),
                ..Default::default()
            };
            self.box_of.insert(i, self.boxes.len());
            self.boxes.push(b);
        }
    }

    pub(super) fn box_start(&mut self, b: usize) {
        if self.boxes[b].started {
            return;
        }
        self.boxes[b].started = true;
        let n = self.boxes[b].node;
        self.boxes[b].initial_pos = self.w.position(n);
        self.boxes[b].initial_rot = self.w.rotation(n);
    }

    pub(super) fn boxes_fixed(&mut self) {
        let ended = self.scores.iter().any(|s| s.ended && self.w.active_in_hierarchy(s.node));
        let paused_end = self.pauses.iter().any(|p| p.ended && self.w.active_in_hierarchy(p.node));
        for b in 0..self.boxes.len() {
            let n = self.boxes[b].node;
            if !self.w.active_in_hierarchy(n) {
                continue;
            }
            self.box_start(b);
            let v = self.w.nodes[n].body.as_ref().map(|x| x.velocity).unwrap_or(V3::ZERO);
            let bx = &mut self.boxes[b];
            bx.prev_vel = bx.cur_vel;
            bx.cur_vel = v;
            // ScoreManager / PauseMenu end-of-round respawns (only boxes inside a match)
            let in_match = self.scores.iter().any(|s| self.w.active_in_hierarchy(s.node)) && !bx.menu;
            if in_match {
                if ended {
                    self.tasks.push(Task::BoxRespawn { b, wait: ticks(0.525) });
                    self.boxes[b].ended_triggered = true;
                } else if paused_end {
                    self.tasks.push(Task::BoxRespawn { b, wait: 0 });
                }
            }
            self.boxes[b].rave_timer += 1;
        }
    }

    pub(super) fn is_being_thrown(&self, b: usize) -> bool {
        let x = &self.boxes[b];
        if x.spent {
            return false;
        }
        x.prev_vel.x.abs() > 10.0 || x.prev_vel.y.abs() > 10.0 || x.being_thrown
    }

    /// BoxState.OnCollisionEnter
    pub(super) fn box_collision(&mut self, b: usize, other: usize, normal: V3) {
        if std::env::var("NB_TRACE").is_ok() {
            eprintln!("box_collision {} vs {} (layer {}) thrown={} prev={:?} carried={:?} at {:?} other at {:?} frame {}", self.w.nodes[self.boxes[b].node].name, self.w.nodes[other].name, self.w.nodes[other].layer, self.is_being_thrown(b), self.boxes[b].prev_vel, self.boxes[b].carried_by, self.w.position(self.boxes[b].node), self.w.position(other), self.frame);
        }
        if !self.is_being_thrown(b) {
            return;
        }
        let n = self.boxes[b].node;
        let pos = self.w.position(n);
        let other_layer = self.w.nodes[other].layer;
        if other_layer != crate::physics::LAYER_BOX && other_layer != crate::physics::LAYER_PLAYER && !self.boxes[b].intro && self.box_bounce(b, other, pos, normal) {
            return;
        }
        // the break: a synthesised glass shatter (the original was one long
        // explosion sample, the same every time); how hard and what it hit
        // shape it - amount = speed 0..1 + 10 x material (0 wall, 1 box, 2 soul)
        let hit_speed = (self.boxes[b].prev_vel.length() / 80.0).clamp(0.0, 0.99);
        let material = match self.w.nodes[other].layer {
            crate::physics::LAYER_BOX => 1.0,
            crate::physics::LAYER_PLAYER => 2.0,
            _ => 0.0,
        };
        self.cue("break", pos, hit_speed + 10.0 * material);
        if self.w.nodes[other].layer != crate::physics::LAYER_PLAYER {
            self.fx_impact(pos, normal);
            self.fx_flex(other, pos, normal);
        }
        let prev = self.boxes[b].prev_vel;
        if let Some(th) = self.boxes[b].carried_by {
            let e = self.throw_ends.entry(th).or_insert([0; 3]);
            match self.w.nodes[other].layer {
                crate::physics::LAYER_PLAYER => e[0] += 1,
                crate::physics::LAYER_BOX => e[1] += 1,
                _ => e[2] += 1,
            }
        }
        let other_tag = self.w.nodes[other].tag.clone();
        if &*other_tag == "Wall" {
            self.boxes[b].reflected = reflect(prev, normal);
        } else if other_layer == crate::physics::LAYER_BOX {
            self.boxes[b].reflected = reflect(prev, normal);
            if let Some(&ob) = self.box_of.get(&other) {
                self.box_single_explode(ob);
            }
        } else if other_layer == crate::physics::LAYER_PLAYER && self.practice_target(other).is_some() {
            self.boxes[b].reflected = prev;
            self.practice_hit(other, prev);
        } else if other_layer == crate::physics::LAYER_PLAYER {
            self.boxes[b].reflected = prev;
            if let Some(&k) = self.player_of.get(&other) {
                self.drop_box(k);
            }
            let killer = self.boxes[b].carried_by;
            self.kill_log.push((self.boxes[b].thrown_charge, self.boxes[b].bounces));
            self.kill_impulse = prev;
            self.death(other, killer);
        }
        self.box_single_explode(b);
    }

    /// A thrown box meets a wall: glancing (or soft) hits bounce off with a
    /// spark and lose speed, and a few bounces leave it harmless; only a
    /// square, fast hit shatters. True if it bounced.
    fn box_bounce(&mut self, b: usize, other: usize, pos: V3, normal: V3) -> bool {
        let v = self.boxes[b].prev_vel;
        let sp = (v.x * v.x + v.y * v.y).sqrt();
        let nl = (normal.x * normal.x + normal.y * normal.y).sqrt();
        if sp < 1e-3 || nl < 1e-3 {
            return false;
        }
        let nn = v3(normal.x / nl, normal.y / nl, 0.0);
        let into = (v.x * nn.x + v.y * nn.y).abs();
        if into / sp > SHATTER_SQUARE && into > SHATTER_NORMAL_SPEED {
            return false;
        }
        let out = reflect(v3(v.x, v.y, 0.0), nn) * BOUNCE_KEEP;
        let n = self.boxes[b].node;
        if let Some(body) = self.w.nodes[n].body.as_mut() {
            body.velocity = out;
        }
        let x = &mut self.boxes[b];
        x.bounces += 1;
        x.prev_vel = out;
        if x.bounces >= BOUNCES_LIVE || out.length() < 12.0 {
            x.spent = true;
            x.being_thrown = false;
        }
        self.fx_bounce(pos, out, (sp / 80.0).clamp(0.2, 1.0));
        self.fx_flex(other, pos, normal);
        true
    }

    pub(super) fn box_single_explode(&mut self, b: usize) {
        if !self.boxes[b].intro {
            self.tasks.push(Task::BoxRespawn { b, wait: ticks(10.0) });
        }
        self.shatter(b);
    }

    pub(super) fn box_explode(&mut self, b: usize) {
        self.tasks.push(Task::BoxRespawn { b, wait: ticks(10.0) });
        self.shatter(b);
    }

    fn shatter(&mut self, b: usize) {
        self.boxes[b].raving = true;
        self.switch_to_debris(b, true);
        let n = self.boxes[b].node;
        let pos = self.w.position(n);
        let prev = self.boxes[b].prev_vel;
        let refl = self.boxes[b].reflected;
        // Shard Lab: 13 ink shards replace the mesh debris and its rave lines
        if let Some(dp) = self.boxes[b].debris_parent {
            self.w.set_active(dp, false);
        }
        let speed = (prev.x * prev.x + prev.y * prev.y).sqrt();
        self.shards.shatter(pos.x, pos.y, pos.z, [refl.x, refl.y], speed, 14.0);
    }

    fn switch_to_debris(&mut self, b: usize, on: bool) {
        let n = self.boxes[b].node;
        if let Some(s) = self.w.nodes[n].sprite.as_mut() {
            s.enabled = !on;
        }
        if let Some(&shadow) = self.w.nodes[n].children.first() {
            if let Some(s) = self.w.nodes[shadow].sprite.as_mut() {
                s.enabled = !on;
            }
        }
        if let Some(dp) = self.boxes[b].debris_parent {
            self.w.set_active(dp, on);
        }
        if let Some(c) = self.w.nodes[n].collider.as_mut() {
            c.enabled = !on;
        }
        if let Some(body) = self.w.nodes[n].body.as_mut() {
            body.constraints = if on { FREEZE_ALL } else { 56 };
            if on {
                body.velocity = V3::ZERO;
            }
        }
    }

    pub(super) fn box_respawn(&mut self, b: usize) {
        let n = self.boxes[b].node;
        let (p, r) = (self.boxes[b].initial_pos, self.boxes[b].initial_rot);
        self.w.set_position(n, p);
        self.w.set_rotation(n, r);
        if let Some(body) = self.w.nodes[n].body.as_mut() {
            body.velocity = V3::ZERO;
        }
        self.switch_to_debris(b, false);
        self.fx_box_back(n);
        for k in 0..self.boxes[b].debris.len() {
            let d = self.boxes[b].debris[k];
            self.w.nodes[d].lpos = self.boxes[b].debris_pos[k];
            let rot = self.boxes[b].debris_rot[k];
            self.w.set_rotation(d, rot);
            if let Some(body) = self.w.nodes[d].body.as_mut() {
                body.velocity = V3::ZERO;
                body.spin = 0.0;
            }
        }
        self.boxes[b].raving = false;
        self.boxes[b].ended_triggered = false;
        self.phys.forget(n);
    }

    /// LineRendererManager.fadeInFromSourceToDebris, one step. false = done.
    pub(super) fn rave_line_step(&mut self, b: usize, index: usize, j: &mut u32, a: &mut f32, r1: &mut usize, r2: &mut usize) -> bool {
        let debris = self.boxes[b].debris.clone();
        if debris.len() < 13 || index >= self.boxes[b].lines.len() {
            return false;
        }
        if *j == 0 {
            *r1 = self.rng.range(1, 13) as usize;
            *r2 = self.rng.range(1, 13) as usize;
        }
        let limit = if *j < 10 { 2.0 } else { 5.0 };
        let mut guard = 0;
        while (self.w.position(debris[*r1]) - self.w.position(debris[*r2])).length() > limit && guard < 200 {
            *r1 = self.rng.range(1, 13) as usize;
            *r2 = self.rng.range(1, 13) as usize;
            guard += 1;
        }
        *a += if *j < 10 { 0.1 } else { -0.1 };
        let (p1, p2) = (self.w.position(debris[*r1]), self.w.position(debris[*r2]));
        let line = self.boxes[b].lines[index];
        if let Some(l) = self.w.nodes[line].line.as_mut() {
            l.colors = [[0.0, 0.0, 0.0, *a], [0.0, 0.0, 0.0, *a]];
            l.points = vec![p1, p2];
            l.world = true;
        }
        *j += 1;
        *j < 20
    }
}

fn reflect(v: V3, n: V3) -> V3 {
    let d = v.x * n.x + v.y * n.y + v.z * n.z;
    v - n * (2.0 * d)
}

#[allow(dead_code)]
fn _unused() -> V3 {
    v3(0.0, 0.0, 0.0)
}
