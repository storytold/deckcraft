//! File menu: new, open, save, export, close, properties.

use std::collections::HashMap;
use std::sync::Arc;

use deckcraft_media::MediaInfo;
use deckcraft_model::{MediaId, MediaItem, Presentation, Shape, ShapeKind, defaults};
use serde_json::{Value, json};

use super::*;
use crate::{DocState, EngineError, Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            noundo "file.new",
            "New Presentation",
            ["File"],
            Some("Cmd+N"),
            "{theme?: name, template?: starter name, size?: [w,h] pt, blank?: bool}",
            always,
            new
        ),
        cmd!(query "file.templates", "Starter Templates", [], None, "{} → [{name, summary, theme, slides}]", always, templates),
        cmd!(noundo "file.open", "Open…", ["File"], Some("Cmd+O"), "{path}", always, open),
        cmd!(
            noundo "file.adopt",
            "Open",
            [],
            None,
            "{path, token?} (a presentation the host read in the background and staged with Session::stage_loaded; without one, reads path like file.open)",
            always,
            adopt
        ),
        cmd!(noundo "file.openBytes", "Open Data", [], None, "{name, data: base64}", always, open_bytes),
        cmd!(noundo "file.save", "Save", ["File"], Some("Cmd+S"), "{path?, format?: deckcraft|pptx}", has_doc, save),
        cmd!(noundo "file.saveAs", "Save As…", ["File"], Some("Cmd+Shift+S"), "{path, format?}", has_doc, save_as),
        cmd!(noundo "file.saveTemplate", "Save as Template…", ["File"], None, "{path}", has_doc, save_as),
        cmd!(query "file.saveBytes", "Save to Data", [], None, "{format?: deckcraft|pptx} → {data: base64}", has_doc, save_bytes_cmd),
        cmd!(noundo "file.export", "Export…", ["File"], None, "{path, format?: png|jpeg|pptx|deckcraft|outline|pdf|gif (else by path), slide?: index, all?: bool, scale?: px per pt, layout?: slides|notes|handouts (pdf), perPage?: 1|2|3|4|6|9, paper?: letter|a4|legal|a3 (pdf), orientation?: portrait|landscape (pdf), dpi?, slides?: [index]|range (\"1-3,5\"), includeHidden?, textLayer?, frame?, transparentBackground?: bool (png: leave the slide background out), width?: px 16-3840|small|medium|large|extraLarge (gif), secondsPerSlide?: 0.1-600 (gif; 5), loop?: bool (gif; true: forever)} → {path, bytes} or {files} (png, jpeg); gif adds frames, width, height, secondsPerSlide", has_doc, export),
        cmd!(
            query "file.info",
            "Info",
            ["File"],
            None,
            "{} → {title, path, format, dirty, slides, hiddenSlides, sections, bytes?, properties}",
            has_doc,
            info
        ),
        cmd!(query "file.render", "Render Slide", [], None, "{slide?: index, scale?, edit?: bool, transparentBackground?: bool} → {png: base64, width, height}", has_doc, render),
        cmd!(
            query "slide.colorAt",
            "Color At",
            [],
            None,
            "{slide?: index, x, y (slide units), scale?: px per unit (2)} → {color: \"RRGGBB\", alpha: 0..255, x, y}: the colour drawn at a point of the slide (the Eyedropper)",
            has_doc,
            color_at
        ),
        cmd!(noundo "file.close", "Close", ["File"], Some("Cmd+W"), "{}", has_doc, close),
        cmd!(
            "file.properties",
            "Properties",
            ["File"],
            None,
            "{title?, subject?, author?, keywords?, comments?, category?, company?} → properties",
            has_doc,
            properties
        ),
        cmd!(noundo "file.recovery.save", "Save AutoRecover Information", [], None, "{} → {written}", has_doc, recovery_save),
        cmd!(query "file.recovery.list", "Recovered Presentations", [], None, "{} → [{uid, path, title, saved}]", always, recovery_list),
        cmd!(noundo "file.recovery.open", "Open Recovered Presentations", [], None, "{} → {opened: [document index]}", always, recovery_open),
        cmd!(noundo "file.recovery.discard", "Discard Recovered Presentations", [], None, "{uid?: one entry (else all)}", always, recovery_discard),
        cmd!(noundo "file.revert", "Revert", [], None, "{}", has_doc, revert),
        cmd!(noundo "window.next", "Next Window", ["Window"], Some("Cmd+`"), "{index?}", has_doc, next_window),
    ]
}

