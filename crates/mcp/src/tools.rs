//! MCP tool definitions (names, descriptions, JSON Schemas) and their implementations on top of a
//! [`Backend`].

use serde_json::{Value, json};

use crate::backend::Backend;
use crate::headless::NEEDS_APP;

/// The result of `tools/call`: MCP content blocks plus the `isError` flag.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolResult {
    pub content: Vec<Value>,
    pub is_error: bool,
}

impl ToolResult {
    pub fn text(t: impl Into<String>) -> Self {
        Self { content: vec![json!({"type": "text", "text": t.into()})], is_error: false }
    }
    pub fn json(v: &Value) -> Self {
        Self::text(serde_json::to_string_pretty(v).unwrap_or_default())
    }
    pub fn error(t: impl Into<String>) -> Self {
        Self { content: vec![json!({"type": "text", "text": t.into()})], is_error: true }
    }
    pub fn image(png_base64: String, info: &Value) -> Self {
        Self {
            content: vec![json!({"type": "image", "data": png_base64, "mimeType": "image/png"}), json!({"type": "text", "text": info.to_string()})],
            is_error: false,
        }
    }
    pub fn to_value(&self) -> Value {
        json!({"content": self.content, "isError": self.is_error})
    }
}

fn num(desc: &str) -> Value {
    json!({"type": "number", "description": desc})
}
fn int(desc: &str) -> Value {
    json!({"type": "integer", "minimum": 0, "description": desc})
}
fn string(desc: &str) -> Value {
    json!({"type": "string", "description": desc})
}
fn boolean(desc: &str) -> Value {
    json!({"type": "boolean", "description": desc})
}
fn mods_schema() -> Value {
    json!({"type": "object", "description": "Modifier keys (cmd = Command on macOS / Ctrl elsewhere; alt = Option)", "properties": {"shift": {"type": "boolean"}, "alt": {"type": "boolean"}, "cmd": {"type": "boolean"}}})
}
fn obj(props: Value, required: &[&str]) -> Value {
    let mut o = json!({"type": "object", "properties": props});
    if !required.is_empty() {
        o["required"] = json!(required);
    }
    o
}
fn tool(name: &str, title: &str, desc: &str, schema: Value, read_only: bool) -> Value {
    json!({"name": name, "title": title, "description": desc, "inputSchema": schema, "annotations": {"title": title, "readOnlyHint": read_only, "openWorldHint": false}})
}

const APP_ONLY: &str = " Desktop app only (`deckcraft-cli mcp --connect PORT`).";

