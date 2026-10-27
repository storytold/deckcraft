use super::*;

/// A two-finger twist on the trackpad. macOS delivers it as [`egui::Event::Rotate`];
/// other platforms leave the delta at zero.
///
/// The selected shapes turn with the fingers. The whole twist is one undo step. Pinch-zoom
/// and two-finger scrolling are left as they are.
pub(crate) fn trackpad_twist(app: &mut SlideApp, ctx: &egui::Context) {
    if ctx.will_discard() {
        // Multi-pass layout runs this function again with the input already consumed. Acting on
        // the discarded pass would start the twist and the replay would immediately end it,
        // splitting one gesture into two undo steps.
        return;
    }
    let delta = ctx.input(|i| i.rotation_delta());
    let delta = if delta.is_finite() { delta.clamp(-std::f32::consts::PI, std::f32::consts::PI) } else { 0.0 };
    let turning = delta.abs() > 1e-4;
    let blocked = app.show.is_some()
        || app.backstage_open()
        || app.dialog.as_ref().is_some_and(|d| d.modal())
        || app.palette.is_some()
        || app.session.tool.dragging();
    let over = app.canvas_rect.is_some_and(|r| ctx.input(|i| i.pointer.hover_pos()).is_some_and(|p| r.contains(p)));
    let rotatable = app.session.active().is_some_and(|d| {
        d.selection.text.is_none() && d.selection.shapes.iter().any(|id| d.shape(*id).is_some_and(|sh| !sh.locked && sh.xfrm.is_some()))
    });
    if !blocked && turning && rotatable && (app.twisting || over) {
        let _ = app.run("shape.rotate", json!({"by": f64::from(delta.to_degrees()), "gesture": "move"}));
        app.twisting = true;
    } else if app.twisting {
        let _ = app.run("shape.rotate", json!({"gesture": "end", "ids": []}));
        app.twisting = false;
    }
}

/// A trackpad flick or mouse-wheel notch that should turn the page.
///
/// The stronger axis wins, so a sideways swipe is not read as a vertical scroll. Negative
/// movement — the content moves left or up, which is a swipe left or up with natural scrolling —
/// is the next slide. A nudge under 18 points is ignored so a resting finger does not change slides.
pub(crate) fn page_flick(wheel: egui::Vec2) -> Option<bool> {
    const MIN_FLICK: f32 = 18.0;
    let along = if wheel.x.abs() > wheel.y.abs() { wheel.x } else { wheel.y };
    (along.abs() > MIN_FLICK).then_some(along < 0.0)
}

/// Discrete wheel input must not share the trackpad's gesture cooldown. Keep all the steps,
/// including several notches delivered in one frame, and clamp at each end of the deck.
///
/// High-resolution and free-spin wheels send fractions of a line (0.125 per event on Windows), so
/// the delta is accumulated in `carry` and each whole line is one slide; a change of direction
/// drops what was left over. Returns the target, `None` when no wheel event was read.
pub(super) fn wheel_slide_target(events: &[egui::Event], current: usize, slides: usize, carry: &mut f32) -> Option<usize> {
    let last = slides.checked_sub(1)?;
    let mut target = current.min(last);
    let mut handled = false;
    for event in events {
        if let egui::Event::MouseWheel { unit, delta, modifiers, .. } = event
            && matches!(unit, egui::MouseWheelUnit::Line | egui::MouseWheelUnit::Page)
            && !modifiers.ctrl
            && !modifiers.command
            && !modifiers.shift
            && delta.y.is_finite()
            && delta.y.abs() > 0.0
            && delta.y.abs() >= delta.x.abs()
        {
            if *carry * delta.y < 0.0 {
                *carry = 0.0;
            }
            *carry += delta.y;
            // The small slack keeps sums such as 10 × 0.1 from falling just short of a line.
            let whole = (carry.abs() + 1e-4).floor();
            *carry -= whole.copysign(*carry);
            let steps = whole.min(slides as f32) as usize;
            target = if delta.y < 0.0 { target.saturating_add(steps).min(last) } else { target.saturating_sub(steps) };
            handled = true;
        }
    }
    handled.then_some(target)
}

