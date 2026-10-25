// NeoBrawl host: runs the WASM game at 50 Hz, draws its records with WebGPU,
// maps touch / keyboard / gamepads to the original twin-stick controls.
// URL flags: ?debug  ?mute  ?fresh (ignore saved prefs)  ?seed=N

import init, { Engine } from "./pkg/neobrawl.js";

const params = new URLSearchParams(location.search);
const TICK = 1 / 50;
const RECORD = 28;
const PREFS_KEY = "neobrawl.prefs";
const fresh = params.has("fresh");
const $ = (id) => document.getElementById(id);
const canvas = $("screen");

const nb = (window.__nb = { ready: false, errors: [], fps: 0, ticks: 0, draws: 0,
  state: () => (engine ? JSON.parse(engine.state()) : null) });
addEventListener("error", (e) => nb.errors.push(String(e.message || e)));
addEventListener("unhandledrejection", (e) => nb.errors.push(String(e.reason)));
let engine = null;

const loadPrefs = () => { if (fresh) return "{}"; try { return localStorage.getItem(PREFS_KEY) || "{}"; } catch { return "{}"; } };
const savePrefs = (s) => { if (fresh) return; try { localStorage.setItem(PREFS_KEY, s); } catch {} };

// ---------------------------------------------------------------- audio ---
class Audio {
  constructor(files) {
    this.ctx = new (window.AudioContext || window.webkitAudioContext)();
    this.buffers = new Map();
    this.sources = new Map(); // music clip -> {node, gain}
    this.files = files;
  }
  load() {
    for (const [name, file] of this.files) {
      fetch(`assets/${file}`).then((r) => r.arrayBuffer()).then((d) => this.ctx.decodeAudioData(d))
        .then((b) => this.buffers.set(name, b)).catch((e) => nb.errors.push(`audio ${file}: ${e}`));
    }
  }
  unlock() { if (this.ctx.state !== "running") this.ctx.resume(); }
  shot(name, vol) {
    const b = this.buffers.get(name);
    if (!b || this.ctx.state !== "running") return;
    const s = this.ctx.createBufferSource(), g = this.ctx.createGain();
    s.buffer = b; g.gain.value = Math.min(1, vol);
    s.connect(g).connect(this.ctx.destination); s.start();
  }
  music(list) {
    for (const [clip, playing, volume] of list) {
      let m = this.sources.get(clip);
      if (playing && !m) {
        const b = this.buffers.get(clip);
        if (!b) continue;
        const node = this.ctx.createBufferSource(), gain = this.ctx.createGain();
        node.buffer = b; node.loop = true;
        node.connect(gain).connect(this.ctx.destination); node.start();
        m = { node, gain }; this.sources.set(clip, m);
      }
      if (!m) continue;
      if (!playing) { m.node.stop(); this.sources.delete(clip); continue; }
      m.gain.gain.value = volume;
    }
  }
}

// ------------------------------------------------------------- renderer ---
const WGSL = /* wgsl */ `
struct Pass { vp: mat4x4f };
struct Mesh { m: mat4x4f, color: vec4f };
@group(0) @binding(0) var<uniform> pass_: Pass;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;
struct VOut { @builtin(position) pos: vec4f, @location(0) uv: vec2f, @location(1) color: vec4f };

// quads: per-vertex world position, uv and color
@vertex fn vs_quad(@location(0) p: vec3f, @location(1) uv: vec2f, @location(2) c: vec4f) -> VOut {
  var o: VOut; o.pos = pass_.vp * vec4f(p, 1.0); o.uv = vec2f(uv.x, 1.0 - uv.y); o.color = c; return o;
}
@fragment fn fs_quad(i: VOut) -> @location(0) vec4f { return textureSample(tex, samp, i.uv) * i.color; }
@fragment fn fs_text(i: VOut) -> @location(0) vec4f { return vec4f(i.color.rgb, i.color.a * textureSample(tex, samp, i.uv).a); }

@group(2) @binding(0) var<uniform> mesh: Mesh;
@vertex fn vs_mesh(@location(0) p: vec3f) -> VOut {
  var o: VOut; o.pos = pass_.vp * mesh.m * vec4f(p, 1.0); o.uv = vec2f(0.0); o.color = mesh.color; return o;
}
@fragment fn fs_mesh(i: VOut) -> @location(0) vec4f { return i.color; }`;

