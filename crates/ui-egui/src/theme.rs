//! Design tokens (light, PowerPoint-like; and dark), the interface themes built on them (high
//! contrast, tinted, custom) and fonts for the UI.

use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Stroke, Visuals};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Brightness {
    #[default]
    Light,
    Dark,
}

/// The interface theme. It colours DeckCraft's own chrome, in light or dark; slides keep their
/// presentation theme.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UiTheme {
    #[default]
    Standard,
    HighContrast,
    /// Blue-tinted surfaces with a green accent.
    Lagoon,
    /// Plum-tinted surfaces with a coral accent.
    Sunset,
    /// The user's own tint and accent ([`CustomTheme`]).
    Custom,
}

impl UiTheme {
    pub const ALL: [UiTheme; 5] = [UiTheme::Standard, UiTheme::HighContrast, UiTheme::Lagoon, UiTheme::Sunset, UiTheme::Custom];

    pub fn id(self) -> &'static str {
        match self {
            UiTheme::Standard => "standard",
            UiTheme::HighContrast => "highContrast",
            UiTheme::Lagoon => "lagoon",
            UiTheme::Sunset => "sunset",
            UiTheme::Custom => "custom",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            UiTheme::Standard => "Standard",
            UiTheme::HighContrast => "High Contrast",
            UiTheme::Lagoon => "Lagoon",
            UiTheme::Sunset => "Sunset",
            UiTheme::Custom => "Custom",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            UiTheme::Standard => "DeckCraft's neutral greys with an orange accent.",
            UiTheme::HighContrast => "Pure black and white with strong outlines, for low vision and bright rooms.",
            UiTheme::Lagoon => "A green-blue blend: blue-tinted panels with a green accent.",
            UiTheme::Sunset => "Warm plum-tinted panels with a coral accent.",
            UiTheme::Custom => "Your own tint and accent. Text colours adjust so they stay readable.",
        }
    }
}

/// The Custom theme's colours.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CustomTheme {
    /// The colour window surfaces are tinted towards.
    pub tint: [u8; 3],
    pub accent: [u8; 3],
    /// How strongly surfaces take the tint, 0–1.
    pub strength: f32,
}

impl Default for CustomTheme {
    fn default() -> Self {
        CustomTheme { tint: [0x2B, 0x6C, 0xB0], accent: [0x1E, 0x9E, 0x6A], strength: 0.5 }
    }
}

/// `#rrggbb` (the `#` is optional).
pub fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| h.get(i..i + 2).and_then(|b| u8::from_str_radix(b, 16).ok());
    Some([byte(0)?, byte(2)?, byte(4)?])
}

pub fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// WCAG 2 relative luminance.
pub fn luminance(c: Color32) -> f32 {
    let lin = |v: u8| {
        let s = v as f32 / 255.0;
        if s <= 0.040_45 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
}

/// WCAG 2 contrast ratio, 1–21.
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// `a` moved `t` (0–1) of the way to `b`, keeping `a`'s alpha.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
    let ch = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgba_unmultiplied(ch(a.r(), b.r()), ch(a.g(), b.g()), ch(a.b(), b.b()), a.a())
}

/// `c`, moved towards black or white just far enough to reach `ratio` against every background.
pub fn ensure_contrast(c: Color32, backgrounds: &[Color32], ratio: f32) -> Color32 {
    let worst = |c: Color32| backgrounds.iter().map(|b| contrast(c, *b)).fold(f32::INFINITY, f32::min);
    if worst(c) >= ratio {
        return c;
    }
    let target = if worst(Color32::WHITE) >= worst(Color32::BLACK) { Color32::WHITE } else { Color32::BLACK };
    (1..=20).map(|i| mix(c, target, i as f32 / 20.0)).find(|m| worst(*m) >= ratio).unwrap_or(target)
}

