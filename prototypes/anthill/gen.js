// Shared by the page and the worker: the city generator, the macro world,
// the grid solver. Only two primitives ever exist: the wall (a lens, L long)
// and the box (BOX wide). Everything is built from them.
"use strict";
const L = 2.4, BOX = 0.86, CITY = 160, TAU = Math.PI * 2;
function rng(seed) { let a = seed >>> 0; return () => { a = (a + 0x6d2b79f5) >>> 0; let t = a; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) / 4294967296; }; }
const hash = (...v) => { let h = 2166136261; for (const x of v) { h ^= x | 0; h = Math.imul(h, 16777619); h ^= h >>> 13; } return (h >>> 0) / 4294967296; };
const rot = (p, a) => [p[0] * Math.cos(a) - p[1] * Math.sin(a), p[0] * Math.sin(a) + p[1] * Math.cos(a)];
const dist = (a, b) => Math.hypot(a[0] - b[0], a[1] - b[1]);
function d2xy(n, d) { let x = 0, y = 0, t = d; for (let s = 1; s < n; s *= 2) { const rx = 1 & (t / 2), ry = 1 & (t ^ rx); if (ry === 0) { if (rx === 1) { x = s - 1 - x; y = s - 1 - y; } const tmp = x; x = y; y = tmp; } x += s * rx; y += s * ry; t = Math.floor(t / 4); } return [x, y]; }
function segDist(p, a, b) { const dx = b[0] - a[0], dy = b[1] - a[1], l2 = dx * dx + dy * dy; const t = l2 ? Math.max(0, Math.min(1, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2)) : 0; return Math.hypot(p[0] - a[0] - dx * t, p[1] - a[1] - dy * t); }
function crosses(a, b, c, d) { const o = (p, q, r) => Math.sign((q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])); return o(a, b, c) * o(a, b, d) < 0 && o(c, d, a) * o(c, d, b) < 0; }
function segSeg(a, b, c, d) { return crosses(a, b, c, d) ? 0 : Math.min(segDist(a, c, d), segDist(b, c, d), segDist(c, a, b), segDist(d, a, b)); }

