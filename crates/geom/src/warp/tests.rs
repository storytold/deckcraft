use super::{
    Band, BezPath, CATALOG, Guide, Point, Rect, WarpLine, adjust_count, adjust_defaults, band_of_line, handles, is_follow, is_supported, warp,
};

const W: f64 = 420.0;
const H: f64 = 200.0;

fn rect() -> Rect {
    Rect::new(0.0, 0.0, W, H)
}

/// One line whose ink box is `ink`.
fn one_line(ink: Rect) -> Vec<WarpLine> {
    vec![WarpLine { ink: Some(ink), extent: (ink.x0, ink.x1), ref_y: ink.y1 }]
}

/// Where the unit-square point (u, v) of a single line's ink box lands, in unit coordinates.
fn land(name: &str, adj: &[f64], u: f64, v: f64) -> (f64, f64) {
    let w = warp(name, adj, rect()).unwrap();
    let ink = Rect::new(100.0, 50.0, 300.0, 90.0);
    let lines = one_line(ink);
    let m = w.mapper(&lines, rect().inset(-5.0));
    let p = m.map(0, Point::new(ink.x0 + u * ink.width(), ink.y0 + v * ink.height()));
    (p.x / W, p.y / H)
}

fn close(a: (f64, f64), b: (f64, f64), tol: f64) -> bool {
    (a.0 - b.0).abs() <= tol && (a.1 - b.1).abs() <= tol
}

#[track_caller]
fn corners(name: &str, adj: &[f64], tl: (f64, f64), tr: (f64, f64), bl: (f64, f64), br: (f64, f64)) {
    for ((u, v), want) in [((0.0, 0.0), tl), ((1.0, 0.0), tr), ((0.0, 1.0), bl), ((1.0, 1.0), br)] {
        let got = land(name, adj, u, v);
        assert!(close(got, want, 0.004), "{name} corner ({u},{v}): got {got:?}, want {want:?}");
    }
}

#[test]
fn catalog_is_the_forty_drawn_presets() {
    assert_eq!(CATALOG.len(), 40);
    let mut seen = std::collections::BTreeSet::new();
    for p in CATALOG {
        assert!(seen.insert(p.id), "duplicate {}", p.id);
        assert!(!p.label.is_empty(), "{}", p.id);
        assert!(is_supported(p.id), "{}", p.id);
        assert_eq!(is_follow(p.id), matches!(p.id, "textArchUp" | "textArchDown" | "textCircle" | "textButton"), "{}", p.id);
    }
}

#[test]
fn every_preset_builds_finite_guides() {
    for p in CATALOG {
        let name = p.id;
        assert!(is_supported(name), "{name}");
        let w = warp(name, &[], rect()).unwrap_or_else(|| panic!("{name}"));
        assert!(!w.bands.is_empty(), "{name}");
        for b in &w.bands {
            let l = b.length();
            assert!(l.is_finite() && l > 10.0, "{name}: band length {l}");
        }
        assert_eq!(adjust_count(name), adjust_defaults(name).len());
    }
    assert!(warp("textNoShape", &[], rect()).is_none());
    assert!(warp("bogus", &[], rect()).is_none());
    assert!(!is_supported("textNoShape"));
}

#[test]
fn straight_presets_put_the_ink_corners_on_their_guides() {
    corners("textPlain", &[], (0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0));
    // A smaller adjust shears the text: the top edge starts 40 % in, the bottom edge ends 60 % in.
    corners("textPlain", &[30_000.0], (0.4, 0.0), (1.0, 0.0), (0.0, 1.0), (0.6, 1.0));
    corners("textStop", &[], (0.0, 0.25), (1.0, 0.25), (0.0, 0.75), (1.0, 0.75));
    corners("textTriangle", &[], (0.0, 0.5), (1.0, 0.5), (0.0, 1.0), (1.0, 1.0));
    corners("textTriangleInverted", &[25_000.0], (0.0, 0.0), (1.0, 0.0), (0.0, 0.25), (1.0, 0.25));
    corners("textChevron", &[], (0.0, 0.25), (1.0, 0.25), (0.0, 1.0), (1.0, 1.0));
    corners("textChevronInverted", &[], (0.0, 0.0), (1.0, 0.0), (0.0, 0.75), (1.0, 0.75));
    corners("textFadeRight", &[], (0.0, 0.0), (1.0, 0.33333), (0.0, 1.0), (1.0, 0.66667));
    corners("textFadeLeft", &[], (0.0, 0.33333), (1.0, 0.0), (0.0, 0.66667), (1.0, 1.0));
    corners("textFadeUp", &[], (0.33333, 0.0), (0.66667, 0.0), (0.0, 1.0), (1.0, 1.0));
    corners("textFadeDown", &[], (0.0, 0.0), (1.0, 0.0), (0.33333, 1.0), (0.66667, 1.0));
    corners("textSlantUp", &[], (0.0, 0.55556), (1.0, 0.0), (0.0, 1.0), (1.0, 0.44444));
    corners("textSlantDown", &[], (0.0, 0.0), (1.0, 0.55556), (0.0, 0.44444), (1.0, 1.0));
    corners("textCascadeUp", &[], (0.0, 0.13889), (1.0, 0.0), (0.0, 1.0), (1.0, 0.44444));
    corners("textCascadeDown", &[], (0.0, 0.0), (1.0, 0.13889), (0.0, 0.44444), (1.0, 1.0));
}

