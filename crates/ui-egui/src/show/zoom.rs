use super::presenter::{NOTES_MAX, NOTES_MIN};
use super::render::render_thumb;
use super::{Align2, Color32, CornerRadius, Pos2, Presentation, Rect, Show, Stroke, json, pos2, theme, vec2};

impl Show {
    pub(super) fn zoom_uv(&self) -> Rect {
        self.zoom.map(Zoom::uv).unwrap_or_else(unit_uv)
    }

    pub(super) fn zoom_label(&self) -> &'static str {
        if self.zoom.is_some() {
            "Zoom on"
        } else if self.zoom_arm {
            "Zoom…"
        } else {
            "Zoom"
        }
    }

    /// G opens See All Slides in the window it was pressed in: Presenter View's grid stays off the audience screen (#1130).
    pub(super) fn open_grid(&mut self, presenter: bool) {
        if presenter {
            self.presenter_grid = true;
        } else {
            self.grid = true;
        }
    }

    /// Z, or the Zoom button: arm a click-to-zoom, or leave a zoom that is already on.
    pub(super) fn zoom_key(&mut self) {
        if self.zoom.is_some() || self.zoom_arm {
            self.dismiss_zoom();
        } else {
            self.zoom_arm = true;
            self.pen = false;
        }
    }

    /// Ctrl/Cmd+wheel: `lines` notches in (positive) or out around the slide point under `p`.
    /// Back to the whole slide leaves the zoom. A pointer off the slide does nothing.
    pub(super) fn wheel_zoom(&mut self, srect: Rect, p: Pos2, lines: f32) {
        let uv = self.zoom_uv();
        let Some((nx, ny)) = norm_on_slide(srect, uv, p) else { return };
        let from = self.zoom.map_or(1.0, |z| z.scale);
        let scale = (from * 2f32.powf(lines / WHEEL_NOTCHES_PER_DOUBLING)).min(WHEEL_ZOOM_MAX);
        if !scale.is_finite() {
            return;
        }
        self.zoom_arm = false;
        if scale <= 1.0 + 1e-3 {
            self.zoom = None;
            return;
        }
        // The new window puts (nx, ny) at the same place on screen as before.
        let w = 1.0 / scale;
        let fx = (p.x - srect.min.x) / srect.width();
        let fy = (p.y - srect.min.y) / srect.height();
        let x = (nx - fx * w).clamp(0.0, 1.0 - w);
        let y = (ny - fy * w).clamp(0.0, 1.0 - w);
        self.zoom = Some(Zoom { nx: x + w * 0.5, ny: y + w * 0.5, scale });
    }

    /// Esc closes the See All Slides of the window it was pressed in, then a zoom, and only then leaves the show.
    pub(super) fn back_out(&mut self, presenter: bool) -> bool {
        if presenter && self.presenter_grid {
            self.presenter_grid = false;
            true
        } else if !presenter && self.grid {
            self.grid = false;
            true
        } else {
            self.dismiss_zoom()
        }
    }

    /// Esc backs out of a zoom before it leaves the show. True when it did.
    pub(super) fn dismiss_zoom(&mut self) -> bool {
        if self.zoom.is_some() || self.zoom_arm {
            self.zoom = None;
            self.zoom_arm = false;
            true
        } else {
            false
        }
    }

    pub fn blank_name(&self) -> Option<&'static str> {
        match self.blank {
            Some(c) if c == Color32::BLACK => Some("black"),
            Some(c) if c == Color32::WHITE => Some("white"),
            Some(_) => Some("blank"),
            None => None,
        }
    }

    pub fn zoom_view(&self) -> Option<(f32, f32, f32)> {
        self.zoom.map(|z| (z.nx, z.ny, z.scale))
    }

    pub(super) fn push_slide_number(&mut self, digit: char, now: f64) {
        if !digit.is_ascii_digit() || self.goto.len() >= GOTO_DIGITS {
            return;
        }
        self.goto.push(digit);
        self.goto_at = if now.is_finite() { now } else { 0.0 };
    }

    /// 1-based slide number, or `None` for 0 or a non-number. The buffer is cleared either way.
    pub(super) fn take_goto(&mut self) -> Option<usize> {
        let raw = std::mem::take(&mut self.goto);
        raw.parse::<usize>().ok().and_then(|n| n.checked_sub(1))
    }

    pub(super) fn expire_goto(&mut self, now: f64) {
        if self.goto.is_empty() || !now.is_finite() || !self.goto_at.is_finite() {
            return;
        }
        if now - self.goto_at > GOTO_HOLD {
            self.goto.clear();
        }
    }

    /// Esc drops a number in progress. True when it did.
    pub(super) fn clear_goto(&mut self) -> bool {
        if self.goto.is_empty() {
            false
        } else {
            self.goto.clear();
            true
        }
    }

    pub(super) fn nudge_notes(&mut self, delta: f32) {
        let next = self.notes_px + delta;
        if next.is_finite() {
            self.notes_px = next.clamp(NOTES_MIN, NOTES_MAX);
        }
    }

    pub fn showing_value(&self, n_slides: usize, aspect: f32, full: Rect, now: f64) -> serde_json::Value {
        let mut v = json!({
            "slide": self.state.slide,
            "step": self.state.step,
            "ended": self.ended,
            "grid": self.grid,
            "zoomArm": self.zoom_arm,
            "blank": self.blank_name(),
            "zoom": self.zoom_view().map(|(x, y, scale)| json!({"x": x, "y": y, "scale": scale})),
            "presenterPaused": self.timer.paused(),
            "presenterSecs": self.timer.elapsed(now),
            "notesPx": self.notes_px,
            "custom": self.state.custom_name(),
            "goto": if self.goto.is_empty() { None } else { Some(self.goto.clone()) },
        });
        if self.grid {
            let slides = grid_indices(n_slides);
            let cells = see_all_cells(full, slides.len(), aspect);
            v["cells"] =
                serde_json::Value::Array(slides.iter().zip(cells).map(|(i, r)| json!({"slide": i, "x": r.center().x, "y": r.center().y})).collect());
        }
        v
    }

    pub(super) fn reading_bar_hit(&self, ctx: &egui::Context) -> bool {
        // Clicks on the on-screen controls must not also advance.
        ctx.input(|i| i.pointer.interact_pos()).is_some_and(|p| {
            let r = ctx.content_rect();
            (self.reading && p.y > r.max.y - 34.0) || (!self.reading && tool_bar_rect(r).expand(8.0).contains(p))
        })
    }
}

