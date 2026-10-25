//! Box-shatter debris and its electricity, ported from Shard Lab
//! (prototypes/shardlab/index.html; its slider defaults are the spec).
//!
//! A shattered box becomes 13 ink shards. They burst lopsided and
//! heavy-tailed, bond in pairs and short chains that dance and get bored,
//! wander with moods, and run out of lives into a last gasp and a quiet
//! fade. Electricity is critical branching cascades with dropout, drawn as
//! tapered "katana" blades. Stepped at 50 Hz; drawing is in render.rs.

use std::collections::{HashMap, HashSet};

use crate::shared::rng::Rng;

pub const PER_BOX: usize = 13;
const DT: f32 = 0.02;

// the lab's slider defaults
const INHERIT: f32 = 0.015;
const RADIAL: f32 = 0.14;
const AIM: f32 = 0.7;
const TANGENTIAL: f32 = 0.3;
const CHARGE_PER_E: f32 = 1.0;
const K_REP: f32 = 0.15;
const REP_RANGE: f32 = 0.8;
const K_ATT: f32 = 7.0;
const DRAG: f32 = 4.5;
const DRAG_START: f32 = 1.2;
const DRAG_RAMP: f32 = 0.6;
const DECAY: f32 = 1.6;
const D0: f32 = 0.8;
const V0: f32 = 3.0;
const LINK_RATE: f32 = 0.4;
const SNAP_V: f32 = 9.0;
const MAX_LINKS: u8 = 2;
const DANCE: f32 = 6.0;
const SWIRL: f32 = 2.0;
const FLICKER: f32 = 0.35;
const REWIRE: f32 = 0.3;
const BOLT_RATE: f32 = 0.25;
const BOREDOM: f32 = 0.3;
const COOLDOWN: f32 = 0.8;
const PART_KICK: f32 = 1.2;
pub const MAX_LINK_DIST: f32 = 0.6;
const LIVES: i32 = 3;
const LONELY: f32 = 1.1;
const RAVE: f32 = 4.0;
const BRANCH: f32 = 0.95;
pub const DROPOUT: f32 = 0.55;
const RAVE_DIST: f32 = 1.1;
const ANIM: f32 = 4.0;
const ANIM_NEAR: f32 = 0.3;
const ANIM_RANGE: f32 = 1.2;
const MOOD: f32 = 0.5;
const WANDER: f32 = 2.5;
const DART: f32 = 0.8;
pub const MAX_W: f32 = 0.035;
pub const ARC_W: f32 = 0.02;
pub const HOP: f32 = 0.3;
const GASP: f32 = 0.4;
const GASP_RANGE: f32 = 2.2;
const GASP_CHARGE: f32 = 0.6;
const REVIVE: i32 = 1;
pub const FADE_T: f32 = 0.5;
const TRANSIT: f32 = 2.5;
const BOLT_THR: f32 = 1.6;
const BOLT_RANGE: f32 = 4.0;
const BOLT_SHARE: f32 = 0.35;

#[derive(Clone, Debug)]
pub struct Shard {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub vx: f32,
    pub vy: f32,
    pub imp: usize,
    lives: i32,
    alone: f32,
    pub dead: bool,
    pub gasp: f32,
    pub fade: f32,
    revived: i32,
    pub q: f32,
    pub a: f32,
    pub w: f32,
    pub size: f32,
    pub tri: [[f32; 2]; 3],
    mood: f32,
    hd: f32,
    sparked: bool,
}

#[derive(Clone, Debug)]
pub struct Line {
    pub a: usize,
    pub b: usize,
    pub born: u64,
    pub life: u64,
    pub snap: bool,
    pub w: f32,
    pub bow: f32,
    pub show: bool,
}

#[derive(Clone, Debug)]
struct Pending {
    from: usize,
    gen: u32,
    at: u64,
}

#[derive(Clone, Debug)]
pub struct Impact {
    pub x: f32,
    pub y: f32,
    pub t: u64,
    pub lines: Vec<Line>,
    queue: Vec<Pending>,
}

