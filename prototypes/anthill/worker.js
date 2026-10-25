// Builds cities, roads and breeding candidates off the main thread, so the
// page never stalls. Results come back as transferable typed arrays and bitmaps.
importScripts("gen.js");
const cities = new Map(); // id -> { occ, G, foods }
const AG = 200; // ant grid per city (0.8 units a cell)
function thumb(walls, boxes, px) {
  if (typeof OffscreenCanvas === "undefined") return null;
  const c = new OffscreenCanvas(px, px), x = c.getContext("2d");
  x.translate(px / 2, px / 2); x.scale(px / CITY, -px / CITY);
  drawHair(x, walls, boxes, CITY / px);
  return c.transferToImageBitmap();
}
onmessage = (e) => {
  const m = e.data;
  if (m.type === "realm") {
    const rl = realm(m.seed, m.scale);
    postMessage({ type: "realm", ver: m.ver, Rw: rl.Rw, hubs: rl.hubs, roads: rl.roads.map((r) => ({ a: r.a, b: r.b, poly: Float32Array.from(r.poly.flat()) })) });
    const T = roadTerrain(rl), p = T.pack();
    postMessage({ type: "roads", ver: m.ver, walls: p.walls, boxes: p.boxes }, [p.walls.buffer, p.boxes.buffer]);
  } else if (m.type === "city") {
    const W = generate(m.seed, m.genome, m.exits), SV = solve(W, AG), p = W.T.pack();
    const foods = Float32Array.from(W.food.flatMap((f) => [f.p[0], f.p[1], f.rich]));
    cities.set(m.id, { occ: SV.occ, G: AG, foods: W.food.map((f) => f.p) });
    const t = thumb(p.walls, p.boxes, 256), occ = SV.occ.slice(), D = SV.D;
    const tr = [p.walls.buffer, p.boxes.buffer, foods.buffer, occ.buffer, D.buffer]; if (t) tr.push(t);
    postMessage({ type: "city", ver: m.ver, id: m.id, walls: p.walls, boxes: p.boxes, foods, occ, D, G: AG, thumb: t, reach: SV.reach }, tr);
  } else if (m.type === "fields") {
    const c = cities.get(m.id); if (!c) return;
    const fields = c.foods.map((f) => bfs(c.occ, c.G, discCells(c.G, f, 1.8).filter((i) => !c.occ[i])));
    postMessage({ type: "fields", ver: m.ver, id: m.id, fields }, fields.map((f) => f.buffer));
  } else if (m.type === "breed") {
    const out = m.cands.map((c) => {
      const W = generate(c.seed, c.genome), p = W.T.pack(), occ = raster(W.T, m.GB);
      return { desc: descriptor(p.walls, p.boxes), occ, foods: W.food.map((f) => f.p), thumb: thumb(p.walls, p.boxes, 420) };
    });
    postMessage({ type: "breed", ver: m.ver, out }, out.flatMap((o) => [o.occ.buffer, ...(o.thumb ? [o.thumb] : [])]));
  }
};
