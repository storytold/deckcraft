use super::*;

pub(super) fn master_view(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let mi = usize_param(p, "master").unwrap_or_else(|| master_index(&st.doc, &st.selection));
    // A master thumbnail sends `{master}` and means the master itself. `{master, layout}` selects
    // that layout. With neither key, follow the current slide's layout.
    let layout = if p.get("master").is_some() && p.get("layout").is_none() {
        None
    } else {
        usize_param(p, "layout")
            .or_else(|| st.current_slide().and_then(|sl| st.doc.masters.get(mi).and_then(|m| m.layouts.iter().position(|l| l.id == sl.layout))))
    };
    let target = match layout {
        Some(l) => Target::Layout { master: mi, layout: l },
        None => Target::Master { master: mi },
    };
    s.select(|_, sel| {
        sel.target = target;
        sel.shapes.clear();
        sel.text = None;
    })?;
    Ok(serde_json::to_value(target).unwrap_or_default())
}

pub(super) fn in_handout(s: &Session) -> std::result::Result<(), String> {
    if s.active().is_some_and(|d| d.selection.target == Target::Handout) { Ok(()) } else { Err("open View ▸ Handout Master first".into()) }
}

/// The four handout-master placeholders PowerPoint offers, in a page of `size`.
pub fn handout_placeholder(kind: PhType, size: Size, id: ShapeId) -> Shape {
    let (w, h) = (size.width, size.height);
    let (name, idx, x, y) = match kind {
        PhType::Header => ("Header Placeholder", 0, 0.0, 0.0),
        PhType::Date => ("Date Placeholder", 1, w * 0.57, 0.0),
        PhType::Footer => ("Footer Placeholder", 2, 0.0, h * 0.95),
        _ => ("Slide Number Placeholder", 3, w * 0.57, h * 0.95),
    };
    let mut text = TextBody::default();
    if let Some(p) = text.paragraphs.first_mut()
        && matches!(kind, PhType::Date | PhType::SlideNum)
    {
        p.props.align = Some(deckcraft_model::text::Align::Right);
    }
    Shape {
        id,
        name: format!("{name} {}", idx + 1),
        xfrm: Some(deckcraft_geom::Xfrm::new(x, y, w * 0.43, h * 0.05)),
        ph: Some(deckcraft_model::Placeholder { kind, idx, size: Some("quarter".into()), ..Default::default() }),
        text: Some(text),
        ..Default::default()
    }
}

const HANDOUT_KINDS: [(&str, PhType); 4] =
    [("header", PhType::Header), ("date", PhType::Date), ("footer", PhType::Footer), ("pageNumber", PhType::SlideNum)];

pub(super) fn handout_view(s: &mut Session, _p: &Value) -> Result<Value> {
    let created = s.doc()?.doc.handout_master.is_none();
    if created {
        s.edit(|doc, _| {
            let mut m = deckcraft_model::Master {
                name: "Handout Master".into(),
                theme: doc.masters.first().map(|m| m.theme.clone()).unwrap_or_default(),
                color_map: doc.masters.first().map(|m| m.color_map.clone()).unwrap_or_default(),
                ..Default::default()
            };
            let size = doc.notes_size;
            for (_, k) in HANDOUT_KINDS {
                let id = ShapeId(doc.alloc_id().ok_or_else(|| bad("view.handoutMaster", NO_ID_SPACE))?);
                m.shapes.push(handout_placeholder(k, size, id));
            }
            doc.handout_master = Some(Arc::new(m));
            Ok(())
        })?;
    }
    s.select(|_, sel| {
        sel.target = Target::Handout;
        sel.shapes.clear();
        sel.text = None;
    })?;
    Ok(json!({"created": created}))
}

pub(super) fn handout_placeholders(s: &mut Session, p: &Value) -> Result<Value> {
    s.edit(|doc, _| {
        let size = doc.notes_size;
        let mut ids = vec![];
        for (key, _) in HANDOUT_KINDS {
            let id = match p.get(key).and_then(Value::as_bool) {
                Some(_) => Some(ShapeId(doc.alloc_id().ok_or_else(|| bad("handout.placeholders", NO_ID_SPACE))?)),
                None => None,
            };
            ids.push(id);
        }
        let m = Arc::make_mut(doc.handout_master.as_mut().ok_or_else(|| bad("handout.placeholders", "no handout master"))?);
        for ((key, kind), id) in HANDOUT_KINDS.into_iter().zip(ids) {
            let (Some(on), Some(id)) = (p.get(key).and_then(Value::as_bool), id) else { continue };
            let has = m.shapes.iter().any(|x| x.ph_type() == Some(kind));
            if on && !has {
                m.shapes.push(handout_placeholder(kind, size, id));
            } else if !on {
                m.shapes.retain(|x| x.ph_type() != Some(kind));
            }
        }
        Ok(())
    })?;
    let m = s.doc()?.doc.handout_master.clone();
    let shown = |k: PhType| m.as_ref().is_some_and(|m| m.shapes.iter().any(|x| x.ph_type() == Some(k)));
    Ok(json!({"header": shown(PhType::Header), "date": shown(PhType::Date), "footer": shown(PhType::Footer), "pageNumber": shown(PhType::SlideNum)}))
}

