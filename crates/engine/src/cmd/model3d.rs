//! Insert ▸ 3D Models and the 3D Model tab: inserting a model from a file, turning it (3D Model
//! Views, the rotation fields, the 3-D rotation handle) and resetting it.
//!
//! Models are kept as glTF binary, as in PowerPoint files: GLB files go in as they are, OBJ (with
//! its MTL and textures), STL, PLY, 3MF and glTF JSON files are converted when inserted.

use deckcraft_geom::Xfrm;
use deckcraft_geom::space::{self, Vec3};
use deckcraft_mesh::{Format, Scene};
use deckcraft_model::{Model3d, Presentation, Shape, ShapeId, ShapeKind};
use serde_json::{Value, json};

use super::*;
use crate::{EngineError, Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "insert.model3d",
            "3D Models from This Device…",
            ["Insert", "Illustrations", "3D Models"],
            None,
            "{path? | data: base64, name?: file name (its extension tells the format: glb|gltf|obj|stl|ply|3mf), files?: {name: base64} (an OBJ's .mtl and textures; read from the model's folder when `path` is given), rect?: [x,y,w,h]}",
            has_slide,
            insert
        ),
        cmd!(
            "model3d.rotate",
            "3D Model Rotation",
            ["3D Model", "3D Model Views"],
            None,
            "{x?, y?, z?: degrees (the model's rotation, X turns first), dx?, dy?, dz?: degrees more about the view's axes (x right, y up, z towards the viewer), ids?}",
            has_model,
            rotate
        ),
        cmd!(
            "model3d.view",
            "3D Model Views",
            ["3D Model", "3D Model Views"],
            None,
            "{view: front|back|left|right|top|bottom|aboveFrontLeft|… (model3d.views), ids?}",
            has_model,
            view
        ),
        cmd!(
            "model3d.reset",
            "Reset 3D Model",
            ["3D Model", "Adjust"],
            None,
            "{size?: bool (Reset 3D Model and Size: also the size it was inserted at), ids?}",
            has_model,
            reset
        ),
        cmd!(query "model3d.views", "3D Model Views Gallery", [], None, "{} → [{id, label, rotation: [x, y, z]}]", always, views),
        cmd!(
            query "shape.saveAsPicture",
            "Save as Picture…",
            [],
            None,
            "{path?: where to write the PNG (else it is returned), scale?: pixels per point (default 2), id?} → {name, data?: base64}: a 3-D model drawn on a transparent background, or a picture's own image",
            has_selection,
            save_as_picture
        ),
    ]
}

/// The 3D Model Views gallery: id, label, how far the view turns round the model to the left
/// (degrees) and how far above it looks down.
pub const VIEWS: &[(&str, &str, f64, f64)] = &[
    ("front", "Front", 0.0, 0.0),
    ("back", "Back", 180.0, 0.0),
    ("left", "Left", 90.0, 0.0),
    ("right", "Right", -90.0, 0.0),
    ("top", "Top", 0.0, 90.0),
    ("bottom", "Bottom", 0.0, -90.0),
    ("aboveFrontLeft", "Above Front Left", 35.0, 25.0),
    ("aboveFrontRight", "Above Front Right", -35.0, 25.0),
    ("belowFrontLeft", "Below Front Left", 35.0, -25.0),
    ("belowFrontRight", "Below Front Right", -35.0, -25.0),
    ("aboveBackLeft", "Above Back Left", 145.0, 25.0),
    ("aboveBackRight", "Above Back Right", -145.0, 25.0),
];

/// The model's rotation for a view `yaw` degrees round to its left and `pitch` degrees above.
pub fn view_rotation(yaw: f64, pitch: f64) -> Vec3 {
    space::turn(space::turn([0.0; 3], [0.0, 1.0, 0.0], yaw), [1.0, 0.0, 0.0], pitch)
}

