//! The web build framed by a page that keeps the files (`?host=parent`): what Save, Save As,
//! the exports and Print hand that page, under which name, and whether it is a new file.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::cell::RefCell;
use std::rc::Rc;

use serde_json::json;

use super::{Services, SlideApp};
use crate::Session;

/// What the page was handed: (name, Save As, bytes).
type Got = Rc<RefCell<Vec<(String, bool, Vec<u8>)>>>;

/// The web app inside such a page, with `host_save` and `download` pointed at it as the bridge
/// does; `fail` makes every save miss the page.
fn host_app(fail: bool) -> (SlideApp, Got) {
    let got: Got = Rc::default();
    let (saves, downloads) = (got.clone(), got.clone());
    let services = Services {
        host_save: Some(Box::new(move |name: &str, bytes: &[u8], save_as: bool| {
            if fail {
                return Err("the page is gone".into());
            }
            saves.borrow_mut().push((name.to_string(), save_as, bytes.to_vec()));
            Ok(())
        })),
        download: Some(Box::new(move |name: &str, bytes: &[u8]| downloads.borrow_mut().push((name.to_string(), false, bytes.to_vec())))),
        ..Default::default()
    };
    // Host mode starts as the web shell starts it: an empty presentation for the page's file.
    let mut app = SlideApp::new(Session::new(), services);
    app.start_for_host().unwrap();
    (app, got)
}

/// A .pptx with `slides` slides, as the page would send it.
fn pptx(slides: usize) -> Vec<u8> {
    let mut s = Session::with_new();
    while s.doc().unwrap().doc.slides.len() < slides {
        s.execute("slide.new", &json!({})).unwrap();
    }
    deckcraft_engine::cmd::file::save_bytes(&s.doc().unwrap().doc, "pptx").unwrap()
}

fn edit(app: &mut SlideApp) {
    app.session.execute("slide.new", &json!({})).unwrap();
    assert!(app.session.active().unwrap().is_dirty());
}

fn calls(got: &Got) -> Vec<(String, bool)> {
    got.borrow().iter().map(|(n, s, _)| (n.clone(), *s)).collect()
}

fn path(app: &SlideApp) -> Option<String> {
    app.session.active().and_then(|d| d.path.clone())
}

/// The bug this prevents: Save of a .pptx the page sent came back as `<name>.deckcraft`. Save
/// now hands the page the opened file under its own name and format, not as a new file; Save As
/// says it is a new file and becomes the file the next Save writes.
#[test]
fn host_saves_go_back_to_the_opened_file() {
    let (mut app, got) = host_app(false);
    app.open_from_host(Some("Lecture 3.pptx"), &pptx(2));
    assert_eq!(app.session.documents().len(), 1, "the untouched blank presentation gave way");
    edit(&mut app);
    app.save();
    assert!(!app.session.active().unwrap().is_dirty());
    edit(&mut app);
    app.save_as_dialog("deckcraft");
    assert!(!app.session.active().unwrap().is_dirty());
    edit(&mut app);
    app.save();
    let names = [("Lecture 3.pptx", false), ("Lecture 3.deckcraft", true), ("Lecture 3.deckcraft", false)];
    assert_eq!(calls(&got), names.map(|(n, s)| (n.to_string(), s)));
    // The first save is a .pptx that reopens with the edit in it.
    let first = got.borrow()[0].2.clone();
    assert!(first.windows(20).any(|w| w == b"ppt/presentation.xml"), "Save writes PowerPoint, not .deckcraft");
    let reopened = deckcraft_engine::cmd::file::loaded_file("Lecture 3.pptx", &first).unwrap();
    assert_eq!(reopened.doc.slides.len(), 3);
}

/// A presentation the page never sent is a new file, as Save As: the page must not overwrite a
/// file of the same name. The next Save goes back to it.
#[test]
fn host_save_of_an_untitled_presentation_is_save_as() {
    let (mut app, got) = host_app(false);
    let name = format!("{}.deckcraft", app.session.active().unwrap().title());
    edit(&mut app);
    app.save();
    edit(&mut app);
    app.save();
    assert_eq!(calls(&got), [(name.clone(), true), (name, false)]);
}