const GOTO_HOLD: f64 = 3.0;

pub(super) const GOTO_DIGITS: usize = 4;

const ZOOM_SCALE: f32 = 2.0;

/// Four notches double the zoom, so the wheel passes through the click-zoom's 2× on its way to 4×.
const WHEEL_NOTCHES_PER_DOUBLING: f32 = 4.0;

/// PowerPoint's slide show magnifier goes up to 400%.
const WHEEL_ZOOM_MAX: f32 = 4.0;

pub(super) const GRID_CAP: usize = 48;

const TOOL_BAR_W: f32 = 520.0;

#[derive(Clone, Copy)]
pub(super) struct Zoom {
    pub(super) nx: f32,
    pub(super) ny: f32,
    pub(super) scale: f32,
}

impl Zoom {
    pub(super) fn at(nx: f32, ny: f32) -> Self {
        Zoom { nx: finite_unit(nx), ny: finite_unit(ny), scale: ZOOM_SCALE }
    }

    pub(super) fn uv(self) -> Rect {
        zoom_uv(self.nx, self.ny, self.scale)
    }
}

fn finite_unit(n: f32) -> f32 {
    if n.is_finite() { n.clamp(0.0, 1.0) } else { 0.5 }
}

pub(super) fn plain(m: egui::Modifiers) -> bool {
    !m.command && !m.ctrl && !m.alt && !m.shift
}

pub(super) fn unit_uv() -> Rect {
    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0))
}

pub(super) fn slide_aspect(doc: &Presentation) -> f32 {
    let a = doc.slide_size.width as f32 / doc.slide_size.height.max(1.0) as f32;
    if a.is_finite() && a > 0.05 { a } else { 16.0 / 9.0 }
}

