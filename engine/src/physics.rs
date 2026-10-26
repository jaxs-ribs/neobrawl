//! A small 2D (XY-plane) rigidbody solver standing in for PhysX: oriented box
//! and circle (capsule seen end-on) colliders, continuous movement via
//! substeps, inelastic contacts with the frictionless "Slide" material,
//! layer-collision matrix, Physics.IgnoreCollision, collision/trigger
//! enter/exit events and raycasts.

use crate::det::HashSet;

use crate::shared::math::{v3, V3};

use crate::scene::{Shape, World, FREEZE_ALL};

pub const LAYER_PLAYER: u32 = 10;
pub const LAYER_BOX: u32 = 11;
pub const LAYER_DEBRIS: u32 = 12;

#[derive(Clone, Copy, Debug)]
pub enum Geo {
    /// center, unit axis x, unit axis y, half extents
    Obb { c: [f32; 2], ax: [f32; 2], ay: [f32; 2], h: [f32; 2] },
    Circle { c: [f32; 2], r: f32 },
    /// convex polygon in world space, CCW, first n points used
    Poly { n: u8, p: [[f32; 2]; 16] },
}

impl Geo {
    /// Boxes and hulls alike as a CCW polygon.
    fn poly(&self) -> Option<(usize, [[f32; 2]; 16])> {
        match self {
            Geo::Obb { c, ax, ay, h } => {
                let mut p = [[0.0; 2]; 16];
                p[..4].copy_from_slice(&corners(*c, *ax, *ay, *h));
                Some((4, ccw(4, p)))
            }
            Geo::Poly { n, p } => Some((*n as usize, *p)),
            Geo::Circle { .. } => None,
        }
    }
}

fn ccw(n: usize, mut p: [[f32; 2]; 16]) -> [[f32; 2]; 16] {
    let mut a = 0.0;
    for k in 0..n {
        let (u, v) = (p[k], p[(k + 1) % n]);
        a += u[0] * v[1] - v[0] * u[1];
    }
    if a < 0.0 {
        p[..n].reverse();
    }
    p
}

/// Outward unit normal of edge k of a CCW polygon.
fn edge_normal(p: &[[f32; 2]; 16], n: usize, k: usize) -> [f32; 2] {
    let (a, b) = (p[k], p[(k + 1) % n]);
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l = (dx * dx + dy * dy).sqrt().max(1e-9);
    [dy / l, -dx / l]
}

/// SAT between two convex polygons; normal points from b towards a.
fn poly_poly(na: usize, pa: &[[f32; 2]; 16], nb: usize, pb: &[[f32; 2]; 16], slop: f32) -> Option<Contact> {
    let (pa, pb) = (&pa[..na], &pb[..nb]);
    let mut best = (f32::MAX, [0.0f32, 0.0f32]);
    let axes = (0..na).map(|k| (true, k)).chain((0..nb).map(|k| (false, k)));
    let mut fa = [[0.0; 2]; 16];
    fa[..na].copy_from_slice(pa);
    let mut fb = [[0.0; 2]; 16];
    fb[..nb].copy_from_slice(pb);
    for (from_a, k) in axes {
        let axis = if from_a { edge_normal(&fa, na, k) } else { edge_normal(&fb, nb, k) };
        let (mut amin, mut amax, mut bmin, mut bmax) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for p in pa {
            let d = dot(*p, axis);
            amin = amin.min(d);
            amax = amax.max(d);
        }
        for p in pb {
            let d = dot(*p, axis);
            bmin = bmin.min(d);
            bmax = bmax.max(d);
        }
        let o = (amax - bmin).min(bmax - amin);
        if o < -slop {
            return None;
        }
        if o < best.0 {
            best = (o, axis);
        }
    }
    let centroid = |p: &[[f32; 2]]| {
        let s = p.iter().fold([0.0f32; 2], |a, q| [a[0] + q[0], a[1] + q[1]]);
        [s[0] / p.len() as f32, s[1] / p.len() as f32]
    };
    let (ca, cb) = (centroid(pa), centroid(pb));
    let mut n = best.1;
    if dot([ca[0] - cb[0], ca[1] - cb[1]], n) < 0.0 {
        n = [-n[0], -n[1]];
    }
    let ia = pa.iter().min_by(|p, q| dot(**p, n).partial_cmp(&dot(**q, n)).unwrap()).unwrap();
    let ib = pb.iter().max_by(|p, q| dot(**p, n).partial_cmp(&dot(**q, n)).unwrap()).unwrap();
    Some(Contact { normal: n, depth: best.0, point: [(ia[0] + ib[0]) / 2.0, (ia[1] + ib[1]) / 2.0] })
}

