//! Programmatic control of the running app (agents, tests, MCP `--connect`).
//!
//! Methods (JSON lines over the host's transport, see `docs/control-protocol.md`):
//! - `engine.execute {command, params}` / `ui.menu.invoke {command, params}`: run any command (engine or UI)
//! - `engine.commands`: engine + UI commands with enablement; `document.inspect`; `ui.inspect`
//! - `ui.pointer {events:[{kind: down|drag|up|move|doubleclick, x, y, pressure?}], mods}`: drive the active tool in
//!   slide points (same path as the mouse; `pressure` is `0..=1` when a pen reported one)
//! - `ui.key {key, shift?, alt?, cmd?}` / `ui.text {text}`: keyboard through the engine
//! - `ui.tool.select {tool, preset?}`
//! - `ui.click {x, y, button?, count?}` / `ui.move {x, y}` / `ui.drag {x, y, toX, toY, steps?}`: real egui pointer
//!   input in window points (reaches every widget)
//! - `ui.set {tab?, pane?, view?, zoom?, dark?, notes?, ribbonCollapsed?}`; `ui.dialog.close`
//! - `ui.screenshot {path?}`: the whole window as PNG; `ui.render {slide?, scale?, path?}`: one slide (headless)
//! - `ui.resize {width, height}`, `ui.focus`
//! - `show.start {from?}`, `show.key {key}`, `show.end`
//! - `app.open {path}` / `app.save {path?}` / `app.export {path, …}` / `app.quit`

use std::sync::mpsc::Sender;

use deckcraft_engine::{Mods, PointerEvent};
use serde_json::{Value, json};

use crate::SlideApp;

pub type ControlResponse = Value;

pub struct ControlRequest {
    pub method: String,
    pub params: Value,
    pub reply: Sender<ControlResponse>,
    /// Who has the request: [`PENDING`], [`TAKEN`] (the app runs it) or [`ABANDONED`] (the
    /// sender gave up on it, so the app must not run it late: issue #719).
    pub state: std::sync::Arc<std::sync::atomic::AtomicU8>,
}

/// Queued, not yet run.
pub const PENDING: u8 = 0;
/// The app has started it: it will answer.
pub const TAKEN: u8 = 1;
/// Its sender gave up waiting (the reply timeout expired): it is dropped unrun.
pub const ABANDONED: u8 = 2;

impl ControlRequest {
    pub fn new(method: impl Into<String>, params: Value) -> (Self, std::sync::mpsc::Receiver<ControlResponse>) {
        let (tx, rx) = std::sync::mpsc::channel();
        (Self { method: method.into(), params, reply: tx, state: Default::default() }, rx)
    }

    /// Claim the request for running; false when its sender already gave up on it.
    pub fn take(&self) -> bool {
        use std::sync::atomic::Ordering;
        self.state.compare_exchange(PENDING, TAKEN, Ordering::SeqCst, Ordering::SeqCst).is_ok() || self.state.load(Ordering::SeqCst) == TAKEN
    }

    /// Give up on the request; false when the app has already started it (it will answer).
    pub fn abandon(state: &std::sync::atomic::AtomicU8) -> bool {
        use std::sync::atomic::Ordering;
        state.compare_exchange(PENDING, ABANDONED, Ordering::SeqCst, Ordering::SeqCst).is_ok() || state.load(Ordering::SeqCst) == ABANDONED
    }
}

pub enum Outcome {
    Done(Value),
    Screenshot { path: Option<String> },
}

fn ok(v: Value) -> Outcome {
    Outcome::Done(json!({"ok": true, "result": v}))
}
fn err(e: impl std::fmt::Display) -> Outcome {
    Outcome::Done(json!({"ok": false, "error": e.to_string()}))
}
fn wrap(r: Result<Value, String>) -> Outcome {
    match r {
        Ok(v) => ok(v),
        Err(e) => err(e),
    }
}

