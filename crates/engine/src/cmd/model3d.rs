//! Insert ▸ 3D Models: STL, OBJ (with its `.mtl` colours), FBX and glTF binary files, embedded as
//! `.glb` the way PowerPoint keeps them, so a saved `.pptx` carries a model PowerPoint can open.
//! The 3D Model tab turns them: named views, a turn about each axis, a drag-sized turn (what the
//! canvas's 3-D handle sends) and Reset.

use std::collections::HashMap;
use std::path::{Component, Path};

use deckcraft_geom::Xfrm;
use deckcraft_mesh::Format;
use deckcraft_mesh::raster::View;
use deckcraft_model::{MediaId, Model3dPose, Presentation, Shape, ShapeId, ShapeKind};
use serde_json::{Value, json};

use super::insert::{add_shape, media_bytes};
use super::*;
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "insert.model3d",
            "3D Model from File…",
            ["Insert", "Illustrations", "3D Models"],
            None,
            "{path? | data: base64, name?, rect?: [x,y,w,h]} (.glb, .fbx, .obj, .stl; an OBJ's .mtl colours are read from beside `path`) → {id, format, triangles}",
            has_slide,
            insert_model
        ),
        cmd!(
            "model3d.view",
            "3D Model Views",
            ["3D Model", "3D Model Views"],
            None,
            "{view: front|back|left|right|top|bottom|aboveFrontLeft|aboveFrontRight|belowFrontLeft|belowFrontRight (see model3d.views), ids?: [id]}",
            has_model,
            set_view
        ),
        cmd!(query "model3d.views", "3D Model View Gallery", [], None, "{} → [{name, label}]", always, list_views),
        cmd!(
            "model3d.rotate",
            "Rotate 3D Model",
            ["3D Model", "Arrange"],
            None,
            "{x?, y?, z?: degrees (turned about X, then Y, then Z; one left out is kept), ids?: [id]}",
            has_model,
            rotate
        ),
        cmd!("model3d.turn", "Turn 3D Model", [], None, "{dx?, dy?: points dragged on screen (half a degree each), ids?: [id]}", has_model, turn),
        cmd!(
            "model3d.reset",
            "Reset 3D Model",
            ["3D Model", "Adjust"],
            None,
            "{size?: bool (Reset 3D Model and Size), ids?: [id]}",
            has_model,
            reset
        ),
    ]
}

/// The 3D Model Views gallery: (name, label, yaw, pitch) for [`View::from_yaw_pitch`].
pub const VIEWS: &[(&str, &str, f64, f64)] = &[
    ("front", "Front", 0.0, 0.0),
    ("back", "Back", 180.0, 0.0),
    ("left", "Left", 90.0, 0.0),
    ("right", "Right", -90.0, 0.0),
    ("top", "Top", 0.0, 90.0),
    ("bottom", "Bottom", 0.0, -90.0),
    ("aboveFrontLeft", "Above Front Left", 30.0, 20.0),
    ("aboveFrontRight", "Above Front Right", -30.0, 20.0),
    ("belowFrontLeft", "Below Front Left", 30.0, -20.0),
    ("belowFrontRight", "Below Front Right", -30.0, -20.0),
];

/// Whether `name` / `bytes` look like a 3-D model file [`insert_model`] reads.
pub fn is_model(name: &str, bytes: &[u8]) -> bool {
    Format::detect(name, bytes).is_some()
}

/// The 3D Model tab's commands need a selected model.
pub fn has_model(s: &Session) -> std::result::Result<(), String> {
    let any = s.active().is_some_and(|d| d.selected_shapes().iter().any(|x| matches!(x.kind, ShapeKind::Model3d { .. })));
    if any { Ok(()) } else { Err("select a 3D model".into()) }
}

/// Reads files a model names next to itself (an OBJ's `mtllib`), only by plain relative names
/// inside the model's folder, and through the same access checks as every other read.
fn sidecar_reader(path: Option<&str>) -> impl FnMut(&str) -> Option<Vec<u8>> {
    let dir = path.and_then(|p| Path::new(p).parent()).map(Path::to_path_buf);
    move |name: &str| {
        let rel = Path::new(name);
        if name.is_empty() || !rel.components().all(|c| matches!(c, Component::Normal(_))) {
            log::warn!("insert.model3d: not reading `{name}` (only files beside the model are read)");
            return None;
        }
        let full = dir.as_ref()?.join(rel);
        crate::io::read_file(full.to_str()?).map_err(|e| log::info!("insert.model3d: {e}")).ok()
    }
}

