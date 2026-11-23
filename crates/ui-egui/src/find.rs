//! Find (⌘F): a small bar over the top of the canvas. Typing searches every slide's text
//! (`text.find`); Enter goes to the next place it appears and ⇧Enter the one before, selecting the
//! words on their slide (`edit.showMatch`); Aa matches case, the other toggle whole words; Esc
//! closes it.

use egui::{Align2, CornerRadius, Margin, Stroke, vec2};
use serde_json::{Value, json};

use crate::SlideApp;
use crate::icons::{self, Icon};
use crate::theme::{self, Tokens};

#[derive(Default)]
pub struct FindBar {
    pub query: String,
    /// The match shown, an index into `matches`.
    pub index: usize,
    pub case: bool,
    pub whole: bool,
    /// Give the field the keyboard next frame.
    focus: bool,
    /// What `matches` was found for: (presentation, revision, query, case, whole).
    key: Option<(u64, u64, String, bool, bool)>,
    pub matches: Vec<Value>,
}

/// Open the bar (or give it the keyboard again), starting from the text selected on the slide.
pub fn open(app: &mut SlideApp) {
    let selected = selected_words(app);
    let bar = app.find.get_or_insert_with(FindBar::default);
    if let Some(s) = selected {
        bar.query = s;
    }
    bar.focus = true;
}

/// The words selected in the text being edited, when they're within one paragraph.
fn selected_words(app: &SlideApp) -> Option<String> {
    let st = app.session.active()?;
    let t = st.selection.text.as_ref().filter(|t| !t.notes && t.cell.is_none())?;
    let (a, c) = (t.anchor, t.caret);
    if a.0 != c.0 || a.1 == c.1 {
        return None;
    }
    let para = st.shape(t.shape)?.text.as_ref()?.paragraphs.get(a.0)?.text();
    let (lo, hi) = (a.1.min(c.1), a.1.max(c.1));
    Some(para.chars().skip(lo).take(hi - lo).collect())
}

fn refresh(app: &mut SlideApp) {
    let Some(st) = app.session.active() else { return };
    let (uid, rev) = (st.uid, st.revision);
    let Some(bar) = app.find.as_ref() else { return };
    let key = (uid, rev, bar.query.clone(), bar.case, bar.whole);
    if bar.key.as_ref() == Some(&key) {
        return;
    }
    // (An empty query finds nothing: `text.find` refuses it.)
    let found = app.session.execute("text.find", &json!({"text": bar.query, "matchCase": bar.case, "wholeWords": bar.whole})).ok();
    if let Some(bar) = app.find.as_mut() {
        bar.matches = found.and_then(|v| v.as_array().cloned()).unwrap_or_default();
        bar.index = bar.index.min(bar.matches.len().saturating_sub(1));
        bar.key = Some(key);
    }
}

/// Go to match `k` (wrapping around).
fn go(app: &mut SlideApp, k: isize) {
    let Some(bar) = app.find.as_mut() else { return };
    let n = bar.matches.len() as isize;
    if n == 0 {
        return;
    }
    bar.index = k.rem_euclid(n) as usize;
    let Some(m) = bar.matches.get(bar.index).cloned() else { return };
    // Selecting the words changes the selection, not the document: the matches stay.
    let _ = app.session.execute("edit.showMatch", &m);
    app.slides_focus = false;
}

pub fn show(app: &mut SlideApp, ctx: &egui::Context) {
    if app.find.is_none() || app.show.is_some() {
        return;
    }
    refresh(app);
    let t = Tokens::get(ctx);
    let mut close = false;
    let mut step: Option<isize> = None;
    let mut changed = false;
    let top = if app.ui.shell == crate::Shell::Canvas { 112.0 } else { 160.0 };
    egui::Area::new(egui::Id::unique("find-bar")).anchor(Align2::CENTER_TOP, vec2(0.0, top)).order(egui::Order::Foreground).show(ctx, |ui| {
        egui::Frame::NONE
            .fill(t.card)
            .stroke(Stroke::new(1.0, t.card_border))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::symmetric(10, 8))
            .shadow(egui::epaint::Shadow { offset: [0, 4], blur: 14, spread: 0, color: t.shadow })
            .show(ui, |ui| {
                let Some(bar) = app.find.as_mut() else { return };
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let (sr, _) = ui.allocate_exact_size(vec2(16.0, 16.0), egui::Sense::hover());
                    icons::paint_mono(ui.painter(), sr, Icon::Search, t.text_dim, false);
                    let before = bar.query.clone();
                    let r = crate::form::text(ui, &mut bar.query, 240.0, "Find in presentation");
                    if bar.focus {
                        r.request_focus();
                        bar.focus = false;
                    }
                    changed = bar.query != before;
                    let (enter, shift, esc) = ui.input(|i| (i.key_pressed(egui::Key::Enter), i.modifiers.shift, i.key_pressed(egui::Key::Escape)));
                    if r.lost_focus() && enter {
                        step = Some(if shift { -1 } else { 1 });
                        r.request_focus();
                    }
                    if esc {
                        close = true;
                    }
                    let n = bar.matches.len();
                    let count = match (bar.query.is_empty(), n) {
                        (true, _) => String::new(),
                        (false, 0) => "No results".to_string(),
                        (false, n) => format!("{} of {n}", bar.index + 1),
                    };
                    ui.add_sized(vec2(72.0, 20.0), egui::Label::new(egui::RichText::new(count).font(theme::font(12.0)).color(t.text_dim)));
                    let small = |ui: &mut egui::Ui, label: &str, tip: &str, on: bool| {
                        let b = egui::Button::new(egui::RichText::new(label).font(theme::font(12.0))).selected(on).min_size(vec2(26.0, 24.0));
                        ui.add(b).on_hover_text(tip)
                    };
                    if small(ui, "Aa", "Match case", bar.case).clicked() {
                        bar.case = !bar.case;
                        changed = true;
                    }
                    if small(ui, "W", "Whole words only", bar.whole).clicked() {
                        bar.whole = !bar.whole;
                        changed = true;
                    }
                    if ui.add_enabled(n > 0, egui::Button::new("‹").min_size(vec2(24.0, 24.0))).on_hover_text("Previous (⇧↩)").clicked() {
                        step = Some(-1);
                    }
                    if ui.add_enabled(n > 0, egui::Button::new("›").min_size(vec2(24.0, 24.0))).on_hover_text("Next (↩)").clicked() {
                        step = Some(1);
                    }
                    let (cr, cresp) = ui.allocate_exact_size(vec2(22.0, 22.0), egui::Sense::click());
                    if cresp.hovered() {
                        ui.painter().rect_filled(cr, CornerRadius::same(4), t.hover);
                    }
                    icons::paint_mono(ui.painter(), cr.shrink(5.0), Icon::Close, t.text_dim, false);
                    cresp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Close find"));
                    if cresp.on_hover_text("Close (Esc)").clicked() {
                        close = true;
                    }
                });
            });
    });
    if close {
        app.find = None;
        return;
    }
    if changed {
        // A new search starts at its first match.
        refresh(app);
        if let Some(bar) = app.find.as_mut() {
            bar.index = 0;
        }
        go(app, 0);
    } else if let Some(d) = step {
        let i = app.find.as_ref().map(|b| b.index as isize).unwrap_or(0);
        go(app, i + d);
    }
}
