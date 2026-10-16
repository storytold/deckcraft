//! Direct host adoption must validate the same template graph as native file loading.
use std::sync::Arc;

use deckcraft_model::{Presentation, PresentationTemplateId};
use serde_json::json;

use crate::{Session, cmd::file::LoadedFile};

#[test]
fn staged_host_adoption_refuses_dangling_template_selection_before_opening_a_document() {
    let mut session = Session::with_new();
    let before = session.doc().unwrap().doc.clone();
    let revision = session.doc().unwrap().revision;
    let document_count = session.documents().len();
    let mut malformed = Presentation::default();
    Arc::make_mut(&mut malformed.slides[0]).presentation_template = Some(PresentationTemplateId(123));
    let token = session.stage_loaded(LoadedFile { path: "staged-template.deckcraft".into(), doc: malformed, native: true, damaged: vec![] });
    let error = session.execute("file.adopt", &json!({"path":"staged-template.deckcraft","token":token})).unwrap_err();
    assert!(error.to_string().contains("presentation template:"), "{error}");
    assert_eq!(session.documents().len(), document_count);
    assert!(Arc::ptr_eq(&session.doc().unwrap().doc, &before));
    assert_eq!(session.doc().unwrap().revision, revision);
}
