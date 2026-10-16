//! Where a file dropped onto the window lands.
//!
//! A picture dropped on the slide is centred on the pointer. Anywhere else in the window (the
//! ribbon, the thumbnail pane) it keeps the centred insert. A presentation still opens.

// Only the macOS drop path reads the AppKit pointer; elsewhere these helpers are test-only.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use egui::{Pos2, Rect};

/// Cocoa reports the pointer from the bottom-left of the main display, in screen points.
/// The window origin egui stores is the top-left of the content view, in those points divided
/// by the interface zoom.
pub(crate) fn cocoa_point_to_window(mouse_x: f32, mouse_y: f32, screen_height: f32, zoom: f32, inner_min: Pos2) -> Option<Pos2> {
    let finite = [mouse_x, mouse_y, screen_height, zoom, inner_min.x, inner_min.y].iter().copied().all(|v| v.is_finite());
    if finite && screen_height > 0.0 && zoom > 0.0 {
        let x = mouse_x / zoom - inner_min.x;
        let y = (screen_height - mouse_y) / zoom - inner_min.y;
        (x.is_finite() && y.is_finite()).then_some(Pos2::new(x, y))
    } else {
        None
    }
}

/// The slide point under `pos`, when `pos` is over the canvas. The pasteboard around the slide
/// counts: a drop there places the picture off the slide, where Normal view can still select it.
pub(crate) fn slide_anchor(canvas: Option<Rect>, slide: Option<Rect>, scale: f32, pos: Pos2) -> Option<(f64, f64)> {
    let (canvas, slide) = (canvas?, slide?);
    if !scale.is_finite() || scale <= 0.0 || !canvas.contains(pos) {
        return None;
    }
    let x = f64::from(pos.x - slide.min.x) / f64::from(scale);
    let y = f64::from(pos.y - slide.min.y) / f64::from(scale);
    (x.is_finite() && y.is_finite()).then_some((x, y))
}

/// The pointer in window points while a file is dragged in.
///
/// On macOS a Finder drag does not move egui's pointer, so the position comes from AppKit.
/// The egui pointer is the fallback for the other platforms, where the drag does move it.
pub(crate) fn drop_pointer(ctx: &egui::Context) -> Option<Pos2> {
    #[cfg(target_os = "macos")]
    if let Some(p) = macos_pointer_in_window(ctx) {
        return Some(p);
    }
    ctx.input(|i| i.pointer.hover_pos())
}

#[cfg(target_os = "macos")]
fn macos_pointer_in_window(ctx: &egui::Context) -> Option<Pos2> {
    let mtm = objc2::MainThreadMarker::new()?;
    let primary = objc2_app_kit::NSScreen::screens(mtm).firstObject()?;
    let height = primary.frame().size.height as f32;
    let mouse = objc2_app_kit::NSEvent::mouseLocation();
    let inner = ctx.input(|i| i.viewport().inner_rect)?;
    cocoa_point_to_window(mouse.x as f32, mouse.y as f32, height, ctx.zoom_factor(), inner.min)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cocoa_points_flip_to_the_window() {
        let inner = Pos2::new(100.0, 50.0);
        let p = cocoa_point_to_window(150.0, 800.0, 1000.0, 1.0, inner).unwrap();
        assert_eq!(p, Pos2::new(50.0, 150.0));
        // Interface zoom 2: the same Cocoa point is half as far, in egui points, from an origin
        // that is already stored in those points.
        let zoomed_origin = Pos2::new(50.0, 25.0);
        let z = cocoa_point_to_window(150.0, 800.0, 1000.0, 2.0, zoomed_origin).unwrap();
        assert_eq!(z, Pos2::new(25.0, 75.0));
        assert!(cocoa_point_to_window(0.0, 0.0, 0.0, 1.0, inner).is_none());
        assert!(cocoa_point_to_window(f32::NAN, 0.0, 100.0, 1.0, inner).is_none());
    }

    #[test]
    fn the_anchor_is_the_slide_point_under_the_canvas() {
        let canvas = Rect::from_min_size(Pos2::new(200.0, 40.0), egui::vec2(800.0, 500.0));
        let slide = Rect::from_min_size(Pos2::new(300.0, 100.0), egui::vec2(400.0, 225.0));
        assert_eq!(slide_anchor(Some(canvas), Some(slide), 0.5, Pos2::new(300.0, 100.0)), Some((0.0, 0.0)));
        assert_eq!(slide_anchor(Some(canvas), Some(slide), 0.5, Pos2::new(400.0, 200.0)), Some((200.0, 200.0)));
        // On the pasteboard, still a slide point.
        assert_eq!(slide_anchor(Some(canvas), Some(slide), 0.5, Pos2::new(250.0, 80.0)), Some((-100.0, -40.0)));
        // The ribbon is outside the canvas, so the picture stays centred.
        assert_eq!(slide_anchor(Some(canvas), Some(slide), 0.5, Pos2::new(20.0, 10.0)), None);
        assert_eq!(slide_anchor(Some(canvas), Some(slide), 0.0, Pos2::new(400.0, 200.0)), None);
    }
}
