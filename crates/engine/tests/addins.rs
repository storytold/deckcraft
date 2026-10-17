//! Add-ins and macros through a real add-in process (protocol 1.1): the add-in reads and writes
//! the presentation while it works (`host/sync`), keeps its code in the presentation's
//! `vbaProject.bin`, and every command or macro is one undo step. The add-in is this test binary
//! started again with `--serve` (no libtest harness, so standard output is the protocol's alone).

use std::io::{BufRead, Write};

use deckcraft_engine::Session;
use serde_json::{Value as Json, json};

const PLUGIN: &str = "org.deckcraft.test-addin";

fn main() {
    if std::env::args().any(|a| a == "--serve") {
        serve();
        return;
    }
    let tests: [(&str, fn()); 13] = [
        ("commands_answer_engine_commands_as_one_step", commands_answer_engine_commands_as_one_step),
        ("macros_drive_the_presentation_as_one_undo_step", macros_drive_the_presentation_as_one_undo_step),
        ("a_failing_macro_changes_nothing", a_failing_macro_changes_nothing),
        ("macro_security_gates_running", macro_security_gates_running),
        ("modules_live_in_the_presentations_vba_project", modules_live_in_the_presentations_vba_project),
        ("ribbon_buttons_run_their_macro_and_are_saved", ribbon_buttons_run_their_macro_and_are_saved),
        ("recording_sends_the_steps_and_keeps_the_code", recording_sends_the_steps_and_keeps_the_code),
        ("commands_and_macros_see_the_document", commands_and_macros_see_the_document),
        ("programmatic_runs_return_open_and_dialog", programmatic_runs_return_open_and_dialog),
        ("dialog_steps_that_change_nothing_are_no_edit", dialog_steps_that_change_nothing_are_no_edit),
        ("declared_events_follow_opens_saves_and_closes", declared_events_follow_opens_saves_and_closes),
        ("events_never_hold_up_or_fail_a_save", events_never_hold_up_or_fail_a_save),
        ("events_cant_change_the_presentation", events_cant_change_the_presentation),
    ];
    for (name, t) in tests {
        t();
        println!("test {name} ... ok");
    }
}

