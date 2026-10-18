//! Ink annotations during the slide show: a per-slide stroke store with tools and undo.
//!
//! Pure state, no egui: `show.rs` feeds pointer positions (normalized to the slide rect) and
//! paints the strokes. Kept annotations become document ink (`slide.annotate`) when the show
//! ends, so stroke width is stored in slide units (points) and stays resolution-independent.

use std::collections::BTreeMap;

use deckcraft_color::Rgba;

/// The annotation tool active during the show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShowTool {
    /// Plain mouse: clicks advance the show.
    #[default]
    Pointer,
    Pen,
    Highlighter,
    Eraser,
    Laser,
    /// Select ink strokes: click to select, drag to move, marquee on empty space.
    Select,
}

/// Default pen width in slide units (points).
pub const PEN_WIDTH: f64 = 2.5;
/// Default highlighter width in slide units (points).
pub const HIGHLIGHTER_WIDTH: f64 = 14.0;
/// Highlighter ink is translucent so the slide shows through.
pub const HIGHLIGHTER_ALPHA: u8 = 110;

/// The width ladder (slide units / points) that `[` and `]` step through.
pub const WIDTHS: [f64; 8] = [0.75, 1.5, 2.5, 4.0, 6.0, 9.0, 14.0, 20.0];
/// The widths offered as quick-pick dots in the toolbar for the pen.
pub const WIDTH_DOTS: [f64; 4] = [1.5, 2.5, 4.0, 9.0];
/// The highlighter's quick-pick dots (translucent ink below ~6pt is nearly invisible).
pub const WIDTH_DOTS_HL: [f64; 4] = [6.0, 9.0, 14.0, 20.0];
/// The highlighter never goes thinner than this.
pub const HIGHLIGHTER_MIN_WIDTH: f64 = 6.0;

/// The ink color palette offered during the show (our own picks; the highlighter defaults
/// to Yellow).
pub const PALETTE: [(&str, Rgba); 8] = [
    ("Red", Rgba::rgb(0xE5, 0x39, 0x35)),
    ("Orange", Rgba::rgb(0xFB, 0x8C, 0x00)),
    ("Yellow", Rgba::rgb(0xFF, 0xE0, 0x00)),
    ("Green", Rgba::rgb(0x43, 0xA0, 0x47)),
    ("Blue", Rgba::rgb(0x1E, 0x88, 0xE5)),
    ("Purple", Rgba::rgb(0x8E, 0x24, 0xAA)),
    ("Black", Rgba::rgb(0x00, 0x00, 0x00)),
    ("White", Rgba::rgb(0xFF, 0xFF, 0xFF)),
];

/// One stroke: points normalized to the slide rect (0..=1 while over the slide; the pointer
/// may leave that range onto the letterbox bars — values are clamped when converted to
/// document ink). Width is in slide units (points).
#[derive(Clone, Debug, PartialEq)]
pub struct ShowStroke {
    /// Unique within a show (drives undo and eraser bookkeeping).
    pub id: u64,
    pub points: Vec<(f32, f32)>,
    pub color: Rgba,
    pub width: f64,
    pub highlighter: bool,
}

/// Points per stroke segment; a longer draw continues in a fresh segment (with the same
/// style) so one stroke cannot grow without bound.
const MAX_POINTS: usize = 8192;

impl ShowTool {
    /// Lowercase name (for `ui.inspect` and logs).
    pub fn name(self) -> &'static str {
        match self {
            ShowTool::Pointer => "pointer",
            ShowTool::Pen => "pen",
            ShowTool::Highlighter => "highlighter",
            ShowTool::Eraser => "eraser",
            ShowTool::Laser => "laser",
            ShowTool::Select => "select",
        }
    }
}

/// One undoable ink action (the journal, oldest first).
#[derive(Clone, Debug)]
enum Op {
    /// A stroke was drawn (slide, stroke id).
    Draw(usize, u64),
    /// Strokes were created by duplicate/paste (slide, stroke ids).
    Added(usize, Vec<u64>),
    /// Strokes were erased, deleted or cleared (slide, (position in the slide's stroke list,
    /// stroke), positions ascending; each restores at `min(pos, len)`).
    Erased(usize, Vec<(usize, ShowStroke)>),
    /// Strokes were moved by (dx, dy) in normalized slide coordinates; undo subtracts the
    /// accumulated total. The round trip through per-frame f32 deltas can drift the restore
    /// by sub-point ulps — invisible, and not cumulative.
    Moved(usize, Vec<u64>, f32, f32),
}

/// Ink state for one running show. Strokes are kept per slide, so they survive navigation.
#[derive(Clone, Debug)]
pub struct ShowInk {
    tool: ShowTool,
    strokes: BTreeMap<usize, Vec<ShowStroke>>,
    journal: Vec<Op>,
    next_id: u64,
    /// The stroke still being drawn (pointer down); `push` writes only to that stroke, so
    /// undo, erasing or a slide change mid-drag cannot corrupt finished strokes.
    active: Option<(usize, u64)>,
    /// Inside one erase gesture (button held): consecutive `erase_at` calls merge into a
    /// single journal entry, so one sweep is one undo.
    erase_open: bool,
    /// Selected stroke ids on `select_slide`, worked on by the select tool.
    selected: Vec<u64>,
    select_slide: usize,
    /// Ink clipboard for copy/paste (strokes keep their styles; ids are re-minted on paste).
    clipboard: Vec<ShowStroke>,
    /// Live move gesture: (slide, ids, accumulated dx, dy) between `begin_move` and
    /// `end_move`. Anchored to the slide where the drag started, like the eraser.
    move_open: Option<(usize, Vec<u64>, f32, f32)>,
    pub pen_color: Rgba,
    pub highlighter_color: Rgba,
    pub pen_width: f64,
    pub highlighter_width: f64,
}

/// Points closer than this (normalized) merge into one, so a stationary pointer does not
/// flood the stroke.
const EPS: f32 = 0.0006;

/// Subdivisions per quadratic segment when flattening a stroke for painting/keeping.
const SMOOTH_SUBDIV: usize = 4;

/// Offset applied to duplicated/pasted strokes (normalized; ≈ 14 pt on a 16:9 slide).
const DUP_OFFSET: (f32, f32) = (0.015, 0.015);

/// Flatten stroke points into a smooth polyline: quadratic Béziers through the segment
/// midpoints with the input points as controls (classic pen smoothing). Endpoints are
/// preserved; 0–2 points pass through unchanged.
pub fn smooth_stroke(pts: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let Some(&first) = pts.first() else { return vec![] };
    if pts.len() <= 2 {
        return pts.to_vec();
    }
    let n = pts.len();
    let mut out = Vec::with_capacity((n - 1) * SMOOTH_SUBDIV + 1);
    out.push(first);
    let mut start = first;
    for (i, w) in pts.windows(2).enumerate() {
        let &[c, next] = w else { continue };
        // Segment end: the midpoint, except for the final leg which lands on the last point.
        let end = if i + 2 == n { next } else { ((c.0 + next.0) * 0.5, (c.1 + next.1) * 0.5) };
        for step in 1..=SMOOTH_SUBDIV {
            let t = step as f32 / SMOOTH_SUBDIV as f32;
            let u = 1.0 - t;
            out.push((u * u * start.0 + 2.0 * u * t * c.0 + t * t * end.0, u * u * start.1 + 2.0 * u * t * c.1 + t * t * end.1));
        }
        start = end;
    }
    out
}

impl ShowInk {
    pub fn new(pen_color: Rgba) -> Self {
        ShowInk {
            tool: ShowTool::Pointer,
            strokes: BTreeMap::new(),
            journal: vec![],
            next_id: 0,
            active: None,
            erase_open: false,
            selected: vec![],
            select_slide: 0,
            clipboard: vec![],
            move_open: None,
            pen_color,
            highlighter_color: Rgba::rgb(0xFF, 0xE0, 0x00),
            pen_width: PEN_WIDTH,
            highlighter_width: HIGHLIGHTER_WIDTH,
        }
    }

    /// The active annotation tool.
    pub fn tool(&self) -> ShowTool {
        self.tool
    }

    /// Select a tool; toggling the active pen/highlighter off returns to the pointer.
    pub fn toggle(&mut self, tool: ShowTool) {
        self.set_tool(if self.tool == tool { ShowTool::Pointer } else { tool });
    }