// ---------------------------------------------------------------- terrain
class Terrain {
  constructor() { this.els = []; this.grid = new Map(); this.cs = 3; }
  key(gx, gy) { return (gx + 100000) * 200003 + gy + 100000; }
  near(x, y, r, fn) { const c = this.cs; for (let gx = Math.floor((x - r) / c); gx <= Math.floor((x + r) / c); gx++) for (let gy = Math.floor((y - r) / c); gy <= Math.floor((y + r) / c); gy++) { const b = this.grid.get(this.key(gx, gy)); if (b) for (const e of b) if (e.alive) fn(e); } }
  ends(e) { const c = Math.cos(e.a) * L / 2, s = Math.sin(e.a) * L / 2; return [[e.x - c, e.y - s], [e.x + c, e.y + s]]; }
  overlaps(e, f) {
    if (e.t === "box" && f.t === "box") return Math.hypot(e.x - f.x, e.y - f.y) < BOX * 1.05;
    if (e.t === "wall" && f.t === "wall") { const [a, b] = this.ends(e), [c, d] = this.ends(f), sh = (p, q, k) => [p[0] + (q[0] - p[0]) * k, p[1] + (q[1] - p[1]) * k]; return segSeg(sh(a, b, 0.1), sh(b, a, 0.1), sh(c, d, 0.1), sh(d, c, 0.1)) < 0.26; }
    const w = e.t === "wall" ? e : f, bx = e.t === "box" ? e : f, [a, b] = this.ends(w);
    return segDist([bx.x, bx.y], a, b) < BOX * 0.5 + 0.14;
  }
  add(e, mode) {
    e.alive = true;
    if (mode === "polite") { let hit = false; this.near(e.x, e.y, L, (f) => { if (!hit && this.overlaps(e, f)) hit = true; }); if (hit) return false; }
    this.els.push(e);
    const k = this.key(Math.floor(e.x / this.cs), Math.floor(e.y / this.cs)); let b = this.grid.get(k); if (!b) this.grid.set(k, (b = [])); b.push(e);
    return true;
  }
  wall(p, q, mode) { return this.add({ t: "wall", x: (p[0] + q[0]) / 2, y: (p[1] + q[1]) / 2, a: Math.atan2(q[1] - p[1], q[0] - p[0]) }, mode); }
  box(x, y, a, mode) { return this.add({ t: "box", x, y, a }, mode); }
  carve(poly, hw) {
    for (let i = 0; i < poly.length; i++) this.near(poly[i][0], poly[i][1], hw + L, (e) => {
      const pts = e.t === "wall" ? [0.15, 0.5, 0.85].map((k) => { const [a, b] = this.ends(e); return [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k]; }) : [[e.x, e.y]];
      const r = e.t === "box" ? hw + BOX * 0.4 : hw;
      for (const p of pts) for (let j = Math.max(0, i - 1); j < Math.min(poly.length - 1, i + 1); j++) if (segDist(p, poly[j], poly[j + 1]) < r) { e.alive = false; return; }
    });
  }
  live() { return this.els.filter((e) => e.alive); }
  pack(ox = 0, oy = 0) {
    const live = this.live(), nw = live.filter((e) => e.t === "wall").length;
    const walls = new Float32Array(nw * 3), boxes = new Float32Array((live.length - nw) * 3);
    let i = 0, j = 0;
    for (const e of live) { const o = e.t === "wall" ? walls : boxes, k = e.t === "wall" ? i++ : j++; o[k * 3] = e.x + ox; o[k * 3 + 1] = e.y + oy; o[k * 3 + 2] = e.a; }
    return { walls, boxes };
  }
}
function arc(a, b, bow, n = 48) { const m = [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2], l = dist(a, b) || 1, nx = -(b[1] - a[1]) / l, ny = (b[0] - a[0]) / l, c = [m[0] + nx * bow * l, m[1] + ny * bow * l]; const out = []; for (let i = 0; i <= n; i++) { const t = i / n, u = 1 - t; out.push([u * u * a[0] + 2 * u * t * c[0] + t * t * b[0], u * u * a[1] + 2 * u * t * c[1] + t * t * b[1]]); } return out; }
function offsetPoly(P, d) { return P.map((p, i) => { const q = P[Math.min(P.length - 1, i + 1)], o = P[Math.max(0, i - 1)], dx = q[0] - o[0], dy = q[1] - o[1], l = Math.hypot(dx, dy) || 1; return [p[0] - (dy / l) * d, p[1] + (dx / l) * d]; }); }
function trim(P, r0, r1) { const a = P[0], b = P[P.length - 1]; return P.filter((p) => dist(p, a) > r0 && dist(p, b) > r1); }
function chordWalk(P, fn) {
  if (P.length < 2) return;
  let cur = P[0], i = 0;
  while (i < P.length - 1) {
    let j = i + 1; while (j < P.length && dist(cur, P[j]) < L) j++;
    if (j >= P.length) break;
    const a = P[j - 1], b = P[j]; let lo = 0, hi = 1;
    for (let it = 0; it < 22; it++) { const m = (lo + hi) / 2; if (dist(cur, [a[0] + (b[0] - a[0]) * m, a[1] + (b[1] - a[1]) * m]) < L) lo = m; else hi = m; }
    const q = [a[0] + (b[0] - a[0]) * lo, a[1] + (b[1] - a[1]) * lo];
    fn(cur, q); cur = q; i = j - 1;
  }
}
const ringR = (n) => L / (2 * Math.sin(Math.PI / n));
function ring(T, c, n, phase, mode) { const R = ringR(n); for (let k = 0; k < n; k++) { const a0 = phase + (k / n) * TAU, a1 = phase + ((k + 1) / n) * TAU; T.wall([c[0] + Math.cos(a0) * R, c[1] + Math.sin(a0) * R], [c[0] + Math.cos(a1) * R, c[1] + Math.sin(a1) * R], mode); } return R; }

