//! Shape trees: `p:sp`, `p:pic`, `p:grpSp`, `p:cxnSp`, `p:graphicFrame` (tables, charts, SmartArt,
//! OLE and other objects) and `p:contentPart` (3-D models).

use std::collections::{HashMap, HashSet};

use deckcraft_color::SchemeSlot;
use deckcraft_geom::{Xfrm, preset};
use deckcraft_model::style::{ColorRef, Effects, Fill};
use deckcraft_model::table::{Cell, Row, Table};
use deckcraft_model::text::{Anchor, TextDir};
use deckcraft_model::{Geom, MediaClip, PhType, Placeholder, Shape, ShapeId, ShapeKind, ShapeStyle};

use super::{Imp, Part, dml};
use crate::xml::El;

/// File shape ids at or above this are reassigned.
pub const MAX_KEPT_ID: u32 = 1 << 30;
const MAX_TREE_DEPTH: usize = 32;
const MAX_SHAPES: usize = 20_000;

/// Shape id bookkeeping for one part.
#[derive(Default)]
pub struct IdCtx {
    pub used: HashSet<u32>,
    /// File id → model id.
    pub map: HashMap<u32, ShapeId>,
    /// Always allocate new ids (shapes pulled in from other parts).
    pub fresh: bool,
    pub count: usize,
}

impl IdCtx {
    fn assign(&mut self, imp: &mut Imp, file_id: Option<u32>) -> ShapeId {
        if !self.fresh
            && let Some(i) = file_id
            && (2..MAX_KEPT_ID).contains(&i)
            && !self.used.contains(&i)
        {
            self.used.insert(i);
            self.map.insert(i, ShapeId(i));
            return ShapeId(i);
        }
        let n = imp.alloc();
        self.used.insert(n);
        if let Some(i) = file_id {
            self.map.entry(i).or_insert(ShapeId(n));
        }
        ShapeId(n)
    }
    pub fn lookup(&self, file_id: u32) -> Option<ShapeId> {
        self.map.get(&file_id).copied()
    }
}

/// Read the children of an `spTree` / `grpSp`.
pub fn sp_tree(imp: &mut Imp, part: &Part, ctx: &mut IdCtx, tree: &El, depth: usize) -> Vec<Shape> {
    let mut out = vec![];
    if depth > MAX_TREE_DEPTH {
        log::warn!("pptx: group nesting too deep in {}", part.name);
        return out;
    }
    for e in tree.elements() {
        if ctx.count >= MAX_SHAPES {
            log::warn!("pptx: too many shapes in {}", part.name);
            break;
        }
        let s = match e.local() {
            "sp" => Some(sp(imp, part, ctx, e)),
            "pic" => Some(pic(imp, part, ctx, e)),
            "grpSp" => Some(grp(imp, part, ctx, e, depth)),
            "cxnSp" => Some(cxn(imp, part, ctx, e)),
            "graphicFrame" => Some(frame(imp, part, ctx, e, depth)),
            "contentPart" => content_part(imp, part, ctx, e).or_else(|| {
                log::warn!("pptx: content part in {} not imported", part.name);
                None
            }),
            "AlternateContent" => read_model_alternate(imp, part, ctx, e),
            _ => None,
        };
        if let Some(s) = s {
            ctx.count += 1;
            out.push(s);
        }
    }
    if depth == 0 {
        remap_connectors(&mut out, ctx, 0);
    }
    out
}

/// ActiveX controls (`p:controls`): each control's picture (`p:pic`), the image PowerPoint shows
/// for it, as a picture above the slide's shapes. Controls without one are left out.
pub fn controls(imp: &mut Imp, part: &Part, ctx: &mut IdCtx, controls: &El) -> Vec<Shape> {
    let mut out = vec![];
    for c in controls.children_named("control") {
        if ctx.count >= MAX_SHAPES {
            break;
        }
        if let Some(p) = c.child("pic") {
            ctx.count += 1;
            out.push(pic(imp, part, ctx, p));
        }
    }
    out
}

fn remap_connectors(shapes: &mut [Shape], ctx: &IdCtx, depth: usize) {
    if depth > MAX_TREE_DEPTH {
        return;
    }
    for s in shapes.iter_mut() {
        match &mut s.kind {
            ShapeKind::Connector { start, end } => {
                for e in [start, end] {
                    *e = e.and_then(|(id, site)| ctx.lookup(id.0).map(|m| (m, site)));
                }
            }
            ShapeKind::Group { children, .. } => remap_connectors(children, ctx, depth + 1),
            _ => {}
        }
    }
}

