//! A checkbox's shapes: a box and a stroked tick or cross.

use deckcraft_geom::Xfrm;
use deckcraft_geom::preset::FillMode;
use deckcraft_model::style::{LineCap, LineJoin};
use deckcraft_model::{CheckState, Checkbox, CustomPath, Fill, Geom, Line, Shape};

/// The path space the tick and cross are drawn in (a `.pptx` stores path points as integers).
const UNIT: f64 = 100.0;
/// (0.20, 0.55) → (0.42, 0.75) → (0.80, 0.28) of the square.
const TICK: &str = "M 20 55 L 42 75 L 80 28";
/// (0.25, 0.25)–(0.75, 0.75) and (0.75, 0.25)–(0.25, 0.75).
const CROSS: &str = "M 25 25 L 75 75 M 75 25 L 25 75";

/// The checkbox's shapes inside square `sq` (child space), in z-order, with id 0: the box, then the
/// tick or cross. The box is always there so the group keeps its frame; unboxed, it has no line.
pub(super) fn shapes(c: &Checkbox, sq: Xfrm) -> Vec<Shape> {
    let size = sq.w.min(sq.h).max(0.0);
    let line = if c.boxed { Line::solid(c.color.clone(), (size / 14.0).max(0.75)) } else { Line::none() };
    let mut v = vec![Shape {
        name: "Box".into(),
        xfrm: Some(sq),
        geom: Geom::preset("rect"),
        fill: Some(Fill::None),
        line: Some(line),
        ..Default::default()
    }];
    let mark = match c.state {
        CheckState::Unchecked => return v,
        CheckState::Checked => ("Tick", TICK),
        CheckState::Crossed => ("Cross", CROSS),
    };
    v.push(Shape {
        name: mark.0.into(),
        xfrm: Some(sq),
        geom: Geom::Custom { paths: vec![CustomPath { w: UNIT, h: UNIT, d: mark.1.into(), fill: FillMode::None, stroke: true }] },
        fill: Some(Fill::None),
        line: Some(Line {
            fill: Some(Fill::solid(c.color.clone())),
            width: Some(size / 9.0),
            cap: Some(LineCap::Round),
            join: Some(LineJoin::Round),
            ..Default::default()
        }),
        ..Default::default()
    });
    v
}
