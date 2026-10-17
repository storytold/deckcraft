//! Add-ins (Home › Add-ins, Developer › Add-ins): load and unload out-of-process plugins, list
//! them, and run their commands (see `deckcraft_plugin`). A plugin command is one undoable step:
//! the engine commands it answers with, and whatever it changed through the host meanwhile
//! (protocol 1.1, [`super::developer::host_call`]), succeed or go back together.

use std::sync::Arc;

use serde_json::{Value as Json, json};

use deckcraft_plugin::install::Fetched;
use deckcraft_plugin::requires::MAX_DEPTH;
use deckcraft_plugin::version::{Range, Version};

use super::{CommandSpec, always, bad, bool_param, cmd, str_param};
use crate::{Result, Session, UiRequest};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "plugins.list",
            "Add-ins",
            [],
            None,
            "{} → [{id, name, version, description, dir, status, state, blockedBy, requires, requiredBy, commands, ui, kind}]. `state`: loaded, running, failed, blocked (a requirement isn't met: `blockedBy` [{id?, kind, need, have?, message}]) or disabled",
            always,
            |_, _| Ok(list())
        ),
        cmd!(
            query "plugins.load",
            "Load Add-in",
            [],
            None,
            "{path, force?} → the add-in. `path`: a folder holding plugin.json (or an Office add-in's manifest.xml), or the manifest. Replacing an add-in with a version outside the range a turned-on add-in requires fails unless `force` (that add-in is then blocked)",
            always,
            load
        ),
        cmd!(
            query "plugins.unload",
            "Unload Add-in",
            [],
            None,
            "{id} → {unloaded, dependentsBlocked}. The add-ins that require it are blocked, and listed",
            always,
            unload
        ),
        cmd!(
            query "plugins.install",
            "Install Add-in from GitHub",
            [],
            None,
            "{url (or github), dir?, version?, withRequirements?, force?} → the add-in, with `release`, `missing` (its required and optional add-ins that aren't installed or are out of range) and, with `withRequirements`, `installed` and `failed`. Downloads the add-in in a GitHub repository (the release zip for this platform, else its source; with `version`, a range, the newest release whose tag is in it) into `dir` (default: the app's add-ins folder) and loads it. `withRequirements` (default false) also installs the missing required add-ins that name `github`. A version outside the range a turned-on add-in requires fails unless `force`",
            always,
            install
        ),
        cmd!(
            query "plugins.installMissing",
            "Install Missing Add-ins",
            [],
            None,
            "{id, dir?, force?} → {id, installed: [{id, name, version, release}], failed: [{id, error}], missing, satisfied}. Installs the missing or out-of-range required add-ins of add-in `id` that name `github` (the newest release in range), and theirs in turn",
            always,
            install_missing
        ),
        cmd!(
            query "plugins.requirements",
            "Add-in Requirements",
            [],
            None,
            "{id} → {id, satisfied, blockedBy, tree: [{id, name, need, have, state, optional, why, github, children}]}. `state` `missing` for an add-in that isn't installed",
            always,
            requirements
        ),
        cmd!(
            query "plugins.setEnabled",
            "Turn Add-in On or Off",
            [],
            None,
            "{id, enabled} → the add-in, with `dependentsBlocked` (the add-ins that require it, now blocked). Off, an add-in stays listed but its commands, ribbon and menu items are gone; remembered in the preferences",
            always,
            set_enabled
        ),
        cmd!(query "plugins.openHomepage", "Add-in Documentation", [], None, "{id} → {url}. Opens the add-in's web page (its manifest's `homepage`) in the browser", always, open_homepage),
        cmd!(
            "plugins.run",
            "Run Add-in Command",
            [],
            None,
            "{plugin, command, params?} → {ran, message?, result?, open?: {path, label?, readOnly}, dialog?, collab?: {join}}. The add-in's commands run as one undoable step; `open` and `dialog` (protocol 1.2) and `collab.join` (a live session, protocol 2) are for the caller to act on: the app's own buttons and menus open the file, show the dialog or join the room, a programmatic run only returns them",
            always,
            run
        ),
    ]
}

