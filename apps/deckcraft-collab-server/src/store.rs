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
use deckcraft_collab_proto::{Applied, encode_server};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const BASE: &str = "base.deckcraft";
const JOURNAL: &str = "journal.jsonl";
const META: &str = "meta.json";
const MEDIA: &str = "media";
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
        if let Some(etag) = stored.get("fileEtag").and_then(|value| value.as_str()) {
            session.set_base_file_etag(etag);
        }
        let journal = dir.join(JOURNAL);
        if journal.exists() {
            let text = fs::read_to_string(&journal).map_err(|e| format!("journal: {e}"))?;
            for line in text.lines().filter(|l| !l.trim().is_empty()) {
                let entry: JournalLine = serde_json::from_str(line).map_err(|e| format!("journal: {e}"))?;
                let applied = entry.applied;
                if applied.version <= session.version() {
                    continue;
                }
                let Some(ref command) = applied.command else { continue };
                // A picture operation can only run with its bytes; they are content-addressed and
                // stored beside the journal, so load them before replaying the operation.
                if let deckcraft_collab_proto::Payload::PictureInsert { sha256, .. } = &command.payload {
                    let bytes = self.media(sha256).ok_or_else(|| format!("replay {}: media {sha256} is missing", applied.version))?;
                    session.insert_media(sha256, bytes).map_err(|e| format!("replay {}: {e}", applied.version))?;
                }
                session.replay(&entry.actor, &applied).map_err(|e| format!("replay {}: {e}", applied.version))?;
            }
        }
        Ok(Some(session))
    }

    /// Write the base for `session` and drop the journal up to its version.
    pub fn snapshot(&self, file_id: &str, session: &mut DocSession) -> Result<(), String> {
        let dir = self.room_dir(file_id);
        fs::create_dir_all(&dir).map_err(|e| format!("store: {e}"))?;
        let state = session.take_snapshot().map_err(|e| format!("snapshot: {e}"))?.clone();
        let mut meta = serde_json::json!({"fileId": file_id, "version": state.version});
        if let Some(etag) = session.base_file_etag() {
            meta["fileEtag"] = serde_json::Value::String(etag.to_string());
        }
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

    /// Record the file version the cabinet accepted after a commit, so a restart still compares
    /// the next commit against the copy the cabinet holds (ADR-0009 §4).
    pub fn set_file_version(&self, file_id: &str, etag: &str) -> Result<(), String> {
        let dir = self.room_dir(file_id);
        fs::create_dir_all(&dir).map_err(|e| format!("store: {e}"))?;
        let path = dir.join(META);
        let mut meta = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .unwrap_or_else(|| serde_json::json!({"fileId": file_id}));
        meta["fileId"] = serde_json::Value::String(file_id.to_string());
        meta["fileEtag"] = serde_json::Value::String(etag.to_string());
        let tmp = dir.join("meta.json.tmp");
        fs::write(&tmp, meta.to_string()).map_err(|e| format!("meta: {e}"))?;
        fs::rename(&tmp, &path).map_err(|e| format!("meta: {e}"))
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

    fn media_path(&self, sha: &str) -> Option<PathBuf> {
        deckcraft_collab::valid_sha(sha).then(|| self.root.join(MEDIA).join(sha))
    }

    /// Store one picture object. Content-addressed, so the name is the digest and a repeat is a
    /// no-op; the write is atomic (temporary file + rename).
    pub fn save_media(&self, sha: &str, bytes: &[u8]) -> Result<(), String> {
        let path = self.media_path(sha).ok_or_else(|| "the media name is not a sha256 digest".to_string())?;
        if path.exists() {
            return Ok(());
        }
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("media: {e}"))?;
        }
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tmp = path.with_extension(format!("{seq}.tmp"));
        {
            let mut f = fs::File::create(&tmp).map_err(|e| format!("media: {e}"))?;
            f.write_all(bytes).map_err(|e| format!("media: {e}"))?;
            f.sync_all().map_err(|e| format!("media: {e}"))?;
        }
        fs::rename(&tmp, &path).map_err(|e| format!("media: {e}"))
    }

    /// One stored picture, if this server has it.
    pub fn media(&self, sha: &str) -> Option<Vec<u8>> {
        let path = self.media_path(sha)?;
        fs::read(path).ok()
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
    use deckcraft_collab_proto::OpEnvelope;
    use deckcraft_model::Presentation;
    use serde_json::json;

    fn deck() -> Presentation {
        let mut s = deckcraft_engine::Session::with_new();
        s.apply_remote("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 100]})).expect("shape");
        s.doc().expect("doc").doc.as_ref().clone()
    }

    #[test]
    fn a_picture_survives_a_restart_through_the_media_store() {
        let dir = std::env::temp_dir().join(format!("deckcraft-store-media-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let store = Store::new(&dir, 100).expect("store");
        let mut session = DocSession::from_document("file-p", deck(), SessionConfig::default()).expect("session");
        let picture = png();
        let sha = session.put_media("red.png", picture.clone()).expect("media");
        store.save_media(&sha, &picture).expect("store media");
        store.snapshot("file-p", &mut session).expect("base at version 0");
        let doc = session.document().expect("doc").clone();
        let slide = session.keys().slide_key(doc.slides[0].id).expect("slide").clone();
        let op = OpEnvelope {
            op_id: "pic-1".into(),
            doc: "file-p".into(),
            base_version: 0,
            kind: deckcraft_collab_proto::OpKind::Structural,
            target: deckcraft_collab_proto::Target { slide: Some(slide), ..Default::default() },
            payload: deckcraft_collab_proto::Payload::PictureInsert {
                sha256: sha.clone(),
                name: "red.png".into(),
                rect: Some([40.0, 40.0, 60.0, 60.0]),
            },
            history: None,
        };
        let applied = session.apply("alice", &op).expect("apply");
        store.record("file-p", &mut session, &applied).expect("record");

        // Only the journal is written (one operation, no snapshot); the replay needs the picture.
        let restored = store.load("file-p").expect("load").expect("stored");
        assert_eq!(restored.version(), 1);
        assert!(restored.media(&sha).is_some(), "the media object came back");
        let doc = restored.document().expect("doc");
        assert!(doc.slides[0].shapes.iter().any(|s| matches!(s.kind, deckcraft_model::ShapeKind::Picture { .. })), "the picture was replayed");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_undo_operation_replays_from_the_journal() {
        let dir = std::env::temp_dir().join(format!("deckcraft-store-undo-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let store = Store::new(&dir, 100).expect("store");
        let mut session = DocSession::from_document("file-u", deck(), SessionConfig::default()).expect("session");
        store.snapshot("file-u", &mut session).expect("snapshot");
        let doc = session.document().expect("doc").clone();
        let shapes_before = doc.slides[0].shapes.len();
        let slide = session.keys().slide_key(doc.slides[0].id).expect("slide").clone();
        let delete = OpEnvelope {
            op_id: "del-1".into(),
            doc: "file-u".into(),
            base_version: 0,
            kind: deckcraft_collab_proto::OpKind::Structural,
            target: deckcraft_collab_proto::Target { slide: Some(slide), ..Default::default() },
            payload: deckcraft_collab_proto::Payload::SlideDelete {},
            history: None,
        };
        let applied = session.apply("alice", &delete).expect("delete");
        store.record("file-u", &mut session, &applied).expect("record delete");
        let undo = session.undo("alice", "undo-1").expect("undo");
        assert!(matches!(undo.payload, deckcraft_collab_proto::Payload::SlideRestore { .. }));
        let applied = session.apply("alice", &undo).expect("undo");
        store.record("file-u", &mut session, &applied).expect("record undo");
        assert_eq!(session.version(), 2);

        // Replay runs the confirmed commands, not the history checks: the restore must survive.
        let restored = store.load("file-u").expect("load").expect("stored");
        assert_eq!(restored.version(), 2);
        let doc = restored.document().expect("doc");
        assert_eq!(doc.slides.len(), 1);
        assert_eq!(doc.slides[0].shapes.len(), shapes_before, "the slide and its shapes came back");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_shape_undo_operation_replays_from_the_journal() {
        let dir = std::env::temp_dir().join(format!("deckcraft-store-shape-undo-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let store = Store::new(&dir, 100).expect("store");
        let mut session = DocSession::from_document("file-s", deck(), SessionConfig::default()).expect("session");
        store.snapshot("file-s", &mut session).expect("snapshot");
        let doc = session.document().expect("doc").clone();
        let slide_id = doc.slides[0].id;
        let shape_id = doc.slides[0].shapes[0].id;
        let shapes_before = doc.slides[0].shapes.len();
        let key = session.keys().shape_key(slide_id, shape_id).expect("key").clone();
        let slide = session.keys().slide_key(slide_id).expect("slide").clone();
        let delete = OpEnvelope {
            op_id: "del-1".into(),
            doc: "file-s".into(),
            base_version: 0,
            kind: deckcraft_collab_proto::OpKind::Structural,
            target: deckcraft_collab_proto::Target { slide: Some(slide), shapes: vec![key.clone()], ..Default::default() },
            payload: deckcraft_collab_proto::Payload::ShapeDelete {},
            history: None,
        };
        let applied = session.apply("alice", &delete).expect("delete");
        store.record("file-s", &mut session, &applied).expect("record delete");
        let undo = session.undo("alice", "undo-1").expect("undo");
        assert!(matches!(undo.payload, deckcraft_collab_proto::Payload::ShapeRestore { .. }));
        let applied = session.apply("alice", &undo).expect("undo");
        store.record("file-s", &mut session, &applied).expect("record undo");
        assert_eq!(session.version(), 2);

        // Replay runs the confirmed command: the shape comes back under the key it had.
        let restored = store.load("file-s").expect("load").expect("stored");
        assert_eq!(restored.version(), 2);
        let doc = restored.document().expect("doc");
        assert_eq!(doc.slides[0].shapes.len(), shapes_before, "the shape came back");
        assert_eq!(doc.slides[0].shapes[0].id, shape_id, "at its place in the z-order");
        assert_eq!(restored.keys().shape_key(doc.slides[0].id, shape_id), Some(&key), "under the same key");
        let _ = fs::remove_dir_all(&dir);
    }

    fn png() -> Vec<u8> {
        vec![
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 8, 0, 0, 0, 8, 8, 2, 0, 0, 0, 75, 109, 41, 220, 0, 0, 0, 17, 73,
            68, 65, 84, 120, 156, 99, 184, 163, 161, 129, 21, 49, 12, 45, 9, 0, 123, 123, 75, 1, 251, 162, 81, 240, 0, 0, 0, 0, 73, 69, 78, 68, 174,
            66, 96, 130,
        ]
    }

    #[test]
    fn the_file_version_the_cabinet_accepted_survives_a_restart() {
        let dir = std::env::temp_dir().join(format!("deckcraft-store-etag-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let store = Store::new(&dir, 100).expect("store");
        let mut session = DocSession::from_document("file-e", deck(), SessionConfig::default()).expect("session");
        store.snapshot("file-e", &mut session).expect("base at version 0");
        assert!(store.load("file-e").expect("load").expect("stored").base_file_etag().is_none(), "a fresh room has no file version");

        session.accept_file_version("etag-1");
        store.set_file_version("file-e", "etag-1").expect("etag");
        let restored = store.load("file-e").expect("load").expect("stored");
        assert_eq!(restored.base_file_etag(), Some("etag-1"), "the next commit compares against the cabinet's copy");

        // A later snapshot keeps the version with the base.
        store.snapshot("file-e", &mut session).expect("snapshot");
        let restored = store.load("file-e").expect("load").expect("stored");
        assert_eq!(restored.base_file_etag(), Some("etag-1"));
        let _ = fs::remove_dir_all(&dir);
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
