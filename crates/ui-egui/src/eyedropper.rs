//! The Eyedropper in the colour menus (Shape Fill, Shape Outline, Font Color…): choose it, then
//! click the slide to take the colour drawn under the pointer; Esc or a click elsewhere cancels.
//!
//! The menus are drawn by many callers that each apply the colour a swatch gives. The Eyedropper
//! therefore hands its colour back through the same menu: the menu closes while the pointer picks,
//! then opens again for one frame and `color_grid` returns the colour as if its swatch had been
//! clicked, so every caller applies it its own way.
//!
//! Picture Format ▸ Color ▸ Set Transparent Color uses the same pointer, as PowerPoint does: the
//! click names the colour of the selected picture under it, which turns transparent.

use egui::{Color32, Context, CursorIcon, Id, Pos2, Rect, Stroke, StrokeKind, Ui, pos2, vec2};

use crate::SlideApp;
use crate::canvas::Xf;
use crate::theme::{self, Tokens};

const REQUEST: &str = "deckcraft.eyedropper.request";
const TRANSPARENT: &str = "deckcraft.eyedropper.transparent";
const MENU: &str = "deckcraft.eyedropper.menu";
const RESULT: &str = "deckcraft.eyedropper.result";

/// Pixels per slide unit of the picture the Eyedropper looks at.
const SCALE: f64 = 2.0;

/// What a click with the pointer does.
#[derive(Clone, Copy)]
enum Pick {
    /// Hands the colour to the colour menu on this button.
    Menu(Id),
    /// Makes the colour of the selected picture under the pointer transparent.
    TransparentColor,
}

/// An Eyedropper waiting for a click: the slide as it is drawn and what the click is for.
pub struct State {
    pick: Pick,
    slide: usize,
    image: deckcraft_render::Image,
}

/// Derive a unique egui id for an eyedropper menu target name.
fn key(name: &str) -> Id {
    Id::unique(name)
}

/// The colour menu button whose menu is open (noted by `color_popup` every frame it is).
pub fn note_open_menu(ctx: &Context, button: Id) {
    ctx.data_mut(|d| d.insert_temp(key(MENU), button));
}

/// The menu button that is open now, if a menu noted itself.
pub fn open_menu(ctx: &Context) -> Option<Id> {
    ctx.data(|d| d.get_temp::<Id>(key(MENU)))
}

/// Keeps `hex` (`RRGGBB`) for the menu on `button`, which opens to hand it to its caller.
pub fn store_result(ctx: &Context, button: Id, hex: &str) {
    ctx.data_mut(|d| d.insert_temp(key(RESULT), Some((button, hex.to_string()))));
}

/// "Eyedropper" was chosen in the open menu.
pub fn request(ctx: &Context) {
    ctx.data_mut(|d| d.insert_temp(key(REQUEST), true));
}

/// Picture Format ▸ Color ▸ "Set Transparent Color" was chosen.
pub fn request_transparent_color(ctx: &Context) {
    ctx.data_mut(|d| d.insert_temp(key(TRANSPARENT), true));
}

/// Has the Eyedropper taken a colour for the menu on `button`?
pub fn result_waits_for(ctx: &Context, button: Id) -> bool {
    ctx.data(|d| d.get_temp::<Option<(Id, String)>>(key(RESULT))).flatten().is_some_and(|(b, _)| b == button)
}

/// The colour the Eyedropper took (`RRGGBB`), once.
pub fn take_result(ctx: &Context) -> Option<String> {
    let r = ctx.data(|d| d.get_temp::<Option<(Id, String)>>(key(RESULT))).flatten();
    if r.is_some() {
        ctx.data_mut(|d| d.insert_temp(key(RESULT), None::<(Id, String)>));
    }
    r.map(|(_, hex)| hex)
}

