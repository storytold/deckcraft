//! The Add-ins window (Insert ▸ Add-ins ▸ Get Add-ins and My Add-ins ▸ Manage Add-ins…) and the
//! form a plug-in command's parameters are asked in (RFC-0046). Every button ends in a `plugin.*`
//! command, so agents get the same result without the window: installing is two of them, a review
//! (`plugin.install {review: true}`) that the confirmation shows, then the install of that bundle
//! by its SHA-256, so nothing is downloaded twice. Downloads (a repository's release, a catalog,
//! an update check) run on a worker thread, so the window keeps drawing.

use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex};

use deckcraft_engine::plugin::craft_plugin::{Access, Fetch, Param, ParamKind};
use deckcraft_engine::plugin::{self, Candidate, Request};
use egui::{RichText, Ui};
use serde_json::{Value, json};

use super::{Dialog, buttons};
use crate::SlideApp;

/// Most characters of a name, a description or an error the window shows from a plug-in, a
/// catalog or a server, none of which are ours to trust.
const MAX_SHOWN: usize = 300;

/// At most `max` characters of `s`, on one line.
fn shown(s: &str, max: usize) -> String {
    let one: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).take(max).collect();
    if s.chars().count() > max { format!("{one}…") } else { one }
}

/// The window's pages.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    /// The plug-ins installed: run, update and remove them.
    #[default]
    Installed,
    /// Get more: from a file, a GitHub repository or a catalog.
    Get,
}

/// What a worker thread hands back.
#[derive(Debug)]
enum Done {
    /// The bundle a `plugin.install` request names (its parameters), or why it couldn't be had.
    Obtained { request: Value, got: Result<Candidate, String> },
    /// The catalogs' packages and their warnings.
    Packages(Vec<Value>, Vec<String>),
    /// The updates found: `{id, installed, available, install}`, and errors.
    Updates(Vec<Value>, Vec<String>),
}

/// The Add-ins window's state.
#[derive(Debug, Default)]
pub struct PluginsForm {
    pub page: Page,
    /// A `.craftplugin` file to install.
    pub path: String,
    /// `owner/repo` or `owner/repo@tag`.
    pub repo: String,
    /// A catalog address being typed.
    pub catalog: String,
    /// The packages the catalogs list (`plugin.catalog.list`).
    pub packages: Vec<Value>,
    /// Updates found by the last check (`plugin.update`).
    pub updates: Vec<Value>,
    /// The bundle under review: what `plugin.install {review: true}` said, and the parameters that
    /// named it. The confirmation shows while it is set.
    pub review: Option<(Value, Value)>,
    pub message: String,
    /// A download running on a worker thread.
    job: Option<Arc<Mutex<Receiver<Done>>>>,
}

impl PluginsForm {
    /// The window, on `page`.
    pub fn new(page: Page) -> PluginsForm {
        PluginsForm { page, ..PluginsForm::default() }
    }

    fn busy(&self) -> bool {
        self.job.is_some()
    }
}

/// Open the Add-ins window on `page`.
pub fn open(app: &mut SlideApp, page: Page) {
    app.plugins_form = Some(PluginsForm::new(page));
    app.dialog = Some(Dialog::new("plugins"));
}

/// Run `work` with an HTTPS client: on a worker thread natively (its answer arrives in the form),
/// at once on the web, where there is no thread and no network (local files only).
fn start(f: &mut PluginsForm, work: impl FnOnce(&mut dyn Fetch) -> Done + Send + 'static) -> Option<Done> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new().name("plugin-download".into()).spawn(move || {
            let done = match crate::plugin_http::http_fetch() {
                Some(mut http) => work(http.as_mut()),
                None => work(&mut plugin::NoNetwork),
            };
            let _ = tx.send(done);
        });
        match spawned {
            Ok(_) => f.job = Some(Arc::new(Mutex::new(rx))),
            Err(e) => f.message = shown(&e.to_string(), MAX_SHOWN),
        }
        None
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = f;
        Some(work(&mut plugin::NoNetwork))
    }
}

