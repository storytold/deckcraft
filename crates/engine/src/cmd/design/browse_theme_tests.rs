use std::sync::Arc;

use deckcraft_model::text::RunKind;
use deckcraft_model::{Background, Fill, PhType};
use serde_json::json;

use crate::{Session, Target};

#[test]
fn applies_the_theme_of_another_deck_and_undoes() {
    let mut other = Session::with_new();
    other.execute("design.fonts", &json!({"major": "Lato", "minor": "Source Sans 3"})).unwrap();
    other.execute("design.colors", &json!({"colors": {"accent1": "#123456"}})).unwrap();
    let data = other.execute("file.saveBytes", &json!({"format": "pptx"})).unwrap()["data"].as_str().unwrap().to_string();
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "titleAndContent", "title": "T", "body": "B"})).unwrap();
    let before = s.doc().unwrap().doc.masters[0].theme.clone();
    s.execute("design.browseTheme", &json!({"name": "Brand.pptx", "data": data})).unwrap();
    let t = &s.doc().unwrap().doc.masters[0].theme;
    assert_eq!(t.fonts.major.latin, "Lato");
    assert_eq!(t.fonts.minor.latin, "Source Sans 3");
    assert_eq!(t.colors.get(deckcraft_color::SchemeSlot::Accent1).hex().to_lowercase(), "123456");
    assert!(s.doc().unwrap().doc.validate().is_empty());
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc.masters[0].theme, before);
    assert!(s.execute("design.browseTheme", &json!({"data": "AAAA"})).is_err());
    assert!(s.execute("design.browseTheme", &json!({})).is_err());
}

/// A 4:3 brand template: a logo and a background on the master, its layouts renamed (so slides
/// can only find them by kind) and its content layout's title moved.
fn brand_template() -> (String, Vec<u8>, deckcraft_model::Xfrm) {
    let mut img = image::RgbaImage::new(2, 2);
    img.put_pixel(0, 0, image::Rgba([200, 20, 30, 255]));
    let mut logo = Vec::new();
    image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut logo), image::ImageFormat::Png).unwrap();
    let mut t = Session::with_new();
    t.execute("design.slideSize", &json!({"preset": "standard", "scale": "none"})).unwrap();
    let title = t
        .edit(|doc, _| {
            let media = doc.add_media("logo.png", "image/png", logo.clone()).unwrap();
            let id = deckcraft_model::ShapeId(doc.alloc_id().unwrap());
            let m = Arc::make_mut(doc.masters.get_mut(0).unwrap());
            m.shapes.push(deckcraft_model::Shape {
                id,
                name: "Logo".into(),
                kind: deckcraft_model::ShapeKind::Picture { fill: deckcraft_model::style::PictureFill { media, ..Default::default() } },
                xfrm: Some(deckcraft_model::Xfrm { x: 600.0, y: 20.0, w: 100.0, h: 40.0, ..Default::default() }),
                ..Default::default()
            });
            m.background = Some(Background::Fill { fill: Fill::solid(deckcraft_model::ColorRef::rgb(deckcraft_color::Rgba::rgb(0x0A, 0x2B, 0x4C))) });
            for l in &mut m.layouts {
                l.name = format!("Brand {}", l.name);
            }
            let obj = m.layouts.iter_mut().find(|l| l.kind == deckcraft_model::LayoutType::TitleAndContent).unwrap();
            let title = obj.shapes.iter_mut().find(|sh| sh.ph_type().is_some_and(PhType::is_title)).unwrap();
            let x = deckcraft_model::Xfrm { x: 72.0, y: 400.0, w: 576.0, h: 80.0, ..Default::default() };
            title.xfrm = Some(x);
            Ok(x)
        })
        .unwrap();
    (crate::cmd::base64_encode(&deckcraft_pptx::export_as(&t.doc().unwrap().doc, "potx").unwrap()), logo, title)
}

