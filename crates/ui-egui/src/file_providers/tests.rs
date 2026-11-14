use super::{Capture, Command, Host, OpenedDocument, Resource};
use serde_json::json;

#[test]
fn disconnect_clears_published_document_metadata_before_another_account_connects() {
    let (host, _) = connected();
    let documents = json!([{"id":"42", "title":"Private account A document", "dirty":true}]);
    host.publish_documents(documents.clone(), &[42]);
    assert_eq!(host.snapshot()["documents"], documents, "the regression must start with nonempty metadata");
    host.disconnect().unwrap();
    assert!(host.snapshot()["documents"].is_null());
    host.connect("test".into(), "Test files".into(), "account-two".into()).unwrap();
    let snapshot = host.snapshot();
    assert_eq!(snapshot["namespace"], "account-two");
    assert!(snapshot["documents"].is_null(), "the new account has not received an editor publication");
}

fn connected() -> (Host, String) {
    let host = Host::default();
    host.set_session("0123456789abcdef0123456789abcdef".into()).unwrap();
    let connection = host.connect("test".into(), "Test files".into(), "account-one".into()).unwrap();
    (host, connection)
}

fn request(host: &Host, doc: u64) -> String {
    host.choose(doc, 1, "Example.docx".into(), false).unwrap();
    host.take_events().remove(0).metadata["requestId"].as_str().unwrap().into()
}

fn capture(doc: u64, revision: u64) -> Capture {
    Capture { doc, revision, name: "Example.docx".into(), save_as: false }
}

fn opened_document(doc: u64, revision: u64) -> OpenedDocument {
    OpenedDocument { doc, revision, binding_name: Some("Example.docx".into()) }
}

#[test]
fn disabled_until_explicit_registration() {
    let host = Host::default();
    assert!(!host.enabled());
    assert!(!host.connected());
    assert!(host.choose(1, 1, "Example.docx".into(), false).is_err());
    assert!(host.request_open("one").is_err());
    assert!(host.take_events().is_empty());
}

#[test]
fn save_choice_does_not_capture_or_acknowledge() {
    let (host, _) = connected();
    let id = request(&host, 1);
    assert!(host.pending_for(1));
    assert!(host.result(id.clone(), "saved".into(), "one".into()).is_err());
    host.resolve_dialog(&id, "cancel", "").unwrap();
    assert!(!host.busy());
    assert!(!host.bound(1));
}

#[test]
fn repeated_save_has_one_destination_choice() {
    let (host, _) = connected();
    host.choose(1, 1, "Example.docx".into(), false).unwrap();
    host.choose(1, 2, "Example.docx".into(), false).unwrap();
    assert_eq!(host.take_events().len(), 1);
}

#[test]
fn exact_capture_ack_is_consumed_once_and_never_mutates_document() {
    let (host, _) = connected();
    let id = request(&host, 1);
    host.resolve_dialog(&id, "provider", "").unwrap();
    assert!(matches!(host.take_commands().remove(0), Command::Save { doc: 1, .. }));
    host.capture(&id, capture(1, 23), vec![1, 2, 3]).unwrap();
    let event = host.take_events().remove(0);
    assert_eq!(event.bytes, Some(vec![1, 2, 3]));
    assert_eq!(event.metadata["revision"], 23);
    host.result(id.clone(), "saved".into(), "one".into()).unwrap();
    assert!(host.result(id, "saved".into(), "one".into()).is_err());
    assert!(matches!(host.take_commands().remove(0), Command::Result { capture: Capture { doc: 1, revision: 23, .. }, .. }));
    assert!(!host.bound(1), "only the application admits a durable result");
}

#[test]
fn failure_cancel_and_account_changes_do_not_rebind() {
    let (host, _) = connected();
    let id = request(&host, 1);
    assert!(host.disconnect().is_err());
    assert!(host.connect("test".into(), "Test files".into(), "other-account".into()).is_err());
    host.resolve_dialog(&id, "provider", "").unwrap();
    host.take_commands();
    host.capture(&id, capture(1, 1), vec![]).unwrap();
    host.result(id, "failed".into(), String::new()).unwrap();
    assert!(host.disconnect().is_err(), "queued results still belong to the old identity");
    host.take_commands();
    host.disconnect().unwrap();
    assert!(!host.bound(1));
}

