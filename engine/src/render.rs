//! Flattens the scene into draw records for the host (28 f32 each):
//!   pass  [9, clear?, 0, 0, viewProj(16), clearColor(4), 0,0,0,0]
//!   mesh  [0, mesh, material, 0, model(16), color(4), 0,0,0,0]
//!   quad  [2, texture (-1 = white), 0, 0, p0 p1 p2 p3 (12), u0 v0 u1 v1, color(4)]
//!   text  [1, node, glyphHeight, align, p0 p1 p2 p3 (12), style, 0, 0, 0, color(4)]
//! Each pass: opaque meshes first, then transparent items back to front.

use crate::shared::math::{v3, Mat4, V3};

use crate::game::Game;

pub const RECORD: usize = 28;

struct Item {
    order: i32,
    dist: f32,
    seq: usize,
    rec: [f32; RECORD],
}

pub(crate) fn quad(tex: f32, p: [V3; 4], uv: [f32; 4], color: [f32; 4]) -> [f32; RECORD] {
    let mut r = [0.0; RECORD];
    r[0] = 2.0;
    r[1] = tex;
    for k in 0..4 {
        r[4 + k * 3] = p[k].x;
        r[5 + k * 3] = p[k].y;
        r[6 + k * 3] = p[k].z;
    }
    r[16..20].copy_from_slice(&uv);
    r[20..24].copy_from_slice(&color);
    r
}

fn perspective(fov: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    // Unity camera space looks down +Z; map z in [near, far] to [0, 1]
    let t = 1.0 / (fov.to_radians() / 2.0).tan();
    let a = far / (far - near);
    Mat4([t / aspect, 0.0, 0.0, 0.0, 0.0, t, 0.0, 0.0, 0.0, 0.0, a, 1.0, 0.0, 0.0, -near * a, 0.0])
}

fn ortho_screen(w: f32, h: f32) -> Mat4 {
    Mat4([2.0 / w, 0.0, 0.0, 0.0, 0.0, 2.0 / h, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0, -1.0, 0.5, 1.0])
}

