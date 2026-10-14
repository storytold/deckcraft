//! DeckCraft's half of Open and Save against cloud storage.
//!
//! The pages themselves are `craft_ui::cloud`. Every Craft app offers the same
//! ones. What is here is what only DeckCraft can answer. A document is a
//! presentation. These extensions and these content types are what that means.
//! Bytes become one through `file.openBytes` and go back through `save_bytes`.
//!
//! DeckCraft holds no provider credential and speaks no provider protocol.
//! Every call goes to the CollabCraft service. That service is the single
//! place a provider is configured and permissions are decided. Replacing one
//! provider with another changes nothing here.
//!
//! Nothing in this file opens a socket either. The transport half is a
//! [`CloudBackend`] the host application supplies (see `apps/deckcraft/cloud`).
//! Every CollabCraft call blocks. A blocking read of a forty megabyte deck
//! inside a frame would freeze the window for seconds. So the pages state a
//! [`Job`]. The host runs it on a worker thread. The answer arrives on a
//! channel that [`pump`] drains on a later frame.

use std::collections::HashMap;
use std::sync::Arc;

// Re-exported so a host application can name these types without depending on
// the crates they come from. A test needs them for the same reason.
pub use craft_cloud::{Capability, Entry, EntryKind};
pub use craft_ui::cloud::{
    Account, Busy, CloudApp, CloudBackend, CloudFiles, Conflict, CopyFormat, Destination, DocumentKind, Event, Fault, Folder, Job, Link, Origin,
    Post, Source,
    UNAVAILABLE, available, browse, can_converse, conflict_modal, destination_name, has_target, link, load_shared, open_entry, open_page, origin,
    parent_of, prefers_cloud, progress, pump, reload_active, save, save_as, save_page, share_button, shares, sign_in, sign_out, start, who,
    work_together,
};
use deckcraft_model::Presentation;
use egui::{Color32, Painter, Rect, Ui};
use serde_json::{Value as Json, json};

use crate::icons::{self, Icon};
use crate::theme::{self, Tokens};
use crate::{SlideApp, now_ms};

/// Extensions DeckCraft opens. The same set the desktop file picker offers.
const OPENS: [&str; 6] = ["deckcraft", "pptx", "potx", "ppsx", "txt", "md"];

/// Content types DeckCraft opens, matched against the start of what the
/// provider said. Read beside the extension because neither is reliable
/// alone. A WebDAV server hands back `application/octet-stream` for a
/// perfectly good `.pptx`. A file exported by a web application often has a
/// correct content type and no extension at all.
///
/// DeckCraft's own format is a zip. The provider therefore calls it
/// `application/zip`. That one is left out. It would offer every archive in
/// the folder. A `.deckcraft` file is already matched by its extension.
const MIMES: [&str; 4] = [
    "application/vnd.openxmlformats-officedocument.presentationml",
    "application/vnd.oasis.opendocument.presentation",
    "text/markdown",
    "text/plain",
];

/// What a DeckCraft document is, as a cloud file list needs to know.
pub const PRESENTATION: DocumentKind = DocumentKind { extensions: &OPENS, mimes: &MIMES, noun: "presentation", default_extension: "deckcraft" };

/// The format every cloud save writes.
///
/// DeckCraft's own format. File › Save writes that to disk. It is also the
/// only one that keeps everything. The pages encode a presentation once and
/// never pass the destination name to the encoder. One format is therefore all
/// there can be. The Save page's name field says which one under it.
const SAVES_AS: &str = "deckcraft";

/// The Save a Copy formats the Share menu offers under the share itself.
///
/// Each identifier is one [`SlideApp::export_as`] takes. Choosing one
/// therefore goes straight through the export the Backstage view already uses.
const COPIES: [(&str, &str); 3] = [
    ("deckcraft", "Save a Copy as DeckCraft Presentation (.deckcraft)…"),
    ("pptx", "Save a Copy as PowerPoint Presentation (.pptx)…"),
    ("pdf", "Export as PDF (.pdf)…"),
];

