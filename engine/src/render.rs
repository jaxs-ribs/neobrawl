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

fn quad(tex: f32, p: [V3; 4], uv: [f32; 4], color: [f32; 4]) -> [f32; RECORD] {
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
        out.clear();
        let w = &self.w;
        let cams: Vec<usize> = (0..w.nodes.len()).filter(|&i| w.nodes[i].camera.is_some() && w.active_in_hierarchy(i)).collect();
        let ui_order = self.ui_draw_order();
        let ui_seq: std::collections::HashMap<usize, usize> = ui_order.iter().enumerate().map(|(k, &n)| (n, k)).collect();
        for &cam in &cams {
            let c = w.nodes[cam].camera.as_ref().unwrap();
            let cm = w.matrix(cam);
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
            for i in 0..w.nodes.len() {
                let n = &w.nodes[i];
                let ui = self.layout.canvas_of.get(&i).copied();
                if let Some((_, 0)) = ui {
                    continue; // overlay canvases draw in the screen pass
                }
                let interesting = n.sprite.is_some() || n.mesh.is_some() || n.line.is_some() || n.image.is_some() || n.text.is_some();
                if !interesting || !w.active_in_hierarchy(i) {
                    continue;
                }
                let m = if ui.is_some() { self.layout.matrix(w, i) } else { w.matrix(i) };
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
                        let p = [m.point(v3(q[0], q[1], 0.0)), m.point(v3(q[2], q[1], 0.0)), m.point(v3(q[2], q[3], 0.0)), m.point(v3(q[0], q[3], 0.0))];
                        let uv = [sp.uv[0], sp.uv[1], sp.uv[0] + sp.uv[2], sp.uv[1] + sp.uv[3]];
                        let center = m.point(V3::ZERO);
                        let (seq, d) = match ui {
                            Some(_) => (ui_seq.get(&i).copied().unwrap_or(0), dist(center)),
                            None => (i, dist(center)),
                        };
                        items.push(Item { order: s.order, dist: d, seq, rec: quad(sp.tex as f32, p, uv, s.color) });
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
            for p in &self.players {
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
                if let (Some(b), true) = (p.picked_box, p.charge_timer > 0.0) {
                    let bp = w.position(b);
                    let f = (p.charge_timer / 90.0).clamp(0.0, 1.0);
                    let t = self.frame as f32 * 0.02;
                    // full charge: the box trembles
                    let shake = if f >= 1.0 { v3((t * 91.0).sin() * 0.025, (t * 77.0).cos() * 0.025, 0.0) } else { V3::ZERO };
                    let c0 = bp + shake;
                    let bd = dist(c0) - 0.05;
                    // core swelling inside the box, in the thrower's colour
                    if disc >= 0.0 {
                        let rad = 0.05 + 0.11 * f + 0.015 * (t * (6.0 + 18.0 * f)).sin();
                        let mut c = color;
                        c[3] = 0.55 + 0.45 * f;
                        let q = [c0 + v3(-rad, -rad, 0.0), c0 + v3(rad, -rad, 0.0), c0 + v3(rad, rad, 0.0), c0 + v3(-rad, rad, 0.0)];
                        items.push(Item { order: 3, dist: bd, seq: b, rec: quad(disc, q, [0.0, 0.0, 1.0, 1.0], c) });
                    }
                    // arcs crawling from the core to the box's edges, more as it charges
                    let arcs = 1 + (f * 5.0) as u32;
                    let seed = (b as u32).wrapping_mul(2246822519) ^ (self.frame as u32).wrapping_mul(3266489917);
                    for k in 0..arcs {
                        let a = (seed.wrapping_add(k * 2654435761) % 6283) as f32 / 1000.0;
                        let edge = c0 + v3(a.cos(), a.sin(), 0.0) * 0.26;
                        let pts = arc_points(c0, edge, 6, 0.25, seed.wrapping_add(k * 97));
                        ribbon(&pts, 0.018, [0.02, 0.02, 0.05, 0.85], -1.0, |rec| items.push(Item { order: 3, dist: bd - 0.001, seq: b, rec }));
                    }
                    // fully charged: a ring pulses outward from the box
                    if f >= 1.0 && ring >= 0.0 {
                        let ph = (t * 2.5).fract();
                        let rad = 0.3 + 0.35 * ph;
                        let mut c = color;
                        c[3] = 0.6 * (1.0 - ph);
                        let q = [c0 + v3(-rad, -rad, 0.0), c0 + v3(rad, -rad, 0.0), c0 + v3(rad, rad, 0.0), c0 + v3(-rad, rad, 0.0)];
                        items.push(Item { order: 3, dist: bd - 0.002, seq: b, rec: quad(ring, q, [0.0, 0.0, 1.0, 1.0], c) });
                    }
                }
            }
            items.sort_by(|a, b| a.order.cmp(&b.order).then(b.dist.partial_cmp(&a.dist).unwrap_or(std::cmp::Ordering::Equal)).then(a.seq.cmp(&b.seq)));
            for it in items {
                out.extend_from_slice(&it.rec);
            }
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
    }

    fn ui_items(&self, i: usize, c: [V3; 4], m: &Mat4, mut push: impl FnMut([f32; RECORD])) {
        let n = &self.w.nodes[i];
        if let Some(img) = n.image.as_ref().filter(|g| g.enabled && g.color[3] * g.tint[3] > 0.0) {
            let color = [img.color[0] * img.tint[0], img.color[1] * img.tint[1], img.color[2] * img.tint[2], img.color[3] * img.tint[3]];
            let (tex, mut uv) = match img.sprite.map(|k| &self.w.sprites[k]) {
                Some(sp) => (sp.tex as f32, [sp.uv[0], sp.uv[1], sp.uv[0] + sp.uv[2], sp.uv[1] + sp.uv[3]]),
                None => (-1.0, [0.0, 0.0, 1.0, 1.0]),
            };
            let mut p = c;
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
            r[20..24].copy_from_slice(&[t.color[0] * t.tint[0], t.color[1] * t.tint[1], t.color[2] * t.tint[2], t.color[3] * t.tint[3]]);
            push(r);
        }
    }
}


/// A jagged lightning polyline from a to b (deterministic for a given seed).
fn arc_points(a: V3, b: V3, segments: usize, jitter: f32, mut seed: u32) -> Vec<V3> {
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
fn ribbon(pts: &[V3], width: f32, color: [f32; 4], tex: f32, mut push: impl FnMut([f32; RECORD])) {
    for k in 0..pts.len() - 1 {
        ribbon_tapered(&pts[k..k + 2], [width, width], color, tex, &mut push);
    }
}

/// One ribbon segment with per-end widths; v runs across the ribbon so a soft
/// radial texture sampled at u = 0.5 gives feathered edges.
fn ribbon_tapered(seg: &[V3], width: [f32; 2], color: [f32; 4], tex: f32, mut push: impl FnMut([f32; RECORD])) {
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
