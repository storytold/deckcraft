//! Screen recording in the editor: the Record tab's Screen Recording button, the target/region
//! picker and the dark control window.
//!
//! The capture device is the host's (`Services::screen_in`); the recording itself lives in the
//! session ([`deckcraft_engine::screen`]), so the control window only drives `show.screenRecord`
//! and reads its state. Errors are the command's one message; nothing here panics.

use deckcraft_screen::CaptureStatus;
use egui::{Align2, Color32, RichText, Vec2};

use crate::SlideApp;

/// How long the Record view trusts a cached capture status, in seconds.
const STATUS_TTL: f64 = 1.0;

/// The screen-capture status the Record view last read, and when it read it.
///
/// `ScreenState::status` asks the OS (`scap::is_supported` and `scap::has_permission`), and the
/// Record view paints several times a second, so the answer is kept for [`STATUS_TTL`] instead of
/// being asked on every frame. A take that starts or stops invalidates it, since those are the
/// moments the host's answer can change.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ScreenStatusCache {
    status: CaptureStatus,
    /// The UI time the value was read at; [`f64::NEG_INFINITY`] means "never read".
    read: f64,
}

impl Default for ScreenStatusCache {
    fn default() -> Self {
        ScreenStatusCache { status: CaptureStatus::Unsupported, read: f64::NEG_INFINITY }
    }
}

impl ScreenStatusCache {
    /// The cached status, re-read from `status` when it is older than [`STATUS_TTL`].
    pub(crate) fn read(&mut self, now: f64, status: impl FnOnce() -> CaptureStatus) -> CaptureStatus {
        // A NaN age (either clock is not a real time) counts as stale: re-read rather than guess.
        if now - self.read < STATUS_TTL {
            return self.status;
        }
        self.status = status();
        self.read = now;
        self.status
    }

