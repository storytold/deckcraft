//! Cameo marker: `alf:liveFeedProps` in a picture's non-visual properties.
//!
//! PowerPoint identifies a camera object by an extension in `p:cNvPicPr`: the shape is an ordinary
//! `p:pic` whose `a:blip` carries the placeholder as an [`asvg:svgBlip`](SVG_BLIP_URI). Both
//! extension URIs are taken from a PowerPoint-authored file; the `livefeed` schema itself
//! (MS-ODRAWXML) describes the feed's background properties, which we keep verbatim.

use crate::xml::{A, W};

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

/// Write the extension that makes PowerPoint read the picture as a Cameo. `raw` is the feed
/// element read from the file, kept so its background settings survive; without one we write the
/// empty feed PowerPoint writes for a plain camera.
pub(crate) fn write_live_feed(w: &mut W, raw: Option<&str>) {
    let feed = raw.and_then(clean_feed);
    w.open0("a:extLst");
    w.open("a:ext", A::new().a("uri", LIVE_FEED_URI));
    match feed {
        Some(xml) => w.raw(&xml),
        None => w.empty("alf:liveFeedProps", A::new().a("xmlns:alf", NS_ALF)),
    }
    w.close("a:ext");
    w.close("a:extLst");
}

/// Re-serialize the kept feed through the parser, so only a single `liveFeedProps` element (with
/// its namespaces) reaches the output and a damaged or hostile string writes nothing extra.
fn clean_feed(raw: &str) -> Option<String> {
    let doc = crate::xml::parse(raw.as_bytes()).ok()?;
    // A feed naming a relationship cannot be written back: the export creates no such
    // relationship, so PowerPoint would see a dangling reference in a repaired file.
    if doc.root.local() != "liveFeedProps" || crate::read::has_rel_refs(&doc.root) {
        return None;
    }
    Some(crate::read::keep_xml(&doc.root, &doc.ns_decls()))
}
