//! Just enough linear algebra to mirror Unity's Transform semantics.

use std::ops::{Add, AddAssign, Mul, Neg, Sub};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct V3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

pub const fn v3(x: f32, y: f32, z: f32) -> V3 {
    V3 { x, y, z }
}

impl V3 {
    pub const ZERO: V3 = v3(0.0, 0.0, 0.0);
    pub const ONE: V3 = v3(1.0, 1.0, 1.0);

    pub fn mul_elem(self, o: V3) -> V3 {
        v3(self.x * o.x, self.y * o.y, self.z * o.z)
    }

    pub fn length(self) -> f32 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }

    pub fn from_slice(s: &[f32]) -> V3 {
        v3(s[0], s[1], s[2])
    }

    /// Unity's Vector3 == compares with a 1e-5 squared-distance tolerance.
    pub fn approx_eq(self, o: V3) -> bool {
        let d = self - o;
        d.x * d.x + d.y * d.y + d.z * d.z < 9.999_999e-11
    }
}

impl Add for V3 {
    type Output = V3;
    fn add(self, o: V3) -> V3 {
        v3(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

impl AddAssign for V3 {
    fn add_assign(&mut self, o: V3) {
        *self = *self + o;
    }
}

impl Sub for V3 {
    type Output = V3;
    fn sub(self, o: V3) -> V3 {
        v3(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}

impl Mul<f32> for V3 {
    type Output = V3;
    fn mul(self, s: f32) -> V3 {
        v3(self.x * s, self.y * s, self.z * s)
    }
}

impl Neg for V3 {
    type Output = V3;
    fn neg(self) -> V3 {
        v3(-self.x, -self.y, -self.z)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quat {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

impl Default for Quat {
    fn default() -> Quat {
        Quat::IDENTITY
    }
}

impl Quat {
    pub const IDENTITY: Quat = Quat { x: 0.0, y: 0.0, z: 0.0, w: 1.0 };

    pub fn from_slice(s: &[f32]) -> Quat {
        Quat { x: s[0], y: s[1], z: s[2], w: s[3] }
    }

    fn axis_angle(axis: V3, deg: f32) -> Quat {
        let h = deg.to_radians() * 0.5;
        let s = h.sin();
        Quat { x: axis.x * s, y: axis.y * s, z: axis.z * s, w: h.cos() }
    }

    /// Unity's Quaternion.Euler: rotates around Z, then X, then Y.
    pub fn euler(x: f32, y: f32, z: f32) -> Quat {
        Quat::axis_angle(v3(0.0, 1.0, 0.0), y)
            * Quat::axis_angle(v3(1.0, 0.0, 0.0), x)
            * Quat::axis_angle(v3(0.0, 0.0, 1.0), z)
    }

    pub fn inverse(self) -> Quat {
        Quat { x: -self.x, y: -self.y, z: -self.z, w: self.w }
    }

    pub fn rotate(self, v: V3) -> V3 {
        let q = v3(self.x, self.y, self.z);
        let c = cross(q, v);
        let cc = cross(q, c);
        v + (c * self.w + cc) * 2.0
    }

    pub fn normalized(self) -> Quat {
        let n = (self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w).sqrt();
        Quat { x: self.x / n, y: self.y / n, z: self.z / n, w: self.w / n }
    }
}

impl Mul for Quat {
    type Output = Quat;
    fn mul(self, b: Quat) -> Quat {
        let a = self;
        Quat {
            x: a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y,
            y: a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x,
            z: a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w,
            w: a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z,
        }
    }
}

fn cross(a: V3, b: V3) -> V3 {
    v3(a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x)
}

/// Column-major 4x4 matrix (same layout WGSL expects).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat4(pub [f32; 16]);

impl Mat4 {
    pub const IDENTITY: Mat4 =
        Mat4([1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);

    pub fn trs(t: V3, r: Quat, s: V3) -> Mat4 {
        let x = r.rotate(v3(1.0, 0.0, 0.0)) * s.x;
        let y = r.rotate(v3(0.0, 1.0, 0.0)) * s.y;
        let z = r.rotate(v3(0.0, 0.0, 1.0)) * s.z;
        Mat4([x.x, x.y, x.z, 0.0, y.x, y.y, y.z, 0.0, z.x, z.y, z.z, 0.0, t.x, t.y, t.z, 1.0])
    }

    pub fn mul(&self, o: &Mat4) -> Mat4 {
        let a = &self.0;
        let b = &o.0;
        let mut r = [0.0f32; 16];
        for c in 0..4 {
            for row in 0..4 {
                r[c * 4 + row] = a[row] * b[c * 4]
                    + a[4 + row] * b[c * 4 + 1]
                    + a[8 + row] * b[c * 4 + 2]
                    + a[12 + row] * b[c * 4 + 3];
            }
        }
        Mat4(r)
    }

    pub fn point(&self, p: V3) -> V3 {
        let m = &self.0;
        v3(
            m[0] * p.x + m[4] * p.y + m[8] * p.z + m[12],
            m[1] * p.x + m[5] * p.y + m[9] * p.z + m[13],
            m[2] * p.x + m[6] * p.y + m[10] * p.z + m[14],
        )
    }

    /// Absolute value of the linear part applied to `e` (AABB half-extent transform).
    pub fn abs_vector(&self, e: V3) -> V3 {
        let m = &self.0;
        v3(
            m[0].abs() * e.x + m[4].abs() * e.y + m[8].abs() * e.z,
            m[1].abs() * e.x + m[5].abs() * e.y + m[9].abs() * e.z,
            m[2].abs() * e.x + m[6].abs() * e.y + m[10].abs() * e.z,
        )
    }

    /// Inverse of an affine matrix.
    pub fn inverse_affine(&self) -> Mat4 {
        let m = &self.0;
        let (a, b, c) = (m[0], m[4], m[8]);
        let (d, e, f) = (m[1], m[5], m[9]);
        let (g, h, i) = (m[2], m[6], m[10]);
        let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
        let inv = 1.0 / det;
        let r = [
            (e * i - f * h) * inv,
            (f * g - d * i) * inv,
            (d * h - e * g) * inv,
            (c * h - b * i) * inv,
            (a * i - c * g) * inv,
            (b * g - a * h) * inv,
            (b * f - c * e) * inv,
            (c * d - a * f) * inv,
            (a * e - b * d) * inv,
        ];
        // r is the inverse 3x3 in column-major order
        let t = v3(m[12], m[13], m[14]);
        let tx = -(r[0] * t.x + r[3] * t.y + r[6] * t.z);
        let ty = -(r[1] * t.x + r[4] * t.y + r[7] * t.z);
        let tz = -(r[2] * t.x + r[5] * t.y + r[8] * t.z);
        Mat4([r[0], r[1], r[2], 0.0, r[3], r[4], r[5], 0.0, r[6], r[7], r[8], 0.0, tx, ty, tz, 1.0])
    }
}

/// Unity's Mathf.SmoothDamp with infinite max speed.
pub fn smooth_damp(current: f32, target: f32, velocity: &mut f32, smooth_time: f32, dt: f32) -> f32 {
    let smooth_time = smooth_time.max(0.0001);
    let omega = 2.0 / smooth_time;
    let x = omega * dt;
    let exp = 1.0 / (1.0 + x + 0.48 * x * x + 0.235 * x * x * x);
    let change = current - target;
    let temp = (*velocity + omega * change) * dt;
    *velocity = (*velocity - omega * temp) * exp;
    let mut out = target + (change + temp) * exp;
    if (target - current > 0.0) == (out > target) {
        out = target;
        *velocity = (out - target) / dt;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn character_collider_gravity_axis_points_down() {
        // CharacterCollider's serialized rotation; Translate(0,0,g,Self) must move it down.
        let q = Quat { x: -3.09e-8, y: -0.70710683, z: 0.70710683, w: -3.09e-8 };
        let d = q.rotate(v3(0.0, 0.0, 1.0));
        assert!((d.y + 1.0).abs() < 1e-5 && d.x.abs() < 1e-5, "{d:?}");
        let x = q.rotate(v3(1.0, 0.0, 0.0));
        assert!((x.x + 1.0).abs() < 1e-5, "{x:?}");
    }

    #[test]
    fn affine_inverse_roundtrip() {
        let m = Mat4::trs(v3(3.0, -2.0, 7.0), Quat::euler(10.0, 20.0, 30.0), v3(2.0, 0.5, 3.0));
        let p = v3(1.0, 2.0, 3.0);
        let q = m.inverse_affine().point(m.point(p));
        assert!((q - p).length() < 1e-4, "{q:?}");
    }

    #[test]
    fn smooth_damp_converges() {
        let mut v = 0.0;
        let mut x = 0.0;
        for _ in 0..100 {
            x = smooth_damp(x, 10.0, &mut v, 0.1, 0.02);
        }
        assert!((x - 10.0).abs() < 1e-3);
    }
}
