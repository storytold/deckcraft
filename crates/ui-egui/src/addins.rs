//! Add-ins in the interface: their ribbon groups and right-click menu items, the Add-ins window
//! (install from GitHub, turn on and off, unload; add-ins that need other add-ins, manifest 1.3:
//! why one is blocked, installing what it needs, asking before turning off one others need), and
//! the Developer tab for an add-in that runs
//! macros (VBA): Visual Basic (the presentation's code modules in an editor), Macros (list, run,
//! edit, create), Macro Security, and the "macros disabled" bar with Enable Content. Everything
//! goes through the `plugins.*` and `developer.*` commands; the add-ins own their behaviour,
//! DeckCraft the interface.
//!
//! Record Macro / Stop Recording (a DeckCraft extra: PowerPoint has no macro recorder) records
//! what you do and puts it in Module1 as the add-in's code. The presentation's own ribbon buttons
//! (Developer › Ribbon Buttons, `developer.*RibbonButton`) run its macros: they show on the tab
//! they name, a built-in one or their own, whether or not a macro add-in is loaded (without one, a
//! click says to install one), and are saved with the presentation so PowerPoint shows them too.

use std::collections::HashSet;
use std::rc::Rc;

use deckcraft_engine::plugin::{self, ButtonSize, MenuKind};
use deckcraft_model::RibbonGroup;
use egui::{RichText, Ui, vec2};
use serde_json::{Value as Json, json};

use crate::SlideApp;
use crate::icons::Icon;
use crate::widgets::{big_button, group, small_button};

/// The add-ins' windows and the install in progress.
#[derive(Default)]
pub struct State {
    pub window: bool,
    pub install: Install,
    pub load_path: String,
    pub macros: Option<MacrosWindow>,
    pub editor: Option<Editor>,
    pub security: bool,
    pub buttons: Option<ButtonsWindow>,
    /// A dialog an add-in asked for (protocol 1.2), drawn by DeckCraft.
    pub dialog: Option<PluginDialog>,
    /// The presentation's ribbon as last read: (presentation uid, model pointer, tabs, editable).
    deck: Option<(u64, usize, DeckTabs, bool)>,
    /// Presentations whose "macros disabled" bar was dismissed or acted on.
    notice_done: HashSet<u64>,
}

/// The presentation's ribbon by DeckCraft tab name: (tab, groups).
type DeckTabs = Rc<Vec<(String, Vec<RibbonGroup>)>>;

/// Developer › Ribbon Buttons: the presentation's buttons, and the form adding one.
#[derive(Default)]
pub struct ButtonsWindow {
    pub macros: Vec<String>,
    pub macro_name: String,
    pub label: String,
    pub tab: String,
    pub group: String,
    pub image: String,
    pub small: bool,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct MacrosWindow {
    pub list: Vec<Json>,
    pub selected: usize,
    pub new_name: String,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct Editor {
    /// (name, kind, source)
    pub modules: Vec<(String, String, String)>,
    pub current: usize,
    pub text: String,
    pub dirty: bool,
    pub error: Option<String>,
    pub new_module: String,
}

/// Installing from GitHub: the address typed, the confirmation step, and the download, which runs
/// on its own thread so the window keeps painting. Also the questions requirements between
/// add-ins raise (manifest 1.3): installing the add-ins an add-in needs, updating one to a version
/// an add-in that needs it doesn't accept, and turning off one others need.
#[derive(Default)]
pub struct Install {
    pub url: String,
    /// The repository waiting for the user's go-ahead (`owner/name`).
    pub confirming: Option<String>,
    pending: Option<std::sync::mpsc::Receiver<Result<Done, String>>>,
    /// The last install's outcome: `Ok(message)` or `Err(error)`.
    pub outcome: Option<Result<String, String>>,
    /// A download whose version an installed add-in that needs it doesn't accept: what to say.
    pub confirm_update: Option<(plugin::install::Fetched, String)>,
    /// The add-ins an add-in needs, offered for installing ("Install it too?").
    pub offer: Option<Offer>,
    /// Turning off or unloading an add-in others need: asking first.
    pub confirm_off: Option<ConfirmOff>,
}

impl Install {
    /// A download is running.
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }
}

/// The missing add-ins of `plugin` offered for installing, each with whether it's ticked
/// (required ones are, optional ones aren't).
pub struct Offer {
    pub plugin: String,
    pub name: String,
    pub items: Vec<(plugin::requires::Missing, bool)>,
}

/// Turning off (or unloading) `id`, which `dependents` need.
pub struct ConfirmOff {
    pub id: String,
    pub unload: bool,
    pub dependents: Vec<String>,
}

/// What a download thread hands back.
enum Done {
    /// Downloaded; the add-ins that need it don't accept its version (the messages).
    Conflicts(Box<plugin::install::Fetched>, Vec<String>),
    /// Unpacked into this folder.
    Unpacked(std::path::PathBuf),
    /// Required add-ins: per add-in, its folder and release, or why it failed.
    Requirements(Vec<(String, Result<(std::path::PathBuf, Option<String>), String>)>),
}

/// A button or menu item an add-in contributes.
#[derive(Clone, Debug)]
pub struct Item {
    pub plugin: String,
    pub command: String,
    pub label: String,
    pub tooltip: String,
    pub icon: Icon,
    pub small: bool,
    pub params: Json,
}

/// Whether the Developer tab shows: an add-in that runs macros is loaded.
pub fn developer_visible() -> bool {
    !plugin::macro_plugins().is_empty()
}

fn language() -> String {
    plugin::macro_plugins().into_iter().next().map(|(_, l)| l).unwrap_or_else(|| "VBA".into())
}

/// Tabs the add-ins add (after View): their own tab names, not the built-in ones.
pub fn extra_tabs(built_in: &[&str]) -> Vec<String> {
    let mut tabs: Vec<String> = Vec::new();
    for p in plugin::list().into_iter().filter(|p| p.active()) {
        for g in &p.manifest.ui.ribbon {
            let known = built_in.iter().any(|t| t.eq_ignore_ascii_case(&g.tab)) || (g.tab == "Developer" && developer_visible());
            if !known && !tabs.contains(&g.tab) {
                tabs.push(g.tab.clone());
            }
        }
    }
    tabs
}

/// The add-ins' groups on `tab`: (group name, buttons).
pub fn groups(tab: &str) -> Vec<(String, Vec<Item>)> {
    let mut out = Vec::new();
    for p in plugin::list().into_iter().filter(|p| p.active()) {
        let m = &p.manifest;
        for g in m.ui.ribbon.iter().filter(|g| g.tab == tab) {
            let buttons = g
                .buttons
                .iter()
                .map(|b| {
                    let decl = m.command(&b.command);
                    Item {
                        plugin: m.id.clone(),
                        command: b.command.clone(),
                        label: b.label.clone().or_else(|| decl.map(|c| c.label.clone())).unwrap_or_else(|| b.command.clone()),
                        tooltip: b.tooltip.clone().or_else(|| decl.map(|c| c.description.clone())).unwrap_or_default(),
                        icon: b.icon.as_deref().and_then(icon_named).unwrap_or(Icon::Macros),
                        small: b.size == ButtonSize::Small,
                        params: b.params.clone(),
                    }
                })
                .collect();
            out.push((g.group.clone(), buttons));
        }
    }
    out
}

/// The add-ins' items for a right-click menu.
pub fn menu_items(kind: MenuKind) -> Vec<Item> {
    let mut out = Vec::new();
    for p in plugin::list().into_iter().filter(|p| p.active()) {
        let m = &p.manifest;
        for item in m.ui.menus.iter().filter(|i| i.menu == kind) {
            let decl = m.command(&item.command);
            out.push(Item {
                plugin: m.id.clone(),
                command: item.command.clone(),
                label: item.label.clone().or_else(|| decl.map(|c| c.label.clone())).unwrap_or_else(|| item.command.clone()),
                tooltip: decl.map(|c| c.description.clone()).unwrap_or_default(),
                icon: Icon::Macros,
                small: true,
                params: item.params.clone(),
            });
        }
    }
    out
}

/// Runs an add-in's command (a ribbon button or menu item), then does what it asked for after
/// its step: open a file, show a dialog.
pub fn run(app: &mut SlideApp, item: &Item) {
    let _ = run_command(app, &item.plugin, &item.command, item.params.clone());
}

/// Runs `plugin`'s `command` from DeckCraft's own interface (a button, a menu item, a dialog's
/// button) and acts on its protocol 1.2 answer: `open` opens the file the way File › Open does,
/// `dialog` shows a dialog whose buttons run more of its commands. A programmatic `plugins.run`
/// only returns them. Errors show in the status bar and are returned.
pub fn run_command(app: &mut SlideApp, plugin: &str, command: &str, params: Json) -> Result<(), String> {
    let r = app.run("plugins.run", json!({"plugin": plugin, "command": command, "params": params}))?;
    if let Some(open) = r.get("open").and_then(|o| serde_json::from_value::<plugin::OpenRequest>(o.clone()).ok()) {
        // `readOnly` is ignored: DeckCraft has no read-only mode.
        app.open_path_labelled(&open.path, open.label);
    }
    if let Some(d) = r.get("dialog").and_then(|d| serde_json::from_value::<plugin::Dialog>(d.clone()).ok()) {
        app.addins.dialog = Some(PluginDialog::new(plugin, d));
    }
    // Protocol 2: a transport's command answered with a room to join or start.
    if let Err(e) = crate::collab_ui::join_from_plugin(app, plugin, &r) {
        app.set_status(e.clone());
        return Err(e);
    }
    Ok(())
}

// ---------------------------------------------------------------- add-ins' dialogs (protocol 1.2)

/// A form field's value as typed.
#[derive(Clone, Debug, PartialEq)]
pub enum FieldValue {
    Text(String),
    Check(bool),
}

/// A dialog an add-in asked for: what it shows and what the user has chosen or typed.
#[derive(Clone, Debug)]
pub struct PluginDialog {
    pub plugin: String,
    pub dialog: plugin::Dialog,
    /// The list's filter (shown above [`plugin::dialog::FILTER_ABOVE`] items).
    pub filter: String,
    /// Selected items, by index, in the order chosen.
    pub selected: Vec<usize>,
    pub values: Vec<FieldValue>,
    /// Why the last OK didn't run (a number that isn't one) or failed.
    pub error: Option<String>,
}

impl PluginDialog {
    pub fn new(plugin: &str, dialog: plugin::Dialog) -> PluginDialog {
        use plugin::dialog::FieldKind;
        let values = dialog
            .fields
            .iter()
            .map(|f| match f.kind {
                FieldKind::Checkbox => FieldValue::Check(f.value.as_bool().unwrap_or(false)),
                _ => FieldValue::Text(f.value.as_str().unwrap_or("").to_string()),
            })
            .collect();
        PluginDialog { plugin: plugin.to_string(), dialog, filter: String::new(), selected: Vec::new(), values, error: None }
    }