    /// Forget the cached value: the next [`read`](Self::read) asks the OS again.
    pub(crate) fn invalidate(&mut self) {
        self.read = f64::NEG_INFINITY;
    }
}

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
    /// The host picks the capture source itself (Linux: `scap` lists no targets and chooses when
    /// the recorder is built), so the picker can start without a listed target.
    implicit: bool,
    /// The recording window is collapsed to a small status bar (the take keeps running).
    compact: bool,
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
            implicit: false,
            compact: false,
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
            app.screen.implicit = v.get("implicit").and_then(serde_json::Value::as_bool).unwrap_or(false);
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
        // Ids are only unique within a kind: a display and a window can share one.
        p["targetKind"] = serde_json::json!(t.kind);
    }
    if !app.screen.full {
        let [x, y, width, height] = app.screen.region;
        p["region"] = serde_json::json!({"x": x, "y": y, "width": width, "height": height});
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

/// Whether the picker can start a take: the host is ready and either lists a target or chooses the
/// source itself (`implicit`, as on Linux where `scap` lists none).
fn can_record(status: &str, targets: usize, implicit: bool) -> bool {
    status == "ready" && (targets > 0 || implicit)
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
    let mut refresh = false;
    // The status bar is not drawn during a slide show: a message it would carry (the path of a
    // take that could not be inserted, for instance) belongs here too, and it has to expire like
    // the status bar's does. `fresh_status` needs `&mut app`, so read it before the closure holds
    // the borrow.
    let status = app.fresh_status();
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
            if !app.screen.compact {
                ui.set_min_width(300.0);
            }
            ui.visuals_mut().override_text_color = Some(text);
            ui.horizontal(|ui| {
                let dot = if recording { accent } else { Color32::from_gray(120) };
                ui.label(RichText::new("●").color(dot).size(16.0));
                ui.label(RichText::new("Screen Recording").strong().size(15.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if !recording {
                        let close_button = ui.small_button("✕");
                        if close_button.clicked() {
                            close = true;
                        }
                        close_button.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Close"));
                        close_button.on_hover_text("Close the screen recording window");
                    }
                });
            });
            ui.add_space(6.0);
            if recording {
                if app.screen.compact {
                    // A small status bar: the take keeps running, the window stops taking space.
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("●").color(accent).size(16.0));
                        ui.label(RichText::new(clock(app.session.screen.elapsed_ms())).monospace().size(18.0));
                        if ui.add(egui::Button::new(RichText::new("Stop").color(Color32::WHITE)).fill(accent)).clicked() {
                            stop_now = true;
                        }
                        let expand = ui.small_button("▸");
                        expand.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Show the recording controls"));
                        if expand.on_hover_text("Show the recording controls").clicked() {
                            app.screen.compact = false;
                        }
                    });
                } else {
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
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let collapse = ui.small_button("▾");
                            collapse.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Collapse to a status bar"));
                            if collapse.on_hover_text("Collapse to a status bar").clicked() {
                                app.screen.compact = true;
                            }
                        });
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
                }
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
                        // A host that picks its own source has nothing to list but can record: ask
                        // for the choice rather than reporting that there is no screen.
                        let note = if app.screen.implicit { "select a screen when recording" } else { "no screen found" };
                        ui.label(RichText::new(note).color(Color32::from_gray(170)));
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
                    // The list is read when the picker opens and after Grant permission; a display
                    // plugged in (or a window opened) since then needs a way to appear.
                    if ui.small_button("Refresh").on_hover_text("List displays and windows again").clicked() {
                        refresh = true;
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
                                // The origin and the size are clamped together: x + w (or y + h)
                                // may not run past the target, or the recorder clips the region
                                // below 64 px and Start fails.
                                if known != 0 {
                                    let span = i64::from(known).max(64);
                                    let (origin, size) = if along_width { (0, 2) } else { (1, 3) };
                                    app.screen.region[origin] = app.screen.region[origin].clamp(0, (span - 64).max(0));
                                    let room = span - app.screen.region[origin];
                                    app.screen.region[size] = app.screen.region[size].clamp(64, room.max(64));
                                }
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
                    let quality = ui.add(egui::Slider::new(&mut app.screen.quality, 10..=51));
                    quality.on_hover_text("Quality: 10 is the sharpest and largest file, 51 the smallest; lower values are better");
                });
                ui.add_space(8.0);
                let ready = can_record(&app.screen.status, app.screen.targets.len(), app.screen.implicit);
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
            if let Some(status) = &status
                && !status.is_empty()
            {
                ui.add_space(6.0);
                ui.label(RichText::new(status).size(11.0).color(Color32::from_rgb(240, 200, 120)));
            }
        });
    if close {
        open = false;
    }
    if refresh {
        reload(app);
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
            // The encoder writes the file on its thread; the take is inserted on the tick that
            // receives it (its message says so).
            open = false;
            app.notify("Finishing the screen recording…");
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

#[cfg(test)]
mod tests {
    use super::can_record;

    /// A control-channel click is automation, not the window: the pass that carries its queued
    /// events runs with `ui_caller` false, so the picker's own Record path must be refused — a
    /// click cannot start screen capture — while the window's caller reaches the capture path and
    /// fails only on the device.
    #[test]
    fn a_control_click_cannot_start_a_screen_recording_from_the_picker() {
        let mut app = crate::SlideApp::new(deckcraft_engine::Session::with_new(), crate::Services::default());
        app.session.execute("slide.new", &serde_json::json!({})).expect("slide");
        // What `SlideApp::ui` sets while the control channel's synthetic batch is the live input.
        app.session.set_ui_caller(false);
        super::start(&mut app);
        let why = app.screen.error.clone().unwrap_or_default();
        assert!(why.contains("only available from the DeckCraft window"), "the control click is refused as automation: {why:?}");
        assert!(!app.session.screen.is_recording(), "a control click does not record the screen");
        // The window's own click is the window: the gate is not the capture path's only answer.
        app.screen.error = None;
        app.session.set_ui_caller(true);
        super::start(&mut app);
        let mine = app.screen.error.clone().unwrap_or_default();
        assert!(!mine.contains("only available from the DeckCraft window"), "the window's own click passes the gate: {mine:?}");
    }

    /// A host that picks its own source lists no targets and still records: the picker must not
    /// disable Record for a missing list (Linux, where `scap` chooses when the recorder is built).
    #[test]
    fn the_picker_starts_without_a_listed_target_when_the_host_picks() {
        assert!(can_record("ready", 0, true), "a self-picking host needs no target");
        assert!(!can_record("ready", 0, false), "a host with neither a target nor a choice cannot record");
        assert!(can_record("ready", 1, false), "a listed target is enough");
        assert!(!can_record("unavailable", 1, true), "an unsupported host stays unavailable");
        assert!(!can_record("permissionDenied", 0, true), "so does one without permission");
    }
}
