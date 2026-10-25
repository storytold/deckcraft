//! Insert tab: shapes, text boxes, pictures, tables, charts, audio, video, WordArt, fields.

use deckcraft_color::SchemeSlot;
use deckcraft_geom::Xfrm;
use deckcraft_model::chart::ChartType;
use deckcraft_model::style::PictureFill;
use deckcraft_model::text::{AutoFit, RunKind, TextBody};
use deckcraft_model::{Chart, ColorRef, Fill, Geom, Line, MediaClip, Shape, ShapeId, ShapeKind, ShapeStyle, Table};
use serde_json::{Value, json};

use super::{
    CommandSpec, NO_ID_SPACE, always, bad, base64_decode, base64_encode, bool_or, bool_param, cmd, color_value, edit_shapes, has_doc, has_selection,
    has_slide, has_text_or_shapes, ok, rect_param, str_param, usize_param, xfrm_of,
};
use crate::{EngineError, Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "shape.insert",
            "Shapes",
            ["Insert", "Illustrations"],
            None,
            "{preset: rect|roundRect|ellipse|triangle|rightArrow|star5|… (see shape.presets), rect: [x,y,w,h] pt, text?: string}",
            has_slide,
            insert_shape
        ),
        cmd!(
            "insert.icon",
            "Icons",
            ["Insert", "Illustrations"],
            None,
            "{name: star|arrow-right|… (see insert.icons), rect?: [x,y,w,h]}",
            has_slide,
            insert_icon
        ),
        cmd!(query "insert.icons", "Icon Gallery", [], None, "{} → [{name, label, category}]", always, list_icons),
        cmd!(query "shape.presets", "Shape Gallery", [], None, "{} → [{name, label, category}]", always, presets),
        cmd!("insert.textBox", "Text Box", ["Insert", "Text"], None, "{rect?: [x,y,w,h], text?: string}", has_slide, text_box),
        cmd!(
            "insert.picture",
            "Picture from File…",
            ["Insert", "Images", "Pictures"],
            None,
            "{path? | data: base64, name?, rect?: [x,y,w,h]}",
            has_slide,
            picture
        ),
        cmd!(
            "insert.photoAlbum",
            "Photo Album…",
            ["Insert", "Images"],
            None,
            "{pictures?: [{path?|data: base64, name?, rot?: deg, brightness?: -1..1, contrast?: -1..1} | {text}], layout?: fit|1|2|4|1title|2title|4title, frame?: none|rect|rounded|simple, captions?: bool, grayscale?: bool, theme?: name} — omit pictures to open the dialog",
            has_doc,
            photo_album
        ),
        cmd!(
            "insert.photoAlbumEdit",
            "Edit Photo Album…",
            ["Insert", "Images"],
            None,
            "{album?: id, pictures?: same as insert.photoAlbum, layout?, frame?, captions?, grayscale?, theme?} — omit pictures to reopen the dialog on the current album",
            has_doc,
            photo_album_edit
        ),
        cmd!("insert.table", "Table…", ["Insert", "Tables"], None, "{rows, cols, rect?}", has_slide, table),
        cmd!(
            "insert.chart",
            "Chart…",
            ["Insert", "Illustrations"],
            None,
            "{type?: column|bar|line|pie|doughnut|area|scatter|stackedColumn|…, rect?, categories?: [..], series?: [{name, values}]}",
            has_slide,
            chart
        ),
        cmd!("insert.audio", "Audio from File…", ["Insert", "Media", "Audio"], None, "{path? | data: base64, name?, rect?}", has_slide, audio),
        cmd!("insert.video", "Video from File…", ["Insert", "Media", "Video"], None, "{path? | data: base64, name?, rect?}", has_slide, video),
        cmd!("insert.wordArt", "WordArt", ["Insert", "Text"], None, "{text?, style?: 0..}", has_slide, word_art),
        cmd!("insert.slideNumber", "Slide Number", ["Insert", "Text"], None, "{}", has_slide, slide_number),
        cmd!(
            "insert.dateTime",
            "Date & Time",
            ["Insert", "Text"],
            None,
            "{format?: datetime1..datetime13, update?: bool, language?: en-US|en-GB|fr-FR|ja-JP|sv-SE} — no keys opens the dialog",
            has_slide,
            date_time
        ),
        cmd!("insert.symbol", "Symbol", ["Insert", "Symbols"], None, "{text: character(s)}", has_slide, symbol),
        cmd!(
            "shape.action",
            "Action Settings",
            ["Insert", "Links"],
            None,
            "{on?: click|hover, action: none|next|previous|first|last|lastViewed|endShow|playMedia|slide|url|customShow|program, slide?: index, url?, show?: custom show name, path?, highlight?: bool, tooltip?, ids?}",
            has_selection,
            shape_action
        ),
        cmd!(
            "insert.hyperlink",
            "Link",
            ["Insert", "Links"],
            Some("Cmd+K"),
            "{url? | slide?: index, tooltip?, text?: shown when nothing is selected, highlightClick?, ids?}",
            has_text_or_shapes,
            hyperlink
        ),
        cmd!("chart.type", "Change Chart Type", ["Chart Design", "Type"], None, "{type, id?}", has_selection, chart_type_cmd),
        cmd!(
            "chart.data",
            "Edit Data",
            ["Chart Design", "Data"],
            None,
            "{categories?: [..], levels?: [[..]] (outer category levels, innermost first), series?: [{name, values}], id?}",
            has_selection,
            chart_data
        ),
        cmd!(
            "chart.options",
            "Chart Elements",
            ["Chart Design", "Chart Layouts"],
            None,
            "{title?: string|null, legend?: b|t|l|r|null, dataLabels?: bool, gridlines?: bool, palette?, binCount?: n|null, binSize?: n|null, subtotals?: [point index], quartiles?: exclusive|inclusive, upDownBars?: bool, textColor?: #RRGGBB|accent1..6|tx1…|null (title, axes, legend and labels; null for PowerPoint's default: the theme's text colour at 65 % plus 35 %, 595959 on a light theme), id?}",
            has_selection,
            chart_options
        ),
        cmd!(
            "insert.smartArt",
            "SmartArt",
            ["Insert", "Illustrations"],
            None,
            "{kind: list|process|cycle|hierarchy|pyramid|matrix, items?: [text] (leading tabs = sub-points), nodes?: [{text, level}]} → {id}",
            has_slide,
            smart_art
        ),
        cmd!(
            "insert.actionButton",
            "Action Buttons",
            ["Insert", "Illustrations"],
            None,
            "{kind: back|forward|beginning|end|home|information|return|movie|document|sound|help|custom, rect?}",
            has_slide,
            action_button
        ),
    ]
}

/// Default size for a click-inserted shape (1 inch square, PowerPoint style).
fn default_rect(s: &Session, w: f64, h: f64) -> Xfrm {
    let size = s.active().map(|d| d.doc.slide_size).unwrap_or(deckcraft_model::defaults::WIDE);
    Xfrm::new((size.width - w) / 2.0, (size.height - h) / 2.0, w, h)
}

/// The word an automatic shape name starts with ("Rectangle" in "Rectangle 3").
pub(crate) fn auto_name_base(shape: &Shape) -> String {
    match &shape.kind {
        ShapeKind::Picture { .. } => "Picture".to_string(),
        ShapeKind::Table(_) => "Table".to_string(),
        ShapeKind::Chart(_) => "Chart".to_string(),
        ShapeKind::Media(m) if m.video => "Video".to_string(),
        ShapeKind::Media(_) => "Audio".to_string(),
        ShapeKind::Model3d { .. } => "3D Model".to_string(),
        _ if shape.text_box => "TextBox".to_string(),
        _ => shape
            .geom
            .preset_name()
            .and_then(deckcraft_geom::preset::info)
            .map(|i| i.label.split(':').next().unwrap_or(i.label).trim().to_string())
            .unwrap_or_else(|| "Shape".into()),
    }
}

/// "{base} n" with the first `n` from the shape count + 1 that no shape on the slide already uses,
/// so a gap left by a deleted shape never makes a second shape with the same name.
pub(crate) fn first_free_name(base: &str, taken: &std::collections::HashSet<String>) -> String {
    (taken.len() + 1..).map(|n| format!("{base} {n}")).find(|c| !taken.contains(c)).unwrap_or_default()
}

pub(crate) fn add_shape(s: &mut Session, cmd: &str, mut shape: Shape, select: bool) -> Result<ShapeId> {
    s.edit(|doc, sel| {
        shape.id = ShapeId(doc.alloc_id().ok_or_else(|| bad(cmd, NO_ID_SPACE))?);
        if shape.name.is_empty() {
            let base = auto_name_base(&shape);
            let taken = super::current_names(doc, sel);
            shape.name = first_free_name(&base, &taken);
        }
        let id = shape.id;
        let list = crate::shapes_mut(doc, sel).ok_or_else(|| bad("insert", "no slide"))?;
        list.push(shape);
        if select {
            sel.shapes = vec![id];
            sel.text = None;
        }
        Ok(id)
    })
}

fn insert_shape(s: &mut Session, p: &Value) -> Result<Value> {
    let preset = str_param(p, "preset").unwrap_or("rect");
    let info = deckcraft_geom::preset::info(preset).ok_or_else(|| bad("shape.insert", format!("unknown preset `{preset}`")))?;
    let line_like = deckcraft_geom::preset::is_line_like(preset);
    let rect = rect_param(p, "rect").unwrap_or_else(|| default_rect(s, 72.0, if line_like { 0.0 } else { 72.0 }));
    let adj: Vec<f64> = p.get("adj").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
    let mut shape = Shape {
        xfrm: Some(rect),
        geom: Geom::Preset { name: info.name.into(), adj },
        style: Some(if line_like { ShapeStyle::line(SchemeSlot::Accent1) } else { ShapeStyle::accent(SchemeSlot::Accent1) }),
        ..Default::default()
    };
    if let Some(fl) = p.get("flip").and_then(Value::as_str)
        && let Some(x) = shape.xfrm.as_mut()
    {
        x.flip_h = fl.contains('h');
        x.flip_v = fl.contains('v');
    }
    if !line_like {
        shape.text = Some(TextBody { paragraphs: vec![], ..Default::default() });
        if let Some(t) = str_param(p, "text") {
            shape.text = Some(TextBody::from_text(t));
        }
        if let Some(tb) = shape.text.as_mut() {
            for para in &mut tb.paragraphs {
                para.props.align = Some(deckcraft_model::text::Align::Center);
            }
        }
    } else {
        shape.kind = if preset.contains("Connector") { ShapeKind::Connector { start: None, end: None } } else { ShapeKind::Shape };
        if preset == "straightConnector1" {
            shape.line = Some(deckcraft_model::Line {
                tail: Some(deckcraft_model::style::LineEnd { kind: "triangle".into(), w: "med".into(), len: "med".into() }),
                ..Default::default()
            });
        }
    }
    if let (Some(look), false) = (&s.default_look, line_like) {
        super::format::apply_painted(&mut shape, look);
    }
    let id = add_shape(s, "shape.insert", shape, true)?;
    Ok(json!({"id": id}))
}

fn presets(_s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(Value::Array(
        deckcraft_geom::preset::CATALOG.iter().map(|i| json!({"name": i.name, "label": i.label, "category": i.category.label()})).collect(),
    ))
}

fn list_icons(_s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(Value::Array(crate::icons::icons().iter().map(|i| json!({"name": i.name, "label": i.label, "category": i.category})).collect()))
}

/// A Lucide stroke icon: custom geometry, no fill, round line scaled from the 24-unit design.
fn insert_icon(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("insert.icon", "missing `name`"))?;
    let icon = crate::icons::find(name).ok_or_else(|| bad("insert.icon", format!("unknown icon `{name}`")))?;
    let d = crate::icons::path_data(icon.name).ok_or_else(|| bad("insert.icon", format!("could not build icon `{}`", icon.name)))?;
    let rect = rect_param(p, "rect").unwrap_or_else(|| default_rect(s, 72.0, 72.0));
    if rect.w < 1.0 || rect.h < 1.0 {
        return Err(bad("insert.icon", "rect needs a positive width and height"));
    }
    let shape = Shape {
        name: icon.label.to_string(),
        xfrm: Some(rect),
        geom: Geom::Custom {
            paths: vec![deckcraft_model::CustomPath {
                w: crate::icons::VIEW,
                h: crate::icons::VIEW,
                d,
                fill: deckcraft_geom::preset::FillMode::None,
                stroke: true,
            }],
        },
        fill: Some(Fill::None),
        line: Some(Line {
            fill: Some(Fill::solid(ColorRef::scheme(SchemeSlot::Tx1))),
            width: Some(crate::icons::stroke_width(icon, rect.w, rect.h)),
            cap: Some(deckcraft_model::style::LineCap::Round),
            join: Some(deckcraft_model::style::LineJoin::Round),
            ..Default::default()
        }),
        style: Some(ShapeStyle::line(SchemeSlot::Tx1)),
        ..Default::default()
    };
    let id = add_shape(s, "insert.icon", shape, true)?;
    Ok(json!({"id": id}))
}

