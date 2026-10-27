use deckcraft_model::text::{Action, Hyperlink, Run, TextBody};
use deckcraft_model::{Presentation, SlideId};

use super::{
    CurrentLink, HyperlinkSeed, common_click, hyperlink_command, hyperlink_seed_of, hyperlink_should_apply, link_covering, seed_from, seed_of_link,
};

fn link(action: Action, tip: &str) -> Hyperlink {
    Hyperlink { action, tooltip: tip.into(), highlight_click: false, sound: None, stop_sound: false, zoom_return: None }
}

fn url(address: &str, tip: &str) -> Hyperlink {
    let mut h = link(Action::Url { url: address.into() }, tip);
    h.highlight_click = tip == "glow";
    h
}

/// Header & Footer ▸ Apply without picking a format leaves a slide's existing date field in
/// its own format instead of rewriting it to the default one.
#[test]
fn header_footer_apply_keeps_an_existing_date_format() {
    use std::sync::Arc;

    use egui::vec2;
    use egui_kittest::kittest::Queryable as _;
    use serde_json::json;

    use crate::SlideApp;
    let mut session = deckcraft_engine::Session::with_new();
    session.execute("design.headerFooter", &json!({"date": true, "dateText": "", "dateFormat": "datetime4", "all": true})).unwrap();
    // A file whose document-level format is unset, as an imported deck's is.
    Arc::make_mut(&mut session.doc_mut().unwrap().doc).header_footer.date_format = String::new();
    let date_fields = |app: &SlideApp| -> Vec<String> {
        app.session
            .doc()
            .unwrap()
            .doc
            .slides
            .iter()
            .flat_map(|sl| sl.shapes.iter())
            .filter(|sh| sh.ph_type() == Some(deckcraft_model::PhType::Date))
            .flat_map(|sh| sh.text.iter())
            .flat_map(|t| t.paragraphs.iter())
            .flat_map(|p| p.runs.iter())
            .filter_map(|r| match &r.kind {
                deckcraft_model::text::RunKind::Field { field } => Some(field.clone()),
                _ => None,
            })
            .collect()
    };
    let mut harness = egui_kittest::Harness::builder().with_size(vec2(1200.0, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(session, crate::Services::default()),
    );
    harness.run_steps(2);
    assert_eq!(date_fields(harness.state()), vec!["datetime4".to_string()]);
    harness.state_mut().run("app.dialog", json!({"id": "headerFooter"})).unwrap();
    harness.run_steps(2);
    harness.get_by_label("Apply").click_accesskit();
    harness.run_steps(3);
    assert_eq!(date_fields(harness.state()), vec!["datetime4".to_string()], "the dialog left the field's format alone");
}

/// The Export… dialog (Animated GIF) shows the GIF options and exports through `file.export` (#171).
#[test]
fn export_dialog_writes_an_animated_gif() {
    use std::cell::RefCell;

    use egui::vec2;
    use egui_kittest::kittest::Queryable as _;
    use serde_json::json;

    use crate::SlideApp;
    let dir = std::env::temp_dir().join(format!("deckcraft-ui-gif-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("deck.gif");
    let picked = path.to_string_lossy().to_string();
    let suggested = std::rc::Rc::new(RefCell::new(String::new()));
    let seen = suggested.clone();
    let services = crate::Services {
        pick_save: Some(Box::new(move |s: &str| {
            *seen.borrow_mut() = s.to_string();
            Some(picked.clone())
        })),
        ..Default::default()
    };
    let mut session = deckcraft_engine::Session::with_new();
    session.execute("slide.new", &json!({})).unwrap();
    let mut harness = egui_kittest::Harness::builder().with_size(vec2(1200.0, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(session, services),
    );
    harness.run_steps(2);
    harness.state_mut().run("app.exportDialog", json!({})).unwrap();
    harness.run_steps(2);
    harness.get_by_label("Animated GIF (.gif)").click_accesskit();
    harness.run_steps(2);
    assert_eq!(harness.state().dialog.as_ref().and_then(|d| d.fields.get("fmt")).map(String::as_str), Some("gif"));
    harness.get_by_label("Seconds per slide:");
    harness.get_by_label("Loop forever").click_accesskit();
    harness.run_steps(2);
    // A range outside the deck keeps the dialog open with a message, and nothing is written.
    harness.state_mut().dialog.as_mut().unwrap().fields.insert("gifTo".into(), "9".into());
    harness.run_steps(2);
    harness.key_press(egui::Key::Enter);
    harness.run_steps(2);
    assert!(harness.state().dialog.is_some());
    assert!(harness.state().status.as_ref().is_some_and(|(m, _)| m.contains("Slides")), "{:?}", harness.state().status);
    assert!(!path.exists());
    harness.state_mut().dialog.as_mut().unwrap().fields.insert("gifTo".into(), "2".into());
    harness.run_steps(2);
    harness.key_press(egui::Key::Enter);
    harness.run_steps(2);
    assert!(harness.state().dialog.is_none(), "the dialog closes after exporting");
    assert!(suggested.borrow().ends_with(".gif"), "{}", suggested.borrow());
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.starts_with(b"GIF89a"));
    assert_eq!(u16::from_le_bytes([bytes[6], bytes[7]]), 960, "medium by default");
    assert!(!bytes.windows(11).any(|w| w == b"NETSCAPE2.0"), "Loop forever unticked: plays once");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The Export… dialog offers "Transparent background" for PNG only, and the picture it
/// writes then leaves the slide background out (#784).
#[test]
fn export_dialog_writes_a_png_with_a_transparent_background() {
    use egui::vec2;
    use egui_kittest::kittest::Queryable as _;
    use serde_json::json;

    use crate::SlideApp;
    let dir = std::env::temp_dir().join(format!("deckcraft-ui-png-clear-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("slide.png");
    let picked = path.to_string_lossy().to_string();
    let services = crate::Services { pick_save: Some(Box::new(move |_: &str| Some(picked.clone()))), ..Default::default() };
    let mut harness = egui_kittest::Harness::builder().with_size(vec2(1200.0, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(deckcraft_engine::Session::with_new(), services),
    );
    harness.run_steps(2);
    harness.state_mut().run("app.exportDialog", json!({})).unwrap();
    harness.run_steps(2);
    assert!(harness.query_by_label("Transparent background").is_none(), "PDF has no transparent background");
    harness.get_by_label("JPEG image (current slide)").click_accesskit();
    harness.run_steps(2);
    assert!(harness.query_by_label("Transparent background").is_none(), "nor has JPEG");
    harness.get_by_label("PNG images (current slide)").click_accesskit();
    harness.run_steps(2);
    harness.get_by_label("Transparent background").click_accesskit();
    harness.run_steps(2);
    assert_eq!(harness.state().dialog.as_ref().and_then(|d| d.fields.get("pngTransparent")).map(String::as_str), Some("true"));
    harness.key_press(egui::Key::Enter);
    harness.run_steps(2);
    assert!(harness.state().dialog.is_none(), "the dialog closes after exporting");
    let img = deckcraft_render::decode_image(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(img.get_pixel(2, 2).0[3], 0, "the background is left out");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn no_link_opens_as_a_new_web_address() {
    let seed = seed_from(CurrentLink::None, &[]);
    assert_eq!(seed, HyperlinkSeed::blank());
    assert!(hyperlink_should_apply(&seed, "url", "https://", "", 0));
}

#[test]
fn web_email_and_slide_links_fill_the_dialog() {
    let ids = [SlideId(7), SlideId(9)];
    let web = seed_of_link(&url("https://example.org", "See"), &ids);
    assert_eq!(web.mode, "url");
    assert_eq!(web.url, "https://example.org");
    assert_eq!(web.tip, "See");
    assert!(!web.preserve);
    assert!(hyperlink_should_apply(&web, "url", "https://example.org", "", 0));

    let mail = seed_of_link(&url("mailto:a@b.org", "Write"), &ids);
    assert_eq!(mail.mode, "email");
    assert_eq!(mail.email, "a@b.org");
    assert_eq!(mail.tip, "Write");
    assert_eq!(hyperlink_command("email", "", "a@b.org", 0, "Write", false)["url"], "mailto:a@b.org");

    let slide = seed_of_link(&link(Action::Slide { slide: SlideId(9) }, "There"), &ids);
    assert_eq!(slide.mode, "slide");
    assert_eq!(slide.slide, 1);
    assert_eq!(slide.tip, "There");
    assert!(!slide.preserve);

    let glow = url("https://example.org", "glow");
    assert!(glow.highlight_click);
    assert!(seed_of_link(&glow, &ids).highlight);
    assert_eq!(hyperlink_command("url", "https://example.org", "", 0, "glow", true)["highlightClick"], true);
}

#[test]
fn actions_this_dialog_does_not_edit_survive_ok() {
    let next = seed_of_link(&link(Action::NextSlide, "On"), &[]);
    assert!(next.preserve);
    assert_eq!(next.tip, "On");
    assert_eq!(next.url, "");
    assert!(!hyperlink_should_apply(&next, "url", "", "", 0));
    assert!(hyperlink_should_apply(&next, "url", "https://example.org", "", 0));

    let gone = seed_of_link(&link(Action::Slide { slide: SlideId(99) }, ""), &[SlideId(1)]);
    assert!(gone.preserve, "a missing slide is not retargeted to the first slide");
    assert!(!hyperlink_should_apply(&gone, "url", "", "", 0));

    let mixed = seed_from(CurrentLink::Mixed, &[]);
    assert!(mixed.preserve);
    assert!(!hyperlink_should_apply(&mixed, "url", "", "", 0));
    assert!(hyperlink_should_apply(&mixed, "email", "", "a@b.org", 0));
}

#[test]
fn a_text_span_reports_the_link_it_covers() {
    let mut body = TextBody::from_text("Hello world");
    body.paragraphs[0].runs.clear();
    body.paragraphs[0].runs.push(Run::new("Hello "));
    let mut world = Run::new("world");
    world.props.link = Some(url("https://example.org", "tip"));
    body.paragraphs[0].runs.push(world);

    assert_eq!(link_covering(&body, (0, 6), (0, 6)), CurrentLink::None, "a caret inserts; it does not edit the run beside it");
    match link_covering(&body, (0, 6), (0, 11)) {
        CurrentLink::One(h) => assert_eq!(h.tooltip, "tip"),
        other => panic!("linked word, got {other:?}"),
    }
    assert_eq!(link_covering(&body, (0, 0), (0, 11)), CurrentLink::Mixed);

    let mut other = url("https://example.org", "tip");
    other.tooltip = "other".into();
    assert_eq!(common_click([None, None]), CurrentLink::None);
    assert!(matches!(common_click([Some(url("https://example.org", "tip"))]), CurrentLink::One(_)));
    assert_eq!(common_click([Some(url("https://example.org", "tip")), Some(other)]), CurrentLink::Mixed);
    assert_eq!(common_click(std::iter::empty()), CurrentLink::None);
}

#[test]
fn the_open_dialog_reads_the_caret_word_and_the_shape_click() {
    let mut doc = Presentation::default();
    let shape_id = {
        let slide = std::sync::Arc::make_mut(&mut doc.slides[0]);
        let mut body = TextBody::from_text("Hello world");
        body.paragraphs[0].runs.clear();
        body.paragraphs[0].runs.push(Run::new("Hello "));
        let mut world = Run::new("world");
        world.props.link = Some(url("https://example.org", "tip"));
        body.paragraphs[0].runs.push(world);
        slide.shapes[0].text = Some(body);
        slide.shapes[0].id
    };
    let mut st = deckcraft_engine::DocState::new(doc, None, "t".into());
    st.selection.text = Some(deckcraft_engine::TextSel { shape: shape_id, anchor: (0, 8), caret: (0, 8), ..Default::default() });
    let seed = hyperlink_seed_of(Some(&st));
    assert_eq!(seed.mode, "url");
    assert_eq!(seed.url, "https://example.org");
    assert_eq!(seed.tip, "tip");
    assert!(!seed.preserve);

    st.selection.text = None;
    st.selection.shapes = vec![shape_id];
    {
        let doc = std::sync::Arc::make_mut(&mut st.doc);
        let slide = std::sync::Arc::make_mut(&mut doc.slides[0]);
        let shape = slide.shapes.iter_mut().find(|sh| sh.id == shape_id).unwrap();
        let mut click = url("https://shape.example", "shape tip");
        click.highlight_click = true;
        shape.click = Some(click);
    }
    let seed = hyperlink_seed_of(Some(&st));
    assert_eq!(seed.url, "https://shape.example");
    assert_eq!(seed.tip, "shape tip");
    assert!(seed.highlight);

    st.selection.text = Some(deckcraft_engine::TextSel { shape: shape_id, anchor: (0, 11), caret: (0, 11), ..Default::default() });
    let seed = hyperlink_seed_of(Some(&st));
    assert_eq!(seed, HyperlinkSeed::blank(), "a caret at the end of the word inserts a new link");
}
