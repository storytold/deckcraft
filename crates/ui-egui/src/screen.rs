//! Screen recording in the editor: the Record tab's Screen Recording button, the target/region
//! picker and the dark control window.
//!
//! The capture device is the host's (`Services::screen_in`); the recording itself lives in the
//! session ([`deckcraft_engine::screen`]), so the control window only drives `show.screenRecord`
//! and reads its state. Errors are the command's one message; nothing here panics.

use egui::{Align2, Color32, RichText, Vec2};

use crate::SlideApp;

/// One capturable display or window.
#[derive(Clone, Debug)]
struct TargetRow {
    id: u64,
    kind: String,
    title: String,
    width: u32,
    height: u32,
}

impl TargetRow {
    fn label(&self) -> String {
        if self.title.trim().is_empty() { format!("{} {}", self.kind, self.id) } else { format!("{} — {}", self.kind, self.title) }
    }
}

/// The picker and control window's own state.
pub struct ScreenUi {
    /// The window is open (picking or controlling a recording).
    pub open: bool,
    targets: Vec<TargetRow>,
    selected: usize,
    /// Record the whole target rather than `region`.
    full: bool,
    region: [i64; 4],
    fps: u32,
    cursor: bool,
    quality: u8,
    status: String,
    error: Option<String>,
}

impl Default for ScreenUi {
    fn default() -> Self {
        ScreenUi {
            open: false,
            targets: Vec::new(),
            selected: 0,
            full: true,
            region: [0, 0, 64, 64],
            fps: 30,
            cursor: true,
            quality: 24,
            status: String::new(),
            error: None,
        }
    }
}

/// The Record tab's button: open the picker on the host's targets.
pub fn pick(app: &mut SlideApp) {
    app.screen.open = true;
    app.screen.error = None;
    reload(app);
}

