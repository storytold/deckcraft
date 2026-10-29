//! Keyboard: text-editing keys while editing text, nudges and shortcuts otherwise. Shared by the
//! UI, the control channel and MCP so a key does the same thing everywhere.

use serde_json::{Value, json};

use crate::tools::Mods;
use crate::{Result, Session};

/// Does a shortcut like `Cmd+Shift+]` match `key` + `mods`?
pub fn shortcut_matches(shortcut: &str, key: &str, mods: Mods) -> bool {
    let parts: Vec<&str> = shortcut.split('+').collect();
    let (k, ms) = match parts.as_slice() {
        [.., "", ""] => ("+", &parts[..parts.len().saturating_sub(2)]),
        _ => match parts.split_last() {
            Some((k, ms)) => (*k, ms),
            None => return false,
        },
    };
    let has = |m: &str| ms.iter().any(|x| x.eq_ignore_ascii_case(m));
    k.eq_ignore_ascii_case(key) && (has("cmd") || has("ctrl")) == mods.cmd && has("shift") == mods.shift && (has("alt") || has("option")) == mods.alt
}

/// A key name in one spelling: lower case, `ArrowLeft` → `left`, and egui's names for
/// punctuation keys (`Period`, `CloseBracket`…) → the character, so `Cmd+Shift+.` matches the
/// key egui reports for Ctrl+Shift+> and `Cmd+]` the one for Ctrl+].
pub fn key_token(key: &str) -> String {
    let k = key.to_ascii_lowercase();
    let k = k.trim_start_matches("arrow");
    match k {
        "period" | ">" => ".",
        "comma" | "<" => ",",
        "openbracket" | "{" => "[",
        "closebracket" | "}" => "]",
        "equals" => "=",
        "minus" => "-",
        "plus" => "+",
        "slash" => "/",
        "backslash" => "\\",
        "semicolon" => ";",
        "quote" => "'",
        "backtick" => "`",
        other => other,
    }
    .to_string()
}

