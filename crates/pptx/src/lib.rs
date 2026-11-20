//! PPTX import and export: Office Open XML PresentationML / DrawingML, delegated to the shared
//! `craft-pptx` crate.
//!
//! [`import`] reads a package leniently (unknown parts are skipped, malformed parts are logged and
//! ignored, every size and depth is bounded) into the DeckCraft model; [`export`] writes a valid
//! package that PowerPoint opens without repair.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod cfb;
mod convert;
mod xml;

pub use craft_pptx::MAX_TAG_BYTES;
/// The bounded XML reader the importer uses, for callers that read kept fragments (a shape's
/// 3-D settings) instead of scanning strings.
pub use xml::{Doc as XmlDoc, El as XmlEl, parse as parse_xml};

use deckcraft_model::{Presentation, Shape, ShapeKind, defaults};

#[derive(Debug, thiserror::Error)]
pub enum PptxError {
    #[error("not a PPTX file: {0}")]
    NotPptx(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("write failed: {0}")]
    Write(String),
    #[error("malformed XML: {0}")]
    Xml(String),
}

impl From<craft_pptx::Error> for PptxError {
    fn from(e: craft_pptx::Error) -> Self {
        match e {
            craft_pptx::Error::NotPptx(m) => PptxError::NotPptx(m),
            craft_pptx::Error::Unsupported(m) => PptxError::Unsupported(m),
            craft_pptx::Error::Write(m) => PptxError::Write(m),
            craft_pptx::Error::Xml(m) => PptxError::Xml(m),
        }
    }
}

struct DeckDefaults;

impl craft_pptx::Defaults for DeckDefaults {
    fn master(&self, size: deckcraft_geom::Size, after: u32) -> (craft_pptx::Master, u32) {
        let (m, last) = defaults::build_master(size, Default::default(), after);
        (convert::master_to_craft(&m), last)
    }

    fn theme(&self) -> craft_drawingml::theme::Theme {
        convert::theme_to_craft(&deckcraft_model::theme::Theme::default())
    }

    fn master_styles(&self) -> (craft_drawingml::text::ListStyle, craft_drawingml::text::ListStyle, craft_drawingml::text::ListStyle) {
        let (t, b, o) = defaults::master_styles();
        (convert::list_style_to_craft(&t), convert::list_style_to_craft(&b), convert::list_style_to_craft(&o))
    }

    fn default_text_style(&self) -> craft_drawingml::text::ListStyle {
        convert::list_style_to_craft(&defaults::default_text_style())
    }

    fn application(&self) -> &str {
        "DeckCraft"
    }
}

/// Does this look like an OOXML package (zip with `[Content_Types].xml`)?
///
/// The part is looked up in the archive's central directory, not in the leading bytes: writers may
/// store it anywhere in the archive (recent PowerPoint builds put it last, megabytes in).
pub fn sniff(bytes: &[u8]) -> bool {
    craft_pptx::sniff(bytes)
}

/// Is this a password-protected presentation (an encrypted package inside a compound file)?
pub fn is_encrypted(bytes: &[u8]) -> bool {
    cfb::is_encrypted(bytes)
}

const MAX_SLIDE_ENTRIES: usize = 10_000;

