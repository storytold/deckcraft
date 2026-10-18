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
        let mut t = t.into();
        // The engine names its own commands; here the tools that open a presentation are the
        // ones a client calls.
        if t.contains("no presentation open") {
            t.push_str("; here: new_presentation or open_presentation");
        }
        Self { content: vec![json!({"type": "text", "text": t})], is_error: true }
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
    json!({"type": "object", "additionalProperties": false, "description": "Modifier keys (cmd = Command on macOS / Ctrl elsewhere; alt = Option)", "properties": {"shift": {"type": "boolean"}, "alt": {"type": "boolean"}, "cmd": {"type": "boolean"}}})
}
fn obj(props: Value, required: &[&str]) -> Value {
    let mut o = json!({"type": "object", "properties": props, "additionalProperties": false});
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
            "The commands, three ways. No arguments: the command families (family, about, count, commands: the names after the dot, space-separated; a command id is family.name), a few KB. `filter`: the commands whose id or label contains it, in full (id, label, ribbon/menu place, shortcut, params doc, enabled now, actsOn); `brief` trims these to id and params. `id`: one command's full entry. Typical: the families once, then {filter: \"table.\"} or {id: \"table.style\"}.",
            obj(
                json!({"filter": string("Substring of the id or label, e.g. `format.` or `slide`"), "brief": boolean("With filter: only id and params (default false)"), "id": string("One command id, e.g. `table.style`: its full entry")}),
                &[],
            ),
            true,
        ),
        tool(
            "run_command",
            "Run a command",
            "Run any command by id with JSON parameters. This is how everything is done: slide.new, shape.insert, text.set, format.bold, shape.fill, design.theme, transition.set, animation.add, table.merge, edit.undo… A tool name (add_shape, go_to_slide…) works here too, with the tool's arguments as params.",
            obj(
                json!({"command": string("Command id from list_commands"), "params": {"type": "object", "description": "Parameters (see the command's params summary)"}}),
                &["command"],
            ),
            false,
        ),
        tool(
            "batch",
            "Run several commands",
            "Run commands in order; stops at the first error unless keepGoing. Returns each result. Items may name a tool (add_shape, add_text_box, go_to_slide…) instead of a command, with the tool's arguments as params. An item with `as: \"box\"` can be referred to by a later item's params as \"$box\" (its id), so insert, connect and animate fit in one batch.",
            obj(
                json!({"commands": {"type": "array", "items": obj(json!({"command": string("id"), "params": {"type": "object"}, "as": string("Name for this item's answer, for \"$name\" in later params")}), &["command"])}, "keepGoing": boolean("Continue after errors")}),
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
            "Export slides: png/jpeg (one slide, or all with all:true), pptx, deckcraft, outline text, pdf (slides, notes pages or handouts), or an animated gif of the deck (one frame per slide).",
            obj(
                json!({
                    "path": string("Output path"),
                    "format": {"type": "string", "enum": ["png", "jpeg", "pptx", "deckcraft", "outline", "rtf", "pdf", "gif"], "description": "Output format (default: from the path's extension)"},
                    "slide": int("Slide index for images"),
                    "all": boolean("All slides (images)"),
                    "scale": num("Pixels per point for images (default 2)"),
                    "layout": {"type": "string", "enum": ["slides", "notes", "handouts"], "description": "PDF page layout (default slides)"},
                    "perPage": {"type": "integer", "enum": [1, 2, 3, 4, 6, 9], "description": "PDF handouts: slides per page (default 6)"},
                    "dpi": num("PDF: raster resolution of slide artwork, 36-600 (default 200)"),
                    "slides": {"type": "array", "items": {"type": "integer", "minimum": 0}, "description": "PDF and GIF: slide indices to export, in order (default all)"},
                    "includeHidden": boolean("PDF and GIF: include hidden slides (default false)"),
                    "textLayer": boolean("PDF: invisible text layer for search, select and copy (default true)"),
                    "frame": boolean("PDF notes/handouts: thin frame around each slide (default true)"),
                    "paper": {"type": "string", "enum": ["letter", "a4", "legal", "a3"], "description": "PDF notes/handouts: paper size (default letter)"},
                    "orientation": {"type": "string", "enum": ["portrait", "landscape"], "description": "PDF notes/handouts: page orientation (default portrait)"},
                    "width": {
                        "oneOf": [
                            {"type": "integer", "minimum": 16, "maximum": 3840},
                            {"type": "string", "enum": ["small", "medium", "large", "extraLarge"]}
                        ],
                        "description": "GIF: frame width in pixels, or a size: small 640, medium 960 (default), large 1280, extraLarge 1920; the height follows the slide's shape"
                    },
                    "secondsPerSlide": {"type": "number", "minimum": 0.1, "maximum": 600, "description": "GIF: seconds each slide shows (default 5)"},
                    "loop": boolean("GIF: loop forever (true, default) or play once (false)")
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
            "One slide's shapes with ids, kinds, boxes [x,y,w,h], placeholder types, text, fills, tables, charts; plus background, transition, notes, and animations as a summary {count, clickSteps} (animations: true for the full list, as run_command animation.get gives it with each one's click step).",
            obj(
                json!({"index": int("Slide index (default: current)"), "animations": boolean("Embed the full animation list instead of the {count, clickSteps} summary (default false)")}),
                &[],
            ),
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
            "Replace a shape's text (keeps its first run's formatting), or one table cell's with `cell`. Leading tabs set bullet levels.",
            obj(
                json!({"id": int("Shape id (from inspect_slide)"), "text": string("New text"), "cell": {"type": "array", "items": {"type": "integer", "minimum": 0}, "description": "Table cell [row, col], 0-based, when id is a table"}}),
                &["id", "text"],
            ),
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
            "Insert a chart (column, bar, line, pie, doughnut, area, scatter, stackedColumn…) with categories and series, optionally at a box.",
            obj(
                json!({"type": string("Chart type"), "title": string("Chart title"), "categories": {"type": "array", "items": {"type": "string"}}, "series": {"type": "array", "items": obj(json!({"name": string("Series name"), "values": {"type": "array", "items": {"type": "number"}}}), &["values"])}, "x": num("Left"), "y": num("Top"), "w": num("Width"), "h": num("Height")}),
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
            "Drive the active tool with pointer events in slide points: down/drag/up/move/doubleclick/contextclick, exactly like the mouse (draws, moves, resizes, rotates, places the caret; contextclick selects like a right click).",
            obj(
                json!({"events": {"type": "array", "items": obj(json!({"kind": string("down|drag|up|move|doubleclick|tripleclick|contextclick"), "x": num("x (pt)"), "y": num("y (pt)"), "mods": mods_schema()}), &["kind", "x", "y"])}, "mods": mods_schema()}),
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
        tool(
            "screenshot",
            "Screenshot",
            &format!(
                "Screenshot of the whole DeckCraft window (UI included), as image content; with `path` the PNG is written there and the answer is text {{path, width, height}}, for a client that cannot read images.{APP_ONLY}"
            ),
            obj(json!({"path": string("Write the PNG to this file and answer {path, width, height} as text")}), &[]),
            true,
        ),
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

/// A tool name where `run_command` and `batch` take a command id runs that tool, so add_shape,
/// add_text_box or go_to_slide can sit in a batch beside commands (#298).
fn is_tool(id: &str) -> bool {
    tool_definitions().iter().any(|t| t["name"] == id)
}

/// One line on a command family (the part of the id before the dot), for the families listing.
/// A family without a line here is listed without `about`.
fn family_about(family: &str) -> Option<&'static str> {
    Some(match family {
        "file" => "New, open, save, export, info, recovery",
        "slide" => "New, delete, move, go, layout, notes, inspect",
        "section" => "Sections of slides",
        "shape" => "Insert, connect, move, fill, line, effects, points",
        "text" => "Edit mode, caret, set/get text, find",
        "format" => "Font, size, colour, bold, align, bullets, spacing",
        "insert" => "Text box, picture, table, chart, media, icon, link",
        "table" => "Rows, columns, cells, merge, style, borders",
        "chart" => "Chart type, data, options",
        "picture" => "Crop, adjust, recolor, effects, compress",
        "media" => "Audio/video playback and options",
        "animation" => "Add, remove, move, timing, get (click steps)",
        "transition" => "Set, options, timing, sound, apply to all",
        "design" => "Theme, palette, fonts, background, slide size",
        "arrange" => "Order, group, align, distribute, rotate, nudge",
        "edit" => "Undo, redo, clipboard, delete, select",
        "selection" => "Selection pane",
        "review" => "Accessibility, spelling, thesaurus, compare",
        "comment" => "Comments: add, reply, resolve, list",
        "master" => "Slide master layouts and placeholders",
        "guide" => "Drawing guides",
        "notes" => "Notes master placeholders",
        "handout" => "Handout master placeholders",
        "show" => "Slide show: start, set up, custom shows",
        "view" => "Master views",
        "window" => "Next window",
        "draw" => "Ink to shape",
        "smartArt" => "Diagrams: nodes, layout, style, from text",
        "font" => "Fonts used, replace, add",
        "document" => "Inspect the presentation",
        "commands" => "This list, from the engine",
        _ => return None,
    })
}

/// The commands grouped by family, compact: `{family, about?, count, commands}`, where `commands`
/// is the names after the dot, space-separated (an array of 322 quoted ids would not fit the
/// few-KB budget this listing exists for).
fn families(all: &[Value]) -> Value {
    let mut fams: Vec<(&str, Vec<&str>)> = vec![];
    for id in all.iter().filter_map(|c| c.get("id").and_then(Value::as_str)) {
        let (fam, name) = id.split_once('.').unwrap_or((id, id));
        match fams.iter_mut().find(|(f, _)| *f == fam) {
            Some((_, names)) => names.push(name),
            None => fams.push((fam, vec![name])),
        }
    }
    Value::Array(
        fams.into_iter()
            .map(|(fam, names)| {
                let mut o = json!({"family": fam, "count": names.len(), "commands": names.join(" ")});
                if let (Some(about), Some(obj)) = (family_about(fam), o.as_object_mut()) {
                    obj.insert("about".into(), json!(about));
                }
                o
            })
            .collect(),
    )
}

/// Replaces every `"$name"` string in a batch item's params with the answer of the earlier item
/// named `as: "name"` (its `id`, else the whole answer). A `$` followed by an identifier that
/// names no earlier item is an error, so a typo is not sent on as text.
fn substitute(v: &mut Value, named: &std::collections::HashMap<String, Value>) -> Result<(), String> {
    match v {
        Value::String(s) => {
            if let Some(key) = s.strip_prefix('$')
                && !key.is_empty()
                && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && !key.starts_with(|c: char| c.is_ascii_digit())
            {
                match named.get(key) {
                    Some(r) => *v = r.clone(),
                    None => return Err(format!("`${key}` names no earlier item of this batch (give an item `\"as\": \"{key}\"` before it)")),
                }
            }
        }
        Value::Array(a) => {
            for x in a {
                substitute(x, named)?;
            }
        }
        Value::Object(o) => {
            for x in o.values_mut() {
                substitute(x, named)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// A command id or a tool name, run either way, as a command's answer.
fn run_either(b: &mut dyn Backend, id: &str, params: Value) -> Result<Value, String> {
    if !is_tool(id) {
        return exec(b, id, params);
    }
    let r = call_tool(b, id, &params);
    let text = r.content.iter().filter_map(|c| c.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("\n");
    if r.is_error { Err(text) } else { Ok(serde_json::from_str(&text).unwrap_or(Value::String(text))) }
}

fn f(a: &Value, k: &str) -> Option<f64> {
    a.get(k).and_then(Value::as_f64)
}

/// The tool's `x`, `y`, `w`, `h` as the command's `rect`; the four keys themselves are taken out,
/// because the command refuses keys it doesn't take.
fn with_rect(a: &Value) -> Value {
    let mut p = a.clone();
    if let Some(o) = p.as_object_mut() {
        if let (Some(x), Some(y), Some(w), Some(h)) = (f(a, "x"), f(a, "y"), f(a, "w"), f(a, "h")) {
            o.insert("rect".into(), json!([x, y, w, h]));
        }
        for k in ["x", "y", "w", "h"] {
            o.remove(k);
        }
    }
    p
}

fn wrap(r: Result<Value, String>) -> ToolResult {
    match r {
        Ok(v) => ToolResult::json(&v),
        Err(e) => ToolResult::error(e),
    }
}

/// Keys other than the tool's schema properties are refused, so a misspelled argument does not
/// succeed with the tool's defaults. Keys inside a free object such as `run_command`'s `params`
/// belong to the command and are left for the command to read.
fn unknown_argument_error(name: &str, args: &Value) -> Option<String> {
    let defs = tool_definitions();
    let def = defs.iter().find(|t| t.get("name").and_then(Value::as_str) == Some(name))?;
    let mut allowed: Vec<&str> = def
        .get("inputSchema")
        .and_then(|s| s.get("properties"))
        .and_then(Value::as_object)
        .map(|p| p.keys().map(String::as_str).collect())
        .unwrap_or_default();
    allowed.sort_unstable();
    let obj = args.as_object()?;
    let mut unknown: Vec<&str> = obj.keys().map(String::as_str).filter(|k| !allowed.contains(k)).collect();
    unknown.sort_unstable();
    let first = unknown.first()?;
    let expected = if allowed.is_empty() { "none".to_string() } else { allowed.join(", ") };
    let (what, list) = if unknown.len() == 1 {
        ("unknown argument", format!("{first:?}"))
    } else {
        ("unknown arguments", unknown.iter().map(|k| format!("{k:?}")).collect::<Vec<_>>().join(", "))
    };
    Some(format!("{name}: {what} {list} (expected: {expected})"))
}

pub fn call_tool(b: &mut dyn Backend, name: &str, a: &Value) -> ToolResult {
    let a = if a.is_object() { a.clone() } else { json!({}) };
    if let Some(msg) = unknown_argument_error(name, &a) {
        return ToolResult::error(msg);
    }
    match name {
        "list_commands" => {
            let all = match b.call("engine.commands", json!({})) {
                Ok(v) => v.as_array().cloned().unwrap_or_default(),
                Err(e) => return ToolResult::error(e),
            };
            if let Some(id) = a.get("id").and_then(Value::as_str) {
                return match all.iter().find(|c| c.get("id").and_then(Value::as_str) == Some(id)) {
                    Some(c) => ToolResult::json(c),
                    None if is_tool(id) => {
                        ToolResult::error(format!("`{id}` is a tool, not a command: call it directly, or list_commands {{filter}} for commands"))
                    }
                    None => ToolResult::error(format!("unknown command `{id}`{}", deckcraft_engine::cmd::like_it(id))),
                };
            }
            let f = a.get("filter").and_then(Value::as_str).unwrap_or("").to_lowercase();
            if f.is_empty() {
                // A bare call used to answer every command (95 KB; 40 KB with brief), and agents
                // made it twice per run. The families fit in a few KB, compact, and `filter` or
                // `id` answer the rest.
                return ToolResult::text(families(&all).to_string());
            }
            let brief = a.get("brief").and_then(Value::as_bool).unwrap_or(false);
            let trim = |c: Value| if brief { json!({"id": c.get("id"), "params": c.get("params")}) } else { c };
            ToolResult::json(&Value::Array(
                all.into_iter()
                    .filter(|c| {
                        c.get("id").and_then(Value::as_str).is_some_and(|i| i.to_lowercase().contains(&f))
                            || c.get("label").and_then(Value::as_str).is_some_and(|i| i.to_lowercase().contains(&f))
                    })
                    .map(trim)
                    .collect(),
            ))
        }
        "run_command" => {
            let Some(c) = a.get("command").and_then(Value::as_str) else { return ToolResult::error("missing `command`") };
            let params = a.get("params").cloned().unwrap_or(json!({}));
            if is_tool(c) {
                return call_tool(b, c, &params);
            }
            wrap(exec(b, c, params))
        }
        "batch" => {
            let keep = a.get("keepGoing").and_then(Value::as_bool).unwrap_or(false);
            let mut out = vec![];
            let mut named: std::collections::HashMap<String, Value> = std::collections::HashMap::new();
            for c in a.get("commands").and_then(Value::as_array).cloned().unwrap_or_default() {
                let id = c.get("command").and_then(Value::as_str).unwrap_or("");
                let mut params = c.get("params").cloned().unwrap_or(json!({}));
                let run = substitute(&mut params, &named).and_then(|()| run_either(b, id, params));
                match run {
                    Ok(v) => {
                        if let Some(name) = c.get("as").and_then(Value::as_str) {
                            named.insert(name.to_string(), v.get("id").cloned().unwrap_or_else(|| v.clone()));
                        }
                        out.push(json!({"command": id, "ok": true, "result": v}));
                    }
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
        "insert_picture" => wrap(exec(b, "insert.picture", with_rect(&a))),
        "insert_table" => wrap(exec(b, "insert.table", with_rect(&a))),
        "insert_chart" => wrap(exec(b, "insert.chart", with_rect(&a))),
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
                return ToolResult::error(format!(
                    "screenshot: {NEEDS_APP}. Headless, render_slide shows a slide, and export {{path, format: \"png\", slide}} writes one to a file."
                ));
            }
            // With a path the app writes the file and answers {path, width, height}: text an agent
            // behind a text-only bridge can read (it used to have to guess call_app ui.screenshot).
            let params = match a.get("path").and_then(Value::as_str) {
                Some(path) => json!({"path": path}),
                None => json!({"base64": true}),
            };
            match b.call("ui.screenshot", params) {
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
        other if deckcraft_engine::find_command(other).is_some() => {
            ToolResult::error(format!("`{other}` is a command, not a tool: run_command {{\"command\": \"{other}\", \"params\": {{…}}}}"))
        }
        other => ToolResult::error(format!("unknown tool `{other}`")),
    }
}