fn theme_named(name: &str) -> Option<deckcraft_model::Theme> {
    deckcraft_model::theme::builtin_themes().into_iter().find(|t| t.name.eq_ignore_ascii_case(name))
}

fn slide_size(p: &Value) -> deckcraft_geom::Size {
    p.get("size")
        .and_then(Value::as_array)
        .and_then(|a| Some(deckcraft_geom::Size::new(a.first()?.as_f64()?, a.get(1)?.as_f64()?)))
        .filter(|s| s.width >= 1.0 && s.height >= 1.0 && s.width < 5000.0 && s.height < 5000.0)
        .unwrap_or(defaults::WIDE)
}

fn templates(_s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(json!({
        "templates": crate::templates::starters().iter().map(|t| json!({
            "name": t.name,
            "summary": t.summary,
            "theme": t.theme,
            "slides": t.slides.len(),
        })).collect::<Vec<_>>(),
    }))
}

fn new(s: &mut Session, p: &Value) -> Result<Value> {
    let template = str_param(p, "template").filter(|t| !t.is_empty());
    let theme_name = str_param(p, "theme").filter(|t| !t.is_empty());
    if template.is_some() && theme_name.is_some() {
        return Err(bad("file.new", "pass `theme` or `template`, not both"));
    }
    if template.is_some() && bool_or(p, "blank", false) {
        return Err(bad("file.new", "a template is not a blank presentation"));
    }
    let size = slide_size(p);
    let (doc, template_name) = if let Some(name) = template {
        let doc = crate::templates::build(name, size)?;
        let chosen = crate::templates::find(name).map(|t| t.name);
        (doc, chosen)
    } else {
        let theme = theme_name.and_then(theme_named).unwrap_or_default();
        (defaults::blank_presentation(size, theme, !bool_or(p, "blank", false)), None)
    };
    let slides = doc.slides.len();
    let title = s.next_untitled();
    let i = s.add_document(DocState::new(doc, None, title));
    let mut out = json!({"document": i, "slides": slides});
    if let Some(name) = template_name {
        out["template"] = json!(name);
    }
    Ok(out)
}

/// Recognise and read presentation bytes (native, PPTX).
pub fn open_presentation(name: &str, bytes: &[u8]) -> Result<Presentation> {
    open_checked(name, bytes).map(|(p, _)| p)
}

/// What a file lost on opening.
#[derive(Debug, Default, Clone)]
pub struct Damage {
    /// Pictures and media a `.deckcraft` file had lost or damaged.
    pub media: Vec<String>,
    /// Slides a `.pptx` lists but that couldn't be read, so they are not in the presentation.
    pub slides: Vec<String>,
}

impl Damage {
    pub fn is_empty(&self) -> bool {
        self.media.is_empty() && self.slides.is_empty()
    }
}

/// [`open_presentation`], plus what the file had lost or damaged.
pub fn open_checked(name: &str, bytes: &[u8]) -> Result<(Presentation, Damage)> {
    if deckcraft_format::sniff(bytes) {
        let (p, media) = deckcraft_format::load_checked(bytes).map_err(|e| EngineError::Other(e.to_string()))?;
        return Ok((p, Damage { media, slides: vec![] }));
    }
    open_other(name, bytes).map(|(p, slides)| (p, Damage { media: vec![], slides }))
}

fn open_other(name: &str, bytes: &[u8]) -> Result<(Presentation, Vec<String>)> {
    if deckcraft_pptx::sniff(bytes) {
        let (mut p, lost_slides) = deckcraft_pptx::import_checked(bytes).map_err(|e| EngineError::Other(e.to_string()))?;
        deckcraft_format::repair(&mut p);
        probe_clips(&mut p);
        return Ok((p, lost_slides));
    }
    if deckcraft_pptx::is_encrypted(bytes) {
        return Err(EngineError::Other(format!("{name} is password-protected. DeckCraft can't open password-protected presentations yet.")));
    }
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".txt") || lower.ends_with(".md") {
        let text = String::from_utf8_lossy(bytes);
        let mut p = defaults::blank_presentation(defaults::WIDE, Default::default(), false);
        if lower.ends_with(".md") {
            deckcraft_format::markdown_to_slides(&mut p, &text);
        } else {
            deckcraft_format::outline_to_slides(&mut p, &text);
        }
        if p.slides.is_empty() {
            p = defaults::new_presentation(None);
        }
        return Ok((p, vec![]));
    }
    Err(EngineError::Other(format!("{name}: not a presentation DeckCraft can read")))
}

