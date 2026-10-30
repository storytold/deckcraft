//! Home ▸ Font and Paragraph, Format menu: character, paragraph and text box formatting, and
//! the Format Painter.

use deckcraft_model::edit as ed;
use deckcraft_model::resolve;
use deckcraft_model::text::{Align, Anchor, AutoFit, Bullet, Caps, ParaProps, RunProps, Spacing, Strike, TabStop, TextDir};
use deckcraft_model::{ColorRef, ColorTransform, Effects, Fill, Line, SchemeSlot, Shape, ShapeKind};
use serde_json::{Value, json};
use std::sync::Arc;

use super::text::{format_body, format_para, format_run};
use super::{
    CommandSpec, always, bad, base64_decode, bool_or, bool_param, cmd, color_param, edit_shapes, f64_or, f64_param, has_doc, has_text,
    has_text_or_shapes, ids_param, ok, str_param,
};
use crate::{Result, Session};
use deckcraft_model::ShapeId;

mod font;
mod painter;
mod paragraph;
mod ruler;
mod text_effects;

pub use font::SIZES;
use font::{
    bold, caps, clear, color, font, font_dialog, grow, highlight, italic, language, language_all, shrink, size, spacing, strike, subscript,
    superscript, text_fill, text_transparency, underline,
};
pub use painter::Painted;
pub(crate) use painter::apply_painted;
use painter::{font_add, font_rescan, painter_apply, painter_pick, state};
use paragraph::{
    align, align_to, anchor, autofit, bullets, columns, direction, level, line_spacing, margins, numbering, paragraph, paragraph_dialog, rtl,
    upright, wrap,
};
use ruler::{ruler, tabs};
use text_effects::{case, text_outline, text_shadow, text_warp};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(noundo "font.add", "Add Font", [], None, "{data: base64 TTF/OTF/TTC} → {faces} added for this session", always, font_add),
        cmd!(noundo "font.rescan", "Rescan Installed Fonts", ["Home", "Font"], None, "{} → {faces, families} (fonts installed since start-up)", always, font_rescan),
        cmd!("format.bold", "Bold", ["Home", "Font"], Some("Cmd+B"), "{on?: bool}", has_text_or_shapes, bold),
        cmd!("format.italic", "Italic", ["Home", "Font"], Some("Cmd+I"), "{on?: bool}", has_text_or_shapes, italic),
        cmd!(
            "format.underline",
            "Underline",
            ["Home", "Font"],
            Some("Cmd+U"),
            "{on?: bool, style?: sng|dbl|heavy|dotted|dash|wavy}",
            has_text_or_shapes,
            underline
        ),
        cmd!(
            "format.strikethrough",
            "Strikethrough",
            ["Home", "Font"],
            Some("Cmd+Shift+X"),
            "{on?: bool, double?: bool}",
            has_text_or_shapes,
            strike
        ),
        cmd!("format.superscript", "Superscript", ["Home", "Font"], Some("Cmd+Shift+="), "{on?: bool}", has_text_or_shapes, superscript),
        cmd!("format.subscript", "Subscript", ["Home", "Font"], Some("Cmd+="), "{on?: bool}", has_text_or_shapes, subscript),
        cmd!("format.font", "Font", ["Home", "Font"], None, "{family}", has_text_or_shapes, font),
        cmd!("format.size", "Font Size", ["Home", "Font"], None, "{size: pt}", has_text_or_shapes, size),
        cmd!("format.grow", "Increase Font Size", ["Home", "Font"], Some("Cmd+Shift+."), "{}", has_text_or_shapes, grow),
        cmd!("format.shrink", "Decrease Font Size", ["Home", "Font"], Some("Cmd+Shift+,"), "{}", has_text_or_shapes, shrink),
        cmd!("format.color", "Font Color", ["Home", "Font"], None, "{color: #RRGGBB|accent1..6|tx1…|{scheme, lumMod…}}", has_text_or_shapes, color),
        cmd!(
            "format.textTransparency",
            "Text Transparency",
            ["Shape Format", "WordArt Styles"],
            None,
            "{value: 0..1} — 0 opaque, 1 invisible; keeps the text's colour (inherited colours included)",
            has_text_or_shapes,
            text_transparency
        ),
        cmd!(
            "format.textFill",
            "Text Fill",
            ["Shape Format", "WordArt Styles"],
            None,
            "{mode: none|solid|gradient, angle?: deg} — solid and gradient start from the colour the text shows now; gradient fades into accent 1",
            has_text_or_shapes,
            text_fill
        ),
        cmd!(
            "format.language",
            "Language",
            ["Review", "Language"],
            None,
            "{lang?, noProof?: bool} (proofing language of the selected text, and Do Not Check Spelling)",
            has_text_or_shapes,
            language
        ),
        cmd!(
            "format.languageAll",
            "Language for All Text",
            ["Review", "Language"],
            None,
            "{lang?, noProof?: bool} (proofing language of every text in the presentation: slides, tables, notes, masters and layouts, and the default for new text; one undo step)",
            has_doc,
            language_all
        ),
        cmd!("format.highlight", "Text Highlight Color", ["Home", "Font"], None, "{color? (absent = none)}", has_text_or_shapes, highlight),
        cmd!(
            "format.case",
            "Change Case",
            ["Home", "Font"],
            Some("Shift+F3"),
            "{mode?: cycle|sentence|lower|upper|title|toggle} — cycle (Shift+F3, the default) goes lowercase → Capitalize Each Word → UPPERCASE; a caret in a word changes that word",
            has_text_or_shapes,
            case
        ),
        cmd!("format.caps", "Caps", [], None, "{caps: none|small|all}", has_text_or_shapes, caps),
        cmd!(
            "format.spacing",
            "Character Spacing",
            ["Home", "Font"],
            None,
            "{pt} (veryTight -3, tight -1.5, normal 0, loose 3, veryLoose 6)",
            has_text_or_shapes,
            spacing
        ),
        cmd!("format.clear", "Clear All Formatting", ["Home", "Font"], Some("Cmd+Space"), "{}", has_text_or_shapes, clear),
        cmd!(
            "format.fontDialog",
            "Font…",
            ["Home", "Font"],
            Some("Cmd+T"),
            "{family?, size?: pt, bold?: bool, italic?: bool, underline?: none|sng|dbl|heavy|dotted|dash|wavy, color?, strike?: none|sng|dbl, baseline?: none|super|sub|<fraction>, caps?: none|small|all, spacing?: pt} — with none of these, opens the Font dialog",
            has_text_or_shapes,
            font_dialog
        ),
        cmd!("format.textShadow", "Text Shadow", ["Home", "Font"], None, "{on?: bool}", has_text_or_shapes, text_shadow),
        cmd!(
            "format.textOutline",
            "Text Outline",
            ["Shape Format", "WordArt Styles"],
            None,
            "{color?, width?: pt (0–1584), dash?: solid|dot|dash|…|sysDashDotDot, none?: bool} — given parts replace those of the outline the text has (new: black, 0.75 pt, solid); no color, width or dash (or none: true) removes it",
            has_text_or_shapes,
            text_outline
        ),
        cmd!(
            "format.textWarp",
            "Text Transform",
            ["Shape Format", "WordArt Styles"],
            None,
            "{preset: a prstTxWarp name or none, adj?: [finite raw guide values, at most 8]} — none clears the transform; omitted adj uses the preset's defaults",
            has_text_or_shapes,
            text_warp
        ),
        cmd!(
            "format.align",
            "Align",
            ["Home", "Paragraph"],
            None,
            "{align: left|center|right|justify|justifyLow|distributed}",
            has_text_or_shapes,
            align
        ),
        cmd!(
            "format.rtl",
            "Right-to-Left Text Direction",
            ["Home", "Paragraph"],
            None,
            "{on?: bool} (off = left-to-right; switching mirrors left/right alignment)",
            has_text_or_shapes,
            rtl
        ),
        cmd!("format.alignLeft", "Align Left", ["Home", "Paragraph"], Some("Cmd+L"), "{}", has_text_or_shapes, |s, _| align_to(s, Align::Left)),
        cmd!("format.alignCenter", "Center", ["Home", "Paragraph"], Some("Cmd+E"), "{}", has_text_or_shapes, |s, _| align_to(s, Align::Center)),
        cmd!("format.alignRight", "Align Right", ["Home", "Paragraph"], Some("Cmd+R"), "{}", has_text_or_shapes, |s, _| align_to(s, Align::Right)),
        cmd!("format.justify", "Justify", ["Home", "Paragraph"], Some("Cmd+J"), "{}", has_text_or_shapes, |s, _| align_to(s, Align::Justify)),
        cmd!(
            "format.bullets",
            "Bullets",
            ["Home", "Paragraph"],
            None,
            "{on?: bool, char?: •|○|■|□|◆|➢|✓|–, color?, size?: % of text}",
            has_text_or_shapes,
            bullets
        ),
        cmd!(
            "format.numbering",
            "Numbering",
            ["Home", "Paragraph"],
            None,
            "{on?: bool, scheme?: arabicPeriod|arabicParenR|romanUcPeriod|romanLcPeriod|alphaUcPeriod|alphaLcParenR|alphaLcPeriod, start?: n (a numbered paragraph whose start differs from that of the last numbered paragraph at its level begins a new run of numbers at n, as PowerPoint's Start at does), color?, size?: % of text}",
            has_text_or_shapes,
            numbering
        ),
        cmd!("format.indent", "Increase List Level", ["Home", "Paragraph"], Some("Tab"), "{}", has_text_or_shapes, |s, _| level(s, 1)),
        cmd!("format.outdent", "Decrease List Level", ["Home", "Paragraph"], Some("Shift+Tab"), "{}", has_text_or_shapes, |s, _| level(s, -1)),
        cmd!(
            "format.lineSpacing",
            "Line Spacing",
            ["Home", "Paragraph"],
            None,
            "{lines?: 1.0|1.5|2.0…, pt?: exactly}",
            has_text_or_shapes,
            line_spacing
        ),
        cmd!(
            "format.paragraph",
            "Paragraph…",
            ["Format"],
            None,
            "{align?, indentLeft?: pt, indentFirst?: pt (negative = hanging), spaceBefore?: pt, spaceAfter?: pt, lineSpacing?: lines}",
            has_text_or_shapes,
            paragraph
        ),
        cmd!(query "format.ruler", "Ruler", [], None, "{} → {x, y, width, rotated, marginLeft, indent, tabs: [{pos, align}], defaultTab} for the paragraph with the caret (x, y: the text area's top-left in slide pt)", has_text, ruler),
        cmd!(
            "format.paragraphDialog",
            "Paragraph…",
            ["Home", "Paragraph"],
            None,
            "{align?: left|center|right|justify|justLow|distributed, indentLeft?: pt, indentRight?: pt, special?: none|first|hanging, specialBy?: pt, indentFirst?: pt, spaceBefore?: pt, spaceAfter?: pt, spaceBeforeKind?: pts|pct, spaceAfterKind?: pts|pct, lineSpacing?: lines, lineSpacingPt?: pt} — with none of these, opens the Paragraph dialog",
            has_text_or_shapes,
            paragraph_dialog
        ),
        cmd!(
            "format.tabs",
            "Tabs…",
            ["Home", "Paragraph"],
            None,
            "{set?: [{pos, align?}] (replaces all), add?: {pos, align?: l|ctr|r|dec}, move?: {from, to}, remove?: pos, clear?: bool, defaultTab?: pt} on the selected paragraphs; with none of them, opens the Tabs dialog (pos in pt from the text area's left)",
            has_text_or_shapes,
            tabs
        ),
        cmd!("format.anchor", "Align Text", ["Home", "Paragraph"], None, "{anchor: top|middle|bottom}", has_text_or_shapes, anchor),
        cmd!(
            "format.direction",
            "Text Direction",
            ["Home", "Paragraph"],
            None,
            "{dir: horizontal|rotate90|rotate270|stacked}",
            has_text_or_shapes,
            direction
        ),
        cmd!(
            "format.columns",
            "Columns",
            ["Home", "Paragraph"],
            None,
            "{count?: 1-16, spacing?: pt between columns}; a key left out keeps the shape's value, and with neither the Columns dialog opens",
            has_text_or_shapes,
            columns
        ),
        cmd!("format.autofit", "Autofit", ["Format Shape", "Text Box"], None, "{mode: none|shrink|resize}", has_text_or_shapes, autofit),
        cmd!("format.wrap", "Wrap Text in Shape", ["Format Shape", "Text Box"], None, "{on: bool}", has_text_or_shapes, wrap),
        cmd!(
            "format.upright",
            "Do Not Rotate Text",
            ["Format Shape", "Text Box"],
            None,
            "{on?: bool (default true)} — the text stays upright when the shape is rotated",
            has_text_or_shapes,
            upright
        ),
        cmd!(
            "format.margins",
            "Text Box Margins",
            ["Format Shape", "Text Box"],
            None,
            "{left?: pt, top?: pt, right?: pt, bottom?: pt}",
            has_text_or_shapes,
            margins
        ),
        cmd!(noundo "format.painter", "Format Painter", ["Home", "Clipboard"], Some("Cmd+Shift+C"), "{sticky?: bool}", has_text_or_shapes, painter_pick),
        cmd!("format.painterApply", "Paste Formatting", [], Some("Cmd+Shift+V"), "{ids?}", has_text_or_shapes, painter_apply),
        cmd!(query "format.state", "Formatting State", [], None, "{} → the effective formatting at the selection (font, size, bold, …)", has_doc, state),
    ]
}

