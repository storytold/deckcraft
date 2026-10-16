use std::{collections::BTreeMap, io::Cursor, sync::Arc};

use deckcraft_model::{defaults, style::PictureFill, *};

use crate::{
    MIME, OdpError, Result,
    loss_report::{ImportReport, Part as AuditPart},
    part,
    xml::{self, El, Node},
};

#[path = "read_chart.rs"]
mod chart;
#[path = "read_comments.rs"]
pub(crate) mod comments_reader;
#[path = "read_extras.rs"]
mod extras;
#[path = "read_masters.rs"]
mod masters_reader;
#[path = "read_notes.rs"]
mod notes_reader;
#[path = "read_resources.rs"]
mod resources;
#[path = "read_templates.rs"]
mod templates_reader;
#[path = "read_text.rs"]
mod text_reader;

pub(crate) type Props = BTreeMap<String, String>;
pub(crate) type Styles<'a> = BTreeMap<(String, String), &'a El>;

/// Import-wide formula fuel travels with the existing resource context. Media
/// helpers still borrow only the image map through Deref; no budget is reset per table.
#[derive(Default)]
struct ReadResources {
    images: BTreeMap<String, MediaId>,
    formulas: deckcraft_model::table::FormulaBudget,
}
impl std::ops::Deref for ReadResources {
    type Target = BTreeMap<String, MediaId>;
    fn deref(&self) -> &Self::Target {
        &self.images
    }
}
impl std::ops::DerefMut for ReadResources {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.images
    }
}

fn parse_part(bytes: &[u8], total: &mut u64, source: xml::SourcePart) -> Result<El> {
    let root = xml::parse_part(bytes, source)?;
    *total = total.saturating_add(root.identity_metadata_bytes() as u64);
    if *total > crate::MAX_TOTAL {
        return Err(OdpError("XML namespace identities exceed total import budget".into()));
    }
    source_ids(&root, total)?;
    Ok(root)
}

fn charge_styles(total: &mut u64, bytes: usize) -> Result<()> {
    *total = total.saturating_add(bytes as u64);
    if *total > crate::MAX_TOTAL {
        return Err(OdpError("style indexes exceed total import budget".into()));
    }
    Ok(())
}

fn clone_styles<'a>(styles: &Styles<'a>, total: &mut u64) -> Result<Styles<'a>> {
    let bytes = styles.keys().fold(0usize, |sum, (family, name)| sum.saturating_add(256).saturating_add(family.len()).saturating_add(name.len()));
    charge_styles(total, bytes)?;
    Ok(styles.clone())
}

fn owner_styles<'a>(base: &Styles<'a>, owner: &'a El, default_layers: Option<&'a El>, total: &mut u64) -> Result<Styles<'a>> {
    let mut styles = clone_styles(base, total)?;
    styles.retain(|(family, _), _| family != "layer");
    if owner.elements().filter(|el| el.name == "draw:layer-set").take(2).count() > 1 {
        return Err(OdpError("multiple layer sets for one ODF page owner".into()));
    }
    if let Some(set) = owner.child("draw:layer-set").or(default_layers) {
        let mut names = std::collections::BTreeSet::new();
        for layer in set.elements() {
            if layer.name != "draw:layer" {
                return Err(OdpError("unsupported ODF layer definition".into()));
            }
            let name = layer.attr("draw:name").filter(|name| !name.is_empty()).ok_or_else(|| OdpError("unnamed ODF layer".into()))?;
            charge_styles(total, 256usize.saturating_add(name.len()))?;
            if !names.insert(name) {
                return Err(OdpError("duplicate ODF layer identity".into()));
            }
            if layer.attr("draw:display").is_some_and(|value| !matches!(value, "always" | "none")) {
                return Err(OdpError("conditional ODF layer visibility is not supported".into()));
            }
            if layer.attr("draw:protected").is_some_and(|value| !matches!(value, "false" | "0")) {
                return Err(OdpError("protected ODF layers require source recovery".into()));
            }
            styles.insert(("layer".into(), name.into()), layer);
        }
    }
    Ok(styles)
}

fn required_properties(styles: &Styles<'_>, family: &str, name: Option<&str>) -> Result<Props> {
    let mut current = name;
    let mut seen = std::collections::BTreeSet::new();
    while let Some(name) = current {
        if seen.len() >= 32 || !seen.insert(name) {
            return Err(OdpError("cyclic or excessive required ODF style chain".into()));
        }
        let style = styles.get(&(family.into(), name.into())).ok_or_else(|| OdpError("missing required ODF style in its package part".into()))?;
        current = style.attr("style:parent-style-name");
    }
    properties(styles, family, name)
}