impl Game {
    pub fn render(&self, out: &mut Vec<f32>) {
        let lap = |i: usize| self.prof.borrow_mut().lap(i);
        self.prof.borrow_mut().begin();
        out.clear();
        let w = &self.w;
        let cams: Vec<usize> = (0..w.nodes.len()).filter(|&i| w.nodes[i].camera.is_some() && w.active_in_hierarchy(i)).collect();
        let ui_order = self.ui_draw_order();
        let ui_seq: std::collections::HashMap<usize, usize> = ui_order.iter().enumerate().map(|(k, &n)| (n, k)).collect();
        let (wm, wact) = w.world_cache();
        let lerp = self.interp_offsets();
        let deform = self.body_deforms();
        let shift = |i: usize, mut m: Mat4| {
            if let Some(o) = lerp.get(&i) {
                m.0[12] += o.x;
                m.0[13] += o.y;
                m.0[14] += o.z;
            }
            m
        };
        for &cam in &cams {
            let c = w.nodes[cam].camera.as_ref().unwrap();
            let mut cm = shift(cam, w.matrix(cam));
            if w.nodes[cam].tag == "MainCamera" {
                let o = self.motion.shake_offset();
                cm.0[12] += o.x;
                cm.0[13] += o.y;
            }
            let view = cm.inverse_affine();
            let vp = perspective(c.fov, self.screen.w / self.screen.h, c.near, c.far).mul(&view);
            let mut head = [0.0f32; RECORD];
            head[0] = 9.0;
            head[1] = 1.0;
            head[4..20].copy_from_slice(&vp.0);
            head[20..24].copy_from_slice(&c.bg);
            out.extend_from_slice(&head);
            let mut items: Vec<Item> = Vec::new();
            // soft radial texture reused as a 1D falloff across ribbons
            let soft = w.textures.iter().position(|t| t.1.ends_with("BlobBWShadow.png")).map(|t| t as f32).unwrap_or(-1.0);
            let dist = |p: V3| view.point(p).z;
            // the original aim line is replaced by the charger's blade on desktop
            let aimers: std::collections::HashSet<usize> = if self.input.desktop { self.players.iter().filter_map(|p| p.aimer).collect() } else { Default::default() };
            for i in 0..w.nodes.len() {
                let n = &w.nodes[i];
                if aimers.contains(&i) {
                    continue;
                }
                let ui = self.layout.canvas_of.get(&i).copied();
                if let Some((_, 0)) = ui {
                    continue; // overlay canvases draw in the screen pass
                }
                let interesting = n.sprite.is_some() || n.mesh.is_some() || n.line.is_some() || n.image.is_some() || n.text.is_some();
                if !interesting || !wact[i] {
                    continue;
                }
                let m = if ui.is_some() {
                    self.layout.matrix(w, i)
                } else {
                    let mut base = wm[i];
                    if let Some(d) = deform.get(&i) {
                        base = d.mul(&base);
                    }
                    if let Some((o, _)) = self.motion.jostles.get(&i).or_else(|| n.parent.and_then(|p| self.motion.jostles.get(&p))) {
                        base.0[12] += o.x;
                        base.0[13] += o.y;
                    }
                    shift(i, base)
                };
                if let Some((mesh, mat, true)) = n.mesh {
                    let mut r = [0.0; RECORD];
                    r[1] = mesh as f32;
                    r[2] = mat as f32;
                    r[4..20].copy_from_slice(&m.0);
                    r[20..24].copy_from_slice(&w.materials[mat].1);
                    out.extend_from_slice(&r);
                }
                if let Some(s) = n.sprite.as_ref().filter(|s| s.enabled && s.color[3] > 0.0) {
                    if let Some(sp) = s.sprite.map(|k| &w.sprites[k]) {
                        let q = sp.quad;
                        let mut p = [m.point(v3(q[0], q[1], 0.0)), m.point(v3(q[2], q[1], 0.0)), m.point(v3(q[2], q[3], 0.0)), m.point(v3(q[0], q[3], 0.0))];
                        let uv = [sp.uv[0], sp.uv[1], sp.uv[0] + sp.uv[2], sp.uv[1] + sp.uv[3]];
                        let center = m.point(V3::ZERO);
                        let mut color = s.color;
                        if let Some((k, a, _)) = self.form_of(i) {
                            if a <= 0.0 {
                                continue;
                            }
                            p = p.map(|q| center + (q - center) * k);
                            color[3] *= a;
                        }
                        let (seq, d) = match ui {
                            Some(_) => (ui_seq.get(&i).copied().unwrap_or(0), dist(center)),
                            None => (i, dist(center)),
                        };
                        if let Some(v) = sp.vector.as_ref() {
                            // traced vector fill instead of the bitmap
                            let k = self.form_of(i).map(|f| f.0).unwrap_or(1.0);
                            let color = [color[0] * v.ink[0], color[1] * v.ink[1], color[2] * v.ink[2], color[3]];
                            let flexing = self.motion.flex.iter().any(|f| f.node == i);
                            for t in &v.tris {
                                let pt = |x: f32, y: f32| {
                                    let q = center + (m.point(v3(x, y, 0.0)) - center) * k;
                                    if flexing { q + self.motion.flex_at(i, q) } else { q }
                                };
                                let (a, b, c) = (pt(t[0], t[1]), pt(t[2], t[3]), pt(t[4], t[5]));
                                items.push(Item { order: s.order, dist: d, seq, rec: quad(-1.0, [a, b, c, c], [0.0, 0.0, 1.0, 1.0], color) });
                            }
                        } else {
                            items.push(Item { order: s.order, dist: d, seq, rec: quad(sp.tex as f32, p, uv, color) });
                        }
                    }
                }
                if let Some(l) = n.line.as_ref().filter(|l| l.enabled && l.points.len() >= 2 && l.colors[0][3] > 0.0) {
                    // Debris "rave" lines become flickering electric arcs: a jagged
                    // core with a soft halo, re-rolled every frame.
                    let (a, b) = if l.world { (l.points[0], l.points[1]) } else { (m.point(l.points[0]), m.point(l.points[1])) };
                    let alpha = l.colors[0][3].clamp(0.0, 1.0);
                    let seed = (i as u32).wrapping_mul(2654435761) ^ (self.frame as u32).wrapping_mul(40503);
                    for (branch, jitter) in [(0u32, 0.12f32), (1, 0.2)] {
                        let pts = arc_points(a, b, 9, jitter, seed.wrapping_add(branch * 7919));
                        let d = dist((a + b) * 0.5);
                        let fade = if branch == 0 { 1.0 } else { 0.45 };
                        ribbon(&pts, 0.16, [0.0, 0.0, 0.0, 0.18 * alpha * fade], soft, |rec| items.push(Item { order: 1, dist: d, seq: i, rec }));
                        ribbon(&pts, 0.025, [0.05, 0.05, 0.08, 0.95 * alpha * fade], -1.0, |rec| items.push(Item { order: 1, dist: d - 0.001, seq: i, rec }));
                    }
                }
                if ui.is_some() {
                    let r = self.layout.rects.get(&i).copied().unwrap_or([0.0; 4]);
                    let corners = [m.point(v3(r[0], r[1], 0.0)), m.point(v3(r[2], r[1], 0.0)), m.point(v3(r[2], r[3], 0.0)), m.point(v3(r[0], r[3], 0.0))];
                    let d = dist(corners[0]);
                    let seq = ui_seq.get(&i).copied().unwrap_or(0);
                    self.ui_items(i, corners, &m, |rec| items.push(Item { order: 1000, dist: d, seq, rec }));
                }
            }
            lap(10);
            // trails: a smoothed, tapering ribbon in the player's colour with a soft glow
            for p in &self.players {
                let Some(tr) = w.nodes[p.node].trail.as_ref() else { continue };
                if !tr.enabled || p.trail.len() < 2 || !w.active_in_hierarchy(p.node) {
                    continue;
                }
                let color = w.nodes[p.node]
                    .children
                    .first()
                    .and_then(|&c| w.nodes[c].sprite.as_ref())
                    .map(|s| s.color)
                    .unwrap_or(tr.color);
                let mut pts: Vec<V3> = p.trail.iter().map(|(q, _)| *q).collect();
                pts.push(w.position(p.node));
                let smooth = catmull_rom(&pts, 4);
                if smooth.len() < 2 {
                    continue;
                }
                let d = dist(w.position(p.node)) + 0.01;
                let n = smooth.len();
                for k in 0..n - 1 {
                    let f0 = k as f32 / (n - 1) as f32;
                    let f1 = (k + 1) as f32 / (n - 1) as f32;
                    let seg = [smooth[k], smooth[k + 1]];
                    // taper from the blob's own width (~0.4) to nothing
                    let head = 0.4f32.min(tr.width[0]);
                    let w0 = head * f0 * f0;
                    let w1 = head * f1 * f1;
                    let mut glow = color;
                    glow[3] = 0.22 * f1;
                    let mut core = color;
                    core[3] = 0.75 * f1 * f1;
                    ribbon_tapered(&seg, [w0 * 1.5, w1 * 1.5], glow, soft, |rec| items.push(Item { order: -1, dist: d, seq: p.node, rec }));
                    ribbon_tapered(&seg, [w0 * 0.7, w1 * 0.7], core, soft, |rec| items.push(Item { order: -1, dist: d - 0.001, seq: p.node, rec }));
                }
            }
            // dash cooldown rings, dash afterimages and the charging-box effect
            let disc = tex_named(w, "BlobBW.png");
            let ring = tex_named(w, "Outer.png");
            for (pk, p) in self.players.iter().enumerate() {
                if p.playback || !w.active_in_hierarchy(p.node) {
                    continue;
                }
                let pos = w.position(p.node);
                if pos.x == -666.0 {
                    continue;
                }
                let color = w.nodes[p.node]
                    .children
                    .first()
                    .and_then(|&c| w.nodes[c].sprite.as_ref())
                    .map(|s| s.color)
                    .unwrap_or([0.2, 0.2, 0.2, 1.0]);
                let d = dist(pos);
                // orbit-aim reticle (pointer-locked mouse / aim stick): always visible
                if let Some(slot) = self.direct_slot_pub(p) {
                    let ctl = self.input.direct[slot];
                    if ctl.active && !ctl.use_cursor && ctl.aim.len() > 0.05 && ring >= 0.0 {
                        let a = ctl.aim.normalized();
                        let c0 = pos + v3(a.x, a.y, 0.0) * 1.3;
                        let rad = if ctl.aiming { 0.14 } else { 0.11 };
                        let mut c = color;
                        c[3] = if ctl.aiming { 0.9 } else { 0.55 };
                        let q = [c0 + v3(-rad, -rad, 0.0), c0 + v3(rad, -rad, 0.0), c0 + v3(rad, rad, 0.0), c0 + v3(-rad, rad, 0.0)];
                        items.push(Item { order: 4, dist: d - 0.01, seq: p.node, rec: quad(ring, q, [0.0, 0.0, 1.0, 1.0], c) });
                    }
                    // cursor aim (League-style): a mark on the exact point under the
                    // cursor; the black aim line carries the direction
                    if ctl.active && ctl.use_cursor && ring >= 0.0 {
                        if let Some(cw) = self.cursor_world(pk) {
                            let to = v3(cw.x - pos.x, cw.y - pos.y, 0.0);
                            let len = (to.x * to.x + to.y * to.y).sqrt();
                            if len > 0.35 {
                                let mark = v3(cw.x, cw.y, pos.z);
                                let rad = if ctl.aiming { 0.17 } else { 0.13 };
                                let mut c = color;
                                c[3] = if ctl.aiming { 0.9 } else { 0.6 };
                                let q = [mark + v3(-rad, -rad, 0.0), mark + v3(rad, -rad, 0.0), mark + v3(rad, rad, 0.0), mark + v3(-rad, rad, 0.0)];
                                items.push(Item { order: 4, dist: d - 0.01, seq: p.node, rec: quad(ring, q, [0.0, 0.0, 1.0, 1.0], c) });
                            }
                        }
                    }
                }
                if p.dash_cd > 0 {
                    let f = 1.0 - p.dash_cd as f32 / crate::game::DASH_COOLDOWN as f32;
                    let n = 32;
                    let shown = ((n as f32) * f).round() as usize;
                    let r = 0.3;
                    let pts: Vec<V3> = (0..=shown)
                        .map(|k| {
                            let a = std::f32::consts::FRAC_PI_2 - k as f32 / n as f32 * std::f32::consts::TAU;
                            pos + v3(a.cos() * r, a.sin() * r, 0.0)
                        })
                        .collect();
                    if pts.len() >= 2 {
                        let mut c = color;
                        c[3] = 0.55;
                        ribbon(&pts, 0.035, c, -1.0, |rec| items.push(Item { order: 2, dist: d, seq: p.node, rec }));
                    }
                }
                if p.dash_t > 0 && disc >= 0.0 {
                    for g in 1..=4 {
                        let back = pos - v3(p.dash_dir.x, p.dash_dir.y, 0.0) * (g as f32 * 0.22);
                        let rad = 0.2 - g as f32 * 0.02;
                        let mut c = color;
                        c[3] = 0.35 - g as f32 * 0.07;
                        let q = [back + v3(-rad, -rad, 0.0), back + v3(rad, -rad, 0.0), back + v3(rad, rad, 0.0), back + v3(-rad, rad, 0.0)];
                        items.push(Item { order: -1, dist: d + 0.02, seq: p.node, rec: quad(disc, q, [0.0, 0.0, 1.0, 1.0], c) });
                    }
                }
                // the charger: an ink blade along the aim whose length is how far
                // this throw would still kill (stopping where the line meets
                // something), and a tapered arc winding round the soul as the
                // charge fills - closing, breathing and crackling when full
                if let (Some(_), false) = (p.picked_box, p.touch[1].is_zero()) {
                    let off = lerp.get(&p.node).copied().unwrap_or(V3::ZERO) + self.bodies.get(pk).map(|b| b.off).unwrap_or(V3::ZERO);
                    let c = pos + off;
                    let f = (p.charge_timer / 90.0).clamp(0.0, 1.0);
                    let t = self.motion.render_now;
                    let a = v3(p.touch[1].x, p.touch[1].y, 0.0);
                    let power = (p.charge_timer * 1.11).max(20.0).min(100.0);
                    let reach = power / 5.0 * 0.94;
                    let hit = (p.last_hit - c).length();
                    let len = reach.min(if hit > 0.3 { hit } else { reach });
                    let on_soul = p.aim_form[0];
                    let root = c + a * 0.32;
                    let tip = c + a * len.max(0.5);
                    let ink = |al: f32| [0.04, 0.04, 0.06, al];
                    let wd = 0.03 + 0.03 * f + 0.012 * on_soul;
                    crate::render::blade(root, tip, 0.0, wd, 0.08, ink(0.35 + 0.5 * f), |rec| items.push(Item { order: 4, dist: d - 0.004, seq: p.node, rec }));
                    // a hairline on past the reach to whatever the line meets
                    if hit > len + 0.2 {
                        let far = c + a * hit;
                        ribbon(&[tip, far], 0.006, ink(0.12), -1.0, |rec| items.push(Item { order: 4, dist: d - 0.004, seq: p.node, rec }));
                    }
                    // where it strikes a soul: a small broken ring
                    if on_soul > 0.5 && hit <= reach {
                        let m = c + a * hit;
                        for s in 0..3 {
                            let a0 = s as f32 * 2.1 + t * 1.5;
                            let pts: Vec<V3> = (0..=6).map(|j| { let q = a0 + j as f32 * 0.17; m + v3(q.cos(), q.sin(), 0.0) * 0.26 }).collect();
                            ribbon(&pts, 0.016, ink(0.55 * on_soul), -1.0, |rec| items.push(Item { order: 4, dist: d - 0.005, seq: p.node, rec }));
                        }
                    }
                    if f > 0.0 {
                        // the winding arc: thin at its start, full where it is being drawn
                        let r = 0.36 + 0.02 * (t * 3.0).sin() * f;
                        let n = (40.0 * f).ceil() as usize;
                        let start = std::f32::consts::FRAC_PI_2;
                        let pts: Vec<V3> = (0..=n)
                            .map(|j| {
                                let q = start - (j as f32 / 40.0) * std::f32::consts::TAU;
                                c + v3(q.cos(), q.sin(), 0.0) * r
                            })
                            .collect();
                        for j in 0..pts.len().saturating_sub(1) {
                            let u0 = j as f32 / n.max(1) as f32;
                            let u1 = (j + 1) as f32 / n.max(1) as f32;
                            let mut col = color;
                            col[3] = 0.9;
                            ribbon_tapered(&pts[j..=j + 1], [0.012 + 0.03 * u0, 0.012 + 0.03 * u1], col, -1.0, |rec| items.push(Item { order: 4, dist: d - 0.003, seq: p.node, rec }));
                        }
                        if f >= 1.0 {
                            // full: it breathes, and a hair of electricity flickers off it
                            let ph = (t * 2.2).fract();
                            let mut col = color;
                            col[3] = 0.35 * (1.0 - ph);
                            let rr = r + 0.08 * ph;
                            let ring_pts: Vec<V3> = (0..=40).map(|j| { let q = j as f32 / 40.0 * std::f32::consts::TAU; c + v3(q.cos(), q.sin(), 0.0) * rr }).collect();
                            ribbon(&ring_pts, 0.012, col, -1.0, |rec| items.push(Item { order: 4, dist: d - 0.003, seq: p.node, rec }));
                            let fl = (t * 24.0) as u32;
                            let hs = crate::motion::hash(fl ^ (pk as u32 * 911));
                            if hs % 3 == 0 {
                                let q = (hs % 628) as f32 / 100.0;
                                let from = c + v3(q.cos(), q.sin(), 0.0) * r;
                                let to = c + v3((q + 0.5).cos(), (q + 0.5).sin(), 0.0) * (r + 0.18);
                                crate::render::blade(from, to, 0.2, 0.02, 0.035, ink(0.8), |rec| items.push(Item { order: 4, dist: d - 0.003, seq: p.node, rec }));
                            }
                        }
                    }
                }
            }
            // motion graphics in the world
            let tx = crate::motion::Tex { disc, soft, ring };
            self.motion.emit(false, &tx, |p| dist(p), |rec, d| items.push(Item { order: 6, dist: d, seq: 0, rec }));
            lap(11);
            self.shard_items(|p| dist(p), |rec, d| items.push(Item { order: 4, dist: d, seq: 0, rec }));
            lap(14);
            if w.nodes[cam].tag == "MainCamera" {
                self.arena_hum(|p| dist(p), |rec, d| items.push(Item { order: 1, dist: d, seq: 0, rec }));
            }
            lap(15);
            // sort small keys, not the 112-byte records; the index keeps it stable
            let mut keys: Vec<(i32, f32, usize, u32)> = items.iter().enumerate().map(|(k, it)| (it.order, it.dist, it.seq, k as u32)).collect();
            keys.sort_unstable_by(|a, b| a.0.cmp(&b.0).then(b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)).then(a.2.cmp(&b.2)).then(a.3.cmp(&b.3)));
            out.reserve(items.len() * RECORD);
            for (_, _, _, k) in keys {
                out.extend_from_slice(&items[k as usize].rec);
            }
            lap(12);
        }
        // overlay canvases in screen pixels
        let mut head = [0.0f32; RECORD];
        head[0] = 9.0;
        head[4..20].copy_from_slice(&ortho_screen(self.screen.w, self.screen.h).0);
        out.extend_from_slice(&head);
        for &root in &self.layout.overlays {
            let mut nodes = Vec::new();
            w.descendants(root, &mut nodes);
            for i in nodes {
                if !w.active_in_hierarchy(i) {
                    continue;
                }
                let m = self.layout.matrix(w, i);
                let r = self.layout.rects.get(&i).copied().unwrap_or([0.0; 4]);
                let corners = [m.point(v3(r[0], r[1], 0.0)), m.point(v3(r[2], r[1], 0.0)), m.point(v3(r[2], r[3], 0.0)), m.point(v3(r[0], r[3], 0.0))];
                self.ui_items(i, corners, &m, |rec| out.extend_from_slice(&rec));
            }
        }
        let tx = crate::motion::Tex { disc: tex_named(w, "BlobBW.png"), soft: tex_named(w, "BlobBWShadow.png"), ring: tex_named(w, "Outer.png") };
        self.motion.emit(true, &tx, |_| 0.0, |rec, _| out.extend_from_slice(&rec));
        self.menu_glyphs(|rec| out.extend_from_slice(&rec));
        self.cursor_spark(&tx, |rec| out.extend_from_slice(&rec));
        lap(13);
        self.prof.borrow_mut().end(10..16, false);
    }

    fn ui_items(&self, i: usize, c: [V3; 4], m: &Mat4, mut push: impl FnMut([f32; RECORD])) {
        let n = &self.w.nodes[i];
        let form = self.form_of(i);
        if form.map(|f| f.1 <= 0.0).unwrap_or(false) {
            return;
        }
        if let Some(img) = n.image.as_ref().filter(|g| g.enabled && g.color[3] * g.tint[3] > 0.0) {
            let color = [img.color[0] * img.tint[0], img.color[1] * img.tint[1], img.color[2] * img.tint[2], img.color[3] * img.tint[3]];
            let (tex, mut uv) = match img.sprite.map(|k| &self.w.sprites[k]) {
                Some(sp) => (sp.tex as f32, [sp.uv[0], sp.uv[1], sp.uv[0] + sp.uv[2], sp.uv[1] + sp.uv[3]]),
                None => (-1.0, [0.0, 0.0, 1.0, 1.0]),
            };
            let mut p = c;
            let mut color = color;
            if let Some((k, a, _)) = form {
                let mid = (c[0] + c[1] + c[2] + c[3]) * 0.25;
                p = p.map(|q| mid + (q - mid) * k);
                color[3] *= a;
            }
            if img.kind == 3 {
                // Filled: shrink along x (horizontal) or y (vertical); radial fills shrink in x too
                let f = img.fill.clamp(0.0, 1.0);
                if f <= 0.0 {
                    return;
                }
                if img.fill_method == 1 {
                    p[2] = p[1] + (p[2] - p[1]) * f;
                    p[3] = p[0] + (p[3] - p[0]) * f;
                    uv[3] = uv[1] + (uv[3] - uv[1]) * f;
                } else {
                    p[1] = p[0] + (p[1] - p[0]) * f;
                    p[2] = p[3] + (p[2] - p[3]) * f;
                    uv[2] = uv[0] + (uv[2] - uv[0]) * f;
                }
            }
            push(quad(tex, p, uv, color));
        }
        if let Some(t) = n.text.as_ref().filter(|t| t.enabled && t.color[3] * t.tint[3] > 0.0 && !t.text.is_empty()) {
            let unit = v3(m.0[4], m.0[5], m.0[6]).length();
            let mut r = [0.0; RECORD];
            r[0] = 1.0;
            r[1] = i as f32;
            r[2] = t.size * unit;
            r[3] = t.align as f32;
            for k in 0..4 {
                r[4 + k * 3] = c[k].x;
                r[5 + k * 3] = c[k].y;
                r[6 + k * 3] = c[k].z;
            }
            r[16] = t.style as f32;
            r[17] = t.spacing * t.size / 100.0 * unit;
            // letters write themselves in (reveal), then hum
            r[18] = form.map(|f| f.2).unwrap_or(1.0);
            r[19] = 1.0 + i as f32;
            r[20..24].copy_from_slice(&[t.color[0] * t.tint[0], t.color[1] * t.tint[1], t.color[2] * t.tint[2], t.color[3] * t.tint[3]]);
            push(r);
        }
    }
}


