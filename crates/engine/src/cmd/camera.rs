//! Camera objects (Camera Format tab): inserting a Cameo picture and the nine Camera Styles.
//!
//! A Cameo is an ordinary picture shape with [`deckcraft_model::Cameo`] set: its frame, fill,
//! outline, effects and crop are the picture's, so every picture command works on it unchanged.
//! Live capture and background effects are separate layers; this module owns the model-side object
//! and its drawing styles only.

use std::sync::Arc;

use deckcraft_color::Rgba;
use deckcraft_geom::Xfrm;
use deckcraft_model::style::{Effects, PictureFill, Shadow};
use deckcraft_model::{Cameo, ColorRef, Fill, Geom, Line, Shape, ShapeId, ShapeKind};
use serde_json::{Value, json};

use super::edit::walk_mut;
use super::insert::first_free_name;
use super::{CommandSpec, NO_ID_SPACE, always, bad, bool_or, cmd, edit_shapes, has_doc, has_slide, opt_usize_param, rect_param, str_param, targets};
use crate::{Result, Session};

/// Default side of an inserted cameo: 2.25 in.
pub const DEFAULT_SIZE: f64 = 162.0;
/// Gap between the cameo and the slide's right and bottom edges.
pub const DEFAULT_MARGIN: f64 = 6.5;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!("insert.cameo", "Cameo", ["Insert", "Images"], None, "{slide?: index, all?: bool, rect?: [x,y,w,h] pt}", has_slide, insert_cameo),
        cmd!(
            "camera.style",
            "Camera Styles",
            ["Camera Format", "Camera Styles"],
            None,
            "{style: none|simpleFrameRectangle|centerShadowRectangle|softEdgeRectangle|simpleFrameCircle|centerShadowCircle|softEdgeCircle|centerShadowDiamond|centerShadowHexagon (see camera.styles), all?: bool, ids?}",
            has_cameo,
            style
        ),
        // Not journaled: the clip arrives as base64 and would sit in the session journal for the
        // whole run. Nothing replays journal entries to rebuild a document; undo keeps the edit.
        CommandSpec {
            id: "show.cameo",
            label: "Record Cameo",
            menu: &["Slide Show", "Record"],
            shortcut: None,
            params: "{index?: index, data: base64 mp4 | path, name?, durationMs?, rect?: [x,y,w,h] pt} → the slide's recorded Cameo video",
            enabled: has_slide,
            run: record_cameo,
            journal: false,
            undoable: true,
        },
        cmd!(
            "camera.applyToAll",
            "Apply to All Slides",
            ["Camera Format", "Camera Styles"],
            None,
            "{ids?} — copies the selected Cameo's shape, frame and effects onto every Cameo in the deck",
            has_cameo,
            apply_to_all
        ),
        cmd!(
            query "camera.styles",
            "Camera Style Gallery",
            ["Camera Format", "Camera Styles"],
            None,
            "{} → [{id, label}]",
            always,
            list_styles
        ),
    ]
}

/// One Camera Styles gallery entry: the shape, frame and effects it gives the cameo.
pub struct Style {
    pub id: &'static str,
    pub label: &'static str,
    /// Shape preset.
    pub preset: &'static str,
    /// Frame width in points; `None` for no frame.
    pub frame: Option<f64>,
    /// A centred outer shadow.
    pub shadow: bool,
    /// Soft-edge radius in points.
    pub soft_edge: Option<f64>,
}