pub(crate) fn new_text_box(rect: Xfrm, text: &str) -> Shape {
    let mut body = TextBody::from_text(text);
    body.body.wrap = Some(true);
    body.body.autofit = Some(AutoFit::Shape);
    Shape { xfrm: Some(rect), text_box: true, fill: None, text: Some(body), geom: Geom::preset("rect"), ..Default::default() }
}

fn text_box(s: &mut Session, p: &Value) -> Result<Value> {
    let text = str_param(p, "text").unwrap_or("");
    let rect = rect_param(p, "rect").unwrap_or_else(|| default_rect(s, 216.0, 36.0));
    let mut shape = new_text_box(rect, text);
    // Click-to-create (no width) text boxes grow sideways without wrapping.
    if rect.w < 1.0 {
        shape.xfrm = Some(Xfrm { w: 100.0, ..rect });
        if let Some(t) = shape.text.as_mut() {
            t.body.wrap = Some(false);
        }
    }
    let id = add_shape(s, "insert.textBox", shape, true)?;
    super::text::fit_text_box(s, id)?;
    if text.is_empty() {
        // Start typing immediately.
        s.select(|_, sel| {
            sel.shapes = vec![id];
            sel.text = Some(crate::TextSel { shape: id, ..Default::default() });
        })?;
    }
    Ok(json!({"id": id}))
}

/// Read media bytes from `data` (base64) or `path`.
pub(crate) fn media_bytes(p: &Value, cmd: &str) -> Result<(String, Vec<u8>)> {
    if let Some(d) = str_param(p, "data") {
        let bytes = base64_decode(d).ok_or_else(|| bad(cmd, "invalid base64 `data`"))?;
        return Ok((str_param(p, "name").unwrap_or("media").to_string(), bytes));
    }
    if let Some(path) = str_param(p, "path") {
        let bytes = crate::io::read_file(path).map_err(EngineError::Other)?;
        let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "media".into());
        return Ok((name, bytes));
    }
    Err(bad(cmd, "missing `path` or `data`"))
}

pub fn content_type(name: &str, bytes: &[u8]) -> &'static str {
    let l = name.to_ascii_lowercase();
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF8") {
        "image/gif"
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        "image/webp"
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE") {
        "audio/wav"
    } else if bytes.starts_with(b"BM") {
        "image/bmp"
    } else if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
        "image/tiff"
    } else if bytes.starts_with(b"fLaC") {
        "audio/flac"
    } else if bytes.starts_with(b"OggS") {
        if l.ends_with(".ogv") { "video/ogg" } else { "audio/ogg" }
    } else if bytes.len() > 1 && bytes[0] == 0xFF && bytes[1] & 0xF6 == 0xF0 {
        // ADTS (layer bits 00): AAC.
        "audio/aac"
    } else if bytes.starts_with(b"ID3") || (bytes.len() > 1 && bytes[0] == 0xFF && (bytes[1] & 0xE0) == 0xE0 && !l.ends_with(".aac")) {
        "audio/mpeg"
    } else if bytes.starts_with(b"FORM") {
        "audio/aiff"
    } else if bytes.get(4..8) == Some(b"ftyp") {
        if l.ends_with(".m4a") || l.ends_with(".m4b") || bytes.get(8..11) == Some(b"M4A") {
            "audio/mp4"
        } else if l.ends_with(".mov") || bytes.get(8..12) == Some(b"qt  ") {
            "video/quicktime"
        } else {
            "video/mp4"
        }
    } else if bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        if l.ends_with(".mka") || l.ends_with(".weba") {
            "audio/webm"
        } else if l.ends_with(".mkv") {
            "video/x-matroska"
        } else {
            "video/webm"
        }
    } else if bytes.starts_with(&[0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11]) {
        if l.ends_with(".wmv") || l.ends_with(".asf") { "video/x-ms-wmv" } else { "audio/x-ms-wma" }
    } else if bytes.starts_with(b"caff") {
        "audio/x-caf"
    } else if l.ends_with(".svg") || bytes.windows(4).take(512).any(|w| w == b"<svg") {
        "image/svg+xml"
    } else if l.ends_with(".aac") {
        "audio/aac"
    } else {
        "application/octet-stream"
    }
}

/// Natural size of an image in points (96 dpi), fitted inside the slide.
fn fitted(s: &Session, bytes: &[u8]) -> (f64, f64) {
    let (w, h) = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()
        .and_then(|r| r.into_dimensions().ok())
        .map(|(w, h)| (w as f64 * 0.75, h as f64 * 0.75))
        .unwrap_or((288.0, 216.0));
    let size = s.active().map(|d| d.doc.slide_size).unwrap_or(deckcraft_model::defaults::WIDE);
    let k = (size.width * 0.9 / w.max(1.0)).min(size.height * 0.9 / h.max(1.0)).min(1.0);
    (w * k, h * k)
}

fn picture(s: &mut Session, p: &Value) -> Result<Value> {
    let (name, bytes) = media_bytes(p, "insert.picture")?;
    let ct = content_type(&name, &bytes);
    if !ct.starts_with("image/") {
        return Err(bad("insert.picture", format!("{name} is not a picture")));
    }
    if ct != "image/svg+xml" && deckcraft_render::decode_image(&bytes).is_none() {
        return Err(EngineError::Other(format!("{name}: the picture can't be read")));
    }
    let (w, h) = fitted(s, &bytes);
    // Into a selected empty picture/content placeholder, if there is one.
    let target_ph = s.active().and_then(|d| {
        d.selected_shapes()
            .into_iter()
            .find(|x| {
                x.ph_type()
                    .is_some_and(|k| matches!(k, deckcraft_model::PhType::Picture | deckcraft_model::PhType::Obj | deckcraft_model::PhType::Body))
                    && x.text.as_ref().is_none_or(|t| t.is_empty())
            })
            .map(|x| (x.id, xfrm_of(&d.doc, &d.selection, x)))
    });
    let rect = rect_param(p, "rect").unwrap_or_else(|| match target_ph {
        Some((_, b)) => {
            // Fill the placeholder, cropping the picture to its aspect.
            b
        }
        None => default_rect(s, w, h),
    });
    let crop = match (target_ph, rect_param(p, "rect")) {
        (Some(_), None) => {
            let (ia, ba) = (w / h.max(1e-9), rect.w / rect.h.max(1e-9));
            if ia > ba {
                let c = (1.0 - ba / ia) / 2.0;
                [c, 0.0, c, 0.0]
            } else {
                let c = (1.0 - ia / ba) / 2.0;
                [0.0, c, 0.0, c]
            }
        }
        _ => [0.0; 4],
    };
    let media = s.edit(|doc, _| doc.add_media(&name, ct, bytes).ok_or_else(|| bad("insert.picture", NO_ID_SPACE)))?;
    let shape = Shape {
        xfrm: Some(rect),
        kind: ShapeKind::Picture { fill: PictureFill { media, crop, ..Default::default() } },
        descr: name.clone(),
        ..Default::default()
    };
    if let Some((ph_id, _)) = target_ph {
        // Replace the placeholder with the picture (it keeps the placeholder link).
        let phv = s.doc()?.shape(ph_id).and_then(|x| x.ph.clone());
        s.edit(|doc, sel| {
            let list = crate::shapes_mut(doc, sel).ok_or_else(|| bad("insert.picture", "no slide"))?;
            if let Some(sh) = list.iter_mut().find(|x| x.id == ph_id) {
                sh.kind = shape.kind.clone();
                sh.xfrm = Some(rect);
                sh.ph = phv.map(|mut p| {
                    p.kind = deckcraft_model::PhType::Picture;
                    p
                });
                sh.text = None;
                sh.descr = name.clone();
            }
            sel.shapes = vec![ph_id];
            Ok(())
        })?;
        return Ok(json!({"id": ph_id}));
    }
    let id = add_shape(s, "insert.picture", shape, true)?;
    Ok(json!({"id": id}))
}

const ALBUM_MAX_ITEMS: usize = 48;
const ALBUM_MAX_BYTES: usize = 32 * 1024 * 1024;
const ALBUM_MAX_TOTAL: usize = 64 * 1024 * 1024;
const ALBUM_MAX_TEXT: usize = 400;

#[derive(Clone, Copy)]
struct AlbumLayout {
    cols: usize,
    rows: usize,
    per: usize,
    title: bool,
    fit: bool,
}

#[derive(Clone, Copy)]
enum AlbumFrame {
    None,
    Rect,
    Rounded,
    Simple,
}

struct AlbumPic {
    name: String,
    bytes: Vec<u8>,
    px_w: f64,
    px_h: f64,
    rot: f64,
    brightness: f64,
    contrast: f64,
}

struct AlbumOpts {
    layout_key: String,
    frame_key: String,
    layout: AlbumLayout,
    frame: AlbumFrame,
    captions: bool,
    grayscale: bool,
    theme: Option<String>,
}

enum AlbumItem {
    Picture(AlbumPic),
    Text(String),
}

enum AlbumSlide {
    Pictures(Vec<AlbumPic>),
    Text(String),
}

/// Insert a photo album after the current slide.
///
/// PowerPoint starts a new presentation. These slides go into the open one, so the album is one
/// undo step. Omitting `pictures` asks the UI to open the dialog and does not edit.
fn photo_album(s: &mut Session, p: &Value) -> Result<Value> {
    if p.get("pictures").is_none_or(Value::is_null) {
        s.ui_requests.push(crate::UiRequest::Dialog { id: "photoAlbum".into(), params: json!({}) });
        return Ok(json!({"dialog": "photoAlbum"}));
    }
    let opts = album_opts(p)?;
    let items = album_items(p)?;
    commit_album(s, items, opts, None)
}

fn photo_album_edit(s: &mut Session, p: &Value) -> Result<Value> {
    if p.get("pictures").is_none_or(Value::is_null) {
        let seed = album_seed(s)?;
        let album = seed.get("album").cloned().unwrap_or(Value::Null);
        s.ui_requests.push(crate::UiRequest::Dialog { id: "photoAlbum".into(), params: seed });
        return Ok(json!({"dialog": "photoAlbum", "album": album}));
    }
    let opts = album_opts(p)?;
    let items = album_items(p)?;
    let (id, ids) = album_target(s, p)?;
    commit_album(s, items, opts, Some((id, ids)))
}

fn commit_album(s: &mut Session, items: Vec<AlbumItem>, opts: AlbumOpts, replace: Option<(u32, Vec<u64>)>) -> Result<Value> {
    let (snapshot, sel) = {
        let st = s.doc()?;
        (st.doc.clone(), st.selection.clone())
    };
    let result = (|| {
        let album_id = match replace.as_ref().map(|(id, _)| *id) {
            Some(id) if id != 0 => id,
            _ => {
                let cmd = if replace.is_some() { "insert.photoAlbumEdit" } else { "insert.photoAlbum" };
                s.edit(|doc, _| doc.alloc_id().ok_or_else(|| bad(cmd, NO_ID_SPACE)))?
            }
        };
        let at = if let Some((_, ids)) = &replace {
            let st = s.doc()?;
            let indices: Vec<usize> = ids.iter().filter_map(|id| st.doc.slide_index(deckcraft_model::SlideId(*id as u32))).collect();
            let Some(at) = indices.iter().copied().min() else {
                return Err(bad("insert.photoAlbumEdit", "that photo album is no longer in this presentation"));
            };
            s.execute("slide.delete", &json!({"ids": ids}))?;
            Some(at)
        } else {
            None
        };
        let mut opts = opts;
        if opts.theme.as_deref().unwrap_or("").is_empty() {
            opts.theme = s.doc().ok().and_then(|st| st.doc.masters.first().map(|m| m.theme.name.clone())).filter(|n| !n.is_empty());
        }
        apply_theme_if_needed(s, opts.theme.as_deref())?;
        insert_album(s, items, &opts, at, album_id)
    })();
    if result.is_err()
        && let Some(st) = s.active_mut()
    {
        st.doc = snapshot;
        st.selection = sel;
    }
    result
}

fn album_norm(s: &str) -> String {
    s.chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_ascii_lowercase()
}

