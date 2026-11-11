use super::{Icon, SlideApp, Ui, big_button, big_button_ex, group, json, run};

// ---------- Draw ----------

pub(super) fn draw(app: &mut SlideApp, ui: &mut Ui) {
    use deckcraft_engine::ToolKind;
    let cur = app.session.tool.kind.clone();
    group(ui, |ui| {
        let sel = matches!(cur, ToolKind::Select);
        if big_button_ex(ui, Icon::LassoSelect, "올가미 선택(준비중)", true, false, sel).0.clicked() {
            app.session.set_tool(ToolKind::Select);
        }
        let er = matches!(&cur, ToolKind::Ink { mode, .. } if mode == "eraser");
        if big_button_ex(ui, Icon::Eraser, "지우개(준비중)", true, false, er).0.clicked() {
            app.session.set_tool(ToolKind::Ink { mode: "eraser".into(), color: deckcraft_color::Rgba::BLACK, width: 2.0 });
        }
    });
    group(ui, |ui| {
        let pens: [(Icon, &str, &str, deckcraft_color::Rgba, f64); 5] = [
            (Icon::Pen, "검정 펜", "pen", deckcraft_color::Rgba::rgb(20, 20, 20), 1.5),
            (Icon::Pen, "빨강 펜", "pen", deckcraft_color::Rgba::rgb(0xD8, 0x3F, 0x3F), 1.5),
            (Icon::Pen, "파랑 펜", "pen", deckcraft_color::Rgba::rgb(0x2E, 0x6F, 0xD8), 1.5),
            (Icon::Pencil, "연필", "pen", deckcraft_color::Rgba::rgb(90, 90, 90), 1.0),
            (Icon::HighlighterPen, "형광펜", "highlighter", deckcraft_color::Rgba::rgb(255, 230, 0), 10.0),
        ];
        for (icon, label, mode, color, width) in pens {
            let on = matches!(&cur, ToolKind::Ink { mode: m, color: c, .. } if m == mode && *c == color);
            if big_button_ex(ui, icon, label, true, false, on).0.clicked() {
                app.session.set_tool(ToolKind::Ink { mode: mode.into(), color, width });
            }
        }
    });
    group(ui, |ui| {
        let has_ink = app.session.doc().is_ok_and(|d| d.shapes().iter().any(|s| matches!(s.kind, deckcraft_model::ShapeKind::Ink { .. })));
        let r = big_button(ui, Icon::InkToShape, "잉크를\n도형으로", has_ink);
        if r.on_hover_text("잉크를 도형으로: 직접 그린 선, 사각형, 타원, 삼각형을 도형으로 변환합니다.").clicked() {
            app.session.set_tool(ToolKind::Select);
            run(app, "draw.inkToShape", json!({}));
        }
        let r = big_button(ui, Icon::InkToText, "잉크를\n텍스트로(준비중)", false);
        let _ = r.on_hover_text("잉크를 텍스트로 (준비 중)");
        let r = big_button(ui, Icon::InkToMath, "잉크를\n수식으로(준비중)", false);
        let _ = r.on_hover_text("잉크를 수식으로 (준비 중)");
    });
}