/// The add-in. Commands: `title` adds a slide titled after the context's slide count; `broken`
/// adds a slide and then names a command that doesn't exist. Macros: `Module1.Stamp` adds a text
/// box on the first slide through `host/sync` and says so; `Module1.Broken` adds a slide, then
/// asks for a slide that isn't there. Modules are the vbaProject's bytes taken as UTF-8 text.
fn serve() {
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    let mut out = std::io::stdout();
    let mut next = 0u32;
    // Sends a host request and waits for its answer.
    let mut host = |out: &mut std::io::Stdout, lines: &mut std::io::Lines<std::io::StdinLock<'_>>, method: &str, params: Json| -> Json {
        next += 1;
        let id = format!("h{next}");
        writeln!(out, "{}", json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})).unwrap();
        out.flush().unwrap();
        for line in lines.by_ref() {
            let msg: Json = serde_json::from_str(&line.unwrap()).unwrap();
            if msg["id"] == json!(id) {
                return msg;
            }
        }
        Json::Null
    };
    let slides = json!([{"p": "presentation"}, {"p": "slides"}]);
    let first_shapes = json!([{"p": "presentation"}, {"p": "slides"}, {"m": "getItemAt", "a": [0]}, {"p": "shapes"}]);
    while let Some(Ok(line)) = lines.next() {
        let Ok(msg) = serde_json::from_str::<Json>(&line) else { continue };
        let Some(id) = msg.get("id").cloned() else { break };
        let p = msg["params"].clone();
        let answer = match msg["method"].as_str() {
            Some("initialize") => json!({"result": {"protocolVersion": 1}}),
            Some("commands/run") if p["command"] == "title" => {
                let n = p["context"]["slides"].as_u64().unwrap_or(0);
                json!({"result": {"commands": [{"command": "slide.new", "params": {"layout": "title", "title": format!("After {n}")}}], "message": "added"}})
            }
            // Protocol 1.2.
            Some("commands/run") if p["command"] == "ctx" => json!({"result": {"result": p["context"]}}),
            Some("commands/run") if p["command"] == "opener" => {
                json!({"result": {"open": {"path": p["params"]["path"], "label": "Cloud · deck", "readOnly": true}, "message": "opening"}})
            }
            Some("commands/run") if p["command"] == "browse" => match p["params"]["id"].as_str() {
                // A folder: another dialog, one level down.
                Some(folder) if folder.ends_with('/') => json!({"result": {"dialog": {
                    "title": format!("In {folder}"), "kind": "list", "items": [{"id": format!("{folder}b.pptx"), "label": "b.pptx", "icon": "presentation"}],
                    "command": "browse", "buttons": [{"label": "Up", "command": "browse"}]
                }}}),
                Some(file) => json!({"result": {"message": format!("picked {file}")}}),
                None => json!({"result": {"dialog": {
                    "title": "Browse", "kind": "list", "prompt": "Documents", "multi": false,
                    "items": [{"id": "/docs/", "label": "docs", "detail": "Folder", "icon": "folder"}, {"id": "/a.pptx", "label": "a.pptx"}],
                    "command": "browse", "params": {"account": "me"}, "okLabel": "Open"
                }}}),
            },
            // A dialog step that reads the presentation and changes nothing.
            Some("commands/run") if p["command"] == "peek" => {
                let r = host(&mut out, &mut lines, "host/sync", json!({"actions": [{"op": "load", "path": slides, "props": ["count"]}]}));
                let n = r["result"]["results"][0]["count"].as_u64().unwrap_or(0);
                json!({"result": {"dialog": {"kind": "message", "prompt": format!("{n} slides"), "buttons": [{"label": "Again", "command": "peek"}]}}})
            }
            Some("commands/run") if p["command"] == "stray" => {
                json!({"result": {"dialog": {"kind": "message", "prompt": "hi", "buttons": [{"label": "Elsewhere", "command": "notMine"}]}}})
            }
            Some("commands/run") if p["command"] == "combo" => json!({"result": {
                "dialog": {"kind": "message", "prompt": "Signed in"}, "result": {"account": "me"}, "message": "connected",
                "commands": [{"command": "slide.new", "params": {}}]
            }}),
            Some("commands/run") if p["command"] == "malformed" => json!({"result": {"dialog": {"kind": "web", "url": "https://x"}}}),
            Some("macros/run") if p["name"] == "Module1.Where" => {
                let doc = &p["context"]["document"];
                host(
                    &mut out,
                    &mut lines,
                    "host/message",
                    json!({"text": format!("in {} dirty {}", doc["name"].as_str().unwrap_or("?"), doc["dirty"])}),
                );
                json!({"result": {}})
            }
            Some(m) if m.starts_with("events/") => {
                let name = p["name"].as_str().unwrap_or("").to_string();
                if name.contains("slow") {
                    std::thread::sleep(std::time::Duration::from_secs(4));
                }
                if name.contains("hang") && m == "events/documentClosed" {
                    std::thread::sleep(std::time::Duration::from_secs(8));
                }
                if name.contains("fail") {
                    json!({"error": {"code": 1, "message": "upload failed"}})
                } else if m == "events/documentOpened" {
                    // Nothing of the presentation can be read or changed during an event.
                    let r = host(&mut out, &mut lines, "host/sync", json!({"actions": [{"op": "load", "path": slides, "props": ["count"]}]}));
                    let w = host(
                        &mut out,
                        &mut lines,
                        "host/sync",
                        json!({"actions": [{"op": "call", "path": first_shapes, "method": "addTextBox", "args": ["x", {"left": 1, "top": 1, "width": 9, "height": 9}]}]}),
                    );
                    let g = host(&mut out, &mut lines, "host/vbaProject/get", json!({}));
                    let v = host(&mut out, &mut lines, "host/vbaProject/set", json!({"base64": ""}));
                    host(&mut out, &mut lines, "host/message", json!({"text": "noted"}));
                    let code = |x: &Json| x["error"]["data"]["code"].as_str().unwrap_or("allowed").to_string();
                    json!({"result": {"message": format!("opened {name}: read {} write {} vba {} {}", code(&r), code(&w), code(&g), code(&v))}})
                } else {
                    json!({"result": {"message": format!("{} {name} {}", &m[7..], p["format"].as_str().unwrap_or("-"))}})
                }
            }
            Some("commands/run") if p["command"] == "broken" => {
                json!({"result": {"commands": [{"command": "slide.new", "params": {"layout": "blank"}}, {"command": "no.suchCommand", "params": {}}]}})
            }
            Some("macros/list") => {
                json!({"result": {"macros": [{"name": "Module1.Stamp", "module": "Module1", "procedure": "Stamp"}, {"name": "Module1.Broken", "module": "Module1", "procedure": "Broken"}]}})
            }
            Some("macros/run") if p["name"] == "Module1.Stamp" || p["name"] == "Stamp" => {
                let r = host(&mut out, &mut lines, "host/sync", json!({"actions": [{"op": "load", "path": slides, "props": ["count"]}]}));
                let n = r["result"]["results"][0]["count"].as_u64().unwrap_or(0);
                let tag = p["control"]["tag"].as_str().unwrap_or("").to_string();
                host(
                    &mut out,
                    &mut lines,
                    "host/sync",
                    json!({"actions": [{"op": "call", "path": first_shapes, "method": "addTextBox", "args": [format!("{n} slides{tag}"), {"left": 10, "top": 10, "width": 200, "height": 40}]}]}),
                );
                host(&mut out, &mut lines, "host/message", json!({"text": format!("stamped {n}")}));
                json!({"result": {}})
            }
            Some("macros/run") if p["name"] == "Module1.Broken" => {
                host(&mut out, &mut lines, "host/sync", json!({"actions": [{"op": "call", "path": slides, "method": "add", "args": [{}]}]}));
                let bad = host(
                    &mut out,
                    &mut lines,
                    "host/sync",
                    json!({"actions": [{"op": "load", "path": [{"p": "presentation"}, {"p": "slides"}, {"m": "getItemAt", "a": [999]}], "props": ["id"]}]}),
                );
                json!({"error": {"code": 1, "message": format!("Run-time error '9': Subscript out of range ({})", bad["error"]["data"]["code"].as_str().unwrap_or(""))}})
            }
            Some("macros/modules") => {
                let r = host(&mut out, &mut lines, "host/vbaProject/get", json!({}));
                let text = r["result"]["base64"]
                    .as_str()
                    .map(|b| String::from_utf8(deckcraft_engine::cmd::base64_decode(b).unwrap()).unwrap())
                    .unwrap_or_default();
                json!({"result": {"modules": if text.is_empty() { json!([]) } else { json!([{"name": "Module1", "kind": "standard", "source": text}]) }}})
            }
            // The recorder: one comment line per step, under the name asked for.
            Some("macros/record") => {
                let steps = p["steps"].as_array().cloned().unwrap_or_default();
                let lines: Vec<String> = steps
                    .iter()
                    .map(|st| {
                        format!("' {} {}", st["command"].as_str().unwrap_or("?"), if st.get("params").is_some() { "with params" } else { "bare" })
                    })
                    .collect();
                let name = p["name"].as_str().unwrap_or("Macro1");
                json!({"result": {"source": format!("Sub {name}()\r\n{}\r\nEnd Sub\r\n", lines.join("\r\n"))}})
            }
            Some("macros/saveModule") => {
                let b64 = deckcraft_engine::cmd::base64_encode(p["source"].as_str().unwrap_or("").as_bytes());
                host(&mut out, &mut lines, "host/vbaProject/set", json!({"base64": b64}));
                json!({"result": {}})
            }
            _ => json!({"result": {}}),
        };
        let mut reply = json!({"jsonrpc": "2.0", "id": id});
        if let (Some(r), Json::Object(a)) = (reply.as_object_mut(), answer) {
            r.extend(a);
        }
        if writeln!(out, "{reply}").and_then(|_| out.flush()).is_err() {
            break;
        }
    }
}

