//! In-canvas animation preview: the current slide's effects played on the editor canvas
//! (Animations ▸ Preview, Animation Pane ▸ Play All). The slide show uses the same mapping.

use deckcraft_anim::Timeline;
use deckcraft_geom::Vec2;
use deckcraft_model::ShapeId;
use deckcraft_render::{ParaState, ShapeState};

/// How long the finished sequence stays on the canvas before the editor draws the slide at rest.
pub const HOLD_SECS: f64 = 0.45;

/// Shape and paragraph state when `step` main steps are done and the current step has played `t` seconds.
pub fn shape_state(tl: &Timeline, id: ShapeId, step: usize, t: f64) -> ShapeState {
    let a = tl.state(id, None, step, t);
    ShapeState {
        visible: a.visible,
        opacity: a.opacity,
        offset: Vec2::new(a.offset_x, a.offset_y),
        scale: (a.scale_x, a.scale_y),
        rotate: a.rotate,
        clip: a.clip,
        tint: a.tint,
        paras: tl
            .paragraph_targets(id)
            .into_iter()
            .map(|p| {
                let a = tl.state(id, Some(p), step, t);
                (p, ParaState { visible: a.visible, opacity: a.opacity, offset: Vec2::new(a.offset_x, a.offset_y) })
            })
            .collect(),
    }
}

/// State on the play-everything clock (preview and Play All).
pub fn shape_state_at(tl: &Timeline, id: ShapeId, time: f64) -> ShapeState {
    let (step, t) = tl.locate(time);
    shape_state(tl, id, step, t)
}
