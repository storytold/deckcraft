//! Clipping regions for EMF and WMF (MS-EMF 2.3.12, MS-WMF 2.3.5).
//!
//! A region is a set of axis-aligned rectangles in device units: `None` is the default clip (the
//! whole surface), `Some([])` draws nothing, and `Some(rects)` is their union. That is how GDI
//! stores a region (`RGNDATA` is a list of rectangles). Counts are capped so a hostile file cannot
//! grow the set without bound; past the cap, extra rectangles are dropped, which can only hide
//! drawing.

/// Rectangles kept from one region. Past this, further rectangles are dropped.
pub(crate) const MAX_CLIP_RECTS: usize = 64;

pub(crate) const RGN_AND: u32 = 1;
pub(crate) const RGN_OR: u32 = 2;
pub(crate) const RGN_XOR: u32 = 3;
pub(crate) const RGN_DIFF: u32 = 4;
pub(crate) const RGN_COPY: u32 = 5;

/// A rectangle `[x0, y0, x1, y1]` with a positive area, or none.
pub(crate) fn normalize(rect: [f64; 4]) -> Option<[f64; 4]> {
    let [l, t, r, b] = rect;
    if ![l, t, r, b].iter().all(|v| v.is_finite()) {
        return None;
    }
    let (x0, x1) = (l.min(r), l.max(r));
    let (y0, y1) = (t.min(b), t.max(b));
    (x1 - x0 > 1e-6 && y1 - y0 > 1e-6).then_some([x0, y0, x1, y1])
}

fn intersection(a: [f64; 4], b: [f64; 4]) -> Option<[f64; 4]> {
    let (x0, y0) = (a[0].max(b[0]), a[1].max(b[1]));
    let (x1, y1) = (a[2].min(b[2]), a[3].min(b[3]));
    (x1 - x0 > 1e-6 && y1 - y0 > 1e-6 && [x0, y0, x1, y1].iter().all(|v| v.is_finite())).then_some([x0, y0, x1, y1])
}

/// `a` with `b` removed, as up to four rectangles.
fn subtract_one(a: [f64; 4], b: [f64; 4]) -> Vec<[f64; 4]> {
    let Some(i) = intersection(a, b) else { return vec![a] };
    let mut out = Vec::new();
    if i[1] > a[1]
        && let Some(r) = normalize([a[0], a[1], a[2], i[1]])
    {
        out.push(r);
    }
    if i[3] < a[3]
        && let Some(r) = normalize([a[0], i[3], a[2], a[3]])
    {
        out.push(r);
    }
    if i[0] > a[0]
        && let Some(r) = normalize([a[0], i[1], i[0], i[3]])
    {
        out.push(r);
    }
    if i[2] < a[2]
        && let Some(r) = normalize([i[2], i[1], a[2], i[3]])
    {
        out.push(r);
    }
    out
}

fn cap(mut rects: Vec<[f64; 4]>) -> Vec<[f64; 4]> {
    rects.truncate(MAX_CLIP_RECTS);
    rects
}

fn subtract_all(subject: Vec<[f64; 4]>, cuts: &[[f64; 4]]) -> Vec<[f64; 4]> {
    let mut cur = subject;
    for cut in cuts {
        let mut next = Vec::new();
        for s in cur {
            for piece in subtract_one(s, *cut) {
                if next.len() >= MAX_CLIP_RECTS {
                    return next;
                }
                next.push(piece);
            }
        }
        cur = next;
    }
    cur
}

/// Intersects the current region with one rectangle.
pub(crate) fn intersect(current: Option<Vec<[f64; 4]>>, rect: [f64; 4]) -> Option<Vec<[f64; 4]>> {
    let Some(rect) = normalize(rect) else { return Some(Vec::new()) };
    Some(match current {
        None => vec![rect],
        Some(rects) => cap(rects.iter().filter_map(|r| intersection(*r, rect)).collect()),
    })
}

/// Removes one rectangle. `surface` is the device bounds, used when the clip is still the default
/// (the whole surface) so the remainder can be named.
pub(crate) fn exclude(current: Option<Vec<[f64; 4]>>, rect: [f64; 4], surface: Option<[f64; 4]>) -> Option<Vec<[f64; 4]>> {
    let Some(rect) = normalize(rect) else { return current };
    match current {
        Some(rects) => Some(subtract_all(rects, &[rect])),
        None => surface.and_then(normalize).map(|s| subtract_all(vec![s], &[rect])),
    }
}

