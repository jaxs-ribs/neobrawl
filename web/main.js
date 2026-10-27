// NeoBrawl host: runs the WASM game at 50 Hz, draws its records with WebGPU,
// maps touch / keyboard / gamepads to the original twin-stick controls.
// URL flags: ?debug  ?mute  ?fresh (ignore saved prefs)  ?seed=N

// the build stamps ?v=<hash> on this script; pass it on to the engine files
const V = new URL(import.meta.url).search;
const { default: init, Engine } = await import(`./pkg/neobrawl.js${V}`);

const params = new URLSearchParams(location.search);
const TICK = 1 / 50;
const RECORD = 28;
const PREFS_KEY = "neobrawl.prefs";
const fresh = params.has("fresh");
const $ = (id) => document.getElementById(id);
const canvas = $("screen");

const nb = (window.__nb = { Audio: null, cpuMs: 0, ready: false, errors: [], fps: 0, ticks: 0, draws: 0,
  state: () => (engine ? JSON.parse(engine.state()) : null) });
addEventListener("error", (e) => nb.errors.push(String(e.message || e)));
addEventListener("unhandledrejection", (e) => nb.errors.push(String(e.reason)));
let engine = null;

const loadPrefs = () => { if (fresh) return "{}"; try { return localStorage.getItem(PREFS_KEY) || "{}"; } catch { return "{}"; } };
const savePrefs = (s) => { if (fresh) return; try { localStorage.setItem(PREFS_KEY, s); } catch {} };

