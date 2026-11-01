//! Optional file-provider integration; engine/control commands remain deterministic and local.

use crate::file_providers::{Capture, Command, Host};
use crate::{SlideApp, json};

impl SlideApp {
    pub(crate) fn choose_provider_save(&mut self, save_as: bool, format: &str) -> bool {
        let Some(host) = self.services.file_providers.clone().filter(|host| host.enabled()) else { return false };
        let Some(doc) = self.session.active() else { return true };
        let extension = if format == "pptx" { "pptx" } else { "deckcraft" };
        let name = if save_as {
            format!("{}.{extension}", doc.title())
        } else {
            host.filename(doc.uid).unwrap_or_else(|| format!("{}.{extension}", doc.title()))
        };
        if let Err(error) = host.choose(doc.uid, doc.revision, name, save_as) {
            self.set_status(error);
        }
        true
    }

    fn provider_document(&self, id: u64) -> Option<usize> {
        self.session.documents().iter().position(|doc| doc.uid == id)
    }

    pub(crate) fn provider_tick(&mut self) {
        let Some(host) = self.services.file_providers.clone() else { return };
        for command in host.take_commands() {
            match command {
                command @ Command::Save { .. } => self.provider_save(&host, command),
                command @ Command::Result { .. } => self.provider_result(&host, command),
                command @ Command::Open { .. } => self.provider_open(&host, command),
            }
        }
        let ids: Vec<u64> = self.session.documents().iter().map(|doc| doc.uid).collect();
        let documents = self
            .session
            .documents()
            .iter()
            .map(|doc| {
                json!({"id":doc.uid.to_string(), "title":doc.title(), "revision":doc.revision,
            "dirty":doc.is_dirty(), "provider":host.bound(doc.uid), "pending":host.pending_for(doc.uid)})
            })
            .collect::<Vec<_>>();
        host.publish_documents(json!(documents), &ids);
    }

