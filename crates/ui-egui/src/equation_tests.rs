//! Real ribbon clicks and typing exercise the on-slide OMML editor, with optional previews.
use crate::{Services, SlideApp};
use deckcraft_engine::Session;
use egui_kittest::{Harness, kittest::Queryable};
use serde_json::json;

fn harness(inline: bool) -> Harness<'static, SlideApp> {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout":"blank"})).unwrap();
    if inline {
        s.execute("insert.textBox", &json!({"text":"Some normal text ","rect":[90,200,700,80]})).unwrap();
        s.execute("text.edit", &json!({})).unwrap();
    }
    let mut app = SlideApp::new(s, Services::default());
    app.ui.tab = "Insert".into();
    let mut h = Harness::builder().with_size(egui::vec2(1600.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}
fn linear(h: &mut Harness<'_, SlideApp>) -> String {
    h.state_mut().session.execute("equation.get", &json!({})).unwrap()["linear"].as_str().unwrap().into()
}
fn preview(h: &mut Harness<'_, SlideApp>, name: &str) {
    if let Ok(dir) = std::env::var("EQUATION_PREVIEW_DIR") {
        h.render().unwrap().save(std::path::Path::new(&dir).join(name)).unwrap();
    }
}
fn click(h: &mut Harness<'_, SlideApp>, p: egui::Pos2) {
    h.hover_at(p);
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
        h.run_steps(1);
    }
    h.run_steps(3);
}
fn math_point(h: &Harness<'_, SlideApp>) -> egui::Pos2 {
    let app = h.state();
    let st = app.session.active().unwrap();
    let t = st.selection.text.as_ref().unwrap();
    let sh = st.shape(t.shape).unwrap();
    let layout = deckcraft_engine::cmd::text::layout_for(st, t).unwrap();
    let rect = layout.equations[0].rect;
    let x = deckcraft_engine::cmd::xfrm_of(&st.doc, &st.selection, sh);
    let tr = deckcraft_render::shape_geometry(sh, x.w, x.h).text_rect;
    let m = deckcraft_render::text_transform(x.affine(), tr, layout.rotation);
    let p = m * rect.center();
    app.slide_rect.unwrap().min + egui::vec2(p.x as f32, p.y as f32) * app.canvas_scale
}
#[test]
fn button_types_immediately_with_command_conversion_and_native_structures() {
    for inline in [false, true] {
        let mut h = harness(inline);
        let before = h.state().session.active().unwrap().shapes().len();
        h.get_by_label("Equation").click();
        h.run_steps(3);
        assert!(h.state().dialog.is_none());
        assert_eq!(h.state().ui.tab, "Equation");
        assert_eq!(h.state().session.active().unwrap().shapes().len(), before + usize::from(!inline));
        assert_eq!(linear(&mut h), "");
        preview(&mut h, if inline { "inline-placeholder.png" } else { "placeholder.png" });
        for c in "\\alpha".chars() {
            h.event(egui::Event::Text(c.to_string()));
            h.run_steps(1);
        }
        assert_eq!(linear(&mut h), "α");
        h.event(egui::Event::Text("+a/b".into()));
        h.run_steps(3);
        let math = h.state_mut().session.execute("equation.get", &json!({})).unwrap();
        assert!(math["omml"].as_str().unwrap().contains("<m:f>"));
        preview(&mut h, if inline { "inline-fraction.png" } else { "fraction.png" });
    }
}
#[test]
fn home_font_picker_changes_the_active_equation_and_its_layout() {
    if !deckcraft_fonts::FontDb::global().has_family("Latin Modern Math") {
        return; // Optional craft-fonts input; CI prepares this family.
    }
    let mut h = harness(true);
    h.get_by_label("Equation").click();
    h.run_steps(3);
    h.event(egui::Event::Text("x^2+\\alpha+a/b".into()));
    h.run_steps(3);
    let width = |h: &Harness<'_, SlideApp>| {
        let st = h.state().session.active().unwrap();
        deckcraft_engine::cmd::text::layout_for(st, st.selection.text.as_ref().unwrap()).unwrap().equations[0].rect.width()
    };
    let before = width(&h);
    let content = linear(&mut h);
    h.get_by_label("Home tab").click();
    h.run_steps(3);
    h.get_by_label("Font").click();
    h.run_steps(3);
    h.get_by_label("Latin Modern Math").scroll_to_me();
    h.run_steps(3);
    h.get_by_label("Latin Modern Math").click();
    h.run_steps(3);
    assert_eq!(h.state_mut().session.execute("format.state", &json!({})).unwrap()["font"], "Latin Modern Math");
    assert_eq!(linear(&mut h), content);
    assert!((width(&h) - before).abs() > 0.1);
    preview(&mut h, "latin-modern-math.png");
    let p = math_point(&h);
    click(&mut h, p);
    h.event(egui::Event::Text("z".into()));
    h.run_steps(3);
    assert!(linear(&mut h).contains('z'));
    preview(&mut h, "latin-modern-math-edit.png");
}

#[test]
fn arrow_exit_restores_tab_and_plain_text_and_click_reenters() {
    let mut h = harness(true);
    h.get_by_label("Equation").click();
    h.run_steps(3);
    h.event(egui::Event::Text("x+y".into()));
    h.run_steps(3);
    let p = math_point(&h);
    preview(&mut h, "active.png");
    h.key_press(egui::Key::ArrowRight);
    h.run_steps(3);
    assert_eq!(h.state().ui.tab, "Insert");
    h.event(egui::Event::Text(" normal again".into()));
    h.run_steps(3);
    let state = h.state_mut().session.execute("format.state", &json!({})).unwrap();
    assert_ne!(state["italic"], true);
    let text = h.state_mut().session.execute("text.get", &json!({})).unwrap();
    assert_eq!(text["text"], "Some normal text x+y normal again");
    preview(&mut h, "exited.png");
    h.get_by_label("Home tab").click();
    h.run_steps(3);
    click(&mut h, p);
    assert_eq!(h.state().ui.tab, "Equation");
    let st = h.state().session.active().unwrap();
    assert!(deckcraft_engine::cmd::equation::active(st).is_some());
    h.get_by_label("Insert tab").click();
    h.run_steps(3);
    assert_eq!(h.state().ui.tab, "Insert");
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert_eq!(h.state().ui.tab, "Home");
    assert!(h.state().equation_previous_tab.is_none());
    click(&mut h, p);
    assert_eq!(h.state().ui.tab, "Equation");
}

#[test]
fn ribbon_galleries_edit_the_current_inline_fraction() {
    let mut h = harness(true);
    h.get_by_label("Equation").click();
    h.run_steps(3);
    let before = h.state().session.active().unwrap().shapes().len();
    h.get_by_label("Fraction").click();
    h.run_steps(3);
    h.get_by_label("Stacked Fraction").click();
    h.run_steps(3);
    preview(&mut h, "gallery-empty-fraction.png");
    h.event(egui::Event::Text("a".into()));
    h.run_steps(2);
    h.key_press(egui::Key::Tab);
    h.event(egui::Event::Text("b".into()));
    h.run_steps(3);
    assert_eq!(linear(&mut h), "a/b");
    assert_eq!(h.state().session.active().unwrap().shapes().len(), before);
    preview(&mut h, "gallery-fraction.png");
    h.get_by_label("Close Equation").click();
    h.run_steps(3);
    assert_eq!(h.state().ui.tab, "Insert");
}