#[test]
fn curved_presets_bend_where_powerpoint_bends_them() {
    corners("textInflate", &[], (0.0, 0.1875), (1.0, 0.1875), (0.0, 0.8125), (1.0, 0.8125));
    assert!(close(land("textInflate", &[], 0.5, 0.0), (0.5, 0.0), 0.004));
    assert!(close(land("textInflate", &[], 0.5, 1.0), (0.5, 1.0), 0.004));
    corners("textDeflate", &[], (0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0));
    assert!(close(land("textDeflate", &[], 0.5, 0.0), (0.5, 0.1875), 0.004));
    corners("textInflateBottom", &[], (0.0, 0.0), (1.0, 0.0), (0.0, 0.6), (1.0, 0.6));
    assert!(close(land("textInflateBottom", &[], 0.5, 1.0), (0.5, 0.8), 0.004));
    assert!(close(land("textDeflateBottom", &[], 0.5, 1.0), (0.5, 0.5), 0.004));
    assert!(close(land("textInflateTop", &[], 0.5, 0.0), (0.5, 0.2), 0.004));
    assert!(close(land("textDeflateTop", &[], 0.5, 0.0), (0.5, 0.5), 0.004));
    // Cans: half ellipses.
    corners("textCanUp", &[], (0.0, 0.15), (1.0, 0.15), (0.0, 1.0), (1.0, 1.0));
    assert!(close(land("textCanUp", &[], 0.5, 0.0), (0.5, 0.0), 0.004));
    assert!(close(land("textCanUp", &[], 0.5, 1.0), (0.5, 0.85), 0.004));
    assert!(close(land("textCanDown", &[], 0.5, 0.0), (0.5, 0.14286), 0.004));
    // Curves and waves.
    let a = 0.45977;
    corners("textCurveUp", &[], (0.0, a), (1.0, 0.0), (0.0, 1.0 - 3.0 * a / 8.0), (1.0, 1.0 - 3.0 * a / 8.0));
    assert!(close(land("textCurveUp", &[], 0.5, 1.0), (0.5, 1.0), 0.004));
    corners("textCurveDown", &[], (0.0, 0.0), (1.0, a), (0.0, 1.0 - 3.0 * a / 8.0), (1.0, 1.0 - 3.0 * a / 8.0));
    corners("textWave1", &[], (0.0, 0.125), (1.0, 0.125), (0.0, 0.875), (1.0, 0.875));
    // Wave 1 rises first: its top is near the box top a quarter of the way along.
    assert!(land("textWave1", &[], 0.25, 0.0).1 < 0.02);
    assert!(land("textWave2", &[], 0.25, 0.0).1 > 0.23);
    // A horizontal shift moves the bottom guide right.
    corners("textWave1", &[12_500.0, 5_000.0], (0.0, 0.125), (0.9, 0.125), (0.1, 0.875), (1.0, 0.875));
    assert!(land("textDoubleWave1", &[], 0.125, 0.0).1 < 0.02);
    assert!(land("textWave4", &[], 0.125, 0.0).1 > 0.1);
}

