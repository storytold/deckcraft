use super::presenter::NOTES_STEP;
use super::zoom::{plain, toggle_blank};
use super::{Color32, Presentation, Show, ShowAction};

/// Read the current viewport's show keys before its widgets can consume them. Both the
/// audience window and Presenter View use this path, including animation steps and end-of-show.
/// `presenter` is true for Presenter View, where G opens that window's own grid (#1130).
pub(super) fn keyboard(ctx: &egui::Context, show: &mut Show, doc: &Presentation, now: f64, presenter: bool) -> bool {
    let keys: Vec<(egui::Key, egui::Modifiers)> = ctx.input(|i| {
        i.events
            .iter()
            .filter_map(|e| if let egui::Event::Key { key, pressed: true, modifiers, .. } = e { Some((*key, *modifiers)) } else { None })
            .collect()
    });
    let (keep, consumed) = handle_keys(show, doc, now, keys, presenter);
    for (key, modifiers) in consumed {
        ctx.input_mut(|i| i.consume_key(modifiers, key));
        ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
    keep
}

pub(super) fn handle_keys(
    show: &mut Show,
    doc: &Presentation,
    now: f64,
    keys: Vec<(egui::Key, egui::Modifiers)>,
    presenter: bool,
) -> (bool, Vec<(egui::Key, egui::Modifiers)>) {
    let mut consumed = vec![];
    let mut keep = true;
    let mut actions = vec![];
    for (k, m) in keys {
        // Editing notes uses the keyboard as a text field. Esc and F2 lock it again
        // without ending the show; every other key is left for the editor.
        if presenter && show.notes_editing && !grid_open(show, presenter) {
            let leave = k == egui::Key::Escape || (k == egui::Key::F2 && plain(m));
            if leave {
                show.notes_editing = false;
                consumed.push((k, m));
            }
            continue;
        }
        let handled = if grid_open(show, presenter) && !matches!(k, egui::Key::Escape | egui::Key::Minus) {
            // See All Slides takes no other keys until a slide is chosen.
            if k == egui::Key::G && plain(m) {
                close_grid(show, presenter);
                true
            } else {
                false
            }
        } else {
            navigation_key(show, doc, k, m, presenter, &mut keep, &mut actions) || tool_key(show, k, m, now, presenter)
        };
        if !handled {
            continue;
        }
        // The same key must not also activate a focused presenter button or be handled again
        // by an embedded presenter viewport.
        consumed.push((k, m));
    }
    for action in actions {
        if !show.apply(action, doc, now) {
            keep = false;
        }
    }
    (keep, consumed)
}

/// Whether the grid that the presenter key G opens is showing in this viewport.
fn grid_open(show: &Show, presenter: bool) -> bool {
    if presenter { show.presenter_grid } else { show.grid }
}

/// Closes the grid that G opens in this viewport.
fn close_grid(show: &mut Show, presenter: bool) {
    if presenter && show.presenter_grid {
        show.presenter_grid = false;
    } else {
        show.grid = false;
    }
}

/// Keys that move through the show or end it. False when `k` isn't one of them.
fn navigation_key(
    show: &mut Show,
    doc: &Presentation,
    k: egui::Key,
    m: egui::Modifiers,
    presenter: bool,
    keep: &mut bool,
    actions: &mut Vec<ShowAction>,
) -> bool {
    use egui::Key::*;
    match k {
        Escape | Minus => {
            let cancelled = k == Escape && show.clear_goto();
            if !cancelled && !show.back_out(presenter) {
                *keep = false;
            }
        }
        ArrowRight | ArrowDown | Space | PageDown | N => {
            if show.ended {
                *keep = false;
            } else {
                actions.push(show.state.next(doc));
            }
        }
        Enter => {
            if !show.goto.is_empty() {
                if let Some(i) = show.take_goto()
                    && i < doc.slides.len()
                {
                    actions.push(show.state.goto(doc, i));
                }
            } else if show.ended {
                *keep = false;
            } else {
                actions.push(show.state.next(doc));
            }
        }
        Backspace if !show.goto.is_empty() => {
            show.goto.pop();
        }
        ArrowLeft | ArrowUp | Backspace | PageUp => {
            show.ended = false;
            actions.push(show.state.prev(doc));
        }
        P if m.command => show.set_pen(!show.pen),
        P => {
            show.ended = false;
            actions.push(show.state.prev(doc));
        }
        Home => {
            if let Some(i) = show.state.first_index(doc) {
                actions.push(show.state.goto(doc, i));
            }
        }
        End => {
            if let Some(i) = show.state.last_index(doc) {
                actions.push(show.state.goto(doc, i));
            }
        }
        _ => return false,
    }
    true
}

/// Pen, laser, blanking, zoom, the timer, note size and typed slide numbers. False when `k`
/// isn't one of them.
fn tool_key(show: &mut Show, k: egui::Key, m: egui::Modifiers, now: f64, presenter: bool) -> bool {
    use egui::Key::*;
    match k {
        L if m.command => show.set_laser(!show.laser),
        A if m.command => show.arrow(),
        B | Period => toggle_blank(show, Color32::BLACK),
        W | Comma => toggle_blank(show, Color32::WHITE),
        E => show.strokes.clear(),
        G if plain(m) => show.open_grid(presenter),
        Z if plain(m) => show.zoom_key(),
        T if plain(m) => show.timer.toggle(now),
        R if plain(m) => show.timer.reset(now),
        OpenBracket if plain(m) => show.nudge_notes(-NOTES_STEP),
        CloseBracket if plain(m) => show.nudge_notes(NOTES_STEP),
        F2 if presenter && plain(m) => {
            show.notes_editing = true;
            show.notes_focus = true;
        }
        Num0 | Num1 | Num2 | Num3 | Num4 | Num5 | Num6 | Num7 | Num8 | Num9 => {
            if let Some(d) = k.name().chars().next() {
                show.push_slide_number(d, now);
            }
        }
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn show_at(presenter: bool) -> (deckcraft_model::Presentation, Show) {
        let doc = deckcraft_model::Presentation::default();
        let show = Show::new(&doc, 0, false, presenter);
        (doc, show)
    }

    #[test]
    fn f2_edits_notes_and_escape_keeps_the_show() {
        let (doc, mut show) = show_at(true);
        let (keep, consumed) = handle_keys(&mut show, &doc, 0.0, vec![(egui::Key::F2, egui::Modifiers::NONE)], true);
        assert!(keep && show.notes_editing && show.notes_focus);
        assert_eq!(consumed.len(), 1);

        let (keep, consumed) =
            handle_keys(&mut show, &doc, 0.0, vec![(egui::Key::ArrowRight, egui::Modifiers::NONE), (egui::Key::Escape, egui::Modifiers::NONE)], true);
        assert!(keep);
        assert!(!show.notes_editing);
        assert!(consumed.iter().any(|(k, _)| *k == egui::Key::Escape));
        assert!(consumed.iter().all(|(k, _)| *k != egui::Key::ArrowRight));
    }

    #[test]
    fn halt_on_the_last_slide_keeps_arrows_and_a_clicker_from_ending_the_show() {
        let mut session = deckcraft_engine::Session::with_new();
        session.execute("slide.new", &serde_json::json!({})).unwrap();
        let doc = (*session.active().unwrap().doc).clone();
        let mut show = Show::new(&doc, 0, false, true);
        show.halt_last = true;
        let (keep, _) = handle_keys(&mut show, &doc, 0.0, vec![(egui::Key::ArrowRight, egui::Modifiers::NONE)], true);
        assert!(keep);
        assert_eq!(show.state.slide, 1, "a slide before the last still advances");
        for key in [egui::Key::ArrowRight, egui::Key::ArrowDown, egui::Key::Space, egui::Key::PageDown, egui::Key::N] {
            let (keep, _) = handle_keys(&mut show, &doc, 0.0, vec![(key, egui::Modifiers::NONE)], true);
            assert!(keep, "{key:?} must not leave the show");
            assert!(!show.ended, "{key:?} must not open the end screen");
            assert_eq!(show.state.slide, 1, "{key:?} stays on the last slide");
        }
        let (keep, _) = handle_keys(&mut show, &doc, 0.0, vec![(egui::Key::Escape, egui::Modifiers::NONE)], true);
        assert!(!keep, "Escape still ends the show");
    }

    #[test]
    fn f2_on_the_audience_window_does_nothing() {
        let (doc, mut show) = show_at(false);
        let (keep, consumed) = handle_keys(&mut show, &doc, 0.0, vec![(egui::Key::F2, egui::Modifiers::NONE)], false);
        assert!(keep);
        assert!(!show.notes_editing);
        assert!(consumed.is_empty());
    }
}