impl Session {
    /// Handle one key press. `key` is a name (`Enter`, `Escape`, `Backspace`, `Delete`, `Tab`,
    /// `ArrowLeft`…, `Home`, `End`, `PageUp`, `PageDown`, `F5`) or a character (`b`, `]`).
    /// Returns `{handled: bool, …}`.
    pub fn key(&mut self, key: &str, mods: Mods) -> Result<Value> {
        self.settle_composition();
        let editing = self.active().is_some_and(|d| d.selection.text.is_some());
        let has_shapes = self.active().is_some_and(|d| !d.selection.shapes.is_empty());
        let k = key_token(key);
        let k = k.as_str();
        if matches!(k, "escape" | "esc") && self.cancel_point_drag() {
            return Ok(json!({"handled": true}));
        }
        if self.tool.drawing_freeform() && matches!(k, "enter" | "return" | "escape") {
            return self.finish_freeform(false);
        }
        if self.tool.edit_points.is_some() && matches!(k, "enter" | "return" | "escape") {
            self.tool.edit_points = None;
            self.tool.edit_vertex = None;
            self.tool.edit_segment = None;
            return Ok(json!({"handled": true, "command": "shape.editPoints"}));
        }
        if editing && let Some((id, params)) = self.text_key(k, mods) {
            let r = self.execute(id, &params)?;
            return Ok(json!({"handled": true, "command": id, "result": r}));
        }
        if editing && !mods.cmd {
            let r = match k {
                "enter" | "return" => Some(self.execute("text.insert", &json!({"text": if mods.shift { "\u{b}" } else { "\n" }}))),
                "escape" | "esc" => Some(self.execute("text.exit", &json!({}))),
                "tab" => {
                    // Tab at the start of a paragraph changes the list level; elsewhere it types a tab.
                    let at_start = self.active().and_then(|d| d.selection.text.as_ref()).is_some_and(|t| t.ordered().0.1 == 0 || t.is_range());
                    let in_cell = self.active().and_then(|d| d.selection.text.as_ref()).is_some_and(|t| t.cell.is_some());
                    if in_cell {
                        Some(self.next_cell(mods.shift))
                    } else if at_start {
                        Some(self.execute(if mods.shift { "format.outdent" } else { "format.indent" }, &json!({})))
                    } else {
                        Some(self.execute("text.insert", &json!({"text": "\t"})))
                    }
                }
                _ => None,
            };
            if let Some(r) = r {
                r?;
                return Ok(json!({"handled": true}));
            }
        }
        if !editing && has_shapes && !mods.cmd && mods.shift && !mods.alt {
            // Shift+arrow grows or shrinks a selected range of table cells.
            let d = match k {
                "left" => Some((0, -1)),
                "right" => Some((0, 1)),
                "up" => Some((-1, 0)),
                "down" => Some((1, 0)),
                _ => None,
            };
            if let Some((dr, dc)) = d
                && crate::cmd::table::extend_selected_cells(self, dr, dc)?
            {
                return Ok(json!({"handled": true}));
            }
        }
        if !editing && has_shapes && !mods.cmd {
            let step = if mods.alt { 1.0 } else { 6.0 };
            let d = match k {
                "left" => Some((-step, 0.0)),
                "right" => Some((step, 0.0)),
                "up" => Some((0.0, -step)),
                "down" => Some((0.0, step)),
                _ => None,
            };
            if let Some((dx, dy)) = d {
                self.execute("shape.move", &json!({"dx": dx, "dy": dy}))?;
                return Ok(json!({"handled": true}));
            }
            match k {
                "backspace" | "delete" => {
                    self.execute("edit.delete", &json!({}))?;
                    return Ok(json!({"handled": true}));
                }
                "enter" | "return" | "f2" => {
                    self.execute("text.edit", &json!({}))?;
                    return Ok(json!({"handled": true}));
                }
                "escape" | "esc" => {
                    self.execute("edit.deselect", &json!({}))?;
                    return Ok(json!({"handled": true}));
                }
                "tab" => {
                    self.cycle_selection(!mods.shift)?;
                    return Ok(json!({"handled": true}));
                }
                _ => {}
            }
        }
        if !editing && !has_shapes && !mods.cmd {
            let r = match k {
                "pagedown" | "down" | "right" => Some("slide.next"),
                "pageup" | "up" | "left" => Some("slide.previous"),
                "home" => Some("slide.first"),
                "end" => Some("slide.last"),
                "enter" | "return" => Some("slide.new"),
                "tab" => {
                    self.cycle_selection(!mods.shift)?;
                    return Ok(json!({"handled": true}));
                }
                _ => None,
            };
            if let Some(id) = r {
                self.execute(id, &json!({}))?;
                return Ok(json!({"handled": true}));
            }
        }
        // Shortcuts from the registry.
        let key_name = match k {
            "left" => "Left",
            "right" => "Right",
            "up" => "Up",
            "down" => "Down",
            "enter" | "return" => "Enter",
            "escape" | "esc" => "Escape",
            "delete" => "Delete",
            "backspace" => "Backspace",
            "pageup" => "PageUp",
            "pagedown" => "PageDown",
            "home" => "Home",
            "end" => "End",
            "tab" => "Tab",
            "space" | " " => "Space",
            _ => k,
        };
        let found = crate::command_specs().iter().find(|c| c.shortcut.is_some_and(|s| shortcut_matches(s, key_name, mods))).map(|c| c.id);
        if let Some(id) = found {
            let r = self.execute(id, &json!({}))?;
            return Ok(json!({"handled": true, "command": id, "result": r}));
        }
        Ok(json!({"handled": false}))
    }