/// A frame of `aspect` (width / height, kept within 1:4 either way) at half the slide's height,
/// inside the slide, centred on `center`.
fn sized_rect(slide_w: f64, slide_h: f64, aspect: Option<f64>, center: (f64, f64)) -> Xfrm {
    let aspect = aspect.filter(|a| a.is_finite() && *a > 0.0).unwrap_or(1.0).clamp(0.25, 4.0);
    let (mut w, mut h) = (slide_h * 0.5 * aspect, slide_h * 0.5);
    if w > slide_w * 0.9 {
        w = slide_w * 0.9;
        h = w / aspect;
    }
    Xfrm::new(center.0 - w / 2.0, center.1 - h / 2.0, w, h)
}

/// The frame for a model whose inserted view is `view_size` (width, height): see [`sized_rect`],
/// centred on the slide.
fn default_rect(slide_w: f64, slide_h: f64, view_size: Option<(f32, f32)>) -> Xfrm {
    sized_rect(slide_w, slide_h, view_size.map(|(w, h)| w as f64 / h as f64), (slide_w / 2.0, slide_h / 2.0))
}

/// The pose a newly inserted model gets: [`View::inserted`], and PowerPoint's scale (its largest
/// side one metre).
fn inserted_pose(bounds: Option<deckcraft_mesh::Bounds>) -> Model3dPose {
    let largest = bounds.map(|b| b.size().into_iter().fold(0f32, f32::max) as f64).filter(|v| v.is_finite() && *v > 0.0);
    Model3dPose {
        rot: View::inserted().rot,
        meter_per_unit: largest.map(|l| (((1e6 / l).round().clamp(1.0, u64::MAX as f64 / 2.0)) as u64, 1_000_000)),
        ..Default::default()
    }
}

/// Fallback pictures are drawn at 2 px per point, their longer side within these bounds.
const POSTER_MIN: f64 = 256.0;
const POSTER_MAX: f64 = 1024.0;

/// Pixel size of the fallback picture for a frame `w` × `h` points.
fn poster_size(w: f64, h: f64) -> (u32, u32) {
    let (w, h) = if w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0 { (w, h) } else { (1.0, 1.0) };
    let k = (POSTER_MAX / w.max(h)).min((2.0f64).max(POSTER_MIN / w.max(h)));
    (((w * k).round() as u32).clamp(1, POSTER_MAX as u32), ((h * k).round() as u32).clamp(1, POSTER_MAX as u32))
}

/// The model in `glb` drawn in `pose` on a transparent `w` × `h` PNG, as older readers show it in
/// place of the model.
pub fn poster_png(glb: &[u8], pose: &Model3dPose, w: u32, h: u32) -> Option<Vec<u8>> {
    let scene = deckcraft_mesh::glb::read(glb).ok()?;
    let img = deckcraft_mesh::raster::render(&scene, &deckcraft_render::model3d_view(pose), w, h)?;
    let img = image::RgbaImage::from_raw(img.width, img.height, img.rgba)?;
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

/// Calls `f` on every 3-D model in `shapes` (inside groups too) that has no fallback picture.
fn bare_models<'a>(shapes: &'a [Shape], f: &mut dyn FnMut(&'a Shape)) {
    for s in shapes {
        match &s.kind {
            ShapeKind::Group { children, .. } => bare_models(children, f),
            ShapeKind::Model3d { poster: None, .. } => f(s),
            _ => {}
        }
    }
}

fn bare_models_mut(shapes: &mut [Shape], f: &mut dyn FnMut(&mut Shape)) {
    for s in shapes {
        match &mut s.kind {
            ShapeKind::Group { children, .. } => bare_models_mut(children, f),
            ShapeKind::Model3d { poster: None, .. } => f(s),
            _ => {}
        }
    }
}

/// A fallback picture is shared by models with the same file, frame size and pose.
type PosterKey = (MediaId, (u32, u32), String);

fn poster_key(s: &Shape) -> Option<(PosterKey, Model3dPose)> {
    let ShapeKind::Model3d { media, pose, .. } = &s.kind else { return None };
    let size = s.xfrm.map_or((512, 512), |x| poster_size(x.w, x.h));
    Some(((*media, size, format!("{:?}{:?}", pose.rot, pose.camera)), pose.clone()))
}

/// `doc` with a fallback picture drawn for every 3-D model on its slides that has none, for a
/// `.pptx` (PowerPoint keeps one beside each model for readers that can't show models).
/// `None` when every model already has one, or there are none.
pub fn with_posters(doc: &Presentation) -> Option<Presentation> {
    let mut wanted: Vec<(PosterKey, Model3dPose)> = Vec::new();
    for slide in &doc.slides {
        bare_models(&slide.shapes, &mut |s| {
            if let Some((k, pose)) = poster_key(s)
                && !wanted.iter().any(|(w, _)| *w == k)
            {
                wanted.push((k, pose));
            }
        });
    }
    if wanted.is_empty() {
        return None;
    }
    let mut out = doc.clone();
    let mut made: HashMap<PosterKey, MediaId> = HashMap::new();
    for (key, pose) in wanted {
        let (media, (w, h), _) = &key;
        let Some(item) = out.media(*media) else { continue };
        let stem = Path::new(&item.name).file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "model".into());
        let Some(png) = poster_png(&item.data, &pose, *w, *h) else {
            log::warn!("export: no fallback picture for 3-D model {}", item.name);
            continue;
        };
        if let Some(id) = out.add_media(&format!("{stem}-poster.png"), "image/png", png) {
            made.insert(key, id);
        }
    }
    if made.is_empty() {
        return None;
    }
    for slide in &mut out.slides {
        let mut has = false;
        bare_models(&slide.shapes, &mut |s| has |= poster_key(s).is_some_and(|(k, _)| made.contains_key(&k)));
        if !has {
            continue;
        }
        bare_models_mut(&mut std::sync::Arc::make_mut(slide).shapes, &mut |s| {
            let id = poster_key(s).and_then(|(k, _)| made.get(&k).copied());
            if let (ShapeKind::Model3d { poster, .. }, Some(id)) = (&mut s.kind, id) {
                *poster = Some(id);
            }
        });
    }
    Some(out)
}