/// Circle against a CCW convex polygon; normal points from the polygon to the circle.
fn circle_poly(c: [f32; 2], r: f32, n: usize, p: &[[f32; 2]; 16], slop: f32) -> Option<Contact> {
    let mut inside = true;
    let (mut best_d, mut best_pt) = (f32::MAX, [0.0f32; 2]);
    let (mut shallow, mut shallow_n) = (f32::MAX, [0.0f32; 2]);
    for k in 0..n {
        let (a, b) = (p[k], p[(k + 1) % n]);
        let en = edge_normal(p, n, k);
        let side = dot([c[0] - a[0], c[1] - a[1]], en);
        if side > 0.0 {
            inside = false;
        }
        if -side < shallow {
            shallow = -side;
            shallow_n = en;
        }
        let ab = [b[0] - a[0], b[1] - a[1]];
        let t = (dot([c[0] - a[0], c[1] - a[1]], ab) / dot(ab, ab).max(1e-12)).clamp(0.0, 1.0);
        let q = [a[0] + ab[0] * t, a[1] + ab[1] * t];
        let d = ((c[0] - q[0]).powi(2) + (c[1] - q[1]).powi(2)).sqrt();
        if d < best_d {
            best_d = d;
            best_pt = q;
        }
    }
    if inside {
        return Some(Contact { normal: shallow_n, depth: shallow + r, point: best_pt });
    }
    let depth = r - best_d;
    if depth < -slop {
        return None;
    }
    let v = [c[0] - best_pt[0], c[1] - best_pt[1]];
    Some(Contact { normal: [v[0] / best_d.max(1e-9), v[1] / best_d.max(1e-9)], depth, point: best_pt })
}

#[derive(Clone, Copy, Debug)]
pub struct Contact {
    /// unit normal pointing from `b` towards `a`
    pub normal: [f32; 2],
    pub depth: f32,
    pub point: [f32; 2],
}

#[derive(Clone, Copy, Debug)]
pub struct CollisionEvent {
    pub a: usize,
    pub b: usize,
    pub point: V3,
    /// normal as reported to `a` (points away from `b`)
    pub normal: V3,
    pub enter: bool,
    pub trigger: bool,
}

#[derive(Default, Clone)]
pub struct Physics {
    pub ignore: HashSet<(usize, usize)>,
    touching: HashSet<(usize, usize)>,
    triggers: HashSet<(usize, usize)>,
    pub events: Vec<CollisionEvent>,
    /// the enabled, active colliders as of the last step, for the node
    /// count it was taken at: raycasts and debris check only these (and
    /// re-check `enabled`, which gameplay flips mid-tick)
    live: (usize, std::rc::Rc<Vec<usize>>),
}

fn key(a: usize, b: usize) -> (usize, usize) {
    if a < b {
        (a, b)
    } else {
        (b, a)
    }
}

pub fn layers_collide(a: u32, b: u32) -> bool {
    !matches!((a, b), (LAYER_PLAYER, LAYER_PLAYER) | (LAYER_BOX, LAYER_DEBRIS) | (LAYER_DEBRIS, LAYER_BOX))
}

pub fn geometry(w: &World, i: usize) -> Option<Geo> {
    geometry_m(w, i, &w.matrix(i))
}

