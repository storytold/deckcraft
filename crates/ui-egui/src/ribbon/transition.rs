//! WordCraft-style ribbon tab motion: a sliding underline and incoming content only.

fn ease(p: f32) -> f32 {
    1.0 - (1.0 - p.clamp(0.0, 1.0)).powi(3)
}

pub(crate) fn reset(ctx: &egui::Context) {
    let viewport = ctx.viewport_id();
    ctx.data_mut(|d| {
        d.remove::<Underline>(egui::Id::unique(("tab_underline", viewport)));
        d.remove::<TabChange>(egui::Id::unique(("ribbon_tab_change", viewport)));
    });
}

const UNDERLINE_SLIDE_SECS: f64 = 0.2;
const CONTENT_SWITCH_SECS: f64 = 0.16;
pub(super) const CONTENT_SWITCH_SLIDE: f32 = 14.0;

#[derive(Clone, Debug, PartialEq)]
struct Underline {
    tab: String,
    from: (f32, f32),
    to: (f32, f32),
    start: f64,
}

fn underline_at(u: &Underline, now: f64) -> (f32, f32) {
    let e = ease(((now - u.start) / UNDERLINE_SLIDE_SECS) as f32);
    (egui::lerp(u.from.0..=u.to.0, e), egui::lerp(u.from.1..=u.to.1, e))
}

pub(super) fn slide_underline(ui: &egui::Ui, tab: &str, target: egui::Rect, color: egui::Color32, now: f64, animate: bool) {
    let id = egui::Id::unique(("tab_underline", ui.ctx().viewport_id()));
    let prev: Option<Underline> = ui.ctx().data(|d| d.get_temp(id));
    let snap = Underline { tab: tab.to_owned(), from: (target.center().x, target.width()), to: (target.center().x, target.width()), start: now };
    let next = match prev {
        Some(p) if animate && p.tab != tab => Underline { from: underline_at(&p, now), ..snap },
        Some(p) if animate && now - p.start < UNDERLINE_SLIDE_SECS => Underline { to: (target.center().x, target.width()), ..p },
        _ => snap,
    };
    let (x, width) = underline_at(&next, now);
    if next.from != next.to && now - next.start < UNDERLINE_SLIDE_SECS {
        ui.ctx().request_repaint();
    }
    ui.ctx().data_mut(|d| d.insert_temp(id, next));
    ui.painter().rect_filled(egui::Rect::from_center_size(egui::pos2(x, target.center().y), egui::vec2(width, target.height())), 2.0, color);
}

#[derive(Clone, Debug, PartialEq)]
struct TabChange {
    tab: String,
    index: usize,
    start: f64,
    dir: f32,
}

pub(super) fn tab_content_entry(ctx: &egui::Context, tab: &str, index: usize, now: f64, animate: bool) -> (f32, f32) {
    let id = egui::Id::unique(("ribbon_tab_change", ctx.viewport_id()));
    let prev: Option<TabChange> = ctx.data(|d| d.get_temp(id));
    let next = match prev {
        Some(p) if p.tab != tab => TabChange {
            tab: tab.to_owned(),
            index,
            start: now,
            dir: if !animate || p.index == index {
                0.0
            } else if index > p.index {
                1.0
            } else {
                -1.0
            },
        },
        Some(p) if animate => p,
        Some(p) => TabChange { start: f64::MIN, dir: 0.0, ..p },
        None => TabChange { tab: tab.to_owned(), index, start: f64::MIN, dir: 0.0 },
    };
    let p = ((now - next.start) / CONTENT_SWITCH_SECS).clamp(0.0, 1.0) as f32;
    let dir = next.dir;
    ctx.data_mut(|d| d.insert_temp(id, next));
    if p < 1.0 && dir != 0.0 {
        ctx.request_repaint();
        (ease(p), dir)
    } else {
        (1.0, 0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn underline(ctx: &egui::Context, tab: &str, x: f32, time: f64, animate: bool) -> egui::Rect {
        let mut out = ctx.run_ui(egui::RawInput { time: Some(time), ..Default::default() }, |ui| {
            slide_underline(ui, tab, egui::Rect::from_center_size(egui::pos2(x, 20.0), egui::vec2(40.0, 3.0)), egui::Color32::WHITE, time, animate);
        });
        out.textures_delta.clear();
        let egui::Shape::Rect(rect) = &out.shapes.last().unwrap().shape else { panic!("underline rectangle") };
        rect.rect
    }

    #[test]
    fn underline_moves_and_reverses_from_its_current_position() {
        let ctx = egui::Context::default();
        assert_eq!(underline(&ctx, "Home", 40.0, 0.0, true).center().x, 40.0);
        assert_eq!(underline(&ctx, "Insert", 140.0, 1.0, true).center().x, 40.0);
        let middle = underline(&ctx, "Insert", 140.0, 1.1, true).center().x;
        assert!(middle > 40.0 && middle < 140.0);
        assert_eq!(underline(&ctx, "Home", 40.0, 1.1, true).center().x, middle);
        assert_eq!(underline(&ctx, "Home", 40.0, 1.4, true).center().x, 40.0);
        assert_eq!(underline(&ctx, "View", 500.0, 1.5, false).center().x, 500.0);
    }

    #[test]
    fn content_enters_from_the_selected_tab_side_and_settles() {
        let ctx = egui::Context::default();
        assert_eq!(tab_content_entry(&ctx, "Home", 0, 0.0, true), (1.0, 0.0));
        assert_eq!(tab_content_entry(&ctx, "Insert", 1, 1.0, true), (0.0, 1.0));
        let (p, dir) = tab_content_entry(&ctx, "Insert", 1, 1.08, true);
        assert!(p > 0.0 && p < 1.0);
        assert_eq!(dir, 1.0);
        assert_eq!(tab_content_entry(&ctx, "Home", 0, 1.09, true), (0.0, -1.0));
        assert_eq!(tab_content_entry(&ctx, "Home", 0, 1.3, true), (1.0, 0.0));
    }

    #[test]
    fn reduced_motion_finishes_a_running_transition_without_replaying_it() {
        let ctx = egui::Context::default();
        tab_content_entry(&ctx, "Home", 0, 0.0, true);
        tab_content_entry(&ctx, "Insert", 1, 1.0, true);
        assert_eq!(tab_content_entry(&ctx, "Insert", 1, 1.02, false), (1.0, 0.0));
        assert_eq!(tab_content_entry(&ctx, "Insert", 1, 1.03, true), (1.0, 0.0));
        assert_eq!(tab_content_entry(&ctx, "View", 9, 1.04, false), (1.0, 0.0));
        reset(&ctx);
        assert_eq!(tab_content_entry(&ctx, "Home", 0, 1.05, true), (1.0, 0.0));
    }

    #[test]
    fn viewports_keep_independent_tab_motion() {
        let ctx = egui::Context::default();
        for (viewport, tab, index, time, want) in
            [(1, "Home", 0, 0.0, (1.0, 0.0)), (2, "View", 9, 0.0, (1.0, 0.0)), (1, "Insert", 1, 1.0, (0.0, 1.0)), (2, "View", 9, 1.01, (1.0, 0.0))]
        {
            let viewport_id = egui::ViewportId::from_hash_of(viewport);
            let mut input = egui::RawInput { viewport_id, ..Default::default() };
            input.viewports.insert(viewport_id, egui::ViewportInfo::default());
            ctx.run_ui(input, |_| assert_eq!(tab_content_entry(&ctx, tab, index, time, true), want)).drop_without_applying_deltas();
        }
    }
}
