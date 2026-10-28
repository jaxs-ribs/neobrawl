// Play many god matches in the browser (the same wasm the recorder uses, so
// every clip found replays exactly) and save each match's play log.
// node sweep.mjs <firstSeed> <count> <ticks> [workers]
import { open } from "./cdp.mjs";
import { writeFileSync, mkdirSync, existsSync } from "node:fs";
const [, , first = "1", count = "16", ticks = "4500", workers = "4"] = process.argv;
const out = new URL("../../../recordings/trailer/sweep/", import.meta.url).pathname;
mkdirSync(out, { recursive: true });
const jobs = [];
for (let s = +first; s < +first + +count; s++) jobs.push(s);
async function worker(w) {
  while (jobs.length) {
    const seed = jobs.shift(), map = seed % 4, file = `${out}s${seed}-m${map}.json`;
    if (existsSync(file)) continue;
    const b = await open(`http://127.0.0.1:8652/?mute&rec&seed=${seed}`, { port: 9340 + w, w: 640, h: 360 });
    try {
      const t0 = Date.now();
      writeFileSync(file, await b.evalJs(`__nb.godPlays(${map}, ${ticks})`));
      console.log(`seed ${seed} map ${map} in ${((Date.now() - t0) / 1000).toFixed(1)}s`);
    } catch (e) { console.error(seed, e.message); } finally { b.close(); }
  }
}
await Promise.all([...Array(+workers).keys()].map(worker));
process.exit(0);
