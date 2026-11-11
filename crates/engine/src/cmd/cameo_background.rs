//! Camera Format ▸ Camera Background: Normal, Remove, Blur and Replace on a Cameo, plus the
//! offscreen preview the control channel and the CLI use to prove the pipeline.
//!
//! The mode is part of the document (undoable, saved in `.deckcraft` and written as
//! `alf:liveFeedProps/alf:backgroundProps` in PPTX), and a mode is stored whether or not a model
//! is installed: a PPTX can import Blur or Removed. Segmentation is the host's: it installs a
//! [`deckcraft_matte::Segmenter`] on the session (or a loader the session asks on first use), and
//! without one `cameo.backgroundPreview` and the Camera Format controls refuse a non-Normal
//! preview with a sentence naming what is missing. The other commands only store the mode.

use std::io::Cursor;
use std::sync::Arc;

use deckcraft_geom::{Size, Xfrm};
use deckcraft_matte::compositing::{CompositeOptions, composite};
use deckcraft_matte::frame::RgbaFrame;
use deckcraft_model::style::PictureFill;
use deckcraft_model::{Cameo, CameraBackground, CameraBackgroundMode, Geom, MediaId, Rgba, Shape, ShapeId, ShapeKind};
use serde_json::{Value, json};

use super::insert::first_free_name;
use super::{CommandSpec, NO_ID_SPACE, always, bad, bool_or, cmd, opt_usize_param, rect_param, str_param, targets};
use crate::{EngineError, Result, Session};

/// Default side of an inserted cameo: 2.25 in.
pub const DEFAULT_SIZE: f64 = 162.0;
/// Gap between the cameo and the slide's right and bottom edges.
pub const DEFAULT_MARGIN: f64 = 6.5;

/// Our own cameo placeholder: a camera body and lens drawn as an SVG, no vendor artwork. It is
/// embedded as the picture's image, so PowerPoint shows it. Capture is #1837's work; this PR only
/// needs a Cameo to exist and keep its background.
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
pub(super) fn default_rect(size: Size) -> Xfrm {
    let w = DEFAULT_SIZE.min(size.width.max(0.0));
    let h = DEFAULT_SIZE.min(size.height.max(0.0));
    Xfrm::new((size.width - w - DEFAULT_MARGIN).max(0.0), (size.height - h - DEFAULT_MARGIN).max(0.0), w, h)
}

fn cameo(id: ShapeId, name: String, xfrm: Xfrm, media: MediaId) -> Shape {
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
    if s.doc()?.selection.target != crate::Target::Slides {
        return Err(bad("insert.cameo", "a Cameo goes on a slide: switch to Normal view first"));
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

/// Largest base64 PNG the preview command accepts (about 48 MB decoded).
const MAX_PREVIEW_BASE64: usize = 64 * 1024 * 1024;
/// Largest side a preview frame may have. 1280 px is 1 MP — four times the model's own 512 input —
/// and keeps the working set (frame, matte, replacement and PNG buffer) to a few megabytes instead
/// of the ~100 MB a 4096 px frame needs. The preview is a proof of the pipeline, not the live
/// capture path (#1837).
pub const MAX_PREVIEW_DIM: u32 = 1280;
/// Largest decoded replacement picture `imageData` stores. A PNG/JPEG photo is far smaller; the
/// check runs before the bytes reach the document. The Camera Format picker uses the same limit.
pub const MAX_INLINE_IMAGE_BYTES: usize = 16 * 1024 * 1024;
/// The most all media may take before an inline picture is refused (the loader's own budget is
/// 4 GiB, so this leaves room to save).
const MAX_DOCUMENT_MEDIA_BYTES: u64 = 2 << 30;
/// The default feather radius the preview applies when the caller names none.
const DEFAULT_FEATHER: u32 = 2;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "insert.cameo",
            "Cameo",
            ["Insert", "Images"],
            None,
            "{slide?: index (0-based; `all` inserts on every slide and `slide` only names the one left selected), all?: bool, rect?: [x,y,w,h] pt} → {id, ids, slide} (slide is 0-based)",
            super::has_doc,
            insert_cameo
        ),
        cmd!(
            "cameo.background",
            "Camera Background",
            ["Camera Format", "Camera Background"],
            None,
            "{mode: normal|removed|blur|custom, color?: [r,g,b]|[r,g,b,a]|#RRGGBB|#RRGGBBAA, image?: media id the document already holds, imageData?: base64 PNG or JPEG (added as media, at most 16 MB), all?: bool, id?: shape, ids?: [shape]} → {mode, ids, changed} (all plus id/ids is refused; without all, id/ids or the selection names the targets, and id/ids win)",
            super::has_doc,
            background
        ),
        cmd!(
            query "cameo.background.state",
            "Camera Background State",
            ["Camera Format", "Camera Background"],
            None,
            "{} → {mode, color ([r,g,b,a] or null), image (media id or null), hasCameo, segmenter, accelerator, accelerators} (mode is the first selected Cameo's; `accelerator` is cpu)",
            always,
            state
        ),
        cmd!(
            "cameo.backgroundApplyToAll",
            "Apply Background to All Cameos",
            ["Camera Format", "Camera Background"],
            None,
            "{mode?, color?, image?, imageData?, id?, ids?} → {mode, ids, changed} (without `mode`, copies the background of the first Cameo among id/ids or the selection; all Cameos in the deck are targets)",
            super::has_doc,
            apply_to_all
        ),
        cmd!(
            query "cameo.backgroundPreview",
            "Preview Camera Background",
            ["Camera Format", "Camera Background"],
            None,
            "{frame: base64 PNG or JPEG (up to 1280x1280), mode: normal|removed|blur|custom, background?: base64 PNG or JPEG (the replacement, same limit), color?: [r,g,b]|[r,g,b,a]|#RRGGBB|#RRGGBBAA, feather?: number 0..=64 (default 2), blur?: number 0..=64 (default 0, which picks a radius from the frame size), accelerator?: cpu} → {image: base64 png, width, height, mode, ms} (does not touch the deck; synchronous, about 2 s for a segmentation mode)",
            always,
            preview
        ),
    ]
}

