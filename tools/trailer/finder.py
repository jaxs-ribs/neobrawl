"""The highlight finder: reads the sweep's play logs and ranks the plays
worth filming. Each candidate is a clip (seed, map, from tick, to tick)
with tags saying what happened and a score.

    python3 finder.py [sweep dir] [how many]   -> prints the top clips, writes clips.json
"""
import json, os, sys, glob, collections

SWEEP = sys.argv[1] if len(sys.argv) > 1 and sys.argv[1] else os.path.join(os.path.dirname(__file__), "..", "..", "..", "recordings", "trailer", "sweep")
TOP = int(sys.argv[2]) if len(sys.argv) > 2 else 40
TPS = 50  # ticks per second

# what a strategy is worth on screen
NOTE = {"bank": 4, "corner": 4, "counter": 5, "rush": 2.5, "punish": 2.5, "chain": 2, "snipe": 1.5, "": 0.5}


def clips_of(plays, seed, mp):
    by = collections.defaultdict(list)
    for p in plays:
        by[p["kind"]].append(p)
    throws = {p["throw"]: p for p in by["throw"]}
    kills = sorted(by["kill"], key=lambda p: p["t"])
    out = []

    def near(kind, who_key, who, t0, t1, note=None):
        return [p for p in by[kind] if p.get(who_key) == who and t0 <= p["t"] <= t1 and (note is None or p.get("note") == note)]

    # kill streaks: the same killer again within 4 s (dash back each time)
    streak = {}
    for i, k in enumerate(kills):
        prev = [q for q in kills[:i] if q["who"] == k["who"] and k["t"] - q["t"] < 4 * TPS]
        streak[id(k)] = (streak[id(prev[-1])] + 1) if prev else 1
    for k in kills:
        th = throws.get(k["throw"])
        if not th:
            continue
        who, t = k["who"], k["t"]
        bounces = int(k["b"])
        flight = round((k["b"] - bounces) * 1000)
        tags, score = [], NOTE.get(k.get("note", ""), 0.5)
        note = k.get("note", "")
        if note:
            tags.append(note)
        if bounces:
            tags.append(f"{bounces} bounce" + "s" * (bounces > 1))
            score += 2 * bounces
        n = streak[id(k)]
        if n >= 2:
            tags.append(f"chain x{n}")
            score += 3 * (n - 1)
        if near("graze", "other", who, t - 100, t):
            tags.append("clutch")  # it nearly died first
            score += 3
        if near("intent", "who", who, t - 100, t, "matrix"):
            tags.append("matrix dodge")
            score += 3
        if [c for c in by["clash"] if who in (c["who"], c["other"]) and t - 100 <= c["t"] <= t]:
            tags.append("clash")
            score += 2
        dash = near("dash", "who", who, th["t"] - 15, th["t"])
        if dash:
            tags.append("dash-throw")
            score += 1.5
        if flight > 18:
            tags.append("long shot")
            score += 1
        if flight <= 2:
            tags.append("point blank")
            score += 0.5
        start = min([th["t"] - 45] + [d["t"] - 15 for d in dash])
        out.append({"seed": seed, "map": mp, "from": max(0, start), "to": t + 50, "hit": t, "score": round(score, 1), "tags": tags, "who": who})
    # whole streaks (4+ kills, each within 4 s): the long hero shots
    run = []
    for k in kills + [None]:
        if k is not None and run and k["who"] == run[-1]["who"] and k["t"] - run[-1]["t"] < 4 * TPS:
            run.append(k)
            continue
        if len(run) >= 4:
            th = throws.get(run[0]["throw"])
            start = (th["t"] if th else run[0]["t"]) - 50
            out.append({"seed": seed, "map": mp, "from": max(0, start), "to": run[-1]["t"] + 60, "hit": run[-1]["t"], "hits": [q["t"] for q in run], "score": 4 * len(run), "tags": [f"streak x{len(run)}"] + sorted({q.get("note", "") for q in run} - {""}), "who": run[0]["who"]})
        run = [k] if k is not None else []
    # clashes: boxes meeting in the air
    for c in by["clash"]:
        notes = [throws.get(c["throw"], {}).get("note", ""), throws.get(int(c["b"]), {}).get("note", "")]
        tags = ["clash"] + (["counter"] if "counter" in notes else [])
        score = 3 + (3 if "counter" in notes else 0)
        out.append({"seed": seed, "map": mp, "from": max(0, c["t"] - 60), "to": c["t"] + 35, "hit": c["t"], "score": score, "tags": tags, "who": c["who"]})
    return out


def main():
    allc = []
    for f in sorted(glob.glob(os.path.join(SWEEP, "s*-m*.json"))):
        name = os.path.basename(f)[:-5]
        seed, mp = int(name.split("-")[0][1:]), int(name.split("-")[1][1:])
        try:
            plays = json.load(open(f))
        except Exception:
            continue
        allc += clips_of(plays, seed, mp)
    allc.sort(key=lambda c: -c["score"])
    # no two clips from the same moment, and a spread of kinds
    picked, used = [], collections.Counter()
    for c in allc:
        if any(p["seed"] == c["seed"] and not (c["to"] < p["from"] or c["from"] > p["to"]) for p in picked):
            continue
        lead = c["tags"][0] if c["tags"] else "-"
        if used[lead] >= max(4, TOP // 4):
            continue
        used[lead] += 1
        picked.append(c)
        if len(picked) >= TOP:
            break
    kinds = collections.Counter(t for c in allc for t in c["tags"])
    print(f"{len(allc)} candidate clips; tags: {dict(kinds.most_common())}")
    for c in picked:
        print(f"{c['score']:5.1f}  seed {c['seed']:>3} map {c['map']}  t {c['from'] / TPS:6.1f}-{c['to'] / TPS:6.1f}s  {', '.join(c['tags'])}")
    json.dump(picked, open(os.path.join(SWEEP, "..", "clips.json"), "w"), indent=1)


main()
