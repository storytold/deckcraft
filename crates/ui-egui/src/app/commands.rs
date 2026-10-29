#[cfg(not(target_arch = "wasm32"))]
use super::updates_ui;
use super::{SlideApp, UI_COMMANDS, Value, ViewMode, backstage, dialogs, doc_window, json, media, qat, theme};

fn grid_options_touched(p: &Value) -> bool {
    ["spacing", "snap", "smartGuides", "gridlines", "guides"].iter().any(|k| p.get(*k).is_some())
}

fn grid_flag(p: &Value, key: &str) -> Result<Option<bool>, String> {
    match p.get(key) {
        None => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(format!("`{key}` must be true or false")),
    }
}

impl SlideApp {
    /// Run a command by id (UI commands here, everything else in the engine). Errors show in the
    /// status bar and are returned.
    pub fn run(&mut self, id: &str, params: Value) -> Result<Value, String> {
        // An arrow key next to an equation goes into it (instead of stepping over it).
        if id == "text.move"
            && let Some(r) = crate::equation_editor::enter_by_arrow(self, &params)
        {
            return r;
        }
        let r = if UI_COMMANDS.iter().any(|c| c.0 == id) {
            self.run_ui(id, &params)
        } else {
            self.session.execute(id, &params).map_err(|e| e.to_string())
        };
        match &r {
            // Copy and Cut from the ribbon, menus, the slide's context menu or the control channel
            // put the text on the system clipboard, as Ctrl+C and Ctrl+X do (#431).
            Ok(v) if matches!(id, "edit.copy" | "edit.cut") => {
                if let Some(t) = v.get("text").and_then(Value::as_str) {
                    self.pending_copy = Some(t.to_string());
                }
            }
            Ok(_) => {}
            Err(e) => self.set_status(e.clone()),
        }
        self.drain_requests();
        r
    }

    fn flag(&mut self, params: &Value, cur: bool) -> bool {
        params.get("on").and_then(Value::as_bool).unwrap_or(!cur)
    }

