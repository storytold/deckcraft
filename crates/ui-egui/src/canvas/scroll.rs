use super::*;

/// Thickness of the canvas scroll bars, in points.
const SCROLL_BAR: f32 = 10.0;

/// The canvas scroll bar along `axis` (0 across, 1 down): its track, its thumb for `offset`, and
/// how far the thumb can travel. `None` when the slide fits that way. `inset` keeps the rulers'
/// strips clear.
pub(super) fn scroll_bar(avail: Rect, inset: f32, overflow: egui::Vec2, offset: egui::Vec2, axis: usize) -> Option<(Rect, Rect, f32)> {
    let over = overflow[axis];
    if !over.is_finite() || over <= 0.0 {
        return None;
    }
    // Both bars showing leave the corner between them empty.
    let corner = if overflow[1 - axis] > 0.0 { SCROLL_BAR } else { 0.0 };
    let track = if axis == 0 {
        Rect::from_min_max(pos2(avail.min.x + inset, avail.max.y - SCROLL_BAR), pos2(avail.max.x - corner, avail.max.y))
    } else {
        Rect::from_min_max(pos2(avail.max.x - SCROLL_BAR, avail.min.y + inset), pos2(avail.max.x, avail.max.y - corner))
    };
    let len = track.size()[axis].max(0.0);
    let view = avail.size()[axis];
    let thumb_len = (len * view / (view + over)).max(SCROLL_BAR * 2.0).min(len);
    let travel = (len - thumb_len).max(0.0);
    let at = travel * (offset[axis] / over).clamp(0.0, 1.0);
    let mut thumb = track;
    thumb.min[axis] = track.min[axis] + at;
    thumb.max[axis] = thumb.min[axis] + thumb_len;
    Some((track, thumb, travel))
}

/// The canvas `avail` in texture pixels (`pixel`: one pixel's size in points) from the slide's
/// top-left corner.
pub(super) fn visible_region(avail: Rect, slide: Rect, pixel: egui::Vec2) -> deckcraft_render::Region {
    let px = |v: f32, k: f32| if v.is_finite() && k > 0.0 { (v / k) as f64 } else { 0.0 };
    let (x0, y0) = (px(avail.min.x - slide.min.x, pixel.x).floor(), px(avail.min.y - slide.min.y, pixel.y).floor());
    let (x1, y1) = (px(avail.max.x - slide.min.x, pixel.x).ceil(), px(avail.max.y - slide.min.y, pixel.y).ceil());
    let lim = i32::MAX as f64 / 4.0;
    let (x0, y0) = (x0.clamp(-lim, lim), y0.clamp(-lim, lim));
    deckcraft_render::Region {
        x: x0 as i32,
        y: y0 as i32,
        w: (x1 - x0).clamp(1.0, u32::MAX as f64) as u32,
        h: (y1 - y0).clamp(1.0, u32::MAX as f64) as u32,
    }
}
