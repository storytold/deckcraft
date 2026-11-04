use super::arrange::{arrange_group, size_group};
use super::galleries::{cref_param, shapes_gallery};
use super::{
    Color32, CornerRadius, Icon, Rect, SlideApp, Stroke, Ui, Value, big, big_button, color_split, enabled, group, json, pos2, rows, run, scheme,
    small, small_button_ex, theme, vec2, widgets,
};

/// Render the Picture Format tab's corrections, colour, artistic effects, transparency, styles,
/// border, and picture actions.
pub(super) fn picture_format(app: &mut SlideApp, ui: &mut Ui) {
    group(ui, |ui| {
        let r = big_button(ui, Icon::RemoveBackground, "Remove\nBackground", enabled(app, "picture.removeBackground"));
        egui::Popup::menu(&r).show(|ui| {
            for (label, tol) in [("Remove Background", 20.0), ("Remove More (looser)", 35.0), ("Remove Less (tighter)", 10.0)] {
                if ui.button(label).clicked() {
                    run(app, "picture.removeBackground", json!({"tolerance": tol}));
                }
            }
        });
        let r = big_button(ui, Icon::Corrections, "Corrections", true);
        egui::Popup::menu(&r).show(|ui| {
            for b in [-0.4, -0.2, 0.0, 0.2, 0.4] {
                if ui.button(format!("Brightness {:+}%", (b * 100.0) as i32)).clicked() {
                    run(app, "picture.adjust", json!({"brightness": b}));
                }
            }
            ui.separator();
            for c in [-0.4, -0.2, 0.0, 0.2, 0.4] {
                if ui.button(format!("Contrast {:+}%", (c * 100.0) as i32)).clicked() {
                    run(app, "picture.adjust", json!({"contrast": c}));
                }
            }
        });
        let r = big_button(ui, Icon::ColorAdjust, "Color", true);
        egui::Popup::menu(&r).show(|ui| {
            for s in [0.0, 0.33, 0.66, 1.0, 1.5, 2.0, 3.0] {
                if ui.button(format!("Saturation {}%", (s * 100.0) as i32)).clicked() {
                    run(app, "picture.adjust", json!({"saturation": s}));
                }
            }
            ui.separator();
            ui.label("Recolor");
            let sc = scheme(app);
            let presets = deckcraft_engine::cmd::recolor::presets();
            let names: Vec<String> = presets.iter().map(|(_, l)| l.clone()).collect();
            if let Some(i) = widgets::gallery(
                ui,
                presets.len(),
                vec2(40.0, 30.0),
                None,
                |p, r, i| {
                    if let Some((id, _)) = presets.get(i) {
                        paint_recolor(p, r, id, &sc);
                    }
                },
                |i| names.get(i).cloned().unwrap_or_default(),
            ) && let Some((id, _)) = presets.get(i)
            {
                run(app, "picture.recolor", json!({"preset": id}));
            }
            ui.separator();
            // As in PowerPoint, the pointer picks the colour in the picture itself: a colour
            // chosen from a grid almost never matches a photo's pixels.
            if ui.button("Set Transparent Color").on_hover_text("Click a colour in the picture to make it transparent").clicked() {
                crate::eyedropper::request_transparent_color(ui.ctx());
                ui.close();
            }
        });
        let r = big_button(ui, Icon::ArtisticEffects, "Artistic\nEffects", enabled(app, "picture.artisticEffect"));
        egui::Popup::menu(&r).show(|ui| {
            if ui.button("None").clicked() {
                run(app, "picture.artisticEffect", json!({"effect": "none"}));
            }
            for (e, l) in deckcraft_engine::cmd::artistic::EFFECTS {
                if ui.button(*l).clicked() {
                    run(app, "picture.artisticEffect", json!({"effect": e}));
                }
            }
        });
        let r = big_button(ui, Icon::Transparency, "Transparency", true);
        egui::Popup::menu(&r).show(|ui| {
            for tr in [0.0, 0.15, 0.3, 0.5, 0.65, 0.8, 0.95] {
                if ui.button(format!("{}%", (tr * 100.0) as i32)).clicked() {
                    run(app, "picture.adjust", json!({"transparency": tr}));
                }
            }
        });
        rows(ui, |ui| {
            small(app, ui, Icon::ChangePicture, "Change Picture", "app.changePictureDialog", json!({}));
            small(app, ui, Icon::ResetPicture, "Reset Picture", "picture.reset", json!({}));
            let (main, arrow) = small_button_ex(ui, Icon::CompressPictures, "Compress Pictures", enabled(app, "picture.compress"), false, true);
            if let Some(arrow) = arrow {
                widgets::split_popup(&main, &arrow)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .width(260.0)
                    .show(|ui| compress_menu(app, ui));
            }
        });
    });
    group(ui, |ui| {
        use deckcraft_engine::cmd::picture_style::{STYLES, look};
        let names: Vec<String> = STYLES.iter().map(|(_, l)| l.to_string()).collect();
        let tiles = |ui: &mut Ui, n: usize| {
            widgets::gallery(
                ui,
                n,
                STYLE_TILE,
                None,
                |p, r, i| paint_picture_style(p, r, STYLES.get(i).and_then(|(id, _)| look(id))),
                |i| names.get(i).cloned().unwrap_or_default(),
            )
        };
        let mut picked = tiles(ui, inline_styles(ui));
        // Every style, in rows, behind the gallery's arrow (PowerPoint's More).
        let more = widgets::drop_button(ui, "", vec2(14.0, 52.0), true);
        more.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "More Picture Styles"));
        // Six to a row: the 24 styles in four rows.
        egui::Popup::menu(&more).width(6.0 * (STYLE_TILE.x + 2.0) + 16.0).show(|ui| {
            ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
            ui.horizontal_wrapped(|ui| {
                if let Some(i) = tiles(ui, STYLES.len()) {
                    picked = Some(i);
                    ui.close();
                }
            });
        });
        if let Some((id, _)) = picked.and_then(|i| STYLES.get(i)) {
            run(app, "picture.style", json!({"style": id}));
        }
    });
    group(ui, |ui| {
        let sc = scheme(app);
        let (m, a) = color_split(ui, Icon::QuickStyles, "Picture Border", Color32::BLACK, true);
        if m.clicked() {
            run(app, "shape.line", json!({"color": "#000000", "width": 1.0}));
        }
        widgets::color_popup(&a).show(|ui| {
            if let Some(c) = widgets::color_grid(ui, &sc, Some("No Outline")) {
                match c {
                    Some(c) => run(app, "shape.line", json!({"color": cref_param(&c)})),
                    None => run(app, "shape.line", json!({"none": true})),
                }
            }
            ui.separator();
            ui.menu_button("Weight", |ui| {
                for w in [0.25, 0.5, 0.75, 1.0, 1.5, 2.25, 3.0, 4.5, 6.0] {
                    if ui.button(format!("{w} pt")).clicked() {
                        run(app, "shape.line", json!({"width": w}));
                    }
                }
            });
            ui.menu_button("Dashes", |ui| {
                for (l, d) in [
                    ("Solid", "solid"),
                    ("Round Dot", "sysDot"),
                    ("Square Dot", "sysDash"),
                    ("Dash", "dash"),
                    ("Dash Dot", "dashDot"),
                    ("Long Dash", "lgDash"),
                ] {
                    if ui.button(l).clicked() {
                        run(app, "shape.line", json!({"dash": d}));
                    }
                }
            });
        });
        if ui.small_button("Reset Style").on_hover_text("Remove the picture style").clicked() {
            run(app, "picture.style", json!({"style": "none"}));
        }
        big(app, ui, Icon::AltText, "Alt\nText", "app.dialog", json!({"id": "altText"}));
    });
    arrange_group(app, ui);
    group(ui, |ui| {
        let r = big_button(ui, Icon::Crop, "Crop", true);
        egui::Popup::menu(&r).show(|ui| {
            ui.label("Crop to Shape");
            if let Some(p) = shapes_gallery(ui) {
                run(app, "shape.change", json!({"preset": p}));
            }
            ui.separator();
            for (l, v) in [("Crop 10% each side", 0.1), ("Crop 20% each side", 0.2), ("Reset Crop", 0.0)] {
                if ui.button(l).clicked() {
                    run(app, "picture.crop", json!({"left": v, "top": v, "right": v, "bottom": v}));
                }
            }
        });
    });
    size_group(app, ui);
}