/// Every colour the chrome uses.
#[derive(Clone, Copy, Debug)]
pub struct Tokens {
    pub dark: bool,
    /// High contrast: widgets get visible outlines.
    pub high_contrast: bool,
    /// Window background (title bar, tab row, side panes).
    pub chrome: Color32,
    /// The ribbon card and popovers.
    pub card: Color32,
    pub card_border: Color32,
    /// Text fields. They have no outline, so the fill alone must set them apart from the card.
    pub text_field: Color32,
    /// Canvas around the slide.
    pub canvas: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_faint: Color32,
    pub border: Color32,
    pub separator: Color32,
    pub hover: Color32,
    pub pressed: Color32,
    pub selected: Color32,
    /// DeckCraft's accent (tabs underline, selected thumbnail, primary buttons).
    pub accent: Color32,
    pub accent_text: Color32,
    pub contextual: Color32,
    pub selection: Color32,
    pub handle_fill: Color32,
    pub handle_stroke: Color32,
    pub guide: Color32,
    pub caret: Color32,
    pub text_selection: Color32,
    pub field_bg: Color32,
    /// Unchecked checkbox and radio boxes and slider rails, drawn without an outline, so they
    /// must stand apart from the card and the chrome.
    pub control: Color32,
    /// [`Tokens::control`] under the pointer and pressed: a step darker each in light mode.
    pub control_hover: Color32,
    pub control_pressed: Color32,
    pub shadow: Color32,
}

impl Tokens {
    pub fn light() -> Self {
        Tokens {
            dark: false,
            high_contrast: false,
            chrome: Color32::from_rgb(0xF5, 0xF5, 0xF5),
            card: Color32::WHITE,
            card_border: Color32::from_rgb(0xE3, 0xE3, 0xE3),
            text_field: Color32::from_rgb(0xEB, 0xEB, 0xEB),
            canvas: Color32::from_rgb(0xF0, 0xF0, 0xF0),
            text: Color32::from_rgb(0x24, 0x24, 0x24),
            text_dim: Color32::from_rgb(0x5C, 0x5C, 0x5C),
            text_faint: Color32::from_rgb(0x99, 0x99, 0x99),
            border: Color32::from_rgb(0xD6, 0xD6, 0xD6),
            separator: Color32::from_rgb(0xE1, 0xE1, 0xE1),
            hover: Color32::from_rgb(0xEB, 0xEB, 0xEB),
            pressed: Color32::from_rgb(0xDE, 0xDE, 0xDE),
            selected: Color32::from_rgb(0xE6, 0xE6, 0xE6),
            accent: Color32::from_rgb(0xD9, 0x5A, 0x1A),
            accent_text: Color32::WHITE,
            contextual: Color32::from_rgb(0xC2, 0x4E, 0x14),
            selection: Color32::from_rgb(0x7A, 0x7A, 0x7A),
            handle_fill: Color32::WHITE,
            handle_stroke: Color32::from_rgb(0x6E, 0x6E, 0x6E),
            guide: Color32::from_rgb(0xE8, 0x5D, 0x2A),
            caret: Color32::BLACK,
            text_selection: Color32::from_rgba_unmultiplied(0x5A, 0x8F, 0xE0, 90),
            field_bg: Color32::WHITE,
            control: Color32::from_rgb(0xD6, 0xD6, 0xD6),
            control_hover: Color32::from_rgb(0xC8, 0xC8, 0xC8),
            control_pressed: Color32::from_rgb(0xBC, 0xBC, 0xBC),
            shadow: Color32::from_black_alpha(28),
        }
    }
    pub fn dark() -> Self {
        Tokens {
            dark: true,
            high_contrast: false,
            chrome: Color32::from_rgb(0x26, 0x26, 0x26),
            card: Color32::from_rgb(0x33, 0x33, 0x33),
            card_border: Color32::from_rgb(0x44, 0x44, 0x44),
            text_field: Color32::from_rgb(0x2A, 0x2A, 0x2A),
            canvas: Color32::from_rgb(0x1E, 0x1E, 0x1E),
            text: Color32::from_rgb(0xEE, 0xEE, 0xEE),
            text_dim: Color32::from_rgb(0xBB, 0xBB, 0xBB),
            text_faint: Color32::from_rgb(0x88, 0x88, 0x88),
            border: Color32::from_rgb(0x4A, 0x4A, 0x4A),
            separator: Color32::from_rgb(0x48, 0x48, 0x48),
            hover: Color32::from_rgb(0x40, 0x40, 0x40),
            pressed: Color32::from_rgb(0x4C, 0x4C, 0x4C),
            selected: Color32::from_rgb(0x47, 0x47, 0x47),
            accent: Color32::from_rgb(0xF2, 0x6B, 0x1D),
            accent_text: Color32::WHITE,
            contextual: Color32::from_rgb(0xF5, 0x8A, 0x4B),
            selection: Color32::from_rgb(0xAA, 0xAA, 0xAA),
            handle_fill: Color32::WHITE,
            handle_stroke: Color32::from_rgb(0x6E, 0x6E, 0x6E),
            guide: Color32::from_rgb(0xF2, 0x8A, 0x4B),
            caret: Color32::BLACK,
            text_selection: Color32::from_rgba_unmultiplied(0x5A, 0x8F, 0xE0, 90),
            field_bg: Color32::from_rgb(0x2A, 0x2A, 0x2A),
            control: Color32::from_rgb(0x2A, 0x2A, 0x2A),
            control_hover: Color32::from_rgb(0x40, 0x40, 0x40),
            control_pressed: Color32::from_rgb(0x4C, 0x4C, 0x4C),
            shadow: Color32::from_black_alpha(90),
        }
    }
    pub fn for_brightness(b: Brightness) -> Self {
        match b {
            Brightness::Light => Self::light(),
            Brightness::Dark => Self::dark(),
        }
    }
    /// The tokens for an interface theme in light or dark.
    pub fn build(theme: UiTheme, b: Brightness, custom: &CustomTheme) -> Self {
        let dark = b == Brightness::Dark;
        let rgb = |c: [u8; 3]| Color32::from_rgb(c[0], c[1], c[2]);
        match theme {
            UiTheme::Standard => Self::for_brightness(b),
            UiTheme::HighContrast => Self::high_contrast(dark),
            UiTheme::Lagoon => Self::tinted(dark, Color32::from_rgb(0x1F, 0x6F, 0xB8), Color32::from_rgb(0x12, 0x9E, 0x6C), 0.55),
            UiTheme::Sunset => Self::tinted(dark, Color32::from_rgb(0x9B, 0x3C, 0x8C), Color32::from_rgb(0xE8, 0x5C, 0x3A), 0.5),
            UiTheme::Custom => Self::tinted(dark, rgb(custom.tint), rgb(custom.accent), custom.strength),
        }
    }