/// PowerPoint's Lock (right-click > Lock, the Selection pane padlock) writes a full set of locking
/// attributes; it reads an object as locked only when the whole set for its kind is there, so a
/// lone `noMove` or `noResize` (other tools' partial locks) isn't Lock.
fn is_locked(l: &El) -> bool {
    let set: &[&str] = match l.local() {
        "grpSpLocks" => &["noGrp", "noUngrp", "noRot", "noMove", "noResize"],
        "graphicFrameLocks" => &["noGrp", "noDrilldown", "noMove", "noResize"],
        _ => &["noGrp", "noRot", "noMove", "noResize", "noEditPoints", "noAdjustHandles", "noChangeArrowheads", "noChangeShapeType"],
    };
    set.iter().all(|a| l.bool(a) == Some(true))
}

/// Common non-visual properties (`p:nvSpPr`, `p:nvPicPr`…).
fn base(imp: &mut Imp, part: &Part, ctx: &mut IdCtx, nv: Option<&El>) -> Shape {
    let c = nv.and_then(|n| n.child("cNvPr"));
    let mut s = Shape { id: ctx.assign(imp, c.and_then(|c| c.u32("id"))), ..Default::default() };
    if let Some(c) = c {
        s.name = c.attr("name").unwrap_or("").to_string();
        s.descr = c.attr("descr").unwrap_or("").to_string();
        s.title = c.attr("title").unwrap_or("").to_string();
        s.hidden = c.bool("hidden").unwrap_or(false);
        s.click = c.child("hlinkClick").and_then(|h| dml::hyperlink(imp, part, h));
        s.hover = c.child("hlinkHover").and_then(|h| dml::hyperlink(imp, part, h));
        s.decorative = c.find("decorative").and_then(|d| d.bool("val")).unwrap_or(false);
    }
    if let Some(nv) = nv {
        for e in nv.elements() {
            match e.local() {
                "cNvSpPr" => {
                    s.text_box = e.bool("txBox").unwrap_or(false);
                    s.locked = e.child("spLocks").is_some_and(is_locked);
                }
                "cNvPicPr" | "cNvGrpSpPr" | "cNvCxnSpPr" | "cNvGraphicFramePr" => {
                    s.locked = e.elements().find(|l| l.local().ends_with("Locks")).is_some_and(is_locked);
                }
                "nvPr" => {
                    if let Some(ph) = e.child("ph") {
                        s.ph = Some(Placeholder {
                            kind: ph.attr("type").map(PhType::from_xml).unwrap_or(PhType::Obj),
                            idx: ph.u32("idx").unwrap_or(0),
                            vertical: ph.attr("orient") == Some("vert"),
                            size: ph.attr("sz").map(String::from),
                            has_custom_prompt: ph.bool("hasCustomPrompt").unwrap_or(false),
                        });
                    }
                    s.tags = super::tags(imp, part, e);
                }
                _ => {}
            }
        }
    }
    s
}

fn preset_geom(pg: &El) -> Geom {
    let name = pg.attr("prst").unwrap_or("rect").to_string();
    let defaults: Vec<f64> = preset::info(&name).map(|i| i.defaults.to_vec()).unwrap_or_default();
    let mut adj = defaults;
    let mut any = false;
    if let Some(av) = pg.child("avLst") {
        for gd in av.children_named("gd").take(16) {
            let n = gd.attr("name").unwrap_or("");
            let idx = if n == "adj" {
                0
            } else if let Some(k) = n.strip_prefix("adj").and_then(|k| k.parse::<usize>().ok()).filter(|k| (1..=16).contains(k)) {
                k - 1
            } else {
                continue;
            };
            let Some(v) =
                gd.attr("fmla").and_then(|f| f.trim().strip_prefix("val")).and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| v.is_finite())
            else {
                continue;
            };
            if adj.len() <= idx {
                adj.resize(idx + 1, 0.0);
            }
            if let Some(slot) = adj.get_mut(idx) {
                *slot = v;
                any = true;
            }
        }
    }
    if !any {
        adj.clear();
    }
    Geom::Preset { name, adj }
}

/// A placeholder whose `spPr` names no geometry takes its layout's (or master's) placeholder's:
/// marked here with an empty custom geometry, filled in once the layouts are read
/// (`inherit_placeholder_geometry`).
fn mark_inherited_geometry(s: &mut Shape, pr: Option<&El>) {
    if s.ph.is_some() && pr.is_none_or(|pr| pr.child("prstGeom").is_none() && pr.child("custGeom").is_none()) {
        s.geom = Geom::Custom { paths: vec![] };
    }
}