    /// The selected items' ids.
    pub fn selected_ids(&self) -> Vec<String> {
        self.selected.iter().filter_map(|i| self.dialog.items.get(*i)).map(|it| it.id.clone()).collect()
    }

    /// The form's `fields`, or what's wrong with them.
    pub fn fields(&self) -> Result<serde_json::Map<String, Json>, String> {
        use plugin::dialog::FieldKind;
        let mut out = serde_json::Map::new();
        for (f, v) in self.dialog.fields.iter().zip(&self.values) {
            let value = match (f.kind, v) {
                (_, FieldValue::Check(b)) => json!(b),
                (FieldKind::Number, FieldValue::Text(t)) => plugin::dialog::number_value(t).ok_or_else(|| format!("{}: type a number", f.label))?,
                (_, FieldValue::Text(t)) => json!(t),
            };
            out.insert(f.id.clone(), value);
        }
        Ok(out)
    }

    /// The items the filter lets through, by index.
    pub fn visible(&self) -> Vec<usize> {
        let f = self.filter.trim().to_lowercase();
        let filtering = self.dialog.items.len() > plugin::dialog::FILTER_ABOVE && !f.is_empty();
        (0..self.dialog.items.len())
            .filter(|i| !filtering || self.dialog.items.get(*i).is_some_and(|it| it.label.to_lowercase().contains(&f)))
            .collect()
    }

    /// A click on item `i`: selects it (with `multi`, adds or removes it).
    pub fn click(&mut self, i: usize) {
        if self.dialog.multi {
            match self.selected.iter().position(|x| *x == i) {
                Some(k) => {
                    self.selected.remove(k);
                }
                None => self.selected.push(i),
            }
        } else {
            self.selected = vec![i];
        }
    }

