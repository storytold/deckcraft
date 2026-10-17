//! The document chat in the window (craft-collab `docs/chat.md`): the Chat task pane (each
//! message with its author's circle and colour, name, time, replies quoted, "edited" and "Message
//! deleted", links and @mentions; a box that sends with Enter, Shift+Enter for a new line; a menu
//! with Export, Clear chat… and the note that Office drops the chat), the title bar's chat button
//! with the unread count beside the people in a session (outside one too, when the presentation
//! has a chat), and the button flashing when someone mentions you.
//!
//! The engine holds the chat and does the work (`chat.*` commands); this only shows it.

use std::collections::{HashMap, HashSet};

use deckcraft_model::chat::{Chat, ChatAuthor, ChatMessage};
use egui::{Align2, Color32, CornerRadius, Key, RichText, Sense, Stroke, Ui, pos2, vec2};
use serde_json::json;

use crate::SlideApp;
use crate::icons::{self, Icon};
use crate::theme::{self, Tokens};

/// How long a mention flashes the chat button.
const FLASH_MS: f64 = 4_000.0;
/// The composer's id (its focus says whether the person is typing in the chat).
const BOX_ID: &str = "chat_box";

/// What the chat pane and button keep between frames.
#[derive(Default)]
pub struct State {
    /// What's typed in the box.
    pub draft: String,
    /// The message the draft answers.
    pub reply_to: Option<String>,
    /// The message being edited (the box holds its text).
    pub editing: Option<String>,
    /// Clear chat… asked: the confirmation is showing.
    pub confirm_clear: bool,
    /// Per presentation (uid): the last message seen, `None` before any.
    seen: HashMap<u64, Option<String>>,
    /// Messages that mentioned you and already flashed the button.
    flashed: HashSet<String>,
    /// The button flashes until then.
    flash_until: f64,
    /// `chat.typing` as last sent.
    typing: bool,
    /// Give the box the focus when it's next drawn.
    focus_box: bool,
}

/// This person's id: the room's user in a session, else the app's own.
fn my_id(app: &SlideApp) -> String {
    match app.session.active_room() {
        Some(r) => r.user.id.clone(),
        None => app.session.prefs.collab_user_id.clone(),
    }
}

fn my_name(app: &SlideApp) -> String {
    match app.session.active_room() {
        Some(r) => r.user.name.clone(),
        None => app.session.prefs.author.trim().to_string(),
    }
}

fn chat(app: &SlideApp) -> Option<&Chat> {
    app.session.active().map(|d| &*d.doc.chat)
}

fn pane_open(app: &SlideApp) -> bool {
    app.ui.pane.as_deref() == Some("chat")
}

/// Others' messages after the last one seen (deleted ones aside).
pub fn unread(app: &SlideApp) -> Vec<&ChatMessage> {
    let (Some(d), Some(c)) = (app.session.active(), chat(app)) else { return Vec::new() };
    let me = my_id(app);
    let from = match app.chat.seen.get(&d.uid) {
        // Not looked at yet: what's there now counts as seen once `tick` runs.
        None => c.messages.len(),
        Some(None) => 0,
        Some(Some(id)) => c.index_of(id).map(|i| i + 1).unwrap_or(0),
    };
    c.messages.iter().skip(from).filter(|m| m.author.id != me && !m.deleted).collect()
}

/// The names `@` can mention: the people in the session and the chat's authors.
fn names(app: &SlideApp) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Some(r) = app.session.active_room() {
        out.push(r.user.name.clone());
        out.extend(r.peers().into_iter().map(|p| p.user.name));
        out.extend(r.listed.iter().map(|u| u.name.clone()));
    }
    if let Some(c) = chat(app) {
        out.extend(c.messages.iter().map(|m| m.author.name.clone()));
    }
    let me = my_name(app);
    if !me.is_empty() {
        out.push(me);
    }
    // First names too ("@Ann" names Ann Lee).
    let firsts: Vec<String> = out.iter().filter_map(|n| n.split_whitespace().next().map(str::to_string)).collect();
    out.extend(firsts);
    out.retain(|n| !n.trim().is_empty());
    out.sort_by_key(|n| std::cmp::Reverse(n.chars().count()));
    out.dedup();
    out
}