/// `plugins.list`.
pub fn list() -> Json {
    Json::Array(deckcraft_plugin::list().iter().map(|p| p.info()).collect())
}

fn load(s: &mut Session, p: &Json) -> Result<Json> {
    const ID: &str = "plugins.load";
    let path = str_param(p, "path").filter(|x| !x.trim().is_empty()).ok_or_else(|| bad(ID, "missing `path`"))?;
    let path = std::path::Path::new(path);
    let (manifest, _) = deckcraft_plugin::read_manifest(path).map_err(|e| bad(ID, e))?;
    check_conflicts(ID, &manifest, bool_param(p, "force").unwrap_or(false))?;
    load_from(s, ID, path)
}

/// Refuses a version outside the range a turned-on add-in requires, unless `force`.
fn check_conflicts(id: &str, manifest: &deckcraft_plugin::Manifest, force: bool) -> Result<()> {
    let conflicts = deckcraft_plugin::conflicts(manifest);
    if conflicts.is_empty() || force {
        return Ok(());
    }
    let what: Vec<&str> = conflicts.iter().map(|c| c.message.as_str()).collect();
    Err(bad(id, format!("{}. Pass `force: true` to install it anyway (those add-ins are then blocked)", what.join("; "))))
}

/// The add-ins folder: `dir`, else the app's.
fn plugins_root(id: &str, p: &Json) -> Result<std::path::PathBuf> {
    match str_param(p, "dir").filter(|x| !x.trim().is_empty()) {
        Some(d) => Ok(std::path::PathBuf::from(d)),
        None => deckcraft_plugin::install::install_dir().ok_or_else(|| bad(id, "no add-ins folder: pass `dir`")),
    }
}

fn install(s: &mut Session, p: &Json) -> Result<Json> {
    const ID: &str = "plugins.install";
    let url = str_param(p, "url")
        .or_else(|| str_param(p, "github"))
        .filter(|x| !x.trim().is_empty())
        .ok_or_else(|| bad(ID, "missing `url` (or `github`)"))?;
    let root = plugins_root(ID, p)?;
    let force = bool_param(p, "force").unwrap_or(false);
    let range = match str_param(p, "version") {
        Some(v) => Some(Range::parse(v).map_err(|e| bad(ID, e))?),
        None => None,
    };
    let fetched = deckcraft_plugin::install::fetch(url, range.as_ref()).map_err(|e| bad(ID, e))?;
    let mut fetch = |github: &str, range: &Range| deckcraft_plugin::install::fetch(github, Some(range));
    install_fetched(s, ID, fetched, &root, force, bool_param(p, "withRequirements").unwrap_or(false), &mut fetch)
}

/// Downloads an add-in for [`install_fetched`] and [`install_requirements`]: `(github, range)`.
pub type Fetcher<'a> = dyn FnMut(&str, &Range) -> std::result::Result<Fetched, String> + 'a;

/// Installs a downloaded add-in into `root` and loads it (`plugins.install`, after the
/// download): refused when its version is outside the range a turned-on add-in requires, unless
/// `force`. With `with_requirements`, its missing required add-ins that name `github` are
/// installed too, through `fetch`.
pub fn install_fetched(
    s: &mut Session,
    id: &str,
    fetched: Fetched,
    root: &std::path::Path,
    force: bool,
    with_requirements: bool,
    fetch: &mut Fetcher<'_>,
) -> Result<Json> {
    check_conflicts(id, &fetched.manifest, force)?;
    let dir = fetched.unpack(root).map_err(|e| bad(id, e))?;
    let mut info = load_from(s, id, &dir)?;
    let plugin = fetched.manifest.id.clone();
    info["release"] = json!(fetched.release);
    if with_requirements {
        let (installed, failed) = install_requirements(s, &plugin, root, false, fetch);
        info["installed"] = Json::Array(installed);
        info["failed"] = Json::Array(failed);
        // Its own state may have changed since `load_from`.
        if let Some(p) = deckcraft_plugin::get(&plugin) {
            info["state"] = json!(p.state());
            info["blockedBy"] = json!(p.blocked_by());
        }
    }
    info["missing"] = json!(deckcraft_plugin::missing(&plugin));
    Ok(info)
}

