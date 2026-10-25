//! Where the game hands moments to the motion system: the UI lifecycle
//! (every element forms, re-forms when its text changes, shatters when it
//! leaves), the cursor, and the match's beats.

use std::collections::HashSet;

use crate::motion::{hf, Kind};
use crate::shared::math::{v3, V3};
use super::V2;

use super::{Game, DT};

const INK: [f32; 4] = [0.04, 0.04, 0.06, 0.9];

fn q3(q: [[f32; 2]; 4]) -> [V3; 4] {
    q.map(|p| v3(p[0], p[1], 0.0))
}

fn center(q: &[[f32; 2]; 4]) -> V3 {
    v3((q[0][0] + q[1][0] + q[2][0] + q[3][0]) / 4.0, (q[0][1] + q[1][1] + q[2][1] + q[3][1]) / 4.0, 0.0)
}

fn closed(q: [[f32; 2]; 4]) -> Vec<V3> {
    let mut v: Vec<V3> = q3(q).to_vec();
    v.push(v[0]);
    v
}

impl Game {
    /// A player's colour (the soul sprite's tint).
    pub fn soul_color(&self, node: usize) -> [f32; 4] {
        self.w.nodes[node].children.first().and_then(|&c| self.w.nodes[c].sprite.as_ref()).map(|s| s.color).unwrap_or([0.2, 0.2, 0.2, 1.0])
    }

    fn soul_pos(&self, k: usize) -> Option<V3> {
        let p = self.w.position(self.players[k].node);
        (p.x != -666.0).then_some(p)
    }

    /// End of every fixed step.
    pub(super) fn fx_step(&mut self) {
        let mut m = std::mem::take(&mut self.motion);
        let w = &self.w;
        let layout = &self.layout;
        let screen = |n: usize| self.ui_screen_quad(n).map(|q| center(&q));
        let world = |n: usize| {
            if n >= w.nodes.len() || !w.active_in_hierarchy(n) {
                return None;
            }
            let p = if layout.canvas_of.contains_key(&n) { layout.position(w, n) } else { w.position(n) };
            (p.x != -666.0).then_some(p)
        };
        m.step(DT, |n, s| if s { screen(n) } else { world(n) });
        self.motion = m;
        self.fx_ui();
        self.fx_cursor();
    }

