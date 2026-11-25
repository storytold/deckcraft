use super::{Align2, Icon, Rect, Sense, SlideApp, Tokens, Ui, icon_toggle, json, pos2, theme, vec2};
use egui::{CursorIcon, ResizeDirection, Stroke};

pub const TITLE_H: f32 = 38.0;

/// macOS draws the window into the title bar (`with_fullsize_content_view`) and AppKit paints the
/// traffic lights itself, 16 pt from the top of the window on macOS 27. Moving them would take
/// AppKit calls (`unsafe`, forbidden here), so on macOS the title bar is the band they are centred
/// in, with the controls beside them (#71). Up to macOS 15 the lights are smaller and centred 14 pt
/// from the top, about 2 pt above this bar's centre.
pub const MAC_TITLE_BAR: f32 = 32.0;

/// The traffic lights as AppKit lays them out, measured on macOS 27: button diameter,
/// centre-to-centre pitch and the first button's left edge.
pub(super) const MAC_LIGHT: f32 = 14.0;

pub(super) const MAC_LIGHT_PITCH: f32 = 23.0;

pub(super) const MAC_LIGHTS_LEFT: f32 = 9.0;

/// Where title-bar content starts on macOS: the end of the three lights plus the gap the lights
/// keep from the top of the window.
pub const MAC_CONTENT_LEFT: f32 = MAC_LIGHTS_LEFT + 2.0 * MAC_LIGHT_PITCH + MAC_LIGHT + (MAC_TITLE_BAR - MAC_LIGHT) / 2.0;

/// The title bar's height, the left edge of its content and the height of its tallest control.
/// With the integrated title bar (macOS) the bar is shorter, so the controls shrink to keep a
/// margin above and below them.
pub(super) fn title_geometry(integrated: bool) -> (f32, f32, f32) {
    if integrated { (MAC_TITLE_BAR, MAC_CONTENT_LEFT, 22.0) } else { (TITLE_H, 10.0, 26.0) }
}

/// The window's title row: traffic-light space, quick access, centred title, actions.
pub fn title_bar(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let (height, left, control_h) = title_geometry(app.integrated_titlebar);
    egui::Panel::top("title_bar").exact_size(height).frame(egui::Frame::NONE.fill(t.chrome)).show(ui, |ui| {
        let rect = ui.max_rect();
        // Dragging the empty title bar moves the window. The resize band takes its edge only while the
        // frame draws one: a maximized window has no edge resize, so its top edge stays draggable.
        let edge_resize = app.window_controls && !ui.input(|i| i.viewport().maximized.unwrap_or(false));
        let drag = ui.interact(if edge_resize { rect.shrink(RESIZE_BAND) } else { rect }, ui.scope_id().with("titledrag"), Sense::click_and_drag());
        // The drag surface is not a button: a full-width Button would be announced as one.
        drag.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Window, true, "Window title bar"));
        if drag.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
            handed_to_os(ui.ctx());
        }
        if drag.double_clicked() {
            let max = ui.input(|i| i.viewport().maximized.unwrap_or(false));
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
        }
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_max(pos2(rect.min.x + left, rect.min.y), pos2(rect.min.x + 420.0, rect.max.y)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        child.spacing_mut().item_spacing.x = 2.0;
        let has_doc = app.session.active().is_some();
        let dirty = app.session.active().is_some_and(|d| d.is_dirty());
        crate::qat::show(app, &mut child);
        let title =
            app.session.active().map(|d| format!("{}{}", d.title(), if dirty { " — Edited" } else { "" })).unwrap_or_else(|| "DeckCraft".into());
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, title, theme::font(13.0), t.text);
        // Right side. The frameless caption buttons keep their corner.
        let right_end = if app.window_controls { rect.max.x - 320.0 - 3.0 * CONTROL_W } else { rect.max.x - 320.0 };
        let mut right = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_max(pos2(right_end, rect.min.y), pos2(rect.max.x - 10.0 - if app.window_controls { 3.0 * CONTROL_W } else { 0.0 }, rect.max.y)))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        right.spacing_mut().item_spacing.x = 6.0;
        if icon_toggle(&mut right, Icon::Search, "Search commands (⇧⌘P)", true, false).clicked() {
            app.palette = Some((String::new(), 0));
        }
        let share = right.add(
            egui::Button::new(egui::RichText::new("  Present  ").color(t.accent_text).font(theme::bold(13.0)))
                .fill(t.accent)
                .corner_radius(control_h / 2.0)
                .min_size(vec2(84.0, control_h)),
        );
        if share.clicked() && has_doc {
            let from = app.session.active().map(|d| d.selection.slide).unwrap_or(0);
            app.start_show(from, false);
        }
        if icon_toggle(&mut right, Icon::CommentsBubble, "Comments", has_doc, app.ui.pane.as_deref() == Some("comments")).clicked() {
            let _ = app.run("view.pane", json!({"pane": "comments", "toggle": true}));
        }
        if icon_toggle(
            &mut right,
            if app.ui.brightness == theme::Brightness::Dark { Icon::Sparkle } else { Icon::Palette },
            "Light/Dark appearance",
            true,
            false,
        )
        .clicked()
        {
            let _ = app.run("view.dark", json!({}));
        }
    });
}

