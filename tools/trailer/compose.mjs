// Run the compositor (compose.html) in headless Chrome over the whole edit.
import { open } from "./cdp.mjs";
const b = await open("http://127.0.0.1:8652/trailer/compose.html", { port: 9350, ready: "!!window.ready" });
try {
  for (let i = 0; i < 100 && !(await b.evalJs("!!window.ready")); i++) await new Promise((r) => setTimeout(r, 200));
  const t0 = Date.now();
  console.log("last frame", await b.evalJs("render()"), "in", ((Date.now() - t0) / 1000).toFixed(0), "s");
} catch (e) { console.error("ERR", e.message); } finally { b.close(); process.exit(0); }