    /// Switch tools; any open stroke, erase gesture or move drag ends (a stroke belongs to
    /// one tool). Switching away from Select drops the selection.
    pub fn set_tool(&mut self, tool: ShowTool) {
        self.end();
        self.end_erase();
        self.end_move();
        if tool != ShowTool::Select {
            self.selected.clear();
        }
        self.tool = tool;
    }

    /// Begin an erase gesture (eraser button pressed): removals until `end_erase` merge into
    /// one undo step.
    pub fn start_erase(&mut self) {
        self.erase_open = true;
    }

    /// End the erase gesture (button released, slide changed, tool switched).
    pub fn end_erase(&mut self) {
        self.erase_open = false;
    }

    pub fn strokes(&self, slide: usize) -> &[ShowStroke] {
        self.strokes.get(&slide).map(Vec::as_slice).unwrap_or(&[])
    }

    /// The color the active pen/highlighter draws with (the pen color for other tools).
    pub fn tool_color(&self) -> Rgba {
        match self.tool {
            ShowTool::Highlighter => self.highlighter_color,
            _ => self.pen_color,
        }
    }

    /// Set the active pen/highlighter's color (a no-op for the other tools).
    pub fn set_color(&mut self, color: Rgba) {
        match self.tool {
            ShowTool::Highlighter => self.highlighter_color = color,
            ShowTool::Pen => self.pen_color = color,
            _ => {}
        }
    }

    /// Cycle the active pen/highlighter through the palette (`C` in the show); a color
    /// outside the palette (e.g. the document's pen color) jumps to the first entry.
    pub fn cycle_color(&mut self) {
        let cur = self.tool_color();
        let next = PALETTE.iter().position(|&(_, c)| c == cur).map_or(PALETTE[0].1, |i| PALETTE[(i + 1) % PALETTE.len()].1);
        self.set_color(next);
    }

    /// The width the active pen/highlighter draws with (the pen width for other tools).
    pub fn tool_width(&self) -> f64 {
        match self.tool {
            ShowTool::Highlighter => self.highlighter_width,
            _ => self.pen_width,
        }
    }

    /// Set the active pen/highlighter's width (a no-op for the other tools). The highlighter
    /// is floored at [`HIGHLIGHTER_MIN_WIDTH`]: translucent ink thinner than that is nearly
    /// invisible.
    pub fn set_width(&mut self, width: f64) {
        match self.tool {
            ShowTool::Highlighter => self.highlighter_width = width.max(HIGHLIGHTER_MIN_WIDTH),
            ShowTool::Pen => self.pen_width = width,
            _ => {}
        }
    }

