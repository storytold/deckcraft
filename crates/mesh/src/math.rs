//! The little linear algebra the readers need: column-major 4 × 4 affine matrices.

/// Column-major, as glTF stores them: `m[col * 4 + row]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat4(pub [f64; 16]);

impl Mat4 {
    pub const IDENTITY: Mat4 = Mat4([1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);

    fn at(&self, row: usize, col: usize) -> f64 {
        self.0.get(col * 4 + row).copied().unwrap_or(0.0)
    }

    pub fn mul(&self, o: &Mat4) -> Mat4 {
        let mut m = [0.0; 16];
        for col in 0..4 {
            for row in 0..4 {
                let v = (0..4).map(|k| self.at(row, k) * o.at(k, col)).sum();
                if let Some(slot) = m.get_mut(col * 4 + row) {
                    *slot = v;
                }
            }
        }
        Mat4(m)
    }

    pub fn translation(t: [f64; 3]) -> Mat4 {
        let mut m = Mat4::IDENTITY;
        m.0[12] = t[0];
        m.0[13] = t[1];
        m.0[14] = t[2];
        m
    }

    pub fn scale(s: [f64; 3]) -> Mat4 {
        let mut m = Mat4::IDENTITY;
        m.0[0] = s[0];
        m.0[5] = s[1];
        m.0[10] = s[2];
        m
    }

    /// Unit quaternion `[x, y, z, w]` (glTF order).
    pub fn rotation(q: [f64; 4]) -> Mat4 {
        let len = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
        if !(len.is_finite() && len > 1e-12) {
            return Mat4::IDENTITY;
        }
        let [x, y, z, w] = [q[0] / len, q[1] / len, q[2] / len, q[3] / len];
        Mat4([
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y + z * w),
            2.0 * (x * z - y * w),
            0.0,
            2.0 * (x * y - z * w),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z + x * w),
            0.0,
            2.0 * (x * z + y * w),
            2.0 * (y * z - x * w),
            1.0 - 2.0 * (x * x + y * y),
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
        ])
    }

    /// Rotation about one axis (0 = X, 1 = Y, 2 = Z) by `deg` degrees.
    pub fn axis_rotation(axis: usize, deg: f64) -> Mat4 {
        let (mut s, mut c) = deg.to_radians().sin_cos();
        if deg.rem_euclid(90.0) == 0.0 {
            // Quarter turns exactly, so axis swaps don't leave 1e-16 behind.
            (s, c) = (s.round(), c.round());
        }
        let mut m = Mat4::IDENTITY;
        let (a, b) = match axis {
            0 => (1, 2),
            1 => (2, 0),
            _ => (0, 1),
        };
        m.0[a * 4 + a] = c;
        m.0[a * 4 + b] = s;
        m.0[b * 4 + a] = -s;
        m.0[b * 4 + b] = c;
        m
    }

    /// Euler angles in degrees applied X first, then Y, then Z (FBX's default `eEulerXYZ`).
    pub fn euler_xyz(r: [f64; 3]) -> Mat4 {
        Mat4::axis_rotation(2, r[2]).mul(&Mat4::axis_rotation(1, r[1])).mul(&Mat4::axis_rotation(0, r[0]))
    }

    /// The upper 3 × 3 block transposed (the inverse of a rotation).
    pub fn transpose3(&self) -> Mat4 {
        let mut m = Mat4::IDENTITY;
        for r in 0..3 {
            for c in 0..3 {
                if let Some(slot) = m.0.get_mut(c * 4 + r) {
                    *slot = self.at(c, r);
                }
            }
        }
        m
    }

    /// The angles (degrees) `[x, y, z]` with [`Mat4::euler_xyz`] of them equal to this rotation.
    /// At ±90° about Y the X and Z turns share an axis; Z is then taken as 0.
    pub fn euler_xyz_angles(&self) -> [f64; 3] {
        let a = |r, c| self.at(r, c);
        let sy = (-a(2, 0)).clamp(-1.0, 1.0);
        let y = sy.asin();
        let (x, z) = if y.cos().abs() > 1e-9 { (a(2, 1).atan2(a(2, 2)), a(1, 0).atan2(a(0, 0))) } else { ((-a(1, 2)).atan2(a(1, 1)), 0.0) };
        let tidy = |v: f64| {
            let d = v.to_degrees();
            let r = (d * 1e6).round() / 1e6;
            if r == 0.0 { 0.0 } else { r }
        };
        [tidy(x), tidy(y), tidy(z)]
    }

    pub fn point(&self, p: [f32; 3]) -> [f32; 3] {
        let [x, y, z] = [p[0] as f64, p[1] as f64, p[2] as f64];
        let r = |row| (self.at(row, 0) * x + self.at(row, 1) * y + self.at(row, 2) * z + self.at(row, 3)) as f32;
        [r(0), r(1), r(2)]
    }

    /// The upper 3 × 3 block's determinant: negative when the transform mirrors.
    pub fn det3(&self) -> f64 {
        let a = |r, c| self.at(r, c);
        a(0, 0) * (a(1, 1) * a(2, 2) - a(1, 2) * a(2, 1)) - a(0, 1) * (a(1, 0) * a(2, 2) - a(1, 2) * a(2, 0))
            + a(0, 2) * (a(1, 0) * a(2, 1) - a(1, 1) * a(2, 0))
    }

    /// The matrix that carries normals: the inverse transpose of the upper 3 × 3 block
    /// (`None` when it is singular).
    pub fn normal_matrix(&self) -> Option<[[f64; 3]; 3]> {
        let d = self.det3();
        if !(d.is_finite() && d.abs() > 1e-30) {
            return None;
        }
        let a = |r, c| self.at(r, c);
        // Cofactor matrix / det = inverse transpose.
        let cof = |r: usize, c: usize| {
            let rows: Vec<usize> = (0..3).filter(|&i| i != r).collect();
            let cols: Vec<usize> = (0..3).filter(|&i| i != c).collect();
            let (r0, r1, c0, c1) = (rows.first().copied()?, rows.get(1).copied()?, cols.first().copied()?, cols.get(1).copied()?);
            let minor = a(r0, c0) * a(r1, c1) - a(r0, c1) * a(r1, c0);
            Some(if (r + c).is_multiple_of(2) { minor } else { -minor } / d)
        };
        let mut m = [[0.0; 3]; 3];
        for (r, row) in m.iter_mut().enumerate() {
            for (c, v) in row.iter_mut().enumerate() {
                *v = cof(r, c)?;
            }
        }
        Some(m)
    }

    pub fn is_finite(&self) -> bool {
        self.0.iter().all(|v| v.is_finite())
    }
}