fn on_param(p: &Value, current: Option<bool>) -> bool {
    bool_param(p, "on").unwrap_or(!current.unwrap_or(false))
}

/// The effective run props at the selection (first selected run).
fn current_run(s: &Session) -> RunProps {
    let Ok(st) = s.doc() else { return RunProps::default() };
    let ctx = super::text::ctx_for(&st.doc, &st.selection);
    let (shape, para, run) = match &st.selection.text {
        Some(t) => {
            let Some(body) = super::text::body_of(st, t) else { return RunProps::default() };
            let a = t.ordered().0;
            let rp = if t.is_range() { ed::props_at(body, (a.0, a.1 + 1)) } else { ed::props_at(body, a) };
            let para = body.paragraphs.get(a.0).cloned().unwrap_or_default();
            (st.shape(t.shape).cloned().unwrap_or_default(), para, rp)
        }
        None => {
            let Some(sh) = st.selected_shapes().first().map(|x| (*x).clone()) else { return RunProps::default() };
            let para = sh.text.as_ref().and_then(|t| t.paragraphs.first().cloned()).unwrap_or_default();
            let rp = para.runs.first().map(|r| r.props.clone()).unwrap_or_default();
            (sh, para, rp)
        }
    };
    match ctx {
        Some(c) => resolve::run(&c, &shape, &para, &run),
        None => run,
    }
}

