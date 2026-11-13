use super::*;
use kurbo::Shape;

#[test]
fn every_catalog_preset_builds_within_bounds() {
    for p in CATALOG {
        for (w, h) in [(200.0, 100.0), (100.0, 200.0), (50.0, 50.0), (0.0, 0.0), (1.0, 300.0)] {
            let g = build(p.name, w, h, &[]).unwrap_or_else(|| panic!("{} did not build", p.name));
            assert!(!g.paths.is_empty(), "{} has no paths", p.name);
            for s in &g.paths {
                let bb = s.path.bounding_box();
                if w > 0.0 && h > 0.0 && !p.name.contains("Connector") && !p.name.contains("allout") {
                    let tol = w.max(h) * 0.35 + 1.0;
                    assert!(bb.x0 >= -tol && bb.y0 >= -tol && bb.x1 <= w + tol && bb.y1 <= h + tol, "{} {w}x{h} out of bounds: {bb:?}", p.name);
                }
                assert!(bb.x0.is_finite() && bb.y1.is_finite(), "{} produced non-finite geometry", p.name);
            }
            assert!(g.text_rect.x0.is_finite() && g.text_rect.y1.is_finite(), "{}", p.name);
        }
    }
}

#[test]
fn hostile_adjust_values_never_panic() {
    for p in CATALOG {
        for v in [f64::NAN, f64::INFINITY, -1e12, 1e12, -1.0, 0.0] {
            let adj = vec![v; 6];
            let g = build(p.name, 120.0, 80.0, &adj);
            assert!(g.is_some(), "{}", p.name);
        }
    }
    assert!(build("rect", f64::NAN, 10.0, &[]).is_none());
    assert!(build("noSuchShape", 10.0, 10.0, &[]).is_none());
}

#[test]
fn catalog_names_are_unique() {
    let mut names: Vec<&str> = CATALOG.iter().map(|p| p.name).collect();
    names.sort_unstable();
    let n = names.len();
    names.dedup();
    assert_eq!(n, names.len());
    assert!(n >= 140, "catalog has {n} presets");
}

#[test]
fn round_rect_handle_maps_back() {
    let g = build("roundRect", 200.0, 100.0, &[16667.0]).unwrap();
    let h = g.handles.first().unwrap();
    assert!((h.value_at(h.pos) - 16667.0).abs() <= 1.0);
    assert_eq!(h.value_at(Point::new(-50.0, 0.0)), 0.0);
    assert_eq!(h.value_at(Point::new(500.0, 0.0)), 50000.0);
}

