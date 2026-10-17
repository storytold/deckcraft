//! Live sessions (co-editing, plugin protocol 2) in the interface: the engine's pump each frame,
//! the Share button at the right end of the title bar (start or join a session; in one, its state
//! and the session popover), the people in the session as circles beside it (click one to go to
//! what they have selected), others' selections and carets drawn on the slide in their colours
//! with name tags, others' edits briefly highlighted, and the rooms' state kept for recovery.
//!
//! The engine does the work (`deckcraft_engine::collab`); this only shows it. Everything is drawn
//! in code with the theme's tokens.

use deckcraft_engine::collab::{self, RoomState, User};
use deckcraft_engine::{TextSel, plugin};
use deckcraft_geom::Point;
use deckcraft_model::ShapeId;
use egui::{Align2, Color32, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::{Value as Json, json};

use crate::SlideApp;
use crate::canvas::Xf;
use crate::theme::{self, Tokens};

/// How often the rooms' state is written for recovery.
const RECOVERY_MS: f64 = 30_000.0;
/// People shown as circles before "+N".
const MAX_CIRCLES: usize = 4;
const CIRCLE_R: f32 = 11.0;
/// Centre to centre: the circles overlap a little.
const CIRCLE_STEP: f32 = 17.0;

/// The live-session windows.
#[derive(Default)]
pub struct State {
    /// The Share window (start or join a session).
    pub share: Option<ShareWindow>,
    /// The session popover under the Share button, and where the button is.
    pub popover: bool,
    pub anchor: Option<Rect>,
    recovery_at: f64,
}

/// What the Share window holds.
#[derive(Clone, Debug, Default)]
pub struct ShareWindow {
    /// The transport add-in chosen.
    pub plugin: String,
    pub name: String,
    pub color: String,
    /// For a transport without its own Share and Join commands: the room and its key.
    pub room: String,
    pub key: String,
    pub encrypt: bool,
    pub error: Option<String>,
}

pub fn color(hex: &str) -> Color32 {
    let v = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0x80_80_80);
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// White or near-black, whichever reads on `c`.
fn ink_on(c: Color32) -> Color32 {
    let l = 0.299 * f32::from(c.r()) + 0.587 * f32::from(c.g()) + 0.114 * f32::from(c.b());
    if l > 165.0 { Color32::from_rgb(0x20, 0x20, 0x20) } else { Color32::WHITE }
}

/// Runs the engine's co-editing between frames.
pub fn pump(app: &mut SlideApp, ctx: &egui::Context) {
    if app.session.collab.rooms.is_empty() && !plugin::push::pending() {
        return;
    }
    let now = crate::now_ms();
    if app.session.collab_poll(now.max(0.0) as u64) {
        ctx.request_repaint();
    }
    for n in app.session.collab_take_notices() {
        app.set_status(n);
    }
    if now - app.collab.recovery_at > RECOVERY_MS {
        app.collab.recovery_at = now;
        app.session.save_collab_recovery();
    }
    if app.session.collab.rooms.is_empty() {
        app.collab.popover = false;
    } else {
        // Remote messages arrive between frames: keep looking.
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }
}

/// The session's state in words, for the Share button and the status bar.
pub fn state_label(app: &SlideApp) -> Option<String> {
    let room = app.session.active_room()?;
    Some(if room.refused.is_some() {
        "Not synced".into()
    } else if room.joining {
        "Joining…".into()
    } else {
        match room.state {
            RoomState::Online => "Live".into(),
            RoomState::Connecting => "Connecting…".into(),
            RoomState::Offline => "Offline".into(),
            RoomState::Ended => "Ended".into(),
        }
    })
}

/// The people in the active presentation's session: you first, then the others with their
/// Yjs client ids.
pub fn people(app: &SlideApp) -> Vec<(Option<u64>, User)> {
    let Some(room) = app.session.active_room() else { return Vec::new() };
    let mut out = vec![(None, room.user.clone())];
    for p in room.peers() {
        out.push((Some(p.client_id), p.user));
    }
    out
}

/// Goes to the slide a person is on and selects what they selected.
pub fn jump_to(app: &mut SlideApp, client_id: u64) {
    let Some((slide, shapes)) = collab::peer_target(&app.session, client_id) else { return };
    let _ = app.run("slide.go", json!({"index": slide}));
    if !shapes.is_empty() {
        let _ = app.run("edit.select", json!({"ids": shapes.iter().map(|s| s.0).collect::<Vec<_>>()}));
    }
}

/// The Share button and the people's circles, at the right end of the title bar (`ui` lays out
/// right to left).
pub fn title_bar(app: &mut SlideApp, ui: &mut Ui, control_h: f32) {
    let t = Tokens::get(ui.ctx());
    let has_doc = app.session.active().is_some();
    let label = state_label(app);
    let in_room = label.is_some();
    let text = label.clone().unwrap_or_else(|| "Share".into());
    let dot = app.session.active_room().map(|r| match (r.joining, r.refused.is_some(), r.state) {
        (_, true, _) => Color32::from_rgb(0xC4, 0x3B, 0x3B),
        (true, _, _) | (_, _, RoomState::Connecting) => Color32::from_rgb(0xE0, 0xA0, 0x1E),
        (_, _, RoomState::Online) => Color32::from_rgb(0x2E, 0x9E, 0x5B),
        _ => t.text_faint,
    });
    let galley = ui.painter().layout_no_wrap(text.clone(), theme::bold(13.0), t.accent);
    let w = galley.size().x + if dot.is_some() { 34.0 } else { 26.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(w.max(72.0), control_h), Sense::click());
    let fill = if resp.hovered() { t.hover } else { t.card };
    ui.painter().rect(rect, control_h / 2.0, fill, Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
    let mut x = rect.min.x + 13.0;
    if let Some(c) = dot {
        ui.painter().circle_filled(pos2(x + 3.0, rect.center().y), 4.0, c);
        x += 12.0;
    }
    ui.painter().galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley, t.accent);
    let tip = if in_room {
        "Live session: who's here, the invitation, Leave".to_string()
    } else if plugin::transports().is_empty() {
        "Share needs a co-editing transport add-in: load one in the Add-ins window".to_string()
    } else {
        "Edit this presentation live with others".to_string()
    };
    let resp = resp.on_hover_text(tip);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, text.clone()));
    app.collab.anchor = Some(rect);
    if resp.clicked() && has_doc {
        if in_room {
            app.collab.popover = !app.collab.popover;
        } else {
            open_share(app);
        }
    }
    if in_room {
        circles(app, ui, control_h);
    }
}

