//! Developer tab: macros. DeckCraft doesn't run macros itself; an add-in that declares
//! `"macros": {"language": "VBA"}` does (protocol 1.1, `macros/*`), and reaches the presentation
//! back through the host requests answered here ([`host_call`]: `host/sync` on the same objects as
//! Office add-ins, the presentation's `vbaProject.bin`, messages). Running a macro is one undoable
//! step.
//!
//! Record Macro (`developer.recordMacro`) is a DeckCraft extra: PowerPoint dropped its macro
//! recorder, DeckCraft keeps one. It notes where the journal stands, and on Stop sends the
//! commands run since (`[{command, params}]`, DeckCraft command ids) to the add-in's
//! `macros/record`, which answers the macro's code; that goes at the end of a module.
//!
//! The presentation's ribbon (`developer.ribbon`, `addRibbonButton`…): tabs, groups and buttons
//! that run its macros, saved with it (`customUI14.xml`) so PowerPoint shows them too. A button runs
//! its macro as PowerPoint does, with the button as the argument (`Sub Go(control As
//! IRibbonControl)`).
//!
//! Macro security (`prefs.macro_security`): `disable` runs nothing; `prompt` (the default) runs a
//! presentation's macros once the user enabled them for it (`developer.enableMacros`, the message
//! bar's Enable Content); `enable` runs all.

use deckcraft_model::ribbon::{MAX_RIBBON_ITEMS, builtin_tab_id, builtin_tab_name};
use deckcraft_model::{Ribbon, RibbonButton, RibbonGroup, RibbonTab};
use deckcraft_plugin::office::{Action, Permission};
use deckcraft_plugin::{HostError, call_macros, macro_plugins};
use serde_json::{Value as Json, json};

use super::{CommandSpec, always, bad, cmd, str_param};
use crate::{EngineError, Result, Session, UiRequest};

/// Largest `vbaProject.bin` a plugin may write.
const MAX_PROJECT: usize = 64 << 20;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "developer.macros", "Macros", ["Developer", "Code"], None, "{} → {macros: [{name, module, procedure, plugin, language}], enabled, security}. The presentation's macros", always, macros),
        cmd!(
            "developer.runMacro",
            "Run Macro",
            ["Developer", "Code"],
            None,
            "{name, plugin?} → {ran}. Runs a macro (Module1.Hello) as one undoable step; needs macros enabled for the presentation",
            always,
            run_macro
        ),
        cmd!(query "developer.modules", "Visual Basic", ["Developer", "Code"], None, "{plugin?} → {modules: [{name, kind, source}]}. The presentation's code modules", always, modules),
        cmd!(
            "developer.saveModule",
            "Save Module",
            ["Developer", "Code"],
            None,
            "{name, source, plugin?} → {}. Adds or replaces a code module (saved with the presentation as .pptm)",
            always,
            save_module
        ),
        cmd!("developer.deleteModule", "Remove Module", ["Developer", "Code"], None, "{name, plugin?} → {}", always, delete_module),
        cmd!(query "developer.recordToCode", "Macro from Steps", ["Developer", "Code"], None, "{steps: [{command, params}], name?, plugin?} → {source}. The steps as a macro's code", always, record_to_code),
        cmd!(
            noundo "developer.recordMacro",
            "Record Macro",
            ["Developer", "Code"],
            None,
            "{stop?: bool, module?: \"Module1\", name?: \"Macro1\"} → {recording} or, when stopping, {module, source, steps}. Records what you do as a macro in the module (a DeckCraft extra: PowerPoint has no recorder)",
            always,
            record_macro
        ),
        cmd!(
            query "developer.ribbon",
            "Ribbon Buttons",
            ["Developer", "Code"],
            None,
            "{} → {tabs: [{tab, id, builtin, label, groups: [{id, label, buttons: [{id, label, macro, image, large, screentip, tag}]}]}], editable}. The presentation's own ribbon buttons",
            always,
            ribbon
        ),
        cmd!(
            "developer.addRibbonButton",
            "Add Ribbon Button",
            ["Developer", "Code"],
            None,
            "{macro, label?, tab?: \"Macros\"|Home|Insert…, group?: \"Macros\", image?, large?, screentip?, tag?} → {id}. A button that runs the macro, saved with the presentation",
            always,
            add_ribbon_button
        ),
        cmd!(
            "developer.editRibbonButton",
            "Edit Ribbon Button",
            ["Developer", "Code"],
            None,
            "{id, macro?, label?, image?, large?, screentip?, tag?} → {id}",
            always,
            edit_ribbon_button
        ),
        cmd!("developer.removeRibbonButton", "Remove Ribbon Button", ["Developer", "Code"], None, "{id} → {}", always, remove_ribbon_button),
        cmd!(
            "developer.pressRibbonButton",
            "Press Ribbon Button",
            ["Developer", "Code"],
            None,
            "{id} → {ran}. Runs the button's macro, as clicking it does",
            always,
            press_ribbon_button
        ),
        cmd!(noundo "developer.enableMacros", "Enable Content", ["Developer", "Code"], None, "{on?: bool} → {enabled}. Lets this presentation's macros run (until it closes)", always, enable_macros),
        cmd!(
            noundo "developer.macroSecurity",
            "Macro Security",
            ["Developer", "Code"],
            None,
            "{level?: disable|prompt|enable} → {level}. Which presentations' macros may run",
            always,
            macro_security
        ),
    ]
}

