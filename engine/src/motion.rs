//! Motion graphics. One grammar for everything that exists:
//!   Form    - electricity traces it in, it snaps solid with a little overshoot
//!   Hum     - nothing is ever fully still
//!   Release - it shatters, arcs jump between the shards, shards seek the cause
//!   Settle  - damped springs everywhere
//! Effects are short-lived records stepped on the fixed tick and drawn at
//! render time (extrapolated to `render_now`, so they stay smooth at 120 Hz).
//! Screen-space effects are in overlay pixels; world-space ones in world units.

use std::collections::{HashMap, VecDeque};

use crate::render::{arc_points, blade, quad, ribbon, ribbon_tapered, RECORD};
use crate::shared::math::{v3, V3};

#[derive(Clone, Debug)]
pub enum Kind {
    /// expanding ring
    Ring { c: V3, r0: f32, r1: f32, w: f32 },
    /// flickering lightning between two points
    Arc { a: V3, b: V3, w: f32, jitter: f32 },
    /// a spinning triangle of something that broke; may home in on a node
    Shard { p: V3, v: V3, rot: f32, spin: f32, tri: [[f32; 2]; 3], seek: Option<usize> },
    /// a glowing point with a tail; may home in on a node or a point
    Spark { p: V3, v: V3, size: f32, seek: Option<usize>, home: Option<V3> },
    /// a path drawn progressively by a racing spark, then fading
    Trace { pts: Vec<V3>, w: f32 },
    /// a soft disc that blooms and fades
    Flash { c: V3, r: f32 },
    /// a short tapered stroke that fades (dart trails)
    Streak { a: V3, b: V3, w: f32 },
    /// a soul flattened by a blow (angle of the hit), then gone
    Blob { c: V3, a: f32, r: f32 },
}

#[derive(Clone, Debug)]
pub struct Fx {
    pub kind: Kind,
    pub screen: bool,
    pub t0: f32,
    pub life: f32,
    pub color: [f32; 4],
    pub seed: u32,
}

/// Damped spring from 0 to 1 with one soft overshoot (t in seconds).
pub fn spring(t: f32) -> f32 {
    if t <= 0.0 {
        return 0.0;
    }
    1.0 - (-t * 11.0).exp() * (t * 19.0).cos()
}

pub fn ease_out(u: f32) -> f32 {
    let u = u.clamp(0.0, 1.0);
    1.0 - (1.0 - u) * (1.0 - u) * (1.0 - u)
}

pub fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846ca68b);
    x ^ (x >> 16)
}

/// [-1, 1) from a seed
pub fn hf(x: u32) -> f32 {
    (hash(x) % 20000) as f32 / 10000.0 - 1.0
}

/// What a UI element looked like last tick (for release / re-form).
#[derive(Clone, Debug)]
pub struct Snap {
    pub quad: [[f32; 2]; 4],
    pub text: String,
    pub color: [f32; 4],
    pub since: f32,
    pub button: bool,
}

/// An obstacle flexing where it was struck (render-only).
#[derive(Clone, Debug)]
pub struct Flex {
    pub node: usize,
    pub p: V3,
    pub dir: V3,
    pub t0: f32,
    pub amp: f32,
}

#[derive(Default)]
pub struct Motion {
    pub flex: Vec<Flex>,
    /// boxes nudged by nearby blows: node -> (offset, velocity)
    pub jostles: HashMap<usize, (V3, V3)>,
    pub fx: Vec<Fx>,
    /// game time of the last fixed step (s)
    pub now: f32,
    /// game time the frame is being drawn at (s)
    pub render_now: f32,
    /// fixed steps left to freeze the match for
    pub hitstop: u32,
    /// node -> time its Form began (may be in the future: staggered)
    pub forms: HashMap<usize, f32>,
    pub snaps: HashMap<usize, Snap>,
    pub last_action: f32,
    /// camera shake: strength and start time; drawn at render time only
    pub shake: f32,
    pub shake_t0: f32,
    pub hover: Option<usize>,
    pub hover_since: f32,
    pub cursor_trail: VecDeque<[f32; 2]>,
    /// eased camera breath factor (1 = framing, <1 = leaning in)
    pub breath: f32,
    pub seed: u32,
    pub booted: bool,
    /// engine-drawn menu glyphs: (button node, 0 = left, 1 = right, 2 = back)
    pub glyphs: Vec<(usize, u8)>,
    /// button node -> last time it fired (for the jab)
    pub pressed_at: HashMap<usize, f32>,
}

