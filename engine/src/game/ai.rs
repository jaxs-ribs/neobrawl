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
}

const MASK: u32 = 1 | (1 << 8) | (1 << crate::physics::LAYER_BOX);

impl Game {
    fn clear_path(&self, from: V3, to: V3, half: f32) -> bool {
        let d = to - from;
        let l = (d.x * d.x + d.y * d.y).sqrt();
        if l < 1e-3 {
            return true;
        }
        let dir = v3(d.x / l, d.y / l, 0.0);
        let side = v3(-dir.y, dir.x, 0.0);
        [-half, 0.0, half].iter().all(|&o| self.phys.raycast(&self.w, from + side * o, dir, l, MASK).is_none())
    }

    /// Run the wanderers before the tick consumes input.
    pub(super) fn ai_step(&mut self) {
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
                    if !self.clear_path(me, p, 0.32) {
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
                    if self.clear_path(me, me + d * 1.1, 0.28) {
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
                && self.clear_path(me, me + dir * 3.2, 0.3);
            let mv = if w.rest > 0 { V2::ZERO } else { V2 { x: dir.x, y: dir.y } };
            self.input.direct[slot] = Direct { active: true, mv, aim: V2::ZERO, aiming: false, use_cursor: false, dash };
            self.ai[slot] = w;
        }
    }
}
