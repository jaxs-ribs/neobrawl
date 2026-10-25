"""Trace the arena's sprites into vector shapes.

For each sprite: subpixel contours of the alpha mask (marching squares at
alpha 0.5), simplified, triangulated (holes bridged), plus a convex hull
for physics. Coordinates are sprite-local units, matching the engine's
sprite quad (pixel - pivot) / ppu, y up. A rasterised check reports how
well the vector fill matches the texture's mask.

    python3 tools/vectorize.py web/assets   ->  web/assets/shapes.json
"""

import json
import math
import struct
import sys
import zlib

SPRITES = ["Box1", "Wall", "BlobBW"]


def read_png(path):
    data = open(path, "rb").read()
    assert data[:8] == b"\x89PNG\r\n\x1a\n"
    pos, idat, w = 8, b"", 0
    while pos < len(data):
        n, kind = struct.unpack(">I4s", data[pos : pos + 8])
        body = data[pos + 8 : pos + 8 + n]
        pos += 12 + n
        if kind == b"IHDR":
            w, h, depth, ctype, _, _, inter = struct.unpack(">IIBBBBB", body)
            assert depth == 8 and inter == 0, "8-bit, non-interlaced only"
            ch = {6: 4, 2: 3, 0: 1, 4: 2}[ctype]
        elif kind == b"IDAT":
            idat += body
    raw = zlib.decompress(idat)
    stride = w * ch
    rows, prev, p = [], bytearray(stride), 0
    for _ in range(h):
        f = raw[p]
        line = bytearray(raw[p + 1 : p + 1 + stride])
        p += 1 + stride
        for i in range(stride):
            a = line[i - ch] if i >= ch else 0
            b = prev[i]
            c = prev[i - ch] if i >= ch else 0
            if f == 1:
                line[i] = (line[i] + a) & 255
            elif f == 2:
                line[i] = (line[i] + b) & 255
            elif f == 3:
                line[i] = (line[i] + (a + b) // 2) & 255
            elif f == 4:
                pa, pb, pc = abs(b - c), abs(a - c), abs(a + b - 2 * c)
                pr = a if pa <= pb and pa <= pc else (b if pb <= pc else c)
                line[i] = (line[i] + pr) & 255
        rows.append(line)
        prev = line
    alpha_ch = {4: 3, 2: 1}.get(ch)
    # alpha[y][x] with y up (row 0 of the PNG is the top)
    alpha = [[(rows[h - 1 - y][x * ch + alpha_ch] if alpha_ch is not None else 255) / 255.0 for x in range(w)] for y in range(h)]
    # the ink colour: alpha-weighted mean rgb of the opaque pixels
    acc, wsum = [0.0, 0.0, 0.0], 0.0
    for y in range(h):
        for x in range(w):
            a = alpha[y][x]
            if a > 0.5:
                px = rows[h - 1 - y][x * ch : x * ch + 3] if ch >= 3 else [rows[h - 1 - y][x * ch]] * 3
                for k in range(3):
                    acc[k] += px[k] / 255.0 * a
                wsum += a
    read_png.ink = [round(c / max(wsum, 1e-9), 4) for c in acc]
    return w, h, alpha


def contours(w, h, a, iso=0.5):
    """Closed subpixel loops of the iso-line, in pixel coords (centres at +0.5)."""
    g = lambda x, y: a[y][x] if 0 <= x < w and 0 <= y < h else 0.0
    segs = []

    def interp(x0, y0, x1, y1):
        v0, v1 = g(x0, y0), g(x1, y1)
        t = 0.5 if v0 == v1 else (iso - v0) / (v1 - v0)
        return (x0 + 0.5 + (x1 - x0) * t, y0 + 0.5 + (y1 - y0) * t)

    for y in range(-1, h):
        for x in range(-1, w):
            # corners: bl, br, tr, tl
            c = [(x, y), (x + 1, y), (x + 1, y + 1), (x, y + 1)]
            idx = sum((1 << k) for k, (cx, cy) in enumerate(c) if g(cx, cy) >= iso)
            if idx in (0, 15):
                continue
            e = {
                "b": lambda: interp(x, y, x + 1, y),
                "r": lambda: interp(x + 1, y, x + 1, y + 1),
                "t": lambda: interp(x, y + 1, x + 1, y + 1),
                "l": lambda: interp(x, y, x, y + 1),
            }
            # oriented so the inside is on the left
            table = {
                1: [("l", "b")], 2: [("b", "r")], 3: [("l", "r")], 4: [("r", "t")],
                5: [("l", "t"), ("r", "b")], 6: [("b", "t")], 7: [("l", "t")],
                8: [("t", "l")], 9: [("t", "b")], 10: [("t", "r"), ("b", "l")],
                11: [("t", "r")], 12: [("r", "l")], 13: [("r", "b")], 14: [("b", "l")],
            }
            for s, t in table[idx]:
                segs.append((e[s](), e[t]()))
    nxt = {}
    for p, q in segs:
        nxt[(round(p[0], 5), round(p[1], 5))] = q
    loops, used = [], set()
    for start in list(nxt):
        if start in used:
            continue
        loop, k = [], start
        while k not in used and k in nxt:
            used.add(k)
            loop.append(k)
            q = nxt[k]
            k = (round(q[0], 5), round(q[1], 5))
        if len(loop) > 8:
            loops.append(loop)
    return loops


def area(pts):
    return 0.5 * sum(pts[i][0] * pts[(i + 1) % len(pts)][1] - pts[(i + 1) % len(pts)][0] * pts[i][1] for i in range(len(pts)))


def rdp(pts, eps):
    if len(pts) < 3:
        return pts
    (x0, y0), (x1, y1) = pts[0], pts[-1]
    dx, dy = x1 - x0, y1 - y0
    l = math.hypot(dx, dy) or 1e-9
    best, bi = 0, 0
    for i in range(1, len(pts) - 1):
        d = abs(dy * pts[i][0] - dx * pts[i][1] + x1 * y0 - y1 * x0) / l
        if d > best:
            best, bi = d, i
    if best > eps:
        return rdp(pts[: bi + 1], eps)[:-1] + rdp(pts[bi:], eps)
    return [pts[0], pts[-1]]


def simplify_loop(loop, eps):
    # split at the two farthest-apart points so RDP has stable anchors
    far = max(range(len(loop)), key=lambda i: (loop[i][0] - loop[0][0]) ** 2 + (loop[i][1] - loop[0][1]) ** 2)
    a = rdp(loop[: far + 1], eps)
    b = rdp(loop[far:] + [loop[0]], eps)
    return a[:-1] + b[:-1]


def cross(o, a, b):
    return (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])


