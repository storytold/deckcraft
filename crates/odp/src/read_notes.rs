//! Editable notes pages, shared notes masters and live owning-slide placeholders.
use super::*;
use deckcraft_model::notes::{NotesDateTime, NotesFields, NotesMaster, NotesPage};
use std::collections::BTreeSet;

fn source_role<'a>(root: &'a El, class: &str, shared: &BTreeSet<&str>, found: &mut Option<&'a El>, total: &mut u64) -> Result<()> {
    // Source identities are resolved repeatedly across notes pages. Charge work
    // as well as allocations so a huge shared master cannot cause unbounded rescans.
    charge(total, 64)?;
    if root.attr("presentation:class") == Some(class)
        && root.attr("presentation:style-name").is_some_and(|name| shared.contains(name))
        && found.replace(root).is_some()
    {
        return Err(OdpError("ambiguous shared notes role style".into()));
    }
    for child in root.elements() {
        source_role(child, class, shared, found, total)?;
    }
    Ok(())
}
fn first_paragraph(root: &El) -> Option<&El> {
    if root.name == "text:p" {
        return Some(root);
    }
    root.elements().find_map(first_paragraph)
}
fn chain<'a>(styles: &'a Styles<'a>, family: &str, name: Option<&'a str>, stop: Option<&str>) -> Result<Vec<&'a El>> {
    let mut out = Vec::new();
    let mut current = name;
    while let Some(name) = current {
        if stop == Some(name) {
            break;
        }
        if out.len() >= 32 {
            return Err(OdpError("excessive notes style inheritance".into()));
        }
        let Some(style) = styles.get(&(family.into(), name.into())).copied() else { break };
        out.push(style);
        current = style.attr("style:parent-style-name");
    }
    Ok(out)
}
fn keys<'a>(styles: &'a Styles<'a>, family: &str, name: Option<&'a str>, stop: Option<&str>) -> Result<BTreeSet<&'a str>> {
    let mut keys = BTreeSet::new();
    for style in chain(styles, family, name, stop)? {
        for bag in style.elements() {
            keys.extend(bag.attrs.keys().map(String::as_str));
            for child in bag.elements() {
                keys.insert(child.name.as_str());
                keys.extend(child.attrs.keys().map(String::as_str));
            }
        }
    }
    Ok(keys)
}
fn local_run(styles: &Styles<'_>, name: Option<&str>, common: &str) -> Result<RunProps> {
    let mut props = Props::new();
    for style in chain(styles, "paragraph", name, Some(common))?.into_iter().rev() {
        if let Some(text) = style.child("style:text-properties") {
            props.extend(text.attrs.clone());
        }
    }
    for (key, target) in [
        ("style:font-name", "fo:font-family"),
        ("style:font-name-asian", "style:font-family-asian"),
        ("style:font-name-complex", "style:font-family-complex"),
    ] {
        if let Some(family) = props.get(key).and_then(|name| styles.get(&("font".into(), name.clone()))).and_then(|font| font.attr("svg:font-family"))
        {
            props.insert(target.into(), family.into());
        }
    }
    Ok(text_reader::run_props(&props))
}
fn collect_local_styles<'a>(el: &'a El, styles: &'a Styles<'a>, common: &str, subset: &mut Styles<'a>) -> Result<()> {
    if let Some(name) = el.attr("text:style-name") {
        let family = match el.name.as_str() {
            "text:p" | "text:h" => "paragraph",
            "text:list" => "list",
            _ => "text",
        };
        for style in chain(styles, family, Some(name), Some(common))? {
            if subset.len() >= 4096 {
                return Err(OdpError("notes local style index exceeds limit".into()));
            }
            if let Some(name) = style.attr("style:name") {
                subset.insert((family.into(), name.into()), style);
            }
            for bag in style.elements() {
                for key in ["style:font-name", "style:font-name-asian", "style:font-name-complex"] {
                    if let Some(name) = bag.attr(key)
                        && let Some(font) = styles.get(&("font".into(), name.into()))
                    {
                        subset.insert(("font".into(), name.into()), font);
                    }
                }
            }
        }
    }
    for child in el.elements() {
        collect_local_styles(child, styles, common, subset)?;
    }
    Ok(())
}
fn paragraph_sources<'a>(el: &'a El, out: &mut Vec<&'a El>) {
    for child in el.elements() {
        if child.name == "text:p" {
            out.push(child);
        } else {
            paragraph_sources(child, out);
        }
    }
}

