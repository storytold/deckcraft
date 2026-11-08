//! Room persistence: a native base plus an append-only journal (ADR 0005).
//!
//! Every accepted operation is appended before the next one is applied; a base is written
//! atomically (temporary file + rename) every `snapshot_every` operations and the journal is
//! rewritten to the tail after it. On start the latest base is loaded and the tail replayed
//! through the same `DocSession::apply` path, so versions, dedup and undo history come back.
//!
//! File ids are hashed for the directory name: the cabinet's id never becomes a path component.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use deckcraft_collab::DocSession;
use deckcraft_collab_proto::{Applied, OpEnvelope, encode_server};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const BASE: &str = "base.deckcraft";
const JOURNAL: &str = "journal.jsonl";
const META: &str = "meta.json";
const MAX_JOURNAL_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct JournalLine {
    actor: String,
    applied: Applied,
}

/// One document's files under `<root>/<hash>`.
pub struct Store {
    root: PathBuf,
    snapshot_every: u64,
}

impl Store {
    pub fn new(root: impl Into<PathBuf>, snapshot_every: u64) -> std::io::Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        Ok(Store { root, snapshot_every: snapshot_every.max(1) })
    }

    pub fn snapshot_every(&self) -> u64 {
        self.snapshot_every
    }

    fn room_dir(&self, file_id: &str) -> PathBuf {
        let digest = Sha256::digest(file_id.as_bytes());
        let name = digest.iter().map(|b| format!("{b:02x}")).collect::<String>();
        self.root.join(name)
    }

    /// Load the latest base and replay the journal. `Ok(None)` means the document was never
    /// stored (a fresh room); an unreadable or inconsistent store is an error, not an empty room.
    pub fn load(&self, file_id: &str) -> Result<Option<DocSession>, String> {
        let dir = self.room_dir(file_id);
        if !dir.exists() {
            return Ok(None);
        }
        let base = dir.join(BASE);
        if !base.exists() {
            return Ok(None);
        }
        let meta = fs::read(dir.join(META)).map_err(|e| format!("meta: {e}"))?;
        let stored: serde_json::Value = serde_json::from_slice(&meta).map_err(|e| format!("meta: {e}"))?;
        if stored.get("fileId").and_then(|v| v.as_str()) != Some(file_id) {
            return Err("the stored room belongs to another file".into());
        }
        let bytes = fs::read(&base).map_err(|e| format!("base: {e}"))?;
        let mut session = DocSession::open(file_id, &bytes).map_err(|e| format!("base: {e}"))?;
        let journal = dir.join(JOURNAL);
        if journal.exists() {
            let text = fs::read_to_string(&journal).map_err(|e| format!("journal: {e}"))?;
            for line in text.lines().filter(|l| !l.trim().is_empty()) {
                let entry: JournalLine = serde_json::from_str(line).map_err(|e| format!("journal: {e}"))?;
                let applied = entry.applied;
                if applied.version <= session.version() {
                    continue;
                }
                let Some(command) = applied.command else { continue };
                let op = OpEnvelope {
                    op_id: applied.op_id.clone(),
                    doc: file_id.to_string(),
                    base_version: applied.version.saturating_sub(1),
                    kind: deckcraft_collab::class_of(&command.payload),
                    target: command.target,
                    payload: command.payload,
                    history: None,
                };
                session.apply(&entry.actor, &op).map_err(|e| format!("replay {}: {}", applied.version, e.message))?;
            }
        }
        Ok(Some(session))
    }

    /// Write the base for `session` and drop the journal up to its version.
    pub fn snapshot(&self, file_id: &str, session: &mut DocSession) -> Result<(), String> {
        let dir = self.room_dir(file_id);
        fs::create_dir_all(&dir).map_err(|e| format!("store: {e}"))?;
        let state = session.take_snapshot().map_err(|e| format!("snapshot: {e}"))?.clone();
        let meta = serde_json::json!({"fileId": file_id, "version": state.version});
        fs::write(dir.join(META), meta.to_string()).map_err(|e| format!("meta: {e}"))?;
        let tmp = dir.join("base.deckcraft.tmp");
        {
            let mut f = fs::File::create(&tmp).map_err(|e| format!("base: {e}"))?;
            f.write_all(&state.bytes).map_err(|e| format!("base: {e}"))?;
            f.sync_all().map_err(|e| format!("base: {e}"))?;
        }
        fs::rename(&tmp, dir.join(BASE)).map_err(|e| format!("base: {e}"))?;
        self.trim_journal(&dir, state.version);
        Ok(())
    }

    /// Append one accepted operation, then take a base when the version reaches the boundary.
    pub fn record(&self, file_id: &str, session: &mut DocSession, applied: &Applied) -> Result<(), String> {
        let dir = self.room_dir(file_id);
        fs::create_dir_all(&dir).map_err(|e| format!("store: {e}"))?;
        let line = JournalLine { actor: applied.actor_id.clone(), applied: applied.clone() };
        let text = serde_json::to_string(&line).map_err(|e| e.to_string())?;
        let journal = dir.join(JOURNAL);
        let size = journal.metadata().map(|m| m.len()).unwrap_or(0);
        if size > MAX_JOURNAL_BYTES {
            // The base is the recovery point; a journal this long is a sign the snapshot failed.
            self.snapshot(file_id, session)?;
        }
        let mut f = fs::OpenOptions::new().create(true).append(true).open(&journal).map_err(|e| format!("journal: {e}"))?;
        f.write_all(text.as_bytes()).map_err(|e| format!("journal: {e}"))?;
        f.write_all(b"\n").map_err(|e| format!("journal: {e}"))?;
        f.sync_data().map_err(|e| format!("journal: {e}"))?;
        if applied.version.is_multiple_of(self.snapshot_every) {
            self.snapshot(file_id, session)?;
        }
        Ok(())
    }

    fn trim_journal(&self, dir: &Path, version: u64) {
        let journal = dir.join(JOURNAL);
        let Ok(text) = fs::read_to_string(&journal) else { return };
        let mut kept = String::new();
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            if let Ok(entry) = serde_json::from_str::<JournalLine>(line)
                && entry.applied.version > version
            {
                kept.push_str(line);
                kept.push('\n');
            }
        }
        let tmp = dir.join("journal.jsonl.tmp");
        if fs::write(&tmp, kept).is_ok() {
            let _ = fs::rename(tmp, journal);
        }
    }

    /// The journal of one room, for tests and operators (no content, only wire operations).
    pub fn journal_bytes(&self, file_id: &str) -> std::io::Result<Vec<u8>> {
        fs::read(self.room_dir(file_id).join(JOURNAL))
    }
}