def inside_tri(p, a, b, c):
    return cross(a, b, p) >= -1e-12 and cross(b, c, p) >= -1e-12 and cross(c, a, p) >= -1e-12


def earclip(poly):
    """Triangulate a simple CCW polygon (may contain bridge seams)."""
    idx = list(range(len(poly)))
    tris, guard = [], 0
    while len(idx) > 3 and guard < 100000:
        guard += 1
        n = len(idx)
        for k in range(n):
            i0, i1, i2 = idx[k - 1], idx[k], idx[(k + 1) % n]
            a, b, c = poly[i0], poly[i1], poly[i2]
            if cross(a, b, c) <= 1e-12:
                continue
            if any(inside_tri(poly[j], a, b, c) for j in idx if j not in (i0, i1, i2) and poly[j] not in (a, b, c)):
                continue
            tris.append((a, b, c))
            idx.pop(k)
            break
        else:
            break
    if len(idx) == 3:
        tris.append(tuple(poly[i] for i in idx))
    return tris


def bridge(outer, holes):
    """Merge holes (CW) into the CCW outer ring with zero-width bridges."""
    poly = list(outer)
    for hole in sorted(holes, key=lambda h: -max(p[0] for p in h)):
        hi = max(range(len(hole)), key=lambda i: hole[i][0])
        hp = hole[hi]
        # nearest visible outer vertex to the right-ish
        oi = min(range(len(poly)), key=lambda i: (poly[i][0] - hp[0]) ** 2 + (poly[i][1] - hp[1]) ** 2 if poly[i][0] >= hp[0] else 1e18)
        ring = hole[hi:] + hole[: hi + 1]
        poly = poly[: oi + 1] + ring + poly[oi:]
    return poly


def hull(pts):
    pts = sorted(set(pts))
    lo, up = [], []
    for p in pts:
        while len(lo) >= 2 and cross(lo[-2], lo[-1], p) <= 0:
            lo.pop()
        lo.append(p)
    for p in reversed(pts):
        while len(up) >= 2 and cross(up[-2], up[-1], p) <= 0:
            up.pop()
        up.append(p)
    return lo[:-1] + up[:-1]


def reduce_hull(h, n):
    """Drop the vertex whose removal loses the least area until n remain."""
    h = list(h)
    while len(h) > n:
        best = min(range(len(h)), key=lambda i: abs(cross(h[i - 1], h[i], h[(i + 1) % len(h)])))
        h.pop(best)
    return h


def raster_iou(w, h, a, tris):
    inside = lambda p: any(inside_tri(p, *t) for t in tris)
    inter = uni = 0
    for y in range(h):
        for x in range(w):
            m = a[y][x] >= 0.5
            v = inside((x + 0.5, y + 0.5))
            inter += m and v
            uni += m or v
    return inter / max(uni, 1)


def main(root):
    assets = json.load(open(f"{root}/assets.json"))
    out = {}
    for name in SPRITES:
        sp = next(s for s in assets["sprites"].values() if s["name"] == name)
        tex = assets["textures"][sp["tex"]]
        w, h, a = read_png(f"{root}/{tex['file']}")
        loops = contours(w, h, a)
        rings = [simplify_loop(l, 0.25) for l in loops]  # within a quarter texel; halves the triangles
        # outer = the biggest ring (made CCW); the rest are holes (made CW)
        rings.sort(key=lambda r: -abs(area(r)))
        o = rings[0] if area(rings[0]) > 0 else rings[0][::-1]
        holes = [r if area(r) < 0 else r[::-1] for r in rings[1:]]
        tris = earclip(bridge(o, holes))
        iou = raster_iou(w, h, a, tris)
        rect, off, ppu = sp["rect"], sp["offset"], sp["ppu"]
        piv = (rect[2] / 2 + off[0], rect[3] / 2 + off[1])
        u = lambda p: [round((p[0] - piv[0]) / ppu, 5), round((p[1] - piv[1]) / ppu, 5)]
        hl = reduce_hull(hull(o), 16)
        out[name] = {
            "outline": [[u(p) for p in r] for r in [o] + holes],
            "tris": [[c for p in t for c in u(p)] for t in tris],
            "hull": [u(p) for p in hl],
            "ink": read_png.ink,
        }
        print(f"{name}: {len(o)} outer pts, {len(holes)} holes, {len(tris)} tris, hull {len(hl)}, IoU {iou:.4f}")
    # the engine reads them from assets.json; shapes.json is for inspection
    json.dump(out, open(f"{root}/shapes.json", "w"), separators=(",", ":"))
    assets["shapes"] = out
    json.dump(assets, open(f"{root}/assets.json", "w"), separators=(",", ":"))


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "web/assets")
