use super::{Rect, SlideApp, Ui, Xf, screen_rect, vec2};

/// Where an input method composes for the slide (#101), on screen: at the insertion point while
/// editing text, or over the one selected shape (typing on it starts editing). `None` when there is
/// nothing to type into.
pub fn ime_area(app: &SlideApp, xf: Xf) -> Option<Rect> {
    let st = app.session.active()?;
    if let Some(ts) = &st.selection.text {
        if ts.notes {
            return None;
        }
        if let Some((top, bot)) = deckcraft_engine::cmd::text::caret_line(st, ts) {
            return Some(Rect::from_two_pos(xf.to_screen(top), xf.to_screen(bot)).expand2(vec2(1.0, 0.0)));
        }
        let sh = st.shape(ts.shape)?;
        return Some(screen_rect(xf, &deckcraft_engine::cmd::xfrm_of(&st.doc, &st.selection, sh)));
    }
    if st.selection.shapes.len() != 1 {
        return None;
    }
    let sh = st.shape(*st.selection.shapes.first()?)?;
    if sh.is_line() {
        return None;
    }
    Some(screen_rect(xf, &deckcraft_engine::cmd::xfrm_of(&st.doc, &st.selection, sh)))
}

/// Turn the input method on for the slide while it takes typing, with its candidate window at
/// the insertion point (#101). Without this egui leaves it off, and a Korean keyboard types
/// separate jamo (ㅇ ㅏ ㄴ) instead of syllables. A focused text field sets its own.
///
/// This only records the screen rect for this pass. [`crate::SlideApp`] observes the committed
/// text, owner and cause once at the end of the pass, after every IME event and editor command
/// has settled.
pub(super) fn ime_output(app: &mut SlideApp, ui: &Ui, xf: Xf) {
    let ctx = ui.ctx();
    if ctx.memory(|m| m.focused().is_some()) || app.dialog.as_ref().is_some_and(|d| d.modal()) || app.palette.is_some() || app.backstage_open() {
        return;
    }
    let Some(r) = ime_area(app, xf) else { return };
    let to_global = ctx.layer_transform_to_global(ui.layer_id()).unwrap_or_default();
    app.ime_pending = Some(to_global * r);
}