/// Shared role/style identity proves ownership. Direct presence determines the
/// local overrides; a local value equal to the master is never removed by equality.
pub(super) fn restore_style_ownership(el: &El, styles: &Styles<'_>, master: Option<&El>, shape: &mut Shape, total: &mut u64) -> Result<bool> {
    let (Some(master), Some(class), Some(shared)) = (master, el.attr("presentation:class"), el.attr("presentation:style-name")) else {
        return Ok(false);
    };
    let mut parent = None;
    let mut shared_names = BTreeSet::from([shared]);
    for style in chain(styles, "presentation", Some(shared), None)? {
        if let Some(name) = style.attr("style:name") {
            shared_names.insert(name);
        }
    }
    source_role(master, class, &shared_names, &mut parent, total)?;
    let Some(parent) = parent else { return Ok(false) };
    let mut direct = keys(styles, "presentation", Some(shared), parent.attr("presentation:style-name"))?;
    direct.extend(keys(styles, "graphic", el.attr("draw:style-name"), None)?);
    if !direct.iter().any(|key| key.starts_with("draw:fill") || *key == "draw:opacity") {
        shape.fill = None;
    }
    if !direct.iter().any(|key| key.starts_with("draw:stroke") || key.starts_with("svg:stroke") || key.starts_with("draw:marker")) {
        shape.line = None;
    }
    if !direct.iter().any(|key| key.starts_with("draw:shadow")) {
        shape.effects = None;
    } else if shape.effects.is_none() {
        shape.effects = Some(Effects::default());
    }
    let Some(body) = shape.text.as_mut() else { return Ok(false) };
    macro_rules! clear { ($field:ident,$($key:literal),+) => {if ![$($key),+].iter().any(|key|direct.contains(key)) {body.body.$field=None;}}; }
    clear!(inset_l, "fo:padding-left", "fo:padding");
    clear!(inset_r, "fo:padding-right", "fo:padding");
    clear!(inset_t, "fo:padding-top", "fo:padding");
    clear!(inset_b, "fo:padding-bottom", "fo:padding");
    clear!(anchor, "draw:textarea-vertical-align");
    clear!(anchor_ctr, "draw:textarea-horizontal-align");
    clear!(wrap, "fo:wrap-option");
    clear!(autofit, "draw:auto-grow-height", "draw:fit-to-size", "style:shrink-to-fit");
    clear!(vert, "style:writing-mode");
    clear!(rtl_col, "style:writing-mode");
    clear!(columns, "fo:column-count");
    clear!(col_spacing, "fo:column-gap");
    let Some(common) = parent.attr("draw:text-style-name").or_else(|| first_paragraph(parent).and_then(|p| p.attr("text:style-name"))) else {
        return Ok(false);
    };
    let mut source = Vec::new();
    paragraph_sources(el, &mut source);
    // Only the explicit shared paragraph ancestry licenses removing text defaults.
    if source.iter().any(|p| {
        let mut name = p.attr("text:style-name");
        for _ in 0..32 {
            if name == Some(common) {
                return false;
            }
            name = name.and_then(|name| styles.get(&("paragraph".into(), name.into()))).and_then(|style| style.attr("style:parent-style-name"));
        }
        true
    }) {
        return Ok(false);
    }
    let mut subset = Styles::new();
    collect_local_styles(el, styles, common, &mut subset)?;
    let mut shadows = String::new();
    for ((family, _), style) in &subset {
        if family == "paragraph" {
            let size = crate::notes_styles::serialized_size(style);
            if shadows.len().saturating_add(size) > crate::MAX_PART as usize {
                return Err(OdpError("notes local style XML exceeds part limit".into()));
            }
            charge(total, size)?;
            shadows.push_str(&crate::notes_styles::fragment(style));
        }
    }
    charge(total, subset.len().saturating_mul(128))?;
    let mut shadow = crate::notes_styles::document(&shadows)?;
    charge(total, shadow.identity_metadata_bytes())?;
    for node in &mut shadow.children {
        if let Node::El(style) = node {
            style.children.retain(|node| !matches!(node,Node::El(el) if el.name=="style:text-properties"));
        }
    }
    let mut raw_styles = Styles::new();
    for (key, style) in &subset {
        if key.0 != "paragraph" {
            raw_styles.insert(key.clone(), *style);
        }
    }
    for style in shadow.elements() {
        if let Some(name) = style.attr("style:name") {
            raw_styles.insert(("paragraph".into(), name.into()), style);
        }
    }
    let mut paragraphs = Vec::new();
    text_reader::text(el, &raw_styles, &mut paragraphs, 0)?;
    if paragraphs.len() != source.len() {
        return Err(OdpError("notes paragraph ownership count changed".into()));
    }
    let mut source_dependent = false;
    for (paragraph, el) in paragraphs.iter_mut().zip(source) {
        if crate::loss_report::notes_line_height_depends_on_shared_normal(styles, el.attr("text:style-name"), common, |bytes| charge(total, bytes))? {
            // Shared Normal owns the current preview. The dormant local modifier is
            // retained only in the original source; the caller must publish its guard.
            paragraph.props.line_height = None;
            paragraph.props.line_spacing = None;
            source_dependent = true;
        }
        paragraph.def_props = local_run(styles, el.attr("text:style-name"), common)?;
    }
    charge(total, paragraphs.iter().map(paragraph_weight).sum())?;
    body.paragraphs = paragraphs;
    Ok(source_dependent)
}

