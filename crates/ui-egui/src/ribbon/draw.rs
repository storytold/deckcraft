use super::*;

// ---------- Draw ----------

pub(super) fn draw(app: &mut SlideApp, ui: &mut Ui) {
    use deckcraft_engine::ToolKind;
    let cur = app.session.tool.kind.clone();
    group(ui, |ui| {
        let sel = matches!(cur, ToolKind::Select);
        if big_button_ex(ui, Icon::LassoSelect, "Select", true, false, sel).0.clicked() {
            app.session.set_tool(ToolKind::Select);
        }
        let er = matches!(&cur, ToolKind::Ink { mode, .. } if mode == "eraser");
        if big_button_ex(ui, Icon::Eraser, "Eraser", true, false, er).0.clicked() {
            app.session.set_tool(ToolKind::Ink { mode: "eraser".into(), color: deckcraft_color::Rgba::BLACK, width: 2.0 });
        }
    });
    group(ui, |ui| {
        let pens: [(Icon, &str, &str, deckcraft_color::Rgba, f64); 5] = [
            (Icon::Pen, "Black Pen", "pen", deckcraft_color::Rgba::rgb(20, 20, 20), 1.5),
            (Icon::Pen, "Red Pen", "pen", deckcraft_color::Rgba::rgb(0xD8, 0x3F, 0x3F), 1.5),
            (Icon::Pen, "Blue Pen", "pen", deckcraft_color::Rgba::rgb(0x2E, 0x6F, 0xD8), 1.5),
            (Icon::Pencil, "Pencil", "pen", deckcraft_color::Rgba::rgb(90, 90, 90), 1.0),
            (Icon::HighlighterPen, "Highlighter", "highlighter", deckcraft_color::Rgba::rgb(255, 230, 0), 10.0),
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
        let r = big_button(ui, Icon::InkToShape, "Ink to\nShape", has_ink);
        if r.on_hover_text("Ink to Shape: turn drawn lines, rectangles, ovals and triangles into shapes").clicked() {
            app.session.set_tool(ToolKind::Select);
            run(app, "draw.inkToShape", json!({}));
        }
        let r = big_button(ui, Icon::InkToText, "Ink to\nText", false);
        let _ = r.on_hover_text("Ink to Text (coming soon)");
        let r = big_button(ui, Icon::InkToMath, "Ink to\nMath", false);
        let _ = r.on_hover_text("Ink to Math (coming soon)");
    });
}