    fn fx_ui(&mut self) {
        let now = self.motion.now;
        let (sw, sh) = (self.screen.w, self.screen.h);
        let mut seen = HashSet::new();
        for i in self.ui_draw_order() {
            let n = &self.w.nodes[i];
            let img = n.image.as_ref().filter(|g| g.enabled).map(|g| [g.color[0] * g.tint[0], g.color[1] * g.tint[1], g.color[2] * g.tint[2], g.color[3] * g.tint[3]]);
            let txt = n.text.as_ref().filter(|t| t.enabled && !t.text.trim().is_empty()).map(|t| (t.text.clone(), [t.color[0] * t.tint[0], t.color[1] * t.tint[1], t.color[2] * t.tint[2], t.color[3] * t.tint[3]]));
            let color = txt.as_ref().map(|t| t.1).or(img).unwrap_or([0.0; 4]);
            if color[3] < 0.03 || !self.w.active_in_hierarchy(i) {
                continue;
            }
            let Some(q) = self.ui_screen_quad(i) else { continue };
            let area = ((q[1][0] - q[0][0]) * (q[3][1] - q[0][1])).abs();
            if area > 0.45 * sw * sh || area < 4.0 {
                continue; // full-screen fades and slivers just fade
            }
            seen.insert(i);
            let text = txt.as_ref().map(|t| t.0.clone()).unwrap_or_default();
            let button = !n.button.is_empty();
            let c = center(&q);
            // things that appear together cascade top to bottom
            let stagger = if self.motion.booted { (1.0 - c.y / sh).clamp(0.0, 1.0) * 0.28 } else { 0.25 + (1.0 - c.y / sh).clamp(0.0, 1.0) * 0.6 };
            match self.motion.snaps.get_mut(&i) {
                None => {
                    self.motion.forms.insert(i, now + stagger);
                    if !text.is_empty() || button {
                        let mut col = INK;
                        col[3] = 0.55;
                        self.motion.trace(true, stagger, closed(q), 1.2, 0.7, col);
                    }
                    self.motion.snaps.insert(i, crate::motion::Snap { quad: q, text, color, since: now, button });
                }
                Some(s) => {
                    if s.text != text && !s.text.is_empty() {
                        // the old value breaks, the new one is written in by the spark
                        let old = s.quad;
                        s.text = text;
                        s.quad = q;
                        s.color = color;
                        self.text_release(old, color, 0.6);
                        self.motion.forms.insert(i, now);
                        let mut col = color;
                        col[3] = 0.7;
                        self.motion.trace(true, 0.0, closed(q), 1.0, 0.5, col);
                    } else {
                        s.quad = q;
                        s.color = color;
                    }
                }
            }
        }
        let gone: Vec<usize> = self.motion.snaps.keys().copied().filter(|k| !seen.contains(k)).collect();
        for i in gone {
            let s = self.motion.snaps.remove(&i).unwrap();
            // shatter only what was switched off (things faded by alpha just fade)
            if self.w.active_in_hierarchy(i) || now - s.since < 0.2 {
                continue;
            }
            if !s.text.is_empty() {
                self.text_release(s.quad, s.color, 1.0);
            } else {
                let (w, h) = ((s.quad[1][0] - s.quad[0][0]).abs(), (s.quad[3][1] - s.quad[0][1]).abs());
                let cells = (((w / 46.0).ceil() as usize).clamp(1, 6), ((h / 46.0).ceil() as usize).clamp(1, 6));
                self.motion.shatter(true, q3(s.quad), cells, 260.0, 0.75, s.color, None, V3::ZERO);
                self.ui_crackle(s.quad, 2);
            }
        }
        self.motion.booted = true;
    }

    /// Text breaks into a thin band of glass along its line, with a crackle.
    fn text_release(&mut self, q: [[f32; 2]; 4], color: [f32; 4], strength: f32) {
        let c = center(&q);
        let (w, h) = ((q[1][0] - q[0][0]).abs(), (q[3][1] - q[0][1]).abs());
        let (bw, bh) = ((w * 0.7).min(h * 9.0), (h * 0.45).min(60.0));
        let band = [c + v3(-bw / 2.0, -bh / 2.0, 0.0), c + v3(bw / 2.0, -bh / 2.0, 0.0), c + v3(bw / 2.0, bh / 2.0, 0.0), c + v3(-bw / 2.0, bh / 2.0, 0.0)];
        let mut col = color;
        col[3] = col[3].min(0.85);
        let nx = ((bw / 22.0).ceil() as usize).clamp(2, 14);
        self.motion.shatter(true, band, (nx, 1), 190.0 * strength, 0.6, col, None, v3(0.0, 40.0, 0.0));
        self.motion.burst(true, c, (6.0 * strength) as usize + 2, 420.0, 2.2, 0.45, col, None);
        self.ui_crackle(q, 1);
    }

    fn ui_crackle(&mut self, q: [[f32; 2]; 4], n: usize) {
        for k in 0..n {
            let s = self.motion.seed.wrapping_add(k as u32 * 977);
            let a = q[(s % 4) as usize];
            let b = q[((s / 4 + 2) % 4) as usize];
            let mut col = INK;
            col[3] = 0.8;
            self.motion.arc(true, v3(a[0], a[1], 0.0), v3(b[0], b[1], 0.0), 1.4, 0.14, 0.22, col);
        }
    }