fn album_layout(p: &Value) -> Result<AlbumLayout> {
    let key = match p.get("layout") {
        None => return Ok(AlbumLayout { cols: 1, rows: 1, per: 1, title: false, fit: false }),
        Some(Value::String(s)) => s.as_str(),
        Some(_) => return Err(bad("insert.photoAlbum", "`layout` must be text")),
    };
    match album_norm(key).as_str() {
        "fit" => Ok(AlbumLayout { cols: 1, rows: 1, per: 1, title: false, fit: true }),
        "1" | "one" => Ok(AlbumLayout { cols: 1, rows: 1, per: 1, title: false, fit: false }),
        "1title" | "onetitle" => Ok(AlbumLayout { cols: 1, rows: 1, per: 1, title: true, fit: false }),
        "2" | "two" => Ok(AlbumLayout { cols: 2, rows: 1, per: 2, title: false, fit: false }),
        "2title" | "twotitle" => Ok(AlbumLayout { cols: 2, rows: 1, per: 2, title: true, fit: false }),
        "4" | "four" => Ok(AlbumLayout { cols: 2, rows: 2, per: 4, title: false, fit: false }),
        "4title" | "fourtitle" => Ok(AlbumLayout { cols: 2, rows: 2, per: 4, title: true, fit: false }),
        _ => Err(bad("insert.photoAlbum", format!("unknown layout `{key}` (use fit, 1, 2, 4, 1title, 2title or 4title)"))),
    }
}

fn album_frame(p: &Value) -> Result<AlbumFrame> {
    let key = match p.get("frame") {
        None => return Ok(AlbumFrame::None),
        Some(Value::String(s)) => s.as_str(),
        Some(_) => return Err(bad("insert.photoAlbum", "`frame` must be text")),
    };
    match album_norm(key).as_str() {
        "none" => Ok(AlbumFrame::None),
        "rect" | "rectangle" => Ok(AlbumFrame::Rect),
        "rounded" | "round" | "roundedrect" | "roundedrectangle" => Ok(AlbumFrame::Rounded),
        "simple" | "simpleframe" | "simplewhite" | "white" => Ok(AlbumFrame::Simple),
        _ => Err(bad("insert.photoAlbum", format!("unknown frame `{key}` (use none, rect, rounded or simple)"))),
    }
}

fn album_captions(p: &Value) -> Result<bool> {
    match p.get("captions") {
        None => Ok(false),
        Some(Value::Bool(v)) => Ok(*v),
        Some(_) => Err(bad("insert.photoAlbum", "`captions` must be true or false")),
    }
}

fn album_bool(p: &Value, key: &str) -> Result<bool> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(v)) => Ok(*v),
        Some(_) => Err(bad("insert.photoAlbum", format!("`{key}` must be true or false"))),
    }
}

fn layout_key_of(layout: AlbumLayout) -> String {
    if layout.fit {
        return "fit".into();
    }
    let n = match layout.per {
        2 => "2",
        4 => "4",
        _ => "1",
    };
    if layout.title { format!("{n}title") } else { n.into() }
}

fn frame_key_of(frame: AlbumFrame) -> &'static str {
    match frame {
        AlbumFrame::None => "none",
        AlbumFrame::Rect => "rect",
        AlbumFrame::Rounded => "rounded",
        AlbumFrame::Simple => "simple",
    }
}

fn album_opts(p: &Value) -> Result<AlbumOpts> {
    let layout = album_layout(p)?;
    let frame = album_frame(p)?;
    let theme = match p.get("theme") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if s.trim().is_empty() => None,
        Some(Value::String(s)) => {
            let name = s.trim();
            let found = deckcraft_model::theme::builtin_themes().into_iter().find(|t| t.name.eq_ignore_ascii_case(name));
            let Some(t) = found else {
                return Err(bad("insert.photoAlbum", format!("unknown theme `{name}`")));
            };
            Some(t.name)
        }
        Some(_) => return Err(bad("insert.photoAlbum", "`theme` must be a theme name")),
    };
    Ok(AlbumOpts {
        layout_key: layout_key_of(layout),
        frame_key: frame_key_of(frame).into(),
        layout,
        frame,
        captions: album_captions(p)?,
        grayscale: album_bool(p, "grayscale")?,
        theme,
    })
}

/// A finite number, or 0 when the key is absent. A present non-number is an error.
fn album_finite(item: &Value, key: &str) -> Result<f64> {
    match item.get(key) {
        None | Some(Value::Null) => Ok(0.0),
        Some(v) => v.as_f64().filter(|n| n.is_finite()).ok_or_else(|| bad("insert.photoAlbum", format!("`{key}` must be a number"))),
    }
}

fn album_rot(item: &Value) -> Result<f64> {
    let n = album_finite(item, "rot")?;
    Ok(((n % 360.0) + 360.0) % 360.0)
}

fn album_unit(item: &Value, key: &str) -> Result<f64> {
    let n = album_finite(item, key)?;
    if !(-1.0..=1.0).contains(&n) {
        return Err(bad("insert.photoAlbum", format!("`{key}` must be between -1 and 1")));
    }
    Ok(n)
}

fn album_items(p: &Value) -> Result<Vec<AlbumItem>> {
    let Some(arr) = p.get("pictures").and_then(Value::as_array) else {
        return Err(bad("insert.photoAlbum", "`pictures` must be a list"));
    };
    if arr.is_empty() {
        return Err(bad("insert.photoAlbum", "add at least one picture or text box"));
    }
    if arr.len() > ALBUM_MAX_ITEMS {
        return Err(bad("insert.photoAlbum", "a photo album holds at most 48 pictures or text boxes"));
    }
    let mut items = Vec::with_capacity(arr.len());
    let mut total = 0usize;
    for item in arr {
        let Some(obj) = item.as_object() else {
            return Err(bad("insert.photoAlbum", "each album item must be a picture or a text box"));
        };
        let has_media = obj.contains_key("data") || obj.contains_key("path");
        if !has_media {
            let Some(text) = obj.get("text").and_then(Value::as_str) else {
                return Err(bad("insert.photoAlbum", "each album item needs `path`, `data` or `text`"));
            };
            let text = text.trim();
            if text.is_empty() {
                return Err(bad("insert.photoAlbum", "a text box needs some text"));
            }
            if text.chars().count() > ALBUM_MAX_TEXT {
                return Err(bad("insert.photoAlbum", "text is longer than 400 characters"));
            }
            items.push(AlbumItem::Text(text.to_string()));
            continue;
        }
        let (name, bytes) = media_bytes(item, "insert.photoAlbum")?;
        if bytes.len() > ALBUM_MAX_BYTES {
            return Err(bad("insert.photoAlbum", format!("{name} is larger than 32 MB")));
        }
        let next = total.saturating_add(bytes.len());
        if next > ALBUM_MAX_TOTAL {
            return Err(bad("insert.photoAlbum", "the album is larger than 64 MB"));
        }
        total = next;
        let ct = content_type(&name, &bytes);
        if !ct.starts_with("image/") {
            return Err(bad("insert.photoAlbum", format!("{name} is not a picture")));
        }
        if ct != "image/svg+xml" && deckcraft_render::decode_image(&bytes).is_none() {
            return Err(bad("insert.photoAlbum", format!("{name}: the picture can't be read")));
        }
        let (px_w, px_h) = image_px(&bytes);
        items.push(AlbumItem::Picture(AlbumPic {
            name,
            bytes,
            px_w,
            px_h,
            rot: album_rot(item)?,
            brightness: album_unit(item, "brightness")?,
            contrast: album_unit(item, "contrast")?,
        }));
    }
    Ok(items)
}

fn image_px(bytes: &[u8]) -> (f64, f64) {
    image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()
        .and_then(|r| r.into_dimensions().ok())
        .map(|(w, h)| (w as f64, h as f64))
        .filter(|(w, h)| *w > 0.0 && *h > 0.0)
        .unwrap_or((4.0, 3.0))
}

fn file_stem(name: &str) -> String {
    let base = std::path::Path::new(name).file_name().and_then(std::ffi::OsStr::to_str).unwrap_or(name);
    let stem = match base.rsplit_once('.') {
        Some((s, ext)) if !s.is_empty() && !ext.is_empty() => s,
        _ => base,
    };
    if stem.is_empty() { "Picture".to_string() } else { stem.to_string() }
}

fn album_plan(items: Vec<AlbumItem>, per: usize) -> Vec<AlbumSlide> {
    let per = per.max(1);
    let mut out = Vec::new();
    let mut batch = Vec::new();
    for item in items {
        match item {
            AlbumItem::Text(text) => {
                if !batch.is_empty() {
                    out.push(AlbumSlide::Pictures(std::mem::take(&mut batch)));
                }
                out.push(AlbumSlide::Text(text));
            }
            AlbumItem::Picture(pic) => {
                batch.push(pic);
                if batch.len() >= per {
                    out.push(AlbumSlide::Pictures(std::mem::take(&mut batch)));
                }
            }
        }
    }
    if !batch.is_empty() {
        out.push(AlbumSlide::Pictures(batch));
    }
    out
}

struct Placed {
    pic: [f64; 4],
    caption: Option<[f64; 4]>,
}

fn fit_in(box_w: f64, box_h: f64, px_w: f64, px_h: f64) -> (f64, f64) {
    let aspect = if px_h > 0.0 { px_w / px_h } else { 4.0 / 3.0 };
    let box_w = box_w.max(1.0);
    let box_h = box_h.max(1.0);
    if !aspect.is_finite() || aspect <= 0.0 {
        return (box_w, box_h);
    }
    if aspect >= box_w / box_h { (box_w, (box_w / aspect).max(1.0)) } else { ((box_h * aspect).max(1.0), box_h) }
}

fn place_album(width: f64, height: f64, layout: AlbumLayout, n: usize, captions: bool) -> (Option<[f64; 4]>, Vec<Placed>) {
    let margin = if layout.fit { 18.0 } else { 36.0 };
    let title_band = if layout.title { 48.0 } else { 0.0 };
    let gap = 12.0;
    let cap_h = 22.0;
    let cap_gap = 4.0;
    let left = margin;
    let top = margin + title_band;
    let grid_w = (width - margin - left).max(1.0);
    let grid_h = (height - margin - top).max(1.0);
    let cols = layout.cols.max(1);
    let rows = layout.rows.max(1);
    let cell_w = ((grid_w - gap * (cols.saturating_sub(1) as f64)) / cols as f64).max(1.0);
    let cell_h = ((grid_h - gap * (rows.saturating_sub(1) as f64)) / rows as f64).max(1.0);
    let pic_h = if captions { (cell_h - cap_h - cap_gap).max(1.0) } else { cell_h };
    let mut placed = Vec::with_capacity(n);
    for i in 0..n {
        let col = i % cols;
        let row = i / cols;
        if row >= rows {
            break;
        }
        let cell_x = left + col as f64 * (cell_w + gap);
        let cell_y = top + row as f64 * (cell_h + gap);
        placed.push(Placed { pic: [cell_x, cell_y, cell_w, pic_h], caption: None });
    }
    let title = if layout.title { Some([margin, 8.0, (width - 2.0 * margin).max(1.0), 32.0]) } else { None };
    (title, placed)
}

fn place_pictures(width: f64, height: f64, layout: AlbumLayout, pics: &[AlbumPic], captions: bool) -> (Option<[f64; 4]>, Vec<Placed>) {
    let (title, slots) = place_album(width, height, layout, pics.len(), captions);
    let placed = pics
        .iter()
        .zip(slots)
        .map(|(pic, slot)| {
            let (dw, dh) = fit_in(slot.pic[2], slot.pic[3], pic.px_w, pic.px_h);
            let x = slot.pic[0] + (slot.pic[2] - dw) / 2.0;
            let y = slot.pic[1] + (slot.pic[3] - dh) / 2.0;
            let caption = if captions { Some([x, y + dh + 4.0, dw.max(36.0), 22.0]) } else { None };
            Placed { pic: [x, y, dw, dh], caption }
        })
        .collect();
    (title, placed)
}

fn apply_frame(s: &mut Session, frame: AlbumFrame, id: u64) -> Result<()> {
    let params = match frame {
        AlbumFrame::None => return Ok(()),
        AlbumFrame::Rect => json!({"id": id, "color": "#1A1A1A", "width": 1.5}),
        AlbumFrame::Rounded => json!({"id": id, "color": "#1A1A1A", "width": 2.25, "join": "round"}),
        AlbumFrame::Simple => json!({"id": id, "color": "#FFFFFF", "width": 6.0}),
    };
    s.execute("shape.line", &params)?;
    Ok(())
}

struct AlbumMark {
    id: u32,
    layout: String,
    frame: String,
    captions: bool,
    grayscale: bool,
    theme: String,
}

fn parse_album_mark(raw: &str) -> Option<AlbumMark> {
    let rest = raw.strip_prefix("deckcraft-album:")?;
    let (id_str, rest) = rest.split_once(';').unwrap_or((rest, ""));
    let id = id_str.parse::<u32>().ok().filter(|id| *id != 0)?;
    let mut mark = AlbumMark { id, layout: String::new(), frame: String::new(), captions: false, grayscale: false, theme: String::new() };
    if !rest.is_empty() {
        for part in rest.split(';') {
            let Some((k, v)) = part.split_once('=') else { continue };
            match k {
                "layout" => mark.layout = v.to_string(),
                "frame" => mark.frame = v.to_string(),
                "captions" => mark.captions = v == "1",
                "gray" => mark.grayscale = v == "1",
                "theme" => mark.theme = v.to_string(),
                _ => {}
            }
        }
    }
    Some(mark)
}