/// `spPr` → transform, geometry, fill, line, effects.
fn sp_pr(imp: &mut Imp, part: &Part, s: &mut Shape, pr: &El) {
    if let Some(x) = pr.child("xfrm") {
        s.xfrm = Some(dml::xfrm(x).0);
    }
    if let Some(pg) = pr.child("prstGeom") {
        s.geom = preset_geom(pg);
    } else if let Some(cg) = pr.child("custGeom") {
        let (w, h) = s.xfrm.map(|x| (deckcraft_geom::pt_to_emu(x.w) as f64, deckcraft_geom::pt_to_emu(x.h) as f64)).unwrap_or((21600.0, 21600.0));
        let paths = crate::custgeom::read_cust_geom(cg, w, h);
        if !paths.is_empty() {
            s.geom = Geom::Custom { paths };
        }
    }
    s.fill = dml::fill_of(imp, part, pr);
    if let Some(ln) = pr.child("ln") {
        s.line = Some(dml::line(imp, part, ln));
    }
    if let Some(fx) = pr.child("effectLst") {
        s.effects = Some(dml::effects(fx));
    }
    let raw: String = pr.elements().filter(|c| c.is("scene3d") || c.is("sp3d")).map(|c| part.keep(c)).collect();
    if !raw.is_empty() {
        let fx = s.effects.get_or_insert_with(Effects::default);
        fx.bevel = pr.path(&["sp3d", "bevelT"]).map(|b| b.attr("prst").unwrap_or("circle").to_string());
        fx.raw3d = Some(raw);
    }
}

fn style(st: &El) -> ShapeStyle {
    let r = |n: &str| {
        st.child(n)
            .map(|e| (e.u32("idx").unwrap_or(0).min(1003), dml::color(e).unwrap_or(ColorRef::scheme(SchemeSlot::Accent1))))
            .unwrap_or((0, ColorRef::scheme(SchemeSlot::Accent1)))
    };
    let font = st.child("fontRef");
    ShapeStyle {
        line_ref: r("lnRef"),
        fill_ref: r("fillRef"),
        effect_ref: r("effectRef"),
        font_ref: (font.and_then(|f| f.attr("idx")).unwrap_or("minor").to_string(), font.and_then(dml::color)),
    }
}

fn sp(imp: &mut Imp, part: &Part, ctx: &mut IdCtx, e: &El) -> Shape {
    let mut s = base(imp, part, ctx, e.child("nvSpPr"));
    if let Some(pr) = e.child("spPr") {
        sp_pr(imp, part, &mut s, pr);
    }
    mark_inherited_geometry(&mut s, e.child("spPr"));
    if e.bool("useBgFill") == Some(true) {
        s.fill = Some(Fill::Background);
    }
    s.style = e.child("style").map(style);
    s.text = e.child("txBody").map(|t| dml::text_body(imp, part, t));
    s
}

fn ms(v: Option<&str>) -> u32 {
    v.and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| v.is_finite()).map(|v| v.clamp(0.0, u32::MAX as f64) as u32).unwrap_or(0)
}

fn find_content_part(el: &El) -> Option<&El> {
    fn rec(el: &El, depth: usize) -> Option<&El> {
        if depth > 16 {
            return None;
        }
        if el.is("contentPart") {
            return Some(el);
        }
        el.elements().find_map(|c| rec(c, depth + 1))
    }
    rec(el, 0)
}

/// `p:contentPart` whose relationship is a 3-D model. Other content parts (ink) are left out.
fn content_part(imp: &mut Imp, part: &Part, ctx: &mut IdCtx, e: &El) -> Option<Shape> {
    let id = e.attr("r:id").filter(|s| !s.is_empty())?;
    let rel = part.rels.get(id)?;
    if !super::is_model3d(rel) {
        return None;
    }
    let target = rel.target.clone();
    let media = imp.media_for(&target).or_else(|| {
        log::warn!("pptx: 3-D model part missing in {}", part.name);
        None
    })?;
    let mut s = base(imp, part, ctx, e.child("nvContentPartPr"));
    if let Some(x) = e.child("xfrm") {
        s.xfrm = Some(dml::xfrm(x).0);
    }
    s.kind = ShapeKind::Model3d { media, poster: None };
    Some(s)
}

/// A 3-D model wrapped in `mc:AlternateContent`: the model choice, plus the fallback picture.
/// If the model part is missing, the fallback picture is the shape.
fn read_model_alternate(imp: &mut Imp, part: &Part, ctx: &mut IdCtx, e: &El) -> Option<Shape> {
    if let Some(content) = e.children_named("Choice").find_map(find_content_part)
        && let Some(mut s) = content_part(imp, part, ctx, content)
    {
        let poster = e.child("Fallback").and_then(|fb| {
            let mut pics = vec![];
            fb.find_all("blip", &mut pics);
            pics.into_iter().find_map(|b| dml::media_ref(imp, part, b))
        });
        if let ShapeKind::Model3d { media, .. } = s.kind {
            s.kind = ShapeKind::Model3d { media, poster };
        }
        return Some(s);
    }
    e.child("Fallback").and_then(|fb| fb.elements().find(|c| c.is("pic")).map(|p| pic(imp, part, ctx, p)))
}