/// The presentation snapshot each save in flight encoded.
///
/// Only DeckCraft knows what a version of a presentation is. The pages hold
/// none of these. A save that succeeds clears the dirty flag for exactly the
/// version that reached the server. An edit made while the bytes were on the
/// wire stays unsaved.
pub type Sent = HashMap<u64, Arc<Presentation>>;

/// Whether DeckCraft can open this entry.
pub fn opens(entry: &Entry) -> bool {
    craft_ui::cloud::opens(entry, PRESENTATION)
}

/// A typed name as a presentation file name. Any folder separators are dropped
/// and a missing extension becomes `.deckcraft`.
pub fn presentation_name(typed: &str) -> String {
    craft_ui::cloud::document_name(typed, PRESENTATION)
}

/// The whole of the cloud state, for the control channel and for tests.
pub fn status(app: &SlideApp) -> Json {
    craft_ui::cloud::report(app)
}

/// The Share menu's export half, as the control it is drawn in wants it.
pub fn copies() -> Vec<CopyFormat> {
    COPIES.iter().map(|(id, label)| CopyFormat::new(*id, *label)).collect()
}

/// The host part of the service address, for the account menu's first line.
///
/// Read from the environment and compiled in nowhere. The service is
/// deployment-specific. Somebody with an account on two deployments needs to
/// know which one a presentation is about to land in.
fn service_host() -> String {
    ["DECKCRAFT_CLOUD_URL", "CRAFT_CLOUD_URL", "COLLABCRAFT_URL"]
        .into_iter()
        .filter_map(|key| std::env::var(key).ok())
        .map(|value| value.trim().trim_end_matches('/').to_owned())
        .find(|value| !value.is_empty())
        .map(|value| value.trim_start_matches("https://").trim_start_matches("http://").to_owned())
        .unwrap_or_default()
}

