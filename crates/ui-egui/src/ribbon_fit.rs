//! Ribbon groups that fold to one-row drop-downs when the card is too narrow.
//!
//! PowerPoint keeps every group on the card and folds groups from the right as the window
//! narrows: a folded group keeps its name in the label band and becomes one drop-down button that
//! opens the group's whole content. DeckCraft has two levels, [`FULL`] (the group as designed) and
//! [`COMPACT`] (one icon button that opens the content in a popup).
//!
//! Each frame [`begin`] plans the levels from the widths the groups measured on earlier frames
//! ([`record`]). Groups are known by their position on the tab and their title, so duplicate names
//! keep their own widths. The first frame only measures; the plan settles once a group has been
//! drawn at each level. A card that is still too narrow after every group has folded scrolls, as
//! before, and its edge chevrons keep the rest reachable.

use std::collections::HashMap;

use egui::{Context, Id};

/// The group as designed.
pub const FULL: u8 = 0;
/// One row: the group's icon over its name and a drop-down chevron; clicking opens the content.
pub const COMPACT: u8 = 1;

/// A compact group before it has been measured: its button and divider.
const COMPACT_ESTIMATE: f32 = 64.0;

/// One group of a tab in drawing order: its title and measured widths at each level.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GroupWidths {
    pub title: String,
    pub widths: [Option<f32>; 2],
}

/// Each group's level (by position) so that the row fits `avail`: the row folds from the right
/// until it fits, so a section only stays as designed while everything after it still has room.
/// The written order is left to right, so the groups the reader reaches first keep their layout.
pub fn plan(groups: &[GroupWidths], avail: f32) -> Vec<u8> {
    let full = |g: &GroupWidths| g.widths[FULL as usize].unwrap_or(0.0);
    let width = |g: &GroupWidths, level: u8| match level {
        FULL => full(g),
        _ => g.widths[COMPACT as usize].unwrap_or(full(g).min(COMPACT_ESTIMATE)),
    };
    let mut levels = vec![FULL; groups.len()];
    let mut total: f32 = groups.iter().map(full).sum();
    for i in (0..groups.len()).rev() {
        if total <= avail {
            break;
        }
        let (Some(g), Some(level)) = (groups.get(i), levels.get_mut(i)) else { continue };
        total += width(g, COMPACT) - width(g, *level);
        *level = COMPACT;
    }
    levels
}

/// What this frame's ribbon is drawing.
#[derive(Clone, Default)]
struct Frame {
    tab: String,
    /// The plan, by position, and the titles it was made for.
    titles: Vec<String>,
    levels: Vec<u8>,
    drawn: Vec<String>,
}

/// Widths measured so far: (tab, position, group, level) → width, and each tab's group order.
#[derive(Clone, Default)]
struct Measured {
    /// The scale the widths were measured at.
    signature: String,
    widths: HashMap<(String, usize, String, u8), f32>,
    order: HashMap<String, Vec<String>>,
    /// Each tab's settled plan and the width it was made for. Re-planning every frame would fight
    /// a gallery whose tile count depends on the space the plan leaves (`spare_room`): folding a
    /// group lets the gallery grow, which can fold the group again, and so on. The plan is made
    /// once from the widths measured before anything folded, and remade when the row resizes or
    /// its groups change.
    plans: HashMap<(String, u32), Vec<u8>>,
}

fn frame_id() -> Id {
    Id::unique("deckcraft_ribbon_fit_frame")
}
fn measured_id() -> Id {
    Id::unique("deckcraft_ribbon_fit_measured")
}
fn active_id() -> Id {
    Id::unique("deckcraft_ribbon_fit_active")
}

/// The plan's key for an available width: a resize of a fraction of a point reuses the plan.
fn width_key(avail: f32) -> u32 {
    avail.round().max(0.0) as u32
}

/// What the measured widths depend on besides the groups themselves.
fn signature(ctx: &Context) -> String {
    format!("{}", ctx.pixels_per_point())
}