    /// The text-editing command for a navigation or deletion key while editing text, by
    /// platform: Ctrl (Windows, Linux) or Option (Mac) moves and deletes by word; Ctrl+Up/Down
    /// moves by paragraph; Ctrl+Home/End (Cmd+Up/Down on the Mac) go to the ends of the text and
    /// Cmd+Left/Right on the Mac to the ends of the line. Shift extends the selection.
    fn text_key(&self, k: &str, mods: Mods) -> Option<(&'static str, Value)> {
        let word = if self.mac_keys { mods.alt && !mods.cmd } else { mods.cmd && !mods.alt };
        let mac_cmd = self.mac_keys && mods.cmd && !mods.alt;
        if mods.cmd && !mods.alt && !mods.shift {
            // Ctrl+] / Ctrl+[ grow and shrink the font, like Ctrl+Shift+> / <.
            match k {
                "]" => return Some(("format.grow", json!({}))),
                "[" => return Some(("format.shrink", json!({}))),
                _ => {}
            }
        }
        let plain = !mods.cmd && !mods.alt;
        let to = match k {
            "left" if word => "wordLeft",
            "right" if word => "wordRight",
            "left" if mac_cmd => "lineStart",
            "right" if mac_cmd => "lineEnd",
            "left" if plain => "left",
            "right" if plain => "right",
            "up" if mac_cmd => "start",
            "down" if mac_cmd => "end",
            "up" if word && self.mac_keys => "paraStart",
            "down" if word && self.mac_keys => "paraEnd",
            "up" if word => "paraUp",
            "down" if word => "paraDown",
            "up" if plain => "up",
            "down" if plain => "down",
            "home" if mods.cmd && !mods.alt => "start",
            "end" if mods.cmd && !mods.alt => "end",
            "home" if plain => "lineStart",
            "end" if plain => "lineEnd",
            "backspace" if word => return Some(("text.delete", json!({"dir": "wordBackward"}))),
            "delete" if word => return Some(("text.delete", json!({"dir": "wordForward"}))),
            "backspace" if plain => return Some(("text.delete", json!({"dir": "backward"}))),
            "delete" if plain => return Some(("text.delete", json!({"dir": "forward"}))),
            _ => return None,
        };
        Some(("text.move", json!({"to": to, "extend": mods.shift})))
    }

    /// Type text: into the text being edited, or (a shape selected) start editing it.
    pub fn type_text(&mut self, text: &str) -> Result<Value> {
        let st = self.doc()?;
        if st.selection.text.is_none() {
            let Some(id) = st.selection.shapes.first().copied() else { return Ok(json!({"handled": false})) };
            if st.selection.shapes.len() != 1 {
                return Ok(json!({"handled": false}));
            }
            // Typing on a selected shape replaces its text (PowerPoint behaviour).
            self.execute("text.edit", &json!({"id": id}))?;
            self.execute("text.selectAll", &json!({}))?;
        }
        self.execute("text.insert", &json!({"text": text}))
    }

    /// An input method's preedit (#101). On a selected shape that isn't being edited, the first
    /// one starts editing its text and replaces it, as typing does ([`Session::type_text`]).
    /// `active` is the part being converted, in characters of `text`.
    pub fn compose_text(&mut self, text: &str, active: Option<(usize, usize)>) -> Result<Value> {
        if !self.start_typing(text)? {
            return Ok(json!({"handled": false}));
        }
        let mut p = json!({"text": text});
        if let Some((a, b)) = active {
            p["active"] = json!([a, b]);
        }
        self.execute("text.compose", &p)
    }

    /// An input method's result (#101): replaces the composition as one undo step.
    pub fn commit_text(&mut self, text: &str) -> Result<Value> {
        if !self.start_typing(text)? {
            return Ok(json!({"handled": false}));
        }
        self.execute("text.commit", &json!({"text": text}))
    }

    /// Remove text around the composition caret or selection, as an input method asks
    /// (`ImeEvent::DeleteSurrounding`/`DeleteSurroundingBytes`). Counts are characters or UTF-8
    /// bytes of the logical text the surrounding query publishes, so a range may cross a
    /// paragraph mark (read as one `\n` byte) and whole scalars only are removed; out-of-range
    /// counts are clamped. A deletion while composing stays inside the composition's single undo
    /// step.
    pub fn delete_surrounding(&mut self, before: usize, after: usize, bytes: bool) -> Result<Value> {
        self.execute("text.deleteSurrounding", &json!({"before": before, "after": after, "bytes": bytes}))
    }

