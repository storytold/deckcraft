//! Shape trees: `p:sp`, `p:pic`, `p:grpSp`, `p:cxnSp`, `p:graphicFrame` (tables, charts, SmartArt,
//! OLE and other objects).

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
            "contentPart" => {
                log::warn!("pptx: ink content part in {} not imported", part.name);
                None
            }
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
                    s.locked = e.child("spLocks").is_some_and(|l| l.bool("noMove") == Some(true) && l.bool("noResize") == Some(true));
                }
                "cNvPicPr" | "cNvGrpSpPr" | "cNvCxnSpPr" | "cNvGraphicFramePr" => {
                    s.locked = e.elements().next().is_some_and(|l| l.bool("noMove") == Some(true) && l.bool("noResize") == Some(true));
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
            let mut clip =
                MediaClip { media, video: vf.is_some() || af.is_none(), poster: pf.as_ref().map(|p| p.media), volume: 1.0, ..Default::default() };
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
            if let Some(o) = nvpr.and_then(|n| n.child("extLst")).and_then(|x| x.find("mediaOptions")) {
                clip.volume = o.f64("volume").map_or(1.0, |v| v.clamp(0.0, 1.0));
                clip.autoplay = o.bool("autoplay").unwrap_or(false);
                clip.loop_play = o.bool("loop").unwrap_or(false);
                clip.rewind = o.bool("rewind").unwrap_or(false);
                clip.play_across_slides = o.bool("acrossSlides").unwrap_or(false);
                clip.hide_while_not_playing = o.bool("hideWhileNotPlaying").unwrap_or(false);
                clip.full_screen = o.bool("fullScreen").unwrap_or(false);
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
            && let Some(bytes) = imp.pkg.get(&rel.target)
            && let Some(chart) = super::read_chart_xml(bytes)
        {
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
    };
    let label = if let Some(o) = gd.and_then(|g| g.find("oleObj")) {
        o.attr("progId").or(o.attr("name")).unwrap_or("Object").to_string()
    } else if uri.ends_with("/diagram") {
        "SmartArt".to_string()
    } else if uri.contains("chart") {
        "Chart".to_string()
    } else {
        uri.rsplit('/').next().unwrap_or("Object").to_string()
    };
    s.kind = ShapeKind::Opaque { xml: part.keep(e), preview, label };
    s
}

/// SmartArt: read the pre-drawn shapes of its drawing part as a group.
fn smartart(imp: &mut Imp, part: &Part, gd: &El, frame: &Shape, depth: usize) -> Option<Shape> {
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
    let mut g = frame.clone();
    let x = frame.xfrm.unwrap_or_default();
    g.kind = ShapeKind::Group { children, child: Xfrm::new(0.0, 0.0, x.w, x.h) };
    Some(g)
}

fn table(imp: &mut Imp, part: &Part, tbl: &El) -> Table {
    let mut t = Table { style: "none".into(), first_row: false, band_row: false, ..Default::default() };
    if let Some(pr) = tbl.child("tblPr") {
        t.first_row = pr.bool("firstRow").unwrap_or(false);
        t.first_col = pr.bool("firstCol").unwrap_or(false);
        t.last_row = pr.bool("lastRow").unwrap_or(false);
        t.last_col = pr.bool("lastCol").unwrap_or(false);
        t.band_row = pr.bool("bandRow").unwrap_or(false);
        t.band_col = pr.bool("bandCol").unwrap_or(false);
        if let Some(id) = pr.child("tableStyleId").map(|e| e.text()) {
            t.style = crate::tables::style_from_guid(id.trim());
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