    /// The command and params OK (`None`) or extra button `button` runs, or why it can't run.
    /// `Ok(None)`: nothing to run (a message's OK without a command just closes).
    pub fn action(&self, button: Option<usize>) -> Result<Option<(String, Json)>, String> {
        let (command, base) = match button {
            Some(b) => match self.dialog.buttons.get(b) {
                Some(b) => (b.command.clone(), &b.params),
                None => return Ok(None),
            },
            None => match &self.dialog.command {
                Some(c) => (c.clone(), &self.dialog.params),
                None => return Ok(None),
            },
        };
        let fields = if self.dialog.kind == plugin::dialog::DialogKind::Form { Some(self.fields()?) } else { None };
        // OK on a list needs an item; a button runs with or without one.
        if button.is_none() && self.dialog.kind == plugin::dialog::DialogKind::List && self.selected.is_empty() {
            return Err("Choose an item first".into());
        }
        Ok(Some((command, plugin::dialog::action_params(base, &self.dialog, &self.selected_ids(), fields))))
    }
}

/// The icon a dialog item names: the app's icon names as for ribbon buttons, a few file kinds, and
/// a generic file for anything else.
pub fn item_icon(name: &str) -> Icon {
    match name.to_ascii_lowercase().as_str() {
        "folder" | "directory" => Icon::Folder,
        "" | "file" | "document" | "workbook" | "presentation" | "spreadsheet" => Icon::File,
        other => icon_named(other).or_else(|| Icon::from_name(other)).unwrap_or(Icon::File),
    }
}

/// Runs what the user chose in an add-in's dialog: the dialog closes and the command runs; if it
/// fails, the dialog comes back with the reason, so a mistyped password can be typed again.
fn dialog_action(app: &mut SlideApp, d: PluginDialog, button: Option<usize>) {
    let (command, params) = match d.action(button) {
        Ok(Some(x)) => x,
        Ok(None) => return,
        Err(e) => {
            app.addins.dialog = Some(PluginDialog { error: Some(e), ..d });
            return;
        }
    };
    if let Err(e) = run_command(app, &d.plugin, &command, params)
        && app.addins.dialog.is_none()
    {
        app.addins.dialog = Some(PluginDialog { error: Some(e), ..d });
    }
}

/// Draws the add-in's dialog, a modal in DeckCraft's own widgets and theme.
pub fn plugin_dialog_window(app: &mut SlideApp, ctx: &egui::Context) {
    use plugin::dialog::{DialogKind, FieldKind};
    let Some(mut d) = app.addins.dialog.take() else { return };
    let t = crate::theme::Tokens::get(ctx);
    let mut close = false;
    let mut act: Option<Option<usize>> = None;
    let title = if d.dialog.title.trim().is_empty() {
        plugin::get(&d.plugin).map(|p| p.manifest.name.clone()).unwrap_or_else(|| d.plugin.clone())
    } else {
        d.dialog.title.clone()
    };
    let modal = egui::Modal::new(egui::Id::new("addin_dialog")).show(ctx, |ui| {
        ui.set_width(480.0);
        ui.label(RichText::new(&title).font(crate::theme::bold(15.0)).color(t.text));
        ui.add_space(6.0);
        if !d.dialog.prompt.is_empty() {
            let prompt = RichText::new(&d.dialog.prompt).color(if d.dialog.kind == DialogKind::Message { t.text } else { t.text_dim });
            egui::ScrollArea::vertical().id_salt("addin_dialog_prompt").max_height(240.0).show(ui, |ui| {
                ui.add(egui::Label::new(prompt).wrap());
            });
            ui.add_space(6.0);
        }
        match d.dialog.kind {
            DialogKind::List => {
                if d.dialog.items.len() > plugin::dialog::FILTER_ABOVE {
                    ui.add(egui::TextEdit::singleline(&mut d.filter).hint_text("Filter").desired_width(f32::INFINITY));
                    ui.add_space(4.0);
                }
                let visible = d.visible();
                let row_h = 26.0;
                egui::Frame::new().fill(t.field_bg).stroke(egui::Stroke::new(1.0, t.border)).corner_radius(4).inner_margin(2).show(ui, |ui| {
                    ui.set_min_height(row_h * 8.0);
                    egui::ScrollArea::vertical().id_salt("addin_dialog_items").max_height(row_h * 12.0).auto_shrink([false, true]).show_rows(
                        ui,
                        row_h,
                        visible.len(),
                        |ui, rows| {
                            for i in visible.get(rows).unwrap_or(&[]).iter().copied() {
                                let Some(item) = d.dialog.items.get(i) else { continue };
                                let selected = d.selected.contains(&i);
                                let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), egui::Sense::click());
                                if selected {
                                    ui.painter().rect_filled(rect, 3.0, t.selected);
                                } else if resp.hovered() {
                                    ui.painter().rect_filled(rect, 3.0, t.hover);
                                }
                                let icon = egui::Rect::from_min_size(rect.min + vec2(6.0, 5.0), vec2(16.0, 16.0));
                                crate::icons::paint(ui.painter(), icon, item_icon(&item.icon), t.text, false);
                                let font = egui::FontId::proportional(13.0);
                                let detail_w = if item.detail.is_empty() { 0.0 } else { (rect.width() * 0.4).min(200.0) };
                                let label_rect = egui::Rect::from_min_max(rect.min + vec2(30.0, 0.0), rect.max - vec2(detail_w + 6.0, 0.0));
                                ui.painter().with_clip_rect(label_rect).text(
                                    label_rect.left_center(),
                                    egui::Align2::LEFT_CENTER,
                                    &item.label,
                                    font.clone(),
                                    t.text,
                                );
                                if detail_w > 0.0 {
                                    let detail_rect =
                                        egui::Rect::from_min_max(egui::pos2(rect.max.x - detail_w - 6.0, rect.min.y), rect.max - vec2(6.0, 0.0));
                                    ui.painter().with_clip_rect(detail_rect).text(
                                        detail_rect.left_center(),
                                        egui::Align2::LEFT_CENTER,
                                        &item.detail,
                                        font,
                                        t.text_dim,
                                    );
                                }
                                let label = item.label.clone();
                                resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, &label));
                                if resp.double_clicked() {
                                    d.selected = vec![i];
                                    act = Some(None);
                                } else if resp.clicked() {
                                    d.click(i);
                                }
                            }
                        },
                    );
                });
                if d.dialog.multi {
                    ui.label(RichText::new(format!("{} selected", d.selected.len())).color(t.text_dim));
                }
            }
            DialogKind::Form => {
                egui::Grid::new("addin_dialog_form").num_columns(2).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
                    for (k, f) in d.dialog.fields.iter().enumerate() {
                        let Some(v) = d.values.get_mut(k) else { continue };
                        match (f.kind, v) {
                            (FieldKind::Checkbox, FieldValue::Check(b)) => {
                                ui.label("");
                                ui.checkbox(b, &f.label);
                            }
                            (FieldKind::Choice, FieldValue::Text(chosen)) => {
                                let l = ui.label(&f.label);
                                let shown = f.options.iter().find(|o| o.id == *chosen).map(|o| o.label.clone()).unwrap_or_default();
                                egui::ComboBox::from_id_salt(("addin_dialog_choice", k))
                                    .selected_text(shown)
                                    .width(300.0)
                                    .show_ui(ui, |ui| {
                                        for o in &f.options {
                                            ui.selectable_value(chosen, o.id.clone(), &o.label);
                                        }
                                    })
                                    .response
                                    .labelled_by(l.id);
                            }
                            (kind, FieldValue::Text(text)) => {
                                let l = ui.label(&f.label);
                                let edit = egui::TextEdit::singleline(text).password(kind == FieldKind::Secret).desired_width(300.0);
                                ui.add(edit).labelled_by(l.id);
                            }
                            (_, FieldValue::Check(b)) => {
                                ui.label("");
                                ui.checkbox(b, &f.label);
                            }
                        }
                        ui.end_row();
                    }
                });
            }
            DialogKind::Message => {}
        }
        if let Some(e) = &d.error {
            ui.add_space(4.0);
            ui.colored_label(crate::icons::RED, e);
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            for (k, b) in d.dialog.buttons.iter().enumerate() {
                if ui.button(&b.label).clicked() {
                    act = Some(Some(k));
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Cancel").clicked() {
                    close = true;
                }
                let ok = egui::Button::new(RichText::new(&d.dialog.ok_label).color(t.accent_text)).fill(t.accent);
                if ui.add(ok).clicked() {
                    act = Some(None);
                }
            });
        });
    });
    if modal.should_close() || ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        close = true;
    }
    match act {
        _ if close => {}
        Some(button) => {
            // A message's OK without a command just closes.
            if button.is_some() || d.dialog.command.is_some() {
                dialog_action(app, d, button);
            }
        }
        None => app.addins.dialog = Some(d),
    }
}

/// The add-ins' items at the end of a right-click menu (with a separator before them).
pub fn context_items(app: &mut SlideApp, ui: &mut Ui, kind: MenuKind) {
    let items = menu_items(kind);
    if items.is_empty() {
        return;
    }
    ui.separator();
    for item in items {
        if ui.button(&item.label).on_hover_text(&item.tooltip).clicked() {
            run(app, &item);
            ui.close();
        }
    }
}

/// Names a painted button for screen readers (and tests).
fn named(resp: egui::Response, label: &str) -> egui::Response {
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    resp
}

/// Draws the add-ins' groups on `tab`.
pub fn ribbon_groups(app: &mut SlideApp, ui: &mut Ui, tab: &str) {
    for (_group, buttons) in groups(tab) {
        group(ui, |ui| {
            let mut k = 0;
            while let Some(b) = buttons.get(k) {
                if !b.small {
                    if named(big_button(ui, b.icon, &b.label, true), &b.label).on_hover_text(&b.tooltip).clicked() {
                        run(app, b);
                    }
                    k += 1;
                    continue;
                }
                let column: Vec<&Item> = buttons.iter().skip(k).take_while(|b| b.small).take(3).collect();
                k += column.len();
                ui.vertical(|ui| {
                    for b in column {
                        if named(small_button(ui, b.icon, &b.label, true, false), &b.label).on_hover_text(&b.tooltip).clicked() {
                            run(app, b);
                        }
                    }
                });
            }
        });
    }
}

/// The icon a manifest names (`"icon": "chart"`); case and `-`/`_` don't matter.
pub fn icon_named(name: &str) -> Option<Icon> {
    let key: String = name.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
    Some(match key.as_str() {
        "script" | "plugin" | "macro" | "macros" => Icon::Macros,
        "paste" => Icon::Paste,
        "copy" => Icon::Copy,
        "cut" => Icon::Cut,
        "slide" | "newslide" => Icon::NewSlide,
        "table" => Icon::Table,
        "chart" => Icon::Chart,
        "picture" | "image" => Icon::Picture,
        "shapes" | "shape" => Icon::Shapes,
        "textbox" | "text" => Icon::TextBox,
        "link" => Icon::Link,
        "comment" => Icon::Comment,
        "search" | "find" => Icon::Search,
        "lock" => Icon::Lock,
        "settings" => Icon::Settings,
        "share" => Icon::Share,
        "folder" | "open" => Icon::OpenFolder,
        "save" => Icon::Save,
        "print" => Icon::Print,
        "info" => Icon::Info,
        "check" => Icon::Check,
        "plus" | "add" => Icon::Plus,
        "minus" => Icon::Minus,
        "pen" => Icon::Pen,
        "star" => Icon::Star,
        "sparkle" | "ai" => Icon::Sparkle,
        "palette" | "theme" => Icon::Palette,
        "play" => Icon::Play,
        "export" => Icon::Export,
        "more" => Icon::More,
        _ => return None,
    })
}

