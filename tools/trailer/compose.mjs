// Run the compositor (compose.html) in headless Chrome over the whole edit.
import { open } from "./cdp.mjs";
// node compose.mjs [preview]: preview = 640x360, 15 fps, into recordings/trailer/preview
const preview = process.argv[2] === "preview";
const b = await open("http://127.0.0.1:8652/trailer/compose.html" + (preview ? "?scale=0.3333&step=4" : ""), { port: 9350, ready: "!!window.ready", w: preview ? 640 : 1920, h: preview ? 360 : 1080 });
try {
  for (let i = 0; i < 100 && !(await b.evalJs("!!window.ready")); i++) await new Promise((r) => setTimeout(r, 200));
  const t0 = Date.now();
  console.log("frames", await b.evalJs(preview ? "render(0, undefined, \"preview\")" : "render()"), "in", ((Date.now() - t0) / 1000).toFixed(0), "s");
} catch (e) { console.error("ERR", e.message); } finally { b.close(); process.exit(0); }