    fn fx_cursor(&mut self) {
        let Some(c) = self.input.cursor.filter(|c| c[0] >= 0.0) else {
            self.motion.cursor_trail.clear();
            self.motion.hover = None;
            return;
        };
        self.motion.cursor_trail.push_back(c);
        while self.motion.cursor_trail.len() > 7 {
            self.motion.cursor_trail.pop_front();
        }
        let hit = self.button_at(c[0], c[1]);
        if hit != self.motion.hover {
            self.motion.hover = hit;
            self.motion.hover_since = self.motion.now;
            if let Some(b) = hit {
                if let Some(q) = self.ui_screen_quad(b) {
                    // the spark leaps over and runs around the button
                    self.motion.trace(true, 0.0, closed(q), 1.6, 0.45, INK);
                    self.motion.arc(true, v3(c[0], c[1], 0.0), center(&q), 1.6, 0.2, 0.16, INK);
                }
            }
        }
    }

    /// Any press on the screen.
    pub(super) fn fx_click(&mut self, x: f32, y: f32) {
        let p = v3(x, y, 0.0);
        self.motion.ring(true, p, 3.0, 34.0, 1.6, 0.4, INK);
        self.motion.burst(true, p, 5, 300.0, 1.8, 0.3, INK, None);
    }

    /// A button fired.
    pub(super) fn fx_button(&mut self, b: usize) {
        let Some(q) = self.ui_screen_quad(b) else { return };
        let c = center(&q);
        let r = ((q[1][0] - q[0][0]).abs().max((q[3][1] - q[0][1]).abs())) * 0.5;
        self.motion.ring(true, c, r * 0.6, r * 1.6, 2.2, 0.5, INK);
        self.motion.flash(true, c, r * 1.4, 0.3, [0.0, 0.0, 0.0, 0.25]);
        self.motion.burst(true, c, 14, 700.0, 2.4, 0.5, INK, None);
        self.ui_crackle(q, 3);
    }

    /// A subtree switched on in the world: it assembles itself, piece by
    /// piece outward from its middle, each solid piece landing with a ring.
    pub(super) fn fx_form_subtree(&mut self, root: usize) {
        let mut nodes = Vec::new();
        self.w.descendants(root, &mut nodes);
        let sprites: Vec<usize> = nodes
            .into_iter()
            .filter(|&n| !self.layout.canvas_of.contains_key(&n) && self.w.nodes[n].sprite.as_ref().map(|s| s.enabled).unwrap_or(false) && self.w.active_in_hierarchy(n))
            .collect();
        if sprites.is_empty() {
            return;
        }
        let pts: Vec<V3> = sprites.iter().map(|&n| self.w.position(n)).collect();
        let mid = pts.iter().fold(V3::ZERO, |a, &b| a + b) * (1.0 / pts.len() as f32);
        let now = self.motion.now;
        for (k, &n) in sprites.iter().enumerate() {
            let d = ((pts[k].x - mid.x).powi(2) + (pts[k].y - mid.y).powi(2)).sqrt();
            let delay = 0.35 + (d * 0.045).min(0.9) + hf(n as u32) * 0.04;
            self.motion.forms.insert(n, now + delay);
            let solid = self.w.nodes[n].collider.is_some() || self.player_of.contains_key(&n);
            if solid {
                let mut col = INK;
                col[3] = 0.35;
                self.motion.ring_at(false, delay, pts[k], 0.1, 1.1, 0.04, 0.5, col);
            }
        }
        // souls condense from sparks a beat after the arena stands
        for k in 0..self.players.len() {
            let n = self.players[k].node;
            if !sprites.contains(&n) && !nodes_contains(&self.w, root, n) {
                continue;
            }
            let p = self.w.position(n);
            let col = self.soul_color(n);
            self.motion.forms.insert(n, now + 1.1);
            self.motion.push(Kind::Flash { c: p, r: 0.9 }, false, 1.1, 0.5, col);
            for j in 0..10 {
                let a = j as f32 / 10.0 * std::f32::consts::TAU;
                let from = p + v3(a.cos(), a.sin(), 0.0) * 2.2;
                self.motion.push(Kind::Spark { p: from, v: v3(-a.sin(), a.cos(), 0.0) * 4.0, size: 0.05, seek: None, home: Some(p) }, false, 0.5, 0.65, col);
            }
        }
        // the fight cue: a single arc snaps between the souls
        let alive: Vec<V3> = (0..self.players.len()).filter(|&k| nodes_contains(&self.w, root, self.players[k].node)).map(|k| self.w.position(self.players[k].node)).collect();
        if alive.len() == 2 {
            self.motion.push(Kind::Arc { a: alive[0], b: alive[1], w: 0.04, jitter: 0.08 }, false, 1.35, 0.35, INK);
        }
        self.motion.last_action = now + 1.5;
    }