#[test]
fn arc_presets_follow_ellipses() {
    // Pour arches: the outer arc from the left end over the top; the inner one is scaled by adj2.
    corners("textArchUpPour", &[], (0.0, 0.5), (1.0, 0.5), (0.25, 0.5), (0.75, 0.5));
    assert!(close(land("textArchUpPour", &[], 0.5, 0.0), (0.5, 0.0), 0.004));
    assert!(close(land("textArchUpPour", &[], 0.5, 1.0), (0.5, 0.25), 0.004));
    assert!(close(land("textArchUpPour", &[10_800_000.0, 30_000.0], 0.5, 1.0), (0.5, 0.35), 0.004));
    assert!(close(land("textArchDownPour", &[], 0.5, 0.0), (0.5, 0.625), 0.004));
    assert!(close(land("textArchDownPour", &[], 0.5, 1.0), (0.5, 1.0), 0.004));
    // Circle pour: once round from the left; the middle of the text is at the right.
    assert!(close(land("textCirclePour", &[], 0.5, 0.0), (1.0, 0.5), 0.004));
    // Rings: the text goes round two stacked ellipses.
    assert!(close(land("textRingInside", &[], 0.0, 0.0), (0.0, 0.3), 0.004));
    assert!(close(land("textRingInside", &[], 0.0, 1.0), (0.0, 0.7), 0.004));
    assert!(close(land("textRingInside", &[], 0.25, 0.0), (0.5, 0.0), 0.01));
    assert!(close(land("textRingOutside", &[], 0.25, 1.0), (0.5, 1.0), 0.01));
}

#[test]
fn follow_presets_keep_text_size_and_stack_lines_outwards() {
    let w = warp("textArchUp", &[], rect()).unwrap();
    // A 40-wide centred line whose reference height is 80.
    let line = WarpLine { ink: Some(Rect::new(190.0, 50.0, 230.0, 88.0)), extent: (190.0, 230.0), ref_y: 80.0 };
    let inner = Rect::new(7.2, 3.6, W - 7.2, H - 3.6);
    let m = w.mapper(&[line], inner);
    // The middle of the reference line lands on the top of the ellipse; 10 pt above it lands
    // 10 pt above the top.
    let p = m.map(0, Point::new(210.0, 80.0));
    assert!((p.x - 210.0).abs() < 0.05 && p.y.abs() < 0.05, "{p:?}");
    let q = m.map(0, Point::new(210.0, 70.0));
    assert!((q.x - 210.0).abs() < 0.15 && (q.y + 10.0).abs() < 0.05, "{q:?}");
    // Natural size: 40 pt of text spans about 40 pt of arc.
    let (a, b) = (m.map(0, Point::new(190.0, 80.0)), m.map(0, Point::new(230.0, 80.0)));
    assert!(((b - a).hypot() - 40.0).abs() < 0.5);
    // Left-aligned text starts at the very start of the arc: insets don't shorten it.
    let left = WarpLine { ink: Some(Rect::new(7.2, 50.0, 47.2, 88.0)), extent: (7.2, 47.2), ref_y: 80.0 };
    let m = w.mapper(&[left], inner);
    let p = m.map(0, Point::new(7.2, 80.0));
    assert!(p.x.abs() < 0.01 && (p.y - 100.0).abs() < 0.01, "{p:?}");
    // Two lines on an arch up: the last sits on the arc, the first stacks outside it.
    let l1 = WarpLine { ink: Some(Rect::new(190.0, 10.0, 230.0, 40.0)), extent: (190.0, 230.0), ref_y: 34.0 };
    let l2 = WarpLine { ink: Some(Rect::new(190.0, 50.0, 230.0, 80.0)), extent: (190.0, 230.0), ref_y: 74.0 };
    let m = w.mapper(&[l1, l2], inner);
    assert!(m.map(1, Point::new(210.0, 74.0)).y.abs() < 0.05);
    assert!((m.map(0, Point::new(210.0, 34.0)).y + 40.0).abs() < 0.05);
    // Arch down: the first line sits on the bottom of the ellipse.
    let w = warp("textArchDown", &[], rect()).unwrap();
    let m = w.mapper(&[l1, l2], inner);
    assert!((m.map(0, Point::new(210.0, 34.0)).y - H).abs() < 0.05);
    assert!((m.map(1, Point::new(210.0, 74.0)).y - H - 40.0).abs() < 0.05);
    // Text longer than the arc is shrunk to fit it.
    let w = warp("textArchUp", &[], rect()).unwrap();
    let long = WarpLine { ink: Some(Rect::new(-500.0, 50.0, 900.0, 88.0)), extent: (-500.0, 900.0), ref_y: 80.0 };
    let m = w.mapper(&[long], inner);
    let (a, b) = (m.map(0, Point::new(-500.0, 80.0)), m.map(0, Point::new(900.0, 80.0)));
    assert!(a.x < 5.0 && b.x > W - 5.0 && (a.y - 100.0).abs() < 10.0 && (b.y - 100.0).abs() < 10.0, "{a:?} {b:?}");
}