fn current_para(s: &Session) -> ParaProps {
    let Ok(st) = s.doc() else { return ParaProps::default() };
    let ctx = super::text::ctx_for(&st.doc, &st.selection);
    let (shape, para) = match &st.selection.text {
        Some(t) => {
            let para = super::text::body_of(st, t).and_then(|b| b.paragraphs.get(t.caret.0).cloned()).unwrap_or_default();
            (st.shape(t.shape).cloned().unwrap_or_default(), para)
        }
        None => {
            let Some(sh) = st.selected_shapes().first().map(|x| (*x).clone()) else { return ParaProps::default() };
            let para = sh.text.as_ref().and_then(|t| t.paragraphs.first().cloned()).unwrap_or_default();
            (sh, para)
        }
    };
    match ctx {
        Some(c) => resolve::para(&c, &shape, &para),
        None => para.props,
    }
}

#[cfg(test)]
#[path = "ruler_tests.rs"]
mod ruler_tests;

#[cfg(test)]
mod tests {
    use deckcraft_model::text::{Align, Caps, ParaProps, Spacing, Strike};
    use deckcraft_model::{ColorBase, Fill, SchemeSlot};
    use serde_json::json;

    use super::*;
    use crate::{Session, UiRequest};

    fn shape() -> Session {
        let mut s = Session::with_new();
        s.execute("shape.insert", &json!({"preset": "rect", "rect": [40.0, 40.0, 240.0, 80.0], "text": "Hello"})).unwrap();
        s
    }