pub fn tool_definitions() -> Vec<Value> {
    vec![
        tool(
            "list_commands",
            "List commands",
            "Every command (id, label, ribbon/menu place, shortcut, parameter summary, whether it is enabled now). Filter by a substring of id or label.",
            obj(json!({"filter": string("Substring of the id or label, e.g. `format.` or `slide`")}), &[]),
            true,
        ),
        tool(
            "run_command",
            "Run a command",
            "Run any command by id with JSON parameters. This is how everything is done: slide.new, shape.insert, text.set, format.bold, shape.fill, design.theme, transition.set, animation.add, table.merge, edit.undo…",
            obj(
                json!({"command": string("Command id from list_commands"), "params": {"type": "object", "description": "Parameters (see the command's params summary)"}}),
                &["command"],
            ),
            false,
        ),
        tool(
            "batch",
            "Run several commands",
            "Run commands in order; stops at the first error unless keepGoing. Returns each result.",
            obj(
                json!({"commands": {"type": "array", "items": obj(json!({"command": string("id"), "params": {"type": "object"}}), &["command"])}, "keepGoing": boolean("Continue after errors")}),
                &["commands"],
            ),
            false,
        ),
        tool(
            "new_presentation",
            "New presentation",
            "Start a new presentation (one title slide). Optional theme: DeckCraft, Harbor, Ember, Meadow, Nocturne, Paper, Coral Reef, Slate.",
            obj(json!({"theme": string("Theme name"), "blank": boolean("No first slide")}), &[]),
            false,
        ),
        tool(
            "open_presentation",
            "Open presentation",
            "Open a .deckcraft or .pptx file by path.",
            obj(json!({"path": string("File path")}), &["path"]),
            false,
        ),
        tool(
            "save_presentation",
            "Save presentation",
            "Save the active presentation (.deckcraft or .pptx by extension).",
            obj(json!({"path": string("File path (optional when it was opened from a file)")}), &[]),
            false,
        ),
        tool(
            "export",
            "Export",
            "Export slides: png/jpeg (one slide, or all with all:true), pptx, deckcraft, outline text, or pdf (slides, notes pages or handouts).",
            obj(
                json!({
                    "path": string("Output path"),
                    "format": {"type": "string", "enum": ["png", "jpeg", "pptx", "deckcraft", "outline", "pdf"], "description": "Output format (default: from the path's extension)"},
                    "slide": int("Slide index for images"),
                    "all": boolean("All slides (images)"),
                    "scale": num("Pixels per point for images (default 2)"),
                    "layout": {"type": "string", "enum": ["slides", "notes", "handouts"], "description": "PDF page layout (default slides)"},
                    "perPage": {"type": "integer", "enum": [1, 2, 3, 4, 6, 9], "description": "PDF handouts: slides per page (default 6)"},
                    "dpi": num("PDF: raster resolution of slide artwork, 36-600 (default 200)"),
                    "slides": {"type": "array", "items": {"type": "integer", "minimum": 0}, "description": "PDF: slide indices to export (default all)"},
                    "includeHidden": boolean("PDF: include hidden slides (default false)"),
                    "textLayer": boolean("PDF: invisible text layer for search, select and copy (default true)"),
                    "frame": boolean("PDF notes/handouts: thin frame around each slide (default true)")
                }),
                &["path"],
            ),
            false,
        ),
        tool(
            "inspect_document",
            "Inspect presentation",
            "Slides (index, id, title, layout, shape count, transition, animations, notes), sections, theme, layouts, media and the selection.",
            obj(json!({}), &[]),
            true,
        ),
        tool(
            "inspect_slide",
            "Inspect slide",
            "One slide's shapes with ids, kinds, boxes [x,y,w,h], placeholder types, text, fills, tables, charts; plus background, transition, animations, notes.",
            obj(json!({"index": int("Slide index (default: current)")}), &[]),
            true,
        ),
        tool(
            "render_slide",
            "Render slide",
            "Render a slide to a PNG image and return it, to check your work visually.",
            obj(
                json!({"slide": int("Slide index (default: current)"), "scale": num("Pixels per point (default 1 = 960×540 for 16:9)"), "edit": boolean("Show placeholder prompts like the editor")}),
                &[],
            ),
            true,
        ),
        tool(
            "add_slide",
            "Add slide",
            "Insert a slide after the current one with a layout (title, titleAndContent, sectionHeader, twoContent, comparison, titleOnly, blank, contentWithCaption, pictureWithCaption) and optional title/body text (body: one paragraph per line, leading tabs indent).",
            obj(json!({"layout": string("Layout name or kind"), "title": string("Title text"), "body": string("Body text")}), &[]),
            false,
        ),
        tool(
            "go_to_slide",
            "Go to slide",
            "Make a slide current (shape commands act on the current slide).",
            obj(json!({"index": int("Slide index")}), &["index"]),
            false,
        ),
        tool(
            "add_shape",
            "Add shape",
            "Insert a preset shape (rect, roundRect, ellipse, triangle, diamond, rightArrow, chevron, star5, heart, cloud, wedgeRectCallout, line, straightConnector1… see shape.presets) at x,y with size w×h, with optional text and fill colour (#RRGGBB or accent1..6).",
            obj(
                json!({"preset": string("Preset name"), "x": num("Left (pt)"), "y": num("Top (pt)"), "w": num("Width (pt)"), "h": num("Height (pt)"), "text": string("Text inside"), "fill": string("Fill colour"), "line": string("Outline colour or `none`")}),
                &["preset", "x", "y", "w", "h"],
            ),
            false,
        ),
        tool(
            "add_text_box",
            "Add text box",
            "Insert a text box. Height grows to fit the text.",
            obj(
                json!({"x": num("Left (pt)"), "y": num("Top (pt)"), "w": num("Width (pt)"), "h": num("Height (pt)"), "text": string("Text (one paragraph per line)"), "size": num("Font size (pt)"), "color": string("Text colour"), "align": string("left|center|right|justify")}),
                &["x", "y", "w", "text"],
            ),
            false,
        ),
        tool(
            "set_text",
            "Set text",
            "Replace a shape's text (keeps its first run's formatting). Leading tabs set bullet levels.",
            obj(json!({"id": int("Shape id (from inspect_slide)"), "text": string("New text")}), &["id", "text"]),
            false,
        ),
        tool(
            "insert_picture",
            "Insert picture",
            "Insert a picture from a file path or base64 data, optionally at a box.",
            obj(
                json!({"path": string("Image file path"), "data": string("Base64 image data (instead of path)"), "name": string("File name for data"), "x": num("Left"), "y": num("Top"), "w": num("Width"), "h": num("Height")}),
                &[],
            ),
            false,
        ),
        tool(
            "insert_table",
            "Insert table",
            "Insert a table with rows×cols and optional cell text (data: rows of strings).",
            obj(
                json!({"rows": int("Rows"), "cols": int("Columns"), "data": {"type": "array", "items": {"type": "array", "items": {"type": "string"}}}, "x": num("Left"), "y": num("Top"), "w": num("Width"), "h": num("Height")}),
                &["rows", "cols"],
            ),
            false,
        ),
        tool(
            "insert_chart",
            "Insert chart",
            "Insert a chart (column, bar, line, pie, doughnut, area, scatter, stackedColumn…) with categories and series.",
            obj(
                json!({"type": string("Chart type"), "title": string("Chart title"), "categories": {"type": "array", "items": {"type": "string"}}, "series": {"type": "array", "items": obj(json!({"name": string("Series name"), "values": {"type": "array", "items": {"type": "number"}}}), &["values"])}}),
                &[],
            ),
            false,
        ),
        tool(
            "select",
            "Select shapes",
            "Select shapes by id (formatting commands then act on them).",
            obj(json!({"ids": {"type": "array", "items": {"type": "integer"}}}), &["ids"]),
            false,
        ),
        tool(
            "pointer",
            "Pointer gesture",
            "Drive the active tool with pointer events in slide points: down/drag/up/move/doubleclick, exactly like the mouse (draws, moves, resizes, rotates, places the caret).",
            obj(
                json!({"events": {"type": "array", "items": obj(json!({"kind": string("down|drag|up|move|doubleclick"), "x": num("x (pt)"), "y": num("y (pt)"), "mods": mods_schema()}), &["kind", "x", "y"])}, "mods": mods_schema()}),
                &["events"],
            ),
            false,
        ),
        tool(
            "select_tool",
            "Select tool",
            "Pick the pointer tool: select, textBox, shape (with preset), pen, highlighter, eraser — or a preset name.",
            obj(json!({"tool": string("Tool name"), "preset": string("Shape preset for the shape tool")}), &["tool"]),
            false,
        ),
        tool(
            "key",
            "Press key",
            "Press a key as on the keyboard: text editing keys (arrows, Backspace, Enter, Tab), nudges, and shortcuts (cmd+B…).",
            obj(json!({"key": string("Key name or character"), "mods": mods_schema()}), &["key"]),
            false,
        ),
        tool(
            "type_text",
            "Type text",
            "Type text into the text being edited (or into the selected shape).",
            obj(json!({"text": string("Text")}), &["text"]),
            false,
        ),
        tool("screenshot", "Screenshot", &format!("Screenshot of the whole DeckCraft window (UI included).{APP_ONLY}"), obj(json!({}), &[]), true),
        tool(
            "call_app",
            "Call app method",
            &format!("Call any control-channel method (ui.click, ui.set, ui.dialog.*, ui.inspect…).{APP_ONLY}"),
            obj(json!({"method": string("Method"), "params": {"type": "object"}}), &["method"]),
            false,
        ),
    ]
}

