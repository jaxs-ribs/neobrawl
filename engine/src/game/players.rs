//! TouchManagerDouble/Single, PlayerMovement*, PlayerAim*, BoxPickupBehaviour,
//! BoxThrowBehaviour*, PlayerBlobPickupReaction and ObjectBurner.

use crate::shared::math::{v3, Quat, V3};

use super::{fbool, field, fref, has_script, ticks, v2, Game, Task, DT, V2};

#[derive(Clone, Debug, Default)]
pub struct Player {
    pub node: usize,
    pub single: bool,
    pub player1: bool,
    pub playback: bool,
    pub touch: [V2; 2],
    pub mag: [f32; 2],
    pub raw: [V2; 2],
    pub prev: [V2; 2],
    pub prev2: [V2; 2],
    pub joy_move: V2,
    pub joy_aim: V2,
    pub inner_move: Option<usize>,
    pub inner_aim: Option<usize>,
    pub outer_move: Option<usize>,
    pub outer_aim: Option<usize>,
    pub charge_bar: Option<usize>,
    pub charge_timer: f32,
    pub aimer_anchor: Option<usize>,
    pub aimer: Option<usize>,
    pub trigger_aim: i32,
    pub hide_anchor: bool,
    pub aim_mask: u32,
    pub last_hit: V3,
    /// eased (width, alpha) of the aim line, by what it is pointing at
    pub aim_form: [f32; 2],
    /// the aim sprite's authored (y scale, alpha)
    pub aim_base: Option<(f32, f32)>,
    pub collided_box: Option<usize>,
    pub picked_box: Option<usize>,
    pub pickup_tutorial: bool,
    pub throw_angle: V2,
    pub blob_scaling: bool,
    pub started: bool,
    pub trail: Vec<(V3, f32)>,
    /// smoothed movement command (desktop acceleration)
    pub cmd: V2,
    /// dash: ticks left, cooldown ticks left, direction, and last held state
    pub dash_t: u32,
    pub dash_cd: u32,
    pub dash_dir: V2,
    pub dash_held: bool,
    /// last non-zero movement direction (dash fallback)
    pub facing: V2,
    /// dash speed (a mid-dash steer bleeds some of it)
    pub dash_speed: f32,
    /// snake mode: current heading, the heading being turned to, the last
    /// input (turns happen on a new press), pace (crawl while charging),
    /// where we were a tick ago and how long we've been blocked
    pub heading: V2,
    pub steer: V2,
    pub last_mv: V2,
    pub pace: f32,
    pub last_pos: V3,
    pub blocked: u8,
    pub glance_side: bool,
}

/// Dash tuning: ~2.9 units in 0.12 s, then a 1 s cooldown.
pub const DASH_TICKS: u32 = 6;
pub const DASH_SPEED: f32 = 24.0;
pub const DASH_COOLDOWN: u32 = 50;
/// Charging costs speed by threat, not by time held. A jab - enough charge
/// to cover JAB_REACH units within a human reaction time - is free; past
/// it, pace eases down to CHARGE_FLOOR at full charge, in step with the
/// extra lethal range bought (which grows linearly with charge).
pub const CHARGE_FLOOR: f32 = 0.45;
pub const JAB_REACH: f32 = 4.0;
pub const REACTION_SECS: f32 = 0.25;
/// Charge fills this much faster until the jab is ready.
pub const JAB_FILL: f32 = 1.5;
pub const FULL_CHARGE: f32 = 90.0;

/// Throw speed for a charge (units/s), as `throw_fixed` gives it.
pub fn throw_speed(charge: f32) -> f32 {
    (charge * 1.11).max(20.0)
}

/// The charge at which a throw covers JAB_REACH within REACTION_SECS,
/// from the box's drag (velocity scales by 1 - drag*dt each tick).
pub fn jab_charge(drag: f32) -> f32 {
    let k = (1.0 - drag * DT).clamp(0.0, 0.999);
    let n = REACTION_SECS / DT;
    let per_speed = DT * k * (1.0 - k.powf(n)) / (1.0 - k);
    (JAB_REACH / per_speed / 1.11).clamp(0.0, FULL_CHARGE)
}

/// Movement pace while holding `charge` (1 = full speed).
pub fn charge_pace(charge: f32, jab: f32) -> f32 {
    if charge <= jab {
        return 1.0;
    }
    let e = ((charge - jab) / (FULL_CHARGE - jab).max(1.0)).clamp(0.0, 1.0);
    let s = e * e * (3.0 - 2.0 * e);
    1.0 - (1.0 - CHARGE_FLOOR) * s
}

#[derive(Clone, Debug, Default)]
pub struct Burner {
    pub node: usize,
    pub previous: Option<usize>,
    pub mask: u32,
    pub wall: Option<usize>,
    pub wall_glow: Option<usize>,
    pub box_: Option<usize>,
    pub box_glow: Option<usize>,
}

