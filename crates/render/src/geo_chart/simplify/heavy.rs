//! Additional screen-space sampling for regions exceeding the basic vertex budget.
use kurbo::{BezPath, PathEl, Point, Vec2};

#[derive(Clone, Copy, Default)]
enum Phase {
    #[default]
    Start,
    First(Point),
    Second(Point),
    Continuing(Vec2),
}

#[derive(Default)]
struct Ring {
    points: Vec<Point>,
    anchor: Point,
    phase: Phase,
}

impl Ring {
    fn sample(&mut self, point: Point) {
        // The first edge survives even when it is shorter than the heavy sample distance.
        if self.points.len() < 2 {
            self.points.push(point);
            self.anchor = point;
            return;
        }
        let Some(&last) = self.points.last() else { return };
        let vector = point - last;
        if vector.x.abs().max(vector.y.abs()) <= 1. {
            return;
        }
        let incoming = last - self.anchor;
        let straight = incoming.cross(vector).abs() < 0.24;
        let forward = incoming.dot(vector) > 0.;
        match self.phase {
            Phase::Start => self.phase = Phase::First(point),
            Phase::First(pending) => {
                if straight {
                    self.phase = if forward { Phase::Second(point) } else { Phase::Continuing(vector) };
                } else {
                    self.anchor = pending;
                    self.phase = Phase::First(point);
                }
            }
            Phase::Second(pending) => {
                if straight {
                    // Two successive small areas with the same anchor establish a
                    // reducible run. Keep the cusp when its direction reverses.
                    self.points.pop();
                    self.points.pop();
                    if !forward {
                        self.points.push(pending);
                    }
                    self.phase = Phase::Continuing(vector);
                } else {
                    self.anchor = pending;
                    self.phase = Phase::First(point);
                }
            }
            Phase::Continuing(direction) => {
                if straight {
                    // Direction comes from the most recent sampled segment. Testing
                    // against the anchor would invent a cusp when a run crosses it.
                    if direction.dot(vector) > 0. {
                        self.points.pop();
                    }
                    self.phase = Phase::Continuing(vector);
                } else {
                    self.anchor = last;
                    self.phase = Phase::First(point);
                }
            }
        }
        self.points.push(point);
    }

    fn emit(&mut self, output: &mut BezPath, closed: bool) {
        if let Some((&first, rest)) = self.points.split_first() {
            output.move_to(first);
            for &point in rest {
                output.line_to(point);
            }
            if closed {
                output.close_path();
            }
        }
        self.points.clear();
        self.phase = Phase::Start;
    }
}

pub(super) fn reduce(path: &BezPath) -> BezPath {
    let mut output = BezPath::new();
    let mut ring = Ring::default();
    // Both buffers are bounded by the projected path's existing point budget.
    for element in path.elements() {
        match *element {
            PathEl::MoveTo(point) => {
                ring.emit(&mut output, false);
                ring.sample(point);
            }
            PathEl::LineTo(point) => ring.sample(point),
            // Only authored vertices participate in the distance/area tests. The
            // implicit closing edge must not restore a vertex either filter rejected.
            PathEl::ClosePath => ring.emit(&mut output, true),
            PathEl::QuadTo(..) | PathEl::CurveTo(..) => return path.clone(),
        }
    }
    ring.emit(&mut output, false);
    output
}