/// Opens the Share window with this person's name and colour and the first transport.
pub fn open_share(app: &mut SlideApp) {
    let status = app.session.execute("collab.status", &json!({})).unwrap_or_default();
    let first = status["transports"].get(0).and_then(|t| t["id"].as_str()).unwrap_or("").to_string();
    let user = &status["user"];
    app.collab.share = Some(ShareWindow {
        plugin: first,
        name: user["name"].as_str().unwrap_or("").to_string(),
        color: user["color"].as_str().unwrap_or("#3B7DD8").to_string(),
        encrypt: true,
        ..Default::default()
    });
}

/// The people's circles: initials on their colour, overlapping, at most four then "+N".
fn circles(app: &mut SlideApp, ui: &mut Ui, control_h: f32) {
    let t = Tokens::get(ui.ctx());
    let people = people(app);
    let shown = people.len().min(MAX_CIRCLES);
    let extra = people.len() - shown;
    let more = if extra > 0 { 28.0 } else { 0.0 };
    let width = CIRCLE_STEP * (shown.saturating_sub(1)) as f32 + CIRCLE_R * 2.0 + more + 4.0;
    let (rect, _) = ui.allocate_exact_size(vec2(width, control_h), Sense::hover());
    let mut jump = None;
    for (i, (client, user)) in people.iter().take(shown).enumerate() {
        let c = pos2(rect.min.x + CIRCLE_R + 2.0 + CIRCLE_STEP * i as f32, rect.center().y);
        let fill = color(&user.color);
        ui.painter().circle(c, CIRCLE_R, fill, Stroke::new(1.5, t.chrome));
        ui.painter().text(c, Align2::CENTER_CENTER, user.initials(), theme::bold(10.0), ink_on(fill));
        let hit = Rect::from_center_size(c, vec2(CIRCLE_R * 2.0, CIRCLE_R * 2.0));
        let id = ui.id().with(("collab_person", i));
        let resp = ui.interact(hit, id, if client.is_some() { Sense::click() } else { Sense::hover() });
        let name = if client.is_some() { user.name.clone() } else { format!("{} (you)", user.name) };
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name.clone()));
        let tip = if client.is_some() { format!("{name} — click to go to their selection") } else { name };
        if resp.on_hover_text(tip).clicked()
            && let Some(id) = client
        {
            jump = Some(*id);
        }
    }
    if extra > 0 {
        let at = pos2(rect.max.x - more / 2.0 - 2.0, rect.center().y);
        let r = ui.painter().text(at, Align2::CENTER_CENTER, format!("+{extra}"), theme::bold(11.0), t.text_dim);
        let names: Vec<String> = people.iter().skip(shown).map(|(_, u)| u.name.clone()).collect();
        let more_id = ui.id().with("collab_more");
        let resp = ui.interact(r.expand(4.0), more_id, Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("+{extra}")));
        if resp.on_hover_text(names.join(", ")).clicked() {
            app.collab.popover = true;
        }
    }
    if let Some(id) = jump {
        jump_to(app, id);
    }
}

