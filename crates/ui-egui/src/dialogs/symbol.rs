//! Font-based character browser. Subset ranges follow WordCraft's symbol dialog
//! (MIT OR Apache-2.0); rendering and insertion use DeckCraft's own font and text paths.

use std::collections::HashMap;

use deckcraft_fonts::FontDb;
use egui::{Color32, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::Dialog;
use crate::{SlideApp, theme};

pub(crate) const RECENT_MAX: usize = 20;
const COLS: usize = 16;
const CELL: f32 = 32.0;
const ROWS: usize = 7;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecentSymbol {
    pub text: String,
    /// Empty means the surrounding text's font.
    pub font: String,
}

#[derive(Clone, Default)]
pub(super) struct SymbolForm {
    pub(super) font: String,
    font_filter: String,
    text_font: String,
    pub(super) selected: Option<char>,
    pub(super) code: String,
    chars: Vec<char>,
    face_id: Option<u32>,
    scroll_to: Option<usize>,
    focus_char: Option<char>,
    special: bool,
    special_index: usize,
    textures: HashMap<(String, char, u32), egui::TextureHandle>,
    renders_left: usize,
}

impl std::fmt::Debug for SymbolForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SymbolForm").field("font", &self.font).field("selected", &self.selected).field("code", &self.code).finish_non_exhaustive()
    }
}

impl SymbolForm {
    fn new(app: &mut SlideApp) -> Self {
        let text_font = app
            .session
            .execute("format.state", &json!({}))
            .ok()
            .and_then(|v| v["font"].as_str().map(str::to_owned))
            .filter(|f| !f.is_empty())
            .unwrap_or_else(|| deckcraft_fonts::DEFAULT_FAMILY.to_string());
        let recent = app.ui.recent_symbols.first();
        let mut form = Self {
            text_font,
            font: recent.map(|r| r.font.clone()).unwrap_or_default(),
            selected: recent.and_then(|r| r.text.chars().next()).or(Some('©')),
            ..Default::default()
        };
        form.load();
        form
    }

    fn shown_font(&self) -> &str {
        if self.font.is_empty() { &self.text_font } else { &self.font }
    }

    fn load(&mut self) {
        let face = FontDb::global().document_face(self.shown_font(), "Regular");
        if self.face_id == Some(face.id()) {
            return;
        }
        self.chars = face.chars().into_iter().filter(|(c, gid)| !c.is_control() && *gid != 0).map(|(c, _)| c).collect();
        self.face_id = Some(face.id());
        if self.selected.is_none_or(|c| self.chars.binary_search(&c).is_err()) {
            self.selected = self.chars.iter().copied().find(|c| !c.is_whitespace());
        }
        self.sync_code();
        self.scroll_to = self.selected.and_then(|c| self.row_of(c));
    }

    fn row_of(&self, c: char) -> Option<usize> {
        self.chars.binary_search(&c).ok().map(|i| i / COLS)
    }

    fn sync_code(&mut self) {
        self.code = self.selected.map(|c| format!("{:04X}", c as u32)).unwrap_or_default();
    }

    fn select_code(&mut self) {
        let code = self.code.trim().trim_start_matches("U+").trim_start_matches("u+");
        self.selected = u32::from_str_radix(code, 16).ok().and_then(char::from_u32).filter(|c| self.chars.binary_search(c).is_ok());
        self.scroll_to = self.selected.and_then(|c| self.row_of(c));
    }

    fn jump_to(&mut self, start: u32, end: u32) {
        let index = self.chars.partition_point(|c| (*c as u32) < start);
        if let Some(c) = self.chars.get(index).copied().filter(|c| (*c as u32) <= end) {
            self.selected = Some(c);
            self.sync_code();
            self.scroll_to = Some(index / COLS);
        }
    }

    fn pick(&mut self, c: char) {
        self.selected = Some(c);
        self.sync_code();
    }