#[derive(Clone, Debug)]
pub struct Bolt {
    pub a: usize,
    pub b: usize,
    pub life: i32,
}

pub fn key(i: usize, j: usize) -> u32 {
    let (i, j) = if i < j { (i, j) } else { (j, i) };
    (i * 4096 + j) as u32
}

pub fn unkey(k: u32) -> (usize, usize) {
    ((k / 4096) as usize, (k % 4096) as usize)
}

pub struct ShardSim {
    pub shards: Vec<Shard>,
    pub links: HashMap<u32, f32>,
    age: HashMap<u32, f32>,
    cool: HashMap<u32, u64>,
    pub active: HashSet<u32>,
    /// per active bond: 0 = flickered off this tick, else brightness
    pub vis: HashMap<u32, f32>,
    pub bolts: Vec<Bolt>,
    pub imps: Vec<Impact>,
    pub t: u64,
    /// sound events since the last drain: (0 spark line, 1 bolt), x, y, size
    pub events: Vec<(u8, f32, f32, f32)>,
    beat_start: u64,
    beat_next: u64,
    beat_len: u64,
    rng: Rng,
}

impl ShardSim {
    pub fn new(seed: u64) -> ShardSim {
        ShardSim {
            shards: Vec::new(),
            links: HashMap::new(),
            age: HashMap::new(),
            cool: HashMap::new(),
            active: HashSet::new(),
            vis: HashMap::new(),
            bolts: Vec::new(),
            imps: Vec::new(),
            t: 0,
            events: Vec::new(),
            beat_start: 0,
            beat_next: 0,
            beat_len: 4,
            rng: Rng::new(seed),
        }
    }

    fn r(&mut self) -> f32 {
        self.rng.rangef(0.0, 1.0)
    }

    pub fn live(&self) -> usize {
        self.shards.iter().filter(|s| !s.dead).count()
    }

    /// A box breaks at (x, y): its velocity reflected off what it hit, its
    /// throw speed, and its spin in degrees per tick.
    pub fn shatter(&mut self, x: f32, y: f32, z: f32, refl: [f32; 2], speed: f32, spin_deg: f32) {
        let (rvx, rvy) = (refl[0], refl[1]);
        let e = 0.5 * speed * speed;
        let spin_rad = spin_deg.to_radians() * 50.0;
        let imp = self.imps.len();
        self.imps.push(Impact { x, y, t: self.t, lines: Vec::new(), queue: Vec::new() });
        // 2-4 chunks; weights rng^-1.5 so one usually dominates
        let n_chunks = 2 + (self.r() * 3.0) as usize;
        let mut chunks = Vec::new();
        for _ in 0..n_chunks {
            let ang = self.r() * 6.283;
            let u = (0.04 * self.r().max(1e-6).powf(-1.0 / 1.2)).min(1.2);
            let spread = 0.1 + self.r() * 0.8;
            let wt = self.r().max(1e-6).powf(-1.5);
            chunks.push((ang, u, spread, wt));
        }
        let wsum: f32 = chunks.iter().map(|c| c.3).sum();
        let bounce = rvy.atan2(rvx);
        for _ in 0..PER_BOX {
            let mut pick = self.r() * wsum;
            let mut ch = chunks[0];
            for c in &chunks {
                pick -= c.3;
                if pick <= 0.0 {
                    ch = *c;
                    break;
                }
            }
            let ang = ch.0 + (self.r() - 0.5) * 2.0 * ch.2;
            let r = 0.04 + self.r().powf(0.7) * 0.24;
            let (px, py) = (x + ang.cos() * r, y + ang.sin() * r);
            let breakaway = if self.r() < 0.06 { 0.05 * self.r().max(1e-6).powf(-1.0 / 1.1) } else { 0.0 };
            let u = (ch.1 * self.r().max(1e-6).powf(-1.0 / 2.5) + breakaway).min(1.4);
            let rad = RADIAL * speed * u;
            let mut da = ang - bounce;
            da = da.sin().atan2(da.cos());
            let dir = ang - da * AIM * u.sqrt();
            let (tx, ty) = (-ang.sin(), ang.cos());
            let tan = spin_rad * r * TANGENTIAL;
            let q = (e / 2500.0) * CHARGE_PER_E * (0.7 + self.r() * 0.6);
            let a = self.r() * 6.28;
            let w = (self.r() - 0.5) * 20.0 * (0.3 + self.r().powi(2) * 2.0);
            let size = 0.035 + 0.075 * self.r().powf(2.2);
            let mut tri = [[0.0f32; 2]; 3];
            for (k, t) in tri.iter_mut().enumerate() {
                let an = k as f32 * 2.09 + (self.r() - 0.5) * 1.3;
                let rr = 0.45 + self.r() * 0.75;
                *t = [an.cos() * rr, an.sin() * rr];
            }
            let hd = self.r() * 6.283;
            self.shards.push(Shard {
                x: px,
                y: py,
                z,
                vx: rvx * INHERIT + dir.cos() * rad + tx * tan,
                vy: rvy * INHERIT + dir.sin() * rad + ty * tan,
                imp,
                lives: LIVES,
                alone: 0.0,
                dead: false,
                gasp: -1.0,
                fade: -1.0,
                revived: 0,
                q,
                a,
                w,
                size,
                tri,
                mood: 1.0,
                hd,
                sparked: false,
            });
        }
    }