/// Installs `plugin`'s missing or out-of-range required add-ins that name `github` (the newest
/// release in range, through `fetch`), then theirs in turn, at most [`MAX_DEPTH`] deep and 64 in
/// all. Returns what was installed (`{id, name, version, release}`) and what failed (`{id, error}`).
pub fn install_requirements(s: &mut Session, plugin: &str, root: &std::path::Path, force: bool, fetch: &mut Fetcher<'_>) -> (Vec<Json>, Vec<Json>) {
    const ID: &str = "plugins.installMissing";
    const MAX_INSTALLS: usize = 64;
    let (mut installed, mut failed) = (Vec::new(), Vec::new());
    let mut seen: Vec<String> = vec![plugin.to_string()];
    let mut queue: std::collections::VecDeque<(String, usize)> = std::collections::VecDeque::from([(plugin.to_string(), 0)]);
    while let Some((pid, depth)) = queue.pop_front() {
        for m in deckcraft_plugin::missing(&pid).into_iter().filter(|m| !m.optional) {
            if seen.contains(&m.id) || installed.len() + failed.len() >= MAX_INSTALLS {
                continue;
            }
            seen.push(m.id.clone());
            let Some(github) = m.github.clone() else { continue };
            let outcome = (|| -> std::result::Result<Json, String> {
                let range = Range::parse(&m.version)?;
                let fetched = fetch(&github, &range)?;
                let got = &fetched.manifest;
                if got.id != m.id {
                    return Err(format!("github.com/{github} holds {}, not {}", got.id, m.id));
                }
                let fits = Version::parse(&got.version).map(|v| range.matches(&v)).unwrap_or(range.is_any());
                if !fits {
                    return Err(format!("{} {} from github.com/{github} isn't in {}", m.name, got.version, range));
                }
                check_conflicts(ID, got, force).map_err(|e| e.to_string())?;
                let dir = fetched.unpack(root)?;
                load_from(s, ID, &dir).map_err(|e| e.to_string())?;
                Ok(json!({"id": m.id, "name": got.name, "version": got.version, "release": fetched.release}))
            })();
            match outcome {
                Ok(done) => {
                    installed.push(done);
                    if depth + 1 < MAX_DEPTH {
                        queue.push_back((m.id.clone(), depth + 1));
                    }
                }
                Err(e) => failed.push(json!({"id": m.id, "error": e})),
            }
        }
    }
    (installed, failed)
}

fn install_missing(s: &mut Session, p: &Json) -> Result<Json> {
    const ID: &str = "plugins.installMissing";
    let id = str_param(p, "id").ok_or_else(|| bad(ID, "missing `id`"))?;
    if deckcraft_plugin::get(id).is_none() {
        return Err(bad(ID, format!("no add-in `{id}` is loaded")));
    }
    let root = plugins_root(ID, p)?;
    let mut fetch = |github: &str, range: &Range| deckcraft_plugin::install::fetch(github, Some(range));
    Ok(install_missing_with(s, id, &root, bool_param(p, "force").unwrap_or(false), &mut fetch))
}

/// `plugins.installMissing` with a given downloader (tests use one without a network).
pub fn install_missing_with(s: &mut Session, id: &str, root: &std::path::Path, force: bool, fetch: &mut Fetcher<'_>) -> Json {
    let (installed, failed) = install_requirements(s, id, root, force, fetch);
    json!({
        "id": id,
        "installed": installed,
        "failed": failed,
        "missing": deckcraft_plugin::missing(id),
        "satisfied": deckcraft_plugin::get(id).is_some_and(|p| !p.is_blocked()),
    })
}