    fn texture(&mut self, ui: &Ui, font: &str, c: char) -> Option<egui::TextureHandle> {
        let scale = ui.ctx().pixels_per_point().clamp(0.5, 4.0);
        let key = (font.to_string(), c, scale.to_bits());
        if let Some(texture) = self.textures.get(&key) {
            return Some(texture.clone());
        }
        if self.renders_left == 0 {
            ui.ctx().request_repaint();
            return None;
        }
        self.renders_left -= 1;
        let image = glyph_image(font, c, scale);
        let texture = ui.ctx().load_texture(format!("symbol-{font}-{:X}", c as u32), image, egui::TextureOptions::LINEAR);
        if self.textures.len() >= 512 {
            self.textures.clear();
        }
        self.textures.insert(key, texture.clone());
        Some(texture)
    }

    fn cell(&mut self, ui: &mut Ui, font: &str, c: char, selected: bool) -> egui::Response {
        let t = theme::Tokens::get(ui.ctx());
        let (rect, response) = ui.allocate_exact_size(vec2(CELL, CELL), Sense::click());
        let label = format!("U+{:04X}", c as u32);
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, &label));
        let fill = if selected {
            t.accent
        } else if response.hovered() {
            t.hover
        } else {
            t.text_field
        };
        ui.painter().rect(rect, 0.0, fill, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
        if let Some(texture) = self.texture(ui, font, c) {
            ui.painter().image(
                texture.id(),
                rect.shrink(1.0),
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                if selected { t.accent_text } else { t.text },
            );
        }
        if response.has_focus() {
            ui.painter().rect_stroke(rect.shrink(2.0), 0.0, Stroke::new(1.0, t.text), egui::StrokeKind::Inside);
        }
        response.on_hover_text(format!("{label} · {font}"))
    }
}

/// Render through the document renderer, rather than substituting the interface font.
fn glyph_image(font: &str, c: char, scale: f32) -> egui::ColorImage {
    use deckcraft_model::text::{Align, Anchor, TextBody};
    use deckcraft_model::{Presentation, Shape};
    let mut doc = Presentation { slide_size: deckcraft_geom::Size::new(CELL as f64, CELL as f64), ..Default::default() };
    let mut body = TextBody::from_text(&c.to_string());
    body.body.anchor = Some(Anchor::Middle);
    body.body.inset_l = Some(0.0);
    body.body.inset_r = Some(0.0);
    body.body.inset_t = Some(0.0);
    body.body.inset_b = Some(0.0);
    body.body.wrap = Some(false);
    for para in &mut body.paragraphs {
        para.props.align = Some(Align::Center);
        for run in &mut para.runs {
            run.props.font = Some(font.to_string());
            run.props.font_ea = Some(font.to_string());
            run.props.font_cs = Some(font.to_string());
            run.props.font_sym = Some(font.to_string());
            run.props.size = Some(20.0);
        }
    }
    if let Some(slide) = doc.slides.first_mut() {
        std::sync::Arc::make_mut(slide).shapes = vec![Shape {
            xfrm: Some(deckcraft_geom::Xfrm::new(0.0, 0.0, CELL as f64, CELL as f64)),
            text_box: true,
            text: Some(body),
            ..Default::default()
        }];
    }
    let image = deckcraft_render::render_slide(
        &doc,
        0,
        &deckcraft_render::RenderOpts { scale: scale as f64, transparent_background: true, ..Default::default() },
    );
    let pixels = image
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| {
            let a = p.get(3).copied().unwrap_or(0);
            Color32::from_rgba_premultiplied(a, a, a, a)
        })
        .collect();
    egui::ColorImage::new([image.width as usize, image.height as usize], pixels)
}

pub(super) fn show(app: &mut SlideApp, ui: &mut Ui, dialog: &mut Dialog) -> bool {
    let mut form = dialog.symbol.take().unwrap_or_else(|| SymbolForm::new(app));
    form.renders_left = 16;
    ui.set_width(COLS as f32 * CELL + 24.0);
    ui.horizontal(|ui| {
        if ui.selectable_label(!form.special, "Symbols").clicked() {
            form.special = false;
        }
        if ui.selectable_label(form.special, "Special Characters").clicked() {
            form.special = true;
        }
    });
    ui.separator();
    let inserted = if form.special { special_ui(ui, &mut form) } else { symbols_ui(app, ui, &mut form) };
    let mut close = false;
    ui.separator();
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        close = ui.button("Close").clicked();
        let choice = if form.special { SPECIAL.get(form.special_index).map(|(_, c)| *c) } else { form.selected };
        let insert = ui.add_enabled(choice.is_some(), egui::Button::new("Insert")).clicked();
        if (insert || inserted)
            && let Some(c) = choice
        {
            let font = if form.special { "" } else { form.font.as_str() };
            insert_symbol(app, c, font);
        }
    });
    dialog.symbol = Some(form);
    close
}

