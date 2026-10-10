//! Shape trees: autoshapes, pictures, groups, connectors, tables, charts, media, ink and kept objects.

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
                let single = if defaults.is_empty() { vals.len() == 1 } else { defaults.len() == 1 };
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

fn nv_pr(w: &mut W, s: &Shape) {
    match &s.ph {
        Some(ph) => {
            w.open0("p:nvPr");
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
            w.close("p:nvPr");
        }
        None => w.empty0("p:nvPr"),
    }
}

fn locks(w: &mut W, tag: &str, s: &Shape, extra: A) {
    let a = extra.t("noGrp", s.ph.is_some()).t("noMove", s.locked).t("noResize", s.locked);
    if a.v.is_empty() {
        return;
    }
    w.empty(tag, a);
}

fn style(w: &mut W, st: &ShapeStyle) {
    w.open0("p:style");
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
    w.close("p:style");
}

/// `p:spPr` contents (also used for pictures and connectors).
fn sp_pr(w: &mut W, x: &mut Exp, o: &mut Out, s: &Shape, geom: bool) {
    w.open0("p:spPr");
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
        }
    }
    w.close("p:spPr");
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
            nv_pr(w, s);
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
        ShapeKind::Group { children, child } => {
            let id = cx.id_for(s);
            w.open0("p:grpSp");
            w.open0("p:nvGrpSpPr");
            cnvpr(w, x, o, id, s);
            w.open0("p:cNvGrpSpPr");
            locks(w, "a:grpSpLocks", s, A::new());
            w.close("p:cNvGrpSpPr");
            nv_pr(w, s);
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
            nv_pr(w, s);
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
            let rid = x.chart_rel(o, c);
            frame_open(w, x, o, id, s);
            w.open("a:graphicData", A::new().a("uri", "http://schemas.openxmlformats.org/drawingml/2006/chart"));
            w.empty("c:chart", A::new().a("xmlns:c", crate::opc::NS_C).a("r:id", rid));
            w.close("a:graphicData");
            frame_close(w);
        }
        ShapeKind::Media(m) => {
            let id = cx.id_for(s);
            media(w, x, o, id, s, m);
        }
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
        ShapeKind::Opaque { xml, preview, label } => opaque(w, x, o, cx, s, xml, *preview, label),
    }
}

fn ink_shape(st: &deckcraft_model::InkStroke, i: usize) -> Option<Shape> {
    let pts: Vec<(f64, f64)> = st.points.iter().filter(|p| p.0.is_finite() && p.1.is_finite()).map(|p| (p.0, p.1)).collect();
    let (x0, y0) = pts.iter().fold((f64::MAX, f64::MAX), |a, p| (a.0.min(p.0), a.1.min(p.1)));
    let (x1, y1) = pts.iter().fold((f64::MIN, f64::MIN), |a, p| (a.0.max(p.0), a.1.max(p.1)));
    if pts.len() < 2 {
        return None;
    }
    let (w, h) = ((x1 - x0).max(1.0), (y1 - y0).max(1.0));
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
            ..deckcraft_model::Line::solid(c, st.width.max(0.25))
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
    nv_pr(w, s);
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
        dml::text_body(w, x, o, "p:txBody", tb, math);
    }
    w.close("p:sp");
}

fn frame_open(w: &mut W, x: &mut Exp, o: &mut Out, id: u32, s: &Shape) {
    w.open0("p:graphicFrame");
    w.open0("p:nvGraphicFramePr");
    cnvpr(w, x, o, id, s);
    w.open0("p:cNvGraphicFramePr");
    w.empty("a:graphicFrameLocks", A::new().a("noGrp", "1").t("noMove", s.locked).t("noResize", s.locked));
    w.close("p:cNvGraphicFramePr");
    nv_pr(w, s);
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
    if embed.is_some() || link.is_some() {
        w.open0("p:extLst");
        if let Some(e) = &embed {
            w.open("p:ext", A::new().a("uri", "{DAA4B4D4-6D71-4841-9C94-3DE7FCFB9230}"));
            w.open("p14:media", A::new().a("xmlns:p14", crate::opc::NS_P14).a("r:embed", e));
            if m.trim_start_ms > 0 || m.trim_end_ms > 0 {
                w.empty(
                    "p14:trim",
                    A::new().o("st", (m.trim_start_ms > 0).then_some(m.trim_start_ms)).o("end", (m.trim_end_ms > 0).then_some(m.trim_end_ms)),
                );
            }
            if m.fade_in_ms > 0 || m.fade_out_ms > 0 {
                w.empty(
                    "p14:fade",
                    A::new().o("in", (m.fade_in_ms > 0).then_some(m.fade_in_ms)).o("out", (m.fade_out_ms > 0).then_some(m.fade_out_ms)),
                );
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
        }
        // PowerPoint keeps playback options in the timing tree; they are kept here so a round trip does not reset them.
        w.open("p:ext", A::new().a("uri", "{6F1D3A52-5B0E-4C1B-9D6A-DECC4A17E001}"));
        w.empty(
            "dk:mediaOptions",
            A::new()
                .a("xmlns:dk", "urn:deckcraft:media")
                .a("volume", m.volume)
                .t("autoplay", m.autoplay)
                .t("loop", m.loop_play)
                .t("rewind", m.rewind)
                .t("acrossSlides", m.play_across_slides)
                .t("hideWhileNotPlaying", m.hide_while_not_playing)
                .t("fullScreen", m.full_screen),
        );
        w.close("p:ext");
        w.close("p:extLst");
    }
    w.close("p:nvPr");
    w.close("p:nvPicPr");
    let poster = deckcraft_model::style::PictureFill { media: m.poster.unwrap_or_default(), ..Default::default() };
    dml::blip_fill(w, x, o, "p:blipFill", &poster);
    let mut s3 = s.clone();
    s3.fill = None;
    sp_pr(w, x, o, &s3, true);
    w.close("p:pic");
}

#[allow(clippy::too_many_arguments)]
fn opaque(w: &mut W, x: &mut Exp, o: &mut Out, cx: &mut Ctx, s: &Shape, xml: &str, preview: Option<deckcraft_model::MediaId>, label: &str) {
    // Kept XML that references no relationships can be written back as it was.
    if let Ok(doc) = crate::xml::parse(xml.as_bytes())
        && !crate::read::has_rel_refs(&doc.root)
        && matches!(doc.root.local(), "graphicFrame" | "sp" | "grpSp" | "cxnSp")
    {
        let mut root = doc.root;
        let id = cx.id_for(s);
        set_cnvpr_id(&mut root, id);
        w.raw(&root.to_xml());
        return;
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
        let mut s2 = s.clone();
        s2.fill = None;
        sp_pr(w, x, o, &s2, true);
        w.close("p:pic");
    } else {
        shape(w, x, o, cx, s, 0);
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
