"""Extract NeoBrawl (Unity 5.3.4, Android) into web-friendly files.

usage: python3 -I extract5.py <merged Data dir> <decompiled Assembly-CSharp dir> <audio mp3 dir> <out dir>
"""
import json
import os
import shutil
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from sfile import SerializedFile  # noqa: E402
from scene import Scene  # noqa: E402
import decode  # noqa: E402
import csfields  # noqa: E402

SRC, CS, AUDIO, OUT = sys.argv[1:5]
SHARED = {"sharedassets0.assets": "sa0", "globalgamemanagers.assets": "gga"}


def key_of(sf, pptr):
    fid, pid = pptr
    if pid == 0:
        return None
    fname = sf.name if fid == 0 else sf.externals[fid - 1]
    short = SHARED.get(fname)
    if short:
        return f"{short}:{pid}"
    if "default resources" in fname or "builtin_extra" in fname:
        return f"builtin:{pid}"
    return f"{fname}:{pid}"


def unity_event(r):
    calls = []
    for _ in range(r.i32()):
        target = r.pptr()
        method = r.string()
        mode = r.i32()
        obj = r.pptr()
        r.string()  # object argument type
        iarg = r.i32()
        farg = r.f32()
        sarg = r.string()
        barg = r.u8(); r.align()
        r.i32()  # call state
        calls.append(dict(target=target, method=method, mode=mode, obj=obj, i=iarg, f=farg, s=sarg, b=barg))
    r.string()  # type name
    return calls


def renderer_base(r):
    """Unity 5.3 Renderer fields up to (and including) sorting order."""
    r.pptr()
    enabled = r.u8() == 1
    r.bytes_(3)
    r.bytes_(4)          # lightmap index + dynamic
    r.bytes_(4)
    r.color(); r.color()  # tiling offsets
    mats = [r.pptr() for _ in range(r.i32())]
    for _ in range(r.i32()):
        r.u32()
    r.pptr()             # static batch root
    r.i32(); r.i32()     # light probes, reflection probes
    r.pptr()             # probe anchor
    r.i32()              # sorting layer id
    order = r.i16(); r.align()
    return enabled, mats, order