#[test]
fn new_connection_rejects_old_list_and_read_result() {
    let (host, connection) = connected();
    host.set_resources(&connection, vec![Resource { id: "one".into(), name: "Example.docx".into(), updated_at: String::new() }]).unwrap();
    let read = host.request_open("one").unwrap();
    host.cancel_open(read.clone(), String::new()).unwrap();
    host.take_events();
    host.disconnect().unwrap();
    let next = host.connect("test".into(), "Test files".into(), "account-one".into()).unwrap();
    assert_ne!(connection, next);
    assert!(host.set_resources(&connection, vec![]).is_err());
    assert!(host.open(read, "Example.docx".into(), vec![]).is_err());
}

#[test]
fn same_resource_alias_is_invalidated_but_homonymous_files_are_not() {
    let (host, _) = connected();
    host.record_save(&capture(1, 1), "one".into(), true);
    host.record_save(&capture(2, 1), "two".into(), true);
    assert_eq!(host.record_save(&capture(3, 1), "one".into(), true), vec![1]);
    assert!(!host.bound(1));
    assert!(host.bound(2));
    assert!(host.bound(3));
}

#[test]
fn save_as_never_overwrites_binding_by_filename() {
    let (host, _) = connected();
    host.record_save(&capture(1, 1), "one".into(), true);
    let id = request(&host, 1);
    host.resolve_dialog(&id, "provider", "").unwrap();
    host.take_commands();
    host.capture(&id, Capture { save_as: true, ..capture(1, 2) }, vec![]).unwrap();
    assert!(host.take_events().remove(0).metadata["resourceId"].is_null());
}

#[test]
fn provider_open_is_correlated_and_committed_after_decode() {
    let (host, connection) = connected();
    host.set_resources(&connection, vec![Resource { id: "one".into(), name: "Example.docx".into(), updated_at: String::new() }]).unwrap();
    let read = host.request_open("one").unwrap();
    assert!(host.open("unknown".into(), "Example.docx".into(), vec![]).is_err());
    host.open(read.clone(), "Example.docx".into(), vec![7]).unwrap();
    assert!(!host.bound(1));
    assert!(host.disconnect().is_err());
    let commands = host.take_commands();
    assert!(matches!(&commands[0], Command::Open { request, name, bytes } if request == &read && name == "Example.docx" && bytes == &[7]));
    host.opened(&read, Ok(opened_document(1, 1)));
    assert!(host.take_events().iter().any(|event| event.metadata["type"] == "opened"
        && event.metadata["resourceId"] == "one"
        && event.metadata["repaired"] == false
        && event.metadata["connectionId"].as_str() == Some(connection.as_str())));
    assert!(host.bound(1));
    assert!(host.open(read, "Example.docx".into(), vec![]).is_err());
}

#[test]
fn metadata_limits_are_atomic() {
    let (host, connection) = connected();
    let resource = Resource { id: "one".into(), name: "Example.docx".into(), updated_at: String::new() };
    host.set_resources(&connection, vec![resource.clone()]).unwrap();
    assert!(host.set_resources(&connection, vec![resource.clone(), resource]).is_err());
    assert_eq!(host.snapshot()["resources"].as_array().unwrap().len(), 1);
    assert!(host.choose(1, 1, "../outside.docx".into(), false).is_err());
    assert!(host.connect("bad/id".into(), "Label".into(), "account".into()).is_err());
}

#[test]
fn closed_documents_retire_choices_but_not_acks() {
    let (host, _) = connected();
    let id = request(&host, 1);
    host.publish_documents(json!([]), &[]);
    assert!(host.resolve_dialog(&id, "provider", "").is_err());
    assert!(!host.busy());
}

#[test]
fn disabled_provider_keeps_pending_result_reachable() {
    let (host, _) = connected();
    let id = request(&host, 1);
    host.resolve_dialog(&id, "provider", "").unwrap();
    host.take_commands();
    host.capture(&id, capture(1, 1), vec![]).unwrap();
    host.set_enabled(false).unwrap();
    host.result(id, "cancel".into(), String::new()).unwrap();
    host.take_commands();
    host.disconnect().unwrap();
}

#[test]
fn taking_a_choice_retains_account_and_document_ownership_until_serialization_finishes() {
    let (host, _) = connected();
    let id = request(&host, 1);
    host.resolve_dialog(&id, "device", "").unwrap();
    host.take_commands();
    assert!(host.pending_for(1));
    assert!(host.disconnect().is_err());
    host.finish_device(&id);
    assert!(!host.busy());
    host.disconnect().unwrap();
}

