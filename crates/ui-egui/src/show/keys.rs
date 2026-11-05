use super::presenter::NOTES_STEP;
use super::zoom::{plain, toggle_blank};
use super::{Color32, MicSlot, Presentation, Show, ShowAction, before_slide_change_with};

/// Read the current viewport's show keys before its widgets can consume them. Both the
/// audience window and Presenter View use this path, including animation steps and end-of-show.
/// `presenter` is true for Presenter View, where G opens that window's own grid (#1130). The
/// microphone goes with every slide change, so a key-driven one ends the take like a click does.
pub(super) fn keyboard(mic: &mut MicSlot, ctx: &egui::Context, show: &mut Show, doc: &Presentation, now: f64, presenter: bool) -> bool {
    let keys: Vec<(egui::Key, egui::Modifiers)> = ctx.input(|i| {
        i.events
            .iter()
            .filter_map(|e| if let egui::Event::Key { key, pressed: true, modifiers, .. } = e { Some((*key, *modifiers)) } else { None })
            .collect()
    });
    let (keep, consumed) = handle_keys(mic, show, doc, now, keys, presenter);
    for (key, modifiers) in consumed {
        ctx.input_mut(|i| i.consume_key(modifiers, key));
        ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
    keep
}

pub(super) fn handle_keys(
    mic: &mut MicSlot,
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
        // The Record view's top bar owns Escape: it closes the view and asks before ending a
        // take. Ending the show here would skip that confirmation. While the confirmation is
        // up, every show key waits for it.
        if show.record && (show.record_view.close_confirm || (k == egui::Key::Escape && plain(m)) || record_shell_owns(k, m)) {
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
        before_slide_change_with(mic, show, &action);
        if !show.apply(action, doc, now) {
            keep = false;
        }
    }
    (keep, consumed)
}

/// The command chords the Record view's top bar owns while the shell is up. Ctrl+Shift+P is the
/// take's Pause (the show's Ctrl+P is the pen), Ctrl+E is Export (the show's E erases the slide's
/// ink) and Ctrl+R, Ctrl+Z start a take or undo its Reset. The top bar reads the keys itself, so
/// the show must leave them alone.
fn record_shell_owns(k: egui::Key, m: egui::Modifiers) -> bool {
    match k {
        egui::Key::P => m.command && m.shift,
        egui::Key::E | egui::Key::R | egui::Key::Z => m.command,
        // Notes sizing: Ctrl+Minus and Ctrl+0 (and a plain Minus, which has no show meaning)
        // belong to the Record view. The show's own handlers would end it or open go-to-slide.
        egui::Key::Minus => true,
        egui::Key::Num0 => m.command,
        _ => false,
    }
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
        // E clears the ink only: the laser samples already recorded belong to the take.
        E => show.strokes.clear(),
        G if plain(m) => show.open_grid(presenter),
        Z if plain(m) => show.zoom_key(),
        T if plain(m) => show.timer.toggle(now),
        R if plain(m) => show.timer.reset(now),
        OpenBracket if plain(m) => show.nudge_notes(-NOTES_STEP),
        CloseBracket if plain(m) => show.nudge_notes(NOTES_STEP),
        Num0 | Num1 | Num2 | Num3 | Num4 | Num5 | Num6 | Num7 | Num8 | Num9 => {
            if let Some(d) = k.name().chars().next() {
                show.push_slide_number(d, now);
            }
        }
        _ => return false,
    }
    true
}
