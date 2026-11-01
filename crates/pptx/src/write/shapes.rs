//! Shape trees: autoshapes, pictures, groups, connectors, tables, charts, media, ink, 3-D models and kept objects.

use std::collections::{HashMap, HashSet};

use deckcraft_geom::Xfrm;
use deckcraft_model::style::Fill;
use deckcraft_model::table::Table;
use deckcraft_model::{Geom, PhType, Shape, ShapeId, ShapeKind, ShapeStyle, walk};

use super::dml::{self, emu, emu_pos};
use super::{Exp, Out};
use crate::opc::NS_MC;
use crate::xml::{A, W};

/// cNvPr ids for one part.
pub struct IdMap {
    pub map: HashMap<ShapeId, u32>,
    next: u32,
}

impl IdMap {
    pub fn build(shapes: &[Shape]) -> Self {
        let mut map = HashMap::new();
        let mut used = HashSet::new();
        let mut max = 1u32;
        walk(shapes, &mut |s, _| {
            if s.id.0 >= 2 && s.id.0 < i32::MAX as u32 {
                max = max.max(s.id.0);
            }
        });
        let mut next = max;
        walk(shapes, &mut |s, _| {
            let id = if s.id.0 >= 2 && s.id.0 < i32::MAX as u32 && !used.contains(&s.id.0) {
                s.id.0
            } else {
                next = next.saturating_add(1);
                next
            };
            used.insert(id);
            map.entry(s.id).or_insert(id);
        });
        IdMap { map, next }
    }
    pub fn get(&self, id: ShapeId) -> Option<u32> {
        self.map.get(&id).copied()
    }
    pub fn fresh(&mut self) -> u32 {
        self.next = self.next.saturating_add(1);
        self.next
    }
}

pub struct Ctx<'a> {
    pub ids: &'a mut IdMap,
    /// The shape id written for the shape currently being written (duplicate model ids get fresh ones).
    pub written: HashSet<u32>,
}

impl Ctx<'_> {
    fn id_for(&mut self, s: &Shape) -> u32 {
        let id = self.ids.get(s.id).unwrap_or(0);
        if id >= 2 && self.written.insert(id) {
            return id;
        }
        let f = self.ids.fresh();
        self.written.insert(f);
        f
    }
}

fn preset_name(name: &str) -> &str {
    if name == "textBox" || name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric()) { "rect" } else { name }
}

/// Non-adjust shape guides some presets carry in their `avLst` (ECMA-376 default values).
fn extra_guides(name: &str) -> &'static [(&'static str, i64)] {
    match name {
        "star5" | "pentagon" => &[("hf", 105_146), ("vf", 110_557)],
        "star7" | "heptagon" => &[("hf", 102_572), ("vf", 105_210)],
        "star6" => &[("hf", 115_470)],
        "star10" => &[("hf", 105_146)],
        "hexagon" => &[("vf", 115_470)],
        "decagon" => &[("vf", 105_146)],
        _ => &[],
    }
}

pub fn geometry(w: &mut W, g: &Geom) {
    match g {
        Geom::Preset { name, adj } => {
            let name = preset_name(name);
            w.open("a:prstGeom", A::new().a("prst", name));
            if adj.is_empty() {
                w.empty0("a:avLst");
            } else {
                // PowerPoint wants the complete guide list once any is given.
                let defaults = deckcraft_geom::preset::info(name).map(|i| i.defaults).unwrap_or(&[]);
                let mut vals: Vec<f64> = adj.iter().take(16).copied().collect();
                while vals.len() < defaults.len() {
                    vals.push(defaults.get(vals.len()).copied().unwrap_or(0.0));
                }
                w.open0("a:avLst");
                // A lone guide is `adj`, except in these presets, which name it `adj1` (as PowerPoint
                // writes them); PowerPoint won't open a file that renames it (#214).
                let adj1 = matches!(name, "bentConnector3" | "curvedConnector3" | "frame" | "mathPlus" | "mathMinus" | "mathMultiply");
                let single = !adj1 && if defaults.is_empty() { vals.len() == 1 } else { defaults.len() == 1 };
                for (i, v) in vals.iter().enumerate() {
                    let n = if single && i == 0 { "adj".to_string() } else { format!("adj{}", i + 1) };
                    let v = if v.is_finite() { v.round().clamp(-2.0e9, 2.0e9) as i64 } else { 0 };
                    w.empty("a:gd", A::new().a("name", n).a("fmla", format!("val {v}")));
                }
                for (n, v) in extra_guides(name) {
                    w.empty("a:gd", A::new().a("name", *n).a("fmla", format!("val {v}")));
                }
                w.close("a:avLst");
            }
            w.close("a:prstGeom");
        }
        Geom::Custom { paths } => {
            if paths.is_empty() {
                w.open("a:prstGeom", A::new().a("prst", "rect"));
                w.empty0("a:avLst");
                w.close("a:prstGeom");
            } else {
                crate::custgeom::write_cust_geom(w, paths);
            }
        }
    }
}

fn cnvpr(w: &mut W, x: &mut Exp, o: &mut Out, id: u32, s: &Shape) {
    let a = A::new()
        .a("id", id)
        .a("name", if s.name.is_empty() { format!("Shape {id}") } else { s.name.clone() })
        .o("descr", (!s.descr.is_empty()).then_some(s.descr.as_str()))
        .t("hidden", s.hidden)
        .o("title", (!s.title.is_empty()).then_some(s.title.as_str()));
    if s.click.is_none() && s.hover.is_none() && !s.decorative {
        w.empty("p:cNvPr", a);
        return;
    }
    w.open("p:cNvPr", a);
    if let Some(h) = &s.click {
        dml::hyperlink(w, x, o, "a:hlinkClick", h);
    }
    if let Some(h) = &s.hover {
        dml::hyperlink(w, x, o, "a:hlinkHover", h);
    }
    if s.decorative {
        w.open0("a:extLst");
        w.open("a:ext", A::new().a("uri", "{C183D7F6-B498-43B3-948B-1728B52AA6E4}"));
        w.empty("adec:decorative", A::new().a("xmlns:adec", "http://schemas.microsoft.com/office/drawing/2017/decorative").a("val", "1"));
        w.close("a:ext");
        w.close("a:extLst");
    }
    w.close("p:cNvPr");
}