fn insert_symbol(app: &mut SlideApp, c: char, font: &str) {
    if app.session.active().is_some_and(|s| s.selection.text.is_none()) && app.run("text.edit", json!({})).is_err() {
        return;
    }
    let mut params = json!({"text": c.to_string()});
    if !font.is_empty() {
        params["font"] = json!(font);
    }
    if app.run("insert.symbol", params).is_ok() {
        let symbol = RecentSymbol { text: c.to_string(), font: font.to_string() };
        app.ui.recent_symbols.retain(|s| *s != symbol);
        app.ui.recent_symbols.insert(0, symbol);
        app.ui.recent_symbols.truncate(RECENT_MAX);
    }
}

fn symbols_ui(app: &SlideApp, ui: &mut Ui, form: &mut SymbolForm) -> bool {
    form.load();
    font_and_subset(app, ui, form);
    let mut double = false;
    let font = form.shown_font().to_string();
    let mut area = egui::ScrollArea::vertical().id_salt("symbol_grid").max_height(CELL * ROWS as f32).auto_shrink([false, false]);
    if let Some(row) = form.scroll_to.take() {
        area = area.vertical_scroll_offset(row.saturating_sub(1) as f32 * CELL);
    }
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        area.show_rows(ui, CELL, form.chars.len().div_ceil(COLS), |ui, range| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            for row in range {
                ui.horizontal(|ui| {
                    for index in row * COLS..(row * COLS + COLS).min(form.chars.len()) {
                        let Some(c) = form.chars.get(index).copied() else { continue };
                        let response = ui.push_id(c as u32, |ui| form.cell(ui, &font, c, form.selected == Some(c))).inner;
                        if response.clicked() || response.double_clicked() {
                            form.pick(c);
                            response.request_focus();
                        }
                        double |= response.double_clicked();
                        double |= navigate(ui, form, c, &response);
                    }
                });
            }
        });
    });
    if form.chars.is_empty() {
        ui.label("This font has no characters to show.");
    }
    if !app.ui.recent_symbols.is_empty() {
        ui.label("Recently used symbols");
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            for (index, recent) in app.ui.recent_symbols.iter().take(COLS).enumerate() {
                let Some(c) = recent.text.chars().next() else { continue };
                let font = if recent.font.is_empty() { form.text_font.as_str() } else { recent.font.as_str() }.to_string();
                let response = ui.push_id(("recent_symbol", index), |ui| form.cell(ui, &font, c, false)).inner;
                if response.clicked() || response.double_clicked() {
                    form.font = recent.font.clone();
                    form.selected = Some(c);
                    form.face_id = None;
                    form.load();
                    form.code = format!("{:04X}", c as u32);
                    form.select_code();
                    form.scroll_to = form.row_of(c);
                }
                double |= response.double_clicked();
            }
        });
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let label = ui.label("Character code");
        let response =
            ui.add(egui::TextEdit::singleline(&mut form.code).id_salt("symbol_code").char_limit(8).desired_width(85.0)).labelled_by(label.id);
        if response.changed() {
            form.select_code();
        }
        ui.label("Unicode (hex)");
        ui.label(format!("{} characters", form.chars.len()));
    });
    if form.selected.is_none() && !form.code.is_empty() {
        ui.label("Enter a valid Unicode character supported by this font.");
    }
    double
}

