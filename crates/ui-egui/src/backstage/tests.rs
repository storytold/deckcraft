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

use crate::backstage::{Backstage, NAV_W, Nav, Page, PrintScope, entries, format_bytes, print_page_size, print_params, print_slides, split_path};
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
    assert_eq!(bottom, vec![Nav::Page(Page::Account), Nav::Options]);
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

/// Export › Change File Type › PNG offers "Transparent background" (#784): the PNG it saves
/// leaves the slide background out. Other types don't show the option.
#[test]
fn change_file_type_png_can_leave_the_background_transparent() {
    let dir = temp_dir("png-clear");
    let (services, _) = saving_into(&dir);
    let mut h = harness(SlideApp::new(Session::with_new(), services));
    h.state_mut().run("app.backstage", json!({"page": "export"})).unwrap();
    h.run_steps(3);
    click_on_page(&h, "Change File Type");
    h.run_steps(3);
    assert!(h.query_by_label("Transparent background").is_none(), "only PNG has the option");
    click_on_page(&h, "PNG Picture (*.png)");
    h.run_steps(2);
    h.get_by_label("Transparent background").click();
    h.run_steps(2);
    assert!(h.state().backstage.as_ref().is_some_and(|b| b.png_transparent));
    click_on_page(&h, "Save As");
    h.run_steps(3);
    let title = h.state().session.active().unwrap().title();
    let png = std::fs::read(dir.join(format!("{title}.png"))).unwrap();
    let img = deckcraft_render::decode_image(&png).unwrap();
    assert_eq!(img.get_pixel(2, 2).0[3], 0, "the background is left out");
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

/// The start screen is not a hidden slide: typing, pasting and IME events must not create or
/// edit a presentation while the File-tab navigation keys still work.
#[test]
fn start_screen_ignores_text_paste_and_ime_while_file_keys_navigate() {
    let mut h = harness(SlideApp::new(Session::new(), Services::default()));
    assert_eq!(page(&h), Some(Page::Home));
    h.input_mut().events.extend([
        egui::Event::Text("x".into()),
        egui::Event::Paste("pasted".into()),
        egui::Event::Ime(egui::ImeEvent::Preedit { text: "あ".into(), active_range_chars: None }),
        egui::Event::Ime(egui::ImeEvent::Commit("あ".into())),
        egui::Event::Key { key: Key::ArrowDown, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::default() },
    ]);
    h.run_steps(2);
    assert!(h.state().session.active().is_none(), "no typing or IME event opened a presentation");
    assert!(h.state().session.documents().is_empty(), "the start screen has no slide to edit");
    assert!(h.state().status.is_none(), "typing, paste and IME were ignored, not run and refused");
    assert_eq!(page(&h), Some(Page::New), "the File-tab arrow still navigates");
    assert!(h.output().platform_output.ime.is_none(), "the start screen publishes no input-method descriptor");
}

/// The start screen keeps the shortcuts that open something in the app: the command palette and
/// Preferences. Both are in-app windows, so no native dialog is involved.
#[test]
fn start_screen_opens_the_palette_and_preferences() {
    let mut h = harness(SlideApp::new(Session::new(), Services::default()));
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::P);
    h.run_steps(3);
    assert!(h.state().palette.is_some(), "Ctrl+Shift+P opens the command palette");
    assert!(h.state().session.active().is_none(), "without creating a presentation");

    let mut h = harness(SlideApp::new(Session::new(), Services::default()));
    h.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("preferences"), "Ctrl+, opens Preferences");
    assert!(h.state().session.active().is_none(), "without creating a presentation");
}

