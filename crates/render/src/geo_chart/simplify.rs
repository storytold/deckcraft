//! Screen-space boundary reduction. Geographic coordinates remain intact in the model.
mod heavy;
use kurbo::{BezPath, PathEl, Point};

const MIN_DISTANCE_SQUARED: f64 = 0.375 * 0.375;

/// The complexity budget belongs to one region, after the initial distance filter.
pub(super) fn reduce(path: &BezPath) -> BezPath {
    let path = basic(path);
    if vertex_count(&path) > 1000 { heavy::reduce(&path) } else { path }
}

fn vertex_count(path: &BezPath) -> usize {
    let (mut total, mut count) = (0usize, 0usize);
    let (mut first, mut last) = (None, None);
    let count_ring = |count: usize, first: Option<Point>, last: Option<Point>| {
        let count = count.saturating_sub(usize::from(count > 1 && first == last));
        // A collapsed island without an edge consumes no complexity budget. A
        // two-point ring still contributes its two endpoints even though its area is zero.
        if count >= 2 { count } else { 0 }
    };
    for element in path.elements() {
        match *element {
            PathEl::MoveTo(point) => {
                total = total.saturating_add(count_ring(count, first, last));
                count = 1;
                first = Some(point);
                last = Some(point);
            }
            PathEl::LineTo(point) => {
                count = count.saturating_add(1);
                last = Some(point);
            }
            PathEl::ClosePath => {
                total = total.saturating_add(count_ring(count, first, last));
                count = 0;
                first = None;
                last = None;
            }
            // Preserve future curved map input, as in the initial filter.
            PathEl::QuadTo(..) | PathEl::CurveTo(..) => return 0,
        }
    }
    total.saturating_add(count_ring(count, first, last))
}

