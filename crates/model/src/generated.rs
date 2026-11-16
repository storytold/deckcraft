//! Generated elements: a group whose shapes are drawn from a small description (a Harvey ball, a
//! checkbox), as a SmartArt group's are drawn from its `diagram`. The description is
//! [`Shape::generated`](crate::Shape::generated); the engine regenerates the group's children from
//! it. In a `.pptx` it travels as the shape tag [`TAG`], so other software sees an ordinary group.

use deckcraft_color::SchemeSlot;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ColorRef;

/// The shape tag (`p:custDataLst`) a `.pptx` carries a generated group's description in.
pub const TAG: &str = "ARTCRAFT_GENERATED";
/// The tag's format version, written as `"v"` beside the description.
pub const TAG_VERSION: u64 = 1;
/// Largest `of` a Harvey ball takes: finer than a hundredth isn't visible at any size.
pub const MAX_OF: u16 = 100;
/// Widest Harvey-ball outline, in points.
pub const MAX_OUTLINE_WIDTH: f64 = 20.0;

/// What a generated group is drawn from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Generated {
    HarveyBall(HarveyBall),
    Checkbox(Checkbox),
}

/// A Harvey ball: a circle filled clockwise from 12 o'clock by `value` parts of `of`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct HarveyBall {
    /// Filled parts, `0..=of`.
    pub value: u16,
    /// Parts in the whole ball, `1..=100`.
    pub of: u16,
    /// The filled wedge.
    pub fill: ColorRef,
    /// The unfilled part; `None` leaves it transparent.
    pub background: Option<ColorRef>,
    /// The ring around the ball; `None` draws none.
    pub outline: Option<ColorRef>,
    /// The ring's width in points, `0..=20`.
    pub outline_width: f64,
}

impl Default for HarveyBall {
    fn default() -> Self {
        HarveyBall {
            value: 1,
            of: 4,
            fill: ColorRef::scheme(SchemeSlot::Tx1),
            background: None,
            outline: Some(ColorRef::scheme(SchemeSlot::Tx1)),
            outline_width: 0.75,
        }
    }
}

/// A checkbox: an optional box with a tick, a cross or nothing in it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Checkbox {
    pub state: CheckState,
    /// The box, tick and cross.
    pub color: ColorRef,
    /// Draw the box around the mark.
    pub boxed: bool,
}

impl Default for Checkbox {
    fn default() -> Self {
        Checkbox { state: CheckState::Checked, color: ColorRef::scheme(SchemeSlot::Tx1), boxed: true }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CheckState {
    #[default]
    Checked,
    Unchecked,
    Crossed,
}

impl CheckState {
    /// The name commands take and answer with.
    pub fn name(self) -> &'static str {
        match self {
            CheckState::Checked => "checked",
            CheckState::Unchecked => "unchecked",
            CheckState::Crossed => "crossed",
        }
    }
    pub fn from_name(s: &str) -> Option<CheckState> {
        match s {
            "checked" => Some(CheckState::Checked),
            "unchecked" => Some(CheckState::Unchecked),
            "crossed" => Some(CheckState::Crossed),
            _ => None,
        }
    }
}

impl HarveyBall {
    /// Why this ball can't be drawn, if it can't: `of` outside `1..=100`, `value` above `of`, or an
    /// outline width that isn't a number in `0..=20`.
    pub fn check(&self) -> Result<(), String> {
        if self.of == 0 || self.of > MAX_OF {
            return Err(format!("`of` must be 1 to {MAX_OF}, not {}", self.of));
        }
        if self.value > self.of {
            return Err(format!("`value` ({}) is more than `of` ({})", self.value, self.of));
        }
        if !(self.outline_width.is_finite() && (0.0..=MAX_OUTLINE_WIDTH).contains(&self.outline_width)) {
            return Err(format!("`outlineWidth` must be 0 to {MAX_OUTLINE_WIDTH} pt"));
        }
        Ok(())
    }
}

impl Generated {
    /// The name of its kind, as serialized: `harveyBall`, `checkbox`.
    pub fn kind_name(&self) -> &'static str {
        match self {
            Generated::HarveyBall(_) => "harveyBall",
            Generated::Checkbox(_) => "checkbox",
        }
    }

    /// Why it can't be drawn, if it can't. A description read from a file is untrusted.
    pub fn check(&self) -> Result<(), String> {
        match self {
            Generated::HarveyBall(b) => b.check(),
            Generated::Checkbox(_) => Ok(()),
        }
    }

    /// The [`TAG`] value: `{"v":1, "kind":…, …}`.
    pub fn to_tag(&self) -> Option<String> {
        let Value::Object(mut o) = serde_json::to_value(self).ok()? else { return None };
        o.insert("v".into(), Value::from(TAG_VERSION));
        serde_json::to_string(&Value::Object(o)).ok()
    }

    /// A [`TAG`] value read back. `None` when it isn't version 1, doesn't parse or describes
    /// something that can't be drawn; the caller then keeps the tag as an ordinary one.
    pub fn from_tag(s: &str) -> Option<Generated> {
        let Value::Object(mut o) = serde_json::from_str::<Value>(s).ok()? else { return None };
        if o.remove("v")?.as_u64()? != TAG_VERSION {
            return None;
        }
        let g: Generated = serde_json::from_value(Value::Object(o)).ok()?;
        g.check().ok()?;
        Some(g)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_round_trips_and_rejects_bad_values() {
        let b = Generated::HarveyBall(HarveyBall { value: 3, of: 8, background: Some(ColorRef::scheme(SchemeSlot::Bg1)), ..Default::default() });
        let t = b.to_tag().unwrap_or_default();
        assert!(t.contains("\"v\":1") && t.contains("\"kind\":\"harveyBall\""), "{t}");
        assert_eq!(Generated::from_tag(&t), Some(b));
        let c = Generated::Checkbox(Checkbox { state: CheckState::Crossed, boxed: false, ..Default::default() });
        assert_eq!(c.to_tag().as_deref().and_then(Generated::from_tag), Some(c));
        // No outline survives as no outline, not as the default one.
        let none = Generated::HarveyBall(HarveyBall { outline: None, ..Default::default() });
        assert_eq!(none.to_tag().as_deref().and_then(Generated::from_tag), Some(none));
        for bad in [
            "",
            "not json",
            "[1]",
            r#"{"kind":"harveyBall"}"#,
            r#"{"v":2,"kind":"harveyBall"}"#,
            r#"{"v":"1","kind":"harveyBall"}"#,
            r#"{"v":1,"kind":"pie"}"#,
            r#"{"v":1,"kind":"harveyBall","of":0}"#,
            r#"{"v":1,"kind":"harveyBall","value":5,"of":4}"#,
            r#"{"v":1,"kind":"harveyBall","of":101}"#,
            r#"{"v":1,"kind":"harveyBall","of":70000}"#,
            r#"{"v":1,"kind":"harveyBall","outlineWidth":1e300}"#,
            r#"{"v":1,"kind":"checkbox","state":"maybe"}"#,
        ] {
            assert_eq!(Generated::from_tag(bad), None, "{bad}");
        }
        // Missing keys take their defaults.
        assert_eq!(Generated::from_tag(r#"{"v":1,"kind":"checkbox"}"#), Some(Generated::Checkbox(Checkbox::default())));
    }
}