/// Once a frame: start the Eyedropper when a menu asked for it, stop it when something else is
/// clicked.
pub fn tick(app: &mut SlideApp, ctx: &Context) {
    if ctx.data_mut(|d| d.remove_temp::<bool>(key(REQUEST))).unwrap_or(false)
        && let Some(menu) = open_menu(ctx)
    {
        begin(app, Pick::Menu(menu));
    }
    if ctx.data_mut(|d| d.remove_temp::<bool>(key(TRANSPARENT))).unwrap_or(false) {
        begin(app, Pick::TransparentColor);
    }
    if app.eyedropper.is_some() {
        let outside =
            ctx.input(|i| i.pointer.primary_pressed() && i.pointer.hover_pos().is_some_and(|p| app.canvas_rect.is_none_or(|c| !c.contains(p))));
        if outside {
            app.eyedropper = None;
        }
    }
}

fn begin(app: &mut SlideApp, pick: Pick) {
    let Some(d) = app.session.active() else { return };
    if d.selection.target != deckcraft_engine::Target::Slides {
        app.set_status("The Eyedropper takes colours from a slide");
        return;
    }
    let slide = d.selection.slide;
    let image = deckcraft_render::render_slide(&d.doc, slide, &deckcraft_render::RenderOpts { scale: SCALE, ..Default::default() });
    app.eyedropper = Some(State { pick, slide, image });
    app.set_status(match pick {
        Pick::Menu(_) => "Eyedropper: click the slide (Esc cancels)",
        Pick::TransparentColor => "Set Transparent Color: click the colour in the picture (Esc cancels)",
    });
}

fn hex(c: [u8; 4]) -> String {
    format!("{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

/// The slide canvas while the Eyedropper waits: the pointer is a crosshair with the colour under it
/// beside it, a click takes it. Returns true when it used the pointer (the tools get none).
pub fn canvas(app: &mut SlideApp, ui: &Ui, resp: &egui::Response, xf: Xf) -> bool {
    let Some(state) = app.eyedropper.as_ref() else { return false };
    let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
    if escape || resp.secondary_clicked() {
        app.eyedropper = None;
        return true;
    }
    let Some(pos) = resp.hover_pos() else { return true };
    let on_slide = app.slide_rect.is_some_and(|r| r.contains(pos));
    if !on_slide {
        ui.ctx().set_cursor_icon(CursorIcon::NotAllowed);
        if resp.clicked() {
            app.eyedropper = None;
        }
        return true;
    }
    ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
    let p = xf.to_slide(pos);
    let under = state.image.straight_at((p.x * SCALE).max(0.0) as u32, (p.y * SCALE).max(0.0) as u32);
    if let Some(c) = under {
        loupe(ui, pos, c);
    }
    if resp.clicked() {
        let (pick, slide) = (state.pick, state.slide);
        app.eyedropper = None;
        match pick {
            Pick::Menu(menu) => {
                match app.session.execute("slide.colorAt", &serde_json::json!({"slide": slide, "x": p.x, "y": p.y, "scale": SCALE})) {
                    Ok(v) => {
                        if let Some(color) = v["color"].as_str() {
                            store_result(ui.ctx(), menu, color);
                        }
                    }
                    Err(e) => app.set_status(e.to_string()),
                }
            }
            Pick::TransparentColor => {
                if let Err(e) = app.session.execute("picture.transparentColor", &serde_json::json!({"at": [p.x, p.y]})) {
                    app.set_status(e.to_string());
                }
            }
        }
        ui.ctx().request_repaint();
    }
    true
}

/// The colour under the pointer, drawn beside it with its hex.
fn loupe(ui: &Ui, pos: Pos2, c: [u8; 4]) {
    let t = Tokens::get(ui.ctx());
    let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, key("deckcraft.eyedropper.loupe")));
    let at = pos + vec2(18.0, 18.0);
    let swatch = Rect::from_min_size(at, vec2(64.0, 26.0));
    painter.rect(swatch, 3, Color32::from_rgb(c[0], c[1], c[2]), Stroke::new(2.0, Color32::WHITE), StrokeKind::Outside);
    painter.rect_stroke(swatch.expand(2.0), 3, Stroke::new(1.0, t.border), StrokeKind::Outside);
    let label = Rect::from_min_size(pos2(at.x, swatch.max.y + 5.0), vec2(64.0, 16.0));
    painter.rect_filled(label, 3, Color32::WHITE);
    painter.text(label.center(), egui::Align2::CENTER_CENTER, format!("#{}", hex(c)), theme::bold(11.0), Color32::BLACK);
}