    pub(super) fn fx_pickup(&mut self, k: usize, b: usize, reset: bool) {
        let Some(p) = self.soul_pos(k) else { return };
        let bp = self.w.position(b);
        let col = self.soul_color(self.players[k].node);
        for _ in 0..3 {
            self.motion.arc(false, p, bp, 0.025, 0.25, 0.16, INK);
        }
        self.motion.ring(false, bp, 0.15, 0.55, 0.035, 0.3, col);
        if reset {
            // the dash comes back: lightning cracks from the box into the soul
            let mut hot = col;
            hot[3] = 1.0;
            self.motion.arc(false, bp, p, 0.06, 0.18, 0.28, hot);
            self.motion.arc(false, bp, p, 0.03, 0.35, 0.22, INK);
            self.motion.flash(false, p, 0.9, 0.3, hot);
            self.motion.trace_circle(false, 0.0, p, 0.34, std::f32::consts::FRAC_PI_2, 0.035, 0.3, hot);
            self.motion.burst(false, p, 10, 7.0, 0.05, 0.35, hot, None);
        }
        self.motion.last_action = self.motion.now;
    }

    pub(super) fn fx_throw(&mut self, k: usize, dir: V2, power: f32) {
        let Some(p) = self.soul_pos(k) else { return };
        let col = self.soul_color(self.players[k].node);
        let f = (power / 100.0).clamp(0.2, 1.0);
        self.motion.ring(false, p, 0.25, 0.7 + 0.8 * f, 0.05, 0.35, col);
        let d = v3(dir.x, dir.y, 0.0);
        for j in 0..(4 + (8.0 * f) as usize) {
            let s = crate::motion::hash(j as u32 ^ self.frame as u32);
            let side = v3(-d.y, d.x, 0.0) * hf(s) * 2.0;
            self.motion.push(Kind::Spark { p: p + d * 0.3, v: d * (6.0 + 10.0 * f) + side, size: 0.035, seek: None, home: None }, false, 0.0, 0.28, col);
        }
        self.motion.kick = self.motion.kick - d * (0.06 + 0.12 * f);
        self.motion.last_action = self.motion.now;
    }

    pub(super) fn fx_dash(&mut self, k: usize, dir: V2) {
        let Some(p) = self.soul_pos(k) else { return };
        let col = self.soul_color(self.players[k].node);
        let mut c = col;
        c[3] = 0.55;
        self.motion.ring(false, p, 0.2, 0.9, 0.03, 0.3, c);
        let d = v3(dir.x, dir.y, 0.0);
        let side = v3(-d.y, d.x, 0.0);
        for s in [-1.0f32, 1.0] {
            self.motion.arc(false, p + side * (0.18 * s), p - d * 1.2 + side * (0.3 * s), 0.018, 0.3, 0.18, INK);
        }
        self.motion.last_action = self.motion.now;
    }

    /// Cooldown over: a spark runs once around the soul.
    pub(super) fn fx_dash_ready(&mut self, k: usize) {
        let Some(p) = self.soul_pos(k) else { return };
        let col = self.soul_color(self.players[k].node);
        self.motion.trace_circle(false, 0.0, p, 0.3, std::f32::consts::FRAC_PI_2, 0.022, 0.32, col);
    }