// ---------------------------------------------------------------- the Developer tab

pub fn developer_ribbon(app: &mut SlideApp, ui: &mut Ui) {
    group(ui, |ui| {
        if named(big_button(ui, Icon::Macros, "Visual\nBasic", true), "Visual Basic").on_hover_text("The presentation's code modules").clicked() {
            open_editor(app, None);
        }
        if named(big_button(ui, Icon::Macros, "Macros", true), "Macros").on_hover_text("Run, edit or create a macro").clicked() {
            open_macros(app);
        }
        ui.vertical(|ui| {
            let recording = app.session.recording.is_some();
            let (icon, label) = if recording { (Icon::Stop, "Stop Recording") } else { (Icon::Record, "Record Macro") };
            if named(small_button(ui, icon, label, true, recording), label)
                .on_hover_text("Records what you do as a macro at the end of Module1 (PowerPoint has no macro recorder: this is DeckCraft's)")
                .clicked()
            {
                toggle_recording(app);
            }
            if named(small_button(ui, Icon::Lock, "Macro Security", true, false), "Macro Security")
                .on_hover_text("Which presentations' macros may run")
                .clicked()
            {
                app.addins.security = true;
            }
        });
        if named(big_button(ui, Icon::Plus, "Ribbon\nButtons", true), "Ribbon Buttons")
            .on_hover_text("Buttons on the ribbon that run this presentation's macros")
            .clicked()
        {
            open_buttons(app, None);
        }
    });
    add_ins_group(app, ui);
}

/// Record Macro, or Stop Recording: the recorded macro opens in the editor.
pub fn toggle_recording(app: &mut SlideApp) {
    let recording = app.session.recording.is_some();
    match app.run("developer.recordMacro", json!({"stop": recording})) {
        Ok(r) if recording => {
            let module = r.get("module").and_then(Json::as_str).unwrap_or("Module1").to_string();
            let steps = r.get("steps").and_then(Json::as_u64).unwrap_or(0);
            app.set_status(format!("Recorded {steps} steps into {module}"));
            open_editor(app, Some(&module));
        }
        Ok(_) => app.set_status("Recording a macro: what you do now goes into it. Stop Recording on the Developer tab ends it."),
        Err(_) => {}
    }
}

/// The presentation's ribbon tabs (DeckCraft tab name, groups), read again when it changed.
fn deck(app: &mut SlideApp) -> (DeckTabs, bool) {
    let Some(d) = app.session.active() else { return Default::default() };
    let key = (d.uid, std::sync::Arc::as_ptr(&d.doc) as usize);
    if let Some((uid, ptr, tabs, editable)) = &app.addins.deck
        && (*uid, *ptr) == key
    {
        return (tabs.clone(), *editable);
    }
    let (r, editable) = deckcraft_engine::cmd::developer::current_ribbon(&app.session);
    let mut tabs: Vec<(String, Vec<RibbonGroup>)> = Vec::new();
    for t in r.tabs {
        let name = deckcraft_engine::cmd::developer::tab_name(&t);
        match tabs.iter_mut().find(|(n, _)| n.eq_ignore_ascii_case(&name)) {
            Some((_, groups)) => groups.extend(t.groups),
            None => tabs.push((name, t.groups)),
        }
    }
    let tabs = Rc::new(tabs);
    app.addins.deck = Some((key.0, key.1, tabs.clone(), editable));
    (tabs, editable)
}

/// The presentation's own tabs (not DeckCraft's or an add-in's), for the tab row.
pub fn deck_tabs(app: &mut SlideApp, shown: &[&str]) -> Vec<String> {
    deck(app).0.iter().map(|(n, _)| n.clone()).filter(|n| !shown.iter().any(|s| s.eq_ignore_ascii_case(n)) && n != "Developer").collect()
}

/// The picture names offered for a button (Office's `imageMso` names) and the icon DeckCraft draws.
const PICTURES: &[(&str, Icon)] = &[
    ("MacroPlay", Icon::Macros),
    ("HappyFace", Icon::Star),
    ("FileSave", Icon::Save),
    ("FilePrint", Icon::Print),
    ("Copy", Icon::Copy),
    ("Paste", Icon::Paste),
    ("Cut", Icon::Cut),
    ("SlideShowFromBeginning", Icon::Play),
];

fn picture(name: Option<&str>) -> Icon {
    let Some(name) = name else { return Icon::Macros };
    PICTURES.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, i)| *i).or_else(|| icon_named(name)).unwrap_or(Icon::Macros)
}

/// Draws the presentation's groups on `tab` (after the tab's own groups and the add-ins').
pub fn deck_ribbon_groups(app: &mut SlideApp, ui: &mut Ui, tab: &str) {
    let (tabs, _) = deck(app);
    let mut press = None;
    for g in tabs.iter().filter(|(n, _)| n.eq_ignore_ascii_case(tab)).flat_map(|(_, g)| g) {
        group(ui, |ui| {
            let mut k = 0;
            while let Some(b) = g.buttons.get(k) {
                if b.large {
                    let tip = b.screentip.clone().unwrap_or_else(|| format!("Runs {}", b.on_action));
                    if named(big_button(ui, picture(b.image.as_deref()), &b.label, true), &b.label).on_hover_text(tip).clicked() {
                        press = Some(b.id.clone());
                    }
                    k += 1;
                    continue;
                }
                let column: Vec<_> = g.buttons.iter().skip(k).take_while(|b| !b.large).take(3).collect();
                k += column.len().max(1);
                ui.vertical(|ui| {
                    for b in column {
                        let tip = b.screentip.clone().unwrap_or_else(|| format!("Runs {}", b.on_action));
                        if named(small_button(ui, picture(b.image.as_deref()), &b.label, true, false), &b.label).on_hover_text(tip).clicked() {
                            press = Some(b.id.clone());
                        }
                    }
                });
            }
        });
    }
    if let Some(id) = press {
        let _ = app.run("developer.pressRibbonButton", json!({"id": id}));
    }
}

/// Opens Developer › Ribbon Buttons, with `macro_name` filled in.
pub fn open_buttons(app: &mut SlideApp, macro_name: Option<&str>) {
    let mut w = ButtonsWindow { tab: "Macros".into(), group: "Macros".into(), image: "MacroPlay".into(), ..Default::default() };
    if let Ok(r) = app.session.execute("developer.macros", &json!({})) {
        w.macros = r
            .get("macros")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .filter_map(|m| m.get("name").and_then(Json::as_str))
            .map(str::to_string)
            .collect();
    }
    w.macro_name = macro_name.map(str::to_string).or_else(|| w.macros.first().cloned()).unwrap_or_default();
    app.addins.buttons = Some(w);
}