pub fn apply3(m: &[[f64; 3]; 3], n: [f32; 3]) -> [f32; 3] {
    let v = [n[0] as f64, n[1] as f64, n[2] as f64];
    let r = |row: &[f64; 3]| (row[0] * v[0] + row[1] * v[1] + row[2] * v[2]) as f32;
    normalize([r(&m[0]), r(&m[1]), r(&m[2])])
}

/// The unit vector along `n`, or +Z when it has no length.
pub fn normalize(n: [f32; 3]) -> [f32; 3] {
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if len.is_finite() && len > 1e-12 { [n[0] / len, n[1] / len, n[2] / len] } else { [0.0, 0.0, 1.0] }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-5)
    }

    #[test]
    fn rotations_turn_the_right_way() {
        // 90° about Z takes +X to +Y.
        assert!(close(Mat4::axis_rotation(2, 90.0).point([1.0, 0.0, 0.0]), [0.0, 1.0, 0.0]));
        // 90° about X takes +Y to +Z.
        assert!(close(Mat4::axis_rotation(0, 90.0).point([0.0, 1.0, 0.0]), [0.0, 0.0, 1.0]));
        // The quaternion for 90° about Y takes +Z to +X.
        let h = std::f64::consts::FRAC_1_SQRT_2;
        assert!(close(Mat4::rotation([0.0, h, 0.0, h]).point([0.0, 0.0, 1.0]), [1.0, 0.0, 0.0]));
    }

    #[test]
    fn normals_follow_a_non_uniform_scale() {
        let m = Mat4::scale([2.0, 1.0, 1.0]);
        let n = m.normal_matrix().map(|nm| apply3(&nm, normalize([1.0, 1.0, 0.0])));
        // The 45° surface gets steeper, so its normal leans towards +Y.
        assert!(n.is_some_and(|n| n[1] > n[0]), "{n:?}");
        assert_eq!(Mat4::scale([0.0, 1.0, 1.0]).normal_matrix(), None);
        assert!(Mat4::scale([-1.0, 1.0, 1.0]).det3() < 0.0);
    }
}