fn nv_pr(w: &mut W, x: &mut Exp, o: &mut Out, s: &Shape) {
    if s.ph.is_none() && s.tags.is_empty() {
        w.empty0("p:nvPr");
        return;
    }
    w.open0("p:nvPr");
    if let Some(ph) = &s.ph {
        let ty = match ph.kind {
            PhType::Obj => None,
            k => Some(k.xml()),
        };
        let sz = ph.size.as_deref().filter(|z| matches!(*z, "full" | "half" | "quarter"));
        w.empty(
            "p:ph",
            A::new()
                .o("type", ty)
                .o("orient", ph.vertical.then_some("vert"))
                .o("sz", sz)
                .o("idx", (ph.idx != 0).then_some(ph.idx))
                .t("hasCustomPrompt", ph.has_custom_prompt),
        );
    }
    super::cust_data(w, x, o, &s.tags);
    w.close("p:nvPr");
}

/// Locking attributes. Lock writes the full set PowerPoint writes for the element's kind (and
/// reads back as locked): see `read::shapes::is_locked`.
fn locks(w: &mut W, tag: &str, s: &Shape, extra: A) {
    let a = extra.t("noGrp", s.ph.is_some() || s.locked);
    let a = match tag {
        "a:grpSpLocks" => a.t("noUngrp", s.locked).t("noRot", s.locked).t("noMove", s.locked).t("noResize", s.locked),
        _ => {
            let a = a
                .t("noRot", s.locked)
                .t("noMove", s.locked)
                .t("noResize", s.locked)
                .t("noEditPoints", s.locked)
                .t("noAdjustHandles", s.locked)
                .t("noChangeArrowheads", s.locked)
                .t("noChangeShapeType", s.locked);
            if tag == "a:picLocks" { a.t("noCrop", s.locked) } else { a }
        }
    };
    if a.v.is_empty() {
        return;
    }
    w.empty(tag, a);
}

fn style(w: &mut W, st: &ShapeStyle) {
    style_tag(w, st, "p:style");
}

fn style_tag(w: &mut W, st: &ShapeStyle, tag: &str) {
    w.open0(tag);
    for (tag, (idx, c)) in [("a:lnRef", &st.line_ref), ("a:fillRef", &st.fill_ref), ("a:effectRef", &st.effect_ref)] {
        w.open(tag, A::new().a("idx", idx));
        dml::color(w, c);
        w.close(tag);
    }
    let idx = match st.font_ref.0.as_str() {
        "major" | "minor" | "none" => st.font_ref.0.as_str(),
        _ => "minor",
    };
    match &st.font_ref.1 {
        Some(c) => {
            w.open("a:fontRef", A::new().a("idx", idx));
            dml::color(w, c);
            w.close("a:fontRef");
        }
        None => w.empty("a:fontRef", A::new().a("idx", idx)),
    }
    w.close(tag);
}

/// `p:spPr` contents (also used for pictures and connectors).
fn sp_pr(w: &mut W, x: &mut Exp, o: &mut Out, s: &Shape, geom: bool) {
    sp_pr_tag(w, x, o, s, geom, "p:spPr");
}

fn sp_pr_tag(w: &mut W, x: &mut Exp, o: &mut Out, s: &Shape, geom: bool, tag: &str) {
    w.open0(tag);
    if let Some(xf) = &s.xfrm {
        dml::xfrm(w, "a:xfrm", xf, None);
    }
    if geom && (s.xfrm.is_some() || s.ph.is_none() || !matches!(&s.geom, Geom::Preset { name, adj } if name == "rect" && adj.is_empty())) {
        geometry(w, &s.geom);
    }
    if let Some(f) = &s.fill {
        dml::fill(w, x, o, f);
    }
    if let Some(l) = &s.line {
        dml::line(w, x, o, "a:ln", l);
    }
    if let Some(fx) = &s.effects {
        dml::effects(w, fx);
        if let Some(raw) = &fx.raw3d {
            w.raw(raw);
        } else if let Some(xml) = fx.bevel.as_deref().and_then(dml::bevel_sp3d) {
            w.raw(&xml);
        }
    }
    w.close(tag);
}

/// Write the children of a shape tree.
pub fn shapes(w: &mut W, x: &mut Exp, o: &mut Out, cx: &mut Ctx, list: &[Shape], depth: usize) {
    if depth > 32 {
        return;
    }
    for s in list {
        shape(w, x, o, cx, s, depth);
    }
}