pub fn all_commands(app: &SlideApp) -> Value {
    let mut v: Vec<Value> = app.session.commands().into_iter().map(|c| serde_json::to_value(c).unwrap_or_default()).collect();
    for (id, label, sc, params) in crate::UI_COMMANDS {
        v.push(json!({"id": id, "label": label, "shortcut": sc, "params": params, "enabled": true, "ui": true}));
    }
    Value::Array(v)
}

pub fn inspect(app: &SlideApp, ctx: &egui::Context) -> Value {
    let r = ctx.content_rect();
    json!({
        "ui": serde_json::to_value(&app.ui).unwrap_or_default(),
        "tool": serde_json::to_value(&app.session.tool.kind).unwrap_or_default(),
        "window": [r.width(), r.height()],
        "canvasRect": app.canvas_rect.map(|c| json!([c.left(), c.top(), c.width(), c.height()])),
        "slideRect": app.slide_rect.map(|c| json!([c.left(), c.top(), c.width(), c.height()])),
        "canvasScale": app.canvas_scale,
        "dialog": app.dialog.as_ref().map(|d| d.id.clone()),
        "home": app.backstage_open(),
        "backstage": app.backstage.as_ref().map(|b| b.page.id()),
        "showing": app.show.as_ref().map(|s| {
            let (n, aspect) = app.session.active().map(|d| {
                let h = d.doc.slide_size.height.max(1.0) as f32;
                (d.doc.slides.len(), d.doc.slide_size.width as f32 / h)
            }).unwrap_or((0, 16.0 / 9.0));
            let now = ctx.input(|i| i.time);
            s.showing_value(n, aspect, ctx.content_rect(), now)
        }),
        "animPreview": app.anim_preview_report().map(|(slide, time, duration)| json!({"slide": slide, "time": time, "duration": duration})),
        "documents": app.session.documents().iter().map(|d| json!({"title": d.title(), "dirty": d.is_dirty(), "path": d.path})).collect::<Vec<_>>(),
        "status": app.status.as_ref().map(|s| s.0.clone()),
        "perf": {"frameMs": app.perf.frame_ms, "renderMs": app.perf.render_ms, "fps": app.perf.fps},
    })
}

fn key_from(name: &str) -> Option<egui::Key> {
    egui::Key::from_name(name).or(match name.to_ascii_lowercase().as_str() {
        "enter" | "return" => Some(egui::Key::Enter),
        "esc" | "escape" => Some(egui::Key::Escape),
        "delete" => Some(egui::Key::Delete),
        "backspace" => Some(egui::Key::Backspace),
        "left" => Some(egui::Key::ArrowLeft),
        "right" => Some(egui::Key::ArrowRight),
        "up" => Some(egui::Key::ArrowUp),
        "down" => Some(egui::Key::ArrowDown),
        "space" => Some(egui::Key::Space),
        "tab" => Some(egui::Key::Tab),
        _ => None,
    })
}

fn f(p: &Value, k: &str) -> Option<f32> {
    p.get(k).and_then(Value::as_f64).map(|v| v as f32)
}

