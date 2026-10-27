use super::arrange::arrange_group;
use super::galleries::cref_param;
use super::*;

pub(super) fn table_design(app: &mut SlideApp, ui: &mut Ui) {
    let tbl = app
        .session
        .active()
        .and_then(|d| d.selected_shapes().into_iter().find_map(|s| if let ShapeKind::Table(t) = &s.kind { Some(t.clone()) } else { None }));
    group(ui, |ui| {
        rows(ui, |ui| {
            let flags = tbl.as_ref().map(|t| (t.first_row, t.last_row, t.band_row, t.first_col, t.last_col, t.band_col)).unwrap_or_default();
            ui.horizontal(|ui| {
                let mut a = flags.0;
                if ui.checkbox(&mut a, "Header Row").changed() {
                    run(app, "table.options", json!({"headerRow": a}));
                }
                let mut d = flags.3;
                if ui.checkbox(&mut d, "First Column").changed() {
                    run(app, "table.options", json!({"firstColumn": d}));
                }
            });
            ui.horizontal(|ui| {
                let mut b = flags.1;
                if ui.checkbox(&mut b, "Total Row").changed() {
                    run(app, "table.options", json!({"totalRow": b}));
                }
                let mut e = flags.4;
                if ui.checkbox(&mut e, "Last Column").changed() {
                    run(app, "table.options", json!({"lastColumn": e}));
                }
            });
            ui.horizontal(|ui| {
                let mut c = flags.2;
                if ui.checkbox(&mut c, "Banded Rows").changed() {
                    run(app, "table.options", json!({"bandedRows": c}));
                }
                let mut f = flags.5;
                if ui.checkbox(&mut f, "Banded Columns").changed() {
                    run(app, "table.options", json!({"bandedColumns": f}));
                }
            });
        });
    });
    group(ui, |ui| {
        let styles = deckcraft_model::table::table_styles();
        let quick: Vec<_> = styles.iter().filter(|(id, _)| id.starts_with("medium2") || id == "none" || id == "grid").take(8).collect();
        let cur = tbl.as_ref().map(|t| t.style.clone()).unwrap_or_default();
        let sel = quick.iter().position(|(id, _)| *id == cur);
        let sc = scheme(app);
        if let Some(i) = widgets::gallery(
            ui,
            quick.len(),
            vec2(48.0, 40.0),
            sel,
            |p, r, i| {
                if let Some((id, _)) = quick.get(i) {
                    paint_table_style(p, r, id, &sc);
                }
            },
            |i| quick.get(i).map(|x| x.1.clone()).unwrap_or_default(),
        ) && let Some((id, _)) = quick.get(i)
        {
            run(app, "table.style", json!({"style": id}));
        }
        let more = widgets::drop_button(ui, "", vec2(14.0, 40.0), true);
        egui::Popup::menu(&more).width(360.0).show(|ui| {
            ui.horizontal_wrapped(|ui| {
                for (id, label) in &styles {
                    let (r, resp) = ui.allocate_exact_size(vec2(48.0, 36.0), Sense::click());
                    paint_table_style(ui.painter(), r.shrink(2.0), id, &sc);
                    if resp.on_hover_text(label).clicked() {
                        run(app, "table.style", json!({"style": id}));
                        ui.close();
                    }
                }
            });
        });
    });
    group(ui, |ui| {
        let sc = scheme(app);
        let (m, a) = color_split(ui, Icon::Shading, "Shading", Color32::from_rgb(0x2E, 0x6F, 0xD8), true);
        if m.clicked() {
            run(app, "table.cellFill", json!({"color": "accent1"}));
        }
        widgets::color_popup(&a).show(|ui| {
            if let Some(c) = widgets::color_grid(ui, &sc, Some("No Fill")) {
                match c {
                    Some(c) => run(app, "table.cellFill", json!({"color": cref_param(&c)})),
                    None => run(app, "table.cellFill", json!({"none": true})),
                }
            }
        });
    });
    group(ui, |ui| {
        let r = big_button(ui, Icon::Borders, "Borders", true);
        egui::Popup::menu(&r).show(|ui| {
            let pen = app.table_pen.clone();
            for (label, which) in [
                ("No Border", "none"),
                ("All Borders", "all"),
                ("Outside Borders", "outside"),
                ("Inside Borders", "inside"),
                ("Top Border", "top"),
                ("Bottom Border", "bottom"),
                ("Left Border", "left"),
                ("Right Border", "right"),
                ("Inside Horizontal Border", "insideH"),
                ("Inside Vertical Border", "insideV"),
                ("Diagonal Down Border", "diagDown"),
                ("Diagonal Up Border", "diagUp"),
            ] {
                let mut p = pen.clone();
                p["which"] = json!(which);
                menu_item(app, ui, Icon::Borders, label, "table.borders", p);
            }
            ui.separator();
            ui.menu_button("Pen Weight", |ui| {
                for w in [0.25, 0.5, 0.75, 1.0, 1.5, 2.25, 3.0, 4.5, 6.0] {
                    let cur = app.table_pen.get("width").and_then(Value::as_f64) == Some(w);
                    if ui.radio(cur, format!("{w} pt")).clicked() {
                        app.table_pen["width"] = json!(w);
                    }
                }
            });
            ui.menu_button("Pen Style", |ui| {
                for (l, d) in [("Solid", "solid"), ("Dash", "dash"), ("Dot", "sysDot"), ("Dash Dot", "dashDot"), ("Long Dash", "lgDash")] {
                    let cur = app.table_pen.get("dash").and_then(Value::as_str).unwrap_or("solid") == d;
                    if ui.radio(cur, l).clicked() {
                        app.table_pen["dash"] = json!(d);
                    }
                }
            });
            ui.menu_button("Pen Color", |ui| {
                let sc = scheme(app);
                if let Some(Some(c)) = widgets::color_grid(ui, &sc, None) {
                    app.table_pen["color"] = cref_param(&c);
                }
            });
        });
    });
    // Draw Borders: the pencil splits cells along a line, the eraser merges across a border.
    group(ui, |ui| {
        use deckcraft_engine::ToolKind;
        use deckcraft_engine::tools::{DRAW_TABLE, TABLE_ERASER};
        let cur = app.session.tool.kind.clone();
        for (icon, label, preset) in [(Icon::Pencil, "Draw Table", DRAW_TABLE), (Icon::Eraser, "Table Eraser", TABLE_ERASER)] {
            let on = matches!(&cur, ToolKind::Shape { preset: p } if p == preset);
            if big_button_ex(ui, icon, label, true, false, on).0.clicked() {
                app.session.set_tool(if on { ToolKind::Select } else { ToolKind::Shape { preset: preset.into() } });
            }
        }
    });
}