fn parse_mode(p: &Value, cmd: &str) -> Result<CameraBackgroundMode> {
    let raw = str_param(p, "mode").ok_or_else(|| bad(cmd, "missing `mode`"))?;
    CameraBackgroundMode::parse(raw).ok_or_else(|| bad(cmd, format!("unknown mode `{raw}`; use normal, removed, blur or custom")))
}

/// `[r, g, b]`, `[r, g, b, a]`, `#RRGGBB` or `#RRGGBBAA`; a value that is present but not one of
/// those is an error rather than silently dropped.
fn parse_color(p: &Value, cmd: &str) -> Result<Option<Rgba>> {
    let Some(v) = p.get("color") else { return Ok(None) };
    if v.is_null() {
        return Ok(None);
    }
    if let Some(s) = v.as_str() {
        let hex = s.trim().trim_start_matches('#');
        let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2).unwrap_or(""), 16).ok();
        if hex.len() == 6 || hex.len() == 8 {
            let (Some(r), Some(g), Some(b)) = (byte(0), byte(2), byte(4)) else {
                return Err(bad(cmd, format!("`color` is not a hex colour: {s}")));
            };
            let a = if hex.len() == 8 { byte(6).ok_or_else(|| bad(cmd, format!("`color` is not a hex colour: {s}")))? } else { 255 };
            return Ok(Some(Rgba { r, g, b, a }));
        }
        return Err(bad(cmd, format!("`color` is not #RRGGBB: {s}")));
    }
    let Some(a) = v.as_array() else {
        return Err(bad(cmd, "`color` must be [r,g,b], [r,g,b,a] or #RRGGBB"));
    };
    if a.len() != 3 && a.len() != 4 {
        return Err(bad(cmd, "`color` needs 3 or 4 channels"));
    }
    let mut c = [0u8; 4];
    c[3] = 255;
    for (i, slot) in c.iter_mut().enumerate() {
        if let Some(v) = a.get(i) {
            let n = v.as_u64().filter(|n| *n <= 255).ok_or_else(|| bad(cmd, "`color` channels are 0..255 integers"))?;
            *slot = n as u8;
        }
    }
    Ok(Some(Rgba { r: c[0], g: c[1], b: c[2], a: c[3] }))
}

fn parse_image(p: &Value, cmd: &str) -> Result<Option<MediaId>> {
    match p.get("image") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_u64().and_then(|n| u32::try_from(n).ok()).map(|n| Some(MediaId(n))).ok_or_else(|| bad(cmd, "`image` is not a media id")),
    }
}

fn requested(p: &Value, cmd: &str) -> Result<CameraBackground> {
    let mode = parse_mode(p, cmd)?;
    let color = parse_color(p, cmd)?;
    let image = parse_image(p, cmd)?;
    if mode == CameraBackgroundMode::Custom && color.is_none() && image.is_none() && p.get("imageData").is_none() {
        return Err(bad(cmd, "custom needs `color` or `image` for the replacement"));
    }
    Ok(CameraBackground { mode, color, image })
}

/// The dimensions a picture claims, from its header only: a hostile PNG that says 100000x100000 is
/// refused before a decoder (or the document) allocates anything for it.
fn header_dimensions(bytes: &[u8], what: &str, cmd: &str) -> Result<(u32, u32)> {
    let (w, h) = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| bad(cmd, format!("{what} is not an image: {e}")))?
        .into_dimensions()
        .map_err(|e| bad(cmd, format!("{what} could not be read: {e}")))?;
    if w == 0 || h == 0 || w > deckcraft_matte::MAX_DIM || h > deckcraft_matte::MAX_DIM {
        return Err(bad(cmd, format!("{what} is {w}x{h}; the pipeline accepts up to {}x{}", deckcraft_matte::MAX_DIM, deckcraft_matte::MAX_DIM)));
    }
    Ok((w, h))
}

/// A replacement picture the caller sends inline: bytes, the name and content type PowerPoint and
/// the media list will show. The command adds it to the document in the same undo step. The magic
/// and the header are checked, and the size is capped, before the bytes can reach the document.
fn image_data(p: &Value, cmd: &str) -> Result<Option<(String, String, Vec<u8>)>> {
    let Some(encoded) = str_param(p, "imageData") else { return Ok(None) };
    if encoded.len() > MAX_PREVIEW_BASE64 {
        return Err(bad(cmd, "`imageData` is larger than the command accepts"));
    }
    let bytes = super::base64_decode(encoded).ok_or_else(|| bad(cmd, "`imageData` is not base64"))?;
    if bytes.len() > MAX_INLINE_IMAGE_BYTES {
        return Err(bad(
            cmd,
            format!("`imageData` is {} MB; the limit is {} MB", bytes.len() / (1024 * 1024), MAX_INLINE_IMAGE_BYTES / (1024 * 1024)),
        ));
    }
    let (ext, mime) = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        ("png", "image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        ("jpg", "image/jpeg")
    } else {
        return Err(bad(cmd, "`imageData` must be a PNG or JPEG"));
    };
    header_dimensions(&bytes, "`imageData`", cmd)?;
    Ok(Some((format!("cameo-background.{ext}"), mime.to_string(), bytes)))
}

/// Refuse an inline picture that would take the presentation past its media budget. Without this a
/// script can add picture after picture in one undoable command and exhaust memory on load.
fn check_media_budget(s: &Session, cmd: &str, bytes: usize) -> Result<()> {
    let doc = s.doc()?;
    let total: u64 = doc.doc.media.iter().map(|m| m.data.len() as u64).sum();
    if total.saturating_add(bytes as u64) > MAX_DOCUMENT_MEDIA_BYTES {
        return Err(bad(
            cmd,
            format!("`imageData` would take the presentation past its {} MB media budget", MAX_DOCUMENT_MEDIA_BYTES / (1024 * 1024)),
        ));
    }
    Ok(())
}

fn set_one(sh: &mut deckcraft_model::Shape, bg: &CameraBackground, cmd: &str) -> Result<()> {
    let c = sh.cameo.as_mut().ok_or_else(|| bad(cmd, "select a Cameo"))?;
    c.background = Some(bg.clone());
    Ok(())
}