fn buttons_window(app: &mut SlideApp, ctx: &egui::Context) {
    let Some(mut w) = app.addins.buttons.take() else { return };
    let mut open = true;
    let mut close = false;
    let (tabs, editable) = deck(app);
    let mut remove = None;
    let mut add = None;
    egui::Window::new("Ribbon Buttons").id(egui::Id::new("dev_buttons")).collapsible(false).resizable(false).default_width(480.0).open(&mut open).show(
        ctx,
        |ui| {
            ui.weak("Buttons that run this presentation's macros. They are saved with the presentation, and PowerPoint shows them too.");
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_min_width(440.0);
                egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                    if tabs.iter().all(|(_, g)| g.is_empty()) {
                        ui.weak("No buttons yet.");
                    }
                    for (tab, groups) in tabs.iter() {
                        for g in groups {
                            for b in &g.buttons {
                                ui.horizontal(|ui| {
                                    ui.label(format!("{tab} › {} › {}", g.label, b.label));
                                    ui.weak(format!("runs {}", b.on_action));
                                    if editable && ui.small_button("Remove").clicked() {
                                        remove = Some(b.id.clone());
                                    }
                                });
                            }
                        }
                    }
                });
            });
            if !editable {
                ui.label(
                    RichText::new("This presentation's ribbon came from another program with more than buttons: its buttons run, but DeckCraft doesn't change it.")
                        .weak(),
                );
                if ui.button("Close").clicked() {
                    close = true;
                }
                return;
            }
            ui.add_space(4.0);
            ui.strong("New button");
            egui::Grid::new("dev_button_form").num_columns(2).spacing(vec2(8.0, 4.0)).show(ui, |ui| {
                ui.label("Macro:");
                egui::ComboBox::from_id_salt("dev_button_macro").selected_text(w.macro_name.clone()).width(260.0).show_ui(ui, |ui| {
                    for m in &w.macros {
                        ui.selectable_value(&mut w.macro_name, m.clone(), m);
                    }
                });
                ui.end_row();
                ui.label("");
                ui.add(egui::TextEdit::singleline(&mut w.macro_name).hint_text("or type a name: Module1.Go").desired_width(260.0));
                ui.end_row();
                ui.label("Label:");
                ui.add(egui::TextEdit::singleline(&mut w.label).hint_text("the macro's name").desired_width(260.0));
                ui.end_row();
                ui.label("Tab:");
                egui::ComboBox::from_id_salt("dev_button_tab").selected_text(w.tab.clone()).width(260.0).show_ui(ui, |ui| {
                    for t in ["Macros", "Home", "Insert", "Design", "Transitions", "Animations", "Slide Show", "Review", "View", "Developer"] {
                        ui.selectable_value(&mut w.tab, t.to_string(), t);
                    }
                });
                ui.end_row();
                ui.label("");
                ui.add(egui::TextEdit::singleline(&mut w.tab).hint_text("or a new tab's name").desired_width(260.0));
                ui.end_row();
                ui.label("Group:");
                ui.add(egui::TextEdit::singleline(&mut w.group).desired_width(260.0));
                ui.end_row();
                ui.label("Picture:");
                egui::ComboBox::from_id_salt("dev_button_picture").selected_text(w.image.clone()).width(260.0).show_ui(ui, |ui| {
                    for (name, _) in PICTURES {
                        ui.selectable_value(&mut w.image, name.to_string(), *name);
                    }
                });
                ui.end_row();
                ui.label("");
                ui.checkbox(&mut w.small, "Small button (three to a column)");
                ui.end_row();
            });
            ui.horizontal(|ui| {
                if ui.add_enabled(!w.macro_name.trim().is_empty(), egui::Button::new("Add")).clicked() {
                    add = Some(json!({"macro": w.macro_name.trim(), "label": w.label.trim(), "tab": w.tab.trim(), "group": w.group.trim(), "image": w.image, "large": !w.small}));
                }
                if ui.button("Close").clicked() {
                    close = true;
                }
            });
            if let Some(e) = &w.error {
                ui.label(RichText::new(e).color(ui.visuals().error_fg_color));
            }
        },
    );
    if let Some(params) = add {
        match app.run("developer.addRibbonButton", params) {
            Ok(_) => {
                w.error = None;
                w.label.clear();
            }
            Err(e) => w.error = Some(e),
        }
    }
    if let Some(id) = remove
        && let Err(e) = app.run("developer.removeRibbonButton", json!({"id": id}))
    {
        w.error = Some(e);
    }
    if open && !close {
        app.addins.buttons = Some(w);
    }
}

/// The Add-ins button (Insert and Developer tabs).
pub fn add_ins_group(app: &mut SlideApp, ui: &mut Ui) {
    group(ui, |ui| {
        if named(big_button(ui, Icon::Plus, "Add-ins", true), "Add-ins").on_hover_text("Install, turn on or off, or unload add-ins").clicked() {
            app.addins.window = true;
        }
    });
}

pub fn open_macros(app: &mut SlideApp) {
    let mut w = MacrosWindow::default();
    match app.session.execute("developer.macros", &json!({})) {
        Ok(r) => w.list = r.get("macros").and_then(Json::as_array).cloned().unwrap_or_default(),
        Err(e) => w.error = Some(e.to_string()),
    }
    app.addins.macros = Some(w);
}

pub fn open_editor(app: &mut SlideApp, module: Option<&str>) {
    let mut e = Editor::default();
    match app.session.execute("developer.modules", &json!({})) {
        Ok(r) => {
            e.modules = r
                .get("modules")
                .and_then(Json::as_array)
                .into_iter()
                .flatten()
                .map(|m| {
                    let s = |k: &str| m.get(k).and_then(Json::as_str).unwrap_or("").to_string();
                    (s("name"), s("kind"), s("source"))
                })
                .collect();
        }
        Err(err) => e.error = Some(err.to_string()),
    }
    if let Some(m) = module {
        e.current = e.modules.iter().position(|(n, _, _)| n == m).unwrap_or(0);
    }
    e.text = e.modules.get(e.current).map(|(_, _, s)| s.clone()).unwrap_or_default();
    app.addins.editor = Some(e);
}

/// The bar under the ribbon for a presentation whose macros are disabled (Macro Security: prompt).
pub fn notice_bar(app: &mut SlideApp, ui: &mut Ui) {
    if !developer_visible() || app.session.prefs.macro_security != "prompt" {
        return;
    }
    let Some(d) = app.session.active() else { return };
    if !d.doc.has_macros() || d.macros_enabled || app.addins.notice_done.contains(&d.uid) {
        return;
    }
    let uid = d.uid;
    let t = crate::theme::Tokens::get(ui.ctx());
    egui::Panel::top("macro_notice").exact_size(32.0).frame(egui::Frame::NONE.fill(t.chrome).inner_margin(egui::Margin::symmetric(14, 4))).show(
        ui,
        |ui| {
            ui.horizontal_centered(|ui| {
                ui.label(RichText::new("SECURITY WARNING").strong());
                ui.label("Macros have been disabled.");
                if ui.button("Enable Content").clicked() {
                    let _ = app.run("developer.enableMacros", json!({}));
                    app.addins.notice_done.insert(uid);
                }
                if ui.small_button("✕").on_hover_text("Close").clicked() {
                    app.addins.notice_done.insert(uid);
                }
            });
        },
    );
}

/// Draws the open add-in windows (every frame).
pub fn windows(app: &mut SlideApp, ctx: &egui::Context) {
    poll_install(app, ctx);
    plugin_dialog_window(app, ctx);
    addins_window(app, ctx);
    macros_window(app, ctx);
    editor_window(app, ctx);
    security_window(app, ctx);
    buttons_window(app, ctx);
}

fn macros_window(app: &mut SlideApp, ctx: &egui::Context) {
    let Some(mut w) = app.addins.macros.take() else { return };
    let mut open = true;
    let mut close = false;
    let mut edit: Option<String> = None;
    let mut to_ribbon: Option<String> = None;
    let enabled = deckcraft_engine::cmd::developer::macros_allowed(&app.session);
    egui::Window::new("Macros").id(egui::Id::new("dev_macros")).collapsible(false).resizable(false).default_width(460.0).open(&mut open).show(
        ctx,
        |ui| {
            if !enabled {
                ui.label(
                    RichText::new("Macros are disabled for this presentation. Enable Content in the message bar, or change Macro Security.").weak(),
                );
            }
            ui.horizontal(|ui| {
                ui.label("Macro name:");
                ui.add(egui::TextEdit::singleline(&mut w.new_name).desired_width(260.0));
            });
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_min_size(vec2(420.0, 180.0));
                egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                    if w.list.is_empty() {
                        ui.weak("This presentation has no macros yet: type a name and Create.");
                    }
                    for (i, m) in w.list.iter().enumerate() {
                        let name = m.get("name").and_then(Json::as_str).unwrap_or("");
                        if ui.selectable_label(i == w.selected, name).clicked() {
                            w.selected = i;
                            w.new_name = name.to_string();
                        }
                    }
                });
            });
            let chosen = w.list.get(w.selected).and_then(|m| m.get("name")).and_then(Json::as_str).map(str::to_string);
            ui.horizontal(|ui| {
                if ui.add_enabled(enabled && chosen.is_some(), egui::Button::new("Run")).clicked()
                    && let Some(name) = &chosen
                {
                    match app.run("developer.runMacro", json!({"name": name})) {
                        Ok(_) => close = true,
                        Err(e) => w.error = Some(e),
                    }
                }
                if ui.add_enabled(chosen.is_some(), egui::Button::new("Edit")).clicked()
                    && let Some(m) = w.list.get(w.selected)
                {
                    edit = m.get("module").and_then(Json::as_str).map(str::to_string);
                }
                if ui
                    .add_enabled(chosen.is_some(), egui::Button::new("Add to Ribbon…"))
                    .on_hover_text("A ribbon button that runs this macro")
                    .clicked()
                {
                    to_ribbon = chosen.clone();
                }
                let name = w.new_name.trim().to_string();
                let valid = !name.is_empty()
                    && name.chars().all(|c| c.is_alphanumeric() || c == '_')
                    && !name.chars().next().is_some_and(|c| c.is_ascii_digit());
                if ui
                    .add_enabled(
                        valid && !w.list.iter().any(|m| m.get("procedure").and_then(Json::as_str) == Some(name.as_str())),
                        egui::Button::new("Create"),
                    )
                    .clicked()
                {
                    // A new Sub at the end of Module1, opened in the editor.
                    edit = Some(create_macro(app, &name, &mut w.error));
                }
                if ui.button("Close").clicked() {
                    close = true;
                }
            });
            if let Some(e) = &w.error {
                ui.label(RichText::new(e).color(ui.visuals().error_fg_color));
            }
        },
    );
    if let Some(module) = edit {
        open_editor(app, Some(&module));
        return;
    }
    if let Some(name) = to_ribbon {
        open_buttons(app, Some(&name));
        return;
    }
    if open && !close {
        app.addins.macros = Some(w);
    }
}