fn album_marker(id: u32, opts: &AlbumOpts) -> String {
    let theme = opts.theme.as_deref().unwrap_or("");
    format!(
        "deckcraft-album:{id};layout={};frame={};captions={};gray={};theme={theme}",
        opts.layout_key,
        opts.frame_key,
        if opts.captions { 1 } else { 0 },
        if opts.grayscale { 1 } else { 0 },
    )
}

fn stamp_album_slide(s: &mut Session, album_id: u32, opts: &AlbumOpts) -> Result<()> {
    let marker = album_marker(album_id, opts);
    s.edit(|doc, sel| {
        let Some(slide) = doc.slides.get_mut(sel.slide) else {
            return Err(bad("insert.photoAlbum", "no slide to mark"));
        };
        let slide = std::sync::Arc::make_mut(slide);
        if slide.raw_ext.as_deref().is_none_or(|ext| ext.starts_with("deckcraft-album:")) {
            slide.raw_ext = Some(marker);
        }
        slide.name = "Photo Album".into();
        Ok(())
    })
}

fn apply_theme_if_needed(s: &mut Session, theme: Option<&str>) -> Result<()> {
    let Some(name) = theme.filter(|n| !n.is_empty()) else { return Ok(()) };
    let current = s.doc()?.doc.masters.first().map(|m| m.theme.name.clone()).unwrap_or_default();
    if current.eq_ignore_ascii_case(name) {
        return Ok(());
    }
    s.execute("design.theme", &json!({"name": name}))?;
    Ok(())
}

fn album_id_param(p: &Value) -> Option<u32> {
    match p.get("album") {
        Some(Value::Number(n)) => n.as_u64().and_then(|v| u32::try_from(v).ok()).filter(|v| *v != 0),
        Some(Value::String(s)) => s.trim().parse::<u32>().ok().filter(|v| *v != 0),
        _ => None,
    }
}

fn slides_with_album(doc: &deckcraft_model::Presentation, id: u32) -> Vec<u64> {
    doc.slides.iter().filter(|s| s.raw_ext.as_deref().and_then(parse_album_mark).is_some_and(|m| m.id == id)).map(|s| u64::from(s.id.0)).collect()
}

fn album_name_run(doc: &deckcraft_model::Presentation, index: usize) -> Vec<u64> {
    if doc.slides.get(index).is_none_or(|s| s.name != "Photo Album") {
        return Vec::new();
    }
    let mut start = index;
    while start > 0 && doc.slides.get(start - 1).is_some_and(|s| s.name == "Photo Album") {
        start -= 1;
    }
    let mut end = index;
    while doc.slides.get(end + 1).is_some_and(|s| s.name == "Photo Album") {
        end += 1;
    }
    doc.slides[start..=end].iter().map(|s| u64::from(s.id.0)).collect()
}

/// The album under the selection, or the one named by `album`. Id 0 is a name-only run (no marker).
fn album_target(s: &Session, p: &Value) -> Result<(u32, Vec<u64>)> {
    let st = s.doc()?;
    if let Some(id) = album_id_param(p) {
        let ids = slides_with_album(&st.doc, id);
        if ids.is_empty() {
            return Err(bad("insert.photoAlbumEdit", "that photo album is no longer in this presentation"));
        }
        return Ok((id, ids));
    }
    let Some(slide) = st.doc.slides.get(st.selection.slide) else {
        return Err(bad("insert.photoAlbumEdit", "this slide is not part of a photo album"));
    };
    if let Some(mark) = slide.raw_ext.as_deref().and_then(parse_album_mark) {
        let ids = slides_with_album(&st.doc, mark.id);
        if ids.is_empty() {
            return Err(bad("insert.photoAlbumEdit", "this slide is not part of a photo album"));
        }
        return Ok((mark.id, ids));
    }
    if slide.name == "Photo Album" {
        let ids = album_name_run(&st.doc, st.selection.slide);
        if !ids.is_empty() {
            return Ok((0, ids));
        }
    }
    Err(bad("insert.photoAlbumEdit", "this slide is not part of a photo album"))
}

fn pic_count(slide: &deckcraft_model::Slide) -> usize {
    slide.shapes.iter().filter(|sh| matches!(sh.kind, ShapeKind::Picture { .. })).count()
}

fn infer_album(slides: &[&deckcraft_model::Slide], theme_now: &str) -> (String, String, bool, bool, String) {
    let max_pics = slides.iter().map(|s| pic_count(s)).max().unwrap_or(1);
    let per = if max_pics >= 3 {
        "4"
    } else if max_pics == 2 {
        "2"
    } else {
        "1"
    };
    let title =
        slides.iter().any(|s| pic_count(s) > 0 && s.shapes.iter().any(|sh| sh.text_box && sh.xfrm.is_some_and(|x| x.y < 36.0 && x.h <= 48.0)));
    let layout = if title { format!("{per}title") } else { per.into() };
    let width = slides.iter().find_map(|s| {
        s.shapes.iter().find_map(|sh| match &sh.kind {
            ShapeKind::Picture { .. } => sh.line.as_ref().and_then(|l| l.width),
            _ => None,
        })
    });
    let frame = match width {
        None => "none",
        Some(w) if (w - 6.0).abs() < 0.2 => "simple",
        Some(w) if (w - 2.25).abs() < 0.2 => "rounded",
        Some(w) if w < 0.2 => "none",
        Some(_) => "rect",
    };
    let captions = slides.iter().any(|s| {
        let bottoms: Vec<f64> =
            s.shapes.iter().filter(|sh| matches!(sh.kind, ShapeKind::Picture { .. })).filter_map(|sh| sh.xfrm.map(|x| x.y + x.h)).collect();
        !bottoms.is_empty() && s.shapes.iter().any(|sh| sh.text_box && sh.xfrm.is_some_and(|x| bottoms.iter().any(|b| x.y + 1.0 >= *b)))
    });
    let mut any = false;
    let mut gray = true;
    for s in slides {
        for sh in &s.shapes {
            if let ShapeKind::Picture { fill } = &sh.kind {
                any = true;
                gray &= fill.adjust.grayscale;
            }
        }
    }
    (layout, frame.into(), captions, any && gray, theme_now.to_string())
}

fn items_from_slides(doc: &deckcraft_model::Presentation, slides: &[&deckcraft_model::Slide]) -> Vec<Value> {
    let mut items = Vec::new();
    for slide in slides {
        let pics: Vec<&deckcraft_model::Shape> = slide.shapes.iter().filter(|sh| matches!(sh.kind, ShapeKind::Picture { .. })).collect();
        if pics.is_empty() {
            let text = slide
                .shapes
                .iter()
                .filter(|sh| sh.text_box)
                .filter_map(|sh| sh.text.as_ref().map(|t| t.text()))
                .find(|t| !t.trim().is_empty())
                .unwrap_or_default();
            if !text.trim().is_empty() {
                items.push(json!({"text": text.trim()}));
            }
            continue;
        }
        for sh in pics {
            let ShapeKind::Picture { fill } = &sh.kind else { continue };
            let media = doc.media(fill.media);
            let name = if !sh.descr.is_empty() {
                sh.descr.clone()
            } else {
                media.map(|m| m.name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| "picture".into())
            };
            items.push(json!({
                "name": name,
                "data": media.map(|m| base64_encode(&m.data)).unwrap_or_default(),
                "rot": sh.xfrm.map(|x| x.rot).unwrap_or(0.0),
                "brightness": fill.adjust.brightness,
                "contrast": fill.adjust.contrast,
            }));
        }
    }
    items
}

fn album_seed(s: &Session) -> Result<Value> {
    let (id, ids) = album_target(s, &Value::Null)?;
    let st = s.doc()?;
    let slides: Vec<&deckcraft_model::Slide> =
        ids.iter().filter_map(|sid| u32::try_from(*sid).ok()).filter_map(|n| st.doc.slide(deckcraft_model::SlideId(n))).collect();
    let theme_now = st.doc.masters.first().map(|m| m.theme.name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| "DeckCraft".into());
    let inferred = infer_album(&slides, &theme_now);
    let mark = slides.first().and_then(|sl| sl.raw_ext.as_deref().and_then(parse_album_mark));
    let (layout, frame, captions, grayscale, theme) = if let Some(mark) = mark {
        (
            if mark.layout.is_empty() { inferred.0 } else { mark.layout },
            if mark.frame.is_empty() { inferred.1 } else { mark.frame },
            mark.captions,
            mark.grayscale,
            if mark.theme.is_empty() { theme_now } else { mark.theme },
        )
    } else {
        inferred
    };
    Ok(json!({
        "album": if id == 0 { Value::Null } else { json!(id) },
        "layout": layout,
        "frame": frame,
        "captions": captions,
        "grayscale": grayscale,
        "theme": theme,
        "items": items_from_slides(&st.doc, &slides),
    }))
}

fn insert_album(s: &mut Session, items: Vec<AlbumItem>, opts: &AlbumOpts, at: Option<usize>, album_id: u32) -> Result<Value> {
    let (width, height, first) = {
        let st = s.doc()?;
        let first = at.unwrap_or_else(|| if st.doc.slides.is_empty() { 0 } else { st.selection.slide.saturating_add(1) });
        (st.doc.slide_size.width, st.doc.slide_size.height, first)
    };
    let layout = opts.layout;
    let frame = opts.frame;
    let captions = opts.captions;
    let plan = album_plan(items, layout.per);
    let mut slides = 0usize;
    let mut pictures = 0usize;
    let mut pin = at;
    for slide in plan {
        if let Some(index) = pin.take() {
            s.execute("slide.new", &json!({"layout": "blank", "at": index}))?;
        } else {
            s.execute("slide.new", &json!({"layout": "blank"}))?;
        }
        stamp_album_slide(s, album_id, opts)?;
        slides += 1;
        match slide {
            AlbumSlide::Text(text) => {
                let rect = if layout.title {
                    [36.0, 18.0, (width - 72.0).max(36.0), 48.0]
                } else {
                    [36.0, 36.0, (width - 72.0).max(36.0), (height - 72.0).max(36.0)]
                };
                s.execute("insert.textBox", &json!({"rect": rect, "text": text}))?;
            }
            AlbumSlide::Pictures(pics) => {
                let stem = pics.first().map(|p| file_stem(&p.name)).unwrap_or_default();
                let (title, placed) = place_pictures(width, height, layout, &pics, captions);
                for (pic, cell) in pics.iter().zip(placed) {
                    let data = base64_encode(&pic.bytes);
                    let inserted = s.execute("insert.picture", &json!({"name": pic.name, "data": data, "rect": cell.pic}))?;
                    let Some(id) = inserted.get("id").and_then(Value::as_u64) else {
                        return Err(bad("insert.photoAlbum", "the picture was not inserted"));
                    };
                    apply_frame(s, frame, id)?;
                    if opts.grayscale || pic.brightness.abs() > 1e-6 || pic.contrast.abs() > 1e-6 {
                        s.execute(
                            "picture.adjust",
                            &json!({"id": id, "grayscale": opts.grayscale, "brightness": pic.brightness, "contrast": pic.contrast}),
                        )?;
                    }
                    if pic.rot > 0.01 {
                        s.execute("shape.rotate", &json!({"id": id, "deg": pic.rot}))?;
                    }
                    pictures += 1;
                    if let Some(cap) = cell.caption {
                        s.execute("insert.textBox", &json!({"rect": cap, "text": file_stem(&pic.name)}))?;
                    }
                }
                if let Some(rect) = title {
                    s.execute("insert.textBox", &json!({"rect": rect, "text": stem}))?;
                }
            }
        }
    }
    s.execute("slide.go", &json!({"index": first}))?;
    Ok(json!({"slides": slides, "first": first, "pictures": pictures, "album": album_id}))
}

fn table(s: &mut Session, p: &Value) -> Result<Value> {
    let rows = usize_param(p, "rows").unwrap_or(2).clamp(1, 75);
    let cols = usize_param(p, "cols").unwrap_or(2).clamp(1, 75);
    let size = s.doc()?.doc.slide_size;
    let w = size.width * 0.8;
    let rh = 40.0;
    let rect = rect_param(p, "rect").unwrap_or(Xfrm::new(size.width * 0.1, (size.height - rh * rows as f64).max(0.0) / 2.0, w, rh * rows as f64));
    let mut t = Table::new(rows, cols, rect.w, rect.h / rows as f64);
    if let Some(data) = p.get("data").and_then(Value::as_array) {
        for (r, row) in data.iter().enumerate() {
            for (c, v) in row.as_array().map(|a| a.as_slice()).unwrap_or(&[]).iter().enumerate() {
                if let (Some(cell), Some(text)) = (t.cell_mut(r, c), v.as_str()) {
                    cell.text = TextBody::from_text(text);
                }
            }
        }
    }
    let id = add_shape(s, "insert.table", Shape { xfrm: Some(rect), kind: ShapeKind::Table(t), ..Default::default() }, true)?;
    Ok(json!({"id": id}))
}