/// A piece of a message's text.
#[derive(Debug, PartialEq)]
pub enum Seg<'a> {
    Text(&'a str),
    Link(&'a str),
    Mention(&'a str),
}

fn boundary(prev: Option<char>) -> bool {
    prev.is_none_or(|c| c.is_whitespace() || "([{\"'".contains(c))
}

/// Splits a line into plain text, links (`http://`, `https://`, `www.`) and mentions of `names`
/// (`@Name`, case-insensitive, ending where the name does).
pub fn segments<'a>(line: &'a str, names: &[String]) -> Vec<Seg<'a>> {
    let mut out = Vec::new();
    let mut plain_from = 0;
    let mut i = 0;
    let mut prev: Option<char> = None;
    while i < line.len() {
        let rest = line.get(i..).unwrap_or("");
        let Some(c) = rest.chars().next() else { break };
        let mut found: Option<(usize, bool)> = None;
        if boundary(prev) {
            let lower = rest.get(..8.min(rest.len())).unwrap_or("").to_ascii_lowercase();
            if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("www.") {
                let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
                let link = rest.get(..end).unwrap_or("").trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '"', '\'']);
                if link.len() > 4 {
                    found = Some((link.len(), true));
                }
            } else if c == '@' {
                let after = rest.get(1..).unwrap_or("");
                for n in names {
                    let len: usize = after.chars().take(n.chars().count()).map(char::len_utf8).sum();
                    let cand = after.get(..len).unwrap_or("");
                    let next = after.get(len..).and_then(|t| t.chars().next());
                    if !cand.is_empty() && cand.to_lowercase() == n.to_lowercase() && next.is_none_or(|ch| !ch.is_alphanumeric()) {
                        found = Some((1 + len, false));
                        break;
                    }
                }
            }
        }
        match found {
            Some((len, link)) => {
                if plain_from < i {
                    out.push(Seg::Text(line.get(plain_from..i).unwrap_or("")));
                }
                let piece = line.get(i..i + len).unwrap_or("");
                out.push(if link { Seg::Link(piece) } else { Seg::Mention(piece) });
                i += len;
                plain_from = i;
                prev = piece.chars().last();
            }
            None => {
                i += c.len_utf8();
                prev = Some(c);
            }
        }
    }
    if plain_from < line.len() {
        out.push(Seg::Text(line.get(plain_from..).unwrap_or("")));
    }
    out
}

/// Whether `text` mentions `name` (or its first word).
pub fn mentions(text: &str, name: &str) -> bool {
    let name = name.trim();
    if name.is_empty() {
        return false;
    }
    let mut ns = vec![name.to_string()];
    if let Some(first) = name.split_whitespace().next()
        && first != name
    {
        ns.push(first.to_string());
    }
    text.lines().any(|l| segments(l, &ns).iter().any(|s| matches!(s, Seg::Mention(_))))
}

