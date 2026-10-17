//! Office add-ins' task panes: the right-hand panel an Office add-in's page shows in, and the
//! messages between that page and the presentation.
//!
//! The page itself is a web view the host draws over the panel ([`OfficeHost`], the desktop
//! app's `wry` web view; none in tests or where there's no web view). The page runs DeckCraft's
//! Office.js (`crates/office-js`), which sends each `context.sync()` as
//! `{"id": n, "actions": [...]}`; [`pump`] runs it with `office.sync` and answers
//! `{"id": n, "results"|"error": …}`. Native web views are drawn above the window's content, so
//! the page is hidden while the File tab or a dialog is open.
//!
//! The library also sends events: `{"event": "ready"}` once it has started in a page (function
//! commands wait for it), and `{"event": "functionDone", "call": n, "error"?}` when a function
//! command's function calls `event.completed()` (or fails). The host calls into the page with
//! `__craftOfficeRunFunction({"call", "name", "control"})` and `__craftOfficeEvent({"type"})`
//! (`documentSelectionChanged`).

use egui::Ui;
use serde_json::{Value as Json, json};

use crate::SlideApp;
use crate::theme::{self, Tokens};

/// Shows add-in pages: implemented by the desktop app with its web view.
pub trait OfficeHost {
    /// Shows the page for `key` (loading `url` the first time) over `rect`, in points.
    fn show(&mut self, key: &str, url: &str, rect: egui::Rect, pixels_per_point: f32);
    /// Hides the page (it keeps its state).
    fn hide(&mut self, key: &str);
    /// Closes the page.
    fn close(&mut self, key: &str);
    /// Loads `url` for `key` without showing it (a function file's page, for `ExecuteFunction`
    /// commands); nothing if that page is already there.
    fn load_hidden(&mut self, key: &str, url: &str);
    /// The messages pages sent since the last call: (key, JSON).
    fn take_messages(&mut self) -> Vec<(String, String)>;
    /// Answers the page for `key` (`__craftOfficeReply(json)`).
    fn reply(&mut self, key: &str, json: &str);
    /// Calls the library's `globalThis.<entry>(json)` in the page for `key`
    /// (`__craftOfficeRunFunction`, `__craftOfficeEvent`).
    fn call(&mut self, key: &str, entry: &str, json: &str);
}

/// How long a function command may run before DeckCraft stops waiting for its
/// `event.completed()` and says so.
pub const FUNCTION_TIMEOUT_MS: f64 = 5.0 * 60.0 * 1000.0;

/// A function command waiting for its page, or for its `event.completed()`.
#[derive(Clone, Debug, PartialEq)]
pub struct FunctionCall {
    pub id: u64,
    /// The function page.
    pub key: String,
    pub plugin: String,
    pub function: String,
    /// The manifest's control id (`event.source.id`).
    pub control: String,
    /// When it was asked for, and when it was handed to the page (ms, [`crate::now_ms`]).
    pub asked_ms: f64,
    pub sent_ms: Option<f64>,
}

/// Most function commands waiting at once (more are refused).
const MAX_CALLS: usize = 100;

/// An open task pane.
#[derive(Clone, Debug, PartialEq)]
pub struct OfficePane {
    /// The add-in (plugin id).
    pub plugin: String,
    pub title: String,
    pub url: String,
    /// Identifies the page to the host: the add-in, and its pane id or page.
    pub key: String,
}

/// The add-ins' panes and function pages.
#[derive(Default)]
pub struct Panes {
    pub open: Vec<OfficePane>,
    /// Which of `open` shows.
    pub current: usize,
    /// Pages running a function command (no pane): key → add-in.
    pub function_pages: Vec<(String, String)>,
    /// Function commands not finished yet.
    pub calls: Vec<FunctionCall>,
    next_call: u64,
    /// Pages whose library has started (`{"event": "ready"}`).
    pub ready: std::collections::BTreeSet<String>,
    /// The selection last told to the pages (presentation, selection).
    selection: Option<(u64, deckcraft_engine::Selection)>,
    /// The page shown last frame, to hide it when it stops showing.
    shown: Option<String>,
}

fn key(plugin: &str, pane: Option<&str>, url: &str) -> String {
    format!("{plugin}#{}", pane.unwrap_or(url))
}

/// Opens (or brings forward) an add-in's task pane.
pub fn open(app: &mut SlideApp, plugin: &str, title: &str, url: &str, pane: Option<&str>) {
    let k = key(plugin, pane, url);
    match app.office.open.iter().position(|p| p.key == k) {
        Some(i) => {
            if let Some(p) = app.office.open.get_mut(i) {
                p.url = url.to_string();
            }
            app.office.current = i;
        }
        None => {
            app.office.open.push(OfficePane { plugin: plugin.into(), title: title.into(), url: url.into(), key: k });
            app.office.current = app.office.open.len() - 1;
        }
    }
}

