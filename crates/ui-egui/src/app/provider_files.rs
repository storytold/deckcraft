//! Optional file-provider integration; engine/control commands remain deterministic and local.

use crate::file_providers::{Capture, Command, Host};
use crate::{ProviderClose, SlideApp, json};

fn suggested_provider_name(title: &str, extension: &str) -> String {
    let suffix = format!(".{extension}");
    let budget = 255usize.saturating_sub(suffix.len());
    let mut stem = String::new();
    for ch in title.chars() {
        let ch = if ch.is_control() || matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { ch };
        if stem.len().saturating_add(ch.len_utf8()) > budget {
            break;
        }
        stem.push(ch);
    }
    let stem = stem.trim().trim_matches('.');
    format!("{}{suffix}", if stem.is_empty() { "Presentation" } else { stem })
}

impl SlideApp {
    fn provider_save_host(&self) -> Option<Host> {
        self.services.file_providers.clone().filter(|host| {
            host.enabled() || self.session.active().is_some_and(|doc| host.bound(doc.uid) || host.pending_for(doc.uid))
        })
    }

    pub(crate) fn choose_provider_save(&mut self, save_as: bool, format: &str) -> bool {
        let Some(host) = self.provider_save_host() else { return false };
        let Some(doc) = self.session.active() else { return true };
        if !host.enabled() {
            if self.provider_close.as_ref().is_some_and(|close| close.document == doc.uid) {
                self.provider_close = None;
            }
            self.set_status("Provider storage is paused. Resume it before saving this presentation.");
            return true;
        }
        let extension = if format == "pptx" { "pptx" } else { "deckcraft" };
        let name = if save_as {
            suggested_provider_name(&doc.title(), extension)
        } else {
            host.filename(doc.uid).unwrap_or_else(|| suggested_provider_name(&doc.title(), extension))
        };
        if let Err(error) = host.choose(doc.uid, doc.revision, name, save_as) {
            self.set_status(error);
        }
        true
    }

    /// The Close prompt's Save waits for this provider request, rather than a browser download.
    pub(crate) fn save_and_close_presentation(&mut self) {
        if let Some(host) = self.provider_save_host() {
            let Some(document) = self.session.active().map(|doc| doc.uid) else { return };
            self.provider_close = None;
            if !host.enabled() {
                self.notify("Provider storage is paused. Resume it before saving this presentation.");
                return;
            }
            if host.pending_for(document) {
                self.notify("Finish the pending save before closing this presentation.");
                return;
            }
            self.save();
            if let Some(request) = host.dialog_request(document) {
                self.provider_close = Some(ProviderClose {
                    request,
                    connection: host.snapshot()["connectionId"].as_str().unwrap_or_default().to_string(),
                    document,
                    revision: None,
                });
            }
            return;
        }
        self.save();
        // Preserve the original provider-free browser and native close paths.
        let web = self.services.pick_save.is_none() && self.services.download.is_some();
        if web || self.session.active().is_some_and(|doc| !doc.is_dirty()) {
            let _ = self.run("file.close", json!({}));
        }
    }

