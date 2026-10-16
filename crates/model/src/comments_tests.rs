#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use super::*;
use serde_json::json;

fn doc(items: Vec<Comment>) -> Presentation {
    let mut doc = crate::defaults::blank_presentation(crate::Size::new(960.0, 540.0), Default::default(), false);
    let slide = crate::Slide { comments: items, ..Default::default() };
    doc.slides.push(Arc::new(slide));
    doc
}
fn message(text: &str) -> Comment {
    Comment { text: text.into(), ..Default::default() }
}
fn extension() -> CommentExtension {
    let root = Comment {
        comment_id: Some(10),
        author: "Pat".into(),
        text: "root".into(),
        replies: vec![Comment { comment_id: Some(11), author: "Pat".into(), text: "reply".into(), ..Default::default() }],
        ..Default::default()
    };
    let visible = |key: &str, text: &str| ProjectionMessage {
        body_xml: None,
        key: key.into(),
        author_key: "a1".into(),
        author: "Pat".into(),
        initials: Some("".into()),
        text: text.into(),
        body: None,
        task: None,
        date: None,
        display_date: None,
        status: None,
        anchor: vec!["slide".into()],
        x: Some("0".into()),
        y: Some("0".into()),
    };
    CommentExtension {
        shape_bindings: BTreeMap::new(),
        slide_bindings: BTreeMap::new(),
        version: 1,
        projection: CommentProjection {
            profile: "test-v1".into(),
            owners: vec![ProjectionOwner {
                key: "page0".into(),
                present: true,
                authors: vec![ProjectionAuthor { key: "a1".into(), name: "Pat".into(), initials: Some("".into()), user_id: None, provider_id: None }],
                messages: vec![visible("c1", "root"), visible("c2", "reply")],
            }],
        },
        owners: vec![ExtensionOwner { key: "owner0".into(), projection_owners: vec!["page0".into()], comments: vec![root] }],
        media: vec![],
    }
}
fn regenerate(owners: &[ExtensionOwner], _: &[ExtensionMedia]) -> Result<CommentProjection> {
    let mut messages = Vec::new();
    fn walk(items: &[Comment], messages: &mut Vec<ProjectionMessage>) {
        for c in items {
            messages.push(ProjectionMessage {
                body_xml: None,
                key: format!("c{}", messages.len() + 1),
                author_key: "a1".into(),
                author: c.author.clone(),
                initials: Some(c.initials.clone()),
                text: c.text.clone(),
                body: c.body.clone(),
                task: None,
                date: if c.date.is_empty() { None } else { Some(c.date.clone()) },
                display_date: c.display_date.clone(),
                status: c.status.map(|status| {
                    match status {
                        CommentStatus::Active => "active",
                        CommentStatus::Resolved => "resolved",
                        CommentStatus::Closed => "closed",
                    }
                    .into()
                }),
                anchor: vec!["slide".into()],
                x: Some(c.x.to_string()),
                y: Some(c.y.to_string()),
            });
            walk(&c.replies, messages);
        }
    }
    for owner in owners {
        walk(&owner.comments, &mut messages);
    }
    Ok(CommentProjection {
        profile: "test-v1".into(),
        owners: vec![ProjectionOwner {
            key: "page0".into(),
            present: true,
            authors: vec![ProjectionAuthor { key: "a1".into(), name: "Pat".into(), initials: Some("".into()), user_id: None, provider_id: None }],
            messages,
        }],
    })
}

