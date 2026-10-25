"""Minimal reader for Unity 4.3 serialized files (format v9, stripped type trees).

Only what Square Lights needs: object table, externals, and a byte reader with
Unity's 4-byte alignment rules. Object layouts are decoded by hand in extract.py.
"""
import struct

CLASS_NAMES = {
    1: "GameObject", 4: "Transform", 21: "Material", 23: "MeshRenderer",
    28: "Texture2D", 33: "MeshFilter", 43: "Mesh", 48: "Shader", 49: "TextAsset",
    65: "BoxCollider", 81: "AudioListener", 82: "AudioSource", 83: "AudioClip",
    20: "Camera", 102: "TextMesh", 114: "MonoBehaviour", 115: "MonoScript",
    128: "Font", 131: "GUITexture", 108: "Light", 136: "CapsuleCollider",
    54: "Rigidbody", 135: "SphereCollider", 64: "MeshCollider", 124: "Behaviour",
    104: "RenderSettings", 127: "LevelGameManager", 157: "LightmapSettings",
    196: "NavMeshSettings", 198: "ParticleSystem", 199: "ParticleSystemRenderer",
    92: "GUILayer", 120: "LineRenderer", 96: "TrailRenderer",
}


class Reader:
    def __init__(self, data, pos=0, little=True, long_ids=False):
        self.d = data
        self.p = pos
        self.e = "<" if little else ">"
        self.long_ids = long_ids  # Unity 5 (format >= 14): 64-bit path IDs

    def _u(self, fmt, n):
        v = struct.unpack_from(self.e + fmt, self.d, self.p)[0]
        self.p += n
        return v

    def u8(self): return self._u("B", 1)
    def i16(self): return self._u("h", 2)
    def u16(self): return self._u("H", 2)
    def i32(self): return self._u("i", 4)
    def i64(self): return self._u("q", 8)
    def u32(self): return self._u("I", 4)
    def f32(self): return self._u("f", 4)

    def boolean(self): return self.u8() != 0

    def align(self):
        self.p = (self.p + 3) & ~3

    def string(self):
        n = self.i32()
        s = self.d[self.p:self.p + n]
        self.p += n
        self.align()
        return s.decode("utf-8", "replace")

    def bytes_(self, n):
        b = self.d[self.p:self.p + n]
        self.p += n
        return b

    def cstr(self):
        end = self.d.index(b"\0", self.p)
        s = self.d[self.p:end].decode()
        self.p = end + 1
        return s

    def pptr(self): return (self.i32(), self.i64() if self.long_ids else self.i32())
    def vec2(self): return (self.f32(), self.f32())
    def vec3(self): return (self.f32(), self.f32(), self.f32())
    def quat(self): return (self.f32(), self.f32(), self.f32(), self.f32())
    def color(self): return (self.f32(), self.f32(), self.f32(), self.f32())


class SerializedFile:
    def __init__(self, path, name=None):
        with open(path, "rb") as fh:
            self.data = fh.read()
        self.name = name or path.rsplit("/", 1)[-1]
        r = Reader(self.data, little=False)
        self.metadata_size = r.u32()
        self.file_size = r.u32()
        self.version = r.u32()
        self.data_offset = r.u32()
        assert self.version in (9, 15), f"unsupported serialized version {self.version}"
        endian = r.u8()
        r.p += 3
        r.e = ">" if endian else "<"
        self.unity_version = r.cstr()
        self.platform = r.i32()
        self.long_ids = self.version >= 14
        if self.version >= 13:
            assert r.u8() == 0, "type trees present; this reader assumes stripped files"
        type_count = r.i32()
        self.types = []
        for _ in range(type_count):
            cid = r.i32()
            if cid < 0:
                r.bytes_(16)  # script id
            r.bytes_(16)      # old type hash
            self.types.append(cid)
        if self.version < 13:
            assert type_count == 0
            r.i32()  # padding / bigID flag
        count = r.i32()
        self.objects = {}
        for _ in range(count):
            if self.long_ids:
                r.align()
                path_id = r.i64()
            else:
                path_id = r.i32()
            start = r.u32()
            size = r.u32()
            type_id = r.i32()
            class_id = r.i16()
            r.i16()  # isDestroyed
            if self.version >= 15:
                r.u8()  # stripped
            self.objects[path_id] = (self.data_offset + start, size, class_id, type_id)
        if self.version >= 11:
            for _ in range(r.i32()):  # script types
                r.i32()
                if self.long_ids:
                    r.align()
                    r.i64()
                else:
                    r.i32()
        self.externals = []
        for _ in range(r.i32()):
            r.cstr()          # asset path (empty in players)
            r.bytes_(16)      # guid
            r.i32()           # type
            self.externals.append(r.cstr())
        self.little = r.e == "<"

    def reader(self, path_id):
        off, size, _, _ = self.objects[path_id]
        return Reader(self.data, off, self.little, self.long_ids)

    def raw(self, path_id):
        off, size, _, _ = self.objects[path_id]
        return self.data[off:off + size]

    def of_class(self, class_id):
        return [pid for pid, o in self.objects.items() if o[2] == class_id]