    fn run_ui(&mut self, id: &str, p: &Value) -> Result<Value, String> {
        match id {
            "view.normal" => self.ui.view = ViewMode::Normal,
            "view.outline" => self.ui.view = ViewMode::Outline,
            "view.sorter" => self.ui.view = ViewMode::Sorter,
            "view.notesPage" => self.ui.view = ViewMode::NotesPage,
            "view.reading" => self.start_show(self.session.active().map(|d| d.selection.slide).unwrap_or(0), true),
            "view.notes" => self.ui.notes = self.flag(p, self.ui.notes),
            "view.ruler" => self.ui.ruler = self.flag(p, self.ui.ruler),
            "view.gridlines" => self.ui.gridlines = self.flag(p, self.ui.gridlines),
            "view.guides" => {
                self.ui.guides = self.flag(p, self.ui.guides);
                // A presentation without guides gets PowerPoint's two centre guides.
                if self.ui.guides && self.session.active().is_some_and(|d| d.doc.guides.is_empty()) {
                    let _ = self.session.execute("guide.add", &json!({"defaults": true}));
                }
            }
            "view.gridOptions" => {
                if grid_options_touched(p) {
                    let spacing = match p.get("spacing") {
                        None => None,
                        Some(raw) => {
                            let n = raw.as_f64().filter(|v| v.is_finite()).ok_or("`spacing` must be a finite number of points")?;
                            if !(1.0..=288.0).contains(&n) {
                                return Err("`spacing` must be from 1 to 288 points".into());
                            }
                            Some(n)
                        }
                    };
                    let snap = grid_flag(p, "snap")?;
                    let smart = grid_flag(p, "smartGuides")?;
                    let gridlines = grid_flag(p, "gridlines")?;
                    let guides = grid_flag(p, "guides")?;
                    if let Some(n) = spacing {
                        self.session.prefs.grid_spacing = n;
                    }
                    if let Some(b) = snap {
                        self.session.prefs.snap_to_grid = b;
                    }
                    if let Some(b) = smart {
                        self.session.prefs.smart_guides = b;
                        self.ui.smart_guides = b;
                    }
                    if let Some(b) = gridlines {
                        self.ui.gridlines = b;
                    }
                    if let Some(b) = guides {
                        self.ui.guides = b;
                        // As View › Guides: a presentation without guides gets the two centre guides.
                        if b && self.session.active().is_some_and(|d| d.doc.guides.is_empty()) {
                            let _ = self.session.execute("guide.add", &json!({"defaults": true}));
                        }
                    }
                } else {
                    self.dialog = Some(dialogs::Dialog::new("gridOptions"));
                }
                return Ok(json!({
                    "spacing": self.session.prefs.grid_spacing,
                    "snap": self.session.prefs.snap_to_grid,
                    "smartGuides": self.session.prefs.smart_guides,
                    "gridlines": self.ui.gridlines,
                    "guides": self.ui.guides,
                }));
            }
            "view.zoomIn" | "view.zoomOut" => {
                let cur = self.ui.zoom.unwrap_or(self.canvas_scale);
                let steps = [0.1, 0.25, 0.33, 0.5, 0.66, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0, 4.0];
                let next = if id == "view.zoomIn" {
                    steps.iter().copied().find(|s| *s > cur + 0.01).unwrap_or(4.0)
                } else {
                    steps.iter().rev().copied().find(|s| *s < cur - 0.01).unwrap_or(0.1)
                };
                self.ui.zoom = Some(next);
            }
            "view.zoom" => {
                let pct = p.get("percent").and_then(Value::as_f64).ok_or("missing `percent`")?;
                self.ui.zoom = Some((pct as f32 / 100.0).clamp(0.1, 4.0));
            }
            "view.zoomDialog" => self.dialog = Some(dialogs::Dialog::new("zoom")),
            "view.fit" => self.ui.zoom = None,
            "review.thesaurusPane" => {
                let word = match p.get("word").and_then(Value::as_str) {
                    Some(w) => w.to_string(),
                    None => self
                        .session
                        .execute("review.thesaurus", &json!({}))
                        .ok()
                        .and_then(|r| r["word"].as_str().map(String::from))
                        .unwrap_or_default(),
                };
                self.thesaurus_query = word;
                self.ui.pane = Some("thesaurus".into());
            }
            "view.pane" => {
                let pane = p.get("pane").and_then(Value::as_str).map(String::from);
                self.ui.pane =
                    if pane.is_some() && pane == self.ui.pane && p.get("toggle").and_then(Value::as_bool).unwrap_or(false) { None } else { pane };
                if let Some(tab) = p.get("tab").and_then(Value::as_str) {
                    self.ui.format_tab = tab.to_string();
                }
            }
            "view.tab" => {
                let tab = p.get("tab").and_then(Value::as_str).ok_or("missing `tab`")?;
                self.ui.tab = tab.to_string();
                self.ui.ribbon_collapsed = false;
            }
            "view.collapseRibbon" => self.ui.ribbon_collapsed = !self.ui.ribbon_collapsed,
            "view.dark" => {
                let on = self.flag(p, self.ui.brightness == theme::Brightness::Dark);
                self.ui.brightness = if on { theme::Brightness::Dark } else { theme::Brightness::Light };
                self.restyle = true;
            }
            "view.grayscale" => self.ui.grayscale = self.flag(p, self.ui.grayscale),
            "view.newWindow" => return doc_window::command(self, p),
            "show.start" => {
                if let Some(name) = p.get("custom").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                    self.start_custom_show(name, false);
                } else {
                    let from = self.show_from(p)?.unwrap_or_else(|| self.show_start_index());
                    self.start_show(from, false);
                }
            }
            "show.presenter" => self.ui.presenter_view = self.flag(p, self.ui.presenter_view),
            "show.end" => {
                self.show = None;
                self.media.stop_owned(media::Owner::Show);
            }
            "animation.preview" => {
                if p.get("on").and_then(Value::as_bool) == Some(false) {
                    self.anim_preview = None;
                    return Ok(json!({"playing": false}));
                }
                let from = match p.get("from") {
                    None | Some(Value::Null) => None,
                    Some(v) => Some(v.as_u64().map(|v| usize::try_from(v).unwrap_or(usize::MAX)).ok_or("`from` must be an effect index")?),
                };
                return self.start_anim_preview(from);
            }
            "app.home" => {
                let on = self.flag(p, self.backstage_open());
                if on {
                    self.open_backstage(backstage::Page::Home);
                } else {
                    self.close_backstage();
                }
            }
            "app.backstage" => {
                if !p.get("on").and_then(Value::as_bool).unwrap_or(true) {
                    self.close_backstage();
                    return Ok(json!({"page": self.backstage.as_ref().map(|b| b.page.id())}));
                }
                let page = match p.get("page").and_then(Value::as_str) {
                    Some(id) => backstage::Page::parse(id).ok_or_else(|| format!("unknown File page `{id}`"))?,
                    None => backstage::Page::Home,
                };
                if page.needs_doc() && self.session.active().is_none() {
                    return Err(format!("File › {} needs an open presentation", page.label()));
                }
                self.open_backstage(page);
                return Ok(json!({"page": page.id()}));
            }
            "app.closePresentation" => self.close_presentation(),
            "app.print" => {
                if self.session.active().is_none() {
                    return Err("nothing to print: no presentation is open".into());
                }
                return Ok(json!({"printed": self.print_pdf(p.clone())}));
            }
            "app.about" => self.dialog = Some(dialogs::Dialog::new("about")),
            "app.checkUpdates" => {
                #[cfg(not(target_arch = "wasm32"))]
                updates_ui::request_check(self);
                #[cfg(target_arch = "wasm32")]
                return Err("updates are handled by the browser: reload the page".into());
            }
            "app.palette" => self.palette = Some((String::new(), 0)),
            "app.preferences" => self.dialog = Some(dialogs::Dialog::new("preferences")),
            "app.openDialog" => self.pick_and_open(),
            "app.insertPictureDialog" => self.pick_and_insert("picture", "insert.picture"),
            "app.smartArtPictureDialog" => {
                if self.services.pick_open.is_none() {
                    return Err("picking a SmartArt picture needs the desktop app".into());
                }
                self.pick_and_run("picture", "smartArt.picture", p);
            }
            "app.changePictureDialog" => self.pick_and_insert("picture", "picture.change"),
            "app.insertAudioDialog" => self.pick_and_insert("audio", "insert.audio"),
            "app.reuseSlidesDialog" => self.pick_and_reuse(p.get("keepFormatting").and_then(Value::as_bool).unwrap_or(false)),
            "app.insertVideoDialog" => self.pick_and_insert("video", "insert.video"),
            "app.saveAsDialog" => self.save_as_dialog("deckcraft"),
            "app.exportDialog" => self.dialog = Some(dialogs::Dialog::new("export")),
            "app.pasteSpecial" => self.dialog = Some(dialogs::Dialog::new("pasteSpecial")),
            "app.browseThemeDialog" => {
                if self.session.active().is_none() {
                    return Err("open a presentation first".into());
                }
                if self.services.pick_open.is_some() {
                    self.pick_and_insert("theme", "design.browseTheme");
                } else {
                    // The browser's asynchronous picker would open the file as a new document.
                    self.set_status("Browse for Themes needs the desktop app for now".to_string());
                }
            }
            "app.qat" => return qat::command(self, p),
            "app.find" | "app.replace" if self.session.active().is_some() => {
                let mut d = dialogs::Dialog::new("find");
                d.fields.insert("replace".into(), if id == "app.replace" { "1" } else { "0" }.into());
                self.dialog = Some(d);
            }
            "app.find" | "app.replace" => self.palette = Some((String::new(), 0)),
            "app.compareDialog" => {
                if self.session.active().is_none() {
                    return Err("open the presentation to compare first".into());
                }
                if self.services.pick_open.is_some() {
                    self.pick_and_insert("open", "review.compare");
                } else {
                    // The browser's asynchronous picker would open the file as a new document.
                    self.set_status("Compare needs the desktop app for now".to_string());
                }
            }
            "app.dialog" => {
                let d = p.get("id").and_then(Value::as_str).ok_or("missing `id`")?;
                self.dialog = Some(dialogs::Dialog::new(d));
            }
            "app.links" => {
                use deckcraft_engine::links::{APP_PAGE, APPS, DISCORD, GITHUB, WEBSITE};
                return Ok(json!({"discord": DISCORD, "website": WEBSITE, "apps": APPS, "appPage": APP_PAGE, "github": GITHUB}));
            }
            _ => return Err(format!("unknown UI command `{id}`")),
        }
        Ok(Value::Null)
    }
}