/// Days since 1970-01-01 of a calendar date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The local time zone's offset from UTC in seconds, as the clock says now (0 where there is
/// none: the browser).
fn local_offset(now_ms: f64) -> i64 {
    let Some(l) = deckcraft_text::datetime::DateTime::now_local() else { return 0 };
    let local = days_from_civil(i64::from(l.year), i64::from(l.month), i64::from(l.day)) * 86_400
        + i64::from(l.hour) * 3600
        + i64::from(l.minute) * 60
        + i64::from(l.second);
    let off = local - (now_ms / 1000.0) as i64;
    // Whole quarter hours: the seconds between the two readings don't count.
    (off as f64 / 900.0).round() as i64 * 900
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// A message's time: relative today ("just now", "5 min ago", "3 h ago"), else the local date and
/// time ("Oct 14, 15:32").
pub fn time_text(at: i64, now_ms: f64, offset: i64) -> String {
    let now = now_ms as i64;
    let local = |ms: i64| ms.div_euclid(1000) + offset;
    let (day, today) = (local(at).div_euclid(86_400), local(now).div_euclid(86_400));
    let ago = (now - at).max(0) / 1000;
    if day == today && at <= now + 60_000 {
        return if ago < 60 {
            "just now".into()
        } else if ago < 3600 {
            format!("{} min ago", ago / 60)
        } else {
            format!("{} h ago", ago / 3600)
        };
    }
    let (y, m, d) = deckcraft_engine::cmd::design::civil(day);
    let secs = local(at).rem_euclid(86_400);
    let month = MONTHS.get((m as usize).saturating_sub(1)).copied().unwrap_or("");
    let (ny, ..) = deckcraft_engine::cmd::design::civil(today);
    if y == ny {
        format!("{month} {d}, {:02}:{:02}", secs / 3600, secs % 3600 / 60)
    } else {
        format!("{month} {d} {y}, {:02}:{:02}", secs / 3600, secs % 3600 / 60)
    }
}

/// Every frame: what the open pane shows counts as seen, mentions of you flash the button, and
/// the room hears whether you're typing.
pub fn tick(app: &mut SlideApp, ctx: &egui::Context) {
    let Some(uid) = app.session.active().map(|d| d.uid) else { return };
    let last = chat(app).and_then(|c| c.messages.last().map(|m| m.id.clone()));
    if pane_open(app) || !app.chat.seen.contains_key(&uid) {
        app.chat.seen.insert(uid, last);
    }
    let me = my_name(app);
    let now = crate::now_ms();
    let fresh: Vec<String> =
        unread(app).iter().filter(|m| !app.chat.flashed.contains(&m.id) && mentions(&m.text, &me)).map(|m| m.id.clone()).collect();
    if !fresh.is_empty() {
        app.chat.flash_until = now + FLASH_MS;
        app.chat.flashed.extend(fresh);
    }
    let focused = ctx.memory(|m| m.has_focus(egui::Id::new(BOX_ID)));
    let want = pane_open(app) && focused && !app.chat.draft.trim().is_empty() && app.session.active_room().is_some();
    if want != app.chat.typing {
        app.chat.typing = want;
        let _ = app.run("chat.typing", json!({"typing": want}));
    }
}

/// Whether the chat button flashes now.
pub fn flashing(app: &SlideApp) -> bool {
    crate::now_ms() < app.chat.flash_until
}

/// The title bar's chat button with the unread count, beside the people in a session (outside
/// one, when the presentation has a chat). `ui` lays out right to left.
pub fn title_button(app: &mut SlideApp, ui: &mut Ui, control_h: f32) {
    let Some(c) = chat(app) else { return };
    let in_room = app.session.active_room().is_some();
    if !in_room && c.is_empty() && !pane_open(app) {
        return;
    }
    let t = Tokens::get(ui.ctx());
    let n = unread(app).len();
    let flash = flashing(app);
    let badge = match n {
        0 => None,
        1..=99 => Some(n.to_string()),
        _ => Some("99+".to_string()),
    };
    let w = if badge.is_some() { 44.0 } else { 30.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(w, control_h), Sense::click());
    let on = pane_open(app);
    let fill = if flash {
        // A slow pulse in the accent colour.
        let phase = (crate::now_ms() / 300.0).sin() as f32 * 0.5 + 0.5;
        t.accent.gamma_multiply(0.25 + 0.45 * phase)
    } else if on {
        t.selected
    } else if resp.hovered() {
        t.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
    let icon = egui::Rect::from_center_size(pos2(rect.min.x + 15.0, rect.center().y), vec2(18.0, 18.0));
    icons::paint(ui.painter(), icon, Icon::CommentsSmall, t.text, false);
    if let Some(b) = &badge {
        let galley = ui.painter().layout_no_wrap(b.clone(), theme::bold(10.0), Color32::WHITE);
        let size = vec2((galley.size().x + 8.0).max(16.0), 16.0);
        let r = egui::Rect::from_center_size(pos2(rect.max.x - size.x / 2.0 - 1.0, rect.center().y - 4.0), size);
        ui.painter().rect_filled(r, CornerRadius::same(8), Color32::from_rgb(0xC4, 0x3B, 0x3B));
        ui.painter().galley(r.center() - galley.size() / 2.0, galley, Color32::WHITE);
    }
    if flash {
        ui.ctx().request_repaint();
    }
    let label = match n {
        0 => "Chat messages".to_string(),
        n => format!("Chat messages ({n} unread)"),
    };
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.clone()));
    let tip = match n {
        0 => "Chat with the people editing this presentation".to_string(),
        1 => "Chat: 1 new message".to_string(),
        n => format!("Chat: {n} new messages"),
    };
    if resp.on_hover_text(tip).clicked() {
        let _ = app.run("view.chat", json!({}));
    }
}

/// `view.chat {on?}`: shows or hides the pane.
pub fn toggle(app: &mut SlideApp, on: Option<bool>) {
    let open = on.unwrap_or(!pane_open(app));
    if open {
        app.ui.pane = Some("chat".into());
        app.chat.focus_box = true;
    } else if pane_open(app) {
        app.ui.pane = None;
    }
}