/// A jagged lightning polyline from a to b (deterministic for a given seed).
pub(crate) fn arc_points(a: V3, b: V3, segments: usize, jitter: f32, mut seed: u32) -> Vec<V3> {
    let d = b - a;
    let len = (d.x * d.x + d.y * d.y).sqrt().max(1e-6);
    let nrm = v3(-d.y / len, d.x / len, 0.0);
    let mut out = Vec::with_capacity(segments + 1);
    for k in 0..=segments {
        let t = k as f32 / segments as f32;
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let r = (seed as f32 / u32::MAX as f32) * 2.0 - 1.0;
        let envelope = (t * std::f32::consts::PI).sin(); // pinned at both shards
        out.push(a + d * t + nrm * (r * jitter * len.min(2.0) * envelope));
    }
    out
}

/// Constant-width ribbon along a polyline.
pub(crate) fn ribbon(pts: &[V3], width: f32, color: [f32; 4], tex: f32, mut push: impl FnMut([f32; RECORD])) {
    for k in 0..pts.len() - 1 {
        ribbon_tapered(&pts[k..k + 2], [width, width], color, tex, &mut push);
    }
}

/// One ribbon segment with per-end widths; v runs across the ribbon so a soft
/// radial texture sampled at u = 0.5 gives feathered edges.
pub(crate) fn ribbon_tapered(seg: &[V3], width: [f32; 2], color: [f32; 4], tex: f32, mut push: impl FnMut([f32; RECORD])) {
    let (a, b) = (seg[0], seg[1]);
    let d = b - a;
    let len = (d.x * d.x + d.y * d.y).sqrt();
    if len < 1e-5 || color[3] <= 0.0 {
        return;
    }
    let nrm = v3(-d.y / len, d.x / len, 0.0);
    let (wa, wb) = (width[0] / 2.0, width[1] / 2.0);
    let p = [a - nrm * wa, b - nrm * wb, b + nrm * wb, a + nrm * wa];
    let uv = if tex < 0.0 { [0.0, 0.0, 1.0, 1.0] } else { [0.5, 0.0, 0.5, 1.0] };
    push(quad(tex, p, uv, color));
}