/// The plugin that runs macros: `plugin` when given, else the first one loaded.
fn macro_plugin(id: &str, p: &Json) -> Result<(String, String)> {
    let all = macro_plugins();
    match str_param(p, "plugin") {
        Some(want) => all.into_iter().find(|(pid, _)| pid == want).ok_or_else(|| bad(id, format!("no macro add-in `{want}` is loaded"))),
        None => all.into_iter().next().ok_or_else(|| bad(id, "no add-in runs macros: install one (VBA) in File › Options › Add-ins")),
    }
}

/// Whether the active presentation's macros may run.
pub fn macros_allowed(s: &Session) -> bool {
    match s.prefs.macro_security.as_str() {
        "enable" => true,
        "disable" => false,
        _ => s.active().is_some_and(|d| d.macros_enabled),
    }
}

/// Answers a plugin's request to the host (protocol 1.1) for the active presentation.
pub fn host_call(s: &mut Session, plugin: &str, method: &str, params: &Json) -> std::result::Result<Json, HostError> {
    let fail = |message: String| HostError { code: -32000, message, data: Json::Null };
    match method {
        "host/sync" => {
            let actions: Vec<Action> = serde_json::from_value(params.get("actions").cloned().unwrap_or(Json::Array(Vec::new())))
                .map_err(|e| fail(format!("`actions`: {e}")))?;
            if actions.len() > deckcraft_plugin::office::MAX_ACTIONS {
                return Err(fail("too many actions".into()));
            }
            // A plugin is a program the user installed: the presentation is open to it.
            match super::office::run_actions(s, plugin, Permission::ReadWriteDocument, &actions) {
                Ok(results) => Ok(json!({"results": results})),
                Err(error) => {
                    let message = error.get("message").and_then(Json::as_str).unwrap_or("failed").to_string();
                    Err(HostError { code: -32000, message, data: error })
                }
            }
        }
        "host/vbaProject/get" => {
            let bytes = s.active().and_then(|d| d.doc.vba_project.as_ref().map(|b| super::base64_encode(b)));
            Ok(json!({"base64": bytes}))
        }
        "host/vbaProject/set" => {
            let b64 = params.get("base64").and_then(Json::as_str).ok_or_else(|| fail("`base64` is required".into()))?;
            if b64.len() > MAX_PROJECT / 3 * 4 + 8 {
                return Err(fail("the project is larger than 64 MiB".into()));
            }
            let bytes = super::base64_decode(b64).ok_or_else(|| fail("invalid base64".into()))?;
            s.edit(|doc, _| {
                doc.vba_project = Some(bytes.into());
                // A signature over the old project doesn't hold for the new one.
                doc.vba_parts.retain(|p| !p.rel_type.to_ascii_lowercase().contains("signature"));
                Ok(())
            })
            .map_err(|e| fail(e.to_string()))?;
            Ok(json!({}))
        }
        "host/message" => {
            let text: String = params.get("text").and_then(Json::as_str).unwrap_or("").chars().take(4000).collect();
            s.ui_requests.push(UiRequest::Message { text });
            // vbOK: the message doesn't wait for the user yet.
            Ok(json!({"result": 1}))
        }
        "host/log" => {
            let text: String = params.get("text").and_then(Json::as_str).unwrap_or("").chars().take(4000).collect();
            log::info!("add-in {plugin}: {text}");
            Ok(json!({}))
        }
        _ => Err(HostError::unknown()),
    }
}