def main():
    for d in ("tex", "audio", "fonts"):
        os.makedirs(f"{OUT}/{d}", exist_ok=True)
    sa = SerializedFile(f"{SRC}/sharedassets0.assets")
    gga = SerializedFile(f"{SRC}/globalgamemanagers.assets")
    ggm = SerializedFile(f"{SRC}/globalgamemanagers")
    level = SerializedFile(f"{SRC}/level0")
    scripts = {("globalgamemanagers.assets", pid): gga.reader(pid).string() for pid in gga.of_class(115)}

    # --- tags (TagManager, class 78)
    tags = {0: "Untagged", 1: "Respawn", 2: "Finish", 3: "EditorOnly", 5: "MainCamera", 6: "Player", 7: "GameController"}
    tm = ggm.of_class(78)[0]
    r = ggm.reader(tm)
    for i in range(r.i32()):
        tags[20000 + i] = r.string()
    layers = [r.string() for _ in range(32)] if False else []

    # --- assets
    textures, sprites, meshes, materials, fonts, audio = {}, {}, {}, {}, {}, {}
    for pid, (_, _, cid, _) in sa.objects.items():
        key = f"sa0:{pid}"
        r = sa.reader(pid)
        if cid == 28:
            t = decode.texture2d_v5(r)
            if not t["w"]:
                continue
            px = decode.decode_pixels_v5(t["fmt"], t["w"], t["h"], t["data"])
            fn = f"tex/{t['name']}.png"
            decode.write_png(f"{OUT}/{fn}", t["w"], t["h"], decode.flip_rows(px, t["w"], t["h"]))
            textures[key] = dict(name=t["name"], file=fn, w=t["w"], h=t["h"])
        elif cid == 213:
            name = r.string()
            rect = (r.f32(), r.f32(), r.f32(), r.f32())
            offset = r.vec2()
            r.color()  # border
            ppu = r.f32()
            r.u32(); r.u8(); r.align()
            tex = key_of(sa, r.pptr())
            r.pptr()
            nv = r.i32(); r.bytes_(12 * nv)
            ni = r.i32(); r.bytes_(2 * ni); r.align()
            trect = (r.f32(), r.f32(), r.f32(), r.f32())
            troff = r.vec2()
            sprites[key] = dict(name=name, rect=rect, offset=offset, ppu=ppu, tex=tex, texRect=trect, texRectOffset=troff)
        elif cid == 43:
            meshes[key] = mesh_v5(r)
        elif cid == 21:
            materials[key] = material_v5(r, sa)
        elif cid == 128:
            name = r.string()
            ttf = decode.font_ttf(sa.raw(pid))
            if ttf:
                fn = f"fonts/{name}.ttf"
                open(f"{OUT}/{fn}", "wb").write(ttf)
                fonts[key] = dict(name=name, file=fn)
        elif cid == 83:
            name = r.string()
            clean = name.lstrip("_")
            src = f"{AUDIO}/{clean}.mp3"
            if os.path.exists(src):
                shutil.copy(src, f"{OUT}/audio/{clean}.mp3")
                audio[key] = dict(name=clean, file=f"audio/{clean}.mp3")
        elif cid == 134:
            name = r.string()
            materials[key] = dict(name=name, physic=True, dynFriction=r.f32(), staticFriction=r.f32(), bounciness=r.f32())

    # --- scene
    sc = Scene(level, scripts)
    enums = csfields.enum_names(CS)
    index = {n.go: i for i, n in enumerate(sc.nodes.values())}
    ui_classes = {"Image", "Text", "Button", "CanvasScaler", "GraphicRaycaster", "EventSystem",
                  "StandaloneInputModule", "LetterSpacing", "PhotonView"}
    nodes = []
    for n in sc.nodes.values():
        d = dict(name=n.name, active=n.active, tag=tags.get(n.tag, str(n.tag)), layer=n.layer,
                 parent=index[n.parent.go] if n.parent else -1,
                 pos=n.lpos, rot=n.lrot, scale=n.lscale, scripts={}, children=len(n.children))
        for cid, pid in n.components:
            r = level.reader(pid)
            if cid == 224:
                r.pptr(); r.quat(); r.vec3(); r.vec3()
                for _ in range(r.i32()):
                    r.pptr()
                r.pptr()
                d["rect"] = dict(anchorMin=r.vec2(), anchorMax=r.vec2(), pos=r.vec2(), size=r.vec2(), pivot=r.vec2())
            elif cid == 212:
                en, mats, order = renderer_base(r)
                spr = key_of(level, r.pptr())
                d["sprite"] = dict(enabled=en, sprite=spr, color=r.color(), order=order,
                                   flipX=r.u8() == 1, flipY=r.u8() == 1)
            elif cid == 23:
                en, mats, order = renderer_base(r)
                d["meshRenderer"] = dict(enabled=en, materials=[key_of(level, m) for m in mats])
            elif cid == 33:
                r.pptr()
                d["mesh"] = key_of(level, r.pptr())
            elif cid == 120:
                en, mats, order = renderer_base(r)
                pts = [r.vec3() for _ in range(r.i32())]
                d["line"] = dict(enabled=en, materials=[key_of(level, m) for m in mats], points=pts,
                                 width=(r.f32(), r.f32()), colors=(r.u32(), r.u32()), world=r.u8() == 1)
            elif cid == 96:
                en, mats, order = renderer_base(r)
                d["trail"] = dict(enabled=en, materials=[key_of(level, m) for m in mats], time=r.f32(),
                                  width=(r.f32(), r.f32()))
            elif cid == 65:
                r.pptr(); mat = r.pptr(); trig = r.u8() == 1; en = r.u8() == 1; r.align()
                d["box"] = dict(enabled=en, trigger=trig, size=r.vec3(), center=r.vec3(), material=key_of(level, mat))
            elif cid == 136:
                r.pptr(); mat = r.pptr(); trig = r.u8() == 1; en = r.u8() == 1; r.align()
                d["capsule"] = dict(enabled=en, trigger=trig, radius=r.f32(), height=r.f32(), direction=r.i32(),
                                    center=r.vec3(), material=key_of(level, mat))
            elif cid == 54:
                r.pptr()
                mass, drag, adrag = r.f32(), r.f32(), r.f32()
                grav, kin = r.u8() == 1, r.u8() == 1
                r.u8(); r.align()
                d["rigidbody"] = dict(mass=mass, drag=drag, angularDrag=adrag, gravity=grav, kinematic=kin,
                                      constraints=r.i32(), detection=r.i32())
            elif cid == 20:
                r.pptr(); en = r.u8() == 1; r.align()
                r.u32(); bg = r.color(); vp = r.color()
                near, far, fov = r.f32(), r.f32(), r.f32()
                ortho = r.u8() == 1; r.align()
                size = r.f32(); depth = r.f32()
                d["camera"] = dict(enabled=en, bg=bg, viewport=vp, near=near, far=far, fov=fov, ortho=ortho,
                                   orthoSize=size, depth=depth)
            elif cid == 82:
                d["audioSource"] = audio_source(r, level)
            elif cid == 223:
                r.pptr(); en = r.u8() == 1; r.align()
                mode = r.i32(); cam = r.pptr(); plane = r.f32()
                d["canvas"] = dict(enabled=en, mode=mode, camera=f"obj:{cam[1]}" if cam[1] else None, planeDistance=plane)
            elif cid == 114:
                r.pptr(); en = r.u8() == 1; r.align()
                script = sc.script_of(pid)
                r.pptr(); r.string()
                try:
                    d["scripts"][script] = dict(enabled=en, **read_component(r, script, level, enums))
                except Exception as e:  # keep going; report what failed
                    d["scripts"][script] = dict(enabled=en, error=str(e))
        nodes.append(d)

    json.dump(dict(textures=textures, sprites=sprites, meshes=meshes, materials=materials, fonts=fonts,
                   audio=audio), open(f"{OUT}/assets.json", "w"))
    objects = {}
    for n in sc.nodes.values():
        objects[str(n.go)] = index[n.go]
        for cid, pid in n.components:
            objects[str(pid)] = index[n.go]
    child_order = {str(index[n.go]): [index[c.go] for c in n.children] for n in sc.nodes.values() if n.children}
    json.dump(dict(nodes=nodes, objects=objects, childOrder=child_order), open(f"{OUT}/scene.json", "w"))
    errors = [(n["name"], s, v["error"]) for n in nodes for s, v in n["scripts"].items() if "error" in v]
    print(f"textures={len(textures)} sprites={len(sprites)} meshes={len(meshes)} materials={len(materials)} "
          f"fonts={len(fonts)} audio={len(audio)} nodes={len(nodes)} script-errors={len(errors)}")
    for e in errors[:20]:
        print("  ", e)


