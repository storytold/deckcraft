//! Last-resort guard: a panic that escapes a command —
//! any edit, import or export, from the UI, the CLI, MCP or the control channel — becomes an
//! error, and the document is kept as it was before the command (and saved for crash recovery
//! when a recovery directory is set). It is a safety net: commands still must not panic.

use std::panic::{AssertUnwindSafe, catch_unwind};

use serde_json::Value;

use crate::{EngineError, Result, Session};

/// Text of a panic payload (`panic!` with a literal or a formatted message).
pub fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(ToString::to_string)
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".into())
}

/// Log every panic (message and location) before it unwinds, then run the previous hook.
/// Installed once per process, when the first [`Session`] is made.
pub fn install_panic_hook() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let at = info.location().map(|l| format!(" at {}:{}", l.file(), l.line())).unwrap_or_default();
            log::error!("panic{at}: {}", panic_message(info.payload()));
            previous(info);
        }));
    });
}

impl Session {
    /// Run command `id` (`f`) inside the guard.
    pub(crate) fn guarded(&mut self, id: &str, f: impl FnOnce(&mut Session) -> Result<Value>) -> Result<Value> {
        let before = self.active().map(|d| (d.uid, d.doc.clone(), d.selection.clone(), d.revision));
        let vertex_kind = self.tool.vertex_kind.clone();
        let depth = self.depth;
        match catch_unwind(AssertUnwindSafe(|| f(self))) {
            Ok(r) => r,
            Err(payload) => {
                self.depth = depth;
                // The remembered point type names a shape of the document it was chosen in.
                let same = before.as_ref().is_some_and(|b| self.active().is_some_and(|st| st.uid == b.0));
                self.tool.vertex_kind = if same { vertex_kind } else { None };
                let msg = panic_message(payload.as_ref());
                // Keep the document as it was before the command.
                if let (Some((uid, doc, selection, revision)), Some(st)) = (before, self.active_mut())
                    && st.uid == uid
                {
                    st.doc = doc;
                    st.selection = selection;
                    st.interaction = None;
                    st.revision = revision.max(st.revision).saturating_add(1);
                }
                log::error!("command `{id}` panicked: {msg}");
                self.panics_caught = self.panics_caught.saturating_add(1);
                if let Some(dir) = self.recovery_dir.clone()
                    && let Err(e) = crate::recovery::save(self, &dir)
                {
                    log::error!("recovery save after `{id}` failed: {e}");
                }
                Err(EngineError::Internal(id.to_string(), msg))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn a_panicking_command_is_an_error_and_keeps_the_document() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("shape.insert", &json!({"preset": "rect", "rect": [36, 36, 200, 200]})).unwrap();
        let before = s.active().unwrap().doc.clone();
        // A command that edits the document and then panics half way.
        let err = s
            .guarded("test.panic", |s| {
                s.execute("shape.insert", &json!({"preset": "ellipse", "rect": [36, 236, 200, 400]}))?;
                panic!("boom");
            })
            .unwrap_err();
        assert!(err.to_string().contains("test.panic") && err.to_string().contains("boom"), "{err}");
        let st = s.active().unwrap();
        assert!(std::sync::Arc::ptr_eq(&st.doc, &before), "the document is the one from before the command");
        assert_eq!(s.panics_caught, 1, "the guard counts what it caught");
        // The session keeps working.
        s.execute("shape.insert", &json!({"preset": "ellipse", "rect": [36, 236, 200, 400]})).unwrap();
    }

    #[test]
    fn a_panicking_command_keeps_the_remembered_point_type() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let kept = Some((deckcraft_model::ShapeId(1), 0, 0, "straight".to_string()));
        s.tool.vertex_kind = kept.clone();
        let r = s.guarded("test.panic", |s| {
            s.tool.vertex_kind = Some((deckcraft_model::ShapeId(1), 0, 0, "corner".to_string()));
            panic!("boom");
        });
        assert!(r.is_err());
        assert_eq!(s.tool.vertex_kind, kept);
        // A command that opens another document and then panics leaves no point type behind: the
        // one from before names a shape of the other document.
        let r = s.guarded("test.panic", |s| {
            s.execute("file.new", &json!({}))?;
            s.tool.vertex_kind = Some((deckcraft_model::ShapeId(1), 0, 0, "corner".to_string()));
            panic!("boom");
        });
        assert!(r.is_err());
        assert_eq!(s.tool.vertex_kind, None);
    }

    #[test]
    fn a_command_that_fails_after_editing_leaves_the_document_alone() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = s.execute("shape.insert", &json!({"preset": "rect", "rect": [36, 36, 100, 100]})).unwrap()["id"].as_u64().unwrap();
        let b = s.execute("shape.insert", &json!({"preset": "rect", "rect": [236, 36, 100, 100]})).unwrap()["id"].as_u64().unwrap();
        let before = s.active().unwrap().doc.clone();
        let undo_len = s.active().unwrap().history.undo.len();
        // `shape.connect` inserts the requested preset first, then rejects it because it is not a line.
        let r = s.execute("shape.connect", &json!({"from": a, "to": b, "preset": "rect"}));
        assert!(r.is_err());
        let st = s.active().unwrap();
        assert!(std::sync::Arc::ptr_eq(&st.doc, &before), "the stray shape is gone");
        assert_eq!(st.history.undo.len(), undo_len);
    }

    #[test]
    fn a_failed_picture_change_adds_no_media() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("shape.insert", &json!({"preset": "rect", "rect": [36, 36, 100, 100]})).unwrap();
        let before = s.active().unwrap().doc.clone();
        // `picture.change` stores the image first, then finds no shape with this id.
        let r = s.execute("picture.change", &json!({"ids": [9999], "name": "a.png", "data": crate::cmd::base64_encode(b"not a picture")}));
        assert!(r.is_err());
        assert!(std::sync::Arc::ptr_eq(&s.active().unwrap().doc, &before), "the stored image is gone");
    }
}
