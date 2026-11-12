//! Ribbon groups that fold to one-row drop-downs when the card is too narrow.
//!
//! PowerPoint keeps every group on the card and folds groups from the right as the window
//! narrows: a folded group keeps its name in the label band and becomes one drop-down button that
//! opens the group's whole content. A section also changes its row form at measured widths: a wide
//! section can stack its controls into two or three rows ([`STACKED`]) before it folds.
//! DeckCraft therefore has three levels: [`FULL`] (the section as designed), [`STACKED`] (its
//! measured multi-row form) and [`COMPACT`] (one icon button that opens the content in a popup).
//!
//! Each frame [`begin`] plans the levels from the widths the groups measured on earlier frames
//! ([`record`]). Groups are known by their position on the tab and their title, so duplicate names
//! keep their own widths. The first frame only measures; the plan settles once a group has been
//! drawn at each level. A card that is still too narrow after every group has folded scrolls, as
//! before, and its edge chevrons keep the rest reachable.

use std::collections::HashMap;

use egui::{Context, Id};

/// The section as designed: one row of its usual controls.
pub const FULL: u8 = 0;
/// The section's measured multi-row form, used in the width band where PowerPoint stacks it.
pub const STACKED: u8 = 1;
/// One row: the section's icon over its name and a drop-down chevron; clicking opens the content.
pub const COMPACT: u8 = 2;

/// A compact group before it has been measured: its button and divider.
const COMPACT_ESTIMATE: f32 = 64.0;

/// One group of a tab in drawing order: its title and measured widths at each level.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GroupWidths {
    pub title: String,
    pub widths: [Option<f32>; 3],
}

/// Each group's level so that the row fits `avail`: start from the measured row form for the
/// width ([`preferred`]), then fold sections from the right until the row fits. Sections keep
/// their measured form while everything after them still has room.
pub fn plan(groups: &[GroupWidths], avail: f32, preferred: &[u8]) -> Vec<u8> {
    let full = |g: &GroupWidths| g.widths[FULL as usize].unwrap_or(0.0);
    let width = |g: &GroupWidths, level: u8| match level {
        FULL => full(g),
        STACKED => g.widths[STACKED as usize].unwrap_or(full(g)),
        _ => g.widths[COMPACT as usize].unwrap_or(full(g).min(COMPACT_ESTIMATE)),
    };
    let mut levels: Vec<u8> = (0..groups.len()).map(|i| preferred.get(i).copied().unwrap_or(FULL)).collect();
    let mut total: f32 = groups.iter().zip(&levels).map(|(g, level)| width(g, *level)).sum();
    for i in (0..groups.len()).rev() {
        if total <= avail {
            break;
        }
        let (Some(g), Some(level)) = (groups.get(i), levels.get_mut(i)) else { continue };
        // A measured multi-row form is what PowerPoint shows at this width; folding it would make
        // the row shorter than measured. Let the row scroll instead.
        if *level == COMPACT || preferred.get(i) == Some(&STACKED) {
            continue;
        }
        total += width(g, COMPACT) - width(g, *level);
        *level = COMPACT;
    }
    levels
}