/// Is `bytes` (named `name`) a 3-D model file Insert ▸ 3D Models reads?
pub fn is_model_file(name: &str, bytes: &[u8]) -> bool {
    deckcraft_mesh::is_model(name, bytes)
}

/// Does a model file of this name refer to other files (an OBJ's materials and textures, a PLY's
/// texture, a glTF's buffers and images)?
pub fn uses_companions(name: &str) -> bool {
    matches!(Format::from_name(name), Some(Format::Obj | Format::Ply | Format::Gltf))
}

pub fn has_model(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if s.active().is_some_and(|d| d.selected_shapes().iter().any(|x| matches!(x.kind, ShapeKind::Model3d(_)))) {
        Ok(())
    } else {
        Err("select a 3-D model".into())
    }
}

/// The mesh of the model on `sh`, read once and shared.
pub fn scene_of(doc: &Presentation, sh: &Shape) -> Option<std::sync::Arc<Scene>> {
    let ShapeKind::Model3d(m) = &sh.kind else { return None };
    deckcraft_render::model_scene(&doc.media(m.media)?.data)
}

/// Turn the model on `sh` to `rot` (degrees), starting from the frame `frame` and the field of
/// view `viewport` it had. When the model fits its frame (AutoFit) the frame follows its outline,
/// around the same centre, and the model keeps its size on the slide.
pub fn turn_to(sh: &mut Shape, frame: Xfrm, viewport: f64, rot: Vec3, scene: Option<&Scene>) {
    let ShapeKind::Model3d(m) = &mut sh.kind else { return };
    m.viewport = viewport;
    m.set_rotation(rot);
    sh.xfrm = Some(frame);
    if !m.auto_fit {
        return;
    }
    let Some(ext) = scene.and_then(|sc| deckcraft_mesh::extent(sc, &m.model_view(), m.is_ortho())) else { return };
    if let Some((w, h)) = m.refit(ext, frame.h) {
        let c = frame.center();
        sh.xfrm = Some(Xfrm { x: c.x - w / 2.0, y: c.y - h / 2.0, w, h, ..frame });
    }
}

/// Apply `f` (old rotation → new rotation) to every target model.
fn turn_models(s: &mut Session, p: &Value, cmd: &str, f: &dyn Fn(Vec3) -> Vec3) -> Result<Value> {
    let ids = targets(s, p)?;
    let st = s.doc()?;
    // Locked models don't turn; the meshes are read before the edit.
    let work: Vec<(ShapeId, Option<std::sync::Arc<Scene>>)> = ids
        .iter()
        .filter_map(|id| st.shape(*id))
        .filter(|sh| matches!(sh.kind, ShapeKind::Model3d(_)) && !sh.locked)
        .map(|sh| (sh.id, scene_of(&st.doc, sh)))
        .collect();
    if work.is_empty() {
        return Err(bad(cmd, if ids.is_empty() { "select a 3-D model" } else { "no unlocked 3-D model selected" }));
    }
    let boxes: Vec<(ShapeId, Xfrm)> = work.iter().filter_map(|(id, _)| st.shape(*id).map(|sh| (*id, xfrm_of(&st.doc, &st.selection, sh)))).collect();
    s.edit(|doc, sel| {
        let list = crate::shapes_mut(doc, sel).ok_or_else(|| bad(cmd, "no slide"))?;
        for (id, scene) in &work {
            let Some(sh) = deckcraft_model::find_shape_mut(list, *id) else { continue };
            let ShapeKind::Model3d(m) = &sh.kind else { continue };
            let (rot, viewport) = (f(m.rot), m.viewport);
            let frame = boxes.iter().find(|(i, _)| i == id).map(|(_, x)| *x).unwrap_or_default();
            turn_to(sh, frame, viewport, rot, scene.as_deref());
        }
        Ok(())
    })?;
    Ok(Value::from(work.len()))
}

