//! Widget-level tests, ported from WordCraft's `eqdialog.rs` kittest tests (those that do not
//! need the document) plus DeckCraft's clipboard keys and checks for a shared context.

use egui::{Key, Modifiers, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use equashion_math::{Caret, Node, to_linear, units};

use super::*;

/// The host: one editor (optionally a second one and a host text field), counting what the
/// responses reported.
struct App {
    ed: EquationEditor,
    other: Option<EquationEditor>,
    host_text: Option<String>,
    committed: u32,
    cancelled: u32,
    changed: u32,
    /// Ask for a second (discarded) pass every frame.
    discard: bool,
}

impl App {
    fn new(ed: EquationEditor) -> Self {
        App { ed, other: None, host_text: None, committed: 0, cancelled: 0, changed: 0, discard: false }
    }
}

fn harness(app: App) -> Harness<'static, App> {
    Harness::builder().with_size(vec2(820.0, 760.0)).build_ui_state(
        |ui, a: &mut App| {
            if a.discard && ui.ctx().current_pass_index() == 0 {
                ui.ctx().request_discard("test: a second pass");
            }
            if let Some(t) = a.host_text.as_mut() {
                ui.add(egui::TextEdit::singleline(t).id(egui::Id::new("host-field")));
            }
            let r = a.ed.show(ui);
            a.committed += u32::from(r.committed);
            a.cancelled += u32::from(r.cancelled);
            a.changed += u32::from(r.changed);
            if let Some(o) = a.other.as_mut() {
                o.show(ui);
            }
        },
        app,
    )
}

fn opened() -> Harness<'static, App> {
    let mut h = harness(App::new(EquationEditor::new()));
    h.run();
    h.run();
    h
}

fn ed<'a>(h: &'a Harness<'static, App>) -> &'a EquationEditor {
    &h.state().ed
}

fn typ(h: &mut Harness<'static, App>, text: &str) {
    h.event(egui::Event::Text(text.into()));
    h.run();
}

fn press(h: &mut Harness<'static, App>, key: Key) {
    h.key_press(key);
    h.run();
}

fn press_mod(h: &mut Harness<'static, App>, m: Modifiers, key: Key) {
    h.key_press_modifiers(m, key);
    h.run();
}

fn click(h: &mut Harness<'static, App>, label: &str) {
    h.get_by_label(label).click_accesskit();
    h.run();
    h.run();
}

fn lin(h: &Harness<'static, App>) -> String {
    to_linear(ed(h).math()).replace(' ', "").replace(['(', ')'], "")
}

fn canvas_focused(h: &Harness<'static, App>) -> bool {
    let id = ed(h).canvas_id();
    h.ctx.memory(|m| m.has_focus(id))
}

/// Screen position of the caret `c` (its vertical middle; `left` at its left edge, else centre).
fn caret_pos(h: &Harness<'static, App>, c: &Caret) -> egui::Pos2 {
    let e = ed(h);
    let cv = e.canvas.as_ref().expect("canvas");
    let r = cv.layout.caret_rect(c).expect("caret rect");
    let (o, k) = e.view;
    pos2(o.x + ((r.x0 + r.x1) / 2.0) as f32 * k, o.y + ((r.y0 + r.y1) / 2.0) as f32 * k)
}

fn mouse(h: &mut Harness<'static, App>, p: egui::Pos2, pressed: bool, m: Modifiers) {
    h.event_modifiers(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: m }, m);
    h.run();
}