fn pic(imp: &mut Imp, part: &Part, ctx: &mut IdCtx, e: &El) -> Shape {
    let mut s = base(imp, part, ctx, e.child("nvPicPr"));
    if let Some(pr) = e.child("spPr") {
        sp_pr(imp, part, &mut s, pr);
    }
    s.style = e.child("style").map(style);
    let pf = e.child("blipFill").and_then(|b| dml::blip_fill(imp, part, b));
    let nvpr = e.path(&["nvPicPr", "nvPr"]);
    let vf = nvpr.and_then(|n| n.child("videoFile"));
    let af = nvpr.and_then(|n| n.child("audioFile"));
    let p14 = nvpr.and_then(|n| n.child("extLst")).and_then(|x| x.find("media"));
    if vf.is_some() || af.is_some() || p14.is_some() {
        let media = p14.and_then(|m| dml::media_ref(imp, part, m)).or_else(|| vf.or(af).and_then(|f| dml::media_ref(imp, part, f)));
        if let Some(media) = media {
            let mut clip = MediaClip {
                media,
                video: vf.is_some() || af.is_none(),
                poster: pf.as_ref().map(|p| p.media),
                crop: pf.as_ref().map(|p| p.crop).unwrap_or_default(),
                volume: 1.0,
                ..Default::default()
            };
            if let Some(m) = p14 {
                if let Some(t) = m.child("trim") {
                    clip.trim_start_ms = ms(t.attr("st"));
                    clip.trim_end_ms = ms(t.attr("end"));
                }
                if let Some(f) = m.child("fade") {
                    clip.fade_in_ms = ms(f.attr("in"));
                    clip.fade_out_ms = ms(f.attr("out"));
                }
                if let Some(l) = m.child("bmkLst") {
                    clip.bookmarks =
                        l.children_named("bmk").take(1000).map(|b| (b.attr("name").unwrap_or("").to_string(), ms(b.attr("time")))).collect();
                }
            }
            s.kind = ShapeKind::Media(clip);
            return s;
        }
    }
    match pf {
        Some(fill) => s.kind = ShapeKind::Picture { fill },
        None => log::warn!("pptx: picture {} in {} has no readable image", s.name, part.name),
    }
    s
}

fn grp(imp: &mut Imp, part: &Part, ctx: &mut IdCtx, e: &El, depth: usize) -> Shape {
    let mut s = base(imp, part, ctx, e.child("nvGrpSpPr"));
    let mut child = None;
    if let Some(pr) = e.child("grpSpPr") {
        if let Some(x) = pr.child("xfrm") {
            let (xf, ch) = dml::xfrm(x);
            s.xfrm = Some(xf);
            child = ch;
        }
        s.fill = dml::fill_of(imp, part, pr);
        if let Some(fx) = pr.child("effectLst") {
            s.effects = Some(dml::effects(fx));
        }
    }
    let children = sp_tree(imp, part, ctx, e, depth + 1);
    let child = child.or_else(|| s.xfrm.map(|x| Xfrm::new(x.x, x.y, x.w, x.h))).unwrap_or_default();
    s.kind = ShapeKind::Group { children, child };
    s
}

fn cxn(imp: &mut Imp, part: &Part, ctx: &mut IdCtx, e: &El) -> Shape {
    let nv = e.child("nvCxnSpPr");
    let mut s = base(imp, part, ctx, nv);
    if let Some(pr) = e.child("spPr") {
        sp_pr(imp, part, &mut s, pr);
    }
    s.style = e.child("style").map(style);
    let c = nv.and_then(|n| n.child("cNvCxnSpPr"));
    let end = |n: &str| c.and_then(|c| c.child(n)).and_then(|x| Some((ShapeId(x.u32("id")?), x.u32("idx").unwrap_or(0))));
    s.kind = ShapeKind::Connector { start: end("stCxn"), end: end("endCxn") };
    s
}