impl Game {
    pub(super) fn collect_players(&mut self) {
        for i in 0..self.w.nodes.len() {
            let double = has_script(&self.w, i, "TouchManagerDouble");
            let single = has_script(&self.w, i, "TouchManagerSingle");
            if !double && !single {
                continue;
            }
            let tm = if double { "TouchManagerDouble" } else { "TouchManagerSingle" };
            let aim = if double { "PlayerAimDouble" } else { "PlayerAimSingle" };
            let w = &self.w;
            let anchor = w.nodes[i].children.get(2).copied();
            let aimer = anchor.and_then(|a| {
                let mut d = Vec::new();
                w.descendants(a, &mut d);
                d.into_iter().find(|&n| w.nodes[n].sprite.is_some())
            });
            let mask = field(w, i, aim, "all").as_u64().unwrap_or(u32::MAX as u64) as u32;
            let p = Player {
                node: i,
                single,
                player1: single || fbool(w, i, tm, "player1"),
                playback: fbool(w, i, tm, "INSPECTOR_shouldReadPlayBack"),
                inner_move: fref(w, i, tm, "InnerMovement"),
                inner_aim: fref(w, i, tm, "InnerAim"),
                outer_move: fref(w, i, tm, "OuterMovement"),
                outer_aim: fref(w, i, tm, "OuterAim"),
                charge_bar: fref(w, i, tm, "chargeBar"),
                aimer_anchor: anchor,
                aimer,
                trigger_aim: field(w, i, aim, "triggerAim").as_i64().unwrap_or(-1) as i32,
                hide_anchor: fbool(w, i, aim, "INSPECTOR_shouldntShowAnchor"),
                aim_mask: mask,
                pickup_tutorial: fbool(w, i, "BoxPickupBehaviour", "tutorial"),
                ..Default::default()
            };
            self.player_of.insert(i, self.players.len());
            self.players.push(p);
        }
        for i in 0..self.w.nodes.len() {
            if has_script(&self.w, i, "ObjectBurner") {
                let w = &self.w;
                let b = Burner {
                    node: i,
                    previous: None,
                    mask: field(w, i, "ObjectBurner", "all").as_u64().unwrap_or(u32::MAX as u64) as u32,
                    wall: sprite_ref(w, i, "WallSprite"),
                    wall_glow: sprite_ref(w, i, "WallGlowSprite"),
                    box_: sprite_ref(w, i, "BoxSprite"),
                    box_glow: sprite_ref(w, i, "BoxGlowSprite"),
                };
                self.burners.push(b);
            }
        }
    }

    /// Start(): joystick anchors depend on the screen size at that moment.
    pub(super) fn player_start(&mut self, k: usize) {
        let (sw, sh) = (self.screen.w, self.screen.h);
        let p = &mut self.players[k];
        if p.started {
            return;
        }
        p.started = true;
        if p.single {
            p.joy_move = v2(0.18 * sw, 0.25 * sh);
            p.joy_aim = v2(0.82 * sw, 0.25 * sh);
        } else if p.player1 {
            p.joy_move = v2(0.18 * sw, 0.82 * sh);
            p.joy_aim = v2(0.18 * sw, 0.18 * sh);
        } else {
            p.joy_move = v2(0.82 * sw, 0.18 * sh);
            p.joy_aim = v2(0.82 * sw, 0.82 * sh);
        }
        let node = p.node;
        if !p.single {
            // PlayerMovementDouble.initializeVars
            self.w.set_euler(node, -90.0, 0.0, 0.0);
        }
        self.place_outer_joysticks(k);
    }

    fn place_outer_joysticks(&mut self, k: usize) {
        let p = &self.players[k];
        let (om, oa, jm, ja) = (p.outer_move, p.outer_aim, p.joy_move, p.joy_aim);
        if let Some(n) = om {
            self.ui_set_position(n, v3(jm.x, jm.y, 0.0));
        }
        if let Some(n) = oa {
            self.ui_set_position(n, v3(ja.x, ja.y, 0.0));
        }
    }

    pub(super) fn player_active(&self, k: usize) -> bool {
        self.w.active_in_hierarchy(self.players[k].node)
    }

    /// FixedUpdate in component order (BoxPickup, BoxThrow, Movement, TouchManager, Aim).
    pub(super) fn players_fixed(&mut self) {
        for k in 0..self.players.len() {
            if !self.player_active(k) {
                continue;
            }
            self.player_start(k);
            if self.players[k].single {
                self.touch_single_fixed(k);
                self.throw_fixed(k);
            } else {
                self.throw_fixed(k);
                self.movement_apply(k);
                self.touch_double_fixed(k);
            }
        }
    }

    pub(super) fn players_update(&mut self) {
        for k in 0..self.players.len() {
            if !self.player_active(k) {
                continue;
            }
            self.player_start(k);
            // BoxPickupBehaviour.Update
            self.carry_box(k);
            self.test_for_pickup(k);
            if self.players[k].single {
                // CameraFollowSingle: the menu camera sits 10 units behind the player
                if has_script(&self.w, self.players[k].node, "CameraFollowSingle") {
                    if let Some(cam) = self.w.with_tag("CameraMenu") {
                        let p = self.w.position(self.players[k].node);
                        self.w.set_position(cam, v3(p.x, p.y, p.z - 10.0));
                    }
                }
                self.aim_update(k);
                self.movement_apply(k);
                self.place_outer_joysticks(k);
            } else {
                self.place_outer_joysticks(k);
                self.aim_update(k);
            }
        }
        for b in 0..self.burners.len() {
            if self.w.active_in_hierarchy(self.burners[b].node) {
                self.burn_up(b);
            }
        }
    }