/// Get the bundle `request` (`plugin.install`'s parameters) names, for the confirmation.
fn obtain(app: &mut SlideApp, f: &mut PluginsForm, request: Value) {
    f.message.clear();
    let req = match Request::parse("plugin.install", &request) {
        Ok(req) => req,
        Err(e) => {
            f.message = shown(&e.to_string(), MAX_SHOWN);
            return;
        }
    };
    if !req.needs_network() {
        // A local file or catalog: quick, and read under the session's rules.
        let mut params = request.clone();
        params["review"] = json!(true);
        reviewed(f, request, app.run("plugin.install", params));
        return;
    }
    let catalogs = app.session.plugins.prefs.catalogs.clone();
    if let Some(done) = start(f, move |fetch| Done::Obtained { got: plugin::obtain_with(&req, fetch, &catalogs), request }) {
        finish(app, f, done);
    }
}

/// Show the confirmation for `result`, a `plugin.install {review: true}` answer.
fn reviewed(f: &mut PluginsForm, request: Value, result: Result<Value, String>) {
    match result {
        Ok(info) => f.review = Some((info, request)),
        Err(e) => f.message = shown(&e, MAX_SHOWN),
    }
}

/// A worker's answer arrived.
fn finish(app: &mut SlideApp, f: &mut PluginsForm, done: Done) {
    match done {
        Done::Obtained { request, got: Ok(candidate) } => {
            // The engine keeps the bundle for the install that follows the confirmation.
            let sha256 = plugin::hold(&mut app.session, request.clone(), candidate);
            let mut params = request.clone();
            params["review"] = json!(true);
            params["sha256"] = json!(sha256);
            reviewed(f, request, app.run("plugin.install", params));
        }
        Done::Obtained { got: Err(e), .. } => f.message = shown(&e, MAX_SHOWN),
        Done::Packages(packages, warnings) => {
            f.packages = packages;
            f.message = shown(&warnings.join("\n"), MAX_SHOWN);
        }
        Done::Updates(updates, errors) => {
            f.message =
                if updates.is_empty() && errors.is_empty() { "Every add-in is up to date.".into() } else { shown(&errors.join("\n"), MAX_SHOWN) };
            f.updates = updates;
        }
    }
}

/// Collect a worker's answer, if it has arrived.
fn poll(app: &mut SlideApp, ui: &Ui, f: &mut PluginsForm) {
    let Some(job) = f.job.clone() else { return };
    let got = job.lock().map(|rx| rx.try_recv()).unwrap_or(Err(TryRecvError::Disconnected));
    match got {
        Ok(done) => {
            f.job = None;
            finish(app, f, done);
        }
        Err(TryRecvError::Empty) => ui.ctx().request_repaint_after(std::time::Duration::from_millis(100)),
        Err(TryRecvError::Disconnected) => {
            f.job = None;
            f.message = "The download stopped.".into();
        }
    }
}

fn heading(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).strong());
}

fn error_line(ui: &mut Ui, message: &str) {
    if !message.is_empty() {
        ui.add(egui::Label::new(RichText::new(message).small().color(ui.visuals().error_fg_color)).wrap());
    }
}

/// The Add-ins window. Returns true to close.
pub(super) fn plugins_dialog(app: &mut SlideApp, ui: &mut Ui, _d: &mut Dialog) -> bool {
    let mut f = app.plugins_form.take().unwrap_or_default();
    let close = window(app, ui, &mut f);
    if !close {
        app.plugins_form = Some(f);
    }
    close
}

fn window(app: &mut SlideApp, ui: &mut Ui, f: &mut PluginsForm) -> bool {
    poll(app, ui, f);
    if f.review.is_some() {
        confirmation(app, ui, f);
        return false;
    }
    ui.horizontal(|ui| {
        ui.selectable_value(&mut f.page, Page::Installed, "My Add-ins");
        ui.selectable_value(&mut f.page, Page::Get, "Get Add-ins");
    });
    ui.separator();
    ui.add_enabled_ui(!f.busy(), |ui| match f.page {
        Page::Installed => installed(app, ui, f),
        Page::Get => get(app, ui, f),
    });
    if f.busy() {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("Downloading…");
        });
    }
    error_line(ui, &f.message);
    ui.add_space(8.0);
    let mut close = false;
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        close = ui.button("Close").clicked();
    });
    close
}