// ---------------------------------------------------------------- genome
const GENES = {
  fold: { min: 1, max: 6, int: true, label: "symmetry" },
  food: { min: 4, max: 24, int: true, label: "food patches" },
  biome: { min: 0, max: 4, int: true, label: "food layout" },
  hil: { min: 0, max: 3, int: true, label: "hilbert layers" },
  spread: { min: 7, max: 18, label: "food spacing" },
  reach: { min: 0.45, max: 1, label: "extent" },
  nestDiv: { min: 1.5, max: 6, label: "food per nest" },
  pull: { min: 0.45, max: 0.95, label: "nests toward food" },
  bow: { min: 0, max: 1.3, label: "trail curvature" },
  roads: { min: 0, max: 1, label: "ring roads" },
  hillRings: { min: 1, max: 4, int: true, label: "hill rings" },
  nestRings: { min: 1, max: 3, int: true, label: "nest rings" },
  width: { min: 0.75, max: 1.5, label: "trail width" },
  rake: { min: 0, max: 1, label: "raked stones" },
  garden: { min: 0.4, max: 2, label: "garden density" },
  hilR: { min: 0.6, max: 1.35, label: "hilbert radius" },
  hilOrder: { min: 2, max: 3, int: true, label: "hilbert depth" },
};
const FOLDS = [1, 2, 3, 4, 6];
const DEF = { fold: 4, food: 11, biome: 0, hil: 2, spread: 11, reach: 1, nestDiv: 3, pull: 0.75, bow: 0.55, roads: 0.7, hillRings: 3, nestRings: 2, width: 1, rake: 1, garden: 1, hilR: 1, hilOrder: 3 };
function fixGenome(g) { const o = { ...DEF, ...g }; for (const [k, d] of Object.entries(GENES)) { o[k] = Math.max(d.min, Math.min(d.max, o[k])); if (d.int) o[k] = Math.round(o[k]); } o.fold = FOLDS.reduce((a, b) => (Math.abs(b - o.fold) < Math.abs(a - o.fold) ? b : a)); return o; }
// a city of the realm: the base genome, shifted a little by where it stands
function cityGenome(base, h, biomeHint) {
  const g = { ...base }, r = (k) => hash(h, k);
  if (r(1) < 0.45) g.fold = FOLDS[Math.floor(r(2) * FOLDS.length)];
  g.biome = biomeHint;
  g.food += Math.round((r(3) - 0.5) * 8);
  g.bow *= 0.6 + r(4) * 0.9;
  g.hil = Math.round(g.hil + (r(5) - 0.5) * 2);
  g.hillRings += Math.round((r(6) - 0.5) * 2);
  g.garden *= 0.7 + r(7) * 0.7;
  g.reach *= 0.8 + r(8) * 0.25;
  return fixGenome(g);
}