    fn touch_count(&self) -> usize {
        self.input.touches.len()
    }

    /// Which desktop controller drives player k, if any.
    pub(super) fn direct_slot(&self, k: usize) -> Option<usize> {
        let p = &self.players[k];
        if !self.input.desktop || p.playback {
            return None;
        }
        Some(if p.single || p.player1 { 0 } else { 1 })
    }

    pub fn direct_slot_pub(&self, p: &Player) -> Option<usize> {
        if !self.input.desktop || p.playback {
            return None;
        }
        Some(if p.single || p.player1 { 0 } else { 1 })
    }

    /// Desktop input: smoothed movement and mouse / stick aiming.
    fn direct_fixed(&mut self, k: usize, slot: usize) {
        if self.input.snake {
            self.snake_glance(k);
        }
        let ctl = self.input.direct[slot];
        let jab = self.jab();
        let p = &mut self.players[k];
        p.prev2 = p.prev;
        p.prev = p.touch;
        // a short ramp keeps it smooth but never floaty: quicker to brake
        // and to reverse than to get going
        let target = if ctl.active { ctl.mv } else { V2::ZERO };
        // holding a charge costs speed by threat: a jab is free, a loaded
        // throw slows you to 45%; letting go gives it all back at once
        p.pace = if ctl.aiming && p.picked_box.is_some() { charge_pace(p.charge_timer, jab) } else { 1.0 };
        let mut uturn = false;
        if self.input.snake {
            uturn = Self::snake_steer(p, target);
        } else {
            let braking = target.is_zero() || (target.x * p.cmd.x + target.y * p.cmd.y) < 0.0;
            let a = 1.0 - (-DT / if braking { 0.025 } else { 0.04 }).exp();
            p.cmd = v2(p.cmd.x + (target.x - p.cmd.x) * a, p.cmd.y + (target.y - p.cmd.y) * a);
            if p.cmd.len() < 0.01 && target.is_zero() {
                p.cmd = V2::ZERO;
            }
            p.touch[0] = p.cmd.normalized();
            p.mag[0] = p.cmd.len().min(1.0) * p.pace;
            if !target.is_zero() {
                p.facing = target.normalized();
            }
        }
        // dash on the press edge, if off cooldown and alive
        let pressed = ctl.active && ctl.dash && !p.dash_held;
        p.dash_held = ctl.active && ctl.dash;
        let mut ready = false;
        if p.dash_cd > 0 {
            p.dash_cd -= 1;
            ready = p.dash_cd == 0;
        }
        if p.dash_t > 0 {
            p.dash_t -= 1;
        }
        let mut dashed = false;
        if pressed && p.dash_cd == 0 {
            let dir = if self.input.snake {
                // along the heading (or the turn just asked for)
                if !p.steer.is_zero() { p.steer } else { p.heading }
            } else if !target.is_zero() {
                target.normalized()
            } else if !p.facing.is_zero() {
                p.facing
            } else {
                v2(1.0, 0.0)
            };
            p.dash_dir = dir;
            p.dash_speed = DASH_SPEED;
            // dashing out of a wind-up keeps the box but bleeds a third of
            // the charge (the dash itself goes the full distance)
            p.charge_timer *= 2.0 / 3.0;
            p.dash_t = DASH_TICKS;
            p.dash_cd = DASH_COOLDOWN;
            dashed = true;
        }
        p.raw = [V2::ZERO; 2];
        let node = p.node;
        let aim = if ctl.active && ctl.aiming {
            if ctl.use_cursor {
                self.cursor_world(k).map(|c| {
                    let me = self.w.position(node);
                    v2(c.x - me.x, c.y - me.y)
                })
            } else {
                Some(ctl.aim)
            }
        } else {
            None
        };
        if dashed && self.w.position(node).x != -666.0 {
            let clip = self.menu.dash_clip.clone();
            self.play(&clip, 0.8);
            let dir = self.players[k].dash_dir;
            self.fx_dash(k, dir);
        }
        if ready {
            self.fx_dash_ready(k);
        }
        if uturn {
            self.body_land(k, 0.6);
            let p = self.w.position(self.players[k].node);
            self.cue("turn", p, 1.0);
        }
        let p = &mut self.players[k];
        p.touch[1] = match aim {
            Some(d) if d.len() > 1e-3 => d.normalized(),
            Some(_) if !p.prev[1].is_zero() => p.prev[1],
            Some(_) => v2(1.0, 0.0),
            None => V2::ZERO,
        };
        p.mag[1] = if p.touch[1].is_zero() { 0.0 } else { 1.0 };
        self.charge(k);
    }