/// The part of the slide a zoom shows, in 0–1 texture space. The window stays on the slide.
pub(super) fn zoom_uv(nx: f32, ny: f32, scale: f32) -> Rect {
    let scale = if scale.is_finite() && scale > 1.0 { scale } else { ZOOM_SCALE };
    let w = 1.0 / scale;
    if w >= 1.0 {
        return unit_uv();
    }
    let x = (nx - w * 0.5).clamp(0.0, 1.0 - w);
    let y = (ny - w * 0.5).clamp(0.0, 1.0 - w);
    Rect::from_min_size(pos2(x, y), vec2(w, w))
}

/// The Ctrl/Cmd+wheel movement this frame in wheel lines (up is positive), `None` when there was
/// none. Read from the raw events: egui's `zoom_delta` also carries the trackpad pinch, which is
/// not a show gesture.
pub(super) fn wheel_zoom_lines(events: &[egui::Event], line_points: f32) -> Option<f32> {
    let mut lines = None;
    for event in events {
        if let egui::Event::MouseWheel { unit, delta, modifiers, .. } = event
            && (modifiers.ctrl || modifiers.command)
        {
            let along = if delta.x.abs() > delta.y.abs() { delta.x } else { delta.y };
            let n = match unit {
                egui::MouseWheelUnit::Point => along / line_points.max(1.0),
                egui::MouseWheelUnit::Line | egui::MouseWheelUnit::Page => along,
            };
            *lines.get_or_insert(0.0) += if n.is_finite() { n } else { 0.0 };
        }
    }
    lines
}

pub(super) fn zoom_frame(slide: Rect, uv: Rect) -> Rect {
    Rect::from_min_size(
        pos2(slide.min.x + uv.min.x * slide.width(), slide.min.y + uv.min.y * slide.height()),
        vec2(uv.width() * slide.width(), uv.height() * slide.height()),
    )
}

pub(super) fn toggle_blank(show: &mut Show, color: Color32) {
    show.blank = if show.blank == Some(color) { None } else { Some(color) };
}

pub(super) fn tool_bar_rect(full: Rect) -> Rect {
    let w = TOOL_BAR_W.min((full.width() - 32.0).max(32.0));
    Rect::from_min_size(pos2(full.min.x + 16.0, full.max.y - 50.0), vec2(w, 34.0))
}

pub(super) fn grid_indices(n: usize) -> Vec<usize> {
    (0..n.min(GRID_CAP)).collect()
}

/// Thumbnail rectangles for `n` slides inside `full`, largest 16:9-style thumbs that fit.
pub(super) fn see_all_cells(full: Rect, n: usize, aspect: f32) -> Vec<Rect> {
    if n == 0 || full.width() < 16.0 || full.height() < 16.0 {
        return vec![];
    }
    let aspect = if aspect.is_finite() && aspect > 0.05 { aspect } else { 16.0 / 9.0 };
    let margin = 28.0;
    let gap = 16.0;
    let label_h = 22.0;
    let inner = full.shrink(margin);
    let inner = Rect::from_min_max(pos2(inner.min.x, inner.min.y + 36.0), inner.max);
    if inner.width() < 8.0 || inner.height() < 8.0 {
        return vec![];
    }
    let mut best: Vec<Rect> = vec![];
    let mut best_area = 0.0_f32;
    for cols in 1..=n.min(8) {
        let rows = n.div_ceil(cols);
        let cell_w = (inner.width() - gap * (cols.saturating_sub(1) as f32)) / cols as f32;
        let cell_h = (inner.height() - gap * (rows.saturating_sub(1) as f32)) / rows as f32;
        if cell_w < 8.0 || cell_h < label_h + 8.0 {
            continue;
        }
        let thumb_max_h = cell_h - label_h;
        let mut tw = cell_w;
        let mut th = tw / aspect;
        if th > thumb_max_h {
            th = thumb_max_h;
            tw = th * aspect;
        }
        if tw < 8.0 || th < 8.0 {
            continue;
        }
        let area = tw * th;
        if area <= best_area {
            continue;
        }
        best_area = area;
        best = (0..n)
            .map(|i| {
                let c = i % cols;
                let r = i / cols;
                let ox = inner.min.x + c as f32 * (cell_w + gap) + (cell_w - tw) * 0.5;
                let oy = inner.min.y + r as f32 * (cell_h + gap);
                Rect::from_min_size(pos2(ox, oy), vec2(tw, th))
            })
            .collect();
    }
    best
}