    fn age_of(&self, s: &Shard) -> f32 {
        (self.t - self.imps[s.imp].t) as f32 * DT
    }

    /// One 50 Hz step. `solid(x0, y0, x1, y1)` returns the fraction along
    /// the move where a shard hits an obstacle, if it does.
    pub fn step(&mut self, solid: impl Fn(f32, f32, f32, f32) -> Option<f32>) {
        self.t += 1;
        let t = self.t;
        if self.shards.is_empty() {
            return;
        }
        let n = self.shards.len();
        let q_decay = 0.5f32.powf(DT / DECAY);
        // the beat drifts: gaps of 0.25-0.85 s, pulses of uneven length
        if t >= self.beat_next {
            self.beat_start = t;
            self.beat_len = 2 + (self.r() * 6.0) as u64;
            self.beat_next = t + 12 + (self.r().powf(1.5) * 30.0) as u64;
        }
        let phase = t - self.beat_start;
        let beat = phase < self.beat_len;
        // each shard keeps only its strongest few arcs: pairs and short chains
        self.active.clear();
        let mut deg = vec![0u8; n];
        let mut sorted: Vec<(u32, f32)> = self.links.iter().map(|(&k, &s)| (k, s)).collect();
        sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap().then(a.0.cmp(&b.0)));
        for (k, sv) in sorted {
            let (i, j) = unkey(k);
            if sv > 0.05 && deg[i] < MAX_LINKS && deg[j] < MAX_LINKS {
                self.active.insert(k);
                deg[i] += 1;
                deg[j] += 1;
            }
        }
        // partners switch on the beat
        if phase == 0 {
            let act: Vec<u32> = self.active.iter().copied().collect();
            for k in act {
                if self.r() < REWIRE {
                    self.links.insert(k, 0.0);
                    self.active.remove(&k);
                }
            }
        }
        self.vis.clear();
        let act: Vec<u32> = self.active.iter().copied().collect();
        for &k in &act {
            let (i, j) = unkey(k);
            let g = self.shards[i].gasp >= 0.0 || self.shards[j].gasp >= 0.0;
            let flick = if self.r() < FLICKER { 0.0 } else { 1.0 };
            self.vis.insert(k, if g { 1.0 } else { flick * if beat { 1.0 } else { 0.65 } });
            // a gasping shard's arc wakes a healthy one: +1 life (once each)
            for (x, y) in [(i, j), (j, i)] {
                let (sx, sy) = (&self.shards[x], &self.shards[y]);
                if sx.gasp >= 0.0 && !sx.sparked && sy.gasp < 0.0 && sy.fade < 0.0 && sy.revived < REVIVE {
                    self.shards[x].sparked = true;
                    let s = &mut self.shards[y];
                    s.revived += 1;
                    s.lives += 1;
                    s.alone = 0.0;
                    s.q += GASP_CHARGE * 0.5;
                }
            }
        }
        // active neighbours (at most two each): friend-of-friend in O(1)
        let mut nb: Vec<[usize; 2]> = vec![[usize::MAX; 2]; n];
        for &k in &act {
            let (i, j) = unkey(k);
            for (a, b) in [(i, j), (j, i)] {
                if nb[a][0] == usize::MAX {
                    nb[a][0] = b;
                } else {
                    nb[a][1] = b;
                }
            }
        }
        let mut fx = vec![0.0f32; n];
        let mut fy = vec![0.0f32; n];
        let far = (MAX_LINK_DIST * GASP_RANGE).max(ANIM_RANGE).max(REP_RANGE);
        for i in 0..n {
            if self.shards[i].dead {
                continue;
            }
            for j in i + 1..n {
                let k = key(i, j);
                let (a, b) = (&self.shards[i], &self.shards[j]);
                if b.dead {
                    self.links.remove(&k);
                    continue;
                }
                let (dx, dy) = (b.x - a.x, b.y - a.y);
                let d = (dx * dx + dy * dy).sqrt().max(1e-4);
                if d > far {
                    if self.links.remove(&k).is_some() {
                        self.age.remove(&k);
                    }
                    continue;
                }
                let dv = ((b.vx - a.vx).powi(2) + (b.vy - a.vy).powi(2)).sqrt();
                let mut s = self.links.get(&k).copied().unwrap_or(0.0);
                let cool_until = self.cool.get(&k).copied().unwrap_or(0);
                let gasping = a.gasp >= 0.0 || b.gasp >= 0.0;
                let reach = MAX_LINK_DIST * if gasping { GASP_RANGE } else { 1.0 };
                if dv > SNAP_V || t < cool_until || d > reach || a.fade >= 0.0 || b.fade >= 0.0 {
                    s = 0.0;
                } else {
                    let (ni, nj) = (nb[i], nb[j]);
                    let has = |l: [usize; 2], m: usize| l[0] == m || l[1] == m;
                    let common = ni.iter().filter(|&&m| m != usize::MAX && m != j && has(nj, m)).count() as f32;
                    let mut union: Vec<usize> = ni.iter().chain(nj.iter()).copied().filter(|&m| m != usize::MAX && m != i && m != j).collect();
                    union.sort_unstable();
                    union.dedup();
                    let via_one = union.len() as f32;
                    let aff = (-d / (D0 * (1.0 + TRANSIT * 0.25 * via_one.min(2.0)))).exp() * (-dv / V0).exp() * (1.0 + TRANSIT * common);
                    let target = aff * ((a.q + b.q) * 2.0).min(1.0);
                    s += (target - s) * LINK_RATE;
                }
                let is_active = self.active.contains(&k);
                // boredom: an arc lives a short while, then the pair parts ways
                let (mut kick_a, mut kick_b) = ((0.0, 0.0), (0.0, 0.0));
                let (ux0, uy0) = (dx / d, dy / d);
                if is_active {
                    let a0 = self.age.get(&k).copied().unwrap_or(0.0) + DT;
                    if a0 > BOREDOM {
                        s = 0.0;
                        self.age.remove(&k);
                        self.cool.insert(k, t + (COOLDOWN / DT).round() as u64);
                        let kx = (ux0 - uy0 * 0.6) * PART_KICK;
                        let ky = (uy0 + ux0 * 0.6) * PART_KICK;
                        kick_a = (-kx, -ky);
                        kick_b = (kx, ky);
                    } else {
                        self.age.insert(k, a0);
                    }
                } else {
                    self.age.remove(&k);
                }
                let (qa, qb, ma, mb) = (a.q, b.q, a.mood, b.mood);
                if s < 0.01 {
                    self.links.remove(&k);
                } else {
                    self.links.insert(k, s);
                }
                if kick_a.0 != 0.0 || kick_a.1 != 0.0 {
                    let sa = &mut self.shards[i];
                    sa.lives -= 1;
                    sa.vx += kick_a.0;
                    sa.vy += kick_a.1;
                    let sb = &mut self.shards[j];
                    sb.lives -= 1;
                    sb.vx += kick_b.0;
                    sb.vy += kick_b.1;
                }
                let (ux, uy) = (dx / d, dy / d);
                // like-charge repulsion
                if d < REP_RANGE {
                    let f = K_REP * (qa + 0.05) * (qb + 0.05) / (d * d + 0.04) * (1.0 - d / REP_RANGE);
                    fx[i] -= ux * f;
                    fy[i] -= uy * f;
                    fx[j] += ux * f;
                    fy[j] += uy * f;
                }
                // animacy: close shards lean in, farther ones drift apart; moods flip it
                if d < ANIM_RANGE {
                    let prof = if d < ANIM_NEAR { 1.0 } else { -0.45 * (1.0 - (d - ANIM_NEAR) / (ANIM_RANGE - ANIM_NEAR)) };
                    let f = ANIM * prof * (ma + mb) / 2.0;
                    fx[i] += ux * f;
                    fy[i] += uy * f;
                    fx[j] -= ux * f;
                    fy[j] -= uy * f;
                }
                if is_active {
                    // spring to a preferred spacing, plus a sideways orbit
                    let f = K_ATT * s * (d - 0.35);
                    fx[i] += ux * f;
                    fy[i] += uy * f;
                    fx[j] -= ux * f;
                    fy[j] -= uy * f;
                    let sign = if k % 2 == 1 { 1.0 } else { -1.0 };
                    let o = DANCE * s * sign;
                    fx[i] += -uy * o;
                    fy[i] += ux * o;
                    fx[j] -= -uy * o;
                    fy[j] -= ux * o;
                }
                // conduction
                if s > 0.05 {
                    let m = (qa + qb) / 2.0;
                    self.shards[i].q += (m - qa) * 0.05 * s;
                    self.shards[j].q += (m - qb) * 0.05 * s;
                }
            }
        }
        self.breakdown(beat);
        self.cascades();
        // impact swirl
        for i in 0..n {
            let p = &self.shards[i];
            if p.dead {
                continue;
            }
            let fade = (-self.age_of(p) / 0.9).exp();
            let imp = &self.imps[p.imp];
            let (rx, ry) = (p.x - imp.x, p.y - imp.y);
            let r = (rx * rx + ry * ry).sqrt() + 0.3;
            fx[i] += (-ry / r) * SWIRL * fade / r.sqrt();
            fy[i] += (rx / r) * SWIRL * fade / r.sqrt();
        }
        // lifecycle: lives and loneliness -> last gasp -> quiet fade
        let mut linked = vec![false; n];
        for &k in &self.active {
            let (i, j) = unkey(k);
            linked[i] = true;
            linked[j] = true;
        }
        for i in 0..n {
            let age = self.age_of(&self.shards[i]);
            let p = &mut self.shards[i];
            if p.dead {
                continue;
            }
            p.alone = if linked[i] { 0.0 } else { p.alone + DT };
            if p.fade >= 0.0 {
                p.fade += DT;
                p.q *= 0.9;
                if p.fade > FADE_T {
                    p.dead = true;
                    p.vx = 0.0;
                    p.vy = 0.0;
                    p.q = 0.0;
                }
            } else if p.gasp >= 0.0 {
                p.gasp += DT;
                if p.gasp > GASP {
                    p.gasp = -1.0;
                    p.fade = 0.0;
                }
            } else if p.lives <= 0 || (age > 0.4 && p.alone > LONELY) {
                p.gasp = 0.0;
                p.sparked = false;
                p.alone = 0.0;
                p.q += GASP_CHARGE;
            }
        }
        // moods, wandering headings, uneven gait, sudden darts
        for i in 0..n {
            if self.shards[i].dead || self.shards[i].fade >= 0.0 {
                continue;
            }
            let (r1, r2, r3, r4, r5) = (self.r(), self.r(), self.r(), self.r(), self.r());
            let settle = (self.age_of(&self.shards[i]) / 0.6).min(1.0);
            let p = &mut self.shards[i];
            p.mood = (p.mood + (1.0 - p.mood) * 0.03 + (r1 - 0.5) * MOOD * 0.5).clamp(-0.8, 2.0);
            p.hd += (r2 - 0.5) * 0.7;
            let gait = 0.5 + 0.5 * (t as f32 * 0.4 + i as f32 * 1.7).sin();
            fx[i] += p.hd.cos() * WANDER * gait * settle;
            fy[i] += p.hd.sin() * WANDER * gait * settle;
            if settle >= 1.0 && r3 < DART * DT {
                let k = 0.6 + 2.4 * r4.powi(3);
                p.vx += p.hd.cos() * k;
                p.vy += p.hd.sin() * k;
                p.w += (r5 - 0.5) * 30.0;
            }
        }
        // integrate: the burst flies, then drag ramps in; obstacles bounce
        for i in 0..n {
            let age = self.age_of(&self.shards[i]);
            let p = &mut self.shards[i];
            if p.dead {
                continue;
            }
            let drag_now = DRAG_START + (DRAG - DRAG_START) * (age / DRAG_RAMP).min(1.0);
            let drag = (1.0 - drag_now * DT).max(0.0);
            p.vx = (p.vx + fx[i] * DT) * drag;
            p.vy = (p.vy + fy[i] * DT) * drag;
            let (nx, ny) = (p.x + p.vx * DT, p.y + p.vy * DT);
            if p.vx.abs() + p.vy.abs() > 0.2 {
                if let Some(f) = solid(p.x, p.y, nx, ny) {
                    p.x += (nx - p.x) * (f - 0.05).max(0.0);
                    p.y += (ny - p.y) * (f - 0.05).max(0.0);
                    p.vx *= -0.4;
                    p.vy *= -0.4;
                } else {
                    p.x = nx;
                    p.y = ny;
                }
            } else {
                p.x = nx;
                p.y = ny;
            }
            p.a += p.w * DT;
            p.q *= q_decay;
        }
        self.bolts.retain_mut(|b| {
            b.life -= 1;
            b.life > 0
        });
        self.compact();
    }

    /// A charged cluster fires a bolt at an outside shard.
    fn breakdown(&mut self, beat: bool) {
        let n = self.shards.len();
        let mut par: Vec<usize> = (0..n).collect();
        fn find(p: &mut [usize], i: usize) -> usize {
            let mut r = i;
            while p[r] != r {
                r = p[r];
            }
            let mut c = i;
            while p[c] != r {
                let nx = p[c];
                p[c] = r;
                c = nx;
            }
            r
        }
        let mut nlinks: HashMap<usize, usize> = HashMap::new();
        for (&k, &s) in &self.links {
            if s > 0.05 {
                let (i, j) = unkey(k);
                let (ri, rj) = (find(&mut par, i), find(&mut par, j));
                par[ri] = rj;
            }
        }
        let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
        for i in 0..n {
            if !self.shards[i].dead {
                let r = find(&mut par, i);
                groups.entry(r).or_default().push(i);
            }
        }
        for (&k, &s) in &self.links {
            if s > 0.05 {
                let r = find(&mut par, unkey(k).0);
                *nlinks.entry(r).or_default() += 1;
            }
        }
        let mut keys: Vec<usize> = groups.keys().copied().collect();
        keys.sort_unstable();
        for r in keys {
            let g = &groups[&r];
            let nl = nlinks.get(&r).copied().unwrap_or(0) as f32;
            let qsum: f32 = g.iter().map(|&i| self.shards[i].q).sum::<f32>() * (1.0 + 0.15 * nl);
            let cx = g.iter().map(|&i| self.shards[i].x).sum::<f32>() / g.len() as f32;
            let cy = g.iter().map(|&i| self.shards[i].y).sum::<f32>() / g.len() as f32;
            let (mut best, mut best_e) = (usize::MAX, 0.0f32);
            for (k, p) in self.shards.iter().enumerate() {
                if p.dead || g.contains(&k) {
                    continue;
                }
                let d = ((p.x - cx).powi(2) + (p.y - cy).powi(2)).sqrt();
                if d > BOLT_RANGE {
                    continue;
                }
                let e = qsum / (d * d + 0.05);
                if e > best_e {
                    best_e = e;
                    best = k;
                }
            }
            if best != usize::MAX && best_e > BOLT_THR && self.r() < BOLT_RATE * if beat { 1.0 } else { 0.3 } {
                let src = *g.iter().max_by(|&&a, &&b| self.shards[a].q.partial_cmp(&self.shards[b].q).unwrap()).unwrap();
                let moved = self.shards[src].q * BOLT_SHARE;
                self.shards[src].q -= moved;
                self.shards[best].q += moved * 0.8;
                self.bolts.push(Bolt { a: src, b: best, life: 6 });
                self.events.push((1, self.shards[src].x, self.shards[src].y, 1.0));
            }
        }
    }

    /// Critical branching: rare spontaneous sparks; each line may light the
    /// next from its far end, so cascade sizes follow a power law.
    fn cascades(&mut self) {
        let t = self.t;
        for imp in 0..self.imps.len() {
            let own: Vec<usize> = (0..self.shards.len()).filter(|&i| self.shards[i].imp == imp && !self.shards[i].dead && self.shards[i].fade < 0.0).collect();
            let mut due: Vec<Pending> = Vec::new();
            if own.len() >= 2 && self.r() < RAVE / 125.0 {
                let from = own[(self.r() * own.len() as f32) as usize % own.len()];
                due.push(Pending { from, gen: 0, at: t });
            }
            let queue = std::mem::take(&mut self.imps[imp].queue);
            let (now, later): (Vec<Pending>, Vec<Pending>) = queue.into_iter().partition(|q| q.at <= t);
            self.imps[imp].queue = later;
            due.extend(now);
            for q in due {
                let a = q.from;
                if self.shards[a].dead || self.shards[a].fade >= 0.0 {
                    continue;
                }
                let near: Vec<usize> = own
                    .iter()
                    .copied()
                    .filter(|&o| o != a && ((self.shards[o].x - self.shards[a].x).powi(2) + (self.shards[o].y - self.shards[a].y).powi(2)).sqrt() < RAVE_DIST)
                    .collect();
                if near.is_empty() {
                    continue;
                }
                let b = near[(self.r() * near.len() as f32) as usize % near.len()];
                let life = (3.0 * self.r().max(1e-6).powf(-1.0 / 1.6)).round().min(30.0) as u64;
                let snap = self.r() < 0.3;
                let w = (0.6 * self.r().max(1e-6).powf(-1.0 / 3.0)).min(1.4);
                let bow = if self.r() < 0.5 { -1.0 } else { 1.0 } * (0.08 + self.r() * 0.22);
                let show = self.r() > DROPOUT;
                if show {
                    self.events.push((0, self.shards[a].x, self.shards[a].y, w));
                }
                self.imps[imp].lines.push(Line { a, b, born: t, life: life.max(1), snap, w, bow, show });
                for _ in 0..2 {
                    if self.r() < BRANCH / 2.0 && q.gen < 60 {
                        let at = t + 1 + (self.r() * 3.0) as u64;
                        self.imps[imp].queue.push(Pending { from: b, gen: q.gen + 1, at });
                    }
                }
            }
            self.imps[imp].lines.retain(|l| t - l.born < l.life);
        }
    }

    /// Drop dead shards and finished impacts, remapping every index.
    fn compact(&mut self) {
        let dead = self.shards.iter().filter(|s| s.dead).count();
        if dead == 0 || (dead < 64 && dead * 2 < self.shards.len()) {
            return;
        }
        let mut map = vec![usize::MAX; self.shards.len()];
        let mut kept = Vec::new();
        for (i, s) in self.shards.iter().enumerate() {
            if !s.dead {
                map[i] = kept.len();
                kept.push(s.clone());
            }
        }
        let remap = |m: &HashMap<u32, f32>| -> HashMap<u32, f32> {
            m.iter()
                .filter_map(|(&k, &v)| {
                    let (i, j) = unkey(k);
                    (map[i] != usize::MAX && map[j] != usize::MAX).then(|| (key(map[i], map[j]), v))
                })
                .collect()
        };
        self.links = remap(&self.links);
        self.age = remap(&self.age);
        self.cool = self
            .cool
            .iter()
            .filter_map(|(&k, &v)| {
                let (i, j) = unkey(k);
                (map[i] != usize::MAX && map[j] != usize::MAX).then(|| (key(map[i], map[j]), v))
            })
            .collect();
        self.active = self.active.iter().filter_map(|&k| {
            let (i, j) = unkey(k);
            (map[i] != usize::MAX && map[j] != usize::MAX).then(|| key(map[i], map[j]))
        }).collect();
        self.vis.clear();
        self.bolts.retain(|b| map[b.a] != usize::MAX && map[b.b] != usize::MAX);
        for b in self.bolts.iter_mut() {
            b.a = map[b.a];
            b.b = map[b.b];
        }
        // impacts: keep only those with live shards
        let mut imp_map = vec![usize::MAX; self.imps.len()];
        let mut imps = Vec::new();
        for (k, imp) in self.imps.iter().enumerate() {
            if kept.iter().any(|s| s.imp == k) {
                imp_map[k] = imps.len();
                let mut imp = imp.clone();
                imp.lines.retain(|l| map[l.a] != usize::MAX && map[l.b] != usize::MAX);
                for l in imp.lines.iter_mut() {
                    l.a = map[l.a];
                    l.b = map[l.b];
                }
                imp.queue.retain(|q| map[q.from] != usize::MAX);
                for q in imp.queue.iter_mut() {
                    q.from = map[q.from];
                }
                imps.push(imp);
            }
        }
        for s in kept.iter_mut() {
            s.imp = imp_map[s.imp];
        }
        self.shards = kept;
        self.imps = imps;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_throw_empties_in_a_few_seconds() {
        // the brief: ~5.3 s on average, ~7 s at most
        let mut total = 0.0;
        let mut worst = 0.0f32;
        for seed in 1..=20u64 {
            let mut s = ShardSim::new(seed);
            s.shatter(0.0, 0.0, 0.0, [-60.0, 10.0], 70.0, 14.0);
            let mut ticks = 0;
            while s.live() > 0 && ticks < 2000 {
                s.step(|_, _, _, _| None);
                ticks += 1;
            }
            let secs = ticks as f32 * DT;
            total += secs;
            worst = worst.max(secs);
        }
        let mean = total / 20.0;
        assert!(mean > 4.3 && mean < 6.3, "mean lifetime {mean}");
        assert!(worst < 8.5, "worst {worst}");
        eprintln!("shard lifetime: mean {mean:.2} s, max {worst:.2} s");
    }

    #[test]
    fn many_boxes_coexist_and_compact() {
        let mut s = ShardSim::new(7);
        for k in 0..8 {
            s.shatter(k as f32 * 0.5, 0.0, 0.0, [40.0, 0.0], 60.0, 14.0);
            for _ in 0..20 {
                s.step(|_, _, _, _| None);
            }
        }
        for _ in 0..1500 {
            s.step(|_, _, _, _| None);
        }
        assert_eq!(s.live(), 0);
        assert!(s.shards.is_empty() && s.imps.is_empty(), "compacted away");
    }
}