const ALIGN = [[0, 1], [0.5, 1], [1, 1], [0, 0.5], [0.5, 0.5], [1, 0.5], [0, 0], [0.5, 0], [1, 0]]; // TextAnchor -> (x, y from bottom)

class Renderer {
  static async create() {
    if (!navigator.gpu) throw new Error("This browser has no WebGPU. Use Chrome, Edge or Safari 26+.");
    const adapter = await navigator.gpu.requestAdapter();
    if (!adapter) throw new Error("No WebGPU adapter.");
    const device = await adapter.requestDevice();
    device.addEventListener?.("uncapturederror", (e) => nb.errors.push(`gpu: ${e.error.message}`));
    return new Renderer(device);
  }
  constructor(device) {
    this.d = device;
    this.ctx = canvas.getContext("webgpu");
    this.format = navigator.gpu.getPreferredCanvasFormat();
    this.ctx.configure({ device, format: this.format, alphaMode: "opaque" });
    const module = device.createShaderModule({ code: WGSL });
    const L = (entries) => device.createBindGroupLayout({ entries });
    this.l0 = L([{ binding: 0, visibility: GPUShaderStage.VERTEX, buffer: { type: "uniform", hasDynamicOffset: true } }]);
    this.l1 = L([{ binding: 0, visibility: GPUShaderStage.FRAGMENT, texture: {} }, { binding: 1, visibility: GPUShaderStage.FRAGMENT, sampler: {} }]);
    this.l2 = L([{ binding: 0, visibility: GPUShaderStage.VERTEX | GPUShaderStage.FRAGMENT, buffer: { type: "uniform", hasDynamicOffset: true } }]);
    const blend = { color: { srcFactor: "src-alpha", dstFactor: "one-minus-src-alpha" }, alpha: { srcFactor: "one", dstFactor: "one-minus-src-alpha" } };
    const quadVB = [{ arrayStride: 36, attributes: [
      { shaderLocation: 0, offset: 0, format: "float32x3" }, { shaderLocation: 1, offset: 12, format: "float32x2" },
      { shaderLocation: 2, offset: 20, format: "float32x4" }] }];
    const mk = (vs, fs, buffers, layouts, depthWrite, b) => device.createRenderPipeline({
      layout: device.createPipelineLayout({ bindGroupLayouts: layouts }),
      vertex: { module, entryPoint: vs, buffers },
      fragment: { module, entryPoint: fs, targets: [{ format: this.format, blend: b }] },
      primitive: { topology: "triangle-list", cullMode: "none" },
      depthStencil: { format: "depth24plus", depthWriteEnabled: depthWrite, depthCompare: "less-equal" },
    });
    this.pQuad = mk("vs_quad", "fs_quad", quadVB, [this.l0, this.l1], false, blend);
    this.pText = mk("vs_quad", "fs_text", quadVB, [this.l0, this.l1], false, blend);
    this.pMesh = mk("vs_mesh", "fs_mesh", [{ arrayStride: 12, attributes: [{ shaderLocation: 0, offset: 0, format: "float32x3" }] }], [this.l0, this.l1, this.l2], true, undefined);
    this.sampler = device.createSampler({ magFilter: "linear", minFilter: "linear" });
    this.passBuf = device.createBuffer({ size: 256 * 16, usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST });
    this.g0 = device.createBindGroup({ layout: this.l0, entries: [{ binding: 0, resource: { buffer: this.passBuf, size: 64 } }] });
    this.meshCap = 0;
    this.vbCap = 0;
    this.verts = new Float32Array(9 * 6 * 4096);
    this.white = this.bind(this.texFrom(new Uint8Array([255, 255, 255, 255]), 1, 1));
    this.atlases = new Map();
  }
  texFrom(px, w, h) {
    const t = this.d.createTexture({ size: [w, h], format: "rgba8unorm", usage: GPUTextureUsage.TEXTURE_BINDING | GPUTextureUsage.COPY_DST });
    this.d.queue.writeTexture({ texture: t }, px, { bytesPerRow: w * 4 }, [w, h]);
    return t;
  }
  texFromSource(src) {
    const t = this.d.createTexture({ size: [src.width, src.height], format: "rgba8unorm",
      usage: GPUTextureUsage.TEXTURE_BINDING | GPUTextureUsage.COPY_DST | GPUTextureUsage.RENDER_ATTACHMENT });
    this.d.queue.copyExternalImageToTexture({ source: src }, { texture: t }, [src.width, src.height]);
    return t;
  }
  bind(t) {
    return this.d.createBindGroup({ layout: this.l1, entries: [{ binding: 0, resource: t.createView() }, { binding: 1, resource: this.sampler }] });
  }
  async load(assets, tables) {
    this.textures = await Promise.all(tables.textures.map(async (file) => {
      const img = await createImageBitmap(await (await fetch(`assets/${file}`)).blob(), { premultiplyAlpha: "none", colorSpaceConversion: "none" });
      return this.bind(this.texFromSource(img));
    }));
    this.meshes = tables.meshes.map((key) => {
      const m = assets.meshes[key];
      const v = new Float32Array(m.pos.flat()), idx = new Uint32Array(m.indices);
      const vb = this.d.createBuffer({ size: v.byteLength, usage: GPUBufferUsage.VERTEX | GPUBufferUsage.COPY_DST });
      const ib = this.d.createBuffer({ size: idx.byteLength, usage: GPUBufferUsage.INDEX | GPUBufferUsage.COPY_DST });
      this.d.queue.writeBuffer(vb, 0, v); this.d.queue.writeBuffer(ib, 0, idx);
      return { vb, ib, count: idx.length };
    });
  }
  // glyph atlas for the game's font at a fixed raster size
  atlas(family, style) {
    const key = `${family}|${style}`;
    let a = this.atlases.get(key);
    if (a) return a;
    const px = 96, css = `${style & 1 ? "bold " : ""}${px}px "${family}"`;
    const c = document.createElement("canvas"), g = c.getContext("2d");
    g.font = css;
    const lineH = Math.ceil(px * 1.25), W = 2048, glyphs = new Map();
    let x = 0, y = 0;
    const cells = [];
    for (let code = 32; code < 127; code++) {
      const ch = String.fromCharCode(code), adv = g.measureText(ch).width, w = Math.ceil(adv) + 6;
      if (x + w > W) { x = 0; y += lineH; }
      cells.push([code, ch, x, y, w, adv]); x += w;
    }
    c.width = W; c.height = y + lineH; g.font = css; g.fillStyle = "#fff"; g.textBaseline = "alphabetic";
    for (const [code, ch, cx, cy, w, adv] of cells) {
      g.fillText(ch, cx + 3, cy + Math.round(px * 0.95));
      glyphs.set(code, { u: cx / W, v: cy / c.height, w: w / W, h: lineH / c.height, pw: w, adv });
    }
    const mg = document.createElement("canvas").getContext("2d"); mg.font = css;
    a = { group: this.bind(this.texFromSource(c)), glyphs, lineH, px, measure: (s) => mg.measureText(s).width };
    this.atlases.set(key, a);
    return a;
  }
  draw(rec, texts, family, screenW, screenH) {
    const d = this.d, dpr = Math.min(devicePixelRatio || 1, 2);
    const W = Math.max(1, Math.round(canvas.clientWidth * dpr)), H = Math.max(1, Math.round(canvas.clientHeight * dpr));
    if (canvas.width !== W || canvas.height !== H || !this.depth) {
      canvas.width = W; canvas.height = H;
      this.depth?.destroy();
      this.depth = d.createTexture({ size: [W, H], format: "depth24plus", usage: GPUTextureUsage.RENDER_ATTACHMENT });
    }
    // 1) build vertex data + a command list
    const n = rec.length / RECORD;
    let v = this.verts, vc = 0;
    const need = (k) => { if ((vc + k) * 9 > v.length) { const g = new Float32Array(Math.max(v.length * 2, (vc + k) * 9 + 9000)); g.set(v.subarray(0, vc * 9)); v = this.verts = g; } };
    const put = (p, uv, c) => { v.set([p[0], p[1], p[2], uv[0], uv[1], c[0], c[1], c[2], c[3]], vc * 9); vc++; };
    const quadV = (P, uv, c) => {
      need(6);
      const [u0, v0, u1, v1] = uv;
      const T = [[u0, v0], [u1, v0], [u1, v1], [u0, v1]];
      for (const k of [0, 1, 2, 0, 2, 3]) put(P[k], T[k], c);
    };
    const passes = [], meshes = [];
    let cur = null;
    const cmd = (kind, bind) => {
      const last = cur.cmds[cur.cmds.length - 1];
      if (last && last.kind === kind && last.bind === bind) return last;
      const c = { kind, bind, first: vc, count: 0 }; cur.cmds.push(c); return c;
    };
    for (let i = 0; i < n; i++) {
      const o = i * RECORD, kind = rec[o];
      if (kind === 9) { cur = { clear: rec[o + 1] === 1, vp: rec.slice(o + 4, o + 20), bg: rec.slice(o + 20, o + 24), cmds: [] }; passes.push(cur); continue; }
      if (kind === 0) { meshes.push(rec.slice(o + 4, o + 24)); cur.cmds.push({ kind: "mesh", mesh: rec[o + 1], idx: meshes.length - 1 }); continue; }
      const P = [0, 1, 2, 3].map((k) => [rec[o + 4 + k * 3], rec[o + 5 + k * 3], rec[o + 6 + k * 3]]);
      const color = [rec[o + 20], rec[o + 21], rec[o + 22], rec[o + 23]];
      if (kind === 2) {
        const t = rec[o + 1] < 0 ? this.white : this.textures[rec[o + 1]];
        const c = cmd("quad", t); const f = vc;
        quadV(P, [rec[o + 16], rec[o + 17], rec[o + 18], rec[o + 19]], color); c.count += vc - f;
      } else if (kind === 1) {
        const t = texts[rec[o + 1]];
        if (!t) continue;
        const at = this.atlas(family, t[1]);
        const glyphH = rec[o + 2], align = ALIGN[rec[o + 3]] || ALIGN[0], spacing = rec[o + 17];
        const ax = [P[1][0] - P[0][0], P[1][1] - P[0][1], P[1][2] - P[0][2]], ay = [P[3][0] - P[0][0], P[3][1] - P[0][1], P[3][2] - P[0][2]];
        const rw = Math.hypot(...ax), rh = Math.hypot(...ay);
        if (rw < 1e-6 || rh < 1e-6) continue;
        const ux = ax.map((q) => q / rw), uy = ay.map((q) => q / rh);
        const k = glyphH / at.px; // world units per atlas pixel
        const lines = t[0].replace(/\n+$/, "").split("\n");
        const lh = at.px * 1.15 * k, total = lh * lines.length;
        const c = cmd("text", at.group); const f = vc;
        lines.forEach((line, li) => {
          const width = at.measure(line) * k + spacing * Math.max(0, line.length - 1);
          let x = (rw - width) * align[0];
          const top = rh - (rh - total) * (1 - align[1]) - li * lh; // baseline-ish placement from the rect bottom
          const y0 = top - lh;
          for (let ci = 0; ci < line.length; ci++) {
            const g = at.glyphs.get(line.charCodeAt(ci)) || at.glyphs.get(63);
            const px0 = x + (at.measure(line.slice(0, ci)) - 3) * k + spacing * ci, gw = g.pw * k, gh = at.lineH * k;
            const pt = (sx, sy) => [P[0][0] + ux[0] * sx + uy[0] * sy, P[0][1] + ux[1] * sx + uy[1] * sy, P[0][2] + ux[2] * sx + uy[2] * sy];
            const yb = y0 + (lh - gh);
            quadV([pt(px0, yb), pt(px0 + gw, yb), pt(px0 + gw, yb + gh), pt(px0, yb + gh)], [g.u, 1 - g.v - g.h, g.u + g.w, 1 - g.v], color);
          }
          x += 0;
        });
        c.count += vc - f;
      }
    }
    // 2) upload
    const vbytes = Math.max(64, vc * 36);
    if (vbytes > this.vbCap) { this.vbCap = Math.max(vbytes, this.vbCap * 2); this.vb?.destroy(); this.vb = d.createBuffer({ size: this.vbCap, usage: GPUBufferUsage.VERTEX | GPUBufferUsage.COPY_DST }); }
    if (vc) d.queue.writeBuffer(this.vb, 0, v, 0, vc * 9);
    if (meshes.length * 256 > this.meshCap) {
      this.meshCap = Math.max(meshes.length * 256, this.meshCap * 2, 256 * 64);
      this.meshBuf?.destroy();
      this.meshBuf = d.createBuffer({ size: this.meshCap, usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST });
      this.g2 = d.createBindGroup({ layout: this.l2, entries: [{ binding: 0, resource: { buffer: this.meshBuf, size: 80 } }] });
    }
    const mdata = new Float32Array(meshes.length * 64);
    meshes.forEach((m, k) => mdata.set(m, k * 64));
    if (meshes.length) d.queue.writeBuffer(this.meshBuf, 0, mdata);
    const pdata = new Float32Array(passes.length * 64);
    passes.forEach((p, k) => pdata.set(p.vp, k * 64));
    d.queue.writeBuffer(this.passBuf, 0, pdata);
    // 3) encode
    const enc = d.createCommandEncoder();
    const view = this.ctx.getCurrentTexture().createView(), dview = this.depth.createView();
    let calls = 0;
    passes.forEach((p, pi) => {
      const first = pi === 0;
      const pass = enc.beginRenderPass({
        colorAttachments: [{ view, loadOp: p.clear || first ? "clear" : "load", storeOp: "store",
          clearValue: { r: p.bg[0], g: p.bg[1], b: p.bg[2], a: 1 } }],
        depthStencilAttachment: { view: dview, depthLoadOp: "clear", depthClearValue: 1, depthStoreOp: "discard" },
      });
      pass.setBindGroup(0, this.g0, [pi * 256]);
      for (const c of p.cmds) {
        if (c.kind === "mesh") {
          const m = this.meshes[c.mesh];
          pass.setPipeline(this.pMesh); pass.setBindGroup(1, this.white); pass.setBindGroup(2, this.g2, [c.idx * 256]);
          pass.setVertexBuffer(0, m.vb); pass.setIndexBuffer(m.ib, "uint32"); pass.drawIndexed(m.count);
        } else if (c.count) {
          pass.setPipeline(c.kind === "text" ? this.pText : this.pQuad); pass.setBindGroup(1, c.bind);
          pass.setVertexBuffer(0, this.vb); pass.draw(c.count, 1, c.first);
        }
        calls++;
      }
      pass.end();
    });
    d.queue.submit([enc.finish()]);
    nb.draws = calls;
  }
}

