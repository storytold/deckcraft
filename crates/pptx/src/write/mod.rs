//! PPTX export: [`Presentation`] → package.

mod chart;
pub(crate) mod dml;
mod shapes;
mod theme;
mod timing;

use std::collections::HashMap;
use std::sync::Arc;

use deckcraft_geom::Xfrm;
use deckcraft_model::text::{Paragraph, TextBody};
use deckcraft_model::{
    Background, Comment, Layout, LayoutId, Master, MediaId, PhType, Placeholder, Presentation, Shape, ShapeId, Slide, SlideId, defaults,
};

use crate::PptxError;
use crate::opc::{self, NS_A, NS_P, NS_P14, NS_R, PackageOut, RelsOut};
use crate::xml::{A, W};

const CT_PRES: &str = "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml";
const CT_SLIDE: &str = "application/vnd.openxmlformats-officedocument.presentationml.slide+xml";
const CT_LAYOUT: &str = "application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml";
const CT_MASTER: &str = "application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml";
const CT_NOTES_MASTER: &str = "application/vnd.openxmlformats-officedocument.presentationml.notesMaster+xml";
const CT_NOTES: &str = "application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml";
const CT_THEME: &str = "application/vnd.openxmlformats-officedocument.theme+xml";
const CT_PRES_PROPS: &str = "application/vnd.openxmlformats-officedocument.presentationml.presProps+xml";
const CT_VIEW_PROPS: &str = "application/vnd.openxmlformats-officedocument.presentationml.viewProps+xml";
const CT_TABLE_STYLES: &str = "application/vnd.openxmlformats-officedocument.presentationml.tableStyles+xml";
const CT_CHART: &str = "application/vnd.openxmlformats-officedocument.drawingml.chart+xml";
const CT_COMMENTS: &str = "application/vnd.openxmlformats-officedocument.presentationml.comments+xml";
const CT_AUTHORS: &str = "application/vnd.openxmlformats-officedocument.presentationml.commentAuthors+xml";
const CT_CORE: &str = "application/vnd.openxmlformats-package.core-properties+xml";
const CT_APP: &str = "application/vnd.openxmlformats-officedocument.extended-properties+xml";

/// Export state.
pub struct Exp<'a> {
    pub p: &'a Presentation,
    pub pkg: PackageOut,
    media_parts: HashMap<MediaId, String>,
    n_image: u32,
    n_media: u32,
    n_chart: u32,
    pub slide_parts: HashMap<SlideId, String>,
    pub table_styles: Vec<String>,
    placeholder: Option<String>,
}

/// A part being written: its name and relationships.
pub struct Out {
    pub name: String,
    pub rels: RelsOut,
}

impl Out {
    fn new(name: &str) -> Self {
        Out { name: name.to_string(), rels: RelsOut::default() }
    }
}

fn ext_for(content_type: &str, name: &str, data: &[u8]) -> String {
    let by_ct = match content_type {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpeg",
        "image/gif" => "gif",
        "image/bmp" => "bmp",
        "image/tiff" => "tiff",
        "image/svg+xml" => "svg",
        "image/x-emf" | "image/emf" => "emf",
        "image/x-wmf" | "image/wmf" => "wmf",
        "image/webp" => "webp",
        "video/mp4" => "mp4",
        "video/quicktime" => "mov",
        "video/x-ms-wmv" => "wmv",
        "video/x-msvideo" => "avi",
        "video/webm" => "webm",
        "audio/mpeg" => "mp3",
        "audio/wav" | "audio/x-wav" | "audio/wave" => "wav",
        "audio/mp4" | "audio/x-m4a" => "m4a",
        "audio/x-ms-wma" => "wma",
        "audio/ogg" => "ogg",
        "audio/flac" => "flac",
        _ => "",
    };
    if !by_ct.is_empty() {
        return by_ct.to_string();
    }
    if data.starts_with(b"\x89PNG") {
        return "png".into();
    }
    if data.starts_with(&[0xFF, 0xD8]) {
        return "jpeg".into();
    }
    if data.starts_with(b"GIF8") {
        return "gif".into();
    }
    let e = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    if !e.is_empty() && e.len() <= 5 && e.chars().all(|c| c.is_ascii_alphanumeric()) { e } else { "bin".into() }
}