fn charge(total: &mut u64, bytes: usize) -> Result<()> {
    *total = total.saturating_add(bytes as u64);
    if *total > crate::MAX_TOTAL {
        return Err(OdpError("expanded notes layout exceeds total size limit".into()));
    }
    Ok(())
}

fn boolean(value: &str) -> Result<bool> {
    match value {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        _ => Err(OdpError("invalid notes-page boolean".into())),
    }
}

fn size(el: &El, layouts: &BTreeMap<String, &El>, p: &mut Presentation) -> Result<Option<Size>> {
    let Some(name) = el.attr("style:page-layout-name") else { return Ok(None) };
    let layout = layouts.get(name).ok_or_else(|| OdpError("missing notes page layout".into()))?;
    let props = layout.child("style:page-layout-properties").ok_or_else(|| OdpError("missing notes page dimensions".into()))?;
    let width = props.attr("fo:page-width").and_then(length).ok_or_else(|| OdpError("invalid notes page width".into()))?;
    let height = props.attr("fo:page-height").and_then(length).ok_or_else(|| OdpError("invalid notes page height".into()))?;
    if width <= 0.0 || height <= 0.0 || width > 5000.0 || height > 5000.0 {
        return Err(OdpError("notes page dimensions exceed limits".into()));
    }
    if props.attrs.iter().any(|(key, value)| {
        (key == "fo:margin" || key.starts_with("fo:margin-")) && length(value) != Some(0.0)
            || key.starts_with("fo:padding") && length(value) != Some(0.0)
            || key.starts_with("fo:border") && value != "none"
    }) || props.elements().next().is_some()
    {
        extras::warn(
            p,
            "ODF notes page margins, borders or additional layout regions are retained in the source archive; editable page geometry is incomplete",
        );
    }
    Ok(Some(Size::new(width, height)))
}

fn declaration<'a>(body: &'a El, name: &str, tag: &str, total: &mut u64) -> Result<&'a El> {
    let mut value = None;
    for child in body.elements() {
        charge(total, 64)?;
        if child.name == tag && child.attr("presentation:name") == Some(name) && value.replace(child).is_some() {
            return Err(OdpError("ambiguous notes field declaration".into()));
        }
    }
    let value = value.ok_or_else(|| OdpError("missing notes field declaration".into()))?;
    if value.elements().next().is_some() {
        return Err(OdpError("ambiguous or structured notes field declaration".into()));
    }
    Ok(value)
}