pub fn shape(w: &mut W, x: &mut Exp, o: &mut Out, cx: &mut Ctx, s: &Shape, depth: usize) {
    match &s.kind {
        ShapeKind::Shape => {
            if s.text.as_ref().is_some_and(dml::has_math) {
                // Equations: OMML for readers that know it, plain text otherwise.
                w.open("mc:AlternateContent", A::new().a("xmlns:mc", NS_MC));
                w.open("mc:Choice", A::new().a("xmlns:a14", crate::opc::NS_A14).a("Requires", "a14"));
                let id = cx.id_for(s);
                sp(w, x, o, id, s, true);
                w.close("mc:Choice");
                w.open0("mc:Fallback");
                sp(w, x, o, id, s, false);
                w.close("mc:Fallback");
                w.close("mc:AlternateContent");
            } else {
                let id = cx.id_for(s);
                sp(w, x, o, id, s, false);
            }
        }
        ShapeKind::Picture { fill } => {
            let id = cx.id_for(s);
            w.open0("p:pic");
            w.open0("p:nvPicPr");
            cnvpr(w, x, o, id, s);
            w.open0("p:cNvPicPr");
            locks(w, "a:picLocks", s, A::new().a("noChangeAspect", "1"));
            w.close("p:cNvPicPr");
            nv_pr(w, x, o, s);
            w.close("p:nvPicPr");
            dml::blip_fill(w, x, o, "p:blipFill", fill);
            let mut s2 = s.clone();
            if matches!(s2.fill, Some(Fill::Picture(_) | Fill::Background | Fill::Group)) {
                s2.fill = None;
            }
            sp_pr(w, x, o, &s2, true);
            if let Some(st) = &s.style {
                style(w, st);
            }
            w.close("p:pic");
        }
        ShapeKind::Group { children, child } if s.diagram.is_some() && depth < 32 => {
            if let Some(d) = &s.diagram {
                diagram(w, x, o, cx, s, d, children, child);
            }
        }
        ShapeKind::Group { children, child } => {
            let id = cx.id_for(s);
            w.open0("p:grpSp");
            w.open0("p:nvGrpSpPr");
            cnvpr(w, x, o, id, s);
            w.open0("p:cNvGrpSpPr");
            locks(w, "a:grpSpLocks", s, A::new());
            w.close("p:cNvGrpSpPr");
            nv_pr(w, x, o, s);
            w.close("p:nvGrpSpPr");
            w.open0("p:grpSpPr");
            let xf = s.xfrm.unwrap_or(*child);
            dml::xfrm(w, "a:xfrm", &xf, Some(child));
            if let Some(f) = &s.fill {
                dml::fill(w, x, o, f);
            }
            if let Some(fx) = &s.effects {
                dml::effects(w, fx);
            }
            w.close("p:grpSpPr");
            shapes(w, x, o, cx, children, depth + 1);
            w.close("p:grpSp");
        }
        ShapeKind::Connector { start, end } => {
            let id = cx.id_for(s);
            w.open0("p:cxnSp");
            w.open0("p:nvCxnSpPr");
            cnvpr(w, x, o, id, s);
            let st = start.and_then(|(sid, site)| cx.ids.get(sid).map(|i| (i, site)));
            let en = end.and_then(|(sid, site)| cx.ids.get(sid).map(|i| (i, site)));
            if st.is_none() && en.is_none() && !s.locked {
                w.empty0("p:cNvCxnSpPr");
            } else {
                w.open0("p:cNvCxnSpPr");
                locks(w, "a:cxnSpLocks", s, A::new());
                if let Some((i, site)) = st {
                    w.empty("a:stCxn", A::new().a("id", i).a("idx", site));
                }
                if let Some((i, site)) = en {
                    w.empty("a:endCxn", A::new().a("id", i).a("idx", site));
                }
                w.close("p:cNvCxnSpPr");
            }
            nv_pr(w, x, o, s);
            w.close("p:nvCxnSpPr");
            let mut s2 = s.clone();
            if matches!(s2.fill, Some(Fill::Background)) {
                s2.fill = None;
            }
            if s2.geom.preset_name() == Some("rect") {
                s2.geom = Geom::preset("line");
            }
            sp_pr(w, x, o, &s2, true);
            if let Some(st) = &s.style {
                style(w, st);
            }
            w.close("p:cxnSp");
        }
        ShapeKind::Table(t) => {
            let id = cx.id_for(s);
            frame_open(w, x, o, id, s);
            w.open("a:graphicData", A::new().a("uri", "http://schemas.openxmlformats.org/drawingml/2006/table"));
            table(w, x, o, t);
            w.close("a:graphicData");
            frame_close(w);
        }
        ShapeKind::Chart(c) => {
            let id = cx.id_for(s);
            // An unchanged Office 2016 chart goes back as itself, with a plain chart of the same
            // data for readers without `cx1`, as PowerPoint writes it.
            let ex = x.chartex_rel(o, c);
            if let Some(exid) = &ex {
                w.open("mc:AlternateContent", A::new().a("xmlns:mc", "http://schemas.openxmlformats.org/markup-compatibility/2006"));
                w.open("mc:Choice", A::new().a("xmlns:cx1", "http://schemas.microsoft.com/office/drawing/2015/9/8/chartex").a("Requires", "cx1"));
                frame_open(w, x, o, id, s);
                w.open("a:graphicData", A::new().a("uri", super::chart::NS_CX));
                w.empty("cx:chart", A::new().a("xmlns:cx", super::chart::NS_CX).a("r:id", exid));
                w.close("a:graphicData");
                frame_close(w);
                w.close("mc:Choice");
                w.open0("mc:Fallback");
            }
            let mut plain = (**c).clone();
            plain.raw = plain.raw.filter(|r| !r.contains(super::chart::NS_CX));
            if ex.is_some() {
                // The kept workbook belongs to the `chartEx` part; the fallback has none.
                plain.kept = Default::default();
            }
            let text_color_explicit =
                if plain.raw.is_none() { chart_text_defaults(&mut plain, s.xfrm.unwrap_or_default()) } else { plain.text_color.is_some() };
            let rid = x.chart_rel(o, &plain, text_color_explicit);
            frame_open(w, x, o, id, s);
            w.open("a:graphicData", A::new().a("uri", "http://schemas.openxmlformats.org/drawingml/2006/chart"));
            w.empty("c:chart", A::new().a("xmlns:c", crate::opc::NS_C).a("r:id", rid));
            w.close("a:graphicData");
            frame_close(w);
            if ex.is_some() {
                w.close("mc:Fallback");
                w.close("mc:AlternateContent");
            }
        }
        ShapeKind::Media(m) => {
            let id = cx.id_for(s);
            media(w, x, o, id, s, m);
        }
        ShapeKind::Model3d { media, poster } => write_model(w, x, o, cx, s, *media, *poster),
        ShapeKind::Ink { strokes } => {
            // Ink becomes a group of freeform lines.
            let id = cx.id_for(s);
            let xf = s.xfrm.unwrap_or_default();
            w.open0("p:grpSp");
            w.open0("p:nvGrpSpPr");
            cnvpr(w, x, o, id, s);
            w.empty0("p:cNvGrpSpPr");
            w.empty0("p:nvPr");
            w.close("p:nvGrpSpPr");
            w.open0("p:grpSpPr");
            dml::xfrm(w, "a:xfrm", &xf, Some(&xf));
            w.close("p:grpSpPr");
            for (i, st) in strokes.iter().enumerate().take(10_000) {
                let Some(stroke) = ink_shape(st, i) else { continue };
                let sid = cx.ids.fresh();
                sp(w, x, o, sid, &stroke, false);
            }
            w.close("p:grpSp");
        }
        ShapeKind::Opaque { xml, preview, label, parts } => opaque(w, x, o, cx, s, xml, *preview, label, parts),
    }
}

