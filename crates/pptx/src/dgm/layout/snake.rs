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
    physical_width: f64,
}

struct Packing {
    boxes: Vec<Xfrm>,
    width: f64,
    height: f64,
    complete: bool,
}

struct Restored<'a> {
    allocations: &'a [BoxStyle],
    cross_limit: f64,
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

#[derive(Clone, Copy, Default)]
struct CenterAdjustment {
    shift: f64,
    growth: f64,
    advance: f64,
    extent_floor: f64,
    early_extent: f64,
}

/// Alternating lines keep the flow extent separate from the right boundary of
/// children that have not yet been placed. That boundary uses physical widths,
/// including for a column flow. Growth and alignment reach subsequent lines at
/// different stages; merging their pending adjustments loses cancellations.
struct ReverseCentering {
    extent: f64,
    boundary: f64,
    offset: f64,
    last_growth: f64,
    first_reverse: bool,
    pending: [CenterAdjustment; 4],
}

impl ReverseCentering {
    fn new(extent: f64) -> Self {
        Self { extent, boundary: extent, offset: 0., last_growth: 0., first_reverse: true, pending: [CenterAdjustment::default(); 4] }
    }

    fn origin(&mut self, line: &Line, following: f64, first_width: f64, reversed: bool) -> f64 {
        let current = self.pending[0];
        self.offset += current.shift;
        self.extent += 2. * current.growth;
        let origin = self.offset - line.length / 2.;
        self.extent = self.extent.max(current.early_extent);
        if reversed {
            self.reverse(line, following, first_width);
        } else if exceeds(line.length, self.extent) {
            self.forward(line.length, current.early_extent > 0.);
        }
        self.extent = self.extent.max(current.extent_floor);
        self.pending.rotate_left(1);
        self.pending[3] = CenterAdjustment::default();
        origin
    }

    fn reverse(&mut self, line: &Line, following: f64, first_width: f64) {
        let prior_negative = (-self.pending[1].shift).max(0.);
        self.extent += 2. * self.pending[0].advance;
        self.boundary += 2. * self.pending[0].advance;
        let old_extent = self.extent;
        let future = self.pending[2].growth;
        let reference = old_extent - 2. * future;
        let equal = !exceeds(line.length, old_extent) && !exceeds(old_extent, line.length);
        let expansion = if equal { 0. } else { (line.physical_width.max(first_width) - self.boundary).max(0.) };
        self.boundary += expansion;
        let shift = expansion - ((line.length - reference) / 2.).max(0.);
        self.pending[1].shift += shift;
        if self.first_reverse || expansion == 0. || exceeds(line.length, reference) {
            self.carry_reverse(line.length, following, reference, expansion, shift);
        } else {
            self.widen_boundary(line.length, following, old_extent);
        }
        self.pending[1].shift += prior_negative.min(((line.length - old_extent) / 2.).max(0.));
        self.last_growth = self.extent - old_extent;
        self.first_reverse = false;
    }

    fn carry_reverse(&mut self, length: f64, following: f64, reference: f64, expansion: f64, shift: f64) {
        let boundary_extent = reference + expansion;
        let grows = exceeds(following, self.extent);
        if grows {
            self.pending[1].extent_floor = self.extent + 2. * shift;
        }
        let advance = if grows { (expansion - (length - following).max(0.)).max(0.) / 2. } else { 0. };
        self.pending[2].advance = advance;
        self.pending[2].shift += if grows { advance - (boundary_extent - reference.max(length)).max(0.) } else { 0. };
        let retreat = (boundary_extent.min(length) - reference).max(0.) / 2.;
        let cancelled = retreat.min(self.pending[2].growth);
        self.pending[2].shift -= cancelled;
        self.pending[2].growth -= cancelled;
        self.pending[3].shift -= retreat - cancelled;
        self.extent = self.extent.max(length);
    }