#[test]
fn recent_provider_entries_obey_search_and_each_callers_limit() {
    let (host, connection) = connected();
    let resources = (0..12)
        .map(|index| Resource { id: format!("file-{index}"), name: format!("Plan {index:02}.deckcraft"), updated_at: String::new() })
        .collect();
    host.set_resources(&connection, resources).unwrap();
    assert_eq!(host.recent_entries("", 8).unwrap().1.len(), 8);
    assert_eq!(host.recent_entries("", usize::MAX).unwrap().1.len(), 12);
    let (_, matching) = host.recent_entries(" PLAN 0 ", 2).unwrap();
    assert_eq!(matching.iter().map(|resource| resource.id.as_str()).collect::<Vec<_>>(), ["file-0", "file-1"]);
    assert!(host.recent_entries("unmatched", usize::MAX).unwrap().1.is_empty());
    assert!(host.recent_entries("", 0).unwrap().1.is_empty());
    host.set_enabled(false).unwrap();
    assert!(host.recent_entries("", usize::MAX).is_none());
}

#[test]
fn cancelled_reads_settle_with_control_free_utf8_bounded_error_text() {
    let (host, connection) = connected();
    host.set_resources(&connection, vec![Resource { id: "one".into(), name: "One.deckcraft".into(), updated_at: String::new() }]).unwrap();
    let request = host.request_open("one").unwrap();
    host.take_events();
    host.cancel_open(request.clone(), format!("\n{}\r\0tail", "界".repeat(800))).unwrap();
    assert!(!host.busy());
    let events = host.take_events();
    let failed = events.iter().find(|event| event.metadata["type"] == "openFailed").unwrap();
    assert_eq!(failed.metadata["requestId"], request);
    assert_eq!(failed.metadata["resourceId"], "one");
    let error = failed.metadata["error"].as_str().unwrap();
    assert!(!error.is_empty());
    assert!(error.len() <= 1024);
    assert!(!error.chars().any(char::is_control));
    assert!(host.cancel_open(request, "duplicate".into()).is_err());
    host.disconnect().unwrap();
}

fn fill_event_queue(host: &Host) {
    let mut state = host.state();
    for index in 0..super::MAX_EVENTS {
        state.event(json!({"type":"queued", "index":index}), None).unwrap();
    }
}

#[test]
fn full_event_queue_retains_open_success_and_failure_until_ordered_drain() {
    for success in [true, false] {
        let (host, connection) = connected();
        host.set_resources(&connection, vec![Resource { id: "one".into(), name: "One.zip".into(), updated_at: String::new() }]).unwrap();
        let read = host.request_open("one").unwrap();
        host.open(read.clone(), "One.zip".into(), vec![7]).unwrap();
        host.take_commands();
        host.take_events();
        fill_event_queue(&host);
        host.opened(&read, success.then(|| opened_document(1, 1)).ok_or("decoder failed\n"));
        host.opened(&read, Err("duplicate"));
        assert!(host.busy());
        assert!(host.disconnect().is_err());
        assert_eq!(host.state().finished.len(), 1);
        let earlier = host.take_events();
        assert_eq!(earlier.len(), super::MAX_EVENTS);
        assert!(earlier.iter().all(|event| event.metadata["type"] == "queued"));
        let terminal = host.take_events();
        assert_eq!(terminal.len(), 1);
        assert_eq!(terminal[0].metadata["requestId"], read);
        assert_eq!(terminal[0].metadata["resourceId"], "one");
        assert_eq!(terminal[0].metadata["namespace"], "account-one");
        assert_eq!(terminal[0].metadata["connectionId"], connection);
        assert_eq!(terminal[0].metadata["type"], if success { "opened" } else { "openFailed" });
        assert_eq!(host.bound(1), success);
        assert!(!host.busy());
        assert!(host.take_events().is_empty());
        host.disconnect().unwrap();
    }
}

#[test]
fn full_event_queue_retains_save_failure_and_rejects_unowned_or_duplicate_terminals() {
    let (host, _) = connected();
    let save = request(&host, 1);
    host.resolve_dialog(&save, "provider", "").unwrap();
    host.take_commands();
    fill_event_queue(&host);
    host.failed(&save, "encoder failed");
    host.failed(&save, "duplicate");
    host.failed("unowned", "forged");
    assert_eq!(host.state().finished.len(), 1);
    assert!(host.pending_for(1));
    assert!(host.disconnect().is_err());
    for doc in 2..100 {
        assert!(host.choose(doc, 1, "Other.zip".into(), false).is_err());
    }
    assert_eq!(host.take_events().len(), super::MAX_EVENTS);
    let terminal = host.take_events();
    assert_eq!(terminal.len(), 1);
    assert_eq!(terminal[0].metadata["type"], "saveFailed");
    assert_eq!(terminal[0].metadata["requestId"], save);
    assert_eq!(terminal[0].metadata["error"], "encoder failed");
    assert!(!host.pending_for(1));
    assert!(!host.busy());
    host.failed(&save, "late duplicate");
    assert!(host.take_events().is_empty());
}