fn requirements(_: &mut Session, p: &Json) -> Result<Json> {
    const ID: &str = "plugins.requirements";
    let id = str_param(p, "id").ok_or_else(|| bad(ID, "missing `id`"))?;
    deckcraft_plugin::requirements(id).map_err(|e| bad(ID, e))
}

/// The add-ins in `ids` that are blocked now.
fn now_blocked(ids: &[String]) -> Vec<String> {
    ids.iter().filter(|d| deckcraft_plugin::get(d).is_some_and(|p| p.is_blocked())).cloned().collect()
}

/// Loads the add-in in `path` (a folder or its manifest).
pub fn load_from(s: &mut Session, id: &str, path: &std::path::Path) -> Result<Json> {
    let plugin = deckcraft_plugin::load(path).map_err(|e| bad(id, e))?;
    // An add-in turned off stays off when it's loaded again.
    if s.prefs.disabled_plugins.contains(&plugin.manifest.id) {
        let _ = deckcraft_plugin::set_enabled(&plugin.manifest.id, false);
    }
    Ok(plugin.info())
}

fn unload(_s: &mut Session, p: &Json) -> Result<Json> {
    let id = str_param(p, "id").ok_or_else(|| bad("plugins.unload", "missing `id`"))?;
    let dependents = deckcraft_plugin::dependents(id);
    if !deckcraft_plugin::unload(id) {
        return Err(bad("plugins.unload", format!("no add-in `{id}` is loaded")));
    }
    Ok(json!({"unloaded": id, "dependentsBlocked": now_blocked(&dependents)}))
}

fn set_enabled(s: &mut Session, p: &Json) -> Result<Json> {
    const ID: &str = "plugins.setEnabled";
    let id = str_param(p, "id").ok_or_else(|| bad(ID, "missing `id`"))?;
    let on = p.get("enabled").and_then(Json::as_bool).ok_or_else(|| bad(ID, "missing `enabled` (true or false)"))?;
    let plugin = deckcraft_plugin::get(id).ok_or_else(|| bad(ID, format!("no add-in `{id}` is loaded")))?;
    let dependents = if on { Vec::new() } else { deckcraft_plugin::dependents(id) };
    deckcraft_plugin::set_enabled(id, on).map_err(|e| bad(ID, e))?;
    s.prefs.disabled_plugins.retain(|x| x != id);
    if !on {
        s.prefs.disabled_plugins.push(id.to_string());
    }
    let mut info = plugin.info();
    info["dependentsBlocked"] = json!(now_blocked(&dependents));
    Ok(info)
}

fn open_homepage(s: &mut Session, p: &Json) -> Result<Json> {
    const ID: &str = "plugins.openHomepage";
    let id = str_param(p, "id").ok_or_else(|| bad(ID, "missing `id`"))?;
    let plugin = deckcraft_plugin::get(id).ok_or_else(|| bad(ID, format!("no add-in `{id}` is loaded")))?;
    let url = plugin.manifest.homepage.trim();
    if url.is_empty() {
        return Err(bad(ID, format!("add-in {id} has no web page")));
    }
    s.ui_requests.push(UiRequest::OpenUrl { url: url.to_string() });
    Ok(json!({"url": url}))
}

/// Turns off the loaded add-ins the preferences list as off (at startup, after loading them).
pub fn apply_prefs(prefs: &crate::Prefs) {
    for id in &prefs.disabled_plugins {
        if deckcraft_plugin::get(id).is_some()
            && let Err(e) = deckcraft_plugin::set_enabled(id, false)
        {
            log::warn!("add-in {id}: {e}");
        }
    }
    // Say which add-ins can't start, and why.
    for (id, why) in deckcraft_plugin::refresh().blocked {
        log::warn!("add-in {id} is blocked: {}", why.iter().map(|b| b.message.as_str()).collect::<Vec<_>>().join("; "));
    }
}