/// Sends `method` to the macro plugin, answering its host requests; what its host requests change
/// is part of the running command's one undo step, and a failure takes it back.
fn call(s: &mut Session, plugin: &str, method: &str, params: Json) -> Result<Json> {
    let before = super::plugins::snapshot(s);
    let r = call_macros(plugin, method, params, &mut |m, p| host_call(s, plugin, m, p));
    if r.is_err() {
        super::plugins::restore(s, before);
    }
    // The add-in's own words (a macro's `Run-time error '9': Subscript out of range`).
    r.map_err(EngineError::Other)
}

fn macros(s: &mut Session, p: &Json) -> Result<Json> {
    let mut out = Vec::new();
    let plugins: Vec<(String, String)> = match str_param(p, "plugin") {
        Some(_) => vec![macro_plugin("developer.macros", p)?],
        None => macro_plugins(),
    };
    for (plugin, language) in plugins {
        let r = call(s, &plugin, "macros/list", json!({}))?;
        for m in r.get("macros").and_then(Json::as_array).into_iter().flatten().take(10_000) {
            let mut m = m.clone();
            m["plugin"] = json!(plugin);
            m["language"] = json!(language);
            out.push(m);
        }
    }
    Ok(json!({"macros": out, "enabled": macros_allowed(s), "security": s.prefs.macro_security}))
}

fn run_macro(s: &mut Session, p: &Json) -> Result<Json> {
    const ID: &str = "developer.runMacro";
    let name = str_param(p, "name").filter(|n| !n.trim().is_empty()).ok_or_else(|| bad(ID, "missing `name`"))?.to_string();
    if !macros_allowed(s) {
        return Err(EngineError::Disabled(
            ID.into(),
            "macros are disabled for this presentation: Enable Content, or change Developer › Macro Security".into(),
        ));
    }
    let (plugin, _) = macro_plugin(ID, p)?;
    let mut params = json!({"name": name});
    // A ribbon button's macro gets the button (`control.Id`, `control.Tag`).
    if let Some(c) = p.get("control").filter(|c| c.is_object()) {
        params["control"] = json!({"id": c.get("id").cloned().unwrap_or(json!("")), "tag": c.get("tag").cloned().unwrap_or(json!(""))});
    }
    // Protocol 1.2: the macro knows which document it runs on.
    params["context"] = super::plugins::macro_context(s);
    call(s, &plugin, "macros/run", params)?;
    Ok(json!({"ran": name}))
}

/// The presentation's ribbon: its own (editable), or the buttons of a richer one kept from the file.
pub fn current_ribbon(s: &Session) -> (Ribbon, bool) {
    match s.active() {
        Some(d) => deckcraft_pptx::custom_ui::current(&d.doc),
        None => (Ribbon::default(), false),
    }
}

/// The DeckCraft tab a ribbon tab shows on.
pub fn tab_name(t: &RibbonTab) -> String {
    match &t.builtin {
        Some(mso) => builtin_tab_name(mso).map(str::to_string).unwrap_or_else(|| mso.trim_start_matches("Tab").to_string()),
        None if !t.label.trim().is_empty() => t.label.clone(),
        None => t.id.clone(),
    }
}

fn ribbon(s: &mut Session, _p: &Json) -> Result<Json> {
    let (r, editable) = current_ribbon(s);
    let tabs: Vec<Json> = r
        .tabs
        .iter()
        .map(|t| {
            let groups: Vec<Json> = t
                .groups
                .iter()
                .map(|g| {
                    let buttons: Vec<Json> = g
                        .buttons
                        .iter()
                        .map(|b| json!({"id": b.id, "label": b.label, "macro": b.on_action, "image": b.image, "large": b.large, "screentip": b.screentip, "tag": b.tag}))
                        .collect();
                    json!({"id": g.id, "label": g.label, "buttons": buttons})
                })
                .collect();
            json!({"tab": tab_name(t), "id": t.id, "builtin": t.builtin, "label": t.label, "groups": groups})
        })
        .collect();
    Ok(json!({"tabs": tabs, "editable": editable}))
}

