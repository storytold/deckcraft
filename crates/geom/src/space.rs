//! 3-D math for models on slides: vectors, 3×3 and 4×4 matrices, rotations and a look-at camera.
//!
//! Right-handed, Y up, the camera looking down −Z: the conventions of glTF and of PowerPoint's 3-D
//! models. Rotations are given as angles in degrees about the fixed X, Y and Z axes, applied X first,
//! then Y, then Z ([`euler_xyz`]), the way PowerPoint's `am3d:rot` turns a model. Matrices are
//! row-major and apply to column vectors (`m · v`). Every function accepts hostile numbers: NaN or
//! infinite input gives a finite, neutral result instead of a panic.

/// A point or direction.
pub type Vec3 = [f64; 3];
/// A 3×3 matrix, `m[row][col]`.
pub type Mat3 = [[f64; 3]; 3];
/// A 4×4 affine matrix, `m[row][col]`; the last row is `[0, 0, 0, 1]`.
pub type Mat4 = [[f64; 4]; 4];

pub const IDENTITY3: Mat3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
pub const IDENTITY4: Mat4 = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

fn finite(v: f64) -> f64 {
    if v.is_finite() { v } else { 0.0 }
}

pub fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub fn scale(a: Vec3, k: f64) -> Vec3 {
    [a[0] * k, a[1] * k, a[2] * k]
}

pub fn dot(a: Vec3, b: Vec3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

pub fn length(a: Vec3) -> f64 {
    dot(a, a).sqrt()
}

/// `a` scaled to length 1; `None` for a zero or non-finite vector.
pub fn normalize(a: Vec3) -> Option<Vec3> {
    let l = length(a);
    (l.is_finite() && l > 1e-12).then(|| scale(a, 1.0 / l))
}

/// An angle in degrees brought into `[0, 360)`; NaN and infinities become 0.
pub fn normalize_deg(d: f64) -> f64 {
    if !d.is_finite() {
        return 0.0;
    }
    let r = d.rem_euclid(360.0);
    // `rem_euclid` can round up to exactly 360 for tiny negative input; −0 reads as 0.
    if r >= 360.0 || r == 0.0 { 0.0 } else { r }
}

pub fn mat3_mul(a: &Mat3, b: &Mat3) -> Mat3 {
    let mut m = [[0.0; 3]; 3];
    for (i, row) in m.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j];
        }
    }
    m
}

pub fn mat3_apply(m: &Mat3, v: Vec3) -> Vec3 {
    [dot(m[0], v), dot(m[1], v), dot(m[2], v)]
}

pub fn transpose(m: &Mat3) -> Mat3 {
    [[m[0][0], m[1][0], m[2][0]], [m[0][1], m[1][1], m[2][1]], [m[0][2], m[1][2], m[2][2]]]
}

/// Rotation by `deg` degrees about the X axis (Y turns towards Z).
pub fn rot_x(deg: f64) -> Mat3 {
    let (s, c) = finite(deg).to_radians().sin_cos();
    [[1.0, 0.0, 0.0], [0.0, c, -s], [0.0, s, c]]
}

/// Rotation by `deg` degrees about the Y axis (Z turns towards X).
pub fn rot_y(deg: f64) -> Mat3 {
    let (s, c) = finite(deg).to_radians().sin_cos();
    [[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]]
}

/// Rotation by `deg` degrees about the Z axis (X turns towards Y).
pub fn rot_z(deg: f64) -> Mat3 {
    let (s, c) = finite(deg).to_radians().sin_cos();
    [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]]
}

/// The rotation that turns by `rot[0]` degrees about X, then `rot[1]` about Y, then `rot[2]` about Z
/// (all fixed axes): `Rz · Ry · Rx`.
pub fn euler_xyz(rot: Vec3) -> Mat3 {
    mat3_mul(&rot_z(rot[2]), &mat3_mul(&rot_y(rot[1]), &rot_x(rot[0])))
}