pub fn chart_type(name: &str) -> ChartType {
    match name.to_ascii_lowercase().replace(['-', '_', ' '], "").as_str() {
        "bar" | "clusteredbar" => ChartType::Bar,
        "stackedbar" => ChartType::StackedBar,
        "percentbar" => ChartType::PercentBar,
        "line" => ChartType::Line,
        "linemarkers" | "linewithmarkers" => ChartType::LineMarkers,
        "stackedline" => ChartType::StackedLine,
        "area" => ChartType::Area,
        "stackedarea" => ChartType::StackedArea,
        "pie" => ChartType::Pie,
        "doughnut" | "donut" => ChartType::Doughnut,
        "scatter" | "xy" => ChartType::Scatter,
        "bubble" => ChartType::Bubble,
        "radar" => ChartType::Radar,
        "filledradar" => ChartType::FilledRadar,
        "stackedcolumn" => ChartType::StackedColumn,
        "percentcolumn" => ChartType::PercentColumn,
        "waterfall" => ChartType::Waterfall,
        "funnel" => ChartType::Funnel,
        "histogram" => ChartType::Histogram,
        "pareto" => ChartType::Pareto,
        "treemap" => ChartType::Treemap,
        "sunburst" => ChartType::Sunburst,
        "boxwhisker" | "box" => ChartType::BoxWhisker,
        "stock" => ChartType::Stock,
        "surface" => ChartType::Surface,
        "combo" => ChartType::Combo,
        _ => ChartType::Column,
    }
}

fn chart(s: &mut Session, p: &Value) -> Result<Value> {
    let kind = chart_type(str_param(p, "type").unwrap_or("column"));
    let mut c = Chart::sample(kind);
    if let Some(cats) = p.get("categories").and_then(Value::as_array) {
        c.categories = cats.iter().map(|v| v.as_str().map(String::from).unwrap_or_else(|| v.to_string())).collect();
    }
    if let Some(series) = p.get("series").and_then(Value::as_array) {
        c.series = series
            .iter()
            .map(|sv| deckcraft_model::chart::Series {
                name: sv.get("name").and_then(Value::as_str).unwrap_or("Series").to_string(),
                values: sv.get("values").and_then(Value::as_array).map(|a| a.iter().map(Value::as_f64).collect()).unwrap_or_default(),
                ..Default::default()
            })
            .collect();
    }
    if let Some(t) = p.get("title") {
        c.title = t.as_str().map(String::from);
    }
    let size = s.doc()?.doc.slide_size;
    let rect = rect_param(p, "rect").unwrap_or(Xfrm::new(size.width * 0.15, size.height * 0.15, size.width * 0.7, size.height * 0.7));
    let id = add_shape(s, "insert.chart", Shape { xfrm: Some(rect), kind: ShapeKind::Chart(Box::new(c)), ..Default::default() }, true)?;
    Ok(json!({"id": id}))
}

fn media(s: &mut Session, p: &Value, video: bool) -> Result<Value> {
    let cmd = if video { "insert.video" } else { "insert.audio" };
    let (name, bytes) = media_bytes(p, cmd)?;
    let ct = content_type(&name, &bytes);
    let bytes = std::sync::Arc::new(bytes);
    let info = deckcraft_media::probe(&bytes);
    let has_video = info.as_ref().is_ok_and(|i| i.video.is_some());
    let has_audio = info.as_ref().is_ok_and(|i| i.audio.is_some());
    match &info {
        Ok(_) if video && !has_video => return Err(bad(cmd, format!("{name} has no video"))),
        Ok(_) if !video && !has_audio && !has_video => return Err(bad(cmd, format!("{name} has no audio"))),
        // Not something we can read: embed it anyway when the type says it's media (it still
        // round-trips through PPTX), otherwise refuse.
        Err(_) if video && !ct.starts_with("video/") => return Err(bad(cmd, format!("{name} is not a supported video ({ct})"))),
        Err(_) if !video && !ct.starts_with("audio/") => return Err(bad(cmd, format!("{name} is not a supported audio file ({ct})"))),
        _ => {}
    }
    let (duration_ms, width, height, playable) = match &info {
        Ok(i) => {
            let (w, h) = i.video.as_ref().map(|v| (v.width, v.height)).unwrap_or((0, 0));
            (i.duration_ms.min(u32::MAX as u64) as u32, w, h, i.playable())
        }
        Err(_) => (0, 0, 0, false),
    };
    let decodable_video = info.as_ref().is_ok_and(|i| i.video.as_ref().is_some_and(|v| v.decodable));
    let warning = if !playable {
        Some(match &info {
            Ok(i) => {
                let codecs: Vec<&str> = i.audio.iter().map(|a| a.codec.as_str()).chain(i.video.iter().map(|v| v.codec.as_str())).collect();
                format!("{name} was inserted, but it can't be played ({} isn't supported yet)", codecs.join(" / "))
            }
            Err(e) => format!("{name} was inserted, but it can't be played ({e})"),
        })
    } else if video && !decodable_video {
        // A video codec without a decoder (VP8, say): the clip's audio still plays, but no frame
        // can ever show — say so instead of leaving an unexplained black box (#1237).
        info.as_ref()
            .ok()
            .and_then(|i| i.video.as_ref())
            .map(|v| format!("{name} was inserted, but its video can't be played ({} isn't supported yet); only the audio plays", v.codec))
    } else {
        None
    };
    let poster_png = if video && decodable_video { super::media::poster_png(&bytes, 0) } else { None };
    let size = s.doc()?.doc.slide_size;
    let rect = rect_param(p, "rect").unwrap_or_else(|| {
        if video {
            // Natural size (96 dpi), fitted inside 90 % of the slide and centred.
            let (w, h) =
                if width > 0 && height > 0 { (width as f64 * 0.75, height as f64 * 0.75) } else { (size.width * 0.6, size.width * 0.6 * 9.0 / 16.0) };
            let k = (size.width * 0.9 / w).min(size.height * 0.9 / h).min(1.0);
            let (w, h) = (w * k, h * k);
            Xfrm::new((size.width - w) / 2.0, (size.height - h) / 2.0, w, h)
        } else {
            Xfrm::new((size.width - 48.0) / 2.0, (size.height - 48.0) / 2.0, 48.0, 48.0)
        }
    });
    let data = std::sync::Arc::try_unwrap(bytes).unwrap_or_else(|a| (*a).clone());
    let (media, poster) = s.edit(|doc, _| {
        let media = doc.add_media(&name, ct, data).ok_or_else(|| bad("insert.media", NO_ID_SPACE))?;
        let poster = match poster_png {
            Some(png) => Some(doc.add_media(&format!("{name}-poster.png"), "image/png", png).ok_or_else(|| bad("insert.media", NO_ID_SPACE))?),
            None => None,
        };
        Ok((media, poster))
    })?;
    let clip = MediaClip { media, video, poster, volume: 1.0, duration_ms, width, height, ..Default::default() };
    let id = add_shape(s, "insert.media", Shape { xfrm: Some(rect), kind: ShapeKind::Media(clip), descr: name, ..Default::default() }, true)?;
    if let Some(w) = &warning {
        s.ui_requests.push(crate::UiRequest::Message { text: w.clone() });
    }
    let mut r =
        json!({"id": id, "contentType": ct, "durationMs": duration_ms, "width": width, "height": height, "playable": playable, "warning": warning});
    if let Ok(i) = &info {
        r["probe"] = super::media::probe_json(i);
    }
    Ok(r)
}

fn audio(s: &mut Session, p: &Value) -> Result<Value> {
    media(s, p, false)
}
fn video(s: &mut Session, p: &Value) -> Result<Value> {
    media(s, p, true)
}

fn word_art(s: &mut Session, p: &Value) -> Result<Value> {
    let text = str_param(p, "text").unwrap_or("Your text here");
    let size = s.doc()?.doc.slide_size;
    let style = usize_param(p, "style").unwrap_or(0);
    let mut shape = new_text_box(Xfrm::new(size.width * 0.2, size.height * 0.4, size.width * 0.6, 80.0), text);
    if let Some(t) = shape.text.as_mut() {
        for para in &mut t.paragraphs {
            para.props.align = Some(deckcraft_model::text::Align::Center);
            for r in &mut para.runs {
                r.props.size = Some(54.0);
                r.props.bold = Some(style % 2 == 1);
                r.props.fill = Some(Fill::solid(ColorRef::scheme(match style % 4 {
                    0 => SchemeSlot::Tx1,
                    1 => SchemeSlot::Accent1,
                    2 => SchemeSlot::Accent2,
                    _ => SchemeSlot::Accent4,
                })));
                if style % 3 == 2 {
                    r.props.outline = Some(deckcraft_model::Line::solid(ColorRef::scheme(SchemeSlot::Bg1), 1.0));
                }
            }
        }
    }
    let id = add_shape(s, "insert.wordArt", shape, true)?;
    super::text::fit_text_box(s, id)?;
    Ok(json!({"id": id}))
}

fn field_run(s: &mut Session, field: &str, text: &str) -> Result<Value> {
    if s.doc()?.selection.text.is_some() {
        return super::text::insert_field(s, field, text);
    }
    // Not editing: turn on the footer field for the slide (Header & Footer).
    let key = if field == "slidenum" { "slideNumber" } else { "date" };
    s.execute("design.headerFooter", &json!({key: true}))
}

fn slide_number(s: &mut Session, _p: &Value) -> Result<Value> {
    field_run(s, "slidenum", "‹#›")
}

const DATE_FORMATS: [&str; 13] = [
    "datetime1",
    "datetime2",
    "datetime3",
    "datetime4",
    "datetime5",
    "datetime6",
    "datetime7",
    "datetime8",
    "datetime9",
    "datetime10",
    "datetime11",
    "datetime12",
    "datetime13",
];

struct DateOpts {
    format: String,
    update: bool,
    /// BCP 47 tag when the caller set `language`; absent keeps the surrounding run's language.
    lang: Option<String>,
    sample: String,
}

/// `true` when the call carries a format, update flag or language (apply it). Empty params open
/// the dialog instead.
fn date_time_apply(p: &Value) -> bool {
    p.as_object().is_some_and(|o| o.contains_key("format") || o.contains_key("update") || o.contains_key("language"))
}

fn canon_lang(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_lowercase().replace('_', "-").as_str() {
        "en" | "en-us" => Some("en-US"),
        "en-gb" => Some("en-GB"),
        "fr" | "fr-fr" => Some("fr-FR"),
        "ja" | "ja-jp" => Some("ja-JP"),
        "sv" | "sv-se" => Some("sv-SE"),
        _ => None,
    }
}

fn date_opts(p: &Value) -> Result<DateOpts> {
    if p.get("format").is_some() {
        let f = str_param(p, "format").filter(|f| DATE_FORMATS.contains(f));
        if f.is_none() {
            return Err(bad("insert.dateTime", "`format` must be datetime1..datetime13"));
        }
    }
    if p.get("update").is_some() && bool_param(p, "update").is_none() {
        return Err(bad("insert.dateTime", "`update` must be true or false"));
    }
    let lang = if p.get("language").is_some() {
        let raw = str_param(p, "language").filter(|l| !l.trim().is_empty());
        let Some(raw) = raw else {
            return Err(bad("insert.dateTime", "`language` must be en-US, en-GB, fr-FR, ja-JP or sv-SE"));
        };
        Some(canon_lang(raw).ok_or_else(|| bad("insert.dateTime", "`language` must be en-US, en-GB, fr-FR, ja-JP or sv-SE"))?.to_string())
    } else {
        None
    };
    let format = str_param(p, "format").unwrap_or("datetime1").to_string();
    let sample = date_sample(&format, lang.as_deref())?;
    Ok(DateOpts { format, update: bool_or(p, "update", true), lang, sample })
}

/// The text `format` shows now in `lang`. Errors when that language has no names for the format
/// (month names and AM/PM are English only). An empty string where there is no clock (wasm).
fn date_sample(format: &str, lang: Option<&str>) -> Result<String> {
    use deckcraft_text::datetime::{DateTime, field_text};
    let probe = DateTime::new(2026, 10, 9, 20, 5, 7);
    if field_text(format, lang, probe).is_none() {
        return Err(bad("insert.dateTime", "this format isn't available in that language"));
    }
    Ok(DateTime::now_local().and_then(|now| field_text(format, lang, now)).unwrap_or_default())
}

