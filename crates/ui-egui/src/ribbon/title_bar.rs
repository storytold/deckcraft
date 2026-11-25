use super::{Align2, Icon, Rect, SlideApp, Tokens, Ui, icon_toggle, json, pos2, theme, vec2};

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
        #[cfg(target_os = "linux")]
        let rect = {
            craft_window::title_bar::empty_area(ui, rect).widget_info(|| egui::WidgetInfo::labeled(egui::Role::Window, true, "Window title bar"));
            app.captions
                .as_ref()
                .filter(|_| !ui.input(|i| i.viewport().fullscreen.unwrap_or(false)))
                .map_or(rect, |captions| linux::caption_strip(ui, rect, captions, app.caption_icon_theme.as_deref(), &t))
        };
        #[cfg(not(target_os = "linux"))]
        {
            // Dragging the empty title bar moves the window.
            let drag = ui.interact(rect, ui.scope_id().with("titledrag"), egui::Sense::click_and_drag());
            // The drag surface is not a button: a full-width Button would be announced as one.
            drag.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Window, true, "Window title bar"));
            if drag.drag_started() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            if drag.double_clicked() {
                let max = ui.input(|i| i.viewport().maximized.unwrap_or(false));
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
            }
        }
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_max(pos2(rect.min.x + left, rect.min.y), pos2(rect.min.x + 420.0, rect.max.y)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        child.shrink_clip_rect(rect);
        child.spacing_mut().item_spacing.x = 2.0;
        let has_doc = app.session.active().is_some();
        let dirty = app.session.active().is_some_and(|d| d.is_dirty());
        crate::qat::show(app, &mut child);
        let title =
            app.session.active().map(|d| format!("{}{}", d.title(), if dirty { " — Edited" } else { "" })).unwrap_or_else(|| "DeckCraft".into());
        // Right side.
        let mut right = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_max(pos2(rect.max.x - 320.0, rect.min.y), pos2(rect.max.x - 10.0, rect.max.y)))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        right.shrink_clip_rect(rect);
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
        // A long name must not paint over the quick-access actions or the window buttons.
        let room = Rect::from_min_max(pos2(child.min_rect().right() + 4.0, rect.top()), pos2(right.min_rect().left() - 4.0, rect.bottom()));
        if room.is_positive() {
            ui.painter().with_clip_rect(room).text(rect.center(), Align2::CENTER_CENTER, title, theme::font(13.0), t.text);
        }
    });
}

#[cfg(target_os = "linux")]
pub(super) mod linux {
    use super::super::linux_caption::Face;
    use super::{Rect, Tokens, Ui};
    use craft_window::{Button, policy::Captions};
    use egui::{CursorIcon, ResizeDirection, ViewportCommand};

    /// Put captions at their physical desktop edges and give the rest of the row to the ribbon.
    pub(super) fn caption_strip(ui: &mut Ui, bar: Rect, captions: &Captions, theme: Option<&str>, t: &Tokens) -> Rect {
        let face = Face::new(captions.family);
        let mut body = bar;
        let max = ui.input(|i| i.viewport().maximized.unwrap_or(false));
        for (left, buttons) in [(true, &captions.layout.left), (false, &captions.layout.right)] {
            for (i, &button) in buttons.iter().enumerate() {
                let (hit, center) = face.slot(bar, left, i, buttons.len());
                let (label, command) = match button {
                    Button::Close => ("Close", ViewportCommand::Close),
                    Button::Minimize => ("Minimize", ViewportCommand::Minimized(true)),
                    Button::Maximize => (if max { "Restore" } else { "Maximize" }, ViewportCommand::Maximized(!max)),
                };
                let resp = ui.interact(hit, ui.scope_id().with(("caption", left, i)), egui::Sense::click());
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
                face.paint(ui, &resp, center, (button, max), theme, (t.text, t.chrome));
                if resp.on_hover_text(label).clicked() {
                    ui.ctx().send_viewport_cmd(command);
                }
            }
            if left {
                body.min.x += face.width(buttons.len());
            } else {
                body.max.x -= face.width(buttons.len());
            }
        }
        body
    }

    pub(crate) fn resize_edges(ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().maximized == Some(true) || i.viewport().fullscreen == Some(true)) {
            return;
        }
        let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) else { return };
        let r = ctx.content_rect();
        let side = |p, min: f32, max: f32| {
            if p < min + 5.0 {
                -1
            } else if p > max - 5.0 {
                1
            } else {
                0
            }
        };
        let (direction, cursor) = match (side(pos.x, r.left(), r.right()), side(pos.y, r.top(), r.bottom())) {
            (-1, -1) => (ResizeDirection::NorthWest, CursorIcon::ResizeNwSe),
            (1, 1) => (ResizeDirection::SouthEast, CursorIcon::ResizeNwSe),
            (-1, 1) => (ResizeDirection::SouthWest, CursorIcon::ResizeNeSw),
            (1, -1) => (ResizeDirection::NorthEast, CursorIcon::ResizeNeSw),
            (-1, 0) => (ResizeDirection::West, CursorIcon::ResizeHorizontal),
            (1, 0) => (ResizeDirection::East, CursorIcon::ResizeHorizontal),
            (0, -1) => (ResizeDirection::North, CursorIcon::ResizeVertical),
            (0, 1) => (ResizeDirection::South, CursorIcon::ResizeVertical),
            _ => return,
        };
        ctx.set_cursor_icon(cursor);
        if ctx.input(|i| i.pointer.primary_pressed()) {
            ctx.send_viewport_cmd(ViewportCommand::BeginResize(direction));
        }
    }
}