fn session() -> Session {
    let mut s = Session::with_new();
    load(&mut s, PLUGIN, json!(["documentOpened", "documentSaved", "documentClosed"]));
    s
}

/// Loads the test add-in as `id`, hearing `events`.
fn load(s: &mut Session, id: &str, events: Json) {
    let dir = std::env::temp_dir().join(format!("deckcraft-engine-addins-{}-{id}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let exe = std::env::current_exe().unwrap();
    let commands: Vec<Json> =
        ["title", "broken", "ctx", "opener", "browse", "stray", "malformed", "combo", "peek"].iter().map(|c| json!({"id": c, "label": c})).collect();
    let manifest = json!({
        "id": id, "name": "Test add-in", "macros": {"language": "TestBasic"}, "run": {"command": exe, "args": ["--serve"]},
        "commands": commands, "events": events
    });
    std::fs::write(dir.join("plugin.json"), manifest.to_string()).unwrap();
    s.execute("plugins.load", &json!({"path": dir})).unwrap();
}

/// A scratch folder for this test.
fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("deckcraft-engine-addins-{}-files-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// What the add-ins answered to events, as `plugin: message` (or `plugin: error …`), waiting
/// until they all were delivered.
fn heard(s: &mut Session) -> Vec<String> {
    assert!(s.wait_events(std::time::Duration::from_secs(20)), "events delivered");
    s.take_event_outcomes()
        .into_iter()
        .map(|o| match o.result {
            Ok(m) => format!("{}: {}{}", o.plugin, m.unwrap_or_default(), o.notes.iter().map(|n| format!(" [{n}]")).collect::<String>()),
            Err(e) => format!("{}: error {e}", o.plugin),
        })
        .collect()
}

fn slides(s: &Session) -> usize {
    s.active().unwrap().doc.slides.len()
}

fn commands_answer_engine_commands_as_one_step() {
    let mut s = session();
    let (n, undo) = (slides(&s), s.active().unwrap().history.undo.len());
    let r = s.execute("plugins.run", &json!({"plugin": PLUGIN, "command": "title"})).unwrap();
    assert_eq!(r["ran"], 1);
    assert_eq!(slides(&s), n + 1);
    assert_eq!(s.active().unwrap().doc.slides.last().unwrap().title(), format!("After {n}"));
    assert_eq!(s.active().unwrap().history.undo.len(), undo + 1);
    let doc = s.active().unwrap().doc.clone();
    let e = s.execute("plugins.run", &json!({"plugin": PLUGIN, "command": "broken"})).unwrap_err().to_string();
    assert!(e.contains("nothing was changed"), "{e}");
    assert!(std::sync::Arc::ptr_eq(&doc, &s.active().unwrap().doc));
    assert!(s.execute("plugins.run", &json!({"plugin": PLUGIN, "command": "nope"})).is_err());
    assert!(deckcraft_engine::plugin::unload(PLUGIN));
}

fn macros_drive_the_presentation_as_one_undo_step() {
    let mut s = session();
    let list = s.execute("developer.macros", &json!({})).unwrap();
    assert_eq!(list["macros"][0]["name"], "Module1.Stamp");
    assert_eq!(list["macros"][0]["language"], "TestBasic");
    assert_eq!(list["enabled"], false, "a presentation's macros are off until enabled");
    s.execute("developer.enableMacros", &json!({})).unwrap();
    let undo = s.active().unwrap().history.undo.len();
    let shapes = s.active().unwrap().doc.slides[0].shapes.len();
    s.execute("developer.runMacro", &json!({"name": "Module1.Stamp", "control": {"id": "b1", "tag": "!"}})).unwrap();
    let first = &s.active().unwrap().doc.slides[0];
    assert_eq!(first.shapes.len(), shapes + 1);
    let n = slides(&s);
    assert_eq!(first.shapes.last().unwrap().text.as_ref().unwrap().text(), format!("{n} slides!"));
    assert_eq!(s.active().unwrap().history.undo.len(), undo + 1, "one undo step for the whole macro");
    assert!(s.ui_requests.iter().any(|q| matches!(q, deckcraft_engine::UiRequest::Message { text } if text == &format!("stamped {n}"))));
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.active().unwrap().doc.slides[0].shapes.len(), shapes);
    assert!(deckcraft_engine::plugin::unload(PLUGIN));
}

fn a_failing_macro_changes_nothing() {
    let mut s = session();
    s.execute("developer.enableMacros", &json!({})).unwrap();
    let (n, undo) = (slides(&s), s.active().unwrap().history.undo.len());
    let e = s.execute("developer.runMacro", &json!({"name": "Module1.Broken"})).unwrap_err().to_string();
    assert!(e.contains("Subscript out of range") && e.contains("ItemNotFound"), "{e}");
    assert_eq!(slides(&s), n, "the slide added before the failure was taken back");
    assert_eq!(s.active().unwrap().history.undo.len(), undo);
    assert!(deckcraft_engine::plugin::unload(PLUGIN));
}

fn macro_security_gates_running() {
    let mut s = session();
    assert!(s.execute("developer.runMacro", &json!({"name": "Stamp"})).unwrap_err().to_string().contains("disabled"));
    s.execute("developer.macroSecurity", &json!({"level": "enable"})).unwrap();
    assert!(s.execute("developer.runMacro", &json!({"name": "Stamp"})).is_ok());
    s.execute("developer.macroSecurity", &json!({"level": "disable"})).unwrap();
    s.execute("developer.enableMacros", &json!({})).unwrap();
    assert!(s.execute("developer.runMacro", &json!({"name": "Stamp"})).is_err(), "Disable wins over a presentation's Enable Content");
    assert!(s.execute("developer.macroSecurity", &json!({"level": "maybe"})).is_err());
    assert!(deckcraft_engine::plugin::unload(PLUGIN));
    assert!(s.execute("developer.macros", &json!({})).unwrap()["macros"].as_array().unwrap().is_empty());
    assert!(s.execute("developer.runMacro", &json!({"name": "Stamp"})).is_err());
}

fn modules_live_in_the_presentations_vba_project() {
    let mut s = session();
    assert_eq!(s.execute("developer.modules", &json!({})).unwrap()["modules"], json!([]));
    s.execute("developer.saveModule", &json!({"name": "Module1", "source": "Sub Hello()\r\nEnd Sub"})).unwrap();
    assert_eq!(s.active().unwrap().doc.vba_project.as_deref().map(Vec::as_slice), Some(&b"Sub Hello()\r\nEnd Sub"[..]));
    assert_eq!(s.execute("developer.modules", &json!({})).unwrap()["modules"][0]["source"], "Sub Hello()\r\nEnd Sub");
    // Saving a module is undoable, like any change to the presentation.
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.active().unwrap().doc.vba_project.is_none());
    assert!(deckcraft_engine::plugin::unload(PLUGIN));
}

fn ribbon_buttons_run_their_macro_and_are_saved() {
    let mut s = session();
    assert_eq!(s.execute("developer.ribbon", &json!({})).unwrap(), json!({"tabs": [], "editable": true}));
    let id =
        s.execute("developer.addRibbonButton", &json!({"macro": "Module1.Stamp", "label": "Go", "tab": "review", "tag": "north"})).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
    s.execute("developer.addRibbonButton", &json!({"macro": "Module1.Fill", "large": false})).unwrap();
    let r = s.execute("developer.ribbon", &json!({})).unwrap();
    assert_eq!((r["tabs"][0]["tab"].as_str(), r["tabs"][0]["builtin"].as_str()), (Some("Review"), Some("TabReview")), "{r}");
    assert_eq!(r["tabs"][0]["groups"][0]["buttons"][0]["label"], "Go");
    assert_eq!(r["tabs"][1]["tab"], "Macros", "a tab of its own by default");
    assert_eq!(r["tabs"][1]["groups"][0]["buttons"][0]["label"], "Fill", "labelled after the macro");

    // Pressing it runs the macro with the button, under Macro Security like any macro.
    assert!(s.execute("developer.pressRibbonButton", &json!({"id": id})).unwrap_err().to_string().contains("disabled"));
    s.execute("developer.enableMacros", &json!({})).unwrap();
    let shapes = s.active().unwrap().doc.slides[0].shapes.len();
    s.execute("developer.pressRibbonButton", &json!({"id": id})).unwrap();
    let first = &s.active().unwrap().doc.slides[0];
    assert_eq!(first.shapes.len(), shapes + 1);
    assert!(first.shapes.last().unwrap().text.as_ref().unwrap().text().ends_with("north"), "the macro got the button's tag");
    assert!(s.execute("developer.pressRibbonButton", &json!({"id": "nope"})).is_err());

    // Saved with the presentation as the Office ribbon part, and read back.
    let doc = s.active().unwrap().doc.clone();
    let bytes = deckcraft_pptx::export_as(&doc, deckcraft_pptx::PptxKind::MacroPresentation).unwrap();
    assert_eq!(deckcraft_pptx::import(&bytes).unwrap().ribbon, doc.ribbon);

    // Edits and removals are undoable; the last button gone, the presentation has no ribbon.
    let label = |s: &Session| s.active().unwrap().doc.ribbon.as_ref().unwrap().button(&id).unwrap().label.clone();
    s.execute("developer.editRibbonButton", &json!({"id": id, "label": "Run it", "image": "HappyFace"})).unwrap();
    assert_eq!(label(&s), "Run it");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(label(&s), "Go");
    assert!(s.execute("developer.editRibbonButton", &json!({"id": id, "image": "<img>"})).is_err());
    assert!(s.execute("developer.addRibbonButton", &json!({"macro": "x\"/><evil"})).is_err());
    let all: Vec<String> =
        s.active().unwrap().doc.ribbon.as_ref().unwrap().ids().map(str::to_string).filter(|i| i.starts_with("craftButton")).collect();
    for b in &all {
        s.execute("developer.removeRibbonButton", &json!({"id": b})).unwrap();
    }
    assert!(s.active().unwrap().doc.ribbon.is_none());
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.active().unwrap().doc.ribbon.as_ref().is_some_and(|r| r.button(&all[all.len() - 1]).is_some()));
    assert!(deckcraft_engine::plugin::unload(PLUGIN));
}

