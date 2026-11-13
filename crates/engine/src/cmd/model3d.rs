//! 3-D Model ▸ 3-D Model View: the camera a `.glb` model is shown from, and its scene.
//!
//! A 3-D model's view is four final values — where the camera stands, what it looks at, which
//! way is up and the vertical field of view. There is deliberately no relative form: "turn 15°"
//! or "zoom in" applied by two replicas to their own views would end in two different views, so
//! the command accepts the whole resulting camera and refuses anything else before the first
//! write (the same rule as `transition.set`, RFC §3.2.1).
//!
//! The scene — one light and the environment — follows the same rule: `dimmer`, `warmer` or
//! "turn the light 10°" are refused, and only the whole resulting value is accepted.

use deckcraft_model::{ModelCamera, ModelEnvironment, ModelLight, ModelLightKind, ModelScene, ShapeKind};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_selection, targets};
use crate::{Result, Session};

/// Largest camera coordinate in EMU: a view a billion EMU (about 27 metres) from the model is
/// already far outside any slide, and larger numbers cannot come from a real file.
const MAX_EMU: f64 = 1.0e9;
/// Largest light direction component. A direction is a vector, not a position, but a component
/// this large cannot come from a real scene.
const MAX_DIR: f64 = 1.0e6;
/// Smallest field of view in degrees.
const MIN_FOV: f64 = 1.0;
/// Largest field of view in degrees (below a half turn, so the projection stays sane).
const MAX_FOV: f64 = 179.0;
/// Smallest spot cone angle in degrees.
const MIN_ANGLE: f64 = 1.0;
/// Largest spot cone angle in degrees (below a half turn, so the cone stays sane).
const MAX_ANGLE: f64 = 179.0;
/// Below this the up vector is treated as degenerate.
const MIN_UP: f64 = 1.0e-6;
/// Below this the up vector is treated as parallel to the view direction.
const MIN_SINE: f64 = 1.0e-6;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "model3d.camera",
            "3-D Model View",
            ["3-D Model"],
            None,
            "{camera: {position: [x, y, z] EMU, target: [x, y, z] EMU, up: [dx, dy, dz], fov: degrees 1–179} | {camera: null (the reader's default view), ids?}} — the whole final view; a relative edit (dx/dy/zoom/orbit) is refused before the first write",
            has_selection,
            camera
        ),
        cmd!(
            "model3d.scene",
            "3-D Model Scene",
            ["3-D Model"],
            None,
            "{scene: {light: {type: directional|point|spot, direction: [dx, dy, dz], intensity: 0–1, angle: degrees 1–179 (a spot only)}, environment: {ambient: 0–1, background: [r, g, b] 0–255}}, ids?} | {scene: null (the reader's default scene)} — the whole final scene; a relative edit (dimmer/rotate/zoom) is refused before the first write",
            has_selection,
            scene
        ),
    ]
}

