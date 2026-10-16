//! First regular-master slice: shared artwork and page-paint ownership, never text matching.
use super::*;
use std::collections::BTreeSet;

const LIMIT: usize = 10_000;
const UNSUPPORTED: &str =
    "ODF regular master placeholders or inherited visibility require additional editable ownership; source retained and edited export blocked";

fn charge(total: &mut u64, amount: usize) -> Result<()> {
    *total = total.saturating_add(amount as u64);
    if *total > crate::MAX_TOTAL {
        return Err(OdpError("regular master inventory exceeds total import budget".into()));
    }
    Ok(())
}
fn boolean(value: &str) -> Result<bool> {
    match value {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        _ => Err(OdpError("invalid regular master visibility flag".into())),
    }
}
fn flags(props: &Props) -> Result<()> {
    for key in [
        "presentation:background-visible",
        "presentation:background-objects-visible",
        "presentation:display-date-time",
        "presentation:display-footer",
        "presentation:display-header",
        "presentation:display-page-number",
    ] {
        props.get(key).map(|value| boolean(value)).transpose()?;
    }
    if props.get("presentation:visibility").is_some_and(|value| !matches!(value.as_str(), "visible" | "hidden")) {
        return Err(OdpError("invalid regular page visibility".into()));
    }
    Ok(())
}
type CommonStyles = BTreeSet<(&'static str, usize, usize)>;
fn source_key(el: &El) -> Option<(&'static str, usize, usize)> {
    el.source_provenance().map(|(part, range)| (part.label(), range.start, range.end))
}
fn style_chain<'a>(styles: &Styles<'a>, family: &str, mut name: Option<&'a str>, common: &CommonStyles) -> Result<()> {
    let mut seen = BTreeSet::new();
    let mut parent = false;
    while let Some(value) = name {
        if seen.len() >= 32 || !seen.insert(value) {
            return Err(OdpError("cyclic or excessive regular master style inheritance".into()));
        }
        let style = styles.get(&(family.into(), value.into())).ok_or_else(|| OdpError("missing regular master style reference".into()))?;
        if parent && source_key(style).is_none_or(|key| !common.contains(&key)) {
            return Err(OdpError("regular master parent style must be a common style".into()));
        }
        name = style.attr("style:parent-style-name");
        parent = true;
    }
    Ok(())
}
fn local_properties<'a>(styles: &Styles<'a>, name: Option<&'a str>, common: &CommonStyles, total: &mut u64) -> Result<Props> {
    page_properties(styles, name, common, total, false)
}
fn page_properties<'a>(
    styles: &Styles<'a>,
    mut name: Option<&'a str>,
    common: &CommonStyles,
    total: &mut u64,
    include_default: bool,
) -> Result<Props> {
    style_chain(styles, "drawing-page", name, common)?;
    let mut chain = Vec::new();
    let mut seen = BTreeSet::new();
    while let Some(value) = name {
        if seen.len() >= 32 || !seen.insert(value) {
            return Err(OdpError("cyclic or excessive regular page style inheritance".into()));
        }
        let style = styles.get(&("drawing-page".into(), value.into())).ok_or_else(|| OdpError("missing regular page style reference".into()))?;
        chain.push(*style);
        name = style.attr("style:parent-style-name");
    }
    let mut props = Props::new();
    let defaults = include_default.then(|| styles.get(&("drawing-page".into(), String::new()))).flatten();
    for style in defaults.copied().into_iter().chain(chain.into_iter().rev()) {
        for bag in style.elements().filter(|bag| bag.name.ends_with("-properties")) {
            for (key, value) in &bag.attrs {
                charge(total, 64usize.saturating_add(key.len()).saturating_add(value.len()))?;
                props.insert(key.clone(), value.clone());
            }
        }
    }
    // Charge expansion before copying resource names, properties and gradient stops. Even
    // overwritten entries count: the cumulative budget bounds repeated shared-style work.
    for (key, family) in [("draw:fill-gradient-name", "gradient"), ("draw:fill-hatch-name", "hatch"), ("draw:fill-image-name", "fill-image")] {
        if let Some(resource) = props.get(key).and_then(|name| styles.get(&(family.into(), name.clone()))) {
            charge(total, 128usize.saturating_add(resource.name.len()))?;
            for (key, value) in &resource.attrs {
                charge(total, 128usize.saturating_add(key.len()).saturating_add(value.len()))?;
            }
            if family == "gradient" {
                for (index, stop) in resource.elements().filter(|element| element.name == "svg:stop").enumerate() {
                    charge(total, std::mem::size_of::<&El>())?;
                    if index < 4096 {
                        for (key, value) in &stop.attrs {
                            charge(total, 128usize.saturating_add(key.len()).saturating_add(value.len()))?;
                        }
                    }
                }
            }
        }
    }
    resources::expand(&mut props, styles);
    Ok(props)
}
fn unsupported(root: &El, total: &mut u64) -> Result<bool> {
    if root.name == "presentation:notes" {
        return Ok(false);
    }
    charge(total, 64)?;
    let mut found = root.attr("presentation:class").is_some()
        || root.attr("presentation:style-name").is_some()
        || root.attr("presentation:class-names").is_some()
        || root.attr("presentation:placeholder").is_some_and(|value| value != "false" && value != "0")
        || root.attr("presentation:user-transformed").is_some()
        || root.name.starts_with("anim:")
        || root.name.starts_with("smil:");
    for child in root.elements() {
        found |= unsupported(child, total)?;
    }
    Ok(found)
}
fn artwork_styles(root: &El, styles: &Styles<'_>, common: &CommonStyles, total: &mut u64) -> Result<()> {
    if root.name == "presentation:notes" {
        return Ok(());
    }
    charge(total, 64)?;
    if root.name.starts_with("draw:") {
        style_chain(styles, "graphic", root.attr("draw:style-name"), common)?;
    }
    if let Some(name) = root.attr("text:style-name") {
        style_chain(
            styles,
            match root.name.as_str() {
                "text:p" | "text:h" => "paragraph",
                "text:list" | "text:numbered-paragraph" => "list",
                _ => "text",
            },
            Some(name),
            common,
        )?;
    }
    for child in root.elements() {
        artwork_styles(child, styles, common, total)?;
    }
    Ok(())
}