/// What a plugin command sees of the presentation: its title, the current slide (0-based index
/// and id), the slides' titles, the selected shapes' ids and the selected or edited text.
fn context(s: &Session) -> Json {
    let Some(d) = s.active() else { return json!({}) };
    let titles: Vec<String> = d.doc.slides.iter().take(10_000).map(|sl| super::office::slide_title(sl)).collect();
    let text = d.selection.text.as_ref().and_then(|t| {
        let sh = d.shape(t.shape)?;
        let body = sh.text.as_ref()?;
        let ((p0, c0), (p1, c1)) = t.ordered();
        Some(super::office::text_between(body, (p0, c0), (p1, c1)))
    });
    json!({
        "document": document(d),
        "presentation": d.title(),
        "slide": d.selection.slide,
        "slideId": d.current_slide().map(|sl| sl.id.0.to_string()),
        "slides": d.doc.slides.len(),
        "titles": titles,
        "selection": d.selection.shapes.iter().map(|id| id.0.to_string()).collect::<Vec<_>>(),
        "text": text,
    })
}

/// `context.document` (protocol 1.2): the file the presentation was opened from or last saved to
/// (`null` for a new one), its name, and whether it has unsaved changes.
pub(crate) fn document(d: &crate::DocState) -> Json {
    let name = match &d.path {
        Some(p) => crate::events::file_name(p),
        None => d.title(),
    };
    json!({"path": d.path, "name": name, "dirty": d.is_dirty()})
}

/// `context` for `macros/run`: the document (protocol 1.2).
pub(crate) fn macro_context(s: &Session) -> Json {
    match s.active() {
        Some(d) => json!({"document": document(d)}),
        None => json!({}),
    }
}

/// Checks a reply's `open` and `dialog` before anything runs: the file must be one File › Open
/// opens, and the dialog may only run the add-in's own commands.
fn check_follow_up(plugin: &str, reply: &deckcraft_plugin::CommandReply) -> std::result::Result<(), String> {
    if let Some(open) = &reply.open {
        #[cfg(not(target_arch = "wasm32"))]
        super::file::check_openable(&open.path).map_err(|e| format!("the add-in asked to open {e}"))?;
        #[cfg(target_arch = "wasm32")]
        return Err(format!("the add-in asked to open {}: the web app can't open local files", open.path));
    }
    if let Some(d) = &reply.dialog {
        let manifest = deckcraft_plugin::get(plugin).map(|p| p.manifest.clone());
        for c in d.commands() {
            if manifest.as_ref().is_none_or(|m| m.command(c).is_none()) {
                return Err(format!("the add-in's dialog runs `{c}`, which isn't one of its commands"));
            }
        }
    }
    Ok(())
}

/// The active presentation and selection, to put back if a step fails.
pub(crate) fn snapshot(s: &Session) -> Option<(u64, Arc<deckcraft_model::Presentation>, crate::Selection)> {
    s.active().map(|d| (d.uid, d.doc.clone(), d.selection.clone()))
}

/// A step that changed nothing (a dialog step that only read the presentation, or wrote back
/// what was there) keeps the presentation it started with: no undo step, and a clean
/// presentation stays clean, so opening a file at the end of a dialog chain doesn't ask to save.
pub(crate) fn keep_if_unchanged(s: &mut Session, before: &Option<(u64, Arc<deckcraft_model::Presentation>, crate::Selection)>) {
    let Some((uid, doc, _)) = before else { return };
    if let Some(d) = s.active_mut().filter(|d| d.uid == *uid)
        && !Arc::ptr_eq(doc, &d.doc)
        && **doc == *d.doc
    {
        d.doc = doc.clone();
        d.revision += 1;
    }
}