/// Fill in each audio and video clip's length and picture size from its bytes: a `.pptx` doesn't
/// store them. Headers only, so opening never decodes a whole audio file.
fn probe_clips(p: &mut Presentation) {
    fn rec(shapes: &mut [Shape], media: &[MediaItem], probed: &mut HashMap<MediaId, Option<MediaInfo>>, depth: usize) {
        if depth > 64 {
            return;
        }
        for s in shapes {
            if let ShapeKind::Media(m) = &mut s.kind
                && (m.duration_ms == 0 || m.width == 0 && m.height == 0)
                && let Some(info) = probed
                    .entry(m.media)
                    .or_insert_with(|| media.iter().find(|i| i.id == m.media).and_then(|i| deckcraft_media::probe_headers(&i.data).ok()))
            {
                if m.duration_ms == 0 {
                    m.duration_ms = info.duration_ms.min(u32::MAX as u64) as u32;
                }
                if m.width == 0
                    && m.height == 0
                    && let Some(v) = &info.video
                {
                    (m.width, m.height) = (v.width, v.height);
                }
            }
            if let Some(children) = s.children_mut() {
                rec(children, media, probed, depth + 1);
            }
        }
    }
    let mut probed = HashMap::new();
    // Just read, so no slide is shared yet and `make_mut` doesn't copy.
    for slide in &mut p.slides {
        rec(&mut Arc::make_mut(slide).shapes, &p.media, &mut probed, 0);
    }
}

pub fn add_opened(s: &mut Session, name: &str, path: Option<String>, p: Presentation) -> usize {
    let title = std::path::Path::new(name).file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| name.to_string());
    crate::fonts::install_embedded_fonts(&p);
    let missing_fonts = crate::fonts::missing_fonts(&p);
    let mut st = DocState::new(p, path, title);
    st.missing_fonts = missing_fonts;
    s.add_document(st)
}

/// A presentation read from disk, not yet added to a session: [`read_presentation_file`] needs no
/// session, so a host can run it on a background thread, hand the result to
/// [`Session::stage_loaded`] and add it with `file.adopt`.
#[derive(Debug)]
pub struct LoadedFile {
    pub path: String,
    pub doc: Presentation,
    /// A native or PPTX file (saving goes back to it); an outline import has no file to save to.
    pub native: bool,
    /// What the file had lost or damaged.
    pub damaged: Damage,
}

/// Read and parse the presentation at `path`.
#[cfg(not(target_arch = "wasm32"))]
pub fn read_presentation_file(path: &str) -> Result<LoadedFile> {
    let bytes = crate::io::read_file(path).map_err(EngineError::Other)?;
    let (doc, damaged) = open_checked(path, &bytes)?;
    let native = deckcraft_format::sniff(&bytes) || deckcraft_pptx::sniff(&bytes);
    Ok(LoadedFile { path: path.to_string(), doc, native, damaged })
}

/// Add a loaded presentation as the active document: the shared end of `file.open` and
/// `file.adopt`. A damaged file opens as an untitled repaired copy, so Save can't overwrite the
/// original with the empty media.
fn adopt_loaded(s: &mut Session, f: LoadedFile) -> Result<Value> {
    let LoadedFile { path, doc, native, damaged } = f;
    let keep_path = native && damaged.is_empty();
    let i = add_opened(s, &path, keep_path.then(|| path.clone()), doc);
    report_damage(s, &damaged)?;
    let st = s.doc()?;
    Ok(json!({
        "document": i, "slides": st.doc.slides.len(), "missingFonts": st.missing_fonts,
        "damagedMedia": damaged.media, "lostSlides": damaged.slides,
    }))
}

#[cfg(not(target_arch = "wasm32"))]
fn open(s: &mut Session, p: &Value) -> Result<Value> {
    let path = str_param(p, "path").ok_or_else(|| bad("file.open", "missing `path`"))?;
    adopt_loaded(s, read_presentation_file(path)?)
}

/// `file.adopt {path, token?}`: add the presentation a host read in the background and staged
/// with [`Session::stage_loaded`]. Without that staged file (a replayed journal, a stale token)
/// it reads `path` like `file.open`, so the journal entry stays replayable.
fn adopt(s: &mut Session, p: &Value) -> Result<Value> {
    let path = str_param(p, "path").ok_or_else(|| bad("file.adopt", "missing `path`"))?;
    let staged = p.get("token").and_then(Value::as_u64).and_then(|t| s.take_loaded(t)).filter(|f| f.path == path);
    match staged {
        Some(f) => adopt_loaded(s, f),
        #[cfg(not(target_arch = "wasm32"))]
        None => adopt_loaded(s, read_presentation_file(path)?),
        #[cfg(target_arch = "wasm32")]
        None => Err(bad("file.adopt", format!("{path} was not loaded"))),
    }
}
#[cfg(target_arch = "wasm32")]
fn open(_s: &mut Session, _p: &Value) -> Result<Value> {
    Err(EngineError::Other("use file.openBytes in the browser".into()))
}

