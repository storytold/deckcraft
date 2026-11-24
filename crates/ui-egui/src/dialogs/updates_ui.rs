//! One background check at start-up (and on demand from Help) for a newer release, the modal
//! prompt that offers to update and restart, and the result of a check, as craft-update's dialogs
//! on the app's [`DialogHost`](craft_ui::dialog::DialogHost). The flow itself is
//! `craft_update::updater::Updater`.

use craft_ui::dialog::DialogResult;
use craft_update::stack_dialogs::{CheckChoice, UpdateCheckDialog};
use craft_update::updater::{Action, Status, Updater};

use crate::SlideApp;

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub(crate) struct Runtime {
    updater: Updater,
    /// The desktop host turns the check at start on; tests and the web build leave it off.
    enabled: bool,
    check_requested: bool,
}

impl Default for Runtime {
    fn default() -> Self {
        Self { updater: Updater::new(&crate::UPDATE_APP, VERSION), enabled: false, check_requested: false }
    }
}

pub(crate) fn enable(app: &mut SlideApp) {
    app.updates_rt.enabled = true;
}

/// Help > Check for Updates…: runs a check and reports the outcome either way.
pub(crate) fn request_check(app: &mut SlideApp) {
    if !matches!(app.updates_rt.updater.status(), Status::Checking) {
        app.updates_rt.check_requested = true;
    }
}

/// Starts the helper of an update that is ready, so that it installs once the process has exited.
/// Later calls do nothing.
pub(crate) fn start_pending_update(app: &mut SlideApp) {
    app.updates_rt.updater.start_pending_update();
}

pub(crate) fn tick(app: &mut SlideApp, ctx: &egui::Context) {
    let rt = &mut app.updates_rt;
    if std::mem::take(&mut rt.check_requested) {
        rt.updater.check_now();
    } else if rt.enabled {
        rt.updater.check_at_start();
    }
    rt.updater.tick(ctx);
}

