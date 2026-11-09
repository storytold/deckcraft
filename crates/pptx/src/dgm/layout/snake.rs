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

#[derive(Clone, Copy)]
enum PackingPhase {
    Nominal,
    Fitted,
    Restored,
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
#[derive(Clone, Copy)]
struct Centering {
    extent: f64,
    offset: f64,
    pending: f64,
    next: f64,
}

impl Centering {
    fn new(extent: f64) -> Self {
        Self { extent, offset: 0., pending: 0., next: 0. }
    }

    fn origin(&mut self, length: f64) -> f64 {
        self.advance(length);
        self.offset - length / 2.
    }

    fn advance(&mut self, length: f64) -> (f64, f64) {
        let drained = self.pending;
        self.offset += self.pending;
        self.extent += 2. * self.pending;
        self.pending = self.next;
        self.next = 0.;
        let mut expansion = 0.;
        if length > self.extent {
            expansion = length - self.extent;
            self.pending = (self.pending - expansion / 2.).max(0.);
            self.next = expansion / 4.;
            self.extent = length;
        }
        (drained, expansion)
    }
}

#[derive(Clone, Copy, Default)]
struct CenterAdjustment {
    shift: f64,
    growth: f64,
    advance: f64,
    // Physical-boundary and logical-extent advances if the following line grows.
    carry_advance: Option<(f64, f64)>,
    // Cancelled growth returned when the intervening forward line grows.
    forward_return: Option<f64>,
    boundary_advance: Option<f64>,
    extent_floor: f64,
    early_extent: f64,
    reference_correction_limit: f64,
    capacity: f64,
    boundary_floor: Option<f64>,
}

/// Alternating lines keep the flow extent separate from the right boundary of
/// children that have not yet been placed. That boundary uses physical widths,
/// including for a column flow. Growth and alignment reach subsequent lines at
/// different stages; merging their pending adjustments loses cancellations.
struct ReverseCentering {
    extent: f64,
    boundary: f64,
    reference: Option<f64>,
    offset: f64,
    last_growth: f64,
    first_reverse: bool,
    pending: [CenterAdjustment; 4],
}

impl ReverseCentering {
    fn new(extent: f64, phase: PackingPhase) -> Self {
        Self {
            extent,
            boundary: extent,
            reference: matches!(phase, PackingPhase::Nominal).then_some(extent),
            offset: 0.,
            last_growth: 0.,
            first_reverse: true,
            pending: [CenterAdjustment::default(); 4],
        }
    }

    fn origin(&mut self, line: &Line, following: f64, first_width: f64, reversed: bool) -> f64 {
        let current = self.pending[0];
        self.offset += current.shift;
        self.extent += 2. * current.growth;
        let origin = self.offset - line.length / 2.;
        if let Some(boundary) = current.boundary_floor {
            self.boundary = self.boundary.max(boundary);
        }
        let entering_extent = self.extent;
        self.extent = self.extent.max(current.early_extent);
        if reversed {
            self.reverse(line, following, first_width);
        } else if exceeds(line.length, self.extent) {
            self.forward(line.length, entering_extent, current.early_extent > 0.);
        }
        self.extent = self.extent.max(current.extent_floor);
        self.pending.rotate_left(1);
        self.pending[3] = CenterAdjustment::default();
        origin
    }

    fn reverse(&mut self, line: &Line, following: f64, first_width: f64) {
        let prior_negative = (-self.pending[1].shift).max(0.);
        self.extent += 2. * self.pending[0].advance;
        self.boundary += 2. * self.pending[0].boundary_advance.unwrap_or(self.pending[0].advance);
        if let Some(reference) = &mut self.reference {
            *reference += 2. * self.pending[0].advance;
        }
        let old_extent = self.extent;
        let future = self.pending[2].growth;
        let reference = old_extent - 2. * future;
        let equal = !exceeds(line.length, old_extent) && !exceeds(old_extent, line.length);
        // Nominal physical growth can remain beyond the current reference even
        // when logical lengths match. Forward growth replaces that reference.
        let retained_boundary = self.reference.is_some_and(|reference| exceeds(self.boundary, reference));
        let expansion = if equal && !retained_boundary { 0. } else { (line.physical_width.max(first_width) - self.boundary).max(0.) };
        self.boundary += expansion;
        let shift = expansion - ((line.length - reference) / 2.).max(0.);
        self.pending[1].shift += shift;
        if self.first_reverse || expansion == 0. || exceeds(line.length, reference) {
            self.carry_reverse(line.length, following, reference, expansion, shift);
        } else {
            self.widen_boundary(line.length, following, old_extent);
        }
        self.pending[1].shift += prior_negative.min(((line.length - old_extent) / 2.).max(0.));
        if exceeds(self.pending[0].reference_correction_limit, 0.) && exceeds(self.pending[0].advance, 0.) && expansion == 0. && following > 0. {
            // The retained reference reaches the next forward line in stages.
            // A partial advance cannot spend the full reflected-boundary debt.
            if let Some(reference) = self.reference {
                self.pending[1].shift += self.pending[0].reference_correction_limit.min((self.boundary - reference) / 4.);
            }
        }
        self.last_growth = self.extent - old_extent;
        self.first_reverse = false;
    }

    fn carry_reverse(&mut self, length: f64, following: f64, reference: f64, expansion: f64, shift: f64) {
        let grows = exceeds(following, self.extent);
        let physical_reference = self.boundary - expansion;
        if grows {
            self.pending[1].extent_floor = self.extent + 2. * shift;
        }
        // A logical floor may already exceed the physical boundary. Deferred
        // alignment and retreat still start at that boundary before expansion.
        let advance_grows = exceeds(following, physical_reference);
        let advance = if advance_grows { (expansion - (length - following).max(0.)).max(0.) / 2. } else { 0. };
        self.pending[2].advance = advance;
        // Borrowed following-child width supplies immediate alignment, but a
        // growing forward line separates the deferred physical and flow extents.
        // Each deferred amount remains bounded by the advance already scheduled.
        self.pending[2].carry_advance = self.reference.map(|_| {
            let physical = advance.min((length - reference).max(0.) / 2.);
            let logical = advance.min(physical + (following - self.boundary).max(0.) / 2.);
            (physical, logical)
        });
        // Prior forward growth consumes only advance that was actually queued.
        let cancelled_advance = if self.reference.is_some() && advance_grows { self.pending[2].growth.min(advance / 2.) } else { 0. };
        self.pending[2].shift += if advance_grows { advance - 2. * cancelled_advance - (self.boundary - reference.max(length)).max(0.) } else { 0. };
        self.pending[2].forward_return = Some(cancelled_advance);
        let retreat = (self.boundary.min(length) - physical_reference).max(0.) / 2.;
        let cancelled = retreat.min(self.pending[2].growth);
        self.pending[2].shift -= cancelled;
        self.pending[2].growth -= cancelled;
        self.pending[3].shift -= retreat - cancelled;
        self.extent = self.extent.max(length);
    }

    fn widen_boundary(&mut self, length: f64, following: f64, old_extent: f64) {
        let future = self.pending[2].growth;
        self.extent = self.extent.max(length + 2. * future);
        let mut alignment = old_extent - self.last_growth + 2. * future;
        // A retained nominal reference differs from an ordinary forward-growth
        // extent. Its alignment moves through the exposed boundary and the next
        // deferred line before reaching the reflected-reference limit. Fitted
        // and restored trials do not own this nominal history.
        if let Some(reference) = self.reference.filter(|&reference| exceeds(old_extent, reference) && exceeds(following, old_extent)) {
            let exposed = following - old_extent;
            let retained = old_extent - reference;
            let reflected = old_extent - 2. * retained + 2. * future;
            let first = old_extent - exposed + 2. * future;
            let second = old_extent + retained - 3. * exposed + 2. * future;
            let corrected = reflected.max(first.min(second));
            if exceeds(alignment, corrected) || exceeds(corrected, alignment) {
                self.pending[2].reference_correction_limit = (exposed - retained / 2.).max(0.);
            }
            alignment = corrected;
        }
        let step = if exceeds(following, old_extent) { (self.boundary - alignment - (length - following).max(0.)) / 2. } else { 0. };
        let step = if self.reference.is_some() {
            // A nominal retreat consumes pending forward growth. Its center
            // stops when that growth is exhausted, and the consumed portion
            // must not later enlarge the logical extent.
            self.pending[2].growth -= (-step).max(0.).min(future);
            step.max(-future)
        } else {
            step
        };
        self.pending[2].advance = step.max(0.);
        let shift = step - (self.boundary - self.extent).max(0.);
        self.pending[2].shift += shift;
        self.pending[3].shift -= ((self.boundary.min(self.extent) - alignment) / 2.).max(shift + future).max(0.);
        // A boundary exposed by a shorter reverse line is available to the next
        // forward line before its growth trial, unlike a deferred flow extent.
        self.pending[1].early_extent = self.extent + 2. * (self.boundary - self.extent.max(alignment)).max(0.);
    }

    fn forward(&mut self, length: f64, entering_extent: f64, early_extent: bool) {
        let expansion = length - self.extent;
        let (boundary_growth, growth) = self.forward_growth(length, entering_extent);
        // Forward growth returns the cancelled adjustment on the next line;
        // half remains after the following line completes the retreat.
        let returned = self.pending[1].forward_return.take().unwrap_or(0.);
        self.pending[1].shift += returned;
        self.pending[2].shift -= returned / 2.;
        // Keep the gross advance while computing this line's growth; only its
        // deferred state changes after the forward growth calculation.
        if let Some((physical, logical)) = self.pending[1].carry_advance {
            self.pending[1].advance = logical;
            self.pending[1].boundary_advance = Some(physical);
        }
        if let Some(reference) = &mut self.reference {
            // Fitting sees this growth on the second following line. Physical
            // extent retained from a reverse line absorbs part of the increase,
            // independently of the later placement and alignment adjustments.
            let retained = (self.boundary - *reference).max(0.);
            self.pending[2].capacity = length + (expansion - retained).max(0.) / 2.;
            *reference = reference.max(length);
        }
        if !early_extent {
            let cancelled = self.pending[1].growth.min(expansion / 2.);
            self.pending[1].shift -= cancelled;
            self.pending[1].growth -= cancelled;
        }
        self.boundary += boundary_growth / 2.;
        let cancelled = growth.min((-self.pending[2].shift).max(0.));
        self.pending[2].shift += cancelled;
        self.pending[3].shift += growth - cancelled;
        self.pending[3].growth += growth - cancelled;
        self.extent = length;
    }

    fn forward_growth(&mut self, length: f64, entering_extent: f64) -> (f64, f64) {
        let expansion = length - self.extent;
        let Some(reference) = self.reference else {
            return (expansion, expansion / 4.);
        };
        let projected = 2. * (self.pending[1].advance + self.pending[1].growth);
        let retained = (self.boundary - reference - projected).max(0.);
        let mut boundary_growth = (expansion - retained).max(0.);
        let mut growth = boundary_growth / 4.;
        let mut boundary = self.boundary + boundary_growth;
        if exceeds(self.extent, entering_extent) {
            // An exposed boundary supplies space before the forward line grows.
            // Its later center is a target, after shifts already due on the
            // intervening lines, rather than another independent translation.
            let raw_growth = length - entering_extent;
            let pending = self.pending[1].shift + self.pending[2].shift + self.pending[3].shift;
            growth = (raw_growth / 4. - (self.boundary - reference) / 2. - pending).max(0.);
            boundary = self.boundary + raw_growth;
            boundary_growth = expansion;
        }
        // The physical boundary reaches the third following line. An earlier
        // wider boundary absorbs this floor instead of receiving it again.
        self.pending[3].boundary_floor = Some(self.pending[3].boundary_floor.map_or(boundary, |old| old.max(boundary)));
        (boundary_growth, growth)
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

    fn layout(&self, children: &[BoxStyle], available: f64, scale: f64, phase: PackingPhase, work: &mut usize) -> Result<Packing, &'static str> {
        self.layout_inner(children, available, scale, phase, None, work)
    }

