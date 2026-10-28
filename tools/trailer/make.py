"""Make the trailer: pick clips from the finder's candidates, record each as a
take (slow-mo on its hit), lay them on the fight theme's bar grid, composite
the type and motion graphics, mix and encode.

    python3 make.py pick      -> picks.json (what goes where; edit by hand if you like)
    python3 make.py record [take...] -> recordings/trailer/<take>/ for every pick (or the named ones)
    python3 make.py edit      -> edit.json for the compositor
    python3 make.py compose   -> trailer frames
    python3 make.py mix       -> neobrawl-trailer.mp4
    python3 make.py all
"""
import json, os, subprocess, sys, collections, concurrent.futures as cf

HERE = os.path.dirname(os.path.abspath(__file__))
REC = os.path.normpath(os.path.join(HERE, "..", "..", "..", "recordings", "trailer"))
MUSIC = os.path.normpath(os.path.join(HERE, "..", "..", "web", "assets", "audio", "Map1Theme.mp3"))
TPS, FPS = 50, 60
beats = json.load(open(os.path.join(REC, "beats.json")))
BAR = beats["bar"]
BEAT = BAR / 4
MUSIC_AT = 0.0  # from the top: two bars of intro, the drop on bar 2
LENGTH = 32 * BAR  # one minute

sys.argv.append("all") if len(sys.argv) < 2 else None


def candidates():
    subprocess.run([sys.executable, os.path.join(HERE, "finder.py"), "", "100000"], capture_output=True)
    return json.load(open(os.path.join(REC, "clips.json")))


def pick():
    allc = candidates()
    used = []

    def free(c):
        return not any(u["seed"] == c["seed"] and not (c["to"] < u["from"] - 150 or c["from"] > u["to"] + 150) for u in used)

    def take(pred, n, key=lambda c: -c["score"]):
        got = [c for c in sorted(allc, key=key) if pred(c) and free(c)][:n]
        used.extend(got)
        return got

    has = lambda *tags: (lambda c: all(any(t == g or t.startswith(g) for t in c["tags"]) for g in tags))
    single = lambda c: "hits" not in c
    # the opener: a long, tense lead-in (a near miss first) that kills on the drop
    opener = take(lambda c: single(c) and has("clutch")(c) and "kill" not in c["tags"], 1)
    # the money shot for the breakdown: a bank off a near miss, the best there is
    money = take(lambda c: single(c) and has("1 bounce")(c) and (has("clutch")(c) or has("matrix dodge")(c)), 1)
    # the long take: a streak that fits four bars
    hero = take(lambda c: "hits" in c and len(c["hits"]) >= 4 and (c["to"] - c["from"]) / TPS < 4 * BAR + 0.5, 1, key=lambda c: -(len(c.get("hits", [])) / ((c["to"] - c["from"]) / TPS)))
    # the rest: a spread of plays and arenas
    shots, per, maps = [], collections.Counter(), collections.Counter()
    for c in sorted(allc, key=lambda c: -c["score"]):
        if not single(c) or not free(c):
            continue
        lead = c["tags"][0]
        if per[lead] >= 12 or maps[c["map"]] >= 14:
            continue
        per[lead] += 1
        maps[c["map"]] += 1
        used.append(c)
        shots.append(c)
        if len(shots) == 46:
            break
    picks = {"opener": opener, "money": money, "hero": hero, "shots": shots}
    json.dump(picks, open(os.path.join(REC, "picks.json"), "w"), indent=1)
    for k, v in picks.items():
        for c in v:
            print(k, c["seed"], c["map"], round(c["hit"] / TPS, 1), c["tags"])


def takes():
    """(name, clip, from tick, to tick, slow-mo spans), from the plan: only
    the frames a shot shows (plus a margin), normal speed except the two
    slow motions."""
    plan = json.load(open(os.path.join(REC, "edit.json")))
    out = []
    for sh in plan["shots"]:
        c, hit = sh["clip"], sh["hit"]
        spans = [tuple(x) for x in sh.get("slow", [])]
        stretch = sum(l / s_ - l for _, l, s_ in spans)  # extra ticks slow motion adds
        pre = int(sh["hitAt"] * TPS) + 15
        post = int((sh["dur"] - sh["hitAt"]) * TPS) + 15
        # slow motion eats real time: fewer game ticks per shown second
        if spans:
            pre = int(pre + 5)
        out.append((sh["take"], c, hit - pre, hit + post, spans))
    return out