    fn props(s: &Session) -> RunProps {
        s.doc()
            .unwrap()
            .selected_shapes()
            .first()
            .and_then(|sh| sh.text.as_ref())
            .and_then(|t| t.paragraphs.first())
            .and_then(|p| p.runs.first())
            .unwrap()
            .props
            .clone()
    }

    #[test]
    fn font_dialog_applies_character_formatting_in_one_undo() {
        let mut s = shape();
        let before = s.doc().unwrap().history.undo.len();
        s.execute(
            "format.fontDialog",
            &json!({
                "family": "Liberation Serif",
                "size": 28,
                "bold": true,
                "italic": false,
                "underline": "wavy",
                "color": "#112233",
                "strike": "dbl",
                "baseline": "super",
                "caps": "small",
                "spacing": 3
            }),
        )
        .unwrap();
        assert_eq!(s.doc().unwrap().history.undo.len(), before + 1);
        assert_eq!(s.doc().unwrap().history.undo.last().unwrap().label, "Font…");
        let r = props(&s);
        assert_eq!(r.font.as_deref(), Some("Liberation Serif"));
        assert_eq!(r.size, Some(28.0));
        assert_eq!(r.bold, Some(true));
        assert_eq!(r.italic, Some(false));
        assert_eq!(r.underline.as_deref(), Some("wavy"));
        assert!(matches!(r.fill, Some(Fill::Solid { color }) if matches!(color.base, ColorBase::Rgb { .. })));
        assert_eq!(r.strike, Some(Strike::Double));
        assert_eq!(r.baseline, Some(0.3));
        assert_eq!(r.caps, Some(Caps::Small));
        assert_eq!(r.spacing, Some(3.0));
        s.execute("edit.undo", &json!({})).unwrap();
        let r = props(&s);
        assert!(r.font.is_none() && r.size.is_none() && r.bold.is_none() && r.underline.is_none() && r.fill.is_none());
        assert_eq!(r.strike, None);
        assert_eq!(r.caps, None);
    }

