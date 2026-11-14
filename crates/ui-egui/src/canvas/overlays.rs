use super::{Color32, CornerRadius, Point, Pos2, Rect, SlideApp, Stroke, Tokens, ToolKind, Ui, Xf, Xfrm, box_handles, pos2, theme};
use deckcraft_engine::DocState;

fn poly(p: &egui::Painter, pts: [Pos2; 4], stroke: Stroke) {
    for i in 0..4 {
        p.line_segment([pts[i], pts[(i + 1) % 4]], stroke);
    }
}

fn selection_handles(app: &SlideApp, st: &DocState, painter: &egui::Painter, xf: Xf, t: &Tokens) {
    let sel_stroke = Stroke::new(1.0, t.selection);
    let editing = st.selection.text.clone();
    for id in &st.selection.shapes {
        let Some(sh) = st.shape(*id) else { continue };
        let x = deckcraft_engine::cmd::xfrm_of(&st.doc, &st.selection, sh);
        let a = x.affine();
        let corners = [Point::new(0.0, 0.0), Point::new(x.w, 0.0), Point::new(x.w, x.h), Point::new(0.0, x.h)].map(|q| xf.to_screen(a * q));
        if sh.is_line() {
            let (p0, p1) = (xf.to_screen(a * Point::new(0.0, 0.0)), xf.to_screen(a * Point::new(x.w, x.h)));
            let glued = match sh.kind {
                deckcraft_model::ShapeKind::Connector { start, end } => [start.is_some(), end.is_some()],
                _ => [false, false],
            };
            for (q, g) in [p0, p1].into_iter().zip(glued) {
                // Glued ends are filled with the accent, like attached connector ends.
                let fill = if g { t.accent } else { t.handle_fill };
                painter.circle(q, 4.5, fill, Stroke::new(1.0, if g { t.handle_fill } else { t.handle_stroke }));
            }
            continue;
        }
        let editing_this = editing.as_ref().is_some_and(|e| e.shape == *id && !e.notes);
        if app.session.tool.editing_points(st) == Some(sh.id) {
            // Edit Points: the outline's vertices are the handles.
            // No selection box: the outline and its vertices are what's edited.
            if let Some((path, index)) = app.session.tool.edit_vertex
                && let Some((hin, hout, v)) = deckcraft_engine::cmd::points::slide_handles(&st.doc, &st.selection, sh, path, index)
            {
                for h in [hin, hout].into_iter().flatten() {
                    painter.line_segment([xf.to_screen(v), xf.to_screen(h)], sel_stroke);
                    painter.rect(
                        egui::Rect::from_center_size(xf.to_screen(h), egui::vec2(6.0, 6.0)),
                        0.0,
                        Color32::WHITE,
                        Stroke::new(1.0, t.selection),
                        egui::StrokeKind::Middle,
                    );
                }
            }
            for q in deckcraft_engine::cmd::points::slide_vertices(&st.doc, &st.selection, sh).into_iter().flatten() {
                let c = xf.to_screen(q);
                painter.rect(
                    egui::Rect::from_center_size(c, egui::vec2(7.0, 7.0)),
                    0.0,
                    Color32::BLACK,
                    Stroke::new(1.0, Color32::WHITE),
                    egui::StrokeKind::Middle,
                );
            }
            continue;
        }
        if editing_this {
            // Dashed box while editing text.
            for i in 0..4 {
                let (s, e) = (corners[i], corners[(i + 1) % 4]);
                painter.add(egui::Shape::dashed_line(&[s, e], Stroke::new(1.0, t.selection), 4.0, 3.0));
            }
        } else {
            poly(painter, corners, sel_stroke);
        }
        if sh.locked && !editing_this {
            // Locked: greyed handles that don't respond, no rotate or adjust handles.
            for h in box_handles(&x).0 {
                painter.circle(xf.to_screen(h), 4.0, t.text_faint, Stroke::new(1.0, t.handle_fill));
            }
            continue;
        }
        if st.selection.shapes.len() == 1 || !editing_this {
            let (hs, rot) = box_handles(&x);
            let rot_s = xf.to_screen(rot);
            if !editing_this && sh.ph.as_ref().is_none_or(|_| true) {
                let top_mid = xf.to_screen(hs[1]);
                painter.line_segment([top_mid, rot_s], sel_stroke);
                rotate_handle(painter, rot_s, t);
            }
            for h in hs {
                painter.circle(xf.to_screen(h), 4.5, t.handle_fill, Stroke::new(1.0, t.handle_stroke));
            }
            let geo = deckcraft_render::shape_geometry(sh, x.w, x.h);
            for hd in &geo.handles {
                let q = xf.to_screen(a * hd.pos);
                painter.circle(q, 4.5, Color32::from_rgb(0xFF, 0xD2, 0x3F), Stroke::new(1.0, Color32::from_rgb(0x8A, 0x6A, 0x00)));
            }
            for hd in deckcraft_engine::tools::wordart_handles(&st.doc, &st.selection, sh, x.w, x.h) {
                let q = xf.to_screen(a * hd.pos);
                painter.circle(q, 4.5, Color32::from_rgb(0xFF, 0xD2, 0x3F), Stroke::new(1.0, Color32::from_rgb(0x8A, 0x6A, 0x00)));
            }
        }
    }
}