/// One pass of the update flow, before the dialog host runs (`SlideApp::ui`): answers the result
/// dialog closed in the last pass, drives the startup prompt, and keeps the result dialog open and
/// current while the updater has something to report.
pub(crate) fn show(app: &mut SlideApp, ctx: &egui::Context) {
    // An installation starts, and a ready update asks the app to close, only while nothing is unsaved.
    let clean = !app.session.documents().iter().any(|d| d.is_dirty());
    // Answered before the dialog is kept open below, so a dismissed result does not open again.
    if let Some(result) = app.framework_dialogs.take::<UpdateCheckDialog>() {
        answer(&mut app.updates_rt, ctx, result, clean);
    }
    match app.updates_rt.updater.run(&mut app.framework_dialogs, clean) {
        Action::OpenUrl(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
        // The app's close path asks about unsaved work; `on_exit` starts the restart helper.
        Action::Close => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        _ => {}
    }
    // The result dialog reports a requested check, and at the first start after an update what
    // that update came to (installed, or why it failed).
    let updater = &app.updates_rt.updater;
    let reports = !matches!(updater.status(), Status::Idle);
    let (status, installing, message) = (updater.status().clone(), updater.is_installing(), updater.status_message());
    let open = app.framework_dialogs.is_open::<UpdateCheckDialog>();
    if reports && !open {
        app.framework_dialogs.open(UpdateCheckDialog::new(VERSION));
    } else if !reports && open {
        app.framework_dialogs.cancel::<UpdateCheckDialog>(ctx);
    }
    if let Some(dialog) = app.framework_dialogs.get_mut::<UpdateCheckDialog>() {
        dialog.set_status(status);
        dialog.set_installing(installing);
        dialog.set_message(message);
    }
}

/// Acts on the result dialog's answer.
fn answer(rt: &mut Runtime, ctx: &egui::Context, result: DialogResult<CheckChoice>, clean: bool) {
    match result {
        DialogResult::Ok(CheckChoice::CheckNow | CheckChoice::CheckAgain) => rt.check_requested = true,
        // The next pass's `run` asks the window to close once nothing is unsaved.
        DialogResult::Ok(CheckChoice::RestartToUpdate) => rt.updater.restart_to_update(),
        DialogResult::Ok(CheckChoice::Update) => {
            rt.updater.request_install(clean);
        }
        DialogResult::Ok(CheckChoice::Download) => ctx.open_url(egui::OpenUrl::new_tab(crate::UPDATE_APP.releases_url())),
        // OK, Esc and the close box dismiss the outcome it showed.
        _ => rt.updater.dismiss_status(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_offered_release_opens_the_prompt() {
        let mut app = SlideApp::new(deckcraft_engine::Session::new(), crate::Services::default());
        let ctx = egui::Context::default();
        crate::framework::install_headless(&ctx);
        assert!(!app.updates_rt.updater.prompt_open());
        app.updates_rt.updater.offer(craft_update::Release { tag: "v99.0.0".into(), ..Default::default() });
        // The offer reads on a thread whether this copy can install the release itself; a later
        // pass's tick opens the prompt.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !app.updates_rt.updater.prompt_open() && std::time::Instant::now() < deadline {
            ctx.run_ui(egui::RawInput::default(), |ui| {
                tick(&mut app, ui.ctx());
                show(&mut app, ui.ctx());
                app.framework_dialogs.run(ui);
            })
            .textures_delta
            .clear();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(app.updates_rt.updater.prompt_open());
    }

    /// An app whose updater keeps its outcome records under `root`, reads no launcher choice and
    /// asks no server, with the check at start turned on.
    fn app_with_records(root: &std::path::Path) -> SlideApp {
        let mut app = SlideApp::new(deckcraft_engine::Session::new(), crate::Services::default());
        app.updates_rt.updater = Updater::new(&crate::UPDATE_APP, VERSION)
            .with_paths(craft_update::Paths::under(root))
            .with_pins_path(root.join("pins.json"))
            .with_source(std::sync::Arc::new(|| Ok(None)));
        app.updates_rt.enabled = true;
        app
    }

    fn records_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("deckcraft-updates-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// The first start after an update reports it; OK dismisses the report.
    #[test]
    fn the_start_after_an_update_reports_it_until_dismissed() {
        let root = records_dir("installed");
        std::fs::create_dir_all(root.join("data")).unwrap();
        std::fs::write(root.join("data").join("self-update-done.txt"), VERSION).unwrap();
        let mut app = app_with_records(&root);
        let ctx = egui::Context::default();
        crate::framework::install_headless(&ctx);
        ctx.run_ui(egui::RawInput::default(), |ui| {
            tick(&mut app, ui.ctx());
            show(&mut app, ui.ctx());
            app.framework_dialogs.run(ui);
        })
        .textures_delta
        .clear();
        assert!(matches!(app.updates_rt.updater.status(), Status::Updated(_)), "{:?}", app.updates_rt.updater.status());
        answer(&mut app.updates_rt, &ctx, DialogResult::Ok(CheckChoice::Close), true);
        assert_eq!(app.updates_rt.updater.status(), &Status::Idle);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The first start after a failed update says why; Check Again asks for a new check.
    #[test]
    fn the_start_after_a_failed_update_reports_why_and_offers_a_new_check() {
        let root = records_dir("failed");
        std::fs::create_dir_all(root.join("data").join("logs")).unwrap();
        std::fs::write(root.join("data").join("logs").join("self-update-error.txt"), "the disk is full").unwrap();
        let mut app = app_with_records(&root);
        let ctx = egui::Context::default();
        crate::framework::install_headless(&ctx);
        ctx.run_ui(egui::RawInput::default(), |ui| {
            tick(&mut app, ui.ctx());
            show(&mut app, ui.ctx());
            app.framework_dialogs.run(ui);
        })
        .textures_delta
        .clear();
        assert_eq!(app.updates_rt.updater.status(), &Status::InstallFailed("the disk is full".into()));
        answer(&mut app.updates_rt, &ctx, DialogResult::Ok(CheckChoice::CheckAgain), true);
        assert!(app.updates_rt.check_requested);
        let _ = std::fs::remove_dir_all(&root);
    }
}