fn frame(imp: &mut Imp, part: &Part, ctx: &mut IdCtx, e: &El, depth: usize) -> Shape {
    let mut s = base(imp, part, ctx, e.child("nvGraphicFramePr"));
    if let Some(x) = e.child("xfrm") {
        s.xfrm = Some(dml::xfrm(x).0);
    }
    let gd = e.path(&["graphic", "graphicData"]);
    let uri = gd.and_then(|g| g.attr("uri")).unwrap_or("");
    if let Some(gd) = gd {
        if let Some(tbl) = gd.child("tbl") {
            s.kind = ShapeKind::Table(table(imp, part, tbl));
            return s;
        }
        if let Some(c) = gd.child("chart")
            && let Some(rel) = part.rels.get(c.attr("r:id").unwrap_or(""))
            && let Some(bytes) = imp.pkg.xml_bytes(&rel.target)
            && let Some(mut chart) = if uri.ends_with("/chartex") { super::read_chartex_xml(bytes) } else { super::read_chart_xml(bytes) }
        {
            chart.kept = super::chart::capture_kept(imp, &rel.target);
            s.kind = ShapeKind::Chart(Box::new(chart));
            return s;
        }
        if uri.ends_with("/diagram")
            && let Some(g) = smartart(imp, part, gd, &s, depth)
        {
            return g;
        }
    }
    // Something we don't model: keep its XML and show a preview picture when it has one.
    let preview = {
        let mut pics = vec![];
        e.find_all("blip", &mut pics);
        pics.into_iter().find_map(|b| dml::media_ref(imp, part, b))
    }
    .or_else(|| gd.and_then(|g| g.find("oleObj")).and_then(|o| vml_preview(imp, part, o.attr("spid")?)));
    // An embedded object's fallback picture (`p:oleObj/p:pic`) can carry a fill and an outline,
    // which PowerPoint draws behind and around the preview (old equation objects on coloured
    // boxes). The frame's own position stays.
    if let Some(pr) = gd.and_then(|g| g.find("oleObj")).and_then(|o| o.find("pic")).and_then(|p| p.child("spPr")) {
        if s.fill.is_none() {
            s.fill = dml::fill_of(imp, part, pr);
        }
        if s.line.is_none() {
            s.line = pr.child("ln").map(|ln| dml::line(imp, part, ln));
        }
    }
    let label = if let Some(o) = gd.and_then(|g| g.find("oleObj")) {
        o.attr("progId").or(o.attr("name")).unwrap_or("Object").to_string()
    } else if uri.ends_with("/diagram") {
        "SmartArt".to_string()
    } else if uri.contains("chart") {
        "Chart".to_string()
    } else {
        uri.rsplit('/').next().unwrap_or("Object").to_string()
    };
    // Keep the parts it points to so a save writes it back whole (#203). A part with relationships
    // of its own, or an external link, can't be carried: the save then falls back to the preview.
    let parts = super::rel_refs(e)
        .into_iter()
        .map(|id| {
            let rel = part.rels.get(&id).filter(|r| !r.external)?;
            if !imp.pkg.rels(&rel.target).items.is_empty() {
                return None;
            }
            let (kind, target) = (rel.ty.clone(), rel.target.clone());
            let media = if rel.kind == "diagramData" { diagram_data(imp, &target) } else { imp.media_for(&target) };
            Some((id, kind, media?))
        })
        .collect::<Option<Vec<_>>>()
        .unwrap_or_default();
    s.kind = ShapeKind::Opaque { xml: part.keep(e), preview, label, parts };
    s
}

/// The preview picture of an old-style embedded object (`p:oleObj spid` with no `p:pic`): the
/// `v:imagedata` of the legacy VML drawing shape with that id, as PowerPoint 2007 wrote them.
fn vml_preview(imp: &mut Imp, part: &Part, spid: &str) -> Option<deckcraft_model::MediaId> {
    for rel in part.rels.items.iter().filter(|r| r.kind == "vmlDrawing" && !r.external) {
        let key = crate::opc::norm(&rel.target);
        if !imp.vml_previews.contains_key(&key) {
            let index = vml_index(imp.pkg, &rel.target);
            imp.vml_previews.insert(key.clone(), index);
        }
        let Some(target) = imp.vml_previews.get(&key).and_then(|m| m.get(spid)).cloned() else { continue };
        return imp.media_for(&target);
    }
    None
}

/// Every shape id in a VML drawing with a picture, mapped to that picture's part: parsed once per
/// drawing however many objects look it up.
fn vml_index(pkg: &crate::opc::Package, vml: &str) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    let Some(doc) = pkg.xml(vml) else { return out };
    let rels = pkg.rels(vml);
    let mut shapes = vec![];
    doc.root.find_all("shape", &mut shapes);
    for s in shapes {
        let Some(id) = s.attr("id") else { continue };
        if out.contains_key(id) {
            continue; // the first shape with an id wins, as the old lookup did
        }
        let Some(img) = s.find("imagedata") else { continue };
        let Some(rid) = img.attr("o:relid").or(img.attr("r:id")) else { continue };
        if let Some(r) = rels.get(rid).filter(|r| !r.external) {
            out.insert(id.to_string(), r.target.clone());
        }
    }
    out
}