fn collect_styles<'a>(root: &'a El, styles: &mut Styles<'a>, total: &mut u64) -> Result<()> {
    fn visit<'a>(el: &'a El, styles: &mut Styles<'a>, seen: &mut std::collections::BTreeSet<(&'a str, &'a str)>, total: &mut u64) -> Result<()> {
        let identity = match el.name.as_str() {
            "style:default-style" => Some((el.attr("style:family").unwrap_or(""), "")),
            "style:font-face" => Some(("font", el.attr("style:name").unwrap_or(""))),
            "text:list-style" => Some(("list", el.attr("style:name").unwrap_or(""))),
            "number:number-style"
            | "number:percentage-style"
            | "number:currency-style"
            | "number:date-style"
            | "number:time-style"
            | "number:boolean-style"
            | "number:text-style" => Some(("data", el.attr("style:name").unwrap_or(""))),
            "style:style" => Some((el.attr("style:family").unwrap_or(""), el.attr("style:name").unwrap_or(""))),
            "draw:gradient" | "svg:linearGradient" | "svg:radialGradient" => el.attr("draw:name").map(|name| ("gradient", name)),
            "draw:hatch" => el.attr("draw:name").map(|name| ("hatch", name)),
            "draw:fill-image" => el.attr("draw:name").map(|name| ("fill-image", name)),
            "draw:stroke-dash" => el.attr("draw:name").map(|name| ("dash", name)),
            _ => None,
        };
        if let Some((family, name)) = identity {
            // Charge key copies and both bounded lookup nodes before allocation.
            charge_styles(total, 256usize.saturating_add(family.len()).saturating_add(name.len()))?;
            if !seen.insert((family, name)) {
                return Err(OdpError("duplicate or ambiguous ODF style definitions in one part".into()));
            }
            let key = (family.to_string(), name.to_string());
            if let Some(previous) = styles.get(&key) {
                // Identical plain font declarations carry the same literal face
                // description. Other common/automatic overlaps need a scoped parent
                // resolver; do not silently overwrite a different source owner.
                let same_font = el.name == "style:font-face"
                    && previous.name == el.name
                    && el.attrs == previous.attrs
                    && el.children.is_empty()
                    && previous.children.is_empty();
                if !same_font {
                    return Err(OdpError("overlapping common and automatic ODF style identities are not supported".into()));
                }
            } else {
                styles.insert(key, el);
            }
        }
        for child in el.elements() {
            visit(child, styles, seen, total)?;
        }
        Ok(())
    }
    visit(root, styles, &mut std::collections::BTreeSet::new(), total)
}

fn source_ids(root: &El, total: &mut u64) -> Result<()> {
    fn visit<'a>(el: &'a El, seen: &mut std::collections::BTreeSet<&'a str>, total: &mut u64) -> Result<()> {
        if let Some(id) = el.attr("xml:id") {
            charge_styles(total, 64)?;
            if !seen.insert(id) {
                return Err(OdpError("duplicate xml:id in one ODF package part".into()));
            }
        }
        for child in el.elements() {
            visit(child, seen, total)?;
        }
        Ok(())
    }
    visit(root, &mut std::collections::BTreeSet::new(), total)
}

fn audit_source_effects(presentation: &mut Presentation, part: &str, root: &El, total: &mut u64) -> Result<()> {
    let mut report = crate::source_effects::Report::default();
    report.scan(part, root, total)?;
    for (present, warning) in [
        (report.has_gamma(), crate::source_effects::GAMMA_WARNING),
        (report.has_protection(), crate::source_effects::PROTECTION_WARNING),
        (report.has_calculation_settings(), crate::source_effects::CALCULATION_WARNING),
    ] {
        if present && !presentation.tags.iter().any(|(key, value)| key == "odp:warning" && value == warning) {
            if presentation.tags.len() >= 1000 {
                return Err(OdpError("source-only effect diagnostic exceeds tag limit".into()));
            }
            presentation.tags.push(("odp:warning".into(), warning.into()));
        }
    }
    Ok(())
}

fn audit_xml(presentation: &mut Presentation, part: AuditPart, root: &El, total: &mut u64) -> Result<()> {
    let mut report = ImportReport::default();
    report.audit(part, root);
    publish_audit(presentation, &report);
    let name = match part {
        AuditPart::Content => "content.xml",
        AuditPart::Styles => "styles.xml",
        AuditPart::Metadata => "meta.xml",
        AuditPart::Embedded => "embedded",
    };
    audit_source_effects(presentation, name, root, total)
}

fn audit_line_heights(presentation: &mut Presentation, part: AuditPart, root: &El, styles: &Styles<'_>, total: &mut u64) -> Result<()> {
    let mut report = ImportReport::default();
    report.audit_line_heights(part, root, styles, total)?;
    publish_audit(presentation, &report);
    Ok(())
}

fn publish_audit(presentation: &mut Presentation, report: &ImportReport) {
    if report.has_blocking_loss() {
        // A capped detail list can never suppress the export guard. Existing warnings already
        // block export; otherwise reserve one fixed summary before any optional diagnostics.
        if !presentation.tags.iter().any(|(key, _)| key == "odp:warning") {
            presentation.tags.push(("odp:warning".into(), report.summary().into()));
        } else {
            extras::warn(presentation, report.summary());
        }
        if !presentation.tags.iter().any(|(key, _)| key == "odp:semantic-audit") && presentation.tags.len() < 1000 {
            presentation.tags.push((
                "odp:semantic-audit".into(),
                format!(
                    "coverage={}; retained={}; omitted={}",
                    crate::loss_report::COVERAGE_VERSION,
                    report.issues().len(),
                    report.omitted_issue_count()
                ),
            ));
        }
    }
}

