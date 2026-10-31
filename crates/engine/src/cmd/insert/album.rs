use super::picture::{content_type, media_bytes};
use super::{NO_ID_SPACE, Result, Session, ShapeKind, Value, bad, base64_encode, json};

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
pub(super) fn photo_album(s: &mut Session, p: &Value) -> Result<Value> {
    if p.get("pictures").is_none_or(Value::is_null) {
        s.ui_requests.push(crate::UiRequest::Dialog { id: "photoAlbum".into(), params: json!({}) });
        return Ok(json!({"dialog": "photoAlbum"}));
    }
    let opts = album_opts(p)?;
    let items = album_items(p)?;
    commit_album(s, items, opts, None)
}

pub(super) fn photo_album_edit(s: &mut Session, p: &Value) -> Result<Value> {
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
