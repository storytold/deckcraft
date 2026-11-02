//! Row/column packing after child constraints have resolved. Every fitting trial
//! recomputes end-of-canvas breaks; a fixed grid cannot preserve these semantics.
use super::{Instance, MAX_WORK, geometry::BoxStyle};
use deckcraft_geom::Xfrm;

fn charge(work: &mut usize, amount: usize) -> Result<(), &'static str> {
    *work = work.saturating_add(amount);
    if *work > MAX_WORK { Err("diagram snake budget exceeded") } else { Ok(()) }
}

struct Line {
    range: std::ops::Range<usize>,
    length: f64,
    thickness: f64,
}

struct Packing {
    boxes: Vec<Xfrm>,
    width: f64,
    height: f64,
}

struct Policy {
    column: bool,
    count: Option<usize>,
    reverse: bool,
    center: bool,
    gap: f64,
}

/// Same-direction rows carry their centering adjustment through the following
/// line before it reaches the placement cursor. A newly wider line can absorb
/// some of that outstanding adjustment. Keep the two pending boundaries apart:
/// replacing the earlier one loses the partial adjustment on successive growth.
struct Centering {
    extent: f64,
    offset: f64,
    pending: f64,
    next: f64,
}

impl Centering {
    fn origin(&mut self, length: f64) -> f64 {
        self.offset += self.pending;
        self.extent += 2. * self.pending;
        self.pending = self.next;
        self.next = 0.;
        if length > self.extent {
            let expansion = length - self.extent;
            self.pending = (self.pending - expansion / 2.).max(0.);
            self.next = expansion / 4.;
            self.extent = length;
        }
        self.offset - length / 2.
    }
}

fn exceeds(value: f64, limit: f64) -> bool {
    value - limit > 16. * f64::EPSILON * value.abs().max(limit.abs()).max(1.)
}

impl Policy {
    fn new(node: &Instance<'_>, style: BoxStyle, count: usize) -> Self {
        let count = match node.param("bkpt", "endCnv").as_str() {
            "fixed" => Some((node.number("bkPtFixedVal", 1.).max(1.) as usize).min(count)),
            "bal" => Some((count as f64).sqrt().ceil() as usize),
            _ => None,
        };
        Self {
            column: node.param("flowDir", "row") == "col",
            count,
            reverse: node.param("contDir", "sameDir") == "revDir",
            center: node.param("off", "off") == "ctr",
            gap: style.get("sp").unwrap_or(0.).max(0.),
        }
    }