/// The format a save should be written in, from where it is going.
///
/// `save_bytes` writes DeckCraft's own format for anything it does not
/// recognise. A `.md` from the cloud would come back as a presentation file
/// under that name, and nothing would say so. The formats it can actually
/// write are named here instead, and anything else is refused.
///
/// A provider that issued no extension still says what the thing is, so a bare
/// name falls back to the content type.
fn writes_as(into: Destination<'_>) -> Result<&'static str, String> {
    const WRITES: [&str; 6] = ["deckcraft", "pptx", "potx", "ppsx", "txt", "pdf"];
    let extension = into.extension();
    let asked = if extension.is_empty() {
        match into.mime.map(|m| m.split(';').next().unwrap_or(m).trim()) {
            Some("application/vnd.openxmlformats-officedocument.presentationml.presentation") => "pptx".to_owned(),
            Some("application/pdf") => "pdf".to_owned(),
            Some("text/plain") => "txt".to_owned(),
            // Nothing readable either way. The application's own format is the
            // only honest answer for a document it alone has ever held.
            _ => SAVES_AS.to_owned(),
        }
    } else {
        extension
    };
    WRITES
        .iter()
        .find(|writes| **writes == asked)
        .copied()
        .ok_or_else(|| format!("DeckCraft cannot write a .{asked}. It writes {}.", WRITES.join(", ")))
}

impl CloudApp for SlideApp {
    fn cloud(&self) -> &CloudFiles {
        &self.cloud
    }

    fn cloud_mut(&mut self) -> &mut CloudFiles {
        &mut self.cloud
    }

    fn kind(&self) -> DocumentKind {
        PRESENTATION
    }

    fn active(&self) -> Option<u64> {
        self.session.active().map(|d| d.uid)
    }

    fn title(&self, uid: u64) -> String {
        self.session.documents().iter().find(|d| d.uid == uid).map(|d| d.title()).unwrap_or_default()
    }

    fn open_bytes(&mut self, name: &str, bytes: Vec<u8>) -> Result<Option<u64>, String> {
        let data = deckcraft_engine::cmd::base64_encode(&bytes);
        self.run("file.openBytes", json!({"name": name, "data": data}))?;
        // The presentation is on screen now. The File view has done its job.
        self.backstage = None;
        Ok(self.session.active().map(|d| d.uid))
    }

    fn encode(&mut self, uid: u64, into: Destination<'_>) -> Result<Vec<u8>, String> {
        let noun = PRESENTATION.noun;
        let doc = self.session.documents().iter().find(|d| d.uid == uid).ok_or(format!("no {noun} open"))?;
        let snapshot = doc.doc.clone();
        let format = writes_as(into)?;
        let bytes = deckcraft_engine::cmd::file::save_bytes(&snapshot, format).map_err(|e| e.to_string())?;
        self.cloud_sent.insert(uid, snapshot);
        Ok(bytes)
    }

    fn close(&mut self, uid: u64) {
        if let Some(index) = self.session.documents().iter().position(|d| d.uid == uid) {
            self.session.close_document(index);
        }
    }

    fn mark_saved(&mut self, uid: u64) {
        let Some(snapshot) = self.cloud_sent.remove(&uid) else { return };
        if let Some(doc) = self.session.document_mut(uid) {
            doc.saved_doc = snapshot;
        }
    }

    fn drop_pending(&mut self, uid: u64) {
        self.cloud_sent.remove(&uid);
    }

    fn ready_to_save(&mut self) -> bool {
        // Notes are typed into a buffer of their own and committed when the
        // field loses focus. A save has to commit them first. Everything else
        // on a slide is edited through a command and is already in the
        // presentation.
        let (uid, index, typed) = self.notes_buf.clone();
        if self.session.active().map(|d| d.uid) != Some(uid) {
            return true;
        }
        let stored = self.session.active().and_then(|d| d.doc.slides.get(index)).map(|s| s.notes_text());
        if stored.is_some_and(|stored| stored != typed) {
            let _ = self.run("slide.notes", json!({"text": typed, "index": index}));
        }
        true
    }

    fn sharing(&mut self) -> &mut craft_ui::share::ShareDialog {
        &mut self.share
    }

    fn presence(&mut self) -> &mut craft_ui::presence::PresenceBar {
        &mut self.presence
    }

    fn paint_icon(&self, painter: &Painter, rect: Rect, kind: EntryKind, colour: Color32) {
        let icon = if kind == EntryKind::Folder { Icon::Folder } else { Icon::Slide };
        icons::paint(painter, rect, icon, colour, false);
    }

    fn open_url(&self, url: &str) {
        // The queue the window drains at the end of a frame. Written straight
        // here because `SlideApp::open_url` shares this method's name. Calling
        // it by name would resolve back to this one.
        self.pending_urls.borrow_mut().push(url.to_owned());
    }

    fn say(&mut self, text: String) {
        // The notice over the window's bottom edge. It carries a Close button
        // and stays until the user is done with it.
        self.toast = Some(crate::toast::Toast { text, at: now_ms() });
    }

    fn toast(&mut self, text: String) {
        self.set_status(text);
    }

    fn go_to_save_as(&mut self) {
        self.open_backstage(crate::backstage::Page::SaveAs);
        if let Some(backstage) = &mut self.backstage {
            backstage.save_to_cloud = Some(true);
        }
    }

    fn app_name(&self) -> &str {
        "DeckCraft"
    }

    fn service(&self) -> String {
        service_host()
    }

    fn save_note(&self) -> Option<String> {
        Some("The presentation is saved as a DeckCraft presentation (.deckcraft) in the folder above.".to_owned())
    }

    fn style(&self, ui: &Ui) -> craft_ui::cloud::CloudStyle {
        let t = Tokens::get(ui.ctx());
        craft_ui::cloud::CloudStyle {
            surface: t.chrome,
            border: t.card_border,
            text: t.text,
            dim: t.text_dim,
            hover: t.hover,
            // A tint of the accent over the page. It reads as an offer.
            // Nothing here is alarming.
            soft: t.accent.gamma_multiply(0.14),
            family: egui::FontFamily::Proportional,
            bold: theme::bold(13.0).family,
        }
    }
}

#[cfg(test)]
#[path = "cloud_tests.rs"]
mod tests;