/// The gallery, in PowerPoint's order.
pub const STYLES: [Style; 9] = [
    Style { id: "none", label: "No Style", preset: "", frame: None, shadow: false, soft_edge: None },
    Style { id: "simpleFrameRectangle", label: "Simple Frame Rectangle", preset: "rect", frame: Some(6.0), shadow: false, soft_edge: None },
    Style { id: "centerShadowRectangle", label: "Center Shadow Rectangle", preset: "rect", frame: None, shadow: true, soft_edge: None },
    Style { id: "softEdgeRectangle", label: "Soft Edge Rectangle", preset: "rect", frame: None, shadow: false, soft_edge: Some(10.0) },
    Style { id: "simpleFrameCircle", label: "Simple Frame Circle", preset: "ellipse", frame: Some(6.0), shadow: false, soft_edge: None },
    Style { id: "centerShadowCircle", label: "Center Shadow Circle", preset: "ellipse", frame: None, shadow: true, soft_edge: None },
    Style { id: "softEdgeCircle", label: "Soft Edge Circle", preset: "ellipse", frame: None, shadow: false, soft_edge: Some(12.0) },
    Style { id: "centerShadowDiamond", label: "Center Shadow Diamond", preset: "diamond", frame: None, shadow: true, soft_edge: None },
    Style { id: "centerShadowHexagon", label: "Center Shadow Hexagon", preset: "hexagon", frame: None, shadow: true, soft_edge: None },
];

/// The style with this id; `none` included.
pub fn look(id: &str) -> Option<&'static Style> {
    STYLES.iter().find(|s| s.id == id)
}

fn center_shadow() -> Shadow {
    Shadow {
        color: ColorRef::rgb(Rgba::BLACK).with(deckcraft_color::ColorTransform::Alpha(40000)),
        blur: 16.0,
        dist: 0.0,
        dir: 90.0,
        inner: false,
        sx: 1.0,
        sy: 1.0,
        kx: 0.0,
        ky: 0.0,
        align: "tl".into(),
        rotate_with_shape: false,
    }
}

/// Apply camera style `id` to `sh`. `none` clears the frame and effects but keeps the shape (the
/// user's Camera Shape choice), exactly as the gallery's first tile reads.
pub fn apply(sh: &mut Shape, id: &str) -> bool {
    if id == "none" {
        sh.line = None;
        sh.effects = None;
        return true;
    }
    let Some(st) = look(id) else { return false };
    sh.geom = Geom::preset(st.preset);
    sh.line = Some(match st.frame {
        Some(w) => Line::solid(ColorRef::rgb(Rgba::WHITE), w),
        None => Line::none(),
    });
    let fx = Effects { outer_shadow: st.shadow.then(center_shadow), soft_edge: st.soft_edge, ..Default::default() };
    sh.effects = (!fx.is_empty()).then_some(fx);
    true
}

/// The camera style `sh` currently matches, or `none` when it has no frame or effects. `None` when
/// a custom frame or effect was applied on top (Camera Border, Camera Effects).
pub fn style_of(sh: &Shape) -> Option<&'static str> {
    let preset = sh.geom.preset_name()?;
    let fx = sh.effects.as_ref();
    let shadow = fx.and_then(|e| e.outer_shadow.as_ref()).is_some();
    let soft = fx.and_then(|e| e.soft_edge);
    let width = sh.line.as_ref().and_then(|l| l.width);
    let framed = sh.line.as_ref().is_some_and(|l| matches!(l.fill, Some(Fill::Solid { .. })));
    if !framed && !shadow && soft.is_none() {
        return Some("none");
    }
    STYLES
        .iter()
        .find(|st| {
            st.id != "none"
                && st.preset == preset
                && st.shadow == shadow
                && match (st.frame, width, framed) {
                    (Some(w), Some(v), true) => (w - v).abs() < 0.01,
                    (None, _, false) => true,
                    _ => false,
                }
                && match (st.soft_edge, soft) {
                    (Some(a), Some(b)) => (a - b).abs() < 0.01,
                    (None, None) => true,
                    _ => false,
                }
        })
        .map(|s| s.id)
}

fn list_styles(_s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(STYLES.iter().map(|s| json!({"id": s.id, "label": s.label})).collect())
}

/// Bottom-right default box for a slide.
fn default_rect(size: deckcraft_geom::Size) -> Xfrm {
    let w = DEFAULT_SIZE.min(size.width.max(0.0));
    let h = DEFAULT_SIZE.min(size.height.max(0.0));
    Xfrm::new((size.width - w - DEFAULT_MARGIN).max(0.0), (size.height - h - DEFAULT_MARGIN).max(0.0), w, h)
}