/// Text for a ribbon attribute: trimmed, at most 1024 characters (RibbonX's limit for labels).
fn text(p: &Json, key: &str) -> Option<String> {
    str_param(p, key).map(|v| v.trim().chars().take(1024).collect())
}

/// Changes the presentation's editable ribbon (one undo step); refused when the file brought a
/// richer ribbon DeckCraft keeps but doesn't change.
fn edit_ribbon<T>(s: &mut Session, id: &str, f: impl FnOnce(&mut Ribbon) -> Result<T>) -> Result<T> {
    if !current_ribbon(s).1 {
        return Err(EngineError::Disabled(
            id.into(),
            "this presentation's ribbon came from another program with more than buttons: DeckCraft runs it but doesn't change it".into(),
        ));
    }
    s.edit(|doc, _| {
        let r = doc.ribbon.get_or_insert_with(Ribbon::default);
        let out = f(r)?;
        // A presentation left without buttons has no ribbon part.
        if r.is_empty() {
            doc.ribbon = None;
        }
        Ok(out)
    })
}

/// A macro name a button may run: `Module1.Go`, `Go`, or `'Deck.pptm'!Module1.Go`.
fn macro_name(id: &str, p: &Json) -> Result<Option<String>> {
    let Some(m) = text(p, "macro") else { return Ok(None) };
    let ok = !m.is_empty() && m.len() <= 255 && m.chars().all(|c| c.is_alphanumeric() || "_.!'[] -".contains(c));
    if !ok {
        return Err(bad(id, "`macro` is a macro's name, like Module1.Hello"));
    }
    Ok(Some(m))
}

/// `image`: absent (`None`), cleared (`Some(None)`) or a built-in picture's name.
fn image(id: &str, p: &Json) -> Result<Option<Option<String>>> {
    match p.get("image") {
        None => Ok(None),
        Some(Json::Null) => Ok(Some(None)),
        Some(Json::String(v)) if v.is_empty() => Ok(Some(None)),
        Some(Json::String(v)) if v.len() <= 64 && v.chars().all(|c| c.is_ascii_alphanumeric()) => Ok(Some(Some(v.clone()))),
        Some(_) => Err(bad(id, "`image` is a built-in picture's name, like HappyFace")),
    }
}

fn add_ribbon_button(s: &mut Session, p: &Json) -> Result<Json> {
    const ID: &str = "developer.addRibbonButton";
    let on_action = macro_name(ID, p)?.ok_or_else(|| bad(ID, "missing `macro`"))?;
    let image = image(ID, p)?.flatten();
    let label = text(p, "label").filter(|l| !l.is_empty()).unwrap_or_else(|| on_action.rsplit(['.', '!']).next().unwrap_or(&on_action).to_string());
    let tab = text(p, "tab").filter(|t| !t.is_empty()).unwrap_or_else(|| "Macros".into());
    let group = text(p, "group").filter(|g| !g.is_empty()).unwrap_or_else(|| "Macros".into());
    let large = p.get("large").and_then(Json::as_bool).unwrap_or(true);
    let screentip = text(p, "screentip").filter(|t| !t.is_empty());
    let tag = text(p, "tag").filter(|t| !t.is_empty());
    edit_ribbon(s, ID, |r| {
        let builtin = builtin_tab_id(&tab);
        let ti = match r.tabs.iter().position(|t| match builtin {
            Some(mso) => t.builtin.as_deref().is_some_and(|b| b.eq_ignore_ascii_case(mso)),
            None => t.builtin.is_none() && tab_name(t).eq_ignore_ascii_case(&tab),
        }) {
            Some(i) => i,
            None => {
                if r.tabs.len() >= MAX_RIBBON_ITEMS {
                    return Err(bad(ID, "the ribbon has too many tabs"));
                }
                let id = if builtin.is_some() { String::new() } else { r.fresh_id("craftTab") };
                r.tabs.push(RibbonTab {
                    id,
                    builtin: builtin.map(str::to_string),
                    label: if builtin.is_some() { String::new() } else { tab.clone() },
                    groups: vec![],
                });
                r.tabs.len() - 1
            }
        };
        let group_id = r.fresh_id("craftGroup");
        let button_id = r.fresh_id("craftButton");
        let t = r.tabs.get_mut(ti).ok_or_else(|| bad(ID, "no such tab"))?;
        let gi = match t.groups.iter().position(|g| g.label.eq_ignore_ascii_case(&group)) {
            Some(i) => i,
            None => {
                if t.groups.len() >= MAX_RIBBON_ITEMS {
                    return Err(bad(ID, "the tab has too many groups"));
                }
                t.groups.push(RibbonGroup { id: group_id, label: group, buttons: vec![] });
                t.groups.len() - 1
            }
        };
        let g = t.groups.get_mut(gi).ok_or_else(|| bad(ID, "no such group"))?;
        if g.buttons.len() >= MAX_RIBBON_ITEMS {
            return Err(bad(ID, "the group has too many buttons"));
        }
        g.buttons.push(RibbonButton { id: button_id.clone(), label, on_action, image, large, screentip, supertip: None, tag });
        Ok(json!({"id": button_id}))
    })
}