/// A diagram's data part, without the link to its cached drawing: that link is a slide
/// relationship id, which a save renumbers. Only diagrams whose drawing is missing or empty get
/// here, and those are drawn from their data.
fn diagram_data(imp: &mut Imp, target: &str) -> Option<deckcraft_model::MediaId> {
    let norm_target = crate::opc::norm(target);
    if let Some(&id) = imp.media_by_part.get(&norm_target) {
        return Some(id);
    }
    fn strip(e: &mut El, depth: usize) {
        if depth > crate::xml::MAX_DEPTH {
            return;
        }
        e.children.retain(|n| !matches!(n, crate::xml::Node::El(c) if c.is("ext") && c.find("dataModelExt").is_some()));
        for c in e.elements_mut() {
            strip(c, depth + 1);
        }
    }
    let mut doc = imp.pkg.xml(target)?;
    strip(&mut doc.root, 0);
    let xml = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n{}", doc.root.to_xml());
    let id = deckcraft_model::MediaId(imp.alloc());
    let content_type = imp.pkg.content_type(target)?;
    imp.media.push(deckcraft_model::MediaItem {
        id,
        name: crate::opc::file_of(target).to_string(),
        content_type,
        data: std::sync::Arc::new(xml.into_bytes()),
        link: None,
    });
    imp.media_by_part.insert(norm_target, id);
    Some(id)
}

/// SmartArt: read the pre-drawn shapes of its drawing part as a group.
fn smartart(imp: &mut Imp, part: &Part, gd: &El, frame: &Shape, depth: usize) -> Option<Shape> {
    if depth > 8 {
        return None;
    }
    let rel_ids = gd.child("relIds")?;
    let dm = part.rels.get(rel_ids.attr("r:dm").unwrap_or(""))?;
    let data = imp.pkg.xml(&dm.target)?;
    let drawing_rel = data.root.find("dataModelExt").and_then(|x| x.attr("relId")).and_then(|id| part.rels.get(id)).map(|r| r.target.clone())?;
    let mut d = imp.pkg.xml(&drawing_rel)?;
    super::resolve_mc(&mut d.root, 0);
    let dpart = Part::new(imp.pkg, &drawing_rel, &d);
    let tree = d.root.child("spTree")?;
    let mut sub = IdCtx { fresh: true, ..Default::default() };
    let children = sp_tree(imp, &dpart, &mut sub, tree, depth + 1);
    if children.is_empty() {
        return None;
    }
    let children = smartart_text_rects(imp, &mut sub, tree, children, depth + 1);
    let mut g = frame.clone();
    let x = frame.xfrm.unwrap_or_default();
    g.kind = ShapeKind::Group { children, child: Xfrm::new(0.0, 0.0, x.w, x.h) };
    g.diagram = diagram_outline(imp, part, rel_ids, &data.root);
    // Node pictures, from the blips of the data model's presentation points.
    if let Some(d) = &mut g.diagram {
        let rels = imp.pkg.rels(&dm.target);
        for (k, rid) in crate::dgm::node_pictures(&data.root) {
            let Some(target) = rels.get(&rid).filter(|r| !r.external).map(|r| r.target.clone()) else { continue };
            let media = imp.media_for(&target);
            if let Some(n) = d.nodes.get_mut(k) {
                n.picture = media;
            }
        }
    }
    Some(g)
}

/// A SmartArt drawing shape (`dsp:sp`) keeps its text rectangle in `dsp:txXfrm`, apart from the
/// shape's own box: the text of a narrow shape turned by 90° is often laid out across it. Put each
/// shape's text on that rectangle. `shapes` are what [`sp_tree`] read from `tree`, in order.
fn smartart_text_rects(imp: &mut Imp, ctx: &mut IdCtx, tree: &El, shapes: Vec<Shape>, depth: usize) -> Vec<Shape> {
    if depth > MAX_TREE_DEPTH {
        return shapes;
    }
    let mut out = Vec::with_capacity(shapes.len());
    let mut shapes = shapes.into_iter();
    // The same elements `sp_tree` turns into shapes.
    for e in tree.elements().filter(|e| matches!(e.local(), "sp" | "pic" | "grpSp" | "cxnSp" | "graphicFrame")) {
        let Some(mut s) = shapes.next() else { break };
        let mut text_box = None;
        match (e.local(), &mut s.kind) {
            ("grpSp", ShapeKind::Group { children, .. }) => {
                let kids = std::mem::take(children);
                *children = smartart_text_rects(imp, ctx, e, kids, depth + 1);
            }
            ("sp", ShapeKind::Shape) => {
                if let Some(t) = e.child("txXfrm") {
                    text_box = text_on_rect(imp, ctx, &mut s, dml::xfrm(t).0);
                }
            }
            _ => {}
        }
        out.push(s);
        out.extend(text_box);
    }
    out.extend(shapes);
    out
}