    /// Black on white or white on black, with outlines; text reaches WCAG AAA (7:1) everywhere.
    pub fn high_contrast(dark: bool) -> Self {
        let rgb = Color32::from_rgb;
        if dark {
            Tokens {
                dark: true,
                high_contrast: true,
                chrome: Color32::BLACK,
                card: Color32::BLACK,
                card_border: Color32::WHITE,
                canvas: rgb(0x14, 0x14, 0x14),
                text: Color32::WHITE,
                text_dim: Color32::WHITE,
                text_faint: rgb(0xB0, 0xB0, 0xB0),
                border: Color32::WHITE,
                separator: rgb(0xC0, 0xC0, 0xC0),
                hover: rgb(0x1F, 0x2F, 0x6B),
                pressed: rgb(0x2E, 0x45, 0x99),
                selected: rgb(0x2E, 0x45, 0x99),
                accent: rgb(0xFF, 0xD4, 0x00),
                accent_text: Color32::BLACK,
                contextual: rgb(0x3F, 0xE8, 0xFF),
                selection: rgb(0x00, 0x9C, 0xFF),
                handle_fill: Color32::WHITE,
                handle_stroke: Color32::BLACK,
                guide: rgb(0xFF, 0x4F, 0xD8),
                caret: Color32::BLACK,
                text_selection: Color32::from_rgba_unmultiplied(0x00, 0x9C, 0xFF, 110),
                field_bg: Color32::BLACK,
                text_field: Color32::BLACK,
                control: Color32::from_gray(0x50),
                control_hover: Color32::from_gray(0x3C),
                control_pressed: Color32::from_gray(0x28),
                shadow: Color32::TRANSPARENT,
            }
        } else {
            Tokens {
                dark: false,
                high_contrast: true,
                chrome: Color32::WHITE,
                card: Color32::WHITE,
                card_border: Color32::BLACK,
                canvas: rgb(0xD4, 0xD4, 0xD4),
                text: Color32::BLACK,
                text_dim: Color32::BLACK,
                text_faint: rgb(0x59, 0x59, 0x59),
                border: Color32::BLACK,
                separator: rgb(0x4D, 0x4D, 0x4D),
                hover: rgb(0xD6, 0xE4, 0xFF),
                pressed: rgb(0xAE, 0xC8, 0xFF),
                selected: rgb(0xAE, 0xC8, 0xFF),
                accent: rgb(0x00, 0x37, 0xA6),
                accent_text: Color32::WHITE,
                contextual: rgb(0x7A, 0x00, 0x99),
                selection: rgb(0x00, 0x37, 0xA6),
                handle_fill: Color32::WHITE,
                handle_stroke: Color32::BLACK,
                guide: rgb(0xC4, 0x00, 0x7A),
                caret: Color32::BLACK,
                text_selection: Color32::from_rgba_unmultiplied(0x00, 0x5A, 0xE0, 90),
                field_bg: Color32::WHITE,
                text_field: Color32::WHITE,
                control: Color32::from_gray(0xB0),
                control_hover: Color32::from_gray(0xC8),
                control_pressed: Color32::from_gray(0xDC),
                shadow: Color32::TRANSPARENT,
            }
        }
    }