/// A shape's bounding circle: centre and radius.
pub fn bound(g: &Geo) -> (f32, f32, f32) {
    match g {
        Geo::Circle { c, r } => (c[0], c[1], *r),
        Geo::Obb { c, h, .. } => (c[0], c[1], (h[0] * h[0] + h[1] * h[1]).sqrt()),
        Geo::Poly { n, p } => {
            let n = *n as usize;
            let (sx, sy) = p[..n].iter().fold((0.0, 0.0), |a, q| (a.0 + q[0], a.1 + q[1]));
            let (cx, cy) = (sx / n as f32, sy / n as f32);
            let r = p[..n].iter().map(|q| ((q[0] - cx).powi(2) + (q[1] - cy).powi(2)).sqrt()).fold(0.0f32, f32::max);
            (cx, cy, r)
        }
    }
}

/// `geometry` with the node's world matrix already in hand.
pub fn geometry_m(w: &World, i: usize, m: &crate::shared::math::Mat4) -> Option<Geo> {
    let col = w.nodes[i].collider.as_ref()?;
    let m = *m;
    match col.shape {
        Shape::Hull { n, pts } => {
            let mut p = [[0.0f32; 2]; 16];
            for k in 0..n as usize {
                let q = m.point(v3(pts[k][0], pts[k][1], 0.0));
                p[k] = [q.x, q.y];
            }
            Some(Geo::Poly { n, p: ccw(n as usize, p) })
        }
        Shape::Box { size, center } => {
            let c = m.point(center);
            let ex = v3(m.0[0], m.0[1], m.0[2]);
            let ey = v3(m.0[4], m.0[5], m.0[6]);
            let ez = v3(m.0[8], m.0[9], m.0[10]);
            // pick the two world axes that lie most in the XY plane
            let mut axes = [(ex, size.x), (ey, size.y), (ez, size.z)];
            axes.sort_by(|a, b| a.0.z.abs().partial_cmp(&b.0.z.abs()).unwrap());
            let (a1, s1) = axes[0];
            let (_, s2) = axes[1];
            let l1 = (a1.x * a1.x + a1.y * a1.y).sqrt().max(1e-6);
            let u = [a1.x / l1, a1.y / l1];
            let a2v = axes[1].0;
            let l2 = (a2v.x * a2v.x + a2v.y * a2v.y).sqrt().max(1e-6);
            Some(Geo::Obb { c: [c.x, c.y], ax: u, ay: [-u[1], u[0]], h: [(s1 * l1 / 2.0).abs(), (s2 * l2 / 2.0).abs()] })
        }
        Shape::Capsule { radius, center } => {
            let c = m.point(center);
            let sx = (m.0[0] * m.0[0] + m.0[1] * m.0[1] + m.0[2] * m.0[2]).sqrt();
            let sy = (m.0[4] * m.0[4] + m.0[5] * m.0[5] + m.0[6] * m.0[6]).sqrt();
            Some(Geo::Circle { c: [c.x, c.y], r: radius * sx.max(sy) })
        }
    }
}

fn corners(c: [f32; 2], ax: [f32; 2], ay: [f32; 2], h: [f32; 2]) -> [[f32; 2]; 4] {
    let p = |sx: f32, sy: f32| [c[0] + ax[0] * h[0] * sx + ay[0] * h[1] * sy, c[1] + ax[1] * h[0] * sx + ay[1] * h[1] * sy];
    [p(-1.0, -1.0), p(1.0, -1.0), p(1.0, 1.0), p(-1.0, 1.0)]
}

fn dot(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}