fn model_rel(x: &mut Exp, o: &mut Out, media: deckcraft_model::MediaId) -> Option<String> {
    if x.p.media(media).is_none_or(|m| m.data.is_empty()) {
        return None;
    }
    x.media_rel(o, media, crate::opc::RT_MODEL3D)
}

fn write_model_part(w: &mut W, x: &mut Exp, o: &mut Out, id: u32, s: &Shape, rid: &str) {
    w.open("p:contentPart", A::new().a("r:id", rid));
    w.open0("p:nvContentPartPr");
    cnvpr(w, x, o, id, s);
    w.empty0("p:cNvContentPartPr");
    w.empty0("p:nvPr");
    w.close("p:nvContentPartPr");
    if let Some(xf) = &s.xfrm {
        dml::xfrm(w, "p:xfrm", xf, None);
    }
    w.close("p:contentPart");
}

fn write_model(
    w: &mut W,
    x: &mut Exp,
    o: &mut Out,
    cx: &mut Ctx,
    s: &Shape,
    media: deckcraft_model::MediaId,
    poster: Option<deckcraft_model::MediaId>,
) {
    let Some(rid) = model_rel(x, o, media) else {
        log::warn!("pptx: 3-D model '{}' has no embedded part", s.name);
        return;
    };
    let id = cx.id_for(s);
    let poster = poster.filter(|p| x.p.media(*p).is_some_and(|m| !m.data.is_empty()));
    let Some(poster) = poster else {
        write_model_part(w, x, o, id, s, &rid);
        return;
    };
    // Older readers draw the fallback picture; readers that know `am3d` use the model.
    w.open("mc:AlternateContent", A::new().a("xmlns:mc", NS_MC));
    w.open("mc:Choice", A::new().a("xmlns:am3d", "http://schemas.microsoft.com/office/drawing/2017/model3d").a("Requires", "am3d"));
    write_model_part(w, x, o, id, s, &rid);
    w.close("mc:Choice");
    w.open0("mc:Fallback");
    let pic = Shape {
        id: ShapeId(0),
        name: s.name.clone(),
        xfrm: s.xfrm,
        kind: ShapeKind::Picture { fill: deckcraft_model::style::PictureFill { media: poster, ..Default::default() } },
        ..Default::default()
    };
    let pid = cx.ids.fresh();
    shape_with_id(w, x, o, cx, &pic, pid);
    w.close("mc:Fallback");
    w.close("mc:AlternateContent");
}

/// A SmartArt graphic: a graphic frame naming its data, layout, quick style and colour parts,
/// with the group's shapes as the cached drawing PowerPoint shows until it lays the diagram out.
#[allow(clippy::too_many_arguments)]
fn diagram(w: &mut W, x: &mut Exp, o: &mut Out, cx: &mut Ctx, s: &Shape, d: &deckcraft_model::Diagram, children: &[Shape], child: &Xfrm) {
    use crate::dgm;
    let id = cx.id_for(s);
    let n = x.next_diagram();
    let frame = s.xfrm.unwrap_or(*child);
    let name = |kind: &str| format!("ppt/diagrams/{kind}{n}.xml");
    // The drawing: the group's shapes in the frame's own coordinates.
    let dname = name("drawing");
    let mut dout = Out::new(&dname);
    let mut dw = W::new();
    dw.open(
        "dsp:drawing",
        A::new().a("xmlns:dgm", dgm::NS_DGM).a("xmlns:dsp", dgm::NS_DSP).a("xmlns:a", crate::opc::NS_A).a("xmlns:r", crate::opc::NS_R),
    );
    dw.open0("dsp:spTree");
    dw.open0("dsp:nvGrpSpPr");
    dw.empty("dsp:cNvPr", A::new().a("id", 0).a("name", ""));
    dw.empty0("dsp:cNvGrpSpPr");
    dw.close("dsp:nvGrpSpPr");
    dw.empty0("dsp:grpSpPr");
    let (sx, sy) = (frame.w / child.w.max(1e-6), frame.h / child.h.max(1e-6));
    let mut k = 0;
    let mut flat = vec![];
    walk(children, &mut |c, _| {
        if matches!(c.kind, ShapeKind::Shape) {
            flat.push(c.clone());
        }
    });
    for mut c in flat.into_iter().take(10_000) {
        if let Some(xf) = &mut c.xfrm {
            *xf = Xfrm { x: (xf.x - child.x) * sx, y: (xf.y - child.y) * sy, w: xf.w * sx, h: xf.h * sy, ..*xf };
        }
        k += 1;
        dw.open("dsp:sp", A::new().a("modelId", format!("{{{:08X}-5D1A-4C0A-9E5B-{k:012X}}}", s.id.0)));
        dw.open0("dsp:nvSpPr");
        dw.empty("dsp:cNvPr", A::new().a("id", 0).a("name", ""));
        dw.empty0("dsp:cNvSpPr");
        dw.close("dsp:nvSpPr");
        sp_pr_tag(&mut dw, x, &mut dout, &c, true, "dsp:spPr");
        if let Some(st) = &c.style {
            style_tag(&mut dw, st, "dsp:style");
        }
        if let Some(tb) = &c.text {
            dml::text_body(&mut dw, x, &mut dout, "dsp:txBody", tb, false);
        }
        dw.close("dsp:sp");
    }
    dw.close("dsp:spTree");
    dw.close("dsp:drawing");
    x.pkg.xml_part(&dname, dgm::CT_DRAWING, dw.finish());
    x.pkg.rels(&dname, &dout.rels);
    let rel = |o: &mut Out, kind: &str, part: &str| o.rels.add_new(kind, &crate::opc::relative(&o.name, part), false);
    let drawing_rel = rel(o, dgm::RT_DRAWING, &dname);
    let data = name("data");
    // Node pictures: on the data model's presentation points, through the data part's rels.
    let mut mout = Out::new(&data);
    let pics: Vec<(usize, String)> =
        d.nodes.iter().enumerate().filter_map(|(k, n)| n.picture.and_then(|m| x.media_rel(&mut mout, m, "image")).map(|r| (k, r))).collect();
    x.pkg.xml_part(&data, dgm::CT_DATA, dgm::data_xml(d, s.id.0, &drawing_rel, &pics));
    x.pkg.rels(&data, &mout.rels);
    let dm = rel(o, "diagramData", &data);
    let layout = name("layout");
    let lo_xml = d.layout_xml.clone().map(String::into_bytes).unwrap_or_else(|| dgm::layout_xml(&d.kind));
    x.pkg.xml_part(&layout, dgm::CT_LAYOUT, lo_xml);
    let lo = rel(o, "diagramLayout", &layout);
    let style = name("quickStyle");
    x.pkg.xml_part(&style, dgm::CT_STYLE, d.style_xml.clone().map(String::into_bytes).unwrap_or_else(|| dgm::style_xml(&d.style)));
    let qs = rel(o, "diagramQuickStyle", &style);
    let colors = name("colors");
    x.pkg.xml_part(&colors, dgm::CT_COLORS, d.colors_xml.clone().map(String::into_bytes).unwrap_or_else(|| dgm::colors_xml(&d.colors)));
    let cs = rel(o, "diagramColors", &colors);
    let mut fs = s.clone();
    fs.xfrm = Some(frame);
    frame_open(w, x, o, id, &fs);
    w.open("a:graphicData", A::new().a("uri", "http://schemas.openxmlformats.org/drawingml/2006/diagram"));
    w.empty("dgm:relIds", A::new().a("xmlns:dgm", dgm::NS_DGM).a("r:dm", dm).a("r:lo", lo).a("r:qs", qs).a("r:cs", cs));
    w.close("a:graphicData");
    frame_close(w);
}