pub fn paint_table_style(p: &egui::Painter, r: Rect, id: &str, sc: &deckcraft_color::ColorScheme) {
    let slot = match id.rsplit('-').next().unwrap_or("accent1") {
        "accent2" => deckcraft_color::SchemeSlot::Accent2,
        "accent3" => deckcraft_color::SchemeSlot::Accent3,
        "accent4" => deckcraft_color::SchemeSlot::Accent4,
        "accent5" => deckcraft_color::SchemeSlot::Accent5,
        "accent6" => deckcraft_color::SchemeSlot::Accent6,
        "tx1" => deckcraft_color::SchemeSlot::Dk1,
        _ => deckcraft_color::SchemeSlot::Accent1,
    };
    let a = theme::to_color32(sc.get(slot));
    let kind = id.split('-').next().unwrap_or("");
    for row in 0..4 {
        let y = r.min.y + row as f32 * r.height() / 4.0;
        let rr = Rect::from_min_size(pos2(r.min.x, y), vec2(r.width(), r.height() / 4.0));
        let c = match (kind, row) {
            ("none", _) | ("grid", _) => Color32::WHITE,
            ("light1", 0) | ("light2", _) => Color32::WHITE,
            ("dark1", 0) => Color32::BLACK,
            ("dark1", _) => a.lerp_to_gamma(Color32::BLACK, 0.3),
            (_, 0) => a,
            (_, k) if k % 2 == 1 => a.lerp_to_gamma(Color32::WHITE, 0.6),
            _ => a.lerp_to_gamma(Color32::WHITE, 0.8),
        };
        p.rect_filled(rr, CornerRadius::ZERO, c);
        p.line_segment([rr.left_bottom(), rr.right_bottom()], Stroke::new(0.5, if kind == "grid" { Color32::GRAY } else { Color32::WHITE }));
    }
    if kind == "grid" || kind == "light2" {
        p.rect_stroke(r, CornerRadius::ZERO, Stroke::new(0.8, if kind == "grid" { Color32::GRAY } else { a }), egui::StrokeKind::Inside);
    }
}

pub(super) fn table_layout(app: &mut SlideApp, ui: &mut Ui) {
    group(ui, |ui| {
        let r = big_button(ui, Icon::DeleteTable, "Delete", true);
        egui::Popup::menu(&r).show(|ui| {
            menu_item(app, ui, Icon::DeleteTable, "Delete Columns", "table.deleteColumn", json!({}));
            menu_item(app, ui, Icon::DeleteTable, "Delete Rows", "table.deleteRow", json!({}));
            menu_item(app, ui, Icon::DeleteTable, "Delete Table", "edit.delete", json!({}));
        });
        big(app, ui, Icon::InsertAbove, "Insert\nAbove", "table.insertRowAbove", json!({}));
        big(app, ui, Icon::InsertBelow, "Insert\nBelow", "table.insertRowBelow", json!({}));
        big(app, ui, Icon::InsertLeft, "Insert\nLeft", "table.insertColumnLeft", json!({}));
        big(app, ui, Icon::InsertRight, "Insert\nRight", "table.insertColumnRight", json!({}));
    });
    group(ui, |ui| {
        big(app, ui, Icon::MergeCells, "Merge\nCells", "table.merge", json!({}));
        big(app, ui, Icon::SplitCells, "Split\nCells", "table.split", json!({}));
    });
    group(ui, |ui| {
        rows(ui, |ui| {
            small(app, ui, Icon::DistributeV, "Distribute Rows", "table.distributeRows", json!({}));
            small(app, ui, Icon::DistributeH, "Distribute Columns", "table.distributeColumns", json!({}));
        });
    });
    group(ui, |ui| {
        rows(ui, |ui| {
            ui.horizontal(|ui| {
                toggle(app, ui, Icon::AlignLeft, "Align Left", "format.alignLeft", false);
                toggle(app, ui, Icon::AlignCenter, "Center", "format.alignCenter", false);
                toggle(app, ui, Icon::AlignRight, "Align Right", "format.alignRight", false);
            });
            ui.horizontal(|ui| {
                for (l, a) in [("Top", "top"), ("Center", "middle"), ("Bottom", "bottom")] {
                    if ui.small_button(l).on_hover_text(format!("Align text to the {l} of the cell").to_lowercase()).clicked() {
                        run(app, "table.cellAlign", json!({"anchor": a}));
                    }
                }
                ui.menu_button("Cell Margins", |ui| {
                    for (l, preset) in [("Normal", "normal"), ("None", "none"), ("Narrow", "narrow"), ("Wide", "wide")] {
                        if ui.button(l).clicked() {
                            run(app, "table.cellMargins", json!({"preset": preset}));
                            ui.close();
                        }
                    }
                });
            });
        });
    });
    arrange_group(app, ui);
}