// ---------------------------------------------------------------- audio ---
class Audio {
  // Mix: the original played effects as 3D one-shots in the world, so they
  // sat well under the music. Effects go through their own bus and a limiter,
  // repeats of the same clip are voice-limited and slightly detuned.
  constructor(files, offline) {
    // `offline` (an OfflineAudioContext) renders silently, for checking levels
    this.offline = !!offline;
    this.ctx = offline || new (window.AudioContext || window.webkitAudioContext)();
    const c = this.ctx;
    this.limiter = c.createDynamicsCompressor();
    this.limiter.threshold.value = -14; this.limiter.knee.value = 10; this.limiter.ratio.value = 6;
    this.limiter.attack.value = 0.003; this.limiter.release.value = 0.15;
    this.master = c.createGain(); this.master.gain.value = 0.9;
    this.sfx = c.createGain(); this.sfx.gain.value = 0.35;
    this.musicBus = c.createGain(); this.musicBus.gain.value = 1.0;
    // the music breathes with the game: darker in lulls, muffled when
    // paused, ducked for a beat when a soul is ended
    this.musicTone = c.createBiquadFilter(); this.musicTone.type = "lowpass"; this.musicTone.frequency.value = 20000; this.musicTone.Q.value = 0.5;
    this.musicDuck = c.createGain(); this.musicDuck.gain.value = 1;
    this.musicBus.connect(this.musicTone).connect(this.musicDuck);
    this.sfx.connect(this.limiter); this.musicDuck.connect(this.limiter);
    // a second, gentler bus for the synthesised voices
    this.synth = c.createGain(); this.synth.gain.value = 4.4; this.synth.connect(this.sfx);
    this.cueGain = 1;
    this.noise = c.createBuffer(1, c.sampleRate, c.sampleRate);
    { const d = this.noise.getChannelData(0); for (let i = 0; i < d.length; i++) d[i] = Math.random() * 2 - 1; }
    this.rate = new Map(); // cue -> recent times (rate limiting)
    this.drones = [null, null];
    this.limiter.connect(this.master).connect(c.destination);
    this.buffers = new Map();
    this.sources = new Map();
    this.voices = new Map(); // clip -> [{end, src}]
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
    if (!b || !this.running()) return;
    const now = this.ctx.currentTime;
    const live = (this.voices.get(name) || []).filter((v) => v.end > now);
    // the same clip retriggered within 60 ms adds nothing but clipping
    if (live.some((v) => now - v.start < 0.06)) return;
    if (live.length >= 3) { const old = live.shift(); try { old.src.stop(); } catch {} }
    const s = this.ctx.createBufferSource(), g = this.ctx.createGain();
    s.buffer = b;
    s.playbackRate.value = 1 + (Math.random() - 0.5) * 0.08; // +-4% so repeats don't sound mechanical
    g.gain.setValueAtTime(0, now);
    g.gain.linearRampToValueAtTime(Math.min(1, vol) * (live.length ? 0.7 : 1), now + 0.004); // de-click
    s.connect(g).connect(this.sfx); s.start(now);
    live.push({ start: now, end: now + b.duration / s.playbackRate.value, src: s });
    this.voices.set(name, live);
  }
  // ---- synthesised voices ------------------------------------------------
  // The originals: a 1 kHz sine beep (click), a bubbly downward bloop
  // (pickup), low airy noise (dash), a sub-heavy noise burst (explosion).
  // The new voices are built from the same stuff - sines, quick pitch
  // drops, filtered noise - kept small and uneven, and every tonal one sits
  // in A minor pentatonic so it is in tune with both themes (Map 1 is
  // C / A minor at ~128 BPM, the menu drone is F major).
  note(deg, oct = 0) {
    const P = [0, 3, 5, 7, 10];
    const d = Math.floor(deg), o = Math.floor(d / 5) + oct;
    return 220 * Math.pow(2, (P[((d % 5) + 5) % 5] + 12 * o) / 12);
  }
  out(pan, gain) {
    const c = this.ctx, g = c.createGain(), p = c.createStereoPanner ? c.createStereoPanner() : null;
    g.gain.value = gain;
    if (p) { p.pan.value = Math.max(-1, Math.min(1, pan)); g.connect(p).connect(this.synth); } else g.connect(this.synth);
    return g;
  }
  env(g, t, a, peak, d) {
    peak *= this.cueGain; // the voice's trim
    g.gain.setValueAtTime(0, t); g.gain.linearRampToValueAtTime(peak, t + a); g.gain.exponentialRampToValueAtTime(0.0001, t + a + d);
  }
  tone(t, f, f2, type, a, d, peak, pan) {
    const c = this.ctx, o = c.createOscillator(), g = this.out(pan, 1);
    o.type = type; o.frequency.setValueAtTime(f, t); if (f2 !== f) o.frequency.exponentialRampToValueAtTime(f2, t + a + d * 0.6);
    this.env(g, t, a, peak, d); o.connect(g); o.start(t); o.stop(t + a + d + 0.05);
  }
  hiss(t, type, f, f2, q, a, d, peak, pan) {
    const c = this.ctx, n = c.createBufferSource(), bp = c.createBiquadFilter(), g = this.out(pan, 1);
    n.buffer = this.noise; n.loop = true; bp.type = type; bp.Q.value = q;
    bp.frequency.setValueAtTime(f, t); if (f2 !== f) bp.frequency.exponentialRampToValueAtTime(f2, t + a + d);
    this.env(g, t, a, peak, d); n.connect(bp).connect(g); n.start(t, Math.random() * 0.9); n.stop(t + a + d + 0.05);
  }
  limited(name, perSec) {
    const now = this.ctx.currentTime, list = (this.rate.get(name) || []).filter((x) => now - x < 1);
    if (list.length >= perSec) return true;
    list.push(now); this.rate.set(name, list); return false;
  }
  running() { return this.offline || this.ctx.state === "running"; }
  cue(name, pan, amt) {
    if (!this.running()) return;
    // levels, set against the originals through the same chain (RMS):
    // explosion -21, dash -22, pickup -25, click -35, music bed ~-15
    this.cueGain = CUE_LEVEL[name] ?? 1;
    const t = this.ctx.currentTime + 0.005, R = Math.random, tail = (k) => Math.min(3, Math.pow(Math.max(R(), 0.02), -1 / k));
    switch (name) {
      case "spark": { // shard electricity: a dry crackle, one to three ticks
        if (this.limited(name, 22)) return;
        const n = 1 + (R() < 0.35 ? 1 : 0) + (R() < 0.1 ? 1 : 0);
        for (let i = 0; i < n; i++) this.hiss(t + i * (0.006 + R() * 0.02), "bandpass", 2400 * tail(2.5), 2400, 7, 0.001, 0.012 + R() * 0.02, 0.05 * Math.min(1.4, amt) / (i + 1), pan);
        break;
      }
      case "bolt": // breakdown: a snap with a falling chirp (the sim fires many; hear a few)
        if (R() < 0.6 || this.limited(name, 3)) return;
        this.hiss(t, "bandpass", 1800, 900, 3, 0.001, 0.05, 0.12, pan);
        this.tone(t, 1300, 420, "triangle", 0.002, 0.07, 0.05, pan);
        break;
      case "fling": // the soul blown apart: a bright whoosh falling away (the dash's air, harder)
        this.hiss(t, "lowpass", 5000, 300, 0.7, 0.004, 0.42, 0.32 * amt, pan);
        this.tone(t, 180, 55, "sine", 0.003, 0.25, 0.28 * amt, pan); // and a body blow under it
        this.duck(0.55, 0.45);
        break;
      case "soul": // the change of domain: a small glass ping per orb, scattered
        this.tone(t + R() * 0.05, this.note(5 + (amt | 0), 1), this.note(5 + (amt | 0), 1), "sine", 0.002, 0.22, 0.045, pan);
        break;
      case "home": { // each soul arriving: a bloop climbing the scale (the pickup sound, as a phrase)
        const f = this.note(amt + 2, 0);
        this.tone(t, f * 1.5, f, "triangle", 0.003, 0.26, 0.12, pan);
        this.tone(t, f * 2, f * 2, "sine", 0.002, 0.12, 0.03, pan);
        break;
      }
      case "refill": // a box refills the dash: crack, then an up-chirp
        this.hiss(t, "highpass", 3500, 3500, 0.7, 0.001, 0.03, 0.12, pan);
        this.tone(t + 0.01, this.note(7), this.note(12), "sine", 0.003, 0.09, 0.09, pan);
        break;
      case "ready": // dash back: one crisp, unmistakable snap-up (the same for a refill)
        this.hiss(t, "highpass", 5200, 5200, 0.7, 0.0005, 0.006, 0.12, pan);
        this.tone(t, this.note(9), this.note(14), "triangle", 0.001, 0.07, 0.07, pan);
        this.tone(t + 0.035, this.note(17), this.note(17), "sine", 0.001, 0.09, 0.04, pan);
        break;
      case "loaded": // the charge passes jab range: a tight two-step tick upward, quieter than the dash's snap
        this.tone(t, this.note(5), this.note(5), "sine", 0.001, 0.035, 0.05, pan);
        this.tone(t + 0.045, this.note(10), this.note(10), "sine", 0.001, 0.06, 0.045, pan);
        break;
      case "bounce": // a box glances off a wall: a dry tick with a short ring, harder with speed
        if (this.limited(name, 10)) return;
        this.hiss(t, "bandpass", 3200, 2400, 4, 0.0005, 0.025, 0.1 * amt, pan);
        this.tone(t, 1150 + 300 * amt, 800, "triangle", 0.001, 0.06, 0.05 * amt, pan);
        break;
      case "thud": // an obstacle flexes: a low wobbling knock
        this.tone(t, 92, 58, "sine", 0.002, 0.2, 0.3 * amt, pan);
        this.hiss(t, "lowpass", 600, 200, 0.7, 0.001, 0.06, 0.08 * amt, pan);
        break;
      case "turn": // a U-turn squash
        if (this.limited(name, 6)) return;
        this.tone(t, 150, 95, "sine", 0.002, 0.05, 0.07, pan);
        break;
      case "gather": // respawn: air drawn in, then a soft bloop
        this.hiss(t, "lowpass", 250, 4000, 0.9, 0.4, 0.06, 0.12, pan);
        this.tone(t + 0.45, this.note(5) * 1.4, this.note(5), "triangle", 0.003, 0.22, 0.1, pan);
        break;
      case "hover": // the cursor's spark leaps to a button
        if (this.limited(name, 8)) return;
        this.hiss(t, "bandpass", 5200, 5200, 9, 0.001, 0.018, 0.035, pan);
        break;
      case "tick": // a value rewritten
        this.tone(t, this.note(12), this.note(12), "sine", 0.001, 0.04, 0.03, pan);
        this.tone(t + 0.035, this.note(14), this.note(14), "sine", 0.001, 0.05, 0.025, pan);
        break;
      case "break": { // a box shatters: the original's character, minus what grated
        // The original sample is the sound. What made it annoying with
        // repetition was its 0.96 s smear, a boomy sub that sat on the
        // music's bass, and being identical every time. So: it is cut off by
        // ~0.4 s and darkens as it fades, the sub is shelved down, every play
        // varies, and a quiet layer of glass (resonant noise "tinks" at random
        // high frequencies, not notes) scatters under the tail.
        if (this.limited(name, 14)) return;
        const mat = Math.floor(amt / 10), s = amt - mat * 10;
        const hard = 0.55 + 0.45 * s, c = this.ctx;
        const buf = this.buffers.get("explosion");
        if (buf) {
          const src = c.createBufferSource(), shelf = c.createBiquadFilter(), lp = c.createBiquadFilter(), g = this.out(pan, 0);
          src.buffer = buf;
          src.playbackRate.value = (1.06 - 0.12 * s) * (0.93 + R() * 0.14); // heavier hits lower, never the same twice
          shelf.type = "lowshelf"; shelf.frequency.value = 120; shelf.gain.value = -5;
          lp.type = "lowpass"; lp.Q.value = 0.5;
          lp.frequency.setValueAtTime(14000, t); lp.frequency.exponentialRampToValueAtTime(3500 + 2500 * s, t + 0.35);
          const pk = 0.85 * hard * this.cueGain, end = 0.36 + 0.1 * s;
          g.gain.setValueAtTime(0, t); g.gain.linearRampToValueAtTime(pk, t + 0.003);
          g.gain.setValueAtTime(pk, t + 0.06); g.gain.exponentialRampToValueAtTime(pk * 0.25, t + end * 0.55); g.gain.exponentialRampToValueAtTime(0.0001, t + end);
          src.connect(shelf).connect(lp).connect(g);
          src.start(t, R() * 0.012); src.stop(t + end + 0.05);
        }
        // glass: resonant noise tinks, scattered over the tail, quiet
        const grains = Math.round(8 + 10 * s + R() * 4);
        for (let k = 0; k < grains; k++) {
          const at = 0.01 + Math.pow(R(), 1.7) * 0.32;
          const f = 2600 + R() * 5200;
          this.hiss(t + at, "bandpass", f, f * 0.97, 10, 0.0008, 0.015 + R() * 0.045, (0.06 + 0.1 * Math.pow(R(), 3)) * (1 - at * 2) * hard, pan + (R() - 0.5) * 1.0);
        }
        break;
      }
      case "shatter": { // UI glass breaking: a few high pings, unevenly spread
        if (this.limited(name, 10)) return;
        const n = 2 + Math.floor(amt * 4 * R());
        for (let i = 0; i < n; i++) this.tone(t + Math.pow(R(), 2) * 0.09, this.note(10 + Math.floor(R() * 8)), this.note(10 + Math.floor(R() * 8)) * 0.97, "sine", 0.001, 0.04 + R() * 0.08, 0.025, pan + (R() - 0.5) * 0.4);
        break;
      }
      case "assemble": { // the arena drawing itself: a sparse, quickening scatter of ticks, then the fight cue
        const n = Math.round(10 + 14 * amt);
        for (let i = 0; i < n; i++) {
          const at = 0.35 + 0.9 * Math.pow(i / n, 0.8) + (R() - 0.5) * 0.05;
          this.hiss(t + at, "bandpass", 2000 + R() * 3000, 1500, 6, 0.001, 0.02, 0.03, (R() - 0.5) * 1.4);
        }
        // the arc snapping between the souls: a fifth, struck once
        this.tone(t + 1.35, this.note(5), this.note(5), "triangle", 0.002, 0.5, 0.08, 0);
        this.tone(t + 1.35, this.note(8), this.note(8), "sine", 0.002, 0.45, 0.05, 0);
        this.hiss(t + 1.35, "bandpass", 3000, 1200, 2, 0.001, 0.08, 0.06, 0);
        break;
      }
    }
  }
  // music ducks for a moment and comes back
  duck(depth, time) {
    const g = this.musicDuck.gain, f = this.musicTone.frequency, now = this.ctx.currentTime;
    g.cancelScheduledValues(now); g.setValueAtTime(g.value, now); g.linearRampToValueAtTime(depth, now + 0.02); g.setTargetAtTime(1, now + 0.05, time / 3);
    this.kickTone = now;
  }
  // continuous: charge drones per player, and the music's tone
  mix(m) {
    if (!this.running()) return;
    const c = this.ctx, now = c.currentTime;
    for (let k = 0; k < 2; k++) {
      const charge = m[k * 2], pan = m[k * 2 + 1];
      let d = this.drones[k];
      if (charge > 0.02 && !d) {
        // a held breath: a low filtered tone that rises and tightens with the charge
        const o = c.createOscillator(), o2 = c.createOscillator(), lp = c.createBiquadFilter(), g = this.out(pan, 0), trem = c.createGain(), lfo = c.createOscillator();
        o.type = "sawtooth"; o2.type = "sine"; lp.type = "lowpass"; lp.Q.value = 4;
        lfo.frequency.value = 9; trem.gain.value = 0; lfo.connect(trem).connect(g.gain);
        o.connect(lp); o2.connect(lp); lp.connect(g); o.start(); o2.start(); lfo.start();
        d = this.drones[k] = { o, o2, lp, g, trem, lfo };
      }
      if (!d) continue;
      if (charge <= 0.02) {
        d.g.gain.setTargetAtTime(0, now, 0.03);
        const dd = d; setTimeout(() => { try { dd.o.stop(); dd.o2.stop(); dd.lfo.stop(); } catch {} }, 300);
        this.drones[k] = null; continue;
      }
      const f = this.note(-5 + charge * 5); // A2 rising to A3
      d.o.frequency.setTargetAtTime(f, now, 0.05); d.o2.frequency.setTargetAtTime(f * 0.5, now, 0.05);
      d.lp.frequency.setTargetAtTime(180 + 1400 * charge * charge, now, 0.05);
      const base = 0.06 * 0.21 * Math.sqrt(charge);
      d.g.gain.setTargetAtTime(base, now, 0.05);
      // at full charge it trembles (the soul does too)
      d.trem.gain.setTargetAtTime(charge >= 1 ? base * 0.45 : 0, now, 0.05);
      d.lfo.frequency.setTargetAtTime(charge >= 1 ? 18 : 9, now, 0.1);
      if (d.g.pan) d.g.pan.value = pan;
    }
    // the music: muffled when paused, a little darker in a lull (holding its breath)
    const lull = m[4], paused = m[5];
    const target = paused ? 650 : 20000 * Math.pow(0.18, lull);
    this.musicTone.frequency.setTargetAtTime(target, now, paused ? 0.08 : 0.6);
  }
  music(list) {
    const now = this.ctx.currentTime;
    for (const [clip, playing, volume] of list) {
      let m = this.sources.get(clip);
      if (playing && !m) {
        const b = this.buffers.get(clip);
        if (!b) continue;
        const node = this.ctx.createBufferSource(), gain = this.ctx.createGain();
        node.buffer = b; node.loop = true; gain.gain.value = 0;
        node.connect(gain).connect(this.musicBus); node.start();
        m = { node, gain }; this.sources.set(clip, m);
      }
      if (!m) continue;
      if (!playing) {
        // fade instead of cutting
        m.gain.gain.setTargetAtTime(0, now, 0.08);
        const node = m.node; setTimeout(() => { try { node.stop(); } catch {} }, 600);
        this.sources.delete(clip); continue;
      }
      m.gain.gain.setTargetAtTime(volume, now, 0.05);
    }
  }
}

