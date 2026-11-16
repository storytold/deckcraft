use super::{
    Icon, ShapeId, SlideApp, Ui, big, big_button, big_button_ex, enabled, group, json, menu_item, rows, run, small_button_ex, theme, widgets,
};
use crate::i18n;

/// Offer ordering and grouping commands with supported localized display labels.
pub fn arrange_menu(app: &mut SlideApp, ui: &mut Ui) {
    let lang = app.lang();
    ui.label(egui::RichText::new(i18n::text(lang, "ribbon.arrange.order_heading")).font(theme::bold(12.0)));
    for (l, id, ic) in [
        (i18n::text(lang, "pane.selection.bring_front"), "arrange.bringToFront", Icon::BringToFront),
        (i18n::text(lang, "pane.selection.send_back"), "arrange.sendToBack", Icon::SendToBack),
        (i18n::text(lang, "pane.selection.bring_forward"), "arrange.bringForward", Icon::BringForward),
        (i18n::text(lang, "pane.selection.send_backward"), "arrange.sendBackward", Icon::SendBackward),
    ] {
        menu_item(app, ui, ic, l, id, json!({}));
    }
    ui.separator();
    ui.label(egui::RichText::new(i18n::text(lang, "ribbon.arrange.group_heading")).font(theme::bold(12.0)));
    menu_item(app, ui, Icon::Group, i18n::text(lang, "ribbon.arrange.group"), "arrange.group", json!({}));
    menu_item(app, ui, Icon::Ungroup, i18n::text(lang, "ribbon.arrange.ungroup"), "arrange.ungroup", json!({}));
    menu_item(app, ui, Icon::Group, i18n::text(lang, "ribbon.arrange.regroup"), "arrange.regroup", json!({}));
    ui.separator();
    ui.label(egui::RichText::new(i18n::text(lang, "ribbon.arrange.position_heading")).font(theme::bold(12.0)));
    ui.menu_button(i18n::text(lang, "ribbon.arrange.align"), |ui| align_menu(app, ui));
    ui.menu_button(i18n::text(lang, "ribbon.arrange.rotate"), |ui| {
        menu_item(app, ui, Icon::RotateRight, i18n::text(lang, "ribbon.arrange.rotate_right"), "arrange.rotateRight", json!({}));
        menu_item(app, ui, Icon::RotateLeft, i18n::text(lang, "ribbon.arrange.rotate_left"), "arrange.rotateLeft", json!({}));
        menu_item(app, ui, Icon::FlipV, i18n::text(lang, "ribbon.arrange.flip_v"), "arrange.flipVertical", json!({}));
        menu_item(app, ui, Icon::FlipH, i18n::text(lang, "ribbon.arrange.flip_h"), "arrange.flipHorizontal", json!({}));
    });
    if ui.button(i18n::text(lang, "ribbon.arrange.selection")).clicked() {
        let _ = app.run("view.pane", json!({"pane": "selection"}));
        ui.close();
    }
}

/// Offer alignment commands while retaining their stored values and target scope.
pub fn align_menu(app: &mut SlideApp, ui: &mut Ui) {
    let lang = app.lang();
    let selection = app.session.active().map(|d| d.selection.shapes.clone()).unwrap_or_default();
    let to_slide = align_to_slide(app.align_to.as_ref(), &selection);
    let to = if to_slide { "slide" } else { "selection" };
    for (l, e, ic) in [
        (i18n::text(lang, "ribbon.align.left"), "left", Icon::AlignObjLeft),
        (i18n::text(lang, "ribbon.align.center"), "center", Icon::AlignObjCenter),
        (i18n::text(lang, "ribbon.align.right"), "right", Icon::AlignObjRight),
        (i18n::text(lang, "ribbon.align.top"), "top", Icon::AlignObjTop),
        (i18n::text(lang, "ribbon.align.middle"), "middle", Icon::AlignObjMiddle),
        (i18n::text(lang, "ribbon.align.bottom"), "bottom", Icon::AlignObjBottom),
    ] {
        menu_item(app, ui, ic, l, "arrange.align", json!({"edge": e, "to": to}));
    }
    ui.separator();
    menu_item(
        app,
        ui,
        Icon::DistributeH,
        i18n::text(lang, "ribbon.arrange.distribute_h"),
        "arrange.distribute",
        json!({"dir": "horizontal", "to": to}),
    );
    menu_item(
        app,
        ui,
        Icon::DistributeV,
        i18n::text(lang, "ribbon.arrange.distribute_v"),
        "arrange.distribute",
        json!({"dir": "vertical", "to": to}),
    );
    ui.separator();
    let en = enabled(app, "arrange.align");
    for (l, slide) in [("Align to Slide", true), ("Align Selected Objects", false)] {
        if widgets::check_item(ui, l, en, to_slide == slide).clicked() {
            app.align_to = Some((selection.clone(), slide));
            ui.close();
        }
    }
}

/// Align to Slide or to the selected objects, as in PowerPoint: one object aligns to the slide and
/// several to each other, unless the other choice was ticked for this same selection.
fn align_to_slide(choice: Option<&(Vec<ShapeId>, bool)>, selection: &[ShapeId]) -> bool {
    match choice {
        Some((ids, to_slide)) if ids.as_slice() == selection => *to_slide,
        _ => selection.len() < 2,
    }
}

