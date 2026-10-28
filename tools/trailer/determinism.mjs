// The browser (wasm) must replay a god match exactly as the native sweep did.
import { open } from "./cdp.mjs";
import { writeFileSync } from "node:fs";
const [, , seed = "1", map = "1", ticks = "3000"] = process.argv;
const b = await open(`http://127.0.0.1:8652/?mute&rec&seed=${seed}`);
try {
  const plays = await b.evalJs(`__nb.godPlays(${map}, ${ticks})`);
  writeFileSync(process.argv[5] || "wasm-plays.json", plays);
  console.log("wasm plays:", JSON.parse(plays).length);
} finally { b.close(); process.exit(0); }