    /// Snake movement: always going; a new press sets a heading that is
    /// curved into fast, a head-on wall glances you along it, and a turn
    /// during a dash bends the dash (it bleeds speed the sharper it is).
    /// Pace (charge slowdown) is set by the caller. Returns true on a U-turn.
    fn snake_steer(p: &mut Player, mv: V2) -> bool {
        const TURN: f32 = 0.55; // rad per tick: ~90 degrees in 60 ms
        if p.heading.is_zero() {
            p.heading = if !p.facing.is_zero() { p.facing } else { v2(1.0, 0.0) };
            p.steer = p.heading;
        }
        let fresh = !mv.is_zero() && ((mv.x - p.last_mv.x).abs() + (mv.y - p.last_mv.y).abs()) > 0.01;
        p.last_mv = mv;
        let mut uturn = false;
        if fresh {
            let want = mv.normalized();
            let ang = p.heading.y.atan2(p.heading.x);
            let to = want.y.atan2(want.x);
            let mut d = to - ang;
            d = d.sin().atan2(d.cos());
            uturn = d.abs() > 2.6;
            p.steer = want;
            if p.dash_t > 0 {
                // bend the dash
                let mut dd = to - p.dash_dir.y.atan2(p.dash_dir.x);
                dd = dd.sin().atan2(dd.cos()).abs();
                p.dash_speed *= (1.0 - 0.3 * dd / std::f32::consts::FRAC_PI_2).clamp(0.35, 1.0);
                p.dash_dir = want;
                p.heading = want;
            }
        }
        // curve toward the steer
        let ang = p.heading.y.atan2(p.heading.x);
        let to = p.steer.y.atan2(p.steer.x);
        let mut d = to - ang;
        d = d.sin().atan2(d.cos());
        let na = ang + d.clamp(-TURN, TURN);
        p.heading = v2(na.cos(), na.sin());
        p.cmd = v2(p.heading.x * p.pace, p.heading.y * p.pace);
        p.touch[0] = p.heading;
        p.mag[0] = p.pace;
        p.facing = p.heading;
        uturn
    }

    /// Snake mode: if we barely moved last tick, glance off the obstacle:
    /// slide along it if we were sliding, else turn a quarter away.
    fn snake_glance(&mut self, k: usize) {
        let pos = self.w.position(self.players[k].node);
        let p = &mut self.players[k];
        let moved = pos - p.last_pos;
        p.last_pos = pos;
        // only once a turn has finished: turning away from a wall is not
        // being blocked by it
        let turning = (p.heading.x * p.steer.x + p.heading.y * p.steer.y) < 0.98;
        if pos.x == -666.0 || p.dash_t > 0 || p.heading.is_zero() || turning {
            p.blocked = 0;
            return;
        }
        let expected = 8.0 * p.pace * DT;
        let m = (moved.x * moved.x + moved.y * moved.y).sqrt();
        if m > 1.0 || m >= 0.35 * expected {
            p.blocked = 0;
            return;
        }
        p.blocked += 1;
        if p.blocked < 2 {
            return;
        }
        p.blocked = 0;
        let new = if m > 0.04 * expected {
            v2(moved.x / m, moved.y / m)
        } else {
            // head-on: always turn the same way, so a corner takes two
            // quarter turns and lets you out (alternating would ping-pong)
            let s = if p.glance_side { 1.0 } else { -1.0 };
            v2(-p.heading.y * s, p.heading.x * s)
        };
        p.steer = new;
        p.heading = new;
    }

    /// The point under the mouse cursor on the player's plane (z of the player).
    pub(crate) fn cursor_world(&self, k: usize) -> Option<V3> {
        let [sx, sy] = self.input.cursor?;
        let p = &self.players[k];
        let cam = if p.single { self.w.with_tag("CameraMenu") } else { self.camera_main() }?;
        let c = self.w.nodes[cam].camera.as_ref()?;
        let t = (c.fov.to_radians() / 2.0).tan();
        let aspect = self.screen.w / self.screen.h;
        let nx = sx / self.screen.w * 2.0 - 1.0;
        let ny = sy / self.screen.h * 2.0 - 1.0;
        let m = self.w.matrix(cam);
        let origin = m.point(V3::ZERO);
        let dir = m.point(v3(nx * t * aspect, ny * t, 1.0)) - origin;
        let plane = self.w.position(p.node).z;
        if dir.z.abs() < 1e-6 {
            return None;
        }
        let s = (plane - origin.z) / dir.z;
        Some(origin + dir * s)
    }

    fn movement_apply(&mut self, k: usize) {
        let p = &self.players[k];
        let node = p.node;
        let x = self.w.position(node).x;
        let direct = self.direct_slot(k).is_some();
        let go = if direct {
            x != -666.0
        } else if p.single {
            self.touch_count() > 0 && p.trigger_aim != 0 && x != -666.0
        } else {
            (self.touch_count() > 0 && p.trigger_aim != 0 && x != -666.0) || p.playback
        };
        let v = if go && p.dash_t > 0 {
            v3(p.dash_dir.x, p.dash_dir.y, 0.0) * if p.dash_speed > 0.0 { p.dash_speed } else { DASH_SPEED }
        } else if go {
            v3(p.touch[0].x * p.mag[0], p.touch[0].y * p.mag[0], 0.0) * 8.0
        } else {
            V3::ZERO
        };
        if let Some(b) = self.w.nodes[node].body.as_mut() {
            b.velocity = v;
        }
    }

    fn shift_history(p: &mut Player) {
        p.prev2 = p.prev;
        p.prev = p.touch;
    }