fn ink_shape(st: &deckcraft_model::InkStroke, i: usize) -> Option<Shape> {
    let pts: Vec<(f64, f64)> = st.points.iter().filter(|p| p.0.is_finite() && p.1.is_finite()).map(|p| (p.0, p.1)).collect();
    let (x0, y0) = pts.iter().fold((f64::MAX, f64::MAX), |a, p| (a.0.min(p.0), a.1.min(p.1)));
    let (x1, y1) = pts.iter().fold((f64::MIN, f64::MIN), |a, p| (a.0.max(p.0), a.1.max(p.1)));
    if pts.len() < 2 {
        return None;
    }
    let (w, h) = ((x1 - x0).max(1.0), (y1 - y0).max(1.0));
    // PPTX keeps one line width. A pressured stroke exports at its mean sample width;
    // a legacy flat `0.5` stroke stays at the pen width.
    let legacy = deckcraft_model::ink_pressure_is_legacy(&st.points);
    let width = if legacy {
        st.width
    } else {
        let n = st.points.len().max(1) as f64;
        st.points.iter().map(|p| deckcraft_model::ink_sample_width(st.width, p.2, false)).sum::<f64>() / n
    };
    let mut d = String::new();
    for (k, (px, py)) in pts.iter().enumerate() {
        d.push_str(if k == 0 { "M " } else { "L " });
        d.push_str(&format!("{:.2} {:.2} ", px - x0, py - y0));
    }
    let mut c = deckcraft_model::style::ColorRef::rgb(st.color.with_alpha(255));
    if st.highlighter {
        c = c.with(deckcraft_color::ColorTransform::Alpha(50_000));
    }
    Some(Shape {
        id: ShapeId(0),
        name: format!("Ink {}", i + 1),
        xfrm: Some(Xfrm::new(x0, y0, w, h)),
        geom: Geom::Custom { paths: vec![deckcraft_model::CustomPath { w, h, d, fill: deckcraft_geom::preset::FillMode::None, stroke: true }] },
        fill: Some(Fill::None),
        line: Some(deckcraft_model::Line {
            cap: Some(deckcraft_model::style::LineCap::Round),
            join: Some(deckcraft_model::style::LineJoin::Round),
            ..deckcraft_model::Line::solid(c, width.max(0.25))
        }),
        ..Default::default()
    })
}

fn sp(w: &mut W, x: &mut Exp, o: &mut Out, id: u32, s: &Shape, math: bool) {
    w.open("p:sp", A::new().t("useBgFill", matches!(s.fill, Some(Fill::Background))));
    w.open0("p:nvSpPr");
    cnvpr(w, x, o, id, s);
    let mut cnv = A::new();
    if s.text_box {
        cnv = cnv.a("txBox", "1");
    }
    let lock = s.ph.is_some() || s.locked;
    if lock {
        w.open("p:cNvSpPr", cnv);
        locks(w, "a:spLocks", s, A::new());
        w.close("p:cNvSpPr");
    } else {
        w.empty("p:cNvSpPr", cnv);
    }
    nv_pr(w, x, o, s);
    w.close("p:nvSpPr");
    let tmp;
    let sref = if matches!(s.fill, Some(Fill::Background)) {
        let mut c = s.clone();
        c.fill = None;
        tmp = c;
        &tmp
    } else {
        s
    };
    sp_pr(w, x, o, sref, true);
    if let Some(st) = &s.style {
        style(w, st);
    }
    if let Some(tb) = &s.text {
        // A shape's text without its own anchor sits in the middle in the model; a file without
        // one would put it at the top, so say so.
        if s.ph.is_none() && !s.text_box && tb.body.anchor.is_none() {
            let mut tb = tb.clone();
            tb.body.anchor = Some(deckcraft_model::text::Anchor::Middle);
            dml::text_body(w, x, o, "p:txBody", &tb, math);
        } else {
            dml::text_body(w, x, o, "p:txBody", tb, math);
        }
    }
    w.close("p:sp");
}

