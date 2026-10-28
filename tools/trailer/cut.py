"""The editor: cuts the long gameplay takes (windows.py, make.py long) into
the trailer, without new footage.

Gameplay is the subject: shots are stretches of play chosen for what's going
on in them (jukes, near misses, banks, clashes, dashes, throws), entered and
left on action, in irregular lengths counted in eighths of the theme. A kill
can happen inside a shot, but the cut comes a beat after it - a soul that's
dead is never on screen. The order never repeats a take or arena back to
back and keeps motion flowing across cuts; the framing follows what the
shot holds.

    python3 cut.py   -> recordings/trailer/edit.json (then compose.mjs, make.py mix)
"""
import json, os, collections

HERE = os.path.dirname(os.path.abspath(__file__))
REC = os.path.normpath(os.path.join(HERE, "..", "..", "..", "recordings", "trailer"))
beats = json.load(open(os.path.join(REC, "beats.json")))
BEAT = beats["bar"] / 4
E8 = BEAT / 2
B0 = beats["beat0"]
FPS = 60
LENGTH = 32 * beats["bar"]
t8 = lambda n: B0 + n * E8
W = {"graze": 6, "clash": 6, "bounce": 4, "juke": 2.5, "throw": 2, "dash": 1.5, "kill": 4}
AFTER_KILL = 15  # frames of a kill's burst we may keep

# ---- the takes: frames, who's alive, and what happens when
takes = {}
for i, w in enumerate(json.load(open(os.path.join(REC, "windows.json")))):
    name = f"w{i}"
    m = json.load(open(os.path.join(REC, name, "meta.json")))
    t, souls = m["tickOf"], m["souls"]
    n = len(t)
    fr = lambda tick: next((j for j, x in enumerate(t) if x >= tick), n - 1)
    log = json.load(open(os.path.join(REC, "sweep", f"s{w['seed']}-m{w['map']}.json")))
    evs = []
    for p in log:
        if not (t[0] + 1 <= p["t"] <= t[-1] - 1):
            continue
        kind = "juke" if p["kind"] == "intent" and p["note"] == "juke" else p["kind"]
        if kind not in W or (kind == "kill" and p["who"] == p["other"]):
            continue
        who = p["other"] if kind == "graze" else p["who"]
        evs.append({"f": fr(p["t"]), "kind": kind, "who": who, "victim": p["other"] if kind == "kill" else None})
    alive = [bool(s[2] and s[5]) for s in souls]
    # a kill's burst may stay on screen a moment; past that, dead time
    ok = alive[:]
    for e in evs:
        if e["kind"] == "kill":
            for j in range(max(0, e["f"] - 2), min(n, e["f"] + AFTER_KILL)):  # (frames are stamped a tick early)
                ok[j] = True
    takes[name] = {"n": n, "map": w["map"], "evs": evs, "ok": ok, "souls": souls}


def motion(tk, f, k):
    s = tk["souls"]
    a, b = s[max(0, f - 4)], s[min(len(s) - 1, f + 4)]
    if not (a[k * 3 + 2] and b[k * 3 + 2]):
        return None
    dx, dy = b[k * 3] - a[k * 3], b[k * 3 + 1] - a[k * 3 + 1]
    l = (dx * dx + dy * dy) ** 0.5
    return (dx / l, dy / l) if l > 8 else None


def score(tk, a, b):
    """What a stretch holds: its events, entering and leaving on action, and
    a kill only if the cut follows it."""
    inside = [e for e in tk["evs"] if a <= e["f"] < b]
    if not all(tk["ok"][a:b]):
        return None, None
    for e in inside:
        if e["kind"] == "kill" and e["f"] < b - AFTER_KILL - 2:
            return None, None  # the cut must come right after a kill
    s = sum(W[e["kind"]] for e in inside)
    if any(a <= e["f"] < a + 10 for e in inside):
        s += 2  # in on action
    if any(b - 10 <= e["f"] < b for e in inside):
        s += 2  # out on action
    # the shot's centre of gravity: its biggest moment
    kill = [e for e in inside if e["kind"] == "kill"]
    main = kill[0] if kill else max(inside, key=lambda e: (W[e["kind"]], -abs(e["f"] - (a + b) / 2)), default=None)
    return s, main


used = collections.defaultdict(list)
shots, prev, n8, last_mode = [], None, 0, [None]
mode_turn = collections.Counter()


def free(take, a, b):
    return all(b + 4 < x or a - 4 > y for x, y in used[take])


def framing(main, L):
    """Framing from what the shot holds and how long it is."""
    kind = main["kind"] if main else "dash"
    if L >= 9:
        opts = ["wide", "pair", "pull"]
    elif kind in ("graze", "juke", "dash"):
        opts = ["follow", "killer", "pair"]
    elif kind == "kill":
        opts = ["push", "tight"]
    elif kind in ("clash", "bounce"):
        opts = ["pair", "push", "wide"]
    else:
        opts = ["pair", "follow"]
    i = mode_turn[tuple(opts)]
    m = opts[i % len(opts)]
    if m == last_mode[0]:
        m = opts[(i + 1) % len(opts)]
    mode_turn[tuple(opts)] += 1
    last_mode[0] = m
    return m