/// Read a packaged ODP. Remote images and embedded objects are never fetched or executed.
pub fn import(bytes: &[u8]) -> Result<Presentation> {
    if bytes.len() as u64 > crate::MAX_TOTAL {
        return Err(OdpError("source backup exceeds size limit".into()));
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| OdpError(e.to_string()))?;
    let mut total = 0;
    if part(&mut zip, "mimetype", &mut total)? != MIME.as_bytes() {
        return Err(OdpError("not an OpenDocument presentation".into()));
    }
    let manifest = parse_part(&part(&mut zip, "META-INF/manifest.xml", &mut total)?, &mut total, xml::SourcePart::Manifest)?;
    let mut encrypted = Vec::new();
    manifest.descendants("manifest:encryption-data", &mut encrypted);
    if !encrypted.is_empty() {
        return Err(OdpError("password-protected presentations are not supported".into()));
    }
    let content = parse_part(&part(&mut zip, "content.xml", &mut total)?, &mut total, xml::SourcePart::Content)?;
    let styles_doc = if zip.index_for_name("styles.xml").is_some() {
        Some(parse_part(&part(&mut zip, "styles.xml", &mut total)?, &mut total, xml::SourcePart::Styles)?)
    } else {
        None
    };
    let mut styles = Styles::new();
    let mut master_styles = Styles::new();
    if let Some(root) = &styles_doc {
        collect_styles(root, &mut master_styles, &mut total)?;
        if let Some(common) = root.child("office:styles") {
            collect_styles(common, &mut styles, &mut total)?;
        }
        if let Some(fonts) = root.child("office:font-face-decls") {
            collect_styles(fonts, &mut styles, &mut total)?;
        }
    }
    collect_styles(&content, &mut styles, &mut total)?;
    if styles_doc.is_none() {
        master_styles = clone_styles(&styles, &mut total)?;
    }
    let body =
        content.child("office:body").and_then(|b| b.child("office:presentation")).ok_or_else(|| OdpError("missing presentation body".into()))?;
    let pages: Vec<_> = body.elements().filter(|e| e.name == "draw:page").collect();
    if pages.len() > 10_000 {
        return Err(OdpError("too many slides".into()));
    }
    let master_index = masters_reader::MasterIndex::build(&content, styles_doc.as_ref(), &pages, &mut total)?;
    let default_layers = styles_doc.as_ref().unwrap_or(&content).child("office:master-styles").and_then(|root| root.child("draw:layer-set"));
    let masters = &master_index.sources;
    let layouts = &master_index.page_layouts;
    let page_size = |page: &El| -> Option<Size> {
        let master = masters.get(page.attr("draw:master-page-name")?)?;
        let layout = layouts.get(master.attr("style:page-layout-name")?)?;
        let props = layout.child("style:page-layout-properties")?;
        let size = Size::new(length(props.attr("fo:page-width")?)?, length(props.attr("fo:page-height")?)?);
        (size.width > 0.0 && size.height > 0.0 && size.width <= 5000.0 && size.height <= 5000.0).then_some(size)
    };
    let size = pages.first().and_then(|p| page_size(p)).unwrap_or(defaults::WIDE);
    let mut p = defaults::blank_presentation(size, Theme::default(), false);
    for registry in [&styles, &master_styles] {
        for ((family, name), style) in registry {
            if family == "data" && crate::table_number_styles::has_program(style) {
                charge_styles(&mut total, 512usize.saturating_add(style.attrs.values().map(String::len).sum::<usize>()))?;
                if crate::table_number_styles::read(registry, name).is_err() {
                    extras::warn(&mut p, "ODF number-format extension projection is incomplete or inconsistent; edited export blocked");
                }
            }
        }
    }
    let templates = templates_reader::TemplateRead::build(&content, styles_doc.as_ref(), &pages, &mut p, &mut total)?;
    if templates.unhandled_templates {
        extras::warn(&mut p, "Unsupported ODF presentation-template definitions remain source-recovery-only; edited export blocked");
    }
    if templates.unhandled_references {
        extras::warn(&mut p, "Unsupported ODF presentation-template references remain source-recovery-only; edited export blocked");
    }
    if templates.regular_object_semantics {
        extras::warn(
            &mut p,
            "ODF regular-page placeholder and presentation-style ownership require additional editable mapping; edited export blocked",
        );
    }
    p.tags.push(("odp:imported".into(), "1".into()));
    audit_source_effects(&mut p, "META-INF/manifest.xml", &manifest, &mut total)?;
    audit_xml(&mut p, AuditPart::Content, &content, &mut total)?;
    audit_line_heights(&mut p, AuditPart::Content, &content, &styles, &mut total)?;
    if let Some(styles_root) = &styles_doc {
        audit_xml(&mut p, AuditPart::Styles, styles_root, &mut total)?;
        audit_line_heights(&mut p, AuditPart::Styles, styles_root, &master_styles, &mut total)?;
    }
    if content.foreign_content() || styles_doc.as_ref().is_some_and(El::foreign_content) {
        extras::warn(
            &mut p,
            "Unmapped XML content or extensions are retained in the original ODP; their editing and export semantics are not fully supported",
        );
    }
    if zip.index_for_name("meta.xml").is_some() {
        let meta = parse_part(&part(&mut zip, "meta.xml", &mut total)?, &mut total, xml::SourcePart::Metadata)?;
        audit_xml(&mut p, AuditPart::Metadata, &meta, &mut total)?;
        extras::metadata(&meta, &mut p.props);
    }
    if zip.index_for_name("settings.xml").is_some() {
        let settings = parse_part(&part(&mut zip, "settings.xml", &mut total)?, &mut total, xml::SourcePart::Metadata)?;
        audit_source_effects(&mut p, "settings.xml", &settings, &mut total)?;
    }
    let layout = defaults::layout_of_kind(&p, LayoutType::Blank).unwrap_or_default();
    let mut images = ReadResources::default();
    let notes_masters = notes_reader::masters(masters, layouts, &master_styles, default_layers, body, &mut p, &mut zip, &mut total, &mut images)?;
    let master_bindings = master_index.import(&master_styles, &styles, &mut p, &mut zip, &mut total, &mut images)?;

    let mut comment_state = comments_reader::State::new();
    for (page_index, page) in pages.into_iter().enumerate() {
        if page_size(page).is_some_and(|s| s != size) {
            return Err(OdpError("mixed slide sizes are not supported".into()));
        }
        let page_state = master_index.page(page, &master_styles, &styles, &master_bindings, &mut p, &mut zip, &mut total, &mut images)?;
        let mut slide = Slide {
            id: SlideId(id(&mut p)?),
            presentation_template: templates.selection(page)?,
            layout: page_state.layout.unwrap_or(layout),
            name: page.attr("draw:name").unwrap_or("").into(),
            show_master_shapes: page_state.layout.is_some() && page_state.show_master_shapes,
            tags: source_tags(page, &mut total)?,
            ..Default::default()
        };
        let page_styles = owner_styles(&styles, page, default_layers, &mut total)?;
        let page_props = page_state.props;
        slide.hidden = page_state.hidden;
        if page_state.layout.is_some() {
            slide.background = page_state.background;
        } else {
            if let Some(master) = page.attr("draw:master-page-name").and_then(|n| masters.get(n)) {
                read_shapes(master, &page_styles, &mut p, &mut slide.shapes, &mut zip, &mut total, &mut images, Some(&page_props), None, None)?;
            }
            if let Some(fill) = resources::resolve_fill(&page_props, &mut p, &mut zip, &mut total, &mut images, size.width, size.height)? {
                slide.background = Some(Background::Fill { fill });
            }
        }
        read_shapes(page, &page_styles, &mut p, &mut slide.shapes, &mut zip, &mut total, &mut images, None, None, None)?;
        if let Some(notes) = page.child("presentation:notes") {
            let notes_master = page.attr("draw:master-page-name").and_then(|name| notes_masters.get(name)).copied();
            let source_master =
                page.attr("draw:master-page-name").and_then(|name| masters.get(name)).and_then(|master| master.child("presentation:notes"));
            let (notes_page, body_text) = notes_reader::page(
                notes,
                notes_master,
                source_master,
                layouts,
                &styles,
                default_layers,
                body,
                &mut p,
                &mut zip,
                &mut total,
                &mut images,
                page_index + 1,
            )?;
            slide.notes = body_text;
            slide.notes_page = Some(notes_page);
        }
        comment_state.page(page, page_index, &mut slide, &mut p, &page_styles)?;
        extras::timing(page, &page_styles, &page_props, &mut slide, &mut p, &mut zip, &mut total, &mut images)?;
        p.slides.push(Arc::new(slide));
    }
    comment_state.finish(&mut p, &mut zip, &manifest, &mut total)?;
    extras::finish_timing(&mut p);
    text_reader::resolve_links(&mut p);
    {
        let media = MediaId(id(&mut p)?);
        p.media.push(MediaItem {
            id: media,
            name: "source.odp".into(),
            content_type: "application/vnd.deckcraft.odp-source".into(),
            data: Arc::new(bytes.to_vec()),
            link: None,
        });
    }
    images.formulas.next_pass();
    deckcraft_model::table::validate_constants_with_budget(&p, &mut images.formulas).map_err(|error| OdpError(error.to_string()))?;
    deckcraft_model::presentation_template::validate(&p).map_err(|error| OdpError(error.to_string()))?;
    Ok(p)
}