/// Checks `ppt/presentation.xml` for > 10,000 slide-list entries (refused instead of truncated)
/// and collects any `sldId` entries whose relationship is missing so `import_checked` /
/// `import_report` report them as lost slides.
fn preflight_slides(bytes: &[u8]) -> Result<Vec<String>, PptxError> {
    let Ok(mut pkg) = craft_opc::LazyPackage::open(bytes, &craft_pptx::LIMITS) else {
        return Ok(vec![]);
    };
    let root_rels = pkg.rels("").unwrap_or_default();
    let pres_path = root_rels.first("officeDocument").map(|r| r.target.clone()).unwrap_or_else(|| "ppt/presentation.xml".into());
    let Ok(Some(pres_bytes)) = pkg.read_limited(&pres_path, craft_pptx::MAX_XML_PART as u64) else {
        return Ok(vec![]);
    };
    let Ok(pres_el) = craft_drawingml::xmlx::parse(&pres_bytes) else {
        return Ok(vec![]);
    };
    let pres_rels = pkg.rels(&pres_path).unwrap_or_default();
    let mut lost = Vec::new();
    if let Some(list) = pres_el.child(craft_xml::Local("sldIdLst")) {
        let mut count = 0usize;
        for s in list.children(craft_xml::Local("sldId")) {
            count += 1;
            if count > MAX_SLIDE_ENTRIES {
                return Err(PptxError::Unsupported(format!("presentation has more than {MAX_SLIDE_ENTRIES} slides")));
            }
            let rid = s.attr_local("r:id").unwrap_or("");
            if pres_rels.target(rid).is_none() {
                let id = s.attr_local("id").and_then(craft_xml::value::to_u32_lossy).unwrap_or(0);
                lost.push(format!("slide {id} (missing relationship {rid})"));
            }
        }
    }
    Ok(lost)
}

fn has_unfolded_paragraph_builds(p: &Presentation) -> bool {
    p.slides.iter().any(|s| s.animations.windows(2).any(|w| w[0].paragraph.is_some() && w[1].paragraph.is_some() && w[0].shape == w[1].shape))
}

fn slide_opt_out_spids(bytes: &[u8]) -> Vec<std::collections::HashSet<u32>> {
    let Ok(mut pkg) = craft_opc::LazyPackage::open(bytes, &craft_pptx::LIMITS) else {
        return vec![];
    };
    let root_rels = pkg.rels("").unwrap_or_default();
    let pres_path = root_rels.first("officeDocument").map(|r| r.target.clone()).unwrap_or_else(|| "ppt/presentation.xml".into());
    let Ok(Some(pres_bytes)) = pkg.read_limited(&pres_path, craft_pptx::MAX_XML_PART as u64) else {
        return vec![];
    };
    let Ok(pres_el) = craft_drawingml::xmlx::parse(&pres_bytes) else {
        return vec![];
    };
    let pres_rels = pkg.rels(&pres_path).unwrap_or_default();
    let Some(list) = pres_el.child(craft_xml::Local("sldIdLst")) else {
        return vec![];
    };
    let mut out = Vec::new();
    for s in list.children(craft_xml::Local("sldId")).take(MAX_SLIDE_ENTRIES) {
        let mut set = std::collections::HashSet::new();
        if let Some(target) = s.attr_local("r:id").and_then(|rid| pres_rels.target(rid))
            && let Ok(Some(sb)) = pkg.read_limited(target, craft_pptx::MAX_XML_PART as u64)
            && let Ok(se) = craft_drawingml::xmlx::parse(&sb)
            && let Some(bld_lst) = se.path(&[craft_xml::Local("timing"), craft_xml::Local("bldLst")])
        {
            for b in bld_lst.children(craft_xml::Local("bldP")) {
                if b.attr_local("build").unwrap_or("") != "p"
                    && let Some(spid) = b.attr_local("spid").and_then(craft_xml::value::to_u32_lossy)
                {
                    set.insert(spid);
                }
            }
        }
        out.push(set);
    }
    out
}

fn span_ms(a: &deckcraft_model::anim::Animation) -> u32 {
    let iterations = if a.repeat == u32::MAX || a.repeat <= 1 { 1 } else { a.repeat.min(1_000) };
    a.duration_ms.saturating_mul(if a.auto_reverse { 2 } else { 1 }).saturating_mul(iterations)
}