impl Exp<'_> {
    fn media_part(&mut self, id: MediaId) -> Option<String> {
        if let Some(p) = self.media_parts.get(&id) {
            return Some(p.clone());
        }
        let m = self.p.media(id)?;
        if m.data.is_empty() {
            return None;
        }
        let ext = ext_for(&m.content_type, &m.name, &m.data);
        let ct = if m.content_type.is_empty() || !m.content_type.contains('/') {
            crate::mime_for(&format!("x.{ext}")).to_string()
        } else {
            m.content_type.clone()
        };
        let name = if ct.starts_with("image/") {
            self.n_image += 1;
            format!("ppt/media/image{}.{ext}", self.n_image)
        } else {
            self.n_media += 1;
            format!("ppt/media/media{}.{ext}", self.n_media)
        };
        self.pkg.binary(&name, &ct, m.data.as_ref().clone());
        self.media_parts.insert(id, name.clone());
        Some(name)
    }
    /// A relationship from `o` to a media item (embedded part, or external link).
    pub fn media_rel(&mut self, o: &mut Out, id: MediaId, kind: &str) -> Option<String> {
        let m = self.p.media(id)?;
        if m.data.is_empty() {
            let link = m.link.clone()?;
            return Some(o.rels.add(kind, &link, true));
        }
        let part = self.media_part(id)?;
        Some(o.rels.add(kind, &opc::relative(&o.name, &part), false))
    }
    pub fn media_rel_new(&mut self, o: &mut Out, id: MediaId, kind: &str) -> Option<String> {
        let m = self.p.media(id)?;
        if m.data.is_empty() {
            return None;
        }
        let part = self.media_part(id)?;
        Some(o.rels.add(kind, &opc::relative(&o.name, &part), false))
    }
    /// A tiny transparent PNG for pictures whose image is missing.
    pub fn placeholder_rel(&mut self, o: &mut Out) -> String {
        let part = match &self.placeholder {
            Some(p) => p.clone(),
            None => {
                self.n_image += 1;
                let name = format!("ppt/media/image{}.png", self.n_image);
                self.pkg.binary(&name, "image/png", crate::png::transparent_1x1());
                self.placeholder = Some(name.clone());
                name
            }
        };
        o.rels.add("image", &opc::relative(&o.name, &part), false)
    }
    pub fn chart_rel(&mut self, o: &mut Out, c: &deckcraft_model::Chart) -> String {
        self.n_chart += 1;
        let name = format!("ppt/charts/chart{}.xml", self.n_chart);
        self.pkg.xml_part(&name, CT_CHART, chart::chart_xml(c));
        o.rels.add_new("chart", &opc::relative(&o.name, &name), false)
    }
}

fn root_open(w: &mut W, tag: &str, extra: A) {
    let mut a = A::new().a("xmlns:a", NS_A).a("xmlns:r", NS_R).a("xmlns:p", NS_P);
    a.v.extend(extra.v);
    w.open(tag, a);
}

fn background(w: &mut W, x: &mut Exp, o: &mut Out, bg: &Background) {
    w.open0("p:bg");
    match bg {
        Background::Fill { fill } => {
            w.open0("p:bgPr");
            match fill {
                deckcraft_model::Fill::Background | deckcraft_model::Fill::Group => w.empty0("a:noFill"),
                f => dml::fill(w, x, o, f),
            }
            w.empty0("a:effectLst");
            w.close("p:bgPr");
        }
        Background::Ref { idx, color } => {
            let idx = if (1..=3).contains(idx) || (1001..=1003).contains(idx) { *idx } else { 1001 };
            w.open("p:bgRef", A::new().a("idx", idx));
            dml::color(w, color);
            w.close("p:bgRef");
        }
    }
    w.close("p:bg");
}

fn csld(w: &mut W, x: &mut Exp, o: &mut Out, name: &str, bg: Option<&Background>, list: &[Shape]) -> shapes::IdMap {
    w.open("p:cSld", A::new().o("name", (!name.is_empty()).then_some(name)));
    if let Some(bg) = bg {
        background(w, x, o, bg);
    }
    w.open0("p:spTree");
    shapes::tree_header(w);
    let mut ids = shapes::IdMap::build(list);
    {
        let mut cx = shapes::Ctx { ids: &mut ids, written: Default::default() };
        shapes::shapes(w, x, o, &mut cx, list, 0);
    }
    w.close("p:spTree");
    w.close("p:cSld");
    ids
}

const CLR_KEYS: [(&str, &str); 12] = [
    ("bg1", "lt1"),
    ("tx1", "dk1"),
    ("bg2", "lt2"),
    ("tx2", "dk2"),
    ("accent1", "accent1"),
    ("accent2", "accent2"),
    ("accent3", "accent3"),
    ("accent4", "accent4"),
    ("accent5", "accent5"),
    ("accent6", "accent6"),
    ("hlink", "hlink"),
    ("folHlink", "folHlink"),
];

