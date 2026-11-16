//! Which part of a Morph frame differs from the one before it, so the show draws and uploads only
//! that part: moving one object across a slide changes a few percent of the picture.

use deckcraft_geom::Rect;

use crate::morph::MorphFrame;

/// How a Morph frame differs from the previous one ([`MorphFrame::changed_since`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Change {
    /// The same picture.
    Nothing,
    /// Only this region of the slide (points) differs; the rest is as before.
    Region(Rect),
    /// Anything may differ: draw the whole frame.
    Everything,
}

impl MorphFrame {
    /// How this frame differs from `prev`, the frame drawn just before it for the same pair of
    /// slides. Cautious: whenever what changed is not a shape with a known [`MorphFrame::reach`]
    /// (text, effects, groups, a different set of shapes, a backdrop that cross-fades…) the answer
    /// is [`Change::Everything`].
    pub fn changed_since(&self, prev: &MorphFrame) -> Change {
        let n = self.shapes.len();
        if prev.shapes.len() != n
            || self.reach.len() != n
            || prev.reach.len() != n
            || !self.text.is_empty()
            || !prev.text.is_empty()
            || (self.mix != prev.mix && !(self.same_backdrop && prev.same_backdrop))
        {
            return Change::Everything;
        }
        let mut region: Option<Rect> = None;
        for (i, ((s, old), (p, was_old))) in self.shapes.iter().zip(&prev.shapes).enumerate() {
            // An outline morphing between two geometries is drawn from the progress alone.
            let morphing = self.mix != prev.mix && self.paths.iter().any(|m| m.id == s.id);
            if old == was_old && s == p && self.opacity_of(s.id) == prev.opacity_of(p.id) && !morphing {
                continue;
            }
            let (Some(Some(now)), Some(Some(before))) = (self.reach.get(i), prev.reach.get(i)) else { return Change::Everything };
            region = Some(region.map_or(now.union(*before), |r| r.union(now.union(*before))));
        }
        region.map_or(Change::Nothing, Change::Region)
    }
}
