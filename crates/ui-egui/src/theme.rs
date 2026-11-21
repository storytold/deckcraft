//! Design tokens (light, PowerPoint-like; and dark) and fonts for the UI.

use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Stroke, Visuals};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Brightness {
    #[default]
    Light,
    Dark,
}

/// Every colour the chrome uses.
#[derive(Clone, Copy, Debug)]
pub struct Tokens {
    pub dark: bool,
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
    /// Canvas shell panel: number, colour and dropdown fields and segmented controls.
    pub field: Color32,
    /// Canvas shell panel: the small labels above fields.
    pub caption: Color32,
    /// A field's outline under the pointer, and the ring around the one being typed in (at least
    /// 3:1 against the panel), as in FigCraft's UI3 tokens.
    pub field_hover_border: Color32,
    pub focus: Color32,
    /// Canvas shell lists (Layers): a selected row's highlight, blue as in canvas editors.
    pub selected_row: Color32,
}

impl Tokens {
    pub fn light() -> Self {
        Tokens {
            dark: false,
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
            field: Color32::from_rgb(0xF5, 0xF5, 0xF5),
            caption: Color32::from_rgb(0x80, 0x80, 0x80),
            field_hover_border: Color32::from_rgb(0xE0, 0xE0, 0xE0),
            focus: Color32::from_rgb(0xB8, 0x46, 0x0F),
            selected_row: Color32::from_rgb(0xE5, 0xF4, 0xFF),
        }
    }
    pub fn dark() -> Self {
        Tokens {
            dark: true,
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
            field: Color32::from_rgb(0x3D, 0x3D, 0x3D),
            caption: Color32::from_rgb(0xA6, 0xA6, 0xA6),
            field_hover_border: Color32::from_rgb(0x4D, 0x4D, 0x4D),
            focus: Color32::from_rgb(0xF5, 0x8A, 0x4B),
            selected_row: Color32::from_rgb(0x2A, 0x45, 0x5C),
        }
    }
    pub fn for_brightness(b: Brightness) -> Self {
        match b {
            Brightness::Light => Self::light(),
            Brightness::Dark => Self::dark(),
        }
    }
    pub fn get(ctx: &egui::Context) -> Self {
        if ctx.global_style().visuals.dark_mode { Self::dark() } else { Self::light() }
    }
}