fn insert_model(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "insert.model3d";
    let (name, bytes) = media_bytes(p, CMD)?;
    let imported = deckcraft_mesh::import(&name, &bytes, &mut sidecar_reader(str_param(p, "path"))).map_err(|e| bad(CMD, format!("{name}: {e}")))?;
    let size = s.doc()?.doc.slide_size;
    let rect = rect_param(p, "rect").unwrap_or_else(|| default_rect(size.width, size.height, imported.view_size));
    let stem = Path::new(&name).file_stem().map(|n| n.to_string_lossy().into_owned()).filter(|n| !n.is_empty()).unwrap_or_else(|| "model".into());
    let part_name = format!("{stem}.glb");
    let pose = inserted_pose(imported.bounds);
    let media = s.edit(|doc, _| doc.add_media(&part_name, "model/gltf-binary", imported.glb).ok_or_else(|| bad(CMD, NO_ID_SPACE)))?;
    let shape = Shape { xfrm: Some(rect), kind: ShapeKind::Model3d { media, poster: None, pose }, descr: name, ..Default::default() };
    let id = add_shape(s, CMD, shape, true)?;
    Ok(json!({"id": id, "format": imported.format.name(), "triangles": imported.triangles}))
}

/// The models a 3D Model command acts on: `ids`, else the selected models.
fn targets(s: &Session, p: &Value, cmd: &str) -> Result<Vec<ShapeId>> {
    let d = s.doc()?;
    let ids = ids_param(p, "ids").unwrap_or_else(|| d.selection.shapes.clone());
    let models: Vec<ShapeId> = ids.into_iter().filter(|id| d.shape(*id).is_some_and(|x| matches!(x.kind, ShapeKind::Model3d { .. }))).collect();
    if models.is_empty() {
        return Err(bad(cmd, "no 3D model selected"));
    }
    Ok(models)
}

/// Changes the pose of each model in `ids`; a fallback picture of the old pose is dropped (a
/// `.pptx` save draws a new one).
fn change_pose(s: &mut Session, cmd: &str, ids: &[ShapeId], mut f: impl FnMut(&mut Model3dPose, &mut Option<Xfrm>)) -> Result<Value> {
    s.edit(|doc, sel| {
        let list = crate::shapes_mut(doc, sel).ok_or_else(|| bad(cmd, "no slide"))?;
        for id in ids {
            if let Some(sh) = deckcraft_model::find_shape_mut(list, *id)
                && let ShapeKind::Model3d { pose, poster, .. } = &mut sh.kind
            {
                f(pose, &mut sh.xfrm);
                *poster = None;
            }
        }
        Ok(())
    })?;
    let rots: Vec<Value> = {
        let d = s.doc()?;
        ids.iter()
            .filter_map(|id| d.shape(*id))
            .filter_map(|x| if let ShapeKind::Model3d { pose, .. } = &x.kind { Some(json!({"id": x.id, "rot": pose.rot})) } else { None })
            .collect()
    };
    Ok(json!({"models": rots}))
}

fn set_view(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "model3d.view";
    let name = str_param(p, "view").ok_or_else(|| bad(CMD, "missing `view`"))?;
    let &(_, _, yaw, pitch) = VIEWS.iter().find(|v| v.0 == name).ok_or_else(|| bad(CMD, format!("unknown view `{name}` (see model3d.views)")))?;
    let ids = targets(s, p, CMD)?;
    let rot = View::from_yaw_pitch(yaw, pitch).rot;
    change_pose(s, CMD, &ids, |pose, _| {
        pose.rot = rot;
        pose.camera = None;
    })
}