/// Catmull-Rom resampling for smooth trails.
fn catmull_rom(p: &[V3], steps: usize) -> Vec<V3> {
    if p.len() < 3 {
        return p.to_vec();
    }
    let mut out = Vec::new();
    for i in 0..p.len() - 1 {
        let p0 = p[i.saturating_sub(1)];
        let p1 = p[i];
        let p2 = p[i + 1];
        let p3 = p[(i + 2).min(p.len() - 1)];
        for s in 0..steps {
            let t = s as f32 / steps as f32;
            let t2 = t * t;
            let t3 = t2 * t;
            out.push((p1 * 2.0 + (p2 - p0) * t + (p0 * 2.0 - p1 * 5.0 + p2 * 4.0 - p3) * t2 + (p1 * 3.0 - p0 - p2 * 3.0 + p3) * t3) * 0.5);
        }
    }
    out.push(*p.last().unwrap());
    out
}

fn tex_named(w: &crate::scene::World, file: &str) -> f32 {
    w.textures.iter().position(|t| t.1.ends_with(file)).map(|t| t as f32).unwrap_or(-1.0)
}

impl Game {
    fn form_of(&self, i: usize) -> Option<(f32, f32, f32)> {
        self.motion.form(i)
    }

    /// The map menu's arrows, drawn as live strokes: they breathe, lean
    /// toward the cursor on hover with an echo and a crackle, jab when
    /// pressed, and go quiet at the end of the row.
    fn menu_glyphs(&self, mut push: impl FnMut([f32; RECORD])) {
        let now = self.motion.render_now;
        let flick = (now * 30.0) as u32;
        for &(n, kind) in &self.motion.glyphs {
            if !self.w.active_in_hierarchy(n) {
                continue;
            }
            let Some(q) = self.ui_screen_quad(n) else { continue };
            let c = v3((q[0][0] + q[2][0]) / 2.0, (q[0][1] + q[2][1]) / 2.0, 0.0);
            let s = ((q[1][0] - q[0][0]).abs().min((q[3][1] - q[0][1]).abs()) * 0.3).clamp(10.0, 60.0);
            let cm = self.menu.current_map;
            let enabled = match kind {
                0 => cm > 0,
                1 => cm < 3,
                _ => true,
            };
            // forms in with its menu
            let (fk, fa) = self.motion.form(n).map(|f| (f.0, f.1)).unwrap_or((1.0, 1.0));
            if fa <= 0.0 {
                continue;
            }
            let hovered = self.motion.hover == Some(n) && enabled;
            let h = if hovered { crate::motion::ease_out(((now - self.motion.hover_since) / 0.15).min(1.0)) } else { 0.0 };
            let dir = match kind {
                0 => v3(-1.0, 0.0, 0.0),
                1 => v3(1.0, 0.0, 0.0),
                _ => v3(-1.0, 0.0, 0.0),
            };
            // press jab: a damped kick in the arrow's direction
            let jab = self
                .motion
                .pressed_at
                .get(&n)
                .map(|&t0| {
                    let t = now - t0;
                    if t < 0.0 || t > 0.6 {
                        0.0
                    } else {
                        (t * 22.0).sin() * (-t * 9.0).exp()
                    }
                })
                .unwrap_or(0.0);
            let breathe = (now * 2.2 + kind as f32).sin() * 0.05;
            let shift = dir * (s * (breathe + 0.18 * h + 0.6 * jab));
            let alpha = if enabled { 0.92 } else { 0.18 } * fa;
            let width = 2.2 + 1.2 * h;
            let shape = |o: V3, k: f32| -> Vec<V3> {
                let sk = s * k * fk;
                match kind {
                    0 | 1 => {
                        let sx = if kind == 0 { 1.0 } else { -1.0 };
                        vec![o + v3(sx * sk * 0.5, sk, 0.0), o + v3(-sx * sk * 0.5, 0.0, 0.0), o + v3(sx * sk * 0.5, -sk, 0.0)]
                    }
                    _ => {
                        // a return arrow: shaft, a half turn up the right side, a short tail
                        let mut v = vec![o + v3(-sk, -sk * 0.3, 0.0), o + v3(sk * 0.3, -sk * 0.3, 0.0)];
                        for j in 1..=10 {
                            let a = -std::f32::consts::FRAC_PI_2 + j as f32 / 10.0 * std::f32::consts::PI;
                            v.push(o + v3(sk * 0.3 + a.cos() * sk * 0.45, sk * 0.15 + a.sin() * sk * 0.45, 0.0));
                        }
                        v.push(o + v3(sk * 0.05, sk * 0.6, 0.0));
                        v
                    }
                }
            };
            let ink = [0.04, 0.04, 0.06, alpha];
            let main = shape(c + shift, 1.0);
            ribbon(&main, width, ink, -1.0, &mut push);
            if kind == 2 {
                let tip = main[0];
                let sk = s * fk;
                ribbon(&[tip + v3(sk * 0.35, sk * 0.35, 0.0), tip, tip + v3(sk * 0.35, -sk * 0.35, 0.0)], width, ink, -1.0, &mut push);
            }
            if h > 0.0 || jab.abs() > 0.05 {
                // the echo: a fainter twin running ahead
                let mut ghost = ink;
                ghost[3] = alpha * 0.3 * h.max(jab.abs());
                let echo = shape(c + shift + dir * (s * 0.45), 0.85);
                ribbon(&echo, width * 0.7, ghost, -1.0, &mut push);
                // and a small crackle off its point
                let tip = main[main.len() / 2];
                let hs = crate::motion::hash(flick ^ n as u32);
                if hs % 3 != 0 {
                    let end = tip + dir * (s * 0.55) + v3(0.0, crate::motion::hf(hs) * s * 0.35, 0.0);
                    let pts = arc_points(tip, end, 4, 0.35, hs);
                    let mut cc = ink;
                    cc[3] = 0.6 * h.max(jab.abs());
                    ribbon(&pts, 1.0, cc, -1.0, &mut push);
                }
            }
        }
    }