/// Writes the text colour and sizes DeckCraft draws a chart in when the chart has none of its own
/// (without them PowerPoint falls back to black text at 10 pt): the theme's text at 65 %, and a
/// size scaled to the frame, as `render::chart` picks it.
fn chart_text_defaults(c: &mut deckcraft_model::Chart, xf: deckcraft_geom::Xfrm) -> bool {
    use deckcraft_color::ColorTransform;
    use deckcraft_model::{ColorRef, SchemeSlot};
    let text_color_explicit = c.text_color.is_some();
    c.text_color.get_or_insert_with(|| ColorRef::scheme(SchemeSlot::Tx1).with(ColorTransform::LumMod(65000)).with(ColorTransform::LumOff(35000)));
    let base = c.text_size.unwrap_or_else(|| (xf.w.min(xf.h) / 22.0).clamp(7.0, 18.0));
    c.text_size = Some(base);
    c.title_size.get_or_insert(base * 1.4);
    text_color_explicit
}

fn frame_open(w: &mut W, x: &mut Exp, o: &mut Out, id: u32, s: &Shape) {
    w.open0("p:graphicFrame");
    w.open0("p:nvGraphicFramePr");
    cnvpr(w, x, o, id, s);
    w.open0("p:cNvGraphicFramePr");
    w.empty("a:graphicFrameLocks", A::new().a("noGrp", "1").t("noDrilldown", s.locked).t("noMove", s.locked).t("noResize", s.locked));
    w.close("p:cNvGraphicFramePr");
    nv_pr(w, x, o, s);
    w.close("p:nvGraphicFramePr");
    let xf = s.xfrm.unwrap_or_default();
    let mut xf0 = xf;
    xf0.rot = 0.0;
    xf0.flip_h = false;
    xf0.flip_v = false;
    dml::xfrm(w, "p:xfrm", &xf0, None);
    w.open0("a:graphic");
}

fn frame_close(w: &mut W) {
    w.close("a:graphic");
    w.close("p:graphicFrame");
}

fn table(w: &mut W, x: &mut Exp, o: &mut Out, t: &Table) {
    let mut t = t.clone();
    t.normalize();
    w.open0("a:tbl");
    let (guid, _) = crate::tables::guid_for_style(&t.style);
    if !x.table_styles.contains(&t.style) {
        x.table_styles.push(t.style.clone());
    }
    w.open(
        "a:tblPr",
        A::new()
            .t("firstRow", t.first_row)
            .t("firstCol", t.first_col)
            .t("lastRow", t.last_row)
            .t("lastCol", t.last_col)
            .t("bandRow", t.band_row)
            .t("bandCol", t.band_col),
    );
    if let Some(f) = &t.background {
        dml::fill(w, x, o, f);
    }
    w.elt("a:tableStyleId", &guid);
    w.close("a:tblPr");
    w.open0("a:tblGrid");
    for c in &t.cols {
        w.empty("a:gridCol", A::new().a("w", emu_pos(*c)));
    }
    w.close("a:tblGrid");
    for row in &t.rows {
        w.open("a:tr", A::new().a("h", emu_pos(row.height)));
        for c in &row.cells {
            w.open(
                "a:tc",
                A::new()
                    .o("gridSpan", (c.grid_span > 1).then_some(c.grid_span))
                    .o("rowSpan", (c.row_span > 1).then_some(c.row_span))
                    .t("hMerge", c.h_merge)
                    .t("vMerge", c.v_merge),
            );
            dml::text_body(w, x, o, "a:txBody", &c.text, false);
            let mut a = A::new();
            if let Some(m) = c.margins {
                a = a.a("marL", emu(m[0])).a("marR", emu(m[1])).a("marT", emu(m[2])).a("marB", emu(m[3]));
            }
            a = a.o("vert", c.vertical.map(|v| v.xml())).o(
                "anchor",
                c.anchor.map(|a| match a {
                    deckcraft_model::text::Anchor::Justified | deckcraft_model::text::Anchor::Distributed => "t",
                    other => other.xml(),
                }),
            );
            w.open("a:tcPr", a);
            for (tag, l) in [
                ("a:lnL", &c.borders[0]),
                ("a:lnR", &c.borders[1]),
                ("a:lnT", &c.borders[2]),
                ("a:lnB", &c.borders[3]),
                ("a:lnTlToBr", &c.diag_down),
                ("a:lnBlToTr", &c.diag_up),
            ] {
                if let Some(l) = l {
                    dml::line(w, x, o, tag, l);
                }
            }
            if let Some(f) = &c.fill {
                match f {
                    Fill::Background => {}
                    f => dml::fill(w, x, o, f),
                }
            }
            w.close("a:tcPr");
            w.close("a:tc");
        }
        w.close("a:tr");
    }
    w.close("a:tbl");
}