#[test]
fn typing_x_caret_2_raises_the_exponent_at_once() {
    let mut h = opened();
    assert!(canvas_focused(&h), "the canvas takes the keyboard when it opens");
    typ(&mut h, "x");
    typ(&mut h, "^");
    assert_eq!(ed(&h).editor().caret().path.len(), 1);
    typ(&mut h, "2");
    assert_eq!(lin(&h), "x^2");
    let e = ed(&h);
    assert!(matches!(e.math().body.first(), Some(Node::Script { sup: Some(_), .. })), "{:?}", e.math().body);
    let cv = e.canvas.as_ref().expect("canvas");
    let sup = cv.layout.caret_rect(e.editor().caret()).expect("sup caret");
    let base = cv.layout.caret_rect(&Caret { path: vec![], pos: 0 }).expect("base caret");
    assert!(sup.y0 < base.y0, "{sup:?} {base:?}");
    assert!(cv.tex.size()[0] > 1);
    press(&mut h, Key::ArrowRight);
    assert!(ed(&h).editor().caret().path.is_empty());
    typ(&mut h, "+1");
    assert_eq!(lin(&h), "x^2+1");
    assert_eq!(ed(&h).linear.replace(' ', ""), "x^2+1", "the linear field follows");
    assert_eq!(ed(&h).linear().replace(' ', ""), "x^2+1");
    assert!(ed(&h).omml().contains("<m:sSup>"), "{}", ed(&h).omml());
    assert!(h.state().changed >= 3);
}

#[test]
fn underscore_slash_sqrt_and_greek_names() {
    let mut h = opened();
    typ(&mut h, "a_1");
    assert_eq!(lin(&h), "a_1");
    press(&mut h, Key::ArrowRight);
    typ(&mut h, "/");
    assert!(matches!(ed(&h).math().body.first(), Some(Node::Frac { .. })));
    assert_eq!(ed(&h).editor().caret().path, vec![(0, 1)]);
    typ(&mut h, "sqrt");
    typ(&mut h, "alpha");
    let l = lin(&h);
    assert!(l.contains('\u{221A}') && l.contains('\u{3B1}'), "{l}");
}

#[test]
fn arrows_and_backspace_edit_the_tree() {
    let mut h = opened();
    typ(&mut h, "a/b");
    press(&mut h, Key::ArrowUp);
    assert_eq!(ed(&h).editor().caret().path, vec![(0, 0)], "Up goes to the numerator");
    press(&mut h, Key::ArrowDown);
    assert_eq!(ed(&h).editor().caret().path, vec![(0, 1)]);
    press(&mut h, Key::ArrowLeft);
    press(&mut h, Key::ArrowLeft);
    assert_eq!(ed(&h).editor().caret().path, vec![(0, 0)], "Left crosses into the numerator");
    press(&mut h, Key::Home);
    press(&mut h, Key::Backspace);
    assert_eq!(lin(&h), "ab", "Backspace at the start unwraps the fraction");
    press(&mut h, Key::Delete);
    assert_eq!(lin(&h), "b");
    press(&mut h, Key::End);
    press(&mut h, Key::Backspace);
    assert!(ed(&h).editor().is_blank());
    assert!(canvas_focused(&h), "the arrow keys did not move the focus away");
}