fn recording_sends_the_steps_and_keeps_the_code() {
    let mut s = session();
    assert_eq!(s.execute("developer.recordMacro", &json!({"stop": true})).unwrap(), json!({"recording": false}), "nothing to stop");
    assert_eq!(s.execute("developer.recordMacro", &json!({})).unwrap()["recording"], true);
    assert!(s.recording.is_some());
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    s.execute("developer.macros", &json!({})).unwrap();
    s.execute("shape.insert", &json!({"preset": "star5", "rect": [10, 10, 100, 100]})).unwrap();
    // Stopping (no `stop`: the same button toggles) sends the steps and appends the code to Module1.
    let r = s.execute("developer.recordMacro", &json!({"name": "Recorded"})).unwrap();
    assert_eq!(r["recording"], false);
    assert_eq!(r["steps"], 2, "the Developer tab's own commands aren't recorded: {r}");
    assert_eq!(r["module"], "Module1");
    let source = r["source"].as_str().unwrap();
    assert!(source.starts_with("Sub Recorded()") && source.contains("' slide.new with params") && source.contains("' shape.insert"), "{source}");
    let module = s.execute("developer.modules", &json!({})).unwrap()["modules"][0]["source"].as_str().unwrap().to_string();
    assert!(module.contains("Sub Recorded()"), "{module}");
    assert!(s.recording.is_none());
    // A second recording goes after the first.
    s.execute("developer.recordMacro", &json!({})).unwrap();
    s.execute("developer.recordMacro", &json!({"stop": true, "name": "Again"})).unwrap();
    let module = s.execute("developer.modules", &json!({})).unwrap()["modules"][0]["source"].as_str().unwrap().to_string();
    assert!(module.find("Sub Recorded()").unwrap() < module.find("Sub Again()").unwrap(), "{module}");
    // Steps given directly; hostile names are refused.
    let code = s.execute("developer.recordToCode", &json!({"steps": [{"command": "slide.new", "params": null}], "name": "Direct"})).unwrap();
    assert!(code["source"].as_str().unwrap().starts_with("Sub Direct()"));
    assert!(s.execute("developer.recordToCode", &json!({"steps": [], "name": "1 bad"})).is_err());
    assert!(s.execute("developer.recordToCode", &json!({})).is_err());
    assert!(deckcraft_engine::plugin::unload(PLUGIN));
    assert!(s.execute("developer.recordMacro", &json!({})).is_err(), "no add-in, no recording");
    assert!(s.recording.is_none());
}

