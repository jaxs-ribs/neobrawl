//! Bodies: the souls (and what they carry) as soft, living things.
//! Render-only springs, stepped on the tick, that never touch physics:
//! stretch along velocity and squash on stops, follow-through on the
//! offset, breath at rest, tension while charging, anticipation and
//! recoil around throws and dashes, and a carried box that lags and swings.

use crate::motion::{hash, hf};
use crate::shared::math::{v3, Mat4, V3};

use super::{Game, DT};

#[derive(Clone, Debug, Default)]
pub struct SoulBody {
    /// stretch along `axis` (+) or squash (-), and its velocity
    pub st: f32,
    st_v: f32,
    /// direction of the stretch (radians)
    pub axis: f32,
    /// uniform scale offset (breath, tension, landings)
    pub sq: f32,
    sq_v: f32,
    /// render offset (follow-through, anticipation, recoil, idle shifts)
    pub off: V3,
    off_v: V3,
    idle_target: V3,
    still: f32,
    prev_vel: V3,
    /// carried box lag and swing
    pub lag: V3,
    lag_v: V3,
    pub swing: f32,
    swing_v: f32,
    pub tremble: f32,
}

/// Semi-implicit damped spring toward 0 (k stiffness, z damping ratio).
fn spring(x: &mut f32, v: &mut f32, target: f32, k: f32, z: f32) {
    let c = 2.0 * k.sqrt() * z;
    *v += (-(*x - target) * k - *v * c) * DT;
    *x += *v * DT;
}

fn spring3(x: &mut V3, v: &mut V3, target: V3, k: f32, z: f32) {
    let c = 2.0 * k.sqrt() * z;
    *v = *v + ((target - *x) * k - *v * c) * DT;
    *x = *x + *v * DT;
}

impl Game {
    pub(super) fn bodies_step(&mut self) {
        if self.bodies.len() != self.players.len() {
            self.bodies = vec![SoulBody::default(); self.players.len()];
        }
        let t = self.frame as f32 * DT;
        for k in 0..self.players.len() {
            let node = self.players[k].node;
            let pos = self.w.position(node);
            let alive = pos.x != -666.0 && self.w.active_in_hierarchy(node);
            let vel = match (self.prev_pos.get(&node), alive) {
                (Some(&p), true) if p.x != -666.0 => (pos - p) * (1.0 / DT),
                _ => V3::ZERO,
            };
            let charge = (self.players[k].charge_timer / 90.0).clamp(0.0, 1.0);
            let aim = self.players[k].touch[1];
            let carrying = self.players[k].picked_box.is_some();
            let b = &mut self.bodies[k];
            if !alive {
                *b = SoulBody { axis: b.axis, ..Default::default() };
                continue;
            }
            let sp = (vel.x * vel.x + vel.y * vel.y).sqrt();
            let accel = (vel - b.prev_vel) * (1.0 / DT);
            let prev_sp = (b.prev_vel.x * b.prev_vel.x + b.prev_vel.y * b.prev_vel.y).sqrt();
            // a sudden stop (braking, a wall): squash along the old heading
            if prev_sp - sp > 4.0 {
                b.st_v -= (prev_sp - sp) * 0.9;
                b.axis = b.prev_vel.y.atan2(b.prev_vel.x);
            } else if sp > 0.6 {
                // turn the stretch toward the heading, easing through the angle
                let want = vel.y.atan2(vel.x);
                let mut d = want - b.axis;
                d = d.sin().atan2(d.cos());
                b.axis += d * 0.35;
            }
            spring(&mut b.st, &mut b.st_v, (sp * 0.018).min(0.34), 260.0, 0.32);
            // breath at rest, tension while charging (compressed, trembling)
            let breath = 0.022 * (t * 2.3 + k as f32 * 1.7).sin() * (1.0 - (sp / 3.0).min(1.0));
            spring(&mut b.sq, &mut b.sq_v, breath - 0.12 * charge, 180.0, 0.4);
            b.tremble = if charge >= 1.0 { 1.0 } else { charge * charge * 0.5 };
            // follow-through: the body keeps going a little when you change pace
            b.off_v = b.off_v - accel * (DT * 0.012);
            // anticipation: drawing back against the aim while charging
            let back = if charge > 0.0 && (aim.x != 0.0 || aim.y != 0.0) { v3(-aim.x, -aim.y, 0.0) * (0.09 * charge) } else { V3::ZERO };
            // idle micro-shifts: a soul waiting, not a sprite
            b.still = if sp < 0.3 { b.still + DT } else { 0.0 };
            if b.still > 1.2 && hash((t * 50.0) as u32 ^ (k as u32 * 977)) % 90 == 0 {
                let h = hash((t * 50.0) as u32 + k as u32);
                b.idle_target = v3(hf(h), hf(h ^ 7), 0.0) * 0.035;
            }
            if b.still < 0.5 {
                b.idle_target = V3::ZERO;
            }
            spring3(&mut b.off, &mut b.off_v, back + b.idle_target, 220.0, 0.45);
            // the carried box lags behind and swings with your moves
            if carrying {
                b.lag_v = b.lag_v - accel * (DT * 0.03);
                spring3(&mut b.lag, &mut b.lag_v, V3::ZERO, 110.0, 0.35);
                b.swing_v += (accel.x * vel.y - accel.y * vel.x).clamp(-4000.0, 4000.0) * 0.00004 + (-accel.x) * 0.0004;
                spring(&mut b.swing, &mut b.swing_v, 0.0, 90.0, 0.3);
            } else {
                b.lag = V3::ZERO;
                b.lag_v = V3::ZERO;
                b.swing = 0.0;
                b.swing_v = 0.0;
            }
            b.prev_vel = vel;
        }
    }

