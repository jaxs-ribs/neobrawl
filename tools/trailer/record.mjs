// Record one clip of a god match: node record.mjs <take> <seed> <map> <fromTick> <seconds> [fps] [slowmo] [chrome port]
// slowmo: "tick:len:speed,..." - from game tick `tick`, `len` ticks play at `speed`.
// Frames and audio land in recordings/trailer/<take>/.
import { open } from "./cdp.mjs";
import { writeFileSync } from "node:fs";
const [, , take, seed, map, from, secs, fps = "60", slowmo = "", port = "9334"] = process.argv;
const b = await open(`http://127.0.0.1:8652/?mute&rec&seed=${seed}`, { port: +port });
try {
  const ok = await b.evalJs(`__nb.godSetup(${map}, ${from})`);
  if (!ok) throw new Error("no god match");
  const spans = slowmo ? slowmo.split(",").map((s) => s.split(":").map(Number)) : [];
  const t0 = Date.now();
  await b.evalJs(`(async () => {
    __nb.take = ${JSON.stringify(take)}; __nb.noMusic = true;
    const spans = ${JSON.stringify(spans)}, start = ${from};
    // speed by the game tick the capture has reached
    const speed = () => { const t = __nb.engine.tick(); for (const [a, l, s] of spans) if (t >= a && t < a + l) return s; return 1; };
    __nb.tickOf = [];
    await __nb.capture(${secs}, ${fps}, (i) => { __nb.tickOf[i] = __nb.engine.tick(); return speed(); });
  })()`);
  // which game tick each output frame shows (the compositor finds hits by it)
  const tickOf = await b.evalJs("__nb.tickOf"), souls = await b.evalJs("__nb.soulsAt");
  writeFileSync(new URL(`../../../recordings/trailer/${take}/meta.json`, import.meta.url).pathname, JSON.stringify({ take, seed: +seed, map: +map, from: +from, fps: +fps, spans, tickOf, souls, size: [1920, 1080] }));
  console.log("frames in", ((Date.now() - t0) / 1000).toFixed(1), "s; audio", JSON.stringify(await b.evalJs(`__nb.renderAudio(${fps})`)));
} catch (e) { console.error("ERR", e.message); } finally { b.close(); process.exit(0); }