/// Every Cameo in the deck, in slide order. Counted before any media is added, so `all` with no
/// Cameo cannot leave an orphan picture in the document.
fn cameo_positions(doc: &deckcraft_model::Presentation) -> Vec<(usize, ShapeId)> {
    let mut out = Vec::new();
    for (slide_index, slide) in doc.slides.iter().enumerate() {
        deckcraft_model::walk(&slide.shapes, &mut |sh, _| {
            if sh.is_cameo() {
                out.push((slide_index, sh.id));
            }
        });
    }
    out
}

fn background(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "cameo.background";
    let all = bool_or(p, "all", false);
    if all && (p.get("id").is_some() || p.get("ids").is_some()) {
        return Err(bad(CMD, "`all` and `id`/`ids` name different targets; send one"));
    }
    let bg = requested(p, CMD)?;
    check_media(s, CMD, &bg)?;
    let data = image_data(p, CMD)?;
    if let Some((_, _, bytes)) = &data {
        check_media_budget(s, CMD, bytes.len())?;
    }
    if all {
        let (changed, mut ids) = s.edit(|doc, _| {
            let mut ids: Vec<ShapeId> = Vec::new();
            let positions = cameo_positions(doc);
            if positions.is_empty() {
                return Ok((0usize, ids));
            }
            let mut bg = bg.clone();
            if let Some((name, mime, bytes)) = data.clone() {
                bg.image = Some(doc.add_media(&name, &mime, bytes).ok_or_else(|| bad(CMD, NO_ID_SPACE))?);
            }
            let mut n = 0usize;
            for (slide_index, id) in positions {
                let slide = std::sync::Arc::make_mut(&mut doc.slides[slide_index]);
                if let Some(sh) = slide.shape_mut(id) {
                    set_one(sh, &bg, CMD)?;
                    n += 1;
                    ids.push(id);
                }
            }
            Ok((n, ids))
        })?;
        ids.sort_by_key(|id| id.0);
        return Ok(json!({"mode": bg.mode.as_str(), "ids": ids.iter().map(|i| i.0).collect::<Vec<_>>(), "changed": changed}));
    }
    let mut ids = targets(s, p)?;
    let explicit = p.get("ids").is_some() || p.get("id").is_some();
    if !explicit {
        let doc = s.doc()?;
        ids.retain(|id| doc.shape(*id).is_some_and(deckcraft_model::Shape::is_cameo));
    }
    let ids = cameo_ids(s, ids, CMD)?;
    s.edit(|doc, sel| {
        let mut bg = bg.clone();
        if let Some((name, mime, bytes)) = data.clone() {
            bg.image = Some(doc.add_media(&name, &mime, bytes).ok_or_else(|| bad(CMD, NO_ID_SPACE))?);
        }
        let shapes = crate::shapes_mut(doc, sel).ok_or_else(|| bad(CMD, "no slide"))?;
        for id in &ids {
            let sh = deckcraft_model::find_shape_mut(shapes, *id).ok_or_else(|| bad(CMD, "no such shape"))?;
            set_one(sh, &bg, CMD)?;
        }
        Ok(())
    })?;
    Ok(json!({
        "mode": bg.mode.as_str(),
        "ids": ids.iter().map(|i| i.0).collect::<Vec<_>>(),
        "changed": ids.len(),
    }))
}

fn cameo_ids(s: &Session, ids: Vec<ShapeId>, cmd: &str) -> Result<Vec<ShapeId>> {
    if ids.is_empty() {
        return Err(bad(cmd, "nothing selected"));
    }
    let doc = s.doc()?;
    if !ids.iter().all(|id| doc.shape(*id).is_some_and(deckcraft_model::Shape::is_cameo)) {
        return Err(bad(cmd, "select a Cameo"));
    }
    Ok(ids)
}

fn state(s: &mut Session, _p: &Value) -> Result<Value> {
    let (bg, has) = {
        let doc = s.doc().ok();
        let selected = doc
            .as_ref()
            .and_then(|d| d.selected_shapes().iter().find(|sh| sh.is_cameo()).map(|sh| (sh.cameo.as_ref().and_then(|c| c.background.clone()), true)));
        match selected {
            Some((bg, has)) => (bg, has),
            None => (None, false),
        }
    };
    let mode = bg.as_ref().map(|b| b.mode).unwrap_or_default();
    let color = bg.as_ref().and_then(|b| b.color).map(|c| json!([c.r, c.g, c.b, c.a]));
    let image = bg.as_ref().and_then(|b| b.image).map(|m| m.0);
    Ok(json!({
        "mode": mode.as_str(),
        "color": color,
        "image": image,
        "hasCameo": has,
        "segmenter": s.segmenter_name(),
        "accelerator": deckcraft_matte::MANIFEST.default_accelerator.as_str(),
        "accelerators": deckcraft_matte::MANIFEST.accelerators.iter().map(|a| a.as_str()).collect::<Vec<_>>(),
    }))
}

/// `image` must name a media item the document already holds. Both commands that store a
/// background check it, so a dangling id never reaches save or export; `imageData` adds the media
/// in the same undo step instead.
fn check_media(s: &Session, cmd: &str, bg: &CameraBackground) -> Result<()> {
    let Some(image) = bg.image else { return Ok(()) };
    if s.doc()?.doc.media(image).is_none() {
        return Err(bad(cmd, format!("no media with id {}", image.0)));
    }
    Ok(())
}