pub struct Tex {
    pub disc: f32,
    pub soft: f32,
    pub ring: f32,
}

impl Motion {
    fn next_seed(&mut self) -> u32 {
        self.seed = self.seed.wrapping_add(0x9e3779b9);
        hash(self.seed)
    }

    pub fn push(&mut self, kind: Kind, screen: bool, delay: f32, life: f32, color: [f32; 4]) {
        let seed = self.next_seed();
        self.fx.push(Fx { kind, screen, t0: self.now + delay, life, color, seed });
    }

    pub fn ring(&mut self, screen: bool, c: V3, r0: f32, r1: f32, w: f32, life: f32, color: [f32; 4]) {
        self.push(Kind::Ring { c, r0, r1, w }, screen, 0.0, life, color);
    }

    pub fn ring_at(&mut self, screen: bool, delay: f32, c: V3, r0: f32, r1: f32, w: f32, life: f32, color: [f32; 4]) {
        self.push(Kind::Ring { c, r0, r1, w }, screen, delay, life, color);
    }

    pub fn arc(&mut self, screen: bool, a: V3, b: V3, w: f32, jitter: f32, life: f32, color: [f32; 4]) {
        self.push(Kind::Arc { a, b, w, jitter }, screen, 0.0, life, color);
    }

    pub fn flash(&mut self, screen: bool, c: V3, r: f32, life: f32, color: [f32; 4]) {
        self.push(Kind::Flash { c, r }, screen, 0.0, life, color);
    }

    pub fn trace(&mut self, screen: bool, delay: f32, pts: Vec<V3>, w: f32, life: f32, color: [f32; 4]) {
        if pts.len() >= 2 {
            self.push(Kind::Trace { pts, w }, screen, delay, life, color);
        }
    }

    pub fn trace_circle(&mut self, screen: bool, delay: f32, c: V3, r: f32, start: f32, w: f32, life: f32, color: [f32; 4]) {
        let pts = (0..=40)
            .map(|k| {
                let a = start - k as f32 / 40.0 * std::f32::consts::TAU;
                c + v3(a.cos() * r, a.sin() * r, 0.0)
            })
            .collect();
        self.trace(screen, delay, pts, w, life, color);
    }

    /// Sparks flying out from c.
    pub fn burst(&mut self, screen: bool, c: V3, n: usize, speed: f32, size: f32, life: f32, color: [f32; 4], seek: Option<usize>) {
        for _ in 0..n {
            let s = self.next_seed();
            // dropout: some never show; the rest are heavy-tailed in speed and
            // size (most small and slow, the odd one big and fast), and their
            // directions are not an even fan
            if (hash(s ^ 31) % 100) < 40 {
                continue;
            }
            let a = (hf(s) * 0.5 + 0.5) * std::f32::consts::TAU;
            let tail = |x: u32, k: f32| ((hf(x) * 0.5 + 0.5).max(1e-3)).powf(-1.0 / k);
            let sp = speed * (0.35 * tail(s ^ 77, 2.2)).min(1.6);
            let sz = size * (0.6 * tail(s ^ 55, 3.0)).min(2.2);
            let v = v3(a.cos() * sp, a.sin() * sp, 0.0);
            let l = life * (0.5 + 0.5 * (hf(s ^ 99) * 0.5 + 0.5));
            let delay = (hf(s ^ 41) * 0.5 + 0.5).powi(3) * 0.08;
            self.push(Kind::Spark { p: c, v, size: sz, seek, home: None }, screen, delay, l, color);
        }
    }