fn media(w: &mut W, x: &mut Exp, o: &mut Out, id: u32, s: &Shape, m: &deckcraft_model::MediaClip) {
    let kind = if m.video { "video" } else { "audio" };
    let link = x.media_rel(o, m.media, kind);
    let embed = x.media_rel_new(o, m.media, crate::opc::RT_MEDIA);
    w.open0("p:pic");
    w.open0("p:nvPicPr");
    let mut s2 = s.clone();
    if s2.click.is_none() {
        s2.click = Some(deckcraft_model::text::Hyperlink {
            action: deckcraft_model::text::Action::PlayMedia,
            tooltip: String::new(),
            highlight_click: false,
            sound: None,
            stop_sound: false,
            zoom_return: None,
        });
    }
    cnvpr(w, x, o, id, &s2);
    w.open0("p:cNvPicPr");
    w.empty("a:picLocks", A::new().a("noChangeAspect", "1"));
    w.close("p:cNvPicPr");
    w.open0("p:nvPr");
    if let Some(ph) = &s.ph {
        let _ = ph;
    }
    let tag = if m.video { "a:videoFile" } else { "a:audioFile" };
    if let Some(l) = &link {
        w.empty(tag, A::new().a("r:link", l));
    }
    if let Some(e) = &embed {
        w.open0("p:extLst");
        w.open("p:ext", A::new().a("uri", "{DAA4B4D4-6D71-4841-9C94-3DE7FCFB9230}"));
        w.open("p14:media", A::new().a("xmlns:p14", crate::opc::NS_P14).a("r:embed", e));
        if m.trim_start_ms > 0 || m.trim_end_ms > 0 {
            w.empty(
                "p14:trim",
                A::new().o("st", (m.trim_start_ms > 0).then_some(m.trim_start_ms)).o("end", (m.trim_end_ms > 0).then_some(m.trim_end_ms)),
            );
        }
        if m.fade_in_ms > 0 || m.fade_out_ms > 0 {
            w.empty("p14:fade", A::new().o("in", (m.fade_in_ms > 0).then_some(m.fade_in_ms)).o("out", (m.fade_out_ms > 0).then_some(m.fade_out_ms)));
        }
        if !m.bookmarks.is_empty() {
            w.open0("p14:bmkLst");
            for (n, t) in m.bookmarks.iter().take(1000) {
                w.empty("p14:bmk", A::new().a("name", n).a("time", t));
            }
            w.close("p14:bmkLst");
        }
        w.close("p14:media");
        w.close("p:ext");
        w.close("p:extLst");
    }
    w.close("p:nvPr");
    w.close("p:nvPicPr");
    let poster = deckcraft_model::style::PictureFill { media: m.poster.unwrap_or_default(), crop: m.crop, ..Default::default() };
    dml::blip_fill(w, x, o, "p:blipFill", &poster);
    let mut s3 = s.clone();
    s3.fill = None;
    sp_pr(w, x, o, &s3, true);
    w.close("p:pic");
}

#[allow(clippy::too_many_arguments)]
fn opaque(
    w: &mut W,
    x: &mut Exp,
    o: &mut Out,
    cx: &mut Ctx,
    s: &Shape,
    xml: &str,
    preview: Option<deckcraft_model::MediaId>,
    label: &str,
    parts: &[(String, String, deckcraft_model::MediaId)],
) {
    // Kept XML is written back as it was when every part it references was kept too, with those
    // parts re-added under this slide's relationship ids.
    if let Ok(doc) = crate::xml::parse(xml.as_bytes())
        && matches!(doc.root.local(), "graphicFrame" | "sp" | "grpSp" | "cxnSp")
    {
        let mut part_map: std::collections::HashMap<&str, (&str, deckcraft_model::MediaId)> = std::collections::HashMap::new();
        for (id, kind, m) in parts {
            part_map.entry(id.as_str()).or_insert((kind.as_str(), *m));
        }
        if let Some(ids) = crate::read::rel_refs(&doc.root)
            .iter()
            .map(|r| {
                let (kind, m) = part_map.get(r.as_str())?;
                Some((r.clone(), x.media_rel(o, *m, kind)?))
            })
            .collect::<Option<std::collections::HashMap<_, _>>>()
        {
            let mut root = doc.root;
            set_rel_ids(&mut root, &ids, 0);
            ole_fallback(&mut root, x, o, preview);
            let id = cx.id_for(s);
            set_cnvpr_id(&mut root, id);
            w.raw(&root.to_xml());
            return;
        }
    }
    log::warn!("pptx: object '{label}' written as its preview picture");
    let id = cx.id_for(s);
    let mut p = s.clone();
    if p.name.is_empty() {
        p.name = label.to_string();
    }
    match preview {
        Some(m) if x.p.media(m).is_some() => {
            p.kind = ShapeKind::Picture { fill: deckcraft_model::style::PictureFill { media: m, ..Default::default() } };
            p.ph = None;
            shape_with_id(w, x, o, cx, &p, id);
        }
        _ => {
            p.kind = ShapeKind::Shape;
            p.ph = None;
            p.geom = Geom::preset("rect");
            p.fill = Some(Fill::None);
            p.text = None;
            sp(w, x, o, id, &p, false);
        }
    }
}

fn shape_with_id(w: &mut W, x: &mut Exp, o: &mut Out, cx: &mut Ctx, s: &Shape, id: u32) {
    // Pictures only: reuse the given id.
    if let ShapeKind::Picture { fill } = &s.kind {
        w.open0("p:pic");
        w.open0("p:nvPicPr");
        cnvpr(w, x, o, id, s);
        w.open0("p:cNvPicPr");
        w.empty("a:picLocks", A::new().a("noChangeAspect", "1"));
        w.close("p:cNvPicPr");
        w.empty0("p:nvPr");
        w.close("p:nvPicPr");
        dml::blip_fill(w, x, o, "p:blipFill", fill);
        // As for any picture, a colour or pattern fill shows behind the image (an object's preview
        // keeps its fallback picture's fill).
        let mut s2 = s.clone();
        if matches!(s2.fill, Some(Fill::Picture(_) | Fill::Background | Fill::Group)) {
            s2.fill = None;
        }
        sp_pr(w, x, o, &s2, true);
        w.close("p:pic");
    } else {
        shape(w, x, o, cx, s, 0);
    }
}