fn open_bytes(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").unwrap_or("Presentation");
    let data = str_param(p, "data").and_then(base64_decode).ok_or_else(|| bad("file.openBytes", "missing or invalid base64 `data`"))?;
    let (doc, damaged) = open_checked(name, &data)?;
    let i = add_opened(s, name, None, doc);
    report_damage(s, &damaged)?;
    Ok(json!({"document": i, "missingFonts": s.doc()?.missing_fonts, "damagedMedia": damaged.media, "lostSlides": damaged.slides}))
}

/// Tell the user which pictures, media and slides a damaged file lost, and mark the copy repaired.
fn report_damage(s: &mut Session, damaged: &Damage) -> Result<()> {
    if damaged.is_empty() {
        return Ok(());
    }
    let st = s.doc_mut()?;
    st.title = format!("{} (Repaired)", st.title);
    let mut text = String::new();
    let n = damaged.media.len();
    if n > 0 {
        text = format!(
            "{n} picture{} or media file{} in this presentation couldn't be read and {} empty. ",
            if n == 1 { "" } else { "s" },
            if n == 1 { "" } else { "s" },
            if n == 1 { "is" } else { "are" },
        );
    }
    let n = damaged.slides.len();
    if n > 0 {
        text.push_str(&format!(
            "{n} slide{} in this file couldn't be read and {} left out. ",
            if n == 1 { "" } else { "s" },
            if n == 1 { "was" } else { "were" },
        ));
    }
    text.push_str("It was opened as a repaired copy; save it under a new name to keep the original file.");
    s.ui_requests.push(crate::UiRequest::Message { text });
    Ok(())
}

/// Encode the presentation in `format` (`deckcraft`, `pptx`, `potx`, `ppsx`, `outline`, `pdf`, `gif`).
pub fn save_bytes(doc: &Presentation, format: &str) -> Result<Vec<u8>> {
    match format {
        f @ ("pptx" | "potx" | "ppsx") => deckcraft_pptx::export_as(doc, f).map_err(|e| EngineError::Other(e.to_string())),
        "outline" | "txt" => Ok(deckcraft_format::slides_to_outline(doc).into_bytes()),
        "pdf" => deckcraft_pdf::export(doc, &Default::default()).map_err(|e| EngineError::Other(e.to_string())),
        "gif" => gif_bytes(doc, &Value::Null),
        _ => deckcraft_format::save(doc).map_err(|e| EngineError::Other(e.to_string())),
    }
}

/// Paper sizes for notes and handouts, for the UI's Print page.
pub use deckcraft_pdf::{PAPERS, paper_size};

/// PDF options from command params: `{layout: slides|notes|handouts, perPage, dpi, slides,
/// includeHidden, textLayer, frame, paper: letter|a4|legal|a3, orientation: portrait|landscape}` for a presentation of `count` slides. `slides` is a list of
/// 0-based indices or a range as typed in PowerPoint's Print page (`"1-3, 5"`, 1-based).
pub fn pdf_options(p: &Value, count: usize) -> Result<deckcraft_pdf::PdfOptions> {
    let mut o = deckcraft_pdf::PdfOptions { layout: deckcraft_pdf::PageLayout::Slides, ..Default::default() };
    o.layout = match str_param(p, "layout").unwrap_or("slides") {
        "notes" => deckcraft_pdf::PageLayout::Notes,
        "handouts" => deckcraft_pdf::PageLayout::Handouts { per_page: usize_param(p, "perPage").unwrap_or(6).clamp(1, 9) as u8 },
        _ => deckcraft_pdf::PageLayout::Slides,
    };
    o.dpi = f64_or(p, "dpi", o.dpi).clamp(36.0, 600.0);
    o.slides = match p.get("slides") {
        Some(Value::Array(a)) => Some(a.iter().filter_map(|v| v.as_u64().map(|x| x as usize)).filter(|i| *i < count).collect()),
        Some(Value::String(text)) if !text.trim().is_empty() => Some(parse_slide_range(text, count)?),
        _ => None,
    };
    o.include_hidden = bool_or(p, "includeHidden", false);
    o.text_layer = bool_or(p, "textLayer", true);
    o.frame_slides = bool_or(p, "frame", true);
    let landscape = match str_param(p, "orientation").unwrap_or("portrait") {
        "portrait" => false,
        "landscape" => true,
        other => return Err(bad("file.export", format!("orientation “{other}” isn't portrait or landscape"))),
    };
    let paper = str_param(p, "paper").unwrap_or("letter");
    o.paper = deckcraft_pdf::paper_size(paper, landscape).ok_or_else(|| {
        let ids: Vec<&str> = deckcraft_pdf::PAPERS.iter().map(|p| p.0).collect();
        bad("file.export", format!("paper “{paper}” isn't one of {}", ids.join(", ")))
    })?;
    Ok(o)
}

