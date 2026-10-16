//! Contributor-original native table style regression fixtures.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use deckcraft_model::{
    style::{Compound, Dash, Gradient, GradientShape, GradientStop, LineCap},
    table::Row,
    text::{Anchor, TextDir},
    *,
};
use deckcraft_odp::{export, import};
use std::{
    io::{Cursor, Read},
    sync::Arc,
};
fn deck(table: Table) -> Presentation {
    let mut p = defaults::blank_presentation(Size::new(400.0, 240.0), Theme::default(), false);
    let layout = defaults::layout_of_kind(&p, LayoutType::Blank).unwrap();
    p.slides.push(Arc::new(Slide {
        id: SlideId(50),
        layout,
        show_master_shapes: false,
        shapes: vec![Shape {
            id: ShapeId(51),
            xfrm: Some(Xfrm::new(20.0, 20.0, table.width(), table.height())),
            kind: ShapeKind::Table(table),
            ..Default::default()
        }],
        ..Default::default()
    }));
    p
}
fn table() -> Table {
    Table {
        cols: vec![100.0; 3],
        rows: (0..2).map(|_| Row { height: 90.0, cells: (0..3).map(|_| Cell::new("Own cell")).collect() }).collect(),
        style: "none".into(),
        first_row: false,
        band_row: false,
        ..Default::default()
    }
}
fn line(i: usize) -> Line {
    let mut l = Line::solid(ColorRef::rgb(Rgba::rgb(20, 60, 100)), 1.0 + i as f64 * 0.25);
    match i {
        1 => l.dash = Some(Dash::Preset { name: "dot".into() }),
        2 => l.dash = Some(Dash::Preset { name: "dash".into() }),
        3 => l.dash = Some(Dash::Preset { name: "dashDot".into() }),
        4 => l.dash = Some(Dash::Preset { name: "lgDashDotDot".into() }),
        5 => l.compound = Some(Compound::Double),
        _ => {}
    }
    l
}
fn assert_line(a: &Line, b: &Line) {
    assert_eq!(a.width, b.width);
    assert_eq!(a.dash, b.dash);
    assert_eq!(a.compound.unwrap_or_default(), b.compound.unwrap_or_default());
    assert_eq!(a.fill, b.fill);
}
fn part(bytes: &[u8], name: &str) -> String {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut s = String::new();
    z.by_name(name).unwrap().read_to_string(&mut s).unwrap();
    s
}
#[test]
fn borders_diagonals_and_cell_writing_modes_remain_native_after_export() {
    let mut t = table();
    t.background = Some(Fill::solid(ColorRef::rgb(Rgba::rgb(240, 240, 220))));
    for (r, row) in t.rows.iter_mut().enumerate() {
        for (c, cell) in row.cells.iter_mut().enumerate() {
            let i = r * 3 + c;
            cell.borders = std::array::from_fn(|_| Some(line(i)));
            cell.vertical = Some([TextDir::Horizontal, TextDir::Vertical, TextDir::EaVertical][c]);
            cell.anchor = Some(Anchor::Bottom);
            cell.margins = Some([1.0, 2.0, 3.0, 4.0]);
        }
    }
    t.rows[0].cells[0].diag_down = Some(line(3));
    t.rows[0].cells[0].diag_up = Some(line(5));
    let p = deck(t.clone());
    let bytes = export(&p).unwrap();
    let xml = part(&bytes, "content.xml");
    assert!(xml.contains("style:diagonal-tl-br=\"1.75pt dash-dot #143C64\""));
    assert!(xml.contains("style:diagonal-bl-tr=\"2.25pt double #143C64\""));
    assert!(xml.contains("style:writing-mode=\"tb-lr\"") && xml.contains("style:writing-mode=\"tb-rl\""));
    let q = import(&bytes).unwrap();
    assert!(!q.tags.iter().any(|(k, _)| k == "odp:warning"), "{:?}", q.tags);
    let ShapeKind::Table(u) = &q.slides[0].shapes[0].kind else { panic!("native table") };
    assert_eq!(t.background, u.background);
    for (r, row) in t.rows.iter().enumerate() {
        for (c, a) in row.cells.iter().enumerate() {
            let b = &u.rows[r].cells[c];
            assert_eq!(a.vertical, b.vertical);
            assert_eq!(a.margins, b.margins);
            assert_eq!(a.anchor, b.anchor);
            for (a, b) in a.borders.iter().zip(&b.borders) {
                assert_line(a.as_ref().unwrap(), b.as_ref().unwrap());
            }
        }
    }
    assert_line(t.rows[0].cells[0].diag_down.as_ref().unwrap(), u.rows[0].cells[0].diag_down.as_ref().unwrap());
    assert_line(t.rows[0].cells[0].diag_up.as_ref().unwrap(), u.rows[0].cells[0].diag_up.as_ref().unwrap());
    if let Ok(dir) = std::env::var("DECKCRAFT_ODP_TABLE_QA_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["content.xml", "styles.xml"] {
            std::fs::write(std::path::Path::new(&dir).join(name), part(&bytes, name)).unwrap();
        }
    }
}
#[test]
fn unsupported_cell_border_properties_refuse_instead_of_becoming_solid() {
    let base = line(0);
    let mut cases = Vec::new();
    let mut l = base.clone();
    l.fill = Some(Fill::solid(ColorRef::rgb(Rgba::BLACK).with(ColorTransform::Alpha(50000))));
    cases.push(l);
    let mut l = base.clone();
    l.fill = Some(Fill::Gradient(Gradient {
        shape: GradientShape::Linear { angle: 0.0, scaled: false },
        stops: vec![GradientStop { pos: 0.0, color: ColorRef::rgb(Rgba::BLACK) }, GradientStop { pos: 1.0, color: ColorRef::rgb(Rgba::WHITE) }],
        rotate_with_shape: true,
        tile: [0.0; 4],
    }));
    cases.push(l);
    let mut l = base.clone();
    l.dash = Some(Dash::Custom { pattern: vec![(2.0, 3.0)] });
    cases.push(l);
    let mut l = base.clone();
    l.dash = Some(Dash::Preset { name: "lgDash".into() });
    cases.push(l);
    let mut l = base.clone();
    l.compound = Some(Compound::Triple);
    cases.push(l);
    let mut l = line(2);
    l.compound = Some(Compound::Double);
    cases.push(l);
    let mut l = base.clone();
    l.cap = Some(LineCap::Round);
    cases.push(l);
    let mut l = base;
    l.width = Some(f64::NAN);
    cases.push(l);
    for (i, l) in cases.into_iter().enumerate() {
        for diagonal in [false, true] {
            let mut t = table();
            if diagonal {
                t.rows[0].cells[0].diag_up = Some(l.clone());
            } else {
                t.rows[0].cells[0].borders[0] = Some(l.clone());
            }
            assert!(export(&deck(t)).is_err(), "case{i},diagonal{diagonal}");
        }
    }
}
#[test]
fn unsupported_cell_direction_and_invalid_padding_refuse() {
    let mut t = table();
    t.rows[0].cells[0].vertical = Some(TextDir::Vertical270);
    assert!(export(&deck(t)).unwrap_err().0.contains("writing mode"));
    let mut t = table();
    t.rows[0].cells[0].margins = Some([f64::INFINITY, 0.0, 0.0, 0.0]);
    assert!(export(&deck(t)).is_err());
    let mut t = table();
    t.rows[0].cells[0].margins = Some([-1.0, 0.0, 0.0, 0.0]);
    assert!(export(&deck(t)).is_err());
}