    fn touch_double_fixed(&mut self, k: usize) {
        if let Some(slot) = self.direct_slot(k) {
            self.hide_joysticks(k);
            return self.direct_fixed(k, slot);
        }
        Self::shift_history(&mut self.players[k]);
        if self.players[k].playback {
            // PlayerMovementRecorder playback: the recordings lived in the
            // developer's PlayerPrefs, so shipped builds read zeros.
            let p = &mut self.players[k];
            p.touch = [V2::ZERO; 2];
            p.mag = [0.0; 2];
        } else {
            Self::shift_history(&mut self.players[k]);
            if self.touch_count() > 0 {
                self.relative_touch_double(k);
            } else {
                self.tasks.push(Task::TouchReset { player: k, mv: true, aim: true, wait: 0 });
            }
            self.move_joysticks(k);
        }
        self.charge(k);
    }

    /// Desktop play has no on-screen joysticks.
    fn hide_joysticks(&mut self, k: usize) {
        let p = &self.players[k];
        for n in [p.inner_move, p.inner_aim, p.outer_move, p.outer_aim].into_iter().flatten() {
            if let Some(img) = self.w.nodes[n].image.as_mut() {
                img.enabled = false;
            }
        }
    }

    fn touch_single_fixed(&mut self, k: usize) {
        if let Some(slot) = self.direct_slot(k) {
            self.hide_joysticks(k);
            return self.direct_fixed(k, slot);
        }
        Self::shift_history(&mut self.players[k]);
        if self.touch_count() > 0 {
            let (sw, sh) = (self.screen.w, self.screen.h);
            let touches = self.input.touches.clone();
            for t in &touches {
                let pos = v2(t.x, t.y);
                let p = &mut self.players[k];
                if pos.x < sw / 3.0 {
                    p.touch[0] = v2(pos.x - p.joy_move.x, pos.y - p.joy_move.y);
                    Self::magnitude(p, 0, sw);
                    p.raw[0] = pos;
                    p.touch[0] = p.touch[0].normalized();
                } else {
                    p.touch[1] = v2(pos.x - p.joy_aim.x, pos.y - p.joy_aim.y);
                    Self::magnitude(p, 1, sw);
                    p.raw[1] = pos;
                    p.touch[1] = p.touch[1].normalized();
                }
                let _ = sh;
            }
            if touches.len() == 1 {
                let right = touches[0].x > sw / 2.0;
                self.tasks.push(Task::TouchReset { player: k, mv: right, aim: !right, wait: 0 });
            }
        } else {
            self.tasks.push(Task::TouchReset { player: k, mv: true, aim: true, wait: 0 });
        }
        self.move_joysticks(k);
        self.charge(k);
    }

    fn magnitude(p: &mut Player, i: usize, sw: f32) {
        let m = p.touch[i].len();
        p.mag[i] = m.min(sw / 20.0) / (sw / 20.0);
    }

    fn relative_touch_double(&mut self, k: usize) {
        let (sw, sh) = (self.screen.w, self.screen.h);
        let touches = self.input.touches.clone();
        for t in &touches {
            let pos = v2(t.x, t.y);
            let p = &mut self.players[k];
            if p.player1 {
                if pos.x < sw / 2.0 {
                    if pos.y > 2.0 * sh / 3.0 {
                        p.touch[0] = v2(pos.x - p.joy_move.x, pos.y - p.joy_move.y);
                        Self::magnitude(p, 0, sw);
                        p.raw[0] = pos;
                        p.touch[0] = p.touch[0].normalized();
                    } else {
                        p.touch[1] = v2(pos.x - p.joy_aim.x, pos.y - p.joy_aim.y);
                        Self::magnitude(p, 1, sw);
                        p.raw[1] = pos;
                        p.touch[1] = p.touch[1].normalized();
                    }
                }
            } else if pos.x > sw / 2.0 {
                if pos.y < sh / 3.0 {
                    p.touch[0] = v2(pos.x - p.joy_move.x, pos.y - p.joy_move.y);
                    Self::magnitude(p, 0, sw);
                    p.raw[0] = pos;
                    p.touch[0] = p.touch[0].normalized();
                } else {
                    p.touch[1] = v2(pos.x - p.joy_aim.x, pos.y - p.joy_aim.y);
                    Self::magnitude(p, 1, sw);
                    p.raw[1] = pos;
                    p.touch[1] = p.touch[1].normalized();
                }
            }
        }
        // resetTouch: quadrants (by halves) without a finger get cleared next step
        let (mut f1, mut f2, mut f3, mut f4) = (false, false, false, false);
        for t in &touches {
            if t.x < sw / 2.0 {
                if t.y > sh / 2.0 {
                    f1 = true;
                } else {
                    f2 = true;
                }
            } else if t.x > sw / 2.0 {
                if t.y < sh / 2.0 {
                    f3 = true;
                } else {
                    f4 = true;
                }
            }
        }
        let (mv, aim) = if self.players[k].player1 { (!f1, !f2) } else { (!f3, !f4) };
        self.tasks.push(Task::TouchReset { player: k, mv, aim, wait: 0 });
    }

    pub(super) fn touch_reset(&mut self, k: usize, mv: bool, aim: bool) {
        let p = &mut self.players[k];
        if mv {
            p.touch[0] = V2::ZERO;
            p.raw[0] = V2::ZERO;
        }
        if aim {
            p.touch[1] = V2::ZERO;
            p.raw[1] = V2::ZERO;
        }
    }

