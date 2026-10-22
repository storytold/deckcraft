//! 3-D models (Insert ▸ 3D Models).
//!
//! A [`Model3d`] holds what PowerPoint's `am3d:model3d` element stores: the model (a glTF binary in
//! the presentation's media), how it is placed in a scene measured in metres (scaled to metres,
//! centred, scaled, turned, moved), the camera that looks at it, the lights, and how the camera's
//! view maps onto the shape's frame on the slide.
//!
//! The scene is right-handed with Y up; the camera looks down its −Z axis. The model turns by
//! [`Model3d::rot`] degrees about X, then Y, then Z (fixed axes). The vertical field of view spans
//! [`Model3d::viewport`] times the frame's height on the slide, centred on the frame.

use deckcraft_geom::space::{self, Mat4, Vec3};
use serde::{Deserialize, Serialize};

use crate::MediaId;

/// The model's field of view when inserted, in degrees.
pub const DEFAULT_FOV: f64 = 45.0;

/// A camera looking at the scene.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Camera3d {
    /// Metres.
    pub pos: Vec3,
    pub up: Vec3,
    pub look_at: Vec3,
    /// Vertical field of view in degrees; 0 or less: orthographic.
    pub fov: f64,
    /// Height an orthographic camera sees, in metres.
    pub ortho: f64,
}

impl Default for Camera3d {
    fn default() -> Self {
        Camera3d { pos: [0.0, 0.0, 2.0], up: [0.0, 1.0, 0.0], look_at: [0.0; 3], fov: DEFAULT_FOV, ortho: 1.0 }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LightKind {
    /// Light from everywhere; `intensity` is its illuminance.
    #[default]
    Ambient,
    /// From `pos`, in every direction; `intensity` in candela.
    Point,
    /// Parallel light travelling along `dir`; `intensity` is its illuminance.
    Directional,
    /// From `pos` towards `dir`, within `angle` degrees of it; `intensity` in candela.
    Spot,
}

/// A light, placed in the scene (metres).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Light3d {
    pub kind: LightKind,
    /// Linear RGB, 0..1.
    pub color: [f64; 3],
    pub intensity: f64,
    pub pos: Vec3,
    pub dir: Vec3,
    /// A spot light's half-angle in degrees.
    pub angle: f64,
}

/// DeckCraft's lighting for a newly inserted model: soft light from everywhere, a warm key light
/// above right, a cool fill from the left and a rim light from behind. In PowerPoint's units, so
/// the model looks the same when PowerPoint draws it.
pub fn default_lights() -> Vec<Light3d> {
    vec![
        Light3d { kind: LightKind::Ambient, color: [1.0, 1.0, 1.0], intensity: 0.1, ..Default::default() },
        Light3d { kind: LightKind::Point, color: [1.0, 0.95, 0.88], intensity: 6.0, pos: [1.2, 1.8, 2.0], ..Default::default() },
        Light3d { kind: LightKind::Point, color: [0.77, 0.85, 1.0], intensity: 3.0, pos: [-2.0, 0.6, 1.4], ..Default::default() },
        Light3d { kind: LightKind::Point, color: [1.0, 1.0, 1.0], intensity: 2.5, pos: [0.3, 1.6, -2.4], ..Default::default() },
    ]
}

/// A 3-D model on a slide (`am3d:model3d`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Model3d {
    /// The model, a glTF binary (`model/gltf.binary`).
    pub media: MediaId,
    /// A picture of the model (PowerPoint's raster, the fallback picture older readers show),
    /// shown when the model can't be drawn.
    pub poster: Option<MediaId>,
    /// Model units to metres (`meterPerModelUnit`).
    pub meter_per_unit: f64,
    /// Moves the scaled model, in metres: its centre to the origin (`preTrans`).
    pub pre: Vec3,
    pub scale: Vec3,
    /// Degrees about X, then Y, then Z, each in `[0, 360)` (`rot`).
    pub rot: Vec3,
    /// Moves the turned model, in metres (`postTrans`).
    pub post: Vec3,
    pub camera: Camera3d,
    /// How many frame heights the camera's field of view spans on the slide (PowerPoint's
    /// `objViewport` size over the frame height). Resizing the frame resizes the model with it.
    pub viewport: f64,
    /// The frame follows the model's outline when it turns (PowerPoint's AutoFit).
    pub auto_fit: bool,
    pub lights: Vec<Light3d>,
    /// The lights as a PowerPoint file had them, written back unchanged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lights_xml: Option<String>,
}

