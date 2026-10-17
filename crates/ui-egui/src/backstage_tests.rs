//! The File tab (Backstage, #76) through real egui input: the File tab, Alt+F and Esc, every page,
//! and the commands its pages run.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::cell::RefCell;
use std::rc::Rc;

use deckcraft_engine::Session;
use egui::accesskit::Role;
use egui::{Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use super::*;
use crate::{Services, SlideApp};

fn harness(app: SlideApp) -> Harness<'static, SlideApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    // The app keeps asking for frames, so step instead of run().
    h.run_steps(3);
    h
}

fn new_app() -> SlideApp {
    SlideApp::new(Session::with_new(), Services::default())
}

/// A folder of its own for one test's files.
fn temp_dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("deckcraft-backstage-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Desktop services whose Save dialog answers with `dir`/<suggested name> and records the names.
fn saving_into(dir: &std::path::Path) -> (Services, Rc<RefCell<Vec<String>>>) {
    let asked: Rc<RefCell<Vec<String>>> = Default::default();
    let log = asked.clone();
    let dir = dir.to_path_buf();
    let services = Services {
        pick_save: Some(Box::new(move |suggested: &str| {
            log.borrow_mut().push(suggested.to_string());
            Some(dir.join(suggested).to_string_lossy().to_string())
        })),
        ..Default::default()
    };
    (services, asked)
}

/// Is `r` in the side menu (the harness insets the window by its default margin)?
fn in_nav(r: egui::Rect) -> bool {
    r.left() < 20.0 && r.right() <= NAV_W + 20.0
}

/// Clicks the button called `label` on the page (not the side menu's entry of the same name).
fn click_on_page(h: &Harness<'_, SlideApp>, label: &str) {
    let node = h
        .query_all_by_role_and_label(Role::Button, label)
        .find(|n| !in_nav(n.rect()))
        .or_else(|| h.query_all_by_label(label).find(|n| !in_nav(n.rect())));
    node.unwrap_or_else(|| panic!("no “{label}” button on the page")).click();
}

/// Clicks the side menu's entry called `label`.
fn click_nav(h: &Harness<'_, SlideApp>, label: &str) {
    let node = h.query_all_by_label(label).find(|n| in_nav(n.rect()));
    node.unwrap_or_else(|| panic!("no “{label}” in the side menu")).click();
}

fn page(h: &Harness<'_, SlideApp>) -> Option<Page> {
    h.state().backstage.as_ref().map(|b| b.page)
}

#[test]
fn page_ids_round_trip_and_unknown_ids_are_refused() {
    for p in Page::ALL {
        assert_eq!(Page::parse(p.id()), Some(p));
    }
    assert_eq!(Page::parse(" SAVEAS "), Some(Page::SaveAs));
    assert_eq!(Page::parse("share"), None);
    assert_eq!(Page::parse(""), None);
    let (top, bottom) = entries(false);
    assert_eq!(top, vec![Nav::Page(Page::Home), Nav::Page(Page::New), Nav::Page(Page::Open)], "the start screen has no presentation pages");
    assert_eq!(bottom, vec![Nav::Page(Page::Account), Nav::Page(Page::Settings), Nav::Options]);
    assert_eq!(entries(true).0.len(), 9);
    assert_eq!(split_path("/home/me/Talk.pptx"), ("Talk.pptx".into(), "/home/me".into()));
    assert_eq!(format_bytes(512), "512 bytes");
    assert_eq!(format_bytes(33_485), "32.7 KB");
}

/// The File tab opens Home in place of the ribbon; Esc and the back arrow return.
#[test]
fn the_file_tab_opens_the_backstage_and_esc_or_back_returns() {
    let mut h = harness(new_app());
    assert!(h.query_by_label("Blank Presentation").is_none(), "the slide shows first");
    h.get_by_label("File").click();
    h.run_steps(3);
    assert_eq!(page(&h), Some(Page::Home));
    assert!(h.state().backstage_open());
    assert!(h.query_by_label("Blank Presentation").is_some(), "Home offers new presentations");
    assert!(h.query_by_label("File").is_none(), "the ribbon is hidden");
    h.key_press(Key::Escape);
    h.run_steps(3);
    assert!(!h.state().backstage_open(), "Esc goes back to the slide");
    assert!(h.query_by_label("File").is_some());

    h.get_by_label("File").click();
    h.run_steps(3);
    h.get_by_label("Back").click();
    h.run_steps(3);
    assert!(!h.state().backstage_open(), "so does the back arrow");
    assert_eq!(h.state().session.documents().len(), 1, "nothing was created or closed");
}

/// Alt+F opens it with the keyboard in the side menu; arrows walk it (pages show as they get the
/// focus); Enter runs an action; Esc goes back.
#[test]
fn alt_f_arrows_enter_and_esc_drive_the_side_menu() {
    let mut h = harness(new_app());
    h.key_press_modifiers(Modifiers::ALT, Key::F);
    h.run_steps(3);
    assert_eq!(page(&h), Some(Page::Home));
    assert_eq!(h.state().backstage.as_ref().and_then(|b| b.focus), Some(0));
    for want in [Page::New, Page::Open, Page::Info] {
        h.key_press(Key::ArrowDown);
        h.run_steps(2);
        assert_eq!(page(&h), Some(want));
    }
    // Save has the focus now; the page stays Info until it is run.
    h.key_press(Key::ArrowDown);
    h.run_steps(2);
    assert_eq!(page(&h), Some(Page::Info));
    // A presentation that was never saved goes to Save As, as in PowerPoint.
    h.key_press(Key::Enter);
    h.run_steps(2);
    assert_eq!(page(&h), Some(Page::SaveAs));
    // Up from the top wraps to Options at the bottom.
    for _ in 0..20 {
        h.key_press(Key::ArrowUp);
    }
    h.run_steps(2);
    h.key_press(Key::Escape);
    h.run_steps(3);
    assert!(!h.state().backstage_open());
    assert!(!h.state().session.active().unwrap().is_dirty(), "the keys never reached the slide");
}

/// Every page draws its own content.
#[test]
fn every_page_renders() {
    let mut h = harness(new_app());
    let last_theme = deckcraft_model::theme::builtin_themes().last().map(|t| t.name.clone()).unwrap();
    let checks: [(Page, &str); 8] = [
        (Page::Home, "Recent"),
        (Page::New, &last_theme),
        (Page::Open, "Browse"),
        (Page::Info, "Hidden slides"),
        (Page::SaveAs, "Not saved yet"),
        (Page::Print, "Print Hidden Slides"),
        (Page::Export, "Create PDF Document"),
        (Page::Account, "About DeckCraft"),
    ];
    for (p, marker) in checks {
        h.state_mut().run("app.backstage", json!({"page": p.id()})).unwrap();
        h.run_steps(3);
        assert_eq!(page(&h), Some(p));
        assert!(h.query_all_by_label_contains(marker).next().is_some(), "{} shows “{marker}”", p.label());
        let selected = h.query_all_by_label(p.label()).any(|n| in_nav(n.rect()));
        assert!(selected, "{} is in the side menu", p.label());
    }
    assert!(h.state_mut().run("app.backstage", json!({"page": "share"})).is_err(), "Share (cloud) isn't offered");
}

/// A starter template from File › New opens its deck and leaves the File tab.
#[test]
fn new_template_opens_a_starter_deck() {
    let mut h = harness(new_app());
    h.state_mut().run("app.backstage", json!({"page": "new"})).unwrap();
    h.run_steps(3);
    h.get_by_label("Briefing").click();
    h.run_steps(3);
    let doc = &h.state().session.active().unwrap().doc;
    assert_eq!(doc.masters[0].theme.name, "Harbor");
    assert_eq!(doc.slides[0].title(), "Weekly briefing");
    assert_eq!(doc.slides.len(), 5);
    assert!(h.state().backstage.is_none(), "the starter deck shows");
}

/// New and Home create presentations, which leaves the File tab.
#[test]
fn new_creates_a_presentation() {
    let mut h = harness(new_app());
    h.state_mut().run("app.backstage", json!({"page": "new"})).unwrap();
    h.run_steps(3);
    h.get_by_label("Blank Presentation").click();
    h.run_steps(3);
    assert_eq!(h.state().session.documents().len(), 2);
    assert!(h.state().backstage.is_none(), "the new presentation shows");

    let theme = deckcraft_model::theme::builtin_themes().into_iter().nth(1).unwrap();
    h.state_mut().run("app.backstage", json!({})).unwrap();
    h.run_steps(3);
    h.get_by_label(&theme.name).click();
    h.run_steps(3);
    assert_eq!(h.state().session.documents().len(), 3);
    assert_eq!(h.state().session.active().unwrap().doc.masters[0].theme.name, theme.name);
    assert!(h.state().backstage.is_none());
}

/// Open lists recent presentations by name (filtered by the search box) and opens one.
#[test]
fn open_lists_recent_files_and_opens_one() {
    let dir = temp_dir("open");
    let path = dir.join("Quarterly review.pptx").to_string_lossy().to_string();
    let mut s = Session::with_new();
    s.execute("file.saveAs", &json!({"path": path})).unwrap();
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    app.ui.recent = vec![path.clone(), dir.join("Other.pptx").to_string_lossy().to_string()];
    let mut h = harness(app);
    h.state_mut().run("app.backstage", json!({"page": "open"})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label("Other.pptx").is_some());
    h.state_mut().backstage.as_mut().unwrap().query = "quarter".into();
    h.run_steps(2);
    assert!(h.query_by_label("Other.pptx").is_none(), "the search box filters by name");
    h.get_by_label("Quarterly review.pptx").click();
    h.run_steps(1);
    // The file is read in the background (#350); its progress window shows meanwhile.
    for _ in 0..300 {
        if h.state().opening.is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        h.run_steps(1);
    }
    h.run_steps(3);
    assert_eq!(h.state().session.documents().len(), 2);
    assert_eq!(h.state().session.active().unwrap().path.as_deref(), Some(path.as_str()));
    assert!(h.state().backstage.is_none(), "the opened presentation shows");
    assert_eq!(h.state().ui.recent.first(), Some(&path));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Save As saves through the Save dialog and the presentation takes the new file; Export writes
/// copies (PDF, another file type) and leaves the presentation's own file alone.
#[test]
fn save_as_and_export_run_the_file_commands() {
    let dir = temp_dir("saveas");
    let (services, asked) = saving_into(&dir);
    let mut h = harness(SlideApp::new(Session::with_new(), services));
    let title = h.state().session.active().unwrap().title();

    // Save on a presentation without a file goes to Save As.
    h.state_mut().run("app.backstage", json!({})).unwrap();
    h.run_steps(3);
    click_nav(&h, "Save");
    h.run_steps(3);
    assert_eq!(page(&h), Some(Page::SaveAs));
    click_on_page(&h, "Save");
    h.run_steps(3);
    let pptx = dir.join(format!("{title}.pptx")).to_string_lossy().to_string();
    assert!(std::path::Path::new(&pptx).is_file(), "saved as PowerPoint by default");
    assert_eq!(h.state().session.active().unwrap().path.as_deref(), Some(pptx.as_str()));
    assert!(h.state().backstage.is_none(), "a save returns to the slide");

    // Export › Create PDF Document.
    h.state_mut().run("app.backstage", json!({"page": "export"})).unwrap();
    h.run_steps(3);
    click_on_page(&h, "Create PDF");
    h.run_steps(3);
    let pdf = std::fs::read(dir.join(format!("{title}.pdf"))).unwrap();
    assert!(pdf.starts_with(b"%PDF"));

    // Export › Change File Type › Outline.
    click_on_page(&h, "Change File Type");
    h.run_steps(3);
    click_on_page(&h, "Outline (*.txt)");
    h.run_steps(2);
    click_on_page(&h, "Save As");
    h.run_steps(3);
    assert!(dir.join(format!("{title}.txt")).is_file());
    assert_eq!(h.state().session.active().unwrap().path.as_deref(), Some(pptx.as_str()), "exports are copies");
    assert_eq!(*asked.borrow(), vec![format!("{title}.pptx"), format!("{title}.pdf"), format!("{title}.txt")]);

    // Save with a file saves in place and goes back.
    h.state_mut().run("slide.new", json!({})).unwrap();
    h.state_mut().run("app.backstage", json!({})).unwrap();
    h.run_steps(3);
    click_nav(&h, "Save");
    h.run_steps(3);
    assert!(!h.state().session.active().unwrap().is_dirty());
    assert!(h.state().backstage.is_none());
    assert_eq!(asked.borrow().len(), 3, "no Save dialog for a presentation with a file");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Close closes a saved presentation at once and asks about unsaved changes first.
#[test]
fn close_closes_and_asks_about_unsaved_changes() {
    let mut h = harness(new_app());
    h.state_mut().run("app.backstage", json!({})).unwrap();
    h.run_steps(3);
    click_nav(&h, "Close");
    h.run_steps(3);
    assert_eq!(h.state().session.documents().len(), 0);
    assert_eq!(page(&h), Some(Page::Home), "the start screen shows");
    assert!(h.query_by_label("Back").is_none() && h.query_by_label("Close").is_none(), "nothing to go back to or close");

    // Unsaved: Cancel keeps it, Don't Save closes it.
    h.get_by_label("Blank Presentation").click();
    h.run_steps(3);
    h.state_mut().run("slide.new", json!({})).unwrap();
    h.state_mut().run("app.backstage", json!({})).unwrap();
    h.run_steps(3);
    click_nav(&h, "Close");
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("close"));
    h.get_by_label("Cancel").click();
    h.run_steps(3);
    assert_eq!(h.state().session.documents().len(), 1);
    assert!(h.state().backstage_open(), "Cancel leaves the File tab as it was");
    click_nav(&h, "Close");
    h.run_steps(3);
    h.get_by_label("Don't Save").click();
    h.run_steps(3);
    assert_eq!(h.state().session.documents().len(), 0);
}

/// Ctrl+W asks too (it used to close unsaved presentations without a word).
#[test]
fn ctrl_w_asks_before_closing_unsaved_changes() {
    let mut h = harness(new_app());
    h.state_mut().run("slide.new", json!({})).unwrap();
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::W);
    h.run_steps(3);
    assert_eq!(h.state().session.documents().len(), 1);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("close"));
}