fn edit_ribbon_button(s: &mut Session, p: &Json) -> Result<Json> {
    const ID: &str = "developer.editRibbonButton";
    let id = str_param(p, "id").ok_or_else(|| bad(ID, "missing `id`"))?.to_string();
    let on_action = macro_name(ID, p)?;
    let image = image(ID, p)?;
    edit_ribbon(s, ID, |r| {
        let b = r.button_mut(&id).ok_or_else(|| bad(ID, format!("no ribbon button `{id}`")))?;
        if let Some(m) = on_action {
            b.on_action = m;
        }
        if let Some(l) = text(p, "label").filter(|l| !l.is_empty()) {
            b.label = l;
        }
        if let Some(i) = image {
            b.image = i;
        }
        if let Some(l) = p.get("large").and_then(Json::as_bool) {
            b.large = l;
        }
        if let Some(t) = text(p, "screentip") {
            b.screentip = Some(t).filter(|t| !t.is_empty());
        }
        if let Some(t) = text(p, "tag") {
            b.tag = Some(t).filter(|t| !t.is_empty());
        }
        Ok(json!({"id": id}))
    })
}

fn remove_ribbon_button(s: &mut Session, p: &Json) -> Result<Json> {
    const ID: &str = "developer.removeRibbonButton";
    let id = str_param(p, "id").ok_or_else(|| bad(ID, "missing `id`"))?.to_string();
    edit_ribbon(s, ID, |r| if r.remove_button(&id) { Ok(json!({})) } else { Err(bad(ID, format!("no ribbon button `{id}`"))) })
}

fn press_ribbon_button(s: &mut Session, p: &Json) -> Result<Json> {
    const ID: &str = "developer.pressRibbonButton";
    let id = str_param(p, "id").ok_or_else(|| bad(ID, "missing `id`"))?;
    let (r, _) = current_ribbon(s);
    let b = r.button(id).ok_or_else(|| bad(ID, format!("no ribbon button `{id}`")))?;
    // The form `'Deck.pptm'!Module1.Go` names this presentation's macro.
    let name = b.on_action.rsplit('!').next().unwrap_or(&b.on_action).to_string();
    run_macro(s, &json!({"name": name, "control": {"id": b.id, "tag": b.tag.clone().unwrap_or_default()}}))
}

/// Most steps one recording sends to the add-in (the journal holds at most 10 000).
const MAX_RECORDED_STEPS: usize = 10_000;

fn record_to_code(s: &mut Session, p: &Json) -> Result<Json> {
    const ID: &str = "developer.recordToCode";
    let steps = p
        .get("steps")
        .and_then(Json::as_array)
        .filter(|a| a.len() <= MAX_RECORDED_STEPS)
        .cloned()
        .ok_or_else(|| bad(ID, "missing `steps` (a list of at most 10000 {command, params})"))?;
    let (plugin, _) = macro_plugin(ID, p)?;
    let mut params = json!({"steps": steps});
    if let Some(name) = macro_proc_name(ID, p)? {
        params["name"] = json!(name);
    }
    call(s, &plugin, "macros/record", params)
}

/// `name`: the recorded macro's name (a VBA identifier), when given.
fn macro_proc_name(id: &str, p: &Json) -> Result<Option<String>> {
    let Some(name) = str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()) else { return Ok(None) };
    let ok = name.len() <= 255 && name.chars().all(|c| c.is_alphanumeric() || c == '_') && name.chars().next().is_some_and(char::is_alphabetic);
    if !ok {
        return Err(bad(id, "`name` is a macro's name: letters, digits and _, starting with a letter"));
    }
    Ok(Some(name.to_string()))
}

