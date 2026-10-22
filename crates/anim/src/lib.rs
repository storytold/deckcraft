//! Slide show timing for DeckCraft.
//!
//! - [`Timeline`]: a slide's animation list grouped into click steps, with per-shape (and
//!   per-paragraph) [`AnimState`] at any point of the show.
//! - [`transition_layers`]: a slide transition frame as textured quads of the old/new slide images.
//! - [`morph_frame`]: a Morph transition frame (shapes matched by [`morph_pairs`], moved by
//!   [`morph_xfrm`]).
//! - [`ShowState`]: slide show navigation (hidden slides, custom shows, loop, timings), what was
//!   played on each visited slide, and what a click does ([`ShowState::click`]).
//!
//! Pure logic: no rendering, no UI toolkit. Every function accepts hostile numbers (NaN, huge or
//! negative times) and never panics.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod easing;
mod effects;
mod interact;
mod mesh;
mod morph;
mod path;
mod show;
mod timeline;
mod transition;
mod transition_fx;
mod transition_geom;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_interact;
#[cfg(test)]
mod transition_tests;

pub use deckcraft_model::Rgba;
use serde::{Deserialize, Serialize};

pub use easing::ease;
pub use effects::{MODEL_EFFECTS, is_model_effect, model_effect};
pub use interact::{Click, HitShape, Target};
pub use mesh::{MeshVertex, layer_mesh};
pub use morph::{MorphFrame, MorphPath, MorphText, morph_crop, morph_frame, morph_pairs, morph_xfrm};
pub use path::{MotionPath, default_path};
pub use show::{LinkJump, ShowAction, ShowState, safe_url, show_order};
pub use timeline::{EffectTiming, Sequence, StepStamp, Timeline, TriggerPlay};
pub use transition::{DEFAULT_ASPECT, Layer, Source, transition_frame, transition_layers, uses_slide_parts};

/// Animation state of one shape (or one paragraph of a shape's text) at one moment of the show.
///
/// Same meaning as the renderer's `ShapeState`: translation in points, scale and rotation (degrees)
/// around the shape's centre, a reveal clip in shape-local unit space `[l, t, r, b]`, and a fill
/// colour override.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnimState {
    pub visible: bool,
    pub opacity: f64,
    pub offset_x: f64,
    pub offset_y: f64,
    pub scale_x: f64,
    pub scale_y: f64,
    pub rotate: f64,
    pub clip: Option<[f64; 4]>,
    pub tint: Option<Rgba>,
    /// What a 3-D effect does to a 3-D model.
    pub model: ModelAnim,
}

impl Default for AnimState {
    fn default() -> Self {
        AnimState {
            visible: true,
            opacity: 1.0,
            offset_x: 0.0,
            offset_y: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            rotate: 0.0,
            clip: None,
            tint: None,
            model: ModelAnim::default(),
        }
    }
}

/// What a 3-D effect (Arrive, Turntable, Swing, Jump & Turn, Leave) does to a 3-D model on top of
/// its own pose: turns about the view's axes (after the model's rotation) and about its own (before
/// it), a move in the scene and a scale along its own axes.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelAnim {
    /// Degrees about the view's X, Y, Z axes.
    pub view_turn: [f64; 3],
    /// Degrees about the model's own X, Y, Z axes.
    pub own_turn: [f64; 3],
    /// Metres in the scene (Y up).
    pub shift: [f64; 3],
    pub scale: [f64; 3],
}

impl Default for ModelAnim {
    fn default() -> Self {
        ModelAnim { view_turn: [0.0; 3], own_turn: [0.0; 3], shift: [0.0; 3], scale: [1.0; 3] }
    }
}

impl ModelAnim {
    pub fn is_identity(&self) -> bool {
        *self == ModelAnim::default()
    }

    /// `d` on top of `self`: turns and moves add, scales multiply.
    pub fn compose(&self, d: &ModelAnim) -> ModelAnim {
        let add = |a: [f64; 3], b: [f64; 3]| [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
        ModelAnim {
            view_turn: add(self.view_turn, d.view_turn),
            own_turn: add(self.own_turn, d.own_turn),
            shift: add(self.shift, d.shift),
            scale: [self.scale[0] * d.scale[0], self.scale[1] * d.scale[1], self.scale[2] * d.scale[2]],
        }
    }

    fn sanitized(self) -> ModelAnim {
        let f = |v: [f64; 3], d: f64| v.map(|c| if c.is_finite() { c.clamp(-1e6, 1e6) } else { d });
        ModelAnim { view_turn: f(self.view_turn, 0.0), own_turn: f(self.own_turn, 0.0), shift: f(self.shift, 0.0), scale: f(self.scale, 1.0) }
    }
}

impl AnimState {
    /// A hidden state.
    pub fn hidden() -> Self {
        AnimState { visible: false, ..Default::default() }
    }
    /// Is this the identity (nothing to apply)?
    pub fn is_identity(&self) -> bool {
        *self == AnimState::default()
    }
    /// Apply `d` on top of `self`: opacities and scales multiply, offsets and rotations add, clips
    /// intersect, a tint replaces.
    pub fn compose(&self, d: &AnimState) -> AnimState {
        let clip = match (self.clip, d.clip) {
            (Some(a), Some(b)) => Some([a[0].max(b[0]), a[1].max(b[1]), a[2].min(b[2]), a[3].min(b[3])]),
            (a, None) => a,
            (None, b) => b,
        };
        AnimState {
            visible: self.visible && d.visible,
            opacity: self.opacity * d.opacity,
            offset_x: self.offset_x + d.offset_x,
            offset_y: self.offset_y + d.offset_y,
            scale_x: self.scale_x * d.scale_x,
            scale_y: self.scale_y * d.scale_y,
            rotate: self.rotate + d.rotate,
            clip,
            tint: d.tint.or(self.tint),
            model: self.model.compose(&d.model),
        }
    }
    /// Replace any non-finite number with its neutral value and clamp opacity and clip.
    pub fn sanitized(mut self) -> AnimState {
        let f = |v: f64, d: f64| if v.is_finite() { v } else { d };
        self.opacity = f(self.opacity, 1.0).clamp(0.0, 1.0);
        self.offset_x = f(self.offset_x, 0.0);
        self.offset_y = f(self.offset_y, 0.0);
        self.scale_x = f(self.scale_x, 1.0);
        self.scale_y = f(self.scale_y, 1.0);
        self.rotate = f(self.rotate, 0.0);
        self.clip = self.clip.map(|c| {
            let l = f(c[0], 0.0).clamp(0.0, 1.0);
            let t = f(c[1], 0.0).clamp(0.0, 1.0);
            let r = f(c[2], 1.0).clamp(0.0, 1.0);
            let b = f(c[3], 1.0).clamp(0.0, 1.0);
            [l, t, r.max(l), b.max(t)]
        });
        self.model = self.model.sanitized();
        self
    }
}

/// Clamp a hostile number into `[lo, hi]`, mapping NaN to `nan`.
pub(crate) fn clampf(v: f64, lo: f64, hi: f64, nan: f64) -> f64 {
    if v.is_nan() { nan } else { v.clamp(lo, hi) }
}
