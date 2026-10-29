use super::{
    Align2, AnimClass, AnimStart, Color32, CornerRadius, Icon, Rect, Sense, SlideApp, Stroke, Tokens, Ui, animtimeline, icons, json, paint_anim_tile,
    pos2, run, theme, vec2,
};

/// Render the animation pane and route its controls through the engine's animation commands.
pub(super) fn animation(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(st) = app.session.active() else { return };
    let Some(slide) = st.current_slide() else { return };
    let anims = slide.animations.clone();
    let size = (st.doc.slide_size.width, st.doc.slide_size.height);
    let tl = deckcraft_anim::Timeline::new(slide, size.0, size.1);
    let effects = tl.effects();
    let names: Vec<(u32, String)> = st.shapes().iter().map(|s| (s.id.0, s.name.clone())).collect();
    let selected: Vec<u32> = st.selection.shapes.iter().map(|i| i.0).collect();
    // Play From starts at the effect last clicked here, or else the first effect of the selected
    // shape; with nothing selected it is Play All. Reordering or removing forgets the click: the
    // index would point at another effect.
    let pick_id = egui::Id::unique(("anim_pane_pick", st.uid, st.selection.slide));
    let pick = ui.data(|m| m.get_temp::<usize>(pick_id));
    // A triggered effect is not on the click sequence, so it can't be played from.
    let eligible = |i: usize| anims.get(i).is_some_and(|a| selected.contains(&a.shape.0)) && tl.start_of(i).is_some();
    let from = pick.filter(|&i| eligible(i)).or_else(|| (0..anims.len()).find(|&i| eligible(i)));
    ui.horizontal(|ui| {
        let playing = app.anim_preview_active();
        let label = match (playing, from) {
            (true, _) => "■ Stop",
            (false, Some(_)) => "▶ Play From",
            (false, None) => "▶ Play All",
        };
        if ui.button(label).clicked() {
            let p = match (playing, from) {
                (false, Some(i)) => json!({"on": true, "from": i}),
                _ => json!({"on": !playing}),
            };
            run(app, "animation.preview", p);
        }
    });
    ui.separator();
    if anims.is_empty() {
        ui.label(egui::RichText::new("Select an object on the slide and add an animation from the Animations tab.").color(t.text_dim));
        return;
    }
    let bars = animtimeline::bars(anims.len(), &effects);
    let seconds = animtimeline::ruler_seconds(&bars);
    let track_w = (ui.available_width() * 0.45).max(60.0);
    let pps = animtimeline::pixels_per_second(track_w, seconds);
    let mut click = 0;
    for (i, a) in anims.iter().enumerate() {
        if a.start == AnimStart::OnClick {
            click += 1;
        }
        let name = names.iter().find(|(id, _)| *id == a.shape.0).map(|(_, n)| n.clone()).unwrap_or_else(|| format!("Shape {}", a.shape));
        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
        let sel = selected.contains(&a.shape.0);
        if sel || resp.hovered() {
            ui.painter().rect_filled(r, CornerRadius::same(4), if sel { t.selected } else { t.hover });
        }
        if from == Some(i) {
            ui.painter().rect_stroke(r, CornerRadius::same(4), Stroke::new(1.5, t.accent), egui::StrokeKind::Inside);
        }
        let p = ui.painter();
        p.text(
            pos2(r.min.x + 6.0, r.center().y),
            Align2::LEFT_CENTER,
            if a.start == AnimStart::OnClick { click.to_string() } else { String::new() },
            theme::font(12.0),
            t.text_dim,
        );
        let icon = match a.class {
            AnimClass::Entrance => Icon::AnimEntrance,
            AnimClass::Emphasis => Icon::AnimEmphasis,
            AnimClass::Exit => Icon::AnimExit,
            _ => Icon::AnimPath,
        };
        icons::paint(p, Rect::from_center_size(pos2(r.min.x + 30.0, r.center().y), vec2(18.0, 18.0)), icon, t.text, false);
        p.text(pos2(r.min.x + 44.0, r.center().y), Align2::LEFT_CENTER, &name, theme::font(12.5), t.text);
        // Advanced timeline bar: drag the body to change the delay, the right edge for the duration.
        if let Some(Some(b)) = bars.get(i) {
            let col = match a.class {
                AnimClass::Entrance => Color32::from_rgb(0x2E, 0xA3, 0x6F),
                AnimClass::Emphasis => Color32::from_rgb(0xE3, 0xB2, 0x1C),
                AnimClass::Exit => Color32::from_rgb(0xD8, 0x3F, 0x6B),
                _ => Color32::from_rgb(0x2E, 0x6F, 0xD8),
            };
            let x0 = r.max.x - track_w - 6.0;
            let x_of = |sec: f64| x0 + (sec as f32 * pps).min(track_w);
            let mut bar = Rect::from_min_max(
                pos2(x_of(b.start), r.center().y - 5.0),
                pos2(x_of(b.start + b.span).max(x_of(b.start) + 4.0), r.center().y + 5.0),
            );
            let edge = Rect::from_min_max(pos2(bar.max.x - 3.0, bar.min.y - 3.0), pos2(bar.max.x + 4.0, bar.max.y + 3.0));
            let body_resp = ui.interact(bar, ui.scope_id().with(("anim_bar", i)), Sense::drag()).on_hover_text(animtimeline::label(b));
            body_resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Unknown, true, format!("Timing bar {}", i + 1)));
            let edge_resp = ui.interact(edge, ui.scope_id().with(("anim_edge", i)), Sense::drag());
            edge_resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Unknown, true, format!("Duration handle {}", i + 1)));
            if edge_resp.hovered() || edge_resp.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            } else if body_resp.hovered() || body_resp.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            }
            // The drag offset is remembered: the pointer's press origin is gone on release.
            let dx_id = ui.scope_id().with(("anim_drag_dx", i));
            let live = ui.input(|inp| match (inp.pointer.press_origin(), inp.pointer.interact_pos()) {
                (Some(o), Some(p)) => Some(p.x - o.x),
                _ => None,
            });
            let dx = match live {
                Some(d) if body_resp.dragged() || edge_resp.dragged() => {
                    ui.data_mut(|m| m.insert_temp(dx_id, d));
                    d
                }
                _ => ui.data_mut(|m| m.get_temp::<f32>(dx_id)).unwrap_or(0.0),
            };
            if body_resp.drag_stopped() || edge_resp.drag_stopped() {
                ui.data_mut(|m| m.remove::<f32>(dx_id));
            }
            let iter_secs = f64::from(a.duration_ms) / 1000.0;
            let factor = if iter_secs > 0.0 && b.span > 0.0 { b.span / iter_secs } else { 1.0 };
            if edge_resp.dragged() || edge_resp.drag_stopped() {
                let d = animtimeline::dragged_duration(a.duration_ms, dx, pps, factor);
                bar.max.x = x_of(b.start + f64::from(d) / 1000.0 * factor).max(bar.min.x + 4.0);
                if edge_resp.drag_stopped() && d != a.duration_ms {
                    run(app, "animation.timing", json!({"index": i, "duration": d}));
                }
            } else if body_resp.dragged() || body_resp.drag_stopped() {
                let d = animtimeline::dragged_delay(a.delay_ms, dx, pps);
                let shift = (f64::from(d) - f64::from(a.delay_ms)) / 1000.0;
                let w = bar.width();
                bar.min.x = x_of((b.start + shift).max(0.0));
                bar.max.x = bar.min.x + w;
                if body_resp.drag_stopped() && d != a.delay_ms {
                    run(app, "animation.timing", json!({"index": i, "delay": d}));
                }
            }
            let p = ui.painter();
            p.rect_filled(bar, CornerRadius::same(2), col);
            if a.repeat == u32::MAX {
                p.text(pos2(bar.max.x + 2.0, bar.center().y), Align2::LEFT_CENTER, "⟳", theme::font(10.0), t.text_dim);
            }
        }
        if resp.clicked() {
            ui.data_mut(|m| m.insert_temp(pick_id, i));
            run(app, "edit.select", json!({"ids": [a.shape.0]}));
        }
        resp.context_menu(|ui| {
            for (l, v) in [("Start On Click", "onClick"), ("Start With Previous", "withPrevious"), ("Start After Previous", "afterPrevious")] {
                if ui.button(l).clicked() {
                    run(app, "animation.timing", json!({"index": i, "start": v}));
                    ui.close();
                }
            }
            ui.separator();
            if ui.button("Move Up").clicked() {
                ui.data_mut(|m| m.remove::<usize>(pick_id));
                run(app, "animation.move", json!({"index": i, "to": i.saturating_sub(1)}));
                ui.close();
            }
            if ui.button("Move Down").clicked() {
                ui.data_mut(|m| m.remove::<usize>(pick_id));
                run(app, "animation.move", json!({"index": i, "to": i + 1}));
                ui.close();
            }
            if ui.button("Remove").clicked() {
                ui.data_mut(|m| m.remove::<usize>(pick_id));
                run(app, "animation.remove", json!({"index": i}));
                ui.close();
            }
        });
    }
    // Seconds ruler under the bars.
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 18.0), Sense::hover());
    let x0 = r.max.x - track_w - 6.0;
    let p = ui.painter();
    p.line_segment([pos2(x0, r.min.y + 2.0), pos2(x0 + track_w, r.min.y + 2.0)], Stroke::new(1.0, t.text_faint));
    let every = (seconds / 6).max(1);
    for s in (0..=seconds).step_by(every as usize) {
        let x = x0 + s as f32 * pps;
        if x > x0 + track_w + 0.5 {
            break;
        }
        p.line_segment([pos2(x, r.min.y), pos2(x, r.min.y + 5.0)], Stroke::new(1.0, t.text_faint));
        p.text(pos2(x, r.min.y + 6.0), Align2::CENTER_TOP, format!("{s}s"), theme::font(10.0), t.text_dim);
    }
    let _ = paint_anim_tile;
}