fn text_overlays(st: &DocState, ui: &Ui, painter: &egui::Painter, xf: Xf, t: &Tokens) {
    let editing = st.selection.text.clone();
    // Caret / text selection, in a shape or a table cell.
    if let Some(ts) = &editing {
        for quad in text_marks(st, ts).iter().flat_map(|m| &m.selection) {
            painter.add(egui::Shape::convex_polygon(quad.iter().map(|q| xf.to_screen(*q)).collect(), t.text_selection, Stroke::NONE));
        }
        // An input method's composition: underlined in place, thicker under the part being converted.
        for (a, b, thick) in deckcraft_engine::cmd::text::composition_underlines(st) {
            painter.line_segment([xf.to_screen(a), xf.to_screen(b)], Stroke::new(if thick { 2.0 } else { 1.0 }, t.caret));
        }
        if !ts.is_range() {
            let blink = (ui.input(|i| i.time) * 1.6).fract() < 0.6;
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(300));
            // An empty placeholder shows the caret where typing begins too.
            if blink && let Some((top, bot)) = deckcraft_engine::cmd::text::caret_line(st, ts) {
                // In the colour of the text being typed: a black caret is lost on a dark slide.
                let color = deckcraft_engine::cmd::text::caret_color(st, ts).map_or(t.caret, theme::to_color32);
                painter.line_segment([xf.to_screen(top), xf.to_screen(bot)], Stroke::new(1.5, color));
            }
        }
    }
    // Table cell editing: outline the cell (merged cells as drawn).
    if let Some(ts) = &editing
        && let (Some((r, c)), Some(sh)) = (ts.cell, st.shape(ts.shape))
        && let Some(f) = deckcraft_engine::cmd::table::table_frame(st, sh)
        && let Some(cr) = f.cell_rect(r, c)
    {
        let pts = [Point::new(cr.x0, cr.y0), Point::new(cr.x1, cr.y0), Point::new(cr.x1, cr.y1), Point::new(cr.x0, cr.y1)]
            .map(|q| xf.to_screen(f.affine * q));
        poly(painter, pts, Stroke::new(2.0, t.accent));
    }
    // Shade the selected table cells in the text-selection color, each merged cell as it is drawn.
    if let Some((id, ..)) = st.selection.cells
        && st.selection.text.is_none()
        && let Some(sh) = st.shape(id)
        && let deckcraft_model::ShapeKind::Table(table) = &sh.kind
        && let Some(f) = deckcraft_engine::cmd::table::table_frame(st, sh)
    {
        use deckcraft_engine::cmd::table::{in_range, selected_range};
        let range = selected_range(st.selection.cells, id, table);
        for (_, _, cr) in f.cells.iter().filter(|(r, c, _)| range.is_some() && in_range(range, *r, *c)) {
            let pts = [Point::new(cr.x0, cr.y0), Point::new(cr.x1, cr.y0), Point::new(cr.x1, cr.y1), Point::new(cr.x0, cr.y1)];
            painter.add(egui::Shape::convex_polygon(pts.iter().map(|q| xf.to_screen(f.affine * *q)).collect(), t.text_selection, Stroke::NONE));
        }
    }
}