fn cameo(id: ShapeId, name: String, xfrm: Xfrm, media: deckcraft_model::MediaId) -> Shape {
    Shape {
        id,
        name,
        xfrm: Some(xfrm),
        kind: ShapeKind::Picture { fill: PictureFill { media, ..Default::default() } },
        geom: Geom::preset("ellipse"),
        cameo: Some(Cameo::live()),
        descr: "Camera preview".into(),
        ..Default::default()
    }
}

/// Names and shape count of one slide, for `Camera N`.
fn slide_names(shapes: &[Shape]) -> (std::collections::HashSet<String>, usize) {
    let mut names = std::collections::HashSet::new();
    let mut count = 0;
    deckcraft_model::walk(shapes, &mut |sh, _| {
        names.insert(sh.name.clone());
        count += 1;
    });
    (names, count)
}

fn insert_cameo(s: &mut Session, p: &Value) -> Result<Value> {
    let all = bool_or(p, "all", false);
    let wanted = opt_usize_param(p, "slide", "insert.cameo")?;
    let rect = rect_param(p, "rect");
    if rect.is_some_and(|r| r.w < 1.0 || r.h < 1.0) {
        return Err(bad("insert.cameo", "rect needs a positive width and height"));
    }
    let (target, ids, id) = s.edit(|doc, sel| {
        let n = doc.slides.len();
        if n == 0 {
            return Err(bad("insert.cameo", "no slides"));
        }
        let target = match wanted {
            Some(i) if i < n => i,
            Some(i) => return Err(bad("insert.cameo", format!("no slide {}", i + 1))),
            None => sel.slide.min(n - 1),
        };
        let size = doc.slide_size;
        // Our own cameo placeholder: a camera body and lens, no vendor artwork. It is the PNG the
        // PPTX reader also uses for a Cameo whose poster cannot be read, so every renderer draws it
        // (the SVG art of the same camera is not renderable); PowerPoint shows it and the capture
        // layer swaps it for the live feed.
        let media =
            doc.add_media("cameo-placeholder.png", "image/png", deckcraft_pptx::cameo_poster()).ok_or_else(|| bad("insert.cameo", NO_ID_SPACE))?;
        let box_ = rect.unwrap_or_else(|| default_rect(size));
        let mut ids = Vec::new();
        let mut on_target = None;
        for i in 0..n {
            if !all && i != target {
                continue;
            }
            let id = ShapeId(doc.alloc_id().ok_or_else(|| bad("insert.cameo", NO_ID_SPACE))?);
            let shapes = &mut Arc::make_mut(&mut doc.slides[i]).shapes;
            let (taken, count) = slide_names(shapes);
            let name = first_free_name("Camera", &taken, count);
            shapes.push(cameo(id, name, box_, media));
            if i == target {
                on_target = Some(id);
            }
            ids.push(id);
        }
        sel.target = crate::Target::Slides;
        sel.slide = target;
        sel.text = None;
        let Some(first) = on_target.or_else(|| ids.first().copied()) else {
            return Err(bad("insert.cameo", "no slide"));
        };
        sel.shapes = vec![first];
        Ok((target, ids, first))
    })?;
    Ok(json!({"id": id, "ids": ids, "slide": target}))
}

/// Largest recorded clip one slide's Cameo may hold.
const MAX_CAMEO_BYTES: usize = deckcraft_media::MAX_CLIP_BYTES;

