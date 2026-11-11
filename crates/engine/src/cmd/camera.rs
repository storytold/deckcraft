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
    let no_line = sh.line.as_ref().is_none_or(|l| matches!(l.fill, None | Some(Fill::None)));
    let no_effects = sh.effects.as_ref().is_none_or(|e| *e == Effects::default());
    if no_line && no_effects {
        return Some("none");
    }
    STYLES
        .iter()
        .find(|st| {
            if st.id == "none" || st.preset != preset {
                return false;
            }
            let line_ok = match st.frame {
                Some(w) => {
                    sh.line.as_ref().is_some_and(|l| matches!(l.fill, Some(Fill::Solid { .. })) && l.width.is_some_and(|v| (v - w).abs() < 0.01))
                }
                None => no_line,
            };
            let want = Effects { outer_shadow: st.shadow.then(center_shadow), soft_edge: st.soft_edge, ..Default::default() };
            line_ok && sh.effects.as_ref().is_none_or(|e| *e == want) && (sh.effects.is_some() || want == Effects::default())
        })
        .map(|s| s.id)
}

fn list_styles(_s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(STYLES.iter().map(|s| json!({"id": s.id, "label": s.label})).collect())
}

/// Our own cameo placeholder: a camera body and lens drawn as an SVG, no vendor artwork. It is
/// embedded as the picture's image, so PowerPoint shows it and the capture layer can swap it for
/// the live feed.
pub const PLACEHOLDER_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="320" height="320" viewBox="0 0 320 320">
  <defs>
    <linearGradient id="bg" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="#33404f"/>
      <stop offset="1" stop-color="#141a22"/>
    </linearGradient>
  </defs>
  <rect width="320" height="320" rx="36" fill="url(#bg)"/>
  <path d="M104 112h36l14-22h44l14 22h32a18 18 0 0 1 18 18v92a18 18 0 0 1-18 18H104a18 18 0 0 1-18-18v-92a18 18 0 0 1 18-18Z"
        fill="none" stroke="#e8eef7" stroke-width="11" stroke-linejoin="round"/>
  <circle cx="160" cy="176" r="40" fill="#0d1218" stroke="#e8eef7" stroke-width="11"/>
  <circle cx="160" cy="176" r="17" fill="#8fd3ff"/>
  <circle cx="244" cy="128" r="6" fill="#8fd3ff"/>
</svg>"##;