/// Combines `incoming` with the current region in a region mode (`RGN_*`). An unknown mode leaves
/// the region as it was. `RGN_COPY` replaces it. An `RGN_OR` or `RGN_XOR` against the default clip
/// stays the default clip: the whole surface already covers the incoming rectangles.
pub(crate) fn combine(current: Option<Vec<[f64; 4]>>, incoming: Vec<[f64; 4]>, mode: u32, surface: Option<[f64; 4]>) -> Option<Vec<[f64; 4]>> {
    let incoming: Vec<[f64; 4]> = cap(incoming.iter().copied().filter_map(normalize).collect());
    match mode {
        RGN_COPY => Some(incoming),
        RGN_AND => match current {
            None => Some(incoming),
            Some(rects) => {
                let mut out = Vec::new();
                for a in &rects {
                    for b in &incoming {
                        if out.len() >= MAX_CLIP_RECTS {
                            return Some(out);
                        }
                        if let Some(r) = intersection(*a, *b) {
                            out.push(r);
                        }
                    }
                }
                Some(out)
            }
        },
        RGN_OR => match current {
            None => None,
            Some(mut rects) => {
                rects.extend(incoming);
                Some(cap(rects))
            }
        },
        RGN_DIFF => match current {
            Some(rects) => Some(subtract_all(rects, &incoming)),
            None => surface.and_then(normalize).map(|s| subtract_all(vec![s], &incoming)),
        },
        RGN_XOR => {
            let base = current?;
            let mut out = subtract_all(base.clone(), &incoming);
            let rest = subtract_all(incoming, &base);
            for r in rest {
                if out.len() >= MAX_CLIP_RECTS {
                    break;
                }
                out.push(r);
            }
            Some(out)
        }
        _ => current,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intersect_shrinks_to_the_overlap() {
        let r = intersect(None, [0.0, 0.0, 100.0, 50.0]);
        let r = intersect(r, [40.0, 10.0, 80.0, 40.0]);
        assert_eq!(r, Some(vec![[40.0, 10.0, 80.0, 40.0]]));
        assert_eq!(intersect(r, [0.0, 0.0, 10.0, 10.0]), Some(vec![]));
        assert_eq!(intersect(Some(vec![[0.0, 0.0, 10.0, 20.0]]), [40.0, 0.0, 80.0, 20.0]), Some(vec![]));
    }

    #[test]
    fn modes_copy_and_or_diff_and_xor() {
        let a = Some(vec![[0.0, 0.0, 100.0, 40.0]]);
        assert_eq!(combine(a.clone(), vec![[10.0, 10.0, 30.0, 20.0]], RGN_COPY, None), Some(vec![[10.0, 10.0, 30.0, 20.0]]));
        let and = combine(a.clone(), vec![[50.0, 0.0, 150.0, 40.0]], RGN_AND, None);
        assert_eq!(and, Some(vec![[50.0, 0.0, 100.0, 40.0]]));
        let or = combine(a.clone(), vec![[200.0, 0.0, 210.0, 10.0]], RGN_OR, None).unwrap();
        assert_eq!(or.len(), 2);
        let diff = combine(a.clone(), vec![[20.0, 0.0, 40.0, 40.0]], RGN_DIFF, None).unwrap();
        assert_eq!(diff, vec![[0.0, 0.0, 20.0, 40.0], [40.0, 0.0, 100.0, 40.0]]);
        let xor = combine(a, vec![[80.0, 0.0, 120.0, 40.0]], RGN_XOR, None).unwrap();
        assert!(xor.contains(&[0.0, 0.0, 80.0, 40.0]));
        assert!(xor.contains(&[100.0, 0.0, 120.0, 40.0]));
    }

    #[test]
    fn excluding_from_the_default_clip_uses_the_surface() {
        let r = exclude(None, [10.0, 0.0, 20.0, 40.0], Some([0.0, 0.0, 40.0, 40.0])).unwrap();
        assert_eq!(r, vec![[0.0, 0.0, 10.0, 40.0], [20.0, 0.0, 40.0, 40.0]]);
        assert_eq!(exclude(None, [10.0, 0.0, 20.0, 40.0], None), None);
    }

    #[test]
    fn a_zero_area_rectangle_clears_an_intersection() {
        assert_eq!(intersect(Some(vec![[0.0, 0.0, 10.0, 10.0]]), [5.0, 5.0, 5.0, 8.0]), Some(vec![]));
    }
}
