"""The fight theme, read for the edit: on the 128 bpm grid, which 16ths have a
kick, a snare/clap or a hat, how loud each bar is, and where phrases turn
(fills, drops, breakdowns). Writes music.json; prints each bar's pattern.

    python3 music.py <track> <bpm> <first beat s>
"""
import subprocess, struct, sys, json, math

src, BPM, BEAT0 = sys.argv[1], float(sys.argv[2]), float(sys.argv[3])
SR, HOP = 11025, 55  # 5 ms hops


def env(filt):
    raw = subprocess.run(["ffmpeg", "-v", "error", "-i", src, "-af", filt, "-ac", "1", "-ar", str(SR), "-f", "s16le", "-"], capture_output=True).stdout
    x = struct.unpack(f"<{len(raw) // 2}h", raw)
    n = len(x) // HOP
    e = [math.log(1 + sum(abs(v) for v in x[i * HOP:(i + 1) * HOP]) / HOP) for i in range(n)]
    # onset: rise against the recent past
    return [max(0.0, e[i] - max(e[max(0, i - 4):i] or [0])) for i in range(n)], e


bands = {"K": "lowpass=f=110,lowpass=f=110", "S": "bandpass=f=2200:width_type=o:w=1.2", "H": "highpass=f=7500,highpass=f=7500"}
on, lvl = {}, {}
for k, f in bands.items():
    on[k], lvl[k] = env(f)
fps = SR / HOP
step = 60 / BPM / 4
n16 = int((len(on["K"]) / fps - BEAT0) / step)


def at(sig, t, w=0.025):
    a, b = int((t - w) * fps), int((t + w) * fps)
    return max(sig[max(0, a):max(a + 1, b)] or [0])


grid = {k: [at(on[k], BEAT0 + i * step) for i in range(n16)] for k in bands}
# a hit is an onset well above that band's typical strength
thr = {}
for k in bands:
    s = sorted(v for v in grid[k] if v > 0)
    thr[k] = s[int(len(s) * 0.7)] if s else 1
bars = []
for b in range(n16 // 16):
    pat = ""
    for i in range(16):
        j = b * 16 + i
        c = "."
        for k in "HSK":  # kick wins, then snare, then hat
            if grid[k][j] > thr[k]:
                c = k
        pat += c
    loud = sum(lvl["K"][int((BEAT0 + b * 16 * step) * fps):int((BEAT0 + (b + 1) * 16 * step) * fps)]) / (16 * step * fps)
    bars.append({"bar": b, "t": round(BEAT0 + b * 16 * step, 3), "pat": pat, "loud": round(loud, 2),
                 "kicks": [i for i in range(16) if grid["K"][b * 16 + i] > thr["K"]],
                 "snares": [i for i in range(16) if grid["S"][b * 16 + i] > thr["S"]],
                 "accent": [round(grid["K"][b * 16 + i] + grid["S"][b * 16 + i], 2) for i in range(16)]})
# sections by loudness, fills where a bar is busier than its neighbours
for i, b in enumerate(bars):
    busy = b["pat"].count("K") + b["pat"].count("S")
    near = [bars[j]["pat"].count("K") + bars[j]["pat"].count("S") for j in (i - 1, i + 1) if 0 <= j < len(bars)]
    b["fill"] = bool(near) and busy >= max(near) + 3
mean = sum(b["loud"] for b in bars) / len(bars)
for b in bars:
    b["section"] = "full" if b["loud"] > mean * 0.95 else "low"
for b in bars:
    print(f'{b["bar"]:>3} {b["t"]:7.2f}s {b["section"]:>4} {b["loud"]:5.2f} {b["pat"]}{"  FILL" if b["fill"] else ""}')
json.dump({"bpm": BPM, "beat0": BEAT0, "step": step, "bars": bars}, open(sys.argv[4] if len(sys.argv) > 4 else "music.json", "w"))