    /// The cursor is a small spark with a tail; over a button it is tethered
    /// to it by a live arc.
    fn cursor_spark(&self, tx: &crate::motion::Tex, mut push: impl FnMut([f32; RECORD])) {
        let Some(c) = self.input.cursor else { return };
        if c[0] < 0.0 {
            return;
        }
        let now = self.motion.render_now;
        let ink = [0.04, 0.04, 0.06, 0.9];
        let tip = v3(c[0], c[1], 0.0);
        let trail: Vec<V3> = self.motion.cursor_trail.iter().map(|p| v3(p[0], p[1], 0.0)).chain(std::iter::once(tip)).collect();
        for k in 1..trail.len() {
            let f0 = (k - 1) as f32 / trail.len() as f32;
            let f1 = k as f32 / trail.len() as f32;
            let mut col = ink;
            col[3] = 0.5 * f1;
            ribbon_tapered(&trail[k - 1..=k], [2.6 * f0, 2.6 * f1], col, -1.0, &mut push);
        }
        let breathe = 1.0 + 0.15 * (now * 5.0).sin();
        let sz = 3.4 * breathe;
        if tx.disc >= 0.0 {
            push(quad(tx.disc, [tip + v3(-sz, -sz, 0.0), tip + v3(sz, -sz, 0.0), tip + v3(sz, sz, 0.0), tip + v3(-sz, sz, 0.0)], [0.0, 0.0, 1.0, 1.0], ink));
        }
        let flick = (now * 30.0) as u32;
        // two tiny crackles around the tip
        for j in 0..2u32 {
            let s = crate::motion::hash(flick ^ (j * 911));
            if s % 3 == 0 {
                continue;
            }
            let a = (s % 628) as f32 / 100.0;
            let end = tip + v3(a.cos(), a.sin(), 0.0) * 11.0;
            let pts = arc_points(tip, end, 4, 0.45, s);
            let mut col = ink;
            col[3] = 0.6;
            ribbon(&pts, 1.0, col, -1.0, &mut push);
        }
        if let Some(b) = self.motion.hover {
            if let Some(q) = self.ui_screen_quad(b) {
                let mid = v3((q[0][0] + q[2][0]) / 2.0, (q[0][1] + q[2][1]) / 2.0, 0.0);
                // land on the button's edge, not its middle
                let (hw, hh) = (((q[1][0] - q[0][0]) / 2.0).abs(), ((q[3][1] - q[0][1]) / 2.0).abs());
                let d = tip - mid;
                let k = 1.0 / ((d.x.abs() / hw.max(1.0)).max(d.y.abs() / hh.max(1.0))).max(1.0);
                let land = mid + d * k;
                let s = crate::motion::hash(flick ^ 0xabc);
                let pts = arc_points(tip, land, 7, 0.22, s);
                let since = (now - self.motion.hover_since).max(0.0);
                let mut col = ink;
                col[3] = 0.35 + 0.25 * (since * 9.0).sin().abs();
                ribbon(&pts, 1.2, col, -1.0, &mut push);
            }
        }
    }
}