    /// A dash: squash back for an instant, then shoot long.
    pub(super) fn body_dash(&mut self, k: usize, dir: super::V2) {
        if let Some(b) = self.bodies.get_mut(k) {
            b.axis = dir.y.atan2(dir.x);
            b.st = -0.25;
            b.st_v = 22.0;
        }
    }

    /// A throw: the soul recoils from the release and squashes.
    pub(super) fn body_throw(&mut self, k: usize, dir: super::V2, power: f32) {
        if let Some(b) = self.bodies.get_mut(k) {
            let f = (power / 100.0).clamp(0.2, 1.0);
            b.off_v = b.off_v - v3(dir.x, dir.y, 0.0) * (2.5 * f);
            b.axis = dir.y.atan2(dir.x);
            b.st_v -= 9.0 * f;
            b.sq_v += 3.0 * f;
        }
    }

    /// Landing (respawn, pickup): a squash that settles.
    pub(super) fn body_land(&mut self, k: usize, amount: f32) {
        if let Some(b) = self.bodies.get_mut(k) {
            b.sq_v -= 5.0 * amount;
            b.st_v -= 4.0 * amount;
        }
    }

    /// Render-time transforms: node -> matrix to premultiply. Souls deform
    /// (stretch/squash about their centre, offset, tremble); carried boxes
    /// lag and swing.
    pub fn body_deforms(&self) -> std::collections::HashMap<usize, Mat4> {
        let mut out = std::collections::HashMap::new();
        let flick = (self.motion.render_now * 60.0) as u32;
        for (k, b) in self.bodies.iter().enumerate() {
            let Some(p) = self.players.get(k) else { continue };
            let c = self.w.position(p.node);
            if c.x == -666.0 {
                continue;
            }
            let s_along = (1.0 + b.st).max(0.45);
            let s_across = (1.0 / s_along.sqrt()).max(0.6);
            let u = 1.0 + b.sq;
            let tr = if b.tremble > 0.0 { v3(hf(flick ^ (k as u32 * 31)), hf(flick ^ (k as u32 * 37 + 5)), 0.0) * (0.012 * b.tremble) } else { V3::ZERO };
            let m = affine_about(c, b.axis, s_along * u, s_across * u, b.off + tr, 0.0);
            // the soul's own drawing (and its shadow), not the aim line
            for &ch in &self.w.nodes[p.node].children {
                if Some(ch) == p.aimer_anchor {
                    continue;
                }
                let mut sub = Vec::new();
                self.w.descendants(ch, &mut sub);
                for n in sub {
                    out.insert(n, m);
                }
            }
            if let Some(bx) = p.picked_box {
                let bm = affine_about(self.w.position(bx), 0.0, 1.0, 1.0, b.lag, b.swing);
                let mut sub = Vec::new();
                self.w.descendants(bx, &mut sub);
                for n in sub {
                    out.insert(n, bm);
                }
            }
        }
        out
    }
}

/// Scale (sa along angle a, sc across), rotate by `spin`, about centre c,
/// then translate by `off` - as a column-major Mat4 acting in the XY plane.
fn affine_about(c: V3, a: f32, sa: f32, sc: f32, off: V3, spin: f32) -> Mat4 {
    let (s, co) = a.sin_cos();
    // R(a) S R(-a)
    let m00 = co * co * sa + s * s * sc;
    let m01 = co * s * (sa - sc);
    let m11 = s * s * sa + co * co * sc;
    let (ss, cs) = spin.sin_cos();
    // then spin: Rspin * L
    let l = [[m00, m01], [m01, m11]];
    let r = [[cs * l[0][0] - ss * l[1][0], cs * l[0][1] - ss * l[1][1]], [ss * l[0][0] + cs * l[1][0], ss * l[0][1] + cs * l[1][1]]];
    // x' = c + off + r (x - c)
    let tx = c.x + off.x - (r[0][0] * c.x + r[0][1] * c.y);
    let ty = c.y + off.y - (r[1][0] * c.x + r[1][1] * c.y);
    Mat4([r[0][0], r[1][0], 0.0, 0.0, r[0][1], r[1][1], 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, tx, ty, 0.0, 1.0])
}
