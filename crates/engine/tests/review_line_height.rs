//! Independent public commands and native/core export boundary tests.
use std::sync::Arc;

use deckcraft_engine::{Session, UiRequest};
use deckcraft_model::notes::NotesMaster;
use deckcraft_model::text::{LevelStyle, LineHeight, ParaProps, Spacing};
use deckcraft_model::{Master, Presentation};
use serde_json::json;

fn session() -> Session {
    let mut s = Session::with_new();
    s.execute("shape.insert", &json!({"preset":"rect","rect":[40,40,300,100],"text":"Independent review paragraph"})).unwrap();
    s
}

fn props(s: &Session) -> ParaProps {
    s.active().unwrap().selected_shapes()[0].text.as_ref().unwrap().paragraphs[0].props.clone()
}

#[test]
fn rejected_policy_commands_preserve_redo_document_and_dialog_requests() {
    let mut s = session();
    s.execute("format.lineSpacing", &json!({"mode":"minimum","pt":0.0})).unwrap();
    s.execute("format.lineSpacing", &json!({"mode":"normal"})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    let before = s.active().unwrap().doc.clone();
    let history = (s.active().unwrap().history.undo.len(), s.active().unwrap().history.redo.len());
    let request_count = s.ui_requests.len();
    for value in [
        json!({"mode":null}),
        json!({"mode":false}),
        json!({"mode":"minimum","pt":null}),
        json!({"mode":"minimum","pt":"0"}),
        json!({"mode":"minimum","pt":-0.001}),
        json!({"mode":"minimum","pt":1_000_000.001}),
        json!({"mode":"normal","pt":0}),
        json!({"mode":"minimum","pt":0,"lines":null}),
        json!({"mode":"atLeast","pt":20}),
    ] {
        assert!(s.execute("format.lineSpacing", &value).is_err(), "{value}");
        assert_eq!(s.active().unwrap().doc, before);
        assert_eq!((s.active().unwrap().history.undo.len(), s.active().unwrap().history.redo.len()), history);
        assert_eq!(s.ui_requests.len(), request_count);
    }
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(props(&s).line_height, Some(LineHeight::Normal));
}

#[test]
fn dialog_readback_keeps_fractional_policy_after_native_reload_and_alignment_edit() {
    let mut s = session();
    let points = 63.123_456_789_012_3;
    s.execute("format.paragraph", &json!({"lineHeight":"minimum","lineHeightPt":points})).unwrap();
    s.execute("format.paragraphDialog", &json!({"align":"center"})).unwrap();
    let native = deckcraft_engine::cmd::file::save_bytes(&s.active().unwrap().doc, "deckcraft").unwrap();
    let restored = deckcraft_format::load(&native).unwrap();
    let text = restored.slides[0].shapes.last().unwrap().text.as_ref().unwrap();
    assert_eq!(text.paragraphs[0].props.line_height, Some(LineHeight::Minimum { points }));
    s.execute("format.paragraphDialog", &json!({})).unwrap();
    let Some(UiRequest::Dialog { params, .. }) = s.ui_requests.last() else { panic!("expected paragraph dialog seed") };
    assert_eq!(params["lineKind"], "minimum");
    assert_eq!(params["lineValue"], points);
    assert_eq!(s.execute("format.state", &json!({})).unwrap()["lineHeight"]["points"], points);
    s.execute("format.paragraphDialog", &json!({"lineSpacingPt":21})).unwrap();
    assert_eq!(props(&s).line_spacing, Some(Spacing::Pts(21.0)));
    assert_eq!(props(&s).line_height, None);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(props(&s).line_height, Some(LineHeight::Minimum { points }));
}

#[test]
fn unused_distinct_notes_master_style_cannot_bypass_native_validation_or_pptx_guard() {
    for policy in [LineHeight::Normal, LineHeight::Minimum { points: 0.0 }, LineHeight::Minimum { points: 77.0 }] {
        let mut p = Presentation::default();
        let mut master = Master::default();
        master.other_style.set(8, LevelStyle { para: ParaProps { line_height: Some(policy), ..Default::default() }, ..Default::default() });
        p.notes_masters.push(NotesMaster { master: Arc::new(master), ..Default::default() });
        assert!(deckcraft_model::line_height::validate(&p).is_ok());
        let core = deckcraft_pptx::export(&p).unwrap_err().to_string();
        assert!(core.contains("line-height"), "{core}");
        let engine = deckcraft_engine::cmd::file::save_bytes(&p, "pptx").unwrap_err().to_string();
        assert!(engine.contains("line-height"), "{engine}");
        let native = deckcraft_format::save(&p).unwrap();
        let restored = deckcraft_format::load(&native).unwrap();
        assert_eq!(restored.notes_masters[0].master.other_style.level(8).unwrap().para.line_height, Some(policy));
    }
    for points in [f64::NAN, f64::INFINITY, -1.0, 1_000_001.0] {
        let mut p = Presentation::default();
        p.default_text_style
            .set(8, LevelStyle { para: ParaProps { line_height: Some(LineHeight::Minimum { points }), ..Default::default() }, ..Default::default() });
        assert!(deckcraft_format::save(&p).is_err(), "invalid policy must be rejected before finite-number repair");
    }
}
