use super::{
    FilePlace, FilePurpose, FileTarget, InboxEvent, LocatedPlace, PendingFile, RECENT_MAX, SlideApp, Target, UiRequest, Value, ViewMode, dialogs,
    json, media,
};

#[derive(Default)]
pub(crate) struct PrintPreviews(Vec<std::path::PathBuf>);

impl PrintPreviews {
    fn create(&mut self, title: &str) -> std::io::Result<std::path::PathBuf> {
        self.create_in(&std::env::temp_dir(), title)
    }

    fn create_in(&mut self, base: &std::path::Path, title: &str) -> std::io::Result<std::path::PathBuf> {
        let dir = create_private_print_dir(base)?;
        self.0.push(dir.clone());
        let safe: String = title.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' }).collect();
        Ok(dir.join(format!("{safe} (print).pdf")))
    }

    fn discard(&mut self, path: &std::path::Path) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::remove_dir_all(dir);
            self.0.retain(|owned| owned != dir);
        }
    }

    pub(crate) fn cleanup(&mut self) {
        for dir in self.0.drain(..) {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

impl Drop for PrintPreviews {
    fn drop(&mut self) {
        self.cleanup();
    }
}

fn create_private_print_dir(base: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

    for _ in 0..128 {
        let id = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let path = base.join(format!("deckcraft-print-{}-{id}", std::process::id()));
        #[cfg(unix)]
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(not(unix))]
        let builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        match builder.create(&path) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "could not allocate a unique print preview directory"))
}

fn purpose_of(purpose: &str) -> FilePurpose {
    match purpose {
        "open" => FilePurpose::Open,
        "picture" => FilePurpose::Picture,
        "audio" => FilePurpose::Audio,
        "video" => FilePurpose::Video,
        _ => FilePurpose::Infer,
    }
}

fn is_presentation_name(lower: &str) -> bool {
    lower.ends_with(".deckcraft")
        || lower.ends_with(".slidecraft")
        || lower.ends_with(".pptx")
        || lower.ends_with(".potx")
        || lower.ends_with(".ppsx")
}

/// File → Open keeps the requested operation: a presentation opens, an outline becomes slides.
fn open_command(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    if is_presentation_name(&lower) {
        Some("file.openBytes")
    } else if lower.ends_with(".txt") || lower.ends_with(".md") || lower.ends_with(".rtf") {
        Some("slide.fromOutline")
    } else {
        None
    }
}

/// A drop has no requested operation, so the name and bytes decide: a presentation opens,
/// then a recognised picture, sound or video is inserted, then an outline becomes slides.
fn infer_command(name: &str, bytes: &[u8]) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    if is_presentation_name(&lower) {
        return Some("file.openBytes");
    }
    let ct = deckcraft_engine::cmd::insert::content_type(name, bytes);
    if ct.starts_with("image/") {
        Some("insert.picture")
    } else if ct.starts_with("audio/") {
        Some("insert.audio")
    } else if ct.starts_with("video/") {
        Some("insert.video")
    } else if lower.ends_with(".txt") || lower.ends_with(".md") || lower.ends_with(".rtf") {
        Some("slide.fromOutline")
    } else {
        None
    }
}

fn command_for(purpose: FilePurpose, name: &str, bytes: &[u8]) -> Result<&'static str, String> {
    let cmd = match purpose {
        FilePurpose::Picture => Some("insert.picture"),
        FilePurpose::Audio => Some("insert.audio"),
        FilePurpose::Video => Some("insert.video"),
        FilePurpose::ChangePicture => deckcraft_engine::cmd::insert::content_type(name, bytes).starts_with("image/").then_some("picture.change"),
        FilePurpose::Open => open_command(name),
        FilePurpose::Infer => infer_command(name, bytes),
    };
    cmd.ok_or_else(|| format!("{name}: DeckCraft can't open this kind of file"))
}

