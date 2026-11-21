/// Temp key holding whether the current viewport is inside an egui UI pass.
pub(crate) fn ime_in_ui_pass_key(viewport: egui::ViewportId) -> egui::Id {
    egui::Id::unique(("deckcraft-ime-in-ui-pass", viewport))
}

/// Stable key marking that the IME pass hooks are installed.
fn ime_pass_hooks_key() -> egui::Id {
    egui::Id::unique(("deckcraft-ime-pass-hooks", "installed"))
}

/// Install the per-viewport UI-pass mode hooks exactly once for this [`egui::Context`].
///
/// Call this before the context's first `run_ui` (desktop/web shells and the test harness).
/// The callbacks receive `&mut Ui`; they obtain the context through `ui.ctx()` and capture no
/// context or app reference, so they cannot form a reference cycle. The flag distinguishes a
/// full `run_ui` pass (where `App::logic` runs before the UI that owns the final output) from a
/// standalone `run_logic` callback. It is only a mode marker, not an IME publication receipt.
pub fn install_ime_pass_hooks(ctx: &egui::Context) {
    if ctx.data(|d| d.get_temp::<bool>(ime_pass_hooks_key()).is_some_and(|v| v)) {
        return;
    }
    ctx.on_begin_pass(
        "deckcraft-ime-ui-pass-begin",
        std::sync::Arc::new(|ui: &mut egui::Ui| {
            let ctx = ui.ctx();
            let key = ime_in_ui_pass_key(ctx.viewport_id());
            ctx.data_mut(|d| d.insert_temp(key, true));
        }),
    );
    ctx.on_end_pass(
        "deckcraft-ime-ui-pass-end",
        std::sync::Arc::new(|ui: &mut egui::Ui| {
            let ctx = ui.ctx();
            let key = ime_in_ui_pass_key(ctx.viewport_id());
            ctx.data_mut(|d| d.insert_temp(key, false));
        }),
    );
    ctx.data_mut(|d| d.insert_temp(ime_pass_hooks_key(), true));
}