/// Contact between a and b, if they overlap (or are within `slop`).
pub fn collide(a: &Geo, b: &Geo, slop: f32) -> Option<Contact> {
    match (a, b) {
        (Geo::Circle { c: ca, r: ra }, Geo::Circle { c: cb, r: rb }) => {
            let d = [ca[0] - cb[0], ca[1] - cb[1]];
            let l = dot(d, d).sqrt();
            let depth = ra + rb - l;
            if depth < -slop {
                return None;
            }
            let n = if l > 1e-6 { [d[0] / l, d[1] / l] } else { [1.0, 0.0] };
            Some(Contact { normal: n, depth, point: [cb[0] + n[0] * rb, cb[1] + n[1] * rb] })
        }
        (Geo::Circle { c, r }, Geo::Obb { c: bc, ax, ay, h }) => circle_obb(*c, *r, *bc, *ax, *ay, *h, slop),
        (Geo::Obb { .. } | Geo::Poly { .. }, Geo::Circle { .. }) => collide(b, a, slop).map(|k| Contact { normal: [-k.normal[0], -k.normal[1]], ..k }),
        (Geo::Circle { c, r }, Geo::Poly { n, p }) => circle_poly(*c, *r, *n as usize, p, slop),
        (Geo::Poly { .. }, _) | (_, Geo::Poly { .. }) => {
            let ((na, pa), (nb, pb)) = (a.poly()?, b.poly()?);
            poly_poly(na, &pa, nb, &pb, slop)
        }
        (Geo::Obb { c: c1, ax: x1, ay: y1, h: h1 }, Geo::Obb { c: c2, ax: x2, ay: y2, h: h2 }) => {
            let pa = corners(*c1, *x1, *y1, *h1);
            let pb = corners(*c2, *x2, *y2, *h2);
            let mut best = (f32::MAX, [0.0f32, 0.0f32]);
            for axis in [*x1, *y1, *x2, *y2] {
                let (mut amin, mut amax, mut bmin, mut bmax) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
                for p in pa {
                    let d = dot(p, axis);
                    amin = amin.min(d);
                    amax = amax.max(d);
                }
                for p in pb {
                    let d = dot(p, axis);
                    bmin = bmin.min(d);
                    bmax = bmax.max(d);
                }
                let o = (amax - bmin).min(bmax - amin);
                if o < -slop {
                    return None;
                }
                if o < best.0 {
                    best = (o, axis);
                }
            }
            let mut n = best.1;
            if dot([c1[0] - c2[0], c1[1] - c2[1]], n) < 0.0 {
                n = [-n[0], -n[1]];
            }
            // contact point: deepest corner of a along -n, or of b along n
            let ia = pa.iter().min_by(|p, q| dot(**p, n).partial_cmp(&dot(**q, n)).unwrap()).unwrap();
            let ib = pb.iter().max_by(|p, q| dot(**p, n).partial_cmp(&dot(**q, n)).unwrap()).unwrap();
            let point = [(ia[0] + ib[0]) / 2.0, (ia[1] + ib[1]) / 2.0];
            Some(Contact { normal: n, depth: best.0, point })
        }
    }
}

fn circle_obb(c: [f32; 2], r: f32, bc: [f32; 2], ax: [f32; 2], ay: [f32; 2], h: [f32; 2], slop: f32) -> Option<Contact> {
    let d = [c[0] - bc[0], c[1] - bc[1]];
    let lx = dot(d, ax);
    let ly = dot(d, ay);
    let cx = lx.clamp(-h[0], h[0]);
    let cy = ly.clamp(-h[1], h[1]);
    let closest = [bc[0] + ax[0] * cx + ay[0] * cy, bc[1] + ax[1] * cx + ay[1] * cy];
    let v = [c[0] - closest[0], c[1] - closest[1]];
    let dist = dot(v, v).sqrt();
    if lx.abs() <= h[0] && ly.abs() <= h[1] {
        // center inside the box: push out along the shallowest face
        let px = h[0] - lx.abs();
        let py = h[1] - ly.abs();
        let (n, depth) = if px < py {
            let s = lx.signum();
            ([ax[0] * s, ax[1] * s], px + r)
        } else {
            let s = ly.signum();
            ([ay[0] * s, ay[1] * s], py + r)
        };
        return Some(Contact { normal: n, depth, point: closest });
    }
    let depth = r - dist;
    if depth < -slop {
        return None;
    }
    Some(Contact { normal: [v[0] / dist, v[1] / dist], depth, point: closest })
}