    fn move_joysticks(&mut self, k: usize) {
        let sw = self.screen.w;
        let p = self.players[k].clone();
        if let Some(n) = p.inner_move {
            let mut v = p.joy_move;
            if !p.raw[0].is_zero() {
                v = p.raw[0];
                let d = v2(v.x - p.joy_move.x, v.y - p.joy_move.y);
                if d.len() > sw / 20.0 {
                    let u = d.normalized();
                    v = v2(u.x * sw / 20.0 + p.joy_move.x, u.y * sw / 20.0 + p.joy_move.y);
                }
            }
            self.ui_set_position(n, v3(v.x, v.y, 0.0));
        }
        if let Some(n) = p.inner_aim {
            let mut v = p.joy_aim;
            if !p.raw[1].is_zero() {
                v = p.raw[1];
                let d = v2(v.x - p.joy_aim.x, v.y - p.joy_aim.y);
                if d.len() > sw / 2.0 {
                    let u = d.normalized();
                    v = v2(u.x * sw / 2.0 + p.joy_aim.x, u.y * sw / 2.0 + p.joy_aim.y);
                }
            }
            self.ui_set_position(n, v3(v.x, v.y, 0.0));
        }
    }

    /// The jab threshold for this arena's boxes.
    pub fn jab_threshold(&self) -> f32 {
        self.jab()
    }

    pub(crate) fn jab(&self) -> f32 {
        let drag = self.boxes.first().and_then(|b| self.w.nodes[b.node].body.as_ref()).map(|b| b.drag).unwrap_or(5.0);
        jab_charge(drag)
    }

    fn charge(&mut self, k: usize) {
        let jab = self.jab();
        let p = &mut self.players[k];
        if !p.touch[1].is_zero() {
            if p.picked_box.is_some() && p.charge_timer < FULL_CHARGE {
                // quick to a jab, then the long pull to a loaded throw
                let before = p.charge_timer;
                p.charge_timer = (p.charge_timer + if before < jab { JAB_FILL } else { 1.0 }).min(FULL_CHARGE);
                let fill = p.charge_timer / FULL_CHARGE;
                let crossed = before < jab && p.charge_timer >= jab && !p.single;
                if !p.playback {
                    if let Some(bar) = p.charge_bar {
                        if let Some(img) = self.w.nodes[bar].image.as_mut() {
                            img.fill = fill;
                        }
                    }
                }
                if crossed {
                    self.fx_loaded(k);
                }
            }
        } else if p.charge_timer != 0.0 {
            self.tasks.push(Task::ResetChargeTimer { player: k, wait: ticks(0.1) });
            self.tasks.push(Task::ScaleBackChargeBar { player: k });
        }
    }

    pub(super) fn reset_charge_timer(&mut self, k: usize) {
        self.players[k].charge_timer = 0.0;
    }

    /// returns false when the coroutine ends
    pub(super) fn scale_back_charge_bar(&mut self, k: usize) -> bool {
        let p = &self.players[k];
        let Some(bar) = p.charge_bar else { return false };
        let ct = p.charge_timer;
        let Some(img) = self.w.nodes[bar].image.as_mut() else { return false };
        if img.fill > 0.0 && ct != 0.0 {
            img.fill -= 0.2;
            true
        } else {
            false
        }
    }

    fn aim_update(&mut self, k: usize) {
        let p = &self.players[k];
        let (Some(anchor), Some(aimer)) = (p.aimer_anchor, p.aimer) else { return };
        let allowed = !p.single || self.tutorial.as_ref().map(|t| t.allowed_to_aim).unwrap_or(false);
        if allowed {
            let aim = p.touch[1];
            let hide = aim.is_zero() || p.hide_anchor;
            if let Some(s) = self.w.nodes[aimer].sprite.as_mut() {
                s.enabled = !hide;
            }
            if !hide {
                let a = -aim.x.atan2(aim.y).to_degrees() + 90.0;
                self.w.set_euler(anchor, 0.0, 0.0, a);
            }
        }
        // determineAimSize: stretch the aim line to the first hit along +x
        let pos = self.w.position(anchor);
        let dir = self.w.rotation(anchor).rotate(v3(1.0, 0.0, 0.0));
        let mask = self.players[k].aim_mask;
        let hit = self.phys.raycast(&self.w, pos, dir, 50.0, mask);
        if let Some(h) = &hit {
            self.players[k].last_hit = h.point;
        }
        // the line takes on what it touches: thin and soft on walls, firmer
        // on a box, heaviest and faintly trembling on a soul
        let target = match hit.as_ref().map(|h| self.hit_kind(h.node)) {
            Some(2) => [1.0, 1.0],
            Some(1) => [0.45, 1.0],
            _ => [0.0, 0.8],
        };
        let p = &mut self.players[k];
        for j in 0..2 {
            p.aim_form[j] += (target[j] - p.aim_form[j]) * 0.25;
        }
        if p.aim_form[1] == 0.0 {
            p.aim_form = target;
        }
        if let Some(s) = self.w.nodes[aimer].sprite.as_ref() {
            let base = *p.aim_base.get_or_insert((self.w.nodes[aimer].lscale.y, s.color[3]));
            let tremble = 1.0 + 0.06 * ((p.aim_form[0] - 0.5).max(0.0) * 2.0) * (self.frame as f32 * 0.9).sin();
            let wy = base.0 * (1.0 + 0.4 * p.aim_form[0]) * tremble;
            let a = base.1 * p.aim_form[1];
            self.w.nodes[aimer].lscale.y = wy;
            self.w.nodes[aimer].sprite.as_mut().unwrap().color[3] = a;
        }
        let last = self.players[k].last_hit;
        let mut sx = -(last - pos).length() * 12.5;
        if p_single(&self.players[k]) && hit.is_none() {
            sx = -1000.0;
        }
        self.w.nodes[anchor].lscale.x = sx;
    }