/// The installed page: each plug-in with its commands, an update when one was found, and Remove.
fn installed(app: &mut SlideApp, ui: &mut Ui, f: &mut PluginsForm) {
    let list: Vec<plugin::InstalledPlugin> = app.session.plugins.list().to_vec();
    if list.is_empty() {
        ui.label("No add-ins are installed. Get them from a file, a GitHub repository or a catalog.");
    }
    if !app.session.plugins.has_store() && !list.is_empty() {
        ui.label(RichText::new("These add-ins are kept for this session only.").small());
    }
    for p in &list {
        let m = &p.manifest;
        ui.add_space(4.0);
        ui.label(RichText::new(format!("{} {}", shown(&m.name, MAX_SHOWN), shown(&m.version, 40))).strong());
        if !m.author.is_empty() {
            ui.label(RichText::new(shown(&m.author, MAX_SHOWN)).small());
        }
        let access = if m.permissions.access == Access::Write { "Can edit the presentation" } else { "Can read the presentation" };
        ui.label(RichText::new(access).small());
        ui.horizontal_wrapped(|ui| {
            for c in &m.commands {
                if ui.button(shown(&c.label, MAX_SHOWN)).clicked() {
                    run_command(app, &m.id, &c.id);
                }
            }
            if let Some(u) = f.updates.iter().find(|u| u["id"].as_str().is_some_and(|id| id.eq_ignore_ascii_case(&m.id))).cloned()
                && ui.button(format!("Update to {}", shown(u["available"].as_str().unwrap_or_default(), 40))).clicked()
            {
                obtain(app, f, u["install"].clone());
            }
            if ui.button("Remove").clicked() {
                match app.run("plugin.remove", json!({"id": m.id})) {
                    Ok(_) => f.message.clear(),
                    Err(e) => f.message = shown(&e, MAX_SHOWN),
                }
            }
        });
    }
    for problem in app.session.plugins.problems().to_vec() {
        ui.label(RichText::new(shown(&problem, MAX_SHOWN)).small().color(ui.visuals().warn_fg_color));
    }
    ui.add_space(6.0);
    if !list.is_empty() && ui.button("Check for Updates").clicked() {
        f.message.clear();
        let mine = list;
        if let Some(done) = start(f, move |fetch| {
            let (mut updates, mut errors) = (Vec::new(), Vec::new());
            for p in &mine {
                match plugin::update_for(fetch, p) {
                    Ok(Some((install, available))) => {
                        updates.push(json!({"id": p.manifest.id, "installed": p.manifest.version, "available": available, "install": install}));
                    }
                    Ok(None) => {}
                    Err(e) => errors.push(format!("{}: {e}", p.manifest.name)),
                }
            }
            Done::Updates(updates, errors)
        }) {
            finish(app, f, done);
        }
    }
}

/// Can this host pick a file and hand its path back at once (the desktop)?
fn can_pick(app: &SlideApp) -> bool {
    app.services.open_async.is_none() && app.services.pick_open.is_some()
}