// ---------------------------------------------------------------- input ---
const keys = new Set();
// P1: WASD move, TFGH aim.  P2: arrows move, IJKL aim.
const BIND = [
  { mv: ["KeyW", "KeyA", "KeyS", "KeyD"], aim: ["KeyT", "KeyF", "KeyG", "KeyH"] },
  { mv: ["ArrowUp", "ArrowLeft", "ArrowDown", "ArrowRight"], aim: ["KeyI", "KeyJ", "KeyK", "KeyL"] },
];
const axis = ([up, left, down, right]) => {
  const x = (keys.has(right) ? 1 : 0) - (keys.has(left) ? 1 : 0), y = (keys.has(up) ? 1 : 0) - (keys.has(down) ? 1 : 0);
  const l = Math.hypot(x, y) || 1;
  return [x / l, y / l];
};
function pollSticks() {
  const pads = (navigator.getGamepads?.() || []).filter(Boolean);
  for (let p = 0; p < 2; p++) {
    let [mx, my] = axis(BIND[p].mv), [ax, ay] = axis(BIND[p].aim);
    const pad = pads[p];
    if (pad) {
      const dz = (v) => (Math.abs(v) < 0.2 ? 0 : v);
      if (dz(pad.axes[0]) || dz(pad.axes[1])) { mx = dz(pad.axes[0]); my = -dz(pad.axes[1]); }
      if (dz(pad.axes[2]) || dz(pad.axes[3])) { ax = dz(pad.axes[2]); ay = -dz(pad.axes[3]); }
    }
    const active = !!(mx || my || ax || ay);
    engine.stick(p, mx, my, ax, ay, active);
  }
}