fn id(p: &mut Presentation) -> Result<u32> {
    if p.next_id >= 2_000_000 {
        return Err(OdpError("too many objects".into()));
    }
    p.alloc_id().ok_or_else(|| OdpError("too many objects".into()))
}

/// ODF lengths are absolute physical units; the model uses points.
pub(crate) fn length(s: &str) -> Option<f64> {
    let s = s.trim();
    for (suffix, factor) in [("pt", 1.0), ("cm", 72.0 / 2.54), ("mm", 72.0 / 25.4), ("in", 72.0), ("pc", 12.0)] {
        if let Some(n) = s.strip_suffix(suffix) {
            return n.trim().parse::<f64>().ok().filter(|v| v.is_finite() && v.abs() <= 1_000_000.0).map(|v| v * factor);
        }
    }
    (s == "0").then_some(0.0)
}

pub(crate) fn properties(styles: &Styles<'_>, family: &str, name: Option<&str>) -> Result<Props> {
    let mut chain = Vec::new();
    let mut current = name;
    while let Some(name) = current {
        if chain.len() >= 32 {
            return Err(OdpError("cyclic or excessive style inheritance".into()));
        }
        let Some(style) = styles.get(&(family.into(), name.into())) else { break };
        chain.push(*style);
        current = style.attr("style:parent-style-name");
    }
    let mut props = Props::new();
    if let Some(default) = styles.get(&(family.into(), String::new())) {
        for e in default.elements() {
            props.extend(e.attrs.clone());
        }
    }
    for style in chain.into_iter().rev() {
        for e in style.elements() {
            if e.name.ends_with("-properties") {
                props.extend(e.attrs.clone());
            }
        }
    }
    if let Some(name) = props.get("style:font-name")
        && let Some(face) = styles.get(&("font".into(), name.clone()))
        && let Some(family) = face.attr("svg:font-family")
    {
        props.insert("fo:font-family".into(), family.to_string());
    }
    resources::expand(&mut props, styles);
    Ok(props)
}