    /// Sparks that gather from a ring of radius r into c, arriving after `life`.
    pub fn converge(&mut self, screen: bool, c: V3, n: usize, r: f32, size: f32, life: f32, color: [f32; 4]) {
        for k in 0..n {
            let s = self.next_seed();
            let a = k as f32 / n as f32 * std::f32::consts::TAU + hf(s) * 0.4;
            let rr = r * (0.7 + 0.3 * (hf(s ^ 5) * 0.5 + 0.5));
            let p = c + v3(a.cos() * rr, a.sin() * rr, 0.0);
            // tangential start velocity: they spiral in
            let v = v3(-a.sin(), a.cos(), 0.0) * (rr * 2.0);
            self.push(Kind::Spark { p, v, size, seek: None, home: Some(c) }, screen, 0.0, life, color);
        }
    }

    /// Break a quad (bl, br, tr, tl) into spinning triangles.
    pub fn shatter(&mut self, screen: bool, q: [V3; 4], cells: (usize, usize), speed: f32, life: f32, color: [f32; 4], seek: Option<usize>, push_dir: V3) {
        let (nx, ny) = cells;
        let center = (q[0] + q[1] + q[2] + q[3]) * 0.25;
        let lerp = |u: f32, v: f32| {
            let bottom = q[0] + (q[1] - q[0]) * u;
            let top = q[3] + (q[2] - q[3]) * u;
            bottom + (top - bottom) * v
        };
        for ix in 0..nx {
            for iy in 0..ny {
                let s = self.next_seed();
                let j = |k: u32, a: f32, b: f32| -> f32 {
                    // jitter interior grid points so cracks are irregular
                    let v = a + hf(s ^ k) * 0.18 / b;
                    v.clamp(0.0, 1.0)
                };
                let (u0, u1) = (ix as f32 / nx as f32, (ix + 1) as f32 / nx as f32);
                let (v0, v1) = (iy as f32 / ny as f32, (iy + 1) as f32 / ny as f32);
                let c = [
                    lerp(if ix == 0 { 0.0 } else { j(1, u0, nx as f32) }, v0),
                    lerp(u1, if iy == 0 { 0.0 } else { j(2, v0, ny as f32) }),
                    lerp(if ix + 1 == nx { 1.0 } else { j(3, u1, nx as f32) }, v1),
                    lerp(u0, if iy + 1 == ny { 1.0 } else { j(4, v1, ny as f32) }),
                ];
                let tris = if s & 1 == 0 { [[c[0], c[1], c[2]], [c[0], c[2], c[3]]] } else { [[c[0], c[1], c[3]], [c[1], c[2], c[3]]] };
                for (ti, t) in tris.iter().enumerate() {
                    let s2 = hash(s ^ (ti as u32 + 11));
                    let p = (t[0] + t[1] + t[2]) * (1.0 / 3.0);
                    let tri = [[t[0].x - p.x, t[0].y - p.y], [t[1].x - p.x, t[1].y - p.y], [t[2].x - p.x, t[2].y - p.y]];
                    let out = p - center;
                    let ol = (out.x * out.x + out.y * out.y).sqrt().max(1e-4);
                    let dir = v3(out.x / ol, out.y / ol, 0.0);
                    let k = speed * (0.3 * ((hf(s2) * 0.5 + 0.5).max(1e-3)).powf(-1.0 / 2.5)).min(1.4);
                    let v = dir * k + v3(hf(s2 ^ 3), hf(s2 ^ 4), 0.0) * (speed * 0.35) + push_dir;
                    let spin = hf(s2 ^ 9) * 9.0;
                    let l = life * (0.7 + 0.3 * (hf(s2 ^ 13) * 0.5 + 0.5));
                    self.push(Kind::Shard { p, v, rot: 0.0, spin, tri, seek }, screen, 0.0, l, color);
                }
            }
        }
    }