const STYLE_TILE: egui::Vec2 = vec2(56.0, 44.0);
/// Styles in the ribbon row before the tab has been measured once.
const DEFAULT_INLINE_STYLES: usize = 6;
const MIN_INLINE_STYLES: usize = 2;

/// How many Picture Styles the ribbon row shows: as many as leave room for the groups after the
/// gallery, never fewer than [`MIN_INLINE_STYLES`]. With all 24 in the row, Crop and Size sat
/// past the ribbon's edge in any window narrower than about 2500 px.
fn inline_styles(ui: &Ui) -> usize {
    use deckcraft_engine::cmd::picture_style::STYLES;
    // A tile plus the group's 1 pt spacing.
    let step = STYLE_TILE.x + 1.0;
    let id = egui::Id::unique("picture_styles_inline");
    let last = ui.ctx().data(|d| d.get_temp::<usize>(id)).unwrap_or(DEFAULT_INLINE_STYLES);
    let n = match super::spare_room(ui.ctx(), "Picture Format") {
        // The groups around the gallery keep their width, so this settles in one frame.
        Some(spare) if spare.is_finite() => (last as f32 + (spare / step).floor()).clamp(MIN_INLINE_STYLES as f32, STYLES.len() as f32) as usize,
        _ => last,
    };
    if n != last {
        ui.ctx().data_mut(|d| d.insert_temp(id, n));
        ui.ctx().request_repaint();
    }
    n
}