pub(crate) fn color(s: &str) -> Option<ColorRef> {
    let s = s.strip_prefix('#')?;
    if s.len() != 6 || !s.is_ascii() {
        return None;
    }
    let n = u32::from_str_radix(s, 16).ok()?;
    Some(ColorRef::rgb(Rgba::rgb((n >> 16) as u8, (n >> 8) as u8, n as u8)))
}

pub(crate) fn fill(props: &Props) -> Result<Option<Fill>> {
    match props.get("draw:fill").map(String::as_str) {
        Some("none") => Ok(Some(Fill::None)),
        Some("solid") => {
            let mut color = props.get("draw:fill-color").and_then(|s| color(s)).ok_or_else(|| OdpError("solid fill has no valid color".into()))?;
            if let Some(alpha) =
                props.get("draw:opacity").and_then(|s| s.strip_suffix('%')).and_then(|s| s.parse::<f64>().ok()).filter(|a| a.is_finite())
            {
                color = color.with(ColorTransform::Alpha((alpha.clamp(0.0, 100.0) * 1000.0).round() as i32));
            }
            Ok(Some(Fill::solid(color)))
        }
        None => Ok(None),
        Some(_) => resources::special_fill(props),
    }
}

pub(crate) fn text(el: &El, styles: &Styles<'_>, paragraphs: &mut Vec<Paragraph>, level: u8) -> Result<()> {
    text_reader::text(el, styles, paragraphs, level)
}

pub(crate) use text_reader::TableTextState;
pub(crate) fn table_text_member(
    el: &El,
    styles: &Styles<'_>,
    name: Option<&str>,
    paragraphs: &mut Vec<Paragraph>,
    state: &mut TableTextState,
) -> Result<()> {
    text_reader::table_text_member(el, styles, name, paragraphs, state)
}

pub(crate) fn table_text_with_style(el: &El, styles: &Styles<'_>, name: Option<&str>, paragraphs: &mut Vec<Paragraph>) -> Result<()> {
    text_reader::table_text_with_style(el, styles, name, paragraphs)
}

