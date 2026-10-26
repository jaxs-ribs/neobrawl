//! uGUI layout: CanvasScaler + RectTransform anchoring, canvas placement for
//! Overlay / ScreenSpace-Camera / World modes, and pointer hit-testing.

use crate::det::HashMap;

use crate::shared::math::{v3, Mat4, Quat, V3};

use crate::scene::World;

#[derive(Clone, Copy, Debug)]
pub struct Screen {
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Default)]
pub struct Layout {
    /// root canvas -> world matrix (overlay canvases live in screen-pixel space)
    pub canvas_root: HashMap<usize, Mat4>,
    /// rect (x0, y0, x1, y1) of each RectTransform in its own local space
    pub rects: HashMap<usize, [f32; 4]>,
    /// canvas root of each UI node and whether it is an overlay
    pub canvas_of: HashMap<usize, (usize, i32)>,
    /// overlay roots in draw order
    pub overlays: Vec<usize>,
}

impl Layout {
    pub fn compute(w: &mut World, screen: Screen, cam_world: impl Fn(&World, usize) -> Option<(Mat4, f32)>) -> Layout {
        let mut l = Layout::default();
        let roots: Vec<usize> = (0..w.nodes.len())
            .filter(|&i| w.nodes[i].canvas.is_some() && !has_canvas_ancestor(w, i) && w.active_in_hierarchy(i))
            .collect();
        for root in roots {
            let (mode, cam, plane) = w.nodes[root].canvas.unwrap();
            let s = expand_scale(w, root, screen);
            let size = [screen.w / s, screen.h / s];
            match mode {
                0 => {
                    l.canvas_root.insert(root, Mat4::trs(v3(screen.w / 2.0, screen.h / 2.0, 0.0), Quat::IDENTITY, v3(s, s, 1.0)));
                    l.overlays.push(root);
                    l.rects.insert(root, [-size[0] / 2.0, -size[1] / 2.0, size[0] / 2.0, size[1] / 2.0]);
                }
                1 => {
                    if let Some((cm, fov)) = cam.and_then(|c| cam_world(w, c)) {
                        let world_h = 2.0 * plane * (fov.to_radians() / 2.0).tan();
                        let k = world_h / size[1];
                        let m = cm.mul(&Mat4::trs(v3(0.0, 0.0, plane), Quat::IDENTITY, v3(k, k, k)));
                        l.canvas_root.insert(root, m);
                    }
                    l.rects.insert(root, [-size[0] / 2.0, -size[1] / 2.0, size[0] / 2.0, size[1] / 2.0]);
                }
                _ => {
                    let r = w.nodes[root].rect.clone().unwrap_or_default();
                    let x0 = -r.pivot[0] * r.size[0];
                    let y0 = -r.pivot[1] * r.size[1];
                    l.rects.insert(root, [x0, y0, x0 + r.size[0], y0 + r.size[1]]);
                }
            }
            l.layout_children(w, root, root, mode);
            l.canvas_of.insert(root, (root, mode));
        }
        l
    }

    fn layout_children(&mut self, w: &mut World, parent: usize, root: usize, mode: i32) {
        let pr = self.rects[&parent];
        let (pw, ph) = (pr[2] - pr[0], pr[3] - pr[1]);
        for k in 0..w.nodes[parent].children.len() {
            let c = w.nodes[parent].children[k];
            self.canvas_of.insert(c, (root, mode));
            if let Some(r) = w.nodes[c].rect.clone() {
                let size = [pw * (r.anchor_max[0] - r.anchor_min[0]) + r.size[0], ph * (r.anchor_max[1] - r.anchor_min[1]) + r.size[1]];
                let ax = pr[0] + pw * (r.anchor_min[0] + (r.anchor_max[0] - r.anchor_min[0]) * r.pivot[0]);
                let ay = pr[1] + ph * (r.anchor_min[1] + (r.anchor_max[1] - r.anchor_min[1]) * r.pivot[1]);
                let n = &mut w.nodes[c];
                n.lpos.x = ax + r.pos[0];
                n.lpos.y = ay + r.pos[1];
                let x0 = -r.pivot[0] * size[0];
                let y0 = -r.pivot[1] * size[1];
                self.rects.insert(c, [x0, y0, x0 + size[0], y0 + size[1]]);
            } else {
                self.rects.insert(c, [0.0, 0.0, 0.0, 0.0]);
            }
            self.layout_children(w, c, root, mode);
        }
    }

    /// World (or overlay-pixel) matrix of a UI node.
    pub fn matrix(&self, w: &World, i: usize) -> Mat4 {
        if let Some(m) = self.canvas_root.get(&i) {
            return *m;
        }
        let local = w.local_matrix(i);
        match w.nodes[i].parent {
            Some(p) if self.canvas_of.contains_key(&p) => self.matrix(w, p).mul(&local),
            Some(p) => w.matrix(p).mul(&local),
            None => local,
        }
    }

    /// RectTransform.position = p (world / overlay pixels): adjust anchoredPosition.
    pub fn set_position(&self, w: &mut World, i: usize, p: V3) {
        let Some(par) = w.nodes[i].parent else { return };
        let local = self.matrix(w, par).inverse_affine().point(p);
        let cur = w.nodes[i].lpos;
        if let Some(r) = w.nodes[i].rect.as_mut() {
            r.pos[0] += local.x - cur.x;
            r.pos[1] += local.y - cur.y;
        }
        w.nodes[i].lpos = v3(local.x, local.y, cur.z);
    }

    pub fn position(&self, w: &World, i: usize) -> V3 {
        self.matrix(w, i).point(V3::ZERO)
    }

    /// Rect corners (bl, br, tr, tl) in world / overlay space.
    pub fn corners(&self, w: &World, i: usize) -> [V3; 4] {
        let r = self.rects.get(&i).copied().unwrap_or([0.0; 4]);
        let m = self.matrix(w, i);
        [m.point(v3(r[0], r[1], 0.0)), m.point(v3(r[2], r[1], 0.0)), m.point(v3(r[2], r[3], 0.0)), m.point(v3(r[0], r[3], 0.0))]
    }
}

fn has_canvas_ancestor(w: &World, i: usize) -> bool {
    let mut c = w.nodes[i].parent;
    while let Some(p) = c {
        if w.nodes[p].canvas.is_some() {
            return true;
        }
        c = w.nodes[p].parent;
    }
    false
}

fn expand_scale(w: &World, canvas: usize, s: Screen) -> f32 {
    let Some((refr, m, mode)) = w.nodes[canvas].scaler else { return 1.0 };
    if refr[0] <= 0.0 {
        return 1.0;
    }
    let sx = s.w / refr[0];
    let sy = s.h / refr[1];
    match mode {
        1 => sx.min(sy), // Expand
        2 => sx.max(sy), // Shrink
        _ => 2f32.powf(sx.log2() + (sy.log2() - sx.log2()) * m),
    }
}

/// Point-in-convex-quad test in 2D.
pub fn in_quad(p: [f32; 2], q: [[f32; 2]; 4]) -> bool {
    let mut sign = 0.0f32;
    for k in 0..4 {
        let a = q[k];
        let b = q[(k + 1) % 4];
        let c = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
        if c.abs() < 1e-9 {
            continue;
        }
        if sign == 0.0 {
            sign = c.signum();
        } else if c.signum() != sign {
            return false;
        }
    }
    true
}
