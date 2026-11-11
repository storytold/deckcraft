//! Home ▸ New Slide ▸ Reuse Slides: insert slides from another presentation.
//!
//! The slides take the destination's look by default: each one goes on the destination layout that
//! has the same placeholders (type and idx), else a copy of its own layout joins the destination's
//! master, as PowerPoint does (named `1_L2` when `L2` is taken); with `keepFormatting` the source
//! master and its layouts come along. Pictures, media and sounds are copied under fresh ids, and shape ids are
//! renumbered with animations, triggers and connector ends following them. Links to another
//! reused slide follow its copy.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use deckcraft_model::table::TableStyleDef;
use deckcraft_model::text::{Action, Hyperlink, ListStyle, RunProps, Spacing, TextBody};
use deckcraft_model::{Layout, LayoutId, Master, MasterId, MediaId, Presentation, ShapeId, ShapeKind, Slide, SlideId};
use serde_json::{Value, json};

use super::{CommandSpec, NO_ID_SPACE, bad, bool_or, cmd, has_doc, usize_param};
use crate::{Result, Session, media_refs};

/// Scale physical measurements in imported content to the destination slide's coordinate space.
/// Text has one size, so it uses the geometric mean of the horizontal and vertical scale.
fn scale_text(t: &mut TextBody, sx: f64, sy: f64) {
    let unit = (sx * sy).sqrt();
    for v in [&mut t.body.inset_l, &mut t.body.inset_r, &mut t.body.col_spacing].into_iter().flatten() {
        *v *= sx;
    }
    for v in [&mut t.body.inset_t, &mut t.body.inset_b].into_iter().flatten() {
        *v *= sy;
    }
    scale_list(&mut t.list_style, sx, unit);
    for p in &mut t.paragraphs {
        for v in [&mut p.props.margin_left, &mut p.props.margin_right, &mut p.props.indent, &mut p.props.default_tab].into_iter().flatten() {
            *v *= sx;
        }
        if let Some(stops) = &mut p.props.tabs {
            for stop in stops {
                stop.pos *= sx;
            }
        }
        for spacing in [&mut p.props.line_spacing, &mut p.props.space_before, &mut p.props.space_after] {
            if let Some(Spacing::Pts(v)) = spacing {
                *v *= unit;
            }
        }
        if let Some(v) = &mut p.props.bullet_size_points {
            *v *= unit;
        }
        scale_run(&mut p.def_props, unit);
        scale_run(&mut p.end_props, unit);
        for r in &mut p.runs {
            scale_run(&mut r.props, unit);
        }
    }
}

fn scale_list(list: &mut ListStyle, sx: f64, unit: f64) {
    for level in list.levels.iter_mut().flatten() {
        for v in
            [&mut level.para.margin_left, &mut level.para.margin_right, &mut level.para.indent, &mut level.para.default_tab].into_iter().flatten()
        {
            *v *= sx;
        }
        for spacing in [&mut level.para.line_spacing, &mut level.para.space_before, &mut level.para.space_after] {
            if let Some(Spacing::Pts(v)) = spacing {
                *v *= unit;
            }
        }
        if let Some(v) = &mut level.para.bullet_size_points {
            *v *= unit;
        }
        scale_run(&mut level.run, unit);
    }
}

fn scale_run(run: &mut RunProps, factor: f64) {
    for v in [&mut run.size, &mut run.spacing, &mut run.kern, &mut run.soft_edge].into_iter().flatten() {
        *v *= factor;
    }
}

fn scale_shapes(shapes: &mut [deckcraft_model::Shape], sx: f64, sy: f64, depth: usize) {
    if depth > 64 {
        return;
    }
    let unit = (sx * sy).sqrt();
    for shape in shapes {
        if let Some(x) = &mut shape.xfrm {
            x.x *= sx;
            x.w *= sx;
            x.y *= sy;
            x.h *= sy;
        }
        if let Some(text) = &mut shape.text {
            scale_text(text, sx, sy);
        }
        match &mut shape.kind {
            ShapeKind::Table(table) => {
                for col in &mut table.cols {
                    *col *= sx;
                }
                for row in &mut table.rows {
                    row.height *= sy;
                    for cell in &mut row.cells {
                        if let Some(margins) = &mut cell.margins {
                            if let Some(v) = margins.get_mut(0) {
                                *v *= sx;
                            }
                            if let Some(v) = margins.get_mut(1) {
                                *v *= sx;
                            }
                            if let Some(v) = margins.get_mut(2) {
                                *v *= sy;
                            }
                            if let Some(v) = margins.get_mut(3) {
                                *v *= sy;
                            }
                        }
                        scale_text(&mut cell.text, sx, sy);
                    }
                }
            }
            ShapeKind::Group { children, child } => {
                child.x *= sx;
                child.w *= sx;
                child.y *= sy;
                child.h *= sy;
                scale_shapes(children, sx, sy, depth + 1);
            }
            _ => {}
        }
        if let Some(effects) = &mut shape.effects {
            for shadow in [&mut effects.outer_shadow, &mut effects.inner_shadow].into_iter().flatten() {
                shadow.blur *= unit;
                shadow.dist *= unit;
            }
            if let Some(glow) = &mut effects.glow {
                glow.radius *= unit;
            }
            if let Some(reflection) = &mut effects.reflection {
                reflection.blur *= unit;
                reflection.dist *= unit;
            }
        }
    }
}

fn scale_master(master: &mut Master, sx: f64, sy: f64) {
    scale_shapes(&mut master.shapes, sx, sy, 0);
    let unit = (sx * sy).sqrt();
    scale_list(&mut master.title_style, sx, unit);
    scale_list(&mut master.body_style, sx, unit);
    scale_list(&mut master.other_style, sx, unit);
    for layout in &mut master.layouts {
        scale_shapes(&mut layout.shapes, sx, sy, 0);
    }
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "slide.reuseList",
            "List Slides to Reuse",
            [],
            None,
            "{path | data: base64, name?} → {name, slides: [{index, title, hidden}]}: the slides of another presentation, for choosing which ones `slide.reuse` brings in",
            has_doc,
            reuse_list
        ),
        cmd!(
            "slide.reuse",
            "Reuse Slides…",
            ["Home", "Slides", "New Slide"],
            None,
            "{path? | data: base64, name?, slides?: [index] (0-based, default all), keepFormatting?: bool (default false: use the destination theme), at?: index (default after the current slide)}",
            has_doc,
            reuse
        ),
    ]
}

fn reuse_list(_s: &mut Session, p: &Value) -> Result<Value> {
    let (name, bytes) = super::insert::media_bytes(p, "slide.reuseList")?;
    let src = super::file::open_presentation(&name, &bytes)?;
    let slides: Vec<Value> =
        src.slides.iter().enumerate().map(|(index, sl)| json!({"index": index, "title": sl.title(), "hidden": sl.hidden})).collect();
    Ok(json!({"name": name, "slides": slides}))
}