fn plays_as_build(effects: &[&deckcraft_model::anim::Animation], text: &deckcraft_model::text::TextBody) -> bool {
    use deckcraft_model::anim::{AnimStart, Animation, BuildRole, build_filled, paragraph_build_roles};
    let Some(first) = effects.first() else { return false };
    if first.start != AnimStart::OnClick && (span_ms(first) != 500 || first.delay_ms != 0) {
        return false;
    }
    let roles = paragraph_build_roles(text);
    let expected = roles.iter().enumerate().filter(|(i, r)| match r {
        BuildRole::Top => true,
        BuildRole::Member(_) => text.paragraphs.get(*i).is_some_and(build_filled),
        BuildRole::Unowned => false,
    });
    let settings = |a: &Animation| Animation { start: AnimStart::OnClick, paragraph: None, ..a.clone() };
    let base = settings(first);
    let mut matched = 0;
    for (k, (i, role)) in expected.enumerate() {
        let Some(a) = effects.get(k) else { return false };
        let start_ok = match role {
            BuildRole::Top if k == 0 => true,
            BuildRole::Top if first.start == AnimStart::OnClick => a.start == AnimStart::OnClick,
            BuildRole::Top => a.start == AnimStart::AfterPrevious,
            _ => a.start == AnimStart::WithPrevious,
        };
        if a.paragraph != u32::try_from(i).ok() || !start_ok || settings(a) != base {
            return false;
        }
        matched = k + 1;
    }
    matched == effects.len()
}

fn fold_slide_paragraph_builds(slide: &mut deckcraft_model::Slide, opt_out: &std::collections::HashSet<u32>) {
    use deckcraft_model::anim::{Animation, TextBuild};
    let same = |x: &Animation, y: &Animation| {
        x.paragraph.is_some()
            && y.paragraph.is_some()
            && x.text_build == TextBuild::AsOne
            && y.text_build == TextBuild::AsOne
            && x.shape == y.shape
            && x.trigger == y.trigger
            && !opt_out.contains(&x.shape.0)
    };
    let mut folded = Vec::with_capacity(slide.animations.len());
    for run in slide.animations.chunk_by(same) {
        let effects: Vec<&Animation> = run.iter().collect();
        let text = effects.first().and_then(|a| deckcraft_model::find_shape(&slide.shapes, a.shape)).and_then(|s| s.text.as_ref());
        if run.len() > 1
            && let (Some(first), Some(text)) = (effects.first(), text)
            && !opt_out.contains(&first.shape.0)
            && plays_as_build(&effects, text)
        {
            folded.push(Animation { text_build: TextBuild::ByParagraph, paragraph: None, ..(*first).clone() });
        } else {
            folded.extend(run.iter().cloned());
        }
    }
    slide.animations = folded;
}

fn fold_paragraph_builds(bytes: &[u8], pres: &mut Presentation) {
    if !has_unfolded_paragraph_builds(pres) {
        return;
    }
    let opt_outs = slide_opt_out_spids(bytes);
    let empty = std::collections::HashSet::new();
    for (i, slide) in pres.slides.iter_mut().enumerate() {
        let opt_out = opt_outs.get(i).unwrap_or(&empty);
        fold_slide_paragraph_builds(std::sync::Arc::make_mut(slide), opt_out);
    }
}

/// Read a `.pptx` / `.potx` / `.ppsx` package.
pub fn import(bytes: &[u8]) -> Result<Presentation, PptxError> {
    preflight_slides(bytes)?;
    let mut pres = craft_pptx::import(bytes, &DeckDefaults).map(convert::pres_from_craft)?;
    fold_paragraph_builds(bytes, &mut pres);
    Ok(pres)
}

/// [`import`], plus the part names of slides the file lists but that couldn't be read and were
/// left out.
/// Files with more than 10,000 slide-list entries return [`PptxError::Unsupported`] instead of a truncated presentation.
pub fn import_checked(bytes: &[u8]) -> Result<(Presentation, Vec<String>), PptxError> {
    let (p, lost, _) = import_report(bytes)?;
    Ok((p, lost))
}