fn clr_map(w: &mut W, map: &[(String, String)]) {
    let mut a = A::new();
    for (k, d) in CLR_KEYS {
        let v = map
            .iter()
            .find(|(mk, _)| mk == k)
            .map(|(_, v)| v.as_str())
            .filter(|v| deckcraft_color::SchemeSlot::from_xml(v).is_some_and(|s| deckcraft_color::SchemeSlot::THEME.contains(&s)))
            .unwrap_or(d);
        a = a.a(k, v);
    }
    w.empty("p:clrMap", a);
}

fn ext_list(w: &mut W, raw: Option<&str>) {
    let Some(raw) = raw.filter(|r| !r.trim().is_empty()) else { return };
    // Only keep well-formed extension elements.
    let Ok(d) = crate::xml::parse(format!("<x>{raw}</x>").as_bytes()) else { return };
    let exts: Vec<String> = d.root.elements().filter(|e| e.is("ext") && e.has_attr("uri")).map(|e| e.to_xml()).collect();
    if exts.is_empty() {
        return;
    }
    w.open0("p:extLst");
    for e in exts {
        w.raw(&e);
    }
    w.close("p:extLst");
}

/// Strip an `<p:extLst>` wrapper (masters/layouts keep the whole element).
fn ext_children(raw: Option<&str>) -> Option<String> {
    let raw = raw?;
    let d = crate::xml::parse(raw.as_bytes()).ok()?;
    if d.root.is("extLst") {
        let ns = d.ns_decls();
        return Some(d.root.elements().map(|e| crate::read::keep_xml(e, &ns)).collect());
    }
    Some(raw.to_string())
}

fn notes_master_shapes(size: deckcraft_geom::Size) -> Vec<Shape> {
    let (w, h) = (size.width, size.height);
    let mk = |id: u32, name: &str, kind: PhType, idx: u32, x: f64, y: f64, ww: f64, hh: f64| Shape {
        id: ShapeId(id),
        name: name.into(),
        xfrm: Some(Xfrm::new(x, y, ww, hh)),
        ph: Some(Placeholder { kind, idx, size: (kind != PhType::SlideImage).then(|| "quarter".to_string()), ..Default::default() }),
        text: (kind != PhType::SlideImage).then(TextBody::default),
        ..Default::default()
    };
    let mut img = mk(4, "Slide Image Placeholder 3", PhType::SlideImage, 2, w * 0.125, h * 0.125, w * 0.75, h * 0.375);
    img.fill = Some(deckcraft_model::Fill::None);
    img.line = Some(deckcraft_model::Line::solid(deckcraft_model::ColorRef::rgb(deckcraft_color::Rgba::BLACK), 1.0));
    let mut body = mk(5, "Notes Placeholder 4", PhType::Body, 3, w * 0.1, h * 0.53, w * 0.8, h * 0.36);
    if let Some(t) = body.text.as_mut() {
        t.paragraphs = vec![Paragraph::new("Click to edit Master text styles")];
    }
    vec![
        mk(2, "Header Placeholder 1", PhType::Header, 0, 0.0, 0.0, w * 0.43, h * 0.05),
        mk(3, "Date Placeholder 2", PhType::Date, 1, w * 0.57, 0.0, w * 0.43, h * 0.05),
        img,
        body,
        mk(6, "Footer Placeholder 5", PhType::Footer, 4, 0.0, h * 0.95, w * 0.43, h * 0.05),
        mk(7, "Slide Number Placeholder 6", PhType::SlideNum, 5, w * 0.57, h * 0.95, w * 0.43, h * 0.05),
    ]
}

fn notes_style() -> deckcraft_model::text::ListStyle {
    let mut ls = deckcraft_model::text::ListStyle::default();
    for i in 0..9u8 {
        ls.set(
            i,
            deckcraft_model::text::LevelStyle {
                para: deckcraft_model::text::ParaProps {
                    margin_left: Some(36.0 * i as f64),
                    align: Some(deckcraft_model::text::Align::Left),
                    ..Default::default()
                },
                run: deckcraft_model::text::RunProps {
                    size: Some(12.0),
                    font: Some("+mn-lt".into()),
                    fill: Some(deckcraft_model::Fill::solid(deckcraft_model::ColorRef::scheme(deckcraft_color::SchemeSlot::Tx1))),
                    ..Default::default()
                },
            },
        );
    }
    ls
}

fn valid_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 10
        && b.iter().take(4).all(|c| c.is_ascii_digit())
        && b.get(4) == Some(&b'-')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || "-:.+TZ".contains(c))
}

