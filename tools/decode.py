"""Asset decoders: Texture2D pixels, Mesh, Material, AudioClip, Font, and a tiny PNG writer."""
import struct
import zlib

from sfile import Reader

# --- PNG -------------------------------------------------------------------


def write_png(path, w, h, rgba_top_down):
    def chunk(tag, data):
        c = struct.pack(">I", len(data)) + tag + data
        return c + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
    rows = b"".join(b"\0" + rgba_top_down[y * w * 4:(y + 1) * w * 4] for y in range(h))
    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b"")
    with open(path, "wb") as fh:
        fh.write(png)


# --- ETC1 ------------------------------------------------------------------

_ETC1_TABLES = [(2, 8), (5, 17), (9, 29), (13, 42), (18, 60), (24, 80), (33, 106), (47, 183)]


def _clamp(v):
    return 0 if v < 0 else 255 if v > 255 else v


def _etc1_block(block):
    hi, lo = struct.unpack(">II", block)
    diff = (hi >> 1) & 1
    flip = hi & 1
    if diff:
        def ext5(v): return (v << 3) | (v >> 2)
        r1, g1, b1 = (hi >> 27) & 31, (hi >> 19) & 31, (hi >> 11) & 31
        dr, dg, db = (hi >> 24) & 7, (hi >> 16) & 7, (hi >> 8) & 7
        dr, dg, db = [d - 8 if d >= 4 else d for d in (dr, dg, db)]
        base = [(ext5(r1), ext5(g1), ext5(b1)), (ext5(r1 + dr), ext5(g1 + dg), ext5(b1 + db))]
    else:
        def ext4(v): return (v << 4) | v
        base = [(ext4((hi >> 28) & 15), ext4((hi >> 20) & 15), ext4((hi >> 12) & 15)),
                (ext4((hi >> 24) & 15), ext4((hi >> 16) & 15), ext4((hi >> 8) & 15))]
    tables = [(hi >> 5) & 7, (hi >> 2) & 7]
    out = [[None] * 4 for _ in range(4)]  # out[y][x]
    for x in range(4):
        for y in range(4):
            i = x * 4 + y
            sub = (y >= 2) if flip else (x >= 2)
            msb = (lo >> (16 + i)) & 1
            lsb = (lo >> i) & 1
            small, big = _ETC1_TABLES[tables[sub]]
            mod = [small, big, -small, -big][(msb << 1) | lsb]
            r, g, b = base[sub]
            out[y][x] = (_clamp(r + mod), _clamp(g + mod), _clamp(b + mod), 255)
    return out