pub fn ray_geo(o: [f32; 2], d: [f32; 2], g: &Geo) -> Option<f32> {
    match g {
        Geo::Circle { c, r } => {
            let m = [o[0] - c[0], o[1] - c[1]];
            let b = dot(m, d);
            let cc = dot(m, m) - r * r;
            if cc <= 0.0 {
                return None; // origin inside: PhysX ignores the starting collider
            }
            if b > 0.0 {
                return None;
            }
            let disc = b * b - cc;
            if disc < 0.0 {
                return None;
            }
            Some((-b - disc.sqrt()).max(0.0))
        }
        Geo::Poly { n, p } => {
            // Cyrus-Beck: clip the ray against each edge's half-plane
            let n = *n as usize;
            let (mut tmin, mut tmax) = (0.0f32, f32::MAX);
            let mut inside = true;
            for k in 0..n {
                let en = edge_normal(p, n, k);
                let num = dot([p[k][0] - o[0], p[k][1] - o[1]], en);
                let den = dot(d, en);
                if num < 0.0 {
                    inside = false;
                }
                if den.abs() < 1e-9 {
                    if num < 0.0 {
                        return None;
                    }
                } else {
                    let t = num / den;
                    if den < 0.0 {
                        tmin = tmin.max(t);
                    } else {
                        tmax = tmax.min(t);
                    }
                    if tmin > tmax {
                        return None;
                    }
                }
            }
            if inside {
                return None; // origin inside: PhysX ignores the starting collider
            }
            Some(tmin)
        }
        Geo::Obb { c, ax, ay, h } => {
            let rel = [o[0] - c[0], o[1] - c[1]];
            if dot(rel, *ax).abs() <= h[0] && dot(rel, *ay).abs() <= h[1] {
                return None;
            }
            let (mut tmin, mut tmax) = (0.0f32, f32::MAX);
            for (axis, half) in [(*ax, h[0]), (*ay, h[1])] {
                let p = dot(rel, axis);
                let q = dot(d, axis);
                if q.abs() < 1e-8 {
                    if p.abs() > half {
                        return None;
                    }
                } else {
                    let t1 = (-half - p) / q;
                    let t2 = (half - p) / q;
                    tmin = tmin.max(t1.min(t2));
                    tmax = tmax.min(t1.max(t2));
                    if tmin > tmax {
                        return None;
                    }
                }
            }
            Some(tmin)
        }
    }
}

pub struct RayHit {
    pub node: usize,
    pub point: V3,
    pub distance: f32,
}

impl Physics {
    pub fn colliders(&self, w: &World) -> Vec<usize> {
        let ok = |i: usize| w.nodes[i].collider.as_ref().map(|c| c.enabled).unwrap_or(false) && w.active_in_hierarchy(i);
        if self.live.0 == w.nodes.len() {
            self.live.1.iter().copied().filter(|&i| w.nodes[i].collider.as_ref().map(|c| c.enabled).unwrap_or(false)).collect()
        } else {
            (0..w.nodes.len()).filter(|&i| ok(i)).collect()
        }
    }

    /// Physics.Raycast in the XY plane against non-trigger colliders on `mask` layers.
    pub fn raycast(&self, w: &World, origin: V3, dir: V3, max: f32, mask: u32) -> Option<RayHit> {
        let l = (dir.x * dir.x + dir.y * dir.y).sqrt();
        if l < 1e-6 {
            return None;
        }
        let d = [dir.x / l, dir.y / l];
        let mut best: Option<RayHit> = None;
        for i in self.colliders(w) {
            if mask & (1 << w.nodes[i].layer) == 0 {
                continue;
            }
            let Some(g) = geometry(w, i) else { continue };
            if let Some(t) = ray_geo([origin.x, origin.y], d, &g) {
                if t <= max && best.as_ref().map(|b| t < b.distance).unwrap_or(true) {
                    best = Some(RayHit { node: i, point: v3(origin.x + d[0] * t, origin.y + d[1] * t, origin.z), distance: t });
                }
            }
        }
        best
    }

    pub fn set_ignore(&mut self, a: usize, b: usize, ignore: bool) {
        if ignore {
            self.ignore.insert(key(a, b));
        } else {
            self.ignore.remove(&key(a, b));
        }
    }