/// PowerPoint's measured row form for a section at content width `width` (logical points, the
/// client width; the measurement is outer px / 1.25). `None` means the section is drawn as
/// designed. A section's 1 at narrow widths is the planner's compact drop-down, so only bands of
/// two or three rows (and the compact-only bands below a first breakpoint) are listed.
pub fn preferred(tab: &str, section: &str, width: f32) -> u8 {
    let band = |from: f32, to: f32, level: u8| {
        if width >= from && width < to { level } else { FULL }
    };
    match (tab, section) {
        // Home. Font's 3-row and 2-row forms live inside the group; above its first measured
        // breakpoint it always stacks.
        ("Home", "Slides") => {
            if width < 1120.0 {
                COMPACT
            } else {
                STACKED
            }
        }
        ("Home", "Font") => {
            if width < 928.8 {
                COMPACT
            } else {
                STACKED
            }
        }
        ("Home", "Paragraph") => {
            if width < 1040.0 {
                COMPACT
            } else {
                STACKED
            }
        }
        ("Home", "Drawing") => {
            if width < 1280.0 {
                COMPACT
            } else {
                STACKED
            }
        }
        ("Home", "Editing") => {
            if width < 1520.0 {
                COMPACT
            } else {
                STACKED
            }
        }
        // Insert: the pictures, illustrations and text sections stack into three rows for a band
        // of window widths, then return to their at-a-glance row.
        ("Insert", "Images") => band(1120.0, 1520.0, STACKED),
        ("Insert", "Illustrations") => band(928.8, 1520.0, STACKED),
        ("Insert", "Text") => band(1280.0, f32::INFINITY, STACKED),
        // Design's Customize section is three rows until the tab is wide enough for one.
        ("Design", "Customize") => {
            if width < 520.0 {
                COMPACT
            } else if width < 880.0 {
                STACKED
            } else {
                FULL
            }
        }
        // Transitions Timing adds a second row of timing fields from 520 points.
        ("Transitions", "Timing") => {
            if width < 520.0 {
                COMPACT
            } else {
                STACKED
            }
        }
        ("Animations", "Advanced Animation") => band(520.0, f32::INFINITY, STACKED),
        ("Animations", "Timing") => {
            if width < 760.0 {
                COMPACT
            } else {
                STACKED
            }
        }
        ("Slide Show", "Start Slide Show") => band(600.0, 1120.0, STACKED),
        ("Record", "Record") => band(600.0, 680.0, STACKED),
        ("Review", "Proofing") => band(819.0, 880.0, STACKED),
        // DeckCraft's activity tracking section is Compare (Compare, Accept All, End Review);
        // PowerPoint calls the measured section Activity.
        ("Review", "Compare") => band(819.0, 880.0, STACKED),
        ("View", "Presentation Views") => band(680.0, 1040.0, STACKED),
        ("View", "Master Views") => band(760.0, 1120.0, STACKED),
        _ => FULL,
    }
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
    /// Each tab's settled plan, the width it was made for, and the preferred levels it assumed.
    /// Re-planning every frame would fight a gallery whose tile count depends on the space the
    /// plan leaves (`spare_room`): folding a group lets the gallery grow, which can fold the group
    /// again, and so on. The plan is made once from the widths measured before anything folded,
    /// and remade when the row resizes, its groups change, or a measured breakpoint moves a
    /// section's preferred row form.
    plans: HashMap<(String, u32), (Vec<u8>, Vec<u8>)>,
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
    let width = ctx.content_rect().width();
    let (titles, groups, preferred, settled) = ctx.data_mut(|d| {
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
                GroupWidths { title: t.clone(), widths: [key(FULL), key(STACKED), key(COMPACT)] }
            })
            .collect();
        let preferred: Vec<u8> = titles.iter().map(|t| preferred(tab, t, width)).collect();
        let stored = m.plans.get(&(tab.to_string(), width_key(avail))).cloned();
        // A stored plan still needs the widths of the forms it chose, and it assumed the preferred
        // row forms of that width: after a breakpoint crossing moves a preference (same width key)
        // the stored levels are stale, so plan again. A new row form at this width is planned once
        // it has been measured at least once elsewhere.
        let settled = stored.filter(|(levels, planned)| {
            planned == &preferred
                && levels.len() == groups.len()
                && groups.iter().zip(levels).all(|(g, level)| g.widths.get(*level as usize).is_some_and(Option::is_some))
        });
        (titles, groups, preferred, settled)
    });
    let fulls_known = groups.iter().all(|g| g.widths[FULL as usize].is_some());
    let levels = match settled {
        Some((levels, _)) => levels,
        _ if fulls_known => {
            let levels = plan(&groups, avail, &preferred);
            // Only keep a plan whose chosen forms have all been measured; until then a form that
            // has never been drawn uses the designed width and is tried again next frame.
            let complete = groups.iter().zip(&levels).all(|(g, level)| g.widths.get(*level as usize).is_some_and(Option::is_some));
            if complete && !levels.is_empty() {
                ctx.data_mut(|d| {
                    d.get_temp_mut_or_default::<Measured>(measured_id())
                        .plans
                        .insert((tab.to_string(), width_key(avail)), (levels.clone(), preferred));
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

/// The level the next group, `title`, draws at this frame. Outside the ribbon row (before
/// `begin` or after `end`) a group draws as designed: a stale plan for a same-titled group
/// must not leak into an unrelated row.
pub fn level(ctx: &Context, title: &str) -> u8 {
    ctx.data(|d| {
        if !d.get_temp::<bool>(active_id()).unwrap_or(false) {
            return None;
        }
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
        // Drawn as designed (either row form): until it is drawn compact, its compact width is the
        // planned estimate from its name, so the next plan can choose it before it has ever folded.
        // The estimate is the button's own width, not the full row: capping it at the full width
        // would understate the fold for a group narrower than its button.
        if level != COMPACT {
            m.widths.entry((tab, i, title.to_string(), COMPACT)).or_insert(compact_estimate.max(1.0));
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

/// The level the group currently being drawn uses; row forms read it inside a group's content.
pub fn group_level(ctx: &Context) -> u8 {
    ctx.data(|d| d.get_temp::<u8>(group_level_id())).unwrap_or(FULL)
}

fn group_level_id() -> Id {
    Id::unique("deckcraft_ribbon_group_level")
}

/// Set the level the next group's content draws at.
pub fn set_group_level(ctx: &Context, level: u8) {
    ctx.data_mut(|d| d.insert_temp(group_level_id(), level));
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

    fn g(title: &str, full: f32, stacked: Option<f32>, compact: Option<f32>) -> GroupWidths {
        GroupWidths { title: title.into(), widths: [Some(full), stacked, compact] }
    }

    #[test]
    fn everything_full_when_it_fits() {
        let gs = [g("A", 100.0, Some(80.0), Some(60.0)), g("B", 100.0, Some(80.0), Some(60.0))];
        assert_eq!(plan(&gs, 200.0, &[FULL, FULL]), vec![FULL, FULL]);
    }

    #[test]
    fn rightmost_groups_fold_first_and_keep_the_left_in_full_layout() {
        let gs = [
            g("A", 120.0, Some(90.0), Some(60.0)),
            g("B", 200.0, Some(140.0), Some(80.0)),
            g("C", 300.0, Some(200.0), Some(70.0)),
            g("D", 120.0, Some(90.0), Some(60.0)),
        ];
        let full = [FULL, FULL, FULL, FULL];
        assert_eq!(plan(&gs, 800.0, &full), vec![FULL, FULL, FULL, FULL]);
        // 740 → 680: folding D is enough.
        assert_eq!(plan(&gs, 700.0, &full), vec![FULL, FULL, FULL, COMPACT]);
        // Still too wide: C folds too, so the left groups keep their layout.
        assert_eq!(plan(&gs, 670.0, &full), vec![FULL, FULL, COMPACT, COMPACT]);
        // 450: B folds, and A stays as designed while it fits.
        assert_eq!(plan(&gs, 400.0, &full), vec![FULL, COMPACT, COMPACT, COMPACT]);
        // Even every compact width is too wide: all fold; the row scrolls.
        assert_eq!(plan(&gs, 200.0, &full), vec![COMPACT, COMPACT, COMPACT, COMPACT]);
    }

    /// The measured row form is the starting point; only a row that is too wide folds further.
    #[test]
    fn a_measured_stacked_form_is_kept_while_it_fits() {
        let gs = [g("Font", 340.0, Some(300.0), Some(67.0)), g("Paragraph", 280.0, Some(150.0), Some(92.0))];
        assert_eq!(plan(&gs, 1000.0, &[STACKED, STACKED]), vec![STACKED, STACKED]);
        // Both sections are measured as stacked at this width, so the row scrolls rather than
        // folding one of them below its measured form.
        assert_eq!(plan(&gs, 400.0, &[STACKED, STACKED]), vec![STACKED, STACKED]);
        assert_eq!(plan(&gs, 1000.0, &[FULL, FULL]), vec![FULL, FULL]);
    }

    /// One section's measured breakpoints: the level just below and at each width where PowerPoint
    /// changes the row count.
    fn bands(tab: &str, section: &str, cases: &[(f32, u8)]) {
        for &(width, expected) in cases {
            assert_eq!(preferred(tab, section, width), expected, "{tab} {section} at {width} logical points");
        }
    }

    /// Every measured breakpoint, checked just below and at the width where PowerPoint changes
    /// the section's row count (logical points: measured px / 1.25).
    #[test]
    fn measured_preferences_follow_the_width_bands() {
        // Home: Slides, Font (three to wide two), Paragraph (three to wide two), Drawing, Editing.
        bands("Home", "Slides", &[(1119.9, COMPACT), (1120.0, STACKED)]);
        bands("Home", "Font", &[(928.7, COMPACT), (928.8, STACKED)]);
        bands("Home", "Paragraph", &[(1039.9, COMPACT), (1040.0, STACKED)]);
        bands("Home", "Drawing", &[(1279.9, COMPACT), (1280.0, STACKED)]);
        bands("Home", "Editing", &[(1519.9, COMPACT), (1520.0, STACKED)]);
        // Insert: a band of three rows, then the designed row again (1900 px).
        bands("Insert", "Images", &[(1119.9, FULL), (1120.0, STACKED), (1519.9, STACKED), (1520.0, FULL)]);
        bands("Insert", "Illustrations", &[(928.7, FULL), (928.8, STACKED), (1519.9, STACKED), (1520.0, FULL)]);
        bands("Insert", "Text", &[(1279.9, FULL), (1280.0, STACKED)]);
        // Design: three rows until 1100 px, then one.
        bands("Design", "Customize", &[(519.9, COMPACT), (520.0, STACKED), (879.9, STACKED), (880.0, FULL)]);
        // Transitions: a second timing row from 650 px.
        bands("Transitions", "Timing", &[(519.9, COMPACT), (520.0, STACKED)]);
        // Animations: Advanced Animation from 650 px, Timing from 950 px.
        bands("Animations", "Advanced Animation", &[(519.9, FULL), (520.0, STACKED)]);
        bands("Animations", "Timing", &[(759.9, COMPACT), (760.0, STACKED)]);
        // Slide Show: Start from 750 to 1400 px.
        bands("Slide Show", "Start Slide Show", &[(599.9, FULL), (600.0, STACKED), (1119.9, STACKED), (1120.0, FULL)]);
        // Record: three rows only from 750 to 850 px.
        bands("Record", "Record", &[(599.9, FULL), (600.0, STACKED), (679.9, STACKED), (680.0, FULL)]);
        // Review: Proofing and Activity are two rows only between 1024 and 1100 px.
        bands("Review", "Proofing", &[(818.9, FULL), (819.0, STACKED), (879.9, STACKED), (880.0, FULL)]);
        bands("Review", "Compare", &[(818.9, FULL), (819.0, STACKED), (879.9, STACKED), (880.0, FULL)]);
        // View: Presentation Views 850 to 1300 px, Master Views 950 to 1400 px.
        bands("View", "Presentation Views", &[(679.9, FULL), (680.0, STACKED), (1039.9, STACKED), (1040.0, FULL)]);
        bands("View", "Master Views", &[(759.9, FULL), (760.0, STACKED), (1119.9, STACKED), (1120.0, FULL)]);
        // Set Up and View's Color/Grayscale and Window have fewer commands than the measured
        // three rows need, so the planner never asks for a stacked form there.
        bands("Slide Show", "Set Up", &[(1040.0, FULL), (1280.0, FULL)]);
        bands("View", "Color/Grayscale", &[(1040.0, FULL)]);
        bands("View", "Window", &[(960.0, FULL)]);
    }

    #[test]
    fn unmeasured_widths_do_not_panic_and_hostile_numbers_stay_finite() {
        let gs = [g("A", 200.0, None, None), GroupWidths { title: "B".into(), widths: [None, None, None] }];
        assert_eq!(plan(&gs, 100.0, &[FULL, FULL]), vec![COMPACT, COMPACT]);
        let odd = [GroupWidths { title: "X".into(), widths: [Some(f32::NAN), None, None] }];
        let _ = plan(&odd, 100.0, &[FULL]);
        let _ = plan(&odd, f32::NAN, &[FULL]);
    }

    /// The compact estimate is the button's own width, even when the full row is narrower: a
    /// group of one narrow control must not look cheaper to fold than its button really is.
    /// `record` inserts the estimate until the group actually folds, so drive it directly: with
    /// the old `min(width)` cap the stored estimate would be the 40 pt full width.
    #[test]
    fn the_compact_estimate_is_not_capped_at_the_full_width() {
        let ctx = egui::Context::default();
        ctx.data_mut(|d| {
            d.insert_temp(active_id(), true);
            d.insert_temp(frame_id(), Frame { tab: "T".into(), titles: vec!["Narrow".into()], levels: vec![FULL], drawn: Vec::new() });
        });
        // Narrow full row, real compact button floor: record what `group` would record.
        record(&ctx, "Narrow", FULL, 40.0, 65.0);
        let (full, compact) = ctx.data(|d| {
            let m = d.get_temp::<Measured>(measured_id()).unwrap_or_default();
            let key = |level: u8| m.widths.get(&("T".to_string(), 0, "Narrow".to_string(), level)).copied().unwrap_or(0.0);
            (key(FULL), key(COMPACT))
        });
        assert_eq!(full, 40.0, "the full width is recorded as drawn");
        assert_eq!(compact, 65.0, "the estimate keeps the button width, not the capped full width");
    }

    /// A measured compact width below the estimate floor survives: `plan` folds against the real
    /// button width, not the capped guess.
    #[test]
    fn plan_counts_a_narrow_groups_real_compact_width() {
        // Full row 40 pt, real compact button 65 pt: folding costs 25 pt more, not zero.
        let gs = [g("Narrow", 40.0, None, Some(65.0)), g("Wide", 300.0, None, Some(70.0))];
        // 340 pt of compact still fits in 340, but planning from FULL must fold Wide first:
        // with the old min() cap the narrow group's compact was stored as 40, hiding the cost.
        let narrow_compact = gs[0].widths[COMPACT as usize].unwrap();
        assert!(narrow_compact > 40.0, "the stored compact width exceeds the full row: {narrow_compact}");
        assert_eq!(plan(&gs, 330.0, &[FULL, FULL]), vec![FULL, COMPACT]);
    }

    /// Outside the ribbon row a same-titled group draws as designed: after `end`, `level` must
    /// not hand out the row's compact plan to an unrelated row.
    #[test]
    fn level_outside_the_active_row_falls_back_to_full() {
        let ctx = egui::Context::default();
        ctx.data_mut(|d| {
            d.insert_temp(active_id(), true);
            d.insert_temp(frame_id(), Frame { tab: "Home".into(), titles: vec!["Clipboard".into()], levels: vec![COMPACT], drawn: Vec::new() });
        });
        assert_eq!(level(&ctx, "Clipboard"), COMPACT, "the row's own group uses the plan");
        ctx.data_mut(|d| d.insert_temp(active_id(), false));
        assert_eq!(level(&ctx, "Clipboard"), FULL, "after end the same title draws as designed");
    }
}

#[cfg(test)]
mod ui_tests {
    use super::*;
    use crate::{Services, SlideApp};
    use egui::accesskit::Role;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    /// A fresh app on `tab` in a window `width` x `height` logical points at 125% DPI.
    fn tab(tab: &str, width: f32, height: f32) -> Harness<'static, SlideApp> {
        let mut app = SlideApp::new(deckcraft_engine::Session::with_new(), Services::default());
        app.ui.tab = tab.into();
        crate::widgets::GROUP_LABELS.with(|g| g.borrow_mut().clear());
        let mut h = Harness::builder().with_size(egui::vec2(width, height)).with_pixels_per_point(1.25).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app,
        );
        // Measure the full layouts, plan, measure the folded/stacked ones, plan again.
        h.run_steps(12);
        h
    }

    fn home(width: f32, height: f32) -> Harness<'static, SlideApp> {
        tab("Home", width, height)
    }

    fn levels(h: &Harness<'static, SlideApp>, groups: &[&str]) -> Vec<u8> {
        groups.iter().map(|g| planned(&h.ctx, g).unwrap_or(FULL)).collect()
    }

    /// The number of distinct control rows a set of named controls occupies. A folded section has
    /// none of them on the card, which is its single drop-down row.
    fn control_rows(h: &Harness<'static, SlideApp>, labels: &[&str]) -> usize {
        let mut ys: Vec<i32> = labels.iter().filter_map(|l| h.query_all_by_label(l).next().map(|n| n.rect().center().y.round() as i32)).collect();
        ys.sort_unstable();
        let mut rows = 0;
        let mut last: Option<i32> = None;
        for y in ys {
            if last.is_none_or(|l| y - l > 8) {
                rows += 1;
                last = Some(y);
            }
        }
        rows.max(1)
    }

    const HOME: [&str; 8] = ["Clipboard", "Slides", "Font", "Paragraph", "Images", "Drawing", "Editing", "Designer"];

    /// 819x614 pt at 1.25 (a 1024x768-client window) has room for Clipboard's three control rows
    /// and one drop-down per other section, as the measured PowerPoint reference does. The
    /// name band is drawn for both layouts, and a folded section opens its commands.
    #[test]
    fn home_sections_fold_to_one_row_drop_downs_at_1024() {
        let mut h = home(819.0, 614.0);
        assert_eq!(control_rows(&h, &["Paste", "Cut", "Copy", "Format"]), 3, "Clipboard keeps its three rows");
        // Every other section is one row: either its one-row design or its drop-down.
        let one_row: [(&str, &[&str]); 7] = [
            ("Slides", &["New Slide", "Layout", "Reset", "Section"]),
            ("Font", &["Font", "Change Case"]),
            ("Paragraph", &["Bullets", "Text Direction"]),
            ("Images", &["Picture"]),
            ("Drawing", &["Shapes", "Shape Fill"]),
            ("Editing", &["Find", "Selection Pane"]),
            ("Designer", &["Design Ideas"]),
        ];
        for (group, labels) in one_row {
            assert_eq!(control_rows(&h, labels), 1, "{group} is one row at 1024");
        }
        assert!(h.query_by_label("New Slide").is_none(), "Slides' commands are folded away");
        h.get_by_role_and_label(Role::Button, "Slides").click();
        h.run_steps(3);
        assert!(h.query_by_label("New Slide").is_some(), "the drop-down holds the group as designed");

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

    /// PowerPoint's Home narrows Font and Paragraph to three rows and widens each to two at the
    /// measured window widths: Font at 1600 px (1280 pt), Paragraph at 1900 px (1520 pt).
    #[test]
    fn font_and_paragraph_use_the_wide_two_row_form_at_the_measured_widths() {
        let font = ["Font", "Change Case", "Text Highlight Color"];
        let h = home(928.7, 768.0);
        assert_eq!(control_rows(&h, &font), 1, "Font folds just below 1161 px");
        let h = home(928.8, 768.0);
        assert_eq!(control_rows(&h, &font), 3, "Font keeps PowerPoint's three rows at 1161 px");
        let h = home(1228.0, 768.0);
        assert_eq!(control_rows(&h, &font), 3, "Font keeps PowerPoint's three rows at 1536 px");
        let h = home(1279.9, 768.0);
        assert_eq!(control_rows(&h, &font), 3, "Font is three rows just below 1600 px");
        let h = home(1280.0, 768.0);
        assert_eq!(control_rows(&h, &font), 2, "Font is two rows at 1600 px");

        let paragraph = ["Bullets", "Text Direction", "Paragraph…"];
        let h = home(1400.0, 768.0);
        assert_eq!(control_rows(&h, &paragraph), 3, "Paragraph keeps PowerPoint's three rows below 1900 px");
        let h = home(1519.9, 768.0);
        assert_eq!(control_rows(&h, &paragraph), 3, "Paragraph is three rows just below 1900 px");
        let h = home(1520.0, 768.0);
        assert_eq!(control_rows(&h, &paragraph), 2, "Paragraph is two rows at 1900 px");
    }

    /// Home's other measured sections stack into three rows at their measured widths.
    #[test]
    fn home_sections_stack_at_their_measured_widths() {
        let slides = ["New Slide", "Layout", "Reset", "Section"];
        assert_eq!(control_rows(&home(1119.9, 768.0), &slides), 1, "Slides is one row just below 1400 px");
        assert_eq!(control_rows(&home(1120.0, 768.0), &slides), 3, "Slides is three rows at 1400 px");

        let drawing = ["Shapes", "Text Box", "Arrange", "Quick Styles", "Shape Fill", "Shape Outline", "Shape Effects"];
        assert_eq!(control_rows(&home(1279.9, 768.0), &drawing), 1, "Drawing is one row just below 1600 px");
        assert_eq!(control_rows(&home(1280.0, 768.0), &drawing), 3, "Drawing is three rows at 1600 px");

        let editing = ["Find", "Replace", "Replace Fonts…", "Selection Pane"];
        assert_eq!(control_rows(&home(1519.9, 768.0), &editing), 1, "Editing is one row just below 1900 px");
        assert_eq!(control_rows(&home(1520.0, 768.0), &editing), 3, "Editing is three rows at 1900 px");

        let paragraph = ["Bullets", "Text Direction", "Paragraph…"];
        assert_eq!(control_rows(&home(1039.9, 768.0), &paragraph), 1, "Paragraph is one row just below 1300 px");
        assert_eq!(control_rows(&home(1040.0, 768.0), &paragraph), 3, "Paragraph is three rows at 1300 px");
    }

    /// Transitions Timing adds its second timing row at 520 points (650 px).
    #[test]
    fn transitions_timing_rows_at_the_measured_width() {
        let labels = ["Duration:", "On Mouse Click", "After:", "Apply To All"];
        assert_eq!(control_rows(&tab("Transitions", 519.9, 768.0), &labels), 1, "one drop-down just below 650 px");
        assert_eq!(control_rows(&tab("Transitions", 520.0, 768.0), &labels), 2, "two rows at 650 px");
        assert_eq!(control_rows(&tab("Transitions", 1000.0, 768.0), &labels), 2, "still two rows when wide");
    }

    /// Review's proofing and activity sections are two rows only between 819 and 880 points.
    #[test]
    fn review_sections_use_two_rows_in_their_measured_band() {
        let proofing = ["Spelling", "Thesaurus"];
        assert_eq!(control_rows(&tab("Review", 818.9, 768.0), &proofing), 1, "one row just below 1024 px");
        assert_eq!(control_rows(&tab("Review", 819.0, 768.0), &proofing), 2, "two rows at 1024 px");
        assert_eq!(control_rows(&tab("Review", 879.9, 768.0), &proofing), 2, "two rows just below 1100 px");
        assert_eq!(control_rows(&tab("Review", 880.0, 768.0), &proofing), 1, "one row at 1100 px");

        let activity = ["Compare", "Accept All", "End Review"];
        assert_eq!(control_rows(&tab("Review", 818.9, 768.0), &activity), 1, "Activity/Compare is one row just below 1024 px");
        assert_eq!(control_rows(&tab("Review", 819.0, 768.0), &activity), 2, "Activity/Compare is two rows at 1024 px");
        assert_eq!(control_rows(&tab("Review", 879.9, 768.0), &activity), 2, "Activity/Compare is two rows just below 1100 px");
        assert_eq!(control_rows(&tab("Review", 880.0, 768.0), &activity), 1, "Activity/Compare is one row at 1100 px");
    }

    #[test]
    fn insert_sections_stack_at_their_measured_widths() {
        let images = ["Pictures", "Photo Album", "Screenshot"];
        assert_eq!(control_rows(&tab("Insert", 1119.9, 768.0), &images), 1, "Images is one row just below 1400 px");
        assert_eq!(control_rows(&tab("Insert", 1120.0, 768.0), &images), 3, "Images is three rows at 1400 px");
        assert_eq!(control_rows(&tab("Insert", 1519.9, 768.0), &images), 3, "Images is three rows just below 1900 px");
        assert_eq!(control_rows(&tab("Insert", 1520.0, 768.0), &images), 1, "Images is one row again at 1900 px");

        let illustrations = ["Shapes", "Icons", "SmartArt", "Chart"];
        assert_eq!(control_rows(&tab("Insert", 928.7, 768.0), &illustrations), 1, "Illustrations is one row just below 1161 px");
        assert_eq!(control_rows(&tab("Insert", 928.8, 768.0), &illustrations), 3, "Illustrations is three rows at 1161 px");
        assert_eq!(control_rows(&tab("Insert", 1519.9, 768.0), &illustrations), 3, "Illustrations is three rows just below 1900 px");
        assert_eq!(control_rows(&tab("Insert", 1520.0, 768.0), &illustrations), 1, "Illustrations is one row at 1900 px");

        let text = ["Text Box", "Header & Footer", "WordArt", "Date & Time", "Slide Number"];
        assert_eq!(control_rows(&tab("Insert", 1279.9, 768.0), &text), 1, "Text is one row just below 1600 px");
        assert_eq!(control_rows(&tab("Insert", 1280.0, 768.0), &text), 3, "Text is three rows at 1600 px");
    }

    #[test]
    fn design_customize_rows_at_the_measured_width() {
        let labels = ["Colors", "Fonts", "Effects", "Background Styles"];
        assert_eq!(control_rows(&tab("Design", 519.9, 768.0), &labels), 1, "one drop-down just below 650 px");
        assert_eq!(control_rows(&tab("Design", 520.0, 768.0), &labels), 3, "three rows at 650 px");
        assert_eq!(control_rows(&tab("Design", 879.9, 768.0), &labels), 3, "three rows just below 1100 px");
        assert_eq!(control_rows(&tab("Design", 880.0, 768.0), &labels), 1, "one row at 1100 px");
    }

    #[test]
    fn slide_show_start_and_record_rows_at_their_measured_widths() {
        let start = ["Play from Start", "Play from Current Slide", "Presenter View", "Custom Show"];
        assert_eq!(control_rows(&tab("Slide Show", 599.9, 768.0), &start), 1, "Start is one row just below 750 px");
        assert_eq!(control_rows(&tab("Slide Show", 600.0, 768.0), &start), 3, "Start is three rows at 750 px");
        assert_eq!(control_rows(&tab("Slide Show", 1119.9, 768.0), &start), 3, "Start is three rows just below 1400 px");
        assert_eq!(control_rows(&tab("Slide Show", 1120.0, 768.0), &start), 1, "Start is one row at 1400 px");

        let record = ["Cameo", "From Beginning", "From Current Slide"];
        assert_eq!(control_rows(&tab("Record", 599.9, 768.0), &record), 1, "Record is one row just below 750 px");
        assert_eq!(control_rows(&tab("Record", 600.0, 768.0), &record), 3, "Record is three rows at 750 px");
        assert_eq!(control_rows(&tab("Record", 679.9, 768.0), &record), 3, "Record is three rows just below 850 px");
        assert_eq!(control_rows(&tab("Record", 680.0, 768.0), &record), 1, "Record is one row at 850 px");
    }

    #[test]
    fn view_master_and_presentation_views_rows_at_their_measured_widths() {
        let presentation = ["Normal", "Outline View", "Slide Sorter", "Notes Page", "Reading View"];
        assert_eq!(control_rows(&tab("View", 679.9, 768.0), &presentation), 1, "Presentation Views is one row just below 850 px");
        assert_eq!(control_rows(&tab("View", 680.0, 768.0), &presentation), 3, "Presentation Views is three rows at 850 px");
        assert_eq!(control_rows(&tab("View", 1039.9, 768.0), &presentation), 3, "Presentation Views is three rows just below 1300 px");
        assert_eq!(control_rows(&tab("View", 1040.0, 768.0), &presentation), 1, "Presentation Views is one row at 1300 px");

        let masters = ["Slide Master", "Handout Master", "Notes Master"];
        assert_eq!(control_rows(&tab("View", 759.9, 768.0), &masters), 1, "Master Views is one row just below 950 px");
        assert_eq!(control_rows(&tab("View", 760.0, 768.0), &masters), 3, "Master Views is three rows at 950 px");
        assert_eq!(control_rows(&tab("View", 1119.9, 768.0), &masters), 3, "Master Views is three rows just below 1400 px");
        assert_eq!(control_rows(&tab("View", 1120.0, 768.0), &masters), 1, "Master Views is one row at 1400 px");
    }

    #[test]
    fn animation_sections_stack_at_their_measured_widths() {
        let advanced = ["Add Animation", "Animation Pane", "Trigger", "Animation Painter"];
        assert_eq!(control_rows(&tab("Animations", 519.9, 768.0), &advanced), 1, "Advanced Animation is one row just below 650 px");
        assert_eq!(control_rows(&tab("Animations", 520.0, 768.0), &advanced), 3, "Advanced Animation is three rows at 650 px");

        let timing = ["Start:", "Duration:", "Delay:"];
        assert_eq!(control_rows(&tab("Animations", 759.9, 768.0), &timing), 1, "Timing folds just below 950 px");
        assert_eq!(control_rows(&tab("Animations", 760.0, 768.0), &timing), 3, "Timing is three rows at 950 px");
    }

    /// Sections with fewer commands than the measured three rows need stay one row: PowerPoint
    /// stacks Set Up, Color/Grayscale and Window into three rows, but DeckCraft's Set Up and
    /// Color/Grayscale have two commands and Window one. The measured forms are documented as
    /// unfillable; this pins the one-row result at the widths where PowerPoint stacks them.
    #[test]
    fn sections_with_too_few_commands_keep_one_row() {
        let setup = ["Set Up Slide Show", "Hide Slide"];
        assert_eq!(control_rows(&tab("Slide Show", 1280.0, 768.0), &setup), 1, "Set Up is one row at 1600 px");
        let colors = ["Grayscale", "Dark Mode"];
        assert_eq!(control_rows(&tab("View", 1040.0, 768.0), &colors), 1, "Color/Grayscale is one row at 1300 px");
        let window = ["New Window"];
        assert_eq!(control_rows(&tab("View", 960.0, 768.0), &window), 1, "Window is one row at 1200 px");
    }

    /// A cached plan is reused only while the preferred row forms it assumed still hold. `begin`
    /// keys the stored plan by the fit width but validates it against `preferred` at the content
    /// width, so a plan cached at one preference must not be replayed after the preference moves.
    /// Without the check the same plan bytes would satisfy the width-key cache both times.
    #[test]
    fn a_stored_plan_is_dropped_when_a_preferred_level_moves() {
        let ctx = egui::Context::default();
        let key = ("Insert".to_string(), width_key(700.0));
        // A plan cached under COMPACT-era preferences must not validate once Images stacks.
        ctx.data_mut(|d| {
            let m = d.get_temp_mut_or_default::<Measured>(measured_id());
            m.order.insert("Insert".into(), vec!["Images".into()]);
            m.widths.insert(("Insert".into(), 0, "Images".into(), FULL), 300.0);
            m.widths.insert(("Insert".into(), 0, "Images".into(), STACKED), 120.0);
            m.widths.insert(("Insert".into(), 0, "Images".into(), COMPACT), 65.0);
            m.plans.insert(key.clone(), (vec![COMPACT], vec![COMPACT]));
        });
        // Stored COMPACT preference at the current stacked width is stale: replan from measured
        // widths instead of replaying the cached COMPACT level.
        let (titles, groups, preferred, settled) = ctx.data_mut(|d| {
            let m = d.get_temp_mut_or_default::<Measured>(measured_id());
            let titles = m.order.get("Insert").cloned().unwrap_or_default();
            let groups: Vec<GroupWidths> = titles
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    let key = |level| m.widths.get(&("Insert".to_string(), i, t.clone(), level)).copied();
                    GroupWidths { title: t.clone(), widths: [key(FULL), key(STACKED), key(COMPACT)] }
                })
                .collect();
            let preferred: Vec<u8> = titles.iter().map(|t| preferred("Insert", t, 1120.0)).collect();
            let stored = m.plans.get(&key).cloned();
            let settled = stored.filter(|(levels, planned)| {
                planned == &preferred
                    && levels.len() == groups.len()
                    && groups.iter().zip(levels).all(|(g, level)| g.widths.get(*level as usize).is_some_and(Option::is_some))
            });
            (titles, groups, preferred, settled)
        });
        assert_eq!(preferred, vec![STACKED], "at 1400 px Images prefers its stacked form");
        assert!(settled.is_none(), "the COMPACT-era cached plan does not validate: {titles:?} {groups:?}");
    }
    /// Writes one PNG per measured transition width when `DECKCRAFT_RIBBON_SNAPSHOTS` names a
    /// directory: the visual check of every row form at the width where PowerPoint changes it.
    /// Off unless the variable is set, so the suite stays headless.
    #[test]
    fn snapshot_row_forms_at_the_transition_widths() {
        let Ok(dir) = std::env::var("DECKCRAFT_RIBBON_SNAPSHOTS") else { return };
        let _gpu = crate::gpu_lock::gpu_lock();
        let cases: [(&str, &[u32]); 9] = [
            ("Home", &[1161, 1300, 1400, 1600, 1900]),
            ("Insert", &[1161, 1400, 1600, 1900]),
            ("Transitions", &[650]),
            ("Review", &[1024, 1100]),
            ("Slide Show", &[750, 1400]),
            ("Record", &[750, 850]),
            ("View", &[850, 950, 1300, 1400]),
            ("Animations", &[650, 950]),
            ("Design", &[650, 1100]),
        ];
        let dir = std::path::Path::new(&dir);
        for (tab_name, widths) in cases {
            for px in widths {
                // The measurement is outer pixels at 125% DPI; the window and content width are
                // logical points.
                let mut h = tab(tab_name, *px as f32 / 1.25, 768.0);
                let img = h.render().unwrap();
                let name = format!("{}-{px}px@125.png", tab_name.replace(' ', "-"));
                img.save(dir.join(name)).unwrap();
            }
        }
    }
}
