//! One background check at start-up (and on demand from Help) for a newer release, and the modal
//! prompt that offers to update and restart. The flow itself is `craft_update::updater::Updater`.

use craft_update::updater::{Action, Status, Updater};
use craft_update::{CheckResultDialog, CheckResultResponse};

use crate::SlideApp;

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub(crate) struct Runtime {
    updater: Updater,
    /// The desktop host turns the check at start on; tests and the web build leave it off.
    enabled: bool,
    check_requested: bool,
    /// A manual check's result dialog is showing (checking, up to date or failed).
    result_open: bool,
}

impl Default for Runtime {
    fn default() -> Self {
        Self { updater: Updater::new(&crate::UPDATE_APP, VERSION), enabled: false, check_requested: false, result_open: false }
    }
}

pub(crate) fn enable(app: &mut SlideApp) {
    app.updates_rt.enabled = true;
}

/// Help > Check for Updates…: runs a check and reports the outcome either way.
pub(crate) fn request_check(app: &mut SlideApp) {
    if !matches!(app.updates_rt.updater.status(), Status::Checking) {
        app.updates_rt.check_requested = true;
        app.updates_rt.result_open = true;
    }
}

pub(crate) fn tick(app: &mut SlideApp, ctx: &egui::Context) {
    let rt = &mut app.updates_rt;
    if std::mem::take(&mut rt.check_requested) {
        rt.updater.check_now(ctx);
    } else if rt.enabled {
        rt.updater.check_at_start(ctx);
    }
    rt.updater.tick(ctx);
}

pub(crate) fn show(app: &mut SlideApp, ctx: &egui::Context) {
    // The restart helper is scheduled before the app closes, so no update starts over unsaved work.
    let clean = !app.session.documents().iter().any(|d| d.is_dirty());
    match app.updates_rt.updater.show_if(ctx, clean) {
        Action::OpenUrl(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
        // The app's close path asks about unsaved work; the restart helper relaunches once the process exits.
        Action::Close => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        Action::None => {}
    }
    let rt = &mut app.updates_rt;
    if rt.result_open {
        match CheckResultDialog::new(&crate::UPDATE_APP, VERSION).show(ctx, rt.updater.status()) {
            CheckResultResponse::Close => rt.result_open = false,
            CheckResultResponse::CheckAgain => rt.check_requested = true,
            CheckResultResponse::None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_offered_release_opens_the_prompt() {
        let mut app = SlideApp::new(deckcraft_engine::Session::new(), crate::Services::default());
        let ctx = egui::Context::default();
        assert!(!app.updates_rt.updater.prompt_open());
        app.updates_rt.updater.offer(craft_update::Release { tag: "v99.0.0".into(), ..Default::default() });
        ctx.run_ui(egui::RawInput::default(), |ui| show(&mut app, ui.ctx())).textures_delta.clear();
        assert!(app.updates_rt.updater.prompt_open());
    }
}