/// Commit a recorded camera clip as the slide's Cameo: the picture keeps its frame and format,
/// its fill becomes the poster (first frame), and the MP4 plays from the slide's start.
fn record_cameo(s: &mut Session, p: &Value) -> Result<Value> {
    let (name, bytes) = super::insert::media_bytes(p, "show.cameo")?;
    let ct = super::insert::content_type(&name, &bytes);
    if !matches!(ct, "video/mp4" | "video/quicktime") {
        return Err(bad("show.cameo", format!("{name} is not an MP4 video ({ct})")));
    }
    if bytes.len() > MAX_CAMEO_BYTES {
        return Err(bad("show.cameo", format!("{name} is larger than the {} MB a Cameo may hold", MAX_CAMEO_BYTES >> 20)));
    }
    let data = Arc::new(bytes);
    let info = deckcraft_media::probe(&data).map_err(|e| bad("show.cameo", format!("{name}: {e}")))?;
    let Some(video) = info.video.as_ref() else {
        return Err(bad("show.cameo", format!("{name} has no video")));
    };
    let (width, height) = (video.width, video.height);
    let duration_ms = match p.get("durationMs").and_then(Value::as_u64) {
        Some(v) => v.min(u64::from(u32::MAX)) as u32,
        None => info.duration_ms.min(u64::from(u32::MAX)) as u32,
    };
    if duration_ms == 0 {
        return Err(bad("show.cameo", format!("{name} has no duration")));
    }
    let (poster, poster_ct) = match crate::cmd::media::poster_png(&data, 0) {
        Some(png) => (png, "image/png"),
        // A clip we cannot decode still plays in PowerPoint; its Cameo shows the raster camera
        // placeholder (the renderer decodes rasters, not SVG).
        None => (deckcraft_pptx::cameo_poster(), "image/png"),
    };
    let rect = rect_param(p, "rect").filter(|r| r.w >= 1.0 && r.h >= 1.0);
    let wanted = opt_usize_param(p, "index", "show.cameo")?;
    let data = Arc::try_unwrap(data).unwrap_or_else(|a| (*a).clone());
    let (target, id, media, poster_id) = s.edit(move |doc, sel| {
        let n = doc.slides.len();
        if n == 0 {
            return Err(bad("show.cameo", "no slides"));
        }
        let target = match wanted {
            Some(i) if i < n => i,
            Some(i) => return Err(bad("show.cameo", format!("no slide {}", i + 1))),
            None => sel.slide.min(n - 1),
        };
        let size = doc.slide_size;
        let media = doc.add_media(&name, ct, data).ok_or_else(|| bad("show.cameo", NO_ID_SPACE))?;
        let poster_media = doc.add_media(&format!("{name}-poster.png"), poster_ct, poster).ok_or_else(|| bad("show.cameo", NO_ID_SPACE))?;
        let next_id = ShapeId(doc.alloc_id().ok_or_else(|| bad("show.cameo", NO_ID_SPACE))?);
        let slide = Arc::make_mut(&mut doc.slides[target]);
        let mut existing = None;
        walk_mut(&mut slide.shapes, &mut |sh| {
            if existing.is_none() && sh.is_cameo() {
                existing = Some(sh.id);
            }
        });
        let id = match existing {
            Some(id) => {
                let Some(sh) = slide.shape_mut(id) else {
                    return Err(bad("show.cameo", "the Cameo is missing"));
                };
                let crop = match &sh.kind {
                    ShapeKind::Picture { fill } => fill.crop,
                    _ => [0.0; 4],
                };
                sh.kind = ShapeKind::Picture { fill: PictureFill { media: poster_media, crop, ..Default::default() } };
                // Keep the imported feed (background in `raw`, device) with the new recording.
                let kept = sh.cameo.take().unwrap_or_default();
                sh.cameo = Some(Cameo { raw: kept.raw, device: kept.device, ..Cameo::recorded(media) });
                if let Some(r) = rect {
                    sh.xfrm = Some(r);
                }
                id
            }
            None => {
                let (taken, count) = slide_names(&slide.shapes);
                let name = first_free_name("Camera", &taken, count);
                let box_ = rect.unwrap_or_else(|| default_rect(size));
                let mut shape = cameo(next_id, name, box_, poster_media);
                shape.cameo = Some(Cameo::recorded(media));
                shape.descr = "Recorded camera clip".into();
                slide.shapes.push(shape);
                next_id
            }
        };
        sel.target = crate::Target::Slides;
        sel.slide = target;
        sel.text = None;
        sel.shapes = vec![id];
        Ok((target, id, media, poster_media))
    })?;
    Ok(json!({
        "slide": target,
        "id": id,
        "media": media.0,
        "poster": poster_id.0,
        "durationMs": duration_ms,
        "width": width,
        "height": height,
    }))
}

