//! One document session on the authoritative side (ADR 0001–0005, 0009, 0010), without a
//! transport.
//!
//! `DocSession` is the server's brain: it owns the engine session, applies operations in one
//! order, dedups on `opId`, keeps a journal tail and snapshots, and builds the commit file. The
//! WebSocket layer (a separate crate) only moves [`deckcraft_collab_proto`] messages in and out,
//! so all of this is testable headless.
//!
//! Snapshots are the native `.deckcraft` format (`crates/format`): the model, the pass-through
//! XML and the media bytes, without the file-id renumbering a `.pptx` export does (ADR-0005).
//! Commit is the separate `.pptx` export (ADR-0009).

use std::collections::{HashMap, VecDeque};

use deckcraft_collab_proto::{Applied, CommitResult, ConfirmedCommand, Hint, OpEnvelope, Rejected, SnapshotMode, SnapshotMsg, SnapshotRef};
use deckcraft_engine::Session;
use deckcraft_engine::cmd::file::{Damage, LoadedFile};
use deckcraft_model::{MediaClip, Presentation, Shape, ShapeKind};
use serde_json::json;

use crate::apply::{Adapter, Rejection};

/// Tunables of one session.
#[derive(Clone, Copy, Debug)]
pub struct SessionConfig {
    /// Operations between snapshots.
    pub snapshot_every: u64,
    /// Remembered `opId`s for dedup (ADR 0003).
    pub dedup_window: usize,
    /// How many applied operations the journal tail keeps between snapshots.
    pub max_tail: usize,
}

impl Default for SessionConfig {
    fn default() -> Self {
        SessionConfig { snapshot_every: 200, dedup_window: 10_000, max_tail: 10_000 }
    }
}

/// A stored base: the native file and the version it represents.
#[derive(Clone, Debug)]
pub struct SnapshotState {
    pub version: u64,
    pub bytes: Vec<u8>,
    pub cursor: String,
}

/// What a commit produced. The bytes are the `.pptx` the cabinet stores.
#[derive(Clone, Debug, PartialEq)]
pub struct Committed {
    pub version: u64,
    pub bytes: Vec<u8>,
    pub warnings: Vec<String>,
    /// The file version this commit was built against, when the session knows it.
    pub file_version: Option<String>,
}

/// Why a commit did not produce a file.
#[derive(Clone, Debug, PartialEq)]
pub enum CommitError {
    /// The file changed since the session opened it, or since this session last wrote it:
    /// the cabinet answers `409` and offers "save a copy" instead of overwriting (ADR-0009 §4).
    Conflict { expected: String, current: Option<String> },
    /// The writer refused the model; the session is untouched.
    Build(String),
}

impl std::fmt::Display for CommitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommitError::Conflict { expected, current } => match current {
                Some(current) => write!(f, "the file changed: expected {expected}, it is {current}"),
                None => write!(f, "the file changed: expected {expected}, the session has no version"),
            },
            CommitError::Build(message) => f.write_str(message),
        }
    }
}

/// One document served to several participants.
pub struct DocSession {
    file_id: String,
    engine: Session,
    adapter: Adapter,
    version: u64,
    dedup: VecDeque<String>,
    applied: HashMap<String, Applied>,
    tail: Vec<Applied>,
    snapshot: Option<SnapshotState>,
    config: SessionConfig,
    /// The version of the file this session was opened from, for `409` on commit (ADR-0009).
    base_file_etag: Option<String>,
    /// The last file this session produced, so a repeated commit is idempotent (ADR-0009 §6).
    last_commit: Option<Committed>,
}