fn date_time(s: &mut Session, p: &Value) -> Result<Value> {
    if !date_time_apply(p) {
        s.ui_requests.push(crate::UiRequest::Dialog { id: "dateTime".into(), params: json!({}) });
        return ok();
    }
    let opts = date_opts(p)?;
    let (doc, sel) = {
        let st = s.doc()?;
        (st.doc.clone(), st.selection.clone())
    };
    let r = apply_date(s, &opts);
    if r.is_err()
        && let Some(st) = s.active_mut()
    {
        st.doc = doc;
        st.selection = sel;
    }
    r
}

fn apply_date(s: &mut Session, o: &DateOpts) -> Result<Value> {
    if s.doc()?.selection.text.is_some() {
        if o.update {
            super::text::insert_field(s, &o.format, &o.sample)?;
            if let Some(lang) = &o.lang {
                stamp_caret_lang(s, lang)?;
            }
        } else {
            insert_plain(s, &o.sample, o.lang.as_deref())?;
        }
        return Ok(json!({"where": "caret", "format": o.format, "update": o.update, "text": o.sample}));
    }
    // Not editing: the slide's date placeholder (this slide only). A layout with no date
    // placeholder gets a text box instead, so the chosen format is still inserted.
    s.execute("design.headerFooter", &json!({"date": true, "all": false}))?;
    if !write_date_placeholder(s, o)? {
        return insert_date_box(s, o);
    }
    Ok(json!({"where": "footer", "format": o.format, "update": o.update, "text": o.sample}))
}

/// Point the current slide's date placeholder at this format (a field, or the sample as plain text).
fn write_date_placeholder(s: &mut Session, o: &DateOpts) -> Result<bool> {
    let format = o.format.clone();
    let sample = o.sample.clone();
    let lang = o.lang.clone();
    let update = o.update;
    s.edit(|doc, sel| {
        let Some(sl) = doc.slides.get_mut(sel.slide) else { return Ok(false) };
        let sl = std::sync::Arc::make_mut(sl);
        let Some(sh) = sl.shapes.iter_mut().find(|x| x.ph_type() == Some(deckcraft_model::PhType::Date)) else {
            return Ok(false);
        };
        let mut props = sh.text.as_ref().and_then(|t| t.paragraphs.first()).and_then(|p| p.runs.first()).map(|r| r.props.clone()).unwrap_or_default();
        if let Some(lang) = lang {
            props.lang = Some(lang);
        }
        let kind = if update { RunKind::Field { field: format } } else { RunKind::Text };
        let para = deckcraft_model::Paragraph { runs: vec![deckcraft_model::Run { text: sample, props, kind }], ..Default::default() };
        let body = sh.text.clone().unwrap_or_default();
        sh.text = Some(TextBody { paragraphs: vec![para], ..body });
        Ok(true)
    })
}

fn insert_date_box(s: &mut Session, o: &DateOpts) -> Result<Value> {
    let size = s.doc()?.doc.slide_size;
    let y = (size.height - 48.0).max(0.0);
    s.execute("insert.textBox", &json!({"rect": [36.0, y, 360.0, 28.0], "text": ""}))?;
    if o.update {
        super::text::insert_field(s, &o.format, &o.sample)?;
    } else {
        super::text::insert_text(s, &o.sample, None)?;
    }
    if let Some(lang) = &o.lang {
        stamp_caret_lang(s, lang)?;
    }
    Ok(json!({"where": "textBox", "format": o.format, "update": o.update, "text": o.sample}))
}

/// Insert `text` at the caret. `lang`, when set, is on that run before runs with equal props merge,
/// so the date stays its own run next to the surrounding text.
fn insert_plain(s: &mut Session, text: &str, lang: Option<&str>) -> Result<()> {
    let text = text.to_string();
    let lang = lang.map(str::to_string);
    super::text::edit_text(s, move |body, t| {
        let (a, b) = t.ordered();
        let at = deckcraft_model::edit::delete(body, a, b);
        let mut props = deckcraft_model::edit::props_at(body, at);
        if let Some(lang) = lang.clone() {
            props.lang = Some(lang);
        }
        let c = deckcraft_model::edit::insert(body, at, &text, Some(props));
        t.anchor = c;
        t.caret = c;
    })?;
    Ok(())
}

/// Tag the run that ends at the caret with `lang`, so a date field keeps that language when it updates.
fn stamp_caret_lang(s: &mut Session, lang: &str) -> Result<()> {
    let lang = lang.to_string();
    super::text::edit_text(s, move |body, t| {
        let Some(p) = body.paragraphs.get_mut(t.caret.0) else { return };
        let mut at = 0usize;
        for r in &mut p.runs {
            let n = r.char_len();
            if n > 0 && at < t.caret.1 && t.caret.1 <= at + n {
                r.props.lang = Some(lang);
                return;
            }
            at += n;
        }
    })?;
    Ok(())
}

fn symbol(s: &mut Session, p: &Value) -> Result<Value> {
    let t = str_param(p, "text").ok_or_else(|| bad("insert.symbol", "missing `text`"))?.to_string();
    if s.doc()?.selection.text.is_none() {
        return Err(bad("insert.symbol", "place the insertion point in text first"));
    }
    super::text::insert_text(s, &t, None)
}

fn hyperlink(s: &mut Session, p: &Value) -> Result<Value> {
    use deckcraft_model::text::{Action, Hyperlink};
    // `display`: what a caret with nothing to link inserts, the address or the slide's title.
    let (action, display) = if let Some(u) = str_param(p, "url") {
        (Action::Url { url: u.to_string() }, u.strip_prefix("mailto:").unwrap_or(u).to_string())
    } else if let Some(i) = usize_param(p, "slide") {
        let slide = s.doc()?.doc.slides.get(i).ok_or_else(|| bad("insert.hyperlink", "no such slide"))?;
        let title = slide.title();
        (Action::Slide { slide: slide.id }, if title.is_empty() { format!("Slide {}", i + 1) } else { title })
    } else {
        return Err(bad("insert.hyperlink", "missing `url` or `slide`"));
    };
    let display = str_param(p, "text").map_or(display, str::to_string);
    let link = Hyperlink {
        action,
        tooltip: str_param(p, "tooltip").unwrap_or("").to_string(),
        highlight_click: bool_or(p, "highlightClick", false),
        sound: None,
        stop_sound: false,
        zoom_return: None,
    };
    if s.doc()?.selection.text.is_some() {
        return super::text::link_text(s, link, &display);
    }
    edit_shapes(s, p, "insert.hyperlink", |sh| {
        sh.click = Some(link.clone());
        Ok(())
    })
}

/// Insert ▸ Action (Action Settings): what clicking the selected shapes, or pointing at them, does
/// in a show.
fn shape_action(s: &mut Session, p: &Value) -> Result<Value> {
    use deckcraft_model::text::{Action, Hyperlink};
    const CMD: &str = "shape.action";
    let hover = match str_param(p, "on").unwrap_or("click") {
        "click" => false,
        "hover" | "over" => true,
        o => return Err(bad(CMD, format!("`on` is click or hover, not `{o}`"))),
    };
    let need = |k: &str| str_param(p, k).map(str::to_string).filter(|v| !v.trim().is_empty()).ok_or_else(|| bad(CMD, format!("missing `{k}`")));
    let action = match str_param(p, "action").unwrap_or("none") {
        "none" => None,
        "next" => Some(Action::NextSlide),
        "previous" => Some(Action::PreviousSlide),
        "first" => Some(Action::FirstSlide),
        "last" => Some(Action::LastSlide),
        "lastViewed" => Some(Action::LastViewed),
        "endShow" => Some(Action::EndShow),
        "playMedia" => Some(Action::PlayMedia),
        "url" => Some(Action::Url { url: need("url")? }),
        "customShow" => {
            let name = need("show")?;
            if !s.doc()?.doc.custom_shows.iter().any(|c| c.name == name) {
                return Err(bad(CMD, format!("no custom show `{name}`")));
            }
            Some(Action::CustomShow { name, returns: true })
        }
        "program" => Some(Action::Program { path: need("path")? }),
        "slide" => {
            let i = usize_param(p, "slide").ok_or_else(|| bad(CMD, "missing `slide`"))?;
            let slide = s.doc()?.doc.slides.get(i).ok_or_else(|| bad(CMD, "no such slide"))?;
            Some(Action::Slide { slide: slide.id })
        }
        a => return Err(bad(CMD, format!("unknown action `{a}`"))),
    };
    let highlight = bool_param(p, "highlight").unwrap_or(true);
    let tooltip = str_param(p, "tooltip").unwrap_or("").to_string();
    let link = action.map(|action| Hyperlink { action, tooltip, highlight_click: highlight, sound: None, stop_sound: false, zoom_return: None });
    edit_shapes(s, p, CMD, |sh| {
        if hover {
            sh.hover = link.clone();
        } else {
            sh.click = link.clone();
        }
        Ok(())
    })
}

fn action_button(s: &mut Session, p: &Value) -> Result<Value> {
    use deckcraft_model::text::{Action, Hyperlink};
    let kind = str_param(p, "kind").unwrap_or("custom");
    let (preset, action) = match kind {
        "back" | "previous" => ("actionButtonBackPrevious", Some(Action::PreviousSlide)),
        "forward" | "next" => ("actionButtonForwardNext", Some(Action::NextSlide)),
        "beginning" => ("actionButtonBeginning", Some(Action::FirstSlide)),
        "end" => ("actionButtonEnd", Some(Action::LastSlide)),
        "home" => ("actionButtonHome", Some(Action::FirstSlide)),
        "information" => ("actionButtonInformation", None),
        "return" => ("actionButtonReturn", Some(Action::LastViewed)),
        "movie" => ("actionButtonMovie", None),
        "document" => ("actionButtonDocument", None),
        "sound" => ("actionButtonSound", None),
        "help" => ("actionButtonHelp", None),
        _ => ("actionButtonBlank", None),
    };
    let rect = rect_param(p, "rect").unwrap_or_else(|| default_rect(s, 48.0, 48.0));
    let shape = Shape {
        xfrm: Some(rect),
        geom: Geom::preset(preset),
        style: Some(ShapeStyle::accent(SchemeSlot::Accent1)),
        click: action.map(|a| Hyperlink {
            action: a,
            tooltip: String::new(),
            highlight_click: true,
            sound: None,
            stop_sound: false,
            zoom_return: None,
        }),
        ..Default::default()
    };
    let id = add_shape(s, "insert.actionButton", shape, true)?;
    Ok(json!({"id": id}))
}

#[allow(dead_code)]
fn _kinds() -> RunKind {
    RunKind::Text
}

fn with_chart(s: &mut Session, p: &Value, cmd: &str, f: impl Fn(&mut Chart) -> Result<()>) -> Result<Value> {
    edit_shapes(s, p, cmd, |sh| match &mut sh.kind {
        ShapeKind::Chart(c) => f(c),
        _ => Err(bad(cmd, "not a chart")),
    })
}

fn chart_type_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let k = chart_type(str_param(p, "type").unwrap_or("column"));
    with_chart(s, p, "chart.type", |c| {
        c.kind = k;
        c.vary_colors =
            matches!(k, ChartType::Pie | ChartType::Doughnut | ChartType::Funnel | ChartType::Treemap | ChartType::Sunburst | ChartType::Waterfall)
                || c.series.len() == 1 && c.vary_colors;
        if matches!(k, ChartType::Waterfall | ChartType::Histogram | ChartType::Pareto | ChartType::BoxWhisker) {
            c.gridlines = true;
        }
        c.raw = None;
        Ok(())
    })
}

fn chart_data(s: &mut Session, p: &Value) -> Result<Value> {
    let cats: Option<Vec<String>> = p
        .get("categories")
        .and_then(Value::as_array)
        .map(|a| a.iter().map(|v| v.as_str().map(String::from).unwrap_or_else(|| v.to_string())).collect());
    let series: Option<Vec<deckcraft_model::chart::Series>> = p.get("series").and_then(Value::as_array).map(|a| {
        a.iter()
            .map(|sv| deckcraft_model::chart::Series {
                name: sv.get("name").and_then(Value::as_str).unwrap_or("Series").to_string(),
                values: sv.get("values").and_then(Value::as_array).map(|v| v.iter().map(Value::as_f64).collect()).unwrap_or_default(),
                ..Default::default()
            })
            .collect()
    });
    let levels: Option<Vec<Vec<String>>> = p.get("levels").and_then(Value::as_array).map(|a| {
        a.iter()
            .take(deckcraft_model::chart::MAX_LEVELS)
            .map(|l| {
                l.as_array().map(|v| v.iter().map(|x| x.as_str().map(String::from).unwrap_or_else(|| x.to_string())).collect()).unwrap_or_default()
            })
            .collect()
    });
    with_chart(s, p, "chart.data", |c| {
        if let Some(cs) = &cats {
            c.categories = cs.clone();
        }
        if let Some(l) = &levels {
            c.category_levels = l.clone();
        }
        if let Some(se) = &series {
            // Keep series formatting by position.
            let old = std::mem::take(&mut c.series);
            c.series = se
                .iter()
                .enumerate()
                .map(|(i, n)| {
                    let mut x = old.get(i).cloned().unwrap_or_default();
                    x.name = n.name.clone();
                    x.values = n.values.clone();
                    x
                })
                .collect();
        }
        c.raw = None;
        Ok(())
    })
}