/// A katana blade: a filled quadratic curve that bows sideways by `bow` x
/// its length, thick at the root and tapering to a point.
pub(crate) fn blade(a: V3, b: V3, bow: f32, w: f32, cap: f32, color: [f32; 4], mut push: impl FnMut([f32; RECORD])) {
    let w = w.min(cap);
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let l = (dx * dx + dy * dy).sqrt().max(1e-4);
    let (nx, ny) = (-dy / l, dx / l);
    let c = v3((a.x + b.x) / 2.0 + nx * bow * l, (a.y + b.y) / 2.0 + ny * bow * l, (a.z + b.z) / 2.0);
    const N: usize = 18;
    let mut prev: Option<(V3, V3)> = None;
    for k in 0..=N {
        let u = k as f32 / N as f32;
        let (p0, p1, p2) = ((1.0 - u) * (1.0 - u), 2.0 * (1.0 - u) * u, u * u);
        let p = a * p0 + c * p1 + b * p2;
        let t = (c - a) * (2.0 * (1.0 - u)) + (b - c) * (2.0 * u);
        let tl = (t.x * t.x + t.y * t.y).sqrt().max(1e-6);
        let hw = w * (u * 10.0).min(1.0) * (1.0 - u).powf(0.8) / 2.0;
        let off = v3(-t.y / tl * hw, t.x / tl * hw, 0.0);
        let (lft, rgt) = (p + off, p - off);
        if let Some((pl, pr)) = prev {
            push(quad(-1.0, [pl, lft, rgt, pr], [0.0, 0.0, 1.0, 1.0], color));
        }
        prev = Some((lft, rgt));
    }
}