fn font_and_subset(app: &SlideApp, ui: &mut Ui, form: &mut SymbolForm) {
    ui.horizontal(|ui| {
        let label = ui.label("Font");
        let response = craft_ui::ComboBox::from_id_salt("symbol_font")
            .selected_text(if form.font.is_empty() { "(normal text)" } else { &form.font })
            .width(180.0)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show_ui_with_header(
                ui,
                |ui| {
                    let label = ui.label("Search fonts");
                    ui.add(egui::TextEdit::singleline(&mut form.font_filter).desired_width(180.0).char_limit(256)).labelled_by(label.id);
                    form.font_filter.to_lowercase()
                },
                |ui, query| {
                    if ui.selectable_label(form.font.is_empty(), "(normal text)").clicked() {
                        form.font.clear();
                        ui.close();
                    }
                    for family in crate::ribbon::font_families(app) {
                        if family.to_lowercase().contains(&query) && ui.selectable_label(form.font == family, &family).clicked() {
                            form.font = family;
                            ui.close();
                        }
                    }
                },
            );
        response.response.labelled_by(label.id);
        form.load();
        let label = ui.label("Subset");
        let current = form.selected.and_then(subset_of).unwrap_or("Other characters");
        let response = craft_ui::ComboBox::from_id_salt("symbol_subset").selected_text(current).width(220.0).show_ui(ui, |ui| {
            for &(name, start, end) in SUBSETS {
                let index = form.chars.partition_point(|c| (*c as u32) < start);
                if form.chars.get(index).is_some_and(|c| (*c as u32) <= end) && ui.selectable_label(current == name, name).clicked() {
                    form.jump_to(start, end);
                    ui.close();
                }
            }
        });
        response.response.labelled_by(label.id);
    });
}

fn navigate(ui: &mut Ui, form: &mut SymbolForm, c: char, response: &egui::Response) -> bool {
    if form.focus_char == Some(c) {
        response.request_focus();
        form.focus_char = None;
    }
    if !response.has_focus() {
        return false;
    }
    let (delta, insert) = ui.input_mut(|i| {
        let delta =
            [(egui::Key::ArrowRight, 1), (egui::Key::ArrowLeft, -1), (egui::Key::ArrowDown, COLS as isize), (egui::Key::ArrowUp, -(COLS as isize))]
                .into_iter()
                .find_map(|(key, delta)| i.consume_key(egui::Modifiers::NONE, key).then_some(delta))
                .unwrap_or(0);
        (delta, i.consume_key(egui::Modifiers::NONE, egui::Key::Enter) || i.consume_key(egui::Modifiers::NONE, egui::Key::Space))
    });
    if delta != 0 {
        let index = form.selected.and_then(|c| form.chars.binary_search(&c).ok()).unwrap_or(0);
        if let Some(next) = form.chars.get(index.saturating_add_signed(delta)).copied() {
            form.pick(next);
            form.focus_char = Some(next);
            form.scroll_to = form.row_of(next);
        }
    }
    insert
}

fn subset_of(c: char) -> Option<&'static str> {
    SUBSETS.iter().find(|(_, start, end)| (*start..=*end).contains(&(c as u32))).map(|(name, _, _)| *name)
}

fn special_ui(ui: &mut Ui, form: &mut SymbolForm) -> bool {
    let mut double = false;
    egui::ScrollArea::vertical().id_salt("special_characters").max_height(CELL * ROWS as f32).auto_shrink([false, false]).show(ui, |ui| {
        for (index, &(name, c)) in SPECIAL.iter().enumerate() {
            ui.horizontal(|ui| {
                let font = form.text_font.clone();
                let response = ui.push_id(index, |ui| form.cell(ui, &font, c, form.special_index == index)).inner;
                let name = ui.selectable_label(form.special_index == index, name);
                if response.clicked() || name.clicked() || response.double_clicked() || name.double_clicked() {
                    form.special_index = index;
                }
                double |= response.double_clicked() || name.double_clicked();
                ui.label(format!("U+{:04X}", c as u32));
            });
        }
    });
    double
}

const SPECIAL: &[(&str, char)] = &[
    ("Copyright", '©'),
    ("Registered", '®'),
    ("Trademark", '™'),
    ("Service mark", '℠'),
    ("Sound recording copyright", '℗'),
    ("Em dash", '—'),
    ("En dash", '–'),
    ("Nonbreaking hyphen", '\u{2011}'),
    ("Optional hyphen", '\u{00AD}'),
    ("Em space", '\u{2003}'),
    ("En space", '\u{2002}'),
    ("Quarter-em space", '\u{2005}'),
    ("Nonbreaking space", '\u{00A0}'),
    ("Section mark", '§'),
    ("Paragraph mark", '¶'),
    ("Ellipsis", '…'),
    ("Single opening quote", '‘'),
    ("Single closing quote", '’'),
    ("Double opening quote", '“'),
    ("Double closing quote", '”'),
    ("Zero-width space", '\u{200B}'),
    ("Word joiner", '\u{2060}'),
];