fn reload(app: &mut SlideApp) {
    match app.run("show.screenRecord", serde_json::json!({"action": "targets"})) {
        Ok(v) => {
            app.screen.status = v.get("status").and_then(|s| s.as_str()).unwrap_or("").to_string();
            app.screen.targets = v
                .get("targets")
                .and_then(|t| t.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|t| {
                            Some(TargetRow {
                                id: t.get("id")?.as_u64()?,
                                kind: t.get("kind").and_then(|k| k.as_str()).unwrap_or("target").to_string(),
                                title: t.get("title").and_then(|k| k.as_str()).unwrap_or("").to_string(),
                                width: t.get("width").and_then(|k| k.as_u64()).unwrap_or(0) as u32,
                                height: t.get("height").and_then(|k| k.as_u64()).unwrap_or(0) as u32,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            let n = app.screen.targets.len();
            app.screen.selected = app.screen.selected.min(n.saturating_sub(1));
            if let Some(t) = app.screen.targets.get(app.screen.selected) {
                app.screen.region = [0, 0, i64::from(t.width).max(64), i64::from(t.height).max(64)];
            }
            app.screen.error = None;
        }
        Err(e) => {
            app.screen.status.clear();
            app.screen.targets.clear();
            app.screen.error = Some(e);
        }
    }
}

fn start(app: &mut SlideApp) {
    let mut p = serde_json::json!({
        "action": "start",
        "fps": app.screen.fps.max(1),
        "cursor": app.screen.cursor,
        "quality": app.screen.quality.clamp(10, 51),
    });
    if let Some(t) = app.screen.targets.get(app.screen.selected) {
        p["target"] = serde_json::json!(t.id);
    }
    if !app.screen.full {
        p["region"] = serde_json::json!(app.screen.region);
    }
    match app.run("show.screenRecord", p) {
        Ok(_) => app.screen.error = None,
        Err(e) => app.screen.error = Some(e),
    }
}

fn command(app: &mut SlideApp, action: &str) -> bool {
    match app.run("show.screenRecord", serde_json::json!({"action": action})) {
        Ok(_) => {
            app.screen.error = None;
            true
        }
        Err(e) => {
            app.screen.error = Some(e);
            false
        }
    }
}

fn clock(ms: u64) -> String {
    let s = ms / 1000;
    format!("{:02}:{:02}", (s / 60).min(9999), s % 60)
}

/// Draw the picker or the live control. Called once per UI pass, after the dialogs.
#[allow(deprecated)] // `Id::new` is the stable way to give a window one identity across passes.
pub fn window(app: &mut SlideApp, ctx: &egui::Context) {
    let recording = app.session.screen.is_recording();
    if !app.screen.open && !recording {
        return;
    }
    let mut open = app.screen.open || recording;
    let dark = Color32::from_rgb(22, 22, 26);
    let text = Color32::from_rgb(228, 228, 232);
    let accent = Color32::from_rgb(232, 72, 72);
    let mut begin = false;
    let mut pause_now = false;
    let mut resume_now = false;
    let mut stop_now = false;
    let mut cancel_now = false;
    let mut permission = false;
    let mut close = false;
    egui::Window::new("Screen Recording")
        .id(egui::Id::new("deckcraft-screen-recording"))
        .anchor(Align2::RIGHT_BOTTOM, Vec2::new(-28.0, -28.0))
        .collapsible(false)
        .resizable(false)
        .title_bar(false)
        .open(&mut open)
        .frame(
            egui::Frame::window(ctx.style_of(egui::Theme::Dark).as_ref())
                .fill(dark)
                .inner_margin(egui::Margin::same(16))
                .corner_radius(egui::CornerRadius::same(10)),
        )
        .show(ctx, |ui| {
            ui.set_min_width(300.0);
            ui.visuals_mut().override_text_color = Some(text);
            ui.horizontal(|ui| {
                let dot = if recording { accent } else { Color32::from_gray(120) };
                ui.label(RichText::new("●").color(dot).size(16.0));
                ui.label(RichText::new("Screen Recording").strong().size(15.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if !recording && ui.small_button("✕").clicked() {
                        close = true;
                    }
                });
            });
            ui.add_space(6.0);
            if recording {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(clock(app.session.screen.elapsed_ms())).monospace().size(26.0));
                    ui.add_space(8.0);
                    if app.session.screen.is_paused() {
                        if ui.button("Resume").clicked() {
                            resume_now = true;
                        }
                    } else if ui.button("Pause").clicked() {
                        pause_now = true;
                    }
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new(RichText::new("Stop and insert").color(Color32::WHITE)).fill(accent)).clicked() {
                        stop_now = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel_now = true;
                    }
                });
                ui.add_space(4.0);
                ui.label(RichText::new(format!("{} frames", app.session.screen.frames())).size(11.0).color(Color32::from_gray(170)));
            } else {
                if app.screen.status == "permissionDenied" {
                    ui.label(RichText::new("DeckCraft needs permission to record the screen.").size(12.0));
                    if ui.button("Grant permission…").clicked() {
                        permission = true;
                    }
                    ui.add_space(6.0);
                }
                ui.horizontal(|ui| {
                    ui.label("Record");
                    if app.screen.targets.is_empty() {
                        ui.label(RichText::new("no screen found").color(Color32::from_gray(170)));
                    } else {
                        let current = app.screen.targets.get(app.screen.selected).map(TargetRow::label).unwrap_or_default();
                        craft_ui::ComboBox::from_id_salt("screen-target").selected_text(current).show_ui(ui, |ui| {
                            for (i, t) in app.screen.targets.iter().enumerate() {
                                if ui.selectable_label(app.screen.selected == i, t.label()).clicked() {
                                    app.screen.selected = i;
                                    app.screen.region = [0, 0, i64::from(t.width).max(64), i64::from(t.height).max(64)];
                                }
                            }
                        });
                    }
                });
                ui.checkbox(&mut app.screen.full, "Full screen");
                ui.add_enabled_ui(!app.screen.full, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Region");
                        // The region must stay inside the picked target: an origin past its edge
                        // makes the recorder reject every frame.
                        let size = app.screen.targets.get(app.screen.selected).map(|t| (t.width, t.height)).unwrap_or((0, 0));
                        for (label, i) in [("x", 0), ("y", 1), ("w", 2), ("h", 3)] {
                            ui.label(label);
                            let mut v = app.screen.region[i];
                            // `x` and `w` are measured across the target's width, `y` and `h`
                            // down its height; the size fields start at the 64 px minimum. A host
                            // that cannot report a target's size (0) leaves the region free — the
                            // recorder clips it to the frame anyway.
                            let along_width = matches!(i, 0 | 2);
                            let known = if along_width { size.0 } else { size.1 };
                            let (lo, hi) = if known == 0 {
                                if i < 2 { (0, 100_000) } else { (64, 100_000) }
                            } else {
                                let span = i64::from(known).max(64);
                                if i < 2 { (0, (span - 64).max(0)) } else { (64, span) }
                            };
                            if ui.add(egui::DragValue::new(&mut v).range(lo..=hi).speed(1.0)).changed() {
                                app.screen.region[i] = v.clamp(lo, hi);
                            }
                        }
                    });
                });
                ui.horizontal(|ui| {
                    ui.label("Frame rate");
                    ui.add(egui::Slider::new(&mut app.screen.fps, 10..=60).suffix(" fps"));
                });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut app.screen.cursor, "Pointer");
                    ui.add_space(10.0);
                    ui.label("Quality");
                    ui.add(egui::Slider::new(&mut app.screen.quality, 10..=51));
                });
                ui.add_space(8.0);
                let ready = app.screen.status == "ready" && !app.screen.targets.is_empty();
                if ui.add_enabled(ready, egui::Button::new(RichText::new("Record").color(Color32::WHITE)).fill(accent)).clicked() {
                    begin = true;
                }
                if !ready && !app.screen.status.is_empty() && app.screen.status != "ready" {
                    let why = if app.screen.status == "permissionDenied" { "permission is needed" } else { "screen capture is unavailable here" };
                    ui.label(RichText::new(why).size(11.0).color(Color32::from_gray(170)));
                }
            }
            if let Some(e) = &app.screen.error {
                ui.add_space(6.0);
                ui.label(RichText::new(e).size(11.0).color(Color32::from_rgb(255, 140, 140)));
            }
            // The status bar is not drawn during a slide show: a message it would carry (the path
            // of a take that could not be inserted, for instance) belongs here too.
            if let Some((text, _)) = &app.status
                && !text.is_empty()
            {
                ui.add_space(6.0);
                ui.label(RichText::new(text).size(11.0).color(Color32::from_rgb(240, 200, 120)));
            }
        });
    if close {
        open = false;
    }
    if permission {
        command(app, "permission");
        reload(app);
    }
    if begin {
        start(app);
    }
    if pause_now {
        command(app, "pause");
    }
    if resume_now {
        command(app, "resume");
    }
    if stop_now {
        let ok = command(app, "stop");
        if ok {
            open = false;
            app.notify("Screen recording inserted on the current slide");
        }
    }
    if cancel_now {
        command(app, "cancel");
        open = false;
    }
    app.screen.open = open || app.session.screen.is_recording();
    if app.session.screen.is_recording() {
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }
}