fn commands_and_macros_see_the_document() {
    let mut s = session();
    let doc = &s.execute("plugins.run", &json!({"plugin": PLUGIN, "command": "ctx"})).unwrap()["result"]["document"];
    assert_eq!(doc["path"], Json::Null, "a new presentation has no file");
    assert_eq!((doc["name"].as_str(), doc["dirty"].as_bool()), (Some("Presentation1"), Some(false)), "{doc}");
    let path = scratch("ctx").join("Deck.pptx").to_string_lossy().to_string();
    s.execute("file.saveAs", &json!({"path": path})).unwrap();
    s.execute("slide.new", &json!({})).unwrap();
    let r = s.execute("plugins.run", &json!({"plugin": PLUGIN, "command": "ctx"})).unwrap();
    assert_eq!(r["result"]["document"], json!({"path": path, "name": "Deck.pptx", "dirty": true}));
    assert!(r["result"]["slides"].is_number(), "the 1.1 fields stay");
    // macros/run gets the same `context.document`.
    s.execute("developer.enableMacros", &json!({})).unwrap();
    s.execute("developer.runMacro", &json!({"name": "Module1.Where"})).unwrap();
    assert!(s.ui_requests.iter().any(|q| matches!(q, deckcraft_engine::UiRequest::Message { text } if text == "in Deck.pptx dirty true")));
    heard(&mut s);
    assert!(deckcraft_engine::plugin::unload(PLUGIN));
}

