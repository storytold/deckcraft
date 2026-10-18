//! Tests for the formatting commands that had none: the run toggles (strikethrough, super and
//! subscript, caps, character spacing, highlight, text shadow and outline), the font-size steps,
//! the paragraph alignment/level/line-spacing commands, the body ones (anchor, direction, columns,
//! margins, wrap) and `format.case`. The Font and Paragraph dialogs, tab stops, the Format Painter,
//! `format.align`, bullets and numbering are covered in `format.rs` and `tests.rs`.

use deckcraft_model::text::{Align, Anchor, BodyProps, Caps, ParaProps, RunProps, Spacing, Strike, TextDir};
use deckcraft_model::{ColorBase, Fill};
use serde_json::json;

use crate::Session;

/// One shape carrying a word, inserted and selected. Run-level commands read the first run,
/// paragraph-level ones the first paragraph, body-level ones the text body.
fn shape() -> Session {
    let mut s = Session::with_new();
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [40.0, 40.0, 240.0, 80.0], "text": "Hello"})).expect("insert a shape with text");
    s
}

fn text(s: &Session) -> deckcraft_model::text::TextBody {
    s.doc().unwrap().selected_shapes().first().and_then(|sh| sh.text.as_ref()).cloned().expect("the selected shape has text")
}

/// Run props of the first run of the first paragraph.
fn props(s: &Session) -> RunProps {
    let t = text(s);
    t.paragraphs.first().expect("a paragraph").runs.first().expect("a run").props.clone()
}

/// Paragraph props and list level of the first paragraph.
fn para(s: &Session) -> (ParaProps, u8) {
    let t = text(s);
    let p = t.paragraphs.first().expect("a paragraph");
    (p.props.clone(), p.level)
}

/// The body props: insets, anchor, direction, columns, wrap.
fn body(s: &Session) -> BodyProps {
    text(s).body.clone()
}

fn words(s: &Session) -> String {
    text(s).text()
}

// ---------------------------------------------------------------------------------------------
// Run-level toggles
// ---------------------------------------------------------------------------------------------

/// A bare click strikes through and a second one takes it off again, so the ribbon button needs no
/// `on` of its own to undo its own work.
#[test]
fn strikethrough_toggles_and_keeps_the_double_variant_separate() {
    let mut s = shape();
    s.execute("format.strikethrough", &json!({})).expect("strike through");
    assert_eq!(props(&s).strike, Some(Strike::Single));
    s.execute("format.strikethrough", &json!({})).expect("strike off again");
    assert_eq!(props(&s).strike, Some(Strike::None), "off is recorded, not merely absent");

    s.execute("format.strikethrough", &json!({"double": true})).expect("double strike");
    assert_eq!(props(&s).strike, Some(Strike::Double));
    // With no `on` of its own the button still toggles, so the same call twice is still off.
    s.execute("format.strikethrough", &json!({"double": true})).expect("again");
    assert_eq!(props(&s).strike, Some(Strike::None));
    s.execute("format.strikethrough", &json!({"on": true})).expect("asking for it on, single");
    assert_eq!(props(&s).strike, Some(Strike::Single));
    s.execute("format.strikethrough", &json!({"on": false})).expect("turn it off");
    assert_eq!(props(&s).strike, Some(Strike::None));

    s.execute("edit.undo", &json!({})).expect("undo the last one");
    assert_eq!(props(&s).strike, Some(Strike::Single), "undo brings the strike back");
}

/// Super and subscript are two offsets on one field, so switching between them is a plain click and
/// neither overwrites the other with a half-height baseline.
#[test]
fn superscript_and_subscript_share_one_baseline_field() {
    let mut s = shape();
    s.execute("format.superscript", &json!({})).expect("superscript");
    assert_eq!(props(&s).baseline, Some(0.3));
    s.execute("format.subscript", &json!({})).expect("subscript takes over from superscript");
    assert_eq!(props(&s).baseline, Some(-0.25));
    s.execute("format.superscript", &json!({})).expect("and back up");
    assert_eq!(props(&s).baseline, Some(0.3));
    s.execute("format.superscript", &json!({})).expect("clicking superscript again lowers the text");
    assert_eq!(props(&s).baseline, Some(0.0));
    s.execute("format.subscript", &json!({"on": false})).expect("and off");
    assert_eq!(props(&s).baseline, Some(0.0));
}