// ---------------------------------------------------------------- one city
// biome -> nests at food centroids -> curved trail graph -> terrain layers
function generate(seed, opt, exits = []) {
  opt = fixGenome(opt);
  const R = rng(seed), K = opt.fold, sec = TAU / K;
  // 1. biome: food in one sector, by one of five layouts, then K copies
  const foodS = [], want = Math.ceil(opt.food / Math.max(1, K / 2));
  const ok = (p) => { const all = foodS.flatMap((f) => Array.from({ length: K }, (_, k) => rot(f.p, k * sec))); return Math.hypot(...p) < 76 && all.every((q) => dist(p, q) > opt.spread) && Array.from({ length: K }, (_, k) => rot(p, k * sec)).every((q, k) => k === 0 || dist(p, q) > opt.spread); };
  const maxR = 14 + 60 * opt.reach;
  for (let tries = 0; foodS.length < want && tries < 3000; tries++) {
    let r, a, rich = 0.3 + Math.pow(R(), 1.6) * 0.7;
    const u = tries / 3000;
    switch (opt.biome) {
      case 1: r = maxR * (0.62 + (R() - 0.5) * 0.18); a = R() * sec; break; // a ring of plenty
      case 2: { const t = R(); r = 14 + t * (maxR - 14); a = (t * 2.4 + R() * 0.25) % sec; break; } // a spiral arm
      case 3: r = 14 + R() * (maxR - 14); a = sec * (0.5 + 0.32 * Math.sin(r / 9)); break; // a meandering river
      case 4: { const far = R() < 0.3; r = far ? maxR * (0.85 + R() * 0.15) : 14 + R() * maxR * 0.35; rich = far ? 0.85 + R() * 0.15 : 0.2 + R() * 0.2; a = R() * sec; break; } // rich edges, poor centre
      default: r = 14 + Math.sqrt(R()) * (maxR - 14); a = R() * sec;
    }
    const p = [Math.cos(a) * r, Math.sin(a) * r];
    if (ok(p) || (u > 0.8 && Math.hypot(...p) < 76 && foodS.every((f) => dist(f.p, p) > 6))) foodS.push({ p, rich });
  }
  const food = []; for (let k = 0; k < K; k++) foodS.forEach((f) => food.push({ p: rot(f.p, k * sec), rich: f.rich }));
  // 2a. nests: weighted k-means on the sector's food, pulled toward the hill
  const kn = Math.max(1, Math.round(foodS.length / opt.nestDiv));
  let cent = [...foodS].sort((a, b) => b.rich - a.rich).slice(0, kn).map((f) => [...f.p]);
  for (let it = 0; it < 10; it++) {
    const acc = cent.map(() => [0, 0, 0]);
    for (const f of foodS) { let b = 0; cent.forEach((c, i) => { if (dist(f.p, c) < dist(f.p, cent[b])) b = i; }); acc[b][0] += f.p[0] * f.rich; acc[b][1] += f.p[1] * f.rich; acc[b][2] += f.rich; }
    cent = cent.map((c, i) => (acc[i][2] ? [acc[i][0] / acc[i][2], acc[i][1] / acc[i][2]] : c));
  }
  cent = cent.map((c) => [c[0] * opt.pull, c[1] * opt.pull]).filter((c) => Math.hypot(...c) > 14);
  const nests = [[0, 0]]; for (let k = 0; k < K; k++) cent.forEach((c) => nests.push(rot(c, k * sec)));
  // 2b. graph
  const edges = [];
  const sectorEdge = (a, b, kind, id) => { for (let k = 0; k < K; k++) edges.push({ a: rot(a, k * sec), b: rot(b, k * sec), kind, bow: (hash(seed, id, 7) - 0.5) * opt.bow }); };
  cent.forEach((c, i) => {
    let best = [0, 0], bd = Math.hypot(...c);
    for (let k = 0; k < K; k++) cent.forEach((d) => { const q = rot(d, k * sec); if (Math.hypot(...q) < Math.hypot(...c) - 6 && dist(q, c) < bd) { bd = dist(q, c); best = q; } });
    sectorEdge(best, c, "trunk", i);
    if (K > 1 && hash(seed, i, 3) < opt.roads) { const q = rot(c, sec); if (dist(q, c) < 55) sectorEdge(c, q, "ring", 100 + i); }
  });
  foodS.forEach((f, i) => { let best = [0, 0], bd = Math.hypot(...f.p); for (let k = 0; k < K; k++) cent.forEach((d) => { const q = rot(d, k * sec); if (dist(q, f.p) < bd) { bd = dist(q, f.p); best = q; } }); sectorEdge(best, f.p, "twig", 200 + i); });
  // highways arrive at the city's rim: run a trunk from the nearest node out to meet each
  for (const ang of exits) {
    const out = [Math.cos(ang) * 79, Math.sin(ang) * 79];
    let best = [0, 0], bd = 79; for (const n of nests) if (dist(n, out) < bd && Math.hypot(...n) < 70) { bd = dist(n, out); best = n; }
    edges.push({ a: best, b: out, kind: "trunk", bow: (hash(seed, ang * 1000) - 0.5) * 0.3, exit: true });
  }
  for (const e of edges) e.poly = arc(e.a, e.b, e.bow);
  // 3. terrain
  const T = new Terrain(), nodeR = new Map();
  let hr = 0; for (let i = 0; i < opt.hillRings; i++) hr = ring(T, [0, 0], 8 * (i + 1), (i % 2) * Math.PI / (8 * (i + 1)), "force");
  for (let k = 0; k < 8; k++) { const a = k * TAU / 8 + TAU / 16; T.wall([Math.cos(a) * 0.6, Math.sin(a) * 0.6], [Math.cos(a) * (0.6 + L), Math.sin(a) * (0.6 + L)], "force"); }
  T.box(0, 0, Math.PI / 4, "force");
  nodeR.set("0,0", hr);
  for (const n of nests.slice(1)) {
    const phase = Math.atan2(n[1], n[0]); let r = 0;
    for (let i = 0; i < opt.nestRings; i++) r = ring(T, n, 6 * (i + 1), phase + (i % 2) * Math.PI / (6 * (i + 1)), "force");
    const rb = opt.nestRings > 1 ? (ringR(6) + ringR(12)) / 2 : ringR(6) + 1.1, nb = Math.floor((TAU * rb) / 1.55);
    for (let j = 0; j < nb; j++) { const a = phase + (j / nb) * TAU; T.box(n[0] + Math.cos(a) * rb, n[1] + Math.sin(a) * rb, a, "polite"); }
    nodeR.set(n.join(","), r);
  }
  const hil = [];
  for (let lv = 0; lv < opt.hil; lv++) {
    const order = lv === 0 ? opt.hilOrder : 2, n = 1 << order, cell = lv === 0 ? 2 * L : L;
    const rr = (lv === 0 ? 34 : lv === 1 ? 58 : 20) * opt.hilR, a0 = sec * (0.5 + 0.17 * lv) + hash(seed, lv) * 0.2;
    for (let k = 0; k < K; k++) {
      const a = a0 + k * sec, c = [Math.cos(a) * rr, Math.sin(a) * rr], u = [-Math.sin(a), Math.cos(a)], v = [Math.cos(a), Math.sin(a)];
      const P = (x, y) => [c[0] + (x - n / 2) * cell * u[0] + (y - n / 2) * cell * v[0], c[1] + (x - n / 2) * cell * u[1] + (y - n / 2) * cell * v[1]];
      const path = []; for (let d = 0; d < n * n; d++) path.push(d2xy(n, d));
      const open = new Set(), kk = (p, q) => (p[0] * 64 + p[1] < q[0] * 64 + q[1] ? `${p}|${q}` : `${q}|${p}`);
      for (let d = 1; d < path.length; d++) open.add(kk(path[d - 1], path[d]));
      const steps = cell / L;
      const edgeWall = (x0, y0, x1, y1) => { for (let s = 0; s < steps; s++) T.wall(P(x0 + ((x1 - x0) * s) / steps, y0 + ((y1 - y0) * s) / steps), P(x0 + ((x1 - x0) * (s + 1)) / steps, y0 + ((y1 - y0) * (s + 1)) / steps), "polite"); };
      for (let x = 0; x <= n; x++) for (let y = 0; y < n; y++) { const inner = x > 0 && x < n && open.has(kk([x - 1, y], [x, y])), door = (x === 0 && y === 0) || (x === n && y === 0); if (!inner && !door) edgeWall(x, y, x, y + 1); }
      for (let y = 0; y <= n; y++) for (let x = 0; x < n; x++) { const inner = y > 0 && y < n && open.has(kk([x, y - 1], [x, y])); if (!inner) edgeWall(x, y, x + 1, y); }
      hil.push(path.map(([x, y]) => P(x + 0.5, y + 0.5)));
    }
  }
  const hw = { trunk: 1.7 * opt.width, ring: 1.4 * opt.width, twig: 1.2 * opt.width };
  for (const e of edges) {
    const ra = nodeR.get(e.a.join(",")) || 2, rb = e.exit ? 0 : nodeR.get(e.b.join(",")) || 2.5;
    T.carve(e.poly, hw[e.kind] * (e.exit ? 1.15 : 1));
    for (const s of [-1, 1]) chordWalk(trim(offsetPoly(e.poly, s * hw[e.kind] * (e.exit ? 1.15 : 1)), ra + 0.6, rb + 0.6), (p, q) => T.wall(p, q, "polite"));
  }
  for (const h of hil) T.carve(h.filter((_, i) => i % 3 === 0), 0.35);
  for (const f of food) {
    const nb = Math.round((5 + f.rich * 14) * opt.garden), ga = Math.PI * (3 - Math.sqrt(5)), base = Math.atan2(f.p[1], f.p[0]);
    for (let j = 2; j < nb + 2; j++) { const r = 0.95 * Math.sqrt(j), a = base + j * ga; T.box(f.p[0] + Math.cos(a) * r, f.p[1] + Math.sin(a) * r, a, "polite"); }
  }
  if (opt.rake > 0.1) edges.forEach((e, ei) => { if (e.kind !== "trunk" || e.exit) return; for (const s of [-1, 1]) { if (hash(seed, ei % Math.max(1, edges.length / K), s) > opt.rake) continue; const P = offsetPoly(e.poly, s * (hw.trunk + 1.5)); for (let i = 3; i < P.length - 3; i += 2) T.box(P[i][0], P[i][1], Math.atan2(P[i + 1][1] - P[i][1], P[i + 1][0] - P[i][0]), "polite"); } });
  return { seed, opt, K, food, nests, edges, T };
}