fn fields(el: &El, props: &Props, body: &El, p: &mut Presentation, total: &mut u64) -> Result<NotesFields> {
    let mut fields = NotesFields::default();
    for (attribute, tag, destination) in [
        ("presentation:use-header-name", "presentation:header-decl", &mut fields.header),
        ("presentation:use-footer-name", "presentation:footer-decl", &mut fields.footer),
    ] {
        if let Some(name) = el.attr(attribute) {
            let value = extras::plain(declaration(body, name, tag, total)?);
            charge(total, value.len())?;
            *destination = Some(value);
        }
    }
    if let Some(name) = el.attr("presentation:use-date-time-name") {
        let value = declaration(body, name, "presentation:date-time-decl", total)?;
        fields.date_time = Some(match value.attr("presentation:source").unwrap_or("fixed") {
            "fixed" => {
                let text = extras::plain(value);
                charge(total, text.len())?;
                NotesDateTime::Fixed { text }
            }
            "current-date" => {
                let format = if let Some(name) = value.attr("style:data-style-name") {
                    extras::warn(
                        p,
                        "ODF notes automatic-date formats are retained in the source archive; the named date-style mapping is incomplete",
                    );
                    format!("odp-unmapped:{name}")
                } else {
                    "datetime1".into()
                };
                charge(total, format.len())?;
                NotesDateTime::Current { format, lang: None }
            }
            _ => return Err(OdpError("invalid notes date declaration source".into())),
        });
    }
    for (key, destination) in [
        ("presentation:display-header", &mut fields.display_header),
        ("presentation:display-footer", &mut fields.display_footer),
        ("presentation:display-date-time", &mut fields.display_date_time),
        ("presentation:display-page-number", &mut fields.display_page_number),
    ] {
        *destination = props.get(key).map(|value| boolean(value)).transpose()?;
    }
    Ok(fields)
}

