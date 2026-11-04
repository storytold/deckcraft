//! Cameo marker: `alf:liveFeedProps` in a picture's non-visual properties.
//!
//! PowerPoint identifies a camera object by an extension in `p:cNvPicPr`: the shape is an ordinary
//! `p:pic` whose `a:blip` carries the placeholder as an [`asvg:svgBlip`](SVG_BLIP_URI). Both
//! extension URIs are public Office drawing extensions (MS-ODRAWXML); the `livefeed` schema itself
//! describes the feed's background properties, which we keep as written by the file.

use crate::xml::{A, W};

/// Extension URI of `alf:liveFeedProps` in `p:cNvPicPr` (MS-ODRAWXML, 2021 live feed).
pub(crate) const LIVE_FEED_URI: &str = "{51228E76-BA90-4043-B771-695A4F85340A}";
/// Extension URI of `asvg:svgBlip` in `a:blip` (Office 2016 SVG picture).
pub(crate) const SVG_BLIP_URI: &str = "{96DAC541-7B7A-43D3-8B79-37D633B846F1}";
pub(crate) const NS_ALF: &str = "http://schemas.microsoft.com/office/drawing/2021/livefeed";
pub(crate) const NS_ASVG: &str = "http://schemas.microsoft.com/office/drawing/2016/SVG/main";

/// Does `p:cNvPicPr` carry the Cameo extension?
pub(crate) fn is_live(nv_pic_pr: Option<&crate::xml::El>) -> bool {
    let Some(exts) = nv_pic_pr.and_then(|n| n.child("extLst")) else { return false };
    exts.elements().any(|e| {
        e.is("ext") && e.attr("uri").is_some_and(|u| u.trim().eq_ignore_ascii_case(LIVE_FEED_URI)) && e.find("liveFeedProps").is_some()
    })
}

/// Write the extension that makes PowerPoint read the picture as a Cameo. The element is empty:
/// the live-feed schema has no device field, and an empty feed keeps the file's default camera.
pub(crate) fn write_live_feed(w: &mut W) {
    w.open0("a:extLst");
    w.open("a:ext", A::new().a("uri", LIVE_FEED_URI));
    w.empty("alf:liveFeedProps", A::new().a("xmlns:alf", NS_ALF));
    w.close("a:ext");
    w.close("a:extLst");
}
