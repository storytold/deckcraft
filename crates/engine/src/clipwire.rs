//! Clipboard transfer between DeckCraft instances.
//!
//! The session clipboard ([`Clipboard`]) lives in one process, so a copied shape or slide never
//! reached a second window. Copy now also serializes it into an HTML comment that rides along on
//! the system clipboard's `text/html` entry, and Paste adopts it when it differs from its own
//! copy. Design, formatting, media and the source theme arrive unchanged. Other apps ignore the
//! comment.
use std::sync::Arc;

use deckcraft_color::ColorScheme;
use deckcraft_model::theme::FontScheme;
use deckcraft_model::{MediaItem, Shape, Slide, TextBody};
use serde::{Deserialize, Serialize};

use crate::Clipboard;

const OPEN: &str = "<!--deckcraft-clip:v1:";
const CLOSE: &str = "-->";
/// Largest encoded payload put on the system clipboard (bytes of base64).
const MAX_BLOB: usize = 96 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Wire {
    shapes: Vec<Shape>,
    slides: Vec<Slide>,
    media: Vec<WireMedia>,
    text: Option<TextBody>,
    plain: String,
    source_theme: Option<(ColorScheme, FontScheme)>,
}

/// A media item with its bytes, which `MediaItem` keeps out of its JSON.
#[derive(Serialize, Deserialize)]
struct WireMedia {
    item: MediaItem,
    data: String,
}

/// The payload for copied shapes or slides, or `None` when there are none (a text copy travels as
/// plain HTML) or the payload is too large for the system clipboard.
pub fn encode(c: &Clipboard) -> Option<String> {
    if c.shapes.is_empty() && c.slides.is_empty() {
        return None;
    }
    let wire = Wire {
        shapes: c.shapes.clone(),
        slides: c.slides.clone(),
        media: c.media.iter().map(|m| WireMedia { item: m.clone(), data: crate::cmd::base64_encode(&m.data) }).collect(),
        text: c.text.clone(),
        plain: c.plain.clone(),
        source_theme: c.source_theme.clone(),
    };
    let json = serde_json::to_vec(&wire).ok()?;
    let blob = crate::cmd::base64_encode(&json);
    (blob.len() <= MAX_BLOB).then_some(blob)
}

/// `html` with the payload appended as a comment.
pub fn wrap(html: &str, blob: &str) -> String {
    format!("{html}{OPEN}{blob}{CLOSE}")
}

/// The payload inside `html`, when it carries one.
pub fn blob(html: &str) -> Option<&str> {
    let start = html.find(OPEN)? + OPEN.len();
    let rest = html.get(start..)?;
    let end = rest.find(CLOSE)?;
    rest.get(..end)
}

/// The clipboard a payload describes; `None` when it is damaged or from an unknown version.
pub fn decode(blob: &str) -> Option<Clipboard> {
    let bytes = crate::cmd::base64_decode(blob)?;
    let w: Wire = serde_json::from_slice(&bytes).ok()?;
    let mut media = Vec::with_capacity(w.media.len());
    for m in w.media {
        let mut item = m.item;
        item.data = Arc::new(crate::cmd::base64_decode(&m.data)?);
        media.push(item);
    }
    Some(Clipboard { shapes: w.shapes, slides: w.slides, media, text: w.text, plain: w.plain, source_theme: w.source_theme, pastes: 0 })
}
