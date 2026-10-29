use super::presenter::{NOTES_MAX, NOTES_MIN};
use super::render::render_thumb;
use super::{Align2, Color32, CornerRadius, Pos2, Presentation, Rect, Show, Stroke, json, pos2, theme, vec2};
use crate::sections::{self, GridFilter};

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
    /// The grid starts on the slide's section, at the top.
    pub(super) fn open_grid(&mut self, presenter: bool, doc: &Presentation) {
        self.grid_filter = sections::current_filter(doc, self.state.slide);
        self.grid_scroll = 0.0;
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

    pub fn showing_value(&self, doc: &Presentation, full: Rect, now: f64) -> serde_json::Value {
        let mut v = json!({
            "slide": self.state.slide,
            "step": self.state.step,
            "ended": self.ended,
            "grid": self.grid,
            "gridFilter": grid_filter_value(self.grid_filter),
            "gridScroll": self.grid_scroll,
            "sections": sections::tabs(doc).into_iter().map(|(filter, name, count)| json!({
                "filter": grid_filter_value(filter),
                "name": name,
                "count": count,
            })).collect::<Vec<_>>(),
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
            let frame = grid_frame(full, doc);
            let slides = sections::grid_slides(doc, self.grid_filter);
            let layout = grid_layout(frame.inner, &slides, slide_aspect(doc), self.grid_scroll);
            v["gridScroll"] = json!(layout.scroll);
            v["cells"] = serde_json::Value::Array(
                layout.cells.iter().map(|c| json!({"slide": c.slide, "x": c.rect.center().x, "y": c.rect.center().y})).collect(),
            );
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

/// The most thumbnails kept in memory at once. The grid only renders the cells in view, so this
/// bounds a long deck without limiting how many slides it can show.
pub(super) const MAX_THUMBS: usize = 128;

const TOOL_BAR_W: f32 = 520.0;

const GRID_MARGIN: f32 = 28.0;
const GRID_GAP: f32 = 16.0;
const GRID_LABEL_H: f32 = 22.0;
const GRID_MIN_W: f32 = 150.0;
const GRID_MAX_W: f32 = 360.0;
const GRID_MAX_COLS: usize = 8;
const GRID_BUDGET: usize = 4;
const TAB_H: f32 = 26.0;
const TAB_GAP: f32 = 6.0;
const TAB_ROW_GAP: f32 = 4.0;

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

#[derive(Clone, Debug, PartialEq)]
pub(super) struct GridTab {
    pub filter: GridFilter,
    pub label: String,
    pub count: usize,
    pub rect: Rect,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct GridCell {
    pub slide: usize,
    pub rect: Rect,
}

pub(super) struct GridFrame {
    pub tabs: Vec<GridTab>,
    pub inner: Rect,
}

pub(super) struct GridLayout {
    pub cells: Vec<GridCell>,
    pub content_h: f32,
    pub scroll: f32,
    pub max_scroll: f32,
}

/// The fixed parts of the grid: its title, its section tabs, and the rectangle the thumbnails
/// scroll in. Drawing and hit testing share it, so they can never disagree.
pub(super) fn grid_frame(full: Rect, doc: &Presentation) -> GridFrame {
    let left = full.min.x + GRID_MARGIN;
    let right = full.max.x - GRID_MARGIN;
    let mut x = left;
    let mut y = full.min.y + 42.0;
    let mut tabs = Vec::new();
    for (filter, label, count) in sections::tabs(doc) {
        let text = format!("{label} ({count})");
        let w = (text.chars().count() as f32 * 7.2 + 24.0).clamp(64.0, 240.0);
        if x + w > right && x > left {
            x = left;
            y += TAB_H + TAB_ROW_GAP;
        }
        let rect = Rect::from_min_size(pos2(x, y), vec2(w, TAB_H));
        tabs.push(GridTab { filter, label, count, rect });
        x += w + TAB_GAP;
    }
    let top = if tabs.is_empty() { full.min.y + 56.0 } else { tabs.last().map(|t| t.rect.max.y).unwrap_or(full.min.y + 42.0) + 10.0 };
    let bottom = (full.max.y - 44.0).max(top);
    let right = (right - 14.0).max(left);
    GridFrame { tabs, inner: Rect::from_min_max(pos2(left, top), pos2(right, bottom)) }
}

/// The grid's cells, with the scroll offset applied. Only cells that touch `inner` are returned:
/// a 300-slide deck lays out every slide but draws and tests only what is on screen.
pub(super) fn grid_layout(inner: Rect, slides: &[usize], aspect: f32, scroll: f32) -> GridLayout {
    let aspect = if aspect.is_finite() && aspect > 0.05 { aspect } else { 16.0 / 9.0 };
    if slides.is_empty() || inner.width() < 8.0 || inner.height() < 8.0 {
        return GridLayout { cells: vec![], content_h: 0.0, scroll: 0.0, max_scroll: 0.0 };
    }
    let cols = (((inner.width() + GRID_GAP) / (GRID_MIN_W + GRID_GAP)).floor() as usize).clamp(1, GRID_MAX_COLS).min(slides.len());
    let cell_w = ((inner.width() - GRID_GAP * (cols.saturating_sub(1)) as f32) / cols as f32).min(GRID_MAX_W);
    let grid_w = cell_w * cols as f32 + GRID_GAP * (cols.saturating_sub(1)) as f32;
    let x0 = inner.min.x + ((inner.width() - grid_w) * 0.5).max(0.0);
    let thumb_h = cell_w / aspect;
    let cell_h = thumb_h + GRID_LABEL_H;
    let rows = slides.len().div_ceil(cols);
    let content_h = (rows as f32 * (cell_h + GRID_GAP) - GRID_GAP).max(0.0);
    let max_scroll = (content_h - inner.height()).max(0.0);
    let scroll = if scroll.is_finite() { scroll.clamp(0.0, max_scroll) } else { 0.0 };
    // A short section sits in the middle of the window instead of clinging to the tabs.
    let y0 = if max_scroll <= 0.0 { inner.min.y + (inner.height() - content_h).max(0.0) * 0.5 } else { inner.min.y };
    let mut cells = Vec::new();
    for (n, slide) in slides.iter().copied().enumerate() {
        let c = n % cols;
        let r = n / cols;
        let rect =
            Rect::from_min_size(pos2(x0 + c as f32 * (cell_w + GRID_GAP), y0 + r as f32 * (cell_h + GRID_GAP) - scroll), vec2(cell_w, thumb_h));
        if rect.max.y + GRID_LABEL_H >= inner.min.y && rect.min.y <= inner.max.y {
            cells.push(GridCell { slide, rect });
        }
    }
    GridLayout { cells, content_h, scroll, max_scroll }
}

/// The category tabs at the top of the grid, if the deck has useful sections.
pub(super) fn grid_tab_hit(full: Rect, doc: &Presentation, p: Pos2) -> Option<GridFilter> {
    grid_frame(full, doc).tabs.into_iter().find(|t| t.rect.contains(p)).map(|t| t.filter)
}

/// The global slide index a click on a thumbnail goes to.
pub(super) fn grid_slide_hit(full: Rect, doc: &Presentation, filter: GridFilter, scroll: f32, p: Pos2) -> Option<usize> {
    let frame = grid_frame(full, doc);
    let slides = sections::grid_slides(doc, filter);
    grid_layout(frame.inner, &slides, slide_aspect(doc), scroll).cells.into_iter().find(|c| c.rect.expand(4.0).contains(p)).map(|c| c.slide)
}

fn grid_scrollbar(frame: &GridFrame) -> Rect {
    Rect::from_min_max(pos2(frame.inner.max.x + 6.0, frame.inner.min.y), pos2(frame.inner.max.x + 14.0, frame.inner.max.y))
}

/// Scroll the grid by `delta` points, clamped to its content.
pub(super) fn scroll_grid(show: &mut Show, delta: f32, full: Rect, doc: &Presentation) {
    let frame = grid_frame(full, doc);
    let slides = sections::grid_slides(doc, show.grid_filter);
    let layout = grid_layout(frame.inner, &slides, slide_aspect(doc), show.grid_scroll);
    if layout.max_scroll <= 0.0 || !delta.is_finite() {
        show.grid_scroll = 0.0;
        return;
    }
    show.grid_scroll = (layout.scroll + delta).clamp(0.0, layout.max_scroll);
}

fn grid_filter_value(filter: GridFilter) -> serde_json::Value {
    match filter {
        GridFilter::All => json!("all"),
        GridFilter::Section(i) => json!(i),
        GridFilter::Other => json!("other"),
    }
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
    let t = theme::Tokens::get(ctx);
    painter.rect_filled(full, CornerRadius::ZERO, Color32::from_gray(16));
    let frame = grid_frame(full, doc);
    let filter = show.grid_filter;
    let slides = sections::grid_slides(doc, filter);
    let layout = grid_layout(frame.inner, &slides, slide_aspect(doc), show.grid_scroll);
    show.grid_scroll = layout.scroll;

    painter.text(
        pos2(full.min.x + GRID_MARGIN, full.min.y + 22.0),
        Align2::LEFT_CENTER,
        sections::filter_title(doc, filter),
        theme::font(20.0),
        Color32::WHITE,
    );
    painter.text(
        pos2(full.max.x - GRID_MARGIN, full.min.y + 22.0),
        Align2::RIGHT_CENTER,
        if slides.len() == 1 { "1 slide".to_string() } else { format!("{} slides", slides.len()) },
        theme::font(13.0),
        Color32::from_gray(170),
    );

    for tab in &frame.tabs {
        let selected = tab.filter == filter;
        let bg = if selected { t.accent } else { Color32::from_gray(34) };
        let text = if selected { t.accent_text } else { Color32::from_gray(205) };
        painter.rect_filled(tab.rect, CornerRadius::same(7), bg);
        painter.rect_stroke(
            tab.rect,
            CornerRadius::same(7),
            Stroke::new(1.0, if selected { t.accent } else { Color32::from_gray(58) }),
            egui::StrokeKind::Inside,
        );
        let name = if tab.label.chars().count() > 24 { format!("{}…", tab.label.chars().take(23).collect::<String>()) } else { tab.label.clone() };
        painter.text(tab.rect.center(), Align2::CENTER_CENTER, format!("{name} ({})", tab.count), theme::font(13.0), text);
    }

    if slides.is_empty() {
        painter.text(frame.inner.center(), Align2::CENTER_CENTER, "No slides in this section.", theme::font(15.0), Color32::from_gray(170));
    }

    // Keep only the thumbnails the grid is showing: a 300-slide deck must not keep 300 textures.
    let visible: Vec<usize> = layout.cells.iter().map(|c| c.slide).collect();
    show.thumbs.retain(|(i, _)| visible.contains(i));
    // A scrolled row can start above the grid; clip it so it never paints over the title or tabs.
    let cells = painter.with_clip_rect(frame.inner);
    let budget = if layout.cells.len() <= 12 { 12 } else { GRID_BUDGET };
    let mut made = 0;
    for cell in &layout.cells {
        let i = cell.slide;
        let rect = cell.rect;
        cells.rect_filled(rect, CornerRadius::same(4), Color32::from_gray(32));
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
            cells.image(t.id(), rect, unit_uv(), Color32::WHITE);
        }
        let current = i == show.state.slide;
        cells.rect_stroke(
            rect,
            CornerRadius::same(4),
            Stroke::new(if current { 3.0 } else { 1.0 }, if current { Color32::WHITE } else { Color32::from_gray(70) }),
            egui::StrokeKind::Inside,
        );
        let hidden = doc.slides.get(i).is_some_and(|s| s.hidden);
        let label = if hidden { format!("{}  Hidden", i + 1) } else { format!("{}", i + 1) };
        cells.text(
            pos2(rect.center().x, rect.max.y + 12.0),
            Align2::CENTER_CENTER,
            label,
            theme::font(13.0),
            if current { Color32::WHITE } else { Color32::from_gray(180) },
        );
    }

    if layout.max_scroll > 0.0 {
        let bar = grid_scrollbar(&frame);
        painter.rect_filled(bar, CornerRadius::same(4), Color32::from_gray(30));
        let track = frame.inner.height();
        let thumb_h = (track * (track / layout.content_h.max(1.0))).clamp(18.0, track);
        let travel = (track - thumb_h).max(1.0);
        let top = frame.inner.min.y + (layout.scroll / layout.max_scroll).clamp(0.0, 1.0) * travel;
        let thumb = Rect::from_min_size(pos2(bar.min.x, top), vec2(bar.width(), thumb_h));
        painter.rect_filled(thumb, CornerRadius::same(4), Color32::from_gray(110));
    }

    painter.text(
        pos2(full.min.x + GRID_MARGIN, full.max.y - 24.0),
        Align2::LEFT_CENTER,
        "Wheel or Page Up/Down to scroll · click a slide · Esc to close",
        theme::font(12.0),
        Color32::from_gray(150),
    );
}