fn circle(ui: &Ui, center: egui::Pos2, r: f32, a: &ChatAuthor) {
    let fill = crate::collab_ui::color(&a.color);
    ui.painter().circle_filled(center, r, fill);
    ui.painter().text(center, Align2::CENTER_CENTER, a.initials(), theme::bold(r * 0.8), ink_on(fill));
}

fn ink_on(c: Color32) -> Color32 {
    let l = 0.299 * f32::from(c.r()) + 0.587 * f32::from(c.g()) + 0.114 * f32::from(c.b());
    if l > 165.0 { Color32::from_rgb(0x20, 0x20, 0x20) } else { Color32::WHITE }
}

/// One line of a message's text: plain text selectable, links clickable, mentions highlighted.
fn text_line(ui: &mut Ui, line: &str, names: &[String], t: &Tokens) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        if line.is_empty() {
            ui.label(" ");
        }
        for seg in segments(line, names) {
            match seg {
                Seg::Text(s) => {
                    ui.add(egui::Label::new(RichText::new(s).color(t.text)).selectable(true));
                }
                Seg::Link(s) => {
                    let url = if s.to_ascii_lowercase().starts_with("www.") { format!("https://{s}") } else { s.to_string() };
                    ui.hyperlink_to(s, url);
                }
                Seg::Mention(s) => {
                    ui.add(
                        egui::Label::new(RichText::new(s).strong().color(t.accent).background_color(t.accent.gamma_multiply(0.15))).selectable(true),
                    );
                }
            }
        }
    });
}

/// What the person did in the pane this frame.
enum Act {
    Reply(String),
    Edit(String, String),
    Delete(String),
}

