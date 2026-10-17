//! The document chat's part (craft-collab `docs/chat.md`): `/craft/chat.json`, content type
//! `application/vnd.craft.chat+json`, related from the package root with the relationship type
//! `urn:craft:relationships:chat`. Modelled ([`deckcraft_model::chat`]), not kept as an opaque
//! part. Microsoft Office doesn't know it and drops it when it saves.

use std::sync::Arc;

use deckcraft_model::Presentation;
use deckcraft_model::chat::Chat;

use crate::opc::{Package, PackageOut, Rels, RelsOut};

/// The part's name.
pub const PART: &str = "craft/chat.json";
pub const CONTENT_TYPE: &str = "application/vnd.craft.chat+json";
pub const REL: &str = "urn:craft:relationships:chat";
/// Largest part read (10,000 messages of 4,000 characters fit).
const MAX_PART: usize = 256 << 20;

/// The chat the package holds, if any. A malformed part is ignored with a warning: the file
/// still opens.
pub(crate) fn read(pkg: &Package, root: &Rels, p: &mut Presentation) {
    let Some(r) = root.items.iter().find(|r| r.ty == REL && !r.external) else { return };
    let Some(bytes) = pkg.get(&r.target) else {
        log::warn!("pptx: the chat part {} is missing", r.target);
        return;
    };
    if bytes.len() > MAX_PART {
        log::warn!("pptx: the chat part is larger than 256 MiB and was ignored");
        return;
    }
    match serde_json::from_slice::<serde_json::Value>(bytes).ok().and_then(|v| Chat::from_json(&v)) {
        Some(c) => p.chat = Arc::new(c),
        None => log::warn!("pptx: the chat part isn't a version 1 chat and was ignored"),
    }
}

/// Writes the chat part and its relationship (an empty chat writes none).
pub(crate) fn write(p: &Presentation, pkg: &mut PackageOut, root: &mut RelsOut) {
    if p.chat.is_empty() || pkg.has(PART) {
        return;
    }
    let bytes = match serde_json::to_vec(&p.chat.to_json()) {
        Ok(b) => b,
        Err(e) => {
            log::warn!("pptx: the chat couldn't be written: {e}");
            return;
        }
    };
    pkg.xml_part(PART, CONTENT_TYPE, bytes);
    root.add_new(REL, PART, false);
}