/// The camera a `model3d.camera` parameter names: `Some` for a final view, `None` for "no
/// camera" (the reader's default view). Every key and every number is checked here, so a relative
/// edit or a camera that cannot be projected is refused before the document is touched.
pub fn camera_param(p: &Value) -> Result<Option<ModelCamera>> {
    let cmd = "model3d.camera";
    let value = p.get("camera").ok_or_else(|| bad(cmd, "missing `camera` (the whole final view, or null)"))?;
    if value.is_null() {
        return Ok(None);
    }
    let object = value.as_object().ok_or_else(|| bad(cmd, "`camera` must be an object with position, target, up and fov"))?;
    // Final values only: a relative field means the sender expected the receiving replica to
    // merge it against its own view, which is exactly the drift this command exists to avoid.
    for key in object.keys() {
        if !matches!(key.as_str(), "position" | "target" | "up" | "fov") {
            return Err(bad(cmd, format!("`{key}` is not a final camera field (position, target, up, fov); relative edits are refused")));
        }
    }
    let vec3 = |name: &str| -> Result<[f64; 3]> {
        let array =
            object.get(name).and_then(Value::as_array).filter(|a| a.len() == 3).ok_or_else(|| bad(cmd, format!("`{name}` must be [x, y, z]")))?;
        let mut v = [0.0; 3];
        for (i, item) in array.iter().enumerate() {
            v[i] = item.as_f64().filter(|x| x.is_finite()).ok_or_else(|| bad(cmd, format!("`{name}` must be three finite numbers")))?;
            if v[i].abs() > MAX_EMU {
                return Err(bad(cmd, format!("`{name}` is outside ±{MAX_EMU} EMU")));
            }
        }
        Ok(v)
    };
    let position = vec3("position")?;
    let target = vec3("target")?;
    let up = vec3("up")?;
    let fov =
        object.get("fov").and_then(Value::as_f64).filter(|x| x.is_finite()).ok_or_else(|| bad(cmd, "`fov` must be a finite number of degrees"))?;
    if !(MIN_FOV..=MAX_FOV).contains(&fov) {
        return Err(bad(cmd, format!("`fov` must be {MIN_FOV}–{MAX_FOV} degrees")));
    }
    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let cross = |a: [f64; 3], b: [f64; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let len = |a: [f64; 3]| dot(a, a).sqrt();
    let direction = sub(target, position);
    let distance = len(direction);
    let up_len = len(up);
    if distance <= MIN_UP {
        return Err(bad(cmd, "`position` and `target` are the same point"));
    }
    if up_len <= MIN_UP {
        return Err(bad(cmd, "`up` is a zero vector"));
    }
    if len(cross(direction, up)) / (distance * up_len) < MIN_SINE {
        return Err(bad(cmd, "`up` is parallel to the view direction"));
    }
    Ok(Some(ModelCamera { position, target, up, fov }))
}

/// The scene a `model3d.scene` parameter names: `Some` for a final scene, `None` for "no scene"
/// (the reader's default lighting). Every key and every number is checked here, so a relative
/// edit or a light that cannot be drawn is refused before the document is touched.
pub fn scene_param(p: &Value) -> Result<Option<ModelScene>> {
    let cmd = "model3d.scene";
    let value = p.get("scene").ok_or_else(|| bad(cmd, "missing `scene` (the whole final scene, or null)"))?;
    if value.is_null() {
        return Ok(None);
    }
    let object = value.as_object().ok_or_else(|| bad(cmd, "`scene` must be an object with light and environment"))?;
    // Final values only: a relative field means the sender expected the receiving replica to
    // merge it against its own scene, which is exactly the drift this command exists to avoid.
    for key in object.keys() {
        if !matches!(key.as_str(), "light" | "environment") {
            return Err(bad(cmd, format!("`{key}` is not a final scene field (light, environment); relative edits are refused")));
        }
    }
    let light = object.get("light").and_then(Value::as_object).ok_or_else(|| bad(cmd, "`light` must be an object"))?;
    for key in light.keys() {
        if !matches!(key.as_str(), "type" | "direction" | "intensity" | "angle") {
            return Err(bad(cmd, format!("`{key}` is not a final light field (type, direction, intensity, angle); relative edits are refused")));
        }
    }
    let kind = light
        .get("type")
        .and_then(Value::as_str)
        .and_then(ModelLightKind::parse)
        .ok_or_else(|| bad(cmd, "`light.type` must be `directional`, `point` or `spot`"))?;
    let array = light
        .get("direction")
        .and_then(Value::as_array)
        .filter(|a| a.len() == 3)
        .ok_or_else(|| bad(cmd, "`light.direction` must be [dx, dy, dz]"))?;
    let mut direction = [0.0; 3];
    for (i, item) in array.iter().enumerate() {
        direction[i] = item.as_f64().filter(|x| x.is_finite()).ok_or_else(|| bad(cmd, "`light.direction` must be three finite numbers"))?;
        if direction[i].abs() > MAX_DIR {
            return Err(bad(cmd, format!("`light.direction` is outside ±{MAX_DIR}")));
        }
    }
    if direction.iter().all(|v| *v == 0.0) {
        return Err(bad(cmd, "`light.direction` is a zero vector"));
    }
    let intensity = light
        .get("intensity")
        .and_then(Value::as_f64)
        .filter(|x| x.is_finite())
        .ok_or_else(|| bad(cmd, "`light.intensity` must be a finite fraction 0–1"))?;
    if !(0.0..=1.0).contains(&intensity) {
        return Err(bad(cmd, "`light.intensity` must be 0–1"));
    }
    let angle = match light.get("angle") {
        None | Some(Value::Null) => None,
        Some(v) => {
            let angle = v.as_f64().filter(|x| x.is_finite()).ok_or_else(|| bad(cmd, "`light.angle` must be a finite number of degrees"))?;
            if !(MIN_ANGLE..=MAX_ANGLE).contains(&angle) {
                return Err(bad(cmd, format!("`light.angle` must be {MIN_ANGLE}–{MAX_ANGLE} degrees")));
            }
            Some(angle)
        }
    };
    // The angle is the spot cone and only the spot cone: a spot without one has no cone, and an
    // angle on a directional or point light would be dropped by every renderer.
    match (kind, angle) {
        (ModelLightKind::Spot, None) => return Err(bad(cmd, "a spot light needs `light.angle`")),
        (ModelLightKind::Spot, Some(_)) => {}
        (_, Some(_)) => return Err(bad(cmd, "`light.angle` belongs to a spot light")),
        (_, None) => {}
    }
    let environment = object.get("environment").and_then(Value::as_object).ok_or_else(|| bad(cmd, "`environment` must be an object"))?;
    for key in environment.keys() {
        if !matches!(key.as_str(), "ambient" | "background") {
            return Err(bad(cmd, format!("`{key}` is not a final environment field (ambient, background); relative edits are refused")));
        }
    }
    let ambient = environment
        .get("ambient")
        .and_then(Value::as_f64)
        .filter(|x| x.is_finite())
        .ok_or_else(|| bad(cmd, "`environment.ambient` must be a finite fraction 0–1"))?;
    if !(0.0..=1.0).contains(&ambient) {
        return Err(bad(cmd, "`environment.ambient` must be 0–1"));
    }
    let background = match environment.get("background") {
        None | Some(Value::Null) => None,
        Some(v) => {
            let array = v.as_array().filter(|a| a.len() == 3).ok_or_else(|| bad(cmd, "`environment.background` must be [r, g, b]"))?;
            let mut rgb = [0u8; 3];
            for (i, item) in array.iter().enumerate() {
                let n = item.as_u64().filter(|n| *n <= 255).ok_or_else(|| bad(cmd, "`environment.background` must be three integers 0–255"))?;
                rgb[i] = n as u8;
            }
            Some(rgb)
        }
    };
    Ok(Some(ModelScene { light: ModelLight { kind, direction, intensity, angle }, environment: ModelEnvironment { ambient, background } }))
}

fn scene(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "model3d.scene";
    let value = scene_param(p)?;
    let ids = targets(s, p)?;
    if ids.is_empty() {
        return Err(bad(cmd, "no shapes selected"));
    }
    s.edit(|doc, sel| {
        let shapes = crate::shapes_mut(doc, sel).ok_or_else(|| bad(cmd, "no slide"))?;
        // Address every target before the first write: a plain shape in the selection must not
        // leave half the objects with a new scene.
        for id in &ids {
            let sh = deckcraft_model::find_shape(shapes, *id).ok_or_else(|| bad(cmd, format!("no such shape {}", id.0)))?;
            if !matches!(sh.kind, ShapeKind::Model3d { .. }) {
                return Err(bad(cmd, format!("shape {} is not a 3-D model", id.0)));
            }
        }
        for id in &ids {
            if let Some(sh) = deckcraft_model::find_shape_mut(shapes, *id)
                && let ShapeKind::Model3d { scene, .. } = &mut sh.kind
            {
                *scene = value;
            }
        }
        Ok(json!({"shapes": ids.len(), "scene": value}))
    })
}

fn camera(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "model3d.camera";
    let value = camera_param(p)?;
    let ids = targets(s, p)?;
    if ids.is_empty() {
        return Err(bad(cmd, "no shapes selected"));
    }
    s.edit(|doc, sel| {
        let shapes = crate::shapes_mut(doc, sel).ok_or_else(|| bad(cmd, "no slide"))?;
        // Address every target before the first write: a plain shape in the selection must not
        // leave half the objects with a new view.
        for id in &ids {
            let sh = deckcraft_model::find_shape(shapes, *id).ok_or_else(|| bad(cmd, format!("no such shape {}", id.0)))?;
            if !matches!(sh.kind, ShapeKind::Model3d { .. }) {
                return Err(bad(cmd, format!("shape {} is not a 3-D model", id.0)));
            }
        }
        for id in &ids {
            if let Some(sh) = deckcraft_model::find_shape_mut(shapes, *id)
                && let ShapeKind::Model3d { camera, .. } = &mut sh.kind
            {
                *camera = value;
            }
        }
        Ok(json!({"shapes": ids.len(), "camera": value.map(|c| json!({"fov": c.fov})).unwrap_or(Value::Null)}))
    })
}