pub(super) fn see_all_hit(full: Rect, n: usize, aspect: f32, p: Pos2) -> Option<usize> {
    see_all_cells(full, n, aspect).into_iter().position(|r| r.expand2(vec2(4.0, 18.0)).contains(p))
}

pub(super) fn norm_on_slide(srect: Rect, uv: Rect, p: Pos2) -> Option<(f32, f32)> {
    if srect.width() <= 1.0 || srect.height() <= 1.0 || !srect.contains(p) {
        return None;
    }
    let u = (p.x - srect.min.x) / srect.width();
    let v = (p.y - srect.min.y) / srect.height();
    let nx = uv.min.x + u * uv.width();
    let ny = uv.min.y + v * uv.height();
    (nx.is_finite() && ny.is_finite()).then_some((nx.clamp(0.0, 1.0), ny.clamp(0.0, 1.0)))
}

pub(super) fn slide_to_screen(srect: Rect, uv: Rect, nx: f32, ny: f32) -> Pos2 {
    let u = (nx - uv.min.x) / uv.width().max(1e-4);
    let v = (ny - uv.min.y) / uv.height().max(1e-4);
    pos2(srect.min.x + u * srect.width(), srect.min.y + v * srect.height())
}

pub(super) fn slide_box(srect: Rect, uv: Rect, (slide_w, slide_h): (f32, f32), (x, y, w, h): (f32, f32, f32, f32)) -> Rect {
    let a = slide_to_screen(srect, uv, x / slide_w.max(1.0), y / slide_h.max(1.0));
    let b = slide_to_screen(srect, uv, (x + w) / slide_w.max(1.0), (y + h) / slide_h.max(1.0));
    Rect::from_min_max(a, b)
}

pub(super) fn paint_grid(painter: &egui::Painter, ctx: &egui::Context, show: &mut Show, doc: &Presentation, full: Rect) {
    let slides = grid_indices(doc.slides.len());
    let aspect = slide_aspect(doc);
    let cells = see_all_cells(full, slides.len(), aspect);
    painter.rect_filled(full, CornerRadius::ZERO, Color32::from_gray(16));
    painter.text(pos2(full.center().x, full.min.y + 28.0), Align2::CENTER_CENTER, "All slides", theme::font(20.0), Color32::WHITE);
    if doc.slides.len() > GRID_CAP {
        painter.text(
            pos2(full.center().x, full.min.y + 50.0),
            Align2::CENTER_CENTER,
            format!("First {GRID_CAP} slides"),
            theme::font(13.0),
            Color32::from_gray(180),
        );
    }
    let budget = if slides.len() <= 12 { 12 } else { 4 };
    let mut made = 0;
    for (i, rect) in slides.iter().copied().zip(cells) {
        painter.rect_filled(rect, CornerRadius::same(4), Color32::from_gray(32));
        let tex = match show.thumbs.iter().find(|(k, _)| *k == i).map(|(_, t)| t.clone()) {
            Some(t) => Some(t),
            None if made < budget => {
                made += 1;
                render_thumb(ctx, show, doc, i)
            }
            None => {
                ctx.request_repaint();
                None
            }
        };
        if let Some(t) = tex {
            painter.image(t.id(), rect, unit_uv(), Color32::WHITE);
        }
        let current = i == show.state.slide;
        painter.rect_stroke(
            rect,
            CornerRadius::same(4),
            Stroke::new(if current { 3.0 } else { 1.0 }, if current { Color32::WHITE } else { Color32::from_gray(70) }),
            egui::StrokeKind::Inside,
        );
        let hidden = doc.slides.get(i).is_some_and(|s| s.hidden);
        let label = if hidden { format!("{}  Hidden", i + 1) } else { format!("{}", i + 1) };
        painter.text(
            pos2(rect.center().x, rect.max.y + 12.0),
            Align2::CENTER_CENTER,
            label,
            theme::font(13.0),
            if current { Color32::WHITE } else { Color32::from_gray(180) },
        );
    }
}