    /// The standard surfaces tinted towards `tint` (by `strength`, 0–1) with `accent` as the
    /// accent. Text is pushed towards black or white where the tint would make it hard to read.
    pub fn tinted(dark: bool, tint: Color32, accent: Color32, strength: f32) -> Self {
        let base = if dark { Self::dark() } else { Self::light() };
        let s = if strength.is_finite() { strength.clamp(0.0, 1.0) } else { 0.5 };
        let full = s * if dark { 0.42 } else { 0.24 };
        // Surfaces at tint amount `k`: chrome, card, canvas, field, hover, pressed, selected.
        let at = |k: f32| {
            let m = |c: Color32, f: f32| mix(c, tint, k * f);
            [
                m(base.chrome, 1.0),
                m(base.card, 0.6),
                m(base.canvas, 1.2),
                m(base.field_bg, 0.4),
                m(base.hover, 1.2),
                m(base.pressed, 1.3),
                m(base.selected, 1.3),
            ]
        };
        // A tint close to the text colour (white on dark, black on light) would wash the text
        // out: back off until the theme's own text colour still reads at 7:1 on every surface.
        let k = (0..=10).map(|i| full * (10 - i) as f32 / 10.0).find(|k| at(*k).iter().all(|bg| contrast(base.text, *bg) >= 7.0)).unwrap_or(0.0);
        let m = |c: Color32, f: f32| mix(c, tint, k * f);
        let [chrome, card, canvas, field_bg, hover, pressed, selected] = at(k);
        let surfaces = [chrome, card, canvas, field_bg];
        // The accent is also link and status text: 4.5:1 (WCAG AA) on every surface it sits on.
        let accent = ensure_contrast(accent, &surfaces, 4.5);
        let accent_text = if contrast(Color32::WHITE, accent) >= contrast(Color32::BLACK, accent) { Color32::WHITE } else { Color32::BLACK };
        Tokens {
            chrome,
            card,
            canvas,
            field_bg,
            card_border: m(base.card_border, 1.0),
            text: ensure_contrast(base.text, &[chrome, card, canvas, field_bg, hover, pressed, selected], 7.0),
            text_dim: ensure_contrast(base.text_dim, &surfaces, 4.5),
            text_faint: ensure_contrast(base.text_faint, &surfaces, 2.5),
            border: m(base.border, 1.0),
            separator: m(base.separator, 1.0),
            hover,
            pressed,
            selected,
            accent,
            accent_text,
            contextual: ensure_contrast(accent, &[chrome], 4.5),
            guide: accent,
            ..base
        }
    }