def record():
    def one(it):
        i, (name, c, a, b, spans) = it
        real = sum(l / s - l for _, l, s in spans) / TPS + (b - a) / TPS + 0.6  # hit-stops hold the tick still
        cmd = ["node", os.path.join(HERE, "record.mjs"), name, str(c["seed"]), str(c["map"]), str(max(0, a)), f"{real:.2f}", str(FPS), ",".join(f"{x}:{l}:{s}" for x, l, s in spans), str(9360 + i)]
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=1800)
        return name, (r.stdout + r.stderr).strip().splitlines()[-1:]
    only = set(sys.argv[2:]) if sys.argv[1] == "record" else set()
    with cf.ThreadPoolExecutor(4) as ex:
        for name, msg in ex.map(one, enumerate(t for t in takes() if not only or t[0] in only)):
            print(name, msg)


def edit():
    """The plan: the music's map turned into slots (prose rhythm - long
    build-ups, quick strikes), plays matched to slots, a camera for each."""
    p = json.load(open(os.path.join(REC, "picks.json")))
    B0 = beats["beat0"]
    beat = lambda n: B0 + n * BEAT
    shots, cards = [], []
    pool = list(p["shots"])
    # long slots want a build-up (a near miss, a dodge); short ones a strike
    def take_for(L):
        build = lambda c: any(t in ("clutch", "matrix dodge") for t in c["tags"]) or "1 bounce" in c["tags"]
        strike = lambda c: "point blank" in c["tags"] or c["tags"][0] in ("rush", "punish")
        want = build if L >= 3 else strike if L <= 1 else (lambda c: True)
        for c in pool:
            if want(c):
                pool.remove(c)
                return c
        return pool.pop(0)

    modes_long, modes_mid, modes_short = ["wide", "pull", "pair", "push"], ["pair", "push", "killer", "wide"], ["tight", "killer", "push"]
    last = [None]
    rot = [0]

    def mode_for(L, i):
        opts = modes_long if L >= 4 else modes_mid if L >= 2 else modes_short
        m = opts[i % len(opts)]
        if m == last[0]:
            m = opts[(i + 1) % len(opts)]
        last[0] = m
        return m

    def add(name, c, n0, L, hit_beat, mode, hit_tick=None, trans="cut", slow=(), heavy=False):
        at = beat(n0) if n0 > 0 else 0.0
        dur = beat(n0 + L) - at
        hitAt = beat(hit_beat) - at
        rot[0] += 1
        roll = [0, 0, 2.5, 0, -2, 0, 0, 3][rot[0] % 8] if mode in ("killer", "tight", "push") else 0
        shots.append({"take": name, "clip": c, "hit": hit_tick or c["hit"], "who": c["who"], "at": round(at, 4), "dur": round(dur, 4),
                      "hitAt": round(hitAt, 4), "mode": mode, "roll": roll, "trans": trans, "slow": list(slow), "heavy": heavy})

    # 0 - drop: a long, tense opener; the kill lands on the drop
    o = p["opener"][0]
    add("opener", o, 0, 10, 8, "pull", heavy=True)
    cards.append({"text": "NEOBRAWL", "style": "drop", "at": beat(8), "dur": 2 * BEAT})
    # phrase A: establishing, mixed lengths
    n, i = 10, 0
    for L in [4, 2, 2, 6, 3, 1, 4, 2, 2, 4]:
        c = take_for(L)
        add(f"s{len(shots)}", c, n, L, n + (L - 1 if L >= 3 else 1 if L == 2 else 0.25), mode_for(L, i), trans="flash" if n == 10 else "cut")
        n += L; i += 1
    # phrase B: quicker, tighter
    for L in [2, 2, 1, 1, 2, 4, 2, 1, 1, 2, 2, 3, 1, 4, 2, 2]:
        c = take_for(L)
        add(f"s{len(shots)}", c, n, L, n + (L - 1 if L >= 3 else 1 if L == 2 else 0.25), mode_for(L, i), trans="whip" if n == 40 else "cut")
        n += L; i += 1
    # the long take: a streak, held, its last kill slowed a touch
    h = p["hero"][0]
    last_kill = h["hits"][-1]
    add("hero", h, n, 16, n + 15, "pair", hit_tick=last_kill, trans="flash", slow=[(last_kill - 5, 14, 0.6)], heavy=True)
    n += 16
    # the run into the breakdown: strikes on every beat, then eighths
    for L in [2, 1, 1, 2, 1, 1, 1, 1, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 2]:
        c = take_for(L)
        add(f"s{len(shots)}", c, n, L, n + (1 if L == 2 else 0.25 if L == 1 else 0.1), mode_for(L, i))
        n += L; i += 1
    # the breakdown: the money shot, the one real slow motion
    mo = p["money"][0]
    add("money", mo, n, 12, n + 8, "push", trans="flash", slow=[(mo["hit"] - 10, 26, 0.42)], heavy=True)
    n += 12
    cards.append({"text": "NEOBRAWL", "sub": "JAXS-RIBS.GITHUB.IO/NEOBRAWL", "style": "title", "at": beat(n), "dur": LENGTH - beat(n) + 0.05, "bg": "fade"})
    e = {"fps": FPS, "length": LENGTH, "music_at": 0.0, "out": "trailer", "shots": shots, "cards": cards}
    json.dump(e, open(os.path.join(REC, "edit.json"), "w"), indent=1)
    print(len(shots), "shots, ends at beat", n, "=", round(beat(n), 2), "s; plays left over:", len(pool))