#[test]
fn partial_circle_angle_handles_map_back() {
    // Start 0° sits at the right of the ellipse, end 270° at its top.
    for name in ["pie", "chord", "arc"] {
        let g = build(name, 200.0, 100.0, &[0.0, 16200000.0]).unwrap();
        assert_eq!(g.handles.iter().map(|h| h.index).collect::<Vec<_>>(), vec![0, 1], "{name}");
        let (s, e) = (&g.handles[0], &g.handles[1]);
        assert!((s.pos - Point::new(200.0, 50.0)).hypot() < 1e-6, "{name} {:?}", s.pos);
        assert!((e.pos - Point::new(100.0, 0.0)).hypot() < 1e-6, "{name} {:?}", e.pos);
        assert_eq!(s.value_at(s.pos), 0.0, "{name}");
        assert_eq!(e.value_at(e.pos), 16200000.0, "{name}");
        // Dragging to the bottom of the ellipse gives 90°.
        assert_eq!(s.value_at(Point::new(100.0, 100.0)), 5400000.0, "{name}");
        // Off-axis angles on a non-square ellipse: the visual 45° ray meets the 200×100
        // ellipse at offset (√2000, √2000) from the centre.
        let k = 2000f64.sqrt();
        let g = build(name, 200.0, 100.0, &[2700000.0, 8100000.0]).unwrap();
        let (s, e) = (&g.handles[0], &g.handles[1]);
        assert!((s.pos - Point::new(100.0 + k, 50.0 + k)).hypot() < 1e-6, "{name} {:?}", s.pos);
        assert!((e.pos - Point::new(100.0 - k, 50.0 + k)).hypot() < 1e-6, "{name} {:?}", e.pos);
        for (deg, h) in [(45.0, s), (135.0, e)] {
            let on = ((h.pos.x - 100.0) / 100.0).powi(2) + ((h.pos.y - 50.0) / 50.0).powi(2);
            assert!((on - 1.0).abs() < 1e-9, "{name} {deg}° off ellipse: {:?}", h.pos);
            assert!((h.value_at(h.pos) - deg * 60000.0).abs() <= 1.0, "{name} {deg}°: {}", h.value_at(h.pos));
        }
    }
    for deg in [45.0, 135.0, 200.0, 315.0] {
        let g = build("blockArc", 200.0, 100.0, &[deg * 60000.0, 0.0, 25000.0]).unwrap();
        let start = g.handles.iter().find(|h| h.index == 0).unwrap();
        // Thickness 25 → the handle runs on the 87.5×37.5 mid-ring ellipse.
        let on = ((start.pos.x - 100.0) / 87.5).powi(2) + ((start.pos.y - 50.0) / 37.5).powi(2);
        assert!((on - 1.0).abs() < 1e-9, "blockArc {deg}° off ring: {:?}", start.pos);
        let d = start.pos - Point::new(100.0, 50.0);
        assert!((d.y.atan2(d.x).to_degrees().rem_euclid(360.0) - deg).abs() < 1e-6, "blockArc {deg}°: {:?}", start.pos);
        assert!((start.value_at(start.pos) - deg * 60000.0).abs() <= 1.0, "blockArc {deg}°");
    }
    let g = build("blockArc", 200.0, 100.0, &[10800000.0, 0.0, 25000.0]).unwrap();
    let start = g.handles.iter().find(|h| h.index == 0).unwrap();
    assert_eq!(start.value_at(start.pos), 10800000.0);
    let thick = g.handles.iter().find(|h| h.index == 2).unwrap();
    assert!((thick.value_at(thick.pos) - 25000.0).abs() <= 1.0);
}

#[test]
fn presets_with_adjust_values_expose_handles() {
    // Presets whose adjust values have no handle of their own yet.
    const NO_HANDLES: &[&str] = &[
        "curvedConnector3",
        "snip2SameRect",
        "snip2DiagRect",
        "snipRoundRect",
        "round1Rect",
        "round2SameRect",
        "round2DiagRect",
        "teardrop",
        "halfFrame",
        "corner",
        "diagStripe",
        "foldedCorner",
        "smileyFace",
        "moon",
        "gear6",
        "gear9",
        "bracketPair",
        "bracePair",
        "leftBracket",
        "rightBracket",
        "leftBrace",
        "rightBrace",
        "leftRightArrow",
        "upDownArrow",
        "quadArrow",
        "leftRightUpArrow",
        "bentArrow",
        "uturnArrow",
        "curvedRightArrow",
        "curvedLeftArrow",
        "curvedUpArrow",
        "curvedDownArrow",
        "leftUpArrow",
        "bentUpArrow",
        "circularArrow",
        "leftCircularArrow",
        "leftRightCircularArrow",
        "swooshArrow",
        "rightArrowCallout",
        "downArrowCallout",
        "leftArrowCallout",
        "upArrowCallout",
        "leftRightArrowCallout",
        "upDownArrowCallout",
        "quadArrowCallout",
        "mathPlus",
        "mathMinus",
        "mathMultiply",
        "mathDivide",
        "mathEqual",
        "mathNotEqual",
        "ribbon2",
        "ribbon",
        "ellipseRibbon2",
        "ellipseRibbon",
        "leftRightRibbon",
        "verticalScroll",
        "horizontalScroll",
        "wave",
        "doubleWave",
        "borderCallout1",
        "borderCallout2",
        "borderCallout3",
        "accentCallout1",
        "accentCallout2",
        "accentCallout3",
        "callout1",
        "callout2",
        "callout3",
        "accentBorderCallout1",
        "accentBorderCallout2",
        "accentBorderCallout3",
    ];
    for p in CATALOG {
        if p.defaults.is_empty() || NO_HANDLES.contains(&p.name) {
            continue;
        }
        let g = build(p.name, 200.0, 100.0, &[]).unwrap();
        assert!(!g.handles.is_empty(), "{} has adjust values but no handles", p.name);
    }
}

