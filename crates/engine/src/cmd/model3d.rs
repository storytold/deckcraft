//! 3-D Model ▸ 3-D Model View: the camera a `.glb` model is shown from.
//!
//! A 3-D model's view is four final values — where the camera stands, what it looks at, which
//! way is up and the vertical field of view. There is deliberately no relative form: "turn 15°"
//! or "zoom in" applied by two replicas to their own views would end in two different views, so
//! the command accepts the whole resulting camera and refuses anything else before the first
//! write (the same rule as `transition.set`, RFC §3.2.1).

use deckcraft_model::{ModelCamera, ShapeKind};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_selection, targets};
use crate::{Result, Session};

/// Largest camera coordinate in EMU: a view a billion EMU (about 27 metres) from the model is
/// already far outside any slide, and larger numbers cannot come from a real file.
const MAX_EMU: f64 = 1.0e9;
/// Smallest field of view in degrees.
const MIN_FOV: f64 = 1.0;
/// Largest field of view in degrees (below a half turn, so the projection stays sane).
const MAX_FOV: f64 = 179.0;
/// Below this the up vector is treated as degenerate.
const MIN_UP: f64 = 1.0e-6;
/// Below this the up vector is treated as parallel to the view direction.
const MIN_SINE: f64 = 1.0e-6;

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        "model3d.camera",
        "3-D Model View",
        ["3-D Model"],
        None,
        "{camera: {position: [x, y, z] EMU, target: [x, y, z] EMU, up: [dx, dy, dz], fov: degrees 1–179} | {camera: null (the reader's default view), ids?}} — the whole final view; a relative edit (dx/dy/zoom/orbit) is refused before the first write",
        has_selection,
        camera
    )]
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
