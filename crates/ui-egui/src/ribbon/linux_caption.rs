//! Linux caption faces for the ribbon. Desktop detection and button order come from craft-window.
use craft_window::{Button, policy::CaptionFamily};
use egui::{Color32, ColorImage, Id, Rect, Response, Stroke, StrokeKind, TextureHandle, Ui, Vec2, pos2, vec2};
use std::io::Read as _;

pub(super) struct Face {
    side: f32,
    gap: f32,
    edge: f32,
    family: CaptionFamily,
}

impl Face {
    pub(super) fn new(family: CaptionFamily) -> Self {
        let (side, gap, edge) = if family == CaptionFamily::Breeze { (18.0, 4.5, 8.0) } else { (24.0, 6.5, 10.0) };
        Self { side, gap, edge, family }
    }

    pub(super) fn width(&self, n: usize) -> f32 {
        if n == 0 { 0.0 } else { n as f32 * (self.side + 2.0 * self.gap) + self.edge - self.gap }
    }

    /// The full-height outer hit target reaches the window edge; the disc stays inset.
    pub(super) fn slot(&self, bar: Rect, left: bool, i: usize, n: usize) -> (Rect, egui::Pos2) {
        let step = self.side + 2.0 * self.gap;
        let x = if left { bar.left() + self.edge } else { bar.right() - self.width(n) + self.gap };
        let center = pos2(x + i as f32 * step + self.side / 2.0, bar.center().y);
        let l = if left && i == 0 { bar.left() } else { center.x - step / 2.0 };
        let r = if !left && i + 1 == n { bar.right() } else { center.x + step / 2.0 };
        (Rect::from_min_max(pos2(l, bar.top()), pos2(r, bar.bottom())), center)
    }

    pub(super) fn paint(
        &self,
        ui: &Ui,
        response: &Response,
        center: egui::Pos2,
        icon: (Button, bool),
        theme: Option<&str>,
        colors: (Color32, Color32),
    ) {
        let (button, maximized) = icon;
        let (text, bar) = colors;
        let breeze = self.family == CaptionFamily::Breeze;
        let hot = response.hovered() || response.has_focus();
        let pressed = response.is_pointer_button_down_on();
        let fill = if breeze && (hot || pressed) {
            match (button, pressed) {
                (Button::Close, true) => Color32::from_rgb(109, 34, 41),
                (Button::Close, false) => Color32::from_rgb(255, 152, 162),
                (_, true) => text.gamma_multiply(0.5),
                _ => text,
            }
        } else {
            text.gamma_multiply(if pressed {
                0.30
            } else if hot {
                0.15
            } else if self.family == CaptionFamily::Adwaita {
                0.10
            } else {
                0.0
            })
        };
        let ink = if breeze && (hot || pressed) {
            bar
        } else if !ui.input(|i| i.focused) && !hot {
            text.gamma_multiply(0.65)
        } else {
            text
        };
        let p = ui.painter();
        p.circle_filled(center, self.side / 2.0, fill);
        let ppp = p.pixels_per_point();
        let center = pos2((center.x * ppp).round() / ppp, (center.y * ppp).round() / ppp);
        let name = match button {
            Button::Minimize => "window-minimize-symbolic",
            Button::Maximize if maximized => "window-restore-symbolic",
            Button::Maximize => "window-maximize-symbolic",
            Button::Close => "window-close-symbolic",
        };
        let px = (16.0 * ppp).round().clamp(1.0, 256.0) as u32;
        if let Some(texture) = theme.and_then(|theme| mask(ui, theme, name, px)) {
            p.image(
                texture.id(),
                Rect::from_center_size(center, Vec2::splat(px as f32 / ppp)),
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                ink,
            );
            return;
        }
        let at = |x, y| center + vec2(x, y);
        let stroke = Stroke::new(if breeze { 1.0 } else { 1.5 }, ink);
        match (button, breeze, maximized) {
            (Button::Close, _, _) => {
                p.line_segment([at(-4.0, -4.0), at(4.0, 4.0)], stroke);
                p.line_segment([at(4.0, -4.0), at(-4.0, 4.0)], stroke);
            }
            (Button::Maximize, true, true) => {
                p.add(egui::Shape::closed_line(vec![at(0.0, -5.0), at(5.0, 0.0), at(0.0, 5.0), at(-5.0, 0.0)], stroke));
            }
            (_, true, _) => {
                let dy = if button == Button::Minimize { 2.75 } else { -2.75 };
                p.line(vec![at(-5.5, -dy), at(0.0, dy), at(5.5, -dy)], stroke);
            }
            (Button::Minimize, false, _) => {
                p.rect_filled(Rect::from_min_max(at(-4.0, 2.0), at(4.0, 4.0)), 0, ink);
            }
            (Button::Maximize, false, _) => {
                let r = Rect::from_center_size(center, Vec2::splat(if maximized { 6.0 } else { 8.0 }));
                p.rect_stroke(r, 0, Stroke::new(2.0, ink), StrokeKind::Inside);
            }
        }
    }
}