impl Default for Model3d {
    fn default() -> Self {
        Model3d {
            media: MediaId(0),
            poster: None,
            meter_per_unit: 1.0,
            pre: [0.0; 3],
            scale: [1.0; 3],
            rot: [0.0; 3],
            post: [0.0; 3],
            camera: Camera3d::default(),
            viewport: 1.0,
            auto_fit: true,
            lights: default_lights(),
            lights_xml: None,
        }
    }
}

fn finite(v: f64, or: f64) -> f64 {
    if v.is_finite() { v } else { or }
}

impl Model3d {
    /// A model whose vertices lie in the box `lo`–`hi` (model units), set up the way PowerPoint
    /// inserts one: scaled so its longest side is a metre, centred on the origin, and seen from
    /// straight ahead by a 45° camera just far enough away for its bounding sphere to fill the view.
    pub fn fitted(media: MediaId, lo: Vec3, hi: Vec3) -> Model3d {
        let size = (0..3).map(|k| finite(hi[k] - lo[k], 0.0).abs()).fold(0.0, f64::max);
        let mpu = if size > 1e-12 { 1.0 / size } else { 1.0 };
        let mpu = finite(mpu, 1.0);
        // `+ 0.0` turns −0 into 0.
        let pre = [0, 1, 2].map(|k| finite(-(lo[k] + hi[k]) / 2.0 * mpu, 0.0) + 0.0);
        let radius = (0..3).map(|k| (finite(hi[k] - lo[k], 0.0) * mpu / 2.0).powi(2)).sum::<f64>().sqrt();
        let radius = if radius > 1e-9 { radius } else { 0.5 };
        let dist = radius / (DEFAULT_FOV / 2.0).to_radians().sin();
        Model3d { media, meter_per_unit: mpu, pre, camera: Camera3d { pos: [0.0, 0.0, dist], ..Default::default() }, ..Default::default() }
    }

    /// Model units → scene (metres): `post + R · S · (meter_per_unit · v + pre)`.
    pub fn model_matrix(&self) -> Mat4 {
        let r = space::euler_xyz(self.rot);
        let s = self.scale.map(|v| finite(v, 1.0));
        let k = finite(self.meter_per_unit, 1.0);
        // R · S · k, then translation R · S · pre + post.
        let rs: space::Mat3 = std::array::from_fn(|i| std::array::from_fn(|j| r[i][j] * s[j] * k));
        let pre = self.pre.map(|v| finite(v, 0.0));
        let t =
            space::add(space::mat3_apply(&std::array::from_fn(|i| std::array::from_fn(|j| r[i][j] * s[j])), pre), self.post.map(|v| finite(v, 0.0)));
        space::affine(&rs, t)
    }

    /// Scene → camera space (the camera at the origin looking down −Z).
    pub fn view_matrix(&self) -> Mat4 {
        let c = &self.camera;
        space::look_at(c.pos, c.look_at, c.up)
    }

    /// Model units → camera space.
    pub fn model_view(&self) -> Mat4 {
        space::mat4_mul(&self.view_matrix(), &self.model_matrix())
    }

    pub fn is_ortho(&self) -> bool {
        !(self.camera.fov.is_finite() && self.camera.fov > 0.0)
    }

    /// The height on the slide (points) the camera's field of view spans, for a frame `h` points
    /// tall.
    pub fn span(&self, h: f64) -> f64 {
        let v = finite(self.viewport, 1.0);
        let v = if v > 1e-6 { v } else { 1.0 };
        (v * h).abs()
    }

    /// Points on the slide per unit of `x / −z` in camera space (per metre when orthographic), for
    /// a frame `h` points tall.
    pub fn focal(&self, h: f64) -> f64 {
        let span = self.span(h);
        if self.is_ortho() {
            let o = finite(self.camera.ortho, 1.0).abs().max(1e-9);
            span / o
        } else {
            let half = (self.camera.fov.clamp(0.1, 179.0) / 2.0).to_radians().tan();
            span / 2.0 / half
        }
    }

    /// Turned to `rot` (degrees), each angle brought into `[0, 360)`.
    pub fn set_rotation(&mut self, rot: Vec3) {
        self.rot = rot.map(space::normalize_deg);
    }