    fn layout(&self, children: &[BoxStyle], available: f64, scale: f64, work: &mut usize) -> Result<Packing, &'static str> {
        charge(work, children.len().saturating_mul(3))?;
        let mut lines = Vec::<Line>::new();
        let mut line = Line { range: 0..0, length: 0., thickness: 0. };
        for (i, child) in children.iter().enumerate() {
            let (length, thickness) = if self.column { (child.rect.h, child.rect.w) } else { (child.rect.w, child.rect.h) };
            let length = length * scale;
            let thickness = thickness * scale;
            let full = self.count.map_or_else(|| exceeds(line.length + length, available), |count| line.range.len() >= count);
            if !line.range.is_empty() && full {
                lines.push(line);
                line = Line { range: i..i, length: 0., thickness: 0. };
            }
            line.range.end = i + 1;
            line.length += length;
            line.thickness = line.thickness.max(thickness);
        }
        if !line.range.is_empty() {
            lines.push(line);
        }
        let longest = lines.iter().map(|line| line.length).fold(0., f64::max);
        let mut centering = Centering { extent: lines.first().map_or(0., |line| line.length), offset: 0., pending: 0., next: 0. };
        let mut boxes = vec![Xfrm::new(0., 0., 0., 0.); children.len()];
        let (mut cursor, mut cross, mut low, mut high) = (0., 0., 0_f64, 0_f64);
        for (i, line) in lines.iter().enumerate() {
            let reversed = self.reverse && i % 2 == 1;
            if !self.reverse {
                cursor = 0.;
            }
            let origin = if self.center && !self.reverse {
                centering.origin(line.length)
            } else if self.center {
                (longest - line.length) / 2.
            } else if reversed {
                cursor - line.length
            } else {
                cursor
            };
            let mut pos = if reversed { origin + line.length } else { origin };
            low = low.min(origin);
            high = high.max(origin + line.length);
            for (child, rect) in
                children.get(line.range.clone()).unwrap_or_default().iter().zip(boxes.get_mut(line.range.clone()).unwrap_or_default())
            {
                let (length, thickness) = if self.column { (child.rect.h, child.rect.w) } else { (child.rect.w, child.rect.h) };
                let (length, thickness) = (length * scale, thickness * scale);
                if reversed {
                    pos -= length;
                }
                let perpendicular = cross + (line.thickness - thickness) / 2.;
                *rect = if self.column { Xfrm::new(perpendicular, pos, thickness, length) } else { Xfrm::new(pos, perpendicular, length, thickness) };
                if !reversed {
                    pos += length;
                }
            }
            cursor = pos;
            cross += line.thickness + self.gap * scale;
        }
        if !lines.is_empty() {
            cross -= self.gap * scale;
        }
        for rect in &mut boxes {
            if self.column { rect.y -= low } else { rect.x -= low }
        }
        let (width, height) = if self.column { (cross, high - low) } else { (high - low, cross) };
        Ok(Packing { boxes, width, height })
    }
}