/// Info shows the presentation's counts and properties, and edits them through file.properties.
#[test]
fn info_shows_and_edits_the_properties() {
    let mut app = new_app();
    app.run("file.properties", json!({"title": "Quarterly", "keywords": "sales"})).unwrap();
    app.run("slide.new", json!({})).unwrap();
    app.run("slide.hide", json!({})).unwrap();
    let mut h = harness(app);
    h.state_mut().run("app.backstage", json!({"page": "info"})).unwrap();
    h.run_steps(3);
    assert_eq!(h.get_by_role_and_label(Role::TextInput, "Title").value().as_deref(), Some("Quarterly"));
    assert_eq!(h.get_by_role_and_label(Role::TextInput, "Tags").value().as_deref(), Some("sales"));
    assert!(h.query_all_by_label("Not saved yet").next().is_some());
    let author = h.get_by_role_and_label(Role::TextInput, "Author");
    author.click();
    h.run_steps(2);
    h.get_by_role_and_label(Role::TextInput, "Author").type_text("Ada");
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert_eq!(h.state().session.active().unwrap().doc.props.author, "Ada");
    // Esc in a field leaves the field; the next one goes back.
    assert!(h.state().backstage_open());
}

/// Leaving a property field with a click on another field keeps what was typed.
#[test]
fn info_keeps_a_property_draft_when_another_field_is_clicked() {
    let mut h = harness(new_app());
    h.state_mut().run("app.backstage", json!({"page": "info"})).unwrap();
    h.run_steps(3);
    h.get_by_role_and_label(Role::TextInput, "Author").click();
    h.run_steps(2);
    h.get_by_role_and_label(Role::TextInput, "Author").type_text("Ada");
    h.run_steps(2);
    h.get_by_role_and_label(Role::TextInput, "Title").click();
    h.run_steps(4);
    assert_eq!(h.state().session.active().unwrap().doc.props.author, "Ada");
}