/// A Picture Styles gallery tile: a landscape "picture" in the style's shape and frame.
fn paint_picture_style(p: &egui::Painter, r: Rect, lk: Option<deckcraft_engine::cmd::picture_style::Look>) {
    let Some(lk) = lk else { return };
    let r = r.shrink(7.0);
    if lk.shadow.is_some() {
        p.rect_filled(r.translate(vec2(2.0, 2.0)), CornerRadius::same(2), Color32::from_black_alpha(60));
    }
    let sky = Color32::from_rgb(0x7F, 0xB2, 0xE5);
    let frame = lk.frame.map(|(c, w)| Stroke::new((w as f32 / 2.5).clamp(1.0, 4.0), theme::to_color32(c)));
    let edge = Stroke::new(0.5, Color32::from_gray(150));
    match lk.preset {
        "ellipse" => {
            p.add(egui::Shape::ellipse_filled(r.center(), r.size() / 2.0, sky));
            p.add(egui::Shape::ellipse_stroke(r.center(), r.size() / 2.0, frame.unwrap_or(edge)));
        }
        preset => {
            let round = if preset == "rect" { 0 } else { 6 };
            p.rect_filled(r, CornerRadius::same(round), sky);
            p.rect_stroke(r, CornerRadius::same(round), frame.unwrap_or(edge), egui::StrokeKind::Middle);
        }
    }
    // Hills, so the tile reads as a picture.
    let base = r.bottom() - r.height() * 0.15;
    p.line_segment(
        [pos2(r.left() + r.width() * 0.2, base), pos2(r.center().x, r.top() + r.height() * 0.45)],
        Stroke::new(1.5, Color32::from_rgb(0x3A, 0x7D, 0x44)),
    );
    p.line_segment(
        [pos2(r.center().x, r.top() + r.height() * 0.45), pos2(r.right() - r.width() * 0.2, base)],
        Stroke::new(1.5, Color32::from_rgb(0x3A, 0x7D, 0x44)),
    );
    if lk.reflection {
        p.rect_filled(
            Rect::from_min_max(pos2(r.left(), r.bottom() + 1.0), pos2(r.right(), r.bottom() + 5.0)),
            CornerRadius::ZERO,
            sky.gamma_multiply(0.3),
        );
    }
}