pub(super) fn in_notes_master(s: &Session) -> std::result::Result<(), String> {
    if s.active().is_some_and(|d| d.selection.target == Target::NotesMaster) { Ok(()) } else { Err("open View ▸ Notes Master first".into()) }
}

const NOTES_KINDS: [(&str, PhType); 6] = [
    ("header", PhType::Header),
    ("date", PhType::Date),
    ("slideImage", PhType::SlideImage),
    ("body", PhType::Body),
    ("footer", PhType::Footer),
    ("pageNumber", PhType::SlideNum),
];

/// A notes-master placeholder in a page of `size`, laid out as PowerPoint's default notes master.
pub fn notes_placeholder(kind: PhType, size: Size, id: ShapeId) -> Shape {
    let (w, h) = (size.width, size.height);
    match kind {
        PhType::SlideImage => Shape {
            id,
            name: "Slide Image Placeholder 3".into(),
            xfrm: Some(deckcraft_geom::Xfrm::new(w * 0.125, h * 0.125, w * 0.75, h * 0.375)),
            ph: Some(deckcraft_model::Placeholder { kind, idx: 2, ..Default::default() }),
            fill: Some(Fill::None),
            line: Some(deckcraft_model::Line::solid(deckcraft_model::ColorRef::rgb(deckcraft_color::Rgba::BLACK), 1.0)),
            ..Default::default()
        },
        PhType::Body => {
            let mut text = TextBody::default();
            text.paragraphs = vec![deckcraft_model::text::Paragraph::new("Click to edit Master text styles")];
            Shape {
                id,
                name: "Notes Placeholder 4".into(),
                xfrm: Some(deckcraft_geom::Xfrm::new(w * 0.1, h * 0.53, w * 0.8, h * 0.36)),
                ph: Some(deckcraft_model::Placeholder { kind, idx: 3, size: Some("quarter".into()), ..Default::default() }),
                text: Some(text),
                ..Default::default()
            }
        }
        _ => {
            let mut s = handout_placeholder(kind, size, id);
            if let Some(ph) = s.ph.as_mut() {
                ph.idx = match kind {
                    PhType::Header => 0,
                    PhType::Date => 1,
                    PhType::Footer => 4,
                    _ => 5,
                };
            }
            s
        }
    }
}

pub(super) fn notes_view(s: &mut Session, _p: &Value) -> Result<Value> {
    let created = s.doc()?.doc.notes_master.as_ref().is_none_or(|m| m.shapes.is_empty());
    if created {
        s.edit(|doc, _| {
            let mut m = doc.notes_master.as_deref().cloned().unwrap_or_else(|| deckcraft_model::Master {
                name: "Notes Master".into(),
                theme: doc.masters.first().map(|m| m.theme.clone()).unwrap_or_default(),
                color_map: doc.masters.first().map(|m| m.color_map.clone()).unwrap_or_default(),
                ..Default::default()
            });
            let size = doc.notes_size;
            for (_, k) in NOTES_KINDS {
                let id = ShapeId(doc.alloc_id().ok_or_else(|| bad("view.notesMaster", NO_ID_SPACE))?);
                m.shapes.push(notes_placeholder(k, size, id));
            }
            doc.notes_master = Some(Arc::new(m));
            Ok(())
        })?;
    }
    s.select(|_, sel| {
        sel.target = Target::NotesMaster;
        sel.shapes.clear();
        sel.text = None;
    })?;
    Ok(json!({"created": created}))
}