/// Journal entries a recording leaves out: the Developer tab itself and what add-ins do.
fn recordable(id: &str) -> bool {
    !id.starts_with("developer.") && !id.starts_with("plugins.") && !id.starts_with("office.")
}

fn record_macro(s: &mut Session, p: &Json) -> Result<Json> {
    const ID: &str = "developer.recordMacro";
    let stop = p.get("stop").and_then(Json::as_bool).unwrap_or(s.recording.is_some());
    let name = macro_proc_name(ID, p)?;
    if !stop {
        macro_plugin(ID, p)?;
        s.recording = Some(s.journal.len());
        return Ok(json!({"recording": true}));
    }
    let Some(start) = s.recording.take() else { return Ok(json!({"recording": false})) };
    let steps: Vec<Json> = s
        .journal
        .iter()
        .skip(start)
        .filter(|(id, _)| recordable(id))
        .take(MAX_RECORDED_STEPS)
        .map(|(id, p)| json!({"command": id, "params": p}))
        .collect();
    let (plugin, _) = macro_plugin(ID, p)?;
    let mut params = json!({"steps": steps});
    if let Some(n) = &name {
        params["name"] = json!(n);
    }
    let code = call(s, &plugin, "macros/record", params)?;
    let source = code.get("source").and_then(Json::as_str).unwrap_or("").to_string();
    // The new macro goes at the end of the module (Module1 by default).
    let module = str_param(p, "module").map(str::trim).filter(|m| !m.is_empty()).unwrap_or("Module1").chars().take(255).collect::<String>();
    let existing = call(s, &plugin, "macros/modules", json!({}))?;
    let before = existing
        .get("modules")
        .and_then(Json::as_array)
        .and_then(|m| m.iter().find(|x| x.get("name").and_then(Json::as_str) == Some(module.as_str())))
        .and_then(|m| m.get("source").and_then(Json::as_str))
        .map(|t| format!("{}\r\n\r\n", t.trim_end()))
        .unwrap_or_default();
    call(s, &plugin, "macros/saveModule", json!({"name": module, "source": format!("{before}{source}")}))?;
    Ok(json!({"recording": false, "module": module, "source": source, "steps": steps.len()}))
}

fn modules(s: &mut Session, p: &Json) -> Result<Json> {
    let (plugin, _) = macro_plugin("developer.modules", p)?;
    let mut r = call(s, &plugin, "macros/modules", json!({}))?;
    r["plugin"] = json!(plugin);
    Ok(r)
}

fn save_module(s: &mut Session, p: &Json) -> Result<Json> {
    const ID: &str = "developer.saveModule";
    let name = str_param(p, "name").filter(|n| !n.trim().is_empty()).ok_or_else(|| bad(ID, "missing `name`"))?;
    let source = str_param(p, "source").ok_or_else(|| bad(ID, "missing `source`"))?;
    if source.len() > 16 << 20 {
        return Err(bad(ID, "a module is limited to 16 MiB"));
    }
    let (plugin, _) = macro_plugin(ID, p)?;
    call(s, &plugin, "macros/saveModule", json!({"name": name, "source": source}))
}

fn delete_module(s: &mut Session, p: &Json) -> Result<Json> {
    const ID: &str = "developer.deleteModule";
    let name = str_param(p, "name").filter(|n| !n.trim().is_empty()).ok_or_else(|| bad(ID, "missing `name`"))?;
    let (plugin, _) = macro_plugin(ID, p)?;
    call(s, &plugin, "macros/deleteModule", json!({"name": name}))
}

fn enable_macros(s: &mut Session, p: &Json) -> Result<Json> {
    let on = p.get("on").and_then(Json::as_bool).unwrap_or(true);
    let d = s.active_mut().ok_or_else(|| bad("developer.enableMacros", "no presentation is open"))?;
    d.macros_enabled = on;
    Ok(json!({"enabled": on}))
}

fn macro_security(s: &mut Session, p: &Json) -> Result<Json> {
    if let Some(level) = str_param(p, "level") {
        if !matches!(level, "disable" | "prompt" | "enable") {
            return Err(bad("developer.macroSecurity", "`level` is disable, prompt or enable"));
        }
        s.prefs.macro_security = level.to_string();
    }
    Ok(json!({"level": s.prefs.macro_security}))
}