fn programmatic_runs_return_open_and_dialog() {
    let mut s = session();
    let dir = scratch("open");
    let deck = dir.join("Budget.pptx").to_string_lossy().to_string();
    std::fs::write(&deck, deckcraft_engine::cmd::file::save_bytes(&s.active().unwrap().doc, "pptx").unwrap()).unwrap();
    let docs = s.documents().len();
    // A programmatic run opens nothing: the caller gets `open`.
    let r = s.execute("plugins.run", &json!({"plugin": PLUGIN, "command": "opener", "params": {"path": deck}})).unwrap();
    assert_eq!(r["open"], json!({"path": deck, "label": "Cloud · deck", "readOnly": true}));
    assert_eq!(r["message"], "opening");
    assert_eq!(s.documents().len(), docs);
    // The caller opens it the normal way; the label shows as its location.
    s.execute("file.open", &json!({"path": deck, "label": r["open"]["label"]})).unwrap();
    assert_eq!(s.execute("file.info", &json!({})).unwrap()["label"], "Cloud · deck");
    // Not absolute, missing, a folder, not a presentation: the command fails and changes nothing.
    let txt = dir.join("notes.odp");
    std::fs::write(&txt, "x").unwrap();
    for bad in [
        "Budget.pptx".to_string(),
        dir.join("gone.pptx").to_string_lossy().to_string(),
        dir.to_string_lossy().to_string(),
        txt.to_string_lossy().to_string(),
    ] {
        let e = s.execute("plugins.run", &json!({"plugin": PLUGIN, "command": "opener", "params": {"path": bad}})).unwrap_err().to_string();
        assert!(e.contains("asked to open"), "{e}");
    }
    // A dialog comes back for the caller to show; its OK runs the command with the selection,
    // whose answer can be another dialog (chaining), and so on.
    let d = s.execute("plugins.run", &json!({"plugin": PLUGIN, "command": "browse"})).unwrap()["dialog"].clone();
    assert_eq!((d["kind"].as_str(), d["okLabel"].as_str(), d["items"].as_array().map(Vec::len)), (Some("list"), Some("Open"), Some(2)), "{d}");
    let dialog: deckcraft_engine::plugin::Dialog = serde_json::from_value(d).unwrap();
    let p = deckcraft_engine::plugin::dialog::action_params(&dialog.params, &dialog, &["/docs/".into()], None);
    let next = s.execute("plugins.run", &json!({"plugin": PLUGIN, "command": "browse", "params": p})).unwrap();
    assert_eq!(next["dialog"]["title"], "In /docs/");
    let p = json!({"id": "/docs/b.pptx"});
    assert_eq!(s.execute("plugins.run", &json!({"plugin": PLUGIN, "command": "browse", "params": p})).unwrap()["message"], "picked /docs/b.pptx");
    // Fields combine: `open` + `message` (above), and `dialog` + `result` + `message` + commands.
    s.ui_requests.clear();
    let n = slides(&s);
    let r = s.execute("plugins.run", &json!({"plugin": PLUGIN, "command": "combo"})).unwrap();
    assert_eq!((r["ran"].as_u64(), r["message"].as_str(), r["result"]["account"].as_str()), (Some(1), Some("connected"), Some("me")), "{r}");
    assert_eq!(r["dialog"]["prompt"], "Signed in");
    assert_eq!(slides(&s), n + 1);
    assert!(s.ui_requests.iter().any(|q| matches!(q, deckcraft_engine::UiRequest::Message { text } if text == "connected")), "the message shows too");
    // A dialog that runs another add-in's command, or isn't one: an error, not a crash.
    assert!(s.execute("plugins.run", &json!({"plugin": PLUGIN, "command": "stray"})).unwrap_err().to_string().contains("notMine"));
    assert!(s.execute("plugins.run", &json!({"plugin": PLUGIN, "command": "malformed"})).unwrap_err().to_string().contains("kind"));
    heard(&mut s);
    assert!(deckcraft_engine::plugin::unload(PLUGIN));
}