#[test]
fn retained_terminals_are_bounded_by_already_admitted_reads_and_saves() {
    let (host, connection) = connected();
    host.set_resources(&connection, vec![Resource { id: "one".into(), name: "One.zip".into(), updated_at: String::new() }]).unwrap();
    let mut saves = Vec::new();
    let mut reads = Vec::new();
    for doc in 1..=super::MAX_PENDING as u64 {
        let save = request(&host, doc);
        host.resolve_dialog(&save, "provider", "").unwrap();
        host.take_commands();
        saves.push(save);
        let read = host.request_open("one").unwrap();
        host.open(read.clone(), "One.zip".into(), vec![]).unwrap();
        host.take_commands();
        host.take_events();
        reads.push(read);
    }
    fill_event_queue(&host);
    for read in &reads {
        host.opened(read, Err("decode failed"));
    }
    for save in &saves {
        host.failed(save, "encode failed");
    }
    assert_eq!(host.state().finished.len(), super::MAX_PENDING * 2);
    assert!(host.request_open("one").is_err());
    assert!(host.choose(999, 1, "New.zip".into(), false).is_err());
    assert_eq!(host.take_events().len(), super::MAX_EVENTS);
    let terminals = host.take_events();
    assert_eq!(terminals.len(), super::MAX_PENDING * 2);
    let expected: Vec<&str> = reads.iter().chain(saves.iter()).map(String::as_str).collect();
    let actual: Vec<&str> = terminals.iter().map(|event| event.metadata["requestId"].as_str().unwrap()).collect();
    assert_eq!(actual, expected, "retained open failures precede later save failures");
    for event in &terminals {
        assert_eq!(event.metadata["connectionId"], connection);
    }
    assert!(terminals.len() <= super::MAX_EVENTS);
    assert!(!host.busy());
    assert!(host.take_events().is_empty());
}

// Original synthetic transport fixtures; no external deck or media assets.
#[test]
fn failed_save_delivery_becomes_a_correlated_failure_and_later_events_still_dispatch() {
    let (host, _) = connected();
    let id = request(&host, 1);
    host.resolve_dialog(&id, "provider", "").unwrap();
    host.take_commands();
    host.capture(&id, capture(1, 23), vec![1, 2, 3]).unwrap();
    host.state().event(json!({"type":"after-save"}), None).unwrap();
    let mut errors = Vec::new();
    let mut delivered = Vec::new();
    let repaint = host.publish_events_with(
        |metadata, bytes| {
            if metadata["type"] == "save" {
                assert_eq!(metadata["requestId"], id);
                assert_eq!(bytes, Some(vec![1, 2, 3]));
                Err("recoverable JavaScript payload allocation failure")
            } else {
                delivered.push(metadata["type"].as_str().unwrap().to_string());
                Ok(())
            }
        },
        |error| errors.push(error),
    );
    assert!(repaint, "the app must process the failed result without another host callback");
    assert_eq!(errors, vec!["recoverable JavaScript payload allocation failure"]);
    assert_eq!(delivered, vec!["after-save"]);
    assert!(host.state().captures.is_empty(), "taken bytes cannot leave an unreachable capture");
    assert!(host.result(id.clone(), "saved".into(), "late".into()).is_err(), "the failed delivery cannot later acknowledge persistence");
    let commands = host.take_commands();
    assert_eq!(commands.len(), 1);
    let Command::Result { request, capture, resource, status } = &commands[0] else { panic!("expected correlated delivery failure") };
    assert_eq!(request, &id);
    assert_eq!((capture.doc, capture.revision), (1, 23));
    assert_eq!(status, "failed");
    assert!(resource.is_empty());
    assert!(!host.bound(1));
    assert!(!host.pending_for(1));
    assert!(!host.busy());
    host.disconnect().unwrap();
}