/// Start drawing `tab` in a row `avail` points wide: plan the groups' levels.
pub fn begin(ctx: &Context, tab: &str, avail: f32) {
    let sig = signature(ctx);
    let (titles, groups, settled) = ctx.data_mut(|d| {
        let m = d.get_temp_mut_or_default::<Measured>(measured_id());
        if m.signature != sig {
            // Another scale: labels have new widths, so measure again.
            *m = Measured { signature: sig, ..Default::default() };
        }
        let titles = m.order.get(tab).cloned().unwrap_or_default();
        let groups: Vec<GroupWidths> = titles
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let key = |level| m.widths.get(&(tab.to_string(), i, t.clone(), level)).copied();
                GroupWidths { title: t.clone(), widths: [key(FULL), key(COMPACT)] }
            })
            .collect();
        let settled = m.plans.get(&(tab.to_string(), width_key(avail))).cloned();
        (titles, groups, settled)
    });
    let levels = match settled {
        Some(levels) if levels.len() == groups.len() => levels,
        _ if groups.iter().all(|g| g.widths[FULL as usize].is_some()) => {
            let levels = plan(&groups, avail);
            if !levels.is_empty() {
                ctx.data_mut(|d| {
                    d.get_temp_mut_or_default::<Measured>(measured_id()).plans.insert((tab.to_string(), width_key(avail)), levels.clone());
                });
            }
            levels
        }
        _ => Vec::new(),
    };
    ctx.data_mut(|d| {
        d.insert_temp(frame_id(), Frame { tab: tab.to_string(), titles, levels, drawn: Vec::new() });
        d.insert_temp(active_id(), true);
    });
}

/// How many groups of this ribbon row are drawn so far: the next group's place, which tells apart
/// groups that share a title.
pub fn position(ctx: &Context) -> usize {
    ctx.data(|d| d.get_temp::<Frame>(frame_id()).map(|f| f.drawn.len())).unwrap_or(0)
}

/// The level the next group, `title`, draws at this frame.
pub fn level(ctx: &Context, title: &str) -> u8 {
    ctx.data(|d| {
        let f = d.get_temp::<Frame>(frame_id())?;
        let i = f.drawn.len();
        // Planned for another group here (one was added or removed): draw it as designed.
        if f.titles.get(i).map(String::as_str) != Some(title) {
            return None;
        }
        f.levels.get(i).copied()
    })
    .unwrap_or(FULL)
}

/// Group `title` drew at `level`, `width` points wide (its content and divider).
pub fn record(ctx: &Context, title: &str, level: u8, width: f32, compact_estimate: f32) {
    ctx.data_mut(|d| {
        // Only groups on the ribbon row between `begin` and `end` are planned.
        if !d.get_temp::<bool>(active_id()).unwrap_or(false) {
            return;
        }
        let f = d.get_temp_mut_or_default::<Frame>(frame_id());
        let i = f.drawn.len();
        f.drawn.push(title.to_string());
        let tab = f.tab.clone();
        if !(width.is_finite() && width > 0.0) {
            return;
        }
        let m = d.get_temp_mut_or_default::<Measured>(measured_id());
        m.widths.insert((tab.clone(), i, title.to_string(), level), width);
        // Drawn as designed: until it is drawn compact, its compact width is the planned estimate
        // from its name, so the next plan can choose it before it has ever been folded.
        if level == FULL {
            m.widths.entry((tab, i, title.to_string(), COMPACT)).or_insert(compact_estimate.min(width).max(1.0));
        }
    });
}

/// The row is drawn: remember its group order for the next frame's plan.
pub fn end(ctx: &Context) {
    let changed = ctx.data_mut(|d| {
        d.insert_temp(active_id(), false);
        let (drawn, tab) = {
            let f = d.get_temp_mut_or_default::<Frame>(frame_id());
            (std::mem::take(&mut f.drawn), f.tab.clone())
        };
        let m = d.get_temp_mut_or_default::<Measured>(measured_id());
        let changed = m.order.get(&tab) != Some(&drawn);
        if changed {
            m.plans.retain(|(planned, _), _| planned != &tab);
            m.order.insert(tab, drawn);
        }
        changed
    });
    if changed {
        // A new order (another tab, a group appeared): plan again with it next frame.
        ctx.request_repaint();
    }
}