/// Lay out the text of `s` on `t`, a text rectangle in the same space as the shape's box, turned by
/// the shape's rotation plus its own. When that is the shape's text area shrunk by some margin, the
/// shape keeps its text with wider insets. Otherwise (a rectangle reaching outside the shape, or
/// turned against it) the text moves to a new frameless text shape over `s`, which is returned:
/// both export as plain DrawingML that any reader lays out the same way.
fn text_on_rect(imp: &mut Imp, ctx: &mut IdCtx, s: &mut Shape, t: Xfrm) -> Option<Shape> {
    const EPS: f64 = 0.01;
    let x = s.xfrm?;
    if !x.is_finite() || !t.is_finite() || t.w <= EPS || t.h <= EPS {
        return None;
    }
    let body = s.text.as_mut().filter(|b| !b.is_empty())?;
    let turn = |a: f64| a.rem_euclid(360.0);
    let upright = |a: f64| turn(a + EPS) < 2.0 * EPS;
    let bp = &mut body.body;
    if !x.flip_h
        && !x.flip_v
        && upright(t.rot)
        && bp.rot.is_none_or(upright)
        && bp.vert.is_none_or(|v| v == TextDir::Horizontal)
        && bp.upright != Some(true)
    {
        // Same orientation as the shape: express the rectangle as insets from the shape's text area.
        let c = x.affine().inverse() * t.center();
        let (l, top) = (c.x - t.w / 2.0, c.y - t.h / 2.0);
        let (r, b) = (l + t.w, top + t.h);
        let area = text_area(&s.geom, x.w, x.h);
        let extra = [l - area.x0, top - area.y0, area.x1 - r, area.y1 - b];
        if extra.iter().all(|v| v.is_finite() && *v >= -EPS) {
            let [el, et, er, eb] = extra.map(|v| v.max(0.0));
            bp.inset_l = Some(bp.inset_l.unwrap_or(7.2) + el);
            bp.inset_t = Some(bp.inset_t.unwrap_or(3.6) + et);
            bp.inset_r = Some(bp.inset_r.unwrap_or(7.2) + er);
            bp.inset_b = Some(bp.inset_b.unwrap_or(3.6) + eb);
            return None;
        }
    }
    if ctx.count >= MAX_SHAPES {
        return None;
    }
    ctx.count += 1;
    let text = s.text.take();
    // No fill, outline or effects; the style keeps only the text colour and font.
    let style = s.style.clone().map(|mut st| {
        st.fill_ref.0 = 0;
        st.line_ref.0 = 0;
        st.effect_ref.0 = 0;
        st
    });
    Some(Shape {
        id: ctx.assign(imp, None),
        name: s.name.clone(),
        xfrm: Some(Xfrm { rot: turn(x.rot + t.rot), flip_h: false, flip_v: false, ..t }),
        fill: Some(Fill::None),
        line: Some(deckcraft_model::style::Line { fill: Some(Fill::None), ..Default::default() }),
        style,
        text,
        click: s.click.clone(),
        hover: s.hover.clone(),
        ..Default::default()
    })
}

/// Where a shape's geometry puts its text, in shape-local points (as the renderer computes it).
fn text_area(geom: &Geom, w: f64, h: f64) -> deckcraft_geom::Rect {
    let full = deckcraft_geom::Rect::new(0.0, 0.0, w, h);
    match geom {
        Geom::Preset { name, adj } => {
            preset::build(name, w, h, adj).or_else(|| preset::build("rect", w, h, &[])).map(|g| g.text_rect).unwrap_or(full)
        }
        Geom::Custom { .. } => full,
    }
}

/// The outline of a SmartArt graphic (for the Text Pane), with its layout, quick style and colour
/// parts kept to be written back.
fn diagram_outline(imp: &Imp, part: &Part, rel_ids: &El, data: &El) -> Option<deckcraft_model::Diagram> {
    let nodes = crate::dgm::outline(data);
    if nodes.is_empty() {
        return None;
    }
    let raw = |attr: &str| -> Option<String> {
        let rel = part.rels.get(rel_ids.attr(attr)?)?;
        let bytes = imp.pkg.get(&rel.target)?;
        (bytes.len() <= 4 << 20).then(|| String::from_utf8(bytes.to_vec()).ok()).flatten()
    };
    let layout_xml = raw("r:lo");
    let (layout_id, cat) = layout_xml
        .as_deref()
        .and_then(|x| crate::xml::parse(x.as_bytes()).ok())
        .map(|d| {
            let cat = d.root.child("catLst").and_then(|c| c.child("cat")).and_then(|c| c.attr("type")).unwrap_or("").to_string();
            (d.root.attr("uniqueId").unwrap_or("").to_string(), cat)
        })
        .unwrap_or_default();
    let lo_type = data
        .path(&["ptLst"])
        .and_then(|l| l.children_named("pt").find(|p| p.attr("type") == Some("doc")))
        .and_then(|p| p.child("prSet"))
        .and_then(|p| p.attr("loTypeId"))
        .unwrap_or("");
    let id = if layout_id.is_empty() { lo_type.to_string() } else { layout_id };
    let pr = data.path(&["ptLst"]).and_then(|l| l.children_named("pt").find(|p| p.attr("type") == Some("doc"))).and_then(|p| p.child("prSet"));
    let style = crate::dgm::style_for(pr.and_then(|p| p.attr("qsTypeId")).unwrap_or("")).to_string();
    let colors = crate::dgm::colors_for(pr.and_then(|p| p.attr("csTypeId")).unwrap_or(""));
    Some(deckcraft_model::Diagram {
        kind: crate::dgm::kind_for(&id, &cat).to_string(),
        nodes,
        style,
        colors,
        imported: true,
        layout_id: id,
        layout_xml,
        style_xml: raw("r:qs"),
        colors_xml: raw("r:cs"),
    })
}