def read_component(r, script, sf, enums):
    if script in ("Image", "Text"):
        out = dict(material=key_of(sf, r.pptr()), color=r.color())
        out["raycast"] = r.u8() == 1; r.align()
        unity_event(r)                    # onCullStateChanged
        if script == "Image":
            out.update(sprite=key_of(sf, r.pptr()), type=r.i32())
            r.u8(); r.align(); r.u8(); r.align()
            out.update(fillMethod=r.i32(), fillAmount=r.f32())
            out["fillClockwise"] = r.u8() == 1; r.align()
            out["fillOrigin"] = r.i32()
        else:
            out["font"] = key_of(sf, r.pptr())
            out.update(fontSize=r.i32(), fontStyle=r.i32())
            r.u8(); r.align()
            r.i32(); r.i32()
            out["alignment"] = r.i32()
            r.u8(); r.align(); r.u8(); r.align()
            out.update(hOverflow=r.i32(), vOverflow=r.i32(), lineSpacing=r.f32(), text=r.string())
        return out
    if script == "Button":
        r.i32()
        for _ in range(4):
            r.pptr()
        transition = r.i32()
        normal = r.color()
        for _ in range(3):
            r.color()
        mult = r.f32(); r.f32()
        for _ in range(3):
            r.pptr()
        for _ in range(4):
            r.string()
        r.u8(); r.align()
        tg = r.pptr()
        calls = unity_event(r)
        ref = lambda p: (f"obj:{p[1]}" if p[0] == 0 else key_of(sf, p)) if p[1] else None
        return dict(onClick=[dict(target=ref(c["target"]), method=c["method"], mode=c["mode"],
                                  obj=ref(c["obj"]), i=c["i"], s=c["s"]) for c in calls],
                    transition=transition, tint=[min(1.0, x * mult) for x in normal], targetGraphic=ref(tg))
    if script == "CanvasScaler":
        mode, refppu, sf_ = r.i32(), r.f32(), r.f32()
        return dict(mode=mode, scaleFactor=sf_, refResolution=r.vec2(), matchMode=r.i32(), match=r.f32())
    if script == "LetterSpacing":
        return dict(spacing=r.f32())
    if script in ("GraphicRaycaster", "EventSystem", "StandaloneInputModule", "PhotonView",
                  "PhotonTransformView", "PhotonAnimatorView", "PhotonRigidbodyView", "ServerSettings"):
        return {}
    bases = []
    vals = csfields.read_script(r, CS, script, enums, bases)
    return dict(fields=_keys(vals, sf))