/// The Get Add-ins page: from a file, from a GitHub repository, from a catalog.
fn get(app: &mut SlideApp, ui: &mut Ui, f: &mut PluginsForm) {
    heading(ui, "From a file");
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut f.path).desired_width(260.0)).on_hover_text("A .craftplugin file");
        if ui.add_enabled(can_pick(app), egui::Button::new("Browse…")).clicked()
            && let Some(pick) = app.services.pick_open.as_mut()
            && let Some(path) = pick("plugin")
        {
            f.path = path;
        }
        if ui.add_enabled(!f.path.trim().is_empty(), egui::Button::new("Review…")).clicked() {
            let path = f.path.trim().to_string();
            obtain(app, f, json!({"path": path}));
        }
    });
    ui.add_space(6.0);
    heading(ui, "From a GitHub repository");
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut f.repo).desired_width(260.0)).on_hover_text("owner/repo, or owner/repo@tag for one release");
        if ui.add_enabled(!f.repo.trim().is_empty(), egui::Button::new("Review…")).clicked() {
            let repo = f.repo.trim().to_string();
            obtain(app, f, json!({"repo": repo}));
        }
    });
    ui.add_space(6.0);
    heading(ui, "From a catalog");
    let catalogs = app.session.plugins.prefs.catalogs.clone();
    for url in &catalogs {
        ui.horizontal(|ui| {
            ui.label(RichText::new(shown(url, MAX_SHOWN)).small());
            if ui.small_button("Remove").clicked() {
                let _ = app.run("plugin.catalog.add", json!({"url": url, "remove": true}));
                f.packages.retain(|p| p["catalog"].as_str() != Some(url.as_str()));
            }
        });
    }
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut f.catalog).desired_width(260.0).hint_text("https://"));
        if ui.add_enabled(!f.catalog.trim().is_empty(), egui::Button::new("Add Catalog")).clicked() {
            match app.run("plugin.catalog.add", json!({"url": f.catalog.trim()})) {
                Ok(_) => {
                    f.catalog.clear();
                    f.message.clear();
                }
                Err(e) => f.message = shown(&e, MAX_SHOWN),
            }
        }
    });
    let catalogs = app.session.plugins.prefs.catalogs.clone();
    if !catalogs.is_empty() && ui.button("Show the Catalogs' Add-ins").clicked() {
        f.message.clear();
        if let Some(done) = start(f, move |fetch| {
            let (mut packages, mut warnings) = (Vec::new(), Vec::new());
            for url in &catalogs {
                match plugin::catalog_packages_with(fetch, url) {
                    Ok((found, w)) => {
                        packages.extend(found);
                        warnings.extend(w.into_iter().map(|w| format!("{url}: {w}")));
                    }
                    Err(e) => warnings.push(format!("{url}: {e}")),
                }
            }
            Done::Packages(packages, warnings)
        }) {
            finish(app, f, done);
        }
    }
    for pkg in f.packages.clone() {
        ui.horizontal_wrapped(|ui| {
            let name = shown(pkg["name"].as_str().or(pkg["id"].as_str()).unwrap_or_default(), MAX_SHOWN);
            ui.label(RichText::new(format!("{name} {}", shown(pkg["version"].as_str().unwrap_or_default(), 40))).strong());
            if let Some(d) = pkg["description"].as_str() {
                ui.label(RichText::new(shown(d, MAX_SHOWN)).small());
            }
            if ui.button("Review…").clicked() {
                obtain(app, f, json!({"catalog": pkg["catalog"], "id": pkg["id"]}));
            }
        });
    }
}

/// The install confirmation: what the plug-in is, what it may do, where it comes from and its
/// SHA-256, and plainly when no catalog in use lists it.
fn confirmation(app: &mut SlideApp, ui: &mut Ui, f: &mut PluginsForm) {
    let Some((info, request)) = f.review.clone() else { return };
    let text = |k: &str| shown(info[k].as_str().unwrap_or_default(), MAX_SHOWN);
    ui.label(RichText::new(format!("{} {}", text("name"), text("version"))).strong());
    if !text("author").is_empty() {
        ui.label(format!("By {}", text("author")));
    }
    egui::Grid::new("plugin_review").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
        ui.label("Kind:");
        ui.label(text("kind"));
        ui.end_row();
        ui.label("Access:");
        let write = info["permissions"]["access"].as_str() == Some("write");
        ui.label(if write { "Can edit the presentation" } else { "Can read the presentation" });
        ui.end_row();
        ui.label("Commands it may call:");
        let commands: Vec<String> = info["permissions"]["commands"]
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_str).map(|c| shown(c, 80)).collect())
            .unwrap_or_default();
        ui.add(egui::Label::new(if commands.is_empty() { "Any command add-ins may call".to_string() } else { commands.join(", ") }).wrap());
        ui.end_row();
        ui.label("Source:");
        ui.label(source_text(&info["source"]));
        ui.end_row();
        ui.label("SHA-256:");
        ui.label(RichText::new(text("sha256")).monospace().small());
        ui.end_row();
    });
    let warn = ui.visuals().warn_fg_color;
    match info["inCatalog"].as_bool() {
        Some(true) => {}
        Some(false) => {
            ui.colored_label(warn, "This repository is in no catalog you use: install it only if you trust where it comes from.");
        }
        None => {
            ui.colored_label(warn, "This file is in no catalog: install it only if you trust where it comes from.");
        }
    }
    if let Some(old) = info["replaces"].as_str() {
        ui.label(format!("Replaces version {}.", shown(old, 40)));
    }
    if info["permissionsChanged"].as_bool() == Some(true) {
        ui.colored_label(warn, "It asks for other permissions than the version installed.");
    }
    error_line(ui, &f.message);
    let (ok, cancel) = buttons(ui, "Install");
    if cancel {
        f.review = None;
        f.message.clear();
    } else if ok {
        let mut params = request;
        params["sha256"] = info["sha256"].clone();
        match app.run("plugin.install", params) {
            Ok(r) => {
                f.review = None;
                f.message.clear();
                f.updates.retain(|u| u["id"] != info["id"]);
                f.page = Page::Installed;
                app.set_status(format!("Installed {}", shown(r["name"].as_str().unwrap_or_default(), MAX_SHOWN)));
            }
            Err(e) => f.message = shown(&e, MAX_SHOWN),
        }
    }
}