fn warnings(el: &El, p: &mut Presentation) {
    if el.child("office:forms").is_some_and(|forms| forms.elements().next().is_some()) {
        extras::warn(p, "ODF notes forms are retained in the source archive; editable form controls are not supported");
    }
    if el.attr("draw:z-index").is_some() {
        extras::warn(
            p,
            "ODF explicit notes z-index values are retained in the source archive; editable notes currently preserve document drawing order",
        );
    }
    for child in el.elements() {
        warnings(child, p);
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn masters(
    sources: &BTreeMap<String, &El>,
    layouts: &BTreeMap<String, &El>,
    styles: &Styles<'_>,
    default_layers: Option<&El>,
    body: &El,
    p: &mut Presentation,
    zip: &mut zip::ZipArchive<Cursor<&[u8]>>,
    total: &mut u64,
    images: &mut ReadResources,
) -> Result<BTreeMap<String, MasterId>> {
    let mut ids = BTreeMap::new();
    for (name, source) in sources {
        let Some(notes) = source.child("presentation:notes") else { continue };
        if p.notes_masters.len() >= 10_000 {
            return Err(OdpError("too many notes masters".into()));
        }
        warnings(notes, p);
        let size = size(notes, layouts, p)?.unwrap_or(p.notes_size);
        let props = required_properties(styles, "drawing-page", notes.attr("draw:style-name"))?;
        let fields = fields(notes, &props, body, p, total)?;
        let mut master = Master {
            id: MasterId(id(p)?),
            name: name.clone(),
            theme: p.masters.first().map(|master| master.theme.clone()).unwrap_or_default(),
            ..Default::default()
        };
        master.background = if props.get("presentation:background-visible").map(|value| boolean(value)).transpose()? == Some(false) {
            Some(Background::Fill { fill: Fill::None })
        } else {
            resources::resolve_fill(&props, p, zip, total, images, size.width, size.height)?.map(|fill| Background::Fill { fill })
        };
        let scoped = owner_styles(styles, notes, default_layers, total)?;
        read_shapes(notes, &scoped, p, &mut master.shapes, zip, total, images, None, Some(0), None)?;
        for message in text_reader::warnings(notes, styles, &props) {
            extras::warn(p, message);
        }
        charge(total, std::mem::size_of::<NotesMaster>().saturating_add(name.len()))?;
        ids.insert(name.clone(), master.id);
        p.notes_masters.push(NotesMaster { master: Arc::new(master), size, fields });
    }
    Ok(ids)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn page(
    el: &El,
    master: Option<MasterId>,
    master_source: Option<&El>,
    layouts: &BTreeMap<String, &El>,
    styles: &Styles<'_>,
    default_layers: Option<&El>,
    body: &El,
    p: &mut Presentation,
    zip: &mut zip::ZipArchive<Cursor<&[u8]>>,
    total: &mut u64,
    images: &mut ReadResources,
    owning_page: usize,
) -> Result<(NotesPage, TextBody)> {
    if owning_page == 0 || owning_page > 10_000 {
        return Err(OdpError("invalid notes owning page number".into()));
    }
    warnings(el, p);
    let size = size(el, layouts, p)?;
    let effective_size = size
        .or_else(|| master.and_then(|id| p.notes_masters.iter().find(|master| master.master.id == id).map(|master| master.size)))
        .unwrap_or(p.notes_size);
    let props = required_properties(styles, "drawing-page", el.attr("draw:style-name"))?;
    let fields = fields(el, &props, body, p, total)?;
    let background = if props.get("presentation:background-visible").map(|value| boolean(value)).transpose()? == Some(false) {
        Some(Background::Fill { fill: Fill::None })
    } else {
        resources::resolve_fill(&props, p, zip, total, images, effective_size.width, effective_size.height)?.map(|fill| Background::Fill { fill })
    };
    let show_master_shapes = props.get("presentation:background-objects-visible").map(|value| boolean(value)).transpose()?.unwrap_or(true);
    let mut shapes = Vec::new();
    let scoped = owner_styles(styles, el, default_layers, total)?;
    read_shapes(el, &scoped, p, &mut shapes, zip, total, images, None, Some(owning_page), master_source)?;
    for message in text_reader::warnings(el, styles, &props) {
        extras::warn(p, message);
    }
    let mut candidates = Vec::new();
    deckcraft_model::walk(&shapes, &mut |shape, _| {
        if shape.ph_type() == Some(PhType::Body) {
            candidates.push(shape.id);
        }
    });
    if candidates.len() > 1 {
        return Err(OdpError("multiple primary notes body frames cannot share one canonical notes body".into()));
    }
    if candidates.is_empty() {
        // Some producers use a regular text frame for the sole speaker-notes body.
        // A unique frame is unambiguous; independent labels are never concatenated.
        let mut unclassified = Vec::new();
        deckcraft_model::walk(&shapes, &mut |shape, _| {
            if shape.ph.is_none() && shape.text_box && shape.text.is_some() {
                unclassified.push(shape.id);
            }
        });
        if unclassified.len() == 1 {
            candidates.extend(unclassified);
            if let Some(shape) = candidates.first().and_then(|id| find_shape_mut(&mut shapes, *id)) {
                shape.ph = Some(Placeholder { kind: PhType::Body, ..Default::default() });
            }
        } else if unclassified.len() > 1 {
            extras::warn(
                p,
                "ODF notes have multiple unclassified text frames; all frames remain editable but the canonical speaker-notes body is ambiguous",
            );
        }
    }
    let body_shape = candidates.first().copied();
    let canonical = body_shape.and_then(|id| find_shape_mut(&mut shapes, id)).and_then(|shape| shape.text.take()).unwrap_or_default();
    charge(total, std::mem::size_of::<NotesPage>())?;
    Ok((NotesPage { master, size, shapes, body_shape, background, show_master_shapes, fields }, canonical))
}