pub(super) fn overlays(app: &mut SlideApp, ui: &Ui, painter: &egui::Painter, xf: Xf, t: &Tokens) {
    // Spelling squiggles under misspelt words (checked in the background).
    if let Some(st) = app.session.active() {
        let lines = app.squiggles.update(st, Some(ui.ctx()));
        let scale = (xf.to_screen(Point::new(1.0, 0.0)) - xf.to_screen(Point::new(0.0, 0.0))).length();
        for (a, b) in lines {
            crate::squiggles::paint(painter, xf.to_screen(a), xf.to_screen(b), scale);
        }
    }
    let Some(st) = app.session.active() else { return };
    selection_handles(app, st, painter, xf, t);
    text_overlays(st, ui, painter, xf, t);
    // Smart guides, marquee, previews.
    for g in &app.session.tool.guides {
        let (a, b) =
            if g.vertical { (Point::new(g.pos, g.from), Point::new(g.pos, g.to)) } else { (Point::new(g.from, g.pos), Point::new(g.to, g.pos)) };
        painter.add(egui::Shape::dashed_line(&[xf.to_screen(a), xf.to_screen(b)], Stroke::new(1.0, t.guide), 5.0, 3.0));
    }
    if let Some(m) = app.session.tool.marquee {
        let r = Rect::from_two_pos(xf.to_screen(Point::new(m.x0, m.y0)), xf.to_screen(Point::new(m.x1, m.y1)));
        painter.rect(
            r,
            CornerRadius::ZERO,
            Color32::from_rgba_unmultiplied(80, 130, 230, 30),
            Stroke::new(1.0, Color32::from_rgb(80, 130, 230)),
            egui::StrokeKind::Inside,
        );
    }
    if let Some(pv) = app.session.tool.preview {
        draw_preview(app, painter, xf, pv, t);
    }
    // Freeform being drawn.
    let path = &app.session.tool.path_preview;
    if path.len() > 1 {
        let pts: Vec<Point> = if app.session.tool.path_smooth && path.len() > 2 {
            let d = deckcraft_engine::cmd::shape::freeform_path(path, false, true);
            let mut v = vec![];
            let bp = deckcraft_render::parse_path(&d);
            deckcraft_geom::preset_flatten(&bp, &mut |el| match el {
                deckcraft_geom::PathEl::MoveTo(q) | deckcraft_geom::PathEl::LineTo(q) => v.push(q),
                _ => {}
            });
            v
        } else {
            path.clone()
        };
        painter.add(egui::Shape::line(pts.iter().map(|q| xf.to_screen(*q)).collect(), Stroke::new(1.5, t.accent)));
        if app.session.tool.path_closing
            && let Some(s0) = path.first()
        {
            painter.circle(xf.to_screen(*s0), 5.0, t.accent, Stroke::new(1.5, t.handle_fill));
        }
    }
    // Connection sites of the shape under a connector end, and the site it would glue to.
    for q in &app.session.tool.sites {
        let c = xf.to_screen(*q);
        painter.circle(c, 3.5, t.handle_fill, Stroke::new(1.0, t.handle_stroke));
    }
    if let Some(g) = app.session.tool.glue {
        painter.circle(xf.to_screen(g), 5.0, t.accent, Stroke::new(1.5, t.handle_fill));
    }
    if app.session.tool.ink_preview.len() > 1
        && let ToolKind::Ink { color, width, mode } = &app.session.tool.kind
    {
        let pts = &app.session.tool.ink_preview;
        let legacy = deckcraft_engine::model::ink_pressure_is_legacy(pts);
        let c = theme::to_color32(*color);
        let c = if mode == "highlighter" { c.gamma_multiply(0.45) } else { c };
        if legacy || mode == "highlighter" {
            // One line, so see-through highlighter ink doesn't darken where segments meet.
            let line: Vec<Pos2> = pts.iter().map(|(x, y, _)| xf.to_screen(Point::new(*x, *y))).collect();
            painter.add(egui::Shape::line(line, Stroke::new((*width as f32 * xf.scale).max(1.0), c)));
        } else {
            for pair in pts.windows(2) {
                let [q0, q1] = pair else { continue };
                let a = deckcraft_engine::model::ink_sample_width(*width, q0.2, legacy);
                let b = deckcraft_engine::model::ink_sample_width(*width, q1.2, legacy);
                let sw = ((a + b) * 0.5 * f64::from(xf.scale)).max(1.0) as f32;
                let p0 = xf.to_screen(Point::new(q0.0, q0.1));
                let p1 = xf.to_screen(Point::new(q1.0, q1.1));
                painter.add(egui::Shape::line(vec![p0, p1], Stroke::new(sw, c)));
            }
        }
    }
}