/// Turn models `ids` about the view's axis like a shape turns on the slide: to `abs` degrees
/// clockwise, then `by` degrees more.
pub(crate) fn spin(s: &mut Session, ids: &[ShapeId], abs: Option<f64>, by: Option<f64>) -> Result<Value> {
    let p = json!({"ids": ids.iter().map(|i| i.0).collect::<Vec<_>>()});
    turn_models(s, &p, "shape.rotate", &|r| {
        let mut r = r;
        // Clockwise on the slide is negative about +Z, which points at the viewer.
        if let Some(a) = abs {
            r[2] = -a;
        }
        if let Some(b) = by {
            r = space::turn(r, [0.0, 0.0, 1.0], -b);
        }
        r
    })
}

fn rotate(s: &mut Session, p: &Value) -> Result<Value> {
    let abs = [f64_param(p, "x"), f64_param(p, "y"), f64_param(p, "z")];
    let rel = [f64_param(p, "dx"), f64_param(p, "dy"), f64_param(p, "dz")];
    if abs.iter().chain(rel.iter()).all(Option::is_none) {
        return Err(bad("model3d.rotate", "give x, y, z or dx, dy, dz in degrees"));
    }
    turn_models(s, p, "model3d.rotate", &|r| {
        let mut r = r;
        for k in 0..3 {
            if let Some(v) = abs[k] {
                r[k] = v;
            }
        }
        for (k, axis) in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]].into_iter().enumerate() {
            if let Some(d) = rel[k] {
                r = space::turn(r, axis, d);
            }
        }
        r
    })
}

fn save_as_picture(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "shape.saveAsPicture";
    let st = s.doc()?;
    let id = targets(s, p)?.first().copied().ok_or_else(|| bad(CMD, "nothing selected"))?;
    let sh = st.shape(id).ok_or_else(|| bad(CMD, "no such shape"))?;
    let name = if sh.name.is_empty() { "Picture".to_string() } else { sh.name.clone() };
    let (bytes, ext) = match &sh.kind {
        ShapeKind::Model3d(m) => {
            let x = xfrm_of(&st.doc, &st.selection, sh);
            let scale = f64_or(p, "scale", 2.0).clamp(0.1, 8.0);
            let png = deckcraft_render::model_png(&st.doc, m, x.w, x.h, scale)
                .ok_or_else(|| EngineError::Other(format!("{name}: the model can't be drawn")))?;
            (png, "png".to_string())
        }
        // A picture: its own image, as it was inserted.
        ShapeKind::Picture { fill } => {
            let item = st
                .doc
                .media(fill.media)
                .filter(|m| !m.data.is_empty())
                .ok_or_else(|| EngineError::Other(format!("{name}: the picture has no image")))?;
            let ext = item
                .name
                .rsplit_once('.')
                .map(|(_, e)| e.to_ascii_lowercase())
                .filter(|e| !e.is_empty() && e.len() <= 4 && e.chars().all(|c| c.is_ascii_alphanumeric()))
                .unwrap_or_else(|| "png".into());
            (item.data.to_vec(), ext)
        }
        _ => return Err(bad(CMD, "Save as Picture saves 3-D models and pictures")),
    };
    let file = format!("{name}.{ext}");
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(path) = str_param(p, "path") {
        crate::io::write_file(path, &bytes).map_err(EngineError::Other)?;
        return Ok(json!({"name": file, "path": path}));
    }
    Ok(json!({"name": file, "data": base64_encode(&bytes)}))
}

fn view(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "view").ok_or_else(|| bad("model3d.view", "missing `view`"))?;
    let (_, _, yaw, pitch) = *VIEWS
        .iter()
        .find(|v| v.0.eq_ignore_ascii_case(name))
        .ok_or_else(|| bad("model3d.view", format!("unknown view `{name}` (model3d.views)")))?;
    let r = view_rotation(yaw, pitch);
    turn_models(s, p, "model3d.view", &|_| r)
}

