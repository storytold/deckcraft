//! An in-process engine session that answers the engine-level control-channel methods itself.

use deckcraft_engine::{Mods, PointerEvent, PointerKind, Session, ToolKind};
use std::sync::Arc;

use deckcraft_engine::file_access::{self, AutomationRoots};
use serde_json::{Value, json};

use crate::backend::Backend;

/// Headless backend: a [`Session`] rendering slides on the CPU.
pub struct Headless {
    pub session: Session,
    roots: Option<Arc<AutomationRoots>>,
}

impl Default for Headless {
    fn default() -> Self {
        Self::new()
    }
}

/// Hint appended to errors for methods that need a window.
pub(crate) const NEEDS_APP: &str = "it needs the desktop app: start `deckcraft --control 7979` and run the MCP server \
with `deckcraft-cli mcp --connect 7979`";

fn s<'a>(p: &'a Value, k: &str) -> Option<&'a str> {
    p.get(k).and_then(Value::as_str)
}

pub fn pointer_kind(k: &str) -> Option<PointerKind> {
    Some(match k {
        "down" => PointerKind::Down,
        "drag" => PointerKind::Drag,
        "up" => PointerKind::Up,
        "move" => PointerKind::Move,
        "doubleclick" | "dblclick" => PointerKind::DoubleClick,
        "tripleclick" => PointerKind::TripleClick,
        "contextclick" | "rightclick" => PointerKind::ContextClick,
        _ => return None,
    })
}

pub fn mods_from(p: &Value) -> Mods {
    let b = |n: &str| p.get(n).and_then(Value::as_bool).unwrap_or(false);
    Mods { shift: b("shift"), alt: b("alt"), cmd: b("cmd") || b("ctrl") }
}

/// A tool name (`select`, `textBox`, `shape`, `pen`, `highlighter`, `eraser`) to a [`ToolKind`].
pub fn tool_kind(name: &str, preset: Option<&str>) -> Option<ToolKind> {
    Some(match name {
        "select" | "selection" | "arrow" => ToolKind::Select,
        "textBox" | "text" | "textbox" => ToolKind::TextBox,
        "shape" => ToolKind::Shape { preset: preset.unwrap_or("rect").to_string() },
        "pen" => ToolKind::Ink { mode: "pen".into(), color: deckcraft_engine::model::Rgba::BLACK, width: 2.0 },
        "highlighter" => ToolKind::Ink { mode: "highlighter".into(), color: deckcraft_engine::model::Rgba::rgb(255, 230, 0), width: 10.0 },
        "eraser" => ToolKind::Ink { mode: "eraser".into(), color: deckcraft_engine::model::Rgba::BLACK, width: 2.0 },
        other if deckcraft_engine::geom::preset::info(other).is_some() => ToolKind::Shape { preset: other.to_string() },
        other if deckcraft_engine::tools::is_freeform_tool(other) => ToolKind::Shape { preset: other.to_string() },
        _ => return None,
    })
}

impl Headless {
    pub fn new() -> Self {
        Self { session: Session::new(), roots: None }
    }

    /// A session with a fresh presentation.
    pub fn with_document() -> Self {
        Self { session: Session::with_new(), roots: None }
    }

    /// Confine every file the session's commands read and write to `roots`
    /// (`deckcraft-cli mcp --automation-read-root … --automation-write-root …`).
    pub fn with_automation_roots(mut self, roots: Option<Arc<AutomationRoots>>) -> Self {
        self.roots = roots;
        self
    }

    fn exec(&mut self, id: &str, params: &Value) -> Result<Value, String> {
        let r = self.session.execute(id, params).map_err(|e| {
            let ui_only = ["app.", "view.", "window.", "ui."].iter().any(|p| id.starts_with(p));
            if ui_only && deckcraft_engine::find_command(id).is_none() { format!("`{id}` is a UI command; {NEEDS_APP}") } else { e.to_string() }
        });
        self.session.ui_requests.clear();
        r
    }