/// The angles [`euler_xyz`] turns by to give `m` (a rotation matrix), each in `[0, 360)`. When Y is
/// ±90° (X and Z turn about the same axis) the turn is put in X and Z is 0.
pub fn euler_xyz_of(m: &Mat3) -> Vec3 {
    let sy = (-m[2][0]).clamp(-1.0, 1.0);
    let y = sy.asin();
    let (x, z) = if sy.abs() < 1.0 - 1e-9 {
        (m[2][1].atan2(m[2][2]), m[1][0].atan2(m[0][0]))
    } else {
        // cos(y) = 0: only x − z (or x + z) is defined.
        ((-m[1][2]).atan2(m[1][1]), 0.0)
    };
    let tidy = |r: f64| {
        let d = normalize_deg(finite(r).to_degrees());
        // Round off float noise so 89.99999999 reads as 90 and 359.9999999 as 0.
        let k = (d * 1e6).round() / 1e6;
        if k >= 360.0 { 0.0 } else { k }
    };
    [tidy(x), tidy(y), tidy(z)]
}

/// Rotation by `deg` degrees about the unit `axis` (right-hand rule). A zero axis gives the
/// identity.
pub fn axis_angle(axis: Vec3, deg: f64) -> Mat3 {
    let Some([x, y, z]) = normalize(axis) else { return IDENTITY3 };
    let (s, c) = finite(deg).to_radians().sin_cos();
    let t = 1.0 - c;
    [
        [t * x * x + c, t * x * y - s * z, t * x * z + s * y],
        [t * x * y + s * z, t * y * y + c, t * y * z - s * x],
        [t * x * z - s * y, t * y * z + s * x, t * z * z + c],
    ]
}

/// `rot` ([`euler_xyz`] angles) turned a further `deg` degrees about the fixed world `axis`.
pub fn turn(rot: Vec3, axis: Vec3, deg: f64) -> Vec3 {
    euler_xyz_of(&mat3_mul(&axis_angle(axis, deg), &euler_xyz(rot)))
}

/// A unit quaternion `[w, x, y, z]` for a rotation matrix.
pub fn quat_of(m: &Mat3) -> [f64; 4] {
    let tr = m[0][0] + m[1][1] + m[2][2];
    let q = if tr > 0.0 {
        let s = (tr + 1.0).sqrt() * 2.0;
        [0.25 * s, (m[2][1] - m[1][2]) / s, (m[0][2] - m[2][0]) / s, (m[1][0] - m[0][1]) / s]
    } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
        let s = (1.0 + m[0][0] - m[1][1] - m[2][2]).max(0.0).sqrt() * 2.0;
        [(m[2][1] - m[1][2]) / s, 0.25 * s, (m[0][1] + m[1][0]) / s, (m[0][2] + m[2][0]) / s]
    } else if m[1][1] > m[2][2] {
        let s = (1.0 + m[1][1] - m[0][0] - m[2][2]).max(0.0).sqrt() * 2.0;
        [(m[0][2] - m[2][0]) / s, (m[0][1] + m[1][0]) / s, 0.25 * s, (m[1][2] + m[2][1]) / s]
    } else {
        let s = (1.0 + m[2][2] - m[0][0] - m[1][1]).max(0.0).sqrt() * 2.0;
        [(m[1][0] - m[0][1]) / s, (m[0][2] + m[2][0]) / s, (m[1][2] + m[2][1]) / s, 0.25 * s]
    };
    let n = q.iter().map(|v| v * v).sum::<f64>().sqrt();
    if n.is_finite() && n > 1e-12 { q.map(|v| v / n) } else { [1.0, 0.0, 0.0, 0.0] }
}

/// The rotation matrix of a unit quaternion `[w, x, y, z]`.
pub fn mat_of_quat(q: [f64; 4]) -> Mat3 {
    let [w, x, y, z] = q;
    [
        [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - w * z), 2.0 * (x * z + w * y)],
        [2.0 * (x * y + w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - w * x)],
        [2.0 * (x * z - w * y), 2.0 * (y * z + w * x), 1.0 - 2.0 * (x * x + y * y)],
    ]
}