    fn lines(&self, children: &[BoxStyle], available: f64, scale: f64) -> Vec<Line> {
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
        lines
    }

    fn layout_inner(
        &self,
        children: &[BoxStyle],
        available: f64,
        scale: f64,
        phase: PackingPhase,
        restored: Option<Restored<'_>>,
        work: &mut usize,
    ) -> Result<Packing, &'static str> {
        charge(work, children.len().saturating_mul(3))?;
        let lines = self.lines(children, available, scale);
        let mut centering = Centering { extent: lines.first().map_or(0., |line| line.length), offset: 0., pending: 0., next: 0. };
        let mut reverse_centering = ReverseCentering::new(centering.extent, phase);
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

struct ReverseTrial {
    lines: Vec<Line>,
    origins: Vec<f64>,
    logical: f64,
    cross: f64,
    phase: CenterFitPhase,
}

struct ReverseSearch {
    initial: ReverseTrial,
    best: ReverseTrial,
    scale: f64,
    phase: CenterFitPhase,
}

impl Policy {
    fn reverse_trial(&self, children: &[BoxStyle], flow: f64, cross_limit: f64, scale: f64, work: &mut usize) -> Result<ReverseTrial, &'static str> {
        charge(work, children.len().saturating_mul(4))?;
        let lines = self.lines(children, flow, scale);
        let mut centering = ReverseCentering::new(lines.first().map_or(0., |line| line.length), PackingPhase::Nominal);
        let mut origins = Vec::with_capacity(lines.len());
        let (mut logical, mut cross) = (0_f64, 0.);
        let (mut raw_failed, mut stop) = (false, None);
        for (index, line) in lines.iter().enumerate() {
            let next = lines.get(index + 1);
            let first_width = next.and_then(|line| children.get(line.range.start)).map_or(0., |c| c.rect.w * scale);
            let early = centering.pending[0].early_extent;
            let capacity = centering.pending[0].capacity;
            let origin = centering.origin(line, next.map_or(0., |line| line.length), first_width, index % 2 == 1);
            origins.push(origin);
            let midpoint = (centering.boundary + centering.reference.unwrap_or(centering.boundary)) / 2.;
            // Painted bounds are centered after fitting and can exceed the
            // canvas even when every required logical extent fits.
            logical = logical.max(line.length).max(early).max(midpoint).max(capacity);
            raw_failed |= exceeds(line.length, flow);
            if stop.is_none() && exceeds(logical, flow) {
                stop = Some(index);
            }
            cross += line.thickness + if index > 0 { self.gap * scale } else { 0. };
        }
        let phase = if raw_failed {
            CenterFitPhase::RawOverflow
        } else if exceeds(logical, flow) {
            CenterFitPhase::CenterOverflow(stop.unwrap_or(0))
        } else if exceeds(cross, cross_limit) {
            CenterFitPhase::CrossOverflow
        } else {
            CenterFitPhase::Fits
        };
        Ok(ReverseTrial { lines, origins, logical, cross, phase })
    }

    fn fit_reversed(&self, children: &[BoxStyle], frame: Xfrm, grow: &str, refit: bool, work: &mut usize) -> Result<Packing, &'static str> {
        let (flow, cross) = if self.column { (frame.h, frame.w) } else { (frame.w, frame.h) };
        let initial = self.reverse_trial(children, flow, cross, 1., work)?;
        if !refit && matches!(initial.phase, CenterFitPhase::Fits) {
            return self.layout(children, flow, 1., PackingPhase::Nominal, work);
        }
        let (mut low, mut high) = (0., 1.);
        while !matches!(self.reverse_trial(children, flow, cross, high * 0.5, work)?.phase, CenterFitPhase::Fits) {
            high *= 0.5;
        }
        let mut best = self.reverse_trial(children, flow, cross, 0., work)?;
        let mut phase = CenterFitPhase::Fits;
        for _ in 0..10 {
            let scale = (low + high) / 2.;
            let trial = self.reverse_trial(children, flow, cross, scale, work)?;
            phase = trial.phase;
            if matches!(phase, CenterFitPhase::Fits) {
                low = scale;
                best = trial;
            } else {
                high = scale;
            }
        }
        self.finish_reversed(children, ReverseSearch { initial, best, scale: low, phase }, flow, grow, work)
    }

    fn finish_reversed(
        &self,
        children: &[BoxStyle],
        search: ReverseSearch,
        flow: f64,
        grow: &str,
        work: &mut usize,
    ) -> Result<Packing, &'static str> {
        charge(work, children.len().saturating_mul(6))?;
        let ReverseSearch { initial, best, scale, phase } = search;
        let mut boxes: Vec<_> = children.iter().map(|c| Xfrm::new(0., 0., c.rect.w * scale, c.rect.h * scale)).collect();
        let first = initial.lines.first().map_or(0., |line| line.length);
        let stop = if let CenterFitPhase::CenterOverflow(index) = phase { Some(index) } else { None };
        let mut cross = 0.;
        // Dimensions fit first. The last trial decides which positions survive:
        // an accepted trial keeps nominal placement; logical rejection leaves
        // unvisited children at the original origin before any reflection.
        for (index, (line, &origin)) in initial.lines.iter().zip(&initial.origins).enumerate() {
            let reversed = index % 2 == 1;
            let mut pos = first / 2. + origin + if reversed { line.length } else { 0. };
            for (child, rect) in
                children.get(line.range.clone()).unwrap_or_default().iter().zip(boxes.get_mut(line.range.clone()).unwrap_or_default())
            {
                let (length, thickness) = if self.column { (child.rect.h, child.rect.w) } else { (child.rect.w, child.rect.h) };
                if reversed {
                    pos -= length;
                }
                rect.x = pos;
                rect.y = cross + (line.thickness - thickness) / 2.;
                if !reversed {
                    pos += length;
                }
            }
            cross += line.thickness + self.gap;
            if stop == Some(index) {
                break;
            }
        }
        if stop.is_none() {
            for (child, rect) in children.iter().zip(&mut boxes) {
                let (length, thickness) = if self.column { (child.rect.h, child.rect.w) } else { (child.rect.w, child.rect.h) };
                if grow.ends_with('R') {
                    rect.x = initial.logical - rect.x - length;
                }
                if grow.starts_with('b') {
                    rect.y = initial.cross - rect.y - thickness;
                }
            }
        }
        let first = best.lines.first().map_or(0., |line| line.length);
        let mut centering = ReverseCentering::new(first, PackingPhase::Fitted);
        let (mut cross, mut previous_origin, mut previous_center) = (0., 0., 0.);
        for (index, line) in best.lines.iter().enumerate() {
            let reversed = index % 2 == 1;
            let raw_origin = previous_origin + if reversed { centering.extent - line.length } else { 0. };
            let next = best.lines.get(index + 1);
            let first_width = next.and_then(|line| children.get(line.range.start)).map_or(0., |c| c.rect.w * scale);
            let mut wanted = first / 2. + centering.origin(line, next.map_or(0., |line| line.length), first_width, reversed);
            if matches!(phase, CenterFitPhase::CrossOverflow) && index + 1 == best.lines.len() {
                // The final cross-capacity rejection stops the last alignment
                // from consuming the preceding center, even when flow fits.
                wanted += previous_center;
            }
            let mut pos = raw_origin + if reversed { line.length } else { 0. };
            for (child, rect) in
                children.get(line.range.clone()).unwrap_or_default().iter().zip(boxes.get_mut(line.range.clone()).unwrap_or_default())
            {
                let (length, thickness) = if self.column { (child.rect.h, child.rect.w) } else { (child.rect.w, child.rect.h) };
                if reversed {
                    pos -= length * scale;
                }
                rect.x = pos;
                rect.y = cross + (line.thickness - thickness * scale) / 2.;
                if !reversed {
                    pos += length * scale;
                }
            }
            // Reverse placement uses the carried right boundary before the new
            // logical extent is checked. An overflow returns before centering,
            // column transposition and the requested growth reflection.
            if exceeds(centering.extent, flow) {
                return Ok(occupied_packing(boxes));
            }
            for rect in boxes.get_mut(line.range.clone()).unwrap_or_default() {
                rect.x += wanted - raw_origin;
            }
            previous_origin = wanted;
            previous_center = wanted + line.length / 2. - first / 2.;
            cross += line.thickness + self.gap * scale;
        }
        if self.column {
            for rect in &mut boxes {
                std::mem::swap(&mut rect.x, &mut rect.y);
            }
        }
        let mut packed = occupied_packing(boxes);
        packed.complete = true;
        Ok(packed)
    }
}

#[derive(Clone, Copy)]
enum CenterFitPhase {
    Fits,
    RawOverflow,
    CenterOverflow(usize),
    CrossOverflow,
}

struct CenterTrial {
    lines: Vec<Line>,
    centers: Vec<Centering>,
    phase: CenterFitPhase,
}

#[derive(Default)]
struct CenterExit {
    stop: Option<usize>,
    cross_failed: bool,
    raw_failed: bool,
}

struct CenterSearch {
    normalized: bool,
    initial: CenterTrial,
    best: CenterTrial,
    scale: f64,
    exit: CenterExit,
}

impl CenterExit {
    fn observe(&mut self, phase: CenterFitPhase) {
        self.raw_failed = matches!(phase, CenterFitPhase::RawOverflow);
        match phase {
            CenterFitPhase::Fits => {
                self.stop = None;
                self.cross_failed = false;
            }
            CenterFitPhase::CenterOverflow(index) => {
                self.stop = Some(index);
                self.cross_failed = false;
            }
            CenterFitPhase::CrossOverflow => self.cross_failed = true,
            CenterFitPhase::RawOverflow => {}
        }
    }
}

#[derive(Clone, Copy)]
struct FittedLine {
    reference_adjustment: f64,
    nominal: Centering,
    fitted: Centering,
    reference: f64,
    length: f64,
    largest: f64,
    largest_offset: f64,
    correction: f64,
    expansion: f64,
}

/// Fitting retains the nominal alignment references while dimensions change.
/// Keep both centering cursors: an alignment correction moves the fitted cursor,
/// and its portion not consumed by line growth becomes pending for the next line.
struct FittedCentering {
    consumption_limit: Option<(f64, usize)>,
    nominal: Centering,
    fitted: Centering,
    nominal_refs: [f64; 2],
    fitted_refs: [f64; 2],
    reference_adjustments: [f64; 2],
    held: [bool; 2],
    balance: f64,
    reference_origin: f64,
    corrected: bool,
    previous: Option<f64>,
    previous_correction: f64,
    previous_reference: f64,
    previous_carried: bool,
    previous_expansion: f64,
    largest: f64,
    largest_offset: f64,
    rebound: Option<f64>,
    rebound_adjustment: f64,
}

impl FittedCentering {
    fn new(first: f64, scale: f64, consumption_limit: Option<(f64, usize)>) -> Self {
        Self {
            consumption_limit,
            nominal: Centering::new(first),
            fitted: Centering::new(first * scale),
            nominal_refs: [first; 2],
            fitted_refs: [first * scale; 2],
            reference_adjustments: [0.; 2],
            held: [false; 2],
            balance: 0.,
            reference_origin: 0.,
            corrected: false,
            previous: None,
            previous_correction: 0.,
            previous_reference: first,
            previous_carried: false,
            previous_expansion: 0.,
            largest: 0.,
            largest_offset: 0.,
            rebound: None,
            rebound_adjustment: 0.,
        }
    }