    /// 2 = a player, 1 = a box, 0 = anything else.
    fn hit_kind(&self, mut n: usize) -> u8 {
        loop {
            if self.player_of.contains_key(&n) {
                return 2;
            }
            if self.boxes.iter().any(|b| b.node == n) {
                return 1;
            }
            match self.w.nodes[n].parent {
                Some(p) => n = p,
                None => return 0,
            }
        }
    }

    fn carry_box(&mut self, k: usize) {
        if let Some(b) = self.players[k].picked_box {
            let p = self.w.position(self.players[k].node);
            self.w.set_position(b, p);
        }
    }

    fn test_for_pickup(&mut self, k: usize) {
        let p = &self.players[k];
        let (Some(col), None) = (p.collided_box, p.picked_box) else { return };
        if p.pickup_tutorial {
            self.pick_box_up(k, col);
            return;
        }
        let bi = self.box_of.get(&col).copied();
        let thrown_pickup = bi.map(|b| self.boxes[b].being_thrown_pickup).unwrap_or(false);
        if !thrown_pickup {
            self.pick_box_up(k, col);
        } else {
            self.players[k].collided_box = None;
        }
    }

    pub(super) fn pick_box_up(&mut self, k: usize, b: usize) {
        let node = self.players[k].node;
        self.players[k].picked_box = Some(b);
        self.players[k].collided_box = None;
        if let Some(c) = self.w.nodes[b].collider.as_mut() {
            c.enabled = false;
        }
        if let Some(&bi) = self.box_of.get(&b) {
            self.boxes[bi].carried_by = Some(node);
        }
        self.phys.set_ignore(b, node, true);
        // pick-swing: grabbing a box refills the dash
        let reset = self.input.desktop && self.players[k].dash_cd > 0;
        if reset {
            self.players[k].dash_cd = 0;
        }
        self.fx_pickup(k, b, reset);
    }

    #[doc(hidden)]
    pub fn pick_box_up_pub(&mut self, k: usize, b: usize) {
        self.pick_box_up(k, b);
    }

    pub(super) fn drop_box(&mut self, k: usize) {
        if let Some(b) = self.players[k].picked_box.take() {
            if let Some(c) = self.w.nodes[b].collider.as_mut() {
                c.enabled = true;
            }
            let node = self.players[k].node;
            self.tasks.push(Task::Unignore { a: b, b: node, wait: ticks(0.5) });
            if let Some(&bi) = self.box_of.get(&b) {
                self.box_explode(bi);
            }
            self.players[k].collided_box = None;
        }
    }

    fn throw_started(p: &super::Player) -> bool {
        p.touch[1].is_zero() && (!p.prev[1].is_zero() || !p.prev2[1].is_zero())
    }

    fn throw_fixed(&mut self, k: usize) {
        let Some(b) = self.players[k].picked_box else { return };
        let p = &mut self.players[k];
        p.throw_angle = p.prev2[1].normalized();
        if !Self::throw_started(p) {
            return;
        }
        let angle = p.throw_angle;
        let power = (p.charge_timer * 1.11).max(20.0);
        let single = p.single;
        let node = p.node;
        let woosh = self.menu.woosh.clone();
        self.play(&woosh, 1.0);
        if !single {
            if let Some(c) = self.w.nodes[b].collider.as_mut() {
                c.enabled = true;
            }
        } else {
            self.tasks.push(Task::ReinstateCollision { player: k, wait: 2 });
        }
        if let Some(body) = self.w.nodes[b].body.as_mut() {
            body.velocity = body.velocity + v3(angle.x * power, angle.y * power, 0.0);
        }
        // the box leaves from inside your body: whatever it overlaps right now
        // (the wall you hug, a neighbour) must not deflect the throw
        if let Some(gb) = crate::physics::geometry(&self.w, b) {
            let touching: Vec<usize> = (0..self.w.nodes.len())
                .filter(|&o| o != b && o != node && self.w.nodes[o].collider.as_ref().map(|c| c.enabled && !c.trigger).unwrap_or(false) && self.w.active_in_hierarchy(o))
                .filter(|&o| crate::physics::layers_collide(self.w.nodes[b].layer, self.w.nodes[o].layer))
                // never a soul: a point-blank throw must still land
                .filter(|&o| self.w.nodes[o].layer != crate::physics::LAYER_PLAYER)
                // touching (or nearly) and the throw heads away from it; throwing
                // into something you touch still hits it
                .filter(|&o| {
                    crate::physics::geometry(&self.w, o)
                        .and_then(|g| crate::physics::collide(&gb, &g, 0.05))
                        .map(|c| c.normal[0] * angle.x + c.normal[1] * angle.y > -0.2)
                        .unwrap_or(false)
                })
                .collect();
            for o in touching {
                self.phys.set_ignore(b, o, true);
                self.tasks.push(Task::Unignore { a: b, b: o, wait: ticks(0.25) });
            }
        }
        self.tasks.push(Task::ThrowRotation { node: b, i: 0, n: (power / 3.0).ceil() as i32 });
        self.fx_throw(k, angle, power);
        self.tasks.push(Task::Unignore { a: b, b: node, wait: ticks(0.5) });
        if let Some(&bi) = self.box_of.get(&b) {
            // live from the instant it leaves your hands (the original only
            // counted it from the third tick, so point-blank hits just shoved)
            self.boxes[bi].being_thrown = true;
            self.boxes[bi].being_thrown_pickup = true;
            self.boxes[bi].bounces = 0;
            self.boxes[bi].spent = false;
            self.boxes[bi].thrown_charge = self.players[k].charge_timer;
            if !single {
                self.throw_log.push(self.players[k].charge_timer);
            }
            self.tasks.push(Task::ResetBeingThrown { b: bi, stage: 0, wait: 3 });
        }
        if !single {
            self.players[k].picked_box = None;
        }
    }

