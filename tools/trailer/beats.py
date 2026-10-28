"""Tempo, beat grid and loudness map of the fight theme (pure python: an
onset envelope from ffmpeg's decode, autocorrelated for the tempo)."""
import subprocess, struct, sys, json, math
src = sys.argv[1]
SR, HOP = 11025, 110  # ~10 ms hops
raw = subprocess.run(["ffmpeg", "-v", "error", "-i", src, "-ac", "1", "-ar", str(SR), "-f", "s16le", "-"], capture_output=True).stdout
x = struct.unpack(f"<{len(raw) // 2}h", raw)
n = len(x) // HOP
energy = [sum(abs(v) for v in x[i * HOP:(i + 1) * HOP]) / HOP for i in range(n)]
# onset strength: positive change in log energy
le = [math.log(1 + e) for e in energy]
on = [max(0.0, le[i] - le[i - 1]) for i in range(1, n)]
fps = SR / HOP
best = None
for bpm10 in range(800, 1800, 2):
    lag = 60 * fps / (bpm10 / 10)
    l0 = int(lag)
    fr = lag - l0
    s = sum(on[i] * (on[i + l0] * (1 - fr) + on[i + l0 + 1] * fr) for i in range(0, len(on) - l0 - 2, 2))
    if best is None or s > best[0]:
        best = (s, bpm10 / 10)
bpm = best[1]
period = 60 * fps / bpm
# phase: the offset whose grid collects the most onset
ph = max(range(int(period)), key=lambda p: sum(on[int(p + k * period)] for k in range(int((len(on) - p) / period))))
beat0 = (ph + 1) / fps
# loudness per bar (4 beats)
bar = 4 * 60 / bpm
bars = []
t = beat0
while t + bar < n / fps:
    a, b = int(t * fps), int((t + bar) * fps)
    bars.append(round(sum(energy[a:b]) / (b - a)))
    t += bar
print(json.dumps({"bpm": bpm, "beat0": round(beat0, 3), "bar": round(bar, 3), "bars": bars}))