/// A slide range as typed in the Print page's Slides box (`"1-3, 5"`; 1-based, either separator
/// `,` or `;`, ranges either way round) → 0-based indices in order, without repeats. Numbers past
/// the last slide or a range that isn't numbers is an error, as in PowerPoint.
pub fn parse_slide_range(text: &str, count: usize) -> Result<Vec<usize>> {
    let invalid = || bad("file.export", format!("“{text}” isn't a valid slide range: use numbers and ranges such as 1-3, 5"));
    let mut out: Vec<usize> = vec![];
    for part in text.split([',', ';']).map(str::trim).filter(|p| !p.is_empty()) {
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => (a.trim(), b.trim()),
            None => (part, part),
        };
        let (a, b) = (a.parse::<usize>().map_err(|_| invalid())?, b.parse::<usize>().map_err(|_| invalid())?);
        let (lo, hi) = (a.min(b), a.max(b));
        if lo == 0 || hi > count {
            return Err(bad("file.export", format!("the presentation has {count} slides; “{part}” is outside them")));
        }
        for i in lo - 1..hi {
            if !out.contains(&i) {
                out.push(i);
            }
        }
    }
    if out.is_empty() {
        return Err(invalid());
    }
    Ok(out)
}

/// PDF bytes for `doc` with [`pdf_options`] from `p`.
pub fn pdf_bytes(doc: &Presentation, p: &Value) -> Result<Vec<u8>> {
    deckcraft_pdf::export(doc, &pdf_options(p, doc.slides.len())?).map_err(|e| EngineError::Other(e.to_string()))
}

/// The output extensions [`format_for_path`] maps to a writer of that format.
pub const WRITABLE_EXTENSIONS: &[&str] = &["deckcraft", "pptx", "potx", "ppsx", "pdf", "png", "jpg", "jpeg", "gif", "txt", "md"];

/// The format a file named `path` is written in, or `None` when its extension names no format
/// DeckCraft writes (`.odp`, `.svg`, no extension …): [`format_for_path`] would save
/// those as native `.deckcraft` bytes under the wrong name.
pub fn writable_format_for_path(path: &str) -> Option<&'static str> {
    let ext = std::path::Path::new(path).extension()?.to_str()?.to_ascii_lowercase();
    WRITABLE_EXTENSIONS.contains(&ext.as_str()).then(|| format_for_path(path))
}

/// Animated GIF bytes for `doc` with the options in `p` (see [`crate::gif_export::GifOptions::from_params`]).
pub fn gif_bytes(doc: &Presentation, p: &Value) -> Result<Vec<u8>> {
    Ok(crate::gif_export::encode(doc, &crate::gif_export::GifOptions::from_params(p)?)?.bytes)
}

pub fn format_for_path(path: &str) -> &'static str {
    let l = path.to_ascii_lowercase();
    if l.ends_with(".pptx") {
        "pptx"
    } else if l.ends_with(".potx") {
        "potx"
    } else if l.ends_with(".ppsx") {
        "ppsx"
    } else if l.ends_with(".png") {
        "png"
    } else if l.ends_with(".jpg") || l.ends_with(".jpeg") {
        "jpeg"
    } else if l.ends_with(".pdf") {
        "pdf"
    } else if l.ends_with(".gif") {
        "gif"
    } else if l.ends_with(".txt") || l.ends_with(".md") {
        "outline"
    } else {
        "deckcraft"
    }
}

fn mark_saved(s: &mut Session, path: Option<String>) -> Result<()> {
    let st = s.doc_mut()?;
    st.saved_doc = st.doc.clone();
    if path.is_some() {
        st.path = path;
    }
    st.revision += 1;
    let uid = st.uid;
    if let Some(dir) = &s.recovery_dir {
        crate::recovery::discard(dir, uid);
    }
    Ok(())
}