/// Caps is stored rather than applied, so the renderer can measure the original text.
#[test]
fn caps_is_stored_and_an_unknown_value_clears_it() {
    let mut s = shape();
    s.execute("format.caps", &json!({"caps": "small"})).expect("small caps");
    assert_eq!(props(&s).caps, Some(Caps::Small));
    s.execute("format.caps", &json!({"caps": "all"})).expect("all caps");
    assert_eq!(props(&s).caps, Some(Caps::All));
    // No value at all is the ribbon's "no caps", and so is a value nobody recognises.
    s.execute("format.caps", &json!({})).expect("no caps");
    assert_eq!(props(&s).caps, Some(Caps::None));
    s.execute("format.caps", &json!({"caps": "small"})).expect("small caps again");
    s.execute("format.caps", &json!({"caps": "spaced"})).expect("an unknown value");
    assert_eq!(props(&s).caps, Some(Caps::None));
}

/// Character spacing is in points and clamped: past the ends there is nothing to show, and a
/// runaway number must not reach the XML.
#[test]
fn character_spacing_is_points_and_is_clamped_to_what_can_be_shown() {
    let mut s = shape();
    s.execute("format.spacing", &json!({"pt": 3})).expect("wider");
    assert_eq!(props(&s).spacing, Some(3.0));
    s.execute("format.spacing", &json!({"pt": -1.5})).expect("tighter");
    assert_eq!(props(&s).spacing, Some(-1.5));
    s.execute("format.spacing", &json!({"pt": 9999})).expect("clamped above");
    assert_eq!(props(&s).spacing, Some(400.0));
    s.execute("format.spacing", &json!({"pt": -9999})).expect("clamped below");
    assert_eq!(props(&s).spacing, Some(-100.0));
    s.execute("format.spacing", &json!({})).expect("no spacing is zero spacing");
    assert_eq!(props(&s).spacing, Some(0.0));
}

/// Highlight takes a colour, and an absent one means none — the same call that sets it clears it.
#[test]
fn highlight_sets_a_colour_and_an_absent_one_clears_it() {
    let mut s = shape();
    s.execute("format.highlight", &json!({"color": "#ffee00"})).expect("highlight");
    let r = props(&s);
    let is_colour = matches!(&r.highlight, Some(c) if matches!(c.base, ColorBase::Rgb { .. }));
    assert!(is_colour, "a highlight colour, not a scheme reference: {r:?}");
    s.execute("format.highlight", &json!({})).expect("no colour clears it");
    assert!(props(&s).highlight.is_none());
}

/// The shadow the button applies is the one PowerPoint's Text Effects gallery offers, so its
/// numbers are asserted rather than merely "some shadow appeared".
#[test]
fn text_shadow_is_the_gallery_shadow_and_toggles_off() {
    let mut s = shape();
    s.execute("format.textShadow", &json!({})).expect("shadow on");
    let sh = props(&s).shadow.expect("a shadow");
    assert_eq!(sh.blur, 3.0);
    assert_eq!(sh.dist, 2.0);
    assert_eq!(sh.dir, 45.0);
    assert!(!sh.inner, "the run's outer shadow, not the inner one");
    s.execute("format.textShadow", &json!({})).expect("shadow off");
    assert!(props(&s).shadow.is_none());
}