/// A Recolor gallery tile: a dark-to-light ramp as the preset would draw it.
fn paint_recolor(p: &egui::Painter, r: Rect, id: &str, sc: &deckcraft_color::ColorScheme) {
    let mut adj = deckcraft_model::style::PictureAdjust::default();
    deckcraft_engine::cmd::recolor::apply(&mut adj, id);
    let r = r.shrink(3.0);
    let n = 8;
    for k in 0..n {
        let t = k as f64 / (n - 1) as f64;
        // A colourful ramp, so "No Recolor" shows colour and the presets show their mapping.
        let src = [t * 255.0, (0.4 + 0.6 * t) * 200.0, (1.0 - t) * 220.0];
        let y = 0.299 * src[0] + 0.587 * src[1] + 0.114 * src[2];
        let mut rgb = if adj.grayscale { [y; 3] } else { src };
        if adj.brightness != 0.0 || adj.contrast != 0.0 {
            rgb = rgb.map(|v| (v - 127.5) * (1.0 + adj.contrast) + 127.5 + adj.brightness * 255.0);
        }
        if let Some((d, l)) = &adj.duotone {
            let (d, l) = (d.resolve(sc, None), l.resolve(sc, None));
            let u = y / 255.0;
            rgb =
                [d.r as f64 + (l.r as f64 - d.r as f64) * u, d.g as f64 + (l.g as f64 - d.g as f64) * u, d.b as f64 + (l.b as f64 - d.b as f64) * u];
        }
        if let Some(th) = adj.bilevel {
            rgb = [if y >= th * 255.0 { 255.0 } else { 0.0 }; 3];
        }
        let c = Color32::from_rgb(rgb[0].clamp(0.0, 255.0) as u8, rgb[1].clamp(0.0, 255.0) as u8, rgb[2].clamp(0.0, 255.0) as u8);
        let w = r.width() / n as f32;
        p.rect_filled(Rect::from_min_size(pos2(r.left() + k as f32 * w, r.top()), vec2(w + 0.5, r.height())), CornerRadius::ZERO, c);
    }
    p.rect_stroke(r, CornerRadius::ZERO, Stroke::new(0.5, Color32::from_gray(160)), egui::StrokeKind::Outside);
}

/// Compress Pictures options, as in PowerPoint's dialog: resolution, apply to this picture or
/// all, delete cropped areas. The choices last for the session.
fn compress_menu(app: &mut SlideApp, ui: &mut Ui) {
    let id = egui::Id::unique("compress_pictures_opts");
    let (mut ppi, mut only_selected, mut delete_cropped): (u32, bool, bool) = ui.data_mut(|d| d.get_temp(id).unwrap_or((220, true, true)));
    ui.label(egui::RichText::new("Compression options").strong());
    ui.checkbox(&mut only_selected, "Apply only to this picture");
    ui.checkbox(&mut delete_cropped, "Delete cropped areas of pictures");
    ui.separator();
    ui.label(egui::RichText::new("Resolution").strong());
    for (v, label) in [
        (330, "HD (330 ppi): good quality for high-definition displays"),
        (220, "Print (220 ppi): excellent quality on most printers and screens"),
        (150, "Web (150 ppi): good for web pages and projectors"),
        (96, "E-mail (96 ppi): minimise document size for sharing"),
    ] {
        ui.radio_value(&mut ppi, v, label);
    }
    ui.data_mut(|d| d.insert_temp(id, (ppi, only_selected, delete_cropped)));
    ui.separator();
    if ui.button("Compress").clicked() {
        let p = json!({"ppi": ppi, "deleteCropped": delete_cropped, "all": !only_selected});
        match app.session.execute("picture.compress", &p) {
            Ok(r) => {
                let kb = |v: &Value| v.as_u64().unwrap_or(0) / 1024;
                let msg = match r["pictures"].as_u64().unwrap_or(0) {
                    0 => "Pictures are already at or below that resolution".to_string(),
                    n => format!("Compressed {n} picture(s): {} KB → {} KB", kb(&r["before"]), kb(&r["after"])),
                };
                app.set_status(msg);
            }
            Err(e) => app.set_status(e.to_string()),
        }
        ui.close();
    }
}