#[test]
fn rect_area_and_ellipse_area() {
    let g = build("rect", 100.0, 50.0, &[]).unwrap();
    assert!((g.paths[0].path.area().abs() - 5000.0).abs() < 1e-6);
    let e = build("ellipse", 100.0, 50.0, &[]).unwrap();
    let expect = std::f64::consts::PI * 50.0 * 25.0;
    assert!((e.paths[0].path.area().abs() - expect).abs() / expect < 0.01);
}

#[test]
fn lines_are_open() {
    assert!(build("line", 10.0, 10.0, &[]).unwrap().is_open());
    assert!(!build("rect", 10.0, 10.0, &[]).unwrap().is_open());
}

fn on_path(g: &Geometry, p: Point, tol: f64) -> bool {
    use kurbo::ParamCurveNearest;
    g.paths.iter().any(|sp| sp.path.segments().any(|seg| seg.nearest(p, 1e-6).distance_sq <= tol * tol))
}

#[test]
fn fixed_shapes_follow_the_standards_coordinates() {
    // Points of ECMA-376's definitions (in 21600ths), checked on a 216 × 108 box.
    let (w, h) = (216.0, 108.0);
    let at = |x: f64, y: f64| Point::new(w * x / 21600.0, h * y / 21600.0);
    let g = build("lightningBolt", w, h, &[]).unwrap();
    for (x, y) in [(8472.0, 0.0), (16577.0, 12007.0), (21600.0, 21600.0), (0.0, 3890.0)] {
        assert!(on_path(&g, at(x, y), 1e-6), "lightningBolt ({x}, {y})");
    }
    let g = build("irregularSeal1", w, h, &[]).unwrap();
    assert!(on_path(&g, at(14522.0, 0.0), 1e-6) && on_path(&g, at(8485.0, 21600.0), 1e-6));
    let g = build("irregularSeal2", w, h, &[]).unwrap();
    assert!(on_path(&g, at(14790.0, 0.0), 1e-6) && on_path(&g, at(4917.0, 21600.0), 1e-6));
    // The heart's point is the middle of the bottom edge, its notch a quarter down.
    let g = build("heart", w, h, &[]).unwrap();
    assert!(on_path(&g, Point::new(w / 2.0, h), 1e-6) && on_path(&g, Point::new(w / 2.0, h / 4.0), 1e-6));
    let g = build("flowChartDocument", w, h, &[]).unwrap();
    assert!(on_path(&g, at(21600.0, 17322.0), 1e-6) && on_path(&g, at(0.0, 20172.0), 1e-6));
    // Punched tape: the top edge dips to a fifth of the height a quarter of the way along.
    let g = build("flowChartPunchedTape", w, h, &[]).unwrap();
    assert!(on_path(&g, Point::new(w / 4.0, h / 5.0), 1e-3) && on_path(&g, Point::new(w * 0.75, 0.0), 1e-3));
    let g = build("flowChartMultidocument", w, h, &[]).unwrap();
    assert!(on_path(&g, at(2972.0, 0.0), 1e-6) && on_path(&g, at(18595.0, 18022.0), 1e-6));
}

#[test]
fn notched_arrow_notch_parallels_the_head() {
    // presetShapeDefinitions: the notch's point is at x1 = dy1 · dx2 / hd2, so its edges run
    // parallel to the head's (25 on a 200 × 100 arrow at the default adjusts).
    for (w, h, adj, x1) in [(200.0, 100.0, [50000.0, 50000.0], 25.0), (300.0, 120.0, [80000.0, 30000.0], 28.8)] {
        let g = build("notchedRightArrow", w, h, &adj).unwrap();
        let notch = g.paths[0].path.elements().iter().find_map(|e| match e {
            kurbo::PathEl::MoveTo(p) | kurbo::PathEl::LineTo(p) if (p.y - h / 2.0).abs() < 1e-9 && p.x < w / 2.0 => Some(*p),
            _ => None,
        });
        assert!(notch.is_some_and(|p| (p.x - x1).abs() < 1e-6), "{w}x{h} {adj:?}: notch at {notch:?}, want x = {x1}");
    }
}