/// Runs an add-in's function command in a page with no pane: loads its function file (hidden)
/// and, once the library there is ready, calls the function with `event.source.id` = `control`.
pub fn run_function(app: &mut SlideApp, plugin: &str, url: &str, function: &str, control: &str) {
    let k = key(plugin, Some("functions"), url);
    let Some(host) = app.services.office.as_mut() else {
        app.set_status("Office add-ins need the DeckCraft desktop app on macOS or Windows.");
        return;
    };
    if app.office.calls.len() >= MAX_CALLS {
        app.set_status(format!("{function}: too many add-in commands are still running"));
        return;
    }
    host.load_hidden(&k, url);
    if !app.office.function_pages.iter().any(|(fk, _)| *fk == k) {
        app.office.function_pages.push((k.clone(), plugin.to_string()));
    }
    app.office.next_call = app.office.next_call.wrapping_add(1);
    let id = app.office.next_call;
    app.office.calls.push(FunctionCall {
        id,
        key: k,
        plugin: plugin.into(),
        function: function.into(),
        control: control.into(),
        asked_ms: crate::now_ms(),
        sent_ms: None,
    });
    send_calls(app);
}

/// Hands waiting function calls to the pages that are ready.
fn send_calls(app: &mut SlideApp) {
    let Some(host) = app.services.office.as_mut() else { return };
    let now = crate::now_ms();
    for c in app.office.calls.iter_mut().filter(|c| c.sent_ms.is_none() && app.office.ready.contains(&c.key)) {
        let call = json!({"call": c.id, "name": c.function, "control": c.control}).to_string();
        host.call(&c.key, "__craftOfficeRunFunction", &call);
        c.sent_ms = Some(now);
    }
}

/// Stops waiting for function commands asked for more than [`FUNCTION_TIMEOUT_MS`] before
/// `now_ms`, and says which one didn't finish.
pub fn expire_calls(app: &mut SlideApp, now_ms: f64) {
    let (late, keep): (Vec<FunctionCall>, Vec<FunctionCall>) = app.office.calls.drain(..).partition(|c| now_ms - c.asked_ms > FUNCTION_TIMEOUT_MS);
    app.office.calls = keep;
    if let Some(c) = late.first() {
        let name = deckcraft_engine::plugin::get(&c.plugin).map(|p| p.manifest.name.clone()).unwrap_or_else(|| c.plugin.clone());
        let what = if c.sent_ms.is_some() { "didn't finish (it never called event.completed())" } else { "couldn't start (its page didn't load)" };
        app.set_status(format!("{name}: {} {what}", c.function));
    }
}

/// An event from a page's library (not a `context.sync()`): true if `msg` was one.
fn event(app: &mut SlideApp, key: &str, msg: &Json) -> bool {
    let Some(kind) = msg.get("event").and_then(Json::as_str) else { return false };
    match kind {
        "ready" => {
            app.office.ready.insert(key.to_string());
            send_calls(app);
        }
        "functionDone" => {
            let id = msg.get("call").and_then(Json::as_u64);
            if let Some(i) = app.office.calls.iter().position(|c| Some(c.id) == id && c.key == key) {
                let c = app.office.calls.remove(i);
                if let Some(e) = msg.get("error").and_then(Json::as_str) {
                    let e: String = e.chars().take(300).collect();
                    app.set_status(format!("{}: {e}", c.function));
                }
            }
        }
        _ => {}
    }
    true
}

/// Tells the ready pages that the selection changed (`Office.EventType.DocumentSelectionChanged`).
fn selection_events(app: &mut SlideApp) {
    let now = app.session.active().map(|d| (d.uid, d.selection.clone()));
    if now == app.office.selection {
        return;
    }
    let first = app.office.selection.is_none();
    app.office.selection = now;
    if first || app.office.ready.is_empty() {
        return;
    }
    let Some(host) = app.services.office.as_mut() else { return };
    let ev = json!({"type": "documentSelectionChanged"}).to_string();
    for k in &app.office.ready {
        host.call(k, "__craftOfficeEvent", &ev);
    }
}

/// Closes the pane at `i`.
fn close(app: &mut SlideApp, i: usize) {
    if i >= app.office.open.len() {
        return;
    }
    let pane = app.office.open.remove(i);
    if let Some(host) = app.services.office.as_mut() {
        host.close(&pane.key);
    }
    app.office.ready.remove(&pane.key);
    if app.office.shown.as_deref() == Some(pane.key.as_str()) {
        app.office.shown = None;
    }
    app.office.current = app.office.current.min(app.office.open.len().saturating_sub(1));
}