    /// One FixedUpdate physics step: integrate, resolve, then report events.
    pub fn step(&mut self, w: &mut World, dt: f32) {
        self.events.clear();
        // one pass for every world matrix and active flag
        let (wm, act) = w.world_cache();
        let cols: Vec<usize> = (0..w.nodes.len()).filter(|&i| act[i] && w.nodes[i].collider.as_ref().map(|c| c.enabled).unwrap_or(false)).collect();
        self.live = (w.nodes.len(), std::rc::Rc::new(cols.clone()));
        let solid: Vec<usize> = cols.iter().copied().filter(|&i| !w.nodes[i].collider.as_ref().unwrap().trigger).collect();
        let dynamic: Vec<usize> = solid
            .iter()
            .copied()
            .filter(|&i| w.nodes[i].body.as_ref().map(|b| b.constraints != FREEZE_ALL).unwrap_or(false))
            .collect();
        let mut touching: HashSet<(usize, usize)> = HashSet::default();
        let mut contacts: Vec<(usize, usize, Contact)> = Vec::new();

        for &i in &dynamic {
            let body = w.nodes[i].body.as_mut().unwrap();
            if body.drag > 0.0 {
                let k = (1.0 - body.drag * dt).max(0.0);
                body.velocity = body.velocity * k;
            }
        }
        // only bodies in motion sweep; resting ones act as solids until pushed
        let is_awake = |w: &World, i: usize| {
            let b = w.nodes[i].body.as_ref().unwrap();
            b.velocity.x.abs() + b.velocity.y.abs() > 1e-3 || b.spin.abs() > 1e-3
        };
        let mut awake_flag = vec![false; w.nodes.len()];
        let awake: Vec<usize> = dynamic.iter().copied().filter(|&i| is_awake(w, i)).collect();
        for &i in &awake {
            awake_flag[i] = true;
        }
        let mut dyn_flag = vec![false; w.nodes.len()];
        for &i in &dynamic {
            dyn_flag[i] = true;
        }
        // shapes that won't move this step, once, with bounding circles
        let mut still: Vec<Option<(Geo, (f32, f32, f32))>> = vec![None; w.nodes.len()];
        for &j in &solid {
            if !awake_flag[j] {
                if let Some(g) = geometry_m(w, j, &wm[j]) {
                    still[j] = Some((g, bound(&g)));
                }
            }
        }
        // Continuous movement: advance every moving body in small substeps.
        let max_speed = awake
            .iter()
            .map(|&i| {
                let v = w.nodes[i].body.as_ref().unwrap().velocity;
                (v.x * v.x + v.y * v.y).sqrt()
            })
            .fold(0.0f32, f32::max);
        let steps = ((max_speed * dt / 0.06).ceil() as usize).clamp(1, 40);
        let h = dt / steps as f32;
        for _ in 0..steps {
            for &i in &awake {
                let body = w.nodes[i].body.as_ref().unwrap();
                let v = body.velocity;
                let spin = body.spin;
                if v.x != 0.0 || v.y != 0.0 {
                    w.translate_world(i, v3(v.x * h, v.y * h, 0.0));
                }
                if spin != 0.0 {
                    w.rotate_self(i, 0.0, 0.0, spin * h);
                }
            }
            let moving: Vec<(usize, Geo, (f32, f32, f32))> = awake.iter().filter_map(|&i| geometry(w, i).map(|g| (i, g, bound(&g)))).collect();
            for &(i, gi, bi) in moving.iter() {
                let mut gi = gi;
                let mut bi = bi;
                for &j in &solid {
                    if i == j || self.ignore.contains(&key(i, j)) {
                        continue;
                    }
                    if !layers_collide(w.nodes[i].layer, w.nodes[j].layer) {
                        continue;
                    }
                    // moving pairs are handled once (from the lower index)
                    if awake_flag[j] && j < i {
                        continue;
                    }
                    let (gj, bj) = if awake_flag[j] {
                        match moving.iter().find(|m| m.0 == j) {
                            Some(m) => (m.1, m.2),
                            None => continue,
                        }
                    } else {
                        match still[j] {
                            Some(x) => x,
                            None => continue,
                        }
                    };
                    // broad phase: bounding circles
                    let (dx, dy) = (bi.0 - bj.0, bi.1 - bj.1);
                    let reach = bi.2 + bj.2 + 0.02;
                    if dx * dx + dy * dy > reach * reach {
                        continue;
                    }
                    let Some(c) = collide(&gi, &gj, 0.01) else { continue };
                    if !touching.contains(&key(i, j)) {
                        contacts.push((i, j, c));
                    }
                    touching.insert(key(i, j));
                    if c.depth <= 0.0 {
                        continue;
                    }
                    let n = v3(c.normal[0], c.normal[1], 0.0);
                    if dyn_flag[j] {
                        let (ma, mb) = (w.nodes[i].body.as_ref().unwrap().mass, w.nodes[j].body.as_ref().unwrap().mass);
                        let t = ma + mb;
                        w.translate_world(i, n * (c.depth * mb / t));
                        w.translate_world(j, n * (-c.depth * ma / t));
                        let va = w.nodes[i].body.as_ref().unwrap().velocity;
                        let vb = w.nodes[j].body.as_ref().unwrap().velocity;
                        let rel = (va.x - vb.x) * n.x + (va.y - vb.y) * n.y;
                        if rel < 0.0 {
                            // perfectly inelastic along the normal
                            let imp = rel / t;
                            w.nodes[i].body.as_mut().unwrap().velocity = va - n * (imp * mb);
                            w.nodes[j].body.as_mut().unwrap().velocity = vb + n * (imp * ma);
                        }
                        // a resting body that was pushed has moved: refresh its shape
                        if !awake_flag[j] {
                            if let Some(g) = geometry(w, j) {
                                still[j] = Some((g, bound(&g)));
                            }
                        }
                    } else {
                        w.translate_world(i, n * c.depth);
                        let b = w.nodes[i].body.as_mut().unwrap();
                        let vn = b.velocity.x * n.x + b.velocity.y * n.y;
                        if vn < 0.0 {
                            b.velocity = b.velocity - n * vn;
                        }
                    }
                    // we moved: later pairs see where we are now
                    if let Some(g) = geometry(w, i) {
                        gi = g;
                        bi = bound(&g);
                    }
                }
            }
        }
        // Collision enter / exit
        for (a, b, c) in contacts {
            let k = key(a, b);
            if !self.touching.contains(&k) {
                let p = v3(c.point[0], c.point[1], 0.0);
                let n = v3(c.normal[0], c.normal[1], 0.0);
                self.events.push(CollisionEvent { a, b, point: p, normal: n, enter: true, trigger: false });
                self.events.push(CollisionEvent { a: b, b: a, point: p, normal: -n, enter: true, trigger: false });
            }
        }
        for &(a, b) in self.touching.difference(&touching) {
            self.events.push(CollisionEvent { a, b, point: V3::ZERO, normal: V3::ZERO, enter: false, trigger: false });
            self.events.push(CollisionEvent { a: b, b: a, point: V3::ZERO, normal: V3::ZERO, enter: false, trigger: false });
        }
        self.touching = touching;

        // Triggers: OnTriggerEnter between triggers and dynamic bodies
        let triggers: Vec<usize> = cols.iter().copied().filter(|&i| w.nodes[i].collider.as_ref().unwrap().trigger).collect();
        let mut now = HashSet::default();
        let dyn_geo: Vec<(usize, Geo, (f32, f32, f32))> = if triggers.is_empty() {
            Vec::new()
        } else {
            dynamic.iter().filter_map(|&i| geometry(w, i).map(|g| (i, g, bound(&g)))).collect()
        };
        for &t in &triggers {
            let Some(gt) = geometry(w, t) else { continue };
            let bt = bound(&gt);
            for &(i, gi, bi) in &dyn_geo {
                let (dx, dy) = (bt.0 - bi.0, bt.1 - bi.1);
                if dx * dx + dy * dy > (bt.2 + bi.2).powi(2) {
                    continue;
                }
                if collide(&gt, &gi, 0.0).map(|c| c.depth > 0.0).unwrap_or(false) {
                    now.insert((t, i));
                    if !self.triggers.contains(&(t, i)) {
                        self.events.push(CollisionEvent { a: t, b: i, point: V3::ZERO, normal: V3::ZERO, enter: true, trigger: true });
                    }
                }
            }
        }
        self.triggers = now;
    }