fn style(s: &mut Session, p: &Value) -> Result<Value> {
    let id = str_param(p, "style").ok_or_else(|| bad("camera.style", "missing `style`"))?.to_string();
    if id != "none" && look(&id).is_none() {
        return Err(bad("camera.style", format!("unknown style `{id}`; see camera.styles")));
    }
    if bool_or(p, "all", false) {
        s.edit(|doc, _| {
            for slide in &mut doc.slides {
                walk_mut(&mut Arc::make_mut(slide).shapes, &mut |sh| {
                    if sh.is_cameo() {
                        apply(sh, &id);
                    }
                });
            }
            Ok(())
        })?;
        return Ok(json!({"style": id, "all": true}));
    }
    let ids = targets(s, p)?;
    if ids.is_empty() {
        return Err(bad("camera.style", "nothing selected"));
    }
    {
        let doc = s.doc()?;
        if !ids.iter().all(|id| doc.shape(*id).is_some_and(Shape::is_cameo)) {
            return Err(bad("camera.style", "select a Cameo"));
        }
    }
    edit_shapes(s, p, "camera.style", |sh| {
        apply(sh, &id);
        Ok(())
    })?;
    Ok(json!({"style": id}))
}

fn copy_format(from: &Shape, to: &mut Shape) {
    to.geom = from.geom.clone();
    to.line = from.line.clone();
    to.effects = from.effects.clone();
}

fn apply_to_all(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    let source = {
        let doc = s.doc()?;
        let id = *ids.first().ok_or_else(|| bad("camera.applyToAll", "nothing selected"))?;
        doc.shape(id).filter(|sh| sh.is_cameo()).cloned().ok_or_else(|| bad("camera.applyToAll", "select a Cameo"))?
    };
    let styled = s.edit(|doc, _| {
        let mut styled = 0usize;
        for slide in &mut doc.slides {
            let slide = Arc::make_mut(slide);
            let mut existing = Vec::new();
            deckcraft_model::walk(&slide.shapes, &mut |sh, _| {
                if sh.is_cameo() {
                    existing.push(sh.id);
                }
            });
            for id in existing {
                if let Some(sh) = slide.shape_mut(id) {
                    copy_format(&source, sh);
                    styled += 1;
                }
            }
        }
        Ok(styled)
    })?;
    Ok(json!({"styled": styled}))
}