/// Where a bundle comes from, for the confirmation.
fn source_text(source: &Value) -> String {
    let s = |k: &str| shown(source[k].as_str().unwrap_or_default(), MAX_SHOWN);
    match source["type"].as_str() {
        Some("github") => format!("github.com/{} {}", s("repo"), s("tag")),
        Some("catalog") => format!("{} ({})", s("package"), s("catalog")),
        Some("file") => s("path"),
        _ => shown(&source.to_string(), MAX_SHOWN),
    }
}

/// Run plug-in `id`'s command `command`: at once when it takes no parameters, else after its form.
pub fn run_command(app: &mut SlideApp, id: &str, command: &str) {
    let Some(p) = app.session.plugins.get(id).cloned() else { return };
    let Some(c) = p.manifest.command(Some(command)) else { return };
    let params: Vec<Param> = c.params.iter().filter(|p| !p.name.is_empty() && p.is_field()).cloned().collect();
    if params.is_empty() {
        let _ = run(app, id, command, json!({}));
        return;
    }
    let values = params.iter().map(Param::default_value).collect();
    let title = if p.manifest.commands.len() > 1 {
        format!("{}: {}", p.manifest.name, c.label.trim_end_matches(['…', '.']))
    } else {
        p.manifest.name.clone()
    };
    app.plugin_params = Some(ParamsForm {
        id: id.to_string(),
        command: command.to_string(),
        title: shown(&title, MAX_SHOWN),
        params,
        values,
        message: String::new(),
    });
    app.dialog = Some(Dialog::new("pluginParams"));
}

/// Run it, and show what it says (its notes, or why it failed) on the status line.
fn run(app: &mut SlideApp, id: &str, command: &str, params: Value) -> Result<(), String> {
    match app.run("plugin.run", json!({"id": id, "command": command, "params": params})) {
        Ok(r) => {
            let notes: Vec<String> =
                r["notes"].as_array().map(|a| a.iter().filter_map(Value::as_str).map(|n| shown(n, MAX_SHOWN)).collect()).unwrap_or_default();
            if let Some(panel) = r["panel"].as_str() {
                app.set_status(shown(panel, MAX_SHOWN));
            } else if !notes.is_empty() {
                app.set_status(notes.join(" · "));
            }
            Ok(())
        }
        // `SlideApp::run` already put the error on the status line
        Err(e) => Err(shown(&e, MAX_SHOWN)),
    }
}

/// A plug-in command's parameters, asked before it runs (the manifest's `params`).
#[derive(Clone, Debug)]
pub struct ParamsForm {
    pub id: String,
    pub command: String,
    /// The plug-in's name (and the command's), as the window's heading.
    pub title: String,
    params: Vec<Param>,
    pub values: Vec<Value>,
    pub message: String,
}

