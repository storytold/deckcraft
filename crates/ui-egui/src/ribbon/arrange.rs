use super::{
    Icon, ShapeId, SlideApp, Ui, big, big_button, big_button_ex, enabled, group, json, menu_item, rows, run, small_button_ex, theme, widgets,
};

pub fn arrange_menu(app: &mut SlideApp, ui: &mut Ui) {
    ui.label(egui::RichText::new("개체 순서").font(theme::bold(12.0)));
    for (l, id, ic) in [
        ("맨 앞으로 가져오기", "arrange.bringToFront", Icon::BringToFront),
        ("맨 뒤로 보내기", "arrange.sendToBack", Icon::SendToBack),
        ("앞으로 가져오기", "arrange.bringForward", Icon::BringForward),
        ("뒤로 보내기", "arrange.sendBackward", Icon::SendBackward),
    ] {
        menu_item(app, ui, ic, l, id, json!({}));
    }
    ui.separator();
    ui.label(egui::RichText::new("개체 그룹화").font(theme::bold(12.0)));
    menu_item(app, ui, Icon::Group, "그룹", "arrange.group", json!({}));
    menu_item(app, ui, Icon::Ungroup, "그룹 해제", "arrange.ungroup", json!({}));
    menu_item(app, ui, Icon::Group, "재그룹", "arrange.regroup", json!({}));
    ui.separator();
    ui.label(egui::RichText::new("개체 위치").font(theme::bold(12.0)));
    ui.menu_button("맞춤", |ui| align_menu(app, ui));
    ui.menu_button("회전", |ui| {
        menu_item(app, ui, Icon::RotateRight, "Rotate Right 90°", "arrange.rotateRight", json!({}));
        menu_item(app, ui, Icon::RotateLeft, "Rotate Left 90°", "arrange.rotateLeft", json!({}));
        menu_item(app, ui, Icon::FlipV, "상하 대칭", "arrange.flipVertical", json!({}));
        menu_item(app, ui, Icon::FlipH, "좌우 대칭", "arrange.flipHorizontal", json!({}));
    });
    if ui.button("Selection Pane…").clicked() {
        let _ = app.run("view.pane", json!({"pane": "selection"}));
        ui.close();
    }
}

pub fn align_menu(app: &mut SlideApp, ui: &mut Ui) {
    let selection = app.session.active().map(|d| d.selection.shapes.clone()).unwrap_or_default();
    let to_slide = align_to_slide(app.align_to.as_ref(), &selection);
    let to = if to_slide { "slide" } else { "selection" };
    for (l, e, ic) in [
        ("왼쪽 맞춤", "left", Icon::AlignObjLeft),
        ("가운데 맞춤", "center", Icon::AlignObjCenter),
        ("오른쪽 맞춤", "right", Icon::AlignObjRight),
        ("위쪽 맞춤", "top", Icon::AlignObjTop),
        ("중간 맞춤", "middle", Icon::AlignObjMiddle),
        ("아래쪽 맞춤", "bottom", Icon::AlignObjBottom),
    ] {
        menu_item(app, ui, ic, l, "arrange.align", json!({"edge": e, "to": to}));
    }
    ui.separator();
    menu_item(app, ui, Icon::DistributeH, "Distribute Horizontally", "arrange.distribute", json!({"dir": "horizontal", "to": to}));
    menu_item(app, ui, Icon::DistributeV, "Distribute Vertically", "arrange.distribute", json!({"dir": "vertical", "to": to}));
    ui.separator();
    let en = enabled(app, "arrange.align");
    for (l, slide) in [("슬라이드에 맞춤", true), ("선택한 개체 맞춤", false)] {
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
    group(ui, |ui| {
        let sel = enabled(app, "arrange.bringForward");
        let (m, a) = big_button_ex(ui, Icon::BringForward, "Bring\nForward", sel, true, false);
        if m.clicked() {
            run(app, "arrange.bringForward", json!({}));
        }
        if let Some(a) = a {
            egui::Popup::menu(&a).show(|ui| {
                menu_item(app, ui, Icon::BringForward, "앞으로 가져오기", "arrange.bringForward", json!({}));
                menu_item(app, ui, Icon::BringToFront, "맨 앞으로 가져오기", "arrange.bringToFront", json!({}));
            });
        }
        let (m, a) = big_button_ex(ui, Icon::SendBackward, "Send\nBackward", sel, true, false);
        if m.clicked() {
            run(app, "arrange.sendBackward", json!({}));
        }
        if let Some(a) = a {
            egui::Popup::menu(&a).show(|ui| {
                menu_item(app, ui, Icon::SendBackward, "뒤로 보내기", "arrange.sendBackward", json!({}));
                menu_item(app, ui, Icon::SendToBack, "맨 뒤로 보내기", "arrange.sendToBack", json!({}));
            });
        }
        big(app, ui, Icon::SelectionPane, "Selection\nPane", "view.pane", json!({"pane": "selection"}));
        let r = big_button(ui, Icon::AlignObjects, "맞춤", sel);
        egui::Popup::menu(&r).show(|ui| align_menu(app, ui));
        rows(ui, |ui| {
            let (main, arrow) = small_button_ex(ui, Icon::Group, "그룹", sel, false, true);
            if main.clicked() {
                run(app, "arrange.group", json!({}));
            }
            if let Some(arrow) = arrow {
                egui::Popup::menu(&arrow).show(|ui| {
                    menu_item(app, ui, Icon::Group, "그룹", "arrange.group", json!({}));
                    menu_item(app, ui, Icon::Ungroup, "그룹 해제", "arrange.ungroup", json!({}));
                    menu_item(app, ui, Icon::Group, "재그룹", "arrange.regroup", json!({}));
                });
            }
            let (main, arrow) = small_button_ex(ui, Icon::Rotate, "회전", sel, false, true);
            if main.clicked() {
                run(app, "arrange.rotateRight", json!({}));
            }
            if let Some(arrow) = arrow {
                egui::Popup::menu(&arrow).show(|ui| {
                    menu_item(app, ui, Icon::RotateRight, "Rotate Right 90°", "arrange.rotateRight", json!({}));
                    menu_item(app, ui, Icon::RotateLeft, "Rotate Left 90°", "arrange.rotateLeft", json!({}));
                    menu_item(app, ui, Icon::FlipV, "상하 대칭", "arrange.flipVertical", json!({}));
                    menu_item(app, ui, Icon::FlipH, "좌우 대칭", "arrange.flipHorizontal", json!({}));
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
                ui.label("높이:");
                ui.label("너비:");
                return;
            };
            let len = app.session.prefs.units;
            let (ppu, suffix) = (len.points_per_unit(), len.suffix());
            // " cm" needs more room than the inch mark.
            let width = if len == deckcraft_geom::Unit::Inches { 54.0 } else { 66.0 };
            ui.horizontal(|ui| {
                ui.label("높이:");
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
        if ui.add_enabled(bx.is_some(), egui::Button::new("크기 및 위치...")).clicked() {
            run(app, "shape.sizePosition", json!({}));
        }
    });
}