pub fn handle(app: &mut SlideApp, ctx: &egui::Context, req: &ControlRequest) -> Outcome {
    let p = &req.params;
    let s = |k: &str| p.get(k).and_then(Value::as_str);
    match req.method.as_str() {
        "engine.execute" | "ui.menu.invoke" | "command" => {
            let Some(id) = s("command").or(s("id")) else { return err("missing `command`") };
            let params = p.get("params").cloned().filter(|v| !v.is_null()).unwrap_or(json!({}));
            wrap(app.run(id, params))
        }
        "engine.commands" => ok(all_commands(app)),
        "document.inspect" => wrap(app.run("document.inspect", json!({}))),
        "ui.inspect" => ok(inspect(app, ctx)),
        "ui.pointer" => {
            let Some(events) = p.get("events").and_then(Value::as_array) else { return err("missing `events`") };
            let base = p.get("mods").map(mods_from).unwrap_or_default();
            let mut last = Value::Null;
            for e in events {
                let kind = e.get("kind").and_then(Value::as_str).unwrap_or("");
                let Some(kind) = deckcraft_mcp_kind(kind) else { return err(format!("unknown pointer kind `{kind}`")) };
                let (Some(x), Some(y)) = (e.get("x").and_then(Value::as_f64), e.get("y").and_then(Value::as_f64)) else {
                    return err("pointer events need x and y");
                };
                let mods = e.get("mods").map(mods_from).unwrap_or(base);
                let pressure = e.get("pressure").and_then(Value::as_f64).map(|v| v as f32);
                match app.session.pointer(PointerEvent { kind, x, y, mods, tol: (4.0 / app.canvas_scale.max(0.05)) as f64, pressure }) {
                    Ok(v) => last = v,
                    Err(e) => return err(e),
                }
            }
            ok(json!({"result": last, "selection": app.session.active().map(|d| serde_json::to_value(&d.selection).unwrap_or_default())}))
        }
        "ui.key" => {
            let Some(k) = s("key") else { return err("missing `key`") };
            if app.anim_preview_active() && app.show.is_none() && matches!(k.to_ascii_lowercase().as_str(), "escape" | "esc") {
                app.stop_anim_preview();
                return ok(json!({"preview": false}));
            }
            if let Some(show) = app.show.as_ref()
                && !show.ended
                && let Some(key) = key_from(k)
            {
                app.synthetic.push(egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE });
                app.synthetic.push(egui::Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: egui::Modifiers::NONE });
                return ok(json!({"show": true}));
            }
            wrap(app.session.key(k, mods_from(p)).map_err(|e| e.to_string()))
        }
        "ui.text" => {
            let Some(t) = s("text") else { return err("missing `text`") };
            wrap(app.session.type_text(t).map_err(|e| e.to_string()))
        }
        "ui.ime" => {
            // An input method's event, through egui like a real one: it reaches the slide's text
            // or a focused text field (#101).
            let ev = if let Some(t) = s("commit") {
                egui::ImeEvent::Commit(t.into())
            } else if let Some(t) = s("preedit") {
                let active = p.get("active").and_then(Value::as_array).and_then(|a| {
                    let (x, y) = (a.first()?.as_u64()? as usize, a.get(1)?.as_u64()? as usize);
                    (x <= y).then_some(x..y)
                });
                egui::ImeEvent::Preedit { text: t.into(), active_range_chars: active }
            } else {
                return err("missing `preedit` or `commit`");
            };
            app.synthetic.push(egui::Event::Ime(ev));
            ok(json!({"queued": true}))
        }
        "ui.tool.select" => {
            let Some(name) = s("tool") else { return err("missing `tool`") };
            match tool_kind(name, s("preset")) {
                Some(k) => {
                    app.session.set_tool(k);
                    ok(json!({"tool": name}))
                }
                None => err(format!("unknown tool `{name}`")),
            }
        }
        "ui.move" | "ui.click" | "ui.drag" => {
            let (Some(x), Some(y)) = (f(p, "x"), f(p, "y")) else { return err("missing x/y") };
            let pos = egui::pos2(x, y);
            app.synthetic.push(egui::Event::PointerMoved(pos));
            if req.method == "ui.click" {
                let button = match s("button") {
                    Some("right" | "secondary") => egui::PointerButton::Secondary,
                    _ => egui::PointerButton::Primary,
                };
                let count = p.get("count").and_then(Value::as_u64).unwrap_or(1).clamp(1, 3);
                for _ in 0..count {
                    app.synthetic.push(egui::Event::PointerButton { pos, button, pressed: true, modifiers: egui::Modifiers::NONE });
                    app.synthetic.push(egui::Event::PointerButton { pos, button, pressed: false, modifiers: egui::Modifiers::NONE });
                }
            } else if req.method == "ui.drag" {
                let (Some(tx), Some(ty)) = (f(p, "toX"), f(p, "toY")) else { return err("missing toX/toY") };
                let steps = p.get("steps").and_then(Value::as_u64).unwrap_or(8).clamp(1, 120);
                app.synthetic.push(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                });
                for k in 1..=steps {
                    let a = k as f32 / steps as f32;
                    app.synthetic.push(egui::Event::PointerMoved(egui::pos2(x + (tx - x) * a, y + (ty - y) * a)));
                }
                app.synthetic.push(egui::Event::PointerButton {
                    pos: egui::pos2(tx, ty),
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            ctx.request_repaint();
            ok(json!({"queued": app.synthetic.len()}))
        }
        "ui.set" => {
            if let Some(t) = s("tab") {
                app.ui.tab = t.to_string();
                app.ui.ribbon_collapsed = false;
            }
            if let Some(pv) = p.get("pane") {
                app.ui.pane = pv.as_str().map(String::from);
            }
            if let Some(v) = s("view") {
                app.ui.view = match v {
                    "sorter" => crate::ViewMode::Sorter,
                    "outline" => crate::ViewMode::Outline,
                    "notesPage" => crate::ViewMode::NotesPage,
                    _ => crate::ViewMode::Normal,
                };
            }
            if let Some(z) = p.get("zoom") {
                app.ui.zoom = z
                    .as_f64()
                    .map(|v| v as f32)
                    .filter(|v| v.is_finite() && *v > 0.0)
                    .map(|v| v.clamp(*crate::ZOOM_RANGE.start(), *crate::ZOOM_RANGE.end()));
            }
            if let Some(d) = p.get("dark").and_then(Value::as_bool) {
                let _ = app.run("view.dark", json!({"on": d}));
            }
            if let Some(n) = p.get("notes").and_then(Value::as_bool) {
                app.ui.notes = n;
            }
            if let Some(c) = p.get("ribbonCollapsed").and_then(Value::as_bool) {
                app.ui.ribbon_collapsed = c;
            }
            if let Some(ft) = s("formatTab") {
                app.ui.format_tab = ft.to_string();
            }
            ok(serde_json::to_value(&app.ui).unwrap_or_default())
        }
        "ui.dialog.close" => {
            app.dialog = None;
            ok(Value::Null)
        }
        "ui.dialog.open" => {
            let Some(id) = s("id") else { return err("missing `id`") };
            let mut d = crate::dialogs::Dialog::new(id);
            if let Some(fields) = p.get("fields").and_then(Value::as_object) {
                for (k, v) in fields {
                    let v = v.as_str().map_or_else(|| v.to_string(), str::to_string);
                    d.fields.insert(k.clone(), v);
                }
            }
            app.dialog = Some(d);
            ok(Value::Null)
        }
        "ui.screenshot" => Outcome::Screenshot { path: s("path").map(String::from) },
        "ui.render" => {
            let Some(d) = app.session.active() else { return err("no presentation open") };
            let i = p.get("slide").and_then(Value::as_u64).map(|v| v as usize).unwrap_or(d.selection.slide);
            if i >= d.doc.slides.len() {
                return err(format!("no slide {i}"));
            }
            let (png, w, h) = deckcraft_engine::cmd::file::render_png(
                &d.doc,
                i,
                p.get("scale").and_then(Value::as_f64).unwrap_or(1.0),
                p.get("edit").and_then(Value::as_bool).unwrap_or(false),
            );
            match s("path") {
                Some(path) => match app.services.write.as_mut().map(|w| w(path, &png)) {
                    Some(Ok(())) => ok(json!({"path": path, "width": w, "height": h})),
                    Some(Err(e)) => err(e),
                    None => err("no writer"),
                },
                None => ok(json!({"png": deckcraft_engine::cmd::base64_encode(&png), "width": w, "height": h, "slide": i})),
            }
        }
        "ui.resize" => {
            let (Some(w), Some(h)) = (f(p, "width"), f(p, "height")) else { return err("missing width/height") };
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(w, h)));
            ok(Value::Null)
        }
        "ui.focus" => {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            ok(Value::Null)
        }
        "show.start" => {
            let reading = p.get("reading").and_then(Value::as_bool).unwrap_or(false);
            if let Some(name) = p.get("custom").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                app.start_custom_show(name, reading);
            } else if p.get("from").is_some() {
                let from = p.get("from").and_then(Value::as_u64).unwrap_or(0) as usize;
                app.start_show(from, reading);
            } else {
                app.start_show(app.show_start_index(), reading);
            }
            ok(Value::Null)
        }
        "show.end" => {
            if app.show.is_some() {
                app.synthetic.push(egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            ok(Value::Null)
        }
        "app.open" => {
            let Some(path) = s("path") else { return err("missing `path`") };
            wrap(app.open_path(path).map(|_| Value::Null))
        }
        "app.save" => wrap(app.run("file.save", p.clone())),
        "app.export" => wrap(app.run("file.export", p.clone())),
        "app.quit" => {
            // Automation quits without asking (`{"save": true}` saves first).
            if p.get("save").and_then(Value::as_bool) == Some(true) && !app.save_all() {
                return err("not every presentation could be saved");
            }
            app.quit_confirmed = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            ok(Value::Null)
        }
        other => err(format!("unknown method `{other}`")),
    }
}