pub(crate) fn table_cell_style_warnings(styles: &Styles<'_>, name: Option<&str>) -> Vec<&'static str> {
    let mut el = El::default();
    el.name = "table:table-cell".into();
    if let Some(name) = name {
        el.attrs.insert("table:style-name".into(), name.into());
    }
    text_reader::warnings(&el, styles, &Props::new())
}

#[allow(clippy::too_many_arguments)]
fn read_shapes(
    parent: &El,
    styles: &Styles<'_>,
    p: &mut Presentation,
    shapes: &mut Vec<Shape>,
    zip: &mut zip::ZipArchive<Cursor<&[u8]>>,
    total: &mut u64,
    images: &mut ReadResources,
    master_flags: Option<&Props>,
    notes_context: Option<usize>,
    notes_master_source: Option<&El>,
) -> Result<()> {
    for e in parent.elements() {
        if e.name == "draw:layer-set" {
            continue;
        }
        if e.name == "draw:a" {
            let start = shapes.len();
            read_shapes(e, styles, p, shapes, zip, total, images, master_flags, notes_context, notes_master_source)?;
            if let Some(url) = e.attr("xlink:href") {
                for shape in shapes.iter_mut().skip(start) {
                    if shape.click.is_none() {
                        *total = total.saturating_add(url.len() as u64);
                        if *total > crate::MAX_TOTAL {
                            return Err(OdpError("expanded shape links exceed size limit".into()));
                        }
                        shape.click = Some(text_reader::hyperlink(url, e.attr("office:title").unwrap_or("")));
                    }
                }
            }
            continue;
        }
        if let Some(flags) = master_flags {
            let class = e.attr("presentation:class").unwrap_or("");
            if e.attr("presentation:placeholder") == Some("true") && !matches!(class, "date-time" | "footer" | "header" | "page-number") {
                continue;
            }
            let flag = match class {
                "date-time" => Some("presentation:display-date-time"),
                "footer" => Some("presentation:display-footer"),
                "header" => Some("presentation:display-header"),
                "page-number" => Some("presentation:display-page-number"),
                _ => None,
            };
            if flag.is_some_and(|key| flags.get(key).is_none_or(|v| v != "true")) {
                continue;
            }
        }
        if e.name == "draw:g" {
            let mut children = Vec::new();
            read_shapes(e, styles, p, &mut children, zip, total, images, master_flags, notes_context, notes_master_source)?;
            let bounds =
                children.iter().filter_map(|s| s.xfrm).map(|x| x.bounds()).reduce(|a, b| a.union(b)).unwrap_or(Rect::new(0.0, 0.0, 1.0, 1.0));
            let child = Xfrm::new(bounds.x0, bounds.y0, bounds.width().max(0.001), bounds.height().max(0.001));
            let matrix = e.attr("draw:transform").map(crate::geometry::transform).transpose()?.unwrap_or(deckcraft_geom::Affine::IDENTITY);
            let xfrm = crate::geometry::decompose(matrix * deckcraft_geom::Affine::translate((child.x, child.y)), child.w, child.h)?;
            *total = total.saturating_add(std::mem::size_of::<Shape>() as u64);
            if *total > crate::MAX_TOTAL {
                return Err(OdpError("expanded group structure exceeds total size limit".into()));
            }
            shapes.push(Shape {
                hidden: e
                    .attr("draw:layer")
                    .and_then(|name| styles.get(&("layer".into(), name.into())))
                    .is_some_and(|layer| layer.attr("draw:display") == Some("none")),
                id: ShapeId(id(p)?),
                tags: source_tags(e, total)?,
                name: e.attr("draw:name").unwrap_or("").into(),
                xfrm: Some(xfrm),
                kind: ShapeKind::Group { children, child },
                ..Default::default()
            });
            continue;
        }
        if !e.name.starts_with("draw:") {
            if e.name.starts_with("unknown:") {
                let mut shape = extras::opaque(e, p, total)?;
                shape.tags = source_tags(e, total)?;
                shapes.push(shape);
            }
            continue;
        }
        let notes_thumbnail = notes_context.is_some() && e.name == "draw:page-thumbnail";
        if notes_thumbnail && let Some(number) = e.attr("draw:page-number") {
            let ordinal = number.parse::<usize>().map_err(|_| OdpError("invalid notes thumbnail page number".into()))?;
            if ordinal == 0 || notes_context.is_some_and(|owner| owner != 0 && owner != ordinal) {
                return Err(OdpError("notes thumbnail must reference its owning drawing page".into()));
            }
        }
        if !notes_thumbnail
            && !matches!(
                e.name.as_str(),
                "draw:rect"
                    | "draw:ellipse"
                    | "draw:circle"
                    | "draw:line"
                    | "draw:connector"
                    | "draw:frame"
                    | "draw:custom-shape"
                    | "draw:path"
                    | "draw:polygon"
                    | "draw:polyline"
            )
        {
            let mut shape = extras::opaque(e, p, total)?;
            shape.tags = source_tags(e, total)?;
            shape.xfrm = Some(Xfrm::new(
                e.attr("svg:x").and_then(length).unwrap_or(0.0),
                e.attr("svg:y").and_then(length).unwrap_or(0.0),
                e.attr("svg:width").and_then(length).unwrap_or(1.0),
                e.attr("svg:height").and_then(length).unwrap_or(1.0),
            ));
            shapes.push(shape);
            continue;
        }
        let mut props = properties(styles, "presentation", e.attr("presentation:style-name"))?;
        let direct_props = properties(styles, "graphic", e.attr("draw:style-name"))?;
        let inherited_notes_paint = !direct_props.keys().any(|key| key.starts_with("draw:fill") || key == "draw:opacity")
            && e.attr("presentation:style-name").is_some_and(|name| {
                notes_master_source.is_some_and(|master| {
                    master.elements().any(|parent| {
                        parent.attr("presentation:class") == e.attr("presentation:class")
                            && e.attr("presentation:class").is_some()
                            && parent.attr("presentation:style-name") == Some(name)
                    })
                })
            });
        props.extend(direct_props);
        let dim = |key| e.attr(key).and_then(length).unwrap_or(0.0);
        let mut xfrm = Xfrm::new(dim("svg:x"), dim("svg:y"), dim("svg:width"), dim("svg:height"));
        if matches!(e.name.as_str(), "draw:line" | "draw:connector") {
            let (x1, y1, x2, y2) = (dim("svg:x1"), dim("svg:y1"), dim("svg:x2"), dim("svg:y2"));
            xfrm = Xfrm::new(x1.min(x2), y1.min(y2), (x2 - x1).abs(), (y2 - y1).abs());
            xfrm.flip_h = x2 < x1;
            xfrm.flip_v = y2 < y1;
        }
        if !xfrm.is_finite() || xfrm.w < 0.0 || xfrm.h < 0.0 || xfrm.w > 1.0e7 || xfrm.h > 1.0e7 {
            return Err(OdpError("invalid shape dimensions".into()));
        }
        if let Some(transform) = e.attr("draw:transform") {
            xfrm = crate::geometry::decompose(crate::geometry::transform(transform)? * xfrm.affine(), xfrm.w, xfrm.h)?;
        }
        if let Some(g) = e.child("draw:enhanced-geometry") {
            xfrm.flip_h ^= g.attr("draw:mirror-horizontal") == Some("true");
            xfrm.flip_v ^= g.attr("draw:mirror-vertical") == Some("true");
        }
        let geom = match if notes_thumbnail {
            Ok(Geom::preset("rect"))
        } else {
            crate::geometry::read_with_style(
                e,
                xfrm.w,
                xfrm.h,
                props.get("draw:fill").is_some_and(|v| v != "none"),
                props.get("draw:stroke").is_some_and(|v| v != "none"),
            )
        } {
            Ok(geom) => geom,
            Err(error) => {
                let mut shape = extras::opaque(e, p, total)?;
                shape.tags = source_tags(e, total)?;
                shape.xfrm = Some(xfrm);
                extras::warn(p, &format!("{}; object retained for source recovery", error.0));
                shapes.push(shape);
                continue;
            }
        };
        let mut shape = Shape {
            id: ShapeId(id(p)?),
            tags: source_tags(e, total)?,
            name: e.attr("draw:name").unwrap_or("").into(),
            xfrm: Some(xfrm),
            geom,
            fill: resources::resolve_fill(&props, p, zip, total, images, xfrm.w, xfrm.h)?.or(Some(Fill::None)),
            line: Some(Line::none()),
            ..Default::default()
        };
        if inherited_notes_paint {
            shape.fill = None;
        }
        if e.name == "draw:connector" {
            shape.kind = ShapeKind::Connector { start: None, end: None };
        }
        if let Some(class) = e.attr("presentation:class") {
            shape.ph = match class {
                "title" => Some(Placeholder { kind: PhType::Title, ..Default::default() }),
                "subtitle" => Some(Placeholder { kind: PhType::SubTitle, ..Default::default() }),
                "notes" if notes_context.is_some() => Some(Placeholder { kind: PhType::Body, ..Default::default() }),
                "header" if notes_context.is_some() => Some(Placeholder { kind: PhType::Header, ..Default::default() }),
                "footer" if notes_context.is_some() => Some(Placeholder { kind: PhType::Footer, ..Default::default() }),
                "date-time" if notes_context.is_some() => Some(Placeholder { kind: PhType::Date, ..Default::default() }),
                "page-number" if notes_context.is_some() => Some(Placeholder { kind: PhType::SlideNum, ..Default::default() }),
                _ => None,
            };
        }
        if notes_thumbnail {
            shape.ph = Some(Placeholder { kind: PhType::SlideImage, ..Default::default() });
        }
        shape.line = Some(resources::outline(&props, styles)?);
        shape.effects = resources::effects(&props);
        if notes_context.is_some_and(|page| page != 0) {
            match e.attr("presentation:user-transformed") {
                Some("false") if shape.ph.is_some() => shape.xfrm = None,
                Some("true" | "false") | None => {}
                Some(_) => return Err(OdpError("invalid notes placeholder transformation flag".into())),
            }
        }
        if props.contains_key("draw:marker-start") || props.contains_key("draw:marker-end") {
            extras::warn(p, "Custom ODF line markers are preserved in the source archive; marker rendering is incomplete");
        }
        if e.name == "draw:frame" {
            if let Some(table) = e.child("table:table") {
                let formula_settings_allowed =
                    !p.tags.iter().any(|(key, value)| key == "odp:warning" && value == crate::source_effects::CALCULATION_WARNING);
                let table = crate::table::read(table, styles, xfrm.w, total, formula_settings_allowed, &mut images.formulas)?;
                for message in table.warnings {
                    extras::warn(p, message);
                }
                shape.kind = ShapeKind::Table(table.table);
            } else if let Some(object) = e.child("draw:object") {
                if let Some(href) = object.attr("xlink:href") {
                    let object_path = resources::path(href)?;
                    let name = format!("{object_path}/content.xml");
                    if zip.index_for_name(&name).is_some() {
                        let doc = parse_part(&part(zip, &name, total)?, total, xml::SourcePart::Embedded)?;
                        audit_xml(p, AuditPart::Embedded, &doc, total)?;
                        if doc.foreign_content() {
                            extras::warn(
                                p,
                                "Unmapped embedded-object XML is retained in the original ODP; editing and export may lose its semantics",
                            );
                        }
                        for message in chart::warnings(&doc, styles)? {
                            extras::warn(p, message);
                        }
                        if let Some(native) = chart::read(&doc, styles, total)? {
                            shape.kind = ShapeKind::Chart(Box::new(native));
                        } else {
                            shape.kind = extras::opaque(object, p, total)?.kind;
                        }
                    } else {
                        shape.kind = extras::opaque(object, p, total)?.kind;
                    }
                } else {
                    for message in chart::warnings(object, styles)? {
                        extras::warn(p, message);
                    }
                    if let Some(native) = chart::read(object, styles, total)? {
                        shape.kind = ShapeKind::Chart(Box::new(native));
                    } else {
                        shape.kind = extras::opaque(object, p, total)?.kind;
                    }
                }
            } else if let Some(plugin) = e.child("draw:plugin") {
                let href = plugin.attr("xlink:href").ok_or_else(|| OdpError("media object has no package path".into()))?;
                let media = resources::media(href, plugin.attr("draw:mime-type"), p, zip, total, images)?;
                let video = p.media.iter().find(|m| m.id == media).is_some_and(|m| m.content_type.starts_with("video/"));
                let param = |key: &str| {
                    plugin.elements().find(|e| e.name == "draw:param" && e.attr("draw:name") == Some(key)).and_then(|e| e.attr("draw:value"))
                };
                shape.kind = ShapeKind::Media(MediaClip {
                    media,
                    video,
                    volume: 1.0,
                    autoplay: param("autoplay") == Some("true"),
                    loop_play: param("loop") == Some("true"),
                    ..Default::default()
                });
            } else if let Some(image) = e.child("draw:image") {
                let href = image.attr("xlink:href").ok_or_else(|| OdpError("image has no package path".into()))?;
                let media = resources::media(href, image.attr("draw:mime-type"), p, zip, total, images)?;
                shape.kind = ShapeKind::Picture { fill: resources::picture(media, &props, p) };
            } else if e.child("draw:text-box").is_some() {
                shape.text_box = true;
            } else if notes_context.is_none() || shape.ph.is_none() {
                shape.kind = extras::opaque(e, p, total)?.kind;
            }
        }
        if props.get("draw:fill").is_some_and(|v| v == "hatch") {
            extras::warn(p, "Hatch spacing/angle approximated by an editable pattern; source archive retained");
        }
        shape.descr = e.child("svg:desc").map(extras::plain).unwrap_or_default();
        shape.title = e.child("svg:title").map(extras::plain).unwrap_or_default();
        shape.hidden = e.attr("presentation:visibility") == Some("hidden")
            || e.attr("draw:layer")
                .and_then(|name| styles.get(&("layer".into(), name.into())))
                .is_some_and(|layer| layer.attr("draw:display") == Some("none"));
        for message in text_reader::warnings(e, styles, &props) {
            extras::warn(p, message);
        }
        let mut body = TextBody { body: text_reader::body_props(&props, e, styles), ..Default::default() };
        let defaults = text_reader::shape_defaults(e, &props, styles)?;
        text_reader::text_with(e, styles, &mut body.paragraphs, 0, &defaults.def_props, &defaults.props)?;
        if notes_context.is_none() && comments_reader::holder(e, styles).is_some() {
            continue;
        }
        let text_bytes: usize = body.paragraphs.iter().flat_map(|p| &p.runs).map(|r| r.text.len()).sum();
        let text_structures = body.paragraphs.iter().fold(0usize, |sum, p| {
            sum.saturating_add(paragraph_weight(p)).saturating_add(p.runs.iter().fold(0usize, |sum, r| sum.saturating_add(run_weight(&r.props))))
        });
        let geom_bytes = match &shape.geom {
            Geom::Custom { paths } => paths.iter().map(|p| p.d.len()).sum::<usize>(),
            _ => 0,
        };
        *total = total
            .saturating_add(text_bytes as u64)
            .saturating_add(text_structures as u64)
            .saturating_add(geom_bytes as u64)
            .saturating_add(std::mem::size_of::<Shape>() as u64);
        if *total > crate::MAX_TOTAL {
            return Err(OdpError("expanded content exceeds size limit".into()));
        }
        if !body.paragraphs.is_empty() {
            shape.text = Some(body);
        }
        if notes_context.is_some_and(|page| page != 0) && notes_reader::restore_style_ownership(e, styles, notes_master_source, &mut shape, total)? {
            const WARNING: &str = "ODF notes local line-height modifiers depend on shared Normal; editable ownership is incomplete; original source retained and edited export blocked";
            extras::warn(p, WARNING);
            if !p.tags.iter().any(|(key, value)| key == "odp:warning" && value == WARNING) {
                return Err(OdpError("notes line-height dependency warning exceeds retained warning limit".into()));
            }
        }
        shapes.push(shape);
    }
    Ok(())
}

fn text_charge(budget: &mut usize, bytes: usize) -> Result<()> {
    *budget = budget.saturating_add(bytes);
    if *budget > 32 * 1024 * 1024 {
        return Err(OdpError("expanded text/style allocation exceeds size limit".into()));
    }
    Ok(())
}
pub(crate) fn run_weight(props: &RunProps) -> usize {
    let strings = [&props.font, &props.font_ea, &props.font_cs, &props.font_sym, &props.lang, &props.lang_ea, &props.lang_cs, &props.underline]
        .into_iter()
        .flatten()
        .fold(0usize, |sum, s| sum.saturating_add(s.len()));
    let link = props
        .link
        .as_ref()
        .map(|link| {
            link.tooltip.len().saturating_add(match &link.action {
                deckcraft_model::text::Action::Url { url } => url.len(),
                _ => 0,
            })
        })
        .unwrap_or(0);
    std::mem::size_of::<Run>().saturating_add(strings).saturating_add(link)
}

fn source_tags(el: &El, total: &mut u64) -> Result<Vec<(String, String)>> {
    let mut tags: Vec<(String, String)> = el
        .attr("draw:id")
        .or_else(|| el.attr("xml:id"))
        .filter(|value| !value.is_empty() && value.len() <= 8192)
        .map(|value| vec![("odp:id".into(), value.into())])
        .unwrap_or_default();
    if let Some((part, range)) = el.source_provenance() {
        tags.push(("odp:source-part".into(), part.label().into()));
        tags.push(("odp:source-range".into(), format!("v1:{}:{}", range.start, range.end)));
    }
    let cost = tags.iter().fold(0usize, |cost, (key, value)| {
        cost.saturating_add(std::mem::size_of::<(String, String)>()).saturating_add(key.len()).saturating_add(value.len())
    });
    *total = total.saturating_add(cost as u64);
    if *total > crate::MAX_TOTAL {
        return Err(OdpError("source provenance exceeds total import budget".into()));
    }
    Ok(tags)
}
#[allow(clippy::too_many_arguments)]
pub(super) fn timing_media(
    href: &str,
    hint: Option<&str>,
    p: &mut Presentation,
    zip: &mut zip::ZipArchive<Cursor<&[u8]>>,
    total: &mut u64,
    cache: &mut BTreeMap<String, MediaId>,
) -> Result<MediaId> {
    resources::media(href, hint, p, zip, total, cache)
}

pub(crate) fn paragraph_weight(p: &Paragraph) -> usize {
    text_reader::paragraph_weight(p)
}

/// Actual production rich-text adapter used by annotations and their regeneration.
pub(crate) fn comment_styles<'a>(root: &'a El, styles: &mut Styles<'a>) -> Result<()> {
    collect_styles(root, styles, &mut 0)
}
pub(crate) fn comment_body(el: &El, styles: &Styles<'_>) -> Result<crate::comments_codec::DecodedBody> {
    fn unbound_style(el: &El, styles: &Styles<'_>, depth: usize) -> bool {
        if depth > 64 {
            return true;
        }
        let family = match el.name.as_str() {
            "text:p" | "text:h" => Some("paragraph"),
            "text:span" | "text:a" => Some("text"),
            "text:list" => Some("list"),
            _ => None,
        };
        if let (Some(family), Some(name)) = (family, el.attr("text:style-name"))
            && required_properties(styles, family, Some(name)).is_err()
        {
            return true;
        }
        el.elements().any(|child| unbound_style(child, styles, depth + 1))
    }
    let mut body = TextBody::default();
    text(el, styles, &mut body.paragraphs, 0)?;
    let unmapped = unbound_style(el, styles, 0) || !text_reader::warnings(el, styles, &Props::new()).is_empty();
    Ok(crate::comments_codec::DecodedBody { body, unmapped })
}