def place(take, a, L, main, trans="cut", mode=None, heavy=False):
    global prev, n8
    at = 0.0 if n8 == 0 else t8(n8)
    dur = t8(n8 + L) - at
    b = a + round(dur * FPS)
    mode = mode or framing(main, L)
    who = main["who"] if main else 4
    act = 0 if who <= 4 else 1
    tgt = "other" if main and main["kind"] == "kill" else "act"
    roll = [0, 2, 0, -2.5, 0, 0, 2.5, 0][len(shots) % 8] if mode in ("follow", "killer", "tight") else 0
    peak = (main["f"] - a) / FPS if main else dur * 0.6
    shots.append({"take": take, "from": a, "at": round(at, 4), "dur": round(dur, 4), "mode": mode, "roll": roll, "trans": trans,
                  "act": act, "tgt": tgt, "peak": round(peak, 3), "heavy": heavy or bool(main and main["kind"] == "kill"),
                  "what": main["kind"] if main else "-"})
    used[take].append((a, b))
    tk = takes[take]
    prev = {"take": take, "map": tk["map"], "dir": motion(tk, b - 3, act), "who": who, "end": b, "killed": bool(main and main["kind"] == "kill")}
    n8 += L


def best_for(L, want=None, at_frac=None):
    """The best free stretch of L eighths; `want` a kind of moment, landing
    at `at_frac` of the shot if given."""
    dur = (t8(n8 + L) - (0.0 if n8 == 0 else t8(n8)))
    df = round(dur * FPS)
    best = None
    for name, tk in takes.items():
        starts = range(0, tk["n"] - df, 3)
        if want:
            # a kill sits just before the cut (its burst, then out); other moments at at_frac
            starts = [e["f"] - (df - 13 if e["kind"] == "kill" else round(df * at_frac)) for e in tk["evs"] if e["kind"] in want]
        for a in starts:
            b = a + df
            if a < 0 or b > tk["n"] or not free(name, a, b):
                continue
            s, main = score(tk, a, b)
            if s is None:
                continue
            if prev:
                if prev["take"] == name:
                    s -= 50
                if prev["map"] == tk["map"]:
                    s -= 4
                if main and prev["who"] == main["who"]:
                    s -= 1.5
                m1 = motion(tk, a + 3, 0 if (main and main["who"] <= 4) else 1)
                if prev["dir"] and m1:
                    s += 3 * (prev["dir"][0] * m1[0] + prev["dir"][1] * m1[1])
            # density per second, so short shots aren't starved
            s = s / max(1.0, dur ** 0.6)
            if want == ("kill",):
                s += 20
            if best is None or s > best[0]:
                best = (s, name, a, main)
    return best


# the opener: 20 eighths of play, its biggest moment landing on the drop
op = best_for(20, want=("clash", "bounce", "kill", "graze"), at_frac=t8(16) / t8(20))
place(op[1], op[2], 20, op[3], mode="pull", heavy=True)
cards = [{"text": "NEOBRAWL", "style": "drop", "at": t8(16), "dur": 2 * BEAT}]
RHYTHM = [
    # phrase A: establishing; plays given room
    [12, 5, 7, 4, 10, 3, 9, 6, 4],
    # phrase B: quicker, a few long breaths
    [6, 4, 8, 3, 5, 10, 4, 3, 7, 6, 8],
    # phrase C: the peak, tightening into the breakdown
    [10, 6, 4, 8, 3, 3, 6, 4, 2, 2, 4, 12],
]
# where a kill lands (now and then, inside the play), per phrase
KILLS = [{3, 7}, {2, 6, 9}, {1, 4, 7, 10}]
cont = [0]


def continuation(L):
    """The same duel picked up again right after the respawn (a jump cut)."""
    tk = takes[prev["take"]]
    df = round((t8(n8 + L) - t8(n8)) * FPS)
    a = prev["end"]
    while a < tk["n"] and not (tk["souls"][a][2] and tk["souls"][a][5]):
        a += 1
    a += 2
    if a + df > tk["n"] or not free(prev["take"], a, a + df):
        return None
    s, main = score(tk, a, a + df)
    return None if s is None else (s, prev["take"], a, main)


for k, phrase in enumerate(RHYTHM):
    for i, L in enumerate(phrase):
        got = None
        if prev and prev["killed"] and cont[0] % 2 == 0 and L >= 3:
            got = continuation(L)
        if prev and prev["killed"]:
            cont[0] += 1
        if not got and i in KILLS[k]:
            got = best_for(L, want=("kill",), at_frac=0.78)
        if not got:
            got = best_for(L)
        if not got:
            raise SystemExit(f"no footage left for a {L}-eighth shot")
        place(got[1], got[2], L, got[3], trans=("flash", "whip", "flash")[k] if i == 0 else "cut")
# the breakdown: long breaths of play, wide and held (one shot if the footage has it)
for split in ([24], [14, 10], [8, 8, 8]):
    if best_for(split[0]):
        break
for i, L in enumerate(split):
    got = best_for(L)
    place(got[1], got[2], L, got[3], trans="flash" if i == 0 else "cut", mode=["wide", "pull", "pair"][i])
cards.append({"text": "NEOBRAWL", "sub": "JAXS-RIBS.GITHUB.IO/NEOBRAWL", "style": "title", "at": t8(n8), "dur": LENGTH - t8(n8) + 0.05})
json.dump({"fps": FPS, "length": LENGTH, "out": "trailer", "shots": shots, "cards": cards}, open(os.path.join(REC, "edit.json"), "w"), indent=1)
print(len(shots), "shots; ends", round(t8(n8), 2), "s; holds", dict(collections.Counter(s["what"] for s in shots)))
for s in shots:
    print(f'{s["at"]:6.2f} {s["dur"]:5.2f}s {s["take"]:4} from {s["from"]:4} {s["mode"]:7} {s["what"]}')