fn mods_from(p: &Value) -> Mods {
    let b = |n: &str| p.get(n).and_then(Value::as_bool).unwrap_or(false);
    Mods { shift: b("shift"), alt: b("alt"), cmd: b("cmd") || b("ctrl") }
}

fn deckcraft_mcp_kind(k: &str) -> Option<deckcraft_engine::PointerKind> {
    use deckcraft_engine::PointerKind::*;
    Some(match k {
        "down" => Down,
        "drag" => Drag,
        "up" => Up,
        "move" => Move,
        "doubleclick" | "dblclick" => DoubleClick,
        "tripleclick" => TripleClick,
        "contextclick" | "rightclick" => ContextClick,
        _ => return None,
    })
}

pub fn tool_kind(name: &str, preset: Option<&str>) -> Option<deckcraft_engine::ToolKind> {
    use deckcraft_engine::ToolKind;
    Some(match name {
        "select" | "selection" => ToolKind::Select,
        "textBox" | "text" => ToolKind::TextBox,
        "shape" => ToolKind::Shape { preset: preset.unwrap_or("rect").into() },
        "pen" => ToolKind::Ink { mode: "pen".into(), color: deckcraft_color::Rgba::BLACK, width: 2.0 },
        "highlighter" => ToolKind::Ink { mode: "highlighter".into(), color: deckcraft_color::Rgba::rgb(255, 230, 0), width: 10.0 },
        "eraser" => ToolKind::Ink { mode: "eraser".into(), color: deckcraft_color::Rgba::BLACK, width: 2.0 },
        other if deckcraft_geom::preset::info(other).is_some() => ToolKind::Shape { preset: other.into() },
        other if deckcraft_engine::tools::is_freeform_tool(other) => ToolKind::Shape { preset: other.into() },
        _ => return None,
    })
}