fn apply_to_all(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "cameo.backgroundApplyToAll";
    let bg = if p.get("mode").is_some() {
        requested(p, CMD)?
    } else {
        // A mixed selection is allowed by the command, and a plain shape has no background to
        // copy: take the first Cameo in the targets, not the first target.
        let ids = targets(s, p)?;
        let doc = s.doc()?;
        let id = ids
            .iter()
            .copied()
            .find(|id| doc.shape(*id).is_some_and(Shape::is_cameo))
            .ok_or_else(|| bad(CMD, "select a Cameo to copy its background"))?;
        doc.shape(id).and_then(|sh| sh.cameo.as_ref()).and_then(|c| c.background.clone()).unwrap_or_default()
    };
    check_media(s, CMD, &bg)?;
    let data = image_data(p, CMD)?;
    if let Some((_, _, bytes)) = &data {
        check_media_budget(s, CMD, bytes.len())?;
    }
    let (changed, mut ids) = s.edit(|doc, _| {
        let mut ids: Vec<ShapeId> = Vec::new();
        let positions = cameo_positions(doc);
        if positions.is_empty() {
            return Ok((0usize, ids));
        }
        let mut bg = bg.clone();
        if let Some((name, mime, bytes)) = data.clone() {
            bg.image = Some(doc.add_media(&name, &mime, bytes).ok_or_else(|| bad(CMD, NO_ID_SPACE))?);
        }
        let mut n = 0usize;
        for (slide_index, id) in positions {
            let slide = std::sync::Arc::make_mut(&mut doc.slides[slide_index]);
            if let Some(sh) = slide.shape_mut(id)
                && let Some(c) = sh.cameo.as_mut()
            {
                c.background = Some(bg.clone());
                n += 1;
                ids.push(id);
            }
        }
        Ok((n, ids))
    })?;
    ids.sort_by_key(|id| id.0);
    Ok(json!({"mode": bg.mode.as_str(), "ids": ids.iter().map(|i| i.0).collect::<Vec<_>>(), "changed": changed}))
}

/// Decode a picture for the preview. The header is checked first — at most `max_dim` on a side —
/// so a hostile PNG that claims a huge canvas is refused before the decoder allocates it. Every
/// client-caused failure is a `bad()` with the calling command's name.
fn decode_frame(bytes: &[u8], what: &str, cmd: &str, max_dim: u32) -> Result<RgbaFrame> {
    let (w, h) = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| bad(cmd, format!("{what} is not an image: {e}")))?
        .into_dimensions()
        .map_err(|e| bad(cmd, format!("{what} could not be read: {e}")))?;
    if w == 0 || h == 0 || w > max_dim || h > max_dim {
        return Err(bad(cmd, format!("{what} is {w}x{h}; the preview accepts up to {max_dim}x{max_dim}")));
    }
    let img = image::load_from_memory(bytes).map_err(|e| bad(cmd, format!("{what} could not be decoded: {e}")))?;
    let img = img.to_rgba8();
    RgbaFrame::new(img.width(), img.height(), img.into_raw()).map_err(|e| bad(cmd, e.message()))
}

/// A hash of a frame's pixels: the preview cache's key. Two calls with the same frame (a preview
/// loop, a repeated CLI run) reuse the matte instead of running 2 s of inference again.
fn preview_key(frame: &RgbaFrame) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    frame.width().hash(&mut hasher);
    frame.height().hash(&mut hasher);
    frame.pixels().hash(&mut hasher);
    hasher.finish()
}

/// One radius parameter: absent means `default`; present must be a finite number, clamped to the
/// documented 0..=64. A string, a null inside a number or a non-finite value is refused instead of
/// silently replaced by the default (which would make `feather: "2"` look like it worked).
fn radius_param(p: &Value, key: &str, default: u32, cmd: &str) -> Result<u32> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => {
            let n = v.as_f64().ok_or_else(|| bad(cmd, format!("`{key}` must be a number 0..=64")))?;
            if !n.is_finite() {
                return Err(bad(cmd, format!("`{key}` must be a finite number 0..=64")));
            }
            Ok(n.clamp(0.0, 64.0) as u32)
        }
    }
}

/// A millisecond clock for the preview's timing. `Instant::now()` panics on
/// `wasm32-unknown-unknown`, where the browser's clock stands in, as in `cmd::review` and
/// `recovery`.
fn now_ms() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64)
    }
}

fn preview(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "cameo.backgroundPreview";
    let mode = parse_mode(p, CMD)?;
    if let Some(name) = str_param(p, "accelerator") {
        let accelerator = deckcraft_matte::Accelerator::parse(name).ok_or_else(|| bad(CMD, format!("unknown accelerator `{name}`")))?;
        if accelerator != deckcraft_matte::Accelerator::Cpu {
            return Err(bad(CMD, deckcraft_matte::ACCELERATOR_UNAVAILABLE.to_string()));
        }
    }
    let color = parse_color(p, CMD)?;
    let encoded = str_param(p, "frame").ok_or_else(|| bad(CMD, "missing `frame` (base64 PNG or JPEG)"))?;
    if encoded.len() > MAX_PREVIEW_BASE64 {
        return Err(bad(CMD, "`frame` is larger than the preview command accepts"));
    }
    let bytes = super::base64_decode(encoded).ok_or_else(|| bad(CMD, "`frame` is not base64"))?;
    let frame = decode_frame(&bytes, "`frame`", CMD, MAX_PREVIEW_DIM)?;
    let replacement = match str_param(p, "background") {
        None => None,
        Some(b) => {
            if b.len() > MAX_PREVIEW_BASE64 {
                return Err(bad(CMD, "`background` is larger than the preview command accepts"));
            }
            let bytes = super::base64_decode(b).ok_or_else(|| bad(CMD, "`background` is not base64"))?;
            Some(decode_frame(&bytes, "`background`", CMD, MAX_PREVIEW_DIM)?)
        }
    };
    let feather = radius_param(p, "feather", DEFAULT_FEATHER, CMD)?;
    let blur = radius_param(p, "blur", 0, CMD)?;
    let options = CompositeOptions { mode, color: color.map(|c| [c.r, c.g, c.b]), blur_radius: (blur > 0).then_some(blur), feather };
    let start = now_ms();
    let matte = if mode == CameraBackgroundMode::Normal {
        None
    } else {
        let key = preview_key(&frame);
        match s.cached_preview_matte(key) {
            Some(m) => Some(m),
            None => {
                s.ensure_segmenter().map_err(EngineError::Other)?;
                let segmenter = s.segmenter_mut().ok_or_else(|| {
                    EngineError::Other(
                        deckcraft_matte::MatteError::Unavailable(
                            "camera background segmentation is not available: install the birefnet-lite-512 model".into(),
                        )
                        .message(),
                    )
                })?;
                let matte = segmenter.matte(&frame).map_err(|e| EngineError::Other(e.message()))?;
                s.cache_preview_matte(key, matte.clone());
                Some(matte)
            }
        }
    };
    let out = composite(&frame, matte.as_ref(), &options, replacement.as_ref()).map_err(|e| EngineError::Other(e.message()))?;
    let (w, h) = (out.width(), out.height());
    // The composite owns its pixels; move them into the encoder instead of copying the frame once
    // more for `RgbaImage::from_raw`.
    let img =
        image::RgbaImage::from_raw(w, h, out.into_pixels()).ok_or_else(|| EngineError::Other("the composite does not match its size".into()))?;
    let mut png = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|e| EngineError::Other(format!("cannot encode the preview: {e}")))?;
    Ok(json!({
        "image": super::base64_encode(png.get_ref()),
        "width": w,
        "height": h,
        "mode": mode.as_str(),
        "ms": (now_ms() - start).max(0.0) as u64,
    }))
}