/// An outline is a line, so it carries a colour and a width; no colour means no outline.
#[test]
fn text_outline_is_a_line_of_the_given_width() {
    let mut s = shape();
    s.execute("format.textOutline", &json!({"color": "#ff0000", "width": 2})).expect("outline");
    let r = props(&s);
    let line = r.outline.expect("an outline");
    assert_eq!(line.width, Some(2.0));
    assert!(matches!(&line.fill, Some(Fill::Solid { .. })), "{:?}", line.fill);
    s.execute("format.textOutline", &json!({})).expect("no colour takes it away");
    assert!(props(&s).outline.is_none());
}

/// Grow and shrink walk PowerPoint's own size list rather than adding a fixed step, so 18 goes to
/// 20 and 20 back to 18 instead of 26 and 10.
#[test]
fn font_size_steps_through_the_powerpoint_list() {
    let mut s = shape();
    s.execute("format.size", &json!({"size": 18})).expect("18 pt");
    s.execute("format.grow", &json!({})).expect("grow");
    assert_eq!(props(&s).size, Some(20.0));
    s.execute("format.grow", &json!({})).expect("grow again");
    assert_eq!(props(&s).size, Some(24.0));
    s.execute("format.shrink", &json!({})).expect("shrink");
    assert_eq!(props(&s).size, Some(20.0));
    s.execute("format.shrink", &json!({})).expect("shrink again");
    assert_eq!(props(&s).size, Some(18.0));
}

/// Off the ends of the list the step falls back to a plain offset, bounded so neither button can
/// drive the size to nothing or to infinity.
#[test]
fn font_size_stepping_survives_both_ends_of_the_list() {
    let mut s = shape();
    s.execute("format.size", &json!({"size": 8})).expect("the smallest listed size");
    s.execute("format.shrink", &json!({})).expect("shrink below the list");
    assert_eq!(props(&s).size, Some(7.0), "one point down, not zero");
    s.execute("format.size", &json!({"size": 66})).expect("the largest listed size");
    s.execute("format.grow", &json!({})).expect("grow above the list");
    assert_eq!(props(&s).size, Some(74.0));
}

/// Clear Formatting takes the character formatting away and leaves the language and the link, which
/// are not formatting the user chose in the ribbon.
#[test]
fn clear_formatting_empties_the_run_but_keeps_its_language() {
    let mut s = shape();
    s.execute("format.font", &json!({"family": "Liberation Serif"})).expect("a font");
    s.execute("format.size", &json!({"size": 24})).expect("a size");
    s.execute("format.bold", &json!({"on": true})).expect("bold");
    s.execute("format.highlight", &json!({"color": "#ffee00"})).expect("a highlight");
    s.execute("format.language", &json!({"lang": "fr-FR"})).expect("a language");

    s.execute("format.clear", &json!({})).expect("clear");
    let r = props(&s);
    assert!(r.font.is_none() && r.size.is_none() && r.bold.is_none() && r.highlight.is_none(), "{r:?}");
    assert_eq!(r.lang.as_deref(), Some("fr-FR"), "the language is not formatting");
}

// ---------------------------------------------------------------------------------------------
// Paragraph-level commands
// ---------------------------------------------------------------------------------------------

/// The three alignment buttons are three ways of saying the same thing as `format.align`.
#[test]
fn the_alignment_buttons_write_the_alignment_field() {
    let mut s = shape();
    s.execute("format.alignCenter", &json!({})).expect("center");
    assert_eq!(para(&s).0.align, Some(Align::Center));
    s.execute("format.alignRight", &json!({})).expect("right");
    assert_eq!(para(&s).0.align, Some(Align::Right));
    s.execute("format.justify", &json!({})).expect("justify");
    assert_eq!(para(&s).0.align, Some(Align::Justify));
    s.execute("format.alignLeft", &json!({})).expect("left");
    assert_eq!(para(&s).0.align, Some(Align::Left));
}