/// The Share window and the session popover.
pub fn windows(app: &mut SlideApp, ctx: &egui::Context) {
    share_window(app, ctx);
    popover(app, ctx);
}

fn share_window(app: &mut SlideApp, ctx: &egui::Context) {
    let Some(mut w) = app.collab.share.take() else { return };
    let transports = plugin::transports();
    let mut open = true;
    let mut action: Option<&str> = None;
    egui::Window::new("Share").id(egui::Id::new("collab_share")).collapsible(false).resizable(false).default_width(420.0).open(&mut open).show(
        ctx,
        |ui| {
            ui.label("Edit this presentation live with others: everyone sees the others' changes as they make them, and who is where.");
            ui.add_space(6.0);
            if transports.is_empty() {
                ui.label(
                    egui::RichText::new("Live sessions go through a transport add-in (craft-collab's Live (MQTT), for example). None is loaded.")
                        .strong(),
                );
                if ui.button("Add-ins…").clicked() {
                    app.addins.window = true;
                    action = Some("close");
                }
                return;
            }
            egui::Grid::new("collab_share_grid").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                ui.label("Through:");
                let label = |id: &str| {
                    transports
                        .iter()
                        .find(|p| p.manifest.id == id)
                        .map(|p| {
                            p.manifest.collab.as_ref().map(|c| c.label.clone()).filter(|l| !l.is_empty()).unwrap_or_else(|| p.manifest.name.clone())
                        })
                        .unwrap_or_default()
                };
                egui::ComboBox::from_id_salt("collab_transport").selected_text(label(&w.plugin)).show_ui(ui, |ui| {
                    for p in &transports {
                        let l = label(&p.manifest.id);
                        ui.selectable_value(&mut w.plugin, p.manifest.id.clone(), l);
                    }
                });
                ui.end_row();
                ui.label("Your name:");
                ui.add(egui::TextEdit::singleline(&mut w.name).desired_width(220.0));
                ui.end_row();
                ui.label("Your colour:");
                ui.horizontal(|ui| {
                    let mut c = color(&w.color);
                    if ui.color_edit_button_srgba(&mut c).changed() {
                        w.color = format!("#{:02X}{:02X}{:02X}", c.r(), c.g(), c.b());
                    }
                    ui.label(&w.color);
                });
                ui.end_row();
            });
            let own = transports
                .iter()
                .find(|p| p.manifest.id == w.plugin)
                .map(|p| (p.manifest.command("share").is_some(), p.manifest.command("join").is_some()));
            let (has_share, has_join) = own.unwrap_or((false, false));
            if !(has_share && has_join) {
                ui.add_space(4.0);
                egui::Grid::new("collab_share_room").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                    ui.label("Room:");
                    ui.add(egui::TextEdit::singleline(&mut w.room).desired_width(220.0).hint_text("the room's id"));
                    ui.end_row();
                    ui.label("Room key:");
                    ui.add(egui::TextEdit::singleline(&mut w.key).password(true).desired_width(220.0).hint_text("base64, from the invitation"));
                    ui.end_row();
                });
                ui.checkbox(&mut w.encrypt, "Encrypt end to end (a new room key when starting)");
            }
            if let Some(e) = &w.error {
                ui.colored_label(Color32::from_rgb(0xC4, 0x3B, 0x3B), e);
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button(egui::RichText::new("Start live session").strong()).clicked() {
                    action = Some("share");
                }
                if ui.button("Join a session…").clicked() {
                    action = Some("join");
                }
            });
        },
    );
    match action {
        Some("close") => return,
        Some(mode) => match start(app, &w, mode) {
            Ok(()) => return,
            Err(e) => w.error = Some(e),
        },
        None => {}
    }
    if open {
        app.collab.share = Some(w);
    }
}