/// Encode one applied operation for a log line without exposing slide content.
pub fn journal_summary(applied: &Applied) -> String {
    encode_server(&deckcraft_collab_proto::ServerMsg::Applied(applied.clone())).unwrap_or_else(|_| "{}".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_collab::SessionConfig;
    use deckcraft_model::Presentation;
    use serde_json::json;

    fn deck() -> Presentation {
        let mut s = deckcraft_engine::Session::with_new();
        s.apply_remote("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 100]})).expect("shape");
        s.doc().expect("doc").doc.as_ref().clone()
    }

    #[test]
    fn a_base_and_its_tail_replay_into_the_same_version_and_document() {
        let dir = std::env::temp_dir().join(format!("deckcraft-store-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let store = Store::new(&dir, 100).expect("store");
        let mut session = DocSession::from_document("file-9", deck(), SessionConfig::default()).expect("session");
        store.snapshot("file-9", &mut session).expect("snapshot");
        let doc = session.document().expect("doc");
        let slide = session.keys().slide_key(doc.slides[0].id).expect("slide").clone();
        let shape = session.keys().shape_key(doc.slides[0].id, doc.slides[0].shapes[0].id).expect("shape").clone();
        let op = OpEnvelope {
            op_id: "op-1".into(),
            doc: "file-9".into(),
            base_version: 0,
            kind: deckcraft_collab_proto::OpKind::Command,
            target: deckcraft_collab_proto::Target { slide: Some(slide), shape: Some(shape), ..Default::default() },
            payload: deckcraft_collab_proto::Payload::ShapeMove { x: 300.0, y: 120.0 },
            history: None,
        };
        let applied = session.apply("alice", &op).expect("apply");
        store.record("file-9", &mut session, &applied).expect("record");

        let restored = store.load("file-9").expect("load").expect("stored");
        assert_eq!(restored.version(), 1);
        assert_eq!(restored.document().expect("doc").slides[0].shapes[0].xfrm.expect("xfrm").x, 300.0);
        let _ = fs::remove_dir_all(&dir);
    }
}