    fn pointer(&mut self, p: &Value) -> Result<Value, String> {
        let events = p.get("events").and_then(Value::as_array).ok_or("missing `events`")?;
        let base = p.get("mods").map(mods_from).unwrap_or_default();
        if self.session.active().is_none() {
            return Err("no presentation open (use new_presentation first)".into());
        }
        let mut last = Value::Null;
        for e in events {
            let kind = s(e, "kind").unwrap_or("");
            let kind = pointer_kind(kind)
                .ok_or_else(|| format!("unknown pointer kind `{kind}` (down|drag|up|move|doubleclick|tripleclick|contextclick)"))?;
            let x = e.get("x").and_then(Value::as_f64).ok_or("pointer event needs `x`")?;
            let y = e.get("y").and_then(Value::as_f64).ok_or("pointer event needs `y`")?;
            let mods = e.get("mods").map(mods_from).unwrap_or(base);
            let pressure = e.get("pressure").and_then(Value::as_f64).map(|v| v as f32);
            last = self.session.pointer(PointerEvent { kind, x, y, mods, tol: 3.0, pressure }).map_err(|e| e.to_string())?;
        }
        self.session.ui_requests.clear();
        Ok(json!({"result": last, "selection": self.session.active().map(|d| serde_json::to_value(&d.selection).unwrap_or_default())}))
    }

    fn render(&mut self, p: &Value) -> Result<Value, String> {
        let st = self.session.active().ok_or("no presentation open")?;
        let i = p.get("slide").and_then(Value::as_u64).map(|v| v as usize).unwrap_or(st.selection.slide);
        if i >= st.doc.slides.len() {
            return Err(format!("no slide {i}"));
        }
        let scale = p.get("scale").and_then(Value::as_f64).unwrap_or(1.0).clamp(0.05, 8.0);
        let img = deckcraft_engine::render::render_slide(
            &st.doc,
            i,
            &deckcraft_engine::render::RenderOpts { scale, edit: p.get("edit").and_then(Value::as_bool).unwrap_or(false), ..Default::default() },
        );
        let png = img.to_png_at(deckcraft_engine::render::dpi_for_scale(scale));
        if let Some(path) = s(p, "path") {
            deckcraft_engine::io::write_file(path, &png)?;
            return Ok(json!({"path": path, "width": img.width, "height": img.height}));
        }
        Ok(json!({"png": deckcraft_engine::cmd::base64_encode(&png), "width": img.width, "height": img.height, "slide": i}))
    }
}

impl Backend for Headless {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let roots = self.roots.clone();
        file_access::confine(roots.as_ref(), || self.answer(method, params))
    }
    fn has_ui(&self) -> bool {
        false
    }
    fn describe(&self) -> String {
        "headless (in-process engine; no window)".into()
    }
}

impl Headless {
    fn answer(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let p = &params;
        match method {
            "engine.execute" | "command" | "ui.menu.invoke" => {
                let id = s(p, "command").or(s(p, "id")).ok_or("missing `command`")?.to_string();
                let cp = p.get("params").cloned().filter(|v| !v.is_null()).unwrap_or(json!({}));
                self.exec(&id, &cp)
            }
            "engine.commands" => Ok(serde_json::to_value(self.session.commands()).unwrap_or_default()),
            "document.inspect" => self.exec("document.inspect", &json!({})),
            "ui.pointer" => self.pointer(p),
            "ui.key" => {
                let key = s(p, "key").ok_or("missing `key`")?;
                self.session.key(key, mods_from(p)).map_err(|e| e.to_string())
            }
            "ui.text" => {
                let t = s(p, "text").ok_or("missing `text`")?;
                self.session.type_text(t).map_err(|e| e.to_string())
            }
            "ui.tool.select" => {
                let name = s(p, "tool").ok_or("missing `tool`")?;
                let kind = tool_kind(name, s(p, "preset")).ok_or_else(|| format!("unknown tool `{name}`"))?;
                self.session.set_tool(kind);
                Ok(json!({"tool": name}))
            }
            "ui.render" | "app.renderSlide" => self.render(p),
            "app.open" => self.exec("file.open", p),
            "app.save" => self.exec("file.save", p),
            "app.export" => self.exec("file.export", p),
            "ui.inspect" => Ok(json!({"headless": true, "tool": serde_json::to_value(&self.session.tool.kind).unwrap_or_default()})),
            m if m.starts_with("ui.") || m.starts_with("app.") => Err(format!("`{m}`: {NEEDS_APP}")),
            other => Err(format!("unknown method `{other}`")),
        }
    }
}