/// A copy of the presentation with every slide pointing at a layout that exists.
fn sanitized(p: &Presentation) -> std::borrow::Cow<'_, Presentation> {
    let mut need = p.masters.is_empty() || p.masters.iter().any(|m| m.layouts.is_empty());
    need |= p.slides.iter().any(|s| p.layout(s.layout).is_none());
    if !need {
        return std::borrow::Cow::Borrowed(p);
    }
    let mut q = p.clone();
    if q.masters.is_empty() {
        let (m, next) = defaults::build_master(q.slide_size, Default::default(), q.next_id.max(1));
        q.next_id = next;
        q.masters.push(Arc::new(m));
    }
    for m in q.masters.iter_mut() {
        if m.layouts.is_empty() {
            let id = LayoutId(q.next_id.saturating_add(1));
            q.next_id = id.0;
            Arc::make_mut(m).layouts.push(Layout {
                id,
                name: "Blank".into(),
                kind: deckcraft_model::LayoutType::Blank,
                show_master_shapes: true,
                ..Default::default()
            });
        }
    }
    let first = q.masters.first().and_then(|m| m.layouts.first()).map(|l| l.id).unwrap_or_default();
    let ok: Vec<LayoutId> = q.masters.iter().flat_map(|m| m.layouts.iter().map(|l| l.id)).collect();
    for s in q.slides.iter_mut() {
        if !ok.contains(&s.layout) {
            Arc::make_mut(s).layout = first;
        }
    }
    std::borrow::Cow::Owned(q)
}