def check():
    """Every shot's victim dies on the frame the plan put on the beat, and the
    take has the lead-in the shot shows (catches stale builds, bad logs)."""
    e = json.load(open(os.path.join(REC, "edit.json")))
    bad = 0
    for s in e["shots"]:
        m = json.load(open(os.path.join(REC, s["take"], "meta.json")))
        t = m["tickOf"]
        hf = next((i for i, x in enumerate(t) if x >= s["hit"]), None)
        v = 1 if s["who"] <= 4 else 0
        alive = lambda i: m["souls"][min(max(i, 0), len(t) - 1)][v * 3 + 2]
        if hf is None or alive(hf - 4) != 1 or alive(hf + 6) != 0 or hf / m["fps"] < s["hitAt"] - 0.02:
            bad += 1
            print("BAD", s["take"], s["clip"]["seed"], s["hit"], "hit frame", hf)
    print(len(e["shots"]), "shots checked,", bad, "bad")
    return bad == 0


def compose():
    r = subprocess.run(["node", os.path.join(HERE, "compose.mjs")], capture_output=True, text=True)
    print(r.stdout[-400:], r.stderr[-400:])


def mix():
    e = json.load(open(os.path.join(REC, "edit.json")))
    inputs, filters = ["-ss", f"{MUSIC_AT:.3f}", "-t", f"{LENGTH:.3f}", "-i", MUSIC], []
    filters.append(f"[1:a]volume=0.8,afade=t=out:st={LENGTH - 2.5:.2f}:d=2.5[m]")
    n = 2  # 0 is the frames, 1 the music
    for s in e["shots"]:
        meta = json.load(open(os.path.join(REC, s["take"], "meta.json")))
        tick_of = meta["tickOf"]
        if s.get("from") is not None:
            in_f = s["from"]
        elif s.get("hitAt") is not None:
            hit_f = next((i for i, t in enumerate(tick_of) if t >= s["hit"]), len(tick_of) - 1)
            in_f = hit_f - round(s["hitAt"] * meta["fps"])
        else:
            in_f = round(s.get("in", 0) * meta["fps"])
        a = max(0, in_f) / meta["fps"]
        inputs += ["-i", os.path.join(REC, s["take"], "audio.wav")]
        ms = int(s["at"] * 1000)
        filters.append(f"[{n}:a]atrim={a:.3f}:{a + s['dur']:.3f},asetpts=PTS-STARTPTS,afade=t=out:st={s['dur'] - 0.05:.3f}:d=0.05,volume=1.1,adelay={ms}|{ms}[s{n}]")
        n += 1
    mixin = "[m]" + "".join(f"[s{i}]" for i in range(2, n))
    filters.append(f"{mixin}amix=inputs={n - 1}:normalize=0,alimiter=limit=0.95[a]")
    out = os.path.join(REC, "neobrawl-trailer.mp4")
    cmd = ["ffmpeg", "-v", "error", "-y", "-framerate", str(FPS), "-i", os.path.join(REC, "trailer", "frames", "%05d.jpg")] + inputs + [
        "-filter_complex", ";".join(filters), "-map", "0:v", "-map", "[a]", "-c:v", "libx264", "-preset", "slow", "-crf", "17", "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "256k", "-t", f"{LENGTH:.3f}", out]
    subprocess.run(cmd, check=True)
    print(out)


steps = {"check": check, "pick": pick, "record": record, "edit": edit, "compose": compose, "mix": mix}
if sys.argv[1] == "all":
    for s in ["pick", "record", "edit", "compose", "mix"]:
        steps[s]()
else:
    steps[sys.argv[1]]()