fn apply_bin_options(c: &mut deckcraft_model::chart::Chart, bin_count: Option<Option<u32>>, bin_size: Option<Option<f64>>) {
    if let Some(n) = bin_count {
        c.bin_count = n;
        if n.is_some() {
            c.bin_size = None;
        }
    }
    if let Some(w) = bin_size {
        c.bin_size = w;
        if w.is_some() {
            c.bin_count = None;
        }
    }
}

fn apply_subtotals_and_bars(c: &mut deckcraft_model::chart::Chart, p: &Value, subtotals: Option<&[usize]>) {
    if let Some(t) = subtotals {
        let n = c.series.first().map_or(0, |s| s.values.len());
        let mut t: Vec<usize> = t.iter().copied().filter(|i| *i < n).collect();
        t.sort_unstable();
        t.dedup();
        c.subtotals = t;
    }
    if let Some(b) = p.get("upDownBars").and_then(Value::as_bool) {
        c.up_down_bars = b;
        c.raw = None;
    }
    if let Some(q) = p.get("quartiles").and_then(Value::as_str) {
        c.inclusive_quartiles = q.eq_ignore_ascii_case("inclusive");
        c.raw = None;
    }
}

fn chart_options(s: &mut Session, p: &Value) -> Result<Value> {
    // Histogram bins: a count or a width (setting one clears the other; null for automatic).
    let bin_count = p.get("binCount").map(|v| v.as_u64().map(|n| n.clamp(1, deckcraft_model::chart::MAX_BINS as u64) as u32));
    let bin_size = p.get("binSize").map(|v| v.as_f64().filter(|w| w.is_finite() && *w > 0.0));
    if p.get("binSize").is_some_and(|v| !v.is_null()) && bin_size.flatten().is_none() {
        return Err(bad("chart.options", "binSize must be a positive number"));
    }
    // Waterfall total points.
    let subtotals: Option<Vec<usize>> =
        p.get("subtotals").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).map(|i| i as usize).collect());
    // Colour of the chart's text (title, axes, legend, data labels); null goes back to PowerPoint's
    // default (tx1 at lumMod 65 % / lumOff 35 %).
    let text_color = match p.get("textColor") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(v) => {
            Some(Some(color_value(v).ok_or_else(|| bad("chart.options", "`textColor` is a colour: #RRGGBB, accent1…accent6, tx1…, or null"))?))
        }
    };
    with_chart(s, p, "chart.options", |c| {
        if let Some(t) = &text_color {
            c.text_color = t.clone();
        }
        apply_bin_options(c, bin_count, bin_size);
        apply_subtotals_and_bars(c, p, subtotals.as_deref());
        if bin_count.is_some() || bin_size.is_some() || subtotals.is_some() {
            c.raw = None;
        }
        if let Some(t) = p.get("title") {
            c.title = t.as_str().map(String::from);
        }
        if let Some(l) = p.get("legend") {
            c.legend = l.as_str().map(String::from);
        }
        if let Some(v) = bool_param(p, "dataLabels") {
            c.data_labels = v;
            // A series' own setting would otherwise keep overriding the chart-wide switch.
            for series in &mut c.series {
                series.data_labels = None;
            }
        }
        if let Some(v) = bool_param(p, "gridlines") {
            c.gridlines = v;
        }
        if let Some(v) = str_param(p, "palette") {
            c.palette = v.to_string();
        }
        c.raw = None;
        Ok(())
    })
}

/// A SmartArt graphic: a group regenerated from its outline (see [`super::smartart`]). Items with
/// leading tabs are sub-points; `nodes` (`[{text, level}]`) may be given instead.
fn smart_art(s: &mut Session, p: &Value) -> Result<Value> {
    let kind = str_param(p, "kind").unwrap_or("process").to_string();
    let nodes = match p.get("nodes") {
        Some(v) => serde_json::from_value(v.clone()).map_err(|e| bad("insert.smartArt", e.to_string()))?,
        None => super::smartart::nodes_from_lines(p.get("items").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str)),
    };
    super::smartart::insert(s, &kind, nodes)
}

#[cfg(test)]
mod tests {
    use deckcraft_model::text::RunKind;
    use serde_json::json;

    use crate::Session;

    fn session() -> Session {
        Session::with_new()
    }

    fn undo(s: &Session) -> usize {
        s.doc().unwrap().history.undo.len()
    }

    fn date_run(s: &Session) -> Option<deckcraft_model::Run> {
        s.doc()
            .unwrap()
            .current_slide()
            .unwrap()
            .shapes
            .iter()
            .find(|x| x.ph_type() == Some(deckcraft_model::PhType::Date))
            .and_then(|x| x.text.as_ref())
            .and_then(|t| t.paragraphs.first())
            .and_then(|p| p.runs.first())
            .cloned()
    }

    fn title_field(s: &Session) -> deckcraft_model::Run {
        let title = s.doc().unwrap().current_slide().unwrap().shapes[0].text.clone().unwrap();
        title.paragraphs[0].runs.iter().find(|r| r.text.contains(':') || matches!(r.kind, RunKind::Field { .. })).cloned().unwrap()
    }

    fn edit_title(s: &mut Session) {
        let id = s.doc().unwrap().current_slide().unwrap().shapes[0].id.0;
        s.execute("text.edit", &json!({"id": id})).unwrap();
        s.execute("text.insert", &json!({"text": "On "})).unwrap();
    }

    #[test]
    fn no_params_opens_the_dialog_and_edits_nothing() {
        let mut s = session();
        let rev = s.doc().unwrap().revision;
        for p in [json!({}), serde_json::Value::Null] {
            s.ui_requests.clear();
            s.execute("insert.dateTime", &p).unwrap();
            assert!(matches!(&s.ui_requests[..], [crate::UiRequest::Dialog { id, .. }] if id == "dateTime"));
            assert_eq!(s.doc().unwrap().revision, rev);
            assert_eq!(undo(&s), 0);
            assert!(date_run(&s).is_none());
        }
    }

    #[test]
    fn a_format_at_the_caret_is_one_updating_field() {
        let mut s = session();
        edit_title(&mut s);
        let before = undo(&s);
        let v = s.execute("insert.dateTime", &json!({"format": "datetime10", "update": true})).unwrap();
        assert_eq!(v["where"], "caret");
        let run = title_field(&s);
        assert!(matches!(run.kind, RunKind::Field { ref field } if field == "datetime10"), "{run:?}");
        assert!(run.text.contains(':'), "{run:?}");
        assert_eq!(undo(&s), before + 1);
        assert_eq!(s.doc().unwrap().history.undo.last().unwrap().label, "Date & Time");
        s.execute("edit.undo", &json!({})).unwrap();
        let title = s.doc().unwrap().current_slide().unwrap().shapes[0].text.as_ref().unwrap().text();
        assert_eq!(title, "On ");
    }

    #[test]
    fn update_false_inserts_plain_text_in_the_chosen_language() {
        let mut s = session();
        edit_title(&mut s);
        let before = undo(&s);
        let now = deckcraft_text::datetime::DateTime::now_local().unwrap();
        let expect = deckcraft_text::datetime::field_text("datetime1", Some("en-GB"), now).unwrap();
        s.execute("insert.dateTime", &json!({"format": "datetime1", "update": false, "language": "en-GB"})).unwrap();
        let title = s.doc().unwrap().current_slide().unwrap().shapes[0].text.clone().unwrap();
        let run = title.paragraphs[0].runs.iter().find(|r| r.text == expect).cloned().unwrap();
        assert_eq!(run.kind, RunKind::Text);
        assert_eq!(run.props.lang.as_deref(), Some("en-GB"));
        assert_eq!(undo(&s), before + 1);
    }

    #[test]
    fn a_named_format_in_french_and_a_bad_value_change_nothing() {
        let mut s = session();
        edit_title(&mut s);
        let rev = s.doc().unwrap().revision;
        let undos = undo(&s);
        let err = s.execute("insert.dateTime", &json!({"format": "datetime2", "language": "fr-FR"})).unwrap_err();
        assert!(err.to_string().contains("isn't available"), "{err}");
        assert_eq!(s.doc().unwrap().revision, rev);
        assert_eq!(undo(&s), undos);
        for p in [
            json!({"format": "datetime14"}),
            json!({"format": "nope"}),
            json!({"update": "yes"}),
            json!({"language": "de-DE"}),
            json!({"language": ""}),
        ] {
            let err = s.execute("insert.dateTime", &p).unwrap_err();
            assert!(err.to_string().contains("insert.dateTime"), "{err}");
            assert_eq!(s.doc().unwrap().revision, rev);
            assert_eq!(undo(&s), undos);
        }
    }

    #[test]
    fn without_a_caret_the_date_lands_on_this_slide_only() {
        let mut s = session();
        s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
        s.execute("slide.go", &json!({"index": 0})).unwrap();
        let now = deckcraft_text::datetime::DateTime::now_local().unwrap();
        let clock = deckcraft_text::datetime::field_text("datetime10", Some("en-US"), now).unwrap();
        let v = s.execute("insert.dateTime", &json!({"format": "datetime10", "language": "en_US"})).unwrap();
        assert_eq!(v["where"], "footer");
        let run = date_run(&s).unwrap();
        assert!(matches!(run.kind, RunKind::Field { ref field } if field == "datetime10"), "{run:?}");
        assert_eq!(run.text, clock);
        assert_eq!(run.props.lang.as_deref(), Some("en-US"));
        s.execute("slide.go", &json!({"index": 1})).unwrap();
        assert!(date_run(&s).is_none(), "the other slide keeps its own footer");
        s.execute("slide.go", &json!({"index": 0})).unwrap();
        let fixed = deckcraft_text::datetime::field_text("datetime12", None, now).unwrap();
        s.execute("insert.dateTime", &json!({"format": "datetime12", "update": false})).unwrap();
        let run = date_run(&s).unwrap();
        assert_eq!((run.kind, run.text), (RunKind::Text, fixed));
        s.execute("edit.undo", &json!({})).unwrap();
        let run = date_run(&s).unwrap();
        assert!(matches!(run.kind, RunKind::Field { .. }), "one undo restores the field");
    }
}

#[cfg(test)]
mod icon_and_picture_tests {
    use deckcraft_model::ShapeKind;
    use deckcraft_model::style::LineJoin;

    use super::*;

    /// 8×8 red PNG.
    fn png() -> Vec<u8> {
        vec![
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 8, 0, 0, 0, 8, 8, 2, 0, 0, 0, 75, 109, 41, 220, 0, 0, 0, 17, 73,
            68, 65, 84, 120, 156, 99, 184, 163, 161, 129, 21, 49, 12, 45, 9, 0, 123, 123, 75, 1, 251, 162, 81, 240, 0, 0, 0, 0, 73, 69, 78, 68, 174,
            66, 96, 130,
        ]
    }

    fn pic(name: &str) -> Value {
        json!({"name": name, "data": base64_encode(&png())})
    }

    fn n_slides(s: &Session) -> usize {
        s.active().map(|d| d.doc.slides.len()).unwrap_or(0)
    }

    fn pictures(s: &Session, index: usize) -> Vec<deckcraft_model::Shape> {
        s.active()
            .and_then(|d| d.doc.slides.get(index))
            .map(|sl| sl.shapes.iter().filter(|sh| matches!(sh.kind, ShapeKind::Picture { .. })).cloned().collect())
            .unwrap_or_default()
    }

    fn box_text(s: &Session, index: usize) -> Vec<String> {
        s.active()
            .and_then(|d| d.doc.slides.get(index))
            .map(|sl| sl.shapes.iter().filter(|sh| sh.text_box).filter_map(|sh| sh.text.as_ref().map(|t| t.text())).collect())
            .unwrap_or_default()
    }

    #[test]
    fn two_pictures_one_per_slide_are_one_undo() {
        let mut s = Session::with_new();
        let before = n_slides(&s);
        let r = s.execute("insert.photoAlbum", &json!({"pictures": [pic("hill.png"), pic("lake.png")], "layout": "1"})).expect("album");
        assert_eq!(r["slides"], 2);
        assert_eq!(n_slides(&s), before + 2);
        assert_eq!(pictures(&s, 1).len(), 1);
        assert_eq!(pictures(&s, 2).len(), 1);
        assert_eq!(s.active().unwrap().selection.slide, 1);
        let undo = &s.active().unwrap().history.undo;
        assert_eq!(undo.len(), 1);
        assert_eq!(undo[0].label, "Photo Album…");
        s.execute("edit.undo", &json!({})).expect("undo");
        assert_eq!(n_slides(&s), before);
        assert!(pictures(&s, 1).is_empty());
    }