/// A presentation the user dropped (or picked with File › Open) didn't come from the page, so
/// its name isn't one the page holds: Save makes a new file.
#[test]
fn a_file_the_page_never_sent_saves_as_a_new_file() {
    let (mut app, got) = host_app(false);
    app.open_or_insert_bytes("dropped.pptx", &pptx(1));
    app.save();
    assert_eq!(calls(&got), [("dropped.deckcraft".to_string(), true)]);
}

/// Exports and Print are copies: named after the presentation, not a new file, and the
/// presentation keeps its own file and its unsaved changes.
#[test]
fn host_exports_and_print_are_copies() {
    let (mut app, got) = host_app(false);
    app.open_from_host(Some("deck.pptx"), &pptx(1));
    edit(&mut app);
    assert!(app.export_as("pdf", json!({})));
    assert!(app.print_pdf(json!({})));
    assert_eq!(calls(&got), [("deck.pdf".to_string(), false), ("deck.pdf".to_string(), false)]);
    assert!(app.session.active().unwrap().is_dirty());
    assert_eq!(path(&app).as_deref(), Some("deck.pptx"));
}

/// The page not getting the file is a failed save: the presentation keeps its changes.
#[test]
fn a_host_save_that_fails_leaves_the_presentation_unsaved() {
    let (mut app, _) = host_app(true);
    app.open_from_host(Some("deck.pptx"), &pptx(1));
    edit(&mut app);
    app.save();
    assert!(app.session.active().unwrap().is_dirty());
    assert!(app.status.as_ref().is_some_and(|(m, _)| m.contains("Couldn't save deck.pptx")));
}

/// Only an untouched start presentation gives way: one with changes stays open beside the file.
#[test]
fn an_edited_start_presentation_stays_open() {
    let (mut app, _) = host_app(false);
    edit(&mut app);
    app.open_from_host(Some("deck.pptx"), &pptx(1));
    assert_eq!(app.session.documents().len(), 2);
    assert_eq!(path(&app).as_deref(), Some("deck.pptx"));
}

/// Only the first file from the page takes the start presentation's place. The bug this
/// prevents: a file sent without a name opens untitled and unchanged, as the start presentation
/// is, so the page's next file closed it.
#[test]
fn only_the_first_file_from_the_page_replaces_the_start_presentation() {
    let (mut app, _) = host_app(false);
    app.open_from_host(None, &pptx(2));
    app.open_from_host(Some("deck.pptx"), &pptx(3));
    let slides: Vec<usize> = app.session.documents().iter().map(|d| d.doc.slides.len()).collect();
    assert_eq!(slides, [2, 3], "the file sent without a name stays open beside the next one");
    assert_eq!(path(&app).as_deref(), Some("deck.pptx"));
}

/// The command palette runs `file.save` by id. With a file of its own the engine would write to
/// that path, which a browser can't; in host mode it goes to the page like Ctrl+S.
#[test]
fn palette_save_goes_to_the_page() {
    let (mut app, got) = host_app(false);
    app.open_from_host(Some("deck.pptx"), &pptx(1));
    edit(&mut app);
    let r = app.run("file.save", json!({})).unwrap();
    assert_eq!(r["path"], "deck.pptx");
    assert_eq!(calls(&got), [("deck.pptx".to_string(), false)]);
    assert!(!app.session.active().unwrap().is_dirty());
}

/// A file the page sent without a name, or with a name that isn't a presentation's, has no file
/// of its own: Save is a new `.deckcraft`, never an overwrite of an invented `Presentation.pptx`.
#[test]
fn a_file_sent_without_a_presentation_name_saves_as_a_new_file() {
    for (name, saved) in [(None, "Presentation.deckcraft"), (Some("Talk"), "Talk.deckcraft")] {
        let (mut app, got) = host_app(false);
        app.open_from_host(name, &pptx(2));
        assert_eq!(app.session.documents().len(), 1, "{name:?}");
        assert_eq!(app.session.active().unwrap().doc.slides.len(), 2, "{name:?}");
        assert_eq!(path(&app), None, "{name:?}");
        app.save();
        assert_eq!(calls(&got), [(saved.to_string(), true)], "{name:?}");
    }
}