/// Save straight from a property field saves what was typed (the side menu acts before the field
/// is drawn again).
#[test]
fn info_saves_a_property_draft_when_save_is_clicked() {
    let dir = temp_dir("info-save");
    let path = dir.join("Draft.pptx").to_string_lossy().to_string();
    let mut s = Session::with_new();
    s.execute("file.saveAs", &json!({"path": path})).unwrap();
    let mut h = harness(SlideApp::new(s, Services::default()));
    h.state_mut().run("app.backstage", json!({"page": "info"})).unwrap();
    h.run_steps(3);
    h.get_by_role_and_label(Role::TextInput, "Author").click();
    h.run_steps(2);
    h.get_by_role_and_label(Role::TextInput, "Author").type_text("Ada");
    h.run_steps(2);
    click_nav(&h, "Save");
    h.run_steps(3);
    let doc = h.state().session.active().unwrap();
    assert_eq!(doc.doc.props.author, "Ada");
    assert!(!doc.is_dirty(), "the author went into the saved file");
    let _ = std::fs::remove_dir_all(&dir);
}

/// With nothing open the File tab is the start screen: Home, New and Open, no back arrow, and Esc
/// has nowhere to go.
#[test]
fn the_start_screen_is_file_home() {
    let mut h = harness(SlideApp::new(Session::new(), Services::default()));
    assert_eq!(page(&h), Some(Page::Home));
    assert!(h.query_by_label("Back").is_none());
    assert!(h.query_all_by_label("Info").next().is_none());
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert_eq!(page(&h), Some(Page::Home));
    h.key_press(Key::ArrowDown);
    h.run_steps(2);
    assert_eq!(page(&h), Some(Page::New));
    assert!(h.state_mut().run("app.backstage", json!({"page": "print"})).is_err(), "nothing to print");
    h.get_by_label("Blank Presentation").click();
    h.run_steps(3);
    assert_eq!(h.state().session.documents().len(), 1);
    assert!(h.state().backstage.is_none());
}

