// Record one clip of a god match: node record.mjs <take> <seed> <map> <fromTick> <seconds> [fps] [slowmo] [chrome port]
// slowmo: "tick:len:speed,..." - from game tick `tick`, `len` ticks play at `speed`.
// Frames and audio land in recordings/trailer/<take>/.
import { open } from "./cdp.mjs";
import { writeFileSync } from "node:fs";
const [, , take, seed, map, from, secs, fps = "60", slowmo = "", port = "9334"] = process.argv;
// takes are shot at 1.5x so the edit can frame in close and stay sharp
const [W, H] = (process.env.REC_SIZE || "2880x1620").split("x").map(Number);
const b = await open(`http://127.0.0.1:8652/?mute&rec&seed=${seed}`, { port: +port, w: W, h: H });
try {
  // a stale build fails here, not halfway through a take
  const missing = await b.evalJs(`["god_match", "tick", "in_hitstop", "souls_on_screen"].filter((f) => typeof __nb.engine[f] !== "function")`);
  if (missing.length) throw new Error("stale build, missing " + missing.join(", "));
  const ok = await b.evalJs(`__nb.godSetup(${map}, ${from})`);
  if (!ok) throw new Error("no god match");
  const spans = slowmo ? slowmo.split(",").map((s) => s.split(":").map(Number)) : [];
  const t0 = Date.now();
  await b.evalJs(`(async () => {
    __nb.take = ${JSON.stringify(take)}; __nb.noMusic = true;
    const spans = ${JSON.stringify(spans)}, start = ${from};
    // speed by the game tick the capture has reached
    // slow motion eases in and out over its span (never a hard switch), and a
    // hit-stop always plays at full speed (stretched, a freeze reads as a stall)
    const speed = () => {
      if (__nb.engine.in_hitstop()) return 1;
      const t = __nb.engine.tick() + __nb.sub;
      for (const [a, l, s] of spans) if (t >= a && t < a + l) { const u = (t - a) / l, k = Math.sin(Math.PI * u); return 1 - (1 - s) * Math.min(1, k * 1.6); }
      return 1;
    };
    __nb.tickOf = [];
    await __nb.capture(${secs}, ${fps}, (i) => { __nb.tickOf[i] = __nb.engine.tick(); return speed(); });
  })()`);
  // which game tick each output frame shows (the compositor finds hits by it)
  const tickOf = await b.evalJs("__nb.tickOf"), souls = await b.evalJs("__nb.soulsAt");
  writeFileSync(new URL(`../../../recordings/trailer/${take}/meta.json`, import.meta.url).pathname, JSON.stringify({ take, seed: +seed, map: +map, from: +from, fps: +fps, spans, tickOf, souls, size: await b.evalJs("[document.querySelector('canvas').width, document.querySelector('canvas').height]") }));
  console.log("frames in", ((Date.now() - t0) / 1000).toFixed(1), "s; audio", JSON.stringify(await b.evalJs(`__nb.renderAudio(${fps})`)));
} catch (e) { console.error("ERR", e.message); } finally { b.close(); process.exit(0); }