fn list_views(_s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(Value::Array(VIEWS.iter().map(|v| json!({"name": v.0, "label": v.1})).collect()))
}

fn angle(p: &Value, key: &str, cmd: &str) -> Result<Option<f64>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_f64()
            .filter(|a| a.is_finite())
            .map(|a| Some(a.rem_euclid(360.0)))
            .ok_or_else(|| bad(cmd, format!("`{key}` must be a number of degrees"))),
    }
}

fn rotate(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "model3d.rotate";
    let (x, y, z) = (angle(p, "x", CMD)?, angle(p, "y", CMD)?, angle(p, "z", CMD)?);
    let ids = targets(s, p, CMD)?;
    change_pose(s, CMD, &ids, |pose, _| {
        for (slot, v) in pose.rot.iter_mut().zip([x, y, z]) {
            if let Some(v) = v {
                *slot = v;
            }
        }
    })
}

fn turn(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "model3d.turn";
    let num = |k: &str| p.get(k).and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(0.0).clamp(-1e5, 1e5);
    let (dx, dy) = (num("dx"), num("dy"));
    let ids = targets(s, p, CMD)?;
    change_pose(s, CMD, &ids, |pose, _| {
        pose.rot = deckcraft_render::model3d_view(pose).turned(dx, dy).rot;
    })
}

fn reset(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "model3d.reset";
    let size = p.get("size").and_then(Value::as_bool).unwrap_or(false);
    let ids = targets(s, p, CMD)?;
    // The size each model's inserted view asks for, measured before the edit.
    let (slide, aspects): (deckcraft_model::Size, HashMap<ShapeId, Option<f64>>) = {
        let d = s.doc()?;
        let aspects = ids
            .iter()
            .map(|id| {
                let a = d.shape(*id).and_then(|x| match &x.kind {
                    ShapeKind::Model3d { media, .. } => d.doc.media(*media),
                    _ => None,
                });
                let a = a
                    .and_then(|m| deckcraft_mesh::glb::read(&m.data).ok())
                    .and_then(|sc| deckcraft_mesh::raster::projected_size(&sc, &View::inserted()));
                (*id, a.map(|(w, h)| w as f64 / h as f64))
            })
            .collect();
        (d.doc.slide_size, aspects)
    };
    let mut i = 0;
    change_pose(s, CMD, &ids, |pose, xfrm| {
        pose.rot = View::inserted().rot;
        pose.camera = None;
        if size && let Some(x) = xfrm.as_mut() {
            let aspect = ids.get(i).and_then(|id| aspects.get(id).copied().flatten());
            let c = x.center();
            let r = sized_rect(slide.width, slide.height, aspect, (c.x, c.y));
            (x.x, x.y, x.w, x.h) = (r.x, r.y, r.w, r.h);
        }
        i += 1;
    })
}

#[cfg(test)]
mod tests {
    use deckcraft_model::ShapeKind;

    use super::*;

    const OBJ: &str = "mtllib parts.mtl\nv 0 0 0\nv 2 0 0\nv 2 1 0\nv 0 1 0\nusemtl Teal\nf 1 2 3 4\n";
    const STL: &str = "solid t\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 0 3\nendloop\nendfacet\nendsolid t\n";

    fn session() -> Session {
        let mut s = Session::with_new();
        s.execute("slide.new", &json!({"layout": "blank"})).expect("slide");
        s
    }

    fn models(s: &Session) -> Vec<Shape> {
        s.doc()
            .ok()
            .and_then(|d| d.current_slide().map(|sl| sl.shapes.iter().filter(|x| matches!(x.kind, ShapeKind::Model3d { .. })).cloned().collect()))
            .unwrap_or_default()
    }

    fn glb_of(s: &Session, shape: &Shape) -> Vec<u8> {
        let ShapeKind::Model3d { media, .. } = shape.kind else { return Vec::new() };
        s.doc().ok().and_then(|d| d.doc.media(media)).map(|m| m.data.to_vec()).unwrap_or_default()
    }

