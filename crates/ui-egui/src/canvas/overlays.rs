use super::{Color32, CornerRadius, Point, Pos2, Rect, SlideApp, Stroke, Tokens, ToolKind, Ui, Xf, Xfrm, box_handles, pos2, theme};
use deckcraft_engine::DocState;

/// Crop mode, as in PowerPoint: the part of the picture outside its box shows faded with round
/// handles at its edges (they scale it), and the box carries black crop marks.
fn crop_overlay(
    ctx: &egui::Context,
    doc: &deckcraft_model::Presentation,
    painter: &egui::Painter,
    xf: Xf,
    x: &Xfrm,
    fill: &deckcraft_model::style::PictureFill,
    t: &Tokens,
) {
    use deckcraft_engine::cmd::crop;
    let a = x.affine();
    let to = |q: Point| xf.to_screen(a * q);
    let frame = deckcraft_geom::Rect::new(0.0, 0.0, x.w, x.h);
    if let Some(img) = crop::image_rect(x.w, x.h, fill.crop) {
        if let Some(tex) = crop_ghost(ctx, doc, fill.media) {
            // The image outside the box, in bands above, below, left and right of it.
            let (y0, y1) = (img.y0.max(frame.y0), img.y1.min(frame.y1));
            let bands = [
                (img.x0, img.y0, img.x1, img.y1.min(frame.y0)),
                (img.x0, img.y0.max(frame.y1), img.x1, img.y1),
                (img.x0, y0, img.x1.min(frame.x0), y1),
                (img.x0.max(frame.x1), y0, img.x1, y1),
            ];
            let mut mesh = egui::Mesh::with_texture(tex.id());
            let tint = Color32::from_white_alpha(120);
            for (bx0, by0, bx1, by1) in bands {
                if bx1 - bx0 <= 1e-6 || by1 - by0 <= 1e-6 {
                    continue;
                }
                let base = mesh.vertices.len() as u32;
                for (px, py) in [(bx0, by0), (bx1, by0), (bx1, by1), (bx0, by1)] {
                    let uv = pos2(((px - img.x0) / img.width()) as f32, ((py - img.y0) / img.height()) as f32);
                    mesh.vertices.push(egui::epaint::Vertex { pos: to(Point::new(px, py)), uv, color: tint });
                }
                mesh.add_triangle(base, base + 1, base + 2);
                mesh.add_triangle(base, base + 2, base + 3);
            }
            painter.add(mesh);
        }
        let ic = [Point::new(img.x0, img.y0), Point::new(img.x1, img.y0), Point::new(img.x1, img.y1), Point::new(img.x0, img.y1)].map(to);
        poly(painter, ic, Stroke::new(1.0, Color32::from_gray(150)));
        for q in crop::rect_handles(img) {
            painter.circle(to(q), 4.5, t.handle_fill, Stroke::new(1.0, t.handle_stroke));
        }
    }
    let fc = [Point::new(0.0, 0.0), Point::new(x.w, 0.0), Point::new(x.w, x.h), Point::new(0.0, x.h)].map(to);
    poly(painter, fc, Stroke::new(1.0, t.selection));
    // Crop marks: an L at each corner and a bar mid-side, along the box's edges.
    let dir = |q: Point, d: deckcraft_geom::Vec2| {
        let v = to(q + d) - to(q);
        if v.length() > 1e-3 { v.normalized() } else { egui::Vec2::ZERO }
    };
    let (ux, uy) = (deckcraft_geom::Vec2::new(1.0, 0.0), deckcraft_geom::Vec2::new(0.0, 1.0));
    let len = 12.0;
    for (i, q) in crop::rect_handles(frame).into_iter().enumerate() {
        let c = to(q);
        let arms: Vec<(Pos2, Pos2)> = match i {
            0 => vec![(c, c + dir(q, ux) * len), (c, c + dir(q, uy) * len)],
            2 => vec![(c, c + dir(q, -ux) * len), (c, c + dir(q, uy) * len)],
            4 => vec![(c, c + dir(q, -ux) * len), (c, c + dir(q, -uy) * len)],
            6 => vec![(c, c + dir(q, ux) * len), (c, c + dir(q, -uy) * len)],
            1 | 5 => vec![(c - dir(q, ux) * len * 0.75, c + dir(q, ux) * len * 0.75)],
            _ => vec![(c - dir(q, uy) * len * 0.75, c + dir(q, uy) * len * 0.75)],
        };
        for (s, e) in &arms {
            painter.line_segment([*s, *e], Stroke::new(6.0, Color32::WHITE));
        }
        for (s, e) in arms {
            painter.line_segment([s, e], Stroke::new(4.0, Color32::BLACK));
        }
    }
}

/// The picture drawn whole, for the faded part outside the box in crop mode. Decoded once per
/// picture and kept at most 1024 px on a side; `None` when it doesn't decode.
fn crop_ghost(ctx: &egui::Context, doc: &deckcraft_model::Presentation, media: deckcraft_model::MediaId) -> Option<egui::TextureHandle> {
    const SIDE: u32 = 1024;
    let item = doc.media(media)?;
    let key = egui::Id::unique(("crop-ghost", std::sync::Arc::as_ptr(&item.data) as usize, item.data.len()));
    if let Some(t) = ctx.data(|d| d.get_temp::<Option<egui::TextureHandle>>(key)) {
        return t;
    }
    let tex = deckcraft_render::decode_image(&item.data).and_then(|im| {
        let (w, h) = (im.width(), im.height());
        if w == 0 || h == 0 {
            return None;
        }
        let k = (SIDE as f64 / w.max(h) as f64).min(1.0);
        let (nw, nh) = (((w as f64 * k).round() as u32).max(1), ((h as f64 * k).round() as u32).max(1));
        let raw = im.as_raw();
        let mut px = Vec::with_capacity((nw as usize) * (nh as usize) * 4);
        for y in 0..nh {
            let sy = ((y as u64 * h as u64) / nh as u64).min(h as u64 - 1) as usize;
            for x in 0..nw {
                let sx = ((x as u64 * w as u64) / nw as u64).min(w as u64 - 1) as usize;
                let i = (sy * w as usize + sx) * 4;
                px.extend_from_slice(raw.get(i..i + 4).unwrap_or(&[0, 0, 0, 0]));
            }
        }
        let image = egui::ColorImage::from_rgba_unmultiplied([nw as usize, nh as usize], &px);
        Some(ctx.load_texture("crop-ghost", image, egui::TextureOptions::LINEAR))
    });
    ctx.data_mut(|d| d.insert_temp(key, tex.clone()));
    tex
}

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
        if app.session.tool.cropping(st) == Some(sh.id)
            && let deckcraft_model::ShapeKind::Picture { fill } = &sh.kind
        {
            crop_overlay(painter.ctx(), &st.doc, painter, xf, &x, fill, t);
            continue;
        }
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