/// The level this frame planned for the first group called `title` (tests).
#[cfg(test)]
pub(crate) fn planned(ctx: &Context, title: &str) -> Option<u8> {
    ctx.data(|d| {
        let f = d.get_temp::<Frame>(frame_id())?;
        let i = f.titles.iter().position(|t| t == title)?;
        Some(f.levels.get(i).copied().unwrap_or(FULL))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(title: &str, full: f32, compact: Option<f32>) -> GroupWidths {
        GroupWidths { title: title.into(), widths: [Some(full), compact] }
    }

    #[test]
    fn everything_full_when_it_fits() {
        let gs = [g("A", 100.0, Some(60.0)), g("B", 100.0, Some(60.0))];
        assert_eq!(plan(&gs, 200.0), vec![FULL, FULL]);
    }

    #[test]
    fn rightmost_groups_fold_first_and_keep_the_left_in_full_layout() {
        let gs = [g("A", 120.0, Some(60.0)), g("B", 200.0, Some(80.0)), g("C", 300.0, Some(70.0)), g("D", 120.0, Some(60.0))];
        assert_eq!(plan(&gs, 800.0), vec![FULL, FULL, FULL, FULL]);
        // 740 → 680: folding D is enough.
        assert_eq!(plan(&gs, 700.0), vec![FULL, FULL, FULL, COMPACT]);
        // Still too wide: C folds too, so the left groups keep their layout.
        assert_eq!(plan(&gs, 670.0), vec![FULL, FULL, COMPACT, COMPACT]);
        // 450: B folds, and A stays as designed while it fits.
        assert_eq!(plan(&gs, 400.0), vec![FULL, COMPACT, COMPACT, COMPACT]);
        // Even every compact width is too wide: all fold; the row scrolls.
        assert_eq!(plan(&gs, 200.0), vec![COMPACT, COMPACT, COMPACT, COMPACT]);
    }

    #[test]
    fn unmeasured_widths_do_not_panic_and_hostile_numbers_stay_finite() {
        let gs = [g("A", 200.0, None), GroupWidths { title: "B".into(), widths: [None, None] }];
        assert_eq!(plan(&gs, 100.0), vec![COMPACT, COMPACT]);
        let odd = [GroupWidths { title: "X".into(), widths: [Some(f32::NAN), None] }];
        let _ = plan(&odd, 100.0);
        let _ = plan(&odd, f32::NAN);
    }
}

#[cfg(test)]
mod ui_tests {
    use super::*;
    use crate::{Services, SlideApp};
    use egui::accesskit::Role;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    fn home(width: f32, height: f32) -> Harness<'static, SlideApp> {
        let mut app = SlideApp::new(deckcraft_engine::Session::with_new(), Services::default());
        app.ui.tab = "Home".into();
        crate::widgets::GROUP_LABELS.with(|g| g.borrow_mut().clear());
        let mut h = Harness::builder().with_size(egui::vec2(width, height)).with_pixels_per_point(1.25).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app,
        );
        // Measure the full layouts, plan, measure the folded ones, plan again.
        h.run_steps(10);
        h
    }

    fn levels(h: &Harness<'static, SlideApp>, groups: &[&str]) -> Vec<u8> {
        groups.iter().map(|g| planned(&h.ctx, g).unwrap_or(FULL)).collect()
    }

    const HOME: [&str; 8] = ["Clipboard", "Slides", "Font", "Paragraph", "Images", "Drawing", "Editing", "Designer"];

    /// 819x614 pt at 1.25 (a 1024x768-client window) has room for Clipboard's three control rows
    /// and one drop-down per other section, as the measured PowerPoint reference does. The
    /// name band is drawn for both layouts, and a folded section opens its commands.
    #[test]
    fn home_sections_fold_to_one_row_drop_downs_at_1024() {
        let mut h = home(819.0, 614.0);
        assert_eq!(levels(&h, &HOME), vec![FULL, COMPACT, COMPACT, COMPACT, COMPACT, COMPACT, COMPACT, COMPACT]);
        for label in ["Cut", "Copy", "Format"] {
            assert!(h.query_by_label(label).is_some(), "{label} stays in Clipboard's three rows");
        }
        // Folded sections keep their name on a button; their content is in its popup.
        for group in &HOME[1..] {
            assert!(h.query_by_role_and_label(Role::Button, group).is_some(), "{group} is a named drop-down");
        }
        assert!(h.query_by_label("New Slide").is_none(), "Slides' commands are folded away");
        h.get_by_role_and_label(Role::Button, "Slides").click();
        h.run_steps(3);
        assert!(h.query_by_label("New Slide").is_some(), "the drop-down holds the group as designed");

        // Every group's name sits in the card's label band, folded or not.
        let labels: Vec<crate::widgets::DrawnGroupLabel> = crate::widgets::GROUP_LABELS.with(|g| g.borrow_mut().drain(..).collect());
        assert!(labels.iter().any(|l| l.title == "Clipboard"));
        assert!(labels.iter().any(|l| l.title == "Slides"));
        for l in labels {
            assert!(
                l.text.min.y >= l.band.min.y - 0.5 && l.text.max.y <= l.band.max.y + 0.5,
                "{} name {:?} stays in its band {:?}",
                l.title,
                l.text,
                l.band
            );
        }
    }

    /// The control socket may deliver one click's press and release on different frames; the
    /// release must not close the drop-down the press opened.
    #[test]
    fn a_split_press_and_release_keeps_the_drop_down_open() {
        let mut h = home(819.0, 614.0);
        let p = h.get_by_role_and_label(Role::Button, "Slides").rect().center();
        h.input_mut().events.push(egui::Event::PointerMoved(p));
        h.run_steps(1);
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        });
        h.run_steps(1);
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
        h.run_steps(3);
        assert!(h.query_by_label("New Slide").is_some(), "the drop-down stays open after the release");
    }

    /// A card too narrow for even the folded row keeps the horizontal scroll and its edge chevrons,
    /// so every section stays reachable.
    #[test]
    fn a_card_too_narrow_for_the_folded_row_still_scrolls() {
        let mut h = home(400.0, 614.0);
        assert!(levels(&h, &HOME).iter().all(|l| *l == COMPACT), "every Home section folds");
        for _ in 0..8 {
            let Some(cue) = h.query_by_label("Scroll ribbon right") else { break };
            cue.click();
            h.run_steps(1);
        }
        assert!(h.query_by_label("Scroll ribbon left").is_some(), "the row scrolled: the left chevron appeared");
        assert!(h.query_by_role_and_label(Role::Button, "Designer").is_some(), "the last section is reachable");
    }

    /// 1228x768 pt at 1.25 (a 1536 px window) has expanded layouts: Clipboard, Slides and Font no
    /// longer fold, while the later sections still do; the name band stays for both.
    #[test]
    fn home_sections_expand_as_the_card_grows() {
        let h = home(1228.0, 768.0);
        assert_eq!(levels(&h, &HOME), vec![FULL, FULL, FULL, COMPACT, COMPACT, COMPACT, COMPACT, COMPACT]);
        let labels: Vec<crate::widgets::DrawnGroupLabel> = crate::widgets::GROUP_LABELS.with(|g| g.borrow_mut().drain(..).collect());
        for l in &labels {
            assert!(
                l.text.min.y >= l.band.min.y - 0.5 && l.text.max.y <= l.band.max.y + 0.5,
                "{} name {:?} stays in its band {:?}",
                l.title,
                l.text,
                l.band
            );
        }
    }
}