/// The initial native map filter samples each ring by Euclidean distance in chart points.
/// A closing edge is unconditional, but the last authored vertex is sampled like every
/// other vertex. In particular, forcing that vertex back would manufacture tiny islands.
pub(super) fn basic(path: &BezPath) -> BezPath {
    // Map projection currently emits only closed polylines. Preserve any future curved
    // input rather than silently discarding its control points.
    if path.elements().iter().any(|e| matches!(e, PathEl::QuadTo(..) | PathEl::CurveTo(..))) {
        return path.clone();
    }
    let mut output = BezPath::new();
    let mut previous: Option<Point> = None;
    for element in path.elements() {
        match *element {
            PathEl::MoveTo(point) => {
                output.move_to(point);
                previous = Some(point);
            }
            PathEl::LineTo(point) => {
                if previous.is_none_or(|previous| previous.distance_squared(point) > MIN_DISTANCE_SQUARED) {
                    output.line_to(point);
                    previous = Some(point);
                }
            }
            PathEl::ClosePath => {
                output.close_path();
                previous = None;
            }
            _ => (),
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::{Affine, Shape as _};

    fn ring(points: &[(f64, f64)], explicit_close: bool) -> BezPath {
        let mut path = BezPath::new();
        path.move_to(points[0]);
        for &point in &points[1..] {
            path.line_to(point);
        }
        if explicit_close {
            path.line_to(points[0]);
        }
        path.close_path();
        path
    }

    #[test]
    fn distance_is_euclidean_and_measured_from_the_last_retained_vertex() {
        let path = ring(&[(0., 0.), (0.2, 0.), (0.4, 0.), (0.6, 0.), (0.8, 0.), (2., 2.), (0., 2.)], false);
        let reduced = basic(&path);
        assert_eq!(reduced.elements().len(), 6);
        assert_eq!(reduced.elements()[1], PathEl::LineTo(Point::new(0.4, 0.)));
        assert_eq!(reduced.elements()[2], PathEl::LineTo(Point::new(0.8, 0.)));
        let rotated = Affine::rotate(std::f64::consts::FRAC_PI_4) * path;
        assert_eq!(basic(&rotated).elements().len(), reduced.elements().len());
        // Both coordinate deltas are below the threshold, but their diagonal is above it.
        let diagonal = ring(&[(0., 0.), (0.3, 0.3), (2., 2.), (0., 2.)], false);
        assert_eq!(basic(&diagonal), diagonal);
    }

    #[test]
    fn closure_does_not_restore_a_rejected_final_vertex_or_tiny_island() {
        for explicit_close in [false, true] {
            let line = basic(&ring(&[(0., 0.), (0.2, 0.2), (0.4, 0.), (0.2, -0.2)], explicit_close));
            assert_eq!(line.bounding_box().height(), 0.);
            assert_eq!(line.area(), 0.);
            let island = basic(&ring(&[(0., 0.), (0.1, 0.), (0.1, 0.1), (0., 0.1)], explicit_close));
            assert_eq!(island.area(), 0.);
            assert!(matches!(island.elements().last(), Some(PathEl::ClosePath)));
        }
    }

    #[test]
    fn rings_reset_sampling_and_keep_holes_and_chart_size_dependence() {
        let outer = ring(&[(0., 0.), (6., 0.), (6., 6.), (0., 6.)], false);
        let inner = ring(&[(2., 2.), (2., 4.), (4., 4.), (4., 2.)], false);
        let mut path = outer;
        path.extend(inner.elements().iter().copied());
        assert_eq!(basic(&path), path);
        assert_eq!(basic(&path).winding(Point::new(3., 3.)), 0);
        let island = ring(&[(0., 0.), (0.4, 0.), (0.4, 0.4), (0., 0.4)], false);
        assert!(basic(&island).area() > 0.);
        assert_eq!(basic(&(Affine::scale(0.5) * island)).area(), 0.);
    }

    #[test]
    fn complexity_counts_surviving_edges_per_region_and_excludes_closing_duplicates() {
        let mut region = ring(&[(-8., -8.), (0., 0.), (1.5, 0.), (3., 0.1), (4.5, 0.2), (4.5, 8.), (-8., 8.), (-9., 0.)], true);
        let square = ring(&[(20., 20.), (24., 20.), (24., 24.), (20., 24.)], true);
        for _ in 0..248 {
            region.extend(square.elements().iter().copied());
        }
        assert_eq!(vertex_count(&basic(&region)), 1000);
        assert_eq!(reduce(&region), basic(&region));
        let mut over = region.elements().to_vec();
        over.insert(1, PathEl::LineTo(Point::new(-9., -4.)));
        let over = BezPath::from_vec(over);
        assert_eq!(vertex_count(&basic(&over)), 1001);
        assert!(reduce(&over).elements().len() < basic(&over).elements().len());

        let island = ring(&[(30., 30.), (30.1, 30.), (30.1, 30.1), (30., 30.1)], true);
        region.extend(island.elements().iter().copied());
        assert_eq!(vertex_count(&basic(&region)), 1000);
        assert_eq!(reduce(&region), basic(&region));

        let line = ring(&[(40., 40.), (40.2, 40.2), (40.4, 40.), (40.2, 39.8)], true);
        region.extend(line.elements().iter().copied());
        assert_eq!(vertex_count(&basic(&region)), 1002);
        assert!(reduce(&region).elements().len() < basic(&region).elements().len());
        // A separately rendered region cannot consume this one's budget.
        assert_eq!(reduce(&square), basic(&square));
    }

    #[test]
    fn heavy_filter_protects_the_first_edge_and_uses_axis_distances() {
        let path = ring(&[(0., 0.), (0.4, 0.), (1.2, 0.8), (1.2, 1.2), (4., 4.), (0., 4.)], false);
        let reduced = heavy::reduce(&basic(&path));
        assert_eq!(reduced.elements()[1], PathEl::LineTo(Point::new(0.4, 0.)));
        assert_eq!(reduced.elements()[2], PathEl::LineTo(Point::new(1.2, 1.2)));
        assert!(!reduced.elements().contains(&PathEl::LineTo(Point::new(1.2, 0.8))));
    }

    #[test]
    fn heavy_filter_requires_two_small_areas_with_a_shared_anchor() {
        let path = ring(&[(-8., -8.), (0., 0.), (1.5, 0.), (3., 0.1), (4.5, 0.2), (4.5, 8.), (-8., 8.)], false);
        let expected = ring(&[(-8., -8.), (0., 0.), (4.5, 0.2), (4.5, 8.), (-8., 8.)], false);
        assert_eq!(heavy::reduce(&path), expected);
        // A failed second area check flushes pending vertices. It cannot slide
        // backward and reconsider the same suffix against another anchor.
        let context = ring(&[(-8., -8.), (-1.5, 0.), (0., 0.), (1.5, 0.), (3., 0.1), (4.5, 0.2), (4.5, 8.), (-8., 8.)], false);
        assert_eq!(heavy::reduce(&context), context);
    }

    #[test]
    fn heavy_filter_retains_cusps_without_reversing_again_when_crossing_the_anchor() {
        let path = ring(&[(-7., -7.), (0., 0.), (2., 0.), (-1.4, 0.04), (-3.6, 0.028), (-6., 0.016), (-6., 8.), (-8., 8.)], false);
        let expected = ring(&[(-7., -7.), (0., 0.), (2., 0.), (-6., 0.016), (-6., 8.), (-8., 8.)], false);
        assert_eq!(heavy::reduce(&path), expected);
    }

    #[test]
    fn heavy_filter_keeps_implicit_closure_and_resets_between_rings() {
        let path = ring(&[(0., 0.), (10., 0.), (10., 10.), (0., 10.), (0.8, 0.8)], true);
        let expected = ring(&[(0., 0.), (10., 0.), (10., 10.), (0., 10.), (0.8, 0.8)], false);
        assert_eq!(heavy::reduce(&path), expected);
        let mut double = path.clone();
        double.extend(path.elements().iter().copied());
        let mut twice = expected.clone();
        twice.extend(expected.elements().iter().copied());
        assert_eq!(heavy::reduce(&double), twice);

        let mut curve = path;
        curve.quad_to((0., 1.), (1., 1.));
        assert_eq!(reduce(&curve), curve);
    }
}