/// The Chat task pane.
pub fn panel(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(c) = chat(app).cloned() else { return };
    let me = my_id(app);
    let names = names(app);
    let now = crate::now_ms();
    let offset = local_offset(now);
    let in_room = app.session.active_room().is_some();
    let typing: Vec<String> =
        app.session.active_room().map(|r| r.peers().into_iter().filter(|p| p.typing).map(|p| p.user.name).collect()).unwrap_or_default();
    let mut act: Option<Act> = None;
    let mut export: Option<&str> = None;

    // Header: who's typing, and the menu.
    ui.horizontal(|ui| {
        let line = match typing.as_slice() {
            [] => String::new(),
            [a] => format!("{a} is typing…"),
            [a, b] => format!("{a} and {b} are typing…"),
            _ => "Several people are typing…".into(),
        };
        ui.label(RichText::new(line).italics().color(t.text_dim).small());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button("⋯", |ui| {
                if ui.button("Export as text").on_hover_text("Copy the chat as text").clicked() {
                    export = Some("text");
                    ui.close();
                }
                if ui.button("Export as JSON").on_hover_text("Copy the chat as JSON").clicked() {
                    export = Some("json");
                    ui.close();
                }
                ui.separator();
                if ui.add_enabled(!c.is_empty(), egui::Button::new("Clear chat…")).clicked() {
                    app.chat.confirm_clear = true;
                    ui.close();
                }
                ui.separator();
                ui.label(RichText::new("Kept in the file; Office drops it when it saves").small().color(t.text_dim));
            })
            .response
            .on_hover_text("Chat options");
        });
    });
    if !in_room {
        ui.label(RichText::new("Not in a live session: messages stay in this presentation as notes.").small().color(t.text_dim));
    }
    ui.separator();

    // The messages.
    let composer_h = 118.0;
    let list_h = (ui.available_height() - composer_h).max(80.0);
    egui::ScrollArea::vertical().id_salt("chat_messages").stick_to_bottom(true).max_height(list_h).auto_shrink([false, false]).show(ui, |ui| {
        if c.is_empty() {
            ui.add_space(12.0);
            ui.label(RichText::new("No messages yet.").color(t.text_dim));
            ui.label(RichText::new("Messages are saved with the presentation, so they're here when it's opened again.").small().color(t.text_dim));
        }
        for m in &c.messages {
            ui.add_space(6.0);
            ui.horizontal_top(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::hover());
                circle(ui, r.center(), 12.0, &m.author);
                ui.vertical(|ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(&m.author.name).strong().color(crate::collab_ui::color(&m.author.color)));
                        ui.label(RichText::new(time_text(m.at, now, offset)).small().color(t.text_dim));
                        if m.edited.is_some() && !m.deleted {
                            ui.label(RichText::new("edited").small().italics().color(t.text_dim));
                        }
                    });
                    if let Some(q) = m.reply_to.as_deref().and_then(|r| c.get(r)) {
                        let quoted: String =
                            if q.deleted { "Message deleted".into() } else { q.text.lines().next().unwrap_or("").chars().take(80).collect() };
                        egui::Frame::NONE.stroke(Stroke::NONE).inner_margin(egui::Margin { left: 6, right: 2, top: 1, bottom: 1 }).show(ui, |ui| {
                            let r = ui.max_rect();
                            ui.painter().line_segment([pos2(r.min.x - 6.0, r.min.y), pos2(r.min.x - 6.0, r.max.y)], Stroke::new(2.0, t.border));
                            ui.label(RichText::new(format!("{}: {quoted}", q.author.name)).small().color(t.text_dim));
                        });
                    }
                    if m.deleted {
                        ui.label(RichText::new("Message deleted").italics().color(t.text_dim));
                    } else {
                        for line in m.text.split('\n') {
                            text_line(ui, line, &names, &t);
                        }
                    }
                    if !m.deleted {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            let link =
                                |ui: &mut Ui, s: &str| ui.add(egui::Label::new(RichText::new(s).small().color(t.accent)).sense(Sense::click()));
                            if link(ui, "Reply").clicked() {
                                act = Some(Act::Reply(m.id.clone()));
                            }
                            if m.author.id == me {
                                if link(ui, "Edit").clicked() {
                                    act = Some(Act::Edit(m.id.clone(), m.text.clone()));
                                }
                                if link(ui, "Delete").clicked() {
                                    act = Some(Act::Delete(m.id.clone()));
                                }
                            }
                        });
                    }
                });
            });
        }
    });
    ui.separator();

    // The box: Enter sends, Shift+Enter is a new line, Escape stops replying or editing.
    let banner = match (&app.chat.editing, &app.chat.reply_to) {
        (Some(_), _) => Some("Editing your message".to_string()),
        (None, Some(r)) => c.get(r).map(|q| format!("Replying to {}", q.author.name)),
        _ => None,
    };
    let mut cancel = false;
    if let Some(b) = banner {
        ui.horizontal(|ui| {
            ui.label(RichText::new(b).small().color(t.text_dim));
            if ui.small_button("Cancel").clicked() {
                cancel = true;
            }
        });
    }
    let id = egui::Id::new(BOX_ID);
    let focused = ui.memory(|mem| mem.has_focus(id));
    // Enter without Shift sends (taken before the box sees it); Shift+Enter reaches the box.
    let enter = focused
        && ui.input_mut(|i| {
            let before = i.events.len();
            i.events
                .retain(|e| !matches!(e, egui::Event::Key { key: Key::Enter, pressed: true, modifiers, .. } if !modifiers.shift && !modifiers.alt));
            i.events.len() != before
        });
    if focused && ui.input(|i| i.key_pressed(Key::Escape)) {
        cancel = true;
    }
    let can = deckcraft_engine::find_command("chat.send").is_some_and(|spec| (spec.enabled)(&app.session).is_ok());
    let hint = if can { "Message (Enter to send, Shift+Enter for a new line)" } else { "The chat opens once the session's document is here" };
    let edit = egui::TextEdit::multiline(&mut app.chat.draft).id(id).desired_rows(2).desired_width(f32::INFINITY).hint_text(hint);
    let resp = ui.add_enabled(can, edit);
    if app.chat.focus_box {
        app.chat.focus_box = false;
        resp.request_focus();
    }
    let mut send = enter;
    ui.horizontal(|ui| {
        let label = if app.chat.editing.is_some() { "Save" } else { "Send" };
        if ui.add_enabled(can && !app.chat.draft.trim().is_empty(), egui::Button::new(label)).clicked() {
            send = true;
        }
    });
    if cancel {
        app.chat.reply_to = None;
        if app.chat.editing.take().is_some() {
            app.chat.draft.clear();
        }
    }
    if send && !app.chat.draft.trim().is_empty() {
        let text = app.chat.draft.clone();
        let r = match app.chat.editing.clone() {
            Some(id) => app.run("chat.edit", json!({"id": id, "text": text})),
            None => {
                let mut p = json!({"text": text});
                if let Some(r) = &app.chat.reply_to {
                    p["replyTo"] = json!(r);
                }
                app.run("chat.send", p)
            }
        };
        match r {
            Ok(_) => {
                app.chat.draft.clear();
                app.chat.reply_to = None;
                app.chat.editing = None;
            }
            Err(e) => app.set_status(e),
        }
        resp.request_focus();
    }
    match act {
        Some(Act::Reply(id)) => {
            app.chat.reply_to = Some(id);
            app.chat.editing = None;
            app.chat.focus_box = true;
        }
        Some(Act::Edit(id, text)) => {
            app.chat.editing = Some(id);
            app.chat.reply_to = None;
            app.chat.draft = text;
            app.chat.focus_box = true;
        }
        Some(Act::Delete(id)) => {
            if let Err(e) = app.run("chat.delete", json!({"id": id})) {
                app.set_status(e);
            }
        }
        None => {}
    }
    if let Some(format) = export {
        match app.run("chat.export", json!({"format": format})) {
            Ok(v) => {
                app.copy_text(v["text"].as_str().unwrap_or("").to_string());
                app.set_status("Chat copied");
            }
            Err(e) => app.set_status(e),
        }
    }
    confirm_clear(app, ui.ctx());
}