pub(super) fn trackpad_scroll(events: &[egui::Event]) -> egui::Vec2 {
    events.iter().fold(egui::Vec2::ZERO, |sum, event| match event {
        egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta, modifiers, .. } if !modifiers.ctrl && !modifiers.command => sum + *delta,
        _ => sum,
    })
}

pub(super) fn pointer(app: &mut SlideApp, ui: &Ui, resp: &egui::Response, xf: Xf) {
    if crate::eyedropper::canvas(app, ui, resp, xf) {
        return;
    }
    if app.anim_preview_active() {
        // A click on the slide ends the preview, then selects as usual.
        if resp.hovered() && ui.input(|i| i.pointer.primary_pressed()) {
            app.stop_anim_preview();
        } else {
            return;
        }
    }
    let tol = (4.0 / xf.scale) as f64;
    let m = mods(ui);
    let Some(pos) = resp.interact_pointer_pos().or(resp.hover_pos()) else { return };
    // The rulers have their own controls; a press there mustn't reach the slide (and end text
    // editing). A drag already under way keeps going over them.
    if app.ui.ruler && !app.session.tool.dragging() && (pos.y < resp.rect.min.y + 16.0 || pos.x < resp.rect.min.x + 16.0) {
        return;
    }
    let p = xf.to_slide(pos);
    let pressure = reported_pressure(ui);
    let ev = |kind| PointerEvent { kind, x: p.x, y: p.y, mods: m, tol, pressure };
    let primary = ui.input(|i| i.pointer.primary_pressed());
    if resp.hovered() && !app.session.tool.dragging() {
        let _ = app.session.pointer(ev(PointerKind::Move));
    }
    let secondary = resp.hovered() && ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Secondary));
    let r = if secondary && !app.session.tool.dragging() {
        // Select what's under the pointer before the context menu opens (on release).
        app.session.pointer(ev(PointerKind::ContextClick))
    } else if resp.triple_clicked() {
        app.session.pointer(ev(PointerKind::TripleClick))
    } else if resp.double_clicked() {
        app.session.pointer(ev(PointerKind::DoubleClick))
    } else if primary && resp.hovered() && !resp.secondary_clicked() && ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Primary)) {
        app.session.pointer(ev(PointerKind::Down))
    } else if resp.dragged_by(egui::PointerButton::Primary) || (app.session.tool.dragging() && ui.input(|i| i.pointer.primary_down())) {
        app.session.pointer(ev(PointerKind::Drag))
    } else if app.session.tool.dragging() && ui.input(|i| i.pointer.primary_released() || !i.pointer.primary_down()) {
        app.session.pointer(ev(PointerKind::Up))
    } else {
        Ok(serde_json::Value::Null)
    };
    if let Err(e) = r {
        app.set_status(e.to_string());
    }
    // Cursor. The Format and Animation Painters override everything: their whole point is to
    // tell the user the tool is armed and waiting for a click (#830).
    let painter_armed = app.session.painter.is_some() || app.session.anim_painter.is_some();
    let cursor = if painter_armed {
        egui::CursorIcon::Copy
    } else {
        match (&app.session.tool.kind, &app.session.tool.hover) {
            (ToolKind::Shape { .. }, _) => egui::CursorIcon::Crosshair,
            (ToolKind::TextBox, _) => egui::CursorIcon::Text,
            (ToolKind::Ink { .. }, _) => egui::CursorIcon::Crosshair,
            (_, Some(Hit::Resize { id, handle })) => resize_cursor(&app.session, *id, *handle),
            (_, Some(Hit::Rotate { .. })) => egui::CursorIcon::Alias,
            (_, Some(Hit::TableColumn { .. })) => egui::CursorIcon::ResizeColumn,
            (_, Some(Hit::TableRow { .. })) => egui::CursorIcon::ResizeRow,
            (_, Some(Hit::Adjust { .. } | Hit::Warp { .. } | Hit::LineEnd { .. })) => egui::CursorIcon::PointingHand,
            (_, Some(Hit::Shape { text: true, .. })) => {
                if app.session.active().is_some_and(|d| d.selection.text.is_some()) {
                    egui::CursorIcon::Text
                } else {
                    egui::CursorIcon::Move
                }
            }
            (_, Some(Hit::Shape { .. } | Hit::Frame { .. })) => egui::CursorIcon::Move,
            _ => match app.session.tool.guide_drag.map(|g| g.0).or(app.session.tool.guide_hover) {
                Some(i) if app.session.active().and_then(|d| d.doc.guides.get(i)).is_some_and(|g| g.vertical) => egui::CursorIcon::ResizeHorizontal,
                Some(_) => egui::CursorIcon::ResizeVertical,
                None => egui::CursorIcon::Default,
            },
        }
    };
    if resp.hovered() {
        ui.ctx().set_cursor_icon(cursor);
    }
    if app.session.tool.dragging() {
        ui.ctx().request_repaint();
    }
}