/// #346: a file picked in Open that can't be read used to drop back to the start screen without
/// a word. The reason now shows over the start screen.
#[test]
fn a_file_that_cant_be_opened_says_why() {
    let dir = temp_dir("unreadable");
    let path = dir.join("Notes.pptx");
    std::fs::write(&path, b"not a presentation").unwrap();
    let picked = path.to_string_lossy().to_string();
    let services = Services { pick_open: Some(Box::new(move |_: &str| Some(picked.clone()))), ..Default::default() };
    let mut h = harness(SlideApp::new(Session::new(), services));
    h.state_mut().run("app.backstage", json!({"page": "open"})).unwrap();
    h.run_steps(2);
    h.get_by_label("Browse").click();
    h.run_steps(3);
    // The file is read in the background (#350): wait for it.
    for _ in 0..500 {
        if h.state().opening.is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        h.run_steps(1);
    }
    h.run_steps(2);
    assert!(h.state().session.documents().is_empty());
    assert!(h.query_by_label_contains("not a presentation DeckCraft can read").is_some(), "the error is on screen");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Print previews the pages its settings make and prints through a PDF opened in the system
/// viewer.
#[test]
fn print_previews_pages_and_prints_a_pdf() {
    let dir = temp_dir("print");
    let (services, _) = saving_into(&dir);
    let mut app = SlideApp::new(Session::with_new(), services);
    for _ in 0..3 {
        app.run("slide.new", json!({})).unwrap();
    }
    app.run("slide.hide", json!({"index": 1})).unwrap();
    let mut h = harness(app);
    h.key_press_modifiers(Modifiers::COMMAND, Key::P);
    h.run_steps(3);
    assert_eq!(page(&h), Some(Page::Print), "Ctrl+P opens File › Print");
    assert!(h.query_by_label("1 of 3").is_some(), "hidden slides are left out");
    h.state_mut().backstage.as_mut().unwrap().print_layout = "h4";
    h.run_steps(2);
    assert!(h.query_by_label("1 of 1").is_some(), "four slides a page");
    h.state_mut().backstage.as_mut().unwrap().print_layout = "slides";
    h.state_mut().backstage.as_mut().unwrap().print_hidden = true;
    h.run_steps(2);
    assert!(h.query_by_label("1 of 4").is_some());
    h.get_by_label("Next Page").click();
    h.run_steps(2);
    assert!(h.query_by_label("2 of 4").is_some());
    {
        let bs = h.state_mut().backstage.as_mut().unwrap();
        bs.print_scope = PrintScope::Custom;
        bs.print_range = "9".into();
    }
    h.run_steps(2);
    assert!(h.query_by_label_contains("outside them").is_some(), "a bad range says why");
    h.state_mut().backstage.as_mut().unwrap().print_range = "1-2".into();
    h.run_steps(2);
    click_on_page(&h, "Print");
    h.run_steps(2);
    let url = h.state().pending_urls.first().cloned().unwrap_or_default();
    // The URL was handed to egui at the end of the frame; check what it pointed at.
    let printed = std::env::temp_dir().join(format!("{} (print).pdf", h.state().session.active().unwrap().title()));
    assert!(printed.is_file(), "the PDF to print was written: {url}");
    assert!(std::fs::read(&printed).unwrap().starts_with(b"%PDF"));
    let _ = std::fs::remove_file(printed);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn print_slides_follow_the_settings() {
    let mut s = Session::with_new();
    for _ in 0..4 {
        s.execute("slide.new", &json!({})).unwrap();
    }
    s.execute("slide.hide", &json!({"index": 2})).unwrap();
    let doc = s.doc().unwrap().doc.clone();
    let mut bs = Backstage::new(Page::Print, (1, Some(0)));
    assert_eq!(print_slides(&doc, &bs, 0).unwrap(), vec![0, 1, 3, 4]);
    bs.print_hidden = true;
    assert_eq!(print_slides(&doc, &bs, 0).unwrap().len(), 5);
    bs.print_scope = PrintScope::Current;
    assert_eq!(print_slides(&doc, &bs, 2).unwrap(), vec![2], "the current slide prints even when hidden");
    bs.print_scope = PrintScope::Custom;
    bs.print_range = "2-4".into();
    bs.print_hidden = false;
    assert_eq!(print_slides(&doc, &bs, 0).unwrap(), vec![1, 3]);
    bs.print_layout = "h6";
    assert_eq!(print_params(&bs, &[1, 3])["perPage"], 6);
    assert_eq!(print_params(&bs, &[1, 3])["layout"], "handouts");
    assert_eq!(print_params(&bs, &[1])["paper"], "letter");
    assert_eq!(print_page_size(&bs, &doc), (612.0, 792.0));
    bs.print_paper = "a4";
    bs.print_landscape = true;
    assert_eq!(print_params(&bs, &[1])["orientation"], "landscape");
    let (w, h) = print_page_size(&bs, &doc);
    assert!((w - 841.89).abs() < 0.01 && (h - 595.28).abs() < 0.01);
    bs.print_layout = "slides";
    assert_eq!(print_page_size(&bs, &doc), (doc.slide_size.width as f32, doc.slide_size.height as f32), "slides print slide-shaped");
}

// ---------- Interface themes (File › Settings › Themes) ----------

/// The title bar's gear opens File › Settings on its Themes tab, where clicking a theme card switches the interface
/// theme; Custom shows its colour pickers, and Esc goes back to the slide.
#[test]
fn the_gear_opens_settings_and_theme_cards_apply() {
    let mut h = harness(new_app());
    h.get_by_label("Open Settings").click();
    h.run_steps(3);
    assert_eq!(page(&h), Some(Page::Settings));
    assert_eq!(h.state().backstage.as_ref().map(|b| b.settings_tab), Some(SettingsTab::General));
    h.get_by_label("Themes").click();
    h.run_steps(3);
    assert_eq!(h.state().backstage.as_ref().map(|b| b.settings_tab), Some(SettingsTab::Themes));

    h.get_by_label("Lagoon").click();
    h.run_steps(3);
    assert_eq!(h.state().ui.ui_theme, theme::UiTheme::Lagoon);
    assert_eq!(Tokens::get(&h.ctx).accent, h.state().tokens().accent, "the theme is applied");

    h.get_by_label("High Contrast").click();
    h.run_steps(3);
    assert!(Tokens::get(&h.ctx).high_contrast);
    assert!(h.query_by_label("Window tint").is_none());

    h.get_by_label("Custom").click();
    h.run_steps(3);
    assert!(h.query_by_label("Window tint").is_some(), "Custom shows its colours");

    h.key_press(Key::Escape);
    h.run_steps(3);
    assert_eq!(page(&h), None);
    assert_eq!(h.state().ui.ui_theme, theme::UiTheme::Custom, "leaving keeps the theme");
}

#[test]
fn ui_theme_command_sets_and_validates() {
    let mut app = SlideApp::new(Session::new(), Services::default());
    let r = app.run("view.uiTheme", json!({"theme": "highContrast"})).unwrap();
    assert_eq!(r["theme"], "highContrast");
    assert!(app.tokens().high_contrast);

    // A custom colour alone switches to Custom.
    let r = app.run("view.uiTheme", json!({"tint": "#00ff80", "strength": 7.5})).unwrap();
    assert_eq!(app.ui.ui_theme, theme::UiTheme::Custom);
    assert_eq!(app.ui.custom_theme.tint, [0x00, 0xFF, 0x80]);
    assert_eq!(r["strength"].as_f64(), Some(1.0), "strength is clamped");

    // Hostile input is refused and changes nothing.
    let before = (app.ui.ui_theme, app.ui.custom_theme);
    for bad in [
        json!({"theme": "neon"}),
        json!({"theme": 3}),
        json!({"accent": "#12345"}),
        json!({"accent": "#zzzzzz"}),
        json!({"accent": "#+1+2+3"}),
        json!({"tint": 42}),
        json!({"accent": "#ééé"}),
        json!({"strength": "lots"}),
    ] {
        assert!(app.run("view.uiTheme", bad.clone()).is_err(), "{bad} should be refused");
        assert_eq!((app.ui.ui_theme, app.ui.custom_theme), before, "{bad} changed the theme");
    }
}

#[test]
fn the_theme_is_saved_with_the_ui_state() {
    let mut app = SlideApp::new(Session::new(), Services::default());
    app.run("view.uiTheme", json!({"theme": "lagoon"})).unwrap();
    app.run("view.dark", json!({"on": true})).unwrap();
    let saved = serde_json::to_value(&app.ui).unwrap();
    assert_eq!(saved["uiTheme"], "lagoon");
    let back: crate::UiState = serde_json::from_value(saved).unwrap();
    assert_eq!((back.ui_theme, back.brightness), (theme::UiTheme::Lagoon, theme::Brightness::Dark));
    // Settings saved before themes existed still load, as Standard.
    let old: crate::UiState = serde_json::from_value(json!({"brightness": "dark", "tab": "Home"})).unwrap();
    assert_eq!((old.ui_theme, old.brightness), (theme::UiTheme::Standard, theme::Brightness::Dark));
}

// ---------- Keyboard shortcuts (File › Settings, #127) and Reset all settings ----------

fn ctrl_alt() -> Modifiers {
    Modifiers { ctrl: true, alt: true, command: true, ..Modifiers::NONE }
}

/// Ctrl+Alt+Shift (Ctrl+Alt+G and Ctrl+Alt+Shift+G are taken; Ctrl+Alt+Shift+J is free).
fn ctrl_alt_shift() -> Modifiers {
    Modifiers { shift: true, ..ctrl_alt() }
}

/// A custom key runs its action in the editor, and the action's built-in key stops working.
#[test]
fn a_custom_editor_key_replaces_the_built_in_one() {
    let mut h = harness(new_app());
    h.state_mut().run("keys.set", json!({"action": "view.gridlines", "keys": ["Cmd+Alt+Shift+J"]})).unwrap();
    assert!(!h.state().ui.gridlines);
    h.key_press_modifiers(ctrl_alt_shift(), Key::J);
    h.run_steps(2);
    assert!(h.state().ui.gridlines, "Ctrl+Alt+Shift+J toggles the gridlines");
    h.key_press_modifiers(Modifiers::SHIFT, Key::F9);
    h.run_steps(2);
    assert!(h.state().ui.gridlines, "Shift+F9 no longer does");
    h.state_mut().run("keys.reset", json!({})).unwrap();
    h.key_press_modifiers(Modifiers::SHIFT, Key::F9);
    h.run_steps(2);
    assert!(!h.state().ui.gridlines, "after a reset Shift+F9 works again");
}

/// Ctrl+C arrives as a Copy event too: once Copy has other keys, the event no longer copies.
#[test]
fn a_moved_copy_key_no_longer_copies() {
    let mut h = harness(new_app());
    let title = h.state().session.active().unwrap().doc.slides[0].shapes[0].id.0;
    h.state_mut().session.execute("text.edit", &json!({"id": title})).unwrap();
    h.state_mut().session.type_text("Aurora").unwrap();
    h.state_mut().session.key("Escape", deckcraft_engine::Mods::default()).unwrap();
    h.state_mut().run("keys.set", json!({"action": "edit.copy", "keys": ["Cmd+Alt+C"]})).unwrap();
    h.event(egui::Event::Copy);
    h.run_steps(2);
    assert_eq!(h.state().session.clipboard.plain, "", "the Copy event was dropped");
    h.key_press_modifiers(ctrl_alt(), Key::C);
    h.run_steps(2);
    assert_eq!(h.state().session.clipboard.plain, "Aurora", "Ctrl+Alt+C copies");
}

/// Slide-show keys can be remapped to plain keys.
#[test]
fn a_custom_show_key_advances_the_show() {
    let mut app = new_app();
    app.session.execute("slide.new", &json!({})).unwrap();
    app.session.execute("slide.new", &json!({})).unwrap();
    app.run("keys.set", json!({"action": "show:next", "keys": ["X"]})).unwrap();
    let mut h = harness(app);
    h.state_mut().start_show(0, true);
    h.run_steps(3);
    let slide = |h: &Harness<'_, SlideApp>| h.state().show.as_ref().map(|s| s.state.slide);
    assert_eq!(slide(&h), Some(0));
    h.key_press(Key::Space);
    h.run_steps(3);
    assert_eq!(slide(&h), Some(0), "Space no longer advances");
    h.key_press(Key::X);
    h.run_steps(3);
    assert_eq!(slide(&h), Some(1), "X does");
}

/// The page records a key from a real key press, and asks before taking one another command has.
#[test]
fn the_shortcuts_page_records_keys_and_asks_before_moving_one() {
    let mut h = harness(new_app());
    h.state_mut().run("app.backstage", json!({"page": "settings"})).unwrap();
    h.run_steps(3);
    assert_eq!(page(&h), Some(Page::Settings));
    h.get_by_label("Keyboard Shortcuts").click();
    h.run_steps(3);
    assert_eq!(h.state().backstage.as_ref().map(|b| b.settings_tab), Some(SettingsTab::Shortcuts));
    h.state_mut().backstage.as_mut().unwrap().query = "gridlines".into();
    h.run_steps(2);
    h.get_by_label("Add a key to Gridlines").click();
    h.run_steps(2);
    assert_eq!(h.state().backstage.as_ref().unwrap().recording.as_deref(), Some("view.gridlines"));
    h.key_press_modifiers(ctrl_alt_shift(), Key::J);
    h.run_steps(3);
    let keys = |h: &Harness<'_, SlideApp>| h.state().ui.keymap.0.get("view.gridlines").cloned();
    assert_eq!(keys(&h), Some(vec!["Shift+F9".to_string(), "Cmd+Alt+Shift+J".to_string()]));
    assert_eq!(page(&h), Some(Page::Settings), "recording swallowed the key: nothing else ran");

    // Ctrl+O belongs to Open: the page asks, and only moves it when told to.
    h.get_by_label("Add a key to Gridlines").click();
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::O);
    h.run_steps(3);
    assert!(h.state().backstage.as_ref().unwrap().clash.is_some());
    assert_eq!(keys(&h).map(|k| k.len()), Some(2), "nothing moved yet");
    h.get_by_label("Move it here").click();
    h.run_steps(3);
    assert!(keys(&h).unwrap().contains(&"Cmd+O".to_string()));
    assert_eq!(h.state().ui.keymap.0.get("app.openDialog"), Some(&vec![]), "Open gave up Ctrl+O");

    // An editor shortcut without Ctrl or Alt is refused.
    h.get_by_label("Add a key to Gridlines").click();
    h.run_steps(2);
    h.key_press(Key::K);
    h.run_steps(3);
    assert!(h.state().backstage.as_ref().unwrap().key_note.is_some());
    assert_eq!(keys(&h).map(|k| k.len()), Some(3));
}

/// Settings › Reset asks first, then puts everything back but the recent files.
#[test]
fn reset_all_settings_asks_then_resets_but_keeps_recent_files() {
    let mut app = new_app();
    app.ui.recent = vec!["talk.pptx".into()];
    app.run("view.uiTheme", json!({"theme": "highContrast"})).unwrap();
    app.run("view.dark", json!({"on": true})).unwrap();
    app.run("keys.set", json!({"action": "edit.copy", "keys": ["Cmd+Alt+C"]})).unwrap();
    app.session.prefs.smart_quotes = false;
    let mut h = harness(app);
    h.state_mut().run("app.backstage", json!({"page": "settings", "tab": "reset"})).unwrap();
    h.run_steps(3);
    h.get_by_label("Reset all settings…").click();
    h.run_steps(2);
    assert_eq!(h.state().ui.ui_theme, theme::UiTheme::HighContrast, "nothing happens before confirming");
    click_on_page(&h, "Yes, reset everything");
    h.run_steps(3);
    let app = h.state();
    assert_eq!((app.ui.ui_theme, app.ui.brightness), (theme::UiTheme::Standard, theme::Brightness::Light));
    assert!(app.ui.keymap.0.is_empty() && app.session.prefs.smart_quotes);
    assert_eq!(app.ui.recent, vec!["talk.pptx".to_string()]);
    assert!(!Tokens::get(&h.ctx).high_contrast, "the look is reset too");
}

// ---------- File › Settings: General, New Presentations, search, backup, cheat sheet ----------

fn settings_tab(h: &Harness<'_, SlideApp>) -> Option<SettingsTab> {
    h.state().backstage.as_ref().map(|b| b.settings_tab)
}

/// The search box finds a setting by its name or by other words, and jumps to its tab.
#[test]
fn settings_search_finds_settings_and_jumps_to_their_tab() {
    let tabs = |q: &str| settings_search(q).into_iter().map(|h| h.0).collect::<Vec<_>>();
    assert!(tabs("dark").contains(&SettingsTab::Themes));
    assert_eq!(tabs("zoom"), vec![SettingsTab::General]);
    assert_eq!(tabs("factory"), vec![SettingsTab::Backup]);
    assert_eq!(tabs("PRINT cheat"), vec![SettingsTab::Shortcuts], "every word, any case");
    assert!(settings_search("").is_empty() && settings_search("zzz").is_empty());

    let mut h = harness(new_app());
    h.state_mut().run("app.backstage", json!({"page": "settings"})).unwrap();
    h.run_steps(3);
    h.get_by_label("Search settings").click();
    h.run_steps(2);
    h.get_by_label("Search settings").type_text("4:3");
    h.run_steps(3);
    assert_eq!(settings_tab(&h), Some(SettingsTab::NewPresentations));
    assert_eq!(h.query_all_by_label("Slide size").count(), 2, "listed under its tab, and shown on it");
}

/// General: interface size (presets, steps, limits), reduce motion and what opens at start.
#[test]
fn the_general_tab_sets_size_motion_and_startup() {
    let mut h = harness(new_app());
    h.state_mut().run("app.backstage", json!({"page": "settings", "tab": "general"})).unwrap();
    h.run_steps(3);
    h.get_by_label("125%").click();
    h.run_steps(3);
    assert_eq!(h.state().ui.ui_scale, 1.25);
    h.get_by_label("+").click();
    h.run_steps(3);
    assert!((h.state().ui.ui_scale - 1.3).abs() < 1e-4);
    assert_eq!(h.state_mut().run("view.interfaceSize", json!({"scale": 50})).unwrap()["scale"], 2.0);
    assert!(h.state_mut().run("view.interfaceSize", json!({"scale": "big"})).is_err());
    h.state_mut().run("view.interfaceSize", json!({"scale": 1})).unwrap();
    h.run_steps(3);

    h.get_by_label("Reduce motion").click();
    h.run_steps(3);
    assert!(h.state().ui.reduce_motion);
    assert_eq!(h.ctx.global_style().animation_time, 0.0, "the interface stops animating");

    h.get_by_label("A new blank presentation").click();
    h.run_steps(3);
    assert_eq!(h.state().ui.startup, crate::Startup::Blank);
    assert!(h.state_mut().run("app.startup", json!({"open": "sometimes"})).is_err());
}

/// What opens at start, when nothing else did.
#[test]
fn startup_opens_what_the_setting_says() {
    let fresh = |startup: crate::Startup, recent: Vec<String>| {
        let mut app = SlideApp::new(Session::new(), Services::default());
        app.ui.startup = startup;
        app.ui.recent = recent;
        app.startup();
        app
    };
    assert!(fresh(crate::Startup::StartScreen, vec![]).session.active().is_none());
    assert!(fresh(crate::Startup::Blank, vec![]).session.active().is_some());
    // The last file is gone: no crash, the start screen and a note.
    let app = fresh(crate::Startup::LastFile, vec!["/no/such/talk.pptx".into()]);
    assert!(app.session.active().is_none());
    assert!(app.status.as_ref().is_some_and(|s| s.0.contains("talk.pptx")));

    let dir = temp_dir("startup");
    let path = dir.join("talk.deckcraft").to_string_lossy().to_string();
    let mut maker = new_app();
    maker.session.execute("file.saveAs", &json!({"path": path})).unwrap();
    let app = fresh(crate::Startup::LastFile, vec![path.clone()]);
    assert_eq!(app.session.active().and_then(|d| d.path.clone()), Some(path));
    // Something already open (a file given on the command line) wins.
    let mut app = new_app();
    app.ui.startup = crate::Startup::Blank;
    app.startup();
    assert_eq!(app.session.documents().len(), 1);
}

/// New Presentations: theme, slide size and font for every new presentation.
#[test]
fn the_new_presentations_tab_sets_what_new_presentations_start_with() {
    let last = deckcraft_model::theme::builtin_themes().last().map(|t| t.name.clone()).unwrap();
    let mut h = harness(new_app());
    h.state_mut().run("app.backstage", json!({"page": "settings", "tab": "new"})).unwrap();
    h.run_steps(3);
    h.get_by_label("Standard (4:3)").click();
    h.run_steps(3);
    let app = h.state_mut();
    assert_eq!(app.session.prefs.new_size, "standard");
    app.run("settings.newPresentation", json!({"theme": last.to_uppercase(), "font": "Georgia"})).unwrap();
    assert_eq!(app.session.prefs.new_theme, last, "stored under its own name");
    assert!(app.run("settings.newPresentation", json!({"theme": "Not A Theme"})).is_err());
    assert!(app.run("settings.newPresentation", json!({"size": "huge"})).is_err());
    app.run("file.new", json!({})).unwrap();
    let doc = &app.session.active().unwrap().doc;
    assert_eq!(doc.slide_size, deckcraft_model::defaults::STANDARD);
    assert_eq!((doc.masters[0].theme.name.as_str(), doc.masters[0].theme.fonts.minor.latin.as_str()), (last.as_str(), "Georgia"));
    let v = app.run("settings.newPresentation", json!({"theme": "", "size": "widescreen", "font": ""})).unwrap();
    assert_eq!(v, json!({"theme": "", "size": "widescreen", "font": "", "defaultShape": false}));
}

/// Backup and Reset: a backup file brings every setting back, but not the recent files; a file
/// that isn't a backup changes nothing.
#[test]
fn settings_back_up_to_a_file_and_restore_from_it() {
    let dir = temp_dir("settings-backup");
    let file = dir.join("DeckCraft settings.json");
    let services = |dir: std::path::PathBuf| Services {
        pick_save: Some(Box::new(move |name: &str| Some(dir.join(name).to_string_lossy().to_string()))),
        pick_open: Some(Box::new({
            let f = file.to_string_lossy().to_string();
            move |_: &str| Some(f.clone())
        })),
        read: Some(Box::new(|p: &str| std::fs::read(p).map_err(|e| e.to_string()))),
        write: Some(Box::new(|p: &str, b: &[u8]| std::fs::write(p, b).map_err(|e| e.to_string()))),
        ..Default::default()
    };
    let mut app = SlideApp::new(Session::with_new(), services(dir.clone()));
    app.ui.recent = vec!["mine.pptx".into()];
    app.run("view.uiTheme", json!({"theme": "lagoon"})).unwrap();
    app.run("keys.set", json!({"action": "view.gridlines", "keys": ["Cmd+Alt+Shift+J"]})).unwrap();
    app.run("view.reduceMotion", json!({"on": true})).unwrap();
    app.session.prefs.smart_quotes = false;
    let mut h = harness(app);
    h.state_mut().run("app.backstage", json!({"page": "settings", "tab": "backup"})).unwrap();
    h.run_steps(3);
    h.get_by_label("Back up settings…").click();
    h.run_steps(3);
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("lagoon") && !text.contains("mine.pptx"), "the recent files stay out of the backup");

    let mut other = SlideApp::new(Session::with_new(), services(dir.clone()));
    other.ui.recent = vec!["theirs.pptx".into()];
    let mut h = harness(other);
    h.state_mut().run("app.backstage", json!({"page": "settings", "tab": "backup"})).unwrap();
    h.run_steps(3);
    h.get_by_label("Restore from a backup…").click();
    h.run_steps(3);
    let app = h.state_mut();
    assert_eq!(app.ui.ui_theme, theme::UiTheme::Lagoon);
    assert!(app.ui.reduce_motion && !app.session.prefs.smart_quotes);
    assert_eq!(app.ui.keymap.0.get("view.gridlines"), Some(&vec!["Cmd+Alt+Shift+J".to_string()]));
    assert_eq!(app.ui.recent, vec!["theirs.pptx".to_string()]);

    // Not a backup: refused, nothing changes.
    for bad in [json!({"deckcraft": "presentation"}), json!({"deckcraft": "settings", "ui": 5}), json!([1, 2])] {
        assert!(app.run("settings.import", json!({"settings": bad})).is_err());
    }
    std::fs::write(&file, b"\x00\x01 not json").unwrap();
    assert!(app.run("settings.import", json!({"path": file.to_string_lossy()})).is_err());
    assert_eq!(app.ui.ui_theme, theme::UiTheme::Lagoon);
}