impl DocSession {
    /// Open a document from `.pptx` (or native `.deckcraft`) bytes.
    pub fn open(file_id: impl Into<String>, bytes: &[u8]) -> Result<Self, String> {
        let file_id = file_id.into();
        let path = format!("{file_id}.session");
        let loaded = deckcraft_engine::cmd::file::loaded_file(&path, bytes).map_err(|e| e.to_string())?;
        let mut engine = Session::new();
        let token = engine.stage_loaded(loaded);
        engine.execute("file.adopt", &json!({"path": path, "token": token})).map_err(|e| e.to_string())?;
        Ok(Self::with_engine(file_id, engine, SessionConfig::default()))
    }

    /// A session over an already built document (tests, imports from another source).
    pub fn from_document(file_id: impl Into<String>, doc: Presentation, config: SessionConfig) -> Result<Self, String> {
        let file_id = file_id.into();
        let path = format!("{file_id}.session");
        let mut engine = Session::new();
        let loaded = LoadedFile { path: path.clone(), doc, native: true, damaged: Damage { media: vec![], slides: vec![] } };
        let token = engine.stage_loaded(loaded);
        engine.execute("file.adopt", &json!({"path": path, "token": token})).map_err(|e| e.to_string())?;
        Ok(Self::with_engine(file_id, engine, config))
    }

