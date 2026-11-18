//! The sandbox's view of the session.

use craft_plugin::{Access, Manifest};
use serde_json::Value;

use super::{MAX_NOTE_CHARS, MAX_NOTES, refusal, selection_json, set_setting};
use crate::{Session, UiRequest};

/// The sandbox's view of the session: every call is checked by [`refusal`] and run through
/// [`Session::execute`], as the ribbon, the CLI or MCP would run it. Inside `plugin.run` the calls
/// are nested commands, so none records an undo step or a journal entry of its own. A call that
/// asks the window for something (a dialog, a pane, a file picker, the slide show, media playback)
/// or that, from a read-only plug-in, changes the presentation is put back and refused.
pub(super) struct EngineHost<'s> {
    s: &'s mut Session,
    manifest: &'s Manifest,
    notes: Vec<String>,
}

impl<'s> EngineHost<'s> {
    pub(super) fn new(s: &'s mut Session, manifest: &'s Manifest) -> Self {
        EngineHost { s, manifest, notes: Vec::new() }
    }

    /// What the run asked to show the user.
    pub(super) fn finish(self) -> Vec<String> {
        self.notes
    }
}

impl craft_plugin::Host for EngineHost<'_> {
    fn call(&mut self, command: &str, params: &str) -> Result<String, String> {
        let command = command.trim();
        let params: Value = serde_json::from_str(params).map_err(|e| format!("the params for `{command}` aren't JSON: {e}"))?;
        if let Some(why) = refusal(self.manifest, command, &params) {
            return Err(why);
        }
        let reading = self.manifest.permissions.access != Access::Write;
        // The registry says which commands are edits: a read-only plug-in is refused them up front.
        if reading && crate::find_command(command).is_some_and(|c| c.undoable) {
            craft_plugin::check_read_only(self.manifest, command, true)?;
        }
        let before = self.s.active().map(|d| (d.uid, d.doc.clone(), d.selection.clone(), d.revision));
        let requests = self.s.ui_requests.len();
        let r = self.s.execute(command, &params);
        // Commands only ask the window for dialogs, panes, pickers, the show and media playback,
        // which a run can't answer; a status message is harmless.
        let asks = self.s.ui_requests.get(requests..).unwrap_or_default().iter().any(|q| !matches!(q, UiRequest::Message { .. }));
        // And the registry can't know everything a command does: what a read-only plug-in's call
        // changed is put back too.
        let edited = reading
            && before.as_ref().is_some_and(|(uid, doc, ..)| self.s.active().is_some_and(|d| d.uid != *uid || !std::sync::Arc::ptr_eq(&d.doc, doc)));
        if asks || edited {
            self.s.ui_requests.truncate(requests);
            if let (Some((uid, doc, sel, revision)), Some(st)) = (before, self.s.active_mut())
                && st.uid == uid
            {
                st.doc = doc;
                st.selection = sel;
                st.revision = revision.max(st.revision).saturating_add(1);
            }
            return Err(if asks {
                format!("`{command}` opens a dialog, a pane or the slide show, which plug-ins can't: pass its parameters instead")
            } else {
                craft_plugin::check_read_only(self.manifest, command, true).err().unwrap_or_else(|| format!("`{command}` edits the presentation"))
            });
        }
        r.map(|v| v.to_string()).map_err(|e| e.to_string())
    }

    fn selection(&mut self) -> String {
        selection_json(self.s).to_string()
    }

    fn notify(&mut self, message: &str) {
        if self.notes.len() < MAX_NOTES {
            self.notes.push(message.chars().take(MAX_NOTE_CHARS).collect());
        }
    }

    fn log(&mut self, line: &str) {
        // Also in the run's result (`logs`)
        log::info!("plug-in {}: {line}", self.manifest.id);
    }

    fn setting(&mut self, key: &str) -> Option<String> {
        Some(self.s.plugins.prefs.settings.get(&self.manifest.id)?.get(key)?.to_string())
    }

    fn set_setting(&mut self, key: &str, value: Option<&str>) -> Result<(), String> {
        set_setting(&mut self.s.plugins.prefs.settings, &self.manifest.id, key, value)
    }
}