    pub(super) fn fx_kill(&mut self, player: usize, killer: usize) {
        let p = self.w.position(player);
        if p.x == -666.0 {
            return;
        }
        let col = self.soul_color(player);
        let kcol = self.soul_color(killer);
        self.motion.hitstop = 5;
        self.motion.flash(false, p, 2.2, 0.45, col);
        self.motion.flash(false, p, 1.0, 0.25, [1.0, 1.0, 1.0, 0.9]);
        self.motion.ring(false, p, 0.3, 3.2, 0.08, 0.55, col);
        self.motion.ring(false, p, 0.2, 1.8, 0.03, 0.4, INK);
        // the soul breaks, and what it was flows into the one who ended it
        let r = 0.3;
        let q = [p + v3(-r, -r, 0.0), p + v3(r, -r, 0.0), p + v3(r, r, 0.0), p + v3(-r, r, 0.0)];
        self.motion.shatter(false, q, (3, 3), 5.0, 1.4, col, Some(killer), V3::ZERO);
        self.motion.burst(false, p, 26, 11.0, 0.05, 1.3, col, Some(killer));
        for j in 0..6 {
            let a = j as f32 / 6.0 * std::f32::consts::TAU + hf(self.frame as u32 + j) * 0.5;
            self.motion.arc(false, p, p + v3(a.cos(), a.sin(), 0.0) * 1.6, 0.03, 0.3, 0.3, INK);
        }
        let k = self.w.position(killer);
        if k.x != -666.0 {
            self.motion.arc(false, p, k, 0.045, 0.12, 0.4, kcol);
            // the killer drinks it in
            self.motion.ring_at(false, 0.6, k, 1.4, 0.25, 0.06, 0.5, kcol);
        }
        let d = v3(hf(self.frame as u32), hf(self.frame as u32 ^ 3), 0.0);
        self.motion.kick = self.motion.kick + d * 0.35;
        self.motion.last_action = self.motion.now;
    }

    pub(super) fn fx_respawn(&mut self, player: usize) {
        let p = self.w.position(player);
        let col = self.soul_color(player);
        self.motion.converge(false, p, 14, 2.4, 0.05, 0.55, col);
        self.motion.forms.insert(player, self.motion.now + 0.45);
        self.motion.push(Kind::Flash { c: p, r: 1.1 }, false, 0.45, 0.4, col);
        self.motion.ring_at(false, 0.45, p, 0.2, 1.2, 0.05, 0.4, col);
    }

    /// A thrown box hit something that isn't a soul.
    pub(super) fn fx_impact(&mut self, pos: V3, normal: V3) {
        self.motion.ring(false, pos, 0.1, 0.9, 0.04, 0.3, INK);
        let t = v3(-normal.y, normal.x, 0.0);
        for s in [-1.0f32, 1.0] {
            self.motion.arc(false, pos, pos + t * (0.7 * s) + normal * 0.3, 0.02, 0.35, 0.18, INK);
        }
        self.motion.burst(false, pos, 6, 6.0, 0.035, 0.3, INK, None);
        self.motion.kick = self.motion.kick + normal * 0.05;
    }

    /// A box comes back: it condenses in.
    pub(super) fn fx_box_back(&mut self, node: usize) {
        let p = self.w.position(node);
        self.motion.forms.insert(node, self.motion.now);
        let mut col = INK;
        col[3] = 0.5;
        self.motion.converge(false, p, 8, 1.2, 0.03, 0.3, col);
        self.motion.ring_at(false, 0.25, p, 0.1, 0.7, 0.03, 0.35, col);
    }
}

fn nodes_contains(w: &crate::scene::World, root: usize, n: usize) -> bool {
    let mut c = Some(n);
    while let Some(x) = c {
        if x == root {
            return true;
        }
        c = w.nodes[x].parent;
    }
    false
}