/// Clear chat…: "Remove the whole chat from the document for everyone?"
fn confirm_clear(app: &mut SlideApp, ctx: &egui::Context) {
    if !app.chat.confirm_clear {
        return;
    }
    let t = Tokens::get(ctx);
    let mut choice: Option<bool> = None;
    let modal = egui::Modal::new(egui::Id::new("chat_clear")).show(ctx, |ui| {
        ui.set_width(380.0);
        ui.label(RichText::new("Clear the whole chat").font(theme::bold(15.0)).color(t.text));
        ui.add_space(6.0);
        ui.label("Remove the whole chat from the document for everyone?");
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button(RichText::new("Clear chat").strong()).clicked() {
                choice = Some(true);
            }
            if ui.button("Cancel").clicked() {
                choice = Some(false);
            }
        });
    });
    if modal.should_close() && choice.is_none() {
        choice = Some(false);
    }
    match choice {
        Some(true) => {
            app.chat.confirm_clear = false;
            if let Err(e) = app.run("chat.clear", json!({})) {
                app.set_status(e);
            }
        }
        Some(false) => app.chat.confirm_clear = false,
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_and_mentions() {
        let names = vec!["Ann Lee".to_string(), "Ann".to_string(), "Bob".to_string()];
        let s = segments("Hi @ann lee, see https://example.org/a?b=c. and @Bobby or me@Bob", &names);
        assert_eq!(
            s,
            vec![
                Seg::Text("Hi "),
                Seg::Mention("@ann lee"),
                Seg::Text(", see "),
                Seg::Link("https://example.org/a?b=c"),
                Seg::Text(". and @Bobby or me@Bob"),
            ]
        );
        assert_eq!(segments("www.example.org", &names), vec![Seg::Link("www.example.org")]);
        assert_eq!(segments("", &names), vec![]);
        assert!(mentions("thanks @Ann!", "Ann Lee"));
        assert!(!mentions("thanks Ann", "Ann Lee"));
        assert!(!mentions("@Annabel", "Ann"));
        // Hostile text never panics.
        for s in ["@", "@@", "http://", "😀@😀 www.", "\u{0}@Ann\u{301}"] {
            let _ = segments(s, &names);
        }
    }

    #[test]
    fn times() {
        let now = 1_760_605_200_000.0; // 2025-10-16 09:00 UTC
        assert_eq!(time_text(1_760_605_190_000, now, 0), "just now");
        assert_eq!(time_text(1_760_605_200_000 - 5 * 60_000, now, 0), "5 min ago");
        assert_eq!(time_text(1_760_605_200_000 - 3 * 3_600_000, now, 0), "3 h ago");
        assert_eq!(time_text(1_760_605_200_000 - 86_400_000, now, 0), "Oct 15, 09:00");
        assert_eq!(time_text(1_760_605_200_000 - 86_400_000, now, 2 * 3600), "Oct 15, 11:00");
        assert_eq!(time_text(0, now, 0), "Jan 1 1970, 00:00");
        assert_eq!(days_from_civil(1970, 1, 1), 0);
    }
}