    /// The toolbar's quick-pick widths for the active tool.
    pub fn width_dots(&self) -> &'static [f64; 4] {
        match self.tool {
            ShowTool::Highlighter => &WIDTH_DOTS_HL,
            _ => &WIDTH_DOTS,
        }
    }

    /// Step the active pen/highlighter along the width ladder (`[` / `]` in the show); a
    /// width outside the ladder moves to the nearest rung in that direction. Clamps at the
    /// ends.
    pub fn step_width(&mut self, up: bool) {
        let cur = self.tool_width();
        let next = if up {
            WIDTHS.iter().copied().find(|&w| w > cur + 0.001).unwrap_or(cur)
        } else {
            WIDTHS.iter().copied().rev().find(|&w| w < cur - 0.001).unwrap_or(cur)
        };
        self.set_width(next);
    }

    /// Any ink anywhere in the show (drives the keep-on-exit prompt).
    pub fn any(&self) -> bool {
        self.strokes.values().any(|v| !v.is_empty())
    }

    /// Slides with ink, in slide order: (slide index, strokes). Drives the keep-on-exit
    /// conversion (`slide.annotate`).
    pub fn by_slide(&self) -> impl Iterator<Item = (usize, &[ShowStroke])> {
        self.strokes.iter().map(|(i, v)| (*i, v.as_slice()))
    }

    /// The total number of strokes (all slides).
    pub fn count(&self) -> usize {
        self.strokes.values().map(Vec::len).sum()
    }

    /// Start a stroke with the current pen/highlighter settings. Ends any stroke still open.
    pub fn begin(&mut self, slide: usize, x: f32, y: f32) {
        let (color, width, highlighter) = match self.tool {
            ShowTool::Highlighter => (self.highlighter_color.with_alpha(HIGHLIGHTER_ALPHA), self.highlighter_width, true),
            _ => (self.pen_color, self.pen_width, false),
        };
        let id = self.next_id;
        self.next_id += 1;
        self.strokes.entry(slide).or_default().push(ShowStroke { id, points: vec![(x, y)], color, width, highlighter });
        self.journal.push(Op::Draw(slide, id));
        self.active = Some((slide, id));
    }

    /// Extend the open stroke; points that barely moved are dropped. Writes only to the open
    /// stroke: after `undo`/`end`/`clear_slide` mid-drag this is a no-op until the next
    /// `begin`.
    pub fn push(&mut self, slide: usize, x: f32, y: f32) {
        let Some((_, id)) = self.active.filter(|(s, _)| *s == slide) else { return };
        let Some(v) = self.strokes.get_mut(&slide) else {
            self.active = None;
            return;
        };
        let Some(pos) = v.iter().position(|s| s.id == id) else {
            self.active = None;
            return;
        };
        let stroke = &mut v[pos];
        if stroke.points.last().is_some_and(|&(px, py)| (px - x).abs() < EPS && (py - y).abs() < EPS) {
            return;
        }
        stroke.points.push((x, y));
        if stroke.points.len() >= MAX_POINTS {
            let (color, width, highlighter) = (stroke.color, stroke.width, stroke.highlighter);
            let new_id = self.next_id;
            self.next_id += 1;
            v.push(ShowStroke { id: new_id, points: vec![(x, y)], color, width, highlighter });
            self.journal.push(Op::Draw(slide, new_id));
            self.active = Some((slide, new_id));
        }
    }

    /// A stroke is open (pointer still down).
    pub fn open(&self) -> bool {
        self.active.is_some()
    }

    /// Close the open stroke (pointer released, slide changed, show ended).
    pub fn end(&mut self) {
        self.active = None;
    }

    /// True when the journal holds at least one entry `undo()` would act on (empty erasure
    /// sweeps don't count — undo skips them). An in-flight move drag counts: `undo()` commits
    /// and reverts it.
    pub fn can_undo(&self) -> bool {
        if self.move_open.as_ref().is_some_and(|(_, _, tx, ty)| *tx != 0.0 || *ty != 0.0) {
            return true;
        }
        self.journal.iter().rev().any(|op| match op {
            Op::Draw(..) | Op::Added(..) | Op::Moved(..) => true,
            Op::Erased(_, strokes) => !strokes.is_empty(),
        })
    }

    /// Undo the most recent ink action — a drawn stroke is removed, an erasure or clear is
    /// restored (on any slide), a move is reverted, a duplicate/paste vanishes. Returns false
    /// when there is nothing to undo.
    pub fn undo(&mut self) -> bool {
        self.active = None;
        // An in-flight move is committed first so the pop below reverts it (not the op
        // under it) and nothing applied stays unjournaled.
        self.end_move();
        let done = self.undo_step();
        if done {
            self.prune_selection();
        }
        done
    }

    fn undo_step(&mut self) -> bool {
        while let Some(op) = self.journal.pop() {
            match op {
                Op::Draw(slide, id) => {
                    if let Some(v) = self.strokes.get_mut(&slide)
                        && let Some(pos) = v.iter().position(|s| s.id == id)
                    {
                        v.remove(pos);
                        if v.is_empty() {
                            self.strokes.remove(&slide);
                        }
                        return true;
                    }
                }
                Op::Added(slide, ids) => {
                    if let Some(v) = self.strokes.get_mut(&slide) {
                        let before = v.len();
                        v.retain(|s| !ids.contains(&s.id));
                        if v.len() != before {
                            if v.is_empty() {
                                self.strokes.remove(&slide);
                            }
                            return true;
                        }
                    }
                }
                Op::Erased(slide, strokes) => {
                    if strokes.is_empty() {
                        continue;
                    }
                    let v = self.strokes.entry(slide).or_default();
                    for (pos, s) in strokes {
                        v.insert(pos.min(v.len()), s);
                    }
                    return true;
                }
                Op::Moved(slide, ids, dx, dy) => {
                    if let Some(v) = self.strokes.get_mut(&slide) {
                        let mut any = false;
                        for s in v.iter_mut().filter(|s| ids.contains(&s.id)) {
                            for p in &mut s.points {
                                p.0 -= dx;
                                p.1 -= dy;
                            }
                            any = true;
                        }
                        if any {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// Drop selected ids whose strokes no longer exist (after undo/erase/clear).
    fn prune_selection(&mut self) {
        if self.selected.is_empty() {
            return;
        }
        let slide = self.select_slide;
        let alive: Vec<u64> = self.strokes.get(&slide).map(|v| v.iter().map(|s| s.id).collect()).unwrap_or_default();
        self.selected.retain(|id| alive.contains(id));
    }

    /// Erase whole strokes near a point given in normalized slide coordinates. `tol` is the
    /// pointer tolerance in slide units (points); a stroke counts as hit when the point comes
    /// within `tol + width/2` of its polyline. The hit test runs on the smoothed curve in
    /// slide coordinates, so it is isotropic and matches the painted geometry. Erased strokes
    /// are undoable. Returns how many strokes were erased.
    pub fn erase_at(&mut self, slide: usize, x: f32, y: f32, tol: f32, slide_w: f64, slide_h: f64) -> usize {
        let Some(v) = self.strokes.get_mut(&slide) else { return 0 };
        let (sw, sh) = (slide_w.max(1.0), slide_h.max(1.0));
        let (px, py) = (f64::from(x) * sw, f64::from(y) * sh);
        let hit = |s: &ShowStroke| stroke_hit(s, px, py, tol, sw, sh);
        let mut removed: Vec<(usize, ShowStroke)> = vec![];
        let mut i = 0;
        v.retain(|s| {
            let keep = !hit(s);
            if !keep {
                removed.push((i, s.clone()));
            }
            i += 1;
            keep
        });
        if removed.is_empty() {
            return 0;
        }
        if v.is_empty() {
            self.strokes.remove(&slide);
        }
        if self.active.is_some_and(|(_, id)| removed.iter().any(|(_, s)| s.id == id)) {
            self.active = None;
        }
        self.prune_selection();
        let n = removed.len();
        // One gesture = one undo step: merge into the open gesture's journal entry,
        // converting the just-removed positions (against the thinned list) back to positions
        // in the list as it was when the gesture started.
        if self.erase_open
            && let Some(Op::Erased(s, prior)) = self.journal.last_mut()
            && *s == slide
        {
            // Convert against the gesture's earlier removals only; strokes removed in this
            // same call share the pre-call list and must not shift each other.
            let prior_positions: Vec<usize> = prior.iter().map(|(p, _)| *p).collect();
            for (pos, stroke) in removed {
                let mut orig = pos;
                loop {
                    let shifted = pos + prior_positions.iter().filter(|rp| **rp <= orig).count();
                    if shifted == orig {
                        break;
                    }
                    orig = shifted;
                }
                prior.push((orig, stroke));
            }
            prior.sort_by_key(|(p, _)| *p);
            return n;
        }
        self.journal.push(Op::Erased(slide, removed));
        n
    }

    /// Clear one slide's ink (`E` in the show). Undoable.
    pub fn clear_slide(&mut self, slide: usize) {
        self.end_move();
        if self.active.is_some_and(|(s, _)| s == slide) {
            self.active = None;
        }
        if let Some(v) = self.strokes.remove(&slide)
            && !v.is_empty()
        {
            self.journal.push(Op::Erased(slide, v.into_iter().enumerate().collect()));
        }
        self.prune_selection();
    }

    /// The selected stroke ids (empty when nothing is selected).
    pub fn selection(&self) -> &[u64] {
        &self.selected
    }

    /// Number of selected strokes (for `ui.inspect`).
    pub fn selection_len(&self) -> usize {
        self.selected.len()
    }

    /// True when the stroke is in the selection.
    pub fn is_selected(&self, id: u64) -> bool {
        self.selected.contains(&id)
    }

    /// Forget the selection (tool switch away, explicit deselect).
    pub fn clear_selection(&mut self) {
        self.selected.clear();
    }

    /// Keep the selection tied to the shown slide: navigating away ends any move drag
    /// (journaled against its own slide) and drops the selection.
    pub fn sync_slide(&mut self, slide: usize) {
        if slide != self.select_slide {
            self.end_move();
            self.selected.clear();
            self.select_slide = slide;
        }
    }

    /// The topmost stroke under the point (normalized slide coordinates), using the eraser's
    /// hit rule: within `tol + width/2` of the smoothed polyline, in slide units.
    pub fn hit_test(&self, slide: usize, x: f32, y: f32, tol: f32, slide_w: f64, slide_h: f64) -> Option<u64> {
        let v = self.strokes.get(&slide)?;
        let (sw, sh) = (slide_w.max(1.0), slide_h.max(1.0));
        let (px, py) = (f64::from(x) * sw, f64::from(y) * sh);
        v.iter().rev().find(|s| stroke_hit(s, px, py, tol, sw, sh)).map(|s| s.id)
    }

    /// Replace the selection with one stroke.
    pub fn select_only(&mut self, slide: usize, id: u64) {
        self.set_select_slide(slide);
        self.selected = vec![id];
    }

    /// Shift-click: add the stroke to the selection, or remove it when already selected.
    pub fn toggle_selected(&mut self, slide: usize, id: u64) {
        self.set_select_slide(slide);
        match self.selected.iter().position(|s| *s == id) {
            Some(pos) => {
                self.selected.remove(pos);
            }
            None => self.selected.push(id),
        }
    }

    /// Marquee-select strokes whose point bounding box intersects the rect (normalized slide
    /// coordinates, corners in any order); `add` unions with the current selection instead of
    /// replacing it. Returns the size of the resulting selection.
    pub fn select_in_rect(&mut self, slide: usize, a: (f32, f32), b: (f32, f32), add: bool) -> usize {
        self.set_select_slide(slide);
        let (min, max) = ((a.0.min(b.0), a.1.min(b.1)), (a.0.max(b.0), a.1.max(b.1)));
        let intersects = |s: &ShowStroke| {
            let mut it = s.points.iter();
            let Some(&(fx, fy)) = it.next() else { return false };
            let (mut x0, mut y0, mut x1, mut y1) = (fx, fy, fx, fy);
            for &(x, y) in it {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
            x0 <= max.0 && x1 >= min.0 && y0 <= max.1 && y1 >= min.1
        };
        let hit: Vec<u64> = self.strokes.get(&slide).map(|v| v.iter().filter(|s| intersects(s)).map(|s| s.id).collect()).unwrap_or_default();
        if add {
            for id in hit {
                if !self.selected.contains(&id) {
                    self.selected.push(id);
                }
            }
        } else {
            self.selected = hit;
        }
        self.selected.len()
    }

    /// Selections live on one slide at a time; a select on another slide starts fresh.
    fn set_select_slide(&mut self, slide: usize) {
        if slide != self.select_slide {
            self.selected.clear();
            self.select_slide = slide;
        }
    }

    /// Bounding box of the selection on its slide, normalized coordinates (for the overlay).
    pub fn selected_bounds(&self, slide: usize) -> Option<((f32, f32), (f32, f32))> {
        if slide != self.select_slide || self.selected.is_empty() {
            return None;
        }
        let v = self.strokes.get(&slide)?;
        let (mut x0, mut y0, mut x1, mut y1) = (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
        let mut any = false;
        for s in v.iter().filter(|s| self.selected.contains(&s.id)) {
            for &(x, y) in &s.points {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
            any = true;
        }
        any.then_some(((x0, y0), (x1, y1)))
    }

    /// Begin a move drag for the current selection; deltas accumulate until `end_move`, which
    /// journals one undoable entry for the whole gesture. The drag stays anchored to the
    /// slide where it began, even if the show navigates away mid-drag.
    pub fn begin_move(&mut self) {
        self.move_open = (!self.selected.is_empty()).then(|| (self.select_slide, self.selected.clone(), 0.0, 0.0));
    }

    /// Translate the strokes being moved by (dx, dy) (normalized). The store moves live; the
    /// journal entry is written by `end_move`. Deltas that hit no stroke (e.g. the gesture's
    /// strokes are gone) are not accumulated, so the journaled total always matches what was
    /// actually applied.
    pub fn move_selection_by(&mut self, dx: f32, dy: f32) {
        if !dx.is_finite() || !dy.is_finite() {
            return;
        }
        let Some((slide, ids, tx, ty)) = self.move_open.as_mut() else { return };
        let applied = match self.strokes.get_mut(slide) {
            Some(v) => {
                let mut applied = false;
                for s in v.iter_mut().filter(|s| ids.contains(&s.id)) {
                    for p in &mut s.points {
                        p.0 += dx;
                        p.1 += dy;
                    }
                    applied = true;
                }
                applied
            }
            None => false,
        };
        if applied {
            *tx += dx;
            *ty += dy;
        }
    }

    /// End the move drag: journal the accumulated delta (against the slide where the drag
    /// started) as one undo step. Returns false when nothing moved.
    pub fn end_move(&mut self) -> bool {
        let Some((slide, ids, tx, ty)) = self.move_open.take() else { return false };
        if ids.is_empty() || !tx.is_finite() || !ty.is_finite() || (tx == 0.0 && ty == 0.0) {
            return false;
        }
        self.journal.push(Op::Moved(slide, ids, tx, ty));
        true
    }

    /// Duplicate the selection at a small offset; the copies become the new selection.
    /// Returns how many strokes were created.
    pub fn duplicate_selection(&mut self) -> usize {
        self.end_move();
        let slide = self.select_slide;
        let src: Vec<ShowStroke> =
            self.strokes.get(&slide).map(|v| v.iter().filter(|s| self.selected.contains(&s.id)).cloned().collect()).unwrap_or_default();
        self.add_copies(slide, src)
    }

    /// Copy the selection to the ink clipboard. Returns how many strokes were copied.
    pub fn copy_selection(&mut self) -> usize {
        let slide = self.select_slide;
        self.clipboard = self.strokes.get(&slide).map(|v| v.iter().filter(|s| self.selected.contains(&s.id)).cloned().collect()).unwrap_or_default();
        self.clipboard.len()
    }

    /// Paste the ink clipboard onto `slide` at the duplicate offset; the pasted strokes become
    /// the selection. Returns how many strokes were pasted.
    pub fn paste(&mut self, slide: usize) -> usize {
        // A no-op paste must not touch the selection.
        if self.clipboard.is_empty() {
            return 0;
        }
        self.end_move();
        let src = self.clipboard.clone();
        self.select_slide = slide;
        self.selected.clear();
        self.add_copies(slide, src)
    }

    /// Clone strokes onto a slide with fresh ids and the duplicate offset; the copies become
    /// the selection and one `Added` journal entry. Returns how many were added.
    fn add_copies(&mut self, slide: usize, src: Vec<ShowStroke>) -> usize {
        if src.is_empty() {
            return 0;
        }
        let v = self.strokes.entry(slide).or_default();
        let mut ids = Vec::with_capacity(src.len());
        for mut c in src {
            c.id = self.next_id;
            self.next_id += 1;
            for p in &mut c.points {
                p.0 += DUP_OFFSET.0;
                p.1 += DUP_OFFSET.1;
            }
            ids.push(c.id);
            v.push(c);
        }
        self.journal.push(Op::Added(slide, ids.clone()));
        self.select_slide = slide;
        self.selected = ids.clone();
        ids.len()
    }

    /// Delete the selected strokes (undoable). Returns how many were deleted.
    pub fn delete_selection(&mut self) -> usize {
        // Journal any in-flight move before the deletion so undo restores in order.
        self.end_move();
        let slide = self.select_slide;
        let sel = std::mem::take(&mut self.selected);
        let Some(v) = self.strokes.get_mut(&slide) else { return 0 };
        let mut removed: Vec<(usize, ShowStroke)> = vec![];
        let mut i = 0;
        v.retain(|s| {
            let keep = !sel.contains(&s.id);
            if !keep {
                removed.push((i, s.clone()));
            }
            i += 1;
            keep
        });
        if removed.is_empty() {
            return 0;
        }
        if v.is_empty() {
            self.strokes.remove(&slide);
        }
        if self.active.is_some_and(|(_, id)| removed.iter().any(|(_, s)| s.id == id)) {
            self.active = None;
        }
        let n = removed.len();
        self.journal.push(Op::Erased(slide, removed));
        n
    }
}

/// Distance from point (px,py) to the segment (ax,ay)-(bx,by).
fn seg_dist(px: f64, py: f64, ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 { (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
    let (cx, cy) = (ax + t * dx, ay + t * dy);
    ((px - cx).powi(2) + (py - cy).powi(2)).sqrt()
}

/// The eraser/select hit rule: the point (slide units) comes within `tol + width/2` of the
/// stroke's smoothed polyline, so hits match the painted geometry.
fn stroke_hit(s: &ShowStroke, px: f64, py: f64, tol: f32, sw: f64, sh: f64) -> bool {
    let t = f64::from(tol) + s.width / 2.0;
    match s.points.as_slice() {
        [] => false,
        [(sx, sy)] => ((f64::from(*sx) * sw - px).powi(2) + (f64::from(*sy) * sh - py).powi(2)).sqrt() <= t,
        _ => {
            let sm = smooth_stroke(&s.points);
            sm.windows(2)
                .any(|w| seg_dist(px, py, f64::from(w[0].0) * sw, f64::from(w[0].1) * sh, f64::from(w[1].0) * sw, f64::from(w[1].1) * sh) <= t)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ink() -> ShowInk {
        ShowInk::new(Rgba::rgb(0xFF, 0x00, 0x00))
    }

    #[test]
    fn can_undo_tracks_the_journal() {
        let mut ink = ink();
        assert!(!ink.can_undo());
        ink.set_tool(ShowTool::Pen);
        ink.begin(0, 0.1, 0.1);
        ink.push(0, 0.2, 0.2);
        ink.end();
        assert!(ink.can_undo());
        // An erasure that hits nothing journals nothing.
        assert_eq!(ink.erase_at(0, 0.9, 0.9, 1.0, 100.0, 100.0), 0);
        assert!(ink.can_undo());
        // Clearing a slide with no ink journals nothing.
        ink.clear_slide(3);
        assert!(ink.can_undo());
        assert!(ink.undo());
        assert!(!ink.can_undo());
        assert!(!ink.undo());
    }

    #[test]
    fn strokes_are_kept_per_slide() {
        let mut ink = ink();
        ink.set_tool(ShowTool::Pen);
        ink.begin(0, 0.1, 0.1);
        ink.push(0, 0.2, 0.2);
        ink.begin(2, 0.5, 0.5);
        assert_eq!(ink.strokes(0).len(), 1);
        assert_eq!(ink.strokes(1).len(), 0);
        assert_eq!(ink.strokes(2).len(), 1);
        assert_eq!(ink.count(), 2);
        assert!(ink.any());
    }

    #[test]
    fn stroke_uses_current_tool_settings() {
        let mut ink = ink();
        ink.pen_color = Rgba::rgb(0x00, 0x00, 0xFF);
        ink.pen_width = 4.0;
        ink.set_tool(ShowTool::Pen);
        ink.begin(0, 0.1, 0.1);
        assert_eq!(ink.strokes(0)[0].color, Rgba::rgb(0x00, 0x00, 0xFF));
        assert_eq!(ink.strokes(0)[0].width, 4.0);
        assert!(!ink.strokes(0)[0].highlighter);
        ink.set_tool(ShowTool::Highlighter);
        ink.begin(0, 0.2, 0.2);
        let hl = &ink.strokes(0)[1];
        assert!(hl.highlighter);
        assert_eq!(hl.width, HIGHLIGHTER_WIDTH);
        assert_eq!(hl.color.a, HIGHLIGHTER_ALPHA);
    }

    #[test]
    fn toggle_selects_and_deselects() {
        let mut ink = ink();
        ink.toggle(ShowTool::Pen);
        assert_eq!(ink.tool(), ShowTool::Pen);
        ink.toggle(ShowTool::Highlighter);
        assert_eq!(ink.tool(), ShowTool::Highlighter);
        ink.toggle(ShowTool::Highlighter);
        assert_eq!(ink.tool(), ShowTool::Pointer);
    }

    #[test]
    fn push_merges_stationary_points() {
        let mut ink = ink();
        ink.set_tool(ShowTool::Pen);
        ink.begin(0, 0.5, 0.5);
        ink.push(0, 0.5 + EPS / 2.0, 0.5);
        assert_eq!(ink.strokes(0)[0].points.len(), 1);
        ink.push(0, 0.6, 0.6);
        assert_eq!(ink.strokes(0)[0].points.len(), 2);
    }

    #[test]
    fn push_without_begin_is_a_no_op() {
        let mut ink = ink();
        ink.push(0, 0.5, 0.5);
        assert_eq!(ink.count(), 0);
    }

    #[test]
    fn undo_removes_the_latest_stroke_across_slides() {
        let mut ink = ink();
        ink.set_tool(ShowTool::Pen);
        ink.begin(0, 0.1, 0.1);
        ink.begin(1, 0.2, 0.2);
        assert!(ink.undo());
        assert_eq!(ink.strokes(0).len(), 1);
        assert_eq!(ink.strokes(1).len(), 0);
        assert!(ink.undo());
        assert!(!ink.any());
        assert!(!ink.undo());
    }

    #[test]
    fn clear_slide_is_undoable_and_drops_only_that_slide() {
        let mut ink = ink();
        ink.set_tool(ShowTool::Pen);
        ink.begin(0, 0.1, 0.1);
        ink.begin(1, 0.2, 0.2);
        ink.begin(0, 0.3, 0.3);
        ink.clear_slide(0);
        assert_eq!(ink.strokes(0).len(), 0);
        assert_eq!(ink.count(), 1);
        // Undo restores the cleared strokes in their original order.
        assert!(ink.undo());
        assert_eq!(ink.strokes(0).len(), 2);
        assert_eq!(ink.strokes(0)[0].points[0], (0.1, 0.1));
        assert_eq!(ink.strokes(0)[1].points[0], (0.3, 0.3));
        // Further undos walk back the draws.
        assert!(ink.undo());
        assert_eq!(ink.strokes(0).len(), 1);
        assert!(ink.undo());
        assert!(ink.strokes(1).is_empty()); // slide 1's stroke undone
        assert!(ink.undo());
        assert!(!ink.any());
        assert!(!ink.undo());
    }

    #[test]
    fn undo_mid_drag_does_not_corrupt_the_previous_stroke() {
        let mut ink = ink();
        ink.set_tool(ShowTool::Pen);
        ink.begin(0, 0.1, 0.1);
        ink.push(0, 0.15, 0.15);
        ink.end();
        ink.begin(0, 0.5, 0.5);
        assert!(ink.undo());
        // Still holding the pointer: pushes must not extend the finished stroke.
        ink.push(0, 0.9, 0.9);
        assert_eq!(ink.strokes(0)[0].points, vec![(0.1, 0.1), (0.15, 0.15)]);
        assert!(!ink.open());
    }

    #[test]
    fn slide_change_mid_drag_ends_the_stroke() {
        let mut ink = ink();
        ink.set_tool(ShowTool::Pen);
        ink.begin(0, 0.1, 0.1);
        ink.push(0, 0.2, 0.2);
        ink.end(); // the show moved to another slide
        ink.push(1, 0.5, 0.5);
        ink.push(0, 0.9, 0.9);
        assert_eq!(ink.strokes(0)[0].points.len(), 2);
        assert_eq!(ink.strokes(1).len(), 0);
    }

    #[test]
    fn clear_slide_mid_drag_stops_the_open_stroke() {
        let mut ink = ink();
        ink.set_tool(ShowTool::Pen);
        ink.begin(0, 0.1, 0.1);
        ink.clear_slide(0);
        ink.push(0, 0.5, 0.5);
        assert!(!ink.any());
    }

    #[test]
    fn long_draws_continue_in_bounded_segments() {
        let mut ink = ink();
        ink.set_tool(ShowTool::Pen);
        ink.begin(0, 0.0, 0.0);
        for i in 1..=MAX_POINTS * 2 {
            ink.push(0, (i as f32 * 0.001).fract(), 0.5);
        }
        let strokes = ink.strokes(0);
        assert!(strokes.len() >= 2);
        assert!(strokes.iter().all(|s| s.points.len() <= MAX_POINTS));
        // Segments chain: each starts where the previous ended.
        for w in strokes.windows(2) {
            let (a, b) = (&w[0], &w[1]);
            assert_eq!(a.points.last(), b.points.first());
            assert_eq!((a.color, a.width, a.highlighter), (b.color, b.width, b.highlighter));
        }
    }

    #[test]
    fn palette_cycle_wraps_and_remembers_per_tool() {
        let mut ink = ink();
        ink.set_tool(ShowTool::Pen);
        ink.pen_color = PALETTE[0].1;
        ink.cycle_color();
        assert_eq!(ink.pen_color, PALETTE[1].1);
        // The highlighter keeps its own color.
        assert_eq!(ink.highlighter_color, PALETTE[2].1);
        ink.set_tool(ShowTool::Highlighter);
        ink.cycle_color();
        assert_eq!(ink.highlighter_color, PALETTE[3].1);
        assert_eq!(ink.pen_color, PALETTE[1].1);
        // Wrap around the end of the palette.
        ink.pen_color = PALETTE[7].1;
        ink.set_tool(ShowTool::Pen);
        ink.cycle_color();
        assert_eq!(ink.pen_color, PALETTE[0].1);
    }

    #[test]
    fn palette_cycle_from_a_custom_color_starts_at_the_top() {
        let mut ink = ink();
        ink.set_tool(ShowTool::Pen);
        ink.pen_color = Rgba::rgb(0x12, 0x34, 0x56);
        ink.cycle_color();
        assert_eq!(ink.pen_color, PALETTE[0].1);
    }

    #[test]
    fn set_color_only_touches_pen_and_highlighter() {
        let mut ink = ink();
        let before = ink.pen_color;
        ink.set_tool(ShowTool::Pointer);
        ink.set_color(PALETTE[4].1);
        assert_eq!(ink.pen_color, before);
        ink.set_tool(ShowTool::Pen);
        ink.set_color(PALETTE[4].1);
        assert_eq!(ink.tool_color(), PALETTE[4].1);
    }

    #[test]
    fn cycle_color_on_the_pointer_is_a_no_op() {
        let mut ink = ink();
        let before = ink.pen_color;
        ink.cycle_color();
        assert_eq!(ink.pen_color, before);
    }

    #[test]
    fn palette_colors_are_unique() {
        // Position-based cycling breaks if two entries share a color.
        for (i, a) in PALETTE.iter().enumerate() {
            for b in &PALETTE[i + 1..] {
                assert_ne!(a.1, b.1, "duplicate palette colors {} and {}", a.0, b.0);
            }
        }
    }

    #[test]
    fn mid_stroke_color_change_keeps_the_open_strokes_color() {
        let mut ink = ink();
        ink.set_tool(ShowTool::Pen);
        ink.begin(0, 0.1, 0.1);
        ink.cycle_color();
        ink.push(0, 0.2, 0.2);
        // begin() snapshotted the initial color; cycling must not recolor the open stroke.
        assert_eq!(ink.strokes(0)[0].color, Rgba::rgb(0xFF, 0x00, 0x00));
        assert_ne!(ink.strokes(0)[0].color, ink.pen_color);
    }

    #[test]
    fn tool_color_for_the_highlighter_is_the_opaque_stored_color() {
        // The swatch ring compares `tool_color()` against opaque palette entries; returning
        // the alpha-applied stroke color would silently deselect it.
        let mut ink = ink();
        ink.set_tool(ShowTool::Highlighter);
        assert_eq!(ink.tool_color(), ink.highlighter_color);
        assert_eq!(ink.tool_color().a, 255);
    }

    #[test]
    fn width_ladder_is_sorted_and_matches_the_defaults() {
        assert!(WIDTHS.windows(2).all(|w| w[0] < w[1]));
        assert!(WIDTHS.contains(&PEN_WIDTH));
        assert!(WIDTHS.contains(&HIGHLIGHTER_WIDTH));
        assert!(WIDTH_DOTS.iter().all(|d| WIDTHS.contains(d)));
        assert!(WIDTH_DOTS_HL.iter().all(|d| WIDTHS.contains(d)));
        assert!(WIDTH_DOTS_HL.iter().all(|d| *d >= HIGHLIGHTER_MIN_WIDTH));
    }

    #[test]
    fn highlighter_width_has_a_floor() {
        let mut ink = ink();
        ink.set_tool(ShowTool::Highlighter);
        ink.set_width(1.5);
        assert_eq!(ink.highlighter_width, HIGHLIGHTER_MIN_WIDTH);
        // Stepping down from the floor stays at the floor.
        ink.highlighter_width = HIGHLIGHTER_MIN_WIDTH;
        ink.step_width(false);
        assert_eq!(ink.highlighter_width, HIGHLIGHTER_MIN_WIDTH);
        assert_eq!(ink.width_dots(), &WIDTH_DOTS_HL);
        ink.set_tool(ShowTool::Pen);
        assert_eq!(ink.width_dots(), &WIDTH_DOTS);
    }

    #[test]
    fn mid_stroke_width_change_keeps_the_open_strokes_width() {
        let mut ink = ink();
        ink.set_tool(ShowTool::Pen);
        ink.begin(0, 0.1, 0.1);
        ink.step_width(true);
        ink.push(0, 0.2, 0.2);
        assert_eq!(ink.strokes(0)[0].width, PEN_WIDTH);
        assert_ne!(ink.strokes(0)[0].width, ink.pen_width);
    }

    #[test]
    fn eraser_removes_hit_strokes_and_erase_is_undoable() {
        let mut ink = ink();
        ink.set_tool(ShowTool::Pen);
        // A: left line, B: right line, C: bottom line.
        ink.begin(0, 0.1, 0.5);
        ink.push(0, 0.2, 0.5);
        ink.end();
        ink.begin(0, 0.6, 0.5);
        ink.push(0, 0.7, 0.5);
        ink.end();
        ink.begin(0, 0.1, 0.9);
        ink.push(0, 0.2, 0.9);
        ink.end();
        // Erase B by its middle.
        assert_eq!(ink.erase_at(0, 0.65, 0.5, 7.2, 720.0, 405.0), 1);
        assert_eq!(ink.strokes(0).len(), 2);
        // Undo restores B at its original position.
        assert!(ink.undo());
        assert_eq!(ink.strokes(0).len(), 3);
        assert_eq!(ink.strokes(0)[1].points[0], (0.6, 0.5));
        // Then undos remove the draws, newest first.
        assert!(ink.undo());
        assert_eq!(ink.strokes(0).len(), 2);
        assert!(ink.undo());
        assert!(ink.undo());
        assert!(!ink.any());
    }

    #[test]
    fn eraser_misses_distant_points_and_hits_dots() {
        let mut ink = ink();
        ink.set_tool(ShowTool::Pen);
        ink.begin(0, 0.5, 0.5);
        ink.end();
        assert_eq!(ink.erase_at(0, 0.9, 0.9, 7.2, 720.0, 405.0), 0);
        assert_eq!(ink.erase_at(0, 0.5, 0.505, 7.2, 720.0, 405.0), 1);
        assert!(!ink.any());
    }

    #[test]
    fn eraser_respects_stroke_width() {
        let mut ink = ink();
        ink.set_tool(ShowTool::Pen);
        ink.begin(0, 0.1, 0.5);
        ink.push(0, 0.2, 0.5);
        ink.end();
        // Default pen (2.5pt): 8.1pt below the line is a miss (tol 3.6 + 1.25 half-width).
        assert_eq!(ink.erase_at(0, 0.15, 0.52, 3.6, 720.0, 405.0), 0);
        // Thick pen: the same offset hits (3.6 + 10).
        ink.pen_width = 20.0;
        ink.begin(0, 0.1, 0.7);
        ink.push(0, 0.2, 0.7);
        ink.end();
        assert_eq!(ink.erase_at(0, 0.15, 0.72, 3.6, 720.0, 405.0), 1);
        assert_eq!(ink.strokes(0).len(), 1);
    }

    #[test]
    fn eraser_tolerance_is_isotropic() {
        // On a 720x405 slide the same 5pt offset must hit vertically and horizontally alike
        // (a normalized-coordinates hit test would be an ellipse).
        let mut ink = ink();
        ink.set_tool(ShowTool::Pen);
        ink.begin(0, 0.1, 0.5);
        ink.push(0, 0.9, 0.5);
        ink.end();
        assert_eq!(ink.erase_at(0, 0.5, 0.5 + 5.0 / 405.0, 5.0, 720.0, 405.0), 1);
        ink.begin(0, 0.1, 0.5);
        ink.push(0, 0.9, 0.5);
        ink.end();
        assert_eq!(ink.erase_at(0, 0.9 + 5.0 / 720.0, 0.5, 5.0, 720.0, 405.0), 1);
    }

    #[test]
    fn eraser_ignores_other_slides() {
        let mut ink = ink();
        ink.set_tool(ShowTool::Pen);
        ink.begin(1, 0.5, 0.5);
        ink.end();
        assert_eq!(ink.erase_at(0, 0.5, 0.5, 7.2, 720.0, 405.0), 0);
        assert!(ink.any());
    }

    #[test]
    fn switching_tools_closes_the_open_stroke() {
        let mut ink = ink();
        ink.set_tool(ShowTool::Pen);
        ink.begin(0, 0.1, 0.1);
        ink.set_tool(ShowTool::Eraser);
        assert!(!ink.open());
        ink.push(0, 0.9, 0.9);
        assert_eq!(ink.strokes(0)[0].points.len(), 1);
    }

    #[test]
    fn eraser_tests_the_smoothed_curve() {
        // A sharp-corner stroke (right angle at (0.1, 0.9)): the painted curve cuts the
        // corner, so the eraser must hit the visible bulge and miss the raw corner point.
        let draw = |ink: &mut ShowInk| {
            ink.begin(0, 0.1, 0.5);
            ink.push(0, 0.1, 0.9);
            ink.push(0, 0.6, 0.9);
            ink.end();
        };
        let mut ink = ink();
        ink.set_tool(ShowTool::Pen);
        draw(&mut ink);
        // On the smoothed bulge (the midpoint quadratic's closest approach to the corner:
        // (72+360t², ...) at t=0.5 → (162, 344.25) in slide points).
        assert_eq!(ink.erase_at(0, 0.225, 0.85, 5.0, 720.0, 405.0), 1);
        draw(&mut ink);
        // The raw corner point itself is off-curve now.
        assert_eq!(ink.erase_at(0, 0.1, 0.9, 5.0, 720.0, 405.0), 0);
    }

    #[test]
    fn journal_interleaves_draws_and_erasures_in_reverse_order() {
        let mut ink = ink();
        ink.set_tool(ShowTool::Pen);
        ink.begin(0, 0.1, 0.1);
        ink.end();
        assert_eq!(ink.erase_at(0, 0.1, 0.1, 7.2, 720.0, 405.0), 1);
        ink.begin(0, 0.5, 0.5);
        ink.end();
        assert_eq!(ink.strokes(0).len(), 1);
        assert!(ink.undo()); // removes the second draw
        assert_eq!(ink.strokes(0).len(), 0);
        assert!(ink.undo()); // restores the erased stroke
        assert_eq!(ink.strokes(0).len(), 1);
        assert_eq!(ink.strokes(0)[0].points[0], (0.1, 0.1));
        assert!(ink.undo()); // removes the first draw
        assert!(!ink.any());
        assert!(!ink.undo());
    }

    #[test]
    fn eraser_sweep_is_one_undo_and_restores_positions() {
        let mut ink = ink();
        ink.set_tool(ShowTool::Pen);
        // Three horizontal lines at y = 0.2, 0.5, 0.8.
        for y in [0.2f32, 0.5, 0.8] {
            ink.begin(0, 0.1, y);
            ink.push(0, 0.9, y);
            ink.end();
        }
        ink.start_erase();
        // Frame 1 of the sweep erases the top line, frame 2 the bottom line.
        assert_eq!(ink.erase_at(0, 0.5, 0.2, 7.2, 720.0, 405.0), 1);
        assert_eq!(ink.erase_at(0, 0.5, 0.8, 7.2, 720.0, 405.0), 1);
        ink.end_erase();
        assert_eq!(ink.strokes(0).len(), 1);
        // A single undo restores both, in their original positions.
        assert!(ink.undo());
        assert_eq!(ink.strokes(0).len(), 3);
        assert_eq!(ink.strokes(0)[0].points[0], (0.1, 0.2));
        assert_eq!(ink.strokes(0)[1].points[0], (0.1, 0.5));
        assert_eq!(ink.strokes(0)[2].points[0], (0.1, 0.8));
    }

    #[test]
    fn eraser_without_a_gesture_journals_per_call() {
        let mut ink = ink();
        ink.set_tool(ShowTool::Pen);
        for y in [0.2f32, 0.5] {
            ink.begin(0, 0.1, y);
            ink.push(0, 0.9, y);
            ink.end();
        }
        // No start_erase: each call is its own undo step.
        assert_eq!(ink.erase_at(0, 0.5, 0.2, 7.2, 720.0, 405.0), 1);
        assert_eq!(ink.erase_at(0, 0.5, 0.5, 7.2, 720.0, 405.0), 1);
        assert!(!ink.any());
        assert!(ink.undo());
        assert_eq!(ink.strokes(0).len(), 1);
        assert_eq!(ink.strokes(0)[0].points[0], (0.1, 0.5));
        assert!(ink.undo());
        assert_eq!(ink.strokes(0).len(), 2);
    }

    #[test]
    fn smooth_stroke_passthrough_and_bounded() {
        assert_eq!(smooth_stroke(&[]), vec![]);
        assert_eq!(smooth_stroke(&[(0.1, 0.2)]), vec![(0.1, 0.2)]);
        assert_eq!(smooth_stroke(&[(0.0, 0.0), (1.0, 1.0)]), vec![(0.0, 0.0), (1.0, 1.0)]);
        let pts: Vec<(f32, f32)> = (0..10).map(|i| (i as f32, (i % 2) as f32)).collect();
        let out = smooth_stroke(&pts);
        assert_eq!(out.len(), (pts.len() - 1) * SMOOTH_SUBDIV + 1);
    }

    #[test]
    fn smooth_stroke_preserves_endpoints() {
        let pts = [(0.1, 0.2), (0.5, 0.8), (0.9, 0.3)];
        let out = smooth_stroke(&pts);
        assert_eq!(out.first(), pts.first().copied().as_ref());
        assert_eq!(out.last(), pts.last().copied().as_ref());
    }

    #[test]
    fn smooth_stroke_keeps_collinear_points_on_the_line() {
        let pts = [(0.0, 0.5), (0.3, 0.5), (0.6, 0.5), (1.0, 0.5)];
        assert!(smooth_stroke(&pts).iter().all(|&(_, y)| (y - 0.5).abs() < 1e-6));
    }

    #[test]
    fn smooth_stroke_cuts_corners() {
        // A right-angle corner at (0, 1): the smoothed path must pass strictly inside it.
        let pts = [(0.0, 0.0), (0.0, 1.0), (1.0, 1.0)];
        let out = smooth_stroke(&pts);
        let corner_dist = |p: &(f32, f32)| (p.0 * p.0 + (p.1 - 1.0) * (p.1 - 1.0)).sqrt();
        let min_d = out.iter().map(corner_dist).fold(f32::INFINITY, f32::min);
        assert!(min_d > 0.01, "smoothed path still hits the corner");
        assert!(min_d < 0.5, "smoothing swallowed the corner entirely");
    }

    #[test]
    fn step_width_walks_the_ladder_and_clamps() {
        let mut ink = ink();
        ink.set_tool(ShowTool::Pen); // default 2.5
        ink.step_width(true);
        assert_eq!(ink.pen_width, 4.0);
        ink.step_width(true);
        assert_eq!(ink.pen_width, 6.0);
        ink.step_width(false);
        assert_eq!(ink.pen_width, 4.0);
        ink.pen_width = WIDTHS[0];
        ink.step_width(false);
        assert_eq!(ink.pen_width, WIDTHS[0]);
        ink.pen_width = WIDTHS[7];
        ink.step_width(true);
        assert_eq!(ink.pen_width, WIDTHS[7]);
    }

    #[test]
    fn step_width_from_a_custom_width_finds_the_nearest_rung() {
        let mut ink = ink();
        ink.set_tool(ShowTool::Pen);
        ink.pen_width = 3.0; // between 2.5 and 4.0
        ink.step_width(true);
        assert_eq!(ink.pen_width, 4.0);
        ink.pen_width = 3.0;
        ink.step_width(false);
        assert_eq!(ink.pen_width, 2.5);
    }

    #[test]
    fn width_is_remembered_per_tool_and_gated() {
        let mut ink = ink();
        ink.set_tool(ShowTool::Pen);
        ink.set_width(6.0);
        ink.set_tool(ShowTool::Highlighter);
        assert_eq!(ink.tool_width(), HIGHLIGHTER_WIDTH);
        ink.set_width(20.0);
        assert_eq!(ink.highlighter_width, 20.0);
        assert_eq!(ink.pen_width, 6.0);
        ink.set_tool(ShowTool::Pointer);
        ink.step_width(true);
        assert_eq!(ink.pen_width, 6.0); // unchanged
    }

    /// Two strokes on slide 0: a diagonal (0.1,0.1)-(0.5,0.5) and a horizontal
    /// (0.1,0.8)-(0.9,0.8). Slide units 100x100.
    fn two_strokes() -> ShowInk {
        let mut ink = ink();
        ink.set_tool(ShowTool::Pen);
        ink.begin(0, 0.1, 0.1);
        ink.push(0, 0.5, 0.5);
        ink.end();
        ink.begin(0, 0.1, 0.8);
        ink.push(0, 0.9, 0.8);
        ink.end();
        ink
    }

    #[test]
    fn hit_test_finds_the_topmost_and_select_at_manages_the_selection() {
        let mut ink = two_strokes();
        // On the diagonal, 100x100 slide, tol 2pt.
        let id = ink.hit_test(0, 0.3, 0.3, 2.0, 100.0, 100.0).expect("hit the diagonal");
        assert_eq!(id, ink.strokes(0)[0].id);
        // A miss selects nothing and clears.
        assert_eq!(ink.hit_test(0, 0.3, 0.35, 0.01, 100.0, 100.0), None);
        ink.select_only(0, id);
        assert_eq!(ink.selection_len(), 1);
        // Shift-click the horizontal stroke adds it; shift-click again removes it.
        let id2 = ink.hit_test(0, 0.5, 0.8, 2.0, 100.0, 100.0).expect("hit the horizontal");
        ink.toggle_selected(0, id2);
        assert_eq!(ink.selection_len(), 2);
        ink.toggle_selected(0, id2);
        assert_eq!(ink.selection_len(), 1);
        // A click on empty space (no shift) clears.
        ink.clear_selection();
        assert_eq!(ink.selection_len(), 0);
    }

    #[test]
    fn marquee_selects_intersecting_boxes_and_adds_with_shift() {
        let mut ink = two_strokes();
        // Rect over the diagonal's middle: selects only it.
        assert_eq!(ink.select_in_rect(0, (0.2, 0.2), (0.4, 0.4), false), 1);
        // Full-slide rect selects both; add-mode keeps them.
        assert_eq!(ink.select_in_rect(0, (0.0, 0.0), (1.0, 1.0), true), 2);
        // An empty slide region replaces with nothing.
        assert_eq!(ink.select_in_rect(0, (0.55, 0.55), (0.7, 0.7), false), 0);
    }

    #[test]
    fn move_gesture_is_one_undoable_step() {
        let mut ink = two_strokes();
        let id = ink.strokes(0)[0].id;
        let before = ink.strokes(0)[0].points.clone();
        ink.select_only(0, id);
        ink.begin_move();
        ink.move_selection_by(0.1, 0.05);
        ink.move_selection_by(-0.02, 0.01);
        assert!(ink.end_move());
        let s = &ink.strokes(0)[0];
        assert!((s.points[0].0 - (before[0].0 + 0.08)).abs() < 1e-4);
        assert!((s.points[0].1 - (before[0].1 + 0.06)).abs() < 1e-4);
        // One undo restores the original points; the other stroke never moved.
        assert!(ink.undo());
        let restored = &ink.strokes(0)[0].points;
        for (r, b) in restored.iter().zip(before.iter()) {
            assert!((r.0 - b.0).abs() < 1e-6 && (r.1 - b.1).abs() < 1e-6, "undo restores points (float delta round-trip)");
        }
        assert_eq!(ink.strokes(0)[1].points, vec![(0.1, 0.8), (0.9, 0.8)]);
        // A no-delta gesture journals nothing.
        ink.begin_move();
        assert!(!ink.end_move());
    }

    #[test]
    fn duplicate_offsets_copies_and_undoes_cleanly() {
        let mut ink = two_strokes();
        let id = ink.strokes(0)[0].id;
        let orig = ink.strokes(0)[0].points.clone();
        ink.select_only(0, id);
        assert_eq!(ink.duplicate_selection(), 1);
        assert_eq!(ink.strokes(0).len(), 3);
        // The copy is selected, offset by DUP_OFFSET, with a fresh id.
        let copy_id = ink.selection()[0];
        assert_ne!(copy_id, id);
        let copy = ink.strokes(0).iter().find(|s| s.id == copy_id).expect("copy exists");
        assert!((copy.points[0].0 - (orig[0].0 + DUP_OFFSET.0)).abs() < 1e-6);
        // Undo removes only the copy.
        assert!(ink.undo());
        assert_eq!(ink.strokes(0).len(), 2);
        assert_eq!(ink.strokes(0)[0].points, orig);
        assert_eq!(ink.selection_len(), 0, "selection pruned after undo removed the copy");
    }

    #[test]
    fn delete_selection_restores_on_undo() {
        let mut ink = two_strokes();
        let (id0, id1) = (ink.strokes(0)[0].id, ink.strokes(0)[1].id);
        let orig1 = ink.strokes(0)[1].points.clone();
        ink.select_only(0, id0);
        ink.toggle_selected(0, id1);
        assert_eq!(ink.delete_selection(), 2);
        assert_eq!(ink.count(), 0);
        assert!(ink.can_undo());
        assert!(ink.undo());
        assert_eq!(ink.strokes(0).len(), 2);
        assert_eq!(ink.strokes(0)[1].points, orig1, "order preserved");
    }

    #[test]
    fn copy_paste_lands_on_the_target_slide_with_new_ids() {
        let mut ink = two_strokes();
        let id = ink.strokes(0)[0].id;
        ink.select_only(0, id);
        assert_eq!(ink.copy_selection(), 1);
        assert_eq!(ink.paste(2), 1);
        assert_eq!(ink.strokes(2).len(), 1);
        assert_ne!(ink.strokes(2)[0].id, id);
        assert_eq!(ink.selection(), &[ink.strokes(2)[0].id]);
        // Pasting again creates another copy (clipboard survives).
        assert_eq!(ink.paste(2), 1);
        assert_eq!(ink.strokes(2).len(), 2);
        // Two pastes = two Added entries = two undos.
        assert!(ink.undo());
        assert!(ink.undo());
        assert_eq!(ink.strokes(2).len(), 0);
    }

    #[test]
    fn selection_is_pruned_when_strokes_vanish() {
        let mut ink = two_strokes();
        let id = ink.strokes(0)[0].id;
        ink.select_only(0, id);
        // Erase that stroke: the selection must drop it.
        assert_eq!(ink.erase_at(0, 0.3, 0.3, 2.0, 100.0, 100.0), 1);
        assert_eq!(ink.selection_len(), 0);
        // Undo restores the stroke; the selection stays empty (no resurrection).
        assert!(ink.undo());
        assert_eq!(ink.selection_len(), 0);
        // Clear drops the rest.
        ink.select_only(0, id);
        ink.clear_slide(0);
        assert_eq!(ink.selection_len(), 0);
    }

    #[test]
    fn selection_follows_only_its_slide() {
        let mut ink = two_strokes();
        let id = ink.strokes(0)[0].id;
        ink.select_only(0, id);
        ink.sync_slide(0);
        assert_eq!(ink.selection_len(), 1);
        ink.sync_slide(1);
        assert_eq!(ink.selection_len(), 0, "navigating away drops the selection");
        // Selecting on another slide starts fresh.
        ink.begin(1, 0.2, 0.2);
        ink.push(1, 0.4, 0.4);
        ink.end();
        ink.select_only(1, ink.strokes(1)[0].id);
        assert_eq!(ink.selected_bounds(1), Some(((0.2, 0.2), (0.4, 0.4))));
        assert_eq!(ink.selected_bounds(0), None, "bounds only on the selection's slide");
    }

    #[test]
    fn move_is_anchored_to_its_slide_across_navigation() {
        let mut ink = two_strokes();
        let id = ink.strokes(0)[0].id;
        let orig = ink.strokes(0)[0].points.clone();
        ink.select_only(0, id);
        ink.begin_move();
        ink.move_selection_by(0.1, 0.0);
        // Navigation mid-drag: the gesture ends against slide 0, selection drops.
        ink.sync_slide(1);
        assert_eq!(ink.selection_len(), 0);
        // Deltas after the navigation go nowhere (the gesture is closed).
        ink.move_selection_by(0.2, 0.0);
        assert!(!ink.end_move(), "the gesture ended at navigation");
        let s = &ink.strokes(0)[0];
        assert!((s.points[0].0 - (orig[0].0 + 0.1)).abs() < 1e-6, "only pre-navigation deltas applied");
        // One undo reverts exactly that journaled move, on slide 0.
        assert!(ink.undo());
        let s = &ink.strokes(0)[0];
        assert!((s.points[0].0 - orig[0].0).abs() < 1e-6);
    }

    #[test]
    fn undo_mid_move_first_reverts_the_inflight_drag() {
        let mut ink = two_strokes();
        let id = ink.strokes(0)[0].id;
        let orig = ink.strokes(0)[0].points.clone();
        ink.select_only(0, id);
        ink.begin_move();
        ink.move_selection_by(0.2, 0.0);
        // Cmd+Z mid-drag: the in-flight move is committed and immediately reverted — not
        // silently dropped, and the older Draw survives for the next undo.
        assert!(ink.undo());
        let s = &ink.strokes(0)[0];
        assert!((s.points[0].0 - orig[0].0).abs() < 1e-6, "in-flight drag reverted");
        ink.move_selection_by(0.5, 0.0);
        assert!((ink.strokes(0)[0].points[0].0 - orig[0].0).abs() < 1e-6, "gesture is closed");
        assert!(!ink.end_move());
        assert!(ink.undo(), "the stroke's Draw is still undoable");
        assert_eq!(ink.count(), 1);
    }

    #[test]
    fn delete_mid_move_journals_move_before_delete() {
        let mut ink = two_strokes();
        let id = ink.strokes(0)[0].id;
        let orig = ink.strokes(0)[0].points.clone();
        ink.select_only(0, id);
        ink.begin_move();
        ink.move_selection_by(0.2, 0.0);
        assert_eq!(ink.delete_selection(), 1);
        assert_eq!(ink.strokes(0).len(), 1);
        // Undo restores the stroke at its moved position, then reverts the move: strict LIFO.
        assert!(ink.undo());
        let s = &ink.strokes(0)[0];
        assert!((s.points[0].0 - (orig[0].0 + 0.2)).abs() < 1e-6, "restored at the moved position");
        assert!(ink.undo());
        let s = &ink.strokes(0)[0];
        assert!((s.points[0].0 - orig[0].0).abs() < 1e-6, "then the move is reverted");
    }

    #[test]
    fn paste_with_empty_clipboard_keeps_the_selection() {
        let mut ink = two_strokes();
        let id = ink.strokes(0)[0].id;
        ink.select_only(0, id);
        assert_eq!(ink.paste(0), 0);
        assert_eq!(ink.selection(), &[id], "empty clipboard: selection untouched");
        assert_eq!(ink.count(), 2, "empty clipboard: no strokes added");
    }

    #[test]
    fn switching_away_from_select_clears_the_selection() {
        let mut ink = two_strokes();
        let id = ink.strokes(0)[0].id;
        ink.select_only(0, id);
        ink.set_tool(ShowTool::Pen);
        assert_eq!(ink.selection_len(), 0);
        // Switching TO select keeps whatever the pure layer holds (a fresh selection starts).
        ink.select_only(0, id);
        ink.set_tool(ShowTool::Select);
        assert_eq!(ink.selection_len(), 1);
    }

    #[test]
    fn can_undo_sees_an_inflight_move() {
        let mut ink = two_strokes();
        let id = ink.strokes(0)[0].id;
        ink.select_only(0, id);
        ink.begin_move();
        ink.move_selection_by(0.1, 0.0);
        assert!(ink.can_undo(), "the in-flight drag is undoable");
        assert!(ink.undo(), "and undo reverts it");
    }
}