#[test]
fn a_template_brings_its_master_and_layouts_and_slides_follow() {
    let (data, logo, title) = brand_template();
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "titleAndContent", "title": "Agenda", "body": "Point one"})).unwrap();
    let before = s.doc().unwrap().doc.clone();
    let n = undo_len(&s);
    let r = s.execute("design.browseTheme", &json!({"name": "Brand.potx", "data": data})).unwrap();
    assert_eq!(r["slides"], json!(2), "{r}");
    assert_eq!(undo_len(&s), n + 1, "one undo step");

    let doc = &s.doc().unwrap().doc;
    assert!(doc.validate().is_empty(), "{:?}", doc.validate());
    assert_eq!(doc.masters.len(), 1, "the old master is replaced");
    let m = &doc.masters[0];
    assert_ne!(m.id, before.masters[0].id);
    assert!(m.layouts.iter().all(|l| l.name.starts_with("Brand ")));
    assert!(matches!(m.background, Some(Background::Fill { fill: Fill::Solid { .. } })));
    let pic = m.shapes.iter().find(|sh| sh.name == "Logo").expect("the master's logo comes along");
    let deckcraft_model::ShapeKind::Picture { fill } = &pic.kind else { panic!("logo is a picture") };
    assert_eq!(doc.media(fill.media).map(|m| m.data.as_ref().clone()), Some(logo));
    // The 4:3 master stretches to this deck's 16:9.
    let k = doc.slide_size.width / 720.0;
    let px = pic.xfrm.unwrap();
    assert!((px.x - 600.0 * k).abs() < 0.01 && (px.w - 100.0 * k).abs() < 0.01 && (px.y - 20.0).abs() < 0.01, "{px:?}");

    // Each slide moved to the brand layout of its kind, and kept its text.
    for (old, new) in before.slides.iter().zip(&doc.slides) {
        let (_, was) = before.layout(old.layout).unwrap();
        let (owner, now) = doc.layout(new.layout).expect("on a layout of the new master");
        assert_eq!(owner.id, m.id);
        assert_eq!(now.kind, was.kind);
    }
    let agenda = doc.slides.iter().find(|sl| sl.shapes.iter().any(|sh| sh.text.as_ref().is_some_and(|t| t.text() == "Agenda"))).unwrap();
    assert!(agenda.shapes.iter().any(|sh| sh.text.as_ref().is_some_and(|t| t.text() == "Point one")));
    let (_, lay) = doc.layout(agenda.layout).unwrap();
    let lt = lay.shapes.iter().find(|sh| sh.ph_type().is_some_and(PhType::is_title)).unwrap().xfrm.unwrap();
    assert!((lt.x - title.x * k).abs() < 0.01 && (lt.y - title.y).abs() < 0.01, "{lt:?}");
    let st = agenda.shapes.iter().find(|sh| sh.ph_type().is_some_and(PhType::is_title)).unwrap();
    assert!(st.xfrm.is_none(), "the slide title follows the new layout");

    s.execute("edit.undo", &json!({})).unwrap();
    let doc = &s.doc().unwrap().doc;
    assert_eq!(doc.masters, before.masters);
    assert_eq!(doc.slides, before.slides);
}

#[test]
fn a_preserved_master_stays_beside_the_template() {
    let (data, _, _) = brand_template();
    let mut s = Session::with_new();
    s.execute("view.slideMaster", &json!({"master": 0, "layout": 1})).unwrap();
    s.execute("master.preserve", &json!({"preserve": true})).unwrap();
    s.execute("view.slideMaster", &json!({"master": 0})).unwrap();
    s.execute("master.preserve", &json!({"preserve": true})).unwrap();
    let old = s.doc().unwrap().doc.masters[0].id;
    s.execute("view.slideMaster", &json!({"master": 0, "layout": 1})).unwrap();
    s.execute("design.browseTheme", &json!({"name": "Brand.pptx", "data": data})).unwrap();
    let st = s.doc().unwrap();
    assert_eq!(st.doc.masters.len(), 2);
    assert_eq!(st.doc.masters[1].id, old);
    assert_eq!(st.selection.target, Target::Master { master: 0 }, "Slide Master view shows the new master");
    let new = &st.doc.masters[0];
    assert!(st.doc.slides.iter().all(|sl| new.layout(sl.layout).is_some()));
}

fn undo_len(s: &crate::Session) -> usize {
    s.doc().unwrap().history.undo.len()
}