/// The rotation `t` (0..1) of the way from `a` to `b` (both [`euler_xyz`] angles) along the shortest
/// turn between them, as [`euler_xyz`] angles. Morph turns 3-D models this way.
pub fn slerp_euler(a: Vec3, b: Vec3, t: f64) -> Vec3 {
    let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
    let qa = quat_of(&euler_xyz(a));
    let mut qb = quat_of(&euler_xyz(b));
    let mut d: f64 = qa.iter().zip(qb.iter()).map(|(x, y)| x * y).sum();
    if d < 0.0 {
        qb = qb.map(|v| -v);
        d = -d;
    }
    let q: [f64; 4] = if d > 0.9995 {
        // Nearly the same rotation: a normalized straight line is exact enough and stable.
        let v: [f64; 4] = std::array::from_fn(|i| qa[i] + (qb[i] - qa[i]) * t);
        let n = v.iter().map(|x| x * x).sum::<f64>().sqrt().max(1e-12);
        v.map(|x| x / n)
    } else {
        let th = d.clamp(-1.0, 1.0).acos();
        let s = th.sin();
        let (ka, kb) = (((1.0 - t) * th).sin() / s, (t * th).sin() / s);
        std::array::from_fn(|i| qa[i] * ka + qb[i] * kb)
    };
    euler_xyz_of(&mat_of_quat(q))
}

pub fn mat4_mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut m = [[0.0; 4]; 4];
    for (i, row) in m.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = (0..4).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    m
}

/// `m` applied to the point `p` (affine: translation included).
pub fn mat4_point(m: &Mat4, p: Vec3) -> Vec3 {
    [
        m[0][0] * p[0] + m[0][1] * p[1] + m[0][2] * p[2] + m[0][3],
        m[1][0] * p[0] + m[1][1] * p[1] + m[1][2] * p[2] + m[1][3],
        m[2][0] * p[0] + m[2][1] * p[1] + m[2][2] * p[2] + m[2][3],
    ]
}

/// `m` applied to the direction `d` (no translation).
pub fn mat4_dir(m: &Mat4, d: Vec3) -> Vec3 {
    [
        m[0][0] * d[0] + m[0][1] * d[1] + m[0][2] * d[2],
        m[1][0] * d[0] + m[1][1] * d[1] + m[1][2] * d[2],
        m[2][0] * d[0] + m[2][1] * d[1] + m[2][2] * d[2],
    ]
}

/// The affine matrix `p ↦ r · p + t`.
pub fn affine(r: &Mat3, t: Vec3) -> Mat4 {
    [[r[0][0], r[0][1], r[0][2], t[0]], [r[1][0], r[1][1], r[1][2], t[1]], [r[2][0], r[2][1], r[2][2], t[2]], [0.0, 0.0, 0.0, 1.0]]
}