fn views(_s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(Value::Array(VIEWS.iter().map(|v| json!({"id": v.0, "label": v.1, "rotation": view_rotation(v.2, v.3)})).collect()))
}

fn reset(s: &mut Session, p: &Value) -> Result<Value> {
    if !bool_or(p, "size", false) {
        return turn_models(s, p, "model3d.reset", &|_| [0.0; 3]);
    }
    // Reset 3D Model and Size: as inserted (pose, camera, size), where it is now.
    let ids = targets(s, p)?;
    let st = s.doc()?;
    let size = st.doc.slide_size;
    let mut fresh: Vec<(ShapeId, Model3d, Xfrm)> = Vec::new();
    for sh in ids.iter().filter_map(|id| st.shape(*id)).filter(|sh| !sh.locked) {
        let ShapeKind::Model3d(m) = &sh.kind else { continue };
        let Some(scene) = scene_of(&st.doc, sh) else { continue };
        let Some((lo, hi)) = scene.bounds() else { continue };
        let mut n = Model3d { lights: m.lights.clone(), lights_xml: m.lights_xml.clone(), poster: m.poster, ..Model3d::fitted(m.media, lo, hi) };
        let Some((w, h)) = initial_frame(&mut n, &scene, default_span(size)) else { continue };
        let c = xfrm_of(&st.doc, &st.selection, sh).center();
        fresh.push((sh.id, n, Xfrm::new(c.x - w / 2.0, c.y - h / 2.0, w, h)));
    }
    if fresh.is_empty() {
        return Err(bad("model3d.reset", "no unlocked 3-D model selected"));
    }
    let n = fresh.len();
    s.edit(|doc, sel| {
        let list = crate::shapes_mut(doc, sel).ok_or_else(|| bad("model3d.reset", "no slide"))?;
        for (id, m, x) in fresh {
            if let Some(sh) = deckcraft_model::find_shape_mut(list, id) {
                sh.kind = ShapeKind::Model3d(Box::new(m));
                sh.xfrm = Some(x);
            }
        }
        Ok(())
    })?;
    Ok(Value::from(n))
}

/// The height on the slide a newly inserted model's field of view spans: 4/9 of the slide's width
/// (PowerPoint's on a 16:9 slide), at most most of the slide's height.
fn default_span(size: deckcraft_geom::Size) -> f64 {
    (size.width * 4.0 / 9.0).min(size.height * 0.9).max(1.0)
}

/// Size `m` so its field of view spans `span` points, and the frame that holds its outline.
fn initial_frame(m: &mut Model3d, scene: &Scene, span: f64) -> Option<(f64, f64)> {
    let ext = deckcraft_mesh::extent(scene, &m.model_view(), m.is_ortho())?;
    // A viewport of `span` over a one-point frame, refitted.
    m.viewport = span;
    m.refit(ext, 1.0)
}