/// Apply tokens to egui's style.
pub fn apply(ctx: &egui::Context, t: &Tokens) {
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

/// Inter, the interface face: regular, medium and semibold (SIL OFL 1.1, `assets/fonts`).
const BUNDLED_INTER: [(&str, &[u8]); 3] = [
    ("Inter Regular", include_bytes!("../../../assets/fonts/Inter-Regular.ttf")),
    ("Inter Medium", include_bytes!("../../../assets/fonts/Inter-Medium.ttf")),
    ("Inter SemiBold", include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf")),
];

/// UI fonts: Inter (bundled), a Japanese UI face for CJK, then egui's.
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
            // Exact craft-fonts family names: the Korean face is "NanumGothic" (no space), and
            // the symbol faces are "Noto Sans Symbols" / "Noto Sans Symbols 2". A near-miss here
            // would leave Hangul and the ◉ laser toggle without a glyph. Hebrew backs the
            // Language dialog's native "עברית" label where no system font supplies it.
            || matches!(f.family, "Noto Sans Arabic" | "Noto Sans Hebrew" | "NanumGothic" | "Noto Sans Symbols" | "Noto Sans Symbols 2")
            // This opt-in native face follows the existing families. Keep per-character
            // fallback and Latin metrics; no Chinese payload enters the wasm allowlist.
            || (cfg!(not(target_arch = "wasm32")) && f.family == "LXGW WenKai Lite" && f.style == "Regular");
        if !wanted {
            continue;
        }
        let name = format!("craft-{i}-{}-{}", f.family, f.style);
        defs.font_data.insert(name.clone(), std::sync::Arc::new(FontData::from_static(f.bytes)));
        names.push((f.family, f.style, name));
    }
    // Inter ships with DeckCraft (assets/fonts), so the interface looks the same whether or not
    // the craft-fonts checkout has it.
    for (name, bytes) in BUNDLED_INTER {
        defs.font_data.insert(name.to_string(), std::sync::Arc::new(FontData::from_static(bytes)));
    }
    let mut regular: Vec<String> = vec![BUNDLED_INTER[0].0.to_string()];
    regular.extend(names.iter().filter(|(fam, _, _)| *fam != "Inter").map(|(_, _, n)| n.clone()));
    if let Some(list) = defs.families.get_mut(&FontFamily::Proportional) {
        for (k, n) in regular.iter().enumerate() {
            list.insert(k, n.clone());
        }
    }
    let rest = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    // Semibold for headings and selected tabs, medium for labels that stand out a little.
    for (family, face) in [("bold", BUNDLED_INTER[2].0), ("medium", BUNDLED_INTER[1].0)] {
        let mut list = vec![face.to_string()];
        list.extend(rest.iter().filter(|n| *n != BUNDLED_INTER[0].0).cloned());
        defs.families.insert(FontFamily::Name(family.into()), list);
    }
    // Hack is already bundled for Monospace and covers the interface's arrows and triangles.
    // Keep it after every proportional face so ordinary text metrics stay unchanged.
    if defs.font_data.contains_key("Hack") {
        for chain in defs.families.values_mut() {
            if !chain.iter().any(|name| name == "Hack") {
                chain.push("Hack".into());
            }
        }
    }
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

/// Inter Medium: field values and labels a little stronger than body text.
pub fn medium(size: f32) -> egui::FontId {
    egui::FontId::new(size, FontFamily::Name("medium".into()))
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
        // A replacement glyph also gets a nonempty atlas rectangle. Fail before layout when no
        // font can draw a character, so `uv_rect` cannot mistake tofu for real ink.
        ctx.set_missing_glyph_policy(egui::MissingGlyphPolicy::Panic);
        ctx.set_fonts(super::font_definitions(&[]));
        let mut o = ctx.run_ui(Default::default(), |_| {});
        o.textures_delta.clear();
        let mut out = vec![];
        ctx.fonts_mut(|f| {
            for size in [11.0, 12.0, 13.0, 16.0] {
                for family in [egui::FontFamily::Proportional, egui::FontFamily::Name("bold".into()), egui::FontFamily::Name("medium".into())] {
                    let font = egui::FontId::new(size, family);
                    for c in text.chars() {
                        assert!(f.has_glyph(&font, c), "no font supplies {c:?} at {size} px in {font:?}");
                    }
                    let g = f.layout_no_wrap(text.to_string(), font, egui::Color32::WHITE);
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

    /// The UI font library supplies default text fonts and an explicit monochrome emoji face,
    /// independent of kittest defaults.
    #[test]
    fn the_ui_library_has_default_fonts_and_explicit_monochrome_emoji_for_tests() {
        // `egui`'s `default_fonts` is no longer a default feature and kittest's own dev
        // dependencies do not propagate. This library enables it (and the test build enables
        // `monochrome_emoji_fonts`) explicitly.
        let defs = FontDefinitions::default();
        assert!(defs.font_data.contains_key("Hack"), "default_fonts must be enabled by deckcraft-ui-egui itself");
        assert!(defs.font_data.contains_key("Ubuntu-Light"));
        assert!(defs.font_data.contains_key("NotoEmoji-Regular"), "monochrome_emoji_fonts is an explicit test feature");
        assert!(defs.families.contains_key(&FontFamily::Proportional));
        // The production font policy still owns the UI families: craft faces first, then egui's.
        let ours = super::font_definitions(&[]);
        assert!(ours.families.contains_key(&FontFamily::Name("bold".into())));
        assert!(ours.font_data.contains_key("Hack"));
    }

    /// When the Chinese face is present (required builds guarantee it), exactly one LXGW WenKai
    /// Lite Regular is embedded, and Chinese labels have glyph coverage and ink; builds without the
    /// optional face skip this test.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn native_chinese_fallback_draws_initial_document_labels() {
        let faces: Vec<_> = deckcraft_fonts::CRAFT_FONTS.iter().filter(|f| f.family == "LXGW WenKai Lite").collect();
        if faces.is_empty() {
            assert!(option_env!("CRAFT_FONTS_REQUIRED").is_none(), "required native font input must include the reviewed Chinese fallback");
            eprintln!("optional Chinese input absent: bundled Chinese coverage was not qualified");
            return;
        }
        assert_eq!(faces.len(), 1);
        assert_eq!(faces[0].style, "Regular");
        assert_eq!(faces[0].bytes.len(), 13_949_472);
        let face = skrifa::FontRef::new(faces[0].bytes).unwrap();
        use skrifa::MetadataProvider as _;
        assert!(face.charmap().map('简').is_some(), "the approved fallback supplies the original missing glyph");
        let defs = font_definitions(&[]);
        for family in [FontFamily::Proportional, FontFamily::Name("bold".into())] {
            let chain = &defs.families[&family];
            let chinese = chain.iter().position(|name| name.contains("LXGW WenKai Lite")).unwrap();
            let japanese = chain.iter().position(|name| name.contains("BIZ UDPGothic")).unwrap();
            assert!(japanese < chinese, "retain existing glyph selection and Latin metrics");
            assert_eq!(chain.iter().filter(|name| name.contains("LXGW WenKai Lite")).count(), 1);
        }
        let text = "简体中文演示文稿";
        let ctx = egui::Context::default();
        ctx.set_missing_glyph_policy(egui::MissingGlyphPolicy::Panic);
        ctx.set_fonts(defs);
        let mut output = ctx.run_ui(Default::default(), |_| {});
        output.textures_delta.clear();
        ctx.fonts_mut(|fonts| {
            for family in [FontFamily::Proportional, FontFamily::Name("bold".into())] {
                let font = egui::FontId::new(13.0, family);
                for c in text.chars() {
                    assert!(fonts.has_glyph(&font, c), "the embedded fallback does not supply {c:?} in {font:?}");
                }
            }
        });
        let ink = drawn(text);
        assert_eq!(ink.len(), text.chars().count() * 4 * 2);
        for (c, size, visible) in ink {
            assert!(visible, "Chinese label {c:?} has no ink at {size} px");
        }
    }

    /// When the face is present (required builds guarantee it), the native Hebrew fallback is exactly
    /// one static Regular Noto Sans Hebrew face with glyph coverage and ink for language labels.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn native_hebrew_fallback_is_one_regular_face_and_draws_language_labels() {
        let faces: Vec<_> = deckcraft_fonts::CRAFT_FONTS.iter().filter(|f| f.family == "Noto Sans Hebrew").collect();
        if faces.is_empty() {
            assert!(option_env!("CRAFT_FONTS_REQUIRED").is_none(), "required native font input must include Noto Sans Hebrew Regular");
            eprintln!("optional font input absent: native Hebrew coverage was not qualified");
            return;
        }
        assert_eq!(faces.len(), 1, "one static face, without variable named instances");
        assert_eq!(faces[0].style, "Regular");
        assert_eq!(faces[0].bytes.len(), 66_156, "use the approved full static Regular face");
        use skrifa::raw::{ReadError, TableProvider};
        let face = skrifa::FontRef::new(faces[0].bytes).unwrap();
        assert_eq!(face.os2().unwrap().us_weight_class(), 400, "the embedded bytes must be Regular, not a renamed Thin face");
        assert!(matches!(face.fvar(), Err(ReadError::TableIsMissing(_))), "embed one static face, not variable named instances");
        let defs = super::font_definitions(&[]);
        let names: Vec<_> = defs.font_data.keys().filter(|name| name.contains("Noto Sans Hebrew")).collect();
        assert_eq!(names.len(), 1);
        for family in [FontFamily::Proportional, FontFamily::Name("bold".into())] {
            assert_eq!(defs.families[&family].iter().filter(|name| *name == names[0]).count(), 1);
        }
        let ctx = egui::Context::default();
        ctx.set_fonts(defs);
        let mut output = ctx.run_ui(Default::default(), |_| {});
        output.textures_delta.clear();
        ctx.fonts_mut(|fonts| {
            for family in [FontFamily::Proportional, FontFamily::Name("bold".into())] {
                let font = egui::FontId::new(13.0, family);
                for c in "עבריתשלוםdeck42".chars() {
                    assert!(fonts.has_glyph(&font, c), "missing language-label glyph {c:?}");
                }
            }
        });
        // This checks ink and coverage only, not bidi ordering or GPOS mark positioning.
        for (c, size, ink) in drawn("עבריתשלוםdeck42") {
            assert!(ink, "{c:?} has no ink at {size} px");
        }
    }

    /// Hebrew fallback preserves logical-order editing in mixed-direction text.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn native_hebrew_fallback_keeps_mixed_text_editing_in_logical_order() {
        if !deckcraft_fonts::CRAFT_FONTS.iter().any(|f| f.family == "Noto Sans Hebrew") {
            assert!(option_env!("CRAFT_FONTS_REQUIRED").is_none(), "required native Hebrew face is absent");
            return;
        }
        let ctx = egui::Context::default();
        ctx.set_fonts(super::font_definitions(&[]));
        let id = egui::Id::unique("mixed-hebrew-label-edit");
        let mut text = "deck שלום 42".to_owned();
        let frame = |text: &mut String, events: Vec<egui::Event>| {
            let output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(500.0, 100.0))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    ui.memory_mut(|m| m.request_focus(id));
                    ui.add(egui::TextEdit::singleline(text).id(id));
                },
            );
            output.drop_without_applying_deltas();
        };
        frame(&mut text, vec![]);
        // Character offsets remain logical even though the Hebrew glyphs are drawn right-to-left.
        let mut state = egui::TextEdit::load_state(&ctx, id).unwrap();
        state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(7))));
        egui::TextEdit::store_state(&ctx, id, state);
        frame(&mut text, vec![egui::Event::Text("א".into())]);
        assert_eq!(text, "deck שלאום 42");
        let state = egui::TextEdit::load_state(&ctx, id).unwrap();
        assert_eq!(state.cursor.char_range().unwrap().primary.index.0, 8);
        frame(
            &mut text,
            vec![egui::Event::Key { key: egui::Key::Backspace, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }],
        );
        assert_eq!(text, "deck שלום 42");
        ctx.fonts_mut(|fonts| {
            let galley = fonts.layout_no_wrap(text.clone(), egui::FontId::proportional(13.0), egui::Color32::WHITE);
            let row = &galley.rows[0];
            let x = |c| row.glyphs.iter().find(|g| g.chr == c).unwrap().pos.x;
            assert!(x('ש') > x('ל') && x('ל') > x('ו') && x('ו') > x('ם'), "Hebrew paints in visual RTL order");
            assert!(x('4') < x('2'), "the adjacent number keeps its LTR order");
        });
        frame(&mut text, vec![]); // consume the atlas updates from the explicit layout above
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
}