/// Render ordering, align, group/ungroup, and rotate controls for the current selection.
pub(super) fn arrange_group(app: &mut SlideApp, ui: &mut Ui) {
    let lang = app.lang();
    group(ui, |ui| {
        let sel = enabled(app, "arrange.bringForward");
        let (m, a) = big_button_ex(ui, Icon::BringForward, "Bring\nForward", sel, true, false);
        if m.clicked() {
            run(app, "arrange.bringForward", json!({}));
        }
        if let Some(a) = a {
            egui::Popup::menu(&a).show(|ui| {
                menu_item(app, ui, Icon::BringForward, i18n::text(lang, "pane.selection.bring_forward"), "arrange.bringForward", json!({}));
                menu_item(app, ui, Icon::BringToFront, i18n::text(lang, "pane.selection.bring_front"), "arrange.bringToFront", json!({}));
            });
        }
        let (m, a) = big_button_ex(ui, Icon::SendBackward, "Send\nBackward", sel, true, false);
        if m.clicked() {
            run(app, "arrange.sendBackward", json!({}));
        }
        if let Some(a) = a {
            egui::Popup::menu(&a).show(|ui| {
                menu_item(app, ui, Icon::SendBackward, i18n::text(lang, "pane.selection.send_backward"), "arrange.sendBackward", json!({}));
                menu_item(app, ui, Icon::SendToBack, i18n::text(lang, "pane.selection.send_back"), "arrange.sendToBack", json!({}));
            });
        }
        big(app, ui, Icon::SelectionPane, "Selection\nPane", "view.pane", json!({"pane": "selection"}));
        let r = big_button(ui, Icon::AlignObjects, i18n::text(lang, "ribbon.arrange.align"), sel);
        egui::Popup::menu(&r).show(|ui| align_menu(app, ui));
        rows(ui, |ui| {
            let (main, arrow) = small_button_ex(ui, Icon::Group, i18n::text(lang, "ribbon.arrange.group"), sel, false, true);
            if main.clicked() {
                run(app, "arrange.group", json!({}));
            }
            if let Some(arrow) = arrow {
                egui::Popup::menu(&arrow).show(|ui| {
                    menu_item(app, ui, Icon::Group, i18n::text(lang, "ribbon.arrange.group"), "arrange.group", json!({}));
                    menu_item(app, ui, Icon::Ungroup, i18n::text(lang, "ribbon.arrange.ungroup"), "arrange.ungroup", json!({}));
                    menu_item(app, ui, Icon::Group, i18n::text(lang, "ribbon.arrange.regroup"), "arrange.regroup", json!({}));
                });
            }
            let (main, arrow) = small_button_ex(ui, Icon::Rotate, i18n::text(lang, "ribbon.arrange.rotate"), sel, false, true);
            if main.clicked() {
                run(app, "arrange.rotateRight", json!({}));
            }
            if let Some(arrow) = arrow {
                egui::Popup::menu(&arrow).show(|ui| {
                    menu_item(app, ui, Icon::RotateRight, i18n::text(lang, "ribbon.arrange.rotate_right"), "arrange.rotateRight", json!({}));
                    menu_item(app, ui, Icon::RotateLeft, i18n::text(lang, "ribbon.arrange.rotate_left"), "arrange.rotateLeft", json!({}));
                    menu_item(app, ui, Icon::FlipV, i18n::text(lang, "ribbon.arrange.flip_v"), "arrange.flipVertical", json!({}));
                    menu_item(app, ui, Icon::FlipH, i18n::text(lang, "ribbon.arrange.flip_h"), "arrange.flipHorizontal", json!({}));
                });
            }
        });
    });
}

/// Render width and height controls bound to the selected shape, plus the size-position dialog.
pub(super) fn size_group(app: &mut SlideApp, ui: &mut Ui) {
    let bx = app
        .session
        .active()
        .and_then(|d| d.selection.shapes.first().and_then(|id| d.shape(*id).map(|s| deckcraft_engine::cmd::xfrm_of(&d.doc, &d.selection, s))));
    group(ui, |ui| {
        rows(ui, |ui| {
            let Some(x) = bx else {
                ui.label("Height:");
                ui.label("Width:");
                return;
            };
            let len = app.session.prefs.units;
            let (ppu, suffix) = (len.points_per_unit(), len.suffix());
            // " cm" needs more room than the inch mark.
            let width = if len == deckcraft_geom::Unit::Inches { 54.0 } else { 66.0 };
            ui.horizontal(|ui| {
                ui.label("Height:");
                if let Some(v) = widgets::spinner(ui, "sh", "Height", x.h / ppu, 0.1, suffix, width) {
                    run(app, "shape.resize", json!({"h": v.max(0.0) * ppu}));
                }
            });
            ui.horizontal(|ui| {
                ui.label("Width: ");
                if let Some(v) = widgets::spinner(ui, "sw", "Width", x.w / ppu, 0.1, suffix, width) {
                    run(app, "shape.resize", json!({"w": v.max(0.0) * ppu}));
                }
            });
        });
        if ui.add_enabled(bx.is_some(), egui::Button::new("Size and Position…")).clicked() {
            run(app, "shape.sizePosition", json!({}));
        }
    });
}