nb.Audio = Audio; // for offline level checks
// per-voice trims (x the synth bus) aiming at: fling ~-26 dB RMS, home /
// refill / thud ~-30, gather ~-32, soul / assemble ~-36, bolt / shatter /
// tick / turn ~-38, ready ~-40, spark ~-42, hover ~-44
const CUE_LEVEL = { loaded: 2.6, bounce: 1.4, break: 0.6, fling: 1.2, home: 2.1, refill: 2.7, thud: 0.95, gather: 1, soul: 1.45, assemble: 0.72, bolt: 2.8, shatter: 1.45, tick: 2.8, turn: 2.24, ready: 3.85, spark: 9, hover: 10 };

// ----------------------------------------------------------------- perf ---
// The observables: every rAF frame's interval and CPU split, the engine's
// per-phase timings, GPU time (timestamp queries), heap, and long tasks.
// ?perf (or F3) shows the overlay; window.__nb.perf always has the numbers.
class Perf {
  constructor() {
    this.dt = new Float32Array(600); this.cpu = new Float32Array(600); this.n = 0;
    this.split = { ticks: 0, render: 0, draw: 0, host: 0 }; this.tickCount = 0;
    this.long = { count: 0, worst: 0 };
    try { new PerformanceObserver((l) => { for (const e of l.getEntries()) { this.long.count++; this.long.worst = Math.max(this.long.worst, e.duration); } }).observe({ entryTypes: ["longtask"] }); } catch {}
    this.el = null; this.spark = null; this.shown = false;
    this.lat = []; // input event -> frame submitted with it, ms
  }
  frame(dt, parts, ticks) {
    const i = this.n++ % 600;
    this.dt[i] = dt; const cpu = parts.ticks + parts.render + parts.draw + parts.host; this.cpu[i] = cpu;
    for (const k in parts) this.split[k] += parts[k];
    this.tickCount += ticks;
  }
  report(engine, gpuMs) {
    const m = Math.min(this.n, 600); if (!m) return null;
    const dts = Array.from(this.dt.slice(0, m)).sort((a, b) => a - b), cpus = Array.from(this.cpu.slice(0, m)).sort((a, b) => a - b);
    const q = (a, p) => a[Math.min(a.length - 1, Math.floor(p * a.length))];
    const refresh = q(dts, 0.5), budget = refresh;
    const late = Array.from(this.dt.slice(0, m)).filter((d) => d > budget * 1.5).length;
    const prof = engine.profile(), names = JSON.parse(engine.phase_names());
    const phases = {}; names.forEach((nm, k) => (phases[nm] = { mean: +prof[2 + k * 2].toFixed(3), worst: +prof[3 + k * 2].toFixed(3) }));
    const frames = Math.max(1, m);
    const out = {
      hz: +(1000 / refresh).toFixed(0), p50: +q(dts, 0.5).toFixed(2), p95: +q(dts, 0.95).toFixed(2), p99: +q(dts, 0.99).toFixed(2), worst: +dts[dts.length - 1].toFixed(2),
      late, lateRate: +(late / frames).toFixed(4), cpuP50: +q(cpus, 0.5).toFixed(2), cpuP99: +q(cpus, 0.99).toFixed(2), cpuWorst: +cpus[cpus.length - 1].toFixed(2),
      split: Object.fromEntries(Object.entries(this.split).map(([k, v]) => [k, +(v / frames).toFixed(3)])), ticksPerFrame: +(this.tickCount / frames).toFixed(2),
      gpuMs: +gpuMs.toFixed(3), heapMB: performance.memory ? +(performance.memory.usedJSHeapSize / 1048576).toFixed(1) : null,
      longTasks: this.long.count, longWorst: +this.long.worst.toFixed(1), records: nb.records, draws: nb.draws, phases,
      inputToDraw: this.lat.length ? +(this.lat.reduce((a, b) => a + b, 0) / this.lat.length).toFixed(2) : null,
      inputToDrawWorst: this.lat.length ? +Math.max(...this.lat).toFixed(2) : null,
    };
    this.lat = [];
    this.n = 0; this.tickCount = 0; for (const k in this.split) this.split[k] = 0;
    return out;
  }
  show(on) {
    this.shown = on;
    if (on && !this.el) {
      this.el = document.createElement("pre");
      this.el.style.cssText = "position:fixed;left:8px;top:8px;margin:0;padding:8px 10px;font:11px/1.35 ui-monospace,monospace;background:#fffffff2;color:#111;border:1px solid #ddd;pointer-events:none;z-index:9";
      this.spark = document.createElement("canvas"); this.spark.width = 240; this.spark.height = 48;
      this.spark.style.cssText = "display:block;margin-top:6px";
      document.body.appendChild(this.el);
    }
    if (this.el) this.el.style.display = on ? "block" : "none";
  }
  draw(r) {
    if (!this.shown || !r) return;
    const top = Object.entries(r.phases).filter(([, v]) => v.mean > 0.005).sort((a, b) => b[1].mean - a[1].mean).slice(0, 8);
    this.el.textContent =
      `${r.hz} Hz  frame p50 ${r.p50}  p95 ${r.p95}  p99 ${r.p99}  worst ${r.worst} ms\n` +
      `late frames ${r.late} (${(r.lateRate * 100).toFixed(1)}%)   long tasks ${r.longTasks} (worst ${r.longWorst} ms)\n` +
      `cpu p50 ${r.cpuP50}  p99 ${r.cpuP99}  worst ${r.cpuWorst} ms   gpu ${r.gpuMs} ms\n` +
      `per frame: ticks ${r.split.ticks} (${r.ticksPerFrame}/frame)  render ${r.split.render}  draw ${r.split.draw}  host ${r.split.host} ms\n` +
      `records ${r.records}  draws ${r.draws}  heap ${r.heapMB ?? "?"} MB  res ${r.res ?? 1}\n` +
      `input -> drawn ${r.inputToDraw ?? "-"} ms (worst ${r.inputToDrawWorst ?? "-"}), + ~1 refresh to the screen\n` +
      top.map(([k, v]) => `  ${k.padEnd(10)} ${v.mean.toFixed(3)} (worst ${v.worst.toFixed(2)})`).join("\n");
    this.el.appendChild(this.spark);
    const g = this.spark.getContext("2d"), w = 240, h = 48; g.clearRect(0, 0, w, h);
    g.strokeStyle = "#c33"; g.beginPath(); const y8 = h - (8.33 / 25) * h; g.moveTo(0, y8); g.lineTo(w, y8); g.stroke();
    g.strokeStyle = "#111"; g.beginPath();
    const m = Math.min(this.lastN || 0, 240);
    for (let k = 0; k < m; k++) { const v = this.hist[(this.histI - m + k + 240) % 240]; const y = h - Math.min(1, v / 25) * h; k ? g.lineTo(k, y) : g.moveTo(k, y); }
    g.stroke();
  }
  push(dt) { this.hist ||= new Float32Array(240); this.histI = ((this.histI || 0) + 1) % 240; this.hist[this.histI] = dt; this.lastN = Math.min(240, (this.lastN || 0) + 1); }
}