/// Puts back what [`snapshot`] took (a failed step changes nothing).
pub(crate) fn restore(s: &mut Session, snap: Option<(u64, Arc<deckcraft_model::Presentation>, crate::Selection)>) {
    let Some((uid, doc, sel)) = snap else { return };
    if let Some(d) = s.active_mut().filter(|d| d.uid == uid) {
        d.doc = doc;
        d.selection = sel;
        d.revision += 1;
    }
}

fn run(s: &mut Session, p: &Json) -> Result<Json> {
    const ID: &str = "plugins.run";
    let plugin = str_param(p, "plugin").ok_or_else(|| bad(ID, "missing `plugin`"))?;
    let command = str_param(p, "command").ok_or_else(|| bad(ID, "missing `command`"))?;
    let params = p.get("params").cloned().unwrap_or_else(|| json!({}));
    // Commands run inside this one are no undo steps or journal entries of their own: this
    // command's step covers the add-in's host requests and its answer's commands.
    let before = snapshot(s);
    let ctx = context(s);
    let reply = deckcraft_plugin::run_command_with(plugin, command, params, ctx, &mut |m, p| super::developer::host_call(s, plugin, m, p));
    let reply = match reply {
        Ok(r) => r,
        Err(e) => {
            restore(s, before);
            return Err(bad(ID, e));
        }
    };
    // An Office add-in's command opens its task pane or calls its function: nothing runs here.
    if let Some(office) = reply.office {
        let title = deckcraft_plugin::get(plugin).map(|p| p.manifest.name.clone()).unwrap_or_default();
        s.ui_requests.push(match &office {
            deckcraft_plugin::office::OfficeCommand::ShowTaskpane { url, taskpane_id } => {
                UiRequest::OfficePane { plugin: plugin.to_string(), title, url: url.clone(), pane: taskpane_id.clone() }
            }
            deckcraft_plugin::office::OfficeCommand::ExecuteFunction { function, url, control } => {
                UiRequest::OfficeFunction { plugin: plugin.to_string(), url: url.clone(), function: function.clone(), control: control.clone() }
            }
        });
        return Ok(json!({"ran": 0, "office": office}));
    }
    if let Some((id, _)) = reply.commands.iter().find(|(id, _)| id.starts_with("plugins.") || id.starts_with("developer.")) {
        restore(s, before);
        return Err(bad(ID, format!("an add-in command can't run `{id}`")));
    }
    if let Err(e) = check_follow_up(plugin, &reply) {
        restore(s, before);
        return Err(bad(ID, format!("{plugin} › {command}: {e}")));
    }
    let mut failure = None;
    for (id, params) in &reply.commands {
        if let Err(e) = s.execute(id, params) {
            failure = Some(format!("{id}: {e}"));
            break;
        }
    }
    if let Some(why) = failure {
        restore(s, before);
        return Err(bad(ID, format!("{plugin} › {command} failed, nothing was changed: {why}")));
    }
    keep_if_unchanged(s, &before);
    if let Some(m) = &reply.message {
        s.ui_requests.push(UiRequest::Message { text: m.clone() });
    }
    let mut out = json!({"ran": reply.commands.len()});
    if let Some(m) = reply.message {
        out["message"] = json!(m);
    }
    if !reply.result.is_null() {
        out["result"] = reply.result;
    }
    // Protocol 1.2: what to do after the step, for whoever ran it (the app's buttons and menus do
    // it; a programmatic caller gets it here).
    if let Some(open) = reply.open {
        out["open"] = serde_json::to_value(open).unwrap_or(Json::Null);
    }
    if let Some(dialog) = reply.dialog {
        out["dialog"] = serde_json::to_value(dialog).unwrap_or(Json::Null);
    }
    // Protocol 2: a room to join or start, for the caller (the interface runs `collab.join`).
    if let Some(j) = reply.collab_join {
        out["collab"] = json!({"join": j});
    }
    Ok(out)
}