    /// The frame (w, h) that holds the model's outline when it reaches `hx`, `hy` (in `x / −z`
    /// units, from the mesh's extent) from the frame centre, keeping its size on the slide: the
    /// span of a frame `old_h` tall. Also sets [`Model3d::viewport`] for the new frame.
    pub fn refit(&mut self, (hx, hy): (f64, f64), old_h: f64) -> Option<(f64, f64)> {
        let span = self.span(old_h);
        let f = self.focal(old_h);
        let (w, h) = (2.0 * hx * f, 2.0 * hy * f);
        if !(w.is_finite() && h.is_finite() && w > 0.01 && h > 0.01 && span > 0.0) {
            return None;
        }
        self.viewport = span / h;
        Some((w, h))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fitted_matches_powerpoints_insert() {
        // A 2 × 1 × 0.5 box from (2, 0, −0.25): PowerPoint writes meterPerModelUnit 0.5,
        // preTrans (−1.5, −0.25, 0) m and puts the camera 1.496856 m away.
        let m = Model3d::fitted(MediaId(1), [2.0, 0.0, -0.25], [4.0, 1.0, 0.25]);
        assert!((m.meter_per_unit - 0.5).abs() < 1e-12);
        assert_eq!(m.pre, [-1.5, -0.25, 0.0]);
        assert!((m.camera.pos[2] - 1.496856).abs() < 1e-6, "{}", m.camera.pos[2]);
        // A unit cube: the camera 2.263033 m away.
        let c = Model3d::fitted(MediaId(1), [-0.5; 3], [0.5; 3]);
        assert!((c.camera.pos[2] - 2.263033).abs() < 1e-6);
    }

    #[test]
    fn model_view_moves_the_centre_in_front_of_the_camera() {
        let m = Model3d::fitted(MediaId(1), [2.0, 0.0, -0.25], [4.0, 1.0, 0.25]);
        let mv = m.model_view();
        let c = space::mat4_point(&mv, [3.0, 0.5, 0.0]);
        assert!(c[0].abs() < 1e-9 && c[1].abs() < 1e-9 && (c[2] + 1.496856).abs() < 1e-6);
        // Turned 90° about Y, the model's +X end (x = 4) comes towards the viewer.
        let mut t = m.clone();
        t.set_rotation([0.0, -90.0, 0.0]);
        assert_eq!(t.rot, [0.0, 270.0, 0.0]);
        let e = space::mat4_point(&t.model_view(), [4.0, 0.5, 0.0]);
        assert!((e[2] - (-1.496856 + 0.5)).abs() < 1e-6, "{e:?}");
    }

    #[test]
    fn focal_spans_the_viewport() {
        // PowerPoint's probe box: frame 187.71 pt tall, viewport 426.67 pt; the front face, 0.5 m
        // tall and 1.371856 m from the camera, fills the frame height.
        let mut m = Model3d::fitted(MediaId(1), [2.0, 0.0, -0.25], [4.0, 1.0, 0.25]);
        m.viewport = 426.6667 / 187.7137;
        let f = m.focal(187.7137);
        let h = 0.5 / (1.496856 - 0.125) * f;
        assert!((h - 187.7137).abs() < 0.05, "{h}");
    }

    #[test]
    fn refit_keeps_the_model_size() {
        let mut m = Model3d::fitted(MediaId(1), [-0.5; 3], [0.5; 3]);
        m.viewport = 2.0;
        let (w, h) = m.refit((0.3, 0.2), 100.0).unwrap();
        assert!((m.span(h) - 200.0).abs() < 1e-9);
        assert!((w / h - 1.5).abs() < 1e-9);
        assert!(m.refit((f64::NAN, 0.2), 100.0).is_none());
        assert!(m.refit((0.0, 0.0), 100.0).is_none());
    }

    #[test]
    fn hostile_numbers_stay_finite() {
        let mut m = Model3d::fitted(MediaId(1), [f64::NAN; 3], [f64::INFINITY; 3]);
        assert!(m.model_view().iter().flatten().all(|v| v.is_finite()));
        m.viewport = f64::NAN;
        m.camera.fov = f64::NAN;
        assert!(m.focal(100.0).is_finite());
        m.set_rotation([f64::NAN, 1e308, -1e308]);
        assert!(m.rot.iter().all(|v| v.is_finite()));
    }
}