/// Changing the list level carries an explicit indent along with it, or the text would jump out of
/// its own margin. A level that cannot move moves nothing at all.
#[test]
fn the_level_buttons_shift_the_explicit_indent_with_the_level() {
    let mut s = shape();
    s.execute("format.paragraph", &json!({"indentLeft": 10})).expect("an explicit margin");
    s.execute("format.indent", &json!({})).expect("indent");
    assert_eq!(para(&s).1, 1);
    assert_eq!(para(&s).0.margin_left, Some(46.0), "the margin followed the level in");
    s.execute("format.outdent", &json!({})).expect("outdent");
    assert_eq!(para(&s).1, 0);
    assert_eq!(para(&s).0.margin_left, Some(10.0));
    s.execute("format.outdent", &json!({})).expect("outdent below the first level");
    assert_eq!(para(&s).1, 0);
    assert_eq!(para(&s).0.margin_left, Some(10.0), "a level that cannot move takes no margin");
}

/// Levels run 0 to 8, and the ninth indent is a no-op rather than a ninth level.
#[test]
fn the_level_stops_at_the_deepest_one() {
    let mut s = shape();
    s.execute("format.paragraph", &json!({"indentLeft": 10})).expect("an explicit margin");
    for _ in 0..8 {
        s.execute("format.indent", &json!({})).expect("indent");
    }
    assert_eq!(para(&s).1, 8);
    assert_eq!(para(&s).0.margin_left, Some(298.0));
    s.execute("format.indent", &json!({})).expect("one indent too many");
    assert_eq!(para(&s).1, 8);
    assert_eq!(para(&s).0.margin_left, Some(298.0), "and it takes no margin either");
}

/// Line spacing is either points or a multiple of the line, and both are bounded.
#[test]
fn line_spacing_is_points_or_a_multiple_and_both_are_bounded() {
    let mut s = shape();
    s.execute("format.lineSpacing", &json!({"pt": 24})).expect("exactly 24 pt");
    assert_eq!(para(&s).0.line_spacing, Some(Spacing::Pts(24.0)));
    s.execute("format.lineSpacing", &json!({"lines": 1.5})).expect("one and a half lines");
    assert_eq!(para(&s).0.line_spacing, Some(Spacing::Pct(1.5)));
    s.execute("format.lineSpacing", &json!({"lines": 99})).expect("clamped above");
    assert_eq!(para(&s).0.line_spacing, Some(Spacing::Pct(9.99)));
    s.execute("format.lineSpacing", &json!({"pt": 99999})).expect("clamped below");
    assert_eq!(para(&s).0.line_spacing, Some(Spacing::Pts(1584.0)));
    s.execute("format.lineSpacing", &json!({})).expect("single spacing");
    assert_eq!(para(&s).0.line_spacing, Some(Spacing::Pct(1.0)));
}

// ---------------------------------------------------------------------------------------------
// Body-level commands
// ---------------------------------------------------------------------------------------------

/// The three inset buttons set one side each, and a negative inset is not an inset.
#[test]
fn margins_set_only_the_sides_they_are_given() {
    let mut s = shape();
    s.execute("format.margins", &json!({"left": 10, "top": 5})).expect("two sides");
    let b = body(&s);
    assert_eq!(b.inset_l, Some(10.0));
    assert_eq!(b.inset_t, Some(5.0));
    assert!(b.inset_r.is_none() && b.inset_b.is_none(), "the other sides were left alone");
    s.execute("format.margins", &json!({"right": 20})).expect("the right side");
    assert_eq!(body(&s).inset_r, Some(20.0));
    assert_eq!(body(&s).inset_l, Some(10.0), "the left side is still set");
    s.execute("format.margins", &json!({"left": -5})).expect("a negative inset");
    assert_eq!(body(&s).inset_l, Some(0.0), "clamped to the shape's own edge");
}

/// Anchor decides which edge of the shape the text sits against.
#[test]
fn anchor_picks_the_edge_the_text_sits_against() {
    let mut s = shape();
    s.execute("format.anchor", &json!({"anchor": "middle"})).expect("middle");
    assert_eq!(body(&s).anchor, Some(Anchor::Middle));
    s.execute("format.anchor", &json!({"anchor": "bottom"})).expect("bottom");
    assert_eq!(body(&s).anchor, Some(Anchor::Bottom));
    s.execute("format.anchor", &json!({"anchor": "top"})).expect("top");
    assert_eq!(body(&s).anchor, Some(Anchor::Top));
    s.execute("format.anchor", &json!({})).expect("no value means top");
    assert_eq!(body(&s).anchor, Some(Anchor::Top));
}