impl SlideApp {
    /// Does this recent path exist on disk? Probed once per path per session and only on
    /// desktop (the web build has no file system to ask): a stat on a dead network share can
    /// block for seconds, so it must not repeat on every frame. A stale answer is harmless —
    /// opening still tries and reports, and a successful open re-probes.
    pub fn recent_exists(&mut self, path: &str) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = path;
            true
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(ok) = self.recent_checked.get(path) {
                return *ok;
            }
            // A stat on a dead network location (UNC share, URL) can block for the OS timeout,
            // so remote-looking paths are never probed: they stay unmarked and are judged when
            // opened. A disconnected mapped drive would still stall once — accepted.
            if path.starts_with("\\\\") || path.contains("://") {
                return true;
            }
            let ok = std::path::Path::new(path).is_file();
            if self.recent_checked.len() > 64 {
                let live: std::collections::HashSet<&String> = self.ui.recent.iter().collect();
                self.recent_checked.retain(|p, _| live.contains(p));
            }
            self.recent_checked.insert(path.to_string(), ok);
            ok
        }
    }

    /// Drop a path from the Recent list (its file is gone or the user asked for it).
    pub fn forget_recent(&mut self, path: &str) {
        self.ui.recent.retain(|p| p != path);
        self.recent_checked.remove(path);
    }

    /// File › Close and Ctrl+W: closes the active presentation, asking first when it has unsaved
    /// changes (Save, Don't Save, Cancel), as PowerPoint does.
    pub fn close_presentation(&mut self) {
        match self.session.active() {
            None => {}
            Some(d) if d.is_dirty() => self.dialog = Some(dialogs::Dialog::new("close")),
            Some(_) => {
                let _ = self.run("file.close", json!({}));
            }
        }
    }

    /// The file name a save or export suggests: the presentation's title with `ext`.
    fn suggested_name(&self, ext: &str) -> String {
        let name = self.session.active().map(|d| d.title()).unwrap_or_else(|| "Presentation".into());
        format!("{name}.{ext}")
    }

    /// Exports the active presentation as `fmt` (`pdf`, `png`, `pngAll`, `jpeg`, `pptx`,
    /// `deckcraft`, `outline`) through `file.export`: the save picker on the desktop, a download
    /// on the web. `extra` adds `file.export` params (the PDF's layout, slides…). The
    /// presentation keeps its own file. Returns whether a file was written or handed over.
    pub fn export_as(&mut self, fmt: &str, extra: Value) -> bool {
        let Some(slide) = self.session.active().map(|d| d.selection.slide) else { return false };
        let (ext, format, all) = match fmt {
            "pdf" => ("pdf", "pdf", false),
            "png" => ("png", "png", false),
            "pngAll" => ("png", "png", true),
            "jpeg" | "jpg" => ("jpg", "jpeg", false),
            "pptx" => ("pptx", "pptx", false),
            "outline" | "txt" => ("txt", "outline", false),
            "rtf" => ("rtf", "rtf", false),
            "deckcraft" => ("deckcraft", "deckcraft", false),
            "gif" => ("gif", "gif", false),
            other => {
                self.set_status(format!("DeckCraft can't export “{other}”"));
                return false;
            }
        };
        let suggested = self.suggested_name(ext);
        if let Some(pick) = self.services.pick_save.as_mut() {
            let Some(path) = pick(&suggested) else { return false };
            let mut params = json!({"path": path, "format": format, "all": all});
            if let (Some(o), Some(x)) = (params.as_object_mut(), extra.as_object()) {
                o.extend(x.clone());
            }
            return match self.run("file.export", params) {
                Ok(_) => {
                    self.set_status(format!("Exported {path}"));
                    true
                }
                Err(_) => false,
            };
        }
        let Some(doc) = self.session.active().map(|d| d.doc.clone()) else { return false };
        let bytes = match format {
            "png" | "jpeg" => {
                // Every slide for "all slides", one download each, named like the desktop export (#64).
                let name = self.session.active().map(|d| d.title()).unwrap_or_else(|| "Presentation".into());
                let mut any = false;
                let transparent = extra.get("transparentBackground").and_then(Value::as_bool).unwrap_or(false);
                for (file, bytes) in web_image_files(&doc, slide, all, &name, ext, transparent) {
                    any = self.download(&file, Ok(bytes)) || any;
                }
                return any;
            }
            "pdf" => deckcraft_engine::cmd::file::pdf_bytes(&doc, &extra),
            "gif" => deckcraft_engine::cmd::file::gif_bytes(&doc, &extra),
            other => deckcraft_engine::cmd::file::save_bytes(&doc, other),
        };
        self.download(&suggested, bytes)
    }

    /// Hands `bytes` to the browser as a download (web); false (with the reason in the status
    /// bar) when there are no bytes or no way to download.
    fn download(&mut self, name: &str, bytes: deckcraft_engine::Result<Vec<u8>>) -> bool {
        match (bytes, self.services.download.as_mut()) {
            (Ok(b), Some(dl)) => {
                dl(name, &b);
                true
            }
            (Err(e), _) => {
                self.set_status(e.to_string());
                false
            }
            (Ok(_), None) => {
                self.set_status("Saving a copy needs a file picker, which this window doesn't have.");
                false
            }
        }
    }

    /// File › Print: DeckCraft prints through a PDF. On the desktop the PDF (with the Print page's
    /// layout, slides and options) is written to the temporary folder and opened in the system's
    /// PDF viewer, which has the printers; on the web it downloads.
    pub fn print_pdf(&mut self, params: Value) -> bool {
        let Some((title, doc)) = self.session.active().map(|d| (d.title(), d.doc.clone())) else { return false };
        let mut params = if params.is_object() { params } else { json!({}) };
        if self.services.pick_save.is_none() {
            // Web: the browser hands the PDF to the user.
            let bytes = deckcraft_engine::cmd::file::pdf_bytes(&doc, &params);
            return self.download(&format!("{title}.pdf"), bytes);
        }
        let preview_path = match self.print_previews.create(&title) {
            Ok(path) => path,
            Err(e) => {
                self.set_status(format!("Couldn't create a private print preview: {e}"));
                return false;
            }
        };
        let path = preview_path.to_string_lossy().to_string();
        if let Some(o) = params.as_object_mut() {
            o.insert("path".into(), json!(path));
            o.insert("format".into(), json!("pdf"));
        }
        match self.run("file.export", params) {
            Ok(_) => {
                self.open_url(&format!("file:///{}", path.replace('\\', "/").trim_start_matches('/')));
                self.set_status("Opened the presentation as a PDF to print");
                true
            }
            Err(_) => {
                self.print_previews.discard(&preview_path);
                false
            }
        }
    }

    pub(crate) fn pick_and_open(&mut self) {
        if let Some(pick) = self.services.pick_open.as_mut() {
            if let Some(path) = pick("open") {
                self.open_path_in_background(&path);
            }
        } else if self.services.open_async.is_some() {
            let id = self.begin_external_file("open");
            if let Some(f) = self.services.open_async.as_mut() {
                f("open", id);
            }
        }
    }

    pub fn open_path(&mut self, path: &str) -> Result<(), String> {
        self.session.execute("file.open", &json!({"path": path})).map_err(|e| e.to_string())?;
        self.note_recent(path);
        Ok(())
    }

    /// Files the system hands over (the command line, a double-click through the file
    /// association), opened in order. When the last one is a PowerPoint Show (`.ppsx`), its slide
    /// show starts at once from where Play from Start begins, as PowerPoint does; anything else
    /// opens for editing, and File › Open opens a `.ppsx` for editing too. Returns the files that
    /// couldn't be opened, with the reason.
    pub fn open_from_system(&mut self, paths: &[String]) -> Vec<(String, String)> {
        let mut failed = Vec::new();
        let mut last = None;
        for p in paths {
            match self.open_path(p) {
                Ok(()) => last = Some(p),
                Err(e) => failed.push((p.clone(), e)),
            }
        }
        if last.is_some_and(|p| p.to_ascii_lowercase().ends_with(".ppsx")) {
            self.start_show(self.show_start_index(), false);
        }
        failed
    }

    /// A presentation was opened from `path`: first in the recent list, shown in Normal view.
    pub(crate) fn note_recent(&mut self, path: &str) {
        self.status = None; // an older error (a missing file just before this one) must not linger
        self.recent_checked.remove(path);
        self.ui.recent.retain(|p| p != path);
        self.ui.recent.insert(0, path.to_string());
        self.ui.recent.truncate(RECENT_MAX);
        self.ui.view = ViewMode::Normal;
    }

    /// Transitions › Sound › Other Sound…: pick an audio file for the selected slides.
    pub(crate) fn pick_transition_sound(&mut self) {
        if self.services.pick_open.is_some() {
            self.pick_and_insert("audio", "transition.sound");
        }
    }

    /// Home › New Slide › Reuse Slides…: pick a presentation and insert all its slides after the
    /// current one (desktop only: the browser picker opens what it picks as a document).
    pub(super) fn pick_and_reuse(&mut self, keep: bool) {
        let Some(pick) = self.services.pick_open.as_mut() else {
            self.set_status("Reuse Slides needs the desktop app".to_string());
            return;
        };
        let Some(path) = pick("open") else { return };
        // Read the file once: the list shown and the slides inserted come from the same bytes, even
        // if the file changes while the dialog is open. Without a reader the path is used as is.
        let mut source = json!({});
        match self.services.read.as_ref().map(|r| r(&path)) {
            Some(Ok(bytes)) => {
                let name = std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                source["name"] = json!(name);
                source["data"] = json!(deckcraft_engine::cmd::base64_encode(&bytes));
            }
            Some(Err(e)) => {
                self.set_status(e.to_string());
                return;
            }
            None => source["path"] = json!(path),
        }
        // Offer the file's slides to choose from; all of them are ticked to begin with.
        match self.session.execute("slide.reuseList", &source) {
            Ok(list) => {
                let mut d = dialogs::Dialog::new("reuseSlides");
                d.params = json!({"path": path, "source": source, "keep": keep, "slides": list["slides"]});
                self.reuse_target = Some(self.capture_file_target());
                self.dialog = Some(d);
            }
            Err(e) => self.set_status(e.to_string()),
        }
    }

    /// Insert the slides `picked` (indices) of `source` (the `name` + `data`, or `path`, that
    /// `slide.reuseList` listed) after the slide that was current when the picker opened, in that
    /// presentation, even if another one is active now; if it is closed or the slide is gone,
    /// nothing is inserted. Returns whether they were inserted.
    pub fn reuse_slides(&mut self, source: &Value, keep: bool, picked: &[usize]) -> bool {
        let mut args = if source.is_object() { source.clone() } else { json!({}) };
        args["keepFormatting"] = json!(keep);
        args["slides"] = json!(picked);
        let result = match self.reuse_target {
            Some(target) if !self.viewing_target(&target) => self.execute_on_target(&target, "Reuse Slides", "slide.reuse", &args),
            _ => self.session.execute("slide.reuse", &args).map(|_| ()).map_err(|e| e.to_string()),
        };
        match result {
            Ok(()) => {
                self.reuse_target = None;
                self.set_status(format!("Inserted {} slides", picked.len()));
                true
            }
            Err(e) => {
                self.set_status(e);
                false
            }
        }
    }

    pub(crate) fn pick_and_insert(&mut self, purpose: &str, cmd: &str) {
        self.pick_and_run(purpose, cmd, &json!({}));
    }

    /// Pick a file and run `cmd` with it (`name` + `data`, or `path`) added to `extra`.
    pub(super) fn pick_and_run(&mut self, purpose: &str, cmd: &str, extra: &Value) {
        if let Some(pick) = self.services.pick_open.as_mut() {
            if let Some(path) = pick(purpose) {
                let mut args = if extra.is_object() { extra.clone() } else { json!({}) };
                let r = match self.services.read.as_ref().map(|r| r(&path)) {
                    Some(Ok(bytes)) => {
                        let name = std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                        args["name"] = json!(name);
                        args["data"] = json!(deckcraft_engine::cmd::base64_encode(&bytes));
                        self.session.execute(cmd, &args)
                    }
                    _ => {
                        args["path"] = json!(path);
                        self.session.execute(cmd, &args)
                    }
                };
                if let Err(e) = r {
                    self.set_status(e.to_string());
                }
            }
        } else if self.services.open_async.is_some() {
            let id = self.begin_external_file(purpose);
            // Change Picture: the picture that arrives replaces the selected one (#115).
            if cmd == "picture.change"
                && let Some(r) = self.pending_files.iter_mut().find(|r| r.id == id)
            {
                r.purpose = FilePurpose::ChangePicture;
            }
            if let Some(f) = self.services.open_async.as_mut() {
                f(purpose, id);
            }
        }
    }

    pub fn save_as_dialog(&mut self, format: &str) {
        let name = self.session.active().map(|d| d.title()).unwrap_or_else(|| "Presentation".into());
        let ext = if format == "pptx" { "pptx" } else { "deckcraft" };
        let suggested = format!("{name}.{ext}");
        if let Some(pick) = self.services.pick_save.as_mut() {
            if let Some(path) = pick(&suggested) {
                match self.session.execute("file.saveAs", &json!({"path": path})) {
                    Ok(_) => {
                        self.set_status(format!("Saved {path}"));
                        self.ui.recent.retain(|p| *p != path);
                        self.ui.recent.insert(0, path);
                    }
                    Err(e) => self.set_status(e.to_string()),
                }
            }
        } else if let Some(dl) = self.services.download.as_mut()
            && let Some(d) = self.session.active()
        {
            match deckcraft_engine::cmd::file::save_bytes(&d.doc, ext) {
                Ok(bytes) => dl(&suggested, &bytes),
                Err(e) => self.set_status(e.to_string()),
            }
        }
    }

    /// Save every presentation with unsaved changes (asking for a location where needed).
    /// Returns false if one wasn't saved (cancelled or failed).
    pub fn save_all(&mut self) -> bool {
        let dirty: Vec<usize> = self.session.documents().iter().enumerate().filter(|(_, d)| d.is_dirty()).map(|(i, _)| i).collect();
        for i in dirty {
            self.session.set_active(i);
            self.save();
            if self.session.documents().get(i).is_some_and(|d| d.is_dirty()) {
                return false;
            }
        }
        true
    }

    pub fn save(&mut self) {
        if self.session.active().and_then(|d| d.path.clone()).is_some() {
            match self.session.execute("file.save", &json!({})) {
                Ok(_) => self.set_status("Saved"),
                Err(e) => self.set_status(e.to_string()),
            }
        } else {
            self.save_as_dialog("deckcraft");
        }
    }

    /// Act on requests commands left for the UI.
    pub(super) fn drain_requests(&mut self) {
        for r in std::mem::take(&mut self.session.ui_requests) {
            match r {
                UiRequest::Dialog { id, params } => {
                    let mut d = dialogs::Dialog::new(&id);
                    d.params = params;
                    self.dialog = Some(d);
                }
                UiRequest::Pane { id } => self.ui.pane = Some(id),
                UiRequest::StartShow { from } => self.start_show(from, false),
                UiRequest::Message { text } => self.set_status(text),
                UiRequest::PickFile { purpose } => {
                    if purpose == "saveAs" {
                        self.save_as_dialog("deckcraft");
                    }
                }
                UiRequest::Media { action, shape, ms } => {
                    let owner = if self.show.is_some() { media::Owner::Show } else { media::Owner::Editor };
                    // The command resolved `shape` on the editor's slide, also while a show runs.
                    if let Some((uid, doc, slide)) = self.session.active().map(|d| (d.uid, d.doc.clone(), d.selection.slide)) {
                        self.media.request(uid, &doc, slide, &action, (shape, ms), owner);
                    }
                }
            }
        }
    }

    pub(crate) fn drain_inbox(&mut self) {
        let Some(inbox) = self.services.inbox.clone() else { return };
        let events = match inbox.lock() {
            Ok(mut v) => std::mem::take(&mut *v),
            Err(poisoned) => std::mem::take(&mut *poisoned.into_inner()),
        };
        for event in events {
            match event {
                InboxEvent::Cancelled { id } => self.pending_files.retain(|r| r.id != id),
                InboxEvent::File(file) => self.apply_external_file(file.id, &file.name, &file.bytes),
            }
        }
    }

    /// Remember the open presentation, the slide (or master/layout) and the operation for a
    /// file the host will read asynchronously. Returns the cancellation identity the host must
    /// echo on the [`InboxEvent`]. Call this when the pick or the drop starts, not when the
    /// bytes arrive.
    pub fn begin_external_file(&mut self, purpose: &str) -> u64 {
        let id = self.next_file_id;
        self.next_file_id = self.next_file_id.wrapping_add(1).max(1);
        self.pending_files.push(PendingFile { id, purpose: purpose_of(purpose), target: self.capture_file_target() });
        id
    }

    /// A dropped or picked file read synchronously: presentations open, media is inserted on
    /// the slide that is current now.
    pub fn open_or_insert_bytes(&mut self, name: &str, bytes: &[u8]) {
        let id = self.begin_external_file("drop");
        self.apply_external_file(id, name, bytes);
    }

    fn capture_file_target(&self) -> FileTarget {
        let Some(d) = self.session.active() else { return FileTarget::Absent };
        let place = match d.selection.target {
            Target::Slides => match d.current_slide() {
                Some(s) => FilePlace::Slide(s.id),
                None => FilePlace::Unspecified,
            },
            Target::Master { master } => match d.doc.masters.get(master) {
                Some(m) => FilePlace::Master(m.id),
                None => FilePlace::Unspecified,
            },
            Target::Layout { master, layout } => match d.doc.masters.get(master) {
                Some(m) => match m.layouts.get(layout) {
                    Some(l) => FilePlace::Layout { master: m.id, layout: l.id },
                    None => FilePlace::Unspecified,
                },
                None => FilePlace::Unspecified,
            },
            t @ (Target::Handout | Target::NotesMaster) => FilePlace::NotesPage(t),
        };
        FileTarget::Document { doc: d.uid, place }
    }

    /// True when the user is still looking at the place this request captured.
    fn viewing_target(&self, target: &FileTarget) -> bool {
        let FileTarget::Document { doc, place } = target else { return false };
        let Some(d) = self.session.active() else { return false };
        if d.uid != *doc {
            return false;
        }
        match place {
            FilePlace::Slide(id) => d.selection.target == Target::Slides && d.current_slide().is_some_and(|s| s.id == *id),
            FilePlace::Master(id) => {
                matches!(d.selection.target, Target::Master { master } if d.doc.masters.get(master).is_some_and(|m| m.id == *id))
            }
            FilePlace::Layout { master, layout } => {
                matches!(d.selection.target, Target::Layout { master: mi, layout: li }
                    if d.doc.masters.get(mi).is_some_and(|m| m.id == *master && m.layouts.get(li).is_some_and(|l| l.id == *layout)))
            }
            FilePlace::NotesPage(t) => d.selection.target == *t,
            FilePlace::Unspecified => false,
        }
    }

    fn apply_external_file(&mut self, id: u64, name: &str, bytes: &[u8]) {
        let Some(pos) = self.pending_files.iter().position(|r| r.id == id) else { return };
        let req = self.pending_files.remove(pos);
        let cmd = match command_for(req.purpose, name, bytes) {
            Ok(cmd) => cmd,
            Err(e) => {
                self.set_status(e);
                return;
            }
        };
        let params = if cmd == "slide.fromOutline" {
            json!({"text": deckcraft_engine::cmd::file::outline_text(bytes), "markdown": name.to_ascii_lowercase().ends_with(".md")})
        } else {
            json!({"name": name, "data": deckcraft_engine::cmd::base64_encode(bytes)})
        };
        let result = if cmd == "file.openBytes" || self.viewing_target(&req.target) {
            self.session.execute(cmd, &params).map(|_| ()).map_err(|e| e.to_string())
        } else {
            self.execute_on_target(&req.target, name, cmd, &params)
        };
        match result {
            Err(e) if cmd == "file.openBytes" => self.notify(e),
            Err(e) => self.set_status(e),
            Ok(()) => {}
        }
        self.drain_requests();
    }

    /// Run `cmd` on the document and slide captured for the request, then put the view back
    /// if the user has since moved. A missing document or slide cancels the file.
    fn execute_on_target(&mut self, target: &FileTarget, name: &str, cmd: &str, params: &Value) -> Result<(), String> {
        let FileTarget::Document { doc, place } = target else {
            self.session.execute("file.new", &json!({})).map_err(|e| e.to_string())?;
            return self.session.execute(cmd, params).map(|_| ()).map_err(|e| e.to_string());
        };
        let Some(idx) = self.session.documents().iter().position(|d| d.uid == *doc) else {
            return Err(format!("{name} was not applied: the presentation it was chosen for is closed"));
        };
        let view = self.session.active_index();
        let saved = self.session.documents().get(idx).map(|d| d.selection.clone());
        self.session.set_active(idx);
        let located = match self.locate_place(place) {
            Ok(located) => located,
            Err(e) => {
                self.restore_file_view(view);
                return Err(format!("{name} was not applied: {e}"));
            }
        };
        if let Some(st) = self.session.active_mut() {
            st.selection.target = located.target;
            if let Some(slide) = located.slide {
                st.selection.slide = slide;
                st.selection.slides.clear();
            }
            st.selection.shapes.clear();
            st.selection.text = None;
            st.selection.cells = None;
        }
        let result = self.session.execute(cmd, params).map(|_| ()).map_err(|e| e.to_string());
        if view == Some(idx) {
            if let (Some(sel), Some(st)) = (saved, self.session.active_mut()) {
                st.selection = sel;
            }
        } else {
            self.restore_file_view(view);
        }
        result
    }

    fn restore_file_view(&mut self, view: Option<usize>) {
        if let Some(i) = view
            && i < self.session.documents().len()
        {
            self.session.set_active(i);
        }
    }

    fn locate_place(&self, place: &FilePlace) -> Result<LocatedPlace, String> {
        let d = self.session.doc().map_err(|e| e.to_string())?;
        match place {
            FilePlace::Slide(id) => {
                let slide = d.doc.slide_index(*id).ok_or_else(|| "the slide it was chosen for was deleted".to_string())?;
                Ok(LocatedPlace { target: Target::Slides, slide: Some(slide) })
            }
            FilePlace::Master(id) => {
                let master =
                    d.doc.masters.iter().position(|m| m.id == *id).ok_or_else(|| "the slide master it was chosen for was deleted".to_string())?;
                Ok(LocatedPlace { target: Target::Master { master }, slide: None })
            }
            FilePlace::Layout { master, layout } => {
                let mi =
                    d.doc.masters.iter().position(|m| m.id == *master).ok_or_else(|| "the slide master it was chosen for was deleted".to_string())?;
                let li = d
                    .doc
                    .masters
                    .get(mi)
                    .and_then(|m| m.layouts.iter().position(|l| l.id == *layout))
                    .ok_or_else(|| "the layout it was chosen for was deleted".to_string())?;
                Ok(LocatedPlace { target: Target::Layout { master: mi, layout: li }, slide: None })
            }
            FilePlace::NotesPage(t) => Ok(LocatedPlace { target: *t, slide: None }),
            FilePlace::Unspecified => Err("there was no slide to put it on".into()),
        }
    }
}