    fn with_engine(file_id: String, engine: Session, config: SessionConfig) -> Self {
        let doc = engine.doc().map(|d| d.doc.as_ref().clone()).unwrap_or_default();
        let mut adapter = Adapter::new(format!("{file_id}-"));
        adapter.join(&doc);
        DocSession {
            file_id,
            engine,
            adapter,
            version: 0,
            dedup: VecDeque::new(),
            applied: HashMap::new(),
            tail: Vec::new(),
            snapshot: None,
            config,
            base_file_etag: None,
            last_commit: None,
        }
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn file_id(&self) -> &str {
        &self.file_id
    }

    pub fn set_base_file_etag(&mut self, etag: impl Into<String>) {
        self.base_file_etag = Some(etag.into());
    }

    pub fn base_file_etag(&self) -> Option<&str> {
        self.base_file_etag.as_deref()
    }

    pub fn document(&self) -> Option<&Presentation> {
        self.engine.doc().ok().map(|d| d.doc.as_ref())
    }

    /// The session-scoped keys issued so far; the transport sends them with the welcome and the
    /// snapshot so a client can address objects (ADR 0002).
    pub fn keys(&self) -> &crate::keys::KeyMap {
        self.adapter.keys()
    }

    /// Apply one operation in the session's order and return the authoritative result.
    ///
    /// A repeat of an `opId` returns the stored answer and changes nothing (ADR 0003): relative
    /// operations are not idempotent, so a resend must not run twice.
    pub fn apply(&mut self, actor: &str, op: &OpEnvelope) -> Result<Applied, Rejected> {
        if let Some(done) = self.applied.get(&op.op_id) {
            return Ok(done.clone());
        }
        if op.doc != self.file_id {
            return Err(refuse(op, Rejection::invalid("the operation is for another document", Some(self.version))));
        }
        // Text operations carry a character offset; turn it into the position this operation
        // actually has after everything that arrived since its base (ADR 0004, merge class).
        let mut rebased: Option<OpEnvelope> = None;
        if matches!(op.payload, deckcraft_collab_proto::Payload::TextInsert { .. } | deckcraft_collab_proto::Payload::TextDelete { .. })
            && op.base_version < self.version
        {
            let mut adjusted = op.clone();
            self.rebase_text(&mut adjusted)?;
            rebased = Some(adjusted);
        }
        let op = rebased.as_ref().unwrap_or(op);
        let delta = match self.adapter.apply(&mut self.engine, self.version, actor, op) {
            Ok(delta) => delta,
            Err(rejection) => return Err(refuse(op, rejection)),
        };
        self.version = self.version.saturating_add(1);
        let out = Applied {
            op_id: op.op_id.clone(),
            version: self.version,
            actor_id: actor.to_string(),
            command: Some(ConfirmedCommand { cmd: op.payload.command().to_string(), target: op.target.clone(), payload: op.payload.clone() }),
            delta,
        };
        self.remember(&out);
        self.tail.push(out.clone());
        if self.tail.len() > self.config.max_tail {
            self.tail.remove(0);
        }
        self.maybe_snapshot();
        Ok(out)
    }

    /// Move a text operation's offsets past the operations that arrived after its base. The
    /// journal tail is the source: inserts shift positions right, deletes pull them left (and
    /// clamp positions inside a removed range). Ties go to the earlier operation, so two people
    /// typing at one caret both keep their text.
    fn rebase_text(&mut self, op: &mut OpEnvelope) -> Result<(), Rejected> {
        use deckcraft_collab_proto::Payload;
        if self.tail.is_empty() && self.version > op.base_version {
            return Err(refuse(op, Rejection::stale("the text history was compacted; resync first", Some(self.version))));
        }
        if let Some(first) = self.tail.first()
            && op.base_version.saturating_add(1) < first.version
            && self.version > op.base_version
        {
            return Err(refuse(op, Rejection::stale("the text history was compacted; resync first", Some(self.version))));
        }
        let shape = op.target.shape.clone();
        let adjust_point = |point: &mut u32, payload: &Payload| match payload {
            Payload::TextInsert { at, text } if shape.as_ref() == op.target.shape.as_ref() => {
                if *at <= *point {
                    *point = point.saturating_add(text.chars().count() as u32);
                }
            }
            Payload::TextDelete { from, to } if shape.as_ref() == op.target.shape.as_ref() => {
                if *to <= *point {
                    *point = point.saturating_sub(to.saturating_sub(*from));
                } else if *from < *point {
                    *point = *from;
                }
            }
            _ => {}
        };
        let tail: Vec<Applied> = self.tail.iter().filter(|a| a.version > op.base_version).cloned().collect();
        for applied in tail {
            let Some(command) = applied.command else { continue };
            if command.target.shape != op.target.shape {
                continue;
            }
            match &mut op.payload {
                Payload::TextInsert { at, .. } => adjust_point(at, &command.payload),
                Payload::TextDelete { from, to } => {
                    adjust_point(from, &command.payload);
                    adjust_point(to, &command.payload);
                }
                _ => {}
            }
        }
        if let Payload::TextDelete { from, to } = op.payload
            && to <= from
        {
            return Err(refuse(op, Rejection::invalid("the deleted text was already changed", Some(self.version))));
        }
        Ok(())
    }

    /// What a reconnecting client gets: the base cursor and the journal tail after `last_version`.
    pub fn resync(&self, last_version: u64) -> SnapshotMsg {
        let (mode, cursor) = match &self.snapshot {
            Some(snapshot) if last_version < snapshot.version => (SnapshotMode::BaseAndTail, snapshot.cursor.clone()),
            _ => (SnapshotMode::BaseAndTail, format!("tail@{}", last_version.min(self.version))),
        };
        SnapshotMsg {
            version: self.version,
            snapshot: SnapshotRef { mode, cursor },
            tail: self.tail.iter().filter(|a| a.version > last_version).cloned().collect(),
        }
    }

    /// The current base, if one has been taken.
    pub fn snapshot(&self) -> Option<&SnapshotState> {
        self.snapshot.as_ref()
    }

    /// Take a base now (also happens every `snapshot_every` operations).
    pub fn take_snapshot(&mut self) -> Result<&SnapshotState, String> {
        let doc = self.engine.doc().map_err(|e| e.to_string())?;
        let bytes = deckcraft_format::save(&doc.doc).map_err(|e| e.to_string())?;
        let cursor = format!("base@{}", self.version);
        self.snapshot = Some(SnapshotState { version: self.version, bytes, cursor });
        // The tail before the base is no longer needed to rebuild the state.
        self.tail.retain(|a| a.version > self.version);
        self.snapshot.as_ref().ok_or_else(|| "the snapshot was just written".to_string())
    }

    fn maybe_snapshot(&mut self) {
        if self.config.snapshot_every == 0 || !self.version.is_multiple_of(self.config.snapshot_every) {
            return;
        }
        // A snapshot that cannot be written leaves the journal intact; the session keeps serving
        // and tries again at the next boundary.
        let _ = self.take_snapshot();
    }

    /// Build the operation that takes back `actor`'s last step at the current head.
    ///
    /// The answer is an ordinary operation: the caller applies it through [`Self::apply`], so it
    /// is ordered, versioned and checked exactly like any other (ADR 0010 §2).
    pub fn undo(&mut self, actor: &str, op_id: &str) -> Result<OpEnvelope, Rejection> {
        let mut op = self.adapter.undo(&self.engine, self.version, actor, op_id)?;
        op.doc = self.file_id.clone();
        Ok(op)
    }

    /// Build the operation that re-applies `actor`'s last undone step.
    pub fn redo(&mut self, actor: &str, op_id: &str) -> Result<OpEnvelope, Rejection> {
        let mut op = self.adapter.redo(&self.engine, self.version, actor, op_id)?;
        op.doc = self.file_id.clone();
        Ok(op)
    }

    /// The `.pptx` the cabinet receives (ADR-0009) and the list of what will not survive it.
    ///
    /// No version check: the caller decided that it may write. Use [`Self::commit_checked`] for
    /// the cabinet path, where an external change must become a `409`.
    pub fn commit(&self) -> Result<(Vec<u8>, Vec<String>), String> {
        Ok((self.commit_bytes()?, self.commit_warnings()?))
    }

    fn commit_bytes(&self) -> Result<Vec<u8>, String> {
        let doc = self.engine.doc().map_err(|e| e.to_string())?;
        deckcraft_pptx::export_as(&doc.doc, "pptx").map_err(|e| e.to_string())
    }

    fn commit_warnings(&self) -> Result<Vec<String>, String> {
        let doc = self.engine.doc().map_err(|e| e.to_string())?;
        Ok(warnings(&doc.doc))
    }

    /// Commit with the cabinet's version check (ADR-0009).
    ///
    /// - `expected` is the file version the cabinet believes is current; when it does not match
    ///   what the session knows, the answer is `Conflict` (`409`) and nothing is written.
    /// - A repeated commit of the same document version returns the same bytes and does not
    ///   rebuild them (ADR-0009 §6).
    pub fn commit_checked(&mut self, expected: Option<&str>) -> Result<Committed, CommitError> {
        if let Some(expected) = expected
            && self.base_file_etag.as_deref() != Some(expected)
        {
            return Err(CommitError::Conflict { expected: expected.to_string(), current: self.base_file_etag.clone() });
        }
        if let Some(done) = &self.last_commit
            && done.version == self.version
        {
            return Ok(done.clone());
        }
        let bytes = self.commit_bytes().map_err(CommitError::Build)?;
        let warnings = self.commit_warnings().map_err(CommitError::Build)?;
        let done = Committed { version: self.version, bytes, warnings, file_version: self.base_file_etag.clone() };
        self.last_commit = Some(done.clone());
        Ok(done)
    }

    /// The cabinet assigned a new file version after a commit; the next commit checks against it.
    pub fn accept_file_version(&mut self, version: impl Into<String>) {
        self.base_file_etag = Some(version.into());
    }

    /// A commit answer with the file's hash placeholder filled by the transport, if it has one.
    pub fn commit_result(&self, sha256: impl Into<String>) -> Result<CommitResult, String> {
        let warnings = self.commit_warnings()?;
        Ok(CommitResult {
            sha256: sha256.into(),
            version: self.version,
            file_version: self.base_file_etag.clone(),
            bytes_base64: None,
            blob: None,
            warnings,
        })
    }

    fn remember(&mut self, applied: &Applied) {
        self.applied.insert(applied.op_id.clone(), applied.clone());
        self.dedup.push_back(applied.op_id.clone());
        while self.dedup.len() > self.config.dedup_window {
            if let Some(old) = self.dedup.pop_front() {
                self.applied.remove(&old);
            }
        }
    }
}

fn refuse(op: &OpEnvelope, rejection: Rejection) -> Rejected {
    let retryable = rejection.retryable;
    Rejected {
        op_id: op.op_id.clone(),
        code: rejection.code,
        message: rejection.message,
        retryable,
        hint: rejection.hint_version.map(|version| Hint { version }),
    }
}

/// What a `.pptx` export will not keep, by slide and object — not a boolean flag (ADR-0009 §2).
///
/// Only known losses are listed; an empty list means nothing DeckCraft knows about is dropped.
pub fn warnings(doc: &Presentation) -> Vec<String> {
    let mut out = Vec::new();
    for (index, slide) in doc.slides.iter().enumerate() {
        let number = index.saturating_add(1);
        for shape in &slide.shapes {
            collect_warnings(shape, number, &mut out, 0);
        }
    }
    out
}

fn collect_warnings(shape: &Shape, slide: usize, out: &mut Vec<String>, depth: usize) {
    if depth > 64 {
        return;
    }
    let name = if shape.name.is_empty() { format!("#{}", shape.id.0) } else { shape.name.clone() };
    match &shape.kind {
        ShapeKind::Opaque { label, .. } => {
            out.push(format!("slide {slide}: `{name}` ({label}) is kept as its original XML and cannot be edited"));
        }
        ShapeKind::Model3d { .. } => {
            out.push(format!("slide {slide}: `{name}` is a 3-D model: kept in the file, not drawn"));
        }
        ShapeKind::Media(clip) if media_options_differ(clip) => {
            out.push(format!("slide {slide}: `{name}` has playback options (trim, fade, volume, loop, bookmarks) that the writer may not keep"));
        }
        _ => {}
    }
    if shape.diagram.as_ref().is_some_and(|d| d.imported) {
        out.push(format!("slide {slide}: `{name}` is SmartArt imported as graphics; editing its text regenerates the diagram"));
    }
    for child in shape.children() {
        collect_warnings(child, slide, out, depth + 1);
    }
}

fn media_options_differ(clip: &MediaClip) -> bool {
    let defaults = MediaClip::default();
    clip.trim_start_ms != defaults.trim_start_ms
        || clip.trim_end_ms != defaults.trim_end_ms
        || clip.fade_in_ms != defaults.fade_in_ms
        || clip.fade_out_ms != defaults.fade_out_ms
        || (clip.volume - defaults.volume).abs() > f64::EPSILON
        || clip.autoplay != defaults.autoplay
        || clip.loop_play != defaults.loop_play
        || clip.rewind != defaults.rewind
        || clip.play_across_slides != defaults.play_across_slides
        || clip.hide_while_not_playing != defaults.hide_while_not_playing
        || clip.full_screen != defaults.full_screen
        || !clip.bookmarks.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_collab_proto::{OpKind, Payload, RejectCode, ShapeKey, SlideKey, Target};

    fn deck() -> Presentation {
        let mut s = Session::with_new();
        let title = s.doc().expect("doc").doc.slides[0].id;
        s.apply_remote("slide.new", &json!({"layout": "blank", "at": 1})).expect("blank");
        s.apply_remote("slide.delete", &json!({"ids": [title.0]})).expect("drop title");
        s.apply_remote("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 100]})).expect("insert");
        s.doc().expect("doc").doc.as_ref().clone()
    }

    fn keys(session: &DocSession) -> (SlideKey, ShapeKey) {
        // The first slide and its first shape, as the adapter issued them.
        let doc = session.document().expect("doc");
        let slide = doc.slides.first().expect("slide");
        let key = session.adapter.keys();
        (
            key.slide_key(slide.id).expect("slide key").clone(),
            key.shape_key(slide.id, slide.shapes.first().expect("shape").id).expect("shape key").clone(),
        )
    }

    fn move_op(slide: &SlideKey, shape: &ShapeKey, x: f64, base: u64, op_id: &str) -> OpEnvelope {
        OpEnvelope {
            op_id: op_id.into(),
            doc: "d".into(),
            base_version: base,
            kind: OpKind::Command,
            target: Target { slide: Some(slide.clone()), shape: Some(shape.clone()), ..Target::default() },
            payload: Payload::ShapeMove { x, y: 100.0 },
            history: None,
        }
    }

    #[test]
    fn a_repeated_op_id_returns_the_same_answer_and_changes_nothing() {
        let mut s = DocSession::from_document("d", deck(), SessionConfig::default()).expect("session");
        let (slide, shape) = keys(&s);
        let op = move_op(&slide, &shape, 300.0, 0, "op-1");
        let first = s.apply("a", &op).expect("first");
        let doc_after = s.document().expect("doc").clone();
        let again = s.apply("a", &op).expect("dedup");
        assert_eq!(first, again);
        assert_eq!(s.version(), 1);
        assert_eq!(*s.document().expect("doc"), doc_after, "a resend does not apply twice");
    }

    #[test]
    fn versions_advance_in_apply_order() {
        let mut s = DocSession::from_document("d", deck(), SessionConfig::default()).expect("session");
        let (slide, shape) = keys(&s);
        for (i, x) in [200.0, 300.0, 400.0].iter().enumerate() {
            let out = s.apply("a", &move_op(&slide, &shape, *x, i as u64, &format!("op-{i}"))).expect("apply");
            assert_eq!(out.version, i as u64 + 1);
        }
        assert_eq!(s.version(), 3);
    }

    #[test]
    fn a_structural_operation_with_a_stale_base_is_refused_with_a_hint() {
        let mut s = DocSession::from_document("d", deck(), SessionConfig::default()).expect("session");
        let (slide, shape) = keys(&s);
        s.apply("a", &move_op(&slide, &shape, 300.0, 0, "op-1")).expect("apply");
        let stale = OpEnvelope {
            op_id: "op-2".into(),
            doc: "d".into(),
            base_version: 0,
            kind: OpKind::Structural,
            target: Target { slide: Some(slide), shape: Some(shape), ..Target::default() },
            payload: Payload::ShapeDelete {},
            history: None,
        };
        let err = s.apply("a", &stale).expect_err("stale");
        assert_eq!(err.code, RejectCode::Stale);
        assert!(err.retryable);
        assert_eq!(err.hint.map(|h| h.version), Some(1));
        assert_eq!(s.version(), 1, "a refusal does not bump the version");
    }

    #[test]
    fn an_operation_for_another_document_is_refused() {
        let mut s = DocSession::from_document("d", deck(), SessionConfig::default()).expect("session");
        let (slide, shape) = keys(&s);
        let mut op = move_op(&slide, &shape, 300.0, 0, "op-1");
        op.doc = "other".into();
        let err = s.apply("a", &op).expect_err("wrong doc");
        assert_eq!(err.code, RejectCode::Invalid);
    }

    #[test]
    fn a_snapshot_reloads_into_the_same_document() {
        let mut s = DocSession::from_document("d", deck(), SessionConfig { snapshot_every: 1, ..SessionConfig::default() }).expect("session");
        let (slide, shape) = keys(&s);
        s.apply("a", &move_op(&slide, &shape, 300.0, 0, "op-1")).expect("apply");
        let snapshot = s.snapshot().expect("snapshot").clone();
        let reloaded = DocSession::open("d", &snapshot.bytes).expect("reopen");
        assert_eq!(*reloaded.document().expect("doc"), *s.document().expect("doc"));
    }

    #[test]
    fn resync_gives_the_tail_after_the_client_version() {
        let mut s = DocSession::from_document("d", deck(), SessionConfig { snapshot_every: 2, ..SessionConfig::default() }).expect("session");
        let (slide, shape) = keys(&s);
        for i in 0..4 {
            s.apply("a", &move_op(&slide, &shape, 200.0 + i as f64 * 10.0, i, &format!("op-{i}"))).expect("apply");
        }
        // A snapshot was taken at version 4, so a client from nothing gets the base and no tail.
        assert_eq!(s.snapshot().map(|x| x.version), Some(4));
        let at_base = s.resync(0);
        assert_eq!(at_base.version, 4);
        assert!(at_base.tail.is_empty(), "the base already covers every operation");
        assert!(at_base.snapshot.cursor.starts_with("base@4"));

        // One more operation: a client at 4 gets exactly that one.
        s.apply("a", &move_op(&slide, &shape, 500.0, 4, "op-4")).expect("apply");
        let caught = s.resync(4);
        assert_eq!(caught.version, 5);
        assert_eq!(caught.tail.len(), 1);
        assert_eq!(caught.tail[0].version, 5);
        assert!(s.resync(5).tail.is_empty(), "up to date");
    }

    #[test]
    fn commit_writes_a_pptx_that_reads_back() {
        let s = DocSession::from_document("d", deck(), SessionConfig::default()).expect("session");
        let (bytes, warnings) = s.commit().expect("commit");
        assert!(warnings.is_empty(), "{warnings:?}");
        let reopened = deckcraft_pptx::import(&bytes).expect("reimport");
        assert_eq!(reopened.slides.len(), s.document().expect("doc").slides.len());
        assert_eq!(reopened.slides[0].shapes.len(), s.document().expect("doc").slides[0].shapes.len());
    }

    #[test]
    fn commit_lists_what_will_not_survive() {
        let mut doc = deck();
        {
            let slide = std::sync::Arc::make_mut(&mut doc.slides[0]);
            slide.shapes.push(Shape {
                id: deckcraft_model::ShapeId(999),
                name: "Embedded object".into(),
                kind: ShapeKind::Opaque { xml: "<p:oleObj/>".into(), preview: None, label: "OLE".into(), parts: vec![] },
                ..Default::default()
            });
        }
        let s = DocSession::from_document("d", doc, SessionConfig::default()).expect("session");
        let (_, warnings) = s.commit().expect("commit");
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("slide 1"), "{warnings:?}");
        assert!(warnings[0].contains("original XML"), "{warnings:?}");
    }

    #[test]
    fn commit_is_refused_when_the_file_changed_under_the_session() {
        let mut s = DocSession::from_document("d", deck(), SessionConfig::default()).expect("session");
        s.accept_file_version("etag-1");
        let err = s.commit_checked(Some("etag-0")).expect_err("stale file version");
        assert_eq!(err, CommitError::Conflict { expected: "etag-0".into(), current: Some("etag-1".into()) });
        assert!(s.commit_checked(Some("etag-1")).is_ok(), "the current version may be written");
    }

    #[test]
    fn a_repeated_commit_of_one_version_is_idempotent() {
        let mut s = DocSession::from_document("d", deck(), SessionConfig::default()).expect("session");
        let first = s.commit_checked(None).expect("commit");
        let again = s.commit_checked(None).expect("commit again");
        assert_eq!(first, again, "same bytes, no second build");
        assert_eq!(s.version(), 0, "a commit does not advance the document version");
    }

    #[test]
    fn a_new_operation_makes_the_next_commit_fresh() {
        let mut s = DocSession::from_document("d", deck(), SessionConfig::default()).expect("session");
        let (slide, shape) = keys(&s);
        let first = s.commit_checked(None).expect("commit");
        s.apply("a", &move_op(&slide, &shape, 300.0, 0, "op-1")).expect("apply");
        let second = s.commit_checked(None).expect("commit");
        assert_eq!(second.version, 1);
        assert_ne!(first.bytes, second.bytes, "a moved shape changes the file");
    }
}