/// Enabled state of the Camera Styles commands: a Cameo must be selected.
pub fn has_cameo(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    let doc = s.active().ok_or_else(|| "no presentation open".to_string())?;
    let shapes = doc.selected_shapes();
    if shapes.is_empty() {
        // `ids`/`id` callers name their targets; `style` and `applyToAll` check the targets.
        return Err(super::NOTHING_SELECTED.into());
    }
    if shapes.iter().all(|sh| sh.is_cameo()) { Ok(()) } else { Err("no Cameo selected".into()) }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    fn cameo_shapes(s: &Session) -> Vec<deckcraft_model::Shape> {
        s.doc()
            .expect("doc")
            .doc
            .slides
            .iter()
            .flat_map(|sl| {
                let mut v = vec![];
                deckcraft_model::walk(&sl.shapes, &mut |sh, _| {
                    if sh.is_cameo() {
                        v.push(sh.clone());
                    }
                });
                v
            })
            .collect()
    }

    fn cameo_count(s: &Session) -> usize {
        cameo_shapes(s).len()
    }

    #[test]
    fn insert_cameo_adds_a_round_bottom_right_picture() {
        let mut s = Session::with_new();
        let r = s.execute("insert.cameo", &json!({})).expect("insert");
        let id = deckcraft_model::ShapeId(r["id"].as_u64().expect("id") as u32);
        let doc = s.doc().expect("doc");
        let sh = doc.doc.slides[0].shape(id).expect("shape");
        assert_eq!(sh.name, "Camera 3");
        assert!(sh.is_cameo());
        assert_eq!(sh.geom.preset_name(), Some("ellipse"));
        let x = sh.xfrm.expect("xfrm");
        assert!((x.w - 162.0).abs() < 1e-9 && (x.h - 162.0).abs() < 1e-9);
        let size = doc.doc.slide_size;
        assert!((x.x - (size.width - 162.0 - 6.5)).abs() < 1e-9);
        let media = match &sh.kind {
            deckcraft_model::ShapeKind::Picture { fill } => fill.media,
            _ => panic!("picture"),
        };
        // A PNG the renderer decodes: an SVG placeholder drew as a missing-image mark before the
        // preview was on.
        let placeholder = doc.doc.media(media).expect("placeholder media");
        assert_eq!(placeholder.content_type, "image/png");
        assert!(placeholder.data.starts_with(b"\x89PNG"));
    }

    #[test]
    fn insert_cameo_is_undoable() {
        let mut s = Session::with_new();
        s.execute("insert.cameo", &json!({})).expect("insert");
        assert_eq!(cameo_count(&s), 1);
        s.execute("edit.undo", &json!({})).expect("undo");
        assert_eq!(cameo_count(&s), 0);
        s.execute("edit.redo", &json!({})).expect("redo");
        assert_eq!(cameo_count(&s), 1);
    }

    #[test]
    fn insert_cameo_all_adds_one_to_every_slide() {
        let mut s = Session::with_new();
        s.execute("slide.new", &json!({})).expect("slide");
        s.execute("slide.new", &json!({})).expect("slide");
        let r = s.execute("insert.cameo", &json!({"all": true})).expect("insert");
        assert_eq!(r["ids"].as_array().map(Vec::len), Some(3));
        let doc = s.doc().expect("doc");
        for slide in &doc.doc.slides {
            let mut n = 0;
            deckcraft_model::walk(&slide.shapes, &mut |sh, _| {
                if sh.is_cameo() {
                    n += 1;
                }
            });
            assert_eq!(n, 1);
        }
    }

    #[test]
    fn insert_cameo_targets_the_named_slide_and_rect() {
        let mut s = Session::with_new();
        s.execute("slide.new", &json!({})).expect("slide");
        let r = s.execute("insert.cameo", &json!({"slide": 1, "rect": [10, 20, 60, 60]})).expect("insert");
        assert_eq!(r["slide"], 1);
        let doc = s.doc().expect("doc");
        assert_eq!(doc.selection.slide, 1);
        let sh = doc.doc.slides[1].shape(deckcraft_model::ShapeId(r["id"].as_u64().expect("id") as u32)).expect("shape");
        assert_eq!(sh.xfrm, Some(deckcraft_geom::Xfrm::new(10.0, 20.0, 60.0, 60.0)));
    }

    #[test]
    fn styles_set_shape_frame_and_effects() {
        let mut s = Session::with_new();
        s.execute("insert.cameo", &json!({})).expect("insert");
        for st in &super::STYLES {
            s.execute("camera.style", &json!({"style": st.id})).expect("style");
            let sh = &cameo_shapes(&s)[0];
            if st.id != "none" {
                assert_eq!(sh.geom.preset_name(), Some(st.preset), "{}", st.id);
                assert_eq!(sh.line.as_ref().and_then(|l| l.width), st.frame, "{}", st.id);
                assert_eq!(sh.effects.as_ref().and_then(|e| e.soft_edge), st.soft_edge, "{}", st.id);
                assert_eq!(sh.effects.as_ref().and_then(|e| e.outer_shadow.as_ref()).is_some(), st.shadow, "{}", st.id);
            }
            assert_eq!(super::style_of(sh), Some(st.id), "{}", st.id);
        }
    }

    #[test]
    fn apply_to_all_slides_copies_formatting_to_every_cameo_only() {
        let mut s = Session::with_new();
        s.execute("slide.new", &json!({})).expect("slide");
        s.execute("insert.cameo", &json!({"all": true})).expect("insert");
        s.execute("shape.insert", &json!({"preset": "rect", "rect": [10, 10, 40, 40]})).expect("shape");
        // `shape.insert` moved the selection to the new rectangle on the current (second) slide;
        // address the Cameo there explicitly.
        let cameo = cameo_shapes(&s).into_iter().next_back().expect("cameo").id;
        s.doc_mut().expect("doc").selection.shapes = vec![cameo];
        s.execute("camera.style", &json!({"style": "centerShadowCircle"})).expect("style");
        let r = s.execute("camera.applyToAll", &json!({})).expect("apply");
        assert_eq!(r["styled"], 2);
        for sh in cameo_shapes(&s) {
            assert_eq!(sh.geom.preset_name(), Some("ellipse"));
            assert!(sh.effects.as_ref().and_then(|e| e.outer_shadow.as_ref()).is_some());
        }
        let doc = s.doc().expect("doc");
        let plain = doc.doc.slides[1].shapes.iter().find(|sh| sh.name.starts_with("Rectangle")).expect("rectangle");
        assert_ne!(plain.geom.preset_name(), Some("ellipse"));
    }

    #[test]
    fn apply_to_all_copies_the_format_of_the_selected_slides_cameo() {
        let mut s = Session::with_new();
        s.execute("insert.cameo", &json!({})).expect("insert");
        // Imported decks number each slide's shapes from 1 again, so two slides can share an id.
        let same = s.doc().expect("doc").doc.slides[0].clone();
        std::sync::Arc::make_mut(&mut s.doc_mut().expect("doc").doc).slides.push(same);
        let id = cameo_shapes(&s)[0].id;
        // The selected (second) slide's Cameo is a rectangle; the first slide's is the ellipse
        // `insert.cameo` makes. `applyToAll` copies the selected one, not the first with that id.
        {
            let doc = s.doc_mut().expect("doc");
            let pres = std::sync::Arc::make_mut(&mut doc.doc);
            std::sync::Arc::make_mut(&mut pres.slides[1]).shape_mut(id).expect("cameo").geom = deckcraft_model::Geom::preset("rect");
            doc.selection.slide = 1;
            doc.selection.shapes = vec![id];
        }
        let r = s.execute("camera.applyToAll", &json!({})).expect("apply");
        assert_eq!(r["styled"], 2);
        assert!(cameo_shapes(&s).iter().all(|sh| sh.geom.preset_name() == Some("rect")), "the selected slide's Cameo is the source");
    }

    #[test]
    fn recording_onto_an_imported_cameo_keeps_its_feed_properties() {
        let mut s = Session::with_new();
        let r = s.execute("insert.cameo", &json!({})).expect("insert");
        let id = deckcraft_model::ShapeId(r["id"].as_u64().expect("id") as u32);
        {
            let doc = s.doc_mut().expect("doc");
            let pres = std::sync::Arc::make_mut(&mut doc.doc);
            let slide = std::sync::Arc::make_mut(&mut pres.slides[0]);
            let cameo = slide.shape_mut(id).expect("cameo").cameo.get_or_insert_default();
            cameo.raw = Some("<alf:liveFeedProps xmlns:alf=\"http://schemas.microsoft.com/office/drawing/2021/livefeed\"><alf:backgroundProps><alf:Blur/></alf:backgroundProps></alf:liveFeedProps>".into());
            cameo.device = Some("FaceTime HD Camera".into());
        }
        s.execute("show.cameo", &json!({"data": clip_data(), "durationMs": 300})).expect("record");
        let doc = s.doc().expect("doc");
        let cameo = doc.doc.slides[0].shape(id).expect("shape").cameo.clone().expect("cameo");
        assert!(!cameo.live);
        assert!(cameo.recorded.is_some());
        assert_eq!(cameo.device.as_deref(), Some("FaceTime HD Camera"), "the device is kept");
        assert!(cameo.raw.as_deref().is_some_and(|r| r.contains("backgroundProps")), "the camera background is kept");
    }

    /// A short real MP4, base64 as `show.cameo` takes it.
    fn clip_data() -> String {
        let mut track = deckcraft_media::H264Mp4Track::new(32, 32, 15, 30).expect("track");
        for i in 0..5 {
            track.push_rgba(&vec![i as u8; 32 * 32 * 4]).expect("push");
        }
        crate::cmd::base64_encode(&track.finish().expect("finish"))
    }

    #[test]
    fn a_recorded_clip_is_not_kept_in_the_journal() {
        let mut s = Session::with_new();
        s.execute("show.cameo", &json!({"data": clip_data(), "durationMs": 300})).expect("record");
        assert!(s.journal.iter().all(|(id, _)| id != "show.cameo"), "the base64 clip is not journaled");
        // The edit is still one undo step.
        s.execute("edit.undo", &json!({})).expect("undo");
        assert_eq!(cameo_count(&s), 0);
        s.execute("edit.redo", &json!({})).expect("redo");
        assert_eq!(cameo_count(&s), 1);
    }

    #[test]
    fn camera_commands_take_named_targets_without_a_selection() {
        let mut s = Session::with_new();
        s.execute("slide.new", &json!({})).expect("slide");
        s.execute("insert.cameo", &json!({"all": true})).expect("cameo");
        let id = cameo_shapes(&s).first().expect("cameo").id;
        {
            let doc = s.doc_mut().expect("doc");
            doc.selection.shapes.clear();
            doc.selection.slide = 0;
        }
        // `ids` names the target, so the selection-based enablement does not block it.
        s.execute("camera.style", &json!({"style": "centerShadowCircle", "ids": [id.0]})).expect("style by id");
        s.execute("camera.applyToAll", &json!({"ids": [id.0]})).expect("apply by id");
        // `all` names its target too.
        s.execute("camera.style", &json!({"style": "softEdgeCircle", "all": true})).expect("style all");
        assert!(cameo_shapes(&s).iter().all(|sh| sh.geom.preset_name() == Some("ellipse")));
    }

    #[test]
    fn the_all_bypass_needs_a_command_that_declares_all() {
        let mut s = Session::with_new();
        // `arrange.bringToFront` takes no `all`, so the named-target bypass must not let it run.
        assert!(s.execute("arrange.bringToFront", &json!({"all": true})).is_err());
        // `camera.style` declares it and works without a selection.
        s.execute("slide.new", &json!({})).expect("slide");
        s.execute("insert.cameo", &json!({"all": true})).expect("cameo");
        s.doc_mut().expect("doc").selection.shapes.clear();
        s.execute("camera.style", &json!({"style": "softEdgeCircle", "all": true})).expect("style all");
    }

    #[test]
    fn style_rejects_an_unknown_name_and_a_non_cameo() {
        let mut s = Session::with_new();
        s.execute("insert.cameo", &json!({})).expect("insert");
        assert!(s.execute("camera.style", &json!({"style": "nope"})).is_err());
        s.execute("shape.insert", &json!({"preset": "rect"})).expect("shape");
        assert!(s.execute("camera.style", &json!({"style": "none"})).is_err());
    }

    #[test]
    fn a_cameo_keeps_its_picture_media_across_styles() {
        let mut s = Session::with_new();
        s.execute("insert.cameo", &json!({})).expect("insert");
        let before = cameo_shapes(&s)[0].kind.clone();
        s.execute("camera.style", &json!({"style": "softEdgeCircle"})).expect("style");
        assert_eq!(cameo_shapes(&s)[0].kind, before);
    }
}
