//! Netcode building blocks: the per-tick input that crosses the wire, and a
//! checksum of the gameplay state (rollback compares these to catch desyncs).

use serde::{Deserialize, Serialize};

use crate::game::{Direct, Game, V2};

/// One player's input for one tick: ~5 bytes. Movement and aim are unit-ish
/// directions quantised to i8 (aim is a world-space direction, never screen
/// pixels, so window sizes can't change a throw).
#[derive(Copy, Clone, PartialEq, Eq, Default, Debug, Serialize, Deserialize)]
pub struct NetInput {
    pub mv: [i8; 2],
    /// aim as an angle, 65536 steps a turn (~0.0055 degrees)
    pub aim: u16,
    /// bit 0 charging, bit 1 dash held, bit 2 present, bit 3 aiming somewhere
    pub buttons: u8,
}

fn q(v: f32) -> i8 {
    (v.clamp(-1.0, 1.0) * 127.0).round() as i8
}

fn dq(v: i8) -> f32 {
    v as f32 / 127.0
}

impl NetInput {
    pub fn new(mv: V2, aim: V2, charging: bool, dash: bool) -> NetInput {
        let has_aim = aim.x != 0.0 || aim.y != 0.0;
        let turn = if has_aim { (aim.y.atan2(aim.x) / std::f32::consts::TAU).rem_euclid(1.0) } else { 0.0 };
        let aim = (turn * 65536.0).round() as u32 as u16;
        NetInput { mv: [q(mv.x), q(mv.y)], aim, buttons: charging as u8 | (dash as u8) << 1 | 1 << 2 | (has_aim as u8) << 3 }
    }

    pub fn to_direct(self) -> Direct {
        Direct {
            active: self.buttons & 4 != 0,
            mv: V2 { x: dq(self.mv[0]), y: dq(self.mv[1]) },
            aim: if self.buttons & 8 != 0 {
                let a = self.aim as f32 / 65536.0 * std::f32::consts::TAU;
                V2 { x: a.cos(), y: a.sin() }
            } else {
                V2::ZERO
            },
            aiming: self.buttons & 1 != 0,
            use_cursor: false,
            dash: self.buttons & 2 != 0,
        }
    }
}

struct Fnv128(u128);

impl Fnv128 {
    fn new() -> Self {
        Fnv128(0x6c62272e07bb014262b821756295c58d)
    }
    fn u32(&mut self, v: u32) {
        for b in v.to_le_bytes() {
            self.0 ^= b as u128;
            self.0 = self.0.wrapping_mul(0x0000000001000000000000000000013B);
        }
    }
    fn f(&mut self, v: f32) {
        self.u32(v.to_bits());
    }
    fn u64(&mut self, v: u64) {
        self.u32(v as u32);
        self.u32((v >> 32) as u32);
    }
}

impl Game {
    /// Feed one tick of network input to the two direct-control slots.
    pub fn apply_net_inputs(&mut self, inputs: &[NetInput]) {
        self.input.desktop = true;
        for (slot, i) in inputs.iter().enumerate().take(2) {
            self.input.direct[slot] = i.to_direct();
        }
    }

    /// A hash of everything gameplay depends on (not presentation): every
    /// node's transform and active flag, bodies, players, boxes, the rng,
    /// tasks and the debris. Two peers on the same tick must agree exactly.
    pub fn checksum(&self) -> u128 {
        let mut h = Fnv128::new();
        h.u64(self.frame);
        h.u64(self.rng.0);
        // gameplay things only: whatever has a body or a collider (the
        // camera and UI follow each machine's own screen)
        for n in self.w.nodes.iter().filter(|n| n.body.is_some() || n.collider.is_some()) {
            h.u32(n.active as u32);
            for v in [n.lpos.x, n.lpos.y, n.lpos.z, n.lrot.x, n.lrot.y, n.lrot.z, n.lrot.w, n.lscale.x, n.lscale.y] {
                h.f(v);
            }
            if let Some(b) = n.body.as_ref() {
                h.f(b.velocity.x);
                h.f(b.velocity.y);
                h.f(b.spin);
            }
            if let Some(c) = n.collider.as_ref() {
                h.u32(c.enabled as u32);
            }
        }
        for p in &self.players {
            for v in [p.charge_timer, p.pace, p.cmd.x, p.cmd.y, p.touch[0].x, p.touch[0].y, p.touch[1].x, p.touch[1].y, p.heading.x, p.heading.y] {
                h.f(v);
            }
            h.u32(p.dash_t);
            h.u32(p.dash_cd);
            h.u32(p.picked_box.map(|b| b as u32 + 1).unwrap_or(0));
        }
        for b in &self.boxes {
            h.u32(b.being_thrown as u32 | (b.raving as u32) << 1 | (b.being_thrown_pickup as u32) << 2);
            h.u32(b.carried_by.map(|c| c as u32 + 1).unwrap_or(0));
            h.f(b.prev_vel.x);
            h.f(b.prev_vel.y);
        }
        for t in &self.tasks {
            for c in format!("{t:?}").bytes() {
                h.u32(c as u32);
            }
        }
        for s in &self.shards.shards {
            h.f(s.x);
            h.f(s.y);
        }
        h.0
    }
}
