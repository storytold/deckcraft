//! Real commands preserve line-policy authority, undo and cross-format safeguards.
use crate::{Session, UiRequest};
use deckcraft_model::text::{LineHeight, ParaProps, Spacing};
use serde_json::json;

fn session() -> Session {
    let mut s = Session::with_new();
    s.execute("shape.insert", &json!({"preset":"rect","rect":[40,40,200,100],"text":"Original paragraph"})).unwrap();
    s
}
fn props(s: &Session) -> ParaProps {
    s.active().unwrap().selected_shapes()[0].text.as_ref().unwrap().paragraphs[0].props.clone()
}

#[test]
fn minimum_normal_legacy_commands_and_history_keep_one_line_setting() {
    let mut s = session();
    let old = props(&s);
    let undo = s.active().unwrap().history.undo.len();
    s.execute("format.lineSpacing", &json!({"mode":"minimum","pt":29.123456789})).unwrap();
    assert_eq!(s.active().unwrap().history.undo.len(), undo + 1);
    let minimum = props(&s);
    assert_eq!(minimum.line_height, Some(LineHeight::Minimum { points: 29.123456789 }));
    assert_eq!(minimum.line_spacing, None);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(props(&s), old);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(props(&s), minimum);
    s.execute("format.paragraphDialog", &json!({"align":"right"})).unwrap();
    assert_eq!(props(&s).line_height, minimum.line_height);
    s.ui_requests.clear();
    s.execute("format.paragraphDialog", &json!({})).unwrap();
    let Some(UiRequest::Dialog { params, .. }) = s.ui_requests.last() else { panic!("dialog request") };
    assert_eq!(params["lineKind"], "minimum");
    assert_eq!(params["lineValue"], 29.123456789);
    assert_eq!(s.execute("format.state", &json!({})).unwrap()["lineHeight"]["kind"], "minimum");
    s.execute("format.paragraph", &json!({"lineHeight":"normal"})).unwrap();
    assert_eq!(props(&s).line_height, Some(LineHeight::Normal));
    s.execute("format.lineSpacing", &json!({"lines":1.5})).unwrap();
    assert_eq!(props(&s).line_spacing, Some(Spacing::Pct(1.5)));
    assert_eq!(props(&s).line_height, None);
    s.execute("format.paragraphDialog", &json!({"lineHeight":"minimum","lineHeightPt":0.0})).unwrap();
    assert_eq!(props(&s).line_height, Some(LineHeight::Minimum { points: 0.0 }));
    s.execute("format.paragraph", &json!({"lineSpacingPt":18})).unwrap();
    assert_eq!(props(&s).line_height, None);
}

#[test]
fn invalid_or_ambiguous_line_requests_refuse_before_mutation() {
    let mut s = session();
    for (command, value) in [
        ("format.lineSpacing", json!({"mode":"minimum"})),
        ("format.lineSpacing", json!({"mode":"minimum","pt":-1})),
        ("format.lineSpacing", json!({"mode":"minimum","pt":1000001})),
        ("format.lineSpacing", json!({"mode":"normal","pt":12})),
        ("format.lineSpacing", json!({"mode":"normal","lines":1})),
        ("format.lineSpacing", json!({"mode":"unknown"})),
        ("format.lineSpacing", json!({"pt":12,"lines":1.5})),
        ("format.lineSpacing", json!({"pt":"12"})),
        ("format.paragraph", json!({"lineHeight":"minimum","lineHeightPt":24,"lineSpacing":1})),
        ("format.paragraphDialog", json!({"lineHeight":"minimum","lineHeightPt":"24"})),
        ("format.paragraphDialog", json!({"lineHeightPt":24})),
    ] {
        let before = s.active().unwrap().doc.clone();
        let undo = s.active().unwrap().history.undo.len();
        assert!(s.execute(command, &value).is_err(), "{command}: {value}");
        assert_eq!(s.active().unwrap().doc, before);
        assert_eq!(s.active().unwrap().history.undo.len(), undo);
    }
}

#[test]
fn engine_save_preserves_native_and_odp_minimum_and_refuses_pptx() {
    let mut s = session();
    s.execute("format.lineSpacing", &json!({"mode":"minimum","pt":35})).unwrap();
    for extension in ["deckcraft", "odp"] {
        let bytes = crate::cmd::file::save_bytes(&s.active().unwrap().doc, extension).unwrap();
        let p = if extension == "odp" { deckcraft_odp::import(&bytes).unwrap() } else { deckcraft_format::load(&bytes).unwrap() };
        assert_eq!(
            p.slides[0].shapes.last().unwrap().text.as_ref().unwrap().paragraphs[0].props.line_height,
            Some(LineHeight::Minimum { points: 35.0 })
        );
    }
    assert!(crate::cmd::file::save_bytes(&s.active().unwrap().doc, "pptx").unwrap_err().to_string().contains("line-height"));
}
