//! Cameo marker: `alf:liveFeedProps` in a picture's non-visual properties.
//!
//! PowerPoint identifies a camera object by an extension in `p:cNvPicPr`: the shape is an ordinary
//! `p:pic` whose `a:blip` carries the placeholder as an [`asvg:svgBlip`](SVG_BLIP_URI). Both
//! extension URIs are taken from a PowerPoint-authored file; the `livefeed` schema itself
//! (MS-ODRAWXML) describes the feed's background properties, which we keep verbatim.

use deckcraft_model::{CameraBackground, CameraBackgroundMode};

use crate::xml::{A, El, Node, W};

/// Extension URI of `alf:liveFeedProps` in `p:cNvPicPr`, as PowerPoint writes it.
pub(crate) const LIVE_FEED_URI: &str = "{51228E76-BA90-4043-B771-695A4F85340A}";
/// Extension URI of `asvg:svgBlip` in `a:blip` (Office 2016 SVG picture).
pub(crate) const SVG_BLIP_URI: &str = "{96DAC541-7B7A-43D3-8B79-37D633B846F1}";
pub(crate) const NS_ALF: &str = "http://schemas.microsoft.com/office/drawing/2021/livefeed";
pub(crate) const NS_ASVG: &str = "http://schemas.microsoft.com/office/drawing/2016/SVG/main";

fn is_feed_uri(uri: &str) -> bool {
    // PowerPoint writes the GUID; some generators name the schema namespace instead.
    let uri = uri.trim();
    uri.eq_ignore_ascii_case(LIVE_FEED_URI) || uri.eq_ignore_ascii_case(NS_ALF)
}

/// The `alf:liveFeedProps` element inside `p:cNvPicPr/a:extLst`, if the picture is a Cameo.
pub(crate) fn live_feed_props(nv_pic_pr: Option<&crate::xml::El>) -> Option<&crate::xml::El> {
    let exts = nv_pic_pr?.child("extLst")?;
    for e in exts.elements() {
        if e.is("ext")
            && e.attr("uri").is_some_and(is_feed_uri)
            && let Some(props) = e.find("liveFeedProps")
        {
            return Some(props);
        }
    }
    None
}

/// The background mode a feed's `alf:backgroundProps` names, if it names one.
pub(crate) fn read_background(props: &El) -> Option<CameraBackground> {
    let bg = props.child("backgroundProps")?;
    let mode = bg.elements().find_map(|e| CameraBackgroundMode::from_xml_name(e.local()))?;
    Some(CameraBackground::new(mode))
}

/// Write the extension that makes PowerPoint read the picture as a Cameo. `raw` is the feed
/// element read from the file, kept so its settings survive; without one we write the empty feed
/// PowerPoint writes for a plain camera. `background` is our structured choice, written over the
/// kept feed's `backgroundProps` so a mode set in DeckCraft reaches PowerPoint.
pub(crate) fn write_live_feed(w: &mut W, raw: Option<&str>, background: Option<&CameraBackground>) {
    w.open0("a:extLst");
    w.open("a:ext", A::new().a("uri", LIVE_FEED_URI));
    match background {
        None => match raw.and_then(clean_feed) {
            Some(xml) => w.raw(&xml),
            None => w.empty("alf:liveFeedProps", A::new().a("xmlns:alf", NS_ALF)),
        },
        Some(bg) => w.raw(&feed_with_background(raw, bg)),
    }
    w.close("a:ext");
    w.close("a:extLst");
}

/// The kept feed with its `backgroundProps` replaced by ours, preserving every other child (and
/// every unknown child of the old `backgroundProps`). A feed that names a relationship, or isn't a
/// feed at all, is replaced by a feed carrying only the background: writing the old one back would
/// leave a dangling relationship in a file PowerPoint repairs.
fn feed_with_background(raw: Option<&str>, bg: &CameraBackground) -> String {
    let Some((mut root, ns)) = raw.and_then(clean_feed_doc) else {
        let mut root = El { name: "alf:liveFeedProps".into(), attrs: vec![("xmlns:alf".into(), NS_ALF.into())], children: Vec::new() };
        root.children.push(Node::El(background_props("alf", bg, None)));
        return crate::read::keep_xml(&root, &[]);
    };
    let prefix = if root.prefix().is_empty() { "alf" } else { root.prefix() }.to_string();
    if prefix == "alf" && !root.attrs.iter().any(|(k, _)| k == "xmlns:alf") {
        root.attrs.push(("xmlns:alf".into(), NS_ALF.into()));
    }
    let old = root.elements().find(|e| e.is("backgroundProps")).cloned();
    root.children.retain(|n| !matches!(n, Node::El(e) if e.is("backgroundProps")));
    root.children.insert(0, Node::El(background_props(&prefix, bg, old.as_ref())));
    crate::read::keep_xml(&root, &ns)
}

/// `<alf:backgroundProps><alf:Blur/><extLst…/></alf:backgroundProps>`, keeping the children of the
/// old element that are not a mode.
fn background_props(prefix: &str, bg: &CameraBackground, old: Option<&El>) -> El {
    let mut props = El { name: format!("{prefix}:backgroundProps"), attrs: Vec::new(), children: Vec::new() };
    props.children.push(Node::El(El { name: format!("{prefix}:{}", bg.mode.xml_name()), attrs: Vec::new(), children: Vec::new() }));
    if let Some(old) = old {
        for child in old.elements() {
            if CameraBackgroundMode::from_xml_name(child.local()).is_none() {
                props.children.push(Node::El(child.clone()));
            }
        }
    }
    props
}

/// Re-serialize the kept feed through the parser, so only a single `liveFeedProps` element (with
/// its namespaces) reaches the output and a damaged or hostile string writes nothing extra.
fn clean_feed(raw: &str) -> Option<String> {
    clean_feed_doc(raw).map(|(root, ns)| crate::read::keep_xml(&root, &ns))
}

fn clean_feed_doc(raw: &str) -> Option<(El, Vec<(String, String)>)> {
    let doc = crate::xml::parse(raw.as_bytes()).ok()?;
    // A feed naming a relationship cannot be written back: the export creates no such
    // relationship, so PowerPoint would see a dangling reference in a repaired file.
    if doc.root.local() != "liveFeedProps" || crate::read::has_rel_refs(&doc.root) {
        return None;
    }
    let ns = doc.ns_decls();
    Some((doc.root, ns))
}