#[test]
fn equation_shapes_match_powerpoints_outlines() {
    // Corners read off PowerPoint's own outlines of the presets at their default adjusts.
    let g = build("mathMultiply", 216.0, 216.0, &[]).unwrap();
    for (x, y) in [(0.32333, 0.15702), (0.5, 0.33369), (0.84298, 0.32333), (0.66631, 0.5), (0.33369, 0.5)] {
        assert!(on_path(&g, Point::new(216.0 * x, 216.0 * y), 0.01), "multiply ({x}, {y})");
    }
    let g = build("mathEqual", 300.0, 120.0, &[]).unwrap();
    for (x, y) in [(0.13255, 0.5588), (0.86745, 0.794), (0.13255, 0.206), (0.86745, 0.4412)] {
        assert!(on_path(&g, Point::new(300.0 * x, 120.0 * y), 0.01), "equal ({x}, {y})");
    }
    let g = build("mathPlus", 216.0, 216.0, &[]).unwrap();
    assert!(on_path(&g, Point::new(216.0 * 0.3824, 216.0 * 0.13255), 0.01) && on_path(&g, Point::new(216.0 * 0.86745, 216.0 * 0.6176), 0.01));
    // A tall "not equal": its slash, leaning at the default 110° from top to bottom, reaches
    // past the box's sides exactly as far as PowerPoint's (-22.242 to 122.242).
    let g = build("mathNotEqual", 100.0, 260.0, &[]).unwrap();
    use kurbo::Shape as _;
    let bb = g.paths[0].path.bounding_box();
    assert!((bb.x0 + 22.242).abs() < 0.01 && (bb.x1 - 122.242).abs() < 0.01 && bb.y0.abs() < 1e-6 && (bb.y1 - 260.0).abs() < 1e-6, "{bb:?}");
}

#[test]
fn sun_and_waves_match_powerpoints_outlines() {
    // Points read off PowerPoint's outlines of the presets (216 pt square, default adjusts).
    let k = 216.0;
    let g = build("sun", k, k, &[]).unwrap();
    for (x, y) in [(0.85352, 0.14644), (0.76161, 0.33987), (0.66013, 0.23839), (0.42825, 0.79823), (0.20177, 0.57175), (0.75, 0.5), (0.5, 0.25)] {
        assert!(on_path(&g, Point::new(k * x, k * y), 0.02), "sun ({x}, {y})");
    }
    let g = build("wave", k, k, &[]).unwrap();
    for (x, y) in [(0.21875, 0.00484), (1.0, 0.125), (1.0, 0.875), (0.0, 0.875)] {
        assert!(on_path(&g, Point::new(k * x, k * y), 0.05), "wave ({x}, {y})");
    }
    // Shifted: a wave of 300 × 120 with adj (20000, 8000) starts 0.16 of the width in.
    let g = build("wave", 300.0, 120.0, &[20000.0, 8000.0]).unwrap();
    for (x, y) in [(0.18375, 0.00774), (0.84, 0.2), (1.0, 0.8), (0.16, 0.8)] {
        assert!(on_path(&g, Point::new(300.0 * x, 120.0 * y), 0.05), "shifted wave ({x}, {y})");
    }
}

#[test]
fn star_text_rectangles_follow_the_standard() {
    // star5 at its default 19098 in a 144 pt square: the inner pentagon, about 0.38 of the
    // width (PowerPoint wraps "Shape 3" at 18 pt there), from y ≈ 55 to ≈ 110.
    let g = build("star5", 144.0, 144.0, &[]).unwrap();
    let t = g.text_rect;
    assert!((t.width() - 55.0).abs() < 1.0, "{t:?}");
    assert!((t.y0 - 55.0).abs() < 1.0 && (t.y1 - 110.0).abs() < 1.0, "{t:?}");
    // star4 at 12500: the square through its inner points' 45° corners.
    let g = build("star4", 144.0, 144.0, &[]).unwrap();
    assert!((g.text_rect.width() - 72.0 * 0.25 * 2f64.sqrt()).abs() < 0.5, "{:?}", g.text_rect);
}