#[test]
fn lines_share_bands_in_order() {
    assert_eq!((0..2).map(|i| band_of_line(i, 2, 2)).collect::<Vec<_>>(), [0, 1]);
    assert_eq!((0..3).map(|i| band_of_line(i, 3, 2)).collect::<Vec<_>>(), [0, 0, 1]);
    assert_eq!((0..5).map(|i| band_of_line(i, 5, 2)).collect::<Vec<_>>(), [0, 0, 0, 1, 1]);
    assert_eq!((0..5).map(|i| band_of_line(i, 5, 3)).collect::<Vec<_>>(), [0, 0, 1, 1, 2]);
    assert_eq!((0..4).map(|i| band_of_line(i, 4, 3)).collect::<Vec<_>>(), [0, 0, 1, 2]);
    assert_eq!((0..2).map(|i| band_of_line(i, 2, 3)).collect::<Vec<_>>(), [0, 1]);
    assert_eq!(band_of_line(0, 1, 3), 0);
    assert_eq!(band_of_line(7, 9, 1), 0);
    assert_eq!(band_of_line(0, 0, 3), 0);
}

#[test]
fn deflate_inflate_maps_each_band_separately() {
    let w = warp("textDeflateInflate", &[], rect()).unwrap();
    assert_eq!(w.bands.len(), 2);
    let l1 = WarpLine { ink: Some(Rect::new(100.0, 10.0, 300.0, 40.0)), extent: (100.0, 300.0), ref_y: 34.0 };
    let l2 = WarpLine { ink: Some(Rect::new(100.0, 50.0, 300.0, 80.0)), extent: (100.0, 300.0), ref_y: 74.0 };
    let m = w.mapper(&[l1, l2], rect());
    // Band 1 ends at 45 % height on the sides, band 2 starts at 55 %.
    assert!((m.map(0, Point::new(100.0, 40.0)).y / H - 0.45).abs() < 0.004);
    assert!((m.map(1, Point::new(100.0, 50.0)).y / H - 0.55).abs() < 0.004);
    assert!((m.map(1, Point::new(300.0, 80.0)).y / H - 1.0).abs() < 0.004);
    // A band with fewer lines keeps them at the size of the fuller band, centred in its band:
    // two lines (ink 10..60) in band 1, one line (ink 70..80) in band 2 of three.
    let a = WarpLine { ink: Some(Rect::new(100.0, 10.0, 300.0, 30.0)), extent: (100.0, 300.0), ref_y: 28.0 };
    let b = WarpLine { ink: Some(Rect::new(100.0, 40.0, 300.0, 60.0)), extent: (100.0, 300.0), ref_y: 58.0 };
    let c = WarpLine { ink: Some(Rect::new(100.0, 70.0, 300.0, 80.0)), extent: (100.0, 300.0), ref_y: 78.0 };
    let m = w.mapper(&[a, b, c], rect());
    let (top, bot) = (m.map(2, Point::new(200.0, 70.0)).y / H, m.map(2, Point::new(200.0, 80.0)).y / H);
    // Band 2 runs from 0.4 to 1 at the centre; the line takes the middle fifth of it.
    assert!((top - 0.64).abs() < 0.005 && (bot - 0.76).abs() < 0.005, "{top} {bot}");
    // A shorter first line keeps its share of the width.
    let short = WarpLine { ink: Some(Rect::new(150.0, 10.0, 250.0, 40.0)), extent: (150.0, 250.0), ref_y: 34.0 };
    let m = w.mapper(&[short, l2], rect());
    let p = m.map(0, Point::new(150.0, 10.0));
    assert!((p.x / W - 0.25).abs() < 0.01, "{p:?}");
}

#[test]
fn guides_extrapolate_and_wrap() {
    let mut p = BezPath::new();
    p.move_to((0.0, 0.0));
    p.line_to((10.0, 0.0));
    let g = Guide::from_path(&p, false);
    assert!((g.length() - 10.0).abs() < 1e-9);
    assert!((g.at(15.0).0.x - 15.0).abs() < 1e-9);
    assert!((g.at(-5.0).0.x + 5.0).abs() < 1e-9);
    assert!(g.at(f64::NAN).0.x.is_finite());
    let w = warp("textCircle", &[], rect()).unwrap();
    let Some(Band::Follow { path, .. }) = w.bands.first() else { panic!() };
    assert!(path.is_closed());
    let l = path.length();
    let (a, b) = (path.at(5.0).0, path.at(l + 5.0).0);
    assert!((a - b).hypot() < 1e-6);
    let empty = Guide::default();
    assert_eq!(empty.length(), 0.0);
    assert!(empty.at(3.0).0.x.is_finite());
}

