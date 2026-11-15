//! The macOS menu bar as AppKit sees it, on the main thread (`harness = false`): install it through
//! the real muda backend (`src/native_menu.rs`) and read NSApp back. The Window menu must be NSApp's
//! `windowsMenu`: that is where macOS adds Move & Resize, Fill and Center and lists the windows.

#[cfg(target_os = "macos")]
#[path = "../src/native_menu.rs"]
#[allow(unused_imports, dead_code)]
mod native_menu;

#[cfg(target_os = "macos")]
fn main() {
    use deckcraft_engine::Session;
    use deckcraft_ui_egui::{Services, SlideApp};
    use objc2::MainThreadMarker;
    use objc2::rc::Retained;
    use objc2_app_kit::{NSApplication, NSMenu, NSMenuItem};
    use objc2_foundation::NSString;

    let mtm = MainThreadMarker::new().expect("harness = false runs on the main thread");
    let nsapp = NSApplication::sharedApplication(mtm);
    nsapp.finishLaunching();

    let ctx = egui::Context::default();
    let mut app = SlideApp::new(Session::new(), Services::default());
    let mut menu = native_menu::NativeMenu::install(&ctx, &app);

    let bar = nsapp.mainMenu().expect("a main menu");
    let tops: Vec<String> = (0..bar.numberOfItems()).filter_map(|i| bar.itemAtIndex(i)?.submenu()).map(|m| m.title().to_string()).collect();
    assert_eq!(tops, ["DeckCraft", "File", "Edit", "View", "Insert", "Format", "Arrange", "Tools", "Slide Show", "Window", "Help"]);
    assert_eq!(nsapp.windowsMenu().map(|m| m.title().to_string()).as_deref(), Some("Window"), "AppKit knows the Window menu");

    let file: Retained<NSMenu> = bar.itemWithTitle(&NSString::from_str("File")).and_then(|i| i.submenu()).expect("File");
    let save = |m: &NSMenu| -> Retained<NSMenuItem> { m.itemWithTitle(&NSString::from_str("Save")).expect("Save") };
    // With no presentation open, Save is dimmed and Open isn't.
    menu.refresh(&app, &ctx);
    assert!(!save(&file).isEnabled(), "Save is dimmed without a presentation");
    assert!(file.itemWithTitle(&NSString::from_str("Open…")).expect("Open…").isEnabled());
    let _ = app.run("file.new", serde_json::json!({}));
    menu.refresh(&app, &ctx);
    assert!(save(&file).isEnabled(), "Save is enabled once a presentation is open");
    let tools = bar.itemWithTitle(&NSString::from_str("Tools")).and_then(|i| i.submenu()).expect("Tools");
    assert!(!tools.itemWithTitle(&NSString::from_str("Add-ins…")).expect("Add-ins…").isEnabled(), "no add-ins yet");

    println!("mac_menu_appkit: ok");
}

#[cfg(not(target_os = "macos"))]
fn main() {}