pub(super) fn pack(node: &Instance<'_>, style: BoxStyle, children: &mut [BoxStyle], refit: bool, work: &mut usize) -> Result<(), &'static str> {
    if children.is_empty() {
        return Ok(());
    }
    let policy = Policy::new(node, style, children.len());
    let frame = style.rect;
    let available = if policy.column { frame.h } else { frame.w };
    let fits = |packing: &Packing| !exceeds(packing.width, frame.w) && !exceeds(packing.height, frame.h);
    let mut packed = policy.layout(children, available, 1., work)?;
    if refit || !fits(&packed) {
        let (mut low, mut high) = (0., 1.);
        // Normalize the bracket before the observed ten fitting trials. Each
        // attempt consumes the shared work budget, including extreme extents.
        while !fits(&policy.layout(children, available, high * 0.5, work)?) {
            high *= 0.5;
        }
        packed = policy.layout(children, available, 0., work)?;
        for _ in 0..10 {
            let mid = (low + high) / 2.;
            let candidate = policy.layout(children, available, mid, work)?;
            if fits(&candidate) {
                low = mid;
                packed = candidate;
            } else {
                high = mid;
            }
        }
    }
    place(node, frame, children, packed);
    Ok(())
}

/// Repack children after inner linear flows restore their intrinsic cross
/// dimensions. Those dimensions may exceed the canvas; another uniform fitting
/// pass would discard the linear algorithm's intrinsic dimensions again.
pub(super) fn repack(node: &Instance<'_>, style: BoxStyle, children: &mut [BoxStyle], work: &mut usize) -> Result<(), &'static str> {
    let policy = Policy::new(node, style, children.len());
    let frame = style.rect;
    let available = if policy.column { frame.h } else { frame.w };
    let packed = policy.layout(children, available, 1., work)?;
    place(node, frame, children, packed);
    Ok(())
}

fn place(node: &Instance<'_>, frame: Xfrm, children: &mut [BoxStyle], packed: Packing) {
    let grow = node.param("grDir", "tL");
    for (child, mut rect) in children.iter_mut().zip(packed.boxes) {
        if grow.ends_with('R') {
            rect.x = packed.width - rect.x - rect.w;
        }
        if grow.starts_with('b') {
            rect.y = packed.height - rect.y - rect.h;
        }
        rect.x += frame.x + (frame.w - packed.width) / 2.;
        rect.y += frame.y + (frame.h - packed.height) / 2.;
        child.rect = Xfrm { rot: child.rect.rot, flip_h: child.rect.flip_h, flip_v: child.rect.flip_v, ..rect };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packed(sizes: &[(f64, f64)], frame: (f64, f64), params: &[(&str, &str)], gap: f64) -> Vec<Xfrm> {
        let xml = crate::xml::parse(b"<layoutNode/>").unwrap();
        let mut node = Instance::new(&xml.root);
        node.alg = "snake".into();
        node.params = params.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect();
        let mm = 72. / 25.4;
        let mut style = BoxStyle::new(Xfrm::new(13. * mm, 17. * mm, frame.0 * mm, frame.1 * mm));
        style.set("sp", gap * mm, "equ");
        let mut children: Vec<_> = sizes.iter().map(|&(w, h)| BoxStyle::new(Xfrm::new(0., 0., w * mm, h * mm))).collect();
        pack(&node, style, &mut children, false, &mut 0).unwrap();
        children.into_iter().map(|c| Xfrm::new(c.rect.x / mm - 13., c.rect.y / mm - 17., c.rect.w / mm, c.rect.h / mm)).collect()
    }

    fn check(actual: &[Xfrm], expected: &[[f64; 4]]) {
        assert_eq!(actual.len(), expected.len());
        for (a, e) in actual.iter().zip(expected) {
            for (a, e) in [a.x, a.y, a.w, a.h].into_iter().zip(e) {
                assert!((a - e).abs() < 1e-6, "{actual:?} != {expected:?}");
            }
        }
    }

    #[test]
    fn breakpoints_follow_constrained_lengths_and_balanced_counts() {
        let sizes = [(20., 10.); 5];
        check(
            &packed(&sizes, (120., 80.), &[], 0.),
            &[[10., 35., 20., 10.], [30., 35., 20., 10.], [50., 35., 20., 10.], [70., 35., 20., 10.], [90., 35., 20., 10.]],
        );
        check(
            &packed(&sizes, (45., 55.), &[], 0.),
            &[[2.5, 12.5, 20., 10.], [22.5, 12.5, 20., 10.], [2.5, 22.5, 20., 10.], [22.5, 22.5, 20., 10.], [2.5, 32.5, 20., 10.]],
        );
        for frame in [(120., 80.), (200., 40.)] {
            let p = packed(&sizes, frame, &[("bkpt", "bal")], 0.);
            assert!((p[3].x - p[0].x).abs() < 1e-7);
            assert!((p[3].y - p[0].y - 10.).abs() < 1e-7);
            assert!((p[2].x - p[0].x - 40.).abs() < 1e-7);
        }
        check(
            &packed(&sizes, (120., 80.), &[("bkpt", "fixed"), ("bkPtFixedVal", "2")], 0.),
            &[[40., 25., 20., 10.], [60., 25., 20., 10.], [40., 35., 20., 10.], [60., 35., 20., 10.], [40., 45., 20., 10.]],
        );
    }

    #[test]
    fn fitting_rebreaks_rows_and_preserves_unequal_child_proportions() {
        check(
            &packed(&[(80., 60.); 5], (120., 80.), &[], 0.),
            &[[0., 10., 40., 30.], [40., 10., 40., 30.], [80., 10., 40., 30.], [0., 40., 40., 30.], [40., 40., 40., 30.]],
        );
        let sizes = [(10., 30.), (30., 10.), (20., 20.), (25., 15.), (12., 18.)];
        check(
            &packed(&sizes, (120., 80.), &[("bkpt", "fixed"), ("bkPtFixedVal", "2")], 0.),
            &[[37.5, 6., 10., 30.], [47.5, 16., 30., 10.], [37.5, 36., 20., 20.], [57.5, 38.5, 25., 15.], [37.5, 56., 12., 18.]],
        );
        let p = packed(&sizes, (45., 55.), &[], 0.);
        for (p, size) in p.iter().zip(sizes) {
            assert!((p.w / size.0 - 828. / 1024.).abs() < 1e-7);
            assert!((p.h / size.1 - 828. / 1024.).abs() < 1e-7);
        }
    }

    #[test]
    fn spacing_is_between_lines_and_growth_mirrors_the_packed_group() {
        let sizes = [(20., 10.); 5];
        check(
            &packed(&sizes, (45., 55.), &[], 5.),
            &[[2.5, 7.5, 20., 10.], [22.5, 7.5, 20., 10.], [2.5, 22.5, 20., 10.], [22.5, 22.5, 20., 10.], [2.5, 37.5, 20., 10.]],
        );
        let params = [("bkpt", "fixed"), ("bkPtFixedVal", "2"), ("contDir", "revDir"), ("grDir", "bR")];
        check(
            &packed(&sizes, (120., 80.), &params, 0.),
            &[[60., 45., 20., 10.], [40., 45., 20., 10.], [40., 35., 20., 10.], [60., 35., 20., 10.], [60., 25., 20., 10.]],
        );
        check(
            &packed(&sizes, (120., 80.), &[("bkpt", "fixed"), ("bkPtFixedVal", "3"), ("off", "ctr")], 0.),
            &[[30., 30., 20., 10.], [50., 30., 20., 10.], [70., 30., 20., 10.], [40., 40., 20., 10.], [60., 40., 20., 10.]],
        );
    }

    #[test]
    fn centered_rows_propagate_pending_growth_and_center_the_occupied_bounds() {
        let sizes = [(20., 10.), (40., 10.), (20., 10.), (20., 10.), (60., 10.), (20., 10.), (20., 10.), (20., 10.)];
        let params = [("bkpt", "fixed"), ("bkPtFixedVal", "1"), ("off", "ctr")];
        let actual = packed(&sizes, (300., 160.), &params, 0.);
        for (rect, center) in actual.iter().zip([-5., -5., -5., 0., 0., 0., 2.5, 2.5]) {
            assert!((rect.x + rect.w / 2. - 150. - center).abs() < 1e-7);
        }
        // Successive wider rows absorb only part of the earlier pending shift.
        let sizes = [(45.965, 10.), (93.878, 10.), (105.660, 10.), (83.529, 10.), (49.940, 10.), (79.917, 10.), (77.901, 10.)];
        let actual = packed(&sizes, (300., 160.), &params, 0.);
        for (rect, center) in actual.iter().zip([0., 0., 0., 6.08725, 9.03275, 9.03275, 9.03275]) {
            assert!((rect.x + rect.w / 2. - 150. - center).abs() < 1e-7);
        }
        let sizes: Vec<_> = sizes.iter().map(|&(w, h)| (h, w)).collect();
        let params = [("bkpt", "fixed"), ("bkPtFixedVal", "1"), ("off", "ctr"), ("flowDir", "col"), ("grDir", "bR")];
        let actual = packed(&sizes, (160., 300.), &params, 0.);
        for (rect, center) in actual.iter().zip([0., 0., 0., 6.08725, 9.03275, 9.03275, 9.03275]) {
            assert!((rect.y + rect.h / 2. - 150. + center).abs() < 1e-7);
        }
    }

    #[test]
    fn empty_zero_and_extreme_frames_obey_the_shared_work_budget() {
        let xml = crate::xml::parse(b"<layoutNode/>").unwrap();
        let node = Instance::new(&xml.root);
        let style = BoxStyle::new(Xfrm::new(0., 0., 0., 0.));
        pack(&node, style, &mut [], false, &mut 0).unwrap();
        let mut children = [BoxStyle::new(Xfrm::new(0., 0., 100., 100.))];
        pack(&node, style, &mut children, false, &mut 0).unwrap();
        assert!(children[0].rect.is_finite());
        let mut work = MAX_WORK;
        assert!(pack(&node, style, &mut children, false, &mut work).is_err());
    }
}