def _keys(v, sf):
    """Convert {"$ref": [fid, pid]} references to asset keys / scene object ids."""
    if isinstance(v, dict) and "$ref" in v:
        fid, pid = v["$ref"]
        if pid == 0:
            return None
        return key_of(sf, (fid, pid)) if fid else f"obj:{pid}"
    if isinstance(v, dict):
        return {k: _keys(x, sf) for k, x in v.items()}
    if isinstance(v, list):
        return [_keys(x, sf) for x in v]
    if isinstance(v, tuple):
        return list(v)
    return v


def audio_source(r, sf):
    r.pptr(); en = r.u8() == 1; r.align()
    r.pptr()  # output mixer group
    clip = key_of(sf, r.pptr())
    r.pptr()  # play on awake? (5.3: m_PlayOnAwake comes after)
    return dict(enabled=en, clip=clip)


def mesh_v5(r):
    name = r.string()
    subs = []
    for _ in range(r.i32()):
        first_byte, index_count, topology, first_vertex, vertex_count = r.u32(), r.u32(), r.i32(), r.u32(), r.u32()
        r.vec3(); r.vec3()
        subs.append((first_byte, index_count))
    for _ in range(4):
        assert r.i32() == 0
    assert r.i32() == 0
    assert r.i32() == 0
    r.u32()
    r.bytes_(4)
    ib = r.bytes_(r.i32()); r.align()
    assert r.i32() == 0
    r.u32()
    vcount = r.u32()
    channels = [tuple(r.bytes_(4)) for _ in range(r.i32())]
    vdata = r.bytes_(r.i32())
    stride = 0
    for (stream, off, fmt, dim) in channels:
        if dim:
            stride = max(stride, off + 4 * dim)
    pos, uv = [], []
    for v in range(vcount):
        s, off, fmt, dim = channels[0]
        pos.append(struct.unpack_from("<3f", vdata, v * stride + off))
        if len(channels) > 3 and channels[3][3]:
            uv.append(struct.unpack_from("<2f", vdata, v * stride + channels[3][1]))
    indices = []
    for fb, cnt in subs:
        indices += list(struct.unpack_from("<%dH" % cnt, ib, fb))
    return dict(name=name, pos=pos, uv=uv or [(0, 0)] * vcount, indices=indices)


def material_v5(r, sf):
    name = r.string()
    shader = r.pptr()
    r.string()            # keywords
    r.u32(); r.i32()      # lightmap flags, render queue
    for _ in range(r.i32()):  # string tag map
        r.string(); r.string()
    tex = None
    for _ in range(r.i32()):
        k = r.string()
        t = r.pptr(); r.vec2(); r.vec2()
        if k == "_MainTex":
            tex = key_of(sf, t)
    floats = {}
    for _ in range(r.i32()):
        k = r.string(); floats[k] = r.f32()
    colors = {}
    for _ in range(r.i32()):
        k = r.string(); colors[k] = r.color()
    return dict(name=name, shader=key_of(sf, shader), tex=tex, color=colors.get("_Color") or colors.get("_TintColor"),
                floats=floats)


main()