/// Starts or joins a session from the Share window: the transport's own `share` / `join` command
/// when it has one (it asks for what it needs and answers `collab.join`), else `collab.join` with
/// the room typed.
fn start(app: &mut SlideApp, w: &ShareWindow, mode: &str) -> Result<(), String> {
    if w.plugin.is_empty() {
        return Err("Choose a transport.".into());
    }
    let mut user = json!({});
    if !w.name.trim().is_empty() {
        user["name"] = json!(w.name.trim());
    }
    if collab::valid_color(&w.color) {
        user["color"] = json!(w.color);
    }
    app.session.execute("collab.setUser", &user).map_err(|e| e.to_string())?;
    let has_own = plugin::get(&w.plugin).is_some_and(|p| p.manifest.command(mode).is_some());
    if has_own {
        let params = json!({"name": w.name.trim(), "color": w.color, "encrypt": w.encrypt});
        return crate::addins::run_command(app, &w.plugin, mode, params);
    }
    if w.room.trim().is_empty() {
        return Err("Type the room's id (the transport has no Share or Join of its own).".into());
    }
    let mut key = w.key.trim().to_string();
    if key.is_empty() && mode == "share" && w.encrypt {
        key = deckcraft_engine::collab_core::crypto::Key::generate().map_err(|e| e.to_string())?.to_base64();
    }
    let mut p = json!({"plugin": w.plugin, "room": w.room.trim(), "mode": mode});
    if !key.is_empty() {
        p["key"] = json!(key);
    }
    app.run("collab.join", p).map(|_| ())
}

fn popover(app: &mut SlideApp, ctx: &egui::Context) {
    if !app.collab.popover {
        return;
    }
    let Some(room) = app.session.active_room() else {
        app.collab.popover = false;
        return;
    };
    let t = Tokens::get(ctx);
    let anchor = app.collab.anchor.unwrap_or(Rect::from_min_size(pos2(0.0, 0.0), vec2(0.0, 0.0)));
    let (room_id, plugin_id, link, encrypted, message) =
        (room.room.clone(), room.plugin.clone(), room.link.clone(), room.encrypted(), room.message.clone());
    let state = state_label(app).unwrap_or_default();
    let people = people(app);
    let embed_host = app.embed.as_ref().map(|e| e.host.clone());
    let mut leave = false;
    let mut jump = None;
    let mut copy = None;
    let area = egui::Area::new(egui::Id::new("collab_popover"))
        .order(egui::Order::Foreground)
        .fixed_pos(pos2(anchor.max.x - 300.0, anchor.max.y + 6.0))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).fill(t.card).stroke(Stroke::new(1.0, t.card_border)).show(ui, |ui| {
                ui.set_width(288.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(&state).strong());
                    if encrypted {
                        ui.label(egui::RichText::new("· encrypted end to end").color(t.text_dim));
                    }
                });
                ui.label(egui::RichText::new(&room_id).color(t.text_dim).small()).on_hover_text(format!("through {plugin_id}"));
                if let Some(m) = &message {
                    ui.label(egui::RichText::new(m).color(t.text_dim));
                }
                ui.separator();
                for (client, user) in &people {
                    let resp = ui
                        .horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                            let fill = color(&user.color);
                            ui.painter().circle_filled(r.center(), 9.0, fill);
                            ui.painter().text(r.center(), Align2::CENTER_CENTER, user.initials(), theme::bold(8.5), ink_on(fill));
                            let name = if client.is_some() { user.name.clone() } else { format!("{} (you)", user.name) };
                            ui.add(egui::Label::new(name).sense(if client.is_some() { Sense::click() } else { Sense::hover() }))
                        })
                        .inner;
                    if resp.clicked()
                        && let Some(id) = client
                    {
                        jump = Some(*id);
                    }
                }
                if people.len() == 1 {
                    ui.label(egui::RichText::new("Nobody else is here yet.").color(t.text_dim));
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if let Some(l) = &link
                        && ui.button("Copy invitation").on_hover_text("Send it to the people you want to edit with").clicked()
                    {
                        copy = Some(l.clone());
                    }
                    // Embedded (craft-live): leaving the room is closing the editor.
                    let (label, tip) = match &embed_host {
                        Some(host) => ("Close", format!("Close the presentation and go back to {host}")),
                        None => ("Leave", "The presentation stays open here with every edit".to_string()),
                    };
                    if ui.button(label).on_hover_text(tip).clicked() {
                        leave = true;
                    }
                });
            });
        });
    // A click elsewhere (but on the Share button, which toggles) closes it.
    let clicked_out = ctx.input(|i| i.pointer.any_click())
        && ctx.input(|i| i.pointer.interact_pos()).is_some_and(|p| !area.response.rect.contains(p) && !anchor.contains(p));
    if let Some(l) = copy {
        app.copy_text(l);
        app.set_status("Invitation copied");
    }
    if let Some(id) = jump {
        jump_to(app, id);
        app.collab.popover = false;
    }
    if leave {
        match app.embed.as_mut() {
            Some(e) => e.close_requested = true,
            None => {
                let _ = app.run("collab.leave", json!({}));
            }
        }
        app.collab.popover = false;
    }
    if clicked_out || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.collab.popover = false;
    }
}