/// Ctrl+O still asks the app's open dialog on the start screen, and Alt+F opens the File tab with
/// the keyboard in its side menu. The open dialog is a mock service, not the host's picker.
#[test]
fn start_screen_opens_files_and_the_file_tab_from_the_keyboard() {
    let opens: Rc<RefCell<Vec<String>>> = Default::default();
    let log = opens.clone();
    let services = Services {
        pick_open: Some(Box::new(move |purpose: &str| {
            log.borrow_mut().push(purpose.to_string());
            None
        })),
        ..Default::default()
    };
    let mut h = harness(SlideApp::new(Session::new(), services));
    h.key_press_modifiers(Modifiers::COMMAND, Key::O);
    h.run_steps(3);
    let asked = opens.borrow();
    assert_eq!(asked.len(), 1, "Ctrl+O asks the app's open dialog once");
    assert_eq!(asked[0], "open", "Ctrl+O reaches the app's open dialog");
    assert!(h.state().session.active().is_none(), "a cancelled open leaves the start screen");
    assert_eq!(page(&h), Some(Page::Home));

    let mut h = harness(SlideApp::new(Session::new(), Services::default()));
    assert_eq!(h.state().backstage.as_ref().and_then(|b| b.focus), None);
    h.key_press_modifiers(Modifiers::ALT, Key::F);
    h.run_steps(3);
    assert_eq!(page(&h), Some(Page::Home), "Alt+F opens the File tab on Home");
    assert_eq!(h.state().backstage.as_ref().and_then(|b| b.focus), Some(0), "with the keyboard in the side menu");
}