async function main() {
  const [wasm, sceneText, assetsText] = await Promise.all([init(), fetch("assets/scene.json").then((r) => r.text()), fetch("assets/assets.json").then((r) => r.text())]);
  const assets = JSON.parse(assetsText);
  const cssW = () => canvas.clientWidth || innerWidth, cssH = () => canvas.clientHeight || innerHeight;
  engine = new Engine(sceneText, assetsText, loadPrefs(), BigInt(params.get("seed") || Math.floor(Math.random() * 2 ** 31)), cssW(), cssH());
  nb.engine = engine;
  const tables = JSON.parse(engine.tables());
  let family = "sans-serif";
  if (tables.font) {
    const f = Object.values(assets.fonts)[0];
    const face = new FontFace(f.name, `url(assets/${f.file})`);
    try { await face.load(); document.fonts.add(face); family = f.name; } catch (e) { nb.errors.push(`font: ${e}`); }
  }
  const r = await Renderer.create();
  await r.load(assets, tables);
  let audio = null;
  if (!params.has("mute")) { audio = new Audio(Object.values(assets.audio).map((a) => [a.name, a.file])); audio.load(); }

  const toGame = (e) => { const b = canvas.getBoundingClientRect(); return [e.clientX - b.left, b.height - (e.clientY - b.top)]; };
  const ptr = (kind) => (e) => { audio?.unlock(); if (kind === 0) canvas.setPointerCapture?.(e.pointerId); const [x, y] = toGame(e); engine.pointer(e.pointerId, x, y, kind); e.preventDefault(); };
  canvas.addEventListener("pointerdown", ptr(0));
  canvas.addEventListener("pointermove", (e) => { if (e.buttons || e.pointerType === "touch") ptr(1)(e); });
  canvas.addEventListener("pointerup", ptr(2));
  canvas.addEventListener("pointercancel", ptr(2));
  addEventListener("keydown", (e) => { audio?.unlock(); if (e.metaKey || e.ctrlKey) return; keys.add(e.code); if (e.code.startsWith("Arrow") || e.code === "Space") e.preventDefault(); });
  addEventListener("keyup", (e) => keys.delete(e.code));
  addEventListener("blur", () => keys.clear());
  addEventListener("resize", () => engine.set_screen(cssW(), cssH()));

  $("overlay").hidden = true;
  const debug = params.has("debug"); $("debug").hidden = !debug;
  let texts = {}, textsAt = -1;
  let last = performance.now(), acc = 0, frames = 0, fpsT = last;
  const loop = (now) => {
    acc += Math.min(0.25, (now - last) / 1000); last = now;
    let steps = 0;
    while (acc >= TICK && steps < 8) { pollSticks(); engine.frame(); acc -= TICK; steps++; nb.ticks++; }
    if (audio) {
      const s = engine.drain_sounds(); if (s !== "[]") for (const [c, v] of JSON.parse(s)) audio.shot(c, v);
      audio.music(JSON.parse(engine.music()));
    }
    const p = engine.take_prefs(); if (p) savePrefs(p);
    if (nb.ticks !== textsAt) { texts = JSON.parse(engine.texts()); textsAt = nb.ticks; }
    const len = engine.render();
    r.draw(new Float32Array(wasm.memory.buffer, engine.records_ptr(), len), texts, family, cssW(), cssH());
    frames++;
    if (now - fpsT >= 500) { nb.fps = Math.round((frames * 1000) / (now - fpsT)); frames = 0; fpsT = now; }
    if (debug) $("debug").textContent = `fps ${nb.fps} draws ${nb.draws}\n${engine.state()}`;
    requestAnimationFrame(loop);
  };
  requestAnimationFrame(loop);
  nb.ready = true;
}
main().catch((e) => { console.error(e); nb.errors.push(String(e)); $("status").textContent = String(e.message || e); });