pub fn export(p: &Presentation) -> Result<Vec<u8>, PptxError> {
    let p = sanitized(p);
    let p: &Presentation = &p;
    let mut x = Exp {
        p,
        pkg: PackageOut::default(),
        media_parts: HashMap::new(),
        n_image: 0,
        n_media: 0,
        n_chart: 0,
        slide_parts: HashMap::new(),
        table_styles: vec![],
        placeholder: None,
    };
    for (i, s) in p.slides.iter().enumerate() {
        x.slide_parts.insert(s.id, format!("ppt/slides/slide{}.xml", i + 1));
    }
    let mut pres = Out::new("ppt/presentation.xml");

    // Masters, themes and layouts.
    let mut master_rids = vec![];
    let mut layout_parts: HashMap<LayoutId, String> = HashMap::new();
    let mut n_layout = 0u32;
    let mut big_id: u64 = 2_147_483_648;
    for (mi, m) in p.masters.iter().enumerate() {
        let mname = format!("ppt/slideMasters/slideMaster{}.xml", mi + 1);
        let tname = format!("ppt/theme/theme{}.xml", mi + 1);
        let mut mo = Out::new(&mname);
        let mut lrids = vec![];
        for l in &m.layouts {
            n_layout += 1;
            let lname = format!("ppt/slideLayouts/slideLayout{n_layout}.xml");
            layout_parts.insert(l.id, lname.clone());
            let rid = mo.rels.add("slideLayout", &opc::relative(&mname, &lname), false);
            lrids.push(rid);
            let mut lo = Out::new(&lname);
            lo.rels.add("slideMaster", &opc::relative(&lname, &mname), false);
            let xml = layout_xml(&mut x, &mut lo, l);
            x.pkg.xml_part(&lname, CT_LAYOUT, xml);
            x.pkg.rels(&lname, &lo.rels);
        }
        mo.rels.add("theme", &opc::relative(&mname, &tname), false);
        let mut to = Out::new(&tname);
        let txml = theme::theme_xml(&mut x, &mut to, &m.theme);
        x.pkg.xml_part(&tname, CT_THEME, txml);
        x.pkg.rels(&tname, &to.rels);
        let master_id = big_id;
        big_id += 1;
        let mut layout_ids = vec![];
        for rid in lrids {
            layout_ids.push((big_id, rid));
            big_id += 1;
        }
        let xml = master_xml(&mut x, &mut mo, m, &layout_ids);
        x.pkg.xml_part(&mname, CT_MASTER, xml);
        x.pkg.rels(&mname, &mo.rels);
        master_rids.push((master_id, pres.rels.add("slideMaster", &opc::relative("ppt/presentation.xml", &mname), false)));
    }

    // Notes master when any slide has notes.
    let has_notes = p.slides.iter().any(|s| !s.notes.is_empty());
    let notes_master = "ppt/notesMasters/notesMaster1.xml";
    let mut notes_master_rid = None;
    if has_notes {
        let tname = format!("ppt/theme/theme{}.xml", p.masters.len() + 1);
        let mut no = Out::new(notes_master);
        no.rels.add("theme", &opc::relative(notes_master, &tname), false);
        let theme = p.notes_master.as_ref().map(|m| m.theme.clone()).or_else(|| p.masters.first().map(|m| m.theme.clone())).unwrap_or_default();
        let mut to = Out::new(&tname);
        let txml = theme::theme_xml(&mut x, &mut to, &theme);
        x.pkg.xml_part(&tname, CT_THEME, txml);
        x.pkg.rels(&tname, &to.rels);
        let mut w = W::new();
        root_open(&mut w, "p:notesMaster", A::new());
        let bg = Background::Ref { idx: 1001, color: deckcraft_model::ColorRef::scheme(deckcraft_color::SchemeSlot::Bg1) };
        csld(&mut w, &mut x, &mut no, "", Some(&bg), &notes_master_shapes(p.notes_size));
        clr_map(&mut w, &[]);
        dml::list_style(&mut w, &mut x, &mut no, "p:notesStyle", &notes_style());
        w.close("p:notesMaster");
        x.pkg.xml_part(notes_master, CT_NOTES_MASTER, w.finish());
        x.pkg.rels(notes_master, &no.rels);
        notes_master_rid = Some(pres.rels.add("notesMaster", &opc::relative("ppt/presentation.xml", notes_master), false));
    }

    // Comment authors.
    let mut authors: Vec<(String, String, u32)> = vec![];
    let mut n_comments = 0u32;

    // Slides.
    let mut slide_rids = vec![];
    for (i, s) in p.slides.iter().enumerate() {
        let sname = format!("ppt/slides/slide{}.xml", i + 1);
        let mut so = Out::new(&sname);
        if let Some(lp) = layout_parts.get(&s.layout) {
            so.rels.add("slideLayout", &opc::relative(&sname, lp), false);
        }
        if !s.notes.is_empty() {
            let nname = format!("ppt/notesSlides/notesSlide{}.xml", i + 1);
            so.rels.add("notesSlide", &opc::relative(&sname, &nname), false);
            let mut no = Out::new(&nname);
            no.rels.add("notesMaster", &opc::relative(&nname, notes_master), false);
            no.rels.add("slide", &opc::relative(&nname, &sname), false);
            let xml = notes_xml(&mut x, &mut no, &s.notes);
            x.pkg.xml_part(&nname, CT_NOTES, xml);
            x.pkg.rels(&nname, &no.rels);
        }
        if !s.comments.is_empty() {
            n_comments += 1;
            let cname = format!("ppt/comments/comment{n_comments}.xml");
            so.rels.add("comments", &opc::relative(&sname, &cname), false);
            x.pkg.xml_part(&cname, CT_COMMENTS, comments_xml(&s.comments, &mut authors));
        }
        let xml = slide_xml(&mut x, &mut so, s);
        x.pkg.xml_part(&sname, CT_SLIDE, xml);
        x.pkg.rels(&sname, &so.rels);
        slide_rids.push(pres.rels.add("slide", &opc::relative("ppt/presentation.xml", &sname), false));
    }
    if !authors.is_empty() {
        let name = "ppt/commentAuthors.xml";
        let mut w = W::new();
        root_open(&mut w, "p:cmAuthorLst", A::new());
        for (i, (n, ini, last)) in authors.iter().enumerate() {
            w.empty("p:cmAuthor", A::new().a("id", i).a("name", n).a("initials", ini).a("lastIdx", last).a("clrIdx", i % 8));
        }
        w.close("p:cmAuthorLst");
        x.pkg.xml_part(name, CT_AUTHORS, w.finish());
        pres.rels.add("commentAuthors", "commentAuthors.xml", false);
    }

    // Presentation-level parts.
    let theme1 = "theme/theme1.xml";
    pres.rels.add("theme", theme1, false);
    pres.rels.add("presProps", "presProps.xml", false);
    pres.rels.add("viewProps", "viewProps.xml", false);
    pres.rels.add("tableStyles", "tableStyles.xml", false);
    x.pkg.xml_part("ppt/presProps.xml", CT_PRES_PROPS, pres_props_xml(p));
    x.pkg.xml_part("ppt/viewProps.xml", CT_VIEW_PROPS, view_props_xml());
    let ts = crate::tables::table_styles_xml(&x.table_styles);
    x.pkg.xml_part("ppt/tableStyles.xml", CT_TABLE_STYLES, ts);
    let pxml = presentation_xml(&mut x, &mut pres, &master_rids, notes_master_rid.as_deref(), &slide_rids);
    x.pkg.xml_part("ppt/presentation.xml", CT_PRES, pxml);
    x.pkg.rels("ppt/presentation.xml", &pres.rels);

    // Document properties and package relationships.
    x.pkg.xml_part("docProps/core.xml", CT_CORE, core_xml(p));
    x.pkg.xml_part("docProps/app.xml", CT_APP, app_xml(p));
    let mut root = RelsOut::default();
    root.add("officeDocument", "ppt/presentation.xml", false);
    root.add(opc::RT_CORE, "docProps/core.xml", false);
    root.add("extended-properties", "docProps/app.xml", false);
    x.pkg.rels("", &root);
    x.pkg.finish()
}