    fn widen_boundary(&mut self, length: f64, following: f64, old_extent: f64) {
        let future = self.pending[2].growth;
        self.extent = self.extent.max(length + 2. * future);
        let alignment = old_extent - self.last_growth + 2. * future;
        let step = if exceeds(following, old_extent) { (self.boundary - alignment - (length - following).max(0.)) / 2. } else { 0. };
        self.pending[2].advance = step.max(0.);
        let shift = step - (self.boundary - self.extent).max(0.);
        self.pending[2].shift += shift;
        self.pending[3].shift -= ((self.boundary.min(self.extent) - alignment) / 2.).max(shift + future).max(0.);
        // A boundary exposed by a shorter reverse line is available to the next
        // forward line before its growth trial, unlike a deferred flow extent.
        self.pending[1].early_extent = self.extent + 2. * (self.boundary - self.extent.max(alignment)).max(0.);
    }

    fn forward(&mut self, length: f64, early_extent: bool) {
        let expansion = length - self.extent;
        if !early_extent {
            let cancelled = self.pending[1].growth.min(expansion / 2.);
            self.pending[1].shift -= cancelled;
            self.pending[1].growth -= cancelled;
        }
        self.boundary += expansion / 2.;
        let growth = expansion / 4.;
        let cancelled = growth.min((-self.pending[2].shift).max(0.));
        self.pending[2].shift += cancelled;
        self.pending[3].shift += growth - cancelled;
        self.pending[3].growth += growth - cancelled;
        self.extent = length;
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
        self.layout_inner(children, available, scale, None, work)
    }

    fn layout_inner(
        &self,
        children: &[BoxStyle],
        available: f64,
        scale: f64,
        restored: Option<Restored<'_>>,
        work: &mut usize,
    ) -> Result<Packing, &'static str> {
        charge(work, children.len().saturating_mul(3))?;
        let mut lines = Vec::<Line>::new();
        let mut line = Line { range: 0..0, length: 0., thickness: 0., physical_width: 0. };
        for (i, child) in children.iter().enumerate() {
            let (length, thickness) = if self.column { (child.rect.h, child.rect.w) } else { (child.rect.w, child.rect.h) };
            let length = length * scale;
            let thickness = thickness * scale;
            let full = self.count.map_or_else(|| exceeds(line.length + length, available), |count| line.range.len() >= count);
            if !line.range.is_empty() && full {
                lines.push(line);
                line = Line { range: i..i, length: 0., thickness: 0., physical_width: 0. };
            }
            line.range.end = i + 1;
            line.length += length;
            line.thickness = line.thickness.max(thickness);
            line.physical_width = line.physical_width.max(child.rect.w * scale);
        }
        if !line.range.is_empty() {
            lines.push(line);
        }
        let mut centering = Centering { extent: lines.first().map_or(0., |line| line.length), offset: 0., pending: 0., next: 0. };
        let mut reverse_centering = ReverseCentering::new(centering.extent);
        let mut boxes = vec![Xfrm::new(0., 0., 0., 0.); children.len()];
        if let Some(restored) = &restored {
            for ((rect, child), allocation) in boxes.iter_mut().zip(children).zip(restored.allocations) {
                let (w, h) = (child.rect.w, child.rect.h);
                *rect = Xfrm::new((allocation.rect.w - w) / 2., (allocation.rect.h - h) / 2., w, h);
            }
        }
        let (mut cursor, mut cross, mut low, mut high) = (0., 0., 0_f64, 0_f64);
        for (i, line) in lines.iter().enumerate() {
            let reversed = self.reverse && i % 2 == 1;
            if !self.reverse {
                cursor = 0.;
            }
            let origin = if self.center && !self.reverse {
                centering.origin(line.length)
            } else if self.center {
                let next = lines.get(i + 1);
                let first_width = next.and_then(|line| children.get(line.range.start)).map_or(0., |c| c.rect.w * scale);
                reverse_centering.origin(line, next.map_or(0., |line| line.length), first_width, reversed)
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
                // Keep positions in flow coordinates until every line fits.
                // An overflowing restored line returns before the column swap.
                *rect = Xfrm::new(pos, perpendicular, child.rect.w * scale, child.rect.h * scale);
                if !reversed {
                    pos += length;
                }
            }
            cursor = pos;
            cross += line.thickness + self.gap * scale;
            if restored.as_ref().is_some_and(|r| exceeds(high - low, available) || exceeds(cross - self.gap * scale, r.cross_limit)) {
                // A restored intrinsic size can make fitting impossible. Lines
                // already visited retain their working positions; later children
                // stay centered within their previous, uniformly fitted slots.
                return Ok(occupied_packing(boxes));
            }
        }
        if !lines.is_empty() {
            cross -= self.gap * scale;
        }
        for rect in &mut boxes {
            rect.x -= low;
            if self.column {
                std::mem::swap(&mut rect.x, &mut rect.y);
            }
        }
        let (width, height) = if self.column { (cross, high - low) } else { (high - low, cross) };
        Ok(Packing { boxes, width, height, complete: true })
    }
}

