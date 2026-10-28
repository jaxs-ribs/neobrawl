"""The editor: cuts the recorded takes into a trailer without new footage.

It reads what happens in every take (from the sweep's play logs: jukes,
dashes, banks, clashes, throws, kills), cuts each take into plays - stretches
with a beginning and a turn, not just a kill - and lays them on the theme:
irregular lengths counted in eighths, cuts on action, the turn of each play
on a beat, and an order that never repeats a take, an arena, a framing or a
winner's colour back to back, and keeps motion flowing across cuts.

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
t8 = lambda n: B0 + n * E8  # time of eighth n
old = json.load(open(os.path.join(REC, "edit.json")))
plan_shots = {s["take"]: s for s in old["shots"]} if "clip" in old["shots"][0] else json.load(open(os.path.join(REC, "takes.json")))
json.dump(plan_shots, open(os.path.join(REC, "takes.json"), "w"))

# ---- every take: its frames, its events, and the plays in it
takes = {}
for name, s in plan_shots.items():
    m = json.load(open(os.path.join(REC, name, "meta.json")))
    c = s["clip"]
    log = json.load(open(os.path.join(REC, "sweep", f"s{c['seed']}-m{c['map']}.json")))
    t = m["tickOf"]
    fr = lambda tick: next((i for i, x in enumerate(t) if x >= tick), len(t) - 1)
    evs = []
    for p in log:
        if t[0] + 2 <= p["t"] <= t[-1] - 2 and p["kind"] in ("dash", "throw", "kill", "graze", "bounce", "clash"):
            who = p["other"] if p["kind"] == "graze" else p["who"]
            evs.append({"f": fr(p["t"]), "kind": p["kind"], "who": who, "throw": p["throw"]})
    # the souls' motion per frame (screen px), for direction matching
    souls = m["souls"]
    takes[name] = {"n": len(t), "map": c["map"], "seed": c["seed"], "evs": evs, "souls": souls, "size": m["size"], "slowmo": bool(m["spans"])}

WEIGHT = {"graze": 5, "clash": 5, "bounce": 4, "dash": 1.5, "throw": 1.5, "kill": 2}


def motion(tk, f, who):
    """Screen direction the soul `who` is moving at frame f (unit vector or None)."""
    k = 0 if who <= 4 else 1
    s = tk["souls"]
    a, b = s[max(0, f - 4)], s[min(len(s) - 1, f + 4)]
    if not (a[k * 3 + 2] and b[k * 3 + 2]):
        return None
    dx, dy = b[k * 3] - a[k * 3], b[k * 3 + 1] - a[k * 3 + 1]
    l = (dx * dx + dy * dy) ** 0.5
    return (dx / l, dy / l) if l > 8 else None


def plays(name, tk):
    """Candidate plays: around each interesting event, the stretch that sets
    it up and resolves it; its kind, actor, turn frame and what it holds."""
    out = []
    evs = tk["evs"]
    for e in evs:
        if e["kind"] not in ("graze", "clash", "bounce", "kill", "dash"):
            continue
        turn = e["f"]
        near = [x for x in evs if abs(x["f"] - turn) <= 70]
        score = sum(WEIGHT[x["kind"]] for x in near)
        kills = [x for x in near if x["kind"] == "kill"]
        out.append({"take": name, "turn": turn, "kind": e["kind"], "who": e["who"], "score": score, "kill": bool(kills),
                    "kill_f": kills[0]["f"] if kills else None, "map": tk["map"], "seed": tk["seed"],
                    # where the play can start (the action before it) and end
                    "lo": max(0, turn - 150), "hi": min(tk["n"] - 1, turn + 60), "events": near})
    return out


cands = [p for name, tk in takes.items() for p in plays(name, tk)]

# ---- the rhythm: shot lengths in eighths, phrase by phrase, and what each wants
# (kinds: "play" a whole play with its setup; "juke"/"clash"/"bank" a moment;
# "any" whatever fits; "kill" a payoff). Irregular on purpose, like sentences.
OPEN = [("open", 20)]  # bars 0-1: the dance before the drop, 20 eighths, into the drop
A = [("play", 12), ("juke", 3), ("bank", 5), ("play", 10), ("clash", 4), ("any", 2), ("play", 9), ("juke", 3), ("kill", 4), ("play", 8)]
B = [("bank", 4), ("juke", 2), ("play", 6), ("any", 2), ("clash", 3), ("play", 7), ("kill", 2), ("juke", 3), ("bank", 4), ("play", 9), ("any", 3), ("clash", 2), ("play", 6), ("juke", 3), ("kill", 4), ("play", 4)]
HERO = [("hero", 32)]
RUN = [("juke", 3), ("kill", 2), ("clash", 2), ("any", 3), ("bank", 2), ("kill", 2), ("juke", 2), ("any", 2), ("kill", 2), ("clash", 2), ("juke", 2), ("kill", 2), ("play", 6)]
MONEY = [("money", 24)]
for name, seq, want in (("A", A, 60), ("B", B, 64), ("RUN", RUN, 32)):
    assert sum(L for _, L in seq) == want, (name, sum(L for _, L in seq))

MODES = {"play": ["wide", "pair", "pull", "follow"], "juke": ["follow", "tight", "killer"], "bank": ["pair", "push", "wide"],
         "clash": ["tight", "push", "pair"], "kill": ["push", "killer", "tight"], "any": ["pair", "follow", "wide"], "dash": ["follow"]}

used = collections.defaultdict(list)  # take -> [(lo, hi)] frames already shown
shots = []


def overlaps(take, a, b):
    return any(not (b < x or a > y) for x, y in used[take])


def choose(want, L, prev):
    dur_f = round(L * E8 * FPS)
    best = None
    for p in cands:
        tk = takes[p["take"]]
        if p["take"] in ("hero", "money"):
            continue
        # what this slot wants
        if want in ("juke", "clash", "bank", "kill", "dash"):
            k = {"juke": "graze", "clash": "clash", "bank": "bounce", "kill": "kill", "dash": "dash"}[want]
            if p["kind"] != k:
                continue
        # where the turn sits: plays give it room to build (late in the shot), moments hit early
        lead = {"play": 0.72, "open": 0.95, "any": 0.55}.get(want, 0.4 if L > 2 else 0.3)
        a = p["turn"] - round(dur_f * lead)
        b = a + dur_f
        if a < 0 or b >= tk["n"] or overlaps(p["take"], a - 6, b + 6):
            continue
        # a kill in a play is fine, but cut soon after it; kills shouldn't be everywhere
        if p["kill"] and want not in ("kill", "any") and p["kill_f"] < b - 40:
            continue
        s = p["score"]
        if want == "play":
            s += 3 * sum(1 for e in p["events"] if a <= e["f"] <= b and e["kind"] in ("graze", "bounce", "clash", "dash"))
            s -= 6 if p["kill"] else 0
        if prev:
            if prev["take"] == p["take"]:
                s -= 100
            if prev["map"] == p["map"]:
                s -= 8
            if prev.get("winner") is not None and prev["winner"] == p["who"]:
                s -= 4  # the other soul gets its moment
            # match on action: motion out of the last shot continues into this one
            m0 = prev.get("dir_out")
            m1 = motion(tk, a + 3, p["who"])
            if m0 and m1:
                s += 4 * (m0[0] * m1[0] + m0[1] * m1[1])
        if best is None or s > best[0]:
            best = (s, p, a, b)
    return best


prev = None
n8 = 0
mode_i = collections.Counter()
last_mode = None


def place(want, L, p, a, b, trans="cut", heavy=False, mode=None):
    global prev, n8, last_mode
    at = 0.0 if n8 == 0 else t8(n8)
    dur = t8(n8 + L) - at
    if mode is None:
        opts = MODES.get(want, ["pair"])
        mode = opts[mode_i[want] % len(opts)]
        mode_i[want] += 1
        if mode == last_mode:
            mode = opts[mode_i[want] % len(opts)]
            mode_i[want] += 1
    last_mode = mode
    roll = [0, 2.5, 0, -2, 0, 0, 3, 0, -2.5][len(shots) % 9] if mode in ("follow", "tight", "killer", "push") else 0
    peak = (p["kill_f"] if (p["kill"] and want in ("kill", "money", "hero")) else p["turn"]) - a
    shots.append({"take": p["take"], "from": a, "at": round(at, 4), "dur": round(dur, 4), "mode": mode, "roll": roll, "trans": trans,
                  "act": 0 if p["who"] <= 4 else 1, "tgt": "other" if want in ("kill", "money") else "act", "peak": round(peak / FPS, 3), "heavy": heavy, "want": want})
    used[p["take"]].append((a, b))
    tk = takes[p["take"]]
    prev = {"take": p["take"], "map": p["map"], "winner": p["who"], "dir_out": motion(tk, b - 3, p["who"])}
    n8 += L


def run(seq, first_trans="cut"):
    for i, (want, L) in enumerate(seq):
        got = choose(want, L, prev)
        if got is None:
            got = choose("any", L, prev)
        _, p, a, b = got
        place(want, L, p, a, b, trans=first_trans if i == 0 else "cut", heavy=(want == "kill" and L >= 2))


# the opener: the longest dance in the opener take, its turn on the drop
op = max((p for p in cands if p["take"] == "opener"), key=lambda p: (p["kind"] in ("graze", "clash"), p["score"]))
dur_f = round(t8(20) * FPS)
a = max(0, op["turn"] - dur_f)
place("open", 20, dict(op), a, a + dur_f, mode="pull", heavy=True)
cards = [{"text": "NEOBRAWL", "style": "drop", "at": t8(16), "dur": 2 * BEAT}]
run(A, "flash")
run(B, "whip")
# the long take: the streak as one shot, held
h = max((p for p in cands if p["take"] == "hero"), key=lambda p: p["score"])
hk = takes["hero"]
L = 32
dur_f = round((t8(n8 + L) - t8(n8)) * FPS)
a = max(0, hk["n"] - dur_f - 20)
place("hero", L, dict(h, turn=a + dur_f - 30, kill=False), a, a + dur_f, trans="flash", mode="pair")
run(RUN)
# the breakdown: the money play, whole
mp = max((p for p in cands if p["take"] == "money" and p["kill"]), key=lambda p: p["score"])
mk = takes["money"]
L = 24
dur_f = round((t8(n8 + L) - t8(n8)) * FPS)
a = max(0, min(mk["n"] - dur_f - 1, mp["kill_f"] - round(dur_f * 0.7)))
place("money", L, mp, a, a + dur_f, trans="flash", heavy=True, mode="push")
cards.append({"text": "NEOBRAWL", "sub": "JAXS-RIBS.GITHUB.IO/NEOBRAWL", "style": "title", "at": t8(n8), "dur": LENGTH - t8(n8) + 0.05})
edit = {"fps": FPS, "length": LENGTH, "out": "trailer", "shots": shots, "cards": cards}
json.dump(edit, open(os.path.join(REC, "edit.json"), "w"), indent=1)
kinds = collections.Counter(s["want"] for s in shots)
print(len(shots), "shots; ends", round(t8(n8), 2), "s;", dict(kinds))
for s in shots:
    print(f'{s["at"]:6.2f} {s["dur"]:5.2f}s {s["want"]:6} {s["mode"]:7} {s["take"]:7} from {s["from"]}')