fn write_file(path: &str, bytes: &[u8]) -> Result<()> {
    crate::io::write_file(path, bytes).map_err(EngineError::Other)
}

fn save(s: &mut Session, p: &Value) -> Result<Value> {
    let path = str_param(p, "path").map(String::from).or_else(|| s.active().and_then(|d| d.path.clone()));
    let Some(path) = path else {
        s.ui_requests.push(crate::UiRequest::PickFile { purpose: "saveAs".into() });
        return Ok(json!({"needsPath": true}));
    };
    let format = str_param(p, "format").unwrap_or(format_for_path(&path));
    let bytes = save_bytes(&s.doc()?.doc, format)?;
    write_file(&path, &bytes)?;
    mark_saved(s, Some(path.clone()))?;
    Ok(json!({"path": path, "bytes": bytes.len()}))
}

fn save_as(s: &mut Session, p: &Value) -> Result<Value> {
    if str_param(p, "path").is_none() {
        s.ui_requests.push(crate::UiRequest::PickFile { purpose: "saveAs".into() });
        return Ok(json!({"needsPath": true}));
    }
    save(s, p)
}

fn save_bytes_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let format = str_param(p, "format").unwrap_or("deckcraft");
    let bytes = save_bytes(&s.doc()?.doc, format)?;
    Ok(json!({"data": base64_encode(&bytes), "bytes": bytes.len()}))
}

/// `scale` for rendering `doc`'s slides to an image: within 0.01–16, and small enough that the
/// slide fits the renderer's largest picture ([`deckcraft_render::MAX_SIDE`]). Past that the
/// renderer clips, so a 4× export of a 4032 pt slide came out cropped to its first 16,000 pixels.
pub fn image_scale(doc: &Presentation, scale: f64) -> f64 {
    let scale = if scale.is_finite() { scale.clamp(0.01, 16.0) } else { 1.0 };
    let longest = doc.slide_size.width.max(doc.slide_size.height).max(1.0);
    scale.min(f64::from(deckcraft_render::MAX_SIDE) / longest)
}

/// Render slide `index` (current when absent) to PNG bytes.
pub fn render_png(doc: &Presentation, index: usize, scale: f64, edit: bool) -> (Vec<u8>, u32, u32) {
    render_png_with(doc, index, scale, edit, false)
}

/// [`render_png`]; `transparent` leaves the slide background out (#784).
pub fn render_png_with(doc: &Presentation, index: usize, scale: f64, edit: bool, transparent: bool) -> (Vec<u8>, u32, u32) {
    let scale = image_scale(doc, scale);
    let opts = deckcraft_render::RenderOpts { scale, edit, transparent_background: transparent, ..Default::default() };
    let img = deckcraft_render::render_slide(doc, index, &opts);
    (img.to_png_at(deckcraft_render::dpi_for_scale(scale)), img.width, img.height)
}

fn finite_scale(scale: f64) -> f64 {
    if scale.is_finite() { scale.clamp(0.01, 16.0) } else { 1.0 }
}

fn picture_bytes(img: &deckcraft_render::Image, format: &str, scale: f64) -> Vec<u8> {
    let dpi = deckcraft_render::dpi_for_scale(scale);
    if format == "png" { img.to_png_at(dpi) } else { img.to_jpeg_at(92, Some(dpi)) }
}

fn render(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let i = usize_param(p, "slide").unwrap_or(st.selection.slide);
    if i >= st.doc.slides.len() {
        return Err(bad("file.render", format!("no slide {i}")));
    }
    let (png, w, h) = render_png_with(&st.doc, i, f64_or(p, "scale", 1.0), bool_or(p, "edit", false), bool_or(p, "transparentBackground", false));
    Ok(json!({"png": base64_encode(&png), "width": w, "height": h}))
}