fn occupied_packing(mut boxes: Vec<Xfrm>) -> Packing {
    let (mut left, mut top, mut right, mut bottom) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for rect in &boxes {
        left = left.min(rect.x);
        top = top.min(rect.y);
        right = right.max(rect.x + rect.w);
        bottom = bottom.max(rect.y + rect.h);
    }
    if boxes.is_empty() {
        return Packing { boxes, width: 0., height: 0., complete: false };
    }
    for rect in &mut boxes {
        rect.x -= left;
        rect.y -= top;
    }
    Packing { boxes, width: right - left, height: bottom - top, complete: false }
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
pub(super) fn repack(
    node: &Instance<'_>,
    style: BoxStyle,
    children: &mut [BoxStyle],
    allocations: &[BoxStyle],
    work: &mut usize,
) -> Result<(), &'static str> {
    let policy = Policy::new(node, style, children.len());
    let frame = style.rect;
    let available = if policy.column { frame.h } else { frame.w };
    let cross_limit = if policy.column { frame.w } else { frame.h };
    let packed = policy.layout_inner(children, available, 1., Some(Restored { allocations, cross_limit }), work)?;
    place(node, frame, children, packed);
    Ok(())
}

fn place(node: &Instance<'_>, frame: Xfrm, children: &mut [BoxStyle], packed: Packing) {
    let grow = node.param("grDir", "tL");
    for (child, mut rect) in children.iter_mut().zip(packed.boxes) {
        if packed.complete && grow.ends_with('R') {
            rect.x = packed.width - rect.x - rect.w;
        }
        if packed.complete && grow.starts_with('b') {
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

    fn relative_line_centers(rects: &[Xfrm], count: usize, column: bool) -> Vec<f64> {
        let centers: Vec<_> = rects
            .chunks(count)
            .map(|line| {
                let low = line.iter().map(|r| if column { r.y } else { r.x }).fold(f64::INFINITY, f64::min);
                let high = line.iter().map(|r| if column { r.y + r.h } else { r.x + r.w }).fold(f64::NEG_INFINITY, f64::max);
                (low + high) / 2.
            })
            .collect();
        centers.iter().map(|v| v - centers[0]).collect()
    }

    #[test]
    fn reverse_centering_keeps_child_boundaries_and_pending_growth_separate() {
        let params = [("bkpt", "fixed"), ("bkPtFixedVal", "2"), ("off", "ctr"), ("contDir", "revDir")];
        for (first, expected) in [(80., [0., 0., -10., -10., 30., 40., 40., 40., 40.]), (100., [0., 0., -10., -10., 50., 50., 50., 50., 50.])] {
            // Equal total line widths have different pending boundaries when
            // their first child is wider. The following growth cancels an older
            // adjustment without moving the already placed lines.
            let widths = [10., 10., 20., 20., 20., 60., 10., 10., first, 120. - first, 10., 10., 10., 10., 10., 10., 10., 10.];
            let sizes: Vec<_> = widths.into_iter().map(|w| (w, 10.)).collect();
            let actual = packed(&sizes, (300., 200.), &params, 0.);
            for (center, expected) in relative_line_centers(&actual, 2, false).into_iter().zip(expected) {
                assert!((center - expected).abs() < 1e-7, "{first}: {center} != {expected}");
            }
            for (rect, &(w, h)) in actual.iter().zip(&sizes) {
                assert!((rect.w - w).abs() < 1e-7 && (rect.h - h).abs() < 1e-7);
            }
        }
    }

    #[test]
    fn reverse_column_boundaries_use_physical_child_widths() {
        let sizes = [
            (11., 17.5),
            (11., 14.583),
            (9., 2.917),
            (8., 2.615),
            (10., 5.231),
            (11., 9.154),
            (12., 8.333),
            (11., 6.667),
            (12., 5.),
            (12., 46.154),
            (8., 18.462),
            (10., 55.384),
            (11., 15.),
            (11., 15.),
            (8., 10.),
        ];
        let params = [("bkpt", "fixed"), ("bkPtFixedVal", "3"), ("off", "ctr"), ("contDir", "revDir"), ("flowDir", "col")];
        let actual = packed(&sizes, (200., 300.), &params, 0.);
        for (center, expected) in relative_line_centers(&actual, 3, true).into_iter().zip([0., 0., 0., 0., -42.5]) {
            assert!((center - expected).abs() < 1e-7, "{center} != {expected}");
        }
        let low = actual.iter().map(|r| r.y).fold(f64::INFINITY, f64::min);
        let high = actual.iter().map(|r| r.y + r.h).fold(f64::NEG_INFINITY, f64::max);
        assert!((low + high - 300.).abs() < 1e-7);
        for (rect, (w, h)) in actual.iter().zip(sizes) {
            assert!((rect.w - w).abs() < 1e-7 && (rect.h - h).abs() < 1e-7);
        }
    }

    #[test]
    fn restored_overflow_retains_completed_lines_and_unplaced_allocations() {
        let xml = crate::xml::parse(b"<layoutNode/>").unwrap();
        let mut node = Instance::new(&xml.root);
        node.alg = "snake".into();
        node.params = [("bkpt", "fixed"), ("bkPtFixedVal", "2"), ("flowDir", "col")].map(|(k, v)| (k.into(), v.into())).into_iter().collect();
        let sizes = [(8., 12.), (15., 10.), (10., 28.), (20., 18.), (14., 22.)];
        let slots = [(8.1, 6.), (15.1, 5.), (10.1, 14.), (20.1, 9.), (14.1, 11.)];
        let allocations: Vec<_> = slots.into_iter().map(|(w, h)| BoxStyle::new(Xfrm::new(0., 0., w, h))).collect();
        // The second column exceeds the flow limit after both children have
        // been positioned. Its coordinates remain unswapped, while the last
        // child retains the centering inside its old allocation. Returning
        // before orientation also skips the requested growth reflection.
        let positions = [(-1.5, 2.25), (10.5, -1.25), (-1.5, 18.75), (26.5, 13.75), (-1.45, -6.75)];
        for grow in ["tL", "tR", "bL", "bR"] {
            node.params.insert("grDir".into(), grow.into());
            let mut children: Vec<_> = sizes.iter().map(|&(w, h)| BoxStyle::new(Xfrm::new(0., 0., w, h))).collect();
            repack(&node, BoxStyle::new(Xfrm::new(0., 0., 45., 40.)), &mut children, &allocations, &mut 0).unwrap();
            for ((child, (x, y)), (w, h)) in children.iter().zip(positions).zip(sizes) {
                assert!((child.rect.x - x).abs() < 1e-7 && (child.rect.y - y).abs() < 1e-7);
                assert_eq!((child.rect.w, child.rect.h), (w, h));
            }
        }
    }

    #[test]
    fn restored_columns_complete_the_axis_swap_when_all_lines_fit() {
        let xml = crate::xml::parse(b"<layoutNode/>").unwrap();
        let mut node = Instance::new(&xml.root);
        node.alg = "snake".into();
        node.params = [("bkpt", "fixed"), ("bkPtFixedVal", "2"), ("flowDir", "col")].map(|(k, v)| (k.into(), v.into())).into_iter().collect();
        let sizes = [(8., 12.), (15., 10.), (10., 28.), (20., 18.), (14., 22.)];
        let mut children: Vec<_> = sizes.iter().map(|&(w, h)| BoxStyle::new(Xfrm::new(0., 0., w, h))).collect();
        let allocations = children.clone();
        repack(&node, BoxStyle::new(Xfrm::new(0., 0., 80., 80.)), &mut children, &allocations, &mut 0).unwrap();
        let positions = [(19., 17.), (15.5, 29.), (35.5, 17.), (30.5, 45.), (50.5, 17.)];
        for ((child, (x, y)), (w, h)) in children.iter().zip(positions).zip(sizes) {
            assert!((child.rect.x - x).abs() < 1e-7 && (child.rect.y - y).abs() < 1e-7);
            assert_eq!((child.rect.w, child.rect.h), (w, h));
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