fn layout_xml(x: &mut Exp, o: &mut Out, l: &Layout) -> Vec<u8> {
    let mut w = W::new();
    root_open(
        &mut w,
        "p:sldLayout",
        A::new().o("showMasterSp", (!l.show_master_shapes).then_some("0")).a("type", l.kind.xml()).t("preserve", l.preserve),
    );
    csld(&mut w, x, o, &l.name, l.background.as_ref(), &l.shapes);
    w.open0("p:clrMapOvr");
    w.empty0("a:masterClrMapping");
    w.close("p:clrMapOvr");
    ext_list(&mut w, ext_children(l.raw_ext.as_deref()).as_deref());
    w.close("p:sldLayout");
    w.finish()
}

fn master_xml(x: &mut Exp, o: &mut Out, m: &Master, layouts: &[(u64, String)]) -> Vec<u8> {
    let mut w = W::new();
    root_open(&mut w, "p:sldMaster", A::new().t("preserve", m.preserve));
    csld(&mut w, x, o, &m.name, m.background.as_ref(), &m.shapes);
    clr_map(&mut w, &m.color_map);
    w.open0("p:sldLayoutIdLst");
    for (id, rid) in layouts {
        w.empty("p:sldLayoutId", A::new().a("id", id).a("r:id", rid));
    }
    w.close("p:sldLayoutIdLst");
    w.open0("p:txStyles");
    let (dt, db, dother) = defaults::master_styles();
    let pick = |s: &deckcraft_model::text::ListStyle, d: deckcraft_model::text::ListStyle| if s.is_empty() { d } else { s.clone() };
    dml::list_style(&mut w, x, o, "p:titleStyle", &pick(&m.title_style, dt));
    dml::list_style(&mut w, x, o, "p:bodyStyle", &pick(&m.body_style, db));
    dml::list_style(&mut w, x, o, "p:otherStyle", &pick(&m.other_style, dother));
    w.close("p:txStyles");
    ext_list(&mut w, ext_children(m.raw_ext.as_deref()).as_deref());
    w.close("p:sldMaster");
    w.finish()
}

fn slide_xml(x: &mut Exp, o: &mut Out, s: &Slide) -> Vec<u8> {
    let mut w = W::new();
    root_open(&mut w, "p:sld", A::new().o("showMasterSp", (!s.show_master_shapes).then_some("0")).o("show", s.hidden.then_some("0")));
    let ids = csld(&mut w, x, o, &s.name, s.background.as_ref(), &s.shapes);
    w.open0("p:clrMapOvr");
    w.empty0("a:masterClrMapping");
    w.close("p:clrMapOvr");
    if let Some(t) = &s.transition {
        timing::transition(&mut w, x, o, t);
    }
    if !s.animations.is_empty() {
        timing::timing(&mut w, x, &s.animations, &s.shapes, &ids);
    }
    ext_list(&mut w, s.raw_ext.as_deref());
    w.close("p:sld");
    w.finish()
}

fn notes_xml(x: &mut Exp, o: &mut Out, notes: &TextBody) -> Vec<u8> {
    let mut w = W::new();
    root_open(&mut w, "p:notes", A::new());
    let img = Shape {
        id: ShapeId(2),
        name: "Slide Image Placeholder 1".into(),
        ph: Some(Placeholder { kind: PhType::SlideImage, ..Default::default() }),
        ..Default::default()
    };
    let mut tb = notes.clone();
    tb.body = Default::default();
    let body = Shape {
        id: ShapeId(3),
        name: "Notes Placeholder 2".into(),
        ph: Some(Placeholder { kind: PhType::Body, idx: 1, ..Default::default() }),
        text: Some(tb),
        ..Default::default()
    };
    csld(&mut w, x, o, "", None, &[img, body]);
    w.open0("p:clrMapOvr");
    w.empty0("a:masterClrMapping");
    w.close("p:clrMapOvr");
    w.close("p:notes");
    w.finish()
}

fn comments_xml(list: &[Comment], authors: &mut Vec<(String, String, u32)>) -> Vec<u8> {
    let mut w = W::new();
    root_open(&mut w, "p:cmLst", A::new());
    for c in list.iter().take(10_000) {
        comment_xml(&mut w, c, authors, "p:cm", 0);
    }
    w.close("p:cmLst");
    w.finish()
}

