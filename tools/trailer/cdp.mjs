// A headless Chrome on the trailer page, driven over CDP.
import { spawn } from "node:child_process";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
export async function open(url, opts = {}) {
  // Chrome sometimes doesn't come up: try again
  for (let tries = 0; ; tries++) {
    try { return await open1(url, opts); } catch (e) { if (tries >= 3) throw e; await sleep(1000); }
  }
}
async function open1(url, { port = 9334, w = 1920, h = 1080, ready = "!!(window.__nb && __nb.ready)" } = {}) {
  const chrome = spawn("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", [
    "--headless=new", `--remote-debugging-port=${port}`, "--user-data-dir=" + process.env.TMPDIR + "/nb-trailer-" + port,
    "--enable-unsafe-webgpu", "--enable-features=Vulkan", `--window-size=${w},${h}`, "--force-device-scale-factor=1",
    "--hide-scrollbars", "--mute-audio", "--no-first-run", "--no-default-browser-check", "about:blank",
  ], { stdio: "ignore" });
  let target;
  for (let i = 0; i < 50 && !target; i++) { try { target = (await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()).find((x) => x.type === "page"); } catch {} if (!target) await sleep(200); }
  if (!target) { chrome.kill(); throw new Error("no chrome on " + port); }
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((r) => (ws.onopen = r));
  let id = 0; const wait = new Map();
  ws.onmessage = (m) => { const d = JSON.parse(m.data); if (d.id && wait.has(d.id)) { const x = wait.get(d.id); wait.delete(d.id); d.error ? x.rej(new Error(d.error.message)) : x.res(d.result); } };
  const send = (method, params = {}) => new Promise((res, rej) => { const i = ++id; wait.set(i, { res, rej }); ws.send(JSON.stringify({ id: i, method, params })); });
  const evalJs = async (expr) => { const r = await send("Runtime.evaluate", { expression: expr, awaitPromise: true, returnByValue: true }); if (r.exceptionDetails) throw new Error(JSON.stringify(r.exceptionDetails).slice(0, 500)); return r.result.value; };
  await send("Emulation.setDeviceMetricsOverride", { width: w, height: h, deviceScaleFactor: 1, mobile: false });
  await send("Page.navigate", { url });
  for (let i = 0; i < 100; i++) { await sleep(200); try { if (await evalJs(ready)) break; } catch {} }
  return { evalJs, close: () => chrome.kill() };
}