/// Answers the pages' messages (each `context.sync()` is one `office.sync`), takes their events,
/// tells them of selection changes, and gives up on function commands that never finished.
pub fn pump(app: &mut SlideApp) {
    let Some(host) = app.services.office.as_mut() else { return };
    let messages = host.take_messages();
    for (k, text) in messages {
        if let Ok(msg) = serde_json::from_str::<Json>(&text)
            && event(app, &k, &msg)
        {
            continue;
        }
        let plugin = app
            .office
            .open
            .iter()
            .find(|p| p.key == k)
            .map(|p| p.plugin.clone())
            .or_else(|| app.office.function_pages.iter().find(|(fk, _)| *fk == k).map(|(_, p)| p.clone()));
        let answer = answer(app, plugin.as_deref(), &text);
        if let Some(host) = app.services.office.as_mut() {
            host.reply(&k, &answer.to_string());
        }
    }
    selection_events(app);
    if !app.office.calls.is_empty() {
        expire_calls(app, crate::now_ms());
    }
}

/// The answer to one message from an add-in's page.
pub fn answer(app: &mut SlideApp, plugin: Option<&str>, text: &str) -> Json {
    let msg: Json = serde_json::from_str(text).unwrap_or(Json::Null);
    let id = msg.get("id").cloned().unwrap_or(Json::Null);
    let Some(plugin) = plugin else {
        return json!({"id": id, "error": {"code": "GeneralException", "message": "this page's add-in is closed"}});
    };
    let actions = msg.get("actions").cloned().unwrap_or(Json::Array(Vec::new()));
    let out = match app.session.execute("office.sync", &json!({"addin": plugin, "actions": actions})) {
        Ok(out) => out,
        Err(e) => json!({"error": {"code": "GeneralException", "message": e.to_string()}}),
    };
    let mut reply = json!({"id": id});
    if let (Some(r), Json::Object(o)) = (reply.as_object_mut(), out) {
        r.extend(o);
    }
    reply
}

/// The task pane: the add-in's title (a tab per open pane) and its page. Called before the slide
/// area is laid out, so the pane takes the right of the window.
pub fn show(app: &mut SlideApp, ui: &mut Ui) {
    // Add-ins that were unloaded or turned off take their panes with them.
    let gone: Vec<usize> = app
        .office
        .open
        .iter()
        .enumerate()
        .filter(|(_, p)| !deckcraft_engine::plugin::get(&p.plugin).is_some_and(|q| q.active()))
        .map(|(i, _)| i)
        .collect();
    for i in gone.into_iter().rev() {
        close(app, i);
    }
    let covered = app.backstage.is_some() || app.dialog.is_some();
    let Some(pane) = app.office.open.get(app.office.current).cloned() else {
        hide_shown(app);
        return;
    };
    let t = Tokens::get(ui.ctx());
    let max = (ui.available_width() / 2.0).max(320.0);
    let mut page_rect = None;
    egui::Panel::right("office_pane").default_size(340.0).max_size(max).resizable(true).frame(egui::Frame::NONE.fill(t.chrome).inner_margin(8)).show(
        ui,
        |ui| {
            ui.horizontal(|ui| {
                if app.office.open.len() > 1 {
                    let titles: Vec<(usize, String)> = app.office.open.iter().enumerate().map(|(i, p)| (i, p.title.clone())).collect();
                    for (i, title) in titles {
                        if ui.selectable_label(i == app.office.current, title).clicked() {
                            app.office.current = i;
                        }
                    }
                } else {
                    ui.label(egui::RichText::new(&pane.title).font(theme::bold(15.0)));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("✕").on_hover_text("Close").clicked() {
                        let i = app.office.current;
                        close(app, i);
                    }
                });
            });
            ui.separator();
            if app.services.office.is_some() {
                let (rect, _) = ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
                page_rect = Some(rect);
            } else {
                ui.label("Office add-ins need the DeckCraft desktop app on macOS or Windows.");
                ui.add_space(4.0);
                ui.label(egui::RichText::new(&pane.url).weak());
            }
        },
    );
    // The pane may have been closed above.
    let Some(pane) = app.office.open.get(app.office.current).cloned() else {
        hide_shown(app);
        return;
    };
    let ppp = ui.ctx().pixels_per_point();
    match (page_rect, covered) {
        (Some(rect), false) => {
            if app.office.shown.as_deref() != Some(pane.key.as_str()) {
                hide_shown(app);
            }
            if let Some(host) = app.services.office.as_mut() {
                host.show(&pane.key, &pane.url, rect, ppp);
            }
            app.office.shown = Some(pane.key);
        }
        _ => hide_shown(app),
    }
}

/// Hides the page while something covers the window (the File tab, a slide show).
pub fn covered(app: &mut SlideApp) {
    hide_shown(app);
}

fn hide_shown(app: &mut SlideApp) {
    if let Some(k) = app.office.shown.take()
        && let Some(host) = app.services.office.as_mut()
    {
        host.hide(&k);
    }
}