/// Others' selections (outlined in their colours with their names), their carets, and the shapes
/// they just changed (highlighted for two seconds).
pub fn paint_remote(app: &SlideApp, painter: &egui::Painter, xf: Xf) {
    let Some(st) = app.session.active() else { return };
    if st.selection.target != deckcraft_engine::Target::Slides {
        return;
    }
    let Some(room) = app.session.active_room() else { return };
    let Some(slide) = st.current_slide() else { return };
    let corners = |sh: &deckcraft_model::Shape| {
        let x = deckcraft_engine::cmd::xfrm_of(&st.doc, &st.selection, sh);
        let a = x.affine();
        [Point::new(0.0, 0.0), Point::new(x.w, 0.0), Point::new(x.w, x.h), Point::new(0.0, x.h)].map(|q| xf.to_screen(a * q))
    };
    let now = app.session.collab.now();
    for (uid, s, shape, at) in &app.session.collab.remote_edits {
        if *uid != st.uid || *s != slide.id.0 {
            continue;
        }
        let Some(sh) = st.shape(ShapeId(*shape)) else { continue };
        let fade = 1.0 - (now.saturating_sub(*at) as f32 / collab::HIGHLIGHT_MS as f32).clamp(0.0, 1.0);
        let c = Color32::from_rgba_unmultiplied(0xFF, 0xB0, 0x00, (200.0 * fade) as u8);
        let pts = corners(sh);
        painter.add(egui::Shape::closed_line(pts.to_vec(), Stroke::new(3.0, c)));
    }
    for peer in room.peers() {
        let c = color(&peer.user.color);
        let Some((index, shapes)) = &peer.selection else { continue };
        if *index != st.selection.slide {
            continue;
        }
        let mut tag_at: Option<egui::Pos2> = None;
        for id in shapes {
            let Some(sh) = st.shape(*id) else { continue };
            let pts = corners(sh);
            painter.add(egui::Shape::closed_line(pts.to_vec(), Stroke::new(2.0, c)));
            if tag_at.is_none() {
                tag_at = pts.iter().copied().reduce(|a, b| if b.y < a.y || (b.y == a.y && b.x < a.x) { b } else { a });
            }
        }
        // Their caret (or the end of their text selection), always shown.
        if let Some(text) = &peer.text {
            let ts = TextSel {
                shape: ShapeId(text.shape.unwrap_or(0)),
                anchor: text.head,
                caret: text.head,
                cell: None,
                notes: text.notes,
                line_end: None,
            };
            if !text.notes
                && let Some((top, bot)) = deckcraft_engine::cmd::text::caret_line(st, &ts)
            {
                let (a, b) = (xf.to_screen(top), xf.to_screen(bot));
                painter.line_segment([a, b], Stroke::new(2.0, c));
                painter.circle_filled(a, 2.5, c);
                tag_at = Some(a);
            }
        }
        if let Some(p) = tag_at {
            let font = theme::font(10.5);
            let galley = painter.layout_no_wrap(peer.user.name.clone(), font, ink_on(c));
            let size = galley.size() + vec2(8.0, 3.0);
            let tag = Rect::from_min_size(pos2(p.x, p.y - size.y - 2.0), size);
            painter.rect_filled(tag, 3.0, c);
            painter.galley(tag.min + vec2(4.0, 1.5), galley, ink_on(c));
        }
    }
}

/// The session's state for the status bar: `Live`, `Joining…`, the transport's message.
pub fn status_text(app: &SlideApp) -> Option<String> {
    let label = state_label(app)?;
    let room = app.session.active_room()?;
    Some(match room.refused.clone().or_else(|| room.message.clone()) {
        Some(m) => format!("{label} — {m}"),
        None => label,
    })
}

/// A plugin command answered `collab.join` (protocol 2): join the room it named.
pub fn join_from_plugin(app: &mut SlideApp, plugin_id: &str, answer: &Json) -> Result<(), String> {
    let Some(join) = answer.get("collab").and_then(|c| c.get("join")) else { return Ok(()) };
    let mut p = join.clone();
    let Some(o) = p.as_object_mut() else { return Err("the add-in's collab.join isn't an object".into()) };
    o.insert("plugin".into(), json!(plugin_id));
    // The transport's invitation (craft-collab's transports give `result.link`), to copy later.
    if let Some(link) = answer.get("result").and_then(|r| r.get("link")).and_then(Json::as_str) {
        o.entry("link").or_insert(json!(link));
    }
    app.run("collab.join", p).map(|_| ())
}