/// Adds `Sub name()` to Module1 (made when missing); the module's name.
fn create_macro(app: &mut SlideApp, name: &str, error: &mut Option<String>) -> String {
    let module = "Module1".to_string();
    let existing = app
        .session
        .execute("developer.modules", &json!({}))
        .ok()
        .and_then(|r| r.get("modules").and_then(Json::as_array).cloned())
        .and_then(|m| m.into_iter().find(|x| x.get("name").and_then(Json::as_str) == Some("Module1")))
        .and_then(|m| m.get("source").and_then(Json::as_str).map(str::to_string))
        .map(|t| format!("{}\r\n\r\n", t.trim_end()))
        .unwrap_or_default();
    let source = format!("{existing}Sub {name}()\r\n\r\nEnd Sub\r\n");
    if let Err(e) = app.run("developer.saveModule", json!({"name": module, "source": source})) {
        *error = Some(e);
    }
    module
}

fn editor_window(app: &mut SlideApp, ctx: &egui::Context) {
    let Some(mut e) = app.addins.editor.take() else { return };
    let mut open = true;
    let title = format!("Visual Basic — {}", app.session.active().map(|d| d.title()).unwrap_or_default());
    egui::Window::new(title)
        .id(egui::Id::new("dev_editor"))
        .collapsible(false)
        .resizable(true)
        .default_size(vec2(820.0, 520.0))
        .open(&mut open)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(e.dirty, egui::Button::new("Save"))
                    .on_hover_text("Saves the module into the presentation (save it as .pptm to keep it)")
                    .clicked()
                {
                    let name = e.modules.get(e.current).map(|(n, _, _)| n.clone()).unwrap_or_else(|| "Module1".into());
                    match app.run("developer.saveModule", json!({"name": name, "source": e.text})) {
                        Ok(_) => {
                            if let Some(m) = e.modules.get_mut(e.current) {
                                m.2 = e.text.clone();
                            }
                            e.dirty = false;
                            e.error = None;
                        }
                        Err(err) => e.error = Some(err),
                    }
                }
                if ui.button("Run Macro…").clicked() {
                    open_macros(app);
                }
                ui.separator();
                ui.add(egui::TextEdit::singleline(&mut e.new_module).hint_text("Module2").desired_width(120.0));
                let valid = !e.new_module.trim().is_empty() && !e.modules.iter().any(|(n, _, _)| n.eq_ignore_ascii_case(e.new_module.trim()));
                if ui.add_enabled(valid, egui::Button::new("Insert Module")).clicked() {
                    let name = e.new_module.trim().to_string();
                    let source = format!("Attribute VB_Name = \"{name}\"\r\n");
                    match app.run("developer.saveModule", json!({"name": name, "source": source})) {
                        Ok(_) => {
                            e.modules.push((name, "standard".into(), source.clone()));
                            e.current = e.modules.len() - 1;
                            e.text = source;
                            e.dirty = false;
                            e.new_module.clear();
                        }
                        Err(err) => e.error = Some(err),
                    }
                }
                let standard = e.modules.get(e.current).is_some_and(|(_, k, _)| k == "standard");
                if ui.add_enabled(standard, egui::Button::new("Remove Module")).clicked()
                    && let Some((name, _, _)) = e.modules.get(e.current).cloned()
                {
                    match app.run("developer.deleteModule", json!({"name": name})) {
                        Ok(_) => {
                            e.modules.remove(e.current);
                            e.current = 0;
                            e.text = e.modules.first().map(|(_, _, s)| s.clone()).unwrap_or_default();
                            e.dirty = false;
                        }
                        Err(err) => e.error = Some(err),
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.weak(language());
                });
            });
            if let Some(err) = &e.error {
                ui.label(RichText::new(err).color(ui.visuals().error_fg_color));
            }
            ui.separator();
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(170.0);
                    ui.weak("Modules");
                    if e.modules.is_empty() {
                        ui.weak("No code yet: Insert Module.");
                    }
                    let mut pick = None;
                    for (i, (name, kind, _)) in e.modules.iter().enumerate() {
                        let label = if kind == "standard" { name.clone() } else { format!("{name} ({kind})") };
                        if ui.selectable_label(i == e.current, label).clicked() {
                            pick = Some(i);
                        }
                    }
                    if let Some(i) = pick
                        && i != e.current
                    {
                        if let Some(m) = e.modules.get_mut(e.current)
                            && e.dirty
                        {
                            // Unsaved text stays with its module until saved or closed.
                            m.2 = e.text.clone();
                        }
                        e.current = i;
                        e.text = e.modules.get(i).map(|(_, _, s)| s.clone()).unwrap_or_default();
                    }
                });
                ui.separator();
                egui::ScrollArea::both().id_salt("dev_code").show(ui, |ui| {
                    let resp = ui.add(
                        egui::TextEdit::multiline(&mut e.text)
                            .code_editor()
                            .desired_width(f32::INFINITY)
                            .desired_rows(24)
                            .lock_focus(true)
                            .interactive(!e.modules.is_empty()),
                    );
                    if resp.changed() {
                        e.dirty = true;
                    }
                });
            });
        });
    if open {
        app.addins.editor = Some(e);
    }
}

fn security_window(app: &mut SlideApp, ctx: &egui::Context) {
    if !app.addins.security {
        return;
    }
    let mut open = true;
    let level = app.session.prefs.macro_security.clone();
    egui::Window::new("Macro Security").id(egui::Id::new("dev_security")).collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
        for (value, text) in [
            ("disable", "Disable all macros"),
            ("prompt", "Disable macros with notification (enable them per presentation)"),
            ("enable", "Enable all macros (not recommended: a macro is a program)"),
        ] {
            if ui.radio(level == value, text).clicked() {
                let _ = app.run("developer.macroSecurity", json!({"level": value}));
            }
        }
        ui.add_space(6.0);
        if ui.button("Close").clicked() {
            app.addins.security = false;
        }
    });
    if !open {
        app.addins.security = false;
    }
}

// ---------------------------------------------------------------- the Add-ins window

/// Runs `job` on a thread; its outcome is picked up by [`poll_install`].
fn spawn(app: &mut SlideApp, job: impl FnOnce() -> Result<Done, String> + Send + 'static) {
    let st = &mut app.addins.install;
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new().name("addin-install".into()).spawn(move || {
            let _ = tx.send(job());
        });
        match spawned {
            Ok(_) => {
                st.pending = Some(rx);
                st.outcome = None;
            }
            Err(e) => st.outcome = Some(Err(format!("Couldn't start the download: {e}"))),
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        drop(job);
        st.outcome = Some(Err("Add-ins need the desktop app.".into()));
    }
}

