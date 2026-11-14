//! #1952: transition and animation tile names must fit their tile: shrunk, then split over two lines
//! below the picture or icon, never merely clipped.

use deckcraft_model::anim::AnimClass;
use deckcraft_ui_egui::ribbon::{paint_anim_tile, paint_transition_tile};
use egui::{Rect, Shape, pos2, vec2};

const NAMES: [&str; 7] = ["Fade", "Fly In", "Random Bars", "Ferris Wheel", "Complementary Color", "Diagonal Down Right", "Checkerboard"];

struct Text {
    string: String,
    size: f32,
    rect: Rect,
    clip: Rect,
}

fn context() -> egui::Context {
    let ctx = egui::Context::default();
    deckcraft_ui_egui::theme::install_fonts(&ctx);
    ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
    ctx
}

/// Paints on `ctx`; returns every text shape and the bounds of every other shape.
fn frame(ctx: &egui::Context, paint: &dyn Fn(&egui::Painter)) -> (Vec<Text>, Vec<Rect>) {
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| paint(ui.painter()));
    out.textures_delta.clear();
    let (mut texts, mut art) = (vec![], vec![]);
    for c in &out.shapes {
        match &c.shape {
            Shape::Text(s) => texts.push(Text {
                string: s.galley.job.text.clone(),
                size: s.galley.job.sections.first().map_or(0.0, |x| x.format.font_id.size),
                rect: s.galley.rect.translate(s.pos.to_vec2()),
                clip: c.clip_rect,
            }),
            other => art.push(other.visual_bounding_rect()),
        }
    }
    (texts, art)
}

fn width(ctx: &egui::Context, text: &str, size: f32) -> f32 {
    let mut w = 0.0;
    ctx.run_ui(egui::RawInput::default(), |ui| {
        w = ui.painter().layout_no_wrap(text.to_string(), egui::FontId::proportional(size), egui::Color32::WHITE).size().x;
    })
    .textures_delta
    .clear();
    w
}

fn inside(inner: Rect, outer: Rect) -> bool {
    inner.min.x >= outer.min.x - 0.5 && inner.min.y >= outer.min.y - 0.5 && inner.max.x <= outer.max.x + 0.5 && inner.max.y <= outer.max.y + 0.5
}

fn overlaps(a: Rect, b: Rect) -> bool {
    let i = a.intersect(b);
    i.width() > 0.5 && i.height() > 0.5
}

/// The split the issue asks for: at the space nearest the middle of the name.
fn halves(name: &str) -> Option<(&str, &str)> {
    let mid = name.len() / 2;
    let (i, _) = name.match_indices(' ').min_by_key(|(i, _)| i.abs_diff(mid))?;
    Some((&name[..i], &name[i + 1..]))
}

fn check(ctx: &egui::Context, what: &str, tile: Rect, name: &str, paint: &dyn Fn(&egui::Painter)) {
    let (texts, art) = frame(ctx, paint);
    let at = format!("{what} {name:?} in {tile:?}");
    let room = tile.width();
    if width(ctx, name, 10.0) <= room {
        // A name that fits is drawn exactly as before.
        assert_eq!(texts.len(), 1, "{at}: one line");
        assert_eq!((texts[0].string.as_str(), texts[0].size), (name, 10.0), "{at}: at 10 pt");
    } else if width(ctx, name, 7.0) <= room {
        // Smaller on one line, still readable, and really inside the tile without any clipping.
        assert_eq!(texts.len(), 1, "{at}: one line");
        assert!(texts[0].size >= 7.0 && texts[0].size < 10.0, "{at}: size {} should be in [7, 10)", texts[0].size);
        assert!(inside(texts[0].rect, tile), "{at}: text {:?} leaves the tile", texts[0].rect);
    } else if let Some((a, b)) = halves(name) {
        assert_eq!(texts.len(), 2, "{at}: two lines");
        assert_eq!((texts[0].string.as_str(), texts[1].string.as_str()), (a, b), "{at}: split at the middle space");
        assert_eq!(texts[0].size, texts[1].size, "{at}: both lines in one size");
        assert!(texts[0].size >= 6.0 && texts[0].size <= 10.0, "{at}: size {}", texts[0].size);
        if width(ctx, a, 6.0) <= room && width(ctx, b, 6.0) <= room {
            for t in &texts {
                assert!(inside(t.rect, tile), "{at}: line {:?} leaves the tile {:?} (clipping is not a fix)", t.string, t.rect);
            }
        }
    } else {
        // Nothing else to do for one long word: it must at least be clipped to the tile.
        assert_eq!(texts.len(), 1, "{at}: one line");
        assert!(inside(texts[0].clip, tile), "{at}: clip {:?} is wider than the tile", texts[0].clip);
    }
    for t in &texts {
        let shown = t.rect.intersect(t.clip);
        for a in &art {
            assert!(!overlaps(shown, *a), "{at}: text {:?} {shown:?} covers the picture or icon {a:?}", t.string);
        }
    }
}

/// Every gallery and popup tile size of the two ribbon tabs.
fn tiles() -> Vec<Rect> {
    let at = |w: f32, h: f32| Rect::from_min_size(pos2(100.0, 100.0), vec2(w, h));
    vec![at(54.0, 56.0), at(56.0, 58.0).shrink(3.0), at(50.0, 56.0), at(56.0, 54.0).shrink(2.0)]
}

#[test]
fn transition_tile_names_fit_their_tile() {
    let ctx = context();
    for tile in tiles() {
        for name in NAMES {
            check(&ctx, "transition tile", tile, name, &|p| paint_transition_tile(p, tile, ("fade", name)));
        }
    }
}

#[test]
fn animation_tile_names_fit_their_tile() {
    let ctx = context();
    for tile in tiles() {
        for name in NAMES {
            check(&ctx, "animation tile", tile, name, &|p| paint_anim_tile(p, tile, name, AnimClass::Emphasis, true));
        }
    }
}
