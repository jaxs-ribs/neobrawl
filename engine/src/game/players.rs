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
    pub collided_box: Option<usize>,
    pub picked_box: Option<usize>,
    pub pickup_tutorial: bool,
    pub throw_angle: V2,
    pub blob_scaling: bool,
    pub started: bool,
    pub trail: Vec<(V3, f32)>,
    /// smoothed movement command (desktop acceleration)
    pub cmd: V2,
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

    /// Desktop input: smoothed movement and mouse / stick aiming.
    fn direct_fixed(&mut self, k: usize, slot: usize) {
        let ctl = self.input.direct[slot];
        let p = &mut self.players[k];
        p.prev2 = p.prev;
        p.prev = p.touch;
        // a short acceleration ramp (~60 ms to full speed) keeps it snappy but smooth
        let target = if ctl.active { ctl.mv } else { V2::ZERO };
        let a = 1.0 - (-DT / 0.06).exp();
        p.cmd = v2(p.cmd.x + (target.x - p.cmd.x) * a, p.cmd.y + (target.y - p.cmd.y) * a);
        if p.cmd.len() < 0.01 && target.is_zero() {
            p.cmd = V2::ZERO;
        }
        p.touch[0] = p.cmd.normalized();
        p.mag[0] = p.cmd.len().min(1.0);
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

    /// The point under the mouse cursor on the player's plane (z of the player).
    fn cursor_world(&self, k: usize) -> Option<V3> {
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
        let v = if go { v3(p.touch[0].x * p.mag[0], p.touch[0].y * p.mag[0], 0.0) * 8.0 } else { V3::ZERO };
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

    fn charge(&mut self, k: usize) {
        let p = &mut self.players[k];
        if !p.touch[1].is_zero() {
            if p.picked_box.is_some() && p.charge_timer < 90.0 {
                p.charge_timer += 1.0;
                let fill = p.charge_timer / 90.0;
                if !p.playback {
                    if let Some(bar) = p.charge_bar {
                        if let Some(img) = self.w.nodes[bar].image.as_mut() {
                            img.fill = fill;
                        }
                    }
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
        let last = self.players[k].last_hit;
        let mut sx = -(last - pos).length() * 12.5;
        if p_single(&self.players[k]) && hit.is_none() {
            sx = -1000.0;
        }
        self.w.nodes[anchor].lscale.x = sx;
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
        self.tasks.push(Task::ThrowRotation { node: b, i: 0, n: (power / 3.0).ceil() as i32 });
        self.tasks.push(Task::Unignore { a: b, b: node, wait: ticks(0.5) });
        if let Some(&bi) = self.box_of.get(&b) {
            self.boxes[bi].being_thrown_pickup = true;
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
            if self.w.nodes[other].tag == "Box" {
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
        let (glow, _normal) = match tag.as_str() {
            "Wall" => (br.wall_glow, br.wall),
            "Box" => (br.box_glow, br.box_),
            _ => return,
        };
        if Some(target) == br.previous {
            return;
        }
        set_sprite(&mut self.w, target, glow);
        if let Some(prev) = br.previous {
            let restore = match self.w.nodes[prev].tag.as_str() {
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