/// Bottom-right default box for a slide.
pub(super) fn default_rect(size: deckcraft_geom::Size) -> Xfrm {
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
            Some(i) => return Err(bad("insert.cameo", format!("no slide {}", i.saturating_add(1)))),
            None => sel.slide.min(n - 1),
        };
        let size = doc.slide_size;
        let media = doc
            .add_media("cameo-placeholder.svg", "image/svg+xml", PLACEHOLDER_SVG.as_bytes().to_vec())
            .ok_or_else(|| bad("insert.cameo", NO_ID_SPACE))?;
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
    let mut ids = targets(s, p)?;
    if ids.is_empty() {
        return Err(bad("camera.style", "nothing selected"));
    }
    // The Camera Format tab stays enabled for a mixed selection: without an explicit target, style
    // the Cameos in it and leave the ordinary shapes alone. An explicit `id`/`ids` is the caller's
    // choice and is checked as given.
    let explicit = p.get("ids").is_some() || p.get("id").is_some();
    if !explicit {
        let doc = s.doc()?;
        ids.retain(|id| doc.shape(*id).is_some_and(Shape::is_cameo));
    }
    if ids.is_empty() {
        return Err(bad("camera.style", "select a Cameo"));
    }
    {
        let doc = s.doc()?;
        if !ids.iter().all(|id| doc.shape(*id).is_some_and(Shape::is_cameo)) {
            return Err(bad("camera.style", "select a Cameo"));
        }
    }
    // The edit itself must address those ids, or it would style the whole selection again.
    let mut params = p.clone();
    if params.get("ids").is_none() {
        params["ids"] = json!(ids.iter().map(|id| id.0).collect::<Vec<_>>());
    }
    edit_shapes(s, &params, "camera.style", |sh| {
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
    let mut ids = targets(s, p)?;
    // A mixed selection keeps the tab: without an explicit target the Cameos in it are the source.
    if p.get("ids").is_none() && p.get("id").is_none() {
        let doc = s.doc()?;
        ids.retain(|id| doc.shape(*id).is_some_and(Shape::is_cameo));
    }
    let source = {
        let doc = s.doc()?;
        let id = *ids.first().ok_or_else(|| bad("camera.applyToAll", "nothing selected"))?;
        // Resolve the id in the current slide (`targets` came from it): ids repeat across slides,
        // as the PPTX import numbers each slide afresh.
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
        // The shared sentinel: the engine lets a call with explicit `ids` through even when
        // nothing is selected.
        return Err(super::NOTHING_SELECTED.to_string());
    }
    // The Camera Format tab stays up for a mixed selection; each command names the Cameos it
    // changes, so one Cameo in the selection is enough here.
    if shapes.iter().any(|sh| sh.is_cameo()) { Ok(()) } else { Err("no Cameo selected".into()) }
}

/// Enabled state of `cameo.preview`: a Cameo is selected, or a preview is running — a preview can
/// always be stopped, even after its Cameo was deselected or deleted.
pub fn has_cameo_or_preview(s: &Session) -> std::result::Result<(), String> {
    // Only an open (or opening) preview counts: Denied, Lost, Busy and Unsupported are errors,
    // and a toggle in those states would just ask the camera to start again.
    if s.camera_status.is_on() {
        return Ok(());
    }
    // The selection exemption (`NOTHING_SELECTED`) means "no explicit `ids`", not "a preview
    // may start with nothing selected": report it distinctly so the toggle stays disabled.
    has_cameo(s).map_err(|e| if e == super::NOTHING_SELECTED { "no Cameo selected".into() } else { e })
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
        // The number follows the shapes already on the slide, so only the prefix is fixed here.
        assert!(sh.name.starts_with("Camera "), "a Cameo gets a Camera name, got {:?}", sh.name);
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
        assert_eq!(doc.doc.media(media).map(|m| m.content_type.as_str()), Some("image/svg+xml"));
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

    /// The Camera Format tab stays usable when a Cameo shares the selection with an ordinary
    /// shape; the commands address the Cameo ids, so the other shape is untouched. Calls with no
    /// `ids` narrow the selection the same way.
    #[test]
    fn a_mixed_selection_styles_only_the_cameo() {
        let mut s = Session::with_new();
        let cam = s.execute("insert.cameo", &json!({})).expect("cameo")["id"].as_u64().expect("id") as u32;
        let rect = s.execute("shape.insert", &json!({"preset": "rect", "rect": [10, 10, 40, 40]})).expect("rect")["id"].as_u64().expect("id") as u32;
        s.execute("selection.select", &json!({"ids": [rect, cam]})).expect("select");
        assert!(super::has_cameo(&s).is_ok(), "the tab's tools stay available");
        // No `ids`: the command narrows the mixed selection to its Cameo itself.
        s.execute("camera.style", &json!({"style": "centerShadowCircle"})).expect("style");
        let doc = s.doc().expect("doc");
        let cameo = doc.shapes().iter().find(|sh| sh.id.0 == cam).expect("cameo");
        assert_eq!(super::style_of(cameo), Some("centerShadowCircle"));
        let plain = doc.shapes().iter().find(|sh| sh.id.0 == rect).expect("rect");
        assert_ne!(super::style_of(plain), Some("centerShadowCircle"), "the ordinary shape keeps its own outline and effects");
    }

    #[test]
    fn apply_to_all_slides_copies_formatting_to_every_cameo_only() {
        let mut s = Session::with_new();
        s.execute("slide.new", &json!({})).expect("slide");
        s.execute("insert.cameo", &json!({"all": true})).expect("insert");
        s.execute("shape.insert", &json!({"preset": "rect", "rect": [10, 10, 40, 40]})).expect("shape");
        // Style one Cameo, then copy its formatting to all of them.
        let slide = s.doc().expect("doc").selection.slide;
        let cameo = s.doc().expect("doc").doc.slides[slide].shapes.iter().find(|sh| sh.is_cameo()).expect("cameo").id.0;
        s.execute("selection.select", &json!({"ids": [cameo]})).expect("select");
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

    #[test]
    fn apply_to_all_is_one_undo_step_that_restores_every_cameo() {
        let mut s = Session::with_new();
        s.execute("slide.new", &json!({})).expect("slide");
        s.execute("insert.cameo", &json!({"all": true})).expect("insert");
        s.execute("camera.style", &json!({"style": "centerShadowHexagon"})).expect("style");
        // `before` is the state applyToAll starts from: styling the selection is its own undo step.
        let before: Vec<Option<&'static str>> = cameo_shapes(&s).iter().map(super::style_of).collect();
        s.execute("camera.applyToAll", &json!({})).expect("apply");
        assert!(cameo_shapes(&s).iter().all(|sh| super::style_of(sh) == Some("centerShadowHexagon")));
        s.execute("edit.undo", &json!({})).expect("undo");
        let after: Vec<Option<&'static str>> = cameo_shapes(&s).iter().map(super::style_of).collect();
        assert_eq!(after, before, "one undo restores the formatting of every cameo");
        s.execute("edit.redo", &json!({})).expect("redo");
        assert!(cameo_shapes(&s).iter().all(|sh| super::style_of(sh) == Some("centerShadowHexagon")));
    }

    #[test]
    fn no_style_clears_frame_and_effects_but_keeps_the_camera_shape() {
        let mut s = Session::with_new();
        s.execute("insert.cameo", &json!({})).expect("insert");
        s.execute("camera.style", &json!({"style": "centerShadowDiamond"})).expect("style");
        s.execute("camera.style", &json!({"style": "none"})).expect("none");
        let sh = &cameo_shapes(&s)[0];
        assert_eq!(sh.geom.preset_name(), Some("diamond"), "No Style keeps the user's Camera Shape");
        assert!(sh.line.is_none() && sh.effects.is_none());
        assert_eq!(super::style_of(sh), Some("none"));
    }

    #[test]
    fn camera_style_values_survive_a_pptx_round_trip() {
        let mut s = Session::with_new();
        s.execute("insert.cameo", &json!({})).expect("insert");
        s.execute("camera.style", &json!({"style": "centerShadowHexagon"})).expect("style");
        let bytes = deckcraft_pptx::export(&s.doc().expect("doc").doc).expect("export");
        let back = deckcraft_pptx::import(&bytes).expect("import");
        let mut found = None;
        deckcraft_model::walk(&back.slides[0].shapes, &mut |sh, _| {
            if sh.is_cameo() {
                found = Some(sh.clone());
            }
        });
        let sh = found.expect("cameo after import");
        assert!(sh.is_cameo());
        assert_eq!(sh.geom.preset_name(), Some("hexagon"));
        assert!(sh.effects.as_ref().and_then(|e| e.outer_shadow.as_ref()).is_some(), "the centre shadow is written as a:outerShdw");
        assert_eq!(super::style_of(&sh), Some("centerShadowHexagon"), "the gallery still reads the same preset");
        // A second save must not drift: the style is formatting, not an id.
        let again = deckcraft_pptx::import(&deckcraft_pptx::export(&back).expect("export again")).expect("import again");
        let mut second = None;
        deckcraft_model::walk(&again.slides[0].shapes, &mut |sh, _| {
            if sh.is_cameo() {
                second = Some(sh.clone());
            }
        });
        let sh = second.expect("cameo after second import");
        assert_eq!(sh.geom, deckcraft_model::Geom::preset("hexagon"));
        assert_eq!(super::style_of(&sh), Some("centerShadowHexagon"));
    }

    /// #1836 review: the preview toggle reports an empty selection distinctly, so the UI
    /// keeps it disabled; the other Cameo commands keep the shared exemption.
    #[test]
    fn preview_toggle_reports_an_empty_selection_distinctly() {
        use super::super::NOTHING_SELECTED;
        // A fresh deck selects nothing: the shared exemption fires for the other commands.
        let s = Session::with_new();
        assert_eq!(super::has_cameo(&s).unwrap_err(), NOTHING_SELECTED, "the shared exemption stays for the other commands");
        assert_eq!(super::has_cameo_or_preview(&s).unwrap_err(), "no Cameo selected", "the preview toggle stays disabled");
    }
}