    fn advance(&mut self, length: f64, scale: f64, index: usize, nominal_balance: Option<f64>) -> FittedLine {
        let pending_before = self.fitted.next;
        let prior_extent = self.nominal.extent;
        let old_fitted_reference = self.fitted_refs[index % 2];
        let rebound = self.rebound.take();
        let (nominal_growth, nominal_expansion) = self.nominal.advance(length);
        let (fitted_growth, expansion) = self.fitted.advance(length * scale);
        for ((nominal, fitted), held) in self.nominal_refs.iter_mut().zip(&mut self.fitted_refs).zip(self.held) {
            *nominal += 2. * nominal_growth;
            if !held {
                *fitted += 2. * fitted_growth;
            }
        }
        // The slot is internal parity, never an input-derived index.
        let slot = index % 2;
        let reference_adjustment = self.reference_adjustments[slot];
        self.reference_adjustments[slot] = 0.;
        let reference = self.nominal_refs[slot];
        let mut alignment = self.fitted_refs[slot];
        let mut carried_reference = false;
        let mut carried_shrink = 0.;
        if exceeds(fitted_growth, nominal_growth * scale)
            && exceeds(self.previous_correction, 0.)
            && let Some(previous) = self.previous.filter(|&previous| exceeds(prior_extent, previous))
        {
            alignment = old_fitted_reference - (prior_extent - previous);
            carried_reference = true;
            carried_shrink = (prior_extent - previous) * scale / 2.;
            self.reference_adjustments[slot] =
                fitted_growth - nominal_growth * scale + (prior_extent - previous + (reference - length).max(0.)) * scale / 2.;
        }
        let origin = if self.held[slot] { self.reference_origin } else { 0. };
        self.held[slot] = false;
        let mut correction = 0.;
        if let Some(previous) = self.previous.filter(|_| index >= 2 && exceeds(length, reference)) {
            let previous = previous * scale;
            if exceeds(previous, alignment) || exceeds(alignment, previous) {
                // A held boundary that grows replaces the following line's
                // reference. Its nominal correction is scaled once; retaining
                // an earlier fitted correction would suppress later growth.
                if let Some((boundary, nominal)) = rebound.zip(nominal_balance) {
                    alignment = boundary;
                    self.balance = nominal * scale + self.rebound_adjustment;
                }
                let difference = if carried_reference { alignment - previous } else { (alignment - previous).max(0.) };
                let candidate = (length - reference - difference) / 2.;
                let consumed = if exceeds(previous, alignment)
                    && !carried_reference
                    && self.consumption_limit.is_some_and(|(flow, stop)| exceeds(previous, flow * scale) || stop == index + 1)
                {
                    self.balance.max(expansion / 2.)
                } else {
                    self.balance
                };
                correction = (candidate - consumed - origin).max(0.);
                if exceeds(fitted_growth, nominal_growth * scale)
                    && exceeds(self.previous_correction, 0.)
                    && !self.previous_carried
                    && !carried_reference
                    && exceeds(self.previous_expansion, 0.)
                    && exceeds(reference, self.previous_reference)
                    && !exceeds(prior_extent * scale, previous)
                {
                    correction = ((length - previous / scale - (reference - self.previous_reference) * scale) / 2.).max(0.);
                }
                if exceeds(correction, 0.) {
                    // Correcting the left boundary can release pending growth
                    // consumed by this line's expansion. Recompute that balance
                    // from its value before expansion, including partial release.
                    let pending = (pending_before + if carried_reference { correction / 4. } else { (correction - expansion) / 2. }).max(0.)
                        - self.fitted.pending;
                    self.fitted.offset -= correction;
                    self.fitted.pending += pending;
                    self.reference_origin += pending * (2. * scale - 1.);
                    self.balance += correction - 2. * pending.min(self.nominal.pending);
                    self.corrected = true;
                } else if self.corrected {
                    self.held[slot] = true;
                }
            }
        }
        if carried_reference && !exceeds(correction, 0.) {
            self.reference_adjustments[slot] -= (length - reference).max(0.) * scale / 2.;
        }
        if !self.held[slot] {
            self.nominal_refs[slot] = reference.max(length);
        }
        self.fitted_refs[slot] = 2. * self.fitted.extent - reference * scale;
        if exceeds(length, self.largest) {
            self.largest = length;
            self.largest_offset = self.nominal.offset;
        }
        self.previous = Some(length);
        self.previous_correction = correction;
        self.previous_reference = reference;
        self.previous_carried = carried_reference;
        self.previous_expansion = nominal_expansion;
        if self.held[slot] && exceeds(nominal_expansion, 0.) {
            self.rebound = Some(self.fitted.extent - expansion);
            self.rebound_adjustment = carried_shrink;
        }
        FittedLine {
            reference_adjustment,
            nominal: self.nominal,
            fitted: self.fitted,
            reference,
            length,
            largest: self.largest,
            largest_offset: self.largest_offset,
            correction,
            expansion,
        }
    }
}

impl Policy {
    fn center_trial(&self, children: &[BoxStyle], flow: f64, cross: f64, scale: f64, work: &mut usize) -> Result<CenterTrial, &'static str> {
        charge(work, children.len().saturating_mul(3))?;
        let lines = self.lines(children, flow, scale);
        let mut centering = Centering::new(lines.first().map_or(0., |line| line.length));
        let mut centers = Vec::with_capacity(lines.len());
        let (mut cross_extent, mut raw_overflow, mut stop) = (0., false, None);
        for (index, line) in lines.iter().enumerate() {
            centering.origin(line.length);
            centers.push(centering);
            raw_overflow |= exceeds(line.length, flow);
            if stop.is_none() && exceeds(centering.extent + 2. * centering.pending, flow) {
                stop = Some(index);
            }
            cross_extent += line.thickness;
            if index > 0 {
                cross_extent += self.gap * scale;
            }
        }
        let phase = if raw_overflow {
            CenterFitPhase::RawOverflow
        } else if exceeds(centering.extent + 2. * centering.pending, flow) {
            CenterFitPhase::CenterOverflow(stop.unwrap_or(0))
        } else if exceeds(cross_extent, cross) {
            CenterFitPhase::CrossOverflow
        } else {
            CenterFitPhase::Fits
        };
        Ok(CenterTrial { lines, centers, phase })
    }

    fn fit_centered(&self, children: &[BoxStyle], frame: Xfrm, grow: &str, refit: bool, work: &mut usize) -> Result<Packing, &'static str> {
        let (flow, cross) = if self.column { (frame.h, frame.w) } else { (frame.w, frame.h) };
        let initial = self.center_trial(children, flow, cross, 1., work)?;
        if !refit && matches!(initial.phase, CenterFitPhase::Fits) {
            return self.layout(children, flow, 1., PackingPhase::Nominal, work);
        }
        let (mut low, mut high) = (0., 1.);
        let mut exit = CenterExit::default();
        let mut normalized = false;
        loop {
            let trial = self.center_trial(children, flow, cross, high * 0.5, work)?;
            exit.observe(trial.phase);
            if matches!(trial.phase, CenterFitPhase::Fits) {
                break;
            }
            normalized = true;
            high *= 0.5;
        }
        let mut best = self.center_trial(children, flow, cross, 0., work)?;
        for _ in 0..10 {
            let scale = (low + high) / 2.;
            let trial = self.center_trial(children, flow, cross, scale, work)?;
            exit.observe(trial.phase);
            if matches!(trial.phase, CenterFitPhase::Fits) {
                low = scale;
                best = trial;
            } else {
                high = scale;
            }
        }
        // Raw length rejection precedes placement. The accepted dimensions can
        // use an ordinary layout without retaining intermediate alignment state.
        if exit.raw_failed {
            return self.layout(children, flow, low, PackingPhase::Fitted, work);
        }
        self.finish_centered(children, CenterSearch { initial, best, scale: low, exit, normalized }, flow, grow, work)
    }

    fn nominal_seed(&self, children: &[BoxStyle], trial: &CenterTrial, grow: &str) -> Vec<Xfrm> {
        let mut boxes: Vec<_> = children.iter().map(|c| Xfrm::new(0., 0., c.rect.w, c.rect.h)).collect();
        let first = trial.lines.first().map_or(0., |line| line.length);
        let mut cross = 0.;
        for (index, (line, center)) in trial.lines.iter().zip(&trial.centers).enumerate() {
            let mut pos = first / 2. + center.offset - line.length / 2.;
            for (child, rect) in
                children.get(line.range.clone()).unwrap_or_default().iter().zip(boxes.get_mut(line.range.clone()).unwrap_or_default())
            {
                let (length, thickness) = if self.column { (child.rect.h, child.rect.w) } else { (child.rect.w, child.rect.h) };
                rect.x = pos;
                rect.y = cross + (line.thickness - thickness) / 2.;
                pos += length;
            }
            cross += line.thickness + self.gap;
            if matches!(trial.phase, CenterFitPhase::CenterOverflow(stop) if index == stop) {
                return boxes;
            }
        }
        if self.column {
            for rect in &mut boxes {
                std::mem::swap(&mut rect.x, &mut rect.y);
            }
        }
        let left = boxes.iter().map(|r| r.x).fold(f64::INFINITY, f64::min);
        let right = boxes.iter().map(|r| r.x + r.w).fold(f64::NEG_INFINITY, f64::max);
        let top = boxes.iter().map(|r| r.y).fold(f64::INFINITY, f64::min);
        let bottom = boxes.iter().map(|r| r.y + r.h).fold(f64::NEG_INFINITY, f64::max);
        for rect in &mut boxes {
            if grow.ends_with('R') {
                rect.x = left + right - rect.x - rect.w;
            }
            if grow.starts_with('b') {
                rect.y = top + bottom - rect.y - rect.h;
            }
        }
        boxes
    }

    fn aborted_seed(&self, children: &[BoxStyle], lines: &[Line], states: &[FittedLine], stop: usize) -> Vec<Xfrm> {
        let mut boxes: Vec<_> = children.iter().map(|c| Xfrm::new(0., 0., c.rect.w, c.rect.h)).collect();
        let first = states.first().map_or(0., |s| s.length);
        let (mut cross, mut origin) = (0., 0.);
        for (index, (line, state)) in lines.iter().zip(states).enumerate() {
            let members = children.get(line.range.clone()).unwrap_or_default();
            let thickness = members.iter().map(|c| if self.column { c.rect.w } else { c.rect.h }).fold(0_f64, f64::max);
            let mut pos = if index == stop { origin } else { first / 2. + state.nominal.offset - state.length / 2. };
            origin = pos;
            for (child, rect) in members.iter().zip(boxes.get_mut(line.range.clone()).unwrap_or_default()) {
                let (length, cross_size) = if self.column { (child.rect.h, child.rect.w) } else { (child.rect.w, child.rect.h) };
                rect.x = pos;
                rect.y = cross + (thickness - cross_size) / 2.;
                pos += length;
            }
            cross += thickness + self.gap;
            if index == stop {
                break;
            }
        }
        boxes
    }

    fn finish_centered(&self, children: &[BoxStyle], search: CenterSearch, flow: f64, grow: &str, work: &mut usize) -> Result<Packing, &'static str> {
        charge(work, children.len().saturating_mul(10))?;
        let CenterSearch { initial, best, scale, exit, normalized } = search;
        let nominal_length = |line: &Line| {
            children.get(line.range.clone()).unwrap_or_default().iter().map(|c| if self.column { c.rect.h } else { c.rect.w }).sum::<f64>()
        };
        let first = best.lines.first().map_or(0., &nominal_length);
        let mut centering = FittedCentering::new(first, scale, exit.stop.filter(|_| normalized).map(|stop| (flow, stop)));
        let mut nominal =
            FittedCentering::new(initial.lines.first().map_or(0., &nominal_length), 1., exit.stop.filter(|_| normalized).map(|stop| (flow, stop)));
        let mut nominal_balance = 0.;
        let mut nominal_balances = Vec::with_capacity(initial.lines.len());
        for (index, line) in initial.lines.iter().enumerate() {
            nominal_balances.push(nominal_balance);
            nominal_balance += nominal.advance(nominal_length(line), 1., index, None).correction;
        }
        let mut states = Vec::with_capacity(best.lines.len());
        for (index, line) in best.lines.iter().enumerate() {
            let length = nominal_length(line);
            let balance = nominal_balances.get(index).copied().unwrap_or(nominal_balance);
            states.push(centering.advance(length, scale, index, Some(balance)));
        }
        // A failed center trial stops before orientation and leaves unvisited
        // children at their original origin. It must not seed a completed layout.
        let mut boxes = if let Some(stop) = exit.stop {
            self.aborted_seed(children, &best.lines, &states, stop)
        } else {
            self.nominal_seed(children, &initial, grow)
        };
        for rect in &mut boxes {
            rect.w *= scale;
            rect.h *= scale;
        }
        let (mut cross, mut origin, mut low, mut high) = (0., 0., f64::INFINITY, f64::NEG_INFINITY);
        let mut previous: Option<FittedLine> = None;
        let mut deferred_paint_abort = false;
        for (index, ((line, normal), &state)) in best.lines.iter().zip(&best.centers).zip(&states).enumerate() {
            let ordinary = (self.column || grow.ends_with('R')) && !exit.cross_failed && exit.stop.is_none();
            let mut proper = if ordinary { normal.offset } else { state.fitted.offset };
            let mut anchor = proper;
            if let Some(prior) = previous.filter(|p| exceeds(p.length, p.reference)) {
                anchor = (first - prior.largest) / 2. + prior.largest_offset + (4. * state.nominal.offset - prior.largest_offset) * scale
                    - 2. * (prior.fitted.next - prior.nominal.next * scale);
            }
            let mut tail = proper - anchor;
            let mut pos = origin;
            if exit.stop.is_some() && exceeds(high - low, flow) && !deferred_paint_abort {
                return Ok(occupied_packing(boxes));
            }
            for (child, rect) in
                children.get(line.range.clone()).unwrap_or_default().iter().zip(boxes.get_mut(line.range.clone()).unwrap_or_default())
            {
                let (length, thickness) = if self.column { (child.rect.h, child.rect.w) } else { (child.rect.w, child.rect.h) };
                rect.x = pos;
                rect.y = cross + (line.thickness - thickness * scale) / 2.;
                pos += length * scale;
            }
            let wanted = first * scale / 2. + proper - line.length / 2.;
            let logical_overflow = exceeds(state.fitted.extent + 2. * state.fitted.pending, flow);
            let painted_overflow = exceeds(high.max(wanted + line.length) - low.min(wanted), flow);
            deferred_paint_abort = exit.stop.is_some()
                && !logical_overflow
                && painted_overflow
                && (!normalized || exceeds(state.reference_adjustment, 0.) || previous.is_some_and(|prior| exceeds(prior.correction, 0.)));
            if deferred_paint_abort {
                proper -= state.reference_adjustment;
                tail -= state.reference_adjustment;
            }
            if !ordinary
                && (exit.stop.is_none() || logical_overflow)
                && painted_overflow
                && let Some(prior) = previous.filter(|prior| exceeds(prior.correction, 0.) && exceeds(state.length, prior.length))
            {
                let partial =
                    origin + (prior.length - state.length) / 2. + (state.reference - prior.reference) * scale / 2. - state.reference_adjustment;
                for rect in boxes.get_mut(line.range.clone()).unwrap_or_default() {
                    rect.x += partial - origin;
                }
                return Ok(occupied_packing(boxes));
            }
            // Fitting can overflow its logical extent before this row reaches
            // final centering. Growing rows still use the carried boundary;
            // following children retain their nominal positions.
            if !ordinary && logical_overflow && (exit.stop.is_none() || (!normalized && painted_overflow)) {
                let adjustment = if exceeds(state.expansion, 0.) {
                    first * scale / 2. + proper - line.length / 2. + state.expansion / 2. - origin
                } else {
                    -state.correction
                };
                for rect in boxes.get_mut(line.range.clone()).unwrap_or_default() {
                    rect.x += adjustment;
                }
                return Ok(occupied_packing(boxes));
            }
            // The current row has working positions before the earlier painted
            // extent is checked. An abort leaves later nominal positions intact.
            if exceeds(high - low, flow) {
                return Ok(occupied_packing(boxes));
            }
            if exit.stop == Some(index) {
                tail += state.correction;
                if let Some(prior) = previous {
                    tail -= prior.fitted.pending - prior.nominal.pending * scale;
                }
                for rect in boxes.get_mut(line.range.clone()).unwrap_or_default() {
                    rect.x -= tail;
                }
                return Ok(occupied_packing(boxes));
            }
            let wanted = first * scale / 2. + proper - line.length / 2.;
            let tail = if exit.cross_failed && index + 1 == best.lines.len() { tail } else { 0. };
            for rect in boxes.get_mut(line.range.clone()).unwrap_or_default() {
                rect.x += wanted - origin - tail;
            }
            origin = wanted;
            low = low.min(wanted - tail);
            high = high.max(wanted - tail + line.length);
            cross += line.thickness + self.gap * scale;
            previous = Some(state);
        }
        if exceeds(high - low, flow) {
            return Ok(occupied_packing(boxes));
        }
        if self.column {
            for rect in &mut boxes {
                std::mem::swap(&mut rect.x, &mut rect.y);
            }
        }
        let mut packed = occupied_packing(boxes);
        packed.complete = true;
        Ok(packed)
    }
}