    #[test]
    fn font_dialog_changes_only_the_fields_it_is_given() {
        let mut s = shape();
        s.execute("format.bold", &json!({"on": true})).unwrap();
        s.execute("format.fontDialog", &json!({"size": 22, "color": {"scheme": "accent2", "lumMod": 50000}})).unwrap();
        let r = props(&s);
        assert_eq!(r.bold, Some(true));
        assert_eq!(r.size, Some(22.0));
        assert!(r.font.is_none() && r.italic.is_none());
        let Some(Fill::Solid { color }) = r.fill else { panic!("solid fill") };
        assert!(matches!(color.base, ColorBase::Scheme { slot: SchemeSlot::Accent2 }));
        assert!(color.mods.iter().any(|m| m.xml_name() == "lumMod"));
        s.execute("edit.undo", &json!({})).unwrap();
        let r = props(&s);
        assert_eq!(r.bold, Some(true));
        assert!(r.size.is_none() && r.fill.is_none());
    }

    #[test]
    fn font_dialog_with_no_fields_asks_the_ui_to_open() {
        let mut s = shape();
        s.execute("format.italic", &json!({"on": true})).unwrap();
        let rev = s.doc().unwrap().revision;
        let n = s.doc().unwrap().history.undo.len();
        s.ui_requests.clear();
        s.execute("format.fontDialog", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().revision, rev);
        assert_eq!(s.doc().unwrap().history.undo.len(), n);
        let Some(UiRequest::Dialog { id, params }) = s.ui_requests.last() else { panic!("dialog request") };
        assert_eq!(id, "font");
        assert_eq!(params["italic"], true);
        assert_eq!(params["bold"], false);
        assert!(params["family"].as_str().is_some_and(|f| !f.is_empty()));
        assert!(params["size"].as_f64().is_some_and(|n| n > 0.0));
    }

    #[test]
    fn font_dialog_rejects_bad_fields_without_editing() {
        let mut s = shape();
        let n = s.doc().unwrap().history.undo.len();
        let err = s.execute("format.fontDialog", &json!({"underline": "sparkle"})).unwrap_err();
        assert!(err.to_string().contains("underline"), "{err}");
        assert!(s.execute("format.fontDialog", &json!({"size": "large"})).is_err());
        assert!(s.execute("format.fontDialog", &json!({"bold": 1})).is_err());
        assert!(s.execute("format.fontDialog", &json!({"family": "  "})).is_err());
        assert!(s.execute("format.fontDialog", &json!({"color": "not-a-color"})).is_err());
        assert_eq!(s.doc().unwrap().history.undo.len(), n);
        assert!(props(&s).underline.is_none());
        s.execute("edit.deselect", &json!({})).unwrap();
        assert!(s.execute("format.fontDialog", &json!({"bold": true})).is_err());
    }

