//! Quick Access Toolbar: the command buttons at the left of the title bar, which the user can
//! add to (right-click a ribbon button, or the toolbar's Customize menu), remove from and reset.

use egui::Ui;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::SlideApp;
use crate::icons::Icon;

/// One toolbar button: a command, its parameters, and how it looks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct QatItem {
    pub id: String,
    #[serde(skip_serializing_if = "Value::is_null")]
    pub params: Value,
    pub label: String,
    /// An [`Icon::name`].
    pub icon: String,
}

impl Default for QatItem {
    fn default() -> Self {
        QatItem { id: String::new(), params: Value::Null, label: String::new(), icon: Icon::More.name().into() }
    }
}

impl QatItem {
    pub fn new(id: &str, label: &str, icon: Icon) -> Self {
        QatItem { id: id.into(), params: Value::Null, label: label.into(), icon: icon.name().into() }
    }
    fn icon(&self) -> Icon {
        Icon::from_name(&self.icon).unwrap_or(Icon::More)
    }
    fn same(&self, id: &str, params: &Value) -> bool {
        self.id == id && (self.params == *params || (self.params.is_null() && params.as_object().is_some_and(|o| o.is_empty())))
    }
}

/// The commands the Customize menu offers as checkboxes, in PowerPoint's order.
pub fn common() -> Vec<QatItem> {
    vec![
        QatItem::new("app.home", "Home", Icon::Home),
        QatItem::new("file.new", "New", Icon::File),
        QatItem::new("app.openDialog", "Open", Icon::Folder),
        QatItem::new("file.save", "Save", Icon::Save),
        QatItem::new("app.print", "Print", Icon::Print),
        QatItem::new("edit.undo", "Undo", Icon::Undo),
        QatItem::new("edit.redo", "Redo", Icon::Redo),
        QatItem::new("show.fromStart", "Start from Beginning", Icon::PlayFromStart),
        QatItem::new("app.find", "Find", Icon::Search),
        QatItem::new("review.spelling", "Spelling", Icon::Spelling),
        QatItem::new("app.exportDialog", "Export", Icon::Export),
    ]
}

/// The toolbar a new install starts with.
pub fn defaults() -> Vec<QatItem> {
    // Home, Save, Export, Undo, Redo, Start from Beginning: the title bar's buttons before it was customizable.
    let all = common();
    ["app.home", "file.save", "app.exportDialog", "edit.undo", "edit.redo", "show.fromStart"]
        .iter()
        .filter_map(|id| all.iter().find(|q| q.id == *id).cloned())
        .collect()
}

/// `app.qat`: `{add: id, params?, label?, icon?} | {remove: id, params?} | {reset: true}` → the toolbar.
pub fn command(app: &mut SlideApp, p: &Value) -> Result<Value, String> {
    let params = p.get("params").cloned().unwrap_or(Value::Null);
    if p.get("reset").and_then(Value::as_bool) == Some(true) {
        app.ui.qat = defaults();
    }
    if let Some(id) = p.get("add").and_then(Value::as_str) {
        if !crate::ribbon::known(id) {
            return Err(format!("unknown command `{id}`"));
        }
        if !app.ui.qat.iter().any(|q| q.same(id, &params)) {
            let base = common().into_iter().find(|q| q.id == id);
            let label = p.get("label").and_then(Value::as_str).map(String::from).or_else(|| base.as_ref().map(|b| b.label.clone()));
            let label = label.or_else(|| deckcraft_engine::find_command(id).map(|c| c.label.to_string())).unwrap_or_else(|| id.to_string());
            let icon = p.get("icon").and_then(Value::as_str).and_then(Icon::from_name).or_else(|| base.map(|b| b.icon())).unwrap_or(Icon::More);
            app.ui.qat.push(QatItem { id: id.into(), params: params.clone(), label, icon: icon.name().into() });
        }
    }
    if let Some(id) = p.get("remove").and_then(Value::as_str) {
        app.ui.qat.retain(|q| !q.same(id, &params));
    }
    serde_json::to_value(&app.ui.qat).map_err(|e| e.to_string())
}

fn run(app: &mut SlideApp, q: &QatItem) {
    match q.id.as_str() {
        "file.save" => app.save(),
        "show.fromStart" => app.start_show(app.show_start_index(), false),
        "review.spelling" => app.dialog = Some(crate::dialogs::Dialog::new("spelling")),
        id => {
            let p = if q.params.is_null() { json!({}) } else { q.params.clone() };
            let _ = app.run(id, p);
        }
    }
}