/// [`import_checked`], plus warnings for content cut to fit a limit (a table past the cell cap
/// keeps its leading rows); the file still opens.
pub fn import_report(bytes: &[u8]) -> Result<(Presentation, Vec<String>, Vec<String>), PptxError> {
    let mut missing = preflight_slides(bytes)?;
    let (p, lost, warnings) = craft_pptx::import_report(bytes, &DeckDefaults)?;
    missing.extend(lost);
    let mut pres = convert::pres_from_craft(p);
    fold_paragraph_builds(bytes, &mut pres);
    Ok((pres, missing, warnings))
}

/// [`import`] with a lower presentation-wide chart point budget, so tests reach the over-budget
/// path without a multi-million-point chart.
#[doc(hidden)]
pub fn import_with_chart_budget(bytes: &[u8], points: usize) -> Result<Presentation, PptxError> {
    preflight_slides(bytes)?;
    let mut pres = craft_pptx::import_with_chart_budget(bytes, &DeckDefaults, points).map(convert::pres_from_craft)?;
    fold_paragraph_builds(bytes, &mut pres);
    Ok(pres)
}

/// Read the theme of a `.thmx` file, or of a `.pptx` / `.potx`'s first master, with its media.
pub fn import_theme(bytes: &[u8]) -> Result<(deckcraft_model::theme::Theme, Vec<deckcraft_model::MediaItem>), PptxError> {
    let (theme, media) = craft_pptx::import_theme(bytes)?;
    Ok((convert::theme_from_craft(theme), media.into_iter().map(convert::media_item_from_craft).collect()))
}

fn shapes_have_over_cap_table(shapes: &[Shape]) -> bool {
    shapes.iter().any(|s| match &s.kind {
        ShapeKind::Table(t) => t.over_cap(),
        ShapeKind::Group { children, .. } => shapes_have_over_cap_table(children),
        _ => false,
    })
}

fn check_table_caps(p: &Presentation) -> Result<(), PptxError> {
    let over = p.slides.iter().any(|s| shapes_have_over_cap_table(&s.shapes))
        || p.masters.iter().any(|m| shapes_have_over_cap_table(&m.shapes) || m.layouts.iter().any(|l| shapes_have_over_cap_table(&l.shapes)));
    if over {
        return Err(PptxError::Unsupported(format!(
            "a table has more than {} cells; export refused rather than drop rows",
            deckcraft_model::table::MAX_CELLS
        )));
    }
    Ok(())
}

/// Write a `.pptx` package.
pub fn export(p: &Presentation) -> Result<Vec<u8>, PptxError> {
    check_table_caps(p)?;
    craft_pptx::export(&convert::pres_to_craft(p), &DeckDefaults).map_err(Into::into)
}

/// Write a package for `ext`: `potx` (template) and `ppsx` (slide show) differ from `pptx` only in
/// the main part's content type.
pub fn export_as(p: &Presentation, ext: &str) -> Result<Vec<u8>, PptxError> {
    check_table_caps(p)?;
    craft_pptx::export_as(&convert::pres_to_craft(p), &DeckDefaults, ext).map_err(Into::into)
}

#[cfg(test)]
mod import_theme_tests {
    use std::io::{Read, Write};

    use deckcraft_model::Presentation;

    fn custom() -> Presentation {
        let mut p = Presentation::default();
        let m = std::sync::Arc::make_mut(&mut p.masters[0]);
        m.theme.name = "Harbour".into();
        m.theme.fonts.major.latin = "Lato".into();
        m.theme.fonts.minor.latin = "Source Sans 3".into();
        m.theme.colors.set(deckcraft_color::SchemeSlot::Accent1, deckcraft_color::Rgba::rgb(0x12, 0x34, 0x56));
        p
    }