    /// One fixed step. `locate` gives a node's position in the effect's space.
    pub fn step(&mut self, dt: f32, locate: impl Fn(usize, bool) -> Option<V3>) {
        self.now += dt;
        let now = self.now;
        for f in self.fx.iter_mut() {
            if now < f.t0 {
                continue;
            }
            let age = now - f.t0;
            match &mut f.kind {
                Kind::Shard { p, v, rot, spin, seek, .. } => {
                    *rot += *spin * dt;
                    if let Some(t) = seek.and_then(|n| locate(n, f.screen)) {
                        // drift first, then get pulled in harder and harder
                        let pull = ((age / f.life) - 0.25).max(0.0) * 4.0;
                        let d = t - *p;
                        *v = *v * (1.0 - 3.0 * dt) + v3(d.x, d.y, 0.0) * (pull * 26.0 * dt);
                    } else {
                        *v = *v * (1.0 - 2.4 * dt);
                    }
                    *p = *p + *v * dt;
                }
                Kind::Spark { p, v, seek, home, .. } => {
                    let target = seek.and_then(|n| locate(n, f.screen)).or(*home);
                    if let Some(t) = target {
                        let u = (age / f.life).min(1.0);
                        let d = t - *p;
                        let pull = if home.is_some() { 30.0 + 160.0 * u } else { 4.0 + 60.0 * u };
                        *v = *v * (1.0 - 5.0 * dt) + v3(d.x, d.y, 0.0) * (pull * dt);
                    } else {
                        *v = *v * (1.0 - 3.0 * dt);
                    }
                    *p = *p + *v * dt;
                }
                _ => {}
            }
        }
        self.fx.retain(|f| now < f.t0 + f.life);
        self.flex.retain(|f| now - f.t0 < 0.8);
        // jostled boxes spring home
        let (k, c) = (160.0f32, 2.0 * 160.0f32.sqrt() * 0.3);
        for (_, (x, v)) in self.jostles.iter_mut() {
            *v = *v + (*x * -k - *v * c) * dt;
            *x = *x + *v * dt;
        }
        self.jostles.retain(|_, (x, v)| x.length() + v.length() > 1e-3);
        self.forms.retain(|_, t| now < *t + 3.0);
    }

    pub fn jostle(&mut self, node: usize, kick: V3) {
        let e = self.jostles.entry(node).or_insert((V3::ZERO, V3::ZERO));
        e.1 = e.1 + kick;
    }

    /// Displacement of a world point on a flexing obstacle.
    pub fn flex_at(&self, node: usize, q: V3) -> V3 {
        let mut d = V3::ZERO;
        for f in self.flex.iter().filter(|f| f.node == node) {
            let age = self.render_now - f.t0;
            if age < 0.0 {
                continue;
            }
            let r2 = (q.x - f.p.x).powi(2) + (q.y - f.p.y).powi(2);
            let w = (-r2 / 0.3).exp() * (age * 38.0).sin() * (-age * 7.0).exp();
            d = d + f.dir * (f.amp * w);
        }
        d
    }

    /// Add a smooth, decaying shake (strongest wins).
    pub fn add_shake(&mut self, amount: f32) {
        let left = self.shake_offset_amp(self.now);
        if amount > left {
            self.shake = amount;
            self.shake_t0 = self.now;
        }
    }

    fn shake_amp_at(&self, t: f32) -> f32 {
        let a = t - self.shake_t0;
        if a < 0.0 {
            return 0.0;
        }
        self.shake * (-a * 7.0).exp()
    }

    fn shake_offset_amp(&self, t: f32) -> f32 {
        self.shake_amp_at(t)
    }

    /// Camera offset for the frame being drawn: layered sines, no jitter.
    pub fn shake_offset(&self) -> V3 {
        let t = self.render_now;
        let a = self.shake_amp_at(t);
        if a < 1e-4 {
            return V3::ZERO;
        }
        let x = (t * 37.0).sin() * 0.6 + (t * 61.0 + 1.3).sin() * 0.4;
        let y = (t * 43.0 + 0.7).sin() * 0.6 + (t * 71.0 + 2.1).sin() * 0.4;
        v3(x, y, 0.0) * a
    }

    /// Form progress of a node at render time: (scale, alpha, reveal).
    pub fn form(&self, node: usize) -> Option<(f32, f32, f32)> {
        let t0 = *self.forms.get(&node)?;
        let t = self.render_now - t0;
        if t < 0.0 {
            return Some((0.0, 0.0, 0.0));
        }
        Some((spring(t), (t / 0.12).min(1.0), (t / 0.55).min(1.0)))
    }