impl Game {
    /// Shard Lab: ink shards, their rave cascades, bonds and bolts.
    fn shard_items(&self, dist: impl Fn(V3) -> f32, mut push: impl FnMut([f32; RECORD], f32)) {
        use crate::shards::{unkey, ARC_W, DROPOUT, FADE_T, HOP, MAX_LINK_DIST, MAX_W};
        let sim = &self.shards;
        if sim.shards.is_empty() {
            return;
        }
        const INK: [f32; 3] = [0.04, 0.04, 0.06];
        let ink = |a: f32| [INK[0], INK[1], INK[2], a.clamp(0.0, 1.0)];
        let min_w = 0.012;
        let ahead = (self.motion.render_now - self.motion.now).clamp(0.0, 0.02);
        let pos = |i: usize| {
            let s = &sim.shards[i];
            v3(s.x + s.vx * ahead, s.y + s.vy * ahead, s.z)
        };
        let t = sim.t;
        let h = |x: u32| crate::motion::hash(x);
        // motion streaks behind fast shards
        for (i, p) in sim.shards.iter().enumerate() {
            let sp = (p.vx * p.vx + p.vy * p.vy).sqrt();
            if p.dead || sp < 1.0 {
                continue;
            }
            let a = pos(i);
            let b = a - v3(p.vx, p.vy, 0.0) * 0.05;
            ribbon(&[b, a], 0.012, ink((sp / 40.0).min(0.35)), -1.0, |rec| push(rec, dist(a)));
        }
        // rave cascades (within each impact)
        for imp in &sim.imps {
            for l in &imp.lines {
                if !l.show || sim.shards[l.a].dead || sim.shards[l.b].dead {
                    continue;
                }
                let k = (t - l.born) as f32;
                let life = l.life as f32;
                let half = life / 2.0;
                let al = if l.snap { if k < 1.0 { 1.0 } else { (1.0 - k / life).max(0.0).sqrt() } } else if k < half { (k + 1.0) / half } else { (life - k) / half };
                let (a, b) = (pos(l.a), pos(l.b));
                blade(a, b, l.bow, (ARC_W * 1.6 * l.w).max(min_w), MAX_W, ink(al * 0.85), |rec| push(rec, dist(a)));
            }
        }
        // bonds: hidden in 9-tick dropout epochs; an endpoint hops about
        for &k in &sim.active {
            if (h(k ^ (t / 9) as u32).wrapping_mul(2654435761) % 1000) as f32 / 1000.0 < DROPOUT {
                continue;
            }
            let v = sim.vis.get(&k).copied().unwrap_or(0.0);
            if v <= 0.0 {
                continue;
            }
            let s0 = sim.links.get(&k).copied().unwrap_or(0.0);
            let (i, j) = unkey(k);
            let glow = (0.35 + s0 * 1.5).min(1.0) * v;
            let (mut p, mut q) = (i, j);
            let hk = h(k ^ (t as u32).wrapping_mul(747796405));
            if (hk % 1000) as f32 / 1000.0 < HOP {
                let a = &sim.shards[i];
                let near: Vec<usize> = (0..sim.shards.len())
                    .filter(|&o| o != i && !sim.shards[o].dead && sim.shards[o].fade < 0.0 && ((sim.shards[o].x - a.x).powi(2) + (sim.shards[o].y - a.y).powi(2)).sqrt() < MAX_LINK_DIST)
                    .collect();
                if !near.is_empty() {
                    q = near[(hk / 1000) as usize % near.len()];
                }
                if hk & 1 == 1 {
                    std::mem::swap(&mut p, &mut q);
                }
            }
            let bow = ((k % 7) as f32 / 7.0 - 0.5) * 0.5 + if k % 2 == 1 { 0.1 } else { -0.1 };
            let (a, b) = (pos(p), pos(q));
            blade(a, b, bow, (ARC_W * (1.1 + 0.5 * s0)).max(min_w), MAX_W, ink(0.15 + glow * 0.75), |rec| push(rec, dist(a)));
        }
        // breakdown bolts
        for bo in &sim.bolts {
            let (a, b) = (pos(bo.a), pos(bo.b));
            blade(a, b, 0.15, ARC_W * 1.6, MAX_W, ink(bo.life as f32 / 7.0), |rec| push(rec, dist(a)));
        }
        // the shards themselves
        for (i, p) in sim.shards.iter().enumerate() {
            if p.dead {
                continue;
            }
            let life = if p.fade >= 0.0 { 1.0 - p.fade / FADE_T } else { 1.0 };
            let shiver = if p.gasp >= 0.0 { 1.0 + 0.15 * (p.gasp * 60.0).sin() } else { 1.0 };
            let hs = p.size * (0.5 + 0.5 * life) * shiver;
            let c = pos(i);
            let (sn, cs) = (p.a + p.w * ahead).sin_cos();
            let pt = |q: [f32; 2]| c + v3((q[0] * cs - q[1] * sn) * hs, (q[0] * sn + q[1] * cs) * hs, 0.0);
            let (a, b, cc) = (pt(p.tri[0]), pt(p.tri[1]), pt(p.tri[2]));
            push(quad(-1.0, [a, b, cc, cc], [0.0, 0.0, 1.0, 1.0], [0.067, 0.067, 0.067, life * life]), dist(c) - 0.002);
        }
    }
}