/// The colour the slide is drawn with at one point: what the Eyedropper takes.
fn color_at(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let i = usize_param(p, "slide").unwrap_or(st.selection.slide);
    if i >= st.doc.slides.len() {
        return Err(bad("slide.colorAt", format!("no slide {i}")));
    }
    let coord = |key: &str| {
        p.get(key)
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite())
            .ok_or_else(|| bad("slide.colorAt", format!("`{key}` (a number, in slide units) is required")))
    };
    let (x, y) = (coord("x")?, coord("y")?);
    let scale = f64_or(p, "scale", 2.0).clamp(0.01, 16.0);
    let img = deckcraft_render::render_slide(&st.doc, i, &deckcraft_render::RenderOpts { scale, ..Default::default() });
    let (px, py) = (x * scale, y * scale);
    let inside = px >= 0.0 && py >= 0.0 && px < f64::from(img.width) && py < f64::from(img.height);
    let [r, g, b, a] =
        img.straight_at(px as u32, py as u32).filter(|_| inside).ok_or_else(|| bad("slide.colorAt", "the point is outside the slide"))?;
    Ok(json!({"color": format!("{r:02X}{g:02X}{b:02X}"), "alpha": a, "x": x, "y": y}))
}

fn export(s: &mut Session, p: &Value) -> Result<Value> {
    let path = str_param(p, "path").ok_or_else(|| bad("file.export", "missing `path`"))?.to_string();
    let format = str_param(p, "format").unwrap_or(format_for_path(&path)).to_string();
    let st = s.doc()?;
    let scale = finite_scale(f64_or(p, "scale", 2.0));
    let transparent = bool_or(p, "transparentBackground", false);
    if transparent && format != "png" {
        return Err(bad("file.export", format!("a transparent background is for PNG export, not {format}")));
    }
    match format.as_str() {
        "png" | "jpeg" | "jpg" => {
            let slides: Vec<usize> = match usize_param(p, "slide") {
                Some(i) => vec![i],
                None if bool_or(p, "all", false) => (0..st.doc.slides.len()).collect(),
                None => vec![st.selection.slide],
            };
            // Every slide checked before any file is written, so a bad index leaves nothing behind.
            if let Some(i) = slides.iter().find(|i| **i >= st.doc.slides.len()) {
                return Err(bad("file.export", format!("no slide {i}")));
            }
            let scale = image_scale(&st.doc, scale);
            let mut written = vec![];
            for (k, i) in slides.iter().enumerate() {
                let img = deckcraft_render::render_slide(
                    &st.doc,
                    *i,
                    &deckcraft_render::RenderOpts { scale, transparent_background: transparent, ..Default::default() },
                );
                let bytes = picture_bytes(&img, &format, scale);
                let out = if slides.len() == 1 {
                    path.clone()
                } else {
                    let pth = std::path::Path::new(&path);
                    let stem = pth.file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_else(|| "Slide".into());
                    let ext = pth.extension().map(|x| x.to_string_lossy().to_string()).unwrap_or_else(|| format.clone());
                    pth.with_file_name(format!("{stem}{}.{ext}", k + 1)).to_string_lossy().to_string()
                };
                // Say which slides made it when one can't be written (disk full part-way).
                write_file(&out, &bytes)
                    .map_err(|e| if written.is_empty() { e } else { EngineError::Other(format!("{e} (slides 1–{k} were written)")) })?;
                written.push(out);
            }
            Ok(json!({"files": written}))
        }
        "pdf" => {
            let bytes = pdf_bytes(&st.doc, p)?;
            write_file(&path, &bytes)?;
            Ok(json!({"path": path, "bytes": bytes.len()}))
        }
        "gif" => {
            // `slides` may be a range, as for PDF ("1-3,5").
            let mut gp = p.clone();
            if let Some(text) = p.get("slides").and_then(Value::as_str) {
                gp["slides"] = json!(parse_slide_range(text, st.doc.slides.len())?);
            }
            let out = crate::gif_export::encode(&st.doc, &crate::gif_export::GifOptions::from_params(&gp)?)?;
            write_file(&path, &out.bytes)?;
            Ok(json!({
                "path": path,
                "bytes": out.bytes.len(),
                "frames": out.frames,
                "width": out.width,
                "height": out.height,
                "secondsPerSlide": f64::from(out.delay_cs) / 100.0,
            }))
        }
        "pptx" | "potx" | "ppsx" | "deckcraft" | "outline" | "txt" => {
            let bytes = save_bytes(&st.doc, &format)?;
            write_file(&path, &bytes)?;
            Ok(json!({"path": path, "bytes": bytes.len()}))
        }
        other => Err(bad("file.export", format!("unknown format `{other}`"))),
    }
}