/// The parameter form. Returns true to close.
pub(super) fn params_dialog(app: &mut SlideApp, ui: &mut Ui, _d: &mut Dialog) -> bool {
    let Some(mut f) = app.plugin_params.take() else { return true };
    let close = params_form(app, ui, &mut f);
    if !close {
        app.plugin_params = Some(f);
    }
    close
}

fn params_form(app: &mut SlideApp, ui: &mut Ui, f: &mut ParamsForm) -> bool {
    ui.label(RichText::new(f.title.as_str()).strong());
    egui::Grid::new("plugin_params").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
        for (i, (p, v)) in f.params.iter().zip(f.values.iter_mut()).enumerate() {
            ui.label(shown(p.title(), MAX_SHOWN));
            field(ui, i, p, v);
            ui.end_row();
        }
    });
    error_line(ui, &f.message);
    let (ok, cancel) = buttons(ui, "Run");
    if cancel {
        return true;
    }
    if ok {
        let params: serde_json::Map<String, Value> = f.params.iter().zip(&f.values).map(|(p, v)| (p.name.clone(), v.clone())).collect();
        return match run(app, &f.id, &f.command, Value::Object(params)) {
            Ok(()) => true,
            Err(e) => {
                f.message = e;
                false
            }
        };
    }
    false
}

/// One parameter's control.
fn field(ui: &mut Ui, i: usize, p: &Param, v: &mut Value) {
    match p.kind {
        ParamKind::Number | ParamKind::Slider => {
            let mut n = v.as_f64().filter(|n| n.is_finite()).unwrap_or(0.0);
            let (lo, hi) = (p.min.filter(|x| x.is_finite()).unwrap_or(f64::MIN), p.max.filter(|x| x.is_finite()).unwrap_or(f64::MAX));
            let changed = if p.kind == ParamKind::Slider && lo <= hi && lo > f64::MIN && hi < f64::MAX {
                ui.add(egui::Slider::new(&mut n, lo..=hi)).changed()
            } else {
                ui.add(egui::DragValue::new(&mut n).range(lo.min(hi)..=hi.max(lo))).changed()
            };
            if changed {
                *v = json!(n);
            }
        }
        ParamKind::Boolean => {
            let mut b = v.as_bool().unwrap_or(false);
            if ui.checkbox(&mut b, "").changed() {
                *v = json!(b);
            }
        }
        ParamKind::Select | ParamKind::List => {
            let current = v.as_str().unwrap_or_default().to_string();
            craft_ui::ComboBox::from_id_salt(("plugin_param", i)).selected_text(shown(&current, MAX_SHOWN)).show_ui(ui, |ui| {
                for o in &p.options {
                    if ui.selectable_label(*o == current, shown(o, MAX_SHOWN)).clicked() {
                        *v = json!(o);
                    }
                }
            });
        }
        _ => {
            let mut t = v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string());
            if ui.add(egui::TextEdit::singleline(&mut t).desired_width(220.0)).changed() {
                *v = json!(t);
            }
        }
    }
}

/// My Add-ins' menu: every installed plug-in's commands, then Manage Add-ins…. Returns whether an
/// entry was clicked (the menu then closes).
pub fn my_addins_menu(app: &mut SlideApp, ui: &mut Ui) -> bool {
    let list: Vec<plugin::InstalledPlugin> = app.session.plugins.list().to_vec();
    let can_run = app.session.active().is_some();
    let mut clicked = false;
    if list.is_empty() {
        ui.label(RichText::new("No add-ins installed").weak());
    }
    for p in &list {
        let m = &p.manifest;
        for c in &m.commands {
            let label = if m.commands.len() > 1 { format!("{}: {}", m.name, c.label) } else { m.name.clone() };
            if ui.add_enabled(can_run, egui::Button::new(shown(&label, 80))).clicked() {
                run_command(app, &m.id, &c.id);
                clicked = true;
            }
        }
    }
    ui.separator();
    if ui.button("Manage Add-ins…").clicked() {
        open(app, Page::Installed);
        clicked = true;
    }
    clicked
}

#[cfg(test)]
mod tests;