// ---------------------------------------------------------------- solver (grid)
function raster(T, G) {
  const occ = new Uint8Array(G * G), cell = CITY / G, coarse = cell > 0.55;
  const pt = (x, y) => { const gx = Math.floor((x + CITY / 2) / cell), gy = Math.floor((y + CITY / 2) / cell); if (gx >= 0 && gy >= 0 && gx < G && gy < G) occ[gy * G + gx] = 1; };
  const mark = (x, y, r) => { const x0 = Math.floor((x - r + CITY / 2) / cell), x1 = Math.floor((x + r + CITY / 2) / cell), y0 = Math.floor((y - r + CITY / 2) / cell), y1 = Math.floor((y + r + CITY / 2) / cell); for (let gx = Math.max(0, x0); gx <= Math.min(G - 1, x1); gx++) for (let gy = Math.max(0, y0); gy <= Math.min(G - 1, y1); gy++) { const cx = (gx + 0.5) * cell - CITY / 2, cy = (gy + 0.5) * cell - CITY / 2; if (Math.hypot(cx - x, cy - y) <= r + cell * 0.5) occ[gy * G + gx] = 1; } };
  for (const e of T.live()) {
    if (e.t === "box") { if (coarse) for (let k = 0; k < 9; k++) pt(e.x + (k % 3 - 1) * 0.3, e.y + (Math.floor(k / 3) - 1) * 0.3); else mark(e.x, e.y, BOX * 0.42); continue; }
    const [a, b] = T.ends(e);
    for (let s = 0; s <= 12; s++) { const t = s / 12, x = a[0] + (b[0] - a[0]) * t, y = a[1] + (b[1] - a[1]) * t; if (coarse) pt(x, y); else mark(x, y, 0.04 + 0.42 * t * (1 - t)); }
  }
  return occ;
}
const cellOf = (G, x, y) => { const cell = CITY / G; return Math.max(0, Math.min(G - 1, Math.floor((y + CITY / 2) / cell))) * G + Math.max(0, Math.min(G - 1, Math.floor((x + CITY / 2) / cell))); };
function discCells(G, p, r) { const out = [], cell = CITY / G; for (let dx = -r; dx <= r; dx += cell) for (let dy = -r; dy <= r; dy += cell) if (dx * dx + dy * dy <= r * r) out.push(cellOf(G, p[0] + dx, p[1] + dy)); return out; }
function bfs(occ, G, sources) {
  const D = new Uint16Array(G * G).fill(65535), q = new Int32Array(G * G); let h = 0, t = 0;
  for (const s of sources) if (D[s] === 65535) { D[s] = 0; q[t++] = s; }
  while (h < t) { const c = q[h++], d = D[c] + 1, x = c % G; if (x > 0 && !occ[c - 1] && D[c - 1] === 65535) { D[c - 1] = d; q[t++] = c - 1; } if (x < G - 1 && !occ[c + 1] && D[c + 1] === 65535) { D[c + 1] = d; q[t++] = c + 1; } if (c >= G && !occ[c - G] && D[c - G] === 65535) { D[c - G] = d; q[t++] = c - G; } if (c < G * G - G && !occ[c + G] && D[c + G] === 65535) { D[c + G] = d; q[t++] = c + G; } }
  return D;
}
function solve(W, G) {
  let occ, D, pass = 0, fixed = 0;
  for (; pass < 5; pass++) {
    occ = raster(W.T, G);
    for (const c of discCells(G, [0, 0], 0.9)) occ[c] = 0;
    D = bfs(occ, G, discCells(G, [0, 0], 0.6));
    const lost = W.food.filter((f) => !discCells(G, f.p, 1.6).some((c) => D[c] < 65535 && !occ[c]));
    if (!lost.length) break;
    for (const f of lost) { const tw = W.edges.find((e) => e.kind === "twig" && dist(e.b, f.p) < 0.01); W.T.carve([f.p, tw ? tw.a : [0, 0]], 1.0); fixed++; }
  }
  const reach = W.food.filter((f) => discCells(G, f.p, 1.6).some((c) => D[c] < 65535)).length / Math.max(1, W.food.length);
  return { occ, D, G, reach, fixed };
}
function descriptor(walls, boxes) { const n = 12, h = new Float32Array(n * n), add = (arr, w) => { for (let i = 0; i < arr.length; i += 3) { const gx = Math.min(n - 1, Math.max(0, Math.floor(((arr[i] + CITY / 2) / CITY) * n))), gy = Math.min(n - 1, Math.max(0, Math.floor(((arr[i + 1] + CITY / 2) / CITY) * n))); h[gy * n + gx] += w; } }; add(walls, 1); add(boxes, 0.6); let m = 1; for (const v of h) m = Math.max(m, v); return h.map((v) => v / m); }