/// World → camera for a camera at `eye` looking at `target` with `up` roughly up: the camera ends
/// at the origin looking down −Z with Y up. Degenerate input (eye on target, up along the view)
/// falls back to a camera on +Z looking at the origin.
pub fn look_at(eye: Vec3, target: Vec3, up: Vec3) -> Mat4 {
    let fwd = normalize(sub(target, eye)).unwrap_or([0.0, 0.0, -1.0]);
    let right = normalize(cross(fwd, up)).or_else(|| normalize(cross(fwd, [0.0, 1.0, 0.0]))).or_else(|| normalize(cross(fwd, [1.0, 0.0, 0.0])));
    let Some(right) = right else { return IDENTITY4 };
    let true_up = cross(right, fwd);
    let r = [right, true_up, scale(fwd, -1.0)];
    let eye = eye.map(finite);
    affine(&r, [-dot(right, eye), -dot(true_up, eye), dot(fwd, eye)])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3) -> bool {
        a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() < 1e-6)
    }

    fn close_deg(a: Vec3, b: Vec3) -> bool {
        a.iter().zip(b.iter()).all(|(x, y)| {
            let d = (x - y).rem_euclid(360.0);
            d < 1e-4 || d > 360.0 - 1e-4
        })
    }

    #[test]
    fn rotations_follow_the_right_hand_rule() {
        // +X turns the top (+Y) towards the viewer (+Z).
        assert!(close(mat3_apply(&rot_x(90.0), [0.0, 1.0, 0.0]), [0.0, 0.0, 1.0]));
        // +Y turns the front (+Z) towards +X.
        assert!(close(mat3_apply(&rot_y(90.0), [0.0, 0.0, 1.0]), [1.0, 0.0, 0.0]));
        // +Z turns +X towards +Y (counter-clockwise on screen).
        assert!(close(mat3_apply(&rot_z(90.0), [1.0, 0.0, 0.0]), [0.0, 1.0, 0.0]));
    }

    #[test]
    fn x_turns_first_then_y_then_z() {
        let m = euler_xyz([90.0, 90.0, 0.0]);
        // X 90 takes +Y to +Z, then Y 90 takes +Z to +X.
        assert!(close(mat3_apply(&m, [0.0, 1.0, 0.0]), [1.0, 0.0, 0.0]));
    }

    #[test]
    fn euler_round_trips() {
        for rot in [[0.0, 0.0, 0.0], [30.0, 0.0, 0.0], [20.0, 40.0, 60.0], [350.0, 10.0, 200.0], [45.0, 300.0, 15.0], [10.0, 90.0, 0.0]] {
            let back = euler_xyz_of(&euler_xyz(rot));
            assert!(
                close_deg(back, rot) || close(mat3_apply(&euler_xyz(back), [1.0, 2.0, 3.0]), mat3_apply(&euler_xyz(rot), [1.0, 2.0, 3.0])),
                "{rot:?} -> {back:?}"
            );
        }
    }

    #[test]
    fn turning_about_world_axes_composes() {
        let r = turn([0.0, 0.0, 0.0], [0.0, 1.0, 0.0], 30.0);
        assert!(close_deg(r, [0.0, 30.0, 0.0]));
        let r = turn([0.0, 30.0, 0.0], [0.0, 1.0, 0.0], 15.0);
        assert!(close_deg(r, [0.0, 45.0, 0.0]));
        // A turn about the view axis is a turn about Z.
        let r = turn([20.0, 40.0, 0.0], [0.0, 0.0, 1.0], 25.0);
        assert!(close_deg(r, [20.0, 40.0, 25.0]));
    }

    #[test]
    fn slerp_goes_the_short_way() {
        let m = slerp_euler([0.0, 350.0, 0.0], [0.0, 10.0, 0.0], 0.5);
        assert!(close_deg(m, [0.0, 0.0, 0.0]), "{m:?}");
        let q = slerp_euler([0.0, 0.0, 0.0], [0.0, 90.0, 0.0], 0.5);
        assert!(close_deg(q, [0.0, 45.0, 0.0]), "{q:?}");
        assert!(close_deg(slerp_euler([10.0, 20.0, 30.0], [50.0, 60.0, 70.0], 0.0), [10.0, 20.0, 30.0]));
        assert!(close_deg(slerp_euler([10.0, 20.0, 30.0], [50.0, 60.0, 70.0], 1.0), [50.0, 60.0, 70.0]));
    }

    #[test]
    fn look_at_puts_the_target_ahead() {
        let v = look_at([0.0, 0.0, 2.0], [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        assert!(close(mat4_point(&v, [0.0, 0.0, 0.0]), [0.0, 0.0, -2.0]));
        assert!(close(mat4_point(&v, [1.0, 1.0, 0.0]), [1.0, 1.0, -2.0]));
        // Degenerate input stays finite.
        let d = look_at([0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 0.0]);
        assert!(d.iter().flatten().all(|v| v.is_finite()));
        let up_along_view = look_at([0.0, 2.0, 0.0], [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        assert!(up_along_view.iter().flatten().all(|v| v.is_finite()));
    }

    #[test]
    fn hostile_numbers_stay_finite() {
        assert_eq!(normalize_deg(f64::NAN), 0.0);
        assert_eq!(normalize_deg(-1e-20), 0.0);
        assert_eq!(normalize_deg(-30.0), 330.0);
        let r = euler_xyz_of(&euler_xyz([f64::NAN, f64::INFINITY, 1e300]));
        assert!(r.iter().all(|v| v.is_finite()));
        assert!(slerp_euler([f64::NAN; 3], [1.0, 2.0, 3.0], f64::NAN).iter().all(|v| v.is_finite()));
        assert_eq!(axis_angle([0.0; 3], 45.0), IDENTITY3);
    }
}
