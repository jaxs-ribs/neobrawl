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
                if let Some(l) = n.line.as_ref().filter(|l| l.enabled && l.points.len() >= 2) {
                    for k in 0..l.points.len() - 1 {
                        let (a, b) = if l.world { (l.points[k], l.points[k + 1]) } else { (m.point(l.points[k]), m.point(l.points[k + 1])) };
                        let d = b - a;
                        let len = (d.x * d.x + d.y * d.y).sqrt().max(1e-6);
                        let nrm = v3(-d.y / len, d.x / len, 0.0);
                        let (wa, wb) = (l.width[0] / 2.0, l.width[1] / 2.0);
                        let p = [a - nrm * wa, b - nrm * wb, b + nrm * wb, a + nrm * wa];
                        let col = l.colors[0];
                        if col[3] <= 0.0 {
                            continue;
                        }
                        items.push(Item { order: 0, dist: dist((a + b) * 0.5), seq: i, rec: quad(-1.0, p, [0.0, 0.0, 1.0, 1.0], col) });
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
            // trails
            for p in &self.players {
                let Some(tr) = w.nodes[p.node].trail.as_ref() else { continue };
                if !tr.enabled || p.trail.len() < 2 || !w.active_in_hierarchy(p.node) {
                    continue;
                }
                let n = p.trail.len();
                for k in 0..n - 1 {
                    let (a, b) = (p.trail[k].0, p.trail[k + 1].0);
                    let fa = k as f32 / (n - 1) as f32;
                    let fb = (k + 1) as f32 / (n - 1) as f32;
                    let wa = (tr.width[1] + (tr.width[0] - tr.width[1]) * fa) / 2.0;
                    let wb = (tr.width[1] + (tr.width[0] - tr.width[1]) * fb) / 2.0;
                    let d = b - a;
                    let len = (d.x * d.x + d.y * d.y).sqrt();
                    if len < 1e-4 {
                        continue;
                    }
                    let nrm = v3(-d.y / len, d.x / len, 0.0);
                    let mut col = tr.color;
                    col[3] *= fb * 0.6;
                    let q = [a - nrm * wa, b - nrm * wb, b + nrm * wb, a + nrm * wa];
                    items.push(Item { order: -1, dist: dist(a), seq: p.node, rec: quad(-1.0, q, [0.0, 0.0, 1.0, 1.0], col) });
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