/// The text selection (quads) of the text being edited, in slide space: in a shape or a table
/// cell, wherever its text is drawn. The caret is `cmd::text::caret_line`.
pub(crate) struct TextMarks {
    pub selection: Vec<[Point; 4]>,
}

pub(crate) fn text_marks(st: &deckcraft_engine::DocState, ts: &deckcraft_engine::TextSel) -> Option<TextMarks> {
    let (l, m) = deckcraft_engine::cmd::text::text_frame(st, ts)?;
    let pos = |p: (usize, usize)| deckcraft_text::Pos { para: p.0, ch: p.1 };
    let (s0, s1) = ts.ordered();
    let selection = l
        .selection_rects(pos(s0), pos(s1))
        .into_iter()
        .map(|r| [Point::new(r.x0, r.y0), Point::new(r.x1, r.y0), Point::new(r.x1, r.y1), Point::new(r.x0, r.y1)].map(|q| m * q))
        .collect();
    Some(TextMarks { selection })
}

fn rotate_handle(p: &egui::Painter, c: Pos2, t: &Tokens) {
    p.circle_filled(c, 6.0, t.handle_fill);
    p.add(egui::Shape::Path(egui::epaint::PathShape {
        points: (0..=20)
            .map(|i| {
                let a = -0.4 + i as f32 / 20.0 * 5.2;
                pos2(c.x + 4.0 * a.cos(), c.y + 4.0 * a.sin())
            })
            .collect(),
        closed: false,
        fill: Color32::TRANSPARENT,
        stroke: Stroke::new(1.3, t.handle_stroke).into(),
    }));
}

fn draw_preview(app: &SlideApp, p: &egui::Painter, xf: Xf, x: Xfrm, t: &Tokens) {
    let preset = match &app.session.tool.kind {
        ToolKind::Shape { preset } => preset.clone(),
        _ => "rect".into(),
    };
    if preset == deckcraft_engine::tools::DRAW_TABLE {
        // A border inside a table (flat), or the outline of a new table.
        let (a, b) = (xf.to_screen(Point::new(x.x, x.y)), xf.to_screen(Point::new(x.x + x.w, x.y + x.h)));
        if x.w < 0.5 || x.h < 0.5 {
            p.line_segment([a, b], Stroke::new(1.5, t.accent));
        } else {
            p.rect_stroke(Rect::from_two_pos(a, b), 0.0, Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
        }
        return;
    }
    if deckcraft_geom::preset::is_line_like(&preset) {
        let a = x.affine();
        p.line_segment([xf.to_screen(a * Point::new(0.0, 0.0)), xf.to_screen(a * Point::new(x.w, x.h))], Stroke::new(1.5, t.accent));
        return;
    }
    let r = Rect::from_two_pos(xf.to_screen(Point::new(x.x, x.y)), xf.to_screen(Point::new(x.x + x.w, x.y + x.h)));
    if r.width() < 1.0 || r.height() < 1.0 {
        return;
    }
    crate::ribbon::paint_preset(p, r, &preset, t.accent.gamma_multiply(0.25), t.accent);
}