#[test]
fn hostile_adjusts_and_boxes_stay_finite() {
    let adjs: &[&[f64]] =
        &[&[], &[f64::NAN, f64::INFINITY], &[f64::NEG_INFINITY, f64::NAN], &[1e300, -1e300], &[-1e9, 1e9], &[0.0, 0.0], &[21_599_999.0, 100_000.0]];
    let rects = [
        Rect::new(0.0, 0.0, 0.0, 0.0),
        Rect::new(10.0, 10.0, 10.0, 200.0),
        Rect::new(0.0, 0.0, 300.0, 0.0),
        Rect::new(100.0, 100.0, -50.0, -20.0),
        Rect::new(0.0, 0.0, 1e7, 1e7),
    ];
    let lines = [
        WarpLine { ink: Some(Rect::new(0.0, 0.0, 0.0, 0.0)), extent: (0.0, 0.0), ref_y: 0.0 },
        WarpLine { ink: None, extent: (5.0, 5.0), ref_y: 3.0 },
        WarpLine { ink: Some(Rect::new(-1e6, -5.0, 1e6, 5.0)), extent: (-1e6, 1e6), ref_y: 4.0 },
    ];
    for name in CATALOG.iter().map(|p| p.id) {
        for adj in adjs {
            for r in rects {
                let Some(w) = warp(name, adj, r) else { panic!("{name} {adj:?} {r:?}") };
                for b in &w.bands {
                    assert!(b.length().is_finite());
                }
                let m = w.mapper(&lines, r);
                assert!(m.zoom().is_finite() && m.zoom() >= 1.0);
                let probes = [
                    (0, Point::ZERO),
                    (1, Point::new(5.0, 3.0)),
                    (2, Point::new(1e6, -5.0)),
                    (2, Point::new(f64::NAN, 1.0)),
                    (9, Point::new(1.0, 1.0)),
                ];
                for (i, p) in probes {
                    let q = m.map(i, p);
                    assert!(q.x.is_finite() || !p.x.is_finite(), "{name} {adj:?} {r:?}: {q:?}");
                    assert!(q.y.is_finite(), "{name} {adj:?} {r:?}: {q:?}");
                }
            }
        }
    }
    assert!(warp("textPlain", &[], Rect::new(f64::NAN, 0.0, 1.0, 1.0)).is_none());
}

#[test]
fn every_preset_has_one_handle_per_adjust() {
    let r = rect();
    for name in CATALOG.iter().map(|p| p.id) {
        let hs = handles(name, &[], r);
        assert_eq!(hs.len(), adjust_count(name), "{name}");
        let defaults = adjust_defaults(name);
        for h in &hs {
            assert!(h.pos.x.is_finite() && h.pos.y.is_finite(), "{name}");
            let back = h.value_at(h.pos);
            let want = defaults[h.index];
            assert!((back - want).abs() <= 1.0, "{name} handle {}: {back} vs {want}", h.index);
        }
    }
    assert!(handles("textNoShape", &[], r).is_empty());
    assert!(handles("textArchUp", &[], Rect::new(0.0, 0.0, f64::NAN, 10.0)).is_empty());
    assert!(handles("textArchUp", &[], Rect::new(0.0, 0.0, 0.0, 0.0)).is_empty());
    // A hostile adjust still places a finite handle on the clamped default.
    let hs = handles("textInflate", &[f64::NAN], r);
    assert_eq!(hs.len(), 1);
    assert!(hs[0].pos.x.is_finite() && hs[0].pos.y.is_finite());
}

#[test]
fn arch_and_inflate_handles_follow_the_pointer() {
    let r = rect();
    let arch = &handles("textArchUp", &[], r)[0];
    let deg = arch.value_at(Point::new(r.center().x, r.y0)) / 60_000.0;
    assert!((deg - 270.0).abs() < 0.5, "{deg}");
    let down = &handles("textArchDown", &[], r)[0];
    // The arch-down handle starts on the left; a pointer at the bottom is 90°, which is adjust 90
    // (guide angle 180 − adj).
    let deg = down.value_at(Point::new(r.center().x, r.y1)) / 60_000.0;
    assert!((deg - 90.0).abs() < 0.5, "{deg}");
    let inf = &handles("textInflate", &[], r)[0];
    let more = inf.value_at(Point::new(inf.pos.x, r.y0 - 80.0));
    assert!(more > adjust_defaults("textInflate")[0], "{more}");
    assert!((more - 20_000.0).abs() < 1.0, "{more}");
}