/// Text direction covers the East Asian ways of writing downwards as well as turning the box.
#[test]
fn direction_covers_the_ways_of_writing_downwards() {
    let mut s = shape();
    s.execute("format.direction", &json!({"dir": "vert"})).expect("vertical");
    assert_eq!(body(&s).vert, Some(TextDir::Vertical));
    s.execute("format.direction", &json!({"dir": "rotate90"})).expect("the other name for it");
    assert_eq!(body(&s).vert, Some(TextDir::Vertical));
    s.execute("format.direction", &json!({"dir": "rotate270"})).expect("bottom to top");
    assert_eq!(body(&s).vert, Some(TextDir::Vertical270));
    s.execute("format.direction", &json!({"dir": "stacked"})).expect("one letter per line");
    assert_eq!(body(&s).vert, Some(TextDir::Stacked));
    s.execute("format.direction", &json!({})).expect("back to horizontal");
    assert_eq!(body(&s).vert, Some(TextDir::Horizontal));
}

/// Columns are bounded to what a text box can usefully show, and a negative gutter is no gutter.
#[test]
fn columns_are_bounded_and_a_negative_gutter_is_no_gutter() {
    let mut s = shape();
    s.execute("format.columns", &json!({"count": 3, "spacing": 12})).expect("three columns");
    let b = body(&s);
    assert_eq!(b.columns, Some(3));
    assert_eq!(b.col_spacing, Some(12.0));
    s.execute("format.columns", &json!({"count": 99})).expect("more columns than fit");
    assert_eq!(body(&s).columns, Some(16));
    s.execute("format.columns", &json!({"count": 0})).expect("no columns");
    assert_eq!(body(&s).columns, Some(1));
    s.execute("format.columns", &json!({"count": 2, "spacing": -10})).expect("a negative gutter");
    assert_eq!(body(&s).col_spacing, Some(0.0));
}

/// Wrap is a boolean the dialog sets both ways, and the command's default is on.
#[test]
fn wrap_is_set_both_ways_and_defaults_to_on() {
    let mut s = shape();
    s.execute("format.wrap", &json!({"on": false})).expect("no wrap");
    assert_eq!(body(&s).wrap, Some(false));
    s.execute("format.wrap", &json!({"on": true})).expect("wrap");
    assert_eq!(body(&s).wrap, Some(true));
    s.execute("format.wrap", &json!({"on": false})).expect("no wrap again");
    s.execute("format.wrap", &json!({})).expect("a bare click turns it back on");
    assert_eq!(body(&s).wrap, Some(true));
}

// ---------------------------------------------------------------------------------------------
// Changing the case of the text itself
// ---------------------------------------------------------------------------------------------

/// With no text selected, Change Case rewrites every run of the selected shapes. A bare call is the
/// Shift+F3 cycle, which walks lower → title → upper and round again.
#[test]
fn change_case_rewrites_the_text_and_cycles_without_a_mode() {
    let mut s = shape();
    assert_eq!(words(&s), "Hello");
    s.execute("format.case", &json!({"mode": "upper"})).expect("upper case");
    assert_eq!(words(&s), "HELLO");
    s.execute("format.case", &json!({"mode": "lower"})).expect("lower case");
    assert_eq!(words(&s), "hello");

    s.execute("format.case", &json!({})).expect("cycle from lower case");
    assert_eq!(words(&s), "Hello", "lower case cycles to capitalising each word");
    s.execute("format.case", &json!({})).expect("cycle again");
    assert_eq!(words(&s), "HELLO", "and then to upper case");
    s.execute("format.case", &json!({})).expect("cycle once more");
    assert_eq!(words(&s), "hello", "and back to the start");
}
