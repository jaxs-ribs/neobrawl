"""Read a MonoBehaviour's serialized fields using the field list of its
decompiled C# class (Unity serializes public / [SerializeField] fields in
declaration order). Only the types NeoBrawl's scripts use are supported."""
import os
import re

PRIM = {"int": "i32", "float": "f32", "bool": "bool", "string": "string", "uint": "u32",
        "Vector2": "v2", "Vector3": "v3", "Vector4": "v4", "Quaternion": "v4", "Color": "color",
        "LayerMask": "u32", "Rect": "v4", "byte": "u8", "short": "i16", "long": "i64", "double": "f64"}
VALUE_STRUCTS = set(PRIM)
NOT_SERIALIZED = {"Dictionary", "Hashtable", "IEnumerator", "Coroutine", "RaycastHit", "Ray", "Action",
                  "PhotonView", "TextGenerator"}

_field = re.compile(r"^\s*(\[[^\]]*\]\s*)*(public|private|protected|internal)\s+(?!static|const|override|abstract|virtual|event|readonly|class|enum|struct|delegate)([\w<>\[\],\. ]+?)\s+(\w+)\s*(=[^;]*)?;")


def class_fields(src_dir, cls):
    path = os.path.join(src_dir, cls + ".cs")
    if not os.path.exists(path):
        return []
    lines = open(path, encoding="utf-8", errors="replace").read().split("\n")
    fields, attrs = [], []
    depth = 0
    for ln in lines:
        s = ln.strip()
        if depth == 1 and s.startswith("[") and s.endswith("]"):
            attrs.append(s)
            continue
        if depth == 1:
            m = _field.match(ln)
            if m:
                vis, typ, name = m.group(2), m.group(3).strip(), m.group(4)
                tag = " ".join(attrs)
                serialized = (vis == "public" and "NonSerialized" not in tag) or "SerializeField" in tag
                if serialized:
                    fields.append((typ, name))
            attrs = []
        depth += ln.count("{") - ln.count("}")
    return fields


def read_value(r, typ, enums):
    if typ.endswith("[][]"):
        raise ValueError("jagged")
    if typ.endswith("[]") or typ.startswith("List<"):
        inner = typ[:-2] if typ.endswith("[]") else typ[5:-1]
        n = r.i32()
        assert 0 <= n < 200000, f"bad array length {n} for {typ}"
        vals = [read_value(r, inner, enums) for _ in range(n)]
        if inner in ("bool", "byte"):
            r.align()
        return vals
    k = PRIM.get(typ)
    if k == "i32":
        return r.i32()
    if k == "u32":
        return r.u32()
    if k == "f32":
        return r.f32()
    if k == "bool":
        v = r.u8() != 0
        r.align()
        return v
    if k == "u8":
        v = r.u8()
        r.align()
        return v
    if k == "string":
        return r.string()
    if k == "v2":
        return r.vec2()
    if k == "v3":
        return r.vec3()
    if k in ("v4", "color"):
        return r.color()
    if typ in enums:
        return r.i32()
    # anything else is a UnityEngine.Object reference
    fid, pid = r.pptr()
    return {"$ref": [fid, pid]}


def read_script(r, src_dir, cls, enums, base_classes=()):
    out = {}
    for c in list(base_classes) + [cls]:
        for typ, name in class_fields(src_dir, c):
            base = typ.split("<")[0].replace("[]", "")
            if base in NOT_SERIALIZED or typ.endswith("[][]"):
                continue
            out[name] = read_value(r, typ, enums)
    return out


def enum_names(src_dir):
    names = set()
    for f in os.listdir(src_dir):
        if f.endswith(".cs"):
            txt = open(os.path.join(src_dir, f), encoding="utf-8", errors="replace").read()
            names.update(re.findall(r"\benum\s+(\w+)", txt))
    return names | {"KeyCode", "TouchPhase", "RigidbodyConstraints", "ForceMode", "PhotonTargets",
                    "ViewSynchronization", "OwnershipOption", "CloudRegionCode", "ConnectionProtocol",
                    "PhotonLogLevel", "DebugLevel"}