#[test]
fn stars_are_stretched_like_the_standards() {
    // star5 in a 216 pt square: top point at the top middle, side points at 18 degrees on an
    // ellipse 5.146% wider and 10.557% taller, centred 10.557% below the middle; its bottom
    // points reach the box's bottom edge.
    let k = 216.0;
    let g = build("star5", k, k, &[]).unwrap();
    let (rx, ry, cy) = (k / 2.0 * 1.05146, k / 2.0 * 1.10557, k / 2.0 * 1.10557);
    let a = 18f64.to_radians();
    assert!(on_path(&g, Point::new(k / 2.0, 0.0), 1e-6));
    assert!(on_path(&g, Point::new(k / 2.0 + rx * a.cos(), cy - ry * a.sin()), 1e-6));
    use kurbo::Shape as _;
    let bb = g.paths[0].path.bounding_box();
    assert!((bb.x0).abs() < 0.5 && (bb.x1 - k).abs() < 0.5 && (bb.y1 - k).abs() < 0.5, "{bb:?}");
    // star6 is 15.47% wider than its box's half-width allows (its points touch the sides).
    let g = build("star6", k, k, &[]).unwrap();
    let bb = g.paths[0].path.bounding_box();
    assert!((bb.x0).abs() < 0.5 && (bb.x1 - k).abs() < 0.5, "{bb:?}");
}

#[test]
fn moon_disk_and_drum_match_powerpoints_outlines() {
    let k = 216.0;
    // Moon, default: the inner edge runs through (0.5, 0.5) and (0.625, 0.2266) of the box.
    let g = build("moon", k, k, &[]).unwrap();
    for (x, y) in [(0.5, 0.5), (0.626, 0.2266), (0.0, 0.5), (1.0, 0.0)] {
        assert!(on_path(&g, Point::new(k * x, k * y), 0.5), "moon ({x}, {y})");
    }
    // 300 × 120 at 30%: its inner edge reaches 30% of the width in.
    let g = build("moon", 300.0, 120.0, &[30000.0]).unwrap();
    assert!(on_path(&g, Point::new(90.0, 60.0), 0.5));
    // Magnetic disk ends are ellipses h/6 high, drum ends w/6 across.
    let g = build("flowChartMagneticDisk", k, k, &[]).unwrap();
    assert!(on_path(&g, Point::new(k, k / 6.0), 0.01) && on_path(&g, Point::new(k / 2.0, k / 3.0), 0.5));
    let g = build("flowChartMagneticDrum", k, k, &[]).unwrap();
    assert!(on_path(&g, Point::new(k / 6.0, 0.0), 0.01) && on_path(&g, Point::new(0.0, k / 2.0), 0.01));
}

#[test]
fn the_cloud_fills_its_box_at_any_aspect_and_has_creases() {
    use kurbo::Shape as _;
    for (w, h) in [(200.0, 200.0), (300.0, 100.0), (60.0, 180.0)] {
        let g = build("cloud", w, h, &[]).unwrap();
        let bb = g.paths[0].path.bounding_box();
        assert!(bb.x0.abs() < w * 0.02 && (bb.x1 - w).abs() < w * 0.02, "{w}×{h}: {bb:?}");
        assert!(bb.y0.abs() < h * 0.02 && (bb.y1 - h).abs() < h * 0.02, "{w}×{h}: {bb:?}");
        assert_eq!(g.paths.len(), 2, "outline and creases");
        assert!(g.paths[1].stroke && g.paths[1].fill == FillMode::None);
    }
}