/// One comment; replies nest in a `p:replyLst` (the reader accepts the same depth) and a resolved thread gets `status="resolved"`.
fn comment_xml(w: &mut W, c: &Comment, authors: &mut Vec<(String, String, u32)>, tag: &str, depth: usize) {
    let author = if c.author.is_empty() { "Author".to_string() } else { c.author.clone() };
    let ai = match authors.iter().position(|(n, _, _)| *n == author) {
        Some(i) => i,
        None => {
            let ini = if c.initials.is_empty() { author.chars().filter(|c| c.is_alphabetic()).take(2).collect() } else { c.initials.clone() };
            authors.push((author.clone(), ini, 0));
            authors.len() - 1
        }
    };
    let idx = match authors.get_mut(ai) {
        Some(a) => {
            a.2 += 1;
            a.2
        }
        None => 1,
    };
    let dt = if valid_date(&c.date) { c.date.clone() } else { "2024-01-01T00:00:00.000".into() };
    w.open(tag, A::new().a("authorId", ai).a("dt", dt).a("idx", idx).o("status", c.resolved.then_some("resolved")));
    let pos = |v: f64| if v.is_finite() { (v * 8.0).round().clamp(0.0, 1.0e9) as i64 } else { 0 };
    w.empty("p:pos", A::new().a("x", pos(c.x)).a("y", pos(c.y)));
    w.elt("p:text", &c.text);
    if depth < 4 && !c.replies.is_empty() {
        w.open0("p:replyLst");
        for r in c.replies.iter().take(1000) {
            comment_xml(w, r, authors, "p:reply", depth + 1);
        }
        w.close("p:replyLst");
    }
    w.close(tag);
}

/// Sections with every slide in exactly one section, in slide order.
fn section_slides(p: &Presentation) -> Vec<(String, Vec<usize>)> {
    let mut out: Vec<(String, Vec<usize>)> = p.sections.iter().map(|s| (s.name.clone(), vec![])).collect();
    if out.is_empty() {
        return out;
    }
    let mut cur = 0usize;
    for (i, s) in p.slides.iter().enumerate() {
        if let Some(k) = p.sections.iter().position(|sec| sec.slides.contains(&s.id))
            && k >= cur
        {
            cur = k;
        }
        if let Some(sec) = out.get_mut(cur) {
            sec.1.push(i);
        }
    }
    out
}

fn presentation_xml(x: &mut Exp, o: &mut Out, masters: &[(u64, String)], notes: Option<&str>, slides: &[String]) -> Vec<u8> {
    let p = x.p;
    let mut w = W::new();
    let first = (p.first_slide_number != 1).then_some(p.first_slide_number.min(9999));
    root_open(&mut w, "p:presentation", A::new().a("saveSubsetFonts", 1).o("firstSlideNum", first));
    w.open0("p:sldMasterIdLst");
    for (id, rid) in masters {
        w.empty("p:sldMasterId", A::new().a("id", id).a("r:id", rid));
    }
    w.close("p:sldMasterIdLst");
    if let Some(n) = notes {
        w.open0("p:notesMasterIdLst");
        w.empty("p:notesMasterId", A::new().a("r:id", n));
        w.close("p:notesMasterIdLst");
    }
    if !slides.is_empty() {
        w.open0("p:sldIdLst");
        for (i, rid) in slides.iter().enumerate() {
            w.empty("p:sldId", A::new().a("id", 256 + i).a("r:id", rid));
        }
        w.close("p:sldIdLst");
    }
    let clamp = |v: f64| dml::emu(v).clamp(914_400, 51_206_400);
    w.empty("p:sldSz", A::new().a("cx", clamp(p.slide_size.width)).a("cy", clamp(p.slide_size.height)));
    w.empty("p:notesSz", A::new().a("cx", dml::emu_pos(p.notes_size.width).max(1)).a("cy", dml::emu_pos(p.notes_size.height).max(1)));
    if !p.custom_shows.is_empty() {
        w.open0("p:custShowLst");
        for (i, cs) in p.custom_shows.iter().enumerate() {
            w.open("p:custShow", A::new().a("name", &cs.name).a("id", i));
            w.open0("p:sldLst");
            for sid in &cs.slides {
                if let Some(part) = x.slide_parts.get(sid).cloned() {
                    let rid = o.rels.add("slide", &opc::relative("ppt/presentation.xml", &part), false);
                    w.empty("p:sld", A::new().a("r:id", rid));
                }
            }
            w.close("p:sldLst");
            w.close("p:custShow");
        }
        w.close("p:custShowLst");
    }
    let dts = if p.default_text_style.is_empty() { defaults::default_text_style() } else { p.default_text_style.clone() };
    dml::list_style(&mut w, x, o, "p:defaultTextStyle", &dts);
    let sections = section_slides(p);
    let raw = p.raw_ext.as_deref().filter(|r| !r.trim().is_empty());
    if !sections.is_empty() || raw.is_some() {
        let mut inner = W::frag();
        if !sections.is_empty() {
            inner.open("p:ext", A::new().a("uri", crate::SECTION_EXT_URI));
            inner.open("p14:sectionLst", A::new().a("xmlns:p14", NS_P14));
            for (i, (name, list)) in sections.iter().enumerate() {
                inner.open("p14:section", A::new().a("name", name).a("id", crate::tables::guid(&format!("section:{i}:{name}"))));
                inner.open0("p14:sldIdLst");
                for si in list {
                    inner.empty("p14:sldId", A::new().a("id", 256 + si));
                }
                inner.close("p14:sldIdLst");
                inner.close("p14:section");
            }
            inner.close("p14:sectionLst");
            inner.close("p:ext");
        }
        if let Some(r) = raw {
            inner.raw(r);
        }
        ext_list(&mut w, Some(&inner.s));
    }
    w.close("p:presentation");
    w.finish()
}