impl Game {
    /// In a lull the arena holds its breath: faint ink outlines on the
    /// obstacles, a slow wave running round each.
    fn arena_hum(&self, dist: impl Fn(V3) -> f32, mut push: impl FnMut([f32; RECORD], f32)) {
        let idle = ((self.motion.now - self.motion.last_action - 1.5) / 2.0).clamp(0.0, 1.0);
        if idle <= 0.0 || self.players.iter().all(|p| !self.w.active_in_hierarchy(p.node)) {
            return;
        }
        let t = self.motion.render_now;
        let Some(cam) = self.camera_main() else { return };
        let cp = self.w.position(cam);
        for i in 0..self.w.nodes.len() {
            let n = &self.w.nodes[i];
            let Some(s) = n.sprite.as_ref().filter(|s| s.enabled) else { continue };
            if n.collider.is_none() || self.player_of.contains_key(&i) || self.layout.canvas_of.contains_key(&i) {
                continue;
            }
            if s.sprite.and_then(|k| self.w.sprites[k].vector.as_ref()).is_none() || !self.w.active_in_hierarchy(i) {
                continue;
            }
            let c = self.w.position(i);
            if (c.x - cp.x).abs() > 22.0 || (c.y - cp.y).abs() > 14.0 {
                continue;
            }
            let Some(rings) = self.outline_world(i) else { continue };
            // float just outside the shape (ink on ink would vanish), breathing
            let swell = 0.06 + 0.02 * (t * 1.1 + i as f32).sin();
            let ring: Vec<V3> = rings[0]
                .iter()
                .map(|&q| {
                    let d = q - c;
                    let l = (d.x * d.x + d.y * d.y).sqrt().max(1e-4);
                    q + v3(d.x / l, d.y / l, 0.0) * swell
                })
                .collect();
            let m = ring.len() - 1;
            for k in (0..m).step_by(2) {
                let wave = 0.5 + 0.5 * ((k as f32 / m as f32) * std::f32::consts::TAU * 2.0 - t * 1.4 + i as f32 * 0.7).sin();
                let a = 0.24 * idle * wave * wave;
                if a < 0.008 {
                    continue;
                }
                ribbon(&ring[k..=(k + 1).min(m)], 0.018, [0.04, 0.04, 0.06, a], -1.0, |rec| push(rec, dist(c) - 0.01));
            }
        }
    }
}