/// An outline from the page opens as a new untitled presentation, as File › Open does on the
/// desktop: the blank start presentation gives way, and Save is a new file.
#[test]
fn an_outline_from_the_page_opens_as_a_new_presentation() {
    let (mut app, got) = host_app(false);
    app.open_from_host(Some("notes.md"), b"# One\n\n# Two\n\n# Three\n");
    assert_eq!(app.session.documents().len(), 1);
    assert_eq!(app.session.active().unwrap().doc.slides.len(), 3);
    assert_eq!(path(&app), None);
    app.save();
    assert_eq!(calls(&got), [("notes.deckcraft".to_string(), true)]);
}

/// A damaged file from the page opens as an untitled "(Repaired)" copy, so Save is a new file
/// and can't overwrite the page's original with the emptied media.
#[test]
fn a_damaged_file_from_the_page_saves_as_a_new_repaired_file() {
    let mut p = deckcraft_model::Presentation::default();
    p.add_media("a.png", "image/png", b"PICTURE-BYTES-0123456789".to_vec());
    let mut bad = deckcraft_engine::cmd::file::save_bytes(&p, "deckcraft").unwrap();
    // Pictures are stored as they are, so flipping a byte breaks the entry's CRC.
    let at = bad.windows(13).position(|w| w == b"PICTURE-BYTES").unwrap();
    bad[at] ^= 0xff;
    let (mut app, got) = host_app(false);
    app.open_from_host(Some("talk.deckcraft"), &bad);
    assert_eq!(path(&app), None);
    assert!(app.session.active().unwrap().title().ends_with("(Repaired)"));
    app.save();
    assert_eq!(calls(&got), [("talk (Repaired).deckcraft".to_string(), true)]);
}

/// #1898: a read-only hosted session hands the page nothing — not a save, not a Save As, and not
/// the "copies" either (Export › PDF, Print go through the host's `download`, which is a
/// `deckcraft:save` on the page's side). Viewing still works and the deck stays clean.
#[test]
fn a_read_only_host_session_sends_the_page_nothing() {
    let (mut app, got) = host_app(false);
    app.open_from_host(Some("Lecture 3.pptx"), &pptx(2));
    app.session.read_only = true;
    app.save();
    app.save_as_dialog("deckcraft");
    assert!(!app.export_as("pdf", serde_json::json!({})), "Export refuses");
    assert!(!app.export_as("pngAll", serde_json::json!({})), "Export All Images refuses");
    assert!(!app.print_pdf(serde_json::json!({})), "Print refuses");
    assert!(got.borrow().is_empty(), "nothing was posted to the page: {:?}", calls(&got));
    assert!(app.status.as_ref().is_some_and(|s| s.0.contains("read-only")), "{:?}", app.status);
    assert!(!app.session.active().unwrap().is_dirty());
    app.run("slide.next", serde_json::json!({})).unwrap();
    assert_eq!(app.session.active().unwrap().selection.slide, 1, "navigation still works");
}

#[test]
fn hosted_read_only_exports_refuse_before_selecting_an_output() {
    let (mut app, got) = host_app(false);
    app.session.read_only = true;
    let picks = Rc::new(RefCell::new(0));
    let picked = picks.clone();
    app.services.pick_save = Some(Box::new(move |_| {
        *picked.borrow_mut() += 1;
        None
    }));
    for format in ["pdf", "pngAll", "outline"] {
        assert!(!app.export_as(format, serde_json::json!({})));
        assert!(app.status.as_ref().is_some_and(|s| s.0 == deckcraft_engine::READ_ONLY_REFUSED));
    }
    assert!(!app.print_pdf(serde_json::json!({})));
    assert_eq!(*picks.borrow(), 0, "refuse at entry, before even requesting an output path");
    assert!(got.borrow().is_empty());
}

#[test]
fn read_only_ribbon_disables_mutations_but_keeps_navigation() {
    let (mut app, _) = host_app(false);
    assert!(crate::ribbon::enabled(&app, "slide.new"));
    app.session.read_only = true;
    for id in ["slide.new", "edit.undo", "file.save", "file.saveAs"] {
        assert!(!crate::ribbon::enabled(&app, id), "{id}");
    }
    for id in ["slide.next", "slide.inspect", "edit.selectAll"] {
        assert!(crate::ribbon::enabled(&app, id), "{id}");
    }
}