pub(super) fn pack(node: &Instance<'_>, style: BoxStyle, children: &mut [BoxStyle], refit: bool, work: &mut usize) -> Result<(), &'static str> {
    if children.is_empty() {
        return Ok(());
    }
    let policy = Policy::new(node, style, children.len());
    let frame = style.rect;
    if policy.center && !policy.reverse {
        let packed = policy.fit_centered(children, frame, &node.param("grDir", "tL"), refit, work)?;
        place(node, frame, children, packed);
        return Ok(());
    }
    if policy.center && policy.reverse {
        let packed = policy.fit_reversed(children, frame, &node.param("grDir", "tL"), refit, work)?;
        place(node, frame, children, packed);
        return Ok(());
    }
    let available = if policy.column { frame.h } else { frame.w };
    let fits = |packing: &Packing| !exceeds(packing.width, frame.w) && !exceeds(packing.height, frame.h);
    let mut packed = policy.layout(children, available, 1., PackingPhase::Nominal, work)?;
    if refit || !fits(&packed) {
        let (mut low, mut high) = (0., 1.);
        // Normalize the bracket before the observed ten fitting trials. Each
        // attempt consumes the shared work budget, including extreme extents.
        while !fits(&policy.layout(children, available, high * 0.5, PackingPhase::Fitted, work)?) {
            high *= 0.5;
        }
        packed = policy.layout(children, available, 0., PackingPhase::Fitted, work)?;
        for _ in 0..10 {
            let mid = (low + high) / 2.;
            let candidate = policy.layout(children, available, mid, PackingPhase::Fitted, work)?;
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
    let packed = policy.layout_inner(children, available, 1., PackingPhase::Restored, Some(Restored { allocations, cross_limit }), work)?;
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

    struct NativeCenteredControl<'a> {
        widths: &'a [f64],
        heights: &'a [f64],
        fractions: &'a [f64],
        frame: (f64, f64),
        column: bool,
        grow: &'a str,
        gap: f64,
        lines: &'a [[f64; 4]],
    }

    fn check_native_centered_lines(c: NativeCenteredControl<'_>) {
        let sizes: Vec<_> = c
            .widths
            .iter()
            .zip(c.heights)
            .flat_map(|(&w, &h)| c.fractions.iter().map(move |&f| if c.column { (h, w * f) } else { (w * f, h) }))
            .collect();
        let count = c.fractions.len().to_string();
        let params = [
            ("off", "ctr"),
            ("contDir", "sameDir"),
            ("bkpt", "fixed"),
            ("bkPtFixedVal", count.as_str()),
            ("flowDir", if c.column { "col" } else { "row" }),
            ("grDir", c.grow),
        ];
        let boxes = packed(&sizes, c.frame, &params, c.gap);
        let bounds: Vec<_> = boxes
            .chunks(c.fractions.len())
            .map(|line| {
                let x = line.iter().map(|r| r.x).fold(f64::INFINITY, f64::min);
                let y = line.iter().map(|r| r.y).fold(f64::INFINITY, f64::min);
                let right = line.iter().map(|r| r.x + r.w).fold(f64::NEG_INFINITY, f64::max);
                let bottom = line.iter().map(|r| r.y + r.h).fold(f64::NEG_INFINITY, f64::max);
                [x, y, right - x, bottom - y]
            })
            .collect();
        assert_eq!(bounds.len(), c.lines.len());
        for (row, (actual, expected)) in bounds.iter().zip(c.lines).enumerate() {
            let actual = [actual[0] - bounds[0][0], actual[1] - bounds[0][1], actual[2], actual[3]];
            for (actual, expected) in actual.into_iter().zip(expected) {
                assert!((actual - expected).abs() < 0.001, "row{row}: {actual}, native {expected}");
            }
        }
    }

    #[test]
    fn centered_fits_carry_references_through_held_and_growing_rows() {
        // Original native controls compare complete line bounds, including
        // retained working rows, instead of only the final occupied extent.
        for c in [
            NativeCenteredControl {
                widths: &[19., 19., 34., 27., 43., 72., 159., 111.],
                heights: &[9., 10.7, 12.4, 9., 10.7, 12.4, 9., 10.7],
                fractions: &[1.],
                frame: (81., 88.),
                column: false,
                grow: "tL",
                gap: 2.1,
                lines: &[
                    [0., 0., 7.598158, 3.599113],
                    [0., 4.438855, 7.598158, 4.278938],
                    [-2.999279, 9.557621, 13.596694, 4.958784],
                    [-5.599594, 15.35619, 10.79736, 3.599113],
                    [-5.299181, 19.795088, 17.195763, 4.278959],
                    [-25.148433, 24.913875, 28.792956, 4.958784],
                    [-70.348473, 30.712445, 63.584459, 3.599113],
                    [-42.249962, 87.899975, 44.389108, 4.278916],
                ],
            },
            NativeCenteredControl {
                widths: &[18., 18., 32., 24., 40., 70., 163., 113.],
                heights: &[10., 11.3, 12.6, 10., 11.3, 12.6, 10., 11.3],
                fractions: &[1.],
                frame: (97., 105.),
                column: false,
                grow: "tL",
                gap: 2.2,
                lines: &[
                    [0., 0., 8.33205, 4.628873],
                    [0., 5.647265, 8.33205, 5.230624],
                    [-3.240198, 11.896259, 14.812468, 5.832439],
                    [-4.388664, 18.747068, 11.109356, 4.628873],
                    [-4.97166, 24.394291, 18.515601, 5.230645],
                    [-26.906217, 30.643328, 32.402307, 5.832439],
                    [-94.378359, 37.494094, 75.451155, 4.628916],
                    [-94.378359, 43.141402, 52.306637, 5.230645],
                ],
            },
            NativeCenteredControl {
                widths: &[23., 23., 40., 31., 49., 84., 181., 125.],
                heights: &[10., 11.3, 12.6, 10., 11.3, 12.6, 10., 11.3],
                fractions: &[1.],
                frame: (107., 105.),
                column: false,
                grow: "tL",
                gap: 2.2,
                lines: &[
                    [0., 0., 10.713859, 4.658178],
                    [0., 5.683051, 10.713859, 5.263718],
                    [-3.95947, 11.971599, 18.63282, 5.869323],
                    [-5.863273, 18.865752, 14.440441, 4.658178],
                    [-6.075921, 24.54876, 22.825222, 5.263761],
                    [-30.634715, 30.837308, 39.128878, 5.869323],
                    [-101.675383, 37.731461, 84.313458, 4.658221],
                    [-101.675383, 43.414469, 58.227518, 5.263761],
                ],
            },
        ] {
            check_native_centered_lines(c);
        }
    }

    #[test]
    fn centered_fits_stop_logical_and_painted_overflow_at_their_own_stage() {
        // Original native controls compare complete line bounds, including
        // retained working rows, instead of only the final occupied extent.
        for c in [
            NativeCenteredControl {
                widths: &[67., 86., 127., 41., 48., 113., 26.],
                heights: &[9.7, 10.8, 11.9, 9.7, 10.8, 11.9, 9.7],
                fractions: &[1.],
                frame: (90., 112.),
                column: false,
                grow: "tL",
                gap: 2.6,
                lines: &[
                    [0., 0., 40.828107, 5.910923],
                    [-5.789074, 7.495322, 52.406254, 6.581209],
                    [-48.281237, 15.660973, 77.390636, 7.25156],
                    [-48.281237, 24.496847, 24.98436, 5.910944],
                    [0., 0., 29.250002, 6.581252],
                    [0., 0., 68.859363, 7.25156],
                    [0., 0., 15.843736, 5.910923],
                ],
            },
            NativeCenteredControl {
                widths: &[20., 63., 130., 30., 106., 162., 111., 40.],
                heights: &[9.7, 10.8, 11.9, 9.7, 10.8, 11.9, 9.7, 10.8],
                fractions: &[1.],
                frame: (93., 112.),
                column: false,
                grow: "tL",
                gap: 2.6,
                lines: &[
                    [0., 0., 11.367136, 5.513079],
                    [-12.219754, 6.990831, 35.806665, 6.138256],
                    [-67.219713, 14.606839, 73.886683, 6.763498],
                    [-54.999948, 40.200023, 17.050768, 5.513057],
                    [0., 0., 60.246086, 6.138256],
                    [0., 0., 92.074203, 6.763455],
                    [0., 0., 63.087902, 5.513079],
                    [0., 0., 22.734357, 6.138256],
                ],
            },
            NativeCenteredControl {
                widths: &[31., 46., 72., 105., 229., 163.],
                heights: &[11.3, 12.2, 13.1, 11.3, 12.2, 13.1],
                fractions: &[1.],
                frame: (128., 129.),
                column: false,
                grow: "bR",
                gap: 3.4,
                lines: &[
                    [0., 0., 13.623027, 4.965804],
                    [-3.295901, 6.459963, 20.21483, 5.361301],
                    [-29.508772, 13.315423, 31.640597, 5.756841],
                    [-43.778809, 20.566402, 46.142554, 4.965804],
                    [-118.919421, 27.026365, 100.63477, 5.361301],
                    [-118.919421, 33.881825, 71.630856, 5.756841],
                ],
            },
        ] {
            check_native_centered_lines(c);
        }
    }

    #[test]
    fn centered_fits_preserve_subdivided_working_rows_after_partial_placement() {
        // Original native controls compare complete line bounds, including
        // retained working rows, instead of only the final occupied extent.
        check_native_centered_lines(NativeCenteredControl {
            widths: &[33.532, 46.665, 77.902, 115.443, 160.331, 122.798],
            heights: &[14.04, 13.121, 11.101, 9.153, 12.541, 11.97],
            fractions: &[0.13, 0.22, 0.27, 0.38],
            frame: (143., 62.529),
            column: true,
            grow: "bL",
            gap: 1.9,
            lines: &[
                [0., 0., 11.904721, 4.355074],
                [-2.244372, 5.448332, 14.373649, 6.060698],
                [-29.767757, 10.5823, 20.302831, 10.117987],
                [-45.833184, 15.02587, 27.592459, 14.994132],
                [-62.938837, 18.803762, 38.262867, 20.824483],
                [-63.399489, 69.455967, 80.226111, 15.949414],
            ],
        });
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
    fn reverse_physical_retreat_bounds_pending_growth() {
        // Independent native controls cover an advance smaller than pending growth and no advance.
        let controls = [
            (
                2_usize,
                "tL",
                (790., 350.),
                9.,
                vec![14., 15., 23., 24., 75., 20., 180., 1., 70., 50., 10., 11., 10., 11., 10., 11., 10., 11.],
                vec![0., 0., 37., 32., 72., 72., 43.5, 43.5, 43.5],
            ),
            (
                2_usize,
                "tL",
                (790., 350.),
                9.,
                vec![14., 15., 23., 24., 75., 20., 90., 91., 70., 50., 10., 11., 10., 11., 10., 11., 10., 11.],
                vec![0., 0., 37., 32., 9., 12., 12., 12., 12.],
            ),
            (
                3_usize,
                "tR",
                (880., 340.),
                6.,
                vec![
                    17., 18., 18., 27., 28., 28., 113., 21., 21., 259., 2., 2., 91., 47., 47., 11., 12., 12., 11., 12., 12., 11., 12., 12., 11., 12.,
                    12.,
                ],
                vec![0., 0., -45., -45., -98., -98., -61., -61., -61.],
            ),
            (
                3_usize,
                "tR",
                (880., 340.),
                6.,
                vec![
                    17., 18., 18., 27., 28., 28., 113., 21., 21., 87., 88., 88., 91., 47., 47., 11., 12., 12., 11., 12., 12., 11., 12., 12., 11.,
                    12., 12.,
                ],
                vec![0., 0., -45., -45., -18., -21., -21., -21., -21.],
            ),
        ];
        for (count, direction, frame, height, widths, expected) in controls {
            let count_param = count.to_string();
            let params = [("bkpt", "fixed"), ("bkPtFixedVal", count_param.as_str()), ("off", "ctr"), ("contDir", "revDir"), ("grDir", direction)];
            let sizes: Vec<_> = widths.into_iter().map(|width| (width, height)).collect();
            let actual = packed(&sizes, frame, &params, 0.);
            let centers = relative_line_centers(&actual, count, false);
            assert_eq!(centers.len(), expected.len());
            assert!(centers.iter().zip(expected).all(|(a, b)| (a - b).abs() < 1e-7), "{count} {direction}: {centers:?}");
        }
    }
    #[test]
    fn reverse_physical_retreat_returns_growth_in_two_stages() {
        // Independent native controls vary forward growth, tail widths and pending growth in mirrored flows.
        let controls = [
            (
                2_usize,
                "bL",
                (840., 350.),
                8.,
                vec![15., 16., 26., 27., 83., 22., 210., 1., 121., 56., 10., 11., 10., 11., 10., 11., 10., 11.],
                vec![0., 0., 41., 37., 87., 105.5, 68., 68., 68.],
            ),
            (
                2_usize,
                "bL",
                (840., 350.),
                8.,
                vec![15., 16., 26., 27., 83., 22., 210., 1., 167., 86., 10., 11., 10., 11., 10., 11., 10., 11.],
                vec![0., 0., 41., 37., 87., 124.5, 96.5, 96.5, 96.5],
            ),
            (
                2_usize,
                "bL",
                (840., 350.),
                8.,
                vec![15., 16., 26., 27., 83., 22., 210., 1., 167., 86., 35., 36., 35., 36., 35., 36., 35., 36.],
                vec![0., 0., 41., 37., 87., 124.5, 96.5, 96.5, 96.5],
            ),
            (
                2_usize,
                "bL",
                (840., 350.),
                8.,
                vec![15., 16., 26., 27., 83., 14., 210., 1., 167., 86., 10., 11., 10., 11., 10., 11., 10., 11.],
                vec![0., 0., 41., 37., 89., 130.5, 99.5, 99.5, 99.5],
            ),
            (
                3_usize,
                "tR",
                (920., 360.),
                9.,
                vec![
                    19., 20., 20., 30., 30., 31., 127., 22., 22., 279., 2., 2., 163., 44., 44., 11., 12., 12., 11., 12., 12., 11., 12., 12., 11.,
                    12., 12.,
                ],
                vec![0., 0., -52., -50., -104., -120., -84., -84., -84.],
            ),
            (
                3_usize,
                "tR",
                (920., 360.),
                9.,
                vec![
                    19., 20., 20., 30., 30., 31., 127., 22., 22., 279., 2., 2., 211., 59., 59., 11., 12., 12., 11., 12., 12., 11., 12., 12., 11.,
                    12., 12.,
                ],
                vec![0., 0., -52., -50., -104., -140., -113.5, -113.5, -113.5],
            ),
            (
                3_usize,
                "tR",
                (920., 360.),
                9.,
                vec![
                    19., 20., 20., 30., 30., 31., 127., 22., 22., 279., 2., 2., 211., 59., 59., 29., 30., 30., 29., 30., 30., 29., 30., 30., 29.,
                    30., 30.,
                ],
                vec![0., 0., -52., -50., -104., -140., -113.5, -113.5, -113.5],
            ),
            (
                3_usize,
                "tR",
                (920., 360.),
                9.,
                vec![
                    19., 20., 20., 30., 30., 31., 127., 14., 14., 279., 2., 2., 211., 59., 59., 11., 12., 12., 11., 12., 12., 11., 12., 12., 11.,
                    12., 12.,
                ],
                vec![0., 0., -52., -50., -108., -152., -119.5, -119.5, -119.5],
            ),
        ];
        for (count, direction, frame, height, widths, expected) in controls {
            let count_param = count.to_string();
            let params = [("bkpt", "fixed"), ("bkPtFixedVal", count_param.as_str()), ("off", "ctr"), ("contDir", "revDir"), ("grDir", direction)];
            let sizes: Vec<_> = widths.into_iter().map(|width| (width, height)).collect();
            let actual = packed(&sizes, frame, &params, 0.);
            let centers = relative_line_centers(&actual, count, false);
            assert_eq!(centers.len(), expected.len());
            assert!(centers.iter().zip(expected).all(|(a, b)| (a - b).abs() < 1e-7), "{count} {direction}: {centers:?}");
        }
    }

    #[test]
    fn reverse_borrowed_advance_separates_physical_and_logical_extents() {
        let params = [("bkpt", "fixed"), ("bkPtFixedVal", "2"), ("off", "ctr"), ("contDir", "revDir"), ("grDir", "tR")];
        // Independent native controls keep total row lengths fixed and vary the
        // following first child's width. Both produce the same retained centers.
        for following in [[48., 49.], [83., 14.]] {
            let parts = [[11., 12.], [17., 24.], [61., 12.], [60., 61.], following, [9., 10.], [9., 10.], [9., 10.], [9., 10.]];
            let sizes: Vec<_> = parts.into_iter().flatten().map(|w| (w, 13.)).collect();
            let actual = packed(&sizes, (720., 330.), &params, 0.);
            let centers = relative_line_centers(&actual, 2, false);
            let expected = [0., 0., -29., -28., -23., -23., -23., -23., -23.];
            assert!(centers.iter().zip(expected).all(|(a, b)| (a - b).abs() < 1e-7), "{following:?}: {centers:?}");
        }
        let params = [("bkpt", "fixed"), ("bkPtFixedVal", "2"), ("off", "ctr"), ("contDir", "revDir")];
        let parts = [[18., 19.], [26., 27.], [73., 16.], [63., 64.], [53., 54.], [10., 11.], [10., 11.], [10., 11.], [10., 11.]];
        let sizes: Vec<_> = parts.into_iter().flatten().map(|w| (w, 8.)).collect();
        let actual = packed(&sizes, (730., 310.), &params, 0.);
        let centers = relative_line_centers(&actual, 2, false);
        let expected = [0., 0., 28., 26., 24., 25., 25., 25., 25.];
        assert!(centers.iter().zip(expected).all(|(a, b)| (a - b).abs() < 1e-7), "{centers:?}");
    }

    #[test]
    fn reverse_borrowed_advance_preserves_no_growth_and_three_child_phase() {
        let params = [("bkpt", "fixed"), ("bkPtFixedVal", "3"), ("off", "ctr"), ("contDir", "revDir"), ("grDir", "bL")];
        for (following, own, after, expected) in [
            ([19., 20., 20.], [55., 56., 56.], [43., 44., 44.], vec![0., 0., -12., -12., -49., -49., -55.5, -55.5, -55.5]),
            // This independently observed phase ends after the next forward row.
            // Later reverse retreat remains a separate known full-page residual.
            ([89., 11., 13.], [165., 1., 1.], [99., 15., 17.], vec![0., 0., 34., 35., 60.]),
        ] {
            let parts = [[14., 14., 15.], [22., 22., 23.], following, own, after, [9., 10., 10.], [9., 10., 10.], [9., 10., 10.], [9., 10., 10.]];
            let sizes: Vec<_> = parts.into_iter().flatten().map(|w| (w, 9.)).collect();
            let actual = packed(&sizes, (780., 340.), &params, 0.);
            let centers = relative_line_centers(&actual, 3, false);
            assert!(centers.iter().zip(expected).all(|(a, b)| (a - b).abs() < 1e-7), "{following:?}: {centers:?}");
        }
    }

    #[test]
    fn reverse_current_reference_controls_equal_length_boundary_expansion() {
        let params = [("bkpt", "fixed"), ("bkPtFixedVal", "3"), ("off", "ctr"), ("contDir", "revDir")];
        // Native controls vary early physical growth, then distinguish own-child
        // expansion from following-child lookahead. Reset and delay rows expose
        // the lifetime of the current nominal reference.
        for (early, forward, terminal, delay, expected) in [
            ([43., 44., 44.], [20., 22., 26.], 131., false, [0., 0.]),
            ([40., 41., 50.], [20., 22., 26.], 131., false, [0., 0.]),
            ([32., 32., 67.], [20., 22., 26.], 131., false, [17., 18.]),
            ([32., 32., 67.], [48., 51., 52.], 152.5, false, [0., 0.]),
            ([32., 32., 67.], [20., 22., 26.], 131., true, [17., 18.]),
        ] {
            for (probe, &step) in expected.iter().enumerate() {
                let mut parts = vec![[18., 17., 15.], [4.; 3], [4.; 3], early, [11.; 3], [4.; 3], forward, [11.; 3], [11.; 3]];
                if delay {
                    parts.extend([[11.; 3], [11.; 3]]);
                }
                let terminal_parts = if probe == 0 {
                    let left: f64 = terminal - 84.;
                    [(left / 2.).floor(), left - (left / 2.).floor(), 84.]
                } else {
                    let first: f64 = (terminal / 3.).floor();
                    [first, first, terminal - 2. * first]
                };
                parts.extend([terminal_parts, if probe == 0 { [24., 39., 40.] } else { [85., 9., 9.] }]);
                let sizes: Vec<_> = parts.into_iter().flatten().map(|w| (w, 12.)).collect();
                let actual = packed(&sizes, (700., 320.), &params, 0.);
                let centers = relative_line_centers(&actual, 3, false);
                let n = centers.len();
                assert!((centers[n - 1] - centers[n - 2] - step).abs() < 1e-7, "{early:?}/{probe}/{delay}: {centers:?}");
            }
        }
    }

    #[test]
    fn reverse_current_reference_resets_and_persists_with_mirrored_pairs() {
        let params = [("bkpt", "fixed"), ("bkPtFixedVal", "2"), ("off", "ctr"), ("contDir", "revDir"), ("grDir", "bR")];
        for (early, forward, terminal, delay, expected) in [
            ([105., 106.], [39., 41.], 211_f64, false, [0., 0.]),
            ([91., 120.], [39., 41.], 211., false, [0., 0.]),
            ([61., 150.], [39., 41.], 211., false, [-23., -24.]),
            ([61., 150.], [119., 122.], 241., false, [0., 0.]),
            ([61., 150.], [39., 41.], 211., true, [-23., -24.]),
        ] {
            for (probe, &step) in expected.iter().enumerate() {
                let mut parts = vec![[57., 63.], [8.; 2], [8.; 2], early, [20.; 2], [8.; 2], forward, [20.; 2], [20.; 2]];
                if delay {
                    parts.extend([[20.; 2], [20.; 2]]);
                }
                let terminal_parts = if probe == 0 {
                    [terminal - 173., 173.]
                } else {
                    let first: f64 = (terminal / 2.).floor();
                    [first, terminal - first]
                };
                parts.extend([terminal_parts, if probe == 0 { [40., 140.] } else { [174., 6.] }]);
                let sizes: Vec<_> = parts.into_iter().flatten().map(|w| (w, 11.)).collect();
                let actual = packed(&sizes, (740., 350.), &params, 0.);
                let centers = relative_line_centers(&actual, 2, false);
                let n = centers.len();
                assert!((centers[n - 1] - centers[n - 2] - step).abs() < 1e-7, "{early:?}/{probe}/{delay}: {centers:?}");
            }
        }
    }

    #[test]
    fn reverse_negative_advance_cancels_pending_center_and_logical_growth() {
        let params = [("bkpt", "fixed"), ("bkPtFixedVal", "2"), ("off", "ctr"), ("contDir", "revDir")];
        // Short terminal rows expose the physical boundary; long rows expose
        // the logical reference. Native controls distinguish excess retreat,
        // partial cancellation, and a positive advance with the same prefix.
        for (first, prefix, short_end) in [
            (76., [0., 0., -1., 1., 16., 16., 16., 26.], 90.),
            (82., [0., 0., -1., 1., 22., 24., 24., 32.], 90.),
            (104., [0., 0., -1., 1., 44., 41., 41., 46.], 82.),
        ] {
            for terminal in [32., 250.] {
                let parts =
                    [[13., 19.], [17., 25.], [36., 48.], [13., 21.], [first, 128. - first], [16., 16.], [16., 16.], [terminal / 2.; 2], [180., 20.]];
                let sizes: Vec<_> = parts.into_iter().flatten().map(|w| (w, 10.)).collect();
                let actual = packed(&sizes, (620., 250.), &params, 0.);
                let centers = relative_line_centers(&actual, 2, false);
                for (center, expected) in centers.iter().zip(prefix) {
                    assert!((center - expected).abs() < 1e-7, "{first}/{terminal}: {centers:?}");
                }
                let expected = if terminal == 32. { short_end } else { 39. };
                assert!((centers[8] - expected).abs() < 1e-7, "{first}/{terminal}: {centers:?}");
            }
        }
    }

    #[test]
    fn reverse_negative_advance_cancellation_survives_mirrored_subdivision() {
        let params = [("bkpt", "fixed"), ("bkPtFixedVal", "3"), ("off", "ctr"), ("contDir", "revDir"), ("grDir", "bR")];
        for (first, prefix, short_end) in [
            (80., [0., 0., 1., -0.5, -15.5, -15.5, -15.5, -27.25], -100.25),
            (91., [0., 0., 1., -0.5, -26.5, -29.5, -29.5, -38.25], -100.25),
            (117., [0., 0., 1., -0.5, -52.5, -44.5, -44.5, -52.25], -88.25),
        ] {
            for terminal in [36., 270.] {
                let rest = (140. - first) / 2.;
                let parts = [
                    [10., 12., 14.],
                    [11., 15., 18.],
                    [39., 23., 28.],
                    [9., 12., 15.],
                    [first, rest, rest],
                    [12.; 3],
                    [12.; 3],
                    [terminal / 3.; 3],
                    [200., 10., 15.],
                ];
                let sizes: Vec<_> = parts.into_iter().flatten().map(|w| (w, 11.)).collect();
                let actual = packed(&sizes, (650., 280.), &params, 0.);
                let centers = relative_line_centers(&actual, 3, false);
                for (center, expected) in centers.iter().zip(prefix) {
                    assert!((center - expected).abs() < 1e-7, "{first}/{terminal}: {centers:?}");
                }
                let expected = if terminal == 36. { short_end } else { -47. };
                assert!((centers[8] - expected).abs() < 1e-7, "{first}/{terminal}: {centers:?}");
            }
        }
    }

    #[test]
    fn reverse_deferred_boundary_retains_history_after_neutral_lines() {
        let params = [("bkpt", "fixed"), ("bkPtFixedVal", "3"), ("off", "ctr"), ("contDir", "revDir")];
        for pairs in 0..=2 {
            let mut parts = vec![[12., 12., 11.], [3., 3., 3.], [3., 3., 3.], [30., 30., 41.], [9., 9., 9.], [3., 3., 3.]];
            for _ in 0..pairs {
                parts.extend([[9., 9., 9.], [3., 3., 3.]]);
            }
            parts.extend([[37., 37., 37.], [9., 9., 9.], [9., 9., 9.], [28., 29., 59.], [10., 31., 32.]]);
            let sizes: Vec<_> = parts.into_iter().flatten().map(|w| (w, 10.)).collect();
            let actual = packed(&sizes, (600., 240.), &params, 0.);
            let centers = relative_line_centers(&actual, 3, false);
            let event = 6 + 2 * pairs;
            for (index, expected) in [(event, -30.), (event + 2, -30.), (event + 3, -29.), (event + 4, -16.5)] {
                assert!((centers[index] - expected).abs() < 1e-7, "{pairs}: {centers:?}");
            }
            for (rect, &(w, h)) in actual.iter().zip(&sizes) {
                assert!((rect.w - w).abs() < 1e-7 && (rect.h - h).abs() < 1e-7);
            }
        }
    }

    #[test]
    fn reverse_deferred_boundary_and_exposed_center_have_separate_targets() {
        // Native controls include two independent pending-growth profiles and a
        // further subdivision holdout. A wider intervening boundary absorbs an
        // older floor instead of adding the same physical growth twice.
        let params = [("bkpt", "fixed"), ("bkPtFixedVal", "2"), ("off", "ctr"), ("contDir", "revDir")];
        let cases = [
            (
                [[12., 24.], [15., 27.], [34., 50.], [13., 19.], [106., 26.], [15., 15.], [14., 16.], [15., 15.], [180., 20.]],
                [0., 0., -3., -3., 46., 38., 38., 47., 73.],
            ),
            (
                [[13., 19.], [17., 28.], [36., 52.], [12., 22.], [104., 28.], [15., 15.], [14., 16.], [15., 15.], [180., 20.]],
                [0., 0., -2.5, -0.5, 42., 42.5, 42.5, 46., 82.],
            ),
            (
                [[11., 17.], [14., 24.], [31., 43.], [12., 18.], [91., 25.], [15., 15.], [14., 16.], [15., 15.], [160., 20.]],
                [0., 0., -2., -0.5, 38.5, 36.5, 36.5, 41.25, 71.25],
            ),
        ];
        for (parts, expected) in cases {
            let sizes: Vec<_> = parts.into_iter().flatten().map(|w| (w, 10.)).collect();
            let actual = packed(&sizes, (600., 240.), &params, 0.);
            for (center, expected) in relative_line_centers(&actual, 2, false).into_iter().zip(expected) {
                assert!((center - expected).abs() < 1e-7, "{center} != {expected}");
            }
        }
    }

    #[test]
    fn reverse_deferred_logical_growth_follows_the_exposed_center_target() {
        // Paired long reverse rows expose the logical reference independently
        // from the shorter-row physical-boundary observation.
        let params = [("bkpt", "fixed"), ("bkPtFixedVal", "2"), ("off", "ctr"), ("contDir", "revDir")];
        for (mut parts, center, following) in [
            ([[12., 24.], [15., 27.], [34., 50.], [13., 19.], [106., 26.], [15., 15.], [14., 16.], [115., 115.], [180., 20.]], 47., 47.),
            ([[13., 19.], [17., 28.], [36., 52.], [12., 22.], [104., 28.], [15., 15.], [14., 16.], [115., 115.], [180., 20.]], 46., 49.),
        ] {
            for increase in [0., 20.] {
                parts[7] = [(230. + increase) / 2.; 2];
                let sizes: Vec<_> = parts.into_iter().flatten().map(|w| (w, 10.)).collect();
                let actual = packed(&sizes, (600., 240.), &params, 0.);
                let centers = relative_line_centers(&actual, 2, false);
                assert!((centers[7] - center).abs() < 1e-7);
                assert!((centers[8] - following + increase / 2.).abs() < 1e-7, "{centers:?}");
            }
        }
    }

    #[test]
    fn reversed_fitting_defers_growth_capacity_until_the_second_following_line() {
        // Native prefix controls retain the larger dimensions until the second
        // line after growth appears. Extra neutral lines do not grow it again.
        let params = [("bkpt", "fixed"), ("bkPtFixedVal", "1"), ("off", "ctr"), ("contDir", "revDir"), ("flowDir", "col")];
        for (largest, count, numerator) in [(146., 7, 701.), (146., 8, 701.), (146., 9, 575.), (146., 11, 575.), (126., 7, 812.), (126., 9, 691.)] {
            let lengths = [28., 82., 22., 46., 46., 14., largest, 26., 17., 17., 17.];
            let sizes: Vec<_> = lengths[..count].iter().map(|&length| (10., length)).collect();
            let actual = packed(&sizes, (120., 100.), &params, 0.);
            for (rect, &(w, h)) in actual.iter().zip(&sizes) {
                assert!((rect.w / w - numerator / 1024.).abs() < 1e-7, "{largest}, {count}: {rect:?}");
                assert!((rect.h / h - numerator / 1024.).abs() < 1e-7);
            }
        }
        let lengths = [28., 82., 22., 46., 46., 14., 146., 26., 17.];
        let sizes: Vec<_> = lengths.iter().map(|&length| (6., length)).collect();
        let actual = packed(&sizes, (120., 100.), &params, 0.);
        assert!((actual[0].w / 6. - 575. / 1024.).abs() < 1e-7);
        let unfitted = packed(&sizes, (120., 200.), &params, 0.);
        for (rect, &(w, h)) in unfitted.iter().zip(&sizes) {
            assert!((rect.w - w).abs() < 1e-7 && (rect.h - h).abs() < 1e-7);
        }
    }

    #[test]
    fn reversed_fitting_keeps_retained_physical_extent_separate_from_line_length() {
        // Independently subdivided rows preserve total lengths while changing
        // the physical extent retained from their first reverse line.
        let cases: &[(f64, f64, &[f64], f64)] = &[
            (28., 146., &[0.25, 0.75], 635.),
            (42., 146., &[0.25, 0.75], 608.),
            (28., 146., &[0.2, 0.3, 0.5], 597.),
            (28., 126., &[0.2, 0.3, 0.5], 723.),
            (28., 146., &[0.2, 0.8], 643.),
            (28., 126., &[0.2, 0.8], 792.),
            (42., 146., &[0.2, 0.3, 0.5], 575.),
        ];
        for &(first, largest, parts, numerator) in cases {
            let lengths = [first, 82., 22., 46., 46., 14., largest, 26., 17.];
            let sizes: Vec<_> = lengths.iter().flat_map(|length| parts.iter().map(move |ratio| (length * ratio, 10.))).collect();
            let count = parts.len().to_string();
            let params = [("bkpt", "fixed"), ("bkPtFixedVal", count.as_str()), ("off", "ctr"), ("contDir", "revDir")];
            let actual = packed(&sizes, (100., 180.), &params, 0.);
            for (rect, &(w, h)) in actual.iter().zip(&sizes) {
                assert!((rect.w / w - numerator / 1024.).abs() < 1e-7, "{first}, {largest}, {parts:?}: {rect:?}");
                assert!((rect.h / h - numerator / 1024.).abs() < 1e-7);
            }
        }
    }

    #[test]
    fn reversed_fitting_tests_logical_capacity_before_centering_painted_bounds() {
        // Native column and subdivided-row controls allow the painted union to
        // extend past the frame. The longest line and delayed logical extents
        // decide fitting; the final occupied union decides centering.
        for (largest, count, flow, numerator) in [(142., 7, 100., 721.), (134., 7, 100., 764.), (142., 6, 95., 1024.), (142., 7, 142., 1024.)] {
            let lengths = [34., 94., 20., 38., 38., 16., largest];
            for column in [false, true] {
                let parts: &[f64] = if column { &[1.] } else { &[0.2, 0.25, 0.25, 0.3] };
                let sizes: Vec<_> = lengths[..count]
                    .iter()
                    .flat_map(|length| parts.iter().map(move |ratio| if column { (10., length * ratio) } else { (length * ratio, 10.) }))
                    .collect();
                let per_line = parts.len().to_string();
                let params = [
                    ("bkpt", "fixed"),
                    ("bkPtFixedVal", per_line.as_str()),
                    ("off", "ctr"),
                    ("contDir", "revDir"),
                    ("flowDir", if column { "col" } else { "row" }),
                ];
                let actual = packed(&sizes, if column { (120., flow) } else { (flow, 180.) }, &params, 0.);
                for (rect, &(w, h)) in actual.iter().zip(&sizes) {
                    assert!((rect.w / w - numerator / 1024.).abs() < 1e-7);
                    assert!((rect.h / h - numerator / 1024.).abs() < 1e-7);
                }
                let low = actual.iter().map(|r| if column { r.y } else { r.x }).fold(f64::INFINITY, f64::min);
                let high = actual.iter().map(|r| if column { r.y + r.h } else { r.x + r.w }).fold(f64::NEG_INFINITY, f64::max);
                assert!(high - low > flow);
                assert!(((high + low) / 2. - flow / 2.).abs() < 1e-7);
            }
        }
    }

    #[test]
    fn centered_fitting_uses_logical_extent_and_retains_a_failed_tail_anchor() {
        let sizes = [(11., 31.), (28., 12.), (17., 24.), (29., 18.), (14., 22.)];
        let params = [("bkpt", "fixed"), ("bkPtFixedVal", "2"), ("off", "ctr")];
        let actual = packed(&sizes, (43., 51.), &params, 0.);
        for (rect, (w, h)) in actual.iter().zip(sizes) {
            assert!((rect.w / w - 678. / 1024.).abs() < 1e-7);
            assert!((rect.h / h - 678. / 1024.).abs() < 1e-7);
        }
        for (center, expected) in relative_line_centers(&actual, 2, false).into_iter().zip([0., 0., -3.5]) {
            assert!((center - expected).abs() < 1e-7, "{center} != {expected}");
        }
    }

    #[test]
    fn fitted_centering_retains_extent_until_new_growth_exceeds_it() {
        let params = [("bkpt", "fixed"), ("bkPtFixedVal", "1"), ("off", "ctr")];
        for (middle, advance) in [(58.3, 4.755859375), (58.4, 4.739013671875)] {
            let sizes = [20., 40., 30., 20., middle, 20., 90., 20., 20., 20.].map(|w| (w, 9.));
            let actual = packed(&sizes, (200., 99. * 615.25 / 1024.), &params, 1.);
            let centers = relative_line_centers(&actual, 1, false);
            assert!((centers[8] - centers[6] - advance).abs() < 1e-7);
            assert!((centers[2] + 5.).abs() < 1e-7);
            assert!((actual[0].w / 20. - 615. / 1024.).abs() < 1e-7);
        }
    }

    #[test]
    fn low_scale_alignment_carries_only_correction_not_consumed_by_growth() {
        let widths = [23.4, 46.8, 34.9, 17.3, 61.2, 18.1, 103.7, 14.8, 133.3, 22.6];
        let sizes: Vec<_> = widths.into_iter().enumerate().map(|(i, w)| (w, 7.3 + 1.9 * (i % 4) as f64)).collect();
        let cross = sizes.iter().map(|s| s.1).sum::<f64>() + 1.3 * 9.;
        let params = [("bkpt", "fixed"), ("bkPtFixedVal", "1"), ("off", "ctr")];
        let actual = packed(&sizes, (200., cross * 359.75 / 1024.), &params, 1.3);
        let centers = relative_line_centers(&actual, 1, false);
        assert!((actual[0].w / widths[0] - 719. / 2048.).abs() < 1e-7);
        assert!((centers[6] + 14.6219482421875).abs() < 1e-7);
        assert!((centers[7] - centers[6] - 1.8410888671875).abs() < 1e-7);
        assert!((centers[8] - centers[7] - 2.529638671875).abs() < 1e-7);
    }

    #[test]
    fn centering_flow_abort_precedes_pending_drain_and_orientation() {
        let sizes = [20., 40., 20., 20., 80., 20.].map(|w| (w, 10.));
        let params = [("bkpt", "fixed"), ("bkPtFixedVal", "1"), ("off", "ctr")];
        let actual = packed(&sizes, (43., 100.), &params, 0.);
        let centers = relative_line_centers(&actual, 1, false);
        assert!((actual[0].w / 20. - 463. / 1024.).abs() < 1e-7);
        assert!((centers[4] + 11.435546875).abs() < 1e-7);
        assert!((centers[5] + 31.7822265625).abs() < 1e-7);
        let mut reflected = params.to_vec();
        reflected.push(("grDir", "bR"));
        let mirrored = packed(&sizes, (43., 100.), &reflected, 0.);
        assert_eq!(actual, mirrored);
    }

    #[test]
    fn fitted_held_reference_rebinds_and_releases_cancelled_pending_growth() {
        let widths = [41., 33., 76., 18., 42., 90., 49., 108., 122., 57., 27., 69.];
        let sizes: Vec<_> = widths.into_iter().enumerate().map(|(i, w)| (w, 5. + (i * 7 % 13) as f64)).collect();
        let params = [("bkpt", "fixed"), ("bkPtFixedVal", "1"), ("off", "ctr")];
        let actual = packed(&sizes, (200., 57.397395859375), &params, 2.9);
        let expected = [
            0.,
            0.,
            -16.099609375,
            -14.176513671875,
            -11.1131591796875,
            -11.1131591796875,
            -11.1131591796875,
            -11.1131591796875,
            -20.6368408203125,
            -18.01812744140625,
            -16.79278564453125,
            -16.79278564453125,
        ];
        for (center, expected) in relative_line_centers(&actual, 1, false).into_iter().zip(expected) {
            assert!((center - expected).abs() < 1e-7, "{center} != {expected}");
        }
        for (rect, size) in actual.iter().zip(sizes) {
            assert!((rect.w / size.0 - 717. / 2048.).abs() < 1e-7);
        }
    }

    #[test]
    fn fitted_logical_abort_uses_carried_extent_and_preserves_following_nominal_positions() {
        let widths = [47., 39., 84., 21., 49., 98., 55., 116., 138., 65., 29., 73.];
        let sizes: Vec<_> = widths.into_iter().enumerate().map(|(i, w)| (w, 5. + (i * 7 % 13) as f64)).collect();
        let params = [("bkpt", "fixed"), ("bkPtFixedVal", "1"), ("off", "ctr")];
        let actual = packed(&sizes, (43.42291259765625, 43.952473984375), &params, 2.9);
        let centers = relative_line_centers(&actual, 1, false);
        assert!((centers[8] + 22.1077880859375).abs() < 1e-7);
        assert!((centers[9] - 2.66259765625).abs() < 1e-7);
        assert!((actual[9].y - actual[0].y - 115.1).abs() < 1e-7);
        assert!((actual[10].y - actual[9].y - 18.9).abs() < 1e-7);
        assert!((actual[11].y - actual[10].y - 12.9).abs() < 1e-7);
        for (rect, size) in actual.iter().zip(sizes) {
            assert!((rect.h / size.1 - 549. / 2048.).abs() < 1e-7);
        }
    }

    #[test]
    fn failed_center_seed_leaves_unvisited_children_at_the_original_origin() {
        let sizes = [18., 112., 29., 17., 77., 21., 93., 19.].map(|w| (w, 9.));
        let params = [("bkpt", "fixed"), ("bkPtFixedVal", "1"), ("off", "ctr")];
        let actual = packed(&sizes, (39., 200.), &params, 1.3);
        assert!((actual[2].x - actual[0].x + 58.5205078125).abs() < 1e-7);
        for rect in &actual[3..] {
            assert!((rect.x - actual[0].x).abs() < 1e-7);
            assert!((rect.y - actual[0].y).abs() < 1e-7);
        }
        assert!((actual[2].y - actual[1].y - 2.52470703125).abs() < 1e-7);
    }

    #[test]
    fn reverse_nominal_reference_survives_intervening_short_lines() {
        let params = [("bkpt", "fixed"), ("bkPtFixedVal", "1"), ("off", "ctr"), ("contDir", "revDir")];
        for first in [18., 30.] {
            for pairs in 0..=2 {
                let mut widths = vec![first, 66., 12.];
                for _ in 0..pairs {
                    widths.extend([42., 42.]);
                }
                widths.extend([6., 126., 12.]);
                let sizes: Vec<_> = widths.into_iter().map(|w| (w, 10.)).collect();
                let actual = packed(&sizes, (400., 180.), &params, 0.);
                let centers = relative_line_centers(&actual, 1, false);
                let half_growth = (66. - first) / 2.;
                let mut expected = vec![0., 0., half_growth];
                for pair in 0..pairs {
                    expected.extend([if pair == 0 { half_growth } else { 0. }, 0.]);
                }
                expected.extend([if pairs == 0 { half_growth } else { 0. }, 60., 96. - first]);
                for (center, expected) in centers.into_iter().zip(expected) {
                    assert!((center - expected).abs() < 1e-7, "{first}/{pairs}: {center} != {expected}");
                }
            }
        }
    }

    #[test]
    fn reverse_fitted_and_restored_tails_keep_separate_alignment_history() {
        let xml = crate::xml::parse(b"<layoutNode/>").unwrap();
        let mut node = Instance::new(&xml.root);
        node.alg = "snake".into();
        node.params = [("bkpt", "fixed"), ("bkPtFixedVal", "1"), ("off", "ctr"), ("contDir", "revDir"), ("grDir", "bR")]
            .map(|(k, v)| (k.into(), v.into()))
            .into_iter()
            .collect();
        let style = BoxStyle::new(Xfrm::new(0., 0., 90., 65.));
        for (first, scale) in [(22., 663. / 2048.), (38., 725. / 2048.)] {
            let mut children: Vec<_> =
                [first, 74., 18., 44., 44., 10., 158., 22.].into_iter().map(|w| BoxStyle::new(Xfrm::new(0., 0., w, 10.))).collect();
            pack(&node, style, &mut children, false, &mut 0).unwrap();
            let fitted: Vec<_> = children.iter().map(|c| c.rect).collect();
            // The final row overflows before centering and growth reflection.
            // Preserve the original fitting fixture with its observed output.
            let tail = fitted.last().unwrap();
            assert!((tail.x - 45. - 120. * scale).abs() < 1e-7);
            assert!((tail.y - 32.5 - 30. * scale).abs() < 1e-7);
            assert!((tail.w - 22. * scale).abs() < 1e-7);
            assert!((tail.h - 10. * scale).abs() < 1e-7);

            // An explicit restored pass uses unit scale with these dimensions.
            // It completes placement using the restored reference lifetime.
            let allocations = children.clone();
            repack(&node, style, &mut children, &allocations, &mut 0).unwrap();
            let restored = children.last().unwrap().rect;
            assert!((restored.x - 45. - 10. * scale).abs() < 1e-7);
            assert!((restored.y - 32.5 + 40. * scale).abs() < 1e-7);
            for (child, before) in children.iter().zip(fitted) {
                assert_eq!((child.rect.w, child.rect.h), (before.w, before.h));
            }
        }
    }

    #[test]
    fn reverse_cross_only_fit_preserves_its_last_trial_alignment() {
        let xml = crate::xml::parse(b"<layoutNode/>").unwrap();
        let mut node = Instance::new(&xml.root);
        node.alg = "snake".into();
        node.params = [("bkpt", "fixed"), ("bkPtFixedVal", "1"), ("off", "ctr"), ("contDir", "revDir"), ("grDir", "bR")]
            .map(|(k, v)| (k.into(), v.into()))
            .into_iter()
            .collect();
        let style = BoxStyle::new(Xfrm::new(0., 0., 400., 65.));
        for first in [22., 38.] {
            let mut children: Vec<_> =
                [first, 74., 18., 44., 44., 10., 158., 22.].into_iter().map(|w| BoxStyle::new(Xfrm::new(0., 0., w, 10.))).collect();
            pack(&node, style, &mut children, false, &mut 0).unwrap();
            let tail = children.last().unwrap().rect;
            assert!((tail.x - 139.875).abs() < 1e-7 && tail.y.abs() < 1e-7);
            assert!((tail.w - 17.875).abs() < 1e-7 && (tail.h - 8.125).abs() < 1e-7);
            let allocations = children.clone();
            repack(&node, style, &mut children, &allocations, &mut 0).unwrap();
            let tail = children.last().unwrap().rect;
            assert!((tail.x - 208.125).abs() < 1e-7 && tail.y.abs() < 1e-7);
            assert!((tail.w - 17.875).abs() < 1e-7 && (tail.h - 8.125).abs() < 1e-7);
        }
    }

    #[test]
    fn reverse_logical_fit_keeps_the_raw_turn_before_growth_reflection() {
        for (first, scale) in [(18., 101. / 256.), (30., 877. / 2048.)] {
            let sizes = [first, 66., 12., 42., 42., 6., 126., 12.].map(|w| (w, 10.));
            for grow in ["tL", "tR", "bL", "bR"] {
                let params = [("bkpt", "fixed"), ("bkPtFixedVal", "1"), ("off", "ctr"), ("contDir", "revDir"), ("grDir", grow)];
                let actual = packed(&sizes, (90., 100.), &params, 0.);
                let half_growth = (66. - first) / 2.;
                for (center, expected) in
                    relative_line_centers(&actual, 1, false).into_iter().zip([0., 0., half_growth, half_growth, 0., 0., 60., 177.])
                {
                    assert!((center - expected * scale).abs() < 1e-7);
                }
                for (index, rect) in actual.iter().enumerate() {
                    assert!((rect.y - actual[0].y - index as f64 * 10. * scale).abs() < 1e-7);
                    assert!((rect.h - 10. * scale).abs() < 1e-7);
                }
            }
        }
    }

    #[test]
    fn reverse_final_trial_preserves_nominal_or_unvisited_positions() {
        let params = [("bkpt", "fixed"), ("bkPtFixedVal", "1"), ("off", "ctr"), ("contDir", "revDir"), ("grDir", "tR")];
        for (first, scale, seed_x, seed_y) in [(18., 898. / 2048., 0., 0.), (30., 975. / 2048., 132.5, 80.)] {
            let sizes = [first, 66., 12., 42., 42., 6., 126., 12., 17.].map(|w| (w, 10.));
            let actual = packed(&sizes, (100., 180.), &params, 0.);
            let tail = actual.last().unwrap();
            assert!((tail.x - actual[0].x - seed_x).abs() < 1e-7);
            assert!((tail.y - actual[0].y - seed_y).abs() < 1e-7);
            assert!((tail.w - 17. * scale).abs() < 1e-7);
            assert!((tail.h - 10. * scale).abs() < 1e-7);
        }
    }

    #[test]
    fn reverse_nominal_reference_advances_before_the_reflected_boundary() {
        // Original native controls distinguish both sides of the intermediate
        // boundary and the final reflected limit, with a retained short first row.
        let params = [("bkpt", "fixed"), ("bkPtFixedVal", "1"), ("off", "ctr"), ("contDir", "revDir")];
        for (first, following, next, tail) in [
            (14., 64., 2., 1.),
            (14., 84., 22., 11.),
            (14., 88., 28., 15.),
            (14., 108., 68., 45.),
            (14., 110., 72., 48.),
            (14., 122., 78., 57.),
            (34., 64., 2., 1.),
            (34., 74., 12., 6.),
            (34., 78., 18., 10.),
            (34., 88., 38., 25.),
            (34., 90., 42., 28.),
            (34., 102., 48., 37.),
        ] {
            let sizes = [first, 62., 10., 34., 34., 6., following, 26., 12., 12.].map(|w| (w, 10.));
            let actual = packed(&sizes, (400., 180.), &params, 0.);
            let expected = [0., 0., (62. - first) / 2., (62. - first) / 2., 0., 0., following - 62., next, tail, tail];
            for (center, expected) in relative_line_centers(&actual, 1, false).into_iter().zip(expected) {
                assert!((center - expected).abs() < 1e-7, "{first}/{following}: {center} != {expected}");
            }
        }
    }

    #[test]
    fn reverse_reference_advance_uses_the_updated_extent_and_nominal_reference() {
        let params = [("bkpt", "fixed"), ("bkPtFixedVal", "1"), ("off", "ctr"), ("contDir", "revDir")];
        for (following, next, tail) in [(90., 12., 11.), (111., 33., 21.5), (115., 39., 25.5), (136., 81., 57.), (138., 85., 60.), (146., 89., 66.)] {
            let sizes = [18., 68., 38., 8., following, 26., 12., 12.].map(|w| (w, 10.));
            let actual = packed(&sizes, (400., 180.), &params, 0.);
            let expected = [0., 0., 25., 35., following - 78., next, tail, tail];
            for (center, expected) in relative_line_centers(&actual, 1, false).into_iter().zip(expected) {
                assert!((center - expected).abs() < 1e-7, "{following}: {center} != {expected}");
            }
        }
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