/// A command whose answer changes nothing (a dialog step, even one that read the presentation)
/// makes no undo step and leaves the presentation clean: opening a file at the end of a dialog
/// chain mustn't ask to save first.
fn dialog_steps_that_change_nothing_are_no_edit() {
    let mut s = session();
    let undo = s.active().unwrap().history.undo.len();
    for cmd in ["browse", "peek", "peek"] {
        let r = s.execute("plugins.run", &json!({"plugin": PLUGIN, "command": cmd})).unwrap();
        assert!(r["dialog"].is_object(), "{r}");
        let st = s.active().unwrap();
        assert!(!st.is_dirty(), "{cmd} made the presentation dirty");
        assert_eq!(st.history.undo.len(), undo, "{cmd} made an undo step");
    }
    // A step that does change it is still one undo step.
    s.execute("plugins.run", &json!({"plugin": PLUGIN, "command": "title"})).unwrap();
    assert!(s.active().unwrap().is_dirty());
    assert_eq!(s.active().unwrap().history.undo.len(), undo + 1);
    assert!(deckcraft_engine::plugin::unload(PLUGIN));
}

fn declared_events_follow_opens_saves_and_closes() {
    let mut s = session();
    // An add-in that declares no events hears none.
    load(&mut s, "org.deckcraft.test-quiet", json!([]));
    let dir = scratch("events");
    let path = dir.join("Talk.pptx").to_string_lossy().to_string();
    s.execute("file.saveAs", &json!({"path": path})).unwrap();
    assert_eq!(heard(&mut s), [format!("{PLUGIN}: documentSaved Talk.pptx pptx")]);
    // Autosave isn't a save.
    s.recovery_dir = Some(dir.join("recovery"));
    s.execute("slide.new", &json!({})).unwrap();
    s.execute("file.recovery.save", &json!({})).unwrap();
    assert!(heard(&mut s).is_empty());
    s.execute("file.save", &json!({})).unwrap();
    s.execute("file.close", &json!({})).unwrap();
    assert_eq!(heard(&mut s), [format!("{PLUGIN}: documentSaved Talk.pptx pptx"), format!("{PLUGIN}: documentClosed Talk.pptx -")]);
    s.execute("file.open", &json!({"path": path})).unwrap();
    assert!(heard(&mut s)[0].starts_with(&format!("{PLUGIN}: opened Talk.pptx")));
    // A new presentation that was never saved has no file: closing it says nothing.
    s.execute("file.new", &json!({})).unwrap();
    s.execute("file.close", &json!({})).unwrap();
    assert!(heard(&mut s).is_empty());
    // Quitting closes what is open, best effort.
    assert!(s.quit_events());
    assert_eq!(heard(&mut s), [format!("{PLUGIN}: documentClosed Talk.pptx -")]);
    // An add-in turned off hears nothing.
    s.execute("plugins.setEnabled", &json!({"id": PLUGIN, "enabled": false})).unwrap();
    s.execute("file.save", &json!({})).unwrap();
    assert!(heard(&mut s).is_empty());
    s.execute("plugins.setEnabled", &json!({"id": PLUGIN, "enabled": true})).unwrap();
    assert!(deckcraft_engine::plugin::unload("org.deckcraft.test-quiet"));
    assert!(deckcraft_engine::plugin::unload(PLUGIN));
}