pub(super) fn notes_placeholders(s: &mut Session, p: &Value) -> Result<Value> {
    s.edit(|doc, _| {
        let size = doc.notes_size;
        let ids: Vec<ShapeId> =
            NOTES_KINDS.iter().map(|_| doc.alloc_id().map(ShapeId).ok_or_else(|| bad("notes.placeholders", NO_ID_SPACE))).collect::<Result<_>>()?;
        let m = Arc::make_mut(doc.notes_master.as_mut().ok_or_else(|| bad("notes.placeholders", "no notes master"))?);
        for ((key, kind), id) in NOTES_KINDS.into_iter().zip(ids) {
            let Some(on) = p.get(key).and_then(Value::as_bool) else { continue };
            let has = m.shapes.iter().any(|x| x.ph_type() == Some(kind));
            if on && !has {
                m.shapes.push(notes_placeholder(kind, size, id));
            } else if !on {
                m.shapes.retain(|x| x.ph_type() != Some(kind));
            }
        }
        Ok(())
    })?;
    let m = s.doc()?.doc.notes_master.clone();
    let mut out = serde_json::Map::new();
    for (key, kind) in NOTES_KINDS {
        out.insert(key.into(), json!(m.as_ref().is_some_and(|m| m.shapes.iter().any(|x| x.ph_type() == Some(kind)))));
    }
    Ok(Value::Object(out))
}

pub(super) fn close_master(s: &mut Session, _p: &Value) -> Result<Value> {
    s.select(|_, sel| {
        sel.target = Target::Slides;
        sel.shapes.clear();
        sel.text = None;
    })?;
    ok()
}

pub(super) fn insert_layout(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").unwrap_or("Custom Layout").to_string();
    s.edit(|doc, sel| {
        let mi = master_index(doc, sel);
        let id = deckcraft_model::LayoutId(doc.alloc_id().ok_or_else(|| bad("master.insertLayout", NO_ID_SPACE))?);
        let tid = ShapeId(doc.alloc_id().ok_or_else(|| bad("master.insertLayout", NO_ID_SPACE))?);
        let size = doc.slide_size;
        let m = Arc::make_mut(doc.masters.get_mut(mi).ok_or_else(|| bad("master.insertLayout", "no master"))?);
        let title = m.shapes.iter().find(|x| x.ph_type() == Some(PhType::Title)).cloned().map(|mut t| {
            t.id = tid;
            t.text = Some(TextBody::default());
            t
        });
        let _ = size;
        let at = match sel.target {
            Target::Layout { layout, .. } => layout + 1,
            _ => m.layouts.len(),
        };
        m.layouts.insert(
            at.min(m.layouts.len()),
            deckcraft_model::Layout {
                id,
                name,
                kind: deckcraft_model::LayoutType::Custom,
                shapes: title.into_iter().collect(),
                background: None,
                show_master_shapes: true,
                preserve: true,
                color_map: None,
                raw_ext: None,
            },
        );
        sel.target = Target::Layout { master: mi, layout: at.min(m.layouts.len().saturating_sub(1)) };
        Ok(json!({"id": id}))
    })
}

fn layout_target(s: &Session, p: &Value, cmd: &str) -> Result<(usize, usize)> {
    let st = s.doc()?;
    let (m, l) = match st.selection.target {
        Target::Layout { master, layout } => (master, layout),
        _ => (master_index(&st.doc, &st.selection), 0),
    };
    Ok((opt_usize_param(p, "master", cmd)?.unwrap_or(m), opt_usize_param(p, "layout", cmd)?.unwrap_or(l)))
}

pub(super) fn rename_layout(s: &mut Session, p: &Value) -> Result<Value> {
    let (mi, li) = layout_target(s, p, "master.renameLayout")?;
    let name = str_param(p, "name").ok_or_else(|| bad("master.renameLayout", "missing `name`"))?.to_string();
    s.edit(|doc, _| {
        let l =
            doc.masters.get_mut(mi).and_then(|m| Arc::make_mut(m).layouts.get_mut(li)).ok_or_else(|| bad("master.renameLayout", "no such layout"))?;
        l.name = name;
        Ok(())
    })?;
    ok()
}

pub(super) fn delete_layout(s: &mut Session, p: &Value) -> Result<Value> {
    let (mi, li) = layout_target(s, p, "master.deleteLayout")?;
    s.edit(|doc, sel| {
        let id = doc.masters.get(mi).and_then(|m| m.layouts.get(li)).map(|l| l.id).ok_or_else(|| bad("master.deleteLayout", "no such layout"))?;
        if doc.slides.iter().any(|x| x.layout == id) {
            return Err(bad("master.deleteLayout", "slides use this layout"));
        }
        let m = Arc::make_mut(doc.masters.get_mut(mi).ok_or_else(|| bad("master.deleteLayout", "no master"))?);
        if m.layouts.len() <= 1 {
            return Err(bad("master.deleteLayout", "a master needs at least one layout"));
        }
        m.layouts.remove(li);
        sel.target = Target::Layout { master: mi, layout: li.min(m.layouts.len().saturating_sub(1)) };
        Ok(())
    })?;
    ok()
}