#[cfg(test)]
mod tests {
    use deckcraft_matte::{EllipseSegmenter, Segmenter};
    use deckcraft_model::{CameraBackground, CameraBackgroundMode};
    use serde_json::json;

    use crate::Session;

    fn with_cameo() -> Session {
        let mut s = Session::with_new();
        s.execute("insert.cameo", &json!({})).expect("cameo");
        s
    }

    fn selected_background(s: &Session) -> Option<CameraBackground> {
        s.doc().expect("doc").selected_shapes().iter().find_map(|sh| sh.cameo.as_ref()?.background.clone())
    }

    fn png(w: u32, h: u32, px: [u8; 4]) -> String {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba(px));
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img).write_to(&mut out, image::ImageFormat::Png).expect("png");
        super::super::base64_encode(out.get_ref())
    }

    #[test]
    fn apply_to_all_copies_from_a_cameo_in_a_mixed_selection() {
        let mut s = with_cameo();
        let rect = s.execute("shape.insert", &json!({"preset": "rect", "rect": [10, 10, 40, 40]})).expect("rect")["id"].as_u64().expect("id");
        let cameo = s.execute("insert.cameo", &json!({})).expect("second cameo")["id"].as_u64().expect("id");
        s.execute("cameo.background", &json!({"mode": "blur", "id": cameo})).expect("blur");
        // A mixed selection is allowed; the plain shape must not become the source.
        s.execute("edit.select", &json!({"ids": [rect, cameo]})).expect("mixed selection");
        let v = s.execute("cameo.backgroundApplyToAll", &json!({})).expect("apply all");
        assert_eq!(v["mode"], "blur", "the first Cameo is the source, not the first selected shape");
        assert!(v["changed"].as_u64().unwrap_or(0) >= 2);
    }

    #[test]
    fn apply_to_all_refuses_a_media_id_the_document_does_not_hold() {
        let mut s = with_cameo();
        assert!(s.execute("cameo.backgroundApplyToAll", &json!({"mode": "custom", "image": 9999})).is_err());
    }

    #[test]
    fn apply_to_all_adds_an_inline_picture_as_media() {
        let mut s = with_cameo();
        s.execute("cameo.backgroundApplyToAll", &json!({"mode": "custom", "imageData": png(4, 4, [9, 8, 7, 255])})).expect("inline picture");
        let bg = selected_background(&s).expect("background");
        assert_eq!(bg.mode, CameraBackgroundMode::Custom);
        let id = bg.image.expect("media id");
        assert_eq!(s.doc().expect("doc").doc.media(id).map(|m| m.content_type.as_str()), Some("image/png"));
    }

    /// The desktop and MCP sessions have no segmenter of their own: the host hands the session a
    /// loader. `cameo.background.state` must answer from the cheap name probe — a status query
    /// cannot hash and parse 191 MB on the UI thread — and only a preview loads the model.
    #[test]
    fn the_hosts_loader_loads_only_for_a_preview() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

        let installed = Arc::new(AtomicBool::new(true));
        let loads = Arc::new(AtomicUsize::new(0));
        let (name_flag, load_flag, load_count) = (Arc::clone(&installed), Arc::clone(&installed), Arc::clone(&loads));
        let mut s = with_cameo();
        s.set_segmenter_loader(crate::SegmenterLoader {
            name: Box::new(move || name_flag.load(Ordering::Relaxed).then_some("stub-segmenter")),
            load: Box::new(move || {
                load_count.fetch_add(1, Ordering::Relaxed);
                if load_flag.load(Ordering::Relaxed) {
                    Ok(Box::new(EllipseSegmenter::default()) as Box<dyn Segmenter>)
                } else {
                    Err("Background model not installed".to_string())
                }
            }),
        });
        assert_eq!(s.execute("cameo.background.state", &json!({})).expect("state")["segmenter"], "stub-segmenter");
        assert_eq!(s.execute("cameo.background.state", &json!({})).expect("state")["segmenter"], "stub-segmenter");
        assert_eq!(loads.load(Ordering::Relaxed), 0, "a status query never loads the model");

        // The preview is what loads it, and from then on the state names the loaded segmenter.
        s.execute("cameo.backgroundPreview", &json!({"frame": png(8, 8, [1, 2, 3, 255]), "mode": "removed"})).expect("preview");
        assert_eq!(loads.load(Ordering::Relaxed), 1);
        assert_eq!(s.execute("cameo.background.state", &json!({})).expect("state")["segmenter"], Segmenter::name(&EllipseSegmenter::default()));

        // The model goes away and the host says so: a preview asks the loader again, once.
        installed.store(false, Ordering::Relaxed);
        s.sync_segmenter(false);
        assert!(s.execute("cameo.background.state", &json!({})).expect("state")["segmenter"].is_null());
        assert!(s.execute("cameo.backgroundPreview", &json!({"frame": png(8, 8, [1, 2, 3, 255]), "mode": "removed"})).is_err());
        let attempts = loads.load(Ordering::Relaxed);
        assert!(s.execute("cameo.backgroundPreview", &json!({"frame": png(8, 8, [1, 2, 3, 255]), "mode": "removed"})).is_err());
        assert_eq!(loads.load(Ordering::Relaxed), attempts, "a remembered failure is not retried per call");

        // The model is installed while the session runs (an MCP server outlives the install): the
        // probe now names it, so the cached failure is dropped without a restart.
        installed.store(true, Ordering::Relaxed);
        assert_eq!(s.execute("cameo.background.state", &json!({})).expect("state")["segmenter"], "stub-segmenter");
        s.execute("cameo.backgroundPreview", &json!({"frame": png(8, 8, [1, 2, 3, 255]), "mode": "removed"})).expect("the model appeared");
        assert_eq!(loads.load(Ordering::Relaxed), attempts + 1);
    }

    /// A scripted caller has no selection (`file.open` leaves none), and the Camera Format tab's
    /// selection gate must not decide `enabled` for it: `all` and `id` name their targets.
    #[test]
    fn insert_cameo_refuses_a_master_view_instead_of_leaving_it() {
        let mut s = Session::with_new();
        s.execute("view.slideMaster", &json!({})).expect("master view");
        let e = s.execute("insert.cameo", &json!({})).expect_err("not on the master");
        assert!(e.to_string().contains("slide"), "{e}");
        // The user is still in the master view and nothing was inserted.
        let doc = s.doc().expect("doc");
        assert_ne!(doc.selection.target, crate::Target::Slides, "still in the view the user was in");
        assert!(doc.doc.slides.iter().all(|sl| !sl.shapes.iter().any(deckcraft_model::Shape::is_cameo)));
    }

    #[test]
    fn a_scripted_background_needs_no_cameo_in_the_selection() {
        let mut s = Session::with_new();
        let cameo = s.execute("insert.cameo", &json!({})).expect("cameo")["id"].as_u64().expect("id");
        s.execute("shape.insert", &json!({"preset": "rect", "rect": [10, 10, 40, 40]})).expect("rect");
        // The selection holds only a plain shape now; the explicit id still gets through.
        s.execute("cameo.background", &json!({"mode": "removed", "id": cameo})).expect("an explicit id needs no Cameo in the selection");
        // `file.open` leaves no selection at all: `all` must still apply.
        s.execute("slide.new", &json!({})).expect("empty slide");
        let v = s.execute("cameo.background", &json!({"mode": "blur", "all": true})).expect("all needs no selection");
        assert_eq!(v["changed"].as_u64(), Some(1), "the one Cameo on slide 1");
    }

    #[test]
    fn each_mode_is_stored_undoable_and_queryable() {
        let mut s = with_cameo();
        for mode in ["normal", "removed", "blur"] {
            s.execute("cameo.background", &json!({"mode": mode})).expect(mode);
            assert_eq!(selected_background(&s).map(|b| b.mode.as_str()), Some(mode));
            let state = s.execute("cameo.background.state", &json!({})).expect("state");
            assert_eq!(state["mode"], mode);
        }
        s.execute("cameo.background", &json!({"mode": "custom", "color": [10, 20, 30]})).expect("custom");
        assert_eq!(selected_background(&s).and_then(|b| b.color).map(|c| [c.r, c.g, c.b]), Some([10, 20, 30]));
        s.execute("edit.undo", &json!({})).expect("undo");
        assert_eq!(selected_background(&s).map(|b| b.mode.as_str()), Some("blur"));
        s.execute("edit.redo", &json!({})).expect("redo");
        assert_eq!(selected_background(&s).map(|b| b.mode.as_str()), Some("custom"));
    }

    #[test]
    fn a_bad_alpha_nibble_is_refused_and_a_good_one_kept() {
        let mut s = with_cameo();
        assert!(
            s.execute("cameo.background", &json!({"mode": "custom", "color": "#0A141Ezz"})).is_err(),
            "an unparseable alpha is not silently opaque"
        );
        s.execute("cameo.background", &json!({"mode": "custom", "color": "#0A141E80"})).expect("alpha");
        let c = selected_background(&s).and_then(|b| b.color).expect("colour");
        assert_eq!([c.r, c.g, c.b, c.a], [10, 20, 30, 128]);
    }

    #[test]
    fn a_colour_can_be_a_hex_string_and_a_bad_mode_is_refused() {
        let mut s = with_cameo();
        s.execute("cameo.background", &json!({"mode": "custom", "color": "#0A141E"})).expect("hex");
        let c = selected_background(&s).and_then(|b| b.color).expect("colour");
        assert_eq!([c.r, c.g, c.b, c.a], [10, 20, 30, 255]);
        assert!(s.execute("cameo.background", &json!({"mode": "magic"})).is_err());
        assert!(s.execute("cameo.background", &json!({"mode": "custom"})).is_err(), "custom needs a replacement");
        assert!(s.execute("cameo.background", &json!({})).is_err(), "empty params are refused");
        assert!(s.execute("cameo.background", &json!({"mode": "blur", "color": "nope"})).is_err());
    }

    #[test]
    fn the_mode_survives_a_deckcraft_save_and_reopen() {
        let mut s = with_cameo();
        s.execute("cameo.background", &json!({"mode": "removed"})).expect("removed");
        let bytes = deckcraft_format::save(&s.doc().expect("doc").doc).expect("save");
        let back = deckcraft_format::load(&bytes).expect("load");
        let mut found = None;
        deckcraft_model::walk(&back.slides[0].shapes, &mut |sh, _| {
            if sh.is_cameo() {
                found = sh.cameo.as_ref().and_then(|c| c.background.clone());
            }
        });
        assert_eq!(found.map(|b| b.mode), Some(CameraBackgroundMode::Removed));
    }

    #[test]
    fn apply_to_all_copies_the_selected_background() {
        let mut s = with_cameo();
        s.execute("slide.new", &json!({})).expect("slide");
        s.execute("insert.cameo", &json!({"slide": 1})).expect("second cameo");
        s.execute("cameo.backgroundApplyToAll", &json!({"mode": "custom", "color": [1, 2, 3]})).expect("apply all");
        let mut modes = Vec::new();
        for slide in &s.doc().expect("doc").doc.slides {
            deckcraft_model::walk(&slide.shapes, &mut |sh, _| {
                if sh.is_cameo() {
                    modes.push(sh.cameo.as_ref().and_then(|c| c.background.as_ref()).map(|b| b.mode));
                }
            });
        }
        assert_eq!(modes, [Some(CameraBackgroundMode::Custom), Some(CameraBackgroundMode::Custom)]);
    }

    #[test]
    fn a_replacement_picture_arrives_inline_and_becomes_media() {
        let mut s = with_cameo();
        s.execute("cameo.background", &json!({"mode": "custom", "imageData": png(4, 4, [9, 8, 7, 255])})).expect("inline picture");
        let bg = selected_background(&s).expect("background");
        assert_eq!(bg.mode, CameraBackgroundMode::Custom);
        let id = bg.image.expect("media id");
        let doc = s.doc().expect("doc");
        assert_eq!(doc.doc.media(id).map(|m| m.content_type.as_str()), Some("image/png"));
        assert!(s.execute("cameo.background", &json!({"mode": "custom", "imageData": "bm90IGFuIGltYWdl"})).is_err());
        // The media and the mode are one undo step: the cameo had no background before.
        s.execute("edit.undo", &json!({})).expect("undo");
        assert_eq!(selected_background(&s), None, "one undo removes the mode and the media");
    }

    #[test]
    fn a_plain_shape_is_not_a_cameo_and_is_refused() {
        let mut s = Session::with_new();
        s.execute("shape.insert", &json!({"preset": "rect", "rect": [10, 10, 40, 40]})).expect("shape");
        assert!(s.execute("cameo.background", &json!({"mode": "blur"})).is_err());
    }

    #[test]
    fn the_offscreen_preview_composites_with_the_installed_segmenter() {
        let mut s = with_cameo();
        // Normal needs no model at all.
        let r = s.execute("cameo.backgroundPreview", &json!({"frame": png(8, 8, [40, 50, 60, 255]), "mode": "normal"})).expect("normal");
        assert_eq!(r["mode"], "normal");
        assert!(s.execute("cameo.backgroundPreview", &json!({"frame": png(8, 8, [0, 0, 0, 255]), "mode": "removed"})).is_err(), "no segmenter yet");

        s.set_segmenter(Box::new(EllipseSegmenter::default()));
        let removed = s
            .execute("cameo.backgroundPreview", &json!({"frame": png(16, 16, [40, 50, 60, 255]), "mode": "removed", "feather": 0}))
            .expect("removed");
        let bytes = super::super::base64_decode(removed["image"].as_str().expect("png")).expect("base64");
        let img = image::load_from_memory(&bytes).expect("decode").to_rgba8();
        assert_eq!(img.get_pixel(8, 8)[3], 255, "the subject stays opaque");
        assert_eq!(img.get_pixel(0, 0)[3], 0, "the corner is transparent");

        let custom = s
            .execute(
                "cameo.backgroundPreview",
                &json!({
                    "frame": png(16, 16, [40, 50, 60, 255]),
                    "mode": "custom",
                    "color": [200, 10, 10],
                    "feather": 0
                }),
            )
            .expect("custom");
        let bytes = super::super::base64_decode(custom["image"].as_str().expect("png")).expect("base64");
        let img = image::load_from_memory(&bytes).expect("decode").to_rgba8();
        assert_eq!(img.get_pixel(0, 0), &image::Rgba([200, 10, 10, 255]));
        assert_eq!(img.get_pixel(8, 8), &image::Rgba([40, 50, 60, 255]));

        assert!(s.execute("cameo.backgroundPreview", &json!({})).is_err());
        let err = s
            .execute("cameo.backgroundPreview", &json!({"frame": png(4, 4, [1, 2, 3, 255]), "mode": "normal", "accelerator": "openvino"}))
            .expect_err("no openvino");
        assert!(err.to_string().contains("OpenVINO"), "{err}");
        assert!(s.execute("cameo.backgroundPreview", &json!({"frame": "%%%", "mode": "blur"})).is_err());
    }

    #[test]
    fn the_preview_does_not_change_the_document() {
        let mut s = with_cameo();
        s.set_segmenter(Box::new(EllipseSegmenter::default()));
        let before = deckcraft_format::save(&s.doc().expect("doc").doc).expect("save");
        let journal = s.journal.len();
        s.execute("cameo.backgroundPreview", &json!({"frame": png(8, 8, [1, 2, 3, 255]), "mode": "blur"})).expect("preview");
        let after = deckcraft_format::save(&s.doc().expect("doc").doc).expect("save");
        assert_eq!(before, after, "the preview is not a document edit");
        assert_eq!(s.journal.len(), journal, "the preview adds no journal entry and no undo step");
        // A preview needs no document at all: the command is a query.
        let mut empty = Session::new();
        empty.execute("cameo.backgroundPreview", &json!({"frame": png(8, 8, [1, 2, 3, 255]), "mode": "normal"})).expect("a preview with no document");
        assert!(empty.journal.is_empty(), "a preview with no document journals nothing");
    }

    /// The preview is bounded on both axes: a frame the decoder would allocate hundreds of
    /// megabytes for is refused from its header, at 1280 px (not the pipeline's 4096).
    #[test]
    fn the_preview_rejects_a_frame_over_its_pixel_limit() {
        let mut s = with_cameo();
        let err = s
            .execute("cameo.backgroundPreview", &json!({"frame": png(super::MAX_PREVIEW_DIM + 1, 8, [1, 2, 3, 255]), "mode": "normal"}))
            .expect_err("too wide");
        assert!(err.to_string().contains("1280"), "{err}");
        let err = s
            .execute("cameo.backgroundPreview", &json!({"frame": png(8, super::MAX_PREVIEW_DIM + 1, [1, 2, 3, 255]), "mode": "normal"}))
            .expect_err("too tall");
        assert!(err.to_string().contains("1280"), "{err}");
        assert!(s.execute("cameo.backgroundPreview", &json!({"frame": png(4, 4, [1, 2, 3, 255]), "mode": "normal"})).is_ok());
    }

    /// The encoded payload is checked before base64 decoding, so a 64 MB string is refused without
    /// allocating the decoded copy on top of it.
    #[test]
    fn the_preview_rejects_an_encoded_payload_over_its_limit() {
        let mut s = with_cameo();
        let huge = "A".repeat(super::MAX_PREVIEW_BASE64 + 1);
        let err = s.execute("cameo.backgroundPreview", &json!({"frame": huge, "mode": "normal"})).expect_err("too large");
        assert!(err.to_string().contains("larger than"), "{err}");
    }

    /// Custom mode composes a replacement picture: the corner takes its colour, the subject stays.
    #[test]
    fn the_preview_uses_a_background_picture() {
        let mut s = with_cameo();
        s.set_segmenter(Box::new(EllipseSegmenter::default()));
        let answer = s
            .execute(
                "cameo.backgroundPreview",
                &json!({
                    "frame": png(16, 16, [40, 50, 60, 255]),
                    "background": png(8, 8, [0, 200, 0, 255]),
                    "mode": "custom",
                    "feather": 0
                }),
            )
            .expect("custom with a picture");
        let bytes = super::super::base64_decode(answer["image"].as_str().expect("png")).expect("base64");
        let img = image::load_from_memory(&bytes).expect("decode").to_rgba8();
        assert_eq!(img.get_pixel(0, 0), &image::Rgba([0, 200, 0, 255]), "the replacement is resized and drawn");
        assert_eq!(img.get_pixel(8, 8), &image::Rgba([40, 50, 60, 255]), "the subject is kept");
    }

    /// A radius that is present but not a number (or not finite) is an error, not the default; a
    /// huge finite radius is clamped to the documented 0..=64.
    #[test]
    fn the_preview_rejects_a_non_numeric_radius_and_clamps_a_large_one() {
        let mut s = with_cameo();
        for (key, value) in [("feather", json!("2")), ("blur", json!("4")), ("feather", json!(true))] {
            let mut params = json!({"frame": png(4, 4, [1, 2, 3, 255]), "mode": "normal"});
            params[key] = value;
            let err = s.execute("cameo.backgroundPreview", &params).expect_err("not a number");
            assert!(err.to_string().contains(key), "{err}");
        }
        s.execute("cameo.backgroundPreview", &json!({"frame": png(4, 4, [1, 2, 3, 255]), "mode": "normal", "feather": 1e9, "blur": 1e9}))
            .expect("a finite radius is clamped, not refused");
    }

    /// The same frame previewed twice is segmented once: the panel's preview loop with an unchanged
    /// camera frame must not pay 2 s of inference per call. A different frame segments again.
    #[test]
    fn the_preview_reuses_the_matte_for_an_unchanged_frame() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct CountingSegmenter(Arc<AtomicUsize>);
        impl Segmenter for CountingSegmenter {
            fn matte(&mut self, frame: &deckcraft_matte::frame::RgbaFrame) -> Result<deckcraft_matte::Matte, deckcraft_matte::MatteError> {
                self.0.fetch_add(1, Ordering::Relaxed);
                let mut inner = EllipseSegmenter::default();
                inner.matte(frame)
            }
            fn name(&self) -> &'static str {
                "counting"
            }
        }

        let runs = Arc::new(AtomicUsize::new(0));
        let mut s = with_cameo();
        s.set_segmenter(Box::new(CountingSegmenter(Arc::clone(&runs))));
        let frame = png(16, 16, [40, 50, 60, 255]);
        for _ in 0..3 {
            s.execute("cameo.backgroundPreview", &json!({"frame": frame, "mode": "blur"})).expect("preview");
        }
        assert_eq!(runs.load(Ordering::Relaxed), 1, "one frame, one inference");
        s.execute("cameo.backgroundPreview", &json!({"frame": png(16, 16, [1, 2, 3, 255]), "mode": "blur"})).expect("a new frame");
        assert_eq!(runs.load(Ordering::Relaxed), 2, "a changed frame is segmented again");
    }

    /// `all` with no Cameo in the deck changes nothing and adds no media: the picture would
    /// otherwise sit in the document with nothing using it.
    #[test]
    fn all_with_no_cameo_changes_nothing_and_adds_no_media() {
        let mut s = Session::with_new();
        let media = s.doc().expect("doc").doc.media.len();
        let v = s
            .execute("cameo.background", &json!({"mode": "custom", "all": true, "imageData": png(4, 4, [9, 8, 7, 255])}))
            .expect("all with no cameo is not an error");
        assert_eq!(v["changed"].as_u64(), Some(0));
        assert_eq!(v["ids"], json!([]));
        assert_eq!(s.doc().expect("doc").doc.media.len(), media, "no orphan picture");
    }

    /// `all` and `id`/`ids` name different targets: silently preferring one would edit the wrong
    /// shapes.
    #[test]
    fn all_with_an_explicit_id_is_refused() {
        let mut s = with_cameo();
        let id = s.doc().expect("doc").selected_shapes().first().map(|sh| sh.id.0).unwrap_or(1);
        let err = s.execute("cameo.background", &json!({"mode": "blur", "all": true, "id": id})).expect_err("conflicting targets");
        assert!(err.to_string().contains("all"), "{err}");
        assert!(s.execute("cameo.background", &json!({"mode": "blur", "all": true, "ids": [id]})).is_err());
    }

    #[test]
    fn the_installed_segmenter_names_itself_in_the_state() {
        let mut s = with_cameo();
        assert!(s.execute("cameo.background.state", &json!({})).expect("state")["segmenter"].is_null());
        s.set_segmenter(Box::new(EllipseSegmenter::default()));
        assert_eq!(s.execute("cameo.background.state", &json!({})).expect("state")["segmenter"], Segmenter::name(&EllipseSegmenter::default()));
        let state = s.execute("cameo.background.state", &json!({})).expect("state");
        assert_eq!(state["accelerator"], "cpu");
        assert_eq!(state["accelerators"], json!(["cpu"]));
        assert_eq!(state["hasCameo"], true, "the state says whether a Cameo is selected");
        assert_eq!(state["mode"], "normal");
    }
}
