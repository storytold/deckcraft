//! Screen-space boundary reduction. Geographic coordinates remain intact in the model.
use kurbo::{BezPath, PathEl, Point};

const MIN_DISTANCE_SQUARED: f64 = 0.375 * 0.375;

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
}
