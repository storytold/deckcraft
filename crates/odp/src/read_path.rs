//! ODF 1.3 §19.145 enhanced paths. Arc commands are converted to editable cubic paths.
use super::{model_path, number};
use crate::{OdpError, Result};
use deckcraft_geom::{Affine, BezPath, Point, preset::FillMode};
use deckcraft_model::CustomPath;

struct Tokens<'a> {
    remaining: &'a str,
    operations: usize,
}

impl Tokens<'_> {
    fn space(&mut self) {
        self.remaining = self.remaining.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == ',');
    }
    fn tick(&mut self) -> Result<()> {
        self.operations += 1;
        if self.operations > 50_000 {
            return Err(OdpError("enhanced path operation limit exceeded".into()));
        }
        Ok(())
    }
    fn value(&mut self) -> Result<f64> {
        self.space();
        self.tick()?;
        let bytes = self.remaining.as_bytes();
        let mut end = 0;
        if bytes.first().is_some_and(|b| matches!(b, b'+' | b'-')) {
            end += 1;
        }
        let mut digits = 0;
        while bytes.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
            digits += 1;
        }
        if bytes.get(end) == Some(&b'.') {
            end += 1;
            while bytes.get(end).is_some_and(u8::is_ascii_digit) {
                end += 1;
                digits += 1;
            }
        }
        if digits == 0 {
            return Err(OdpError("missing enhanced path coordinate".into()));
        }
        if bytes.get(end).is_some_and(|b| matches!(b, b'e' | b'E')) {
            end += 1;
            if bytes.get(end).is_some_and(|b| matches!(b, b'+' | b'-')) {
                end += 1;
            }
            let first = end;
            while bytes.get(end).is_some_and(u8::is_ascii_digit) {
                end += 1;
            }
            if end == first {
                return Err(OdpError("invalid enhanced path exponent".into()));
            }
        }
        let value = self.remaining.get(..end).and_then(number).ok_or_else(|| OdpError("invalid enhanced path coordinate".into()))?;
        self.remaining = self.remaining.get(end..).ok_or_else(|| OdpError("invalid enhanced path token".into()))?;
        Ok(value)
    }
    fn point(&mut self) -> Result<Point> {
        Ok(Point::new(self.value()?, self.value()?))
    }
}

struct Path {
    path: BezPath,
    current: Option<Point>,
    start: Option<Point>,
    fill: bool,
    stroke: bool,
}

impl Default for Path {
    fn default() -> Self {
        Self { path: BezPath::new(), current: None, start: None, fill: true, stroke: true }
    }
}

impl Path {
    fn move_to(&mut self, point: Point) {
        self.path.move_to(point);
        self.current = Some(point);
        self.start = Some(point);
    }
    fn line_to(&mut self, point: Point) {
        if self.current.is_some() {
            self.path.line_to(point);
            self.current = Some(point);
        } else {
            self.move_to(point);
        }
    }
    fn arc(&mut self, center: Point, rx: f64, ry: f64, start: f64, sweep: f64, connect: bool) -> Result<()> {
        if ![center.x, center.y, rx, ry, start, sweep].iter().all(|v| v.is_finite()) || rx <= 0.0 || ry <= 0.0 {
            return Err(OdpError("invalid enhanced ellipse".into()));
        }
        let pieces = (sweep.abs() / std::f64::consts::FRAC_PI_2).ceil().max(1.0);
        if pieces > 4096.0 {
            return Err(OdpError("enhanced arc exceeds segment limit".into()));
        }
        let pieces = pieces as usize;
        let point = |angle: f64| Point::new(center.x + rx * angle.cos(), center.y + ry * angle.sin());
        let first = point(start);
        if connect {
            self.line_to(first);
        } else {
            self.move_to(first);
        }
        let step = sweep / pieces as f64;
        for i in 0..pieces {
            let a = start + i as f64 * step;
            let b = a + step;
            let k = 4.0 / 3.0 * (step / 4.0).tan();
            let from = point(a);
            let to = point(b);
            let c1 = Point::new(from.x - k * rx * a.sin(), from.y + k * ry * a.cos());
            let c2 = Point::new(to.x + k * rx * b.sin(), to.y - k * ry * b.cos());
            self.path.curve_to(c1, c2, to);
            self.current = Some(to);
        }
        Ok(())
    }
}

fn sweep(start: f64, end: f64, clockwise: bool, full: bool) -> f64 {
    let tau = std::f64::consts::TAU;
    let delta = if clockwise { (end - start).rem_euclid(tau) } else { -(start - end).rem_euclid(tau) };
    if full && delta.abs() < 1e-12 { if clockwise { tau } else { -tau } } else { delta }
}