#[test]
fn successful_save_delivery_stays_pending_until_the_hosts_explicit_result() {
    let (host, _) = connected();
    let id = request(&host, 1);
    host.resolve_dialog(&id, "provider", "").unwrap();
    host.take_commands();
    host.capture(&id, capture(1, 23), vec![4, 5, 6]).unwrap();
    let mut delivered = 0;
    let mut errors = 0;
    assert!(!host.publish_events_with(
        |metadata, bytes| {
            assert_eq!(metadata["requestId"], id);
            assert_eq!(bytes, Some(vec![4, 5, 6]));
            delivered += 1;
            Ok::<(), &str>(())
        },
        |_| errors += 1,
    ));
    assert_eq!((delivered, errors), (1, 0));
    assert!(host.pending_for(1));
    assert!(host.take_commands().is_empty(), "successful delivery is not a persistence acknowledgement");
    host.result(id, "saved".into(), "one".into()).unwrap();
    let commands = host.take_commands();
    assert!(matches!(commands.as_slice(), [Command::Result { status, .. }] if status == "saved"));
}

#[test]
fn failed_delivery_recovers_its_existing_capture_even_with_a_full_command_queue() {
    let (host, connection) = connected();
    host.set_resources(&connection, vec![Resource { id: "one".into(), name: "Other.zip".into(), updated_at: String::new() }]).unwrap();
    let id = request(&host, 1);
    host.resolve_dialog(&id, "provider", "").unwrap();
    host.take_commands();
    host.capture(&id, capture(1, 23), vec![7]).unwrap();
    let mut reads = Vec::new();
    for _ in 0..super::MAX_PENDING {
        let read = host.request_open("one").unwrap();
        host.open(read.clone(), "Other.zip".into(), vec![]).unwrap();
        reads.push(read);
    }
    assert_eq!(host.state().commands.len(), super::MAX_PENDING);
    assert!(host.result(id.clone(), "saved".into(), "one".into()).unwrap_err().contains("queue is full"));
    assert!(host.publish_events_with(|metadata, _| if metadata["type"] == "save" { Err("delivery failed") } else { Ok(()) }, |_| {},));
    assert!(host.state().captures.is_empty());
    let commands = host.take_commands();
    assert_eq!(commands.len(), super::MAX_PENDING + 1);
    assert_eq!(commands.iter().filter(|command| matches!(command, Command::Open { .. })).count(), reads.len());
    let failures: Vec<_> = commands
        .iter()
        .filter_map(|command| match command {
            Command::Result { request, capture, resource, status } => {
                Some((request.as_str(), capture.doc, capture.revision, resource.as_str(), status.as_str()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(failures, vec![(id.as_str(), 1, 23, "", "failed")]);
    assert!(!host.bound(1));
    assert!(host.result(id, "saved".into(), "late".into()).is_err());
    for read in reads {
        host.opened(&read, Err("fixture complete"));
    }
    assert!(!host.publish_events_with(|_, _| Ok::<(), &str>(()), |_| {}));
    assert!(!host.busy());
}

#[test]
fn delivery_error_cannot_replace_a_result_already_accepted_from_a_synchronous_listener() {
    let (host, _) = connected();
    let id = request(&host, 1);
    host.resolve_dialog(&id, "provider", "").unwrap();
    host.take_commands();
    host.capture(&id, capture(1, 23), vec![8]).unwrap();
    assert!(host.publish_events_with(
        |metadata, _| {
            assert_eq!(metadata["requestId"], id);
            host.result(id.clone(), "saved".into(), "one".into()).unwrap();
            Err("dispatcher failed after the listener's result")
        },
        |_| {},
    ));
    let commands = host.take_commands();
    assert_eq!(commands.len(), 1);
    let Command::Result { capture, resource, status, .. } = &commands[0] else { panic!("expected the listener's result") };
    assert_eq!((capture.doc, capture.revision, resource.as_str(), status.as_str()), (1, 23, "one", "saved"));
    assert!(!host.busy());
}

#[test]
fn failed_open_request_delivery_releases_only_its_read_and_preserves_capacity() {
    let (host, connection) = connected();
    host.set_resources(
        &connection,
        vec![
            Resource { id: "one".into(), name: "One.zip".into(), updated_at: String::new() },
            Resource { id: "two".into(), name: "Two.zip".into(), updated_at: String::new() },
        ],
    )
    .unwrap();
    let other = host.request_open("two").unwrap();
    let mut other_deliveries = 0;
    let mut errors = 0;
    for _ in 0..=super::MAX_PENDING {
        let failed = host.request_open("one").unwrap();
        assert!(host.publish_events_with(
            |metadata, _| {
                assert_eq!(metadata["connectionId"], connection);
                if metadata["requestId"] == failed {
                    assert_eq!(metadata["resourceId"], "one");
                    Err("openRequest delivery failed")
                } else {
                    assert_eq!(metadata["requestId"], other);
                    assert_eq!(metadata["resourceId"], "two");
                    other_deliveries += 1;
                    Ok(())
                }
            },
            |_| errors += 1,
        ));
        assert!(!host.state().reads.contains_key(&failed));
        assert!(host.open(failed.clone(), "One.zip".into(), vec![1]).is_err());
        assert!(host.cancel_open(failed, String::new()).is_err());
        assert!(host.state().reads.contains_key(&other), "another admitted read still owns its response");
        assert!(host.take_commands().is_empty());
    }
    assert_eq!((other_deliveries, errors), (1, super::MAX_PENDING + 1));
    let retry = host.request_open("one").unwrap();
    host.open(retry.clone(), "One.zip".into(), vec![7]).unwrap();
    let commands = host.take_commands();
    assert!(matches!(commands.as_slice(), [Command::Open { request, bytes, .. }]
        if request == &retry && bytes.as_slice() == [7]));
    host.opened(&retry, Ok(opened_document(1, 23)));
    host.cancel_open(other, "fixture complete".into()).unwrap();
    let events = host.take_events();
    assert!(events.iter().all(|event| event.metadata["connectionId"] == connection && event.metadata["namespace"] == "account-one"));
    assert!(!host.busy());
    host.disconnect().unwrap();
}

#[test]
fn failed_save_dialog_delivery_releases_only_its_choice_and_allows_retry() {
    let (host, connection) = connected();
    host.choose(1, 23, "One.zip".into(), false).unwrap();
    host.choose(2, 24, "Two.zip".into(), false).unwrap();
    let mut failed = String::new();
    let mut other = String::new();
    let mut errors = 0;
    assert!(host.publish_events_with(
        |metadata, _| {
            assert_eq!(metadata["type"], "saveDialog");
            assert_eq!(metadata["connectionId"], connection);
            let request = metadata["requestId"].as_str().unwrap().to_string();
            if metadata["documentId"] == "1" {
                failed = request;
                Err("saveDialog delivery failed")
            } else {
                assert_eq!(metadata["documentId"], "2");
                other = request;
                Ok(())
            }
        },
        |_| errors += 1,
    ));
    assert_eq!(errors, 1);
    assert!(!failed.is_empty() && !other.is_empty() && failed != other);
    assert!(!host.pending_for(1));
    assert!(host.pending_for(2));
    assert!(host.resolve_dialog(&failed, "provider", "").is_err());
    assert!(host.take_commands().is_empty());
    host.choose(1, 25, "Retry.zip".into(), true).unwrap();
    let mut retries = 0;
    assert!(!host.publish_events_with(
        |metadata, _| {
            assert_eq!(metadata["documentId"], "1");
            assert_eq!(metadata["revision"], 25);
            assert_eq!(metadata["name"], "Retry.zip");
            assert_eq!(metadata["saveAs"], true);
            let request = metadata["requestId"].as_str().unwrap();
            assert_ne!(request, failed.as_str());
            host.resolve_dialog(request, "cancel", "").unwrap();
            retries += 1;
            Ok::<(), String>(())
        },
        |error| panic!("unexpected delivery failure: {error}"),
    ));
    assert_eq!(retries, 1, "retry must emit a fresh choice rather than silently remain pending");
    host.resolve_dialog(&other, "device", "").unwrap();
    let commands = host.take_commands();
    assert!(matches!(commands.as_slice(), [Command::Save { request, doc: 2, destination, name, save_as: false }]
        if request == &other && destination == "device" && name == "Two.zip"));
    host.finish_device(&other);
    assert!(!host.busy());
    host.disconnect().unwrap();
}

#[test]
fn delivery_error_preserves_open_and_choice_already_accepted_by_synchronous_listeners() {
    let (host, connection) = connected();
    host.set_resources(&connection, vec![Resource { id: "one".into(), name: "One.zip".into(), updated_at: String::new() }]).unwrap();
    let read = host.request_open("one").unwrap();
    assert!(host.publish_events_with(
        |metadata, _| {
            assert_eq!(metadata["requestId"], read);
            host.open(read.clone(), "One.zip".into(), vec![7]).unwrap();
            Err("dispatch failed after accepting the read")
        },
        |_| {},
    ));
    let commands = host.take_commands();
    assert!(matches!(commands.as_slice(), [Command::Open { request, bytes, .. }]
        if request == &read && bytes.as_slice() == [7]));
    assert!(host.disconnect().is_err());
    host.opened(&read, Ok(opened_document(1, 23)));
    assert!(!host.publish_events_with(
        |metadata, _| {
            assert_eq!(metadata["type"], "opened");
            assert_eq!(metadata["requestId"], read);
            assert_eq!(metadata["resourceId"], "one");
            assert_eq!(metadata["connectionId"], connection);
            Ok::<(), String>(())
        },
        |error| panic!("unexpected delivery failure: {error}"),
    ));
    assert!(host.bound(1));
    host.disconnect().unwrap();

    let (host, _) = connected();
    host.choose(2, 23, "Library.zip".into(), false).unwrap();
    let mut dialog = String::new();
    assert!(host.publish_events_with(
        |metadata, _| {
            dialog = metadata["requestId"].as_str().unwrap().to_string();
            host.resolve_dialog(&dialog, "provider", "").unwrap();
            Err("dispatch failed after accepting the choice")
        },
        |_| {},
    ));
    let commands = host.take_commands();
    assert!(matches!(commands.as_slice(), [Command::Save { request, doc: 2, destination, .. }]
        if request == &dialog && destination == "provider"));
    host.capture(&dialog, capture(2, 23), vec![8]).unwrap();
    assert!(!host.publish_events_with(
        |metadata, bytes| {
            assert_eq!(metadata["type"], "save");
            assert_eq!(metadata["requestId"], dialog);
            assert_eq!(bytes, Some(vec![8]));
            host.result(dialog.clone(), "saved".into(), "two".into()).unwrap();
            Ok::<(), String>(())
        },
        |error| panic!("unexpected delivery failure: {error}"),
    ));
    let commands = host.take_commands();
    assert!(matches!(commands.as_slice(), [Command::Result { capture: Capture { doc: 2, revision: 23, .. }, resource, status, .. }]
        if resource == "two" && status == "saved"));
    assert!(!host.bound(2), "delivery does not itself admit a durable application checkpoint");
    assert!(!host.busy());
    host.disconnect().unwrap();
}

#[test]
fn disconnect_during_publish_preserves_a_terminal_in_the_current_batch() {
    let (host, connection) = connected();
    host.set_resources(&connection, vec![Resource { id: "one".into(), name: "One.zip".into(), updated_at: String::new() }]).unwrap();
    let read = host.request_open("one").unwrap();
    host.open(read.clone(), "One.zip".into(), vec![7]).unwrap();
    let commands = host.take_commands();
    assert!(matches!(commands.as_slice(), [Command::Open { request, .. }] if request == &read));
    host.opened(&read, Ok(opened_document(1, 23)));
    assert!(!host.busy(), "ordinary owners are finished; only undelivered events retain registration");
    let mut order = Vec::new();
    let mut terminal = None;
    assert!(!host.publish_events_with(
        |metadata, _| {
            order.push(metadata["type"].as_str().unwrap().to_string());
            assert!(host.disconnect().is_err(), "the current batch still owns an undelivered terminal");
            assert_eq!(host.snapshot()["connectionId"], connection);
            assert!(host.bound(1));
            if metadata["type"] == "openRequest" {
                host.choose(2, 1, "Other.zip".into(), false).unwrap();
                let mut nested = 0;
                assert!(!host.publish_events_with(
                    |choice, _| {
                        assert_eq!(choice["type"], "saveDialog");
                        host.resolve_dialog(choice["requestId"].as_str().unwrap(), "cancel", "").unwrap();
                        assert!(host.disconnect().is_err(), "a nested drain cannot release the outer batch's registration");
                        nested += 1;
                        Ok::<(), String>(())
                    },
                    |error| panic!("unexpected nested delivery failure: {error}"),
                ));
                assert_eq!(nested, 1, "independent choices remain available during publication");
                assert!(!host.pending_for(2));
                assert!(host.disconnect().is_err());
            } else {
                assert_eq!(metadata["type"], "opened");
                terminal = Some(metadata.clone());
            }
            Ok::<(), String>(())
        },
        |error| panic!("unexpected delivery failure: {error}"),
    ));
    assert_eq!(order, vec!["openRequest", "opened"]);
    let terminal = terminal.unwrap();
    assert_eq!(terminal["requestId"], read);
    assert_eq!(terminal["resourceId"], "one");
    assert_eq!(terminal["documentId"], "1");
    assert_eq!(terminal["revision"], 23);
    assert_eq!(terminal["connectionId"], connection);
    assert_eq!(terminal["namespace"], "account-one");
    assert!(!host.busy());
    assert!(host.take_events().is_empty());
    host.disconnect().unwrap();
}

#[test]
fn disconnect_during_publish_preserves_the_retained_completion_batch() {
    for kind in ["opened", "openFailed", "saveFailed"] {
        let (host, connection) = connected();
        let request = if kind == "saveFailed" {
            let request = request(&host, 1);
            host.resolve_dialog(&request, "provider", "").unwrap();
            host.take_commands();
            request
        } else {
            host.set_resources(&connection, vec![Resource { id: "one".into(), name: "One.zip".into(), updated_at: String::new() }]).unwrap();
            let request = host.request_open("one").unwrap();
            host.open(request.clone(), "One.zip".into(), vec![7]).unwrap();
            host.take_commands();
            request
        };
        host.take_events();
        fill_event_queue(&host);
        if kind == "saveFailed" {
            host.failed(&request, "encoder failed");
        } else {
            host.opened(&request, (kind == "opened").then(|| opened_document(1, 1)).ok_or("decoder failed"));
        }
        let mut attempted_disconnect = false;
        let mut delivered = Vec::new();
        let failed = host.publish_events_with(
            |metadata, _| {
                if metadata["type"] == "queued" && !attempted_disconnect {
                    attempted_disconnect = true;
                    assert!(host.disconnect().is_err(), "the next batch still owns an undelivered terminal");
                    assert_eq!(host.snapshot()["connectionId"], connection);
                } else if metadata["type"] == kind {
                    delivered.push(metadata.clone());
                }
                Ok::<(), String>(())
            },
            |error| panic!("unexpected delivery failure: {error}"),
        );
        assert!(!failed && attempted_disconnect);
        assert_eq!(delivered.len(), 1);
        assert_eq!(delivered[0]["requestId"], request);
        assert_eq!(delivered[0]["namespace"], "account-one");
        assert_eq!(delivered[0]["connectionId"], connection);
        assert!(!host.busy());
        assert!(host.take_events().is_empty());
        host.disconnect().unwrap();
    }
}

#[test]
fn full_command_queue_keeps_a_choice_until_retry_or_explicit_cancel() {
    for destination in ["provider", "device"] {
        for cancel in [false, true] {
            let (host, connection) = connected();
            host.choose(1, 23, "Chosen.deckcraft".into(), true).unwrap();
            let choice = host.take_events().remove(0).metadata["requestId"].as_str().unwrap().to_string();
            host.set_resources(&connection, vec![Resource { id: "queued-read".into(), name: "Queued.deckcraft".into(), updated_at: String::new() }])
                .unwrap();
            let mut reads = Vec::new();
            // Real accepted read replies fill the ordinary inbox while this choice remains owned.
            for _ in 0..super::MAX_PENDING {
                let read = host.request_open("queued-read").unwrap();
                host.open(read.clone(), "Queued.deckcraft".into(), vec![7]).unwrap();
                reads.push(read);
            }
            assert_eq!(host.state().commands.len(), super::MAX_PENDING);
            assert!(host.resolve_dialog(&choice, destination, "Retried.deckcraft").is_err());
            assert!(host.state().dialogs.contains_key(&choice));
            assert!(host.pending_for(1));
            assert_eq!(host.state().commands.len(), super::MAX_PENDING, "a seventeenth ordinary command must not be admitted");
            if cancel {
                host.resolve_dialog(&choice, "cancel", "").unwrap();
                assert!(!host.state().dialogs.contains_key(&choice));
                assert!(!host.pending_for(1));
                let commands = host.take_commands();
                assert_eq!(commands.len(), super::MAX_PENDING);
                assert!(commands.iter().all(|command| matches!(command, Command::Open { .. })));
            } else {
                let commands = host.take_commands();
                assert_eq!(commands.len(), super::MAX_PENDING);
                assert!(commands.iter().all(|command| matches!(command, Command::Open { .. })));
                host.resolve_dialog(&choice, destination, "Retried.deckcraft").unwrap();
                assert!(!host.state().dialogs.contains_key(&choice));
                let mut commands = host.take_commands();
                assert_eq!(commands.len(), 1);
                let Command::Save { request, doc, destination: chosen, name, save_as } = commands.remove(0) else {
                    panic!("the retained choice must produce its original Save after capacity is freed")
                };
                assert_eq!(request, choice);
                assert_eq!(doc, 1);
                assert_eq!(chosen, destination);
                assert_eq!(name, "Retried.deckcraft");
                assert!(save_as);
                host.failed(&choice, "fixture complete");
            }
            for read in reads {
                host.opened(&read, Err("fixture complete"));
            }
            assert!(!host.publish_events_with(|_, _| Ok::<(), ()>(()), |_| {}));
            assert!(!host.busy());
            host.disconnect().unwrap();
        }
    }
}