pub(super) fn insert_placeholder(s: &mut Session, p: &Value) -> Result<Value> {
    let kind = match str_param(p, "kind").unwrap_or("content") {
        "text" => PhType::Body,
        "picture" => PhType::Picture,
        "chart" => PhType::Chart,
        "table" => PhType::Table,
        "media" => PhType::Media,
        _ => PhType::Obj,
    };
    if !matches!(s.doc()?.selection.target, Target::Layout { .. }) {
        return Err(bad("master.insertPlaceholder", "select a layout in Slide Master view"));
    }
    let size = s.doc()?.doc.slide_size;
    let rect = rect_param(p, "rect").unwrap_or(deckcraft_geom::Xfrm::new(size.width * 0.25, size.height * 0.3, size.width * 0.5, size.height * 0.4));
    let idx = s.doc()?.shapes().iter().filter_map(|x| x.ph.as_ref().map(|p| p.idx)).max().unwrap_or(0) + 1;
    let shape = Shape {
        xfrm: Some(rect),
        ph: Some(deckcraft_model::Placeholder { kind, idx, ..Default::default() }),
        text: Some(TextBody::default()),
        ..Default::default()
    };
    let id = crate::cmd::insert::add_shape(s, "master.insertPlaceholder", shape, true)?;
    Ok(json!({"id": id}))
}

pub(super) fn preserve(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let (on_master, mi, li, current) = match st.selection.target {
        Target::Master { master } => {
            let on = st.doc.masters.get(master).map(|m| m.preserve).ok_or_else(|| bad("master.preserve", "no such master"))?;
            (true, master, 0, on)
        }
        Target::Layout { master, layout } => {
            let on = st
                .doc
                .masters
                .get(master)
                .and_then(|m| m.layouts.get(layout))
                .map(|l| l.preserve)
                .ok_or_else(|| bad("master.preserve", "no such layout"))?;
            (false, master, layout, on)
        }
        Target::Slides | Target::Handout | Target::NotesMaster => return Err(bad("master.preserve", "open Slide Master view")),
    };
    let next = bool_param(p, "preserve").unwrap_or(!current);
    if next == current {
        return Ok(json!({"preserve": next}));
    }
    s.edit(|doc, _| {
        if on_master {
            let m = Arc::make_mut(doc.masters.get_mut(mi).ok_or_else(|| bad("master.preserve", "no such master"))?);
            m.preserve = next;
        } else {
            let l =
                doc.masters.get_mut(mi).and_then(|m| Arc::make_mut(m).layouts.get_mut(li)).ok_or_else(|| bad("master.preserve", "no such layout"))?;
            l.preserve = next;
        }
        Ok(())
    })?;
    Ok(json!({"preserve": next}))
}

pub(super) fn placeholders(s: &mut Session, p: &Value) -> Result<Value> {
    let title = bool_param(p, "title");
    let footers = bool_param(p, "footers");
    if title.is_none() && footers.is_none() {
        return Err(bad("master.placeholders", "give title or footers"));
    }
    let st = s.doc()?;
    if matches!(st.selection.target, Target::Slides) {
        return Err(bad("master.placeholders", "open Slide Master view"));
    }
    let has_title = st.shapes().iter().any(|sh| sh.ph_type().is_some_and(PhType::is_title));
    let has_footers = st.shapes().iter().any(|sh| sh.ph_type().is_some_and(PhType::is_footer_kind));
    let title_changes = title.is_some_and(|want| want != has_title);
    let footer_changes = footers.is_some_and(|want| want != has_footers);
    if !title_changes && !footer_changes {
        return Ok(json!({"title": has_title, "footers": has_footers}));
    }
    let mut add = Vec::new();
    if title == Some(true) && !has_title {
        add.push(PhType::Title);
    }
    if footers == Some(true) && !has_footers {
        add.extend([PhType::Date, PhType::Footer, PhType::SlideNum]);
    }
    let size = st.doc.slide_size;
    s.edit(|doc, sel| {
        let mut ids = Vec::with_capacity(add.len());
        for _ in 0..add.len() {
            ids.push(ShapeId(doc.alloc_id().ok_or_else(|| bad("master.placeholders", NO_ID_SPACE))?));
        }
        let prototypes = match sel.target {
            Target::Master { master } | Target::Layout { master, .. } => doc.masters.get(master).map(|m| m.shapes.clone()).unwrap_or_default(),
            Target::Slides => Vec::new(),
            Target::Handout | Target::NotesMaster => return Err(bad("master.placeholders", "open Slide Master view")),
        };
        let shapes = crate::shapes_mut(doc, sel).ok_or_else(|| bad("master.placeholders", "open Slide Master view"))?;
        if title == Some(false) {
            shapes.retain(|sh| !sh.ph_type().is_some_and(PhType::is_title));
        }
        if footers == Some(false) {
            shapes.retain(|sh| !sh.ph_type().is_some_and(PhType::is_footer_kind));
        }
        for (kind, id) in add.into_iter().zip(ids) {
            let idx = free_ph_idx(shapes, preferred_ph_idx(kind));
            let proto = prototypes.iter().find(|sh| match kind {
                PhType::Title => sh.ph_type().is_some_and(PhType::is_title),
                other => sh.ph_type() == Some(other),
            });
            let shape = match proto {
                Some(src) => retarget_placeholder(src.clone(), id, idx),
                None => synthesized_placeholder(kind, id, idx, size),
            };
            shapes.push(shape);
        }
        Ok(())
    })?;
    let st = s.doc()?;
    let title = st.shapes().iter().any(|sh| sh.ph_type().is_some_and(PhType::is_title));
    let footers = st.shapes().iter().any(|sh| sh.ph_type().is_some_and(PhType::is_footer_kind));
    Ok(json!({"title": title, "footers": footers}))
}

