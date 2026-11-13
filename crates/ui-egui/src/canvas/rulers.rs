use super::{Align2, Color32, CornerRadius, Point, Pos2, Rect, Sense, SlideApp, Stroke, Tokens, Ui, Xf, json, pos2, theme, vec2};

/// The ruler's numbered step in points and the ticks per step: quarter inches, or half
/// centimetres. Any other unit (only a hand-edited `prefs.json` gives one) keeps inches, since a
/// tick per point or pixel would be a solid bar.
pub(crate) fn ruler_steps(unit: deckcraft_geom::Unit) -> (f64, u32) {
    match unit {
        deckcraft_geom::Unit::Centimeters => (unit.points_per_unit(), 2),
        _ => (72.0, 4),
    }
}

pub(super) fn ruler(p: &egui::Painter, avail: Rect, slide: Rect, scale: f32, unit: deckcraft_geom::Unit, t: &Tokens) {
    let top = Rect::from_min_max(avail.min, pos2(avail.max.x, avail.min.y + 16.0));
    let left = Rect::from_min_max(avail.min, pos2(avail.min.x + 16.0, avail.max.y));
    p.rect_filled(top, CornerRadius::ZERO, t.card);
    p.rect_filled(left, CornerRadius::ZERO, t.card);
    let (step, sub) = ruler_steps(unit);
    let tick = step as f32 * scale / sub as f32;
    let len = |k: u32| {
        if k.is_multiple_of(sub) {
            8.0
        } else if k.is_multiple_of(2) {
            5.0
        } else {
            3.0
        }
    };
    let mut k = 0;
    loop {
        let x = slide.min.x + k as f32 * tick;
        if x > slide.max.x {
            break;
        }
        let h = len(k);
        p.line_segment([pos2(x, top.max.y - h), pos2(x, top.max.y)], Stroke::new(1.0, t.text_faint));
        if k.is_multiple_of(sub) {
            p.text(pos2(x + 2.0, top.min.y + 1.0), Align2::LEFT_TOP, format!("{}", k / sub), theme::font(8.0), t.text_dim);
        }
        k += 1;
    }
    let mut k = 0;
    loop {
        let y = slide.min.y + k as f32 * tick;
        if y > slide.max.y {
            break;
        }
        let w = len(k);
        p.line_segment([pos2(left.max.x - w, y), pos2(left.max.x, y)], Stroke::new(1.0, t.text_faint));
        k += 1;
    }
}

/// What is being dragged on the ruler.
#[derive(Clone, Copy, Debug, PartialEq)]
enum RulerDrag {
    /// First-line indent (the top triangle).
    First,
    /// Hanging indent (the bottom triangle): moves the left indent, the first line stays put.
    Hanging,
    /// Left indent (the box under the hanging triangle): moves both together.
    Left,
    /// The tab stop that was at this position (pt from the text area's left).
    Tab(f64),
}