    /// The tokens last applied with [`apply`]; before that, light or dark from egui's visuals.
    pub fn get(ctx: &egui::Context) -> Self {
        ctx.data(|d| d.get_temp::<Tokens>(tokens_id()))
            .unwrap_or_else(|| if ctx.global_style().visuals.dark_mode { Self::dark() } else { Self::light() })
    }
}

fn tokens_id() -> egui::Id {
    egui::Id::new("deckcraft-ui-tokens")
}

/// Apply tokens to egui's style.
pub fn apply(ctx: &egui::Context, t: &Tokens) {
    ctx.data_mut(|d| d.insert_temp(tokens_id(), *t));
    let mut v = if t.dark { Visuals::dark() } else { Visuals::light() };
    v.panel_fill = t.chrome;
    v.window_fill = t.card;
    v.extreme_bg_color = t.field_bg;
    v.text_edit_bg_color = Some(t.text_field);
    v.faint_bg_color = t.hover;
    v.window_stroke = Stroke::new(1.0, t.card_border);
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    v.selection.bg_fill = t.accent.gamma_multiply(0.85);
    v.selection.stroke = Stroke::new(1.0, t.accent_text);
    v.hyperlink_color = t.accent;
    v.override_text_color = Some(t.text);
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = CornerRadius::same(5);
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, t.separator);
    v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    v.widgets.inactive.bg_fill = t.control;
    v.widgets.inactive.bg_stroke = Stroke::NONE;
    v.widgets.hovered.weak_bg_fill = t.hover;
    v.widgets.hovered.bg_fill = t.control_hover;
    v.widgets.hovered.bg_stroke = Stroke::NONE;
    v.widgets.active.weak_bg_fill = t.pressed;
    v.widgets.active.bg_fill = t.control_pressed;
    v.widgets.open.weak_bg_fill = t.pressed;
    if t.high_contrast {
        // Every control shows its edge, and the one under the pointer or keyboard focus is
        // outlined in the accent.
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, t.border);
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, t.border);
        v.widgets.hovered.bg_stroke = Stroke::new(2.0, t.accent);
        v.widgets.active.bg_stroke = Stroke::new(2.0, t.accent);
        v.widgets.open.bg_stroke = Stroke::new(1.0, t.accent);
        // Slider rails and check boxes: a grey that shows against the background and still
        // carries the text colour's tick at 7:1.
        v.widgets.inactive.bg_fill = if t.dark { Color32::from_gray(0x50) } else { Color32::from_gray(0xB0) };
        for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
            w.fg_stroke.color = t.text;
        }
        v.selection.stroke = Stroke::new(1.5, t.accent_text);
    }
    v.window_shadow = egui::epaint::Shadow { offset: [0, 4], blur: 18, spread: 0, color: t.shadow };
    v.popup_shadow = egui::epaint::Shadow { offset: [0, 3], blur: 12, spread: 0, color: t.shadow };
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(6.0, 4.0);
        s.spacing.button_padding = egui::vec2(6.0, 3.0);
        s.spacing.interact_size.y = 22.0;
        s.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(13.0));
        s.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(13.0));
        s.text_styles.insert(egui::TextStyle::Small, egui::FontId::proportional(11.0));
        s.text_styles.insert(egui::TextStyle::Heading, egui::FontId::proportional(18.0));
    });
}

/// UI fonts: Inter (from craft-fonts) when embedded, a Japanese UI face for CJK, then egui's.
pub fn install_fonts(ctx: &egui::Context) {
    install_fonts_with(ctx, &[]);
}

/// [`install_fonts`], with `extra` faces from the font database after every interface font: the
/// installed or embedded faces that draw text the interface faces lack (Hangul, Simplified
/// Chinese, Devanagari…, #101). See [`crate::uifonts`].
pub fn install_fonts_with(ctx: &egui::Context, extra: &[std::sync::Arc<deckcraft_fonts::FontFace>]) {
    ctx.set_fonts(font_definitions(extra));
}