    /// Runs only after a settled UI pass: input and drafts get their chance before any close.
    pub(crate) fn finish_provider_close(&mut self, ctx: &egui::Context) {
        let Some(close) = self.provider_close.as_ref() else { return };
        let Some(host) = self.services.file_providers.as_ref() else {
            self.provider_close = None;
            return;
        };
        let Some(index) = self.provider_document(close.document) else {
            self.provider_close = None;
            return;
        };
        if host.snapshot()["connectionId"].as_str() != Some(close.connection.as_str()) {
            self.provider_close = None;
            return;
        }
        let Some(revision) = close.revision else {
            if !host.pending_for(close.document) {
                self.provider_close = None;
            }
            return;
        };
        let document = &self.session.documents()[index];
        if document.revision != revision || document.is_dirty() {
            self.provider_close = None;
            return;
        }
        if host.pending_for(close.document)
            || self.dialog.is_some()
            || document.interaction.is_some()
            || document.composition.is_some()
            || (self.notes_buf.0 == close.document && self.notes_buf.2.edited)
            || ctx.egui_wants_keyboard_input()
            || ctx.will_discard()
        {
            return;
        }
        if ctx.input(|input| !input.events.is_empty() || !input.raw.dropped_files.is_empty()) {
            ctx.request_repaint();
            return;
        }
        let document = close.document;
        let back = self.session.active().map(|doc| doc.uid);
        self.provider_close = None;
        self.session.set_active(index);
        let _ = self.run("file.close", json!({}));
        if let Some(back) = back.filter(|uid| *uid != document)
            && let Some(index) = self.provider_document(back)
        {
            self.session.set_active(index);
        }
        self.sync_backstage();
        ctx.request_repaint();
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
            "dirty":doc.is_dirty() || (self.notes_buf.0 == doc.uid && self.notes_buf.2.edited),
            "provider":host.bound(doc.uid), "pending":host.pending_for(doc.uid)})
            })
            .collect::<Vec<_>>();
        host.publish_documents(json!(documents), &ids);
    }

    fn provider_mark_saved(&mut self, id: u64, revision: u64) {
        if self.notes_buf.0 == id && self.notes_buf.2.edited {
            return;
        }
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
    fn settle_provider_notes(&mut self, doc: u64) -> Result<(), String> {
        if self.notes_buf.0 != doc || !self.notes_buf.2.edited {
            return Ok(());
        }
        // Commands target the active presentation. Never activate another tab from a callback
        // or consume the current presentation's unrelated notes draft.
        if self.session.active().is_none_or(|state| state.uid != doc) {
            return Err("Activate the presentation and finish its notes before saving it".into());
        }
        self.session
            .execute("slide.notes", &json!({"body":self.notes_buf.2.body,"index":self.notes_buf.1}))
            .map_err(|error| error.to_string())?;
        self.notes_buf.2.edited = false;
        Ok(())
    }
    fn provider_save(&mut self, host: &Host, command: Command) {
        let Command::Save { request, doc, destination, name, save_as } = command else { return };
        if let Err(error) = self.settle_provider_notes(doc) {
            host.failed(&request, &error);
            self.set_status(error);
            return;
        }
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
            Ok((_, bytes)) => {
                let result = self
                    .services
                    .provider_download
                    .as_ref()
                    .ok_or_else(|| "No device download service is available".to_string())
                    .and_then(|download| download(&name, &bytes));
                match result {
                    Ok(()) => {
                        // Acceptance can precede a cancelled picker or a failed browser write.
                        // Device copies never acknowledge a durable document checkpoint.
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
        let Command::Result { request, capture, resource, status } = command else { return };
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
            if let Some(close) = self.provider_close.as_mut().filter(|close| close.request == request && close.document == capture.doc) {
                close.revision = Some(capture.revision);
            }
            self.set_status(format!("Saved {} to provider", capture.name));
        } else if status == "failed" {
            self.set_status("Provider save failed. Your presentation is still open.");
        }
        if status != "saved" && self.provider_close.as_ref().is_some_and(|close| close.request == request) {
            self.provider_close = None;
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

    fn close_choice(app: &mut SlideApp, host: &Host) -> String {
        app.save_and_close_presentation();
        let event = host.take_events().into_iter().find(|event| event.metadata["type"] == "saveDialog").unwrap();
        event.metadata["requestId"].as_str().unwrap().to_string()
    }

    fn settle_close(app: &mut SlideApp) {
        let ctx = egui::Context::default();
        ctx.run_ui(Default::default(), |ui| app.finish_provider_close(ui.ctx())).drop_without_applying_deltas();
    }

    #[test]
    fn real_close_prompt_retains_the_presentation_until_its_durable_provider_result() {
        let (mut app, host) = app();
        // This makes the original web shortcut true; it must not bypass a provider save.
        app.services.download = Some(Box::new(|_, _| {}));
        app.dialog = Some(crate::dialogs::Dialog::new("close"));
        let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app,
        );
        harness.run_steps(3);
        harness.key_press(egui::Key::Enter);
        harness.run_steps(3);
        assert_eq!(harness.state().session.documents().len(), 1, "the host has not even received bytes yet");
        let events = host.take_events();
        let event = events.iter().find(|event| event.metadata["type"] == "saveDialog").unwrap();
        let request = event.metadata["requestId"].as_str().unwrap().to_string();
        host.resolve_dialog(&request, "provider", "Closing.deckcraft").unwrap();
        harness.run_steps(3);
        assert!(host.take_events().iter().any(|event| event.metadata["type"] == "save" && event.bytes.is_some()));
        assert_eq!(harness.state().session.documents().len(), 1, "encoding is not persistence");
        host.result(request, "saved".into(), "closing-resource".into()).unwrap();
        harness.run_steps(4);
        assert!(harness.state().session.documents().is_empty());
    }

    #[test]
    fn paused_provider_saves_and_the_real_close_save_callback_never_download_or_arm_a_close() {
        for initially_bound in [false, true] {
            let (mut app, host) = app();
            let document = app.session.active().unwrap().uid;
            let old_request = if initially_bound {
                let (request, _) = save(&mut app, &host);
                host.result(request.clone(), "saved".into(), "resource-one".into()).unwrap();
                app.provider_tick();
                request
            } else {
                let request = close_choice(&mut app, &host);
                host.resolve_dialog(&request, "provider", "Pending.deckcraft").unwrap();
                app.provider_tick();
                assert!(host.take_events().iter().any(|event| event.metadata["type"] == "save"));
                assert!(app.provider_close.is_some());
                request
            };
            host.set_enabled(false).unwrap();
            app.session.execute("slide.new", &json!({})).unwrap();
            let downloads = std::rc::Rc::new(std::cell::Cell::new(0));
            let received = downloads.clone();
            app.services.download = Some(Box::new(move |_, bytes| {
                assert!(!bytes.is_empty());
                received.set(received.get() + 1);
            }));
            let received = downloads.clone();
            app.services.provider_download = Some(Box::new(move |_, bytes| {
                assert!(!bytes.is_empty());
                received.set(received.get() + 1);
                Ok(())
            }));

            app.save();
            app.save_as_dialog("deckcraft");
            assert_eq!(downloads.get(), 0);
            assert!(app.session.active().unwrap().is_dirty());
            assert!(app.provider_close.is_none());
            assert_eq!(host.bound(document), initially_bound);
            assert_eq!(host.pending_for(document), !initially_bound);
            assert!(host.take_events().is_empty());
            app.close_presentation();
            let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_ui_state(
                |ui, app: &mut SlideApp| {
                    app.logic(ui.ctx());
                    app.ui(ui);
                },
                app,
            );
            harness.run_steps(3);
            harness.key_press(egui::Key::Enter);
            harness.run_steps(3);
            assert_eq!(harness.state().session.active().unwrap().uid, document);
            assert!(harness.state().session.active().unwrap().is_dirty());
            assert!(harness.state().provider_close.is_none());
            assert_eq!(downloads.get(), 0, "the successful browser download callback must never be reached");
            assert!(host.take_events().is_empty());

            host.set_enabled(true).unwrap();
            if !initially_bound {
                host.result(old_request, "saved".into(), "resource-one".into()).unwrap();
                harness.run_steps(3);
                assert_eq!(harness.state().session.active().unwrap().uid, document);
                assert!(harness.state().session.active().unwrap().is_dirty(), "the old capture excludes the later slide");
                assert!(harness.state().provider_close.is_none(), "resuming cannot revive the refused close");
            }
            harness.state_mut().close_presentation();
            harness.run_steps(3);
            harness.key_press(egui::Key::Enter);
            harness.run_steps(3);
            let event = host.take_events().into_iter().find(|event| event.metadata["type"] == "saveDialog").unwrap();
            let request = event.metadata["requestId"].as_str().unwrap().to_string();
            host.resolve_dialog(&request, "provider", "Resumed.deckcraft").unwrap();
            harness.run_steps(3);
            assert!(host.take_events().iter().any(|event| event.metadata["type"] == "save" && event.bytes.is_some()));
            assert_eq!(harness.state().session.documents().len(), 1, "resumed serialization still needs a durable ACK");
            host.result(request, "saved".into(), "resource-one".into()).unwrap();
            harness.run_steps(4);
            assert!(harness.state().session.documents().is_empty());
            assert_eq!(downloads.get(), 0);
        }
    }

    #[test]
    fn cancelled_failed_or_downloaded_close_never_discards_the_document() {
        for destination in ["device", "cancel", "failed", "download"] {
            let (mut app, host) = app();
            let copies = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
            let received = copies.clone();
            app.services.provider_download = Some(Box::new(move |name, bytes| {
                received.borrow_mut().push((name.to_string(), bytes.to_vec()));
                Ok(())
            }));
            let request = close_choice(&mut app, &host);
            let choice = if matches!(destination, "failed" | "download") { "provider" } else { destination };
            host.resolve_dialog(&request, choice, "Keep.deckcraft").unwrap();
            app.provider_tick();
            if choice == "provider" {
                assert!(host.take_events().iter().any(|event| event.metadata["type"] == "save"));
                host.result(request, destination.into(), String::new()).unwrap();
                app.provider_tick();
            }
            settle_close(&mut app);
            app.provider_tick();
            assert_eq!(app.session.documents().len(), 1);
            assert_eq!(host.snapshot()["documents"][0]["dirty"], true);
            assert!(app.session.active().unwrap().is_dirty());
            assert!(!host.busy());
            assert!(!host.bound(app.session.active().unwrap().uid));
            assert_eq!(copies.borrow().len(), usize::from(destination == "device"));
            let count = app.session.active().unwrap().doc.slides.len();
            app.session.execute("slide.new", &json!({})).unwrap();
            app.provider_tick();
            assert_eq!(app.session.active().unwrap().doc.slides.len(), count + 1);
            assert_eq!(host.snapshot()["documents"][0]["dirty"], true);
        }
    }

    #[test]
    fn acknowledged_close_rechecks_input_later_edits_and_uncommitted_notes() {
        for notes in [false, true] {
            let (mut app, host) = app();
            let request = close_choice(&mut app, &host);
            host.resolve_dialog(&request, "provider", "Keep.deckcraft").unwrap();
            app.provider_tick();
            host.take_events();
            host.result(request, "saved".into(), "resource".into()).unwrap();
            app.provider_tick();
            assert_eq!(app.session.documents().len(), 1, "ACK alone cannot close before the UI pass");
            let ctx = egui::Context::default();
            let raw = egui::RawInput { events: vec![egui::Event::Text("late input".into())], ..Default::default() };
            ctx.run_ui(raw, |ui| {
                app.finish_provider_close(ui.ctx());
                assert_eq!(app.session.documents().len(), 1, "pending input must retain its target");
                if notes {
                    app.notes_buf.0 = app.session.active().unwrap().uid;
                    app.notes_buf.2.edited = true;
                    app.notes_buf.2.text = "Uncommitted notes".into();
                    deckcraft_model::edit::replace_text(&mut app.notes_buf.2.body, "Uncommitted notes");
                } else {
                    app.session.execute("slide.new", &json!({})).unwrap();
                }
            })
            .drop_without_applying_deltas();
            settle_close(&mut app);
            app.provider_tick();
            assert_eq!(app.session.documents().len(), 1);
            assert_eq!(host.snapshot()["documents"][0]["dirty"], true);
            if notes {
                assert_eq!(app.notes_buf.2.text, "Uncommitted notes");
                assert!(app.notes_buf.2.edited);
            } else {
                assert!(app.session.active().unwrap().is_dirty());
            }
        }
    }

    #[test]
    fn a_cancelled_close_request_cannot_be_completed_by_a_later_unrelated_save() {
        let (mut app, host) = app();
        let cancelled = close_choice(&mut app, &host);
        host.resolve_dialog(&cancelled, "cancel", "").unwrap();
        let (later, _) = save(&mut app, &host);
        host.result(later, "saved".into(), "resource".into()).unwrap();
        app.provider_tick();
        settle_close(&mut app);
        assert_eq!(app.session.documents().len(), 1);
        assert!(!app.session.active().unwrap().is_dirty());
    }

    #[test]
    fn provider_free_browser_close_keeps_the_original_download_route() {
        for provider in ["absent", "default-off", "paused-unbound"] {
            let (mut app, host) = app();
            match provider {
                "absent" => app.services.file_providers = None,
                "default-off" => app.services.file_providers = Some(Host::default()),
                _ => host.set_enabled(false).unwrap(),
            }
            let downloads = std::rc::Rc::new(std::cell::Cell::new(0));
            let received = downloads.clone();
            app.services.download = Some(Box::new(move |_, bytes| {
                assert!(!bytes.is_empty());
                received.set(received.get() + 1);
            }));
            app.save();
            app.save_as_dialog("deckcraft");
            assert_eq!(downloads.get(), 2);
            app.close_presentation();
            let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_ui_state(
                |ui, app: &mut SlideApp| {
                    app.logic(ui.ctx());
                    app.ui(ui);
                },
                app,
            );
            harness.run_steps(3);
            harness.key_press(egui::Key::Enter);
            harness.run_steps(3);
            assert_eq!(downloads.get(), 3);
            assert!(harness.state().session.documents().is_empty());
        }
    }

    #[test]
    fn invalid_or_long_titles_get_a_bounded_suggestion_and_bound_names_are_preserved() {
        for save_as in [false, true] {
            let (mut app, host) = app();
            app.session.active_mut().unwrap().title = format!("/bad\\title\n{}", "界".repeat(300));
            assert!(app.choose_provider_save(save_as, "deckcraft"));
            let events = host.take_events();
            let dialog = events.iter().find(|event| event.metadata["type"] == "saveDialog").unwrap();
            let name = dialog.metadata["name"].as_str().unwrap();
            assert!(crate::file_providers::valid_name(name));
            assert!(name.len() <= 255);
            assert!(name.ends_with(".deckcraft"));
        }
        let (mut app, host) = app();
        let (request, _) = save(&mut app, &host);
        host.result(request, "saved".into(), "resource".into()).unwrap();
        app.provider_tick();
        app.session.active_mut().unwrap().title = "/different/name".into();
        assert!(app.choose_provider_save(false, "pptx"));
        let events = host.take_events();
        assert_eq!(events[0].metadata["name"], "Presentation.deckcraft");
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
    fn resolving_a_provider_save_commits_notes_to_their_slide_before_encoding() {
        for extension in ["deckcraft", "pptx"] {
            let (mut app, host) = app();
            app.session.execute("slide.notes", &json!({"index":0,"text":"Original notes"})).unwrap();
            let document = app.session.active().unwrap().uid;
            let selected = app.session.active().unwrap().selection.slide;
            assert_ne!(selected, 0, "the draft belongs to a slide other than the current selection");
            assert!(app.choose_provider_save(false, extension));
            let dialog = host.take_events().into_iter().find(|event| event.metadata["type"] == "saveDialog").unwrap();
            let request = dialog.metadata["requestId"].as_str().unwrap().to_string();
            let mut draft = crate::notes::Draft::new(&app.session.active().unwrap().doc.slides[0].notes);
            deckcraft_model::edit::replace_text(&mut draft.body, "Notes typed while choosing storage");
            draft.text = draft.body.text();
            draft.edited = true;
            app.notes_buf = (document, 0, draft);

            let name = format!("Notes.{extension}");
            host.resolve_dialog(&request, "provider", &name).unwrap();
            app.provider_tick();
            let event = host.take_events().into_iter().find(|event| event.metadata["type"] == "save").unwrap();
            let restored = deckcraft_engine::cmd::file::open_presentation(&name, &event.bytes.unwrap()).unwrap();
            assert_eq!(restored.slides[0].notes.text(), "Notes typed while choosing storage");
            assert!(restored.slides[selected].notes.text().is_empty());
            assert_eq!(app.session.active().unwrap().uid, document);
            assert_eq!(app.session.active().unwrap().selection.slide, selected);
            assert!(!app.notes_buf.2.edited);

            // Notes typed after capture cannot be covered by the older package's ACK.
            deckcraft_model::edit::replace_text(&mut app.notes_buf.2.body, "Newer notes after capture");
            app.notes_buf.2.text = app.notes_buf.2.body.text();
            app.notes_buf.2.edited = true;
            host.result(request, "saved".into(), "resource-one".into()).unwrap();
            app.provider_tick();
            assert!(app.session.active().unwrap().is_dirty());
            assert_eq!(host.snapshot()["documents"][0]["dirty"], true);
            assert!(app.notes_buf.2.edited);
        }
    }

    #[test]
    fn inactive_or_invalid_notes_drafts_fail_without_retargeting_or_discarding_them() {
        for inactive in [false, true] {
            let (mut app, host) = app();
            let document = app.session.active().unwrap().uid;
            assert!(app.choose_provider_save(false, "deckcraft"));
            let dialog = host.take_events().into_iter().find(|event| event.metadata["type"] == "saveDialog").unwrap();
            let request = dialog.metadata["requestId"].as_str().unwrap().to_string();
            let mut draft = crate::notes::Draft::default();
            deckcraft_model::edit::replace_text(&mut draft.body, "Keep this draft after failure");
            draft.text = draft.body.text();
            draft.edited = true;
            app.notes_buf = (document, if inactive { 0 } else { usize::MAX }, draft);
            if inactive {
                app.session.execute("file.new", &json!({})).unwrap();
            }
            let active = app.session.active().unwrap().uid;
            let current = app.session.active().unwrap().doc.clone();
            let draft = app.notes_buf.clone();
            host.resolve_dialog(&request, "provider", "Notes.deckcraft").unwrap();
            app.provider_tick();
            let events = host.take_events();
            assert!(events.iter().any(|event| event.metadata["type"] == "saveFailed" && event.metadata["requestId"] == request));
            assert!(!events.iter().any(|event| event.metadata["type"] == "save"));
            assert!(host.result(request, "saved".into(), "resource-one".into()).is_err());
            assert_eq!(app.session.active().unwrap().uid, active);
            assert!(std::sync::Arc::ptr_eq(&app.session.active().unwrap().doc, &current));
            assert_eq!(app.notes_buf, draft);
            assert!(!host.bound(document));
            assert!(!host.busy());
        }
    }

    #[test]
    fn saving_an_inactive_presentation_preserves_the_active_presentations_notes_draft() {
        let (mut app, host) = app();
        app.session.execute("slide.notes", &json!({"index":0,"text":"Notes belonging to the saved presentation"})).unwrap();
        let document = app.session.active().unwrap().uid;
        assert!(app.choose_provider_save(false, "deckcraft"));
        let dialog = host.take_events().into_iter().find(|event| event.metadata["type"] == "saveDialog").unwrap();
        let request = dialog.metadata["requestId"].as_str().unwrap().to_string();
        app.session.execute("file.new", &json!({})).unwrap();
        let active = app.session.active().unwrap().uid;
        let mut draft = crate::notes::Draft::default();
        deckcraft_model::edit::replace_text(&mut draft.body, "Unrelated active presentation draft");
        draft.text = draft.body.text();
        draft.edited = true;
        app.notes_buf = (active, 0, draft);
        let draft = app.notes_buf.clone();
        let current = app.session.active().unwrap().doc.clone();
        host.resolve_dialog(&request, "provider", "Inactive.deckcraft").unwrap();
        app.provider_tick();
        let event = host.take_events().into_iter().find(|event| event.metadata["type"] == "save").unwrap();
        assert_eq!(event.metadata["documentId"], document.to_string());
        let restored = deckcraft_engine::cmd::file::open_presentation("Inactive.deckcraft", &event.bytes.unwrap()).unwrap();
        assert_eq!(restored.slides[0].notes.text(), "Notes belonging to the saved presentation");
        assert_eq!(app.session.active().unwrap().uid, active);
        assert!(std::sync::Arc::ptr_eq(&app.session.active().unwrap().doc, &current));
        assert_eq!(app.notes_buf, draft);
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