    #[test]
    fn inserts_a_model_as_glb_shaped_like_its_front_view() {
        let mut s = session();
        let r = s.execute("insert.model3d", &json!({"name": "bracket.obj", "data": base64_encode(OBJ.as_bytes())})).expect("insert");
        assert_eq!((r["format"].as_str(), r["triangles"].as_u64()), (Some("OBJ"), Some(2)));
        let m = models(&s);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].name, "3D Model 1");
        assert_eq!(m[0].descr, "bracket.obj");
        let x = m[0].xfrm.expect("xfrm");
        let scene = deckcraft_mesh::glb::read(&glb_of(&s, &m[0])).expect("glb");
        let (vw, vh) = deckcraft_mesh::raster::projected_size(&scene, &deckcraft_mesh::raster::View::inserted()).expect("size");
        assert!((x.w / x.h - vw as f64 / vh as f64).abs() < 1e-6, "shaped like the default view: {x:?}");
        assert!(x.w > x.h, "a 2 × 1 plate stays wider than tall");
        let size = s.doc().expect("doc").doc.slide_size;
        assert!((x.x + x.w / 2.0 - size.width / 2.0).abs() < 1e-9 && (x.h - size.height * 0.5).abs() < 1e-9, "centred, half the slide high");
        let item = s.doc().ok().and_then(|d| match m[0].kind {
            ShapeKind::Model3d { media, .. } => d.doc.media(media).map(|i| (i.name.clone(), i.content_type.clone())),
            _ => None,
        });
        assert_eq!(item, Some(("bracket.glb".into(), "model/gltf-binary".into())));
        assert_eq!(deckcraft_mesh::glb::read(&glb_of(&s, &m[0])).map(|g| g.triangle_count()), Ok(2));
        assert!(s.doc().is_ok_and(|d| d.selection.shapes.contains(&m[0].id)), "the new model is selected");

        s.execute("edit.undo", &json!({})).expect("undo");
        assert!(models(&s).is_empty(), "one undo takes it out");
    }

    #[test]
    fn each_format_and_an_explicit_rect() {
        let mut s = session();
        let r =
            s.execute("insert.model3d", &json!({"name": "tall.stl", "data": base64_encode(STL.as_bytes()), "rect": [10, 20, 30, 40]})).expect("stl");
        assert_eq!(r["format"], "STL");
        let x = models(&s)[0].xfrm.expect("xfrm");
        assert_eq!((x.x, x.y, x.w, x.h), (10.0, 20.0, 30.0, 40.0));
        // A .glb is embedded as it is.
        let glb = glb_of(&s, &models(&s)[0]);
        let r = s.execute("insert.model3d", &json!({"name": "again.glb", "data": base64_encode(&glb)})).expect("glb");
        assert_eq!(r["format"], "glTF binary");
        assert_eq!(glb_of(&s, &models(&s)[1]), glb);
    }

    #[test]
    fn obj_colours_come_from_the_mtl_beside_it() {
        let dir = std::env::temp_dir().join(format!("deckcraft-model3d-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("bracket.obj"), OBJ).expect("obj");
        std::fs::write(dir.join("parts.mtl"), "newmtl Teal\nKd 0 0.5 0.5\n").expect("mtl");
        let mut s = session();
        let path = dir.join("bracket.obj");
        s.execute("insert.model3d", &json!({"path": path.to_str()})).expect("insert");
        let scene = deckcraft_mesh::glb::read(&glb_of(&s, &models(&s)[0])).expect("glb");
        assert_eq!(scene.primitives[0].color, [0.0, 0.5, 0.5, 1.0]);

        // Library names that leave the folder are not followed.
        std::fs::write(dir.join("escape.obj"), OBJ.replace("parts.mtl", "../parts.mtl")).expect("obj");
        let path = dir.join("escape.obj");
        s.execute("insert.model3d", &json!({"path": path.to_str()})).expect("insert");
        let scene = deckcraft_mesh::glb::read(&glb_of(&s, &models(&s)[1])).expect("glb");
        assert_eq!(scene.primitives[0].color, deckcraft_mesh::DEFAULT_COLOR);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unreadable_files_are_refused_and_change_nothing() {
        let mut s = session();
        for (name, data) in [
            ("notes.txt", &b"hello"[..]),
            ("broken.obj", b"v 0 0 0\nf 1 2 3\n"),
            ("empty.stl", b"solid x\nendsolid x\n"),
            ("x.glb", b"glTF\x02\0\0\0"),
        ] {
            let e = s.execute("insert.model3d", &json!({"name": name, "data": base64_encode(data)})).expect_err(name);
            assert!(e.to_string().contains(name), "{name}: {e}");
        }
        assert!(s.execute("insert.model3d", &json!({})).is_err());
        assert!(models(&s).is_empty());
    }

    #[test]
    fn a_model_survives_a_pptx_round_trip() {
        let mut s = session();
        s.execute("insert.model3d", &json!({"name": "bracket.obj", "data": base64_encode(OBJ.as_bytes())})).expect("insert");
        let glb = glb_of(&s, &models(&s)[0]);
        let bytes = deckcraft_pptx::export(&s.doc().expect("doc").doc).expect("export");
        let back = deckcraft_pptx::import(&bytes).expect("import");
        let shapes: Vec<&Shape> = back.slides.iter().flat_map(|sl| &sl.shapes).filter(|x| matches!(x.kind, ShapeKind::Model3d { .. })).collect();
        assert_eq!(shapes.len(), 1);
        let ShapeKind::Model3d { media, .. } = shapes[0].kind else { return };
        assert_eq!(back.media(media).map(|m| m.data.to_vec()), Some(glb));
    }

    #[test]
    fn default_rect_keeps_odd_shapes_on_the_slide() {
        let b = |x: f32, y: f32| Some((x, y));
        for bounds in [b(100.0, 1.0), b(1.0, 100.0), b(1.0, 0.0), b(0.0, 0.0), b(f32::NAN, 1.0), None] {
            let r = default_rect(960.0, 540.0, bounds);
            assert!(r.x >= 0.0 && r.y >= 0.0 && r.x + r.w <= 960.0 && r.y + r.h <= 540.0 && r.w > 0.0 && r.h > 0.0, "{bounds:?}: {r:?}");
        }
    }

    #[test]
    fn poster_sizes_follow_the_frame() {
        assert_eq!(poster_size(260.0, 300.0), (520, 600));
        assert_eq!(poster_size(10.0, 5.0), (256, 128));
        assert_eq!(poster_size(2000.0, 1000.0), (1024, 512));
        assert_eq!(poster_size(f64::NAN, 0.0), (256, 256));
        assert_eq!(poster_size(1e6, 1e-6), (1024, 1));
    }

    fn pptx_parts(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
        use std::io::Read;
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip");
        (0..z.len())
            .map(|i| {
                let mut f = z.by_index(i).expect("entry");
                let mut v = Vec::new();
                f.read_to_end(&mut v).expect("read");
                (f.name().to_string(), v)
            })
            .collect()
    }

    #[test]
    fn a_saved_pptx_carries_a_fallback_picture_of_the_model() {
        let mut s = session();
        s.execute("insert.model3d", &json!({"name": "bracket.obj", "data": base64_encode(OBJ.as_bytes()), "rect": [100, 100, 200, 100]}))
            .expect("insert");
        let doc = s.doc().expect("doc").doc.clone();
        let bytes = super::super::file::save_bytes(&doc, "pptx").expect("save");
        assert!(matches!(models(&s)[0].kind, ShapeKind::Model3d { poster: None, .. }), "the open presentation is left as it was");

        let parts = pptx_parts(&bytes);
        let slide =
            parts.iter().find(|(n, x)| n.starts_with("ppt/slides/slide") && String::from_utf8_lossy(x).contains("am3d:model3d")).expect("slide");
        let xml = String::from_utf8_lossy(&slide.1);
        assert!(xml.contains("mc:AlternateContent") && xml.contains("mc:Fallback") && xml.contains("p:pic"), "{xml}");
        let png = parts.iter().find(|(n, _)| n.starts_with("ppt/media/") && n.ends_with(".png")).expect("poster part");
        let img = image::load_from_memory(&png.1).expect("png").to_rgba8();
        assert_eq!(img.dimensions(), (400, 200), "2 px per point, the frame's shape");
        assert_eq!(img.get_pixel(200, 100)[3], 255, "the model is in the middle");
        assert_eq!(img.get_pixel(2, 2)[3], 0, "around it the picture is see-through");

        // Opened again, the model and its picture come back; saving again adds no second picture.
        let back = deckcraft_pptx::import(&bytes).expect("import");
        let model = back.slides.iter().flat_map(|sl| &sl.shapes).find(|x| matches!(x.kind, ShapeKind::Model3d { .. })).expect("model");
        assert!(matches!(model.kind, ShapeKind::Model3d { poster: Some(_), .. }), "{:?}", model.kind);
        assert!(with_posters(&back).is_none());
    }

    #[test]
    fn models_in_groups_get_pictures_and_unreadable_ones_are_saved_without() {
        let mut s = session();
        s.execute("insert.model3d", &json!({"name": "a.stl", "data": base64_encode(STL.as_bytes())})).expect("a");
        s.execute("insert.textBox", &json!({"text": "label"})).expect("text");
        s.execute("edit.selectAll", &json!({})).expect("select");
        s.execute("arrange.group", &json!({})).expect("group");
        // A .glb whose mesh can't be read (compressed) is kept, drawn as a box, and saved bare.
        let json = br#"{"asset":{"version":"2.0"},"meshes":[{"primitives":[{"attributes":{},"extensions":{"KHR_draco_mesh_compression":{}}}]}]}  "#;
        let mut glb = b"glTF".to_vec();
        glb.extend(2u32.to_le_bytes());
        glb.extend(((12 + 8 + json.len()) as u32).to_le_bytes());
        glb.extend((json.len() as u32).to_le_bytes());
        glb.extend(0x4E4F_534Au32.to_le_bytes());
        glb.extend(json);
        s.execute("insert.model3d", &json!({"name": "draco.glb", "data": base64_encode(&glb)})).expect("draco");
        let out = with_posters(&s.doc().expect("doc").doc).expect("one picture made");
        let mut posters = Vec::new();
        for slide in &out.slides {
            fn walk(shapes: &[Shape], out: &mut Vec<bool>) {
                for x in shapes {
                    match &x.kind {
                        ShapeKind::Group { children, .. } => walk(children, out),
                        ShapeKind::Model3d { poster, .. } => out.push(poster.is_some()),
                        _ => {}
                    }
                }
            }
            walk(&slide.shapes, &mut posters);
        }
        posters.sort();
        assert_eq!(posters, vec![false, true], "the grouped STL has one, the unreadable model none");
        assert!(super::super::file::save_bytes(&s.doc().expect("doc").doc, "pptx").is_ok());
    }

    fn pose_of(s: &Session, i: usize) -> Model3dPose {
        match &models(s)[i].kind {
            ShapeKind::Model3d { pose, .. } => pose.clone(),
            _ => Model3dPose::default(),
        }
    }

    fn close(a: [f64; 3], b: [f64; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6)
    }

    #[test]
    fn a_new_model_is_turned_three_quarters_and_sized_one_metre() {
        let mut s = session();
        s.execute("insert.model3d", &json!({"name": "plate.obj", "data": base64_encode(OBJ.as_bytes())})).expect("insert");
        let pose = pose_of(&s, 0);
        assert!(close(pose.rot, View::inserted().rot), "{:?}", pose.rot);
        assert_eq!(pose.meter_per_unit, Some((500_000, 1_000_000)), "its 2-unit side is one metre");
        assert_eq!(pose.camera, None);
    }

    #[test]
    fn views_turns_and_reset() {
        let mut s = session();
        s.execute("insert.model3d", &json!({"name": "plate.obj", "data": base64_encode(OBJ.as_bytes())})).expect("insert");
        let names: Vec<String> = s
            .execute("model3d.views", &json!({}))
            .expect("views")
            .as_array()
            .expect("list")
            .iter()
            .filter_map(|v| v["name"].as_str().map(String::from))
            .collect();
        assert_eq!(names.len(), VIEWS.len());

        s.execute("model3d.view", &json!({"view": "left"})).expect("left");
        assert!(close(pose_of(&s, 0).rot, [0.0, 90.0, 0.0]));
        s.execute("model3d.view", &json!({"view": "top"})).expect("top");
        assert!(close(pose_of(&s, 0).rot, [90.0, 0.0, 0.0]));
        assert!(s.execute("model3d.view", &json!({"view": "sideways"})).is_err());

        // Absolute angles, the rest kept; negative and large ones wrap.
        s.execute("model3d.rotate", &json!({"y": -30, "z": 370})).expect("rotate");
        assert!(close(pose_of(&s, 0).rot, [90.0, 330.0, 10.0]), "{:?}", pose_of(&s, 0).rot);
        assert!(s.execute("model3d.rotate", &json!({"x": "a lot"})).is_err());

        // A drag: 180 pt right from the front is a quarter turn.
        s.execute("model3d.view", &json!({"view": "front"})).expect("front");
        s.execute("model3d.turn", &json!({"dx": 180})).expect("turn");
        assert!(close(pose_of(&s, 0).rot, [0.0, 90.0, 0.0]), "{:?}", pose_of(&s, 0).rot);

        // Reset: the inserted view; with size, the frame takes that view's shape about its centre.
        s.execute("shape.setBounds", &json!({"id": models(&s)[0].id, "x": 100, "y": 100, "w": 50, "h": 300})).expect("squash");
        s.execute("model3d.reset", &json!({})).expect("reset");
        assert!(close(pose_of(&s, 0).rot, View::inserted().rot));
        assert_eq!(models(&s)[0].xfrm.map(|x| (x.w, x.h)), Some((50.0, 300.0)), "a plain reset keeps the size");
        s.execute("model3d.reset", &json!({"size": true})).expect("reset and size");
        let x = models(&s)[0].xfrm.expect("xfrm");
        assert!((x.center().x - 125.0).abs() < 1e-9 && (x.center().y - 250.0).abs() < 1e-9 && x.w > x.h, "{x:?}");

        // Each step is one undo.
        s.execute("edit.undo", &json!({})).expect("undo");
        assert_eq!(models(&s)[0].xfrm.map(|x| (x.w, x.h)), Some((50.0, 300.0)));
    }

    #[test]
    fn turning_drops_a_stale_fallback_picture_and_needs_a_model() {
        let mut s = session();
        assert!(s.execute("model3d.turn", &json!({"dx": 10})).is_err(), "nothing selected");
        s.execute("insert.textBox", &json!({"text": "not a model"})).expect("text");
        assert!(crate::cmd::command_specs().iter().find(|c| c.id == "model3d.turn").is_some_and(|c| (c.enabled)(&s).is_err()));
        s.execute("insert.model3d", &json!({"name": "plate.obj", "data": base64_encode(OBJ.as_bytes())})).expect("insert");
        let id = models(&s)[0].id;
        let poster = s.edit(|doc, _| Ok(doc.add_media("p.png", "image/png", vec![1, 2, 3]))).expect("media");
        s.edit(|doc, sel| {
            if let Some(ShapeKind::Model3d { poster: slot, .. }) =
                crate::shapes_mut(doc, sel).and_then(|l| deckcraft_model::find_shape_mut(l, id)).map(|x| &mut x.kind)
            {
                *slot = poster;
            }
            Ok(())
        })
        .expect("poster");
        s.execute("model3d.turn", &json!({"ids": [id.0], "dy": 20})).expect("turn by id");
        assert!(matches!(models(&s)[0].kind, ShapeKind::Model3d { poster: None, .. }), "the old picture no longer matches");
    }

    #[test]
    fn a_turned_model_saves_its_pose_and_matching_picture() {
        let mut s = session();
        s.execute("insert.model3d", &json!({"name": "plate.obj", "data": base64_encode(OBJ.as_bytes()), "rect": [100, 100, 200, 100]}))
            .expect("insert");
        let front = super::super::file::save_bytes(&s.doc().expect("doc").doc, "pptx").expect("save");
        s.execute("model3d.view", &json!({"view": "back"})).expect("back");
        let back = super::super::file::save_bytes(&s.doc().expect("doc").doc, "pptx").expect("save");
        let png = |bytes: &[u8]| pptx_parts(bytes).into_iter().find(|(n, _)| n.starts_with("ppt/media/") && n.ends_with(".png")).map(|p| p.1);
        assert_ne!(png(&front), png(&back), "the fallback picture follows the pose");
        let read = deckcraft_pptx::import(&back).expect("import");
        let model = read.slides.iter().flat_map(|sl| &sl.shapes).find(|x| matches!(x.kind, ShapeKind::Model3d { .. })).expect("model");
        let ShapeKind::Model3d { pose, .. } = &model.kind else { return };
        assert!(close(pose.rot, [0.0, 180.0, 0.0]), "{:?}", pose.rot);
    }

    #[test]
    fn dragging_the_middle_turns_the_model_in_one_undo_step() {
        use crate::tools::{PointerEvent, PointerKind};
        let mut s = session();
        s.execute("insert.model3d", &json!({"name": "plate.obj", "data": base64_encode(OBJ.as_bytes()), "rect": [100, 100, 200, 100]}))
            .expect("insert");
        s.execute("model3d.view", &json!({"view": "front"})).expect("front");
        let before = models(&s)[0].xfrm;
        let ev = |kind, x, y| PointerEvent { kind, x, y, tol: 4.0, ..Default::default() };
        assert_eq!(s.pointer(ev(PointerKind::Move, 200.0, 150.0)).ok().map(|_| ()), Some(()));
        assert!(matches!(s.tool.hover, Some(crate::tools::Hit::Model3dTurn { .. })), "{:?}", s.tool.hover);
        s.pointer(ev(PointerKind::Down, 200.0, 150.0)).expect("down");
        s.pointer(ev(PointerKind::Drag, 260.0, 150.0)).expect("drag");
        s.pointer(ev(PointerKind::Drag, 380.0, 150.0)).expect("drag");
        s.pointer(ev(PointerKind::Up, 380.0, 150.0)).expect("up");
        assert!(close(pose_of(&s, 0).rot, [0.0, 90.0, 0.0]), "180 pt right is a quarter turn: {:?}", pose_of(&s, 0).rot);
        assert_eq!(models(&s)[0].xfrm, before, "the frame doesn't move");
        s.execute("edit.undo", &json!({})).expect("undo");
        assert!(close(pose_of(&s, 0).rot, [0.0; 3]), "one undo puts the whole drag back: {:?}", pose_of(&s, 0).rot);
        // Pressing elsewhere on the model still moves it.
        s.pointer(ev(PointerKind::Down, 120.0, 110.0)).expect("down");
        s.pointer(ev(PointerKind::Drag, 140.0, 110.0)).expect("drag");
        s.pointer(ev(PointerKind::Up, 140.0, 110.0)).expect("up");
        assert_eq!(models(&s)[0].xfrm.map(|x| x.x), Some(120.0));
    }
}