/// Point every media id under `x` at its copy in the destination (`map`: source → destination).
fn remap(map: &HashMap<MediaId, MediaId>) -> impl FnMut(&mut MediaId) + '_ {
    move |id: &mut MediaId| {
        if let Some(to) = map.get(id) {
            *id = *to;
        }
    }
}

fn shape_ids(shapes: &[deckcraft_model::Shape]) -> Vec<ShapeId> {
    let mut v = vec![];
    deckcraft_model::walk(shapes, &mut |sh, _| v.push(sh.id));
    v
}

/// Point connector ends at the renumbered shapes.
fn remap_connectors(shapes: &mut [deckcraft_model::Shape], map: &HashMap<ShapeId, ShapeId>, depth: usize) {
    if depth > 64 {
        return;
    }
    for sh in shapes {
        if let ShapeKind::Connector { start, end } = &mut sh.kind {
            for c in [start, end] {
                if let Some((id, _)) = c.as_mut() {
                    *id = map.get(id).copied().unwrap_or(*id);
                }
            }
        }
        if let Some(ch) = sh.children_mut() {
            remap_connectors(ch, map, depth + 1);
        }
    }
}

/// Point a link to a reused slide at its copy (`map`: source → destination).
fn relink(h: &mut Option<Hyperlink>, map: &HashMap<SlideId, SlideId>) {
    let Some(h) = h else { return };
    if let Action::Slide { slide } = &mut h.action {
        *slide = map.get(slide).copied().unwrap_or(*slide);
    }
    if let Some(z) = &mut h.zoom_return {
        *z = map.get(z).copied().unwrap_or(*z);
    }
}

fn relink_run(r: &mut RunProps, map: &HashMap<SlideId, SlideId>) {
    relink(&mut r.link, map);
    relink(&mut r.hover, map);
}

fn relink_text(t: &mut TextBody, map: &HashMap<SlideId, SlideId>) {
    for lv in t.list_style.levels.iter_mut().flatten() {
        relink_run(&mut lv.run, map);
    }
    for p in &mut t.paragraphs {
        relink_run(&mut p.def_props, map);
        relink_run(&mut p.end_props, map);
        for r in &mut p.runs {
            relink_run(&mut r.props, map);
        }
    }
}

/// Point the slide links under `shapes` (click and hover actions, text links) at the copies.
fn relink_shapes(shapes: &mut [deckcraft_model::Shape], map: &HashMap<SlideId, SlideId>, depth: usize) {
    if depth > 64 {
        return;
    }
    for sh in shapes {
        relink(&mut sh.click, map);
        relink(&mut sh.hover, map);
        if let Some(t) = &mut sh.text {
            relink_text(t, map);
        }
        if let ShapeKind::Table(t) = &mut sh.kind {
            for c in t.rows.iter_mut().flat_map(|r| r.cells.iter_mut()) {
                relink_text(&mut c.text, map);
            }
        }
        if let Some(ch) = sh.children_mut() {
            relink_shapes(ch, map, depth + 1);
        }
    }
}

/// Fresh shape ids for `shapes`, and the old → new map.
fn renumber(cmd: &str, doc: &mut Presentation, shapes: &mut [deckcraft_model::Shape]) -> Result<HashMap<ShapeId, ShapeId>> {
    let old = shape_ids(shapes);
    super::edit::reid(cmd, doc, shapes)?;
    let map: HashMap<ShapeId, ShapeId> = old.into_iter().zip(shape_ids(shapes)).collect();
    remap_connectors(shapes, &map, 0);
    Ok(map)
}

/// What PowerPoint compares when a slide comes into another presentation: the type and idx of each
/// placeholder of its layout, not the layout's name or where its placeholders sit.
fn placeholder_signature(l: &Layout) -> Vec<String> {
    let mut v: Vec<String> = l.shapes.iter().filter_map(|s| s.ph.as_ref()).map(|p| format!("{:?}/{}", p.kind, p.idx)).collect();
    v.sort();
    v
}

/// The layout of `m` with the same placeholders as `like`, if it has one.
pub(crate) fn find_layout(m: &Master, like: &Layout) -> Option<LayoutId> {
    let want = placeholder_signature(like);
    m.layouts.iter().find(|l| placeholder_signature(l) == want).map(|l| l.id)
}

/// `name` if `m` has no layout of that name, else `1_name`, `2_name`…
fn free_layout_name(m: &Master, name: &str) -> String {
    let taken = |n: &str| m.layouts.iter().any(|l| l.name.eq_ignore_ascii_case(n));
    if !taken(name) {
        return name.to_string();
    }
    (1u32..).map(|k| format!("{k}_{name}")).find(|n| !taken(n)).unwrap_or_else(|| name.to_string())
}

/// The index of the master the slide before `at` is on (the first when there is none).
pub(crate) fn master_before(doc: &Presentation, at: usize) -> usize {
    at.checked_sub(1).and_then(|i| doc.slides.get(i)).and_then(|s| doc.masters.iter().position(|m| m.layout(s.layout).is_some())).unwrap_or(0)
}

/// A copy of `l` joins master `mi` of `doc` under a free name, with fresh ids, its shapes scaled by
/// `scale` and its media ids through `remap_media`. The caller has brought the media in.
pub(crate) fn import_layout(
    cmd: &str,
    doc: &mut Presentation,
    mi: usize,
    l: &Layout,
    scale: (f64, f64),
    mut remap_media: impl FnMut(&mut MediaId),
) -> Result<LayoutId> {
    let mut copy = l.clone();
    copy.id = LayoutId(doc.alloc_id().ok_or_else(|| bad(cmd, NO_ID_SPACE))?);
    media_refs::visit_mut::layout(&mut copy, &mut remap_media);
    renumber(cmd, doc, &mut copy.shapes)?;
    scale_shapes(&mut copy.shapes, scale.0, scale.1, 0);
    let master = doc.masters.get_mut(mi).ok_or_else(|| bad(cmd, "there is no master to bring the layout into"))?;
    let master = Arc::make_mut(master);
    copy.name = free_layout_name(master, &l.name);
    let id = copy.id;
    master.layouts.push(copy);
    Ok(id)
}

/// The destination layout a reused slide moves to: same name, else same kind, else the first.
fn matching_layout(doc: &Presentation, name: &str, kind: deckcraft_model::LayoutType) -> LayoutId {
    let all = || doc.masters.iter().flat_map(|m| m.layouts.iter());
    all()
        .find(|l| !name.is_empty() && l.name.eq_ignore_ascii_case(name))
        .or_else(|| all().find(|l| l.kind == kind))
        .or_else(|| all().next())
        .map(|l| l.id)
        .unwrap_or_default()
}