/// Document commands stay off the empty start screen: no save picker, find palette, hyperlink
/// dialog, close prompt, status error or page change.
#[test]
fn start_screen_ignores_document_shortcuts() {
    let saves: Rc<RefCell<Vec<String>>> = Default::default();
    let log = saves.clone();
    let services = Services {
        pick_save: Some(Box::new(move |suggested: &str| {
            log.borrow_mut().push(suggested.to_string());
            None
        })),
        ..Default::default()
    };
    let mut h = harness(SlideApp::new(Session::new(), services));
    for (mods, key) in [(Modifiers::COMMAND, Key::S), (Modifiers::COMMAND, Key::K), (Modifiers::COMMAND, Key::F), (Modifiers::COMMAND, Key::W)] {
        h.key_press_modifiers(mods, key);
        h.run_steps(2);
        assert!(h.state().session.active().is_none(), "{key:?} did not open a presentation");
        assert!(h.state().session.documents().is_empty(), "{key:?} created no document");
        assert_eq!(page(&h), Some(Page::Home), "{key:?} left the start screen in place");
        assert!(h.state().status.is_none(), "{key:?} was ignored rather than run and refused");
        assert!(h.state().dialog.is_none(), "{key:?} opened no document dialog");
        assert!(h.state().palette.is_none(), "{key:?} opened no find palette");
    }
    assert!(saves.borrow().is_empty(), "Ctrl+S did not ask for a file");
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

/// The Print page's colour box says Grayscale, and ticking it turns the preview gray at once: the
/// gray page is its own raster, requested on the toggle, and unticking brings the colour one back.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn print_grayscale_box_is_named_and_greys_the_preview() {
    fn list() -> Vec<String> {
        vec!["Office".into()]
    }
    fn submit(_: &[u8], _: &craft_print::Job, _: &mut dyn FnMut(usize, usize)) -> Result<String, String> {
        Ok(String::new())
    }
    let mut app = new_app();
    app.local_print = crate::localprint::Local::with(list, submit);
    let mut h = harness(app);
    h.key_press_modifiers(Modifiers::COMMAND, Key::P);
    h.run_steps(3);
    h.state_mut().backstage.as_mut().unwrap().print_dest = crate::print_dest::PrintDest::Local("Office".into());
    h.run_steps(4);
    assert!(h.query_by_label("Black and white").is_none(), "the old wording is gone");
    let colour_thumbs = h.state().textures.thumb_count();
    h.get_by_label("Grayscale").click();
    h.run_steps(6);
    assert!(h.state().local_print.settings.grayscale);
    assert!(crate::print_dest::print_grayscale(h.state(), h.state().backstage.as_ref().unwrap()));
    assert!(h.state().textures.thumb_count() > colour_thumbs, "a gray raster of the page was requested, beside the colour one");
    h.get_by_label("Grayscale").click();
    h.run_steps(4);
    assert!(!h.state().local_print.settings.grayscale);
    assert!(!crate::print_dest::print_grayscale(h.state(), h.state().backstage.as_ref().unwrap()));
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

/// Print > Network printer through the real page: nothing is sent until the printer is queried and
/// the box is ticked, then Print sends DeckCraft's own PDF for the page's settings, and the finished
/// job's words name the printer.
#[cfg(network_print)]
#[test]
fn print_to_a_network_printer_sends_the_pdf_after_query_and_confirmation() {
    use craft_print::wsd::testing::{FakeConfig, FakePrinter, MemTransport};
    use craft_ui::printing::network as net;

    let printer = FakePrinter::new(FakeConfig::default());
    let mut app = new_app();
    for _ in 0..2 {
        app.run("slide.new", json!({})).unwrap();
    }
    app.set_wsd_transport(std::sync::Arc::new(MemTransport(printer.clone())));
    let mut h = harness(app);
    h.key_press_modifiers(Modifiers::COMMAND, Key::P);
    h.run_steps(3);
    assert_eq!(page(&h), Some(Page::Print));
    h.state_mut().backstage.as_mut().unwrap().print_dest = crate::print_dest::PrintDest::Network;
    h.state_mut().net_print.state.draft = net::Draft::for_address("127.0.0.1:5357/printer");
    h.run_steps(3);
    // Not queried, not confirmed: Print does nothing.
    click_on_page(&h, "Print");
    h.run_steps(3);
    assert_eq!(printer.jobs(), 0);
    click_on_page(&h, "Query printer");
    for _ in 0..2000 {
        h.run_steps(1);
        if matches!(h.state().net_print.state.draft.query, net::Query::Done(_)) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(matches!(h.state().net_print.state.draft.query, net::Query::Done(_)), "{:?}", h.state().net_print.state.draft.query);
    click_on_page(&h, "Print");
    h.run_steps(3);
    assert_eq!(printer.jobs(), 0, "queried but not confirmed");
    h.get_by_label("I confirm sending this document to this printer").click();
    h.run_steps(3);
    click_on_page(&h, "Print");
    for _ in 0..2000 {
        h.run_steps(1);
        if matches!(h.state().net_print.state.draft.job, net::JobView::Finished(_) | net::JobView::Failed(_)) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(
        matches!(&h.state().net_print.state.draft.job, net::JobView::Finished(r) if r.outcome == net::Outcome::Completed),
        "{:?}",
        h.state().net_print.state.draft.job
    );
    assert_eq!(printer.jobs(), 1);
    let sent = printer.job(1).unwrap().document.unwrap();
    assert!(sent.starts_with(b"%PDF"), "DeckCraft's own PDF went to the printer");
    // Three slides (the new deck's one and two added) on full-page slide sheets: the job's page total is the sheet count.
    assert_eq!(h.state().net_print.state.draft.expected_pages, Some(3));
}

/// The network printer's confirmation is for one visit: ticked, then the File tab closed and the
/// Print page opened again, the box is clear and Print sends nothing until it is ticked anew.
#[cfg(network_print)]
#[test]
fn the_network_confirmation_is_cleared_when_the_print_page_is_reopened() {
    use craft_print::wsd::testing::{FakeConfig, FakePrinter, MemTransport};
    use craft_ui::printing::network as net;

    let printer = FakePrinter::new(FakeConfig::default());
    let mut app = new_app();
    app.set_wsd_transport(std::sync::Arc::new(MemTransport(printer.clone())));
    let mut h = harness(app);
    h.key_press_modifiers(Modifiers::COMMAND, Key::P);
    h.run_steps(3);
    h.state_mut().backstage.as_mut().unwrap().print_dest = crate::print_dest::PrintDest::Network;
    h.state_mut().net_print.state.draft = net::Draft::for_address("127.0.0.1:5357/printer");
    h.run_steps(3);
    click_on_page(&h, "Query printer");
    for _ in 0..2000 {
        h.run_steps(1);
        if matches!(h.state().net_print.state.draft.query, net::Query::Done(_)) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    h.get_by_label("I confirm sending this document to this printer").click();
    h.run_steps(3);
    assert!(h.state().net_print.state.draft.confirmed, "ticked");
    h.key_press(Key::Escape);
    h.run_steps(3);
    assert!(!h.state().backstage_open(), "the File tab is closed");
    h.key_press_modifiers(Modifiers::COMMAND, Key::P);
    h.run_steps(3);
    assert_eq!(page(&h), Some(Page::Print));
    h.state_mut().backstage.as_mut().unwrap().print_dest = crate::print_dest::PrintDest::Network;
    h.run_steps(3);
    assert!(!h.state().net_print.state.draft.confirmed, "the confirmation from the earlier visit is void");
    click_on_page(&h, "Print");
    h.run_steps(3);
    assert_eq!(printer.jobs(), 0, "nothing is sent without a new yes");
}

/// The printers of this computer: an empty list says so, and the list is read again when the Print
/// page is opened anew, so a printer added since shows up.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn the_local_printer_list_says_when_it_is_empty_and_is_read_again_on_reopening() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static ADDED: AtomicBool = AtomicBool::new(false);
    fn list() -> Vec<String> {
        if ADDED.load(Ordering::SeqCst) { vec!["Office".into()] } else { Vec::new() }
    }
    fn submit(_: &[u8], _: &craft_print::Job, _: &mut dyn FnMut(usize, usize)) -> Result<String, String> {
        Ok(String::new())
    }
    fn open_list_and_wait(h: &mut Harness<'_, SlideApp>, shown: &str) {
        let list = h.query_all_by_value("PDF viewer (choose the printer there)").next();
        list.or_else(|| h.query_all_by_label("PDF viewer (choose the printer there)").next()).expect("the printer list").click();
        for _ in 0..400 {
            h.run_steps(1);
            if h.query_by_label(shown).is_some() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("“{shown}” never showed");
    }
    let dir = temp_dir("printers");
    let (services, _) = saving_into(&dir);
    let mut app = SlideApp::new(Session::with_new(), services);
    app.local_print = crate::localprint::Local::with(list, submit);
    let mut h = harness(app);
    h.key_press_modifiers(Modifiers::COMMAND, Key::P);
    h.run_steps(3);
    open_list_and_wait(&mut h, "No printers found");
    assert!(h.query_by_label("Office").is_none());
    h.key_press(Key::Escape);
    h.run_steps(2);
    h.state_mut().close_backstage();
    h.run_steps(2);
    ADDED.store(true, Ordering::SeqCst);
    h.key_press_modifiers(Modifiers::COMMAND, Key::P);
    h.run_steps(3);
    open_list_and_wait(&mut h, "Office");
    assert!(h.query_by_label("No printers found").is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Options in the File tab opens the Preferences dialog, and Cancel closes it (#782, #752).
#[test]
fn options_in_backstage_opens_preferences_and_cancel_closes_it() {
    let mut h = harness(new_app());
    h.get_by_label("File").click();
    h.run_steps(3);
    assert!(h.state().backstage_open());
    h.get_by_label("Options").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_some(), "Options opens Preferences dialog");
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("preferences"));
    h.query_all_by_role_and_label(Role::Button, "Cancel").next().expect("Cancel button").click_accesskit();
    h.run_steps(3);
    assert!(h.state().dialog.is_none(), "Cancel closes Preferences dialog");
    assert!(h.state().backstage_open(), "Backstage remains open");
}