    /// Ready to take typed `text`: editing text already, or one selected shape whose text editing
    /// starts with everything selected. False when there is nowhere to type (or nothing to type
    /// and nothing being edited).
    fn start_typing(&mut self, text: &str) -> Result<bool> {
        let st = self.doc()?;
        if st.selection.text.is_some() {
            return Ok(true);
        }
        let Some(id) = st.selection.shapes.first().copied() else { return Ok(false) };
        if st.selection.shapes.len() != 1 || text.is_empty() {
            return Ok(false);
        }
        self.execute("text.edit", &json!({"id": id}))?;
        self.execute("text.selectAll", &json!({}))?;
        Ok(true)
    }

    fn cycle_selection(&mut self, forward: bool) -> Result<()> {
        let st = self.doc()?;
        let ids: Vec<_> = st.shapes().iter().filter(|s| !s.hidden).map(|s| s.id).collect();
        if ids.is_empty() {
            return Ok(());
        }
        let cur = st.selection.shapes.first().and_then(|c| ids.iter().position(|x| x == c));
        let n = ids.len();
        let next = match cur {
            Some(i) if forward => (i + 1) % n,
            Some(i) => (i + n - 1) % n,
            None if forward => 0,
            None => n - 1,
        };
        let id = ids.get(next).copied();
        self.select(|_, sel| {
            sel.shapes = id.into_iter().collect();
            sel.text = None;
        })
    }

    fn next_cell(&mut self, back: bool) -> Result<Value> {
        let st = self.doc()?;
        let Some(t) = st.selection.text.clone() else { return Ok(Value::Null) };
        let Some((r, c)) = t.cell else { return Ok(Value::Null) };
        let Some(deckcraft_model::ShapeKind::Table(tb)) = st.shape(t.shape).map(|s| s.kind.clone()) else { return Ok(Value::Null) };
        let (nr, nc) = (tb.n_rows(), tb.n_cols().max(1));
        let idx = r * nc + c;
        // Cells hidden under a merged cell are skipped.
        let visible = |i: &usize| tb.cell(i / nc, i % nc).is_some_and(|x| !x.is_covered());
        let next = if back { (0..idx).rev().find(visible) } else { (idx + 1..nr * nc).find(visible) };
        let ni = match next {
            Some(ni) => ni,
            None if !back => {
                // Tab in the last cell adds a row.
                let last = nr.saturating_sub(1);
                self.execute("table.insertRowBelow", &json!({"id": t.shape, "row": last}))?;
                return self.execute("text.edit", &json!({"id": t.shape, "cell": [last + 1, 0]}));
            }
            None => idx,
        };
        self.execute("text.edit", &json!({"id": t.shape, "cell": [ni / nc, ni % nc]}))?;
        self.execute("text.selectAll", &json!({}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcut_parsing() {
        let m = |shift, alt, cmd| Mods { shift, alt, cmd };
        assert!(shortcut_matches("Cmd+B", "b", m(false, false, true)));
        assert!(!shortcut_matches("Cmd+B", "b", m(true, false, true)));
        assert!(shortcut_matches("Cmd+Shift+]", "]", m(true, false, true)));
        assert!(shortcut_matches("Cmd+Shift+.", ".", m(true, false, true)));
        assert!(shortcut_matches("Delete", "Delete", m(false, false, false)));
    }

    #[test]
    fn keys_drive_editing() {
        let mut s = Session::with_new();
        let title = s.doc().unwrap().current_slide().unwrap().shapes[0].id.0;
        s.execute("edit.select", &json!({"ids": [title]})).unwrap();
        s.type_text("Hi").unwrap();
        s.key("Enter", Mods::default()).unwrap();
        s.type_text("there").unwrap();
        s.key("Backspace", Mods::default()).unwrap();
        s.key("Escape", Mods::default()).unwrap();
        let t = s.execute("text.get", &json!({"id": title})).unwrap();
        assert_eq!(t["text"], "Hi\nther");
        let r = s.key("b", Mods { cmd: true, ..Default::default() }).unwrap();
        assert_eq!(r["command"], "format.bold");
        s.key("Right", Mods::default()).unwrap();
        s.key("Delete", Mods::default()).unwrap();
        assert_eq!(s.doc().unwrap().current_slide().unwrap().shapes.len(), 1);
    }
}