// ---------------------------------------------------------------- the realm
// The same idea one scale up: macro food -> city hubs at its centroids ->
// curved highways between hubs, lined with the same lens walls.
function realm(seed, scale) {
  const R = rng(seed ^ 0x51ed), Rw = 80 * scale;
  if (scale <= 1) return { Rw, hubs: [{ x: 0, y: 0, h: seed, biome: -1, exits: [] }], roads: [] };
  const K = [3, 4, 5, 6][Math.floor(R() * 4)], sec = TAU / K;
  // macro food: Poisson in a sector, denser toward a few macro "oases"
  const oases = Array.from({ length: 3 }, () => { const a = R() * sec, r = Rw * (0.25 + R() * 0.6); return [Math.cos(a) * r, Math.sin(a) * r]; });
  const mf = [];
  for (let t = 0; t < 20000 && mf.length < scale * scale * 0.9; t++) {
    const a = R() * sec, r = Rw * (0.12 + Math.sqrt(R()) * 0.86), p = [Math.cos(a) * r, Math.sin(a) * r];
    const near = Math.min(...oases.map((o) => dist(o, p))) / Rw;
    if (R() > Math.exp(-near * 4) * 0.9 + 0.1) continue;
    if (mf.every((q) => dist(q, p) > 120) && [1, -1].every((s) => dist(rot(p, s * sec), p) > 120 || K === 1)) mf.push(p);
  }
  // hubs: k-means over the macro food, kept apart by a city's breadth
  let hubs = mf.filter((_, i) => i % 2 === 0).map((p) => [...p]);
  for (let it = 0; it < 6; it++) { const acc = hubs.map(() => [0, 0, 0]); for (const p of mf) { let b = 0; hubs.forEach((h, i) => { if (dist(p, h) < dist(p, hubs[b])) b = i; }); acc[b][0] += p[0]; acc[b][1] += p[1]; acc[b][2]++; } hubs = hubs.map((h, i) => (acc[i][2] ? [acc[i][0] / acc[i][2], acc[i][1] / acc[i][2]] : h)); }
  const keep = [];
  for (const h of hubs) { const all = keep.flatMap((q) => Array.from({ length: K }, (_, k) => rot(q, k * sec))); if (Math.hypot(...h) > 200 && all.every((q) => dist(q, h) > 210) && Array.from({ length: K }, (_, k) => rot(h, k * sec)).every((q, k) => k === 0 || dist(q, h) > 210)) keep.push(h); }
  const H = [{ x: 0, y: 0, h: seed, biome: -1, exits: [] }];
  for (let k = 0; k < K; k++) keep.forEach((p, i) => { const q = rot(p, k * sec); H.push({ x: q[0], y: q[1], h: hash(seed, i, k) * 4294967296, biome: Math.floor(hash(seed, i, k, 9) * 5), exits: [], orb: i }); }); // positions keep the realm's symmetry; each city is its own
  // highways: a tree toward the capital, plus ring roads between copies, plus near-neighbour links
  const roads = [], has = new Set(), link = (a, b, bow) => { const k = a < b ? a * 100000 + b : b * 100000 + a; if (has.has(k) || a === b) return; has.add(k); roads.push({ a, b, bow }); };
  H.forEach((h, i) => {
    if (i === 0) return;
    const d0 = Math.hypot(h.x, h.y); let best = 0, bd = d0;
    H.forEach((o, j) => { const dd = Math.hypot(o.x - h.x, o.y - h.y); if (j !== i && Math.hypot(o.x, o.y) < d0 - 80 && dd < bd) { bd = dd; best = j; } });
    link(i, best, (hash(seed, h.orb, 1) - 0.5) * 0.5);
  });
  H.forEach((h, i) => { if (!i) return; const near = H.map((o, j) => [Math.hypot(o.x - h.x, o.y - h.y), j]).filter(([d, j]) => j && j !== i && d < 340).sort((a, b) => a[0] - b[0]).slice(0, 2); for (const [, j] of near) if (hash(seed, h.orb, H[j].orb, 2) < 0.5) link(i, j, (hash(seed, h.orb, 3) - 0.5) * 0.6); });
  for (const r of roads) {
    const A = [H[r.a].x, H[r.a].y], B = [H[r.b].x, H[r.b].y];
    r.poly = arc(A, B, r.bow, Math.max(24, Math.ceil(dist(A, B) / 6)));
    H[r.a].exits.push(Math.atan2(r.poly[1][1] - A[1], r.poly[1][0] - A[0]));
    const n = r.poly.length; H[r.b].exits.push(Math.atan2(r.poly[n - 2][1] - B[1], r.poly[n - 2][0] - B[0]));
  }
  return { Rw, K, hubs: H, roads };
}
function roadTerrain(rl) {
  const T = new Terrain(), hw = 3.2;
  for (const r of rl.roads) {
    const P = trim(r.poly, 80, 80);
    for (const s of [-1, 1]) chordWalk(offsetPoly(P, s * hw), (p, q) => T.wall(p, q, "polite"));
    // gate stones: a pair of boxes every ~30 units, like milestones
    for (let i = 4; i < P.length - 4; i += 5) { const a = Math.atan2(P[i + 1][1] - P[i][1], P[i + 1][0] - P[i][0]); for (const s of [-1, 1]) { const o = offsetPoly([P[i - 1], P[i], P[i + 1]], s * (hw + 1.6))[1]; T.box(o[0], o[1], a, "polite"); } }
  }
  return T;
}

// thin drawing, used for thumbnails (worker, OffscreenCanvas) and far tiles
function drawHair(ctx, walls, boxes, px) {
  ctx.lineWidth = Math.max(px * 1.1, 0.2); ctx.strokeStyle = "#111"; ctx.beginPath();
  for (let i = 0; i < walls.length; i += 3) { const c = Math.cos(walls[i + 2]) * L / 2, s = Math.sin(walls[i + 2]) * L / 2; ctx.moveTo(walls[i] - c, walls[i + 1] - s); ctx.lineTo(walls[i] + c, walls[i + 1] + s); }
  ctx.stroke(); ctx.fillStyle = "#111"; ctx.beginPath();
  const h = Math.max(0.38, px * 0.7); for (let i = 0; i < boxes.length; i += 3) ctx.rect(boxes[i] - h, boxes[i + 1] - h, 2 * h, 2 * h);
  ctx.fill();
}