/// The interface font definitions; see [`install_fonts_with`].
pub fn font_definitions(extra: &[std::sync::Arc<deckcraft_fonts::FontFace>]) -> FontDefinitions {
    let mut defs = FontDefinitions::default();
    let mut names = vec![];
    for (i, f) in deckcraft_fonts::CRAFT_FONTS.iter().enumerate() {
        let wanted = (f.family == "Inter" && (f.style == "Regular" || f.style == "SemiBold" || f.style == "Bold"))
            || (f.family == "BIZ UDPGothic" && f.style == "Regular")
            || (f.family == "Noto Sans Arabic");
        if !wanted {
            continue;
        }
        let name = format!("craft-{i}-{}-{}", f.family, f.style);
        defs.font_data.insert(name.clone(), std::sync::Arc::new(FontData::from_static(f.bytes)));
        names.push((f.family, f.style, name));
    }
    let regular: Vec<String> = names.iter().filter(|(fam, st, _)| !(*fam == "Inter" && *st != "Regular")).map(|(_, _, n)| n.clone()).collect();
    if let Some(list) = defs.families.get_mut(&FontFamily::Proportional) {
        for (k, n) in regular.iter().enumerate() {
            list.insert(k, n.clone());
        }
    }
    // A bold family for headings and selected tabs.
    let mut bold: Vec<String> = names.iter().filter(|(fam, st, _)| *fam == "Inter" && *st == "SemiBold").map(|(_, _, n)| n.clone()).collect();
    bold.extend(defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default());
    defs.families.insert(FontFamily::Name("bold".into()), bold);
    add_fallback_faces(&mut defs, extra);
    // The Japanese interface face (the first one when Inter isn't embedded) hints its en and em
    // dashes, bars thinner than a pixel, down to nothing, so they drew as gaps in interface text;
    // unhinted, they draw.
    for (name, d) in defs.font_data.iter_mut() {
        if name.contains("BIZ UDPGothic") {
            let mut data = (**d).clone();
            data.tweak.hinting = Some(false);
            *d = std::sync::Arc::new(data);
        }
    }
    defs
}

/// Add `faces` after every other font of each interface family (the last fallbacks).
pub fn add_fallback_faces(defs: &mut FontDefinitions, faces: &[std::sync::Arc<deckcraft_fonts::FontFace>]) {
    for f in faces {
        let name = format!("fallback {} {} {}", f.family, f.style, f.id());
        if defs.font_data.contains_key(&name) {
            continue;
        }
        let mut data = FontData::from_owned(f.data().to_vec());
        data.index = f.index();
        defs.font_data.insert(name.clone(), std::sync::Arc::new(data));
        for list in defs.families.values_mut() {
            list.push(name.clone());
        }
    }
}

pub fn bold(size: f32) -> egui::FontId {
    egui::FontId::new(size, FontFamily::Name("bold".into()))
}

pub fn font(size: f32) -> egui::FontId {
    egui::FontId::proportional(size)
}