    /// Draw every effect in one space. World effects get the view distance
    /// from `depth` for sorting.
    pub fn emit(&self, screen: bool, tex: &Tex, depth: impl Fn(V3) -> f32, mut push: impl FnMut([f32; RECORD], f32)) {
        let ahead = (self.render_now - self.now).clamp(0.0, 0.05);
        // flicker clock: arcs re-roll ~30 times a second
        let flick = (self.render_now * 30.0) as u32;
        for f in self.fx.iter().filter(|f| f.screen == screen) {
            let t = self.render_now - f.t0;
            if t < 0.0 || t > f.life {
                continue;
            }
            let u = t / f.life;
            let mut col = f.color;
            match &f.kind {
                Kind::Ring { c, r0, r1, w } => {
                    // never a perfect circle: 1-4 arcs with irregular gaps
                    let r = r0 + (r1 - r0) * ease_out(u);
                    col[3] *= 0.6 * (1.0 - u).powf(1.6);
                    let d = depth(*c);
                    let segs = 1 + ((hf(f.seed ^ 3) * 0.5 + 0.5).powi(2) * 4.0) as u32;
                    let mut a0 = (hf(f.seed ^ 5) * 0.5 + 0.5) * std::f32::consts::TAU;
                    for s in 0..segs {
                        let len = 0.4 + (hf(f.seed ^ (s * 7 + 11)) * 0.5 + 0.5) * 1.6;
                        let n = ((len / 0.12) as usize).max(3);
                        let pts: Vec<V3> = (0..=n)
                            .map(|k| {
                                let a = a0 + len * k as f32 / n as f32;
                                *c + v3(a.cos() * r, a.sin() * r, 0.0)
                            })
                            .collect();
                        ribbon(&pts, w * 0.65 * (1.0 - 0.6 * u), col, -1.0, |rec| push(rec, d));
                        a0 += len + 0.3 + (hf(f.seed ^ (s * 13 + 17)) * 0.5 + 0.5) * 1.2;
                    }
                }
                Kind::Arc { a, b, w, jitter } => {
                    // a katana blade with a bow fixed for its life; it dies by
                    // dropping out in stutters, more often as it ages
                    let epoch = (self.render_now * 25.0) as u32;
                    if u > 0.25 && (hash(f.seed ^ epoch) % 1000) as f32 / 1000.0 < u * 0.8 {
                        continue;
                    }
                    let bow = (if f.seed & 1 == 1 { 1.0 } else { -1.0 }) * (0.08 + (hf(f.seed ^ 9) * 0.5 + 0.5) * 0.22) * (0.5 + jitter.min(1.0));
                    let d = depth((*a + *b) * 0.5);
                    col[3] *= (1.0 - u).powf(0.7);
                    let cap = if f.screen { 3.5 } else { crate::shards::MAX_W * 1.4 };
                    blade(*a, *b, bow, w * 1.6, cap, col, |rec| push(rec, d));
                }
                Kind::Shard { p, v, rot, spin, tri, .. } => {
                    let pp = *p + *v * ahead;
                    let r = rot + spin * ahead;
                    let (s, c) = r.sin_cos();
                    // shrink as they die
                    let k = 1.0 - u * u;
                    let pt = |q: [f32; 2]| pp + v3((q[0] * c - q[1] * s) * k, (q[0] * s + q[1] * c) * k, 0.0);
                    col[3] *= (1.0 - u).powf(0.8);
                    let (a, b, cc) = (pt(tri[0]), pt(tri[1]), pt(tri[2]));
                    push(quad(-1.0, [a, b, cc, cc], [0.0, 0.0, 1.0, 1.0], col), depth(pp));
                }
                Kind::Spark { p, v, size, .. } => {
                    let pp = *p + *v * ahead;
                    let sz = size * (1.0 - u * 0.7);
                    col[3] *= 1.0 - u * u;
                    let d = depth(pp);
                    if tex.disc >= 0.0 {
                        push(quad(tex.disc, [pp + v3(-sz, -sz, 0.0), pp + v3(sz, -sz, 0.0), pp + v3(sz, sz, 0.0), pp + v3(-sz, sz, 0.0)], [0.0, 0.0, 1.0, 1.0], col), d);
                    }
                    // motion tail
                    let tail = pp - *v * 0.035;
                    ribbon_tapered(&[tail, pp], [0.0, sz * 1.2], col, -1.0, |rec| push(rec, d + 0.0005));
                }
                Kind::Trace { pts, w } => {
                    // draw 0..60% of life, hold, then fade
                    let draw = ease_out((u / 0.6).min(1.0));
                    let fade = if u < 0.6 { 1.0 } else { 1.0 - (u - 0.6) / 0.4 };
                    let n = pts.len() - 1;
                    let head = draw * n as f32;
                    let full = head.floor() as usize;
                    let mut path: Vec<V3> = pts[..=full.min(n)].to_vec();
                    if full < n {
                        let fr = head - full as f32;
                        path.push(pts[full] + (pts[full + 1] - pts[full]) * fr);
                    }
                    if path.len() < 2 {
                        continue;
                    }
                    let hp = *path.last().unwrap();
                    let d = depth(hp);
                    col[3] *= fade;
                    // thin at the tail, full at the head; a gap or two where the ink skipped
                    let m = path.len() - 1;
                    let gaps = [(hf(f.seed ^ 21) * 0.5 + 0.5) * 0.8 + 0.1, (hf(f.seed ^ 23) * 0.5 + 0.5) * 0.8 + 0.1];
                    let two = f.seed % 3 == 0;
                    for k in 0..m {
                        let g = k as f32 / n as f32;
                        if (g - gaps[0]).abs() < 0.025 || (two && (g - gaps[1]).abs() < 0.02) {
                            continue;
                        }
                        let (w0, w1) = (*w * (0.25 + 0.75 * k as f32 / m as f32), *w * (0.25 + 0.75 * (k + 1) as f32 / m as f32));
                        ribbon_tapered(&path[k..=k + 1], [w0, w1], col, -1.0, |rec| push(rec, d));
                    }
                    // the racing spark at the head, with a crackle
                    if u < 0.62 && tex.disc >= 0.0 {
                        let sz = w * 3.0;
                        let mut hc = col;
                        hc[3] = f.color[3];
                        push(quad(tex.disc, [hp + v3(-sz, -sz, 0.0), hp + v3(sz, -sz, 0.0), hp + v3(sz, sz, 0.0), hp + v3(-sz, sz, 0.0)], [0.0, 0.0, 1.0, 1.0], hc), d - 0.001);
                        let s = hash(f.seed ^ flick);
                        let tip = hp + v3(hf(s), hf(s ^ 1), 0.0) * (w * 9.0);
                        let ap = arc_points(hp, tip, 4, 0.5, s);
                        ribbon(&ap, w * 0.6, hc, -1.0, |rec| push(rec, d - 0.001));
                    }
                }
                Kind::Streak { a, b, w } => {
                    col[3] *= (1.0 - u) * (1.0 - u);
                    ribbon_tapered(&[*a, *b], [0.0, w * (1.0 - 0.5 * u)], col, -1.0, |rec| push(rec, depth(*b)));
                }
                Kind::Blob { c, a, r } => {
                    // squashed along the hit, spreading across it, then gone
                    let along = r * (0.7 - 0.45 * ease_out(u));
                    let across = r * (1.3 + 0.9 * ease_out(u));
                    col[3] *= 1.0 - u * u;
                    let (s, co) = a.sin_cos();
                    let ax = v3(co, s, 0.0) * along;
                    let ay = v3(-s, co, 0.0) * across;
                    if tex.disc >= 0.0 {
                        push(quad(tex.disc, [*c - ax - ay, *c + ax - ay, *c + ax + ay, *c - ax + ay], [0.0, 0.0, 1.0, 1.0], col), depth(*c));
                    }
                }
                Kind::Flash { c, r } => {
                    // ink on paper, not glow: barely there
                    let rr = r * 0.7 * (0.6 + 0.6 * ease_out(u));
                    col[3] *= 0.3 * (1.0 - u).powi(2);
                    if tex.soft >= 0.0 {
                        push(quad(tex.soft, [*c + v3(-rr, -rr, 0.0), *c + v3(rr, -rr, 0.0), *c + v3(rr, rr, 0.0), *c + v3(-rr, rr, 0.0)], [0.0, 0.0, 1.0, 1.0], col), depth(*c));
                    }
                }
            }
        }
    }
}