fn table(imp: &mut Imp, part: &Part, tbl: &El) -> Table {
    // A table naming no style is drawn by PowerPoint with thin dark grid lines ("No Style, Table
    // Grid"), not without lines.
    let mut t = Table { style: "grid".into(), first_row: false, band_row: false, ..Default::default() };
    if let Some(pr) = tbl.child("tblPr") {
        t.background = pr.elements().find(|e| e.local().ends_with("Fill") && e.local() != "noFill").and_then(|f| dml::fill_el(imp, part, f));
        t.first_row = pr.bool("firstRow").unwrap_or(false);
        t.first_col = pr.bool("firstCol").unwrap_or(false);
        t.last_row = pr.bool("lastRow").unwrap_or(false);
        t.last_col = pr.bool("lastCol").unwrap_or(false);
        t.band_row = pr.bool("bandRow").unwrap_or(false);
        t.band_col = pr.bool("bandCol").unwrap_or(false);
        if let Some(id) = pr.child("tableStyleId").map(|e| e.text()) {
            // A style the file defines is drawn from its definition (by its GUID).
            // DeckCraft's own styles (written with GUIDs derived from their ids) map back to them.
            let up = id.trim().to_ascii_uppercase();
            t.style =
                if imp.table_style_ids.contains(&up) && !crate::tables::is_own_guid(&up) { up } else { crate::tables::style_from_guid(id.trim()) };
        }
    }
    t.cols = tbl
        .child("tblGrid")
        .map(|g| g.children_named("gridCol").take(1000).map(|c| dml::pt(c, "w").unwrap_or(72.0).clamp(0.0, 20_000.0)).collect())
        .unwrap_or_default();
    for tr in tbl.children_named("tr").take(1000) {
        let mut row = Row { height: dml::pt(tr, "h").unwrap_or(0.0).clamp(0.0, 20_000.0), cells: vec![] };
        for tc in tr.children_named("tc").take(1000) {
            row.cells.push(cell(imp, part, tc));
        }
        t.rows.push(row);
    }
    if t.cols.is_empty() {
        let n = t.rows.iter().map(|r| r.cells.len()).max().unwrap_or(0);
        t.cols = vec![72.0; n];
    }
    t.normalize();
    t
}

fn cell(imp: &mut Imp, part: &Part, tc: &El) -> Cell {
    let mut c = Cell {
        grid_span: tc.u32("gridSpan").unwrap_or(1).clamp(1, 1000),
        row_span: tc.u32("rowSpan").unwrap_or(1).clamp(1, 1000),
        h_merge: tc.bool("hMerge").unwrap_or(false),
        v_merge: tc.bool("vMerge").unwrap_or(false),
        ..Default::default()
    };
    if let Some(tx) = tc.child("txBody") {
        c.text = dml::text_body(imp, part, tx);
    }
    if let Some(pr) = tc.child("tcPr") {
        if ["marL", "marR", "marT", "marB"].iter().any(|n| pr.has_attr(n)) {
            c.margins = Some([
                dml::pt(pr, "marL").unwrap_or(7.2),
                dml::pt(pr, "marR").unwrap_or(7.2),
                dml::pt(pr, "marT").unwrap_or(3.6),
                dml::pt(pr, "marB").unwrap_or(3.6),
            ]);
        }
        c.anchor = pr.attr("anchor").and_then(Anchor::from_xml);
        c.vertical = pr.attr("vert").and_then(TextDir::from_xml);
        for (i, n) in ["lnL", "lnR", "lnT", "lnB"].iter().enumerate() {
            if let Some(ln) = pr.child(n)
                && let Some(b) = c.borders.get_mut(i)
            {
                *b = Some(dml::line(imp, part, ln));
            }
        }
        c.diag_down = pr.child("lnTlToBr").map(|l| dml::line(imp, part, l));
        c.diag_up = pr.child("lnBlToTr").map(|l| dml::line(imp, part, l));
        c.fill = dml::fill_of(imp, part, pr);
    }
    c
}