/// Starts downloading the add-in at `url` (after the user confirmed).
fn start_install(app: &mut SlideApp, url: &str) {
    let url = url.trim().to_string();
    app.addins.install.confirming = None;
    let Some(root) = plugin::install::install_dir() else {
        app.addins.install.outcome = Some(Err("There's no add-ins folder to install into.".into()));
        return;
    };
    spawn(app, move || {
        let fetched = plugin::install::fetch(&url, None)?;
        let conflicts = plugin::conflicts(&fetched.manifest);
        if !conflicts.is_empty() {
            return Ok(Done::Conflicts(Box::new(fetched), conflicts.into_iter().map(|c| c.message).collect()));
        }
        fetched.unpack(&root).map(Done::Unpacked)
    });
}

/// Installs the ticked add-ins of the offer: for each, the newest release in its range.
pub fn install_offer(app: &mut SlideApp) {
    let Some(offer) = app.addins.install.offer.take() else { return };
    let Some(root) = plugin::install::install_dir() else {
        app.addins.install.outcome = Some(Err("There's no add-ins folder to install into.".into()));
        return;
    };
    let wanted: Vec<plugin::requires::Missing> = offer.items.into_iter().filter(|(_, on)| *on).map(|(m, _)| m).collect();
    if wanted.is_empty() {
        return;
    }
    spawn(app, move || {
        let done = wanted
            .into_iter()
            .map(|m| {
                let r = (|| -> Result<(std::path::PathBuf, Option<String>), String> {
                    let github = m.github.as_deref().ok_or_else(|| format!("{} has no GitHub repository to install from", m.name))?;
                    let range = plugin::version::Range::parse(&m.version)?;
                    let fetched = plugin::install::fetch(github, Some(&range))?;
                    if fetched.manifest.id != m.id {
                        return Err(format!("github.com/{github} holds {}, not {}", fetched.manifest.id, m.id));
                    }
                    let fits = plugin::version::Version::parse(&fetched.manifest.version).map(|v| range.matches(&v)).unwrap_or(range.is_any());
                    if !fits {
                        return Err(format!("{} {} from github.com/{github} isn't in {range}", m.name, fetched.manifest.version));
                    }
                    if let Some(c) = plugin::conflicts(&fetched.manifest).first() {
                        return Err(c.message.clone());
                    }
                    Ok((fetched.unpack(&root)?, fetched.release.clone()))
                })();
                (m.id, r)
            })
            .collect();
        Ok(Done::Requirements(done))
    });
}

/// Offers the missing or out-of-range add-ins `id` needs that name a GitHub repository; does
/// nothing when there are none.
pub fn offer_missing(app: &mut SlideApp, id: &str) {
    let items: Vec<(plugin::requires::Missing, bool)> =
        plugin::missing(id).into_iter().filter(|m| m.github.is_some()).map(|m| (m.clone(), !m.optional)).collect();
    if items.is_empty() {
        return;
    }
    let name = plugin::get(id).map(|p| p.manifest.name.clone()).unwrap_or_else(|| id.to_string());
    app.addins.install.offer = Some(Offer { plugin: id.to_string(), name, items });
}

/// Whether `id` is blocked by required add-ins that Install Missing can install.
pub fn can_install_missing(id: &str) -> bool {
    plugin::missing(id).iter().any(|m| !m.optional && m.github.is_some())
}

/// Loads an add-in unpacked by a download thread (its version was checked, or the user agreed).
fn load_unpacked(app: &mut SlideApp, dir: &std::path::Path) -> Result<Json, String> {
    app.run("plugins.load", json!({"path": dir, "force": true}))
}

/// Picks up a finished download and loads the add-in (every frame; nothing to do most of them).
fn poll_install(app: &mut SlideApp, ctx: &egui::Context) {
    let Some(rx) = &app.addins.install.pending else { return };
    let result = match rx.try_recv() {
        Ok(r) => r,
        Err(std::sync::mpsc::TryRecvError::Empty) => {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
            return;
        }
        Err(std::sync::mpsc::TryRecvError::Disconnected) => Err("the download stopped unexpectedly".into()),
    };
    app.addins.install.pending = None;
    match result {
        Err(e) => app.addins.install.outcome = Some(Err(e)),
        Ok(Done::Conflicts(fetched, why)) => {
            let text = format!("{}. Install {} {} anyway?", why.join("; "), fetched.manifest.name, fetched.manifest.version);
            app.addins.install.confirm_update = Some((*fetched, text));
        }
        Ok(Done::Unpacked(dir)) => {
            let outcome = load_unpacked(app, &dir).map(|info| {
                let name = info.get("name").and_then(Json::as_str).unwrap_or("The add-in");
                let version = info.get("version").and_then(Json::as_str).unwrap_or("");
                (info.get("id").and_then(Json::as_str).unwrap_or("").to_string(), format!("Installed {name} {version}").trim_end().to_string())
            });
            if let Ok((id, _)) = &outcome {
                app.addins.install.url.clear();
                let id = id.clone();
                offer_missing(app, &id);
            }
            app.addins.install.outcome = Some(outcome.map(|(_, m)| m));
        }
        Ok(Done::Requirements(list)) => {
            let (mut done, mut failed, mut ids) = (Vec::new(), Vec::new(), Vec::new());
            for (id, r) in list {
                match r.and_then(|(dir, release)| load_unpacked(app, &dir).map(|info| (info, release))) {
                    Ok((info, release)) => {
                        let name = info.get("name").and_then(Json::as_str).unwrap_or(&id).to_string();
                        let version = info.get("version").and_then(Json::as_str).unwrap_or("").to_string();
                        done.push(match release {
                            Some(tag) => format!("{name} {version} (release {tag})"),
                            None => format!("{name} {version}").trim_end().to_string(),
                        });
                        ids.push(id);
                    }
                    Err(e) => failed.push(e),
                }
            }
            // What the new ones need in turn.
            for id in ids {
                if app.addins.install.offer.is_none() {
                    offer_missing(app, &id);
                }
            }
            app.addins.install.outcome = Some(if failed.is_empty() {
                Ok(format!("Installed {}", done.join(", ")))
            } else if done.is_empty() {
                Err(failed.join("; "))
            } else {
                Err(format!("Installed {}; {}", done.join(", "), failed.join("; ")))
            });
        }
    }
}

fn addins_window(app: &mut SlideApp, ctx: &egui::Context) {
    if !app.addins.window {
        return;
    }
    let mut open = true;
    egui::Window::new("Add-ins").id(egui::Id::new("addins")).collapsible(false).resizable(true).default_width(560.0).open(&mut open).show(
        ctx,
        |ui| {
            ui.weak("Add-ins are programs that add commands, panes and macros. They use the same manifest as the other craft apps.");
            install_row(app, ui);
            ui.horizontal(|ui| {
                ui.label("Load from a folder:");
                ui.add(egui::TextEdit::singleline(&mut app.addins.load_path).hint_text("/path/to/add-in").desired_width(300.0));
                if ui.add_enabled(!app.addins.load_path.trim().is_empty(), egui::Button::new("Load")).clicked() {
                    let path = app.addins.load_path.trim().to_string();
                    if app.run("plugins.load", json!({"path": path})).is_ok() {
                        app.addins.load_path.clear();
                    }
                }
            });
            ui.separator();
            let all = plugin::list();
            if all.is_empty() {
                ui.weak("No add-ins are loaded.");
            }
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                for p in all {
                    card(app, ui, &p);
                }
            });
        },
    );
    if !open {
        app.addins.window = false;
    }
}