#[test]
fn old_resolved_and_new_closed_decode_without_inventing_times() {
    let old: Comment = serde_json::from_value(json!({"resolved":true,"text":"old"})).unwrap();
    assert_eq!(old.comment_status(), CommentStatus::Resolved);
    assert!(old.date.is_empty());
    let closed: Comment = serde_json::from_value(json!({"status":"closed"})).unwrap();
    assert!(closed.resolved);
    assert_eq!(closed.comment_status(), CommentStatus::Closed);
    let again: Comment = serde_json::from_slice(&serde_json::to_vec(&closed).unwrap()).unwrap();
    assert_eq!(again, closed);
}
#[test]
fn explicit_conflicting_bool_and_status_refuse() {
    assert!(serde_json::from_value::<Comment>(json!({"status":"closed","resolved":false})).is_err());
    assert!(serde_json::from_value::<Comment>(json!({"status":"active","resolved":true})).is_err());
    assert!(serde_json::from_value::<Comment>(json!({"status":"arbitrary"})).is_err());
    assert!(serde_json::from_str::<Comment>(r#"{"commentId":1,"commentId":2}"#).is_err());
}
#[test]
fn migration_is_stable_and_does_not_merge_names_or_invent_identity() {
    let mut p = doc(vec![message("one"), message("two")]);
    normalize(&mut p).unwrap();
    let once = p.clone();
    normalize(&mut p).unwrap();
    assert_eq!(p, once);
    let a = p.slides[0].comments[0].author_identity.as_ref().unwrap();
    let b = p.slides[0].comments[1].author_identity.as_ref().unwrap();
    assert_ne!(a.id, b.id);
    assert_eq!(a.origin, AuthorOrigin::Unknown);
    assert!(a.external_id.is_none() && a.provider_id.is_none() && a.user_id.is_none());
}
#[test]
fn duplicate_ids_and_same_author_conflicts_are_rejected() {
    let mut p = doc(vec![message("one"), message("two")]);
    normalize(&mut p).unwrap();
    let first = p.slides[0].comments[0].clone();
    let slide = Arc::make_mut(&mut p.slides[0]);
    slide.comments[1].comment_id = first.comment_id;
    assert!(validate(&p, false).is_err());
    let slide = Arc::make_mut(&mut p.slides[0]);
    slide.comments[1].comment_id = Some(p.next_id.saturating_add(1));
    slide.comments[1].author_identity = first.author_identity;
    slide.comments[1].author = "Other".into();
    assert!(validate(&p, false).is_err());
}
#[test]
fn metadata_limits_and_invalid_dates_are_closed() {
    let mut c = message("ok");
    c.text = "x".repeat(MAX_TEXT + 1);
    assert!(validate_metadata(&[c], &mut Budget::default()).is_err());
    let mut c = message("ok");
    c.date = "2026-02-30T00:00:00Z".into();
    assert!(validate_metadata(&[c], &mut Budget::default()).is_err());
    let mut c = message("ok");
    c.x = f64::NAN;
    assert!(validate_metadata(&[c], &mut Budget::default()).is_err());
    let mut c = message("leaf");
    for _ in 0..MAX_DEPTH + 1 {
        c = Comment { replies: vec![c], ..Default::default() };
    }
    assert!(validate_metadata(&[c], &mut Budget::default()).is_err());
}
#[test]
fn file_attachments_require_actual_declared_embedded_bytes() {
    let attachment = CommentAttachment { id: 80, media: MediaId(70), name: "original.txt".into() };
    let mut p = doc(vec![Comment { attachments: vec![attachment], ..Default::default() }]);
    assert!(validate(&p, true).is_err());
    p.media.push(crate::MediaItem {
        id: MediaId(70),
        name: "original.txt".into(),
        content_type: "text/plain".into(),
        data: Arc::new(b"contributor original".to_vec()),
        link: None,
    });
    assert!(validate(&p, true).is_ok());
    p.media[0].link = Some("https://example.invalid/never-fetch".into());
    assert!(validate(&p, true).is_err());
}
#[test]
fn native_paths_find_edit_and_delete_nested_messages() {
    let mut p = doc(vec![Comment { replies: vec![message("reply")], ..Default::default() }]);
    normalize(&mut p).unwrap();
    let id = p.slides[0].comments[0].replies[0].comment_id.unwrap();
    let path = path(&p.slides[0].comments, id).unwrap();
    assert_eq!(path, [0, 0]);
    let slide = Arc::make_mut(&mut p.slides[0]);
    at_mut(&mut slide.comments, &path).unwrap().text = "edited".into();
    let removed = remove(&mut slide.comments, &path).unwrap();
    assert_eq!(removed.text, "edited");
    assert!(slide.comments[0].replies.is_empty());
}
#[test]
fn source_guid_and_local_ids_roundtrip_exactly() {
    let mut c = message("source");
    c.source_id = Some(CommentSourceId::ModernPptx { guid: "{ABCDEF01-1234-5678-ABCD-0123456789ab}".into() });
    c.comment_id = Some(9);
    let q: Comment = serde_json::from_slice(&serde_json::to_vec(&c).unwrap()).unwrap();
    assert_eq!(q, c);
    let mut invalid = c;
    invalid.source_id = Some(CommentSourceId::ModernPptx { guid: "not-guid".into() });
    assert!(validate_metadata(&[invalid], &mut Budget::default()).is_err());
}
#[test]
fn exact_sidecar_encode_decode_and_restore_retains_nested_structure() {
    let extension = extension();
    let encoded = extension.encode().unwrap();
    let decoded = CommentExtension::decode(&encoded).unwrap();
    assert_eq!(decoded, extension);
    let mut p = doc(vec![]);
    let owners = BTreeMap::from([("owner0".into(), 0)]);
    decoded.restore(&extension.projection, &mut p, &owners, &BTreeMap::new(), &BTreeMap::new(), regenerate).unwrap();
    assert_eq!(p.slides[0].comments, extension.owners[0].comments);
}
#[test]
fn foreign_edits_and_deleted_last_root_never_resurrect_extension_state() {
    let extension = extension();
    let mut actual = extension.projection.clone();
    actual.owners[0].messages[0].text = "foreign edit".into();
    let mut p = doc(vec![message("foreign")]);
    let before = p.clone();
    let owners = BTreeMap::from([("owner0".into(), 0)]);
    assert!(extension.restore(&actual, &mut p, &owners, &BTreeMap::new(), &BTreeMap::new(), regenerate).is_err());
    assert_eq!(p, before);
    actual.owners[0].messages.clear();
    actual.owners[0].present = false;
    assert!(extension.restore(&actual, &mut p, &owners, &BTreeMap::new(), &BTreeMap::new(), regenerate).is_err());
    assert_eq!(p, before);
}
#[test]
fn hidden_messages_duplicate_owners_and_unbound_projections_refuse() {
    let mut extra = extension();
    extra.owners[0].comments[0].replies.push(message("hidden"));
    assert!(extra.validate().is_err());
    let mut extra = extension();
    extra.owners.push(extra.owners[0].clone());
    assert!(extra.validate().is_err());
    let mut extra = extension();
    extra.projection.owners.push(ProjectionOwner { key: "other".into(), present: false, authors: vec![], messages: vec![] });
    assert!(extra.validate().is_err());
}
#[test]
fn extension_escaped_keys_depth_duplicate_version_and_trailing_input_refuse() {
    assert!(CommentExtension::decode(br#"{"version":1,"version":1}"#).is_err());
    let mut bytes = extension().encode().unwrap();
    bytes.extend_from_slice(b" {}");
    assert!(CommentExtension::decode(&bytes).is_err());
    let oversized = format!("{{\"unknown\":\"{}\"}}", "x".repeat(MAX_TEXT + 1));
    assert!(CommentExtension::decode(oversized.as_bytes()).is_err());
    let deeply_nested = format!("{}0{}", "[".repeat(100), "]".repeat(100));
    assert!(CommentExtension::decode(deeply_nested.as_bytes()).is_err());
}
#[test]
fn copied_message_identity_and_shape_media_maps_are_explicit() {
    let c = Comment {
        comment_id: Some(10),
        shape: Some(ShapeId(20)),
        attachments: vec![CommentAttachment { id: 30, media: MediaId(40), name: "original".into() }],
        ..Default::default()
    };
    let mut items = vec![c];
    let mut next = 100;
    remap(
        &mut items,
        &mut next,
        &BTreeMap::from([(ShapeId(20), ShapeId(120))]),
        &BTreeMap::from([(MediaId(40), MediaId(140))]),
        &mut BTreeMap::new(),
    )
    .unwrap();
    assert_ne!(items[0].comment_id, Some(10));
    assert_eq!(items[0].shape, Some(ShapeId(120)));
    assert_eq!(items[0].attachments[0].media, MediaId(140));
    assert!(items[0].attachments[0].id > 100);
}

#[test]
fn rich_body_is_single_authority_and_legacy_shape_contradictions_refuse() {
    let body = crate::text::TextBody::from_text("same\nbody");
    let c: Comment = serde_json::from_value(json!({"body":body})).unwrap();
    assert_eq!(c.text, "same\nbody");
    assert!(serde_json::from_value::<Comment>(json!({"body":body,"text":"different"})).is_err());
    let c: Comment = serde_json::from_value(json!({"anchor":{"kind":"shape","shape":9}})).unwrap();
    assert_eq!(c.shape, Some(ShapeId(9)));
    assert!(serde_json::from_value::<Comment>(json!({"anchor":{"kind":"shape","shape":9},"shape":null})).is_err());
    assert!(serde_json::from_value::<Comment>(json!({"anchor":{"kind":"slide"},"shape":9})).is_err());
}
#[test]
fn rich_body_optional_nonfinite_values_are_never_silently_nullified() {
    let mut body = crate::text::TextBody::from_text("text");
    body.body.rot = Some(f64::INFINITY);
    let c = Comment { text: "text".into(), body: Some(body), ..Default::default() };
    assert!(validate_metadata(&[c], &mut Budget::default()).is_err());
}
#[test]
fn scalar_text_range_is_checked_against_its_real_owner() {
    let mut p = doc(vec![Comment {
        shape: Some(ShapeId(90)),
        anchor: Some(CommentAnchor::TextRange { shape: ShapeId(90), start: 1, end: 3 }),
        ..Default::default()
    }]);
    Arc::make_mut(&mut p.slides[0]).shapes.push(crate::Shape {
        id: ShapeId(90),
        text: Some(crate::text::TextBody::from_text("Aé🦀Z")),
        ..Default::default()
    });
    assert!(validate(&p, false).is_ok());
    Arc::make_mut(&mut p.slides[0]).comments[0].anchor = Some(CommentAnchor::TextRange { shape: ShapeId(90), start: 1, end: 5 });
    assert!(validate(&p, false).is_err());
}
#[test]
fn task_assignees_require_explicit_matching_author_metadata() {
    let identity = CommentAuthorIdentity {
        id: 44,
        origin: AuthorOrigin::Imported,
        external_id: Some("source-guid".into()),
        user_id: None,
        provider_id: None,
        source_initials: None,
        source_scope: None,
        wire_guid: None,
    };
    let task = CommentTask {
        assigned_to: vec![44],
        complete: Some("50.25%".into()),
        authors: vec![CommentTaskAuthor { identity, name: "Assignee".into(), initials: "".into() }],
        ..Default::default()
    };
    let mut p = doc(vec![Comment { task: Some(task), ..Default::default() }]);
    assert!(validate(&p, false).is_ok());
    Arc::make_mut(&mut p.slides[0]).comments[0].task.as_mut().unwrap().authors.clear();
    assert!(validate(&p, false).is_err());
    assert!(valid_completion("100%"));
    assert!(valid_completion("0.01%"));
    assert!(!valid_completion("101%"));
    assert!(!valid_completion("50"));
}
#[test]
fn a_foreign_rich_body_or_author_provider_edit_invalidates_extension_authority() {
    let extension = extension();
    let mut actual = extension.projection.clone();
    actual.owners[0].messages[0].body = Some(crate::text::TextBody::from_text("root"));
    let mut p = doc(vec![message("foreign")]);
    let before = p.clone();
    let owners = BTreeMap::from([("owner0".into(), 0)]);
    assert!(extension.restore(&actual, &mut p, &owners, &BTreeMap::new(), &BTreeMap::new(), regenerate).is_err());
    assert_eq!(p, before);
    let mut actual = extension.projection.clone();
    actual.owners[0].authors[0].provider_id = Some("foreign-provider".into());
    assert!(extension.restore(&actual, &mut p, &owners, &BTreeMap::new(), &BTreeMap::new(), regenerate).is_err());
    assert_eq!(p, before);
}

#[test]
fn a_matching_payload_witness_cannot_bless_contradictory_core_payload() {
    let mut forged = extension();
    forged.owners[0].comments[0].set_comment_status(CommentStatus::Closed);
    let actual = forged.projection.clone();
    let mut p = doc(vec![message("actual")]);
    let before = p.clone();
    let owners = BTreeMap::from([("owner0".into(), 0)]);
    assert!(forged.restore(&actual, &mut p, &owners, &BTreeMap::new(), &BTreeMap::new(), regenerate).is_err());
    assert_eq!(p, before);
    let mut forged = extension();
    let mut body = crate::text::TextBody::from_text("root");
    body.paragraphs[0].runs[0].props.bold = Some(true);
    forged.owners[0].comments[0].body = Some(body);
    let actual = forged.projection.clone();
    assert!(forged.restore(&actual, &mut p, &owners, &BTreeMap::new(), &BTreeMap::new(), regenerate).is_err());
    assert_eq!(p, before);
}
#[test]
fn explicit_null_legacy_required_scalars_are_not_repaired_to_defaults() {
    assert!(serde_json::from_value::<Comment>(json!({"text":null})).is_err());
    assert!(serde_json::from_value::<Comment>(json!({"resolved":null})).is_err());
}

#[test]
fn completion_percentage_never_exceeds_one_hundred_with_fraction() {
    for valid in ["0%", "0.01%", "99.99%", "100%", "100.0%", "100.00%"] {
        assert!(valid_completion(valid), "{valid}");
    }
    for invalid in ["100.01%", "100.99%", "101%", "100.000%", "-0.01%", "NaN%"] {
        assert!(!valid_completion(invalid), "{invalid}");
    }
}

#[test]
fn reply_deserialization_accepts_depth32_and_rejects_33_before_recursive_wire_decode() {
    fn input(depth: usize) -> String {
        let mut value = r#"{"text":"leaf"}"#.to_owned();
        for _ in 0..depth {
            value = format!(r#"{{"replies":[{value}]}}"#);
        }
        value
    }
    let comment: Comment = serde_json::from_str(&input(32)).unwrap();
    validate_metadata(&[comment], &mut Budget::default()).unwrap();
    assert!(serde_json::from_str::<Comment>(&input(33)).unwrap_err().to_string().contains("reply depth"));
    assert!(serde_json::from_str::<Comment>(&input(245)).is_err());
    let after: Comment = serde_json::from_str(r#"{"text":"after error"}"#).unwrap();
    assert_eq!(after.text, "after error");
}
#[test]
fn iterative_comment_decode_keeps_old_sequence_fields_and_rejects_hidden_duplicates() {
    let old: Comment = serde_json::from_str(r#"["Author","AU","old", "", 1.0, 2.0, true, [{"text":"reply"}], null, null]"#).unwrap();
    assert_eq!(old.author, "Author");
    assert!(old.resolved);
    assert_eq!(old.replies[0].text, "reply");
    assert!(serde_json::from_str::<Comment>(r#"{"replies":[{"text":"first","text":"second"}]}"#).is_err());
    assert!(serde_json::from_str::<Comment>(r#"{"replies":[{"unknown":0}]}"#).is_err());
    assert!(serde_json::from_str::<Comment>(r#"{"replies":{}}"#).is_err());
}