fn preferred_ph_idx(kind: PhType) -> u32 {
    match kind {
        PhType::Title | PhType::CtrTitle => 0,
        PhType::Date => 10,
        PhType::Footer => 11,
        PhType::SlideNum => 12,
        _ => 1,
    }
}

fn free_ph_idx(shapes: &[Shape], preferred: u32) -> u32 {
    let taken = |idx: u32| shapes.iter().any(|sh| sh.ph.as_ref().is_some_and(|p| p.idx == idx));
    if !taken(preferred) { preferred } else { shapes.iter().filter_map(|sh| sh.ph.as_ref().map(|p| p.idx)).max().unwrap_or(0).saturating_add(1) }
}

/// A copy of a master placeholder, with a new id and index. Prompt text is cleared; a slide
/// number keeps its `‹#›` field.
fn retarget_placeholder(mut sh: Shape, id: ShapeId, idx: u32) -> Shape {
    sh.id = id;
    if let Some(ph) = sh.ph.as_mut() {
        ph.idx = idx;
    }
    let slide_num = sh.ph_type() == Some(PhType::SlideNum);
    if slide_num {
        let paras = vec![slide_number_paragraph()];
        if let Some(text) = sh.text.as_mut() {
            text.paragraphs = paras;
        } else {
            sh.text = Some(TextBody { paragraphs: paras, ..Default::default() });
        }
    } else if let Some(text) = sh.text.as_mut() {
        text.paragraphs.clear();
    }
    sh
}

fn synthesized_placeholder(kind: PhType, id: ShapeId, idx: u32, size: Size) -> Shape {
    let (name, text) = match kind {
        PhType::Date => ("Date Placeholder", TextBody::default()),
        PhType::Footer => ("Footer Placeholder", TextBody::default()),
        PhType::SlideNum => ("Slide Number Placeholder", TextBody { paragraphs: vec![slide_number_paragraph()], ..Default::default() }),
        _ => ("Title", TextBody::default()),
    };
    Shape {
        id,
        name: name.into(),
        xfrm: Some(placeholder_rect(kind, size)),
        ph: Some(deckcraft_model::Placeholder { kind, idx, ..Default::default() }),
        text: Some(text),
        ..Default::default()
    }
}

/// Standard placeholder boxes at 960×540, scaled to the slide size.
fn placeholder_rect(kind: PhType, size: Size) -> deckcraft_geom::Xfrm {
    let sx = size.width / 960.0;
    let sy = size.height / 540.0;
    let (x, y, w, h) = match kind {
        PhType::Date => (60.0, 500.0, 216.0, 28.0),
        PhType::Footer => (318.0, 500.0, 324.0, 28.0),
        PhType::SlideNum => (684.0, 500.0, 216.0, 28.0),
        _ => (60.0, 29.0, 840.0, 104.0),
    };
    deckcraft_geom::Xfrm::new(x * sx, y * sy, w * sx, h * sy)
}

fn slide_number_paragraph() -> deckcraft_model::Paragraph {
    deckcraft_model::Paragraph {
        runs: vec![Run { text: "‹#›".into(), props: Default::default(), kind: RunKind::Field { field: "slidenum".into() } }],
        ..Default::default()
    }
}