/// A legacy OLE object finds its preview in the slide's VML drawing through `spid`, and PowerPoint
/// won't open the slide without it. We don't write that drawing, so write PowerPoint's own
/// fallback form instead: no `spid`, and the preview as a `p:pic`.
fn ole_fallback(root: &mut crate::xml::El, x: &mut Exp, o: &mut Out, preview: Option<deckcraft_model::MediaId>) {
    let xfrm = root.child("xfrm").map(|t| {
        let at = |e: &str, a: &str| t.child(e).and_then(|c| c.attr(a)).unwrap_or("0").to_string();
        (at("off", "x"), at("off", "y"), at("ext", "cx"), at("ext", "cy"))
    });
    let Some(ole) = root
        .elements_mut()
        .find(|e| e.is("graphic"))
        .and_then(|g| g.elements_mut().find(|e| e.is("graphicData")))
        .and_then(|d| d.elements_mut().find(|e| e.is("oleObj")))
    else {
        return;
    };
    if ole.child("pic").is_some() || ole.attr("spid").is_none() {
        return;
    }
    ole.attrs.retain(|(k, _)| k != "spid");
    let rel = preview.and_then(|m| x.media_rel(o, m, "image")).unwrap_or_else(|| x.placeholder_rel(o));
    let (ox, oy, cx, cy) = xfrm.unwrap_or_default();
    let pic = format!(
        r#"<p:pic><p:nvPicPr><p:cNvPr id="0" name=""/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="{rel}"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr><a:xfrm><a:off x="{ox}" y="{oy}"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic>"#
    );
    if let Ok(d) = crate::xml::parse(pic.as_bytes()) {
        ole.children.push(crate::xml::Node::El(d.root));
    }
}

fn set_rel_ids(e: &mut crate::xml::El, ids: &std::collections::HashMap<String, String>, depth: usize) {
    if depth > crate::xml::MAX_DEPTH {
        return;
    }
    for (k, v) in &mut e.attrs {
        if crate::read::is_rel_attr(k, v)
            && let Some(new) = ids.get(v.as_str())
        {
            v.clone_from(new);
        }
    }
    for c in e.elements_mut() {
        set_rel_ids(c, ids, depth + 1);
    }
}

fn set_cnvpr_id(el: &mut crate::xml::El, id: u32) {
    fn rec(e: &mut crate::xml::El, id: u32, depth: usize) -> bool {
        if depth > 8 {
            return false;
        }
        if e.is("cNvPr") {
            e.set_attr("id", &id.to_string());
            return true;
        }
        e.elements_mut().any(|c| rec(c, id, depth + 1))
    }
    rec(el, id, 0);
}

/// The fixed group header every shape tree starts with.
pub fn tree_header(w: &mut W) {
    w.open0("p:nvGrpSpPr");
    w.empty("p:cNvPr", A::new().a("id", 1).a("name", ""));
    w.empty0("p:cNvGrpSpPr");
    w.empty0("p:nvPr");
    w.close("p:nvGrpSpPr");
    w.open0("p:grpSpPr");
    w.open0("a:xfrm");
    w.empty("a:off", A::new().a("x", 0).a("y", 0));
    w.empty("a:ext", A::new().a("cx", 0).a("cy", 0));
    w.empty("a:chOff", A::new().a("x", 0).a("y", 0));
    w.empty("a:chExt", A::new().a("cx", 0).a("cy", 0));
    w.close("a:xfrm");
    w.close("p:grpSpPr");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A chart without text styling gets DeckCraft's drawn colour and frame-scaled sizes, so
    /// PowerPoint does not fall back to black 10 pt text; its own values are kept.
    #[test]
    fn chart_text_defaults_fill_only_the_gaps() {
        let mut c = deckcraft_model::Chart::sample(deckcraft_model::chart::ChartType::Column);
        assert!(!chart_text_defaults(&mut c, Xfrm { w: 440.0, h: 220.0, ..Default::default() }));
        assert!(c.text_color.is_some());
        assert_eq!((c.text_size, c.title_size), (Some(10.0), Some(14.0)));
        let mut c = deckcraft_model::Chart::sample(deckcraft_model::chart::ChartType::Column);
        c.text_size = Some(12.0);
        assert!(!chart_text_defaults(&mut c, Xfrm { w: 440.0, h: 220.0, ..Default::default() }));
        assert_eq!((c.text_size, c.title_size), (Some(12.0), Some(12.0 * 1.4)));
    }

    #[test]
    fn chart_shape_defaults_keep_pie_labels_white() {
        let mut c = deckcraft_model::Chart::sample(deckcraft_model::chart::ChartType::Pie);
        c.data_labels = true;
        let text_color_explicit = chart_text_defaults(&mut c, Xfrm { w: 440.0, h: 220.0, ..Default::default() });
        let xml = String::from_utf8(super::super::chart::chart_xml_with_text_color_explicit(&c, text_color_explicit).0).unwrap();
        let labels = xml.split("<c:dLbls>").nth(1).unwrap_or_default().split("</c:dLbls>").next().unwrap_or_default();
        assert!(labels.contains("<a:srgbClr val=\"FFFFFF\""), "{labels}");
        let chart_text = xml.rsplit("<c:txPr>").next().unwrap_or_default();
        assert!(chart_text.contains("<a:schemeClr val=\"tx1\"><a:lumMod val=\"65000\""), "{chart_text}");
    }

    /// PowerPoint asks to repair a `star5` whose guide list lacks `hf`/`vf` (found black-box).
    #[test]
    fn preset_guides_are_complete() {
        let mut w = W::frag();
        geometry(&mut w, &Geom::Preset { name: "star5".into(), adj: vec![20000.0] });
        assert_eq!(
            w.s,
            r#"<a:prstGeom prst="star5"><a:avLst><a:gd name="adj" fmla="val 20000"/><a:gd name="hf" fmla="val 105146"/><a:gd name="vf" fmla="val 110557"/></a:avLst></a:prstGeom>"#
        );
        let mut w = W::frag();
        geometry(&mut w, &Geom::Preset { name: "rightArrow".into(), adj: vec![10000.0] });
        assert!(w.s.contains(r#"name="adj1" fmla="val 10000""#) && w.s.contains(r#"name="adj2" fmla="val 50000""#), "{}", w.s);
        let mut w = W::frag();
        geometry(&mut w, &Geom::Preset { name: "textBox".into(), adj: vec![] });
        assert_eq!(w.s, r#"<a:prstGeom prst="rect"><a:avLst/></a:prstGeom>"#);
    }
}
