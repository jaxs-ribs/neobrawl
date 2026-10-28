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
MUSIC_AT = beats["beat0"] + 27 * BAR  # the breakdown: the drop lands 8 bars in
LENGTH = 32 * BAR  # one minute

sys.argv.append("all") if len(sys.argv) < 2 else None


def candidates():
    subprocess.run([sys.executable, os.path.join(HERE, "finder.py"), "", "100000"], capture_output=True)
    return json.load(open(os.path.join(REC, "clips.json")))


def pick():
    allc = candidates()
    used = []

    def free(c):
        return not any(u["seed"] == c["seed"] and not (c["to"] < u["from"] - 100 or c["from"] > u["to"] + 100) for u in used)

    def take(pred, n, key=lambda c: -c["score"]):
        got = [c for c in sorted(allc, key=key) if pred(c) and free(c)][:n]
        used.extend(got)
        return got

    has = lambda *tags: (lambda c: all(any(t == g or t.startswith(g) for t in c["tags"]) for g in tags))
    short = lambda c: "hits" not in c
    intro = []
    for label, pred in [("BANK", has("bank", "1 bounce")), ("CORNER", has("corner", "dash-throw")), ("COUNTER", has("counter")), ("MATRIX", has("matrix dodge")), ("RUSH", has("rush", "point blank"))]:
        got = take(lambda c, p=pred: short(c) and p(c), 1)
        intro += [dict(c, label=label) for c in got]
    # the hero: a streak of 5+ kills that fits its 6 bars
    hero = take(lambda c: "hits" in c and len(c["hits"]) >= 5 and (c["to"] - c["from"]) / TPS < 6 * BAR + 0.8, 1, key=lambda c: -(len(c.get("hits", [])) / ((c["to"] - c["from"]) / TPS)))
    # the montage: 16 of the rest, a spread of kinds
    montage, per = [], collections.Counter()
    for c in sorted(allc, key=lambda c: -c["score"]):
        if not short(c) or not free(c):
            continue
        lead = c["tags"][0]
        if per[lead] >= 4:
            continue
        per[lead] += 1
        used.append(c)
        montage.append(c)
        if len(montage) == 16:
            break
    picks = {"intro": intro, "montage": montage, "hero": hero}
    json.dump(picks, open(os.path.join(REC, "picks.json"), "w"), indent=1)
    for k, v in picks.items():
        for c in v:
            print(k, c["seed"], c["map"], round(c["hit"] / TPS, 1), c.get("label", ""), c["tags"])


def takes():
    p = json.load(open(os.path.join(REC, "picks.json")))
    out = []
    for i, c in enumerate(p["intro"]):
        out.append((f"intro{i}", c, c["hit"] - 120, c["hit"] + 45, [(c["hit"] - 5, 16, 0.2)]))
    for i, c in enumerate(p["montage"]):
        out.append((f"mont{i}", c, c["hit"] - 110, c["hit"] + 40, [(c["hit"] - 3, 12, 0.3)]))
    for c in p["hero"]:
        out.append(("hero", c, c["from"], c["to"], [(h - 2, 8, 0.35) for h in c["hits"]]))
    return out


def record():
    def one(it):
        i, (name, c, a, b, spans) = it
        real = sum(l / s - l for _, l, s in spans) / TPS + (b - a) / TPS + 0.6  # hit-stops hold the tick still
        cmd = ["node", os.path.join(HERE, "record.mjs"), name, str(c["seed"]), str(c["map"]), str(max(0, a)), f"{real:.2f}", str(FPS), ",".join(f"{x}:{l}:{s}" for x, l, s in spans), str(9360 + i)]
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=1800)
        return name, (r.stdout + r.stderr).strip().splitlines()[-1:]
    only = set(sys.argv[2:]) if sys.argv[1] == "record" else set()
    with cf.ThreadPoolExecutor(3) as ex:
        for name, msg in ex.map(one, enumerate(t for t in takes() if not only or t[0] in only)):
            print(name, msg)


def edit():
    p = json.load(open(os.path.join(REC, "picks.json")))
    shots, cards = [], []

    def label(c):
        main = c.get("label") or c["tags"][0].upper()
        rest = [t.upper() for t in c["tags"] if t.upper() != main and t not in ("point blank", "dash-throw", "clash")][:3]
        return main, "  /  ".join(rest)

    # 0-15 s: the breakdown - the name, then the plays one by one, slowed
    cards.append({"text": "NEOBRAWL", "sub": "TWO SOULS  /  ONE BOX EACH", "style": "title", "at": 0, "dur": 2 * BAR, "bg": True})
    for i, c in enumerate(p["intro"]):
        m, s = label(c)
        shots.append({"take": f"intro{i}", "hit": c["hit"], "at": (2 + i) * BAR - (BEAT if i == 0 else 0), "dur": BAR + (BEAT if i == 0 else 0), "hitAt": BAR * 0.62 + (BEAT if i == 0 else 0), "label": m, "sub": s})
    t = 7 * BAR
    for j, w in enumerate(["EVERY", "KILL", "REFILLS", "YOUR DASH"]):
        cards.append({"text": w, "style": "word", "at": t + j * BEAT, "dur": BEAT, "bg": True})
    # 15-45 s: the drop - a bar a shot
    for i, c in enumerate(p["montage"]):
        m, s = label(c)
        shots.append({"take": f"mont{i}", "hit": c["hit"], "at": (8 + i) * BAR, "dur": BAR, "hitAt": BAR * 0.55, "label": m, "sub": s, "push": 0.06})
    # 45-56 s: the streak, long
    for c in p["hero"]:
        n = len(c["hits"])
        shots.append({"take": "hero", "hit": c["hits"][0], "at": 24 * BAR, "dur": 6 * BAR, "in": 0, "label": f"STREAK X{n}", "sub": "EVERY KILL GIVES THE DASH BACK", "push": 0.03})
    # the end card
    cards.append({"text": "NEOBRAWL", "sub": "PLAY IN YOUR BROWSER  /  JAXS-RIBS.GITHUB.IO/NEOBRAWL", "style": "title", "at": 30 * BAR, "dur": 2 * BAR, "bg": "fade"})
    e = {"fps": FPS, "length": LENGTH, "out": "trailer", "shots": shots, "cards": cards}
    json.dump(e, open(os.path.join(REC, "edit.json"), "w"), indent=1)
    print(len(shots), "shots,", len(cards), "cards")


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
        if s.get("hitAt") is not None:
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


steps = {"pick": pick, "record": record, "edit": edit, "compose": compose, "mix": mix}
if sys.argv[1] == "all":
    for s in ["pick", "record", "edit", "compose", "mix"]:
        steps[s]()
else:
    steps[sys.argv[1]]()
