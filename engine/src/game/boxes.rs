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
}

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
            // LineRendererManager.FixedUpdate
            let bx = &mut self.boxes[b];
            if bx.raving {
                match bx.rave_timer % 25 {
                    0 => {
                        self.tasks.push(Task::RaveLine { b, index: 0, j: 0, a: 0.0, r1: 0, r2: 0 });
                        self.tasks.push(Task::RaveLine { b, index: 1, j: 0, a: 0.0, r1: 0, r2: 0 });
                    }
                    15 => {
                        self.tasks.push(Task::RaveLine { b, index: 2, j: 0, a: 0.0, r1: 0, r2: 0 });
                        self.tasks.push(Task::RaveLine { b, index: 3, j: 0, a: 0.0, r1: 0, r2: 0 });
                    }
                    20 => self.tasks.push(Task::RaveLine { b, index: 4, j: 0, a: 0.0, r1: 0, r2: 0 }),
                    _ => {}
                }
            }
            self.boxes[b].rave_timer += 1;
        }
    }

    pub(super) fn is_being_thrown(&self, b: usize) -> bool {
        let x = &self.boxes[b];
        x.prev_vel.x.abs() > 10.0 || x.prev_vel.y.abs() > 10.0 || x.being_thrown
    }

    /// BoxState.OnCollisionEnter
    pub(super) fn box_collision(&mut self, b: usize, other: usize, normal: V3) {
        if std::env::var("NB_TRACE").is_ok() {
            eprintln!("box_collision {} vs {} (layer {}) thrown={} prev={:?} carried={:?}", self.w.nodes[self.boxes[b].node].name, self.w.nodes[other].name, self.w.nodes[other].layer, self.is_being_thrown(b), self.boxes[b].prev_vel, self.boxes[b].carried_by);
        }
        if !self.is_being_thrown(b) {
            return;
        }
        let n = self.boxes[b].node;
        let pos = self.w.position(n);
        let clip = self.menu.explosion.clone();
        self.play(&clip, 5.0 / 5.0);
        if self.w.nodes[other].layer != crate::physics::LAYER_PLAYER {
            self.fx_impact(pos, normal);
        }
        let prev = self.boxes[b].prev_vel;
        let other_tag = self.w.nodes[other].tag.clone();
        let other_layer = self.w.nodes[other].layer;
        if other_tag == "Wall" {
            self.boxes[b].reflected = reflect(prev, normal);
        } else if other_layer == crate::physics::LAYER_BOX {
            self.boxes[b].reflected = reflect(prev, normal);
            if let Some(&ob) = self.box_of.get(&other) {
                self.box_single_explode(ob);
            }
        } else if other_layer == crate::physics::LAYER_PLAYER {
            self.boxes[b].reflected = prev;
            if let Some(&k) = self.player_of.get(&other) {
                self.drop_box(k);
            }
            let killer = self.boxes[b].carried_by;
            self.death(other, killer);
        }
        self.box_single_explode(b);
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
        for k in 0..self.boxes[b].debris.len() {
            let d = self.boxes[b].debris[k];
            let p = pos + prev * (1.0 / 200.0);
            self.w.set_position(d, p);
            // AddForce(reflected / 80, Impulse) on a 0.1 kg body
            let mass = self.w.nodes[d].body.as_ref().map(|x| x.mass).unwrap_or(0.1).max(1e-3);
            let spin = self.rng.range(-400, 400) as f32;
            if let Some(body) = self.w.nodes[d].body.as_mut() {
                body.velocity = body.velocity + refl * (1.0 / 80.0 / mass);
                body.spin = spin;
            }
        }
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