#[test]
fn spec_arcs_take_angles_as_directions_from_the_centre() {
    // A DrawingML arc of a 2:1 ellipse from 45° to 90°: 45° is the direction (1, 1) from the
    // centre, which on that ellipse is the point (r/√5)·(2, 1)… scaled so it lies on it.
    let mut b = Pb::new();
    b.m(0.0, 0.0).arc_spec(200.0, 100.0, 45.0, 45.0);
    let p = b.take();
    let end = p.elements().last().and_then(|e| match e {
        kurbo::PathEl::CurveTo(_, _, q) => Some(*q),
        _ => None,
    });
    // Centre: the start sits at direction 45°, i.e. at (k, k) from it with (k/200)² + (k/100)² = 1.
    let k = 1.0 / (1.0f64 / 40000.0 + 1.0 / 10000.0).sqrt();
    let c = Point::new(-k, -k);
    let q = end.unwrap();
    assert!((q.x - c.x).abs() < 1e-6 && (q.y - (c.y + 100.0)).abs() < 1e-6, "{q:?} vs centre {c:?}");
}

#[test]
fn brackets_and_braces_fill_out_to_their_straight_side() {
    // PowerPoint fills a bracket or brace (when it has a fill) between its curve and the
    // straight side it opens to; only the curve is outlined.
    for name in ["leftBracket", "rightBracket", "leftBrace", "rightBrace"] {
        let g = build(name, 40.0, 100.0, &[]).unwrap();
        let filled: Vec<_> = g.paths.iter().filter(|p| p.fill != FillMode::None).collect();
        assert_eq!(filled.len(), 1, "{name}");
        assert!(!filled[0].stroke, "{name}: the filled area is not outlined");
        let area = filled[0].path.area().abs();
        assert!(area > 40.0 * 100.0 * 0.4 && area < 40.0 * 100.0, "{name}: {area}");
        assert!(g.paths.iter().any(|p| p.stroke && p.fill == FillMode::None), "{name}: the curve is outlined");
    }
}

/// The end points of a path's move/line elements, in order.
fn vertices(p: &BezPath) -> Vec<Point> {
    p.elements()
        .iter()
        .filter_map(|e| match e {
            kurbo::PathEl::MoveTo(q) | kurbo::PathEl::LineTo(q) => Some(*q),
            _ => None,
        })
        .collect()
}

#[test]
fn wedge_callout_mirrors_with_its_tip() {
    // A tip below and right of the centre draws the mirror image of the same tip on the left;
    // the same for a tip right of the centre, above or below it. presetShapeDefinitions puts
    // the wedge's base at 2..5 twelfths of the edge on the near side of the centre and at
    // 7..10 twelfths on the far side.
    let (w, h) = (240.0, 120.0);
    // Sorted and deduplicated: the outline closes on the corner it starts from, which the
    // mirror moves to the opposite corner.
    let sorted = |mut v: Vec<Point>| {
        v.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
        v.dedup_by(|a, b| (*a - *b).hypot() < 1e-9);
        v
    };
    for (right, left, flip_x) in [([10000.0, 80000.0], [-10000.0, 80000.0], true), ([80000.0, 10000.0], [80000.0, -10000.0], false)] {
        let a = vertices(&build("wedgeRectCallout", w, h, &right).unwrap().paths[0].path);
        let b = vertices(&build("wedgeRectCallout", w, h, &left).unwrap().paths[0].path);
        let b: Vec<Point> = b.iter().map(|p| if flip_x { Point::new(w - p.x, p.y) } else { Point::new(p.x, h - p.y) }).collect();
        let (a, b) = (sorted(a), sorted(b));
        assert_eq!(a.len(), b.len(), "{right:?}: {a:?} vs mirrored {left:?}: {b:?}");
        for (p, q) in a.iter().zip(&b) {
            assert!((*p - *q).hypot() < 1e-6, "{right:?}: {a:?}\n is not the mirror of {left:?}: {b:?}");
        }
    }
    // The bottom wedge of a tip right of the centre leaves the edge at 7/12 and 10/12 of the width.
    let a = vertices(&build("wedgeRectCallout", w, h, &[10000.0, 80000.0]).unwrap().paths[0].path);
    for x in [w * 7.0 / 12.0, w * 10.0 / 12.0] {
        assert!(a.iter().any(|p| (p.x - x).abs() < 1e-6 && (p.y - h).abs() < 1e-6), "no wedge corner at ({x}, {h}): {a:?}");
    }
}
