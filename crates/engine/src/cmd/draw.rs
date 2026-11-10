//! Draw ▸ Stencils: the ruler, a view aid on the canvas (never saved in the presentation).
//!
//! The geometry that snaps ink and dragged objects lives in [`crate::ruler`]; this module holds
//! the commands that show, hide and place the ruler so the UI, the control channel and MCP all
//! change it the same way.

use serde_json::{Value, json};

use super::{CommandSpec, always, bad, bool_param, cmd};
use crate::ruler::{self, RulerState};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            noundo
            "draw.ruler",
            "Ruler",
            ["Draw", "Stencils"],
            None,
            "{} toggles; or {on?: bool, x?: pt, y?: pt, angle?: deg} → {visible, x, y, angle}. The first time it shows it is centred on the page; the angle wraps into 0..360 and the position stays on the slide's neighbourhood",
            always,
            ruler
        ),
        cmd!(
            query "draw.rulerState",
            "Ruler State",
            [],
            None,
            "{} → {visible, x, y, angle, length, snap, manipulate}",
            always,
            state
        ),
    ]
}

/// A finite number from the parameters; a value present but not a finite number is an error, so a
/// bad client call is told rather than silently ignored.
fn num(p: &Value, key: &str) -> Result<Option<f64>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_f64().filter(|v| v.is_finite()).map(Some).ok_or_else(|| bad("draw.ruler", format!("`{key}` is not a finite number: {v}"))),
    }
}

/// Show, hide or place the ruler; the state is clamped and wrapped after every change.
fn ruler(s: &mut Session, p: &Value) -> Result<Value> {
    let mut st = s.view.ruler;
    let (x, y, angle) = (num(p, "x")?, num(p, "y")?, num(p, "angle")?);
    let on = match p.get("on") {
        None | Some(Value::Null) => None,
        Some(_) => Some(bool_param(p, "on").ok_or_else(|| bad("draw.ruler", "`on` is not true or false"))?),
    };
    if let Some(v) = on {
        st.visible = v;
    }
    // No parameter at all is the ribbon button: toggle, like PowerPoint's Ruler button.
    let empty = p.as_object().is_none_or(|o| o.is_empty());
    if empty {
        st.visible = !st.visible;
    }
    if let Some(v) = x {
        st.x = v;
    }
    if let Some(v) = y {
        st.y = v;
    }
    if let Some(v) = angle {
        st.angle = v;
    }
    // The first time it shows, centre it on the page (unless a position came with the call).
    if st.visible && !st.placed {
        if let Some(d) = s.active() {
            let page = crate::page_size(d);
            if x.is_none() {
                st.x = page.width / 2.0;
            }
            if y.is_none() {
                st.y = page.height / 2.0;
            }
        }
        st.placed = true;
    }
    s.view.ruler = st.sanitized();
    Ok(state_json(&s.view.ruler))
}

/// The ruler's current state, for the UI and control channel.
fn state(s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(state_json(&s.view.ruler))
}

/// The ruler's state as the commands return it, with its fixed geometry for clients.
fn state_json(r: &RulerState) -> Value {
    json!({
        "visible": r.visible,
        "x": r.x,
        "y": r.y,
        "angle": r.angle,
        "manipulate": r.manipulate,
        "length": ruler::LENGTH,
        "snap": ruler::SNAP,
    })
}

#[cfg(test)]
#[path = "draw_tests.rs"]
mod draw_tests;
