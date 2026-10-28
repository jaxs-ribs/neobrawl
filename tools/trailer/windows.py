"""Find the densest stretches of play in the sweep: 8 s windows full of jukes,
near misses, banks, clashes, dashes and throws - kills allowed, but a window
built around one isn't the goal, and dead time counts against it.

    python3 windows.py [how many] -> recordings/trailer/windows.json
"""
import json, os, glob, sys

HERE = os.path.dirname(os.path.abspath(__file__))
REC = os.path.normpath(os.path.join(HERE, "..", "..", "..", "recordings", "trailer"))
N = int(sys.argv[1]) if len(sys.argv) > 1 else 12
WIN, STEP = 420, 25  # ticks
W = {"graze": 5, "clash": 5, "bounce": 3, "throw": 1.5, "dash": 1, "kill": 2}

found = []
for f in glob.glob(os.path.join(REC, "sweep", "s*-m*.json")):
    seed, mp = (int(x[1:]) for x in os.path.basename(f)[:-5].split("-"))
    log = json.load(open(f))
    evs = [p for p in log if p["kind"] in W and not (p["kind"] == "kill" and p["who"] == p["other"])]
    jukes = [p for p in log if p["kind"] == "intent" and p["note"] == "juke"]
    end = max((p["t"] for p in log), default=0)
    best = None
    for a in range(200, end - WIN, STEP):
        inside = [p for p in evs if a <= p["t"] < a + WIN]
        kills = sum(1 for p in inside if p["kind"] == "kill")
        s = sum(W[p["kind"]] for p in inside) + 1.5 * sum(1 for p in jukes if a <= p["t"] < a + WIN)
        s -= 4 * max(0, kills - 2)  # dead time
        # both sides doing things, not one soul farming the other
        sides = {p["who"] for p in inside if p["kind"] in ("throw", "dash")}
        if len(sides) < 2:
            continue
        if best is None or s > best[0]:
            best = (s, a, kills)
    if best:
        found.append({"seed": seed, "map": mp, "from": best[1], "to": best[1] + WIN, "score": round(best[0], 1), "kills": best[2]})
found.sort(key=lambda w: -w["score"])
picked, per_map = [], {}
for w in found:
    if per_map.get(w["map"], 0) >= (N + 3) // 4:
        continue
    per_map[w["map"]] = per_map.get(w["map"], 0) + 1
    picked.append(w)
    if len(picked) == N:
        break
json.dump(picked, open(os.path.join(REC, "windows.json"), "w"), indent=1)
for w in picked:
    print(w)