/// Pressure from a touch or pen this frame (`0..=1`). A mouse reports none.
fn reported_pressure(ui: &Ui) -> Option<f32> {
    ui.input(|i| {
        i.events.iter().rev().find_map(|e| match e {
            egui::Event::Touch { force: Some(f), phase, .. }
                if matches!(phase, egui::TouchPhase::Start | egui::TouchPhase::Move) && f.is_finite() =>
            {
                Some(f.clamp(0.0, 1.0))
            }
            _ => None,
        })
    })
}

/// The resize cursor for `handle` of shape `id`: the handle's resize axis is computed in the
/// shape's own space and mapped through its transform, so a rotated or flipped shape shows the
/// diagonal that matches where the drag actually goes (#745). Without a transform the mapping
/// reduces to the classic NW/NE/E/SE/S/SW/W assignment.
pub(super) fn resize_cursor(session: &deckcraft_engine::Session, id: deckcraft_model::ShapeId, handle: u8) -> egui::CursorIcon {
    // The resize handles are drawn from the shape's effective box (a placeholder inherits it
    // from its layout), so the cursor follows the same one.
    let Some(d) = session.active() else { return egui::CursorIcon::Default };
    let Some(sh) = d.shape(id) else { return egui::CursorIcon::Default };
    let x = deckcraft_engine::cmd::xfrm_of(&d.doc, &d.selection, sh);
    // The resize axis in the shape's own coordinates, as (dx, dy) from the box centre toward
    // the handle. Corners resize along the diagonal, edge handles along their edge. The
    // direction is normalised - the diagonal at 45 degrees whatever the shape's proportions -
    // so a wide rectangle still shows a diagonal cursor on its corners.
    let (dx, dy): (f64, f64) = match handle {
        0 => (-1.0, -1.0),
        1 => (0.0, -1.0),
        2 => (1.0, -1.0),
        3 => (1.0, 0.0),
        4 => (1.0, 1.0),
        5 => (0.0, 1.0),
        6 => (-1.0, 1.0),
        _ => (-1.0, 0.0),
    };
    // Map through the linear part of the transform (rotation, flips): two points along the
    // axis, the difference is the slide-space direction.
    let centre = Point::new(x.x + x.w / 2.0, x.y + x.h / 2.0);
    let a = x.affine();
    let p0 = a * centre;
    let p1 = a * Point::new(centre.x + dx, centre.y + dy);
    let v = p1 - p0;
    if !v.x.is_finite() || !v.y.is_finite() || (v.x.abs() < 1e-12 && v.y.abs() < 1e-12) {
        return egui::CursorIcon::Default;
    }
    // Angle in [0, 180): 0° = right (horizontal), 45° = down-right (NW-SE), 90° = down
    // (vertical), 135° = down-left (NE-SW). Pick the closest of the four.
    let mut deg = v.y.atan2(v.x).to_degrees();
    if deg < 0.0 {
        deg += 180.0;
    }
    if (22.5..67.5).contains(&deg) {
        egui::CursorIcon::ResizeNwSe
    } else if (67.5..112.5).contains(&deg) {
        egui::CursorIcon::ResizeVertical
    } else if (112.5..157.5).contains(&deg) {
        egui::CursorIcon::ResizeNeSw
    } else {
        egui::CursorIcon::ResizeHorizontal
    }
}