pub(super) struct MasterIndex<'a> {
    pub(super) sources: BTreeMap<String, &'a El>,
    pub(super) page_layouts: BTreeMap<String, &'a El>,
    referenced: BTreeSet<String>,
    default_layers: Option<&'a El>,
    all_named: bool,
    pages: BTreeMap<&'a str, Vec<&'a El>>,
    common: CommonStyles,
}
pub(super) struct PageState {
    pub(super) layout: Option<LayoutId>,
    pub(super) background: Option<Background>,
    pub(super) props: Props,
    pub(super) show_master_shapes: bool,
    pub(super) hidden: bool,
}
impl<'a> MasterIndex<'a> {
    pub(super) fn build(content: &'a El, styles_doc: Option<&'a El>, pages: &[&'a El], total: &mut u64) -> Result<Self> {
        let mut sources = BTreeMap::new();
        let mut page_layouts = BTreeMap::new();
        let mut common = BTreeSet::new();
        for root in styles_doc.into_iter().chain(std::iter::once(content)) {
            if root.elements().filter(|element| element.name == "office:master-styles").take(2).count() > 1 {
                return Err(OdpError("multiple ODF master-style containers".into()));
            }
            if root
                .child("office:master-styles")
                .into_iter()
                .flat_map(El::elements)
                .filter(|element| element.name == "draw:layer-set")
                .take(2)
                .count()
                > 1
            {
                return Err(OdpError("multiple document default layer sets".into()));
            }
        }
        let default_layers = styles_doc.and_then(|root| root.child("office:master-styles")).and_then(|masters| masters.child("draw:layer-set"));
        let content_layers = content.child("office:master-styles").and_then(|masters| masters.child("draw:layer-set"));
        if default_layers.is_some() && content_layers.is_some() {
            return Err(OdpError("ambiguous document default layer sets".into()));
        }
        let default_layers = default_layers.or(content_layers);
        for root in styles_doc.into_iter().chain(std::iter::once(content)) {
            for container in root.elements().filter(|child| child.name == "office:styles") {
                for style in container.elements() {
                    if let Some(key) = source_key(style) {
                        charge(total, 64)?;
                        common.insert(key);
                    }
                }
            }
            for (kind, container_name, index) in
                [("style:master-page", "office:master-styles", &mut sources), ("style:page-layout", "office:automatic-styles", &mut page_layouts)]
            {
                let mut definitions = Vec::new();
                root.descendants(kind, &mut definitions);
                let direct: Vec<_> =
                    root.elements().filter(|child| child.name == container_name).flat_map(El::elements).filter(|child| child.name == kind).collect();
                if direct.len() != definitions.len() {
                    return Err(OdpError("regular master or page-layout definition is in an invalid context".into()));
                }
                for definition in direct {
                    let name = definition
                        .attr("style:name")
                        .filter(|name| !name.is_empty())
                        .ok_or_else(|| OdpError("unnamed regular master or page layout".into()))?;
                    charge(total, name.len().saturating_add(128))?;
                    if index.len() >= LIMIT || index.insert(name.into(), definition).is_some() {
                        return Err(OdpError("duplicate or excessive regular master/page-layout definitions".into()));
                    }
                }
            }
        }
        for master in sources.values() {
            if let Some(name) = master.attr("style:page-layout-name") {
                let layout = page_layouts.get(name).ok_or_else(|| OdpError("missing regular master page-layout reference".into()))?;
                if let Some(props) = layout.child("style:page-layout-properties") {
                    for key in ["fo:page-width", "fo:page-height"] {
                        if let Some(value) = props.attr(key)
                            && !length(value).is_some_and(|number| number.is_finite() && number > 0.0 && number <= 5000.0)
                        {
                            return Err(OdpError("invalid regular master page size".into()));
                        }
                    }
                }
            }
        }
        let mut referenced = BTreeSet::new();
        let mut page_index: BTreeMap<&str, Vec<&El>> = BTreeMap::new();
        let mut all_named = !pages.is_empty();
        for page in pages {
            if let Some(name) = page.attr("draw:master-page-name") {
                if !sources.contains_key(name) {
                    return Err(OdpError("missing named regular master".into()));
                }
                charge(total, name.len().saturating_add(64))?;
                page_index.entry(name).or_default().push(*page);
                referenced.insert(name.into());
            } else {
                all_named = false;
            }
        }
        Ok(Self { sources, page_layouts, referenced, default_layers, all_named, pages: page_index, common })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn import(
        &self,
        styles: &Styles<'_>,
        page_styles: &Styles<'_>,
        p: &mut Presentation,
        zip: &mut zip::ZipArchive<Cursor<&[u8]>>,
        total: &mut u64,
        images: &mut ReadResources,
    ) -> Result<BTreeMap<String, LayoutId>> {
        let mut bindings = BTreeMap::new();
        let mut imported = Vec::new();
        for (name, source) in &self.sources {
            let props = page_properties(styles, source.attr("draw:style-name"), &self.common, total, true)?;
            flags(&props)?;
            if !self.referenced.contains(name) {
                if source.elements().any(|child| child.name.starts_with("draw:") && child.name != "draw:layer-set")
                    || (source.attr("draw:style-name").is_some() && props.keys().any(|key| key.starts_with("draw:fill") || key == "draw:opacity"))
                {
                    extras::warn(p, "Unused ODF regular master artwork remains source-recovery-only; edited export blocked");
                }
                continue;
            }
            for child in source.elements() {
                artwork_styles(child, styles, &self.common, total)?;
            }
            let inherited_visibility = ["presentation:background-visible", "presentation:background-objects-visible"]
                .into_iter()
                .any(|key| props.get(key).is_some_and(|value| matches!(value.as_str(), "false" | "0")));
            let mut field_flags =
                ["presentation:display-date-time", "presentation:display-footer", "presentation:display-header", "presentation:display-page-number"]
                    .into_iter()
                    .any(|key| props.contains_key(key));
            let mut partial_paint = false;
            for page in self.pages.get(name.as_str()).into_iter().flatten() {
                let local = local_properties(page_styles, page.attr("draw:style-name"), &self.common, total)?;
                field_flags |= [
                    "presentation:display-date-time",
                    "presentation:display-footer",
                    "presentation:display-header",
                    "presentation:display-page-number",
                ]
                .into_iter()
                .any(|key| local.contains_key(key));
                let paint = local.keys().any(|key| key.starts_with("draw:fill") || key == "draw:opacity" || key == "draw:opacity-name");
                partial_paint |= paint
                    && (!local.contains_key("draw:fill")
                        || (local.get("draw:fill").is_some_and(|value| value == "solid") && !local.contains_key("draw:fill-color"))
                        || (local.get("draw:fill").is_none_or(|value| value != "none")
                            && props.contains_key("draw:opacity")
                            && !local.contains_key("draw:opacity")));
                charge(total, 128usize.saturating_add(local.len().saturating_mul(64)))?;
            }
            if unsupported(source, total)? || inherited_visibility || partial_paint || field_flags {
                extras::warn(p, UNSUPPORTED);
                continue;
            }
            let scoped = owner_styles(styles, source, self.default_layers, total)?;
            let master_id = MasterId(id(p)?);
            let layout_id = LayoutId(id(p)?);
            let mut master = Master {
                id: master_id,
                name: source.attr("style:display-name").unwrap_or(name).into(),
                theme: p.masters.first().map(|master| master.theme.clone()).unwrap_or_default(),
                preserve: true,
                ..Default::default()
            };
            master.background = resources::resolve_fill(&props, p, zip, total, images, p.slide_size.width, p.slide_size.height)?
                .map(|fill| Background::Fill { fill });
            read_shapes(source, &scoped, p, &mut master.shapes, zip, total, images, None, None, None)?;
            master.layouts.push(Layout {
                id: layout_id,
                name: "Shared master artwork".into(),
                kind: LayoutType::Blank,
                show_master_shapes: true,
                preserve: true,
                ..Default::default()
            });
            charge(total, std::mem::size_of::<Master>().saturating_add(std::mem::size_of::<Layout>()).saturating_add(master.name.len()))?;
            imported.push(Arc::new(master));
            bindings.insert(name.clone(), layout_id);
        }
        if self.all_named && bindings.len() == self.referenced.len() {
            p.masters = imported;
        } else {
            p.masters.extend(imported);
        }
        Ok(bindings)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn page(
        &self,
        page: &El,
        master_styles: &Styles<'_>,
        styles: &Styles<'_>,
        bindings: &BTreeMap<String, LayoutId>,
        p: &mut Presentation,
        zip: &mut zip::ZipArchive<Cursor<&[u8]>>,
        total: &mut u64,
        images: &mut ReadResources,
    ) -> Result<PageState> {
        let name = page.attr("draw:master-page-name");
        let layout = name.and_then(|name| bindings.get(name)).copied();
        let master_style = name.and_then(|name| self.sources.get(name)).and_then(|source| source.attr("draw:style-name"));
        let mut props = page_properties(master_styles, master_style, &self.common, total, true)?;
        let local = local_properties(styles, page.attr("draw:style-name"), &self.common, total)?;
        let background = if layout.is_some() {
            if local.get("presentation:background-visible").map(|value| boolean(value)).transpose()? == Some(false) {
                Some(Background::Fill { fill: Fill::None })
            } else if local.keys().any(|key| key.starts_with("draw:fill")) {
                resources::resolve_fill(&local, p, zip, total, images, p.slide_size.width, p.slide_size.height)?.map(|fill| Background::Fill { fill })
            } else {
                None
            }
        } else {
            None
        };
        props.extend(local);
        flags(&props)?;
        let show_master_shapes = props.get("presentation:background-objects-visible").map(|value| boolean(value)).transpose()?.unwrap_or(true);
        let hidden = props.get("presentation:visibility").is_some_and(|value| value == "hidden");
        Ok(PageState { layout, background, props, show_master_shapes, hidden })
    }
}