fn collect_table_styles(shapes: &[deckcraft_model::Shape], styles: &mut Vec<String>, seen: &mut HashSet<String>, depth: usize) {
    if depth > 64 {
        return;
    }
    for shape in shapes {
        if let ShapeKind::Table(table) = &shape.kind
            && seen.insert(table.style.to_ascii_lowercase())
        {
            styles.push(table.style.clone());
        }
        collect_table_styles(shape.children(), styles, seen, depth + 1);
    }
}

fn remap_table_styles(shapes: &mut [deckcraft_model::Shape], styles: &HashMap<String, String>, depth: usize) {
    if depth > 64 {
        return;
    }
    for shape in shapes {
        if let ShapeKind::Table(table) = &mut shape.kind
            && let Some(style) = styles.get(&table.style.to_ascii_lowercase())
        {
            table.style = style.clone();
        }
        if let Some(children) = shape.children_mut() {
            remap_table_styles(children, styles, depth + 1);
        }
    }
}

fn same_table_style(a: &TableStyleDef, b: &TableStyleDef) -> bool {
    a.name == b.name && a.parts == b.parts && a.background == b.background && a.raw == b.raw
}

/// Copy only definitions used on selected slides. Conflicting GUIDs receive a fresh GUID so
/// existing destination tables continue to resolve to their original definitions.
fn import_table_styles(cmd: &str, doc: &mut Presentation, src: &Presentation, used: &[String]) -> Result<HashMap<String, String>> {
    let mut map = HashMap::new();
    let mut source_defs = HashMap::new();
    let mut source_ids = HashSet::new();
    for (index, style) in src.table_styles.iter().enumerate() {
        let key = style.id.to_ascii_lowercase();
        source_defs.entry(key.clone()).or_insert(index);
        source_ids.insert(key);
    }
    let mut destination_defs = HashMap::new();
    for (index, style) in doc.table_styles.iter().enumerate() {
        destination_defs.entry(style.id.to_ascii_lowercase()).or_insert(index);
    }
    for id in used {
        let key = id.to_ascii_lowercase();
        if map.contains_key(&key) {
            continue;
        }
        let Some(source_index) = source_defs.get(&key).copied() else { continue };
        let Some(source) = src.table_styles.get(source_index) else { continue };
        let destination = destination_defs.get(&key).and_then(|index| doc.table_styles.get(*index));
        let mut imported = source.clone();
        if let Some(existing) = destination {
            if same_table_style(existing, source) {
                map.insert(key, existing.id.clone());
                continue;
            }
            let candidate = loop {
                let n = doc.alloc_id().ok_or_else(|| bad(cmd, NO_ID_SPACE))?;
                let candidate = format!("{{00000000-0000-4000-8000-{n:012X}}}");
                let key = candidate.to_ascii_lowercase();
                if !destination_defs.contains_key(&key) && !source_ids.contains(&key) {
                    break candidate;
                }
            };
            // The definition's own XML names it too, and PPTX export writes that XML as is.
            imported.raw = imported.raw.replacen(&format!("styleId=\"{}\"", source.id), &format!("styleId=\"{candidate}\""), 1);
            imported.id.clone_from(&candidate);
            map.insert(key, candidate);
        } else {
            map.insert(key, imported.id.clone());
        }
        destination_defs.insert(imported.id.to_ascii_lowercase(), doc.table_styles.len());
        doc.table_styles.push(imported);
    }
    Ok(map)
}

fn reuse(s: &mut Session, p: &Value) -> Result<Value> {
    let (name, bytes) = super::insert::media_bytes(p, "slide.reuse")?;
    let src = super::file::open_presentation(&name, &bytes)?;
    let n = src.slides.len();
    let picked: Vec<usize> = match p.get("slides") {
        Some(Value::Array(a)) => {
            let mut v = vec![];
            for x in a {
                let i = x.as_u64().map(|i| i as usize).filter(|i| *i < n);
                let i = i.ok_or_else(|| bad("slide.reuse", format!("{name} has {n} slides; {x} isn't one of them (indices count from 0)")))?;
                if !v.contains(&i) {
                    v.push(i);
                }
            }
            v
        }
        Some(Value::String(t)) if !t.trim().is_empty() => super::file::parse_slide_range(t, n)?,
        _ => (0..n).collect(),
    };
    if picked.is_empty() {
        return Err(bad("slide.reuse", format!("{name} has no slides to reuse")));
    }
    let keep = bool_or(p, "keepFormatting", false);
    let at = usize_param(p, "at");
    let source_size = src.slide_size;
    s.edit(|doc, sel| {
        let at = at.unwrap_or(sel.slide + 1).min(doc.slides.len());
        let mut used_table_styles = Vec::new();
        let mut seen_table_styles = HashSet::new();
        for &i in &picked {
            if let Some(slide) = src.slides.get(i) {
                collect_table_styles(&slide.shapes, &mut used_table_styles, &mut seen_table_styles, 0);
            }
        }
        let table_style_map = import_table_styles("slide.reuse", doc, &src, &used_table_styles)?;
        let scale = |source: f64, destination: f64| {
            let ratio = destination / source;
            if source > 0.0 && destination > 0.0 && ratio.is_finite() && ratio > 0.0 { ratio } else { 1.0 }
        };
        let sx = scale(source_size.width, doc.slide_size.width);
        let sy = scale(source_size.height, doc.slide_size.height);
        // Media: what the slides use is copied under fresh ids.
        let mut media_map: HashMap<MediaId, MediaId> = HashMap::new();
        // Keep Source Formatting: bring each source master used, once, with fresh ids.
        let mut layout_map: HashMap<LayoutId, LayoutId> = HashMap::new();
        if keep {
            let mut masters_done: Vec<MasterId> = vec![];
            for &i in &picked {
                let Some((m, _)) = src.slides.get(i).and_then(|sl| src.master_for(sl)) else { continue };
                if masters_done.contains(&m.id) {
                    continue;
                }
                masters_done.push(m.id);
                let mut m = import_master("slide.reuse", doc, &src, m, &mut media_map, &mut layout_map)?;
                scale_master(&mut m, sx, sy);
                doc.masters.push(Arc::new(m));
            }
        }
        // Source layout → the destination layout its slides take (found or brought along).
        let mut brought: HashMap<LayoutId, LayoutId> = HashMap::new();
        let mut copies: Vec<Slide> = vec![];
        // Source slide → its copy, so links between reused slides follow (forward ones too).
        let mut slide_map: HashMap<SlideId, SlideId> = HashMap::new();
        for &i in &picked {
            let Some(from) = src.slides.get(i) else { continue };
            copy_media("slide.reuse", doc, &src, &media_refs::slide_media(from), &mut media_map)?;
            let mut sl: Slide = from.as_ref().clone();
            remap_table_styles(&mut sl.shapes, &table_style_map, 0);
            scale_shapes(&mut sl.shapes, sx, sy, 0);
            media_refs::visit_mut::slide(&mut sl, &mut remap(&media_map));
            sl.id = SlideId(doc.alloc_id().ok_or_else(|| bad("slide.reuse", NO_ID_SPACE))?);
            let map = renumber("slide.reuse", doc, &mut sl.shapes)?;
            for a in &mut sl.animations {
                a.shape = map.get(&a.shape).copied().unwrap_or(a.shape);
                a.trigger = a.trigger.map(|t| map.get(&t).copied().unwrap_or(t));
            }
            sl.layout = match layout_map.get(&sl.layout) {
                Some(l) => *l,
                None => match (brought.get(&from.layout), src.layout(from.layout)) {
                    (Some(l), _) => *l,
                    (None, Some((_, src_layout))) => {
                        let mi = master_before(doc, at);
                        let id = match doc.masters.get(mi).and_then(|m| find_layout(m, src_layout)) {
                            Some(id) => id,
                            None => {
                                let mut used = vec![];
                                media_refs::visit::layout(src_layout, &mut |id: &MediaId| used.push(*id));
                                copy_media("slide.reuse", doc, &src, &used, &mut media_map)?;
                                import_layout("slide.reuse", doc, mi, src_layout, (sx, sy), remap(&media_map))?
                            }
                        };
                        brought.insert(from.layout, id);
                        id
                    }
                    (None, None) => matching_layout(doc, "", Default::default()),
                },
            };
            slide_map.insert(from.id, sl.id);
            copies.push(sl);
        }
        let mut new_ids = vec![];
        for (k, mut sl) in copies.into_iter().enumerate() {
            relink_shapes(&mut sl.shapes, &slide_map, 0);
            relink_text(&mut sl.notes, &slide_map);
            new_ids.push(sl.id);
            doc.slides.insert((at + k).min(doc.slides.len()), Arc::new(sl));
        }
        sel.slide = (at + new_ids.len()).saturating_sub(1).min(doc.slides.len().saturating_sub(1));
        sel.slides = new_ids.clone();
        sel.shapes.clear();
        sel.text = None;
        Ok(json!({"slides": new_ids.len(), "ids": new_ids, "from": name, "keepFormatting": keep}))
    })
}