/// Whether a Quick Access Toolbar item can be run right now.
pub fn is_enabled(app: &SlideApp, id: &str) -> bool {
    match id {
        "app.home" => true,
        "file.save" | "show.fromStart" | "review.spelling" | "app.find" => app.session.active().is_some(),
        id => crate::ribbon::enabled(app, id),
    }
}

/// Runs the Quick Access Toolbar item at `index` (0-based) if it exists and is enabled. Returns `true` if run.
pub fn run_index(app: &mut SlideApp, index: usize) -> bool {
    let Some(q) = app.ui.qat.get(index).cloned() else { return false };
    if !is_enabled(app, &q.id) {
        return false;
    }
    run(app, &q);
    true
}

fn tip(q: &QatItem, index: usize, mac: bool) -> String {
    let sc =
        deckcraft_engine::find_command(&q.id).and_then(|c| c.shortcut).or_else(|| crate::UI_COMMANDS.iter().find(|c| c.0 == q.id).and_then(|c| c.2));
    let qat_sc = if !mac && index < 9 { Some(format!("Alt+{}", index + 1)) } else { None };
    match (sc, qat_sc) {
        (Some(s), Some(qsc)) => format!("{} ({}, {})", q.label, crate::ribbon::pretty_shortcut(s), qsc),
        (Some(s), None) => format!("{} ({})", q.label, crate::ribbon::pretty_shortcut(s)),
        (None, Some(qsc)) => format!("{} ({})", q.label, qsc),
        (None, None) => q.label.clone(),
    }
}

/// Draw the toolbar and its Customize menu into a left-to-right row of the title bar.
pub fn show(app: &mut SlideApp, ui: &mut Ui) {
    let items = app.ui.qat.clone();
    let mac = app.session.mac_keys;
    for (i, q) in items.iter().enumerate() {
        let on = q.id == "app.home" && app.backstage_open();
        let en = is_enabled(app, &q.id);
        let r = crate::widgets::icon_toggle(ui, q.icon(), &tip(q, i, mac), en, on);
        if r.clicked() {
            run(app, q);
        }
        r.context_menu(|ui| {
            if ui.button("Remove from Quick Access Toolbar").clicked() {
                let _ = command(app, &json!({"remove": q.id, "params": q.params}));
                ui.close();
            }
        });
    }
    let r = crate::widgets::icon_toggle(ui, Icon::ChevronDown, "Customize Quick Access Toolbar", true, false);
    egui::Popup::menu(&r).show(|ui| {
        ui.label(egui::RichText::new("Customize Quick Access Toolbar").strong());
        for c in common() {
            let mut on = app.ui.qat.iter().any(|q| q.same(&c.id, &Value::Null));
            if ui.checkbox(&mut on, &c.label).changed() {
                let key = if on { "add" } else { "remove" };
                let _ = command(app, &json!({key: c.id}));
            }
        }
        let extra: Vec<QatItem> = app.ui.qat.iter().filter(|q| !common().iter().any(|c| q.same(&c.id, &Value::Null))).cloned().collect();
        if !extra.is_empty() {
            ui.separator();
            for q in extra {
                let mut on = true;
                if ui.checkbox(&mut on, &q.label).changed() {
                    let _ = command(app, &json!({"remove": q.id, "params": q.params}));
                }
            }
        }
        ui.separator();
        ui.label(egui::RichText::new("Right-click any ribbon button to add it.").weak());
        if ui.button("Reset to Defaults").clicked() {
            let _ = command(app, &json!({"reset": true}));
            ui.close();
        }
    });
}

/// The ribbon buttons' right-click menu: add the button to the toolbar.
pub fn ribbon_menu(app: &mut SlideApp, r: &egui::Response, icon: Icon, label: &str, id: &str, p: &Value) {
    r.context_menu(|ui| {
        let has = app.ui.qat.iter().any(|q| q.same(id, p));
        let text = if has { "Remove from Quick Access Toolbar" } else { "Add to Quick Access Toolbar" };
        if ui.button(text).clicked() {
            let label = label.replace('\n', " ");
            let key = if has { "remove" } else { "add" };
            let _ = command(app, &json!({key: id, "params": p, "label": label, "icon": icon.name()}));
            ui.close();
        }
    });
}