def decode_pixels(fmt, w, h, data):
    """Returns RGBA bytes in Unity's row order (bottom row first)."""
    px = bytearray(w * h * 4)
    if fmt == 13:  # RGBA4444
        for i in range(w * h):
            v = data[2 * i] | (data[2 * i + 1] << 8)
            px[4 * i:4 * i + 4] = bytes((((v >> 12) & 15) * 17, ((v >> 8) & 15) * 17,
                                         ((v >> 4) & 15) * 17, (v & 15) * 17))
    elif fmt == 3:  # RGB24
        for i in range(w * h):
            px[4 * i:4 * i + 4] = data[3 * i:3 * i + 3] + b"\xff"
    elif fmt == 4:  # RGBA32
        px[:] = data[:w * h * 4]
    elif fmt == 34:  # ETC1, blocks in raster order
        bw = (w + 3) // 4
        for bi in range((w // 4) * (h // 4)):
            bx, by = bi % bw, bi // bw
            blk = _etc1_block(data[bi * 8:bi * 8 + 8])
            for y in range(4):
                for x in range(4):
                    o = ((by * 4 + y) * w + bx * 4 + x) * 4
                    px[o:o + 4] = bytes(blk[y][x])
    else:
        raise ValueError(f"texture format {fmt} not supported")
    return bytes(px)


def flip_rows(px, w, h):
    stride = w * 4
    return b"".join(px[(h - 1 - y) * stride:(h - y) * stride] for y in range(h))


# --- object layouts (Unity 4.3, stripped type trees) -------------------------


def texture2d(r: Reader):
    name = r.string()
    w, h, size, fmt = r.i32(), r.i32(), r.i32(), r.i32()
    r.bytes_(4)  # mipmap, readable, readAllowed, pad
    for _ in range(8):
        r.i32()  # imageCount, dim, filter, aniso, mipBias, wrap, lightmapFmt, colorSpace
    n = r.i32()
    data = r.bytes_(n)
    return dict(name=name, w=w, h=h, fmt=fmt, data=data)


def mesh(r: Reader):
    name = r.string()
    subs = []
    for _ in range(r.i32()):
        first_byte, index_count, strip = r.u32(), r.u32(), r.i32()
        first_vertex, vertex_count = r.u32(), r.u32()
        r.vec3(); r.vec3()
        subs.append((first_byte, index_count, strip))
    for _ in range(4):  # blend shape vectors
        assert r.i32() == 0
    assert r.i32() == 0  # bind poses
    assert r.i32() == 0  # bone name hashes
    r.u32()  # root bone hash
    r.bytes_(5); r.align()
    ib = r.bytes_(r.i32()); r.align()
    assert r.i32() == 0  # skin
    r.u32()  # current channels
    vcount = r.u32()
    channels = [tuple(r.bytes_(4)) for _ in range(r.i32())]
    streams = []
    for _ in range(r.i32()):
        streams.append((r.u32(), r.u32(), r.u8(), r.u8(), r.u16()))
    vdata = r.bytes_(r.i32())
    pos, uv = [], []
    for v in range(vcount):
        for ci, (stream, offset, fmt, dim) in enumerate(channels):
            if dim == 0 or ci not in (0, 3):
                continue
            mask, soff, stride, _, _ = streams[stream]
            o = soff + v * stride + offset
            vals = struct.unpack_from("<%df" % dim, vdata, o)
            (pos if ci == 0 else uv).append(vals)
    indices = []
    for first_byte, count, strip in subs:
        idx = list(struct.unpack_from("<%dH" % count, ib, first_byte))
        if strip:
            tri = []
            for i in range(len(idx) - 2):
                a, b, c = idx[i], idx[i + 1], idx[i + 2]
                if a == b or b == c or a == c:
                    continue
                tri += [a, b, c] if i % 2 == 0 else [b, a, c]
            idx = tri
        indices += idx
    return dict(name=name, pos=pos, uv=uv or [(0, 0)] * len(pos), indices=indices)


def material(r: Reader):
    name = r.string()
    shader = r.pptr()
    for _ in range(r.i32()):
        r.string()
    r.i32()  # custom render queue
    texenvs = {}
    for _ in range(r.i32()):
        key = r.string()
        tex = r.pptr()
        texenvs[key] = dict(tex=tex, scale=r.vec2(), offset=r.vec2())
    floats = {}
    for _ in range(r.i32()):
        key = r.string()
        floats[key] = r.f32()
    colors = {}
    for _ in range(r.i32()):
        key = r.string()
        colors[key] = r.color()
    return dict(name=name, shader=shader, tex=texenvs, floats=floats, colors=colors)


def audioclip(r: Reader):
    name = r.string()
    fmt, typ = r.i32(), r.i32()
    r.bytes_(2); r.align()
    r.i32()  # stream mode
    data = r.bytes_(r.i32())
    return dict(name=name, format=fmt, type=typ, data=data)


def sniff_audio(data):
    if data[:4] == b"OggS":
        return "ogg"
    if data[:3] == b"ID3" or (data[0] == 0xFF and (data[1] & 0xE0) == 0xE0):
        return "mp3"
    if data[:4] == b"RIFF":
        return "wav"
    return "bin"


def font_ttf(raw):
    """The embedded font file (TrueType or CFF OpenType): the earliest header
    whose preceding int32 holds a plausible length and whose table directory
    has real 4-character tags."""
    best = None
    for sig in (b"\x00\x01\x00\x00", b"OTTO", b"true"):
        i = raw.find(sig)
        while i >= 4:
            nt = struct.unpack_from(">H", raw, i + 4)[0]
            n = struct.unpack_from("<i", raw, i - 4)[0]
            ok = 4 <= nt <= 40 and 0 < n <= len(raw) - i and len(raw) >= i + 12 + 16 * nt
            if ok:
                for k in range(nt):
                    tag = raw[i + 12 + 16 * k:i + 16 + 16 * k]
                    if not all(32 <= c < 127 for c in tag):
                        ok = False
                        break
            if ok and (best is None or i < best[0]):
                best = (i, n)
            i = raw.find(sig, i + 1)
    return raw[best[0]:best[0] + best[1]] if best else None


# --- ETC2 RGB / EAC alpha (Unity format 47 = ETC2_RGBA8) ---------------------

_ETC2_DIST = [3, 6, 11, 16, 23, 32, 41, 64]
_EAC_TABLES = [
    (-3, -6, -9, -15, 2, 5, 8, 14), (-3, -7, -10, -13, 2, 6, 9, 12), (-2, -5, -8, -13, 1, 4, 7, 12),
    (-2, -4, -6, -13, 1, 3, 5, 12), (-3, -6, -8, -12, 2, 5, 7, 11), (-3, -7, -9, -11, 2, 6, 8, 10),
    (-4, -7, -8, -11, 3, 6, 7, 10), (-3, -5, -8, -11, 2, 4, 7, 10), (-2, -6, -8, -10, 1, 5, 7, 9),
    (-2, -5, -8, -10, 1, 4, 7, 9), (-2, -4, -8, -10, 1, 3, 7, 9), (-2, -5, -7, -10, 1, 4, 6, 9),
    (-3, -4, -7, -10, 2, 3, 6, 9), (-1, -2, -3, -10, 0, 1, 2, 9), (-4, -6, -8, -9, 3, 5, 7, 8),
    (-3, -5, -7, -9, 2, 4, 6, 8),
]


def _etc2_rgb_block(block):
    hi, lo = struct.unpack(">II", block)
    diff = (hi >> 1) & 1
    if not diff:
        return _etc1_block(block)
    r, dr = (hi >> 27) & 31, (hi >> 24) & 7
    g, dg = (hi >> 19) & 31, (hi >> 16) & 7
    b, db = (hi >> 11) & 31, (hi >> 8) & 7
    sdr, sdg, sdb = [d - 8 if d >= 4 else d for d in (dr, dg, db)]
    out = [[None] * 4 for _ in range(4)]
    if r + sdr < 0 or r + sdr > 31:  # T mode
        r1 = (((hi >> 27) & 3) << 2) | ((hi >> 24) & 3)
        g1, b1 = (hi >> 20) & 15, (hi >> 16) & 15
        r2, g2, b2 = (hi >> 12) & 15, (hi >> 8) & 15, (hi >> 4) & 15
        da = (((hi >> 2) & 3) << 1) | (hi & 1)
        e4 = lambda v: v * 17
        c1 = (e4(r1), e4(g1), e4(b1))
        c2 = (e4(r2), e4(g2), e4(b2))
        d = _ETC2_DIST[da]
        pal = [c1, tuple(_clamp(c + d) for c in c2), c2, tuple(_clamp(c - d) for c in c2)]
    elif g + sdg < 0 or g + sdg > 31:  # H mode
        r1 = (hi >> 27) & 15
        g1 = (((hi >> 24) & 7) << 1) | ((hi >> 20) & 1)
        b1 = (((hi >> 19) & 1) << 3) | ((hi >> 15) & 7)
        r2, g2, b2 = (hi >> 11) & 15, (hi >> 7) & 15, (hi >> 3) & 15
        e4 = lambda v: v * 17
        c1 = (e4(r1), e4(g1), e4(b1))
        c2 = (e4(r2), e4(g2), e4(b2))
        v1 = (c1[0] << 16) | (c1[1] << 8) | c1[2]
        v2 = (c2[0] << 16) | (c2[1] << 8) | c2[2]
        da = ((hi >> 2) & 1) << 2 | (hi & 1) << 1 | (1 if v1 >= v2 else 0)
        d = _ETC2_DIST[da]
        pal = [tuple(_clamp(c + d) for c in c1), tuple(_clamp(c - d) for c in c1),
               tuple(_clamp(c + d) for c in c2), tuple(_clamp(c - d) for c in c2)]
    elif b + sdb < 0 or b + sdb > 31:  # planar
        bits = (hi << 32) | lo
        ro = (bits >> 57) & 63
        go = (((bits >> 56) & 1) << 6) | ((bits >> 49) & 63)
        bo = (((bits >> 48) & 1) << 5) | (((bits >> 43) & 3) << 3) | ((bits >> 39) & 7)
        rh = (((bits >> 34) & 31) << 1) | ((bits >> 32) & 1)
        gh = (bits >> 25) & 127
        bh = (bits >> 19) & 63
        rv = (bits >> 13) & 63
        gv = (bits >> 6) & 127
        bv = bits & 63
        e6 = lambda v: (v << 2) | (v >> 4)
        e7 = lambda v: (v << 1) | (v >> 6)
        O = (e6(ro), e7(go), e6(bo))
        H = (e6(rh), e7(gh), e6(bh))
        V = (e6(rv), e7(gv), e6(bv))
        for y in range(4):
            for x in range(4):
                out[y][x] = tuple(_clamp((x * (H[i] - O[i]) + y * (V[i] - O[i]) + 4 * O[i] + 2) >> 2) for i in range(3)) + (255,)
        return out
    else:
        return _etc1_block(block)
    for x in range(4):
        for y in range(4):
            i = x * 4 + y
            idx = (((lo >> (16 + i)) & 1) << 1) | ((lo >> i) & 1)
            out[y][x] = pal[idx] + (255,)
    return out


def _eac_alpha_block(block):
    bits = int.from_bytes(block, "big")
    base = (bits >> 56) & 255
    mul = (bits >> 52) & 15
    table = _EAC_TABLES[(bits >> 48) & 15]
    out = [[0] * 4 for _ in range(4)]
    for x in range(4):
        for y in range(4):
            i = x * 4 + y
            idx = (bits >> (45 - 3 * i)) & 7
            out[y][x] = _clamp(base + table[idx] * mul)
    return out


def decode_etc2_rgba8(w, h, data):
    px = bytearray(w * h * 4)
    bw = (w + 3) // 4
    for bi in range(((w + 3) // 4) * ((h + 3) // 4)):
        bx, by = bi % bw, bi // bw
        a = _eac_alpha_block(data[bi * 16:bi * 16 + 8])
        c = _etc2_rgb_block(data[bi * 16 + 8:bi * 16 + 16])
        for y in range(4):
            for x in range(4):
                X, Y = bx * 4 + x, by * 4 + y
                if X < w and Y < h:
                    o = (Y * w + X) * 4
                    px[o:o + 4] = bytes(c[y][x][:3]) + bytes((a[y][x],))
    return bytes(px)


def texture2d_v5(r):
    """Unity 5.3 Texture2D: like 4.3 plus a mip count after the format."""
    name = r.string()
    w, h, size, fmt = r.i32(), r.i32(), r.i32(), r.i32()
    r.i32()  # mip count
    r.bytes_(4)
    for _ in range(8):
        r.i32()
    data = r.bytes_(r.i32())
    return dict(name=name, w=w, h=h, fmt=fmt, data=data)


def decode_pixels_v5(fmt, w, h, data):
    if fmt == 47:
        return decode_etc2_rgba8(w, h, data)
    if fmt == 5:  # ARGB32
        px = bytearray(w * h * 4)
        for i in range(w * h):
            a, r_, g, b = data[4 * i:4 * i + 4]
            px[4 * i:4 * i + 4] = bytes((r_, g, b, a))
        return bytes(px)
    return decode_pixels(fmt, w, h, data)