/// A copy of `src`'s master `m` ready to join `doc`: its media copied in, fresh master, layout and
/// shape ids. `layouts` gains each source layout id → its copy's.
pub(crate) fn import_master(
    cmd: &str,
    doc: &mut Presentation,
    src: &Presentation,
    m: &Master,
    media_map: &mut HashMap<MediaId, MediaId>,
    layouts: &mut HashMap<LayoutId, LayoutId>,
) -> Result<Master> {
    let mut used = vec![];
    media_refs::visit::master(m, &mut |id: &MediaId| used.push(*id));
    copy_media(cmd, doc, src, &used, media_map)?;
    let mut m = m.clone();
    media_refs::visit_mut::master(&mut m, &mut remap(media_map));
    m.id = MasterId(doc.alloc_id().ok_or_else(|| bad(cmd, NO_ID_SPACE))?);
    renumber(cmd, doc, &mut m.shapes)?;
    for l in &mut m.layouts {
        let new = LayoutId(doc.alloc_id().ok_or_else(|| bad(cmd, NO_ID_SPACE))?);
        layouts.insert(l.id, new);
        l.id = new;
        renumber(cmd, doc, &mut l.shapes)?;
    }
    Ok(m)
}

/// Copy the source media `used` into `doc` under fresh ids, once each (identical bytes already in
/// `doc` are shared; a linked item keeps its link).
fn copy_media(cmd: &str, doc: &mut Presentation, src: &Presentation, used: &[MediaId], map: &mut HashMap<MediaId, MediaId>) -> Result<()> {
    for &id in used {
        if map.contains_key(&id) {
            continue;
        }
        let Some(m) = src.media(id) else { continue };
        let new = if m.link.is_some() {
            let mut item = m.clone();
            item.id = MediaId(doc.alloc_id().ok_or_else(|| bad(cmd, NO_ID_SPACE))?);
            let new = item.id;
            doc.media.push(item);
            new
        } else {
            doc.add_media(&m.name, &m.content_type, m.data.as_ref().clone()).ok_or_else(|| bad(cmd, NO_ID_SPACE))?
        };
        map.insert(id, new);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use deckcraft_geom::{Size, Xfrm};
    use deckcraft_model::style::{Artistic, Fill, PictureFill};
    use deckcraft_model::table::TableStyleDef;
    use deckcraft_model::text::{Action, Bullet, Hyperlink, TextBody};
    use deckcraft_model::{Background, MediaClip, Shape, ShapeKind, Table, defaults};
    use serde_json::json;

    use super::*;

    const CUSTOM_STYLE: &str = "{11111111-2222-4333-8444-555555555555}";

    fn styled_table_presentation(id: &str, color: &str) -> Presentation {
        let mut p = deckcraft_model::defaults::new_presentation(None);
        let raw = format!(
            r#"<a:tblStyle xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" styleId="{id}" styleName="Owned"><a:wholeTbl><a:tcStyle><a:fill><a:solidFill><a:srgbClr val="{color}"/></a:solidFill></a:fill></a:tcStyle></a:wholeTbl></a:tblStyle>"#
        );
        p.table_styles.push(TableStyleDef { id: id.into(), name: "Owned".into(), raw, ..Default::default() });
        let table = deckcraft_model::Table { style: id.into(), ..deckcraft_model::Table::new(1, 1, 100.0, 24.0) };
        let child = deckcraft_model::Shape { id: ShapeId(p.alloc_id().expect("shape id")), kind: ShapeKind::Table(table), ..Default::default() };
        let group = deckcraft_model::Shape {
            id: ShapeId(p.alloc_id().expect("group id")),
            kind: ShapeKind::Group { children: vec![child], child: Default::default() },
            ..Default::default()
        };
        let slide = p.slides.first_mut().expect("default slide");
        Arc::make_mut(slide).shapes.push(group);
        p
    }

    fn first_table_style(shapes: &[deckcraft_model::Shape]) -> Option<String> {
        for shape in shapes {
            if let ShapeKind::Table(table) = &shape.kind {
                return Some(table.style.clone());
            }
            if let Some(style) = first_table_style(shape.children()) {
                return Some(style);
            }
        }
        None
    }

    #[test]
    fn reused_grouped_table_styles_survive_native_and_pptx_round_trips_and_undo() {
        for (name, bytes) in [
            ("source.deckcraft", deckcraft_format::save(&styled_table_presentation(CUSTOM_STYLE, "FF0000")).expect("native source")),
            ("source.pptx", deckcraft_pptx::export(&styled_table_presentation(CUSTOM_STYLE, "FF0000")).expect("pptx source")),
        ] {
            let data = super::super::base64_encode(&bytes);
            let mut session = Session::with_new();
            session.execute("slide.reuse", &json!({"data": data, "name": name, "slides": [0], "keepFormatting": true})).expect("reuse");
            let doc = &session.doc().expect("doc").doc;
            assert_eq!(doc.table_styles.iter().filter(|style| style.id == CUSTOM_STYLE).count(), 1);
            assert!(doc.table_styles.iter().any(|style| style.id == CUSTOM_STYLE && style.raw.contains("FF0000")));
            assert_eq!(first_table_style(&doc.slides.get(1).expect("reused slide").shapes).as_deref(), Some(CUSTOM_STYLE));

            session.execute("edit.undo", &json!({})).expect("undo reuse");
            let doc = &session.doc().expect("doc").doc;
            assert!(doc.table_styles.iter().all(|style| style.id != CUSTOM_STYLE));
            session.execute("edit.redo", &json!({})).expect("redo reuse");
            let bytes = crate::cmd::file::save_bytes(&session.doc().expect("doc").doc, "deckcraft").expect("save destination");
            let reopened = crate::cmd::file::open_presentation("destination.deckcraft", &bytes).expect("reopen destination");
            assert!(reopened.table_styles.iter().any(|style| style.id == CUSTOM_STYLE));
            assert_eq!(first_table_style(&reopened.slides.get(1).expect("reused slide").shapes).as_deref(), Some(CUSTOM_STYLE));
        }
    }

    #[test]
    fn reused_table_style_definitions_are_deduplicated_or_remapped_on_collision() {
        let source = styled_table_presentation(CUSTOM_STYLE, "FF0000");
        let source_bytes = deckcraft_format::save(&source).expect("save source");

        let mut identical = Session::with_new();
        identical
            .edit(|doc, _| {
                doc.table_styles.push(source.table_styles.first().cloned().unwrap_or_default());
                Ok(())
            })
            .expect("seed identical definition");
        identical
            .execute("slide.reuse", &json!({"data": super::super::base64_encode(&source_bytes), "name": "source.deckcraft", "slides": [0]}))
            .expect("reuse identical definition");
        let doc = &identical.doc().expect("doc").doc;
        assert_eq!(doc.table_styles.iter().filter(|style| style.id == CUSTOM_STYLE).count(), 1);
        assert_eq!(first_table_style(&doc.slides.get(1).expect("reused slide").shapes).as_deref(), Some(CUSTOM_STYLE));

        let mut conflicting = Session::with_new();
        conflicting
            .edit(|doc, _| {
                let mut own = source.table_styles.first().cloned().unwrap_or_default();
                own.raw = own.raw.replace("FF0000", "0000FF");
                doc.table_styles.push(own);
                let table = deckcraft_model::Table { style: CUSTOM_STYLE.into(), ..deckcraft_model::Table::new(1, 1, 100.0, 24.0) };
                let id = ShapeId(doc.alloc_id().expect("destination table id"));
                if let Some(slide) = doc.slides.first_mut() {
                    Arc::make_mut(slide).shapes.push(Shape { id, kind: ShapeKind::Table(table), ..Default::default() });
                }
                Ok(())
            })
            .expect("seed conflicting definition");
        conflicting
            .execute("slide.reuse", &json!({"data": super::super::base64_encode(&source_bytes), "name": "source.deckcraft", "slides": [0]}))
            .expect("reuse conflicting definition");
        let doc = &conflicting.doc().expect("doc").doc;
        assert_eq!(first_table_style(&doc.slides.first().expect("destination slide").shapes).as_deref(), Some(CUSTOM_STYLE));
        let imported_style = first_table_style(&doc.slides.get(1).expect("reused slide").shapes).expect("grouped table style");
        assert_ne!(imported_style, CUSTOM_STYLE);
        assert!(
            doc.table_styles.iter().any(|style| style.id == imported_style
                && style.raw.contains("FF0000")
                && style.raw.contains(&format!("styleId=\"{imported_style}\"")))
        );
        assert!(doc.table_styles.iter().any(|style| style.id == CUSTOM_STYLE && style.raw.contains("0000FF")));
    }

    #[test]
    fn reused_slide_content_fits_destination_size() {
        let mut src = defaults::new_presentation(None);
        src.slide_size = Size::new(960.0, 540.0);
        let slide_id = src.slides.first().map(|s| s.id).unwrap_or_default();
        if let Some(slide) = src.slides.first_mut().map(Arc::make_mut) {
            let mut text = TextBody::from_text("scaled");
            if let Some(run) = text.paragraphs.first_mut().and_then(|p| p.runs.first_mut()) {
                run.props.size = Some(24.0);
            }
            slide.shapes = vec![
                Shape { xfrm: Some(Xfrm::new(800.0, 400.0, 200.0, 100.0)), ..Default::default() },
                Shape {
                    xfrm: Some(Xfrm::new(40.0, 50.0, 400.0, 200.0)),
                    text: Some(text),
                    kind: ShapeKind::Table({
                        let mut table = Table::new(2, 2, 400.0, 40.0);
                        table.rows[0].cells[0].text = TextBody::from_text("cell");
                        if let Some(run) = table.rows[0].cells[0].text.paragraphs.first_mut().and_then(|p| p.runs.first_mut()) {
                            run.props.size = Some(18.0);
                        }
                        table
                    }),
                    ..Default::default()
                },
                Shape {
                    xfrm: Some(Xfrm::new(100.0, 80.0, 300.0, 150.0)),
                    kind: ShapeKind::Group {
                        child: Xfrm::new(0.0, 0.0, 300.0, 150.0),
                        children: vec![Shape { xfrm: Some(Xfrm::new(200.0, 100.0, 80.0, 40.0)), ..Default::default() }],
                    },
                    ..Default::default()
                },
            ];
        }
        if let Some(master) = src.masters.first_mut().map(Arc::make_mut) {
            master.shapes = vec![Shape { xfrm: Some(Xfrm::new(800.0, 400.0, 100.0, 50.0)), ..Default::default() }];
            if let Some(layout) = master.layouts.first_mut() {
                layout.shapes = vec![Shape { xfrm: Some(Xfrm::new(700.0, 300.0, 100.0, 50.0)), ..Default::default() }];
            }
        }
        let bytes = deckcraft_format::save(&src).expect("source save");
        let data = super::super::base64_encode(&bytes);
        let mut session = Session::with_new();
        session
            .edit(|doc, _| {
                doc.slide_size = Size::new(480.0, 270.0);
                Ok(())
            })
            .expect("destination size");
        session.execute("slide.reuse", &json!({"data": data, "name": "source.deckcraft", "slides": [0], "keepFormatting": true})).expect("reuse");

        let doc = &session.doc().expect("doc").doc;
        let reused = doc.slides.get(1).expect("reused slide");
        let first = reused.shapes.first().and_then(|s| s.xfrm).expect("shape transform");
        assert_eq!(first, Xfrm::new(400.0, 200.0, 100.0, 50.0));
        let table_shape = reused.shapes.get(1).expect("table");
        let table_box = table_shape.xfrm.expect("table transform");
        assert_eq!(table_box, Xfrm::new(20.0, 25.0, 200.0, 100.0));
        assert!(matches!(&table_shape.kind, ShapeKind::Table(_)));
        let ShapeKind::Table(table) = &table_shape.kind else { return };
        assert_eq!(table.cols, vec![100.0, 100.0]);
        assert_eq!(table.rows.first().map(|r| r.height), Some(20.0));
        let font_size = table_shape.text.as_ref().and_then(|t| t.paragraphs.first()).and_then(|p| p.runs.first()).and_then(|r| r.props.size);
        assert_eq!(font_size, Some(12.0));
        let cell_size = table.rows[0].cells[0].text.paragraphs.first().and_then(|p| p.runs.first()).and_then(|r| r.props.size);
        assert_eq!(cell_size, Some(9.0));
        let group = reused.shapes.get(2).expect("group");
        assert!(matches!(&group.kind, ShapeKind::Group { .. }));
        let ShapeKind::Group { children, child } = &group.kind else { return };
        assert_eq!(group.xfrm, Some(Xfrm::new(50.0, 40.0, 150.0, 75.0)));
        assert_eq!(*child, Xfrm::new(0.0, 0.0, 150.0, 75.0));
        assert_eq!(children.first().and_then(|s| s.xfrm), Some(Xfrm::new(100.0, 50.0, 40.0, 20.0)));

        let imported_master = doc.masters.last().expect("imported master");
        assert_eq!(imported_master.shapes.first().and_then(|s| s.xfrm), Some(Xfrm::new(400.0, 200.0, 50.0, 25.0)));
        assert_eq!(imported_master.layouts.first().and_then(|l| l.shapes.first()).and_then(|s| s.xfrm), Some(Xfrm::new(350.0, 150.0, 50.0, 25.0)));
        assert_ne!(reused.id, slide_id);

        let saved = deckcraft_format::save(&doc.clone()).expect("destination save");
        let reopened = crate::cmd::file::open_presentation("destination.deckcraft", &saved).expect("destination reopen");
        let reopened_shape = reopened.slides.get(1).and_then(|s| s.shapes.first()).and_then(|s| s.xfrm);
        assert_eq!(reopened_shape, Some(Xfrm::new(400.0, 200.0, 100.0, 50.0)));
    }

    /// Every kind of media a slide can hold comes along, each pointing at its own bytes: the
    /// artistic-effect original, a background picture, poster, sounds and picture bullets too.
    #[test]
    fn reused_slides_bring_every_media_reference() {
        let mut src = defaults::new_presentation(None);
        let blobs: Vec<Vec<u8>> = (0..8u8).map(|i| vec![0xD0, i, i, i]).collect();
        let ids: Vec<MediaId> = blobs
            .iter()
            .enumerate()
            .map(|(i, b)| src.add_media(&format!("m{i}.bin"), "application/octet-stream", b.clone()).expect("media"))
            .collect();
        let id = |i: usize| ids.get(i).copied().unwrap_or_default();
        let mut text = TextBody::from_text("bullet");
        if let Some(p) = text.paragraphs.first_mut() {
            p.props.bullet = Some(Bullet::Picture { media: id(6) });
        }
        let slide = Arc::make_mut(src.slides.first_mut().expect("slide"));
        slide.shapes = vec![
            Shape {
                id: ShapeId(9001),
                kind: ShapeKind::Picture {
                    fill: PictureFill {
                        media: id(0),
                        artistic: Some(Artistic { effect: "artisticBlur".into(), original: id(1) }),
                        ..Default::default()
                    },
                },
                ..Default::default()
            },
            Shape {
                id: ShapeId(9002),
                kind: ShapeKind::Media(MediaClip { media: id(2), poster: Some(id(3)), ..Default::default() }),
                ..Default::default()
            },
            Shape {
                id: ShapeId(9003),
                click: Some(Hyperlink {
                    action: Action::NextSlide,
                    tooltip: String::new(),
                    highlight_click: false,
                    sound: Some(id(5)),
                    stop_sound: false,
                    zoom_return: None,
                }),
                text: Some(text),
                ..Default::default()
            },
        ];
        slide.background = Some(Background::Fill { fill: Fill::Picture(PictureFill { media: id(4), ..Default::default() }) });
        slide.transition = Some(deckcraft_model::anim::Transition { sound: Some(id(7)), ..Default::default() });
        let bytes = deckcraft_format::save(&src).expect("save");

        let mut s = Session::with_new();
        let own = vec![1u8, 2, 3];
        let own_id = s.edit(|doc, _| Ok(doc.add_media("own.bin", "application/octet-stream", own.clone()).expect("media"))).expect("own media");
        s.execute("slide.reuse", &json!({"data": super::super::base64_encode(&bytes), "name": "src.deckcraft", "slides": [0]})).expect("reuse");
        let doc = &s.doc().expect("doc").doc;
        assert_eq!(doc.media(own_id).map(|m| m.data.as_slice() == own.as_slice()), Some(true), "own media untouched");
        let reused = doc.slides.get(1).expect("reused slide");
        let got: Vec<Vec<u8>> =
            media_refs::slide_media(reused).iter().map(|m| doc.media(*m).map(|m| m.data.as_ref().clone()).unwrap_or_default()).collect();
        let mut got = got;
        got.sort();
        let mut want = blobs.clone();
        want.sort();
        assert_eq!(got, want, "every reference resolves to its own copied bytes");
    }

    fn link_to(slide: SlideId) -> Hyperlink {
        Hyperlink {
            action: Action::Slide { slide },
            tooltip: String::new(),
            highlight_click: false,
            sound: None,
            stop_sound: false,
            zoom_return: None,
        }
    }

    fn linked_text(slide: SlideId) -> TextBody {
        let mut text = TextBody::from_text("go");
        if let Some(r) = text.paragraphs.first_mut().and_then(|p| p.runs.first_mut()) {
            r.props.link = Some(link_to(slide));
        }
        text
    }

    /// Links to a slide that is reused too follow the copy; links to a slide left behind stay.
    #[test]
    fn reused_slides_link_to_each_others_copies() {
        let mut src = defaults::new_presentation(None);
        let template = src.slides.first().map(|s| s.as_ref().clone()).expect("slide");
        for _ in 0..2 {
            let mut sl = template.clone();
            sl.id = SlideId(src.alloc_id().expect("id"));
            src.slides.push(Arc::new(sl));
        }
        let ids: Vec<SlideId> = src.slides.iter().map(|s| s.id).collect();
        let id = |i: usize| ids.get(i).copied().unwrap_or_default();
        if let Some(sl) = src.slides.first_mut().map(Arc::make_mut) {
            sl.shapes = vec![
                Shape { id: ShapeId(9001), click: Some(link_to(id(1))), ..Default::default() },
                Shape { id: ShapeId(9002), text: Some(linked_text(id(1))), ..Default::default() },
                Shape { id: ShapeId(9003), click: Some(link_to(id(2))), ..Default::default() },
            ];
        }
        if let Some(sl) = src.slides.get_mut(1).map(Arc::make_mut) {
            sl.shapes = vec![Shape { id: ShapeId(9004), click: Some(link_to(id(0))), ..Default::default() }];
        }
        let bytes = deckcraft_format::save(&src).expect("save");
        let reopened = crate::cmd::file::open_presentation("src.deckcraft", &bytes).expect("open");
        let left_behind = reopened.slides.get(2).map(|s| s.id).expect("third slide");

        let mut s = Session::with_new();
        for _ in 0..3 {
            s.execute("slide.new", &json!({})).expect("new");
        }
        let out = s
            .execute("slide.reuse", &json!({"data": super::super::base64_encode(&bytes), "name": "src.deckcraft", "slides": [0, 1]}))
            .expect("reuse");
        let new: Vec<SlideId> = serde_json::from_value(out.get("ids").cloned().unwrap_or_default()).expect("ids");
        let doc = &s.doc().expect("doc").doc;
        let copy = |i: usize| new.get(i).and_then(|id| doc.slide(*id)).expect("copied slide");
        let click = |sh: Option<&Shape>| sh.and_then(|sh| sh.click.as_ref()).map(|h| h.action.clone());
        let first = copy(0);
        let to = |i: usize| Some(Action::Slide { slide: new.get(i).copied().unwrap_or_default() });
        assert_eq!(click(first.shapes.first()), to(1), "shape click follows the copy");
        let run_link = first
            .shapes
            .get(1)
            .and_then(|sh| sh.text.as_ref())
            .and_then(|t| t.paragraphs.first())
            .and_then(|p| p.runs.first())
            .and_then(|r| r.props.link.as_ref())
            .map(|h| h.action.clone());
        assert_eq!(run_link, to(1), "text link follows the copy");
        assert_eq!(click(first.shapes.get(2)), Some(Action::Slide { slide: left_behind }), "a slide not reused keeps its link");
        assert_eq!(click(copy(1).shapes.first()), to(0), "a link back to an earlier copy follows too");
    }

    /// The list that lets a dialog offer the slides of the other file; picking from it brings in
    /// exactly those.
    #[test]
    fn reuse_list_names_the_slides_and_the_picked_ones_come_in() {
        let mut src = Session::with_new();
        for t in ["Alpha", "Beta", "Gamma"] {
            src.execute("slide.new", &json!({"layout": "titleAndContent", "title": t})).expect("new");
        }
        let last_slide = src.doc().expect("doc").doc.slides.len() - 1;
        src.execute("slide.hide", &json!({"index": last_slide})).expect("hide");
        let bytes = crate::cmd::file::save_bytes(&src.doc().expect("doc").doc, "deckcraft").expect("save");
        let data = super::super::base64_encode(&bytes);

        let mut s = Session::with_new();
        let listed = s.execute("slide.reuseList", &json!({"data": data, "name": "src.deckcraft"})).expect("list");
        let slides = listed["slides"].as_array().expect("slides");
        let title = |i: usize| slides.iter().find(|x| x["index"] == i).and_then(|x| x["title"].as_str()).unwrap_or_default().to_string();
        assert!(slides.len() >= 3);
        let (a, b, g) = (title(slides.len() - 3), title(slides.len() - 2), title(slides.len() - 1));
        assert_eq!((a.as_str(), b.as_str(), g.as_str()), ("Alpha", "Beta", "Gamma"));
        assert_eq!(slides.last().map(|x| x["hidden"].clone()), Some(json!(true)));
        assert_eq!(s.doc().expect("doc").doc.slides.len(), 1, "listing changes nothing");

        let last = slides.len() - 1;
        s.execute("slide.reuse", &json!({"data": data, "name": "src.deckcraft", "slides": [last - 2, last]})).expect("reuse");
        let titles: Vec<String> = s.doc().expect("doc").doc.slides.iter().map(|x| x.title()).collect();
        assert!(titles.iter().any(|t| t == "Alpha") && titles.iter().any(|t| t == "Gamma"));
        assert!(!titles.iter().any(|t| t == "Beta"), "a slide that was not picked stays out");
    }

    // ---- which layout a slide brought into another deck is on (observed in PowerPoint) ----

    fn frame(x: f64) -> Xfrm {
        Xfrm::new(x, 150.0, 405.0, 320.0)
    }

    /// A layout with a title and one content placeholder per `(idx, frame)`, added to the first master.
    fn add_layout(p: &mut Presentation, name: &str, contents: &[(u32, Xfrm)]) -> LayoutId {
        use deckcraft_model::{PhType, Placeholder};
        let id = LayoutId(p.alloc_id().expect("layout id"));
        let mut shapes = vec![Shape {
            id: ShapeId(p.alloc_id().expect("title id")),
            ph: Some(Placeholder { kind: PhType::Title, idx: 0, ..Default::default() }),
            ..Default::default()
        }];
        for (idx, xfrm) in contents {
            let ph = Some(Placeholder { kind: PhType::Obj, idx: *idx, ..Default::default() });
            shapes.push(Shape { id: ShapeId(p.alloc_id().expect("content id")), ph, xfrm: Some(*xfrm), ..Default::default() });
        }
        let layout = deckcraft_model::Layout { id, name: name.into(), shapes, show_master_shapes: true, ..Default::default() };
        if let Some(m) = p.masters.first_mut() {
            Arc::make_mut(m).layouts.push(layout);
        }
        id
    }

    /// A one-slide deck whose slide ("A", "B", "C") is on a layout "L2" with two contents, idx 10 and 11.
    fn source_with_l2() -> Presentation {
        use deckcraft_model::{PhType, Placeholder};
        let mut p = defaults::new_presentation(None);
        let l2 = add_layout(&mut p, "L2", &[(10, frame(60.0)), (11, frame(495.0))]);
        let ph = |idx, kind| Some(Placeholder { kind, idx, ..Default::default() });
        let mut shapes = vec![];
        for (text, idx, kind) in [("A", 0, PhType::Title), ("B", 10, PhType::Obj), ("C", 11, PhType::Obj)] {
            let id = ShapeId(p.alloc_id().expect("shape id"));
            shapes.push(Shape { id, ph: ph(idx, kind), text: Some(TextBody::from_text(text)), ..Default::default() });
        }
        if let Some(slide) = p.slides.first_mut().map(Arc::make_mut) {
            slide.layout = l2;
            slide.shapes = shapes;
        }
        p
    }

    fn reuse_into(dest: &mut Session, src: &Presentation) {
        let bytes = deckcraft_format::save(src).expect("save source");
        dest.execute("slide.reuse", &json!({"data": super::super::base64_encode(&bytes), "name": "source.deckcraft", "slides": [0]})).expect("reuse");
    }

    fn layout_names(p: &Presentation) -> Vec<String> {
        p.masters.iter().flat_map(|m| m.layouts.iter().map(|l| l.name.clone())).collect()
    }

    fn layout_name_of(p: &Presentation, slide: usize) -> String {
        p.slides.get(slide).and_then(|s| p.layout(s.layout)).map(|(_, l)| l.name.clone()).expect("the slide's layout")
    }

    /// Where the slide's placeholder with this text is drawn, if a layout placeholder gives it its frame.
    fn frame_of(p: &Presentation, slide: usize, text: &str) -> Option<Xfrm> {
        use deckcraft_model::resolve::{Ctx, parents, xfrm};
        let slide = p.slides.get(slide)?;
        let ctx = Ctx::for_slide(p, slide)?;
        let shape = slide.shapes.iter().find(|s| s.text.as_ref().is_some_and(|t| t.text() == text))?;
        parents(&ctx, shape).0.map(|_| xfrm(&ctx, shape))
    }

    #[test]
    fn a_reused_slide_takes_the_destination_layout_with_the_same_placeholders_whatever_its_name_or_frames() {
        let src = source_with_l2();
        // Same name and placeholders, other frames: the destination's layout, with its frames.
        let mut same_name = Session::with_new();
        same_name
            .edit(|doc, _| {
                add_layout(doc, "L2", &[(10, frame(70.0)), (11, frame(505.0))]);
                Ok(())
            })
            .expect("seed");
        let before = layout_names(&same_name.doc().expect("doc").doc);
        reuse_into(&mut same_name, &src);
        let doc = &same_name.doc().expect("doc").doc;
        assert_eq!(layout_names(doc), before, "no layout was brought along");
        assert_eq!(layout_name_of(doc, 1), "L2");
        assert_eq!(frame_of(doc, 1, "C").map(|x| x.x), Some(505.0));
        // Another name with the same placeholders: that layout.
        let mut other_name = Session::with_new();
        other_name
            .edit(|doc, _| {
                add_layout(doc, "L7", &[(10, frame(60.0)), (11, frame(495.0))]);
                Ok(())
            })
            .expect("seed");
        reuse_into(&mut other_name, &src);
        let doc = &other_name.doc().expect("doc").doc;
        assert_eq!(layout_name_of(doc, 1), "L7");
    }

    #[test]
    fn a_reused_slide_brings_its_layout_along_when_no_destination_layout_has_its_placeholders() {
        let src = source_with_l2();
        // The destination's L2 has one content: the source's L2 joins as 1_L2 and C stays paired.
        let mut one_content = Session::with_new();
        one_content
            .edit(|doc, _| {
                add_layout(doc, "L2", &[(10, frame(60.0))]);
                Ok(())
            })
            .expect("seed");
        let before = layout_names(&one_content.doc().expect("doc").doc).len();
        reuse_into(&mut one_content, &src);
        let doc = &one_content.doc().expect("doc").doc;
        assert_eq!(layout_names(doc).len(), before + 1);
        assert_eq!(layout_name_of(doc, 1), "1_L2");
        assert_eq!(frame_of(doc, 1, "C").map(|x| x.x), Some(495.0), "C takes its frame from the layout that came along");
        assert_eq!(frame_of(doc, 1, "B").map(|x| x.x), Some(60.0));
        // No L2 at all (and no built-in layout has idx 10 and 11): it joins under its own name.
        let mut none = Session::with_new();
        reuse_into(&mut none, &src);
        let doc = &none.doc().expect("doc").doc;
        assert_eq!(layout_name_of(doc, 1), "L2");
        assert_eq!(frame_of(doc, 1, "C").map(|x| x.x), Some(495.0));
        // A second reuse finds the layout that came along instead of adding another.
        let count = layout_names(doc).len();
        reuse_into(&mut none, &src);
        assert_eq!(layout_names(&none.doc().expect("doc").doc).len(), count);
    }

    #[test]
    fn a_pasted_slide_takes_or_brings_its_layout_like_a_reused_one() {
        let src = source_with_l2();
        let mut a = Session::with_new();
        a.edit(|doc, _| {
            *doc = src.clone();
            Ok(())
        })
        .expect("source deck");
        a.execute("edit.copy", &json!({"scope": "slides"})).expect("copy");
        // A destination whose L2 has one content: the layout joins as 1_L2 and C keeps its frame.
        let mut b = Session::with_new();
        b.edit(|doc, _| {
            add_layout(doc, "L2", &[(10, frame(60.0))]);
            Ok(())
        })
        .expect("seed");
        b.clipboard = a.clipboard.clone();
        b.execute("edit.paste", &json!({"scope": "slides"})).expect("paste");
        let doc = &b.doc().expect("doc").doc;
        assert_eq!(layout_name_of(doc, 1), "1_L2");
        assert_eq!(frame_of(doc, 1, "C").map(|x| x.x), Some(495.0));
        // A destination with the same placeholders under another name: that layout, nothing added.
        let mut c = Session::with_new();
        c.edit(|doc, _| {
            add_layout(doc, "L7", &[(10, frame(60.0)), (11, frame(495.0))]);
            Ok(())
        })
        .expect("seed");
        let before = layout_names(&c.doc().expect("doc").doc).len();
        c.clipboard = a.clipboard.clone();
        c.execute("edit.paste", &json!({"scope": "slides"})).expect("paste");
        let doc = &c.doc().expect("doc").doc;
        assert_eq!((layout_name_of(doc, 1), layout_names(doc).len()), ("L7".to_string(), before));
        // Pasting in the deck it was copied from keeps the slide on its own layout.
        a.execute("edit.paste", &json!({"scope": "slides"})).expect("paste in the source");
        let doc = &a.doc().expect("doc").doc;
        assert_eq!(doc.slides.len(), 2);
        assert_eq!(doc.slides[0].layout, doc.slides[1].layout);
    }
}