// ------------------------------------------------------------- lag lab ---
// ?lag: the match runs through the real rollback netcode, with the bot as
// the remote player behind a simulated link. 1-7 pick the link, 8 cycles
// your input delay, B starts a blind round (guess the ping with 1-7),
// N hides the readout. ?lag=110 starts at that round trip.
const LINKS = [
  { name: "off", rtt: 0 }, // no netcode at all: the plain local game
  { name: "LAN", rtt: 4, jitter: 1, loss: 0 },
  { name: "same city", rtt: 30, jitter: 3, loss: 0 },
  { name: "good", rtt: 70, jitter: 6, loss: 0.005 },
  { name: "typical", rtt: 110, jitter: 10, loss: 0.01 },
  { name: "far", rtt: 160, jitter: 15, loss: 0.02 },
  { name: "bad", rtt: 220, jitter: 30, loss: 0.04 },
];
const DELAYS = ["auto", 0, 1, 2, 3, 4];
const BLIND_KEY = "neobrawl.blind";
class LagLab {
  constructor(engine, start) {
    this.engine = engine;
    const want = +start;
    this.link = start && !isNaN(want) ? LINKS.reduce((b, l, i) => (Math.abs(l.rtt - want) < Math.abs(LINKS[b].rtt - want) ? i : b), 0) : 4;
    this.delay = 0; // index into DELAYS
    this.blind = null; // { round } while guessing
    this.shown = true;
    this.hist = []; // per-second [rollbacks, deepest]
    try { this.rounds = JSON.parse(localStorage.getItem(BLIND_KEY) || "[]"); } catch { this.rounds = []; }
    this.el = document.createElement("div"); this.el.id = "lag";
    this.el.innerHTML = `<div class="l1"></div><div class="l2"></div><canvas width="180" height="22"></canvas><div class="l3"></div>`;
    document.body.appendChild(this.el);
    $("help").hidden = true; // the readout lists the keys that matter here
    this.banner = document.createElement("div"); this.banner.id = "lagbanner"; document.body.appendChild(this.banner);
    this.apply();
    addEventListener("keydown", (e) => this.key(e));
    this.last = null;
  }
  // input delay in ticks: auto hides about half the one-way trip (GGPO's
  // rule of thumb), so corrections stay around 2-3 frames
  delayTicks(l) {
    const d = DELAYS[this.delay];
    return d === "auto" ? Math.min(3, Math.round(l.rtt / 2 / 20 / 2)) : d;
  }
  apply() {
    const l = LINKS[this.link];
    if (!l.rtt) this.engine.lag_off();
    else this.engine.set_lag(l.rtt / 2, l.jitter, l.loss, this.delayTicks(l));
    this.draw();
  }
  key(e) {
    if (e.repeat || e.metaKey || e.ctrlKey) return;
    const n = +e.key;
    if (e.code === "KeyB") return this.startBlind();
    if (e.code === "KeyN") { this.shown = !this.shown; return this.draw(); }
    if (e.key === "8") { this.delay = (this.delay + 1) % DELAYS.length; return this.apply(); }
    if (n >= 1 && n <= LINKS.length) {
      if (this.blind) return this.guess(n - 1);
      this.link = n - 1; this.apply();
    }
  }
  startBlind() {
    // a different link from the one you're on, so a round is never free
    let k; do k = Math.floor(Math.random() * LINKS.length); while (k === this.link && LINKS.length > 1);
    this.link = k; this.blind = { round: this.rounds.length + 1, t: performance.now() };
    this.apply();
    this.say(`blind round ${this.blind.round} — play a bit, then guess the link: ${LINKS.map((l, i) => `${i + 1} ${l.rtt ? l.rtt + " ms" : "off"}`).join(" · ")}`, 0);
  }
  guess(k) {
    const actual = this.link, secs = Math.round((performance.now() - this.blind.t) / 1000);
    this.rounds.push({ guess: k, actual, secs, delay: DELAYS[this.delay], at: Date.now() });
    try { localStorage.setItem(BLIND_KEY, JSON.stringify(this.rounds)); } catch {}
    this.blind = null;
    const off = k - actual, word = off === 0 ? "exactly right" : `${Math.abs(off)} step${Math.abs(off) > 1 ? "s" : ""} ${off > 0 ? "high" : "low"}`;
    const name = (i) => (LINKS[i].rtt ? `${LINKS[i].rtt} ms` : "off");
    this.say(`you said ${name(k)} · it was ${LINKS[actual].rtt ? `${name(actual)} (${LINKS[actual].name})` : "off (no netcode)"} — ${word}`, 6000);
    this.draw();
  }
  say(text, ms) {
    this.banner.textContent = text; this.banner.classList.add("on");
    clearTimeout(this.sayT);
    if (ms) this.sayT = setTimeout(() => this.banner.classList.remove("on"), ms);
  }
  tally() {
    const r = this.rounds; if (!r.length) return "";
    const exact = r.filter((x) => x.guess === x.actual).length, near = r.filter((x) => Math.abs(x.guess - x.actual) <= 1).length;
    return `blind: ${r.length} round${r.length > 1 ? "s" : ""} · ${exact} exact · ${near} within one step`;
  }
  update(s) {
    this.last = s;
    if (s[0] === 2) { this.hist.push([s[5], s[7]]); if (this.hist.length > 180) this.hist.shift(); }
    this.draw();
  }
  draw() {
    const l = LINKS[this.link], s = this.last, hide = !!this.blind;
    const [l1, l2, cv, l3] = this.el.children;
    this.el.hidden = !this.shown;
    const d = DELAYS[this.delay];
    l1.textContent = hide ? "net · blind round — link hidden" :
      l.rtt ? `net · ${l.name} · ${l.rtt} ms rtt ±${l.jitter} · ${(l.loss * 100).toFixed(1)}% loss · delay ${this.delayTicks(l)}${d === "auto" ? " (auto)" : ""}` : "net · off (local game)";
    let t = "";
    if (!hide && s && l.rtt) {
      if (s[0] === 1) t = "connecting…";
      else if (s[0] === 0) t = "waits for a match";
      else if (s.length > 4) t = `ping ${s[4] < 0 ? "-" : Math.round(s[4])} · rollbacks ${s[5].toFixed(0)}/s · depth ${s[6].toFixed(1)} (max ${s[7]}) · resim ${s[8].toFixed(0)}/s · stalls ${s[9].toFixed(1)}/s · net cost ${s[11].toFixed(2)} ms/tick (worst ${s[12].toFixed(1)})`;
    }
    l2.textContent = t;
    // rollback depth, one bar a second (the last three minutes)
    const g = cv.getContext("2d"); g.clearRect(0, 0, 180, 22);
    cv.style.display = hide || !l.rtt ? "none" : "block";
    g.fillStyle = "#111";
    this.hist.forEach(([, deep], i) => { const h = Math.min(22, deep * 2.2); g.fillRect(i, 22 - h, 1, h); });
    l3.textContent = (hide ? "" : "1-7 link · 8 delay · B blind · N hide   ") + this.tally();
  }
}

