"""Scene graph decoding for Unity 4.3 player data (GameObject, Transform, components)."""
import math
from sfile import SerializedFile


def quat_mul(a, b):
    ax, ay, az, aw = a
    bx, by, bz, bw = b
    return (aw * bx + ax * bw + ay * bz - az * by,
            aw * by - ax * bz + ay * bw + az * bx,
            aw * bz + ax * by - ay * bx + az * bw,
            aw * bw - ax * bx - ay * by - az * bz)


def quat_rot(q, v):
    x, y, z, w = q
    vx, vy, vz = v
    # v' = v + 2w(q x v) + 2 q x (q x v)
    cx = y * vz - z * vy
    cy = z * vx - x * vz
    cz = x * vy - y * vx
    ccx = y * cz - z * cy
    ccy = z * cx - x * cz
    ccz = x * cy - y * cx
    return (vx + 2 * (w * cx + ccx), vy + 2 * (w * cy + ccy), vz + 2 * (w * cz + ccz))


def quat_euler_z(q):
    x, y, z, w = q
    return math.degrees(math.atan2(2 * (w * z + x * y), 1 - 2 * (y * y + z * z)))


class Node:
    __slots__ = ("go", "tag", "name", "active", "layer", "components", "tf", "lpos", "lrot",
                 "lscale", "children", "parent", "wpos", "wrot", "wscale")


class Scene:
    def __init__(self, sf: SerializedFile, scripts=None):
        self.sf = sf
        self.scripts = scripts or {}
        self.nodes = {}
        self.by_tf = {}
        for pid in sf.of_class(1):
            r = sf.reader(pid)
            n = Node()
            n.go = pid
            n.components = []
            for _ in range(r.i32()):
                cid = r.i32()
                _, cpid = r.pptr()
                n.components.append((cid, cpid))
            n.layer = r.u32()
            n.name = r.string()
            n.tag = r.u16()
            n.active = r.boolean()
            n.tf = next(c for cid, c in n.components if cid in (4, 224))
            self.nodes[pid] = n
        for n in self.nodes.values():
            r = sf.reader(n.tf)
            r.pptr()
            n.lrot = r.quat()
            n.lpos = r.vec3()
            n.lscale = r.vec3()
            n.children = [r.pptr()[1] for _ in range(r.i32())]
            n.parent = r.pptr()[1]
            self.by_tf[n.tf] = n
        for n in self.nodes.values():
            n.children = [self.by_tf[c] for c in n.children]
            n.parent = self.by_tf.get(n.parent)
        for n in self.roots():
            self._world(n, (0, 0, 0), (0, 0, 0, 1), (1, 1, 1))

    def _world(self, n, ppos, prot, pscale):
        sp = (n.lpos[0] * pscale[0], n.lpos[1] * pscale[1], n.lpos[2] * pscale[2])
        rp = quat_rot(prot, sp)
        n.wpos = (ppos[0] + rp[0], ppos[1] + rp[1], ppos[2] + rp[2])
        n.wrot = quat_mul(prot, n.lrot)
        n.wscale = (pscale[0] * n.lscale[0], pscale[1] * n.lscale[1], pscale[2] * n.lscale[2])
        for c in n.children:
            self._world(c, n.wpos, n.wrot, n.wscale)

    def roots(self):
        return [n for n in self.nodes.values() if n.parent is None]

    def find(self, name):
        return [n for n in self.nodes.values() if n.name == name]

    def comp(self, n, class_id):
        return [pid for cid, pid in n.components if cid == class_id]

    def script_of(self, mb_pid):
        r = self.sf.reader(mb_pid)
        r.pptr()
        r.u8(); r.align()
        fid, spid = r.pptr()
        return self.scripts.get((self.sf.externals[fid - 1] if fid else self.sf.name, spid), "?")

    def behaviours(self, n):
        return [(self.script_of(p), p) for p in self.comp(n, 114)]

    def mb_fields(self, mb_pid):
        """Reader positioned at the first serialized script field."""
        r = self.sf.reader(mb_pid)
        r.pptr(); r.u8(); r.align(); r.pptr(); r.string()
        return r


def load_scripts(sf: SerializedFile):
    out = {}
    for pid in sf.of_class(115):
        out[(sf.name, pid)] = sf.reader(pid).string()
    return out