fn gradient_fill() -> Fill {
    Fill::Gradient(Gradient {
        shape: GradientShape::Linear { angle: 0.0, scaled: false },
        stops: vec![GradientStop { pos: 0.0, color: ColorRef::rgb(Rgba::BLACK) }, GradientStop { pos: 1.0, color: ColorRef::rgb(Rgba::WHITE) }],
        rotate_with_shape: true,
        tile: [0.0; 4],
    })
}
#[test]
fn unsupported_cell_fills_and_table_backgrounds_refuse() {
    for fill in [gradient_fill(), Fill::solid(ColorRef::rgb(Rgba::BLACK).with(ColorTransform::Alpha(50000)))] {
        let mut t = table();
        t.rows[0].cells[0].fill = Some(fill);
        assert!(export(&deck(t)).unwrap_err().0.contains("cell fill"));
    }
    let mut t = table();
    t.background = Some(gradient_fill());
    assert!(export(&deck(t)).unwrap_err().0.contains("table-wide background"));
}
#[test]
fn inherited_unsupported_paint_refuses_but_explicit_none_overrides_it() {
    use deckcraft_model::table::{TablePart, TablePartStyle, TableStyleDef};
    for fill in [gradient_fill(), Fill::solid(ColorRef::rgb(Rgba::BLACK).with(ColorTransform::Alpha(50000)))] {
        let mut t = table();
        t.style = "own-gradient".into();
        let mut p = deck(t);
        p.table_styles.push(TableStyleDef {
            id: "own-gradient".into(),
            parts: vec![(TablePart::WholeTbl, TablePartStyle { fill: Some(fill), ..Default::default() })],
            ..Default::default()
        });
        assert!(export(&p).unwrap_err().0.contains("cell fill"));
        let ShapeKind::Table(t) = &mut Arc::make_mut(&mut p.slides[0]).shapes[0].kind else { panic!("table") };
        for row in &mut t.rows {
            for cell in &mut row.cells {
                cell.fill = Some(Fill::None);
            }
        }
        let bytes = export(&p).unwrap();
        assert!(part(&bytes, "content.xml").contains("fo:background-color=\"transparent\""));
    }
}
#[test]
fn explicit_no_fill_does_not_inherit_the_builtin_table_color() {
    let mut t = table();
    t.style = "medium2-accent1".into();
    t.first_row = true;
    for row in &mut t.rows {
        for cell in &mut row.cells {
            cell.text = TextBody::default();
        }
    }
    t.rows[0].cells[0].fill = Some(Fill::None);
    let p = deck(t);
    let bytes = export(&p).unwrap();
    let q = import(&bytes).unwrap();
    let a = deckcraft_render::render_slide(&p, 0, &Default::default());
    let b = deckcraft_render::render_slide(&q, 0, &Default::default());
    for (x, y) in [(50, 50), (150, 50), (250, 50), (50, 140)] {
        assert_eq!(a.pixel(x, y), b.pixel(x, y));
    }
}