    fn provider_mark_saved(&mut self, id: u64, revision: u64) {
        let Some(index) = self.provider_document(id) else { return };
        let back = self.session.active_index();
        self.session.set_active(index);
        if let Some(doc) = self.session.active_mut()
            && doc.revision == revision
        {
            doc.saved_doc = doc.doc.clone();
        }
        if let Some(back) = back {
            self.session.set_active(back);
        }
    }
    fn provider_save(&mut self, host: &Host, command: Command) {
        let Command::Save { request, doc, destination, name, save_as } = command else { return };
        let encoded = self.provider_document(doc).ok_or_else(|| "The presentation was closed before saving".to_string()).and_then(|index| {
            let state = &self.session.documents()[index];
            let format = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
            if !matches!(format.as_str(), "deckcraft" | "slidecraft" | "pptx" | "potx" | "ppsx") {
                return Err("Save requires a presentation package. Use Export for pictures, PDF or text copies.".into());
            }
            if destination == "provider" && state.doc.media.iter().any(|media| media.link.is_some() && media.data.is_empty()) {
                return Err("Linked media bytes are unavailable on this host. Embed the linked files before saving to the provider.".into());
            }
            let bytes = deckcraft_engine::cmd::file::save_bytes(&state.doc, &format).map_err(|error| error.to_string())?;
            Ok((state.revision, bytes))
        });
        match encoded {
            Ok((revision, bytes)) if destination == "provider" => {
                if let Err(error) = host.capture(&request, Capture { doc, revision, name, save_as }, bytes) {
                    host.failed(&request, &error);
                    self.set_status(error);
                }
            }
            Ok((revision, bytes)) => {
                let result = self
                    .services
                    .provider_download
                    .as_ref()
                    .ok_or_else(|| "No device download service is available".to_string())
                    .and_then(|download| download(&name, &bytes));
                match result {
                    Ok(()) => {
                        if !host.bound(doc) {
                            self.provider_mark_saved(doc, revision);
                        }
                        host.finish_device(&request);
                        self.set_status(format!("Downloaded {name}"));
                    }
                    Err(error) => {
                        host.failed(&request, &error);
                        self.set_status(error);
                    }
                }
            }
            Err(error) => {
                host.failed(&request, &error);
                self.set_status(error);
            }
        }
    }
    fn provider_result(&mut self, host: &Host, command: Command) {
        let Command::Result { capture, resource, status } = command else { return };
        if status == "saved" && self.provider_document(capture.doc).is_some() {
            let aliases = host.bind(&capture, resource);
            let back = self.session.active_index();
            for alias in aliases {
                if let Some(index) = self.provider_document(alias) {
                    self.session.set_active(index);
                    if let Some(state) = self.session.active_mut() {
                        state.saved_doc = std::sync::Arc::new((*state.saved_doc).clone());
                        state.revision = state.revision.wrapping_add(1);
                    }
                }
            }
            if let Some(back) = back {
                self.session.set_active(back);
            }
            self.provider_mark_saved(capture.doc, capture.revision);
            self.set_status(format!("Saved {} to provider", capture.name));
        } else if status == "failed" {
            self.set_status("Provider save failed. Your presentation is still open.");
        }
    }
    fn provider_open(&mut self, host: &Host, command: Command) {
        let Command::Open { request, name, bytes } = command else { return };
        let data = deckcraft_engine::cmd::base64_encode(&bytes);
        match self.run("file.openBytes", json!({"name":name,"data":data})) {
            Ok(_) => {
                self.backstage = None;
                let capture = self.session.active().map(|doc| Capture { doc: doc.uid, revision: doc.revision, name, save_as: false });
                host.opened(&request, capture, "The imported presentation has no document");
            }
            Err(error) => {
                host.opened(&request, None, &error);
                self.set_status(error);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::file_providers::Host;
    use crate::{Services, SlideApp, json};

    fn app() -> (SlideApp, Host) {
        let host = Host::default();
        host.set_session("0123456789abcdef0123456789abcdef".into()).unwrap();
        host.connect("test".into(), "Test storage".into(), "account".into()).unwrap();
        let services = Services { file_providers: Some(host.clone()), ..Default::default() };
        let mut app = SlideApp::new(deckcraft_engine::Session::with_new(), services);
        app.session.execute("slide.new", &json!({})).unwrap();
        (app, host)
    }

    fn save(app: &mut SlideApp, host: &Host) -> (String, Vec<u8>) {
        assert!(app.choose_provider_save(false, "deckcraft"));
        let dialog = host.take_events().into_iter().find(|event| event.metadata["type"] == "saveDialog").unwrap();
        let request = dialog.metadata["requestId"].as_str().unwrap().to_string();
        host.resolve_dialog(&request, "provider", "Presentation.deckcraft").unwrap();
        app.provider_tick();
        let event = host.take_events().into_iter().find(|e| e.metadata["type"] == "save").unwrap();
        (request, event.bytes.unwrap())
    }

    #[test]
    fn native_presentation_save_and_reopen_are_correlated_and_keep_local_recents_separate() {
        let (mut app, host) = app();
        let count = app.session.active().unwrap().doc.slides.len();
        let (request, bytes) = save(&mut app, &host);
        assert!(app.session.active().unwrap().is_dirty());
        host.result(request, "saved".into(), "resource-one".into()).unwrap();
        app.provider_tick();
        assert!(!app.session.active().unwrap().is_dirty());
        assert!(app.ui.recent.is_empty());
        let open = host.request_open("resource-one").unwrap();
        host.open(open, "Presentation.deckcraft".into(), bytes).unwrap();
        app.provider_tick();
        assert_eq!(app.session.active().unwrap().doc.slides.len(), count);
        assert_eq!(app.session.documents().len(), 2);
        assert!(app.session.active().unwrap().path.is_none());
        assert!(app.ui.recent.is_empty());
    }

    #[test]
    fn old_capture_ack_does_not_clean_a_newer_presentation_revision() {
        let (mut app, host) = app();
        let (request, _) = save(&mut app, &host);
        app.session.execute("slide.new", &json!({})).unwrap();
        host.result(request, "saved".into(), "resource-one".into()).unwrap();
        app.provider_tick();
        assert!(app.session.active().unwrap().is_dirty());
    }

    #[test]
    fn older_ack_cannot_clean_an_alias_after_another_tab_overwrites_its_resource() {
        let (mut app, host) = app();
        let (request, bytes) = save(&mut app, &host);
        host.result(request, "saved".into(), "shared".into()).unwrap();
        app.provider_tick();
        let read = host.request_open("shared").unwrap();
        host.open(read, "Presentation.deckcraft".into(), bytes).unwrap();
        app.provider_tick();
        assert_eq!(app.session.documents().len(), 2);
        app.session.set_active(0);
        app.session.execute("slide.new", &json!({})).unwrap();
        let (old, _) = save(&mut app, &host);
        app.session.set_active(1);
        app.session.execute("slide.new", &json!({})).unwrap();
        let (new, _) = save(&mut app, &host);
        host.result(new, "saved".into(), "shared".into()).unwrap();
        app.provider_tick();
        host.result(old, "saved".into(), "shared".into()).unwrap();
        app.provider_tick();
        assert!(app.session.documents()[0].is_dirty());
    }
    #[test]
    fn linked_media_without_bytes_cannot_be_reported_as_a_complete_provider_save() {
        let (mut app, host) = app();
        let state = app.session.active_mut().unwrap();
        let presentation = std::sync::Arc::make_mut(&mut state.doc);
        let id = presentation.add_media("linked.wav", "audio/wav", vec![]).unwrap();
        presentation.media.iter_mut().find(|m| m.id == id).unwrap().link = Some("/unavailable/linked.wav".into());
        assert!(app.choose_provider_save(false, "deckcraft"));
        let events = host.take_events();
        let request = events[0].metadata["requestId"].as_str().unwrap();
        host.resolve_dialog(request, "provider", "Presentation.deckcraft").unwrap();
        app.provider_tick();
        let events = host.take_events();
        assert!(events.iter().any(|event| event.metadata["type"] == "saveFailed"));
        assert!(!events.iter().any(|event| event.metadata["type"] == "save"));
        assert!(app.session.active().unwrap().is_dirty());
        assert!(!host.busy());
    }
}