    #[test]
    fn two_up_shares_one_slide() {
        let mut s = Session::with_new();
        s.execute("insert.photoAlbum", &json!({"pictures": [pic("a.png"), pic("b.png")], "layout": "2"})).expect("album");
        assert_eq!(n_slides(&s), 2);
        assert_eq!(pictures(&s, 1).len(), 2);
        assert!(box_text(&s, 1).is_empty());
    }

    #[test]
    fn captions_and_titles_use_the_file_stem() {
        let mut s = Session::with_new();
        s.execute("insert.photoAlbum", &json!({"pictures": [pic("harbor.png")], "layout": "1", "captions": true})).expect("captions");
        assert_eq!(box_text(&s, 1), ["harbor"]);
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("insert.photoAlbum", &json!({"pictures": [pic("harbor.png")], "layout": "1title"})).expect("title");
        assert_eq!(box_text(&s, 1), ["harbor"]);
        assert_eq!(pictures(&s, 1).len(), 1);
        let pic_y = pictures(&s, 1)[0].xfrm.unwrap().y;
        assert!(pic_y > 40.0, "the picture sits below the title");
    }

    #[test]
    fn frames_set_the_picture_line() {
        let mut s = Session::with_new();
        s.execute("insert.photoAlbum", &json!({"pictures": [pic("a.png")], "frame": "rect"})).expect("rect");
        let line = pictures(&s, 1)[0].line.clone().expect("line");
        assert_eq!(line.width, Some(1.5));
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("insert.photoAlbum", &json!({"pictures": [pic("a.png")], "frame": "rounded"})).expect("rounded");
        let line = pictures(&s, 1)[0].line.clone().expect("line");
        assert_eq!(line.width, Some(2.25));
        assert_eq!(line.join, Some(LineJoin::Round));
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("insert.photoAlbum", &json!({"pictures": [pic("a.png")], "frame": "simple"})).expect("simple");
        assert_eq!(pictures(&s, 1)[0].line.as_ref().and_then(|l| l.width), Some(6.0));
    }

    #[test]
    fn a_text_box_breaks_the_picture_row() {
        let mut s = Session::with_new();
        let r = s
            .execute("insert.photoAlbum", &json!({"pictures": [pic("a.png"), {"text": "A day on the water"}, pic("b.png")], "layout": "2"}))
            .expect("album");
        assert_eq!(r["slides"], 3);
        assert_eq!(pictures(&s, 1).len(), 1);
        assert_eq!(box_text(&s, 2), ["A day on the water"]);
        assert!(pictures(&s, 2).is_empty());
        assert_eq!(pictures(&s, 3).len(), 1);
        assert_eq!(s.active().unwrap().history.undo.len(), 1);
    }

    #[test]
    fn no_pictures_opens_the_dialog_without_editing() {
        let mut s = Session::with_new();
        let before = n_slides(&s);
        let r = s.execute("insert.photoAlbum", &json!({})).expect("open");
        assert_eq!(r["dialog"], "photoAlbum");
        assert_eq!(n_slides(&s), before);
        assert!(s.active().unwrap().history.undo.is_empty());
        assert_eq!(s.ui_requests.len(), 1);
        match &s.ui_requests[0] {
            crate::UiRequest::Dialog { id, .. } => assert_eq!(id, "photoAlbum"),
            other => panic!("dialog request, got {other:?}"),
        }
    }

    #[test]
    fn bad_input_adds_no_slides() {
        let mut s = Session::with_new();
        let before = n_slides(&s);
        assert!(s.execute("insert.photoAlbum", &json!({"pictures": [], "layout": "1"})).is_err());
        assert!(s.execute("insert.photoAlbum", &json!({"pictures": [pic("a.png")], "layout": "grid"})).is_err());
        assert!(s.execute("insert.photoAlbum", &json!({"pictures": [pic("a.png")], "frame": "glow"})).is_err());
        assert!(s.execute("insert.photoAlbum", &json!({"pictures": [pic("a.png")], "captions": "yes"})).is_err());
        assert!(s.execute("insert.photoAlbum", &json!({"pictures": [{"name": "notes.txt", "data": base64_encode(b"hello")}]} )).is_err());
        assert!(s.execute("insert.photoAlbum", &json!({"pictures": [{"text": "  "}]})).is_err());
        let mut many = Vec::new();
        for i in 0..49 {
            many.push(json!({"text": format!("box {i}")}));
        }
        assert!(s.execute("insert.photoAlbum", &json!({"pictures": many})).is_err());
        assert_eq!(n_slides(&s), before);
        assert!(s.active().unwrap().history.undo.is_empty());
    }

    #[test]
    fn fit_is_larger_than_one_picture_and_four_up_is_one_slide() {
        let mut s = Session::with_new();
        s.execute("insert.photoAlbum", &json!({"pictures": [pic("a.png")], "layout": "1"})).unwrap();
        let one = pictures(&s, 1)[0].xfrm.unwrap().h;
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("insert.photoAlbum", &json!({"pictures": [pic("a.png")], "layout": "fit"})).unwrap();
        let fitted = pictures(&s, 1)[0].xfrm.unwrap().h;
        assert!(fitted > one, "fit {fitted} should be taller than one-up {one}");
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("insert.photoAlbum", &json!({"pictures": [pic("a.png"), pic("b.png"), pic("c.png"), pic("d.png")], "layout": "4"})).unwrap();
        assert_eq!(n_slides(&s), 2);
        assert_eq!(pictures(&s, 1).len(), 4);
    }

    fn adjust(s: &Session, index: usize) -> deckcraft_model::style::PictureAdjust {
        match &pictures(s, index)[0].kind {
            ShapeKind::Picture { fill } => fill.adjust.clone(),
            _ => panic!("picture"),
        }
    }

    fn theme_name(s: &Session) -> String {
        s.active().and_then(|d| d.doc.masters.first()).map(|m| m.theme.name.clone()).unwrap_or_default()
    }

    #[test]
    fn edit_without_pictures_reopens_the_dialog() {
        let mut s = Session::with_new();
        s.execute("insert.photoAlbum", &json!({"pictures": [pic("hill.png"), pic("lake.png")], "layout": "1"})).unwrap();
        s.ui_requests.clear();
        let before = n_slides(&s);
        let undos = s.active().unwrap().history.undo.len();
        let r = s.execute("insert.photoAlbumEdit", &json!({})).expect("reopen");
        assert_eq!(r["dialog"], "photoAlbum");
        assert_eq!(n_slides(&s), before);
        assert_eq!(s.active().unwrap().history.undo.len(), undos);
        let params = match &s.ui_requests[0] {
            crate::UiRequest::Dialog { id, params } => {
                assert_eq!(id, "photoAlbum");
                params
            }
            other => panic!("dialog request, got {other:?}"),
        };
        assert_eq!(params["items"][0]["name"], "hill.png");
        assert_eq!(params["items"][1]["name"], "lake.png");
        assert_eq!(params["layout"], "1");
        assert!(params["album"].as_u64().unwrap_or(0) > 0);
    }

    #[test]
    fn edit_reflows_the_album_in_one_undo() {
        let mut s = Session::with_new();
        s.execute("insert.photoAlbum", &json!({"pictures": [pic("hill.png"), pic("lake.png")], "layout": "1"})).unwrap();
        assert_eq!(n_slides(&s), 3);
        let r = s.execute("insert.photoAlbumEdit", &json!({"pictures": [pic("hill.png"), pic("lake.png")], "layout": "2"})).expect("reflow");
        assert_eq!(r["slides"], 1);
        assert_eq!(n_slides(&s), 2);
        assert_eq!(pictures(&s, 1).len(), 2);
        let undo = &s.active().unwrap().history.undo;
        assert_eq!(undo.last().map(|u| u.label.as_str()), Some("Edit Photo Album…"));
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(n_slides(&s), 3);
        assert_eq!(pictures(&s, 1).len(), 1);
        assert_eq!(pictures(&s, 2).len(), 1);
    }

    #[test]
    fn picture_options_and_theme_are_one_step() {
        let mut s = Session::with_new();
        s.execute(
            "insert.photoAlbum",
            &json!({"pictures": [{"name": "hill.png", "data": base64_encode(&png()), "rot": 90, "brightness": 0.2, "contrast": -0.1}], "grayscale": true, "theme": "Harbor"}),
        )
        .unwrap();
        let adj = adjust(&s, 1);
        assert!(adj.grayscale);
        assert!((adj.brightness - 0.2).abs() < 1e-6);
        assert!((adj.contrast + 0.1).abs() < 1e-6);
        assert!((pictures(&s, 1)[0].xfrm.unwrap().rot - 90.0).abs() < 0.01);
        assert_eq!(theme_name(&s), "Harbor");
        let mark = s.active().unwrap().doc.slides[1].raw_ext.clone().unwrap_or_default();
        assert!(mark.contains("gray=1"), "{mark}");
        assert!(mark.contains("theme=Harbor"), "{mark}");
        assert_eq!(s.active().unwrap().doc.slides[1].name, "Photo Album");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(theme_name(&s), "DeckCraft");
        assert_eq!(n_slides(&s), 1);
    }

    #[test]
    fn unknown_theme_and_a_plain_slide_change_nothing() {
        let mut s = Session::with_new();
        let before = n_slides(&s);
        let err = s.execute("insert.photoAlbum", &json!({"pictures": [pic("a.png")], "theme": "NotATheme"})).unwrap_err();
        assert!(err.to_string().contains("unknown theme"), "{err}");
        assert!(s.execute("insert.photoAlbum", &json!({"pictures": [pic("a.png"), {"brightness": "high"}]})).is_err());
        assert!(s.execute("insert.photoAlbum", &json!({"pictures": [{"name": "a.png", "data": base64_encode(&png()), "brightness": 2}]})).is_err());
        let err = s.execute("insert.photoAlbumEdit", &json!({})).unwrap_err();
        assert!(err.to_string().contains("not part of a photo album"), "{err}");
        s.execute("insert.photoAlbum", &json!({"pictures": [pic("a.png")]})).unwrap();
        s.execute("slide.go", &json!({"index": 0})).unwrap();
        assert!(s.execute("insert.photoAlbumEdit", &json!({"pictures": [pic("b.png")], "layout": "2"})).is_err());
        assert_eq!(n_slides(&s), before + 1);
        assert_eq!(pictures(&s, 1).len(), 1);
        assert_eq!(theme_name(&s), "DeckCraft");
    }

    #[test]
    fn two_albums_stay_independent() {
        let mut s = Session::with_new();
        s.execute("insert.photoAlbum", &json!({"pictures": [pic("a.png"), pic("b.png")], "layout": "1"})).unwrap();
        let last = n_slides(&s) - 1;
        s.execute("slide.go", &json!({"index": last})).unwrap();
        s.execute("insert.photoAlbum", &json!({"pictures": [pic("c.png"), pic("d.png")], "layout": "1"})).unwrap();
        assert_eq!(n_slides(&s), 5);
        s.execute("insert.photoAlbumEdit", &json!({"pictures": [pic("c.png"), pic("d.png")], "layout": "2"})).unwrap();
        assert_eq!(n_slides(&s), 4);
        assert_eq!(pictures(&s, 1).len(), 1);
        assert_eq!(pictures(&s, 2).len(), 1);
        assert_eq!(pictures(&s, 3).len(), 2);
        let first = s.active().unwrap().doc.slides[1].raw_ext.clone().unwrap_or_default();
        let second = s.active().unwrap().doc.slides[3].raw_ext.clone().unwrap_or_default();
        assert!(first.starts_with("deckcraft-album:"));
        assert!(second.starts_with("deckcraft-album:"));
        assert_ne!(first.split(';').next(), second.split(';').next());
    }

    #[test]
    fn a_named_run_without_a_marker_still_reflows() {
        let mut s = Session::with_new();
        s.execute("insert.photoAlbum", &json!({"pictures": [pic("a.png"), pic("b.png")], "layout": "1"})).unwrap();
        {
            let doc = std::sync::Arc::make_mut(&mut s.active_mut().unwrap().doc);
            for sl in doc.slides.iter_mut().skip(1) {
                std::sync::Arc::make_mut(sl).raw_ext = None;
            }
        }
        s.execute("insert.photoAlbumEdit", &json!({"pictures": [pic("a.png"), pic("b.png")], "layout": "2"})).unwrap();
        assert_eq!(n_slides(&s), 2);
        assert_eq!(pictures(&s, 1).len(), 2);
        let mark = s.active().unwrap().doc.slides[1].raw_ext.clone().unwrap_or_default();
        assert!(mark.starts_with("deckcraft-album:"), "{mark}");
    }
}