    /// A `.thmx`: the deck's theme part as the package's main document.
    fn thmx(pptx: &[u8]) -> Vec<u8> {
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(pptx)).unwrap();
        let mut theme = String::new();
        z.by_name("ppt/theme/theme1.xml").unwrap().read_to_string(&mut theme).unwrap();
        let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let o = zip::write::SimpleFileOptions::default();
        out.start_file("[Content_Types].xml", o).unwrap();
        out.write_all(br#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/theme/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/></Types>"#).unwrap();
        out.start_file("_rels/.rels", o).unwrap();
        out.write_all(br#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="theme/theme/theme1.xml"/></Relationships>"#).unwrap();
        out.start_file("theme/theme/theme1.xml", o).unwrap();
        out.write_all(theme.as_bytes()).unwrap();
        out.finish().unwrap().into_inner()
    }

    #[test]
    fn reads_the_theme_of_a_pptx_and_of_a_thmx() {
        let pptx = crate::export(&custom()).unwrap();
        for bytes in [pptx.clone(), thmx(&pptx)] {
            let (t, _) = crate::import_theme(&bytes).unwrap();
            assert_eq!(t.name, "Harbour");
            assert_eq!(t.fonts.major.latin, "Lato");
            assert_eq!(t.fonts.minor.latin, "Source Sans 3");
            assert_eq!(t.colors.get(deckcraft_color::SchemeSlot::Accent1), deckcraft_color::Rgba::rgb(0x12, 0x34, 0x56));
        }
    }

    #[test]
    fn non_theme_packages_are_errors() {
        assert!(crate::import_theme(b"not a zip").is_err());
        let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        out.start_file("[Content_Types].xml", zip::write::SimpleFileOptions::default()).unwrap();
        out.write_all(b"<Types/>").unwrap();
        assert!(crate::import_theme(&out.finish().unwrap().into_inner()).is_err());
    }
}

#[cfg(test)]
mod table_direction_tests {
    use deckcraft_model::table::{Cell, Table};
    use deckcraft_model::{Presentation, Shape, ShapeId, ShapeKind, defaults};
    use std::sync::Arc;

    fn table(p: &Presentation) -> &Table {
        p.slides[0]
            .shapes
            .iter()
            .find_map(|shape| match &shape.kind {
                ShapeKind::Table(table) => Some(table),
                _ => None,
            })
            .expect("table shape")
    }

    #[test]
    fn table_direction_and_column_order_survive_two_pptx_saves() {
        for rtl in [Some(true), Some(false), None] {
            let mut p = defaults::new_presentation(None);
            let mut t = Table::new(1, 2, 200.0, 30.0);
            t.rtl = rtl;
            t.rows[0].cells[0] = Cell::new("First column");
            t.rows[0].cells[1] = Cell::new("Second column");
            let shape = Shape { id: ShapeId(p.alloc_id().expect("id")), kind: ShapeKind::Table(t), ..Default::default() };
            Arc::make_mut(&mut p.slides[0]).shapes.push(shape);

            let mut roundtrip = crate::import(&crate::export(&p).expect("export")).expect("import");
            for _ in 0..2 {
                let t = table(&roundtrip);
                assert_eq!(t.rtl, rtl);
                assert_eq!(t.rows[0].cells[0].text.paragraphs[0].runs[0].text, "First column");
                assert_eq!(t.rows[0].cells[1].text.paragraphs[0].runs[0].text, "Second column");
                roundtrip = crate::import(&crate::export(&roundtrip).expect("export again")).expect("import again");
            }
        }
    }

    #[test]
    fn over_cap_table_export_is_refused() {
        let mut p = defaults::new_presentation(None);
        let mut t = Table::new(1, 2, 200.0, 30.0);
        t.rows.resize_with(deckcraft_model::table::MAX_CELLS / 2 + 1, || deckcraft_model::table::Row {
            height: 20.0,
            cells: vec![Cell::new("x"), Cell::new("y")],
        });
        let shape = Shape { id: ShapeId(p.alloc_id().expect("id")), kind: ShapeKind::Table(t), ..Default::default() };
        Arc::make_mut(&mut p.slides[0]).shapes.push(shape);
        let err = crate::export(&p).expect_err("over-cap table must be refused");
        assert!(matches!(err, crate::PptxError::Unsupported(_)));
    }
}