    fn para(s: &Session) -> ParaProps {
        s.doc().unwrap().selected_shapes().first().and_then(|sh| sh.text.as_ref()).and_then(|t| t.paragraphs.first()).unwrap().props.clone()
    }

    #[test]
    fn paragraph_dialog_applies_alignment_indent_and_spacing_in_one_undo() {
        let mut s = shape();
        let before = s.doc().unwrap().history.undo.len();
        s.execute(
            "format.paragraphDialog",
            &json!({
                "align": "justify",
                "indentLeft": 36.0,
                "indentRight": 18.0,
                "special": "hanging",
                "specialBy": 18.0,
                "spaceBefore": 6.0,
                "spaceAfter": 8.0,
                "lineSpacing": 1.5
            }),
        )
        .unwrap();
        assert_eq!(s.doc().unwrap().history.undo.len(), before + 1);
        assert_eq!(s.doc().unwrap().history.undo.last().unwrap().label, "Paragraph…");
        let p = para(&s);
        assert_eq!(p.align, Some(Align::Justify));
        assert_eq!(p.margin_left, Some(36.0));
        assert_eq!(p.margin_right, Some(18.0));
        assert_eq!(p.indent, Some(-18.0));
        assert_eq!(p.space_before, Some(Spacing::Pts(6.0)));
        assert_eq!(p.space_after, Some(Spacing::Pts(8.0)));
        assert_eq!(p.line_spacing, Some(Spacing::Pct(1.5)));
        s.execute("edit.undo", &json!({})).unwrap();
        let p = para(&s);
        assert_eq!(p.align, Some(Align::Center), "undo restores the alignment shape.insert wrote");
        assert!(p.margin_left.is_none() && p.indent.is_none() && p.space_before.is_none() && p.line_spacing.is_none());
    }

    #[test]
    fn paragraph_dialog_first_line_and_exact_spacing_replace_only_those_fields() {
        let mut s = shape();
        s.execute("format.paragraphDialog", &json!({"align": "right"})).unwrap();
        s.execute("format.paragraphDialog", &json!({"special": "first", "specialBy": 27.0, "lineSpacingPt": 20.0})).unwrap();
        let p = para(&s);
        assert_eq!(p.align, Some(Align::Right));
        assert_eq!(p.indent, Some(27.0));
        assert_eq!(p.line_spacing, Some(Spacing::Pts(20.0)));
        assert!(p.margin_left.is_none());
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(para(&s).align, Some(Align::Right));
        assert!(para(&s).indent.is_none());
    }

    #[test]
    fn paragraph_dialog_with_no_fields_asks_the_ui_to_open() {
        let mut s = shape();
        let rev = s.doc().unwrap().revision;
        let n = s.doc().unwrap().history.undo.len();
        s.ui_requests.clear();
        s.execute("format.paragraphDialog", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().revision, rev);
        assert_eq!(s.doc().unwrap().history.undo.len(), n);
        let Some(UiRequest::Dialog { id, params }) = s.ui_requests.last() else { panic!("dialog request") };
        assert_eq!(id, "paragraph");
        assert_eq!(params["align"], "center", "a new shape's text is centred");
        assert_eq!(params["special"], "none");
        assert_eq!(params["lineKind"], "multiple");
        assert!(params["lineValue"].as_f64().is_some_and(|v| v > 0.0));
    }

    #[test]
    fn paragraph_dialog_rejects_bad_fields_without_editing() {
        let mut s = shape();
        let n = s.doc().unwrap().history.undo.len();
        assert!(s.execute("format.paragraphDialog", &json!({"align": "sparkle"})).unwrap_err().to_string().contains("align"));
        assert!(s.execute("format.paragraphDialog", &json!({"special": "middle"})).is_err());
        assert!(s.execute("format.paragraphDialog", &json!({"indentLeft": "wide"})).is_err());
        assert!(s.execute("format.paragraphDialog", &json!({"lineSpacing": "double"})).is_err());
        assert!(s.execute("format.paragraphDialog", &json!({"spaceBeforeKind": "lines", "spaceBefore": 4.0})).is_err());
        assert_eq!(s.doc().unwrap().history.undo.len(), n);
        assert_eq!(para(&s).align, Some(Align::Center));
        assert!(para(&s).indent.is_none());
    }