fn pres_props_xml(p: &Presentation) -> Vec<u8> {
    let s = &p.show;
    let mut w = W::new();
    root_open(&mut w, "p:presentationPr", A::new());
    w.open(
        "p:showPr",
        A::new()
            .t("loop", s.loop_until_esc)
            .a("showNarration", if s.without_narration { 0 } else { 1 })
            .o("showAnimation", s.without_animation.then_some("0"))
            .o("useTimings", (!s.use_timings).then_some("0")),
    );
    match s.show_type.as_str() {
        "browsed" => w.empty("p:browse", A::new()),
        "kiosk" => w.empty("p:kiosk", A::new()),
        _ => w.empty0("p:present"),
    }
    let custom = s.custom_show.as_ref().and_then(|n| p.custom_shows.iter().position(|c| c.name == *n));
    match (custom, s.range) {
        (Some(i), _) => w.empty("p:custShow", A::new().a("id", i)),
        (None, Some((a, b))) => w.empty("p:sldRg", A::new().a("st", a.max(1)).a("end", b.max(a).max(1))),
        _ => w.empty0("p:sldAll"),
    }
    w.open0("p:penClr");
    w.val("a:srgbClr", s.pen_color.hex());
    w.close("p:penClr");
    w.close("p:showPr");
    w.close("p:presentationPr");
    w.finish()
}

fn view_props_xml() -> Vec<u8> {
    let mut w = W::new();
    root_open(&mut w, "p:viewPr", A::new());
    w.open0("p:normalViewPr");
    w.empty("p:restoredLeft", A::new().a("sz", 15620));
    w.empty("p:restoredTop", A::new().a("sz", 94660));
    w.close("p:normalViewPr");
    w.empty("p:gridSpacing", A::new().a("cx", 76200).a("cy", 76200));
    w.close("p:viewPr");
    w.finish()
}

fn core_xml(p: &Presentation) -> Vec<u8> {
    let pr = &p.props;
    let mut w = W::new();
    w.open(
        "cp:coreProperties",
        A::new()
            .a("xmlns:cp", "http://schemas.openxmlformats.org/package/2006/metadata/core-properties")
            .a("xmlns:dc", "http://purl.org/dc/elements/1.1/")
            .a("xmlns:dcterms", "http://purl.org/dc/terms/")
            .a("xmlns:dcmitype", "http://purl.org/dc/dcmitype/")
            .a("xmlns:xsi", "http://www.w3.org/2001/XMLSchema-instance"),
    );
    for (tag, v) in [
        ("dc:title", &pr.title),
        ("dc:subject", &pr.subject),
        ("dc:creator", &pr.author),
        ("cp:keywords", &pr.keywords),
        ("dc:description", &pr.comments),
        ("cp:category", &pr.category),
        ("cp:lastModifiedBy", &pr.last_modified_by),
    ] {
        if !v.is_empty() {
            w.elt(tag, v);
        }
    }
    if pr.revision > 0 {
        w.elt("cp:revision", &pr.revision.to_string());
    }
    for (tag, v) in [("dcterms:created", &pr.created), ("dcterms:modified", &pr.modified)] {
        if valid_date(v) {
            w.open(tag, A::new().a("xsi:type", "dcterms:W3CDTF"));
            w.text(v);
            w.close(tag);
        }
    }
    w.close("cp:coreProperties");
    w.finish()
}

fn app_xml(p: &Presentation) -> Vec<u8> {
    let mut w = W::new();
    w.open(
        "Properties",
        A::new()
            .a("xmlns", "http://schemas.openxmlformats.org/officeDocument/2006/extended-properties")
            .a("xmlns:vt", "http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes"),
    );
    w.elt("Application", "DeckCraft");
    w.elt("Slides", &p.slides.len().to_string());
    w.elt("Notes", &p.slides.iter().filter(|s| !s.notes.is_empty()).count().to_string());
    w.elt("HiddenSlides", &p.slides.iter().filter(|s| s.hidden).count().to_string());
    if !p.props.company.is_empty() {
        w.elt("Company", &p.props.company);
    }
    w.close("Properties");
    w.finish()
}