fn events_never_hold_up_or_fail_a_save() {
    let mut s = session();
    let dir = scratch("slow");
    // A slow add-in: the save is done at once, the event follows.
    let slow = dir.join("slow.deckcraft").to_string_lossy().to_string();
    let t = std::time::Instant::now();
    s.execute("file.saveAs", &json!({"path": slow})).unwrap();
    assert!(t.elapsed() < std::time::Duration::from_millis(3500), "the save didn't wait for the add-in");
    assert!(std::path::Path::new(&slow).is_file());
    assert!(!s.active().unwrap().is_dirty());
    assert!(!s.wait_events(std::time::Duration::from_millis(100)), "still being delivered");
    assert!(s.events_pending());
    assert_eq!(heard(&mut s), [format!("{PLUGIN}: documentSaved slow.deckcraft deckcraft")]);
    // A failing add-in: the save stands, the error is reported.
    let fail = dir.join("fail.pptx").to_string_lossy().to_string();
    s.execute("file.saveAs", &json!({"path": fail})).unwrap();
    assert!(std::path::Path::new(&fail).is_file());
    assert_eq!(heard(&mut s), [format!("{PLUGIN}: error upload failed")]);
    // documentClosed times out after 5 s; the close is done at once, and nothing breaks.
    let hang = dir.join("hang.pptx").to_string_lossy().to_string();
    s.execute("file.saveAs", &json!({"path": hang})).unwrap();
    heard(&mut s);
    let t = std::time::Instant::now();
    s.execute("file.close", &json!({})).unwrap();
    assert!(t.elapsed() < std::time::Duration::from_millis(3500));
    assert!(s.documents().is_empty());
    let said = heard(&mut s);
    assert!(t.elapsed() < std::time::Duration::from_secs(8), "timed out at 5 s, not when the add-in answered");
    assert!(said[0].contains("error") && said[0].contains("took longer"), "{said:?}");
    // The add-in starts again for the next command.
    s.execute("file.new", &json!({})).unwrap();
    assert_eq!(s.execute("plugins.run", &json!({"plugin": PLUGIN, "command": "title"})).unwrap()["ran"], 1);
    assert!(deckcraft_engine::plugin::unload(PLUGIN));
}

fn events_cant_change_the_presentation() {
    let mut s = session();
    let path = scratch("readonly").join("Look.pptx").to_string_lossy().to_string();
    s.execute("file.saveAs", &json!({"path": path})).unwrap();
    s.execute("file.close", &json!({})).unwrap();
    heard(&mut s);
    s.execute("file.open", &json!({"path": path})).unwrap();
    let undo = s.active().unwrap().history.undo.len();
    let said = heard(&mut s);
    assert_eq!(said, [format!("{PLUGIN}: opened Look.pptx: read AccessDenied write AccessDenied vba AccessDenied AccessDenied [noted]")]);
    let st = s.active().unwrap();
    assert!(!st.is_dirty() && st.history.undo.len() == undo, "an event is no edit and no undo step");
    assert_eq!(st.doc.slides[0].shapes.len(), deckcraft_pptx::import(&std::fs::read(&path).unwrap()).unwrap().slides[0].shapes.len());
    assert!(deckcraft_engine::plugin::unload(PLUGIN));
}