pub fn to_color32(c: deckcraft_color::Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each character of `text` laid out in the interface fonts at interface sizes, and whether it
    /// has ink (a glyph image).
    fn drawn(text: &str) -> Vec<(char, f32, bool)> {
        let ctx = egui::Context::default();
        ctx.set_fonts(super::font_definitions(&[]));
        let mut o = ctx.run_ui(Default::default(), |_| {});
        o.textures_delta.clear();
        let mut out = vec![];
        ctx.fonts_mut(|f| {
            for size in [11.0, 12.0, 13.0, 16.0] {
                for family in [egui::FontFamily::Proportional, egui::FontFamily::Name("bold".into())] {
                    let g = f.layout_no_wrap(text.to_string(), egui::FontId::new(size, family), egui::Color32::WHITE);
                    for gl in g.rows.iter().flat_map(|r| &r.glyphs) {
                        out.push((gl.chr, size, gl.uv_rect.size.x > 0.0 && gl.uv_rect.size.y > 0.0));
                    }
                }
            }
        });
        let mut o = ctx.run_ui(Default::default(), |_| {});
        o.textures_delta.clear();
        out
    }

    #[test]
    fn light_checkboxes_and_slider_rails_stand_out_from_their_background() {
        // An unchecked checkbox and a slider rail are filled with the inactive widget fill and
        // have no outline, so in light mode they vanished into the white dialog (#766).
        let ctx = egui::Context::default();
        super::apply(&ctx, &super::Tokens::light());
        let v = ctx.global_style().visuals.clone();
        assert_eq!(v.widgets.inactive.bg_stroke, egui::Stroke::NONE, "the test assumes no outline");
        let fill = v.widgets.inactive.bg_fill;
        for (name, bg) in [("dialog", v.window_fill), ("panel", v.panel_fill)] {
            let diff = [fill.r().abs_diff(bg.r()), fill.g().abs_diff(bg.g()), fill.b().abs_diff(bg.b())];
            assert!(diff.iter().all(|d| *d >= 24), "control fill {fill:?} is too close to the {name} background {bg:?}");
        }
        // Hovering and pressing darken the control a step each, as they do buttons.
        let (hovered, active) = (v.widgets.hovered.bg_fill, v.widgets.active.bg_fill);
        assert!(fill.r() > hovered.r() && hovered.r() > active.r(), "idle {fill:?}, hovered {hovered:?}, active {active:?} don't darken in turn");
    }

    #[test]
    fn thin_punctuation_is_drawn_in_interface_text() {
        // En and em dashes (thin bars) were hinted down to nothing in the Japanese interface face,
        // so notes and the outline showed gaps where they were.
        for (c, size, ink) in drawn("\u{2013}\u{2014}-_\u{2026}\u{2022}\u{00b7}\u{2018}\u{2019}\u{201c}\u{201d}=~") {
            assert!(ink, "{c:?} ({:04X}) draws nothing at {size} px", c as u32);
        }
    }

    fn all_tokens() -> Vec<(UiTheme, Brightness, Tokens)> {
        let custom = CustomTheme::default();
        UiTheme::ALL.into_iter().flat_map(|th| [Brightness::Light, Brightness::Dark].map(|b| (th, b, Tokens::build(th, b, &custom)))).collect()
    }

    #[test]
    fn standard_is_unchanged() {
        let c = CustomTheme::default();
        assert_eq!(format!("{:?}", Tokens::build(UiTheme::Standard, Brightness::Light, &c)), format!("{:?}", Tokens::light()));
        assert_eq!(format!("{:?}", Tokens::build(UiTheme::Standard, Brightness::Dark, &c)), format!("{:?}", Tokens::dark()));
    }

    #[test]
    fn every_theme_keeps_text_readable() {
        for (th, b, t) in all_tokens() {
            assert_eq!(t.dark, b == Brightness::Dark, "{th:?} {b:?}");
            for (name, bg) in [("chrome", t.chrome), ("card", t.card), ("canvas", t.canvas), ("field", t.field_bg)] {
                assert!(contrast(t.text, bg) >= 4.5, "{th:?} {b:?}: text on {name} is {:.2}:1", contrast(t.text, bg));
                assert!(contrast(t.text_dim, bg) >= 4.5, "{th:?} {b:?}: dim text on {name} is {:.2}:1", contrast(t.text_dim, bg));
            }
            assert!(contrast(t.accent_text, t.accent) >= 3.0, "{th:?} {b:?}: button text on the accent");
            // The tinted themes' accent is also link and status text.
            if matches!(th, UiTheme::Lagoon | UiTheme::Sunset | UiTheme::Custom) {
                for (name, bg) in [("chrome", t.chrome), ("card", t.card), ("canvas", t.canvas), ("field", t.field_bg)] {
                    assert!(contrast(t.accent, bg) >= 4.5, "{th:?} {b:?}: accent on {name} is {:.2}:1", contrast(t.accent, bg));
                }
            }
        }
    }

    #[test]
    fn high_contrast_reaches_aaa() {
        for dark in [false, true] {
            let t = Tokens::high_contrast(dark);
            assert!(t.high_contrast && t.dark == dark);
            for bg in [t.chrome, t.card, t.canvas, t.field_bg, t.hover, t.pressed, t.selected] {
                assert!(contrast(t.text, bg) >= 7.0, "dark={dark}: {:.2}:1", contrast(t.text, bg));
            }
            assert!(contrast(t.accent_text, t.accent) >= 7.0);
            // Check boxes, slider rails and their hovered and pressed fills carry the text at 7:1.
            for (name, fill) in [("control", t.control), ("hovered", t.control_hover), ("pressed", t.control_pressed)] {
                assert!(contrast(t.text, fill) >= 7.0, "dark={dark}: text on the {name} control is {:.2}:1", contrast(t.text, fill));
            }
            assert!(contrast(t.border, t.chrome) >= 7.0, "outlines stand out");
            assert!(contrast(t.text_faint, t.chrome) >= 4.5, "even disabled text is legible");
        }
    }

    #[test]
    fn custom_colours_never_make_text_unreadable() {
        // Worst cases: a tint the same as the text, at full strength.
        for tint in [[0, 0, 0], [255, 255, 255], [128, 128, 128], [255, 255, 0], [0, 0, 255]] {
            for accent in [[255, 255, 255], [0, 0, 0], [255, 255, 0]] {
                let c = CustomTheme { tint, accent, strength: 1.0 };
                for b in [Brightness::Light, Brightness::Dark] {
                    let t = Tokens::build(UiTheme::Custom, b, &c);
                    for bg in [t.chrome, t.card, t.canvas, t.hover, t.selected] {
                        assert!(contrast(t.text, bg) >= 7.0, "tint {tint:?} {b:?}: {:.2}:1", contrast(t.text, bg));
                    }
                    assert!(contrast(t.accent_text, t.accent) >= 3.0, "tint {tint:?} accent {accent:?} {b:?}");
                    for bg in [t.chrome, t.card, t.canvas, t.field_bg] {
                        assert!(contrast(t.accent, bg) >= 4.5, "tint {tint:?} accent {accent:?} {b:?}: link {:.2}:1", contrast(t.accent, bg));
                    }
                }
            }
        }
        // Hostile strengths fall back instead of producing NaN colours.
        for s in [f32::NAN, f32::INFINITY, -3.0, 40.0] {
            let t = Tokens::build(UiTheme::Custom, Brightness::Dark, &CustomTheme { strength: s, ..Default::default() });
            assert!(contrast(t.text, t.chrome) >= 7.0);
        }
    }

    #[test]
    fn contrast_matches_wcag() {
        assert!((contrast(Color32::BLACK, Color32::WHITE) - 21.0).abs() < 0.01);
        assert!((contrast(Color32::WHITE, Color32::WHITE) - 1.0).abs() < 1e-4);
        // #767676 on white is the classic 4.54:1.
        assert!((contrast(Color32::from_gray(0x76), Color32::WHITE) - 4.54).abs() < 0.01);
        let c = ensure_contrast(Color32::from_gray(0xC0), &[Color32::WHITE], 4.5);
        assert!(contrast(c, Color32::WHITE) >= 4.5 && luminance(c) < luminance(Color32::from_gray(0xC0)));
    }

    #[test]
    fn hex_colours_parse_strictly() {
        assert_eq!(parse_hex("#1f6FB8"), Some([0x1F, 0x6F, 0xB8]));
        assert_eq!(parse_hex("00ff80"), Some([0, 255, 128]));
        for bad in ["", "#", "#12345", "#1234567", "#gg0000", "#+1+2+3", "#éé0", "#ééé"] {
            assert_eq!(parse_hex(bad), None, "{bad}");
        }
        assert_eq!(hex([0x1F, 0x6F, 0xB8]), "#1f6fb8");
        for id in UiTheme::ALL {
            assert_eq!(UiTheme::from_id(id.id()), Some(id));
        }
    }

    #[test]
    fn get_returns_the_applied_theme() {
        let ctx = egui::Context::default();
        assert!(!Tokens::get(&ctx).high_contrast);
        apply(&ctx, &Tokens::high_contrast(true));
        let t = Tokens::get(&ctx);
        assert!(t.high_contrast && t.dark && ctx.global_style().visuals.dark_mode);
        assert_eq!(t.accent, Tokens::high_contrast(true).accent);
    }
}