fn exec(b: &mut dyn Backend, cmd: &str, params: Value) -> Result<Value, String> {
    b.call("engine.execute", json!({"command": cmd, "params": params}))
}

fn f(a: &Value, k: &str) -> Option<f64> {
    a.get(k).and_then(Value::as_f64)
}

fn wrap(r: Result<Value, String>) -> ToolResult {
    match r {
        Ok(v) => ToolResult::json(&v),
        Err(e) => ToolResult::error(e),
    }
}

pub fn call_tool(b: &mut dyn Backend, name: &str, a: &Value) -> ToolResult {
    let a = if a.is_object() { a.clone() } else { json!({}) };
    match name {
        "list_commands" => wrap(b.call("engine.commands", json!({})).map(|v| {
            let f = a.get("filter").and_then(Value::as_str).unwrap_or("").to_lowercase();
            Value::Array(
                v.as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|c| {
                        f.is_empty()
                            || c.get("id").and_then(Value::as_str).is_some_and(|i| i.to_lowercase().contains(&f))
                            || c.get("label").and_then(Value::as_str).is_some_and(|i| i.to_lowercase().contains(&f))
                    })
                    .collect(),
            )
        })),
        "run_command" => {
            let Some(c) = a.get("command").and_then(Value::as_str) else { return ToolResult::error("missing `command`") };
            wrap(exec(b, c, a.get("params").cloned().unwrap_or(json!({}))))
        }
        "batch" => {
            let keep = a.get("keepGoing").and_then(Value::as_bool).unwrap_or(false);
            let mut out = vec![];
            for c in a.get("commands").and_then(Value::as_array).cloned().unwrap_or_default() {
                let id = c.get("command").and_then(Value::as_str).unwrap_or("");
                match exec(b, id, c.get("params").cloned().unwrap_or(json!({}))) {
                    Ok(v) => out.push(json!({"command": id, "ok": true, "result": v})),
                    Err(e) => {
                        out.push(json!({"command": id, "ok": false, "error": e}));
                        if !keep {
                            return ToolResult { content: vec![json!({"type": "text", "text": Value::Array(out).to_string()})], is_error: true };
                        }
                    }
                }
            }
            ToolResult::json(&Value::Array(out))
        }
        "new_presentation" => wrap(exec(b, "file.new", a)),
        "open_presentation" => wrap(exec(b, "file.open", a)),
        "save_presentation" => wrap(exec(b, "file.save", a)),
        "export" => wrap(exec(b, "file.export", a)),
        "inspect_document" => wrap(b.call("document.inspect", json!({}))),
        "inspect_slide" => wrap(exec(b, "slide.inspect", a)),
        "render_slide" => match b.call("ui.render", a.clone()) {
            Ok(v) => match v.get("png").and_then(Value::as_str) {
                Some(png) => {
                    ToolResult::image(png.to_string(), &json!({"width": v.get("width"), "height": v.get("height"), "slide": v.get("slide")}))
                }
                None => ToolResult::json(&v),
            },
            Err(e) => ToolResult::error(e),
        },
        "add_slide" => wrap(exec(b, "slide.new", a)),
        "go_to_slide" => wrap(exec(b, "slide.go", a)),
        "add_shape" => {
            let r = exec(
                b,
                "shape.insert",
                json!({"preset": a.get("preset"), "rect": [f(&a, "x"), f(&a, "y"), f(&a, "w"), f(&a, "h")], "text": a.get("text")}),
            );
            let Ok(v) = r else { return wrap(r) };
            let id = v.get("id").cloned().unwrap_or_default();
            if let Some(c) = a.get("fill").filter(|c| c.is_string())
                && let Err(e) = exec(b, "shape.fill", json!({"id": id, "color": c}))
            {
                return ToolResult::error(e);
            }
            if let Some(c) = a.get("line").and_then(Value::as_str) {
                let p = if c == "none" { json!({"id": id, "none": true}) } else { json!({"id": id, "color": c}) };
                if let Err(e) = exec(b, "shape.line", p) {
                    return ToolResult::error(e);
                }
            }
            ToolResult::json(&v)
        }
        "add_text_box" => {
            let r =
                exec(b, "insert.textBox", json!({"rect": [f(&a, "x"), f(&a, "y"), f(&a, "w"), f(&a, "h").unwrap_or(30.0)], "text": a.get("text")}));
            let Ok(v) = r else { return wrap(r) };
            let id = v.get("id").cloned().unwrap_or_default();
            let _ = exec(b, "text.exit", json!({}));
            let _ = exec(b, "edit.select", json!({"ids": [id]}));
            if let Some(s) = f(&a, "size") {
                let _ = exec(b, "format.size", json!({"size": s}));
            }
            if let Some(c) = a.get("color").filter(|c| c.is_string()) {
                let _ = exec(b, "format.color", json!({"color": c}));
            }
            if let Some(al) = a.get("align").and_then(Value::as_str) {
                let _ = exec(b, "format.align", json!({"align": al}));
            }
            ToolResult::json(&v)
        }
        "set_text" => wrap(exec(b, "text.set", a)),
        "insert_picture" => {
            let mut p = a.clone();
            if let (Some(x), Some(y), Some(w), Some(h)) = (f(&a, "x"), f(&a, "y"), f(&a, "w"), f(&a, "h")) {
                p["rect"] = json!([x, y, w, h]);
            }
            wrap(exec(b, "insert.picture", p))
        }
        "insert_table" => {
            let mut p = a.clone();
            if let (Some(x), Some(y), Some(w), Some(h)) = (f(&a, "x"), f(&a, "y"), f(&a, "w"), f(&a, "h")) {
                p["rect"] = json!([x, y, w, h]);
            }
            wrap(exec(b, "insert.table", p))
        }
        "insert_chart" => wrap(exec(b, "insert.chart", a)),
        "select" => wrap(exec(b, "edit.select", a)),
        "pointer" => wrap(b.call("ui.pointer", a)),
        "select_tool" => wrap(b.call("ui.tool.select", a)),
        "key" => {
            let mut p = a.get("mods").cloned().filter(Value::is_object).unwrap_or(json!({}));
            p["key"] = a.get("key").cloned().unwrap_or_default();
            wrap(b.call("ui.key", p))
        }
        "type_text" => wrap(b.call("ui.text", a)),
        "screenshot" => {
            if !b.has_ui() {
                return ToolResult::error(format!("screenshot: {NEEDS_APP}. Use render_slide to see a slide headlessly."));
            }
            match b.call("ui.screenshot", json!({"base64": true})) {
                Ok(v) => match v.get("png").and_then(Value::as_str) {
                    Some(png) => ToolResult::image(png.to_string(), &json!({"width": v.get("width"), "height": v.get("height")})),
                    None => ToolResult::json(&v),
                },
                Err(e) => ToolResult::error(e),
            }
        }
        "call_app" => {
            let Some(m) = a.get("method").and_then(Value::as_str) else { return ToolResult::error("missing `method`") };
            wrap(b.call(m, a.get("params").cloned().unwrap_or(json!({}))))
        }
        other => ToolResult::error(format!("unknown tool `{other}`")),
    }
}