// -------------------------------------------------------------- online ---
// Play a friend: "play a friend" makes a room and an invite link; opening
// the link joins it. PeerJS's free broker introduces the two browsers (and
// relays through its TURN servers when a direct path fails); after that the
// game talks peer to peer. Two channels: PeerJS's own (reliable) for the
// handshake and rematches, and a raw unordered, no-retransmit channel for
// rollback packets, the way UDP-style netcode wants them.
const PEERJS = "https://unpkg.com/peerjs@1.5.5/dist/peerjs.min.js";
const ROOM_PREFIX = "neobrawl-v1-";
class Online {
  constructor(engine) {
    this.engine = engine; this.peer = null; this.conn = null; this.chan = null;
    this.role = null; this.state = "idle"; this.last = null; this.pings = [];
    this.el = document.createElement("div"); this.el.id = "online"; document.body.appendChild(this.el);
    this.entry = document.createElement("button"); this.entry.id = "friend"; this.entry.textContent = "play a friend";
    this.el.addEventListener("pointerdown", (e) => e.stopPropagation());
    this.el.addEventListener("click", (e) => { e.stopPropagation(); const a = e.target.closest("[data-act]")?.dataset.act; if (a) this.act(a); });
    this.entry.onclick = (e) => { e.stopPropagation(); this.host(); };
    this.entry.onpointerdown = (e) => e.stopPropagation();
    $("botbtn").after(this.entry);
    this.hud = document.createElement("div"); this.hud.id = "nethud"; document.body.appendChild(this.hud);
    addEventListener("keydown", (e) => this.key(e));
    const m = location.hash.match(/^#vs-([a-z0-9]{4,12})$/);
    if (m) this.join(m[1]);
  }
  get active() { return this.state !== "idle"; }
  async lib() {
    if (window.Peer) return window.Peer;
    await new Promise((res, rej) => { const s = document.createElement("script"); s.src = PEERJS; s.onload = res; s.onerror = () => rej(new Error("couldn't load the connection library")); document.head.appendChild(s); });
    return window.Peer;
  }
  show(html, sticky = true) { this.el.innerHTML = html; this.el.classList.toggle("on", !!html); this.el.classList.toggle("sticky", sticky); }
  async host() {
    if (this.active) return;
    this.role = "host"; this.state = "waiting"; $("menuextra").hidden = true;
    const code = Array.from(crypto.getRandomValues(new Uint8Array(6)), (b) => "abcdefghjkmnpqrstuvwxyz23456789"[b % 31]).join("");
    const link = `${location.origin}${location.pathname}#vs-${code}`;
    this.show(`<p class="big">play a friend</p><p class="dim">opening a room…</p><div class="acts"><button data-act="leave">cancel</button></div>`);
    try {
      const Peer = await this.lib();
      this.peer = new Peer(ROOM_PREFIX + code);
      this.peer.on("error", (e) => this.fail(e));
      this.peer.on("open", () => {
        this.link = link;
        this.show(`<p class="big">play a friend</p><p>send them this link</p><p class="link" data-act="copy">${link}</p><p class="dim">the match starts when they open it · <span id="wait">waiting</span></p><div class="acts"><button class="main" data-act="copy" id="copy">copy link</button><button data-act="leave">cancel</button></div>`);
        let d = 0; const t = setInterval(() => { const w = $("wait"); if (!w || this.state !== "waiting") return clearInterval(t); w.textContent = "waiting" + ".".repeat(d++ % 4); }, 400);
      });
      this.peer.on("connection", (c) => { if (this.conn) { c.close(); return; } this.wire(c); });
    } catch (e) { this.fail(e); }
  }
  async join(code) {
    this.role = "guest"; this.state = "joining";
    this.show(`<p class="big">play a friend</p><p class="dim">joining…</p><div class="acts"><button data-act="leave">cancel</button></div>`);
    try {
      const Peer = await this.lib();
      this.peer = new Peer();
      this.peer.on("error", (e) => this.fail(e));
      this.peer.on("open", () => this.wire(this.peer.connect(ROOM_PREFIX + code, { reliable: true })));
    } catch (e) { this.fail(e); }
  }
  wire(c) {
    this.conn = c;
    c.on("open", () => {
      // the game channel: pre-agreed on both ends, so no renegotiation
      const ch = c.peerConnection.createDataChannel("ggrs", { negotiated: true, id: 7, ordered: false, maxRetransmits: 0 });
      ch.binaryType = "arraybuffer";
      ch.onmessage = (ev) => this.engine.net_recv(new Uint8Array(ev.data));
      this.chan = ch;
      this.state = "connected";
      this.show(`<p class="big">connected</p><p class="dim">measuring the line…</p>`);
      if (this.role === "host") this.measure();
    });
    c.on("data", (m) => this.control(m));
    c.on("close", () => this.left());
    c.on("error", (e) => this.fail(e));
  }
  // a few round trips over the control channel pick the input delay with
  // the lab's rule (hide about half the one-way trip), then both start
  async measure() {
    this.pings = [];
    for (let i = 0; i < 5; i++) { this.conn.send({ t: "ping", at: performance.now() }); await new Promise((r) => setTimeout(r, 120)); }
    await new Promise((r) => setTimeout(r, 300));
    const rtt = this.pings.length ? this.pings.sort((a, b) => a - b)[Math.floor(this.pings.length / 2)] : 100;
    const delay = Math.max(1, Math.min(3, Math.round(rtt / 2 / 20 / 2)));
    const start = { t: "start", seed: crypto.getRandomValues(new Uint32Array(1))[0], map: Math.floor(Math.random() * 3), delay, rtt: Math.round(rtt) };
    this.conn.send(start); this.begin(start);
  }
  control(m) {
    if (!m || typeof m !== "object") return;
    if (m.t === "ping") this.conn.send({ t: "pong", at: m.at });
    else if (m.t === "pong") this.pings.push(performance.now() - m.at);
    else if (m.t === "start" && this.role === "guest") this.begin(m);
    else if (m.t === "rematch") { if (this.role === "host") this.measure(); else this.show(`<p class="big">rematch</p>`); }
  }
  begin(s) {
    this.engine.net_begin(BigInt(s.seed), s.map, this.role === "host" ? 0 : 1, s.delay);
    this.state = "playing"; this.started = performance.now();
    if (location.hash) history.replaceState(null, "", location.pathname + location.search);
    this.show(`<p>${this.role === "host" ? "you're blue" : "you're red"} · first to strike</p>`, false);
    setTimeout(() => { if (this.state === "playing") this.show(""); }, 2500);
  }
  // every rendered frame: hand GGRS's packets to the channel, read events
  pump() {
    if (this.state !== "playing") return;
    for (let p = this.engine.net_pop(); p.length; p = this.engine.net_pop()) if (this.chan?.readyState === "open") this.chan.send(p);
    const ev = this.engine.net_events();
    if (!ev) return;
    for (const e of ev.split(",")) {
      if (e === "interrupted") this.show(`<p>connection hiccup…</p>`, false);
      else if (e === "resumed") this.show("");
      else if (e === "disconnected") this.left();
      else if (e === "desync") { nb.errors.push("desync"); this.show(`<p class="big">out of sync</p><p class="dim">that's a bug, sorry</p><div class="acts"><button class="main" data-act="leave">back to the menu</button></div>`); }
      else if (e === "ended") { this.state = "over"; this.show(`<p class="big">match over</p><div class="acts"><button class="main" data-act="rematch">rematch <kbd>R</kbd></button><button data-act="leave">leave <kbd>Esc</kbd></button></div>`); }
    }
  }
  key(e) {
    if (!this.active || e.repeat) return;
    if (e.code === "Escape") { e.stopImmediatePropagation(); this.act("leave"); }
    if (e.code === "KeyR") this.act("rematch");
  }
  act(a) {
    if (a === "leave") this.leave();
    else if (a === "copy" && this.link) { navigator.clipboard?.writeText(this.link); const c = $("copy"); if (c) c.textContent = "copied"; }
    else if (a === "rematch" && this.state === "over") {
      this.state = "rematching"; this.conn?.send({ t: "rematch" }); this.show(`<p class="big">rematch</p>`);
      if (this.role === "host") this.measure();
    }
  }
  stats(s) {
    if (this.state !== "playing" || !s || !s[1]) { this.hud.textContent = ""; return; }
    this.hud.textContent = `${Math.round(s[2])} ms`;
    this.hud.title = `rollbacks ${s[3].toFixed(0)}/s · depth ${s[4].toFixed(1)} (max ${s[5]}) · delay ${s[11]}`;
  }
  left() {
    if (this.state === "idle") return;
    const was = this.state; this.state = "gone"; this.engine.net_end();
    this.show(`<p class="big">${was === "waiting" || was === "joining" ? "couldn't connect" : "your friend left"}</p><p class="dim">back to the menu in a moment</p>`);
    setTimeout(() => this.home(), 2500);
  }
  leave() { this.conn?.close(); this.engine.net_end(); this.home(); }
  home() { this.peer?.destroy(); nb.leaving = true; location.href = location.pathname + location.search; }
  fail(e) {
    nb.errors.push("online: " + (e?.type || e?.message || e));
    const msg = e?.type === "peer-unavailable" ? "that room is gone (or the link was mistyped)" : e?.type === "unavailable-id" ? "room clash, try again" : "couldn't connect";
    this.show(`<p class="big">${msg}</p><div class="acts"><button class="main" data-act="leave">back to the menu</button></div>`);
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
    // GPU timing for the perf overlay, where the browser exposes it
    const ts = adapter.features.has("timestamp-query");
    const device = await adapter.requestDevice(ts ? { requiredFeatures: ["timestamp-query"] } : undefined);
    device.__ts = ts;
    device.addEventListener?.("uncapturederror", (e) => nb.errors.push(`gpu: ${e.error.message}`));
    return new Renderer(device);
  }
  constructor(device) {
    this.d = device;
    this.gpuMs = 0;
    if (device.__ts) {
      this.qs = device.createQuerySet({ type: "timestamp", count: 2 });
      this.qResolve = device.createBuffer({ size: 16, usage: GPUBufferUsage.QUERY_RESOLVE | GPUBufferUsage.COPY_SRC });
      this.qRead = device.createBuffer({ size: 16, usage: GPUBufferUsage.MAP_READ | GPUBufferUsage.COPY_DST });
      this.qBusy = false;
    }
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
      // 4x MSAA: the traced vector shapes get smooth edges like the bitmaps had
      multisample: { count: 4 },
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
  // adaptive resolution, once a second: if the GPU can't hold the frame,
  // render a step smaller (never below 1x CSS pixels); step back up only
  // with real headroom (a dead band between, so it never oscillates)
  adapt(budgetMs) {
    this.res ??= 1;
    if (this.gpuMs > budgetMs * 0.8 && this.res > 0.6) this.res = +(this.res - 0.1).toFixed(2);
    else if (this.gpuMs > 0 && this.gpuMs < budgetMs * 0.35 && this.res < 1) this.res = +(this.res + 0.1).toFixed(2);
  }
  draw(rec, texts, family, screenW, screenH) {
    this.res ??= 1;
    const d = this.d, dpr = Math.max(1, Math.min(devicePixelRatio || 1, 2) * this.res);
    const W = Math.max(1, Math.round(canvas.clientWidth * dpr)), H = Math.max(1, Math.round(canvas.clientHeight * dpr));
    if (canvas.width !== W || canvas.height !== H || !this.depth) {
      canvas.width = W; canvas.height = H;
      this.depth?.destroy(); this.msaa?.destroy();
      this.depth = d.createTexture({ size: [W, H], format: "depth24plus", sampleCount: 4, usage: GPUTextureUsage.RENDER_ATTACHMENT });
      this.msaa = d.createTexture({ size: [W, H], format: this.format, sampleCount: 4, usage: GPUTextureUsage.RENDER_ATTACHMENT });
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
      if (kind === 2) {
        // hot path (thousands per frame with vector shapes): straight into the buffer
        const t = rec[o + 1] < 0 ? this.white : this.textures[rec[o + 1]];
        const c = cmd("quad", t);
        need(6);
        const u0 = rec[o + 16], v0 = rec[o + 17], u1 = rec[o + 18], v1 = rec[o + 19];
        for (let j = 0; j < 6; j++) {
          const k = QIDX[j], q = o + 4 + k * 3, w = vc * 9;
          v[w] = rec[q]; v[w + 1] = rec[q + 1]; v[w + 2] = rec[q + 2];
          v[w + 3] = k === 0 || k === 3 ? u0 : u1; v[w + 4] = k < 2 ? v0 : v1;
          v[w + 5] = rec[o + 20]; v[w + 6] = rec[o + 21]; v[w + 7] = rec[o + 22]; v[w + 8] = rec[o + 23];
          vc++;
        }
        c.count += 6;
        continue;
      }
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
        // motion: letters write themselves in (reveal 0..1), then hum; a rare
        // glitch jolts a few letters sideways for a moment
        const reveal = rec[o + 18], hid = rec[o + 19], T = performance.now() / 1000;
        const N = lines.reduce((a, l) => a + l.length, 0) || 1;
        const glitch = hash32((Math.floor(T * 12) * 2654435761) ^ (hid * 40503)) % 160 === 0 ? hash32(hid + Math.floor(T * 12)) : 0;
        let gi = 0;
        lines.forEach((line, li) => {
          const width = at.measure(line) * k + spacing * Math.max(0, line.length - 1);
          let x = (rw - width) * align[0];
          const top = rh - (rh - total) * (1 - align[1]) - li * lh; // baseline-ish placement from the rect bottom
          const y0 = top - lh;
          for (let ci = 0; ci < line.length; ci++, gi++) {
            const lp = reveal >= 1 ? 1 : Math.min(1, Math.max(0, (reveal * (N + 6) - gi) / 6));
            if (lp <= 0) continue;
            let code = line.charCodeAt(ci);
            // still forming: the letter flickers through others before it settles
            if (lp < 0.85 && code !== 32) { const h = hash32(gi * 7919 + Math.floor(T * 40)); code = line.charCodeAt(h % line.length) || code; if (code === 32) code = line.charCodeAt(ci); }
            const g = at.glyphs.get(code) || at.glyphs.get(63);
            const gh = at.lineH * k, gw = g.pw * k;
            let px0 = x + (at.measure(line.slice(0, ci)) - 3) * k + spacing * ci;
            let hum = Math.sin(T * 2.3 + gi * 0.9 + hid) * 0.012 * gh - (1 - lp) * (1 - lp) * 0.45 * gh;
            if (glitch && (gi + glitch) % 3 === 0) { px0 += ((glitch >> 4) % 2 ? 1 : -1) * 0.07 * gh; hum += 0.03 * gh; }
            const lc = lp < 1 ? [color[0], color[1], color[2], color[3] * lp] : color;
            const pt = (sx, sy) => [P[0][0] + ux[0] * sx + uy[0] * sy, P[0][1] + ux[1] * sx + uy[1] * sy, P[0][2] + ux[2] * sx + uy[2] * sy];
            const yb = y0 + (lh - gh) + hum;
            quadV([pt(px0, yb), pt(px0 + gw, yb), pt(px0 + gw, yb + gh), pt(px0, yb + gh)], [g.u, 1 - g.v - g.h, g.u + g.w, 1 - g.v], lc);
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
    const target = this.ctx.getCurrentTexture().createView(), view = this.msaa.createView(), dview = this.depth.createView();
    let calls = 0;
    passes.forEach((p, pi) => {
      const first = pi === 0, last = pi === passes.length - 1;
      const timing = this.qs && !this.qBusy && (first || last) ? { querySet: this.qs, ...(first ? { beginningOfPassWriteIndex: 0 } : {}), ...(last ? { endOfPassWriteIndex: 1 } : {}) } : undefined;
      const pass = enc.beginRenderPass({
        timestampWrites: timing,
        colorAttachments: [{ view, resolveTarget: last ? target : undefined, loadOp: p.clear || first ? "clear" : "load", storeOp: "store",
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
    const timed = this.qs && !this.qBusy && passes.length;
    if (timed) { enc.resolveQuerySet(this.qs, 0, 2, this.qResolve, 0); enc.copyBufferToBuffer(this.qResolve, 0, this.qRead, 0, 16); this.qBusy = true; }
    d.queue.submit([enc.finish()]);
    if (timed) {
      this.qRead.mapAsync(GPUMapMode.READ).then(() => {
        const t = new BigUint64Array(this.qRead.getMappedRange());
        const ms = Number(t[1] - t[0]) / 1e6;
        if (ms > 0 && ms < 1000) this.gpuMs = ms;
        this.qRead.unmap(); this.qBusy = false;
      }).catch(() => { this.qBusy = false; });
    }
    nb.draws = calls;
  }
}

// ---------------------------------------------------------------- input ---
// Desktop: P1 = WASD + mouse (aim at the cursor, hold left button to charge,
// release to throw). P2 = gamepad, or arrows + IJKL. With two gamepads they
// take over both players (left stick move, right stick aim, release to throw).
const keys = new Set();
let mouseHeld = false;
// Aim rides with you: in play the pointer is locked (raw, without the OS's
// acceleration where the browser allows it) and the finger steers a
// reticle held at an offset from your soul, in world units, so walking
// never makes you chase it - a stick, on a trackpad. ?abs aims at the
// absolute cursor point instead (League-style).
let locked = false;
// how you aim with the pointer, remembered: "mouse" rides a locked reticle;
// "trackpad" aims at the visible cursor, and the moment you charge the
// reticle starts where you pointed, the cursor hides, and from there the
// finger turns it - riding with your soul, like the mouse mode
// (one thing on screen to aim with, never a cursor and a target apart)
const AIM_KEY = "neobrawl.aim";
let aimMode = (() => { try { return localStorage.getItem(AIM_KEY); } catch { return null; } })() || (/Mac/.test(navigator.platform) ? "trackpad" : "mouse");
// the anthill always aims at the map under the cursor: the frame follows you
const rideOn = () => aimMode === "mouse" && !params.has("abs") && nb.mode !== "anthill";
let cursorPx = null; // last pointer position, screen px (origin bottom-left)
let tpAim = null; // trackpad: the riding reticle while a charge is held
const reticle = { x: 2.5, y: 0 }; // world units from your soul
const RIDE = {
  gain: 0.0105, // world units per raw count, slow finger
  boost: 0.55, // extra gain per count/ms of finger speed (a flick turns far)
  speedCap: 2.5,
  rMin: 1.2, // never so close that direction gets twitchy
  rMax: 6.0, // never lost off-screen: only direction matters past this
};
let lastMove = 0;
// the reticle steered by one raw movement; `friction` < 1 over the enemy
function clampRide(r) {
  const l = Math.hypot(r.x, r.y) || 1;
  const want = Math.min(RIDE.rMax, Math.max(RIDE.rMin, l));
  r.x *= want / l; r.y *= want / l;
}
function steer(r, dx, dy, t, friction) {
  const dt = Math.max(1, t - lastMove); lastMove = t;
  const speed = Math.min(RIDE.speedCap, Math.hypot(dx, dy) / dt);
  const g = RIDE.gain * (0.7 + RIDE.boost * speed) * friction;
  r.x += dx * g; r.y -= dy * g;
  clampRide(r);
}
// dash presses are latched so a quick tap between two game ticks still counts
const dashLatch = [false, false];
const dashPlayer = (e) => {
  if (e.code === "ShiftLeft") return 0;
  if (e.code === "ShiftRight") return 1;
  if (e.key === "Shift") return e.location === 2 ? 1 : 0; // keyboards without a left/right code
  return -1;
};
const QIDX = [0, 1, 2, 0, 2, 3];
const hash32 = (x) => { x |= 0; x ^= x >>> 16; x = Math.imul(x, 0x7feb352d); x ^= x >>> 15; x = Math.imul(x, 0x846ca68b); return (x ^ (x >>> 16)) >>> 0; };
const desktop = true; // desktop edition: keyboard, mouse and controllers only
const axis = (up, left, down, right) => {
  const x = (keys.has(right) ? 1 : 0) - (keys.has(left) ? 1 : 0), y = (keys.has(up) ? 1 : 0) - (keys.has(down) ? 1 : 0);
  const l = Math.hypot(x, y) || 1;
  return [x / l, y / l];
};
const dz = (v) => (Math.abs(v) < 0.18 ? 0 : v);
function padSticks(pad) {
  let mx = dz(pad.axes[0]), my = -dz(pad.axes[1]), ax = dz(pad.axes[2]), ay = -dz(pad.axes[3]);
  const m = Math.hypot(mx, my); if (m > 1) { mx /= m; my /= m; }
  // dpad also moves
  if (pad.buttons[12]?.pressed) my = 1; if (pad.buttons[13]?.pressed) my = -1;
  if (pad.buttons[14]?.pressed) mx = -1; if (pad.buttons[15]?.pressed) mx = 1;
  const dash = !!(pad.buttons[0]?.pressed || pad.buttons[5]?.pressed || pad.buttons[4]?.pressed);
  return { mx, my, ax, ay, aiming: Math.hypot(ax, ay) > 0.35, dash };
}
function pollControls() {
  if (!desktop) return;
  const pads = (navigator.getGamepads?.() || []).filter(Boolean);
  // player 1: keyboard + mouse, unless two pads are connected
  if (pads.length >= 2) {
    const p = padSticks(pads[0]);
    engine.direct(0, p.mx, p.my, p.ax, p.ay, p.aiming, false, true);
    engine.dash(0, p.dash);
  } else {
    const [mx, my] = axis("KeyW", "KeyA", "KeyS", "KeyD");
    // charge by holding Space (or the button): the aiming hand only points
    const charging = mouseHeld || keys.has("Space");
    if (locked) engine.direct(0, mx, my, reticle.x, reticle.y, charging, false, true);
    // the anthill aims at the cursor, read by the engine every tick
    else if (nb.mode === "anthill") engine.direct(0, mx, my, 0, 0, charging, true, true);
    else if (aimMode === "trackpad" && charging && (tpAim || cursorPx)) {
      if (!tpAim) {
        // the charge begins: the reticle starts exactly where you pointed
        const w = engine.world_at(cursorPx[0], cursorPx[1]), me = engine.my_soul();
        if (w.length && me.length) { tpAim = { x: w[0] - me[0], y: w[1] - me[1] }; clampRide(tpAim); engine.cursor(-1, -1); }
      }
      if (tpAim) engine.direct(0, mx, my, tpAim.x, tpAim.y, true, false, true);
      else engine.direct(0, mx, my, 0, 0, true, true, true);
    } else {
      if (tpAim) { tpAim = null; if (cursorPx) engine.cursor(cursorPx[0], cursorPx[1]); } // let go: the cursor is back
      engine.direct(0, mx, my, 0, 0, charging, true, true);
    }
    engine.dash(0, keys.has("ShiftLeft") || keys.has("Shift0") || dashLatch[0]);
  }
  // player 2: last pad, else arrows + IJKL (hold IJKL to charge in that direction)
  const pad2 = pads.length >= 2 ? pads[1] : pads[0];
  const [kx, ky] = axis("ArrowUp", "ArrowLeft", "ArrowDown", "ArrowRight");
  const [ax, ay] = axis("KeyI", "KeyJ", "KeyK", "KeyL");
  if (pad2) {
    const p = padSticks(pad2);
    engine.direct(1, p.mx || kx, p.my || ky, p.aiming ? p.ax : ax, p.aiming ? p.ay : ay, p.aiming || !!(ax || ay), false, true);
    engine.dash(1, p.dash || keys.has("ShiftRight") || keys.has("Shift1") || dashLatch[1]);
  } else {
    engine.direct(1, kx, ky, ax, ay, !!(ax || ay), false, true);
    engine.dash(1, keys.has("ShiftRight") || keys.has("Shift1") || dashLatch[1]);
  }
  engine.any_key(keys.size > 0 || mouseHeld);
  // player 2 is a wanderer until a human touches its controls; it takes
  // back over after 5 s without any P2 input (?noai keeps it human)
  const p2Keys = ["ArrowUp", "ArrowLeft", "ArrowDown", "ArrowRight", "KeyI", "KeyJ", "KeyK", "KeyL", "ShiftRight", "Shift1"];
  const p2Pad = pad2 && (() => { const p = padSticks(pad2); return Math.hypot(p.mx, p.my) > 0 || p.aiming || p.dash; })();
  if (p2Keys.some((c) => keys.has(c)) || dashLatch[1] || p2Pad) p2Human = performance.now();
  const ai = params.has("lag") || (!params.has("noai") && !params.has("practice") && !nb.practicing && performance.now() - p2Human > 5000);
  if (ai !== p2Ai) { p2Ai = ai; engine.set_ai(1, ai); }
}
let p2Human = -1e9, p2Ai = false;
// the timestamp of the latest control input not yet drawn (latency observable)
let pendingInput = null;
addEventListener("keydown", (e) => { if (!e.repeat && pendingInput === null) pendingInput = e.timeStamp; });
addEventListener("pointerdown", (e) => { if (pendingInput === null) pendingInput = e.timeStamp; });

async function main() {
  const [wasm, sceneText, assetsText] = await Promise.all([init(`./pkg/neobrawl_bg.wasm${V}`), fetch(`assets/scene.json${V}`).then((r) => r.text()), fetch(`assets/assets.json${V}`).then((r) => r.text())]);
  const assets = JSON.parse(assetsText);
  const cssW = () => canvas.clientWidth || innerWidth, cssH = () => canvas.clientHeight || innerHeight;
  engine = new Engine(sceneText, assetsText, loadPrefs(), BigInt(params.get("seed") || Math.floor(Math.random() * 2 ** 31)), cssW(), cssH());
  nb.engine = engine;
  const tables = JSON.parse(engine.tables());
  let family = "sans-serif";
  if (tables.font) {
    const f = Object.values(assets.fonts)[0];
    const face = new FontFace(f.name, `url(assets/${f.file})`);
    try { await face.load(); document.fonts.add(face); family = f.name; document.documentElement.style.setProperty("--font", family); } catch (e) { nb.errors.push(`font: ${e}`); }
  }
  const r = await Renderer.create();
  await r.load(assets, tables);
  let audio = null;
  if (!params.has("mute")) { audio = new Audio(Object.values(assets.audio).map((a) => [a.name, a.file])); audio.load(); }

  const toGame = (e) => { const b = canvas.getBoundingClientRect(); return [e.clientX - b.left, b.height - (e.clientY - b.top)]; };
  const ptr = (kind) => (e) => { audio?.unlock(); if (kind === 0) canvas.setPointerCapture?.(e.pointerId); const [x, y] = toGame(e); engine.pointer(e.pointerId, x, y, kind); e.preventDefault(); };
  const inPlay = () => { const s = JSON.parse(engine.state()).screens; return s.includes("game") || s.includes("tutorial"); };
  canvas.addEventListener("pointerdown", (e) => {
    if (e.button === 0) mouseHeld = true;
    if (locked) return; // clicks while locked are throws, not UI clicks
    ptr(0)(e);
    lockIfPlaying();
  });
  // the lock needs a gesture: the first click or key once a match is up
  const lockIfPlaying = () => {
    if (!rideOn() || locked || !inPlay() || !canvas.requestPointerLock) return;
    const p = canvas.requestPointerLock({ unadjustedMovement: true });
    p?.catch?.(() => canvas.requestPointerLock()?.catch?.(() => {})); // no raw input here: plain lock
  };
  nb.lockIfPlaying = lockIfPlaying;
  // raw updates arrive as soon as the hardware reports (not once a frame)
  const moveEvent = "onpointerrawupdate" in window ? "pointerrawupdate" : "pointermove";
  canvas.addEventListener(moveEvent, (e) => {
    if (locked) steer(reticle, e.movementX, e.movementY, e.timeStamp, engine.aim_friction(reticle.x, reticle.y));
    else if (tpAim) steer(tpAim, e.movementX, e.movementY, e.timeStamp, engine.aim_friction(tpAim.x, tpAim.y));
  });
  canvas.addEventListener("pointermove", (e) => {
    if (locked) return;
    const [x, y] = toGame(e); cursorPx = [x, y];
    if (!tpAim) engine.cursor(x, y); // hidden while a charge steers the reticle
    if (e.buttons || e.pointerType === "touch") ptr(1)(e);
  });
  document.addEventListener("pointerlockchange", () => {
    const now = document.pointerLockElement === canvas;
    // Esc releases the lock (the browser eats that keydown): treat it as pause
    if (locked && !now && inPlay()) engine.toggle_pause();
    locked = now;
    canvas.style.cursor = "none";
  });
  const up = (e) => { if (e.button === 0) mouseHeld = false; if (!locked) ptr(2)(e); };
  canvas.addEventListener("pointerup", up);
  canvas.addEventListener("pointercancel", up);
  canvas.addEventListener("pointerleave", () => engine.cursor(-1, -1));
  canvas.addEventListener("contextmenu", (e) => e.preventDefault());
  engine.set_desktop(desktop);
  // ?snake: you never stop; WASD / the stick only turn you
  if (params.has("snake")) engine.set_snake(true);
  // the AI opponent fights (?ai=1 easy, 2 normal, 3 hard - the default);
  // ?wander makes it a peaceful traveller instead
  engine.set_ai_level(1, params.has("wander") ? 0 : Math.max(1, Math.min(3, +(params.get("ai") || 3) || 3)));
  // the engine draws the cursor (a spark); the system one would double it
  if (desktop) canvas.style.cursor = "none";
  addEventListener("keydown", (e) => {
    audio?.unlock();
    if (e.metaKey || e.ctrlKey) return;
    if (e.code === "Escape" && !e.repeat && !locked && !nb.online?.active) engine.toggle_pause();
    if (!e.repeat && e.code !== "Escape") lockIfPlaying();
    const dp = dashPlayer(e);
    if (dp >= 0 && !e.repeat) { dashLatch[dp] = true; if (!e.code) keys.add("Shift" + dp); }
    keys.add(e.code);
    if (e.code.startsWith("Arrow") || e.code === "Space") e.preventDefault();
  });
  // a trackpad played hard: swipes, pinches and scrolls must never move,
  // zoom or navigate the page (wheel covers two-finger swipes and pinch,
  // which arrives as ctrl+wheel; gesture* is Safari's pinch)
  addEventListener("wheel", (e) => e.preventDefault(), { passive: false });
  for (const g of ["gesturestart", "gesturechange", "gestureend"]) addEventListener(g, (e) => e.preventDefault(), { passive: false });
  // and if the browser still tries to leave mid-match, ask first
  addEventListener("beforeunload", (e) => {
    if (nb.online?.state === "playing" || (inPlay() && !nb.leaving)) { e.preventDefault(); e.returnValue = ""; }
  });
  addEventListener("keyup", (e) => { keys.delete(e.code); const dp = dashPlayer(e); if (dp >= 0) keys.delete("Shift" + dp); });
  addEventListener("blur", () => { keys.clear(); mouseHeld = false; });
  addEventListener("resize", () => engine.set_screen(cssW(), cssH()));

  $("overlay").hidden = true;
  const debug = params.has("debug"); $("debug").hidden = !debug;
  let texts = {}, textsAt = -1, textsVer = -1;
  let last = performance.now(), acc = 0, frames = 0, fpsT = last;
  const perf = new Perf(); nb.perfObj = perf; let perfT = performance.now();
  const lab = params.has("lag") ? new LagLab(engine, params.get("lag")) : null; nb.lab = lab;
  const online = lab ? null : new Online(engine); nb.online = online;
  perf.show(params.has("perf"));
  addEventListener("keydown", (e) => { if (e.code === "F3") { e.preventDefault(); perf.show(!perf.shown); } });
  const loop = (now) => {
    if (nb.capturing) { last = now; requestAnimationFrame(loop); return; } // the recorder drives the frames
    const dtFrame = now - last;
    acc += Math.min(0.25, dtFrame / 1000); last = now;
    let steps = 0;
    const tA = performance.now();
    // input is read every frame (the prediction draws from it right away),
    // and again before each tick
    pollControls();
    if (lab) engine.net_poll();
    if (online?.state === "playing") engine.net_poll();
    while (acc >= TICK && steps < 8) {
      if (steps) pollControls();
      engine.frame(); acc -= TICK; steps++; nb.ticks++;
      dashLatch[0] = dashLatch[1] = false; // a quick tap is only spent once a tick has seen it
    }
    online?.pump();
    const tB = performance.now();
    if (audio) {
      const s = engine.drain_sounds(); if (s !== "[]") for (const [c, v] of JSON.parse(s)) audio.shot(c, v);
      const q = engine.drain_cues(); if (q !== "[]") for (const [n, p, a] of JSON.parse(q)) audio.cue(n, p, a);
      audio.mix(engine.mix());
      audio.music(JSON.parse(engine.music()));
    }
    const p = engine.take_prefs(); if (p) savePrefs(p);
    if (nb.ticks !== textsAt) { const v = engine.texts_version(); if (v !== textsVer) { texts = JSON.parse(engine.texts()); textsVer = v; } textsAt = nb.ticks; }
    engine.set_subtick(acc / TICK);
    const t0 = performance.now();
    const len = engine.render();
    const t1 = performance.now();
    nb.records = len / 28;
    r.draw(new Float32Array(wasm.memory.buffer, engine.records_ptr(), len), texts, family, cssW(), cssH());
    const t2 = performance.now();
    nb.cpuMs = nb.cpuMs * 0.95 + (t2 - t0) * 0.05; // smoothed render + record time
    perf.frame(dtFrame, { ticks: tB - tA, render: t1 - t0, draw: t2 - t1, host: t0 - tB }, steps);
    if (pendingInput !== null) { perf.lat.push(t2 - pendingInput); pendingInput = null; }
    perf.push(dtFrame);
    if (now - perfT >= 1000) {
      nb.perf = perf.report(engine, r.gpuMs);
      if (nb.perf) { r.adapt(1000 / nb.perf.hz); nb.perf.res = r.res; }
      perf.draw(nb.perf); perfT = now;
      if (lab) { nb.lag = engine.lag_stats(); lab.update(nb.lag); }
      if (locked && !inPlay()) document.exitPointerLock?.(); // menus get the cursor back
      if (online) { nb.net = engine.net_stats(); online.stats(nb.net); }
    }
    frames++;
    if (now - fpsT >= 500) { nb.fps = Math.round((frames * 1000) / (now - fpsT)); frames = 0; fpsT = now; }
    if (debug) $("debug").textContent = `fps ${nb.fps} draws ${nb.draws}\n${engine.state()}`;
    requestAnimationFrame(loop);
  };
  requestAnimationFrame(loop);
  // the mode row and the controls legend: the title screen only
  setInterval(() => {
    const title = !online?.active && !nb.practicing && !lab && JSON.parse(engine.state()).screens.includes("start");
    $("menuextra").hidden = $("help").hidden = !title;
  }, 120);
  nb.ready = true;
  // ---- recording (recordings/tool): offline capture at an exact frame rate,
  // each frame posted to the recorder's server, and the same audio code
  // rendered offline from a per-frame log of what the live loop would play
  nb.capture = async (seconds, fps) => {
    nb.capturing = true; let a2 = 0; nb.audioLog = []; engine.drain_sounds(); engine.drain_cues(); // nothing from before the take
    for (let i = 0; i < seconds * fps; i++) {
      a2 += 1 / fps;
      while (a2 >= TICK) { engine.frame(); a2 -= TICK; nb.ticks++; }
      nb.audioLog.push([engine.drain_sounds(), engine.drain_cues(), engine.mix(), engine.music()]);
      if (nb.ticks !== textsAt) { const v = engine.texts_version(); if (v !== textsVer) { texts = JSON.parse(engine.texts()); textsVer = v; } textsAt = nb.ticks; }
      engine.set_subtick(a2 / TICK);
      const len = engine.render();
      r.draw(new Float32Array(wasm.memory.buffer, engine.records_ptr(), len), texts, family, cssW(), cssH());
      if (nb.capStop) break;
      // promise-based readback (toBlob is throttled in a hidden page)
      const bmp = await createImageBitmap(canvas);
      const oc = nb.oc || (nb.oc = new OffscreenCanvas(canvas.width, canvas.height)), ox = oc.getContext("2d");
      ox.drawImage(bmp, 0, 0); bmp.close();
      const blob = await oc.convertToBlob({ type: "image/jpeg", quality: 0.94 });
      await fetch("/frame?i=" + i, { method: "POST", body: blob });
      nb.capFrame = i;
    }
    nb.capDone = true; nb.capturing = false;
  };
  nb.renderAudio = async (fps) => {
    const log = nb.audioLog, dur = log.length / fps + 1.5, rate = 48000;
    const ctx = new OfflineAudioContext(2, Math.ceil(dur * rate), rate);
    const A = new Audio(Object.values(assets.audio).map((a) => [a.name, a.file]), ctx);
    A.load();
    for (let t = 0; t < 200 && A.buffers.size < A.files.length; t++) await new Promise((res) => setTimeout(res, 50));
    log.forEach(([snd, q, m, mu], i) => {
      ctx.suspend(i / fps).then(() => {
        if (snd !== "[]") for (const [c, v] of JSON.parse(snd)) A.shot(c, v);
        if (q !== "[]") for (const [n, p, a] of JSON.parse(q)) A.cue(n, p, a);
        A.mix(m); A.music(JSON.parse(mu));
        ctx.resume();
      });
    });
    const buf = await ctx.startRendering();
    // 16-bit stereo WAV
    const n = buf.length, out = new DataView(new ArrayBuffer(44 + n * 4)), L = buf.getChannelData(0), Rt = buf.getChannelData(1);
    const w = (o, str) => { for (let k = 0; k < str.length; k++) out.setUint8(o + k, str.charCodeAt(k)); };
    w(0, "RIFF"); out.setUint32(4, 36 + n * 4, true); w(8, "WAVE"); w(12, "fmt "); out.setUint32(16, 16, true); out.setUint16(20, 1, true); out.setUint16(22, 2, true);
    out.setUint32(24, rate, true); out.setUint32(28, rate * 4, true); out.setUint16(32, 4, true); out.setUint16(34, 16, true); w(36, "data"); out.setUint32(40, n * 4, true);
    let peak = 0;
    for (let k = 0; k < n; k++) { const a = Math.max(-1, Math.min(1, L[k])), b = Math.max(-1, Math.min(1, Rt[k])); peak = Math.max(peak, Math.abs(a), Math.abs(b)); out.setInt16(44 + k * 4, a * 32767, true); out.setInt16(46 + k * 4, b * 32767, true); }
    await fetch("/audio", { method: "POST", body: out.buffer });
    return { frames: log.length, buffers: A.buffers.size + "/" + A.files.length, peak: +peak.toFixed(3), errors: nb.errors.join(";") };
  };
  // target practice (menu entry, or ?practice): straight in, on Map 2
  const startPractice = async (mode = "practice") => {
    if (nb.practicing) return;
    nb.practicing = mode;
    $("menuextra").hidden = true;
    const wait = (ms) => new Promise((r) => setTimeout(r, ms));
    // straight from the title: the engine skips the menus and starts the
    // mode itself once the arena is up (the title may still be fading in)
    for (let i = 0; i < 400 && !engine.play_mode(mode); i++) await wait(50);
    nb.mode = mode;
    if (mode === "anthill") {
      if (locked) document.exitPointerLock?.();
      return showAnthill();
    }
    $("practice").hidden = false;
    let shown = -1;
    setInterval(() => {
      const [on, hits, streak] = engine.practice_stats();
      if (!on || hits === shown) return;
      shown = hits;
      const h = $("hits"); h.textContent = hits; h.classList.add("tick"); requestAnimationFrame(() => requestAnimationFrame(() => h.classList.remove("tick")));
      $("streak").textContent = streak > 1 ? `x${streak}` : "";
    }, 100);
  };
  // the anthill's page side: shield and tally up top, a count on each stacker
  const showAnthill = () => {
    $("ants").hidden = false;
    const labels = $("antlabels"), hud = $("anthud"), win = $("antwin");
    let last = [];
    const step = () => {
      const a = engine.anthill_stats();
      if (!a[0]) return requestAnimationFrame(step);
      const [, won, shield, deaths, kills, secs] = a;
      const m = Math.floor(secs / 60), sec = String(Math.floor(secs % 60)).padStart(2, "0");
      hud.innerHTML = `<b>${kills}</b> ants · ${m}:${sec}` + (shield ? ` · shield <b>${shield}</b>` : "") + (deaths ? ` · fallen ${deaths}` : "");
      const n = (a.length - 6) / 5;
      while (labels.children.length < n) labels.appendChild(document.createElement("span"));
      for (let k = 0; k < n; k++) {
        const [x, y, stored, left, over] = a.slice(6 + k * 5, 11 + k * 5);
        const el = labels.children[k];
        el.style.left = x + "px"; el.style.top = canvas.getBoundingClientRect().height - y + "px"; // y counts up from the bottom
        el.hidden = x < 0 || y < 0 || x > innerWidth || y > innerHeight;
        const txt = over ? "" : String(stored);
        if (el.textContent !== txt) {
          el.textContent = txt;
          if (last[k] !== undefined) { el.classList.remove("tick"); void el.offsetWidth; el.classList.add("tick"); }
        }
        last[k] = txt;
        el.classList.toggle("gone", !!over);
        el.title = `${left} ants left in this nest`;
      }
      // off-screen soldiers and nests: a small arrow on the edge pointing at them
      const mk = engine.anthill_marks(), marks = $("antmarks"), r = canvas.getBoundingClientRect();
      const W = r.width, H = r.height, pad = 34;
      let used = 0;
      for (let i = 0; i < mk.length; i += 3) {
        const x = mk[i], y = H - mk[i + 1], kind = mk[i + 2];
        if (x > 0 && x < W && y > 0 && y < H) continue;
        const cx = W / 2, cy = H / 2, dx = x - cx, dy = y - cy;
        const s = Math.min((W / 2 - pad) / Math.abs(dx || 1e-3), (H / 2 - pad) / Math.abs(dy || 1e-3));
        const el = marks.children[used] || marks.appendChild(document.createElement("i"));
        el.style.left = cx + dx * s + "px"; el.style.top = cy + dy * s - 6 + "px";
        el.style.transform = `rotate(${Math.atan2(dy, dx)}rad)`;
        el.className = kind === 2 ? "nest" : kind === 1 ? "wind" : "";
        el.hidden = false; used++;
      }
      for (let i = used; i < marks.children.length; i++) marks.children[i].hidden = true;
      if (won && win.hidden) {
        win.hidden = false;
        win.innerHTML = `<p class="big">the hill is empty</p><p>${kills} ants in ${m}:${sec}${deaths ? `, fallen ${deaths} time${deaths > 1 ? "s" : ""}` : ", untouched"}</p><p class="dim">click to go again</p>`;
        win.onpointerdown = (e) => { e.stopPropagation(); nb.leaving = true; location.href = location.pathname + "?anthill"; };
      }
      requestAnimationFrame(step);
    };
    step();
  };
  $("antbtn").onclick = (e) => { e.stopPropagation(); startPractice("anthill"); };
  $("antbtn").onpointerdown = (e) => e.stopPropagation();
  const aimBtn = $("aimbtn");
  const showAim = () => { aimBtn.innerHTML = `aim for a <b>${aimMode}</b> · switch to ${aimMode === "trackpad" ? "mouse" : "trackpad"}`; };
  showAim();
  aimBtn.onclick = (e) => {
    e.stopPropagation();
    aimMode = aimMode === "trackpad" ? "mouse" : "trackpad";
    try { localStorage.setItem(AIM_KEY, aimMode); } catch {}
    if (aimMode === "trackpad" && locked) document.exitPointerLock?.();
    showAim();
  };
  aimBtn.onpointerdown = (e) => e.stopPropagation();
  // the start stack is the bot match too; this names it
  $("botbtn").onclick = (e) => { e.stopPropagation(); const p = engine.locate("StartingScreenButton"); if (p.length) { engine.pointer(900, p[0], p[1], 0); engine.pointer(900, p[0], p[1], 2); } };
  $("botbtn").onpointerdown = (e) => e.stopPropagation();
  $("practicebtn").onclick = (e) => { e.stopPropagation(); startPractice(); };
  $("practicebtn").onpointerdown = (e) => e.stopPropagation();
  if (params.has("practice")) startPractice();
  if (params.has("anthill")) startPractice("anthill");
}
main().catch((e) => { console.error(e); nb.errors.push(String(e)); $("status").textContent = String(e.message || e); });