/// Parameters have already had formula/modifier references evaluated by the bounded interpreter.
pub(super) fn read(input: &str, bounds: [f64; 4]) -> Result<Vec<CustomPath>> {
    let [bx, by, width, height] = bounds;
    let mut tokens = Tokens { remaining: input, operations: 0 };
    let mut state = Path::default();
    let mut paths = Vec::new();
    let mut command = None;
    let mut quadrant_x = true;
    let finish = |state: &mut Path, paths: &mut Vec<CustomPath>| -> Result<()> {
        if !state.path.elements().is_empty() {
            if paths.len() >= 1024 {
                return Err(OdpError("too many enhanced subpaths".into()));
            }
            let path = Affine::translate((-bx, -by)) * &state.path;
            paths.push(CustomPath {
                w: width,
                h: height,
                d: model_path(&path)?,
                fill: if state.fill { FillMode::Norm } else { FillMode::None },
                stroke: state.stroke,
            });
        }
        *state = Path::default();
        Ok(())
    };
    loop {
        tokens.space();
        let Some(first) = tokens.remaining.chars().next() else { break };
        if first.is_ascii_alphabetic() {
            tokens.tick()?;
            tokens.remaining = tokens.remaining.get(first.len_utf8()..).ok_or_else(|| OdpError("invalid enhanced path command".into()))?;
            let previous = command;
            command = Some(first);
            match first {
                'N' => {
                    finish(&mut state, &mut paths)?;
                    command = None;
                    continue;
                }
                'F' => {
                    state.fill = false;
                    command = None;
                    continue;
                }
                'S' => {
                    state.stroke = false;
                    command = None;
                    continue;
                }
                'Z' => {
                    if state.current.is_some() {
                        state.path.close_path();
                        state.current = state.start;
                    }
                    command = None;
                    continue;
                }
                'X' | 'Y' => {
                    if previous != Some(first) {
                        quadrant_x = first == 'X';
                    }
                }
                'M' | 'L' | 'C' | 'Q' | 'A' | 'B' | 'V' | 'W' | 'T' | 'U' => {}
                _ => return Err(OdpError(format!("unsupported enhanced path command '{first}'"))),
            }
        }
        let cmd = command.ok_or_else(|| OdpError("enhanced coordinates have no command".into()))?;
        match cmd {
            'M' => {
                state.move_to(tokens.point()?);
                command = Some('L');
            }
            'L' => state.line_to(tokens.point()?),
            'C' => {
                let a = tokens.point()?;
                let b = tokens.point()?;
                let end = tokens.point()?;
                if state.current.is_none() {
                    return Err(OdpError("curve has no starting point".into()));
                }
                state.path.curve_to(a, b, end);
                state.current = Some(end);
            }
            'Q' => {
                let a = tokens.point()?;
                let end = tokens.point()?;
                if state.current.is_none() {
                    return Err(OdpError("curve has no starting point".into()));
                }
                state.path.quad_to(a, end);
                state.current = Some(end);
            }
            'A' | 'B' | 'V' | 'W' => {
                let first = tokens.point()?;
                let second = tokens.point()?;
                let from = tokens.point()?;
                let to = tokens.point()?;
                let center = Point::new((first.x + second.x) / 2.0, (first.y + second.y) / 2.0);
                let rx = (second.x - first.x).abs() / 2.0;
                let ry = (second.y - first.y).abs() / 2.0;
                if rx == 0.0 || ry == 0.0 {
                    return Err(OdpError("degenerate enhanced ellipse".into()));
                }
                let angle = |p: Point| ((p.y - center.y) / ry).atan2((p.x - center.x) / rx);
                let start = angle(from);
                let end = angle(to);
                state.arc(center, rx, ry, start, sweep(start, end, matches!(cmd, 'V' | 'W'), true), matches!(cmd, 'A' | 'W'))?;
            }
            'T' | 'U' => {
                let center = tokens.point()?;
                let rx = tokens.value()?;
                let ry = tokens.value()?;
                let a = tokens.value()?.to_radians();
                let b = tokens.value()?.to_radians();
                state.arc(center, rx, ry, a, sweep(a, b, true, (b - a).abs() >= std::f64::consts::TAU - 1e-12), cmd == 'T')?;
            }
            'X' | 'Y' => {
                let end = tokens.point()?;
                let from = state.current.ok_or_else(|| OdpError("quadrant has no starting point".into()))?;
                let dx = end.x - from.x;
                let dy = end.y - from.y;
                let k = 4.0 * (2.0f64.sqrt() - 1.0) / 3.0;
                let (a, b) = if quadrant_x {
                    (Point::new(from.x + k * dx, from.y), Point::new(end.x, end.y - k * dy))
                } else {
                    (Point::new(from.x, from.y + k * dy), Point::new(end.x - k * dx, end.y))
                };
                state.path.curve_to(a, b, end);
                state.current = Some(end);
                quadrant_x = !quadrant_x;
            }
            _ => return Err(OdpError("invalid enhanced path state".into())),
        }
    }
    finish(&mut state, &mut paths)?;
    Ok(paths)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]
    use super::*;
    use deckcraft_geom::{KurboShape as _, PathEl};

    fn end(path: &BezPath) -> Point {
        match path.elements().last().unwrap() {
            PathEl::MoveTo(p) | PathEl::LineTo(p) | PathEl::QuadTo(_, p) | PathEl::CurveTo(_, _, p) => *p,
            PathEl::ClosePath => panic!("expected an open path"),
        }
    }

    #[test]
    fn full_angle_ellipse_reaches_the_renderer_with_its_expected_area() {
        let paths = read("U 100 50 100 50 0 360 Z N", [0.0, 0.0, 200.0, 100.0]).unwrap();
        let shape = deckcraft_model::Shape { geom: deckcraft_model::Geom::Custom { paths }, ..Default::default() };
        let geometry = deckcraft_render::shape_geometry(&shape, 200.0, 100.0);
        let path = &geometry.paths[0].path;
        let bounds = path.bounding_box();
        assert!((bounds.width() - 200.0).abs() < 1e-6 && (bounds.height() - 100.0).abs() < 1e-6);
        let expected = std::f64::consts::PI * 100.0 * 50.0;
        assert!((path.area().abs() - expected).abs() / expected < 0.0003);
    }

    #[test]
    fn radial_arcs_preserve_direction_and_connection_semantics() {
        for (cmd, clockwise, connected) in [('A', false, true), ('B', false, false), ('V', true, false), ('W', true, true)] {
            let paths = read(&format!("M 0 0 {cmd} 0 0 200 100 200 50 100 100 N"), [0.0, 0.0, 200.0, 100.0]).unwrap();
            let path = deckcraft_render::parse_path(&paths[0].d);
            assert_eq!(path.elements().get(1).is_some_and(|p| matches!(p, PathEl::LineTo(_))), connected);
            let end = end(&path);
            assert!((end.x - 100.0).abs() < 1e-6 && (end.y - 100.0).abs() < 1e-6);
            let PathEl::CurveTo(first, _, _) = path.elements().iter().find(|e| matches!(e, PathEl::CurveTo(..))).unwrap() else {
                panic!("expected a curve")
            };
            assert_eq!(first.y > 50.0, clockwise);
        }
    }

    #[test]
    fn elliptical_quadrants_alternate_their_initial_tangent() {
        let paths = read("M 0 0 X 50 50 100 0 N", [0.0, 0.0, 100.0, 50.0]).unwrap();
        let path = deckcraft_render::parse_path(&paths[0].d);
        let PathEl::CurveTo(a, _, _) = path.elements()[1] else { panic!("expected first quadrant") };
        let PathEl::CurveTo(b, _, _) = path.elements()[2] else { panic!("expected second quadrant") };
        assert_eq!(a.y, 0.0);
        assert_eq!(b.x, 50.0);
        assert_eq!(end(&path), Point::new(100.0, 0.0));
    }

    #[test]
    fn compact_coordinates_and_independent_subpath_flags_are_preserved() {
        let paths = read("M1e1,2e1L30,40 40,20ZFNM 0 0 L 1 1 S N", [10.0, 20.0, 100.0, 100.0]).unwrap();
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0].fill, FillMode::None);
        assert!(paths[0].stroke);
        assert_eq!(paths[1].fill, FillMode::Norm);
        assert!(!paths[1].stroke);
        assert!(paths[0].d.starts_with("M 0 0"));
    }

    #[test]
    fn malformed_and_degenerate_arcs_fail_without_unbounded_work() {
        for path in ["M 1", "C 1 2 3 4 5 6", "U 0 0 0 20 0 90", "X 10 20", "M 0 0 R 1 2", "M 1e+ 0"] {
            assert!(read(path, [0.0, 0.0, 100.0, 100.0]).is_err(), "{path}");
        }
    }
}