/// File › Info: what the Info page shows about the active presentation.
fn info(s: &mut Session, _p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let doc = &st.doc;
    #[cfg(not(target_arch = "wasm32"))]
    let bytes = st.path.as_deref().and_then(crate::io::file_len);
    #[cfg(target_arch = "wasm32")]
    let bytes: Option<u64> = None;
    Ok(json!({
        "title": st.title(),
        "path": st.path,
        "format": st.path.as_deref().map(format_for_path),
        "dirty": st.is_dirty(),
        "slides": doc.slides.len(),
        "hiddenSlides": doc.slides.iter().filter(|sl| sl.hidden).count(),
        "sections": doc.sections.len(),
        "bytes": bytes,
        "properties": serde_json::to_value(&doc.props).unwrap_or_default(),
    }))
}

fn close(s: &mut Session, _p: &Value) -> Result<Value> {
    if let Some(i) = s.active_index() {
        s.close_document(i);
    }
    ok()
}

fn properties(s: &mut Session, p: &Value) -> Result<Value> {
    let fields = ["title", "subject", "author", "keywords", "comments", "category", "company"];
    if fields.iter().any(|f| p.get(*f).is_some()) {
        s.edit(|doc, _| {
            let pr = &mut doc.props;
            for f in fields {
                if let Some(v) = str_param(p, f) {
                    let slot = match f {
                        "title" => &mut pr.title,
                        "subject" => &mut pr.subject,
                        "author" => &mut pr.author,
                        "keywords" => &mut pr.keywords,
                        "comments" => &mut pr.comments,
                        "category" => &mut pr.category,
                        _ => &mut pr.company,
                    };
                    *slot = v.to_string();
                }
            }
            Ok(())
        })?;
    }
    Ok(serde_json::to_value(&s.doc()?.doc.props).unwrap_or_default())
}

fn revert(s: &mut Session, _p: &Value) -> Result<Value> {
    let st = s.doc_mut()?;
    st.doc = st.saved_doc.clone();
    st.history = crate::History { limit: st.history.limit, ..Default::default() };
    st.revision += 1;
    ok()
}

fn next_window(s: &mut Session, p: &Value) -> Result<Value> {
    let n = s.documents().len();
    if n == 0 {
        return ok();
    }
    let i = usize_param(p, "index").unwrap_or_else(|| (s.active_index().unwrap_or(0) + 1) % n);
    s.set_active(i.min(n - 1));
    ok()
}

fn recovery_dir(s: &Session) -> Result<std::path::PathBuf> {
    s.recovery_dir.clone().ok_or_else(|| EngineError::Other("no recovery folder is set".into()))
}

fn recovery_save(s: &mut Session, _p: &Value) -> Result<Value> {
    let dir = recovery_dir(s)?;
    Ok(json!({"written": crate::recovery::save(s, &dir)?}))
}

fn recovery_list(s: &mut Session, _p: &Value) -> Result<Value> {
    let dir = recovery_dir(s)?;
    Ok(Value::Array(
        crate::recovery::list(&dir)
            .into_iter()
            .map(|(uid, mut m)| {
                if let Some(o) = m.as_object_mut() {
                    o.insert("uid".into(), json!(uid));
                }
                m
            })
            .collect(),
    ))
}

fn recovery_open(s: &mut Session, _p: &Value) -> Result<Value> {
    let dir = recovery_dir(s)?;
    Ok(json!({"opened": crate::recovery::open(s, &dir)?}))
}

fn recovery_discard(s: &mut Session, p: &Value) -> Result<Value> {
    let dir = recovery_dir(s)?;
    match p.get("uid").and_then(Value::as_u64) {
        Some(uid) => crate::recovery::discard(&dir, uid),
        None => {
            for (uid, _) in crate::recovery::list(&dir) {
                crate::recovery::discard(&dir, uid);
            }
        }
    }
    ok()
}

#[cfg(test)]
#[path = "color_at_tests.rs"]
mod color_at_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::write_file;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("deckcraft-write-file-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn write_file_does_not_touch_a_predictable_saving_sibling() {
        let dir = scratch("sibling");
        let dest = dir.join("deck.deckcraft");
        let dest_str = dest.to_str().unwrap();
        std::fs::write(&dest, b"old").unwrap();
        std::fs::write(dir.join("deck.deckcraft.saving"), b"precious").unwrap();
        write_file(dest_str, b"new").unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"new");
        assert_eq!(std::fs::read(dir.join("deck.deckcraft.saving")).unwrap(), b"precious");
        let names: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names.len(), 2, "no temporary file may be left behind: {names:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_file_failed_rename_keeps_destination_and_cleans_up() {
        let dir = scratch("failed");
        let dest = dir.join("target");
        std::fs::create_dir(&dest).unwrap();
        assert!(write_file(dest.to_str().unwrap(), b"new").is_err());
        assert!(dest.is_dir());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