/// Keyboard Shortcuts › Save cheat sheet…: a printable page of every key, the custom ones too.
#[test]
fn the_cheat_sheet_lists_every_shortcut() {
    let dir = temp_dir("cheat-sheet");
    let target = dir.clone();
    let services = Services {
        pick_save: Some(Box::new(move |name: &str| Some(target.join(name).to_string_lossy().to_string()))),
        write: Some(Box::new(|p: &str, b: &[u8]| std::fs::write(p, b).map_err(|e| e.to_string()))),
        ..Default::default()
    };
    let mut app = SlideApp::new(Session::with_new(), services);
    app.run("keys.set", json!({"action": "view.gridlines", "keys": ["Cmd+Alt+Shift+J"]})).unwrap();
    let v = app.run("keys.cheatSheet", json!({})).unwrap();
    let text = v["text"].as_str().unwrap();
    assert!(text.contains("Gridlines") && text.contains("Ctrl+Alt+Shift+J"), "{text}");
    assert!(text.contains("Slide Show"));
    let mut h = harness(app);
    h.state_mut().run("app.backstage", json!({"page": "settings", "tab": "shortcuts"})).unwrap();
    h.run_steps(3);
    h.get_by_label("Save cheat sheet…").click();
    h.run_steps(3);
    let html = std::fs::read_to_string(dir.join("DeckCraft keyboard shortcuts.html")).unwrap();
    assert!(html.starts_with("<!doctype html>") && html.contains("<kbd>"));
    assert!(h.state().status.as_ref().is_some_and(|s| s.0.contains("cheat sheet")), "{:?}", h.state().status);
}