/// "Install from GitHub": an address field, a confirmation naming the repository, progress and
/// the outcome; then the questions an install can raise (an update the add-ins that need it
/// don't accept; the add-ins it needs).
fn install_row(app: &mut SlideApp, ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.label("Install from GitHub:");
        let busy = app.addins.install.busy();
        let field = egui::TextEdit::singleline(&mut app.addins.install.url).hint_text("https://github.com/owner/repository").desired_width(300.0);
        let resp = ui.add_enabled(!busy, field);
        let go = ui.add_enabled(!busy && !app.addins.install.url.trim().is_empty(), egui::Button::new("Install")).clicked()
            || (resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !app.addins.install.url.trim().is_empty());
        if go {
            match plugin::install::parse_github_url(&app.addins.install.url) {
                Ok(r) => app.addins.install.confirming = Some(format!("{}/{}", r.owner, r.name)),
                Err(e) => app.addins.install.outcome = Some(Err(e)),
            }
        }
        if busy {
            ui.spinner();
            ui.label("Installing…");
        }
    });
    if let Some(repo) = app.addins.install.confirming.clone() {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.label(format!(
                "An add-in is a program that runs with your permissions. Install it only if you trust its author. Install and run github.com/{repo}?"
            ));
            ui.horizontal(|ui| {
                if ui.button("Install").clicked() {
                    let url = app.addins.install.url.clone();
                    start_install(app, &url);
                }
                if ui.button("Cancel").clicked() {
                    app.addins.install.confirming = None;
                }
            });
        });
    }
    if let Some((_, text)) = &app.addins.install.confirm_update {
        let text = text.clone();
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.add(egui::Label::new(text).wrap());
            ui.weak("The add-ins that need the installed version are blocked until it suits them again.");
            ui.horizontal(|ui| {
                if ui.button("Install Anyway").clicked()
                    && let Some((fetched, _)) = app.addins.install.confirm_update.take()
                {
                    match plugin::install::install_dir() {
                        Some(root) => spawn(app, move || fetched.unpack(&root).map(Done::Unpacked)),
                        None => app.addins.install.outcome = Some(Err("There's no add-ins folder to install into.".into())),
                    }
                }
                if ui.button("Cancel").clicked() {
                    app.addins.install.confirm_update = None;
                }
            });
        });
    }
    offer_frame(app, ui);
    match &app.addins.install.outcome {
        Some(Ok(m)) => {
            ui.label(RichText::new(m).color(crate::theme::Tokens::get(ui.ctx()).accent));
        }
        Some(Err(e)) => {
            ui.label(RichText::new(e).color(ui.visuals().error_fg_color));
        }
        None => {}
    }
}

/// "Reports needs craft-vba ^0.1.4 (Runs the report macros). Install it too?", one tick box per
/// add-in (optional ones unticked), and Install Selected / Not Now.
fn offer_frame(app: &mut SlideApp, ui: &mut Ui) {
    let busy = app.addins.install.busy();
    let Some(offer) = app.addins.install.offer.as_mut() else { return };
    let mut install = false;
    let mut dismiss = false;
    egui::Frame::group(ui.style()).show(ui, |ui| {
        for (m, on) in offer.items.iter_mut() {
            let why = if m.why.trim().is_empty() { String::new() } else { format!(" ({})", m.why.trim()) };
            let what = match &m.installed {
                Some(v) => format!("{} {}{why}; version {v} is installed", m.name, m.version),
                None => format!("{} {}{why}", m.name, m.version),
            };
            let verb = if m.optional { "can use" } else { "needs" };
            ui.checkbox(on, format!("{} {verb} {what}. Install it too?", offer.name));
            if let Some(g) = &m.github {
                ui.indent(("offer", &m.id), |ui| {
                    ui.weak(format!("github.com/{g} · the newest release in range"));
                });
            }
        }
        ui.weak("Add-ins are programs that run with your permissions. Install them only if you trust their authors.");
        ui.horizontal(|ui| {
            install = ui.add_enabled(!busy && offer.items.iter().any(|(_, on)| *on), egui::Button::new("Install Selected")).clicked();
            dismiss = ui.button("Not Now").clicked();
        });
    });
    if install {
        install_offer(app);
    } else if dismiss {
        app.addins.install.offer = None;
    }
}

/// Turns `id` off (or unloads it), and first the add-ins that need it when the user agreed.
fn turn_off(app: &mut SlideApp, id: &str, unload: bool, dependents: &[String]) {
    // The ones furthest up the chain first.
    for d in dependents.iter().rev() {
        let _ = app.run("plugins.setEnabled", json!({"id": d, "enabled": false}));
    }
    if unload {
        let _ = app.run("plugins.unload", json!({"id": id}));
    } else {
        let _ = app.run("plugins.setEnabled", json!({"id": id, "enabled": false}));
    }
}

/// Deactivate or Unload: asks first when other add-ins need it.
fn ask_turn_off(app: &mut SlideApp, id: &str, unload: bool) {
    let dependents = plugin::dependents(id);
    if dependents.is_empty() {
        turn_off(app, id, unload, &[]);
    } else {
        app.addins.install.confirm_off = Some(ConfirmOff { id: id.to_string(), unload, dependents });
    }
}

fn names(ids: &[String]) -> Vec<String> {
    ids.iter().map(|d| plugin::get(d).map(|p| p.manifest.name.clone()).unwrap_or_else(|| d.clone())).collect()
}

/// One add-in: name and version, status, author, id and where it came from, the description,
/// and Documentation / Activate or Deactivate / Unload. A blocked add-in says why, with Install
/// Missing when that can help; turning off one that others need asks first.
fn card(app: &mut SlideApp, ui: &mut Ui, p: &plugin::Plugin) {
    let m = &p.manifest;
    let on = p.enabled();
    egui::Frame::group(ui.style()).inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(format!("{} {}", m.name, m.version).trim_end().to_string()).strong());
                let status = p.status();
                let mut about = vec![match status.as_str() {
                    "off" => "Inactive".to_string(),
                    s if s.starts_with("blocked") => "Blocked".to_string(),
                    s if s.starts_with("failed") => s.to_string(),
                    _ => "Active".to_string(),
                }];
                if m.office.is_some() {
                    about.push("Office add-in".into());
                } else if let Some(l) = &m.macros {
                    about.push(format!("runs {} macros", l.language));
                }
                if !m.author.is_empty() {
                    about.push(m.author.clone());
                }
                about.push(m.id.clone());
                if let Some(repo) = p.source.as_ref().and_then(|s| s.get("repository")).and_then(Json::as_str) {
                    about.push(repo.trim_start_matches("https://").to_string());
                }
                ui.weak(about.join(" · "));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Unload").on_hover_text("Until the next start").clicked() {
                    ask_turn_off(app, &m.id, true);
                }
                if ui.button(if on { "Deactivate" } else { "Activate" }).clicked() {
                    if on {
                        ask_turn_off(app, &m.id, false);
                    } else {
                        let _ = app.run("plugins.setEnabled", json!({"id": m.id, "enabled": true}));
                    }
                }
                if on
                    && p.is_blocked()
                    && can_install_missing(&m.id)
                    && ui.add_enabled(!app.addins.install.busy(), egui::Button::new("Install Missing")).clicked()
                {
                    offer_missing(app, &m.id);
                }
                if !m.homepage.trim().is_empty() && ui.link("Documentation").on_hover_text(m.homepage.trim()).clicked() {
                    let _ = app.run("plugins.openHomepage", json!({"id": m.id}));
                }
            });
        });
        if on {
            for b in p.blocked_by() {
                ui.add(egui::Label::new(RichText::new(&b.message).color(ui.visuals().warn_fg_color)).wrap());
            }
        }
        if !m.description.is_empty() {
            ui.add(egui::Label::new(&m.description).wrap());
        }
        let asking = app.addins.install.confirm_off.as_ref().filter(|c| c.id == m.id).map(|c| (c.unload, c.dependents.clone()));
        if let Some((unloading, dependents)) = asking {
            let who = names(&dependents).join(", ");
            let (verb, question, label) =
                if dependents.len() == 1 { ("needs", "Turn both off?", "Turn Both Off") } else { ("need", "Turn them all off?", "Turn All Off") };
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.add(egui::Label::new(format!("{who} {verb} {}. {question}", m.name)).wrap());
                ui.horizontal(|ui| {
                    if ui.button(label).clicked() {
                        app.addins.install.confirm_off = None;
                        turn_off(app, &m.id, unloading, &dependents);
                    }
                    if ui.button("Cancel").clicked() {
                        app.addins.install.confirm_off = None;
                    }
                });
            });
        }
    });
}

#[cfg(test)]
#[path = "addins_tests.rs"]
mod tests;