pub fn save_screenshot(app: &mut SlideApp, image: &egui::ColorImage, path: Option<&str>) -> Value {
    let [w, h] = image.size;
    let rgba: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
    let img = deckcraft_render::Image { width: w as u32, height: h as u32, pixels: rgba };
    let png = img.to_png();
    let Some(path) = path else {
        return json!({"ok": true, "result": {"width": w, "height": h, "png": deckcraft_engine::cmd::base64_encode(&png)}});
    };
    match app.services.write.as_mut() {
        Some(wr) => match wr(path, &png) {
            Ok(()) => json!({"ok": true, "result": {"path": path, "width": w, "height": h}}),
            Err(e) => json!({"ok": false, "error": e}),
        },
        None => json!({"ok": false, "error": "no writer configured"}),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    /// `take` and `abandon` are exclusive: whoever wins, the request runs at most once (#719).
    #[test]
    fn take_and_abandon_are_exclusive() {
        let (req, _rx) = ControlRequest::new("m", json!({}));
        assert!(req.take(), "a pending request is taken");
        assert!(!ControlRequest::abandon(&req.state), "a taken request is not abandoned");
        assert!(req.take(), "a taken request stays taken");

        let (req, _rx) = ControlRequest::new("m", json!({}));
        assert!(ControlRequest::abandon(&req.state), "a pending request is abandoned");
        assert!(!req.take(), "an abandoned request is never taken");
        assert!(ControlRequest::abandon(&req.state), "abandoning twice stays abandoned");
    }
}