    /// Forget contact state involving `i` (teleports, deactivation).
    pub fn forget(&mut self, i: usize) {
        self.touching.retain(|&(a, b)| a != i && b != i);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circle_against_box_pushes_out_along_face() {
        let b = Geo::Obb { c: [0.0, 0.0], ax: [1.0, 0.0], ay: [0.0, 1.0], h: [1.0, 1.0] };
        let c = Geo::Circle { c: [1.2, 0.0], r: 0.5 };
        let k = collide(&c, &b, 0.0).unwrap();
        assert!((k.normal[0] - 1.0).abs() < 1e-5 && (k.depth - 0.3).abs() < 1e-5, "{k:?}");
    }

    #[test]
    fn rotated_boxes_sat() {
        let s = std::f32::consts::FRAC_1_SQRT_2;
        let a = Geo::Obb { c: [0.0, 0.0], ax: [1.0, 0.0], ay: [0.0, 1.0], h: [1.0, 1.0] };
        let b = Geo::Obb { c: [2.2, 0.0], ax: [s, s], ay: [-s, s], h: [1.0, 1.0] };
        let k = collide(&b, &a, 0.0).unwrap();
        assert!(k.normal[0] > 0.9 && k.depth > 0.0, "{k:?}");
        let far = Geo::Obb { c: [3.5, 0.0], ax: [s, s], ay: [-s, s], h: [1.0, 1.0] };
        assert!(collide(&far, &a, 0.0).is_none());
    }

    #[test]
    fn ray_hits_nearest_face() {
        let b = Geo::Obb { c: [5.0, 0.0], ax: [1.0, 0.0], ay: [0.0, 1.0], h: [1.0, 1.0] };
        assert!((ray_geo([0.0, 0.0], [1.0, 0.0], &b).unwrap() - 4.0).abs() < 1e-5);
        assert!(ray_geo([0.0, 0.0], [-1.0, 0.0], &b).is_none());
    }

    fn square(c: [f32; 2], h: f32) -> Geo {
        let mut p = [[0.0f32; 2]; 16];
        p[..4].copy_from_slice(&[[c[0] - h, c[1] - h], [c[0] + h, c[1] - h], [c[0] + h, c[1] + h], [c[0] - h, c[1] + h]]);
        Geo::Poly { n: 4, p }
    }

    #[test]
    fn hull_contacts_and_rays() {
        let a = square([0.0, 0.0], 1.0);
        // circle just touching the right face is pushed out along +x
        let k = collide(&Geo::Circle { c: [1.4, 0.0], r: 0.5 }, &a, 0.0).unwrap();
        assert!((k.normal[0] - 1.0).abs() < 1e-4 && (k.depth - 0.1).abs() < 1e-4);
        // polygon vs polygon and polygon vs box agree with the box solver
        let k = collide(&square([1.9, 0.0], 1.0), &a, 0.0).unwrap();
        assert!((k.normal[0] - 1.0).abs() < 1e-4 && (k.depth - 0.1).abs() < 1e-4);
        let b = Geo::Obb { c: [0.0, 1.9], ax: [1.0, 0.0], ay: [0.0, 1.0], h: [1.0, 1.0] };
        let k = collide(&b, &a, 0.0).unwrap();
        assert!((k.normal[1] - 1.0).abs() < 1e-4);
        assert!(collide(&square([3.0, 0.0], 0.5), &a, 0.0).is_none());
        // rays enter at the near face; starting inside is ignored
        assert!((ray_geo([-5.0, 0.0], [1.0, 0.0], &a).unwrap() - 4.0).abs() < 1e-4);
        assert!(ray_geo([-5.0, 3.0], [1.0, 0.0], &a).is_none());
        assert!(ray_geo([0.0, 0.0], [1.0, 0.0], &a).is_none());
    }
}