/// Images for the browser download: the current slide, or every slide (named `<name>1.png`,
/// `<name>2.png`... like the native export) when `all`. `transparent` leaves the slide
/// background out (PNG only, #784).
pub(crate) fn web_image_files(
    doc: &deckcraft_model::Presentation,
    current: usize,
    all: bool,
    name: &str,
    ext: &str,
    transparent: bool,
) -> Vec<(String, Vec<u8>)> {
    let slides: Vec<usize> = if all { (0..doc.slides.len()).collect() } else { vec![current] };
    let single = slides.len() == 1;
    let scale = 2.0;
    let dpi = deckcraft_render::dpi_for_scale(scale);
    slides
        .into_iter()
        .enumerate()
        .map(|(k, i)| {
            let opts = deckcraft_render::RenderOpts { scale, transparent_background: transparent && ext == "png", ..Default::default() };
            let img = deckcraft_render::render_slide(doc, i, &opts);
            let bytes = if ext == "png" { img.to_png_at(dpi) } else { img.to_jpeg_at(92, Some(dpi)) };
            let file = if single { format!("{name}.{ext}") } else { format!("{name}{}.{ext}", k + 1) };
            (file, bytes)
        })
        .collect()
}

#[cfg(test)]
mod print_preview_tests {
    use super::PrintPreviews;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn same_title_gets_distinct_private_paths_and_drop_cleans_them() {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or_default();
        let base = std::env::temp_dir().join(format!("deckcraft-print-test-{}-{stamp}", std::process::id()));
        std::fs::create_dir(&base).unwrap();
        let (first, second) = {
            let mut previews = PrintPreviews::default();
            let first = previews.create_in(&base, "Same title").unwrap();
            let second = previews.create_in(&base, "Same title").unwrap();
            assert_ne!(first, second);
            assert_eq!(first.file_name(), second.file_name());
            assert!(first.parent().is_some_and(|dir| dir.is_dir()));
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = std::fs::metadata(first.parent().unwrap()).unwrap().permissions().mode();
                assert_eq!(mode & 0o777, 0o700);
            }
            (first, second)
        };
        assert!(!first.parent().is_some_and(|dir| dir.exists()));
        assert!(!second.parent().is_some_and(|dir| dir.exists()));
        std::fs::remove_dir(base).unwrap();
    }

    #[test]
    fn explicit_cleanup_removes_previews() {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or_default();
        let base = std::env::temp_dir().join(format!("deckcraft-print-cleanup-test-{}-{stamp}", std::process::id()));
        std::fs::create_dir(&base).unwrap();
        let mut previews = PrintPreviews::default();
        let path = previews.create_in(&base, "Explicit cleanup").unwrap();
        let dir = path.parent().unwrap().to_path_buf();

        previews.cleanup();

        assert!(!dir.exists());
        std::fs::remove_dir(base).unwrap();
    }
}