    pub(super) fn reinstate_collision(&mut self, k: usize) {
        if let Some(b) = self.players[k].picked_box.take() {
            if let Some(c) = self.w.nodes[b].collider.as_mut() {
                c.enabled = true;
            }
        }
    }

    /// OnCollisionEnter / Exit for BoxPickupBehaviour.
    pub(super) fn player_collision(&mut self, k: usize, other: usize, enter: bool) {
        if enter {
            if &*self.w.nodes[other].tag == "Box" {
                self.players[k].collided_box = Some(other);
            }
        } else if self.players[k].collided_box == Some(other) {
            self.players[k].collided_box = None;
        }
    }

    fn burn_up(&mut self, b: usize) {
        let node = self.burners[b].node;
        let pos = self.w.position(node);
        let dir = self.w.rotation(node).rotate(v3(1.0, 0.0, 0.0));
        let Some(hit) = self.phys.raycast(&self.w, pos, dir, 50.0, self.burners[b].mask) else { return };
        let target = sprite_target(&self.w, hit.node);
        let br = self.burners[b].clone();
        let tag = self.w.nodes[target].tag.clone();
        let (glow, _normal) = match &*tag {
            "Wall" => (br.wall_glow, br.wall),
            "Box" => (br.box_glow, br.box_),
            _ => return,
        };
        if Some(target) == br.previous {
            return;
        }
        set_sprite(&mut self.w, target, glow);
        if let Some(prev) = br.previous {
            let restore = match &*self.w.nodes[prev].tag {
                "Wall" => br.wall,
                "Box" => br.box_,
                _ => None,
            };
            if restore.is_some() {
                set_sprite(&mut self.w, prev, restore);
            }
        }
        self.burners[b].previous = Some(target);
    }

    pub(super) fn blob_scale_start(&mut self, player_node: usize) {
        if let Some(&k) = self.player_of.get(&player_node) {
            if !self.players[k].blob_scaling {
                self.players[k].blob_scaling = true;
                self.tasks.push(Task::ScalePlayer { player: k, i: 0 });
            }
        }
    }

    /// returns false when finished
    pub(super) fn scale_player_step(&mut self, k: usize, i: u32) -> bool {
        let node = self.players[k].node;
        let d = if i < 8 { 0.05 } else { -0.025 };
        self.w.nodes[node].lscale = self.w.nodes[node].lscale + v3(d, d, d);
        if i + 1 >= 24 {
            self.players[k].blob_scaling = false;
            return false;
        }
        true
    }

    /// Trails: sample player positions for the TrailRenderer.
    pub(super) fn update_trails(&mut self) {
        let now = self.frame as f32 * super::DT;
        for k in 0..self.players.len() {
            let node = self.players[k].node;
            let Some(tr) = self.w.nodes[node].trail.as_ref() else { continue };
            let life = tr.time;
            let p = self.w.position(node);
            let trail = &mut self.players[k].trail;
            if !self.w.active_in_hierarchy(node) || p.x == -666.0 {
                trail.clear();
                continue;
            }
            if trail.last().map(|(q, _)| (*q - p).length() > 3.0).unwrap_or(false) {
                trail.clear(); // teleported (respawn)
            }
            trail.push((p, now));
            trail.retain(|(_, t)| now - *t <= life);
        }
    }
}

fn p_single(p: &Player) -> bool {
    p.single
}

fn sprite_ref(w: &crate::scene::World, node: usize, name: &str) -> Option<usize> {
    field(w, node, "ObjectBurner", name).as_str().and_then(|k| w.sprite_keys.get(k).copied())
}

/// The object whose SpriteRenderer represents a hit collider (walls keep the
/// collider on the sprite object itself; fall back to the parent).
fn sprite_target(w: &crate::scene::World, n: usize) -> usize {
    if w.nodes[n].sprite.is_some() {
        n
    } else {
        w.nodes[n].parent.filter(|&p| w.nodes[p].sprite.is_some()).unwrap_or(n)
    }
}

fn set_sprite(w: &mut crate::scene::World, n: usize, s: Option<usize>) {
    if let (Some(r), Some(s)) = (w.nodes[n].sprite.as_mut(), s) {
        r.sprite = Some(s);
    }
}

#[allow(dead_code)]
fn _unused(_: Quat) {}