/// The paragraph's indents and tab stops on the horizontal ruler while text is being edited,
/// draggable as in PowerPoint; a click on the ruler adds a left tab, dragging a tab off the
/// ruler removes it.
pub(super) fn ruler_markers(app: &mut SlideApp, ui: &mut Ui, p: &egui::Painter, avail: Rect, xf: Xf, t: &Tokens) {
    if app.session.active().is_none_or(|d| d.selection.text.is_none()) {
        return;
    }
    let Ok(info) = app.session.execute("format.ruler", &json!({})) else { return };
    if info["rotated"].as_bool().unwrap_or(true) {
        return;
    }
    let f = |k: &str| info[k].as_f64().filter(|v| v.is_finite()).unwrap_or(0.0);
    let (x0, width, ml, ind) = (f("x"), f("width"), f("marginLeft"), f("indent"));
    let tabs: Vec<(f64, String)> = info["tabs"]
        .as_array()
        .map(|a| a.iter().filter_map(|t| Some((t["pos"].as_f64()?, t["align"].as_str().unwrap_or("l").to_string()))).collect())
        .unwrap_or_default();
    let top = Rect::from_min_max(pos2(avail.min.x + 16.0, avail.min.y), pos2(avail.max.x, avail.min.y + 16.0));
    let sx = |q: f64| xf.to_screen(Point::new(x0 + q, 0.0)).x;
    let to_q = |x: f32| ((xf.to_slide(pos2(x, 0.0)).x - x0).clamp(0.0, width.max(0.0)) * 2.0).round() / 2.0;
    // The text area's span, outlined on the ruler.
    let area = Rect::from_min_max(pos2(sx(0.0), top.min.y + 2.0), pos2(sx(width), top.max.y - 1.0)).intersect(top);
    p.rect_stroke(area, CornerRadius::ZERO, Stroke::new(1.0, t.text_faint.gamma_multiply(0.5)), egui::StrokeKind::Inside);

    let resp = ui.interact(top, ui.scope_id().with("ruler_markers"), Sense::click_and_drag());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Unknown, true, "Text ruler"));
    let id = ui.scope_id().with("ruler_drag");
    let mut drag: Option<RulerDrag> = ui.data(|d| d.get_temp::<RulerDrag>(id));
    let pointer = resp.interact_pointer_pos().or(resp.hover_pos());
    if resp.drag_started()
        && let Some(at) = ui.input(|i| i.pointer.press_origin()).or(resp.interact_pointer_pos())
    {
        let near = |q: f64| (sx(q) - at.x).abs() <= 5.0;
        let y = at.y - top.min.y;
        drag = if let Some((pos, _)) = tabs.iter().find(|(pos, _)| near(*pos) && y >= 8.0) {
            Some(RulerDrag::Tab(*pos))
        } else if y < 6.0 && near(ml + ind) {
            Some(RulerDrag::First)
        } else if (6.0..11.0).contains(&y) && near(ml) {
            Some(RulerDrag::Hanging)
        } else if y >= 11.0 && near(ml) {
            Some(RulerDrag::Left)
        } else {
            None
        };
        ui.data_mut(|d| match drag {
            Some(v) => {
                d.insert_temp(id, v);
            }
            None => {
                d.remove::<RulerDrag>(id);
            }
        });
    }
    // Where each marker is drawn: the dragged one follows the pointer.
    let live = drag.zip(pointer.filter(|_| resp.dragged())).map(|(d, at)| (d, to_q(at.x), at.y > top.max.y + 20.0));
    let (mut first, mut left) = (ml + ind, ml);
    match live {
        Some((RulerDrag::First, q, _)) => first = q,
        Some((RulerDrag::Hanging, q, _)) => left = q,
        Some((RulerDrag::Left, q, _)) => {
            first = q + ind;
            left = q;
        }
        _ => {}
    }
    let col = t.text_dim;
    let tri = |x: f32, y0: f32, down: bool| {
        let (a, b) = if down { (y0, y0 + 5.0) } else { (y0 + 5.0, y0) };
        p.add(egui::Shape::convex_polygon(vec![pos2(x - 4.0, a), pos2(x + 4.0, a), pos2(x, b)], t.card, Stroke::new(1.0, col)));
    };
    tri(sx(first), top.min.y + 1.0, true);
    tri(sx(left), top.min.y + 6.0, false);
    p.rect(
        Rect::from_min_max(pos2(sx(left) - 4.0, top.min.y + 11.5), pos2(sx(left) + 4.0, top.max.y - 0.5)),
        CornerRadius::ZERO,
        t.card,
        Stroke::new(1.0, col),
        egui::StrokeKind::Inside,
    );
    for (pos, align) in &tabs {
        let (q, gone) = match live {
            Some((RulerDrag::Tab(from), q, off)) if (from - pos).abs() < 0.5 => (q, off),
            _ => (*pos, false),
        };
        if gone {
            continue;
        }
        tab_glyph(p, pos2(sx(q), top.max.y - 2.0), align, t.text);
    }
    // The tab selector in the ruler's corner: a click cycles left, center, right, decimal; a click
    // on the ruler then sets that kind of stop.
    let kind_id = ui.scope_id().with("ruler_tab_kind");
    let mut kind: usize = ui.data(|d| d.get_temp(kind_id)).unwrap_or(0);
    let corner = Rect::from_min_size(avail.min, vec2(16.0, 16.0));
    let cr = ui.interact(corner, ui.scope_id().with("ruler_tab_selector"), Sense::click());
    const KINDS: [(&str, &str); 4] = [("l", "Left Tab"), ("ctr", "Center Tab"), ("r", "Right Tab"), ("dec", "Decimal Tab")];
    if cr.clicked() {
        kind = (kind + 1) % KINDS.len();
        ui.data_mut(|d| d.insert_temp(kind_id, kind));
    }
    let (kind_align, kind_label) = KINDS[kind % KINDS.len()];
    cr.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, kind_label));
    let cr = cr.on_hover_text(format!("{kind_label} (click to change)"));
    p.rect_stroke(
        corner.shrink(2.0),
        CornerRadius::same(2),
        Stroke::new(1.0, if cr.hovered() { t.accent } else { t.text_faint }),
        egui::StrokeKind::Inside,
    );
    tab_glyph(p, pos2(corner.center().x - 1.0, corner.center().y + 3.0), kind_align, t.text);
    if drag.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }

    // Commit on release; a click on bare ruler adds a left tab.
    if resp.drag_stopped() {
        if let (Some(d), Some(at)) = (drag, pointer) {
            let q = to_q(at.x);
            let r = match d {
                RulerDrag::First => app.run("format.paragraph", json!({"indentFirst": q - ml})),
                RulerDrag::Hanging => app.run("format.paragraph", json!({"indentLeft": q, "indentFirst": ml + ind - q})),
                RulerDrag::Left => app.run("format.paragraph", json!({"indentLeft": q})),
                RulerDrag::Tab(from) if at.y > top.max.y + 20.0 => app.run("format.tabs", json!({"remove": from})),
                RulerDrag::Tab(from) => app.run("format.tabs", json!({"move": {"from": from, "to": q}})),
            };
            if let Err(e) = r {
                app.set_status(e);
            }
        }
        ui.data_mut(|d| d.remove::<RulerDrag>(id));
    } else if resp.clicked()
        && let Some(at) = resp.interact_pointer_pos()
        && at.x >= area.min.x
        && at.x <= area.max.x
    {
        let near_tab = tabs.iter().any(|(pos, _)| (sx(*pos) - at.x).abs() <= 5.0);
        if resp.double_clicked() || near_tab {
            // A double-click on a stop opens the Tabs dialog, as in PowerPoint.
            if resp.double_clicked() {
                app.dialog = Some(crate::dialogs::Dialog::new("tabs"));
            }
        } else {
            let _ = app.run("format.tabs", json!({"add": {"pos": to_q(at.x), "align": kind_align}}));
        }
    }
}

/// A tab-stop mark with its foot at `at`: └ left, ┴ center, ┘ right, ┴ with a dot decimal.
fn tab_glyph(p: &egui::Painter, at: Pos2, align: &str, col: Color32) {
    let (x, y) = (at.x, at.y);
    let st = Stroke::new(1.5, col);
    p.line_segment([pos2(x, y - 6.0), pos2(x, y)], st);
    match align {
        "r" => {
            p.line_segment([pos2(x - 5.0, y), pos2(x, y)], st);
        }
        "ctr" => {
            p.line_segment([pos2(x - 4.0, y), pos2(x + 4.0, y)], st);
        }
        "dec" => {
            p.line_segment([pos2(x - 4.0, y), pos2(x + 4.0, y)], st);
            p.circle_filled(pos2(x + 3.0, y - 4.0), 1.0, col);
        }
        _ => {
            p.line_segment([pos2(x, y), pos2(x + 5.0, y)], st);
        }
    }
}
