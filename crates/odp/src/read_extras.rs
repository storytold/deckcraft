//! Metadata, comments and presentation timing. Unknown content is recovery-only.
use super::*;

pub(super) fn plain(el: &El) -> String {
    let mut out = String::new();
    fn add(el: &El, out: &mut String) {
        for child in &el.children {
            match child {
                Node::Text(s) => out.push_str(s),
                Node::El(e) => {
                    add(e, out);
                    if matches!(e.name.as_str(), "text:p" | "text:h") {
                        out.push('\n');
                    }
                }
            }
        }
    }
    add(el, &mut out);
    out.trim_end_matches('\n').to_string()
}

pub(super) fn metadata(el: &El, props: &mut Properties) {
    let mut metadata = Vec::new();
    el.descendants("office:meta", &mut metadata);
    for meta in metadata {
        for e in meta.elements() {
            let text = if e.name == "meta:creation-date" {
                // Creation is a scalar: retain its exact decoded XML whitespace.
                e.children
                    .iter()
                    .filter_map(|node| match node {
                        Node::Text(text) => Some(text.as_str()),
                        Node::El(_) => None,
                    })
                    .collect()
            } else {
                plain(e)
            };
            match e.name.as_str() {
                "dc:title" => props.title = text,
                "dc:subject" => props.subject = text,
                "dc:description" => props.comments = text,
                "meta:initial-creator" => props.author = text,
                "dc:creator" => props.last_modified_by = text,
                "meta:creation-date" => props.created = text,
                "dc:date" => props.modified = text,
                "meta:keyword" => {
                    if !props.keywords.is_empty() {
                        props.keywords.push_str(", ");
                    }
                    props.keywords.push_str(&text);
                }
                "meta:editing-cycles" => props.revision = text.parse().unwrap_or(0),
                _ => {}
            }
        }
    }
}

pub(super) fn warn(p: &mut Presentation, message: &str) {
    if p.tags.len() < 1000 && !p.tags.iter().any(|(key, v)| key == "odp:warning" && v == message) {
        p.tags.push(("odp:warning".into(), message.into()));
    }
}

pub(super) fn opaque(el: &El, p: &mut Presentation, total: &mut u64) -> Result<Shape> {
    let name = el.attr("draw:name").unwrap_or("");
    let fixed = std::mem::size_of::<Shape>().saturating_add(name.len()).saturating_add(el.name.len()).saturating_add(128);
    let available = crate::MAX_TOTAL.saturating_sub(*total).saturating_sub(fixed as u64) as usize;
    let fragment = xml::source_fragment_limited(el, available)?;
    *total = total.saturating_add(fixed as u64).saturating_add(fragment.len() as u64);
    if *total > crate::MAX_TOTAL {
        return Err(OdpError("opaque content exceeds total size limit".into()));
    }
    warn(p, "Some ODF objects are preserved for recovery in source.odp and are not editable; export may refuse them");
    Ok(Shape {
        id: ShapeId(super::id(p)?),
        name: name.into(),
        kind: ShapeKind::Opaque { xml: fragment, preview: None, label: format!("ODF {} (source recovery only)", el.name), parts: Vec::new() },
        ..Default::default()
    })
}

pub(super) fn milliseconds(value: &str) -> Option<u32> {
    // ISO 8601 durations without calendar units, bounded to 24 hours.
    let value = value.strip_prefix("PT")?;
    let mut token = String::new();
    let mut seconds = 0.0;
    for ch in value.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            token.push(ch);
            continue;
        }
        let n = crate::geometry::number(&token)?;
        seconds += n * match ch {
            'H' => 3600.0,
            'M' => 60.0,
            'S' => 1.0,
            _ => return None,
        };
        token.clear();
    }
    if !token.is_empty() || !(0.0..=86400.0).contains(&seconds) {
        return None;
    }
    Some((seconds * 1000.0).round() as u32)
}

#[path = "read_timing.rs"]
mod timing;
pub(super) use timing::{finish_timing, timing};