#[test]
fn ctrl_z_and_ctrl_shift_z_undo_and_redo() {
    let mut h = opened();
    typ(&mut h, "x+a/b");
    press(&mut h, Key::ArrowRight);
    press(&mut h, Key::Backspace);
    assert_eq!(lin(&h), "x+a/b", "the first Backspace only steps inside");
    press(&mut h, Key::Backspace);
    assert_eq!(lin(&h), "x+a/");
    press_mod(&mut h, Modifiers::COMMAND, Key::Z);
    assert_eq!(lin(&h), "x+a/b", "Ctrl+Z brings the atom back");
    press_mod(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    assert_eq!(lin(&h), "x+a/");
    press_mod(&mut h, Modifiers::COMMAND, Key::Z);
    press_mod(&mut h, Modifiers::COMMAND, Key::Y);
    assert_eq!(lin(&h), "x+a/", "Ctrl+Y redoes too");
}

fn copied(h: &Harness<'static, App>) -> Option<String> {
    h.output().platform_output.commands.iter().rev().find_map(|c| match c {
        egui::OutputCommand::CopyText(t) => Some(t.clone()),
        _ => None,
    })
}

/// Events delivered together in one frame; returns the text that frame copied.
fn frame_with(h: &mut Harness<'static, App>, events: Vec<egui::Event>) -> Option<String> {
    h.input_mut().events.extend(events);
    h.step();
    let c = copied(h);
    h.run();
    c
}

fn cmd(key: Key) -> egui::Event {
    egui::Event::Key { key, pressed: true, modifiers: Modifiers::COMMAND, repeat: false, physical_key: None }
}

#[test]
fn ctrl_c_copies_and_ctrl_x_cuts_the_selection_as_linear_text() {
    let mut h = opened();
    typ(&mut h, "a+b");
    press_mod(&mut h, Modifiers::SHIFT, Key::ArrowLeft);
    assert_eq!(frame_with(&mut h, vec![cmd(Key::C)]).as_deref(), Some("b"));
    assert_eq!(lin(&h), "a+b", "copy leaves the equation alone");
    press_mod(&mut h, Modifiers::COMMAND, Key::A);
    assert_eq!(frame_with(&mut h, vec![cmd(Key::X)]).map(|s| s.replace(' ', "")).as_deref(), Some("a+b"));
    assert!(ed(&h).editor().is_blank(), "cut removed it");
    // A platform Cut event and the key in the same frame cut once.
    typ(&mut h, "xyz");
    press_mod(&mut h, Modifiers::SHIFT, Key::ArrowLeft);
    assert_eq!(frame_with(&mut h, vec![egui::Event::Cut, cmd(Key::X)]).as_deref(), Some("z"));
    assert_eq!(lin(&h), "xy", "one cut, not two");
    press_mod(&mut h, Modifiers::COMMAND, Key::Z);
    assert_eq!(lin(&h), "xyz", "a cut undoes");
}

#[test]
fn tab_stays_in_the_canvas_and_moves_between_places() {
    let mut h = opened();
    typ(&mut h, "a/b");
    press(&mut h, Key::ArrowUp);
    press(&mut h, Key::Tab);
    assert_eq!(ed(&h).editor().caret().path, vec![(0, 1)]);
    assert!(canvas_focused(&h), "Tab did not move the focus to another widget");
    typ(&mut h, "c");
    assert_eq!(lin(&h), "a/cb", "typing after Tab goes to the tree, not the linear field");
    press_mod(&mut h, Modifiers::SHIFT, Key::Tab);
    assert_eq!(ed(&h).editor().caret().path, vec![(0, 0)], "Shift+Tab goes back");
}

#[test]
fn escape_with_unsaved_work_asks_before_discarding() {
    let mut h = opened();
    typ(&mut h, "x^2");
    press(&mut h, Key::Escape);
    assert_eq!(h.state().cancelled, 0, "the first Escape only warns");
    assert_eq!(ed(&h).message(), Some(DISCARD_PROMPT));
    assert!(canvas_focused(&h), "Escape does not drop the canvas focus");
    h.get_by_label(DISCARD_PROMPT);
    press(&mut h, Key::Escape);
    assert_eq!(h.state().cancelled, 1, "the second Escape cancels");
    assert!(ed(&h).message().is_none());
    h.state_mut().ed.revert();
    assert!(ed(&h).editor().is_blank());

    // Typing after the warning disarms it.
    typ(&mut h, "y");
    press(&mut h, Key::Escape);
    typ(&mut h, "z");
    press(&mut h, Key::Escape);
    assert_eq!(h.state().cancelled, 1, "the warning starts over after an edit");

    let mut h = opened();
    press(&mut h, Key::Escape);
    assert_eq!(h.state().cancelled, 1, "nothing typed: cancels at once");
}

#[test]
fn enter_commits_and_resets_the_baseline() {
    let mut h = opened();
    typ(&mut h, "x^2");
    press(&mut h, Key::Enter);
    assert_eq!(h.state().committed, 1);
    assert!(!ed(&h).is_dirty());
    press(&mut h, Key::Escape);
    assert_eq!(h.state().cancelled, 1, "after a commit nothing is lost: Esc cancels at once");
}

#[test]
fn shift_arrows_select_and_a_palette_click_wraps_the_selection() {
    let mut h = opened();
    typ(&mut h, "xy");
    press_mod(&mut h, Modifiers::SHIFT, Key::ArrowLeft);
    press_mod(&mut h, Modifiers::SHIFT, Key::ArrowLeft);
    assert_eq!(ed(&h).editor().selection(), Some((0, 2)));
    click(&mut h, "\u{221A}");
    assert!(matches!(ed(&h).math().body.first(), Some(Node::Rad { body, .. }) if units(body) == 2), "{:?}", ed(&h).math().body);
    typ(&mut h, "z");
    assert_eq!(lin(&h), "\u{221A}xyz");
}

#[test]
fn the_quadratic_formula_types_straight_through() {
    let mut h = opened();
    typ(&mut h, "x=(-b+-sqrt(b^2");
    press(&mut h, Key::ArrowRight);
    typ(&mut h, "-4ac))/2a");
    let l = lin(&h);
    assert!(l.contains('\u{B1}') && l.contains('\u{221A}') && l.ends_with("/2a"), "{l}");
    assert!(matches!(ed(&h).math().body.last(), Some(Node::Frac { .. })));
}

#[test]
fn the_caret_and_typing_survive_a_palette_click() {
    let mut h = opened();
    click(&mut h, "a/b");
    assert!(canvas_focused(&h), "the canvas keeps the keyboard after a palette click");
    let e = ed(&h);
    let cv = e.canvas.as_ref().expect("canvas");
    assert_eq!(e.editor().caret().path, vec![(0, 0)], "the caret is in the new numerator");
    assert!(cv.layout.caret_rect(e.editor().caret()).is_some(), "and it has a rectangle to draw");
    click(&mut h, "x^n");
    typ(&mut h, "2");
    assert!(lin(&h).contains('2'), "{}", lin(&h));
}

#[test]
fn palette_structures_insert_at_the_caret() {
    let mut h = opened();
    typ(&mut h, "b");
    click(&mut h, "x^n");
    typ(&mut h, "2");
    assert_eq!(lin(&h), "b^2");
    press(&mut h, Key::ArrowRight);
    click(&mut h, "a/b");
    assert!(matches!(ed(&h).math().body.last(), Some(Node::Frac { .. })), "{:?}", ed(&h).math().body);
}

#[test]
fn greek_and_operator_tabs_insert_symbols() {
    let mut h = opened();
    h.get_by_label("Greek").click();
    h.run();
    click(&mut h, "\u{3B8}");
    h.get_by_label("Operators").click();
    h.run();
    click(&mut h, "\u{2207}");
    click(&mut h, "\u{2264}");
    assert_eq!(lin(&h), "\u{3B8}\u{2207}\u{2264}");
    assert!(canvas_focused(&h));
}

#[test]
fn empty_places_show_boxes_and_the_caret_has_a_rect() {
    let mut h = opened();
    typ(&mut h, "a/");
    let e = ed(&h);
    assert!(to_linear(&e.editor().display_math()).contains('\u{25A1}'));
    assert!(!to_linear(e.math()).contains('\u{25A1}'));
    let cv = e.canvas.as_ref().expect("canvas");
    assert!(cv.layout.caret_rect(e.editor().caret()).is_some(), "caret has a rectangle inside the empty denominator");
}

#[test]
fn clicking_the_canvas_places_the_caret() {
    let mut h = opened();
    typ(&mut h, "ab+cd");
    let e = ed(&h);
    let r = e.canvas.as_ref().and_then(|c| c.layout.caret_rect(&Caret { path: vec![], pos: 1 })).expect("rect");
    let (o, k) = e.view;
    let hit = pos2(o.x + r.x0 as f32 * k, o.y + (r.y0 + r.y1) as f32 * 0.5 * k);
    h.event(egui::Event::PointerMoved(hit));
    mouse(&mut h, hit, true, Modifiers::NONE);
    mouse(&mut h, hit, false, Modifiers::NONE);
    assert_eq!(ed(&h).editor().caret().pos, 1, "caret between a and b");
    typ(&mut h, "x");
    assert_eq!(lin(&h), "axb+cd");
}

#[test]
fn shift_click_extends_the_selection() {
    let mut h = opened();
    typ(&mut h, "abcd");
    let p1 = caret_pos(&h, &Caret { path: vec![], pos: 1 });
    let p3 = caret_pos(&h, &Caret { path: vec![], pos: 3 });
    h.event(egui::Event::PointerMoved(p1));
    h.run();
    mouse(&mut h, p1, true, Modifiers::NONE);
    mouse(&mut h, p1, false, Modifiers::NONE);
    h.event(egui::Event::PointerMoved(p3));
    h.run();
    mouse(&mut h, p3, true, Modifiers::SHIFT);
    mouse(&mut h, p3, false, Modifiers::SHIFT);
    assert_eq!(ed(&h).editor().selection(), Some((1, 3)), "{:?}", ed(&h));
}

#[test]
fn a_drag_from_a_numerator_to_the_top_row_selects_whole_structures() {
    let mut h = opened();
    typ(&mut h, "a/b");
    press(&mut h, Key::ArrowRight);
    typ(&mut h, "+c");
    assert_eq!(lin(&h), "a/b+c");
    let from = Caret { path: vec![(0, 0)], pos: 0 };
    let to = Caret { path: vec![], pos: units(&ed(&h).math().body) };
    let (p0, p1) = (caret_pos(&h, &from), caret_pos(&h, &to));
    // What the edit model says such a drag selects.
    let mut want = ed(&h).editor().clone();
    want.select_between(&from, &to);
    h.event(egui::Event::PointerMoved(p0));
    h.run();
    mouse(&mut h, p0, true, Modifiers::NONE);
    assert_eq!(ed(&h).editor().caret().path, vec![(0, 0)], "the press lands in the numerator");
    for t in [0.3, 0.6, 1.0] {
        h.event(egui::Event::PointerMoved(p0 + (p1 - p0) * t));
        h.run();
    }
    mouse(&mut h, p1, false, Modifiers::NONE);
    let e = ed(&h).editor();
    assert!(e.caret().path.is_empty(), "the selection moved up to the row holding both ends: {:?}", e.caret());
    assert_eq!(e.selection(), want.selection());
    let (a, b) = e.selection().expect("a selection");
    assert!(a == 0 && b >= 3, "the whole fraction and what follows: {a}..{b}");
}

#[test]
fn linear_field_is_still_a_way_in() {
    let mut h = opened();
    h.get_by_role(egui::accesskit::Role::TextInput).click();
    h.run();
    typ(&mut h, "x^2+1");
    assert_eq!(lin(&h), "x^2+1");
    assert!(ed(&h).canvas.as_ref().is_some_and(|c| c.shown.body.len() >= 2), "the canvas follows the linear field");
    assert!(!canvas_focused(&h), "the field keeps the keyboard");
    press(&mut h, Key::Enter);
    assert_eq!(h.state().committed, 1, "Enter in the field commits");
}

#[test]
fn typing_once_types_once() {
    // A host whose frames take two passes (request_discard) must not see each key twice.
    let mut app = App::new(EquationEditor::new());
    app.discard = true;
    let mut h = harness(app);
    h.run();
    h.run();
    typ(&mut h, "ab");
    typ(&mut h, "c");
    assert_eq!(lin(&h), "abc");
    // Two editors on one context: only the focused one types.
    let mut app = App::new(EquationEditor::new());
    app.other = Some(EquationEditor::new().auto_focus(false));
    let mut h = harness(app);
    h.run();
    h.run();
    typ(&mut h, "q");
    assert_eq!(lin(&h), "q");
    assert!(h.state().other.as_ref().is_some_and(|o| o.editor().is_blank()), "the other editor got nothing");
}

#[test]
fn hostile_input_does_not_panic() {
    let mut h = opened();
    let keys = [Key::ArrowLeft, Key::ArrowRight, Key::ArrowUp, Key::ArrowDown, Key::Home, Key::End, Key::Backspace, Key::Delete, Key::Tab];
    let texts = ["x^", "_", "/", "sqrt", "pi", "(", "2", "^^^", "\\frac{", "}", "\\langle"];
    for i in 0..60usize {
        typ(&mut h, texts[i % texts.len()]);
        press(&mut h, keys[(i * 5 + 3) % keys.len()]);
    }
    assert!(ed(&h).editor().is_valid());
}

#[test]
fn typed_latex_works_in_the_canvas() {
    let mut h = opened();
    typ(&mut h, "\\alpha");
    assert_eq!(lin(&h), "\u{3b1}");
    typ(&mut h, "+\\frac{1}{2}");
    assert_eq!(lin(&h), "\u{3b1}+1/2");
    typ(&mut h, "\\sqrt{x}");
    assert!(lin(&h).ends_with('x'), "{}", lin(&h));
    assert!(ed(&h).math().body.iter().any(|n| matches!(n.bare(), Node::Rad { .. })));
}

#[test]
fn typing_lim_with_a_subscript_leaves_no_empty_box() {
    let mut h = opened();
    typ(&mut h, "lim_x");
    assert!(!ed(&h).linear.contains("()_"), "{}", ed(&h).linear);
    let mut h = opened();
    typ(&mut h, "\\lim_{x\\to0}");
    assert!(!ed(&h).linear.contains("()_"), "{}", ed(&h).linear);
}

#[test]
fn the_widget_leaves_a_host_field_alone() {
    let mut app = App::new(EquationEditor::new().auto_focus(false));
    app.host_text = Some(String::new());
    let mut h = harness(app);
    h.run();
    let host = egui::Id::new("host-field");
    h.ctx.memory_mut(|m| m.request_focus(host));
    h.run();
    h.run();
    typ(&mut h, "hi");
    assert!(h.ctx.memory(|m| m.has_focus(host)), "the editor did not take the keyboard");
    assert_eq!(h.state().host_text.as_deref(), Some("hi"));
    assert!(ed(&h).editor().is_blank());
    press(&mut h, Key::Escape);
    assert_eq!(h.state().cancelled, 0, "Esc in a host widget is not the editor's");
}

#[test]
fn textures_are_namespaced() {
    let mut h = opened();
    typ(&mut h, "x");
    let names: Vec<String> = h.ctx.tex_manager().read().allocated().map(|(_, m)| m.name.clone()).collect();
    let ours: Vec<&String> = names.iter().filter(|n| n.starts_with("equashion/")).collect();
    assert!(ours.len() > 10, "canvas and palette textures: {names:?}");
    assert!(names.iter().all(|n| n.starts_with("equashion/") || n.starts_with("egui")), "{names:?}");
}

#[test]
fn set_linear_with_omml_and_revert() {
    let mut e = EquationEditor::with_linear(r"\frac{a}{b}");
    assert!(matches!(e.math().body.first(), Some(Node::Frac { .. })));
    let l = e.linear();
    e.set_linear(&l);
    assert_eq!(e.linear(), l, "linear round trip");
    let o = e.omml();
    let e2 = EquationEditor::with_omml(&o);
    assert_eq!(e2.omml(), o, "OMML round trip");
    e.editor_mut().type_str("+c");
    e.sync_linear();
    assert!(e.is_dirty());
    e.revert();
    assert_eq!(e.linear(), l);
    assert!(EquationEditor::with_linear(r"\foo x").unknown_commands().iter().any(|c| c.contains("foo")));
}

#[test]
fn every_palette_face_is_drawn_from_the_bundled_fonts() {
    use equashion_layout::{DEFAULT_FAMILY, Prim, Rgb, layout_equation};
    let fonts = bundled_fonts();
    let mut faces: Vec<(String, equashion_math::Math)> =
        equashion_math::templates().into_iter().map(|t| (t.id.to_string(), equashion_math::Editor::new(t.math).display_math())).collect();
    for c in [palette::GREEK_LOWER, palette::GREEK_UPPER, palette::OPERATORS].concat().chars() {
        faces.push((c.to_string(), equashion_math::Math::new(vec![Node::Text(c.to_string())])));
    }
    for (name, m) in faces {
        let l = layout_equation(&*fonts, &m, DEFAULT_FAMILY, 15.0, false, Rgb::WHITE, 1.0);
        let notdef = l.scene.items.iter().any(|p| matches!(p, Prim::Glyphs { glyphs, .. } if glyphs.iter().any(|g| g.0 == 0)));
        assert!(!notdef, "palette face {name} has a missing glyph");
    }
}

/// The galleries a host puts in its ribbon insert into the editor they are called on.
#[test]
fn a_gallery_template_inserts_at_the_caret() {
    let mut h = Harness::builder().with_size(vec2(500.0, 600.0)).build_ui_state(
        |ui, ed: &mut EquationEditor| {
            ed.show(ui);
            ed.gallery_ui(ui, "fraction");
        },
        EquationEditor::new().show_palette(false).id_salt("gallery-test"),
    );
    h.run();
    h.get_by_label("Stacked Fraction").click();
    h.run();
    assert!(matches!(h.state().math().body.first().map(Node::bare), Some(Node::Frac { .. })), "{:?}", h.state().math());
}

#[test]
fn a_gallery_symbol_inserts_at_the_caret() {
    let mut h = Harness::builder().with_size(vec2(500.0, 600.0)).build_ui_state(
        |ui, ed: &mut EquationEditor| {
            ed.show(ui);
            ed.symbols_ui(ui, 1);
        },
        EquationEditor::new().show_palette(false).id_salt("symbol-test"),
    );
    h.run();
    h.get_by_label("π").click();
    h.run();
    assert_eq!(h.state().linear(), "π");
}

#[test]
fn a_ready_made_equation_replaces_the_equation() {
    let mut h = Harness::builder().with_size(vec2(500.0, 700.0)).build_ui_state(
        |ui, ed: &mut EquationEditor| {
            ed.show(ui);
            ed.builtins_ui(ui);
        },
        EquationEditor::new().show_palette(false).id_salt("builtin-test"),
    );
    h.run();
    h.get_by_label("Euler's identity").click();
    h.run();
    assert_eq!(h.state().linear(), "e^(iπ)+1=0");
}

/// Inline mode is only the equation: it sizes itself to what was typed and has no palette.
#[test]
fn inline_mode_is_sized_to_the_equation() {
    let mut h = Harness::builder().with_size(vec2(600.0, 300.0)).build_ui_state(
        |ui, ed: &mut EquationEditor| {
            ed.show(ui);
        },
        EquationEditor::with_linear("x").inline(true).font_size(20.0).id_salt("inline-test"),
    );
    h.run_steps(3);
    let small = h.state().content_size().expect("drew");
    assert!(h.query_by_label("Fractions").is_none() && h.query_by_label("Greek").is_none(), "no palette");
    h.state_mut().set_linear("a^2+b^2=c^2+d^2+e^2");
    h.run_steps(3);
    let big = h.state().content_size().expect("drew");
    assert!(big.x > small.x * 3.0, "{small:?} -> {big:?}");
    h.state_mut().set_font_size(40.0);
    h.run_steps(3);
    assert!(h.state().content_size().expect("drew").y > big.y * 1.5);
}

/// An overlay canvas is exactly the equation's size and says where its baseline is, so a host
/// can lay it over its own drawing of the same equation.
#[test]
fn overlay_canvas_reports_its_baseline() {
    let mut h = Harness::builder().with_size(vec2(600.0, 300.0)).build_ui_state(
        |ui, ed: &mut EquationEditor| {
            ed.show(ui);
        },
        EquationEditor::with_linear("a/b").inline(true).overlay(true).font_size(24.0).id_salt("overlay-test"),
    );
    h.run_steps(4);
    let (size, base) = (h.state().content_size().unwrap(), h.state().inline_baseline().unwrap());
    assert!(base > 0.0 && base < size.y, "baseline {base} inside height {}", size.y);
}

/// Display and text style lay out differently (a sum's limits go above and below in display
/// style), so a host can match how it draws the equation.
#[test]
fn display_style_follows_the_host() {
    let mk = |display: bool| {
        let mut h = Harness::builder().with_size(vec2(600.0, 300.0)).build_ui_state(
            |ui, ed: &mut EquationEditor| {
                ed.show(ui);
            },
            EquationEditor::with_linear("sum_(i=1)^n i").inline(true).font_size(24.0).display_style(display).id_salt("style-test"),
        );
        h.run_steps(3);
        h.state().content_size().expect("drew")
    };
    let (text, display) = (mk(false), mk(true));
    assert!(display.y > text.y + 1.0, "limits stack in display style: {text:?} vs {display:?}");
}