#[test]
fn effect_scheme_replaces_format_and_keeps_colours_and_fonts() {
    let mut s = crate::Session::with_new();
    let listed = s.execute("design.themes", &json!({})).unwrap();
    let names: Vec<String> = listed["effectSchemes"].as_array().unwrap().iter().filter_map(|v| v.as_str().map(str::to_string)).collect();
    assert_eq!(names, ["DeckCraft", "Soft", "Crisp", "Lift", "Glow", "Plain"]);

    let n = undo_len(&s);
    s.execute("design.effects", &json!({"name": "DeckCraft"})).unwrap();
    assert_eq!(undo_len(&s), n, "the scheme already on the theme is not an undo step");
    let err = s.execute("design.effects", &json!({"name": "Office"})).unwrap_err().to_string();
    assert!(err.contains("no effect scheme"), "{err}");
    assert!(s.execute("design.effects", &json!({})).is_err());
    assert_eq!(undo_len(&s), n);

    s.execute("design.theme", &json!({"name": "Harbor"})).unwrap();
    let before = s.doc().unwrap().doc.masters[0].theme.clone();
    s.execute("design.effects", &json!({"name": "soft"})).unwrap();
    let theme = s.doc().unwrap().doc.masters[0].theme.clone();
    assert_eq!(theme.name, "Harbor");
    assert_eq!(theme.colors, before.colors);
    assert_eq!(theme.fonts, before.fonts);
    assert_eq!(theme.format.name, "Soft");
    assert!(theme.format.effects[0].outer_shadow.is_some());
    assert_eq!(s.doc().unwrap().history.undo.last().unwrap().label, "Effects");

    let n = undo_len(&s);
    s.execute("design.effects", &json!({"name": "Soft"})).unwrap();
    assert_eq!(undo_len(&s), n, "the same scheme is not an undo step");

    s.execute("design.effects", &json!({"name": "Glow"})).unwrap();
    let glow = s.doc().unwrap().doc.masters[0].theme.format.effects[2].glow.clone();
    assert!(glow.is_some_and(|g| (g.radius - 12.0).abs() < 1e-6));
    s.execute("design.effects", &json!({"name": "Plain"})).unwrap();
    let plain = s.doc().unwrap().doc.masters[0].theme.format.clone();
    assert!(plain.effects.iter().all(|e| e.outer_shadow.is_none() && e.glow.is_none()));
    assert_eq!(plain.lines[2].width, Some(0.75));
}

fn shapes(s: &Session) -> Vec<deckcraft_model::Shape> {
    s.doc().unwrap().shapes().to_vec()
}

fn has_title(s: &Session) -> bool {
    shapes(s).iter().any(|sh| sh.ph_type().is_some_and(PhType::is_title))
}

fn has_footers(s: &Session) -> bool {
    shapes(s).iter().any(|sh| sh.ph_type().is_some_and(PhType::is_footer_kind))
}