const CONTROL_W: f32 = 46.0;
const RESIZE_BAND: f32 = 5.0;
const RESIZE_CORNER: f32 = 14.0;
/// Over a control at the window's edge only this much resizes, so the control keeps its press.
const RESIZE_GRIP: f32 = 3.0;
/// Larger widgets are panels, not controls that need the edge.
const CONTROL_MAX: f32 = 64.0;

/// The window manager keeps the release after a move or resize, so egui would wait for it and
/// drop the next press (emilk/egui#7959).
pub(crate) fn handed_to_os(ctx: &egui::Context) {
    ctx.stop_dragging();
    ctx.input_mut(|i| i.pointer = Default::default());
}

fn over_edge_control(ctx: &egui::Context, screen: Rect, p: egui::Pos2) -> bool {
    let reach = screen.shrink(RESIZE_CORNER);
    if reach.contains(p) {
        return false;
    }
    ctx.viewport(|v| {
        v.prev_pass.widgets.layers().flat_map(|(_, ws)| ws.iter()).any(|w| {
            w.enabled
                && (w.sense.senses_click() || w.sense.senses_drag())
                && w.interact_rect.contains(p)
                && w.interact_rect.size().min_elem() < CONTROL_MAX
        })
    })
}

/// Draw controls and edge resizing for Linux/BSD frameless windows.
pub fn window_frame(app: &mut SlideApp, ctx: &egui::Context) {
    if !app.window_controls {
        return;
    }
    let (maximized, fullscreen) = ctx.input(|i| (i.viewport().maximized.unwrap_or(false), i.viewport().fullscreen.unwrap_or(false)));
    if fullscreen {
        return;
    }
    let t = Tokens::get(ctx);
    let screen = ctx.content_rect();
    let controls = Rect::from_min_size(pos2(screen.max.x - 3.0 * CONTROL_W, screen.min.y), vec2(3.0 * CONTROL_W, TITLE_H));
    egui::Area::new(egui::Id::unique("dc_window_controls")).order(egui::Order::Foreground).fixed_pos(controls.min).show(ctx, |ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        ui.horizontal(|ui| {
            for kind in ["minimize", "maximize", "close"] {
                let (r, response) = ui.allocate_exact_size(vec2(CONTROL_W, TITLE_H), Sense::click());
                let close = kind == "close";
                let hot = response.hovered() || response.is_pointer_button_down_on() || response.has_focus();
                let color = if close && hot {
                    ui.painter().rect_filled(r, 0.0, egui::Color32::from_rgb(0xC4, 0x2B, 0x1C));
                    egui::Color32::WHITE
                } else {
                    if hot {
                        ui.painter().rect_filled(r, 0.0, if response.is_pointer_button_down_on() { t.pressed } else { t.hover });
                    }
                    t.text
                };
                let m = r.center();
                let stroke = Stroke::new(1.0, color);
                let tip = match kind {
                    "minimize" => {
                        ui.painter().line_segment([pos2(m.x - 5.0, m.y), pos2(m.x + 5.0, m.y)], stroke);
                        "Minimize"
                    }
                    "maximize" if maximized => {
                        let front = Rect::from_min_size(pos2(m.x - 5.0, m.y - 3.0), vec2(8.0, 8.0));
                        ui.painter().rect_stroke(front, 0.0, stroke, egui::StrokeKind::Inside);
                        ui.painter().line_segment([pos2(m.x - 3.0, m.y - 5.0), pos2(m.x + 5.0, m.y - 5.0)], stroke);
                        ui.painter().line_segment([pos2(m.x + 5.0, m.y - 5.0), pos2(m.x + 5.0, m.y + 3.0)], stroke);
                        "Restore Down"
                    }
                    "maximize" => {
                        ui.painter().rect_stroke(Rect::from_center_size(m, vec2(10.0, 10.0)), 0.0, stroke, egui::StrokeKind::Inside);
                        "Maximize"
                    }
                    _ => {
                        ui.painter().line_segment([pos2(m.x - 5.0, m.y - 5.0), pos2(m.x + 5.0, m.y + 5.0)], stroke);
                        ui.painter().line_segment([pos2(m.x - 5.0, m.y + 5.0), pos2(m.x + 5.0, m.y - 5.0)], stroke);
                        "Close"
                    }
                };
                response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
                if response.on_hover_text(tip).clicked() {
                    let command = match kind {
                        "minimize" => "ui.window.minimize",
                        "maximize" => "ui.window.maximize",
                        _ => "ui.window.close",
                    };
                    let params = if kind == "maximize" { json!({"on": !maximized}) } else { json!({}) };
                    let _ = app.run(command, params);
                }
            }
        });
    });
    if maximized {
        return;
    }
    ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("dc_window_border"))).rect_stroke(
        screen,
        0.0,
        Stroke::new(1.0, t.border),
        egui::StrokeKind::Inside,
    );
    let Some(pos) = ctx.input(|i| i.pointer.latest_pos()) else { return };
    if !screen.contains(pos) {
        return;
    }
    let (band, corner) = if over_edge_control(ctx, screen, pos) { (RESIZE_GRIP, RESIZE_GRIP) } else { (RESIZE_BAND, RESIZE_CORNER) };
    let hit = resize_direction(screen, pos, band, corner).map(|direction| {
        let icon = match direction {
            ResizeDirection::North => CursorIcon::ResizeNorth,
            ResizeDirection::South => CursorIcon::ResizeSouth,
            ResizeDirection::East => CursorIcon::ResizeEast,
            ResizeDirection::West => CursorIcon::ResizeWest,
            ResizeDirection::NorthEast => CursorIcon::ResizeNorthEast,
            ResizeDirection::NorthWest => CursorIcon::ResizeNorthWest,
            ResizeDirection::SouthEast => CursorIcon::ResizeSouthEast,
            ResizeDirection::SouthWest => CursorIcon::ResizeSouthWest,
        };
        (direction, icon)
    });
    if let Some((direction, icon)) = hit {
        if !controls.contains(pos) {
            ctx.set_cursor_icon(icon);
        }
        if !controls.contains(pos) && ctx.input(|i| i.pointer.primary_pressed()) {
            let direction = match direction {
                ResizeDirection::North => "north",
                ResizeDirection::South => "south",
                ResizeDirection::East => "east",
                ResizeDirection::West => "west",
                ResizeDirection::NorthEast => "northeast",
                ResizeDirection::NorthWest => "northwest",
                ResizeDirection::SouthEast => "southeast",
                ResizeDirection::SouthWest => "southwest",
            };
            let _ = app.run("ui.window.beginResize", json!({"direction": direction}));
            app.flush_viewport_commands(ctx);
            handed_to_os(ctx);
            return;
        }
        app.flush_viewport_commands(ctx);
    }
}

fn resize_direction(screen: Rect, pos: egui::Pos2, band: f32, corner: f32) -> Option<ResizeDirection> {
    let near = |d: f32, limit: f32| d < limit;
    let (dl, dr, dt, db) = (pos.x - screen.min.x, screen.max.x - pos.x, pos.y - screen.min.y, screen.max.y - pos.y);
    let (w, e, n, s) = (near(dl, band), near(dr, band), near(dt, band), near(db, band));
    let (cw, ce, cn, cs) = (near(dl, corner), near(dr, corner), near(dt, corner), near(db, corner));
    if (n && cw) || (w && cn) {
        Some(ResizeDirection::NorthWest)
    } else if (n && ce) || (e && cn) {
        Some(ResizeDirection::NorthEast)
    } else if (s && cw) || (w && cs) {
        Some(ResizeDirection::SouthWest)
    } else if (s && ce) || (e && cs) {
        Some(ResizeDirection::SouthEast)
    } else if n {
        Some(ResizeDirection::North)
    } else if s {
        Some(ResizeDirection::South)
    } else if w {
        Some(ResizeDirection::West)
    } else if e {
        Some(ResizeDirection::East)
    } else {
        None
    }
}