/// Cache the alpha mask (including a miss) per context and device scale, independent of light/dark ink.
fn mask(ui: &Ui, theme: &str, name: &str, px: u32) -> Option<TextureHandle> {
    let id = Id::unique(("desktop-caption-mask", theme, name, px));
    if let Some(hit) = ui.data(|d| d.get_temp::<Option<TextureHandle>>(id)) {
        return hit;
    }
    let texture = std::panic::catch_unwind(|| load_mask(theme, name, px))
        .ok()
        .flatten()
        .map(|image| ui.ctx().load_texture(name, image, egui::TextureOptions::LINEAR));
    ui.data_mut(|d| d.insert_temp(id, texture.clone()));
    texture
}

fn load_mask(theme: &str, name: &str, px: u32) -> Option<ColorImage> {
    if !(1..=256).contains(&px) {
        return None;
    }
    let path = freedesktop_icons::lookup(name).with_theme(theme).with_size(16).force_svg().find()?;
    if path.extension()? != "svg" || !path.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    let file = std::fs::File::open(path).ok()?;
    file.take((1 << 20) + 1).read_to_end(&mut bytes).ok()?;
    rasterize(&bytes, px)
}

fn rasterize(bytes: &[u8], px: u32) -> Option<ColorImage> {
    use craft_svg::tree::{self, usvg};
    let opts = usvg::Options {
        image_href_resolver: usvg::ImageHrefResolver { resolve_data: Box::new(|_, _, _| None), resolve_string: Box::new(|_, _| None) },
        ..Default::default()
    };
    let limits = tree::Limits { bytes: 1 << 20, ..Default::default() };
    let tree = tree::parse(bytes, &limits, &opts).ok()?;
    let size = tree.size();
    let scale = px as f32 / size.width().max(size.height());
    let transform = resvg::tiny_skia::Transform::from_scale(scale, scale)
        .post_translate((px as f32 - size.width() * scale) / 2.0, (px as f32 - size.height() * scale) / 2.0);
    tree::draw::check(tree.root().children(), transform, px, px, 0, 16 << 20).ok()?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(px, px)?;
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    Some(ColorImage::new([px as usize; 2], pixmap.pixels().iter().map(|p| Color32::from_white_alpha(p.alpha())).collect()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn integrated_caption_uses_themes_alpha_not_the_svg_ink() {
        // A red mark in one corner represents a custom symbolic glyph. Its contour must
        // survive a change of DPI while its original colour must not override the button ink.
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path fill="red" d="M0 0h8v8H0z"/></svg>"#;
        for px in [16, 32] {
            let image = super::rasterize(svg, px).unwrap();
            assert_eq!(image[(1, 1)], egui::Color32::WHITE);
            assert_eq!(image[(px as usize - 1, 1)], egui::Color32::TRANSPARENT);
        }
    }
}