#[test]
fn preserve_and_placeholders_edit_the_open_master_or_layout() {
    for id in ["master.preserve", "master.placeholders"] {
        assert!(crate::command_specs().iter().any(|c| c.id == id), "{id}");
    }

    let mut s = Session::with_new();
    let blank = s.doc().unwrap().doc.masters[0].layouts.iter().position(|l| l.name == "Blank").unwrap();

    s.execute("view.slideMaster", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().selection.target, Target::Layout { master: 0, layout: 0 });
    s.execute("view.slideMaster", &json!({"master": 0})).unwrap();
    assert_eq!(s.doc().unwrap().selection.target, Target::Master { master: 0 });
    assert!(!s.doc().unwrap().doc.masters[0].preserve);

    let n = s.doc().unwrap().history.undo.len();
    s.execute("master.preserve", &json!({"preserve": false})).unwrap();
    assert_eq!(s.doc().unwrap().history.undo.len(), n, "the same value is not an undo step");
    s.execute("master.preserve", &json!({})).unwrap();
    assert!(s.doc().unwrap().doc.masters[0].preserve);
    assert_eq!(s.doc().unwrap().history.undo.last().unwrap().label, "Preserve");
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(!s.doc().unwrap().doc.masters[0].preserve);

    s.execute("master.placeholders", &json!({"footers": false})).unwrap();
    assert_eq!(s.doc().unwrap().history.undo.last().unwrap().label, "Layout Placeholders");
    assert!(has_title(&s));
    assert!(!has_footers(&s));
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(has_footers(&s));

    s.execute("view.closeMaster", &json!({})).unwrap();
    let n = s.doc().unwrap().history.undo.len();
    let err = s.execute("master.preserve", &json!({})).unwrap_err();
    assert!(err.to_string().contains("open Slide Master view"), "{err}");
    let err = s.execute("master.placeholders", &json!({"title": true})).unwrap_err();
    assert!(err.to_string().contains("open Slide Master view"), "{err}");
    let err = s.execute("master.placeholders", &json!({})).unwrap_err();
    assert!(err.to_string().contains("give title or footers"), "{err}");
    assert_eq!(s.doc().unwrap().history.undo.len(), n);

    s.execute("view.slideMaster", &json!({"master": 0, "layout": blank})).unwrap();
    assert_eq!(s.doc().unwrap().selection.target, Target::Layout { master: 0, layout: blank });
    assert!(s.doc().unwrap().doc.masters[0].layouts[blank].preserve);
    assert!(!has_title(&s));
    assert!(has_footers(&s));

    let n = s.doc().unwrap().history.undo.len();
    s.execute("master.placeholders", &json!({"title": false, "footers": true})).unwrap();
    assert_eq!(s.doc().unwrap().history.undo.len(), n, "already in that state");

    s.execute("master.placeholders", &json!({"title": true, "footers": false})).unwrap();
    assert_eq!(s.doc().unwrap().history.undo.len(), n + 1, "both keys are one undo step");
    assert_eq!(s.doc().unwrap().history.undo.last().unwrap().label, "Layout Placeholders");
    assert!(has_title(&s));
    assert!(!has_footers(&s));
    let title = shapes(&s).into_iter().find(|sh| sh.ph_type().is_some_and(PhType::is_title)).unwrap();
    assert_eq!(title.name, "Title 1");
    assert_eq!(title.ph.unwrap().idx, 0);
    assert!(title.text.unwrap().paragraphs.iter().all(|para| para.runs.is_empty()));
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(!has_title(&s));
    assert!(has_footers(&s));

    s.execute("master.placeholders", &json!({"footers": false})).unwrap();
    s.execute("master.placeholders", &json!({"footers": true})).unwrap();
    let restored = shapes(&s);
    assert!(restored.iter().any(|sh| sh.ph_type() == Some(PhType::Date)));
    assert!(restored.iter().any(|sh| sh.ph_type() == Some(PhType::Footer)));
    let num = restored.iter().find(|sh| sh.ph_type() == Some(PhType::SlideNum)).unwrap();
    let run = &num.text.as_ref().unwrap().paragraphs[0].runs[0];
    assert_eq!(run.text, "‹#›");
    assert!(matches!(&run.kind, RunKind::Field { field } if field == "slidenum"));

    assert!(s.doc().unwrap().doc.masters[0].layouts[blank].preserve);
    s.execute("master.preserve", &json!({"preserve": false})).unwrap();
    assert!(!s.doc().unwrap().doc.masters[0].layouts[blank].preserve);
    assert_eq!(s.doc().unwrap().history.undo.last().unwrap().label, "Preserve");
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.doc().unwrap().doc.masters[0].layouts[blank].preserve);

    s.edit(|doc, _| {
        let m = std::sync::Arc::make_mut(doc.masters.get_mut(0).unwrap());
        m.shapes.retain(|sh| !sh.ph_type().is_some_and(PhType::is_title));
        Ok(())
    })
    .unwrap();
    s.execute("master.placeholders", &json!({"title": true})).unwrap();
    let title = shapes(&s).into_iter().find(|sh| sh.ph_type() == Some(PhType::Title)).unwrap();
    assert_eq!(title.name, "Title");
    let box_ = title.xfrm.unwrap();
    assert!((box_.x - 60.0).abs() < 0.01 && (box_.y - 29.0).abs() < 0.01);
    assert!((box_.w - 840.0).abs() < 0.01 && (box_.h - 104.0).abs() < 0.01);
}