/// A companion file of the model being inserted: one given with the command (by its path, then by
/// its file name), else one in the model's folder. Names that would leave the folder are not
/// followed, so a model file can't pull other files from the disk into the presentation.
fn companion(files: &[(String, Vec<u8>)], dir: Option<&std::path::Path>, name: &str) -> Option<Vec<u8>> {
    let norm = name.trim().replace('\\', "/");
    let base = norm.rsplit('/').next().unwrap_or(&norm).to_string();
    let file_name = |n: &str| n.replace('\\', "/").rsplit('/').next().unwrap_or("").to_string();
    if let Some((_, b)) = files
        .iter()
        .find(|(n, _)| n.replace('\\', "/").eq_ignore_ascii_case(&norm))
        .or_else(|| files.iter().find(|(n, _)| file_name(n).eq_ignore_ascii_case(&base)))
    {
        return Some(b.clone());
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(dir) = dir {
        use std::path::{Component, Path};
        let inside = |p: &Path| !p.as_os_str().is_empty() && p.components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
        let rel = Path::new(&norm);
        let tries = [inside(rel).then(|| dir.join(rel)), inside(Path::new(&base)).then(|| dir.join(&base))];
        // Through `crate::io`, so an automation read root confines them like any other read.
        for p in tries.into_iter().flatten() {
            let p = p.to_string_lossy();
            if crate::io::file_len(&p).is_some_and(|n| n <= 256 << 20)
                && let Ok(b) = crate::io::read_file(&p)
            {
                return Some(b);
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    let _ = dir;
    None
}

fn insert(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "insert.model3d";
    // `data` with `path`: the file's bytes, and where its companions are.
    let (name, bytes) = match (str_param(p, "data"), str_param(p, "path")) {
        (Some(d), Some(path)) => {
            let bytes = base64_decode(d).ok_or_else(|| bad(CMD, "invalid base64 `data`"))?;
            let n = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "model".into());
            (str_param(p, "name").map(String::from).unwrap_or(n), bytes)
        }
        _ => super::insert::media_bytes(p, CMD)?,
    };
    let dir = str_param(p, "path").and_then(|path| std::path::Path::new(path).parent().map(std::path::Path::to_path_buf));
    let files: Vec<(String, Vec<u8>)> = p
        .get("files")
        .and_then(Value::as_object)
        .map(|o| o.iter().take(256).filter_map(|(k, v)| Some((k.clone(), base64_decode(v.as_str()?)?))).collect())
        .unwrap_or_default();
    let format = Format::sniff(&name, &bytes).ok_or_else(|| bad(CMD, format!("{name} is not a 3-D model (GLB, glTF, OBJ, STL, PLY or 3MF)")))?;
    // Whether the model took anything from another file (a GLB can name external buffers).
    let used_companions = std::cell::Cell::new(false);
    let resolve = |n: &str| {
        let f = companion(&files, dir.as_deref(), n);
        used_companions.set(used_companions.get() || f.is_some());
        f
    };
    let scene = deckcraft_mesh::load(&name, &bytes, &resolve).map_err(|e| EngineError::Other(format!("{name}: {e}")))?;
    let (lo, hi) = scene.bounds().ok_or_else(|| EngineError::Other(format!("{name}: the model has no vertices")))?;
    // Kept as glTF binary: a GLB as it is, anything else converted.
    // A GLB stands alone and is kept as it is; anything that needed other files is converted.
    let glb = if format == Format::Glb && !used_companions.get() { bytes } else { deckcraft_mesh::to_glb(&scene) };
    let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(&name).to_string();
    let slide = s.doc()?.doc.slide_size;
    let mut m = Model3d::fitted(deckcraft_model::MediaId(0), lo, hi);
    let (w, h) = initial_frame(&mut m, &scene, default_span(slide)).ok_or_else(|| EngineError::Other(format!("{name}: the model can't be seen")))?;
    let rect = match rect_param(p, "rect") {
        // Into the given box: as large as fits, centred.
        Some(r) if r.w > 0.0 && r.h > 0.0 => {
            let k = (r.w / w).min(r.h / h);
            let (nw, nh) = (w * k, h * k);
            Xfrm::new(r.x + (r.w - nw) / 2.0, r.y + (r.h - nh) / 2.0, nw, nh)
        }
        _ => Xfrm::new((slide.width - w) / 2.0, (slide.height - h) / 2.0, w, h),
    };
    let media = s.edit(|doc, _| doc.add_media(&format!("{stem}.glb"), "model/gltf.binary", glb).ok_or_else(|| bad(CMD, super::NO_ID_SPACE)))?;
    m.media = media;
    let shape = Shape { xfrm: Some(rect), kind: ShapeKind::Model3d(Box::new(m)), descr: stem, ..Default::default() };
    let id = super::insert::add_shape(s, CMD, shape, true)?;
    Ok(json!({"id": id, "triangles": scene.triangle_count(), "textures": scene.textures.len()}))
}

#[cfg(test)]
#[path = "tests_model3d.rs"]
mod tests;