#[cfg(test)]
mod tests;

const SUBSETS: &[(&str, u32, u32)] = &[
    ("Basic Latin", 0x0020, 0x007F),
    ("Latin-1 Supplement", 0x0080, 0x00FF),
    ("Latin Extended-A", 0x0100, 0x017F),
    ("Latin Extended-B", 0x0180, 0x024F),
    ("IPA Extensions", 0x0250, 0x02AF),
    ("Spacing Modifier Letters", 0x02B0, 0x02FF),
    ("Combining Diacritical Marks", 0x0300, 0x036F),
    ("Greek and Coptic", 0x0370, 0x03FF),
    ("Cyrillic", 0x0400, 0x052F),
    ("Armenian", 0x0530, 0x058F),
    ("Hebrew", 0x0590, 0x05FF),
    ("Arabic", 0x0600, 0x06FF),
    ("Devanagari", 0x0900, 0x097F),
    ("Bengali", 0x0980, 0x09FF),
    ("Gurmukhi", 0x0A00, 0x0A7F),
    ("Gujarati", 0x0A80, 0x0AFF),
    ("Oriya", 0x0B00, 0x0B7F),
    ("Tamil", 0x0B80, 0x0BFF),
    ("Telugu", 0x0C00, 0x0C7F),
    ("Kannada", 0x0C80, 0x0CFF),
    ("Malayalam", 0x0D00, 0x0D7F),
    ("Sinhala", 0x0D80, 0x0DFF),
    ("Thai", 0x0E00, 0x0E7F),
    ("Lao", 0x0E80, 0x0EFF),
    ("Tibetan", 0x0F00, 0x0FFF),
    ("Myanmar", 0x1000, 0x109F),
    ("Georgian", 0x10A0, 0x10FF),
    ("Hangul Jamo", 0x1100, 0x11FF),
    ("Ethiopic", 0x1200, 0x137F),
    ("Khmer", 0x1780, 0x17FF),
    ("Phonetic Extensions", 0x1D00, 0x1DBF),
    ("Latin Extended Additional", 0x1E00, 0x1EFF),
    ("Greek Extended", 0x1F00, 0x1FFF),
    ("General Punctuation", 0x2000, 0x206F),
    ("Superscripts and Subscripts", 0x2070, 0x209F),
    ("Currency Symbols", 0x20A0, 0x20CF),
    ("Letterlike Symbols", 0x2100, 0x214F),
    ("Number Forms", 0x2150, 0x218F),
    ("Arrows", 0x2190, 0x21FF),
    ("Mathematical Operators", 0x2200, 0x22FF),
    ("Miscellaneous Technical", 0x2300, 0x23FF),
    ("Enclosed Alphanumerics", 0x2460, 0x24FF),
    ("Box Drawing", 0x2500, 0x257F),
    ("Block Elements", 0x2580, 0x259F),
    ("Geometric Shapes", 0x25A0, 0x25FF),
    ("Miscellaneous Symbols", 0x2600, 0x26FF),
    ("Dingbats", 0x2700, 0x27BF),
    ("Supplemental Arrows", 0x27F0, 0x27FF),
    ("Braille Patterns", 0x2800, 0x28FF),
    ("Supplemental Mathematical Operators", 0x2A00, 0x2AFF),
    ("CJK Symbols and Punctuation", 0x3000, 0x303F),
    ("Hiragana", 0x3040, 0x309F),
    ("Katakana", 0x30A0, 0x30FF),
    ("CJK Unified Ideographs", 0x4E00, 0x9FFF),
    ("Hangul Syllables", 0xAC00, 0xD7AF),
    ("Private Use Area", 0xE000, 0xF8FF),
    ("Alphabetic Presentation Forms", 0xFB00, 0xFB4F),
    ("Halfwidth and Fullwidth Forms", 0xFF00, 0xFFEF),
    ("Specials", 0xFFF0, 0xFFFF),
    ("Mathematical Alphanumeric Symbols", 0x1D400, 0x1D7FF),
    ("Emoji and Pictographs", 0x1F300, 0x1FAFF),
];