    fn body(s: &Session) -> deckcraft_model::text::BodyProps {
        s.doc().unwrap().selected_shapes().first().and_then(|sh| sh.text.as_ref()).map(|t| t.body.clone()).unwrap_or_default()
    }

    #[test]
    fn columns_set_the_number_and_spacing_and_a_left_out_key_keeps_its_value() {
        let mut s = shape();
        let n = s.doc().unwrap().history.undo.len();
        s.execute("format.columns", &json!({"count": 4, "spacing": 36.0})).unwrap();
        assert_eq!((body(&s).columns, body(&s).col_spacing), (Some(4), Some(36.0)));
        assert_eq!(s.doc().unwrap().history.undo.len(), n + 1, "one undo step");
        s.execute("format.columns", &json!({"count": 2})).unwrap();
        assert_eq!((body(&s).columns, body(&s).col_spacing), (Some(2), Some(36.0)), "the spacing stays");
        s.execute("format.columns", &json!({"spacing": 9.0})).unwrap();
        assert_eq!((body(&s).columns, body(&s).col_spacing), (Some(2), Some(9.0)), "the number stays");
    }

    #[test]
    fn columns_with_no_fields_open_the_dialog_with_the_shapes_values() {
        let mut s = shape();
        s.execute("format.columns", &json!({"count": 3, "spacing": 27.0})).unwrap();
        let rev = s.doc().unwrap().revision;
        let n = s.doc().unwrap().history.undo.len();
        s.ui_requests.clear();
        s.execute("format.columns", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().revision, rev);
        assert_eq!(s.doc().unwrap().history.undo.len(), n);
        let Some(UiRequest::Dialog { id, params }) = s.ui_requests.last() else { panic!("dialog request") };
        assert_eq!(id, "columns");
        assert_eq!(params["count"], 3);
        assert_eq!(params["spacing"], 27.0);
    }

    #[test]
    fn columns_reject_a_bad_number_or_spacing_without_editing() {
        let mut s = shape();
        let n = s.doc().unwrap().history.undo.len();
        for p in [json!({"count": 0}), json!({"count": 17}), json!({"count": 2.5}), json!({"count": "two"}), json!({"spacing": "wide"})] {
            let err = s.execute("format.columns", &p).unwrap_err().to_string();
            assert!(err.contains("format.columns"), "{p}: {err}");
        }
        assert_eq!(s.doc().unwrap().history.undo.len(), n);
        assert_eq!(body(&s).columns, None);
    }

    #[test]
    fn tabs_dialog_sets_stops_in_one_undo_and_opens_with_no_fields() {
        let mut s = shape();
        let before = s.doc().unwrap().history.undo.len();
        s.execute(
            "format.tabs",
            &json!({"set": [{"pos": 144.0, "align": "center"}, {"pos": 72.0, "align": "dec"}, {"pos": 72.02, "align": "right"}], "defaultTab": 36.0}),
        )
        .unwrap();
        assert_eq!(s.doc().unwrap().history.undo.len(), before + 1);
        assert_eq!(s.doc().unwrap().history.undo.last().unwrap().label, "Tabs…");
        let p = para(&s);
        let tabs = p.tabs.unwrap();
        assert_eq!(tabs.len(), 2, "stops within 0.05 pt collapse");
        assert_eq!(tabs[0].pos, 72.02);
        assert_eq!(tabs[0].align, "r");
        assert_eq!(tabs[1].align, "ctr");
        assert_eq!(p.default_tab, Some(36.0));
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(para(&s).tabs.is_none() && para(&s).default_tab.is_none());

        let n = s.doc().unwrap().history.undo.len();
        s.ui_requests.clear();
        s.execute("format.tabs", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().history.undo.len(), n);
        let Some(UiRequest::Dialog { id, params }) = s.ui_requests.last() else { panic!("dialog request") };
        assert_eq!(id, "tabs");
        assert!(params["tabs"].as_array().is_some_and(|a| a.is_empty()));
        assert!(s.execute("format.tabs", &json!({"set": [{"pos": 10.0, "align": "sparkle"}]})).is_err());
        assert!(s.execute("format.tabs", &json!({"set": "nope"})).is_err());
        assert_eq!(s.doc().unwrap().history.undo.len(), n);
    }
}
