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

use base64::Engine as _;
use deckcraft_collab_proto::{
    Applied, BaseSnapshot, CommitResult, ConfirmedCommand, Hint, OpEnvelope, Rejected, SnapshotMode, SnapshotMsg, SnapshotRef,
};
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
    /// Largest native base sent inline to a client whose tail was compacted. Above it the
    /// snapshot names the base but carries no bytes, and the client reopens the file instead.
    pub max_base_inline: usize,
}

impl Default for SessionConfig {
    fn default() -> Self {
        SessionConfig { snapshot_every: 200, dedup_window: 10_000, max_tail: 10_000, max_base_inline: 8 << 20 }
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

    /// Replace the session's tunables (server configuration: snapshot frequency, base size).
    pub fn set_config(&mut self, config: SessionConfig) {
        self.config = config;
    }

    pub fn config(&self) -> SessionConfig {
        self.config
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

    /// Store an uploaded picture and return its digest, or explain why it was refused. The
    /// server calls this before accepting a `picture.insert` that refers to it.
    pub fn put_media(&mut self, name: &str, bytes: Vec<u8>) -> Result<String, crate::MediaError> {
        self.adapter.media_mut().put(name, bytes)
    }

    /// Insert bytes that came with their digest (a journal replay, or a client that fetched
    /// them). The digest is verified, never trusted.
    pub fn insert_media(&mut self, sha: &str, bytes: Vec<u8>) -> Result<(), crate::MediaError> {
        self.adapter.media_mut().insert(sha, bytes)
    }

    /// The bytes named by `sha`, if this session has them.
    pub fn media(&self, sha: &str) -> Option<std::sync::Arc<Vec<u8>>> {
        self.adapter.media().get(sha)
    }

    /// Every picture this session holds, for persistence.
    pub fn media_entries(&self) -> Vec<(String, std::sync::Arc<Vec<u8>>)> {
        self.adapter.media().entries()
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
        // Text operations carry a character offset and/or a table cell address; turn both into
        // the position and cell this operation actually has after everything that arrived since
        // its base (ADR 0004, merge class).
        let rebases = match &op.payload {
            deckcraft_collab_proto::Payload::TextInsert { .. } | deckcraft_collab_proto::Payload::TextDelete { .. } => true,
            deckcraft_collab_proto::Payload::TextSet { cell, .. } => cell.is_some(),
            // Notes edits carry a body offset like shape text; their address is the slide, so a
            // slide move or a shape edit never shifts them.
            deckcraft_collab_proto::Payload::NotesInsert { .. } | deckcraft_collab_proto::Payload::NotesDelete { .. } => true,
            _ => false,
        };
        let mut rebased: Option<OpEnvelope> = None;
        if rebases && op.base_version < self.version {
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

    /// Replay one already-accepted operation from the journal.
    ///
    /// The stored `Applied` already carries the authoritative command: replay must not re-run the
    /// history checks (there is no client undo on this path) and must not record a new undo step,
    /// but it fills the same version, dedup window and journal tail as [`Self::apply`]. This is
    /// what lets an undo operation (an exact `shape_field`, a group's `shape_fields`, a
    /// `shape_restore` or a `slide_restore`) survive a restart.
    pub fn replay(&mut self, actor: &str, stored: &Applied) -> Result<(), String> {
        let Some(command) = &stored.command else { return Ok(()) };
        let op = OpEnvelope {
            op_id: stored.op_id.clone(),
            doc: self.file_id.clone(),
            base_version: self.version,
            kind: crate::apply::class_of(&command.payload),
            target: command.target.clone(),
            payload: command.payload.clone(),
            history: None,
        };
        let delta = self.adapter.apply_confirmed(&mut self.engine, self.version, actor, &op).map_err(|e| e.message)?;
        self.version = self.version.saturating_add(1);
        let out = Applied { op_id: op.op_id, version: self.version, actor_id: actor.to_string(), command: Some(command.clone()), delta };
        self.remember(&out);
        self.tail.push(out);
        if self.tail.len() > self.config.max_tail {
            self.tail.remove(0);
        }
        self.maybe_snapshot();
        Ok(())
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
        // The cell address is part of the text address: a row or column edit moves it, and two
        // operations in different cells of one table never shift each other's offsets.
        let mut cell = match &op.payload {
            Payload::TextSet { cell, .. } | Payload::TextInsert { cell, .. } | Payload::TextDelete { cell, .. } => *cell,
            _ => None,
        };
        let shift = |point: &mut u32, payload: &Payload, cell: Option<(u32, u32)>| match payload {
            Payload::TextInsert { at, text, cell: entry } if *entry == cell => {
                if *at <= *point {
                    *point = point.saturating_add(text.chars().count() as u32);
                }
            }
            Payload::TextDelete { from, to, cell: entry } if *entry == cell => {
                if *to <= *point {
                    *point = point.saturating_sub(to.saturating_sub(*from));
                } else if *from < *point {
                    *point = *from;
                }
            }
            // A notes body has one address (the slide), so its `cell` is always absent.
            Payload::NotesInsert { at, text } if cell.is_none() => {
                if *at <= *point {
                    *point = point.saturating_add(text.chars().count() as u32);
                }
            }
            Payload::NotesDelete { from, to } if cell.is_none() => {
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
            // Two text bodies on different slides (or two notes bodies) never shift each other.
            if command.target.slide != op.target.slide {
                continue;
            }
            // A whole-body notes value replaces every offset: an insert or delete that was based
            // before it cannot say where its text now belongs, so it is refused and the client
            // resyncs instead of writing in the wrong place.
            if matches!(command.payload, Payload::NotesSet { .. }) && matches!(op.payload, Payload::NotesInsert { .. } | Payload::NotesDelete { .. })
            {
                return Err(refuse(op, Rejection::stale("the notes were replaced after this edit began; resync first", Some(self.version))));
            }
            if let Payload::TableEdit { axis, at, insert } = &command.payload {
                let index = match axis {
                    deckcraft_collab_proto::TableAxis::Row => cell.as_mut().map(|(r, _)| r),
                    deckcraft_collab_proto::TableAxis::Column => cell.as_mut().map(|(_, c)| c),
                };
                if let Some(index) = index {
                    if *insert {
                        if *index >= *at {
                            *index = index.saturating_add(1);
                        }
                    } else if *index == *at {
                        return Err(refuse(op, Rejection::stale("the table line holding this operation was removed", Some(self.version))));
                    } else if *index > *at {
                        *index -= 1;
                    }
                }
                continue;
            }
            match &mut op.payload {
                Payload::TextInsert { at, .. } => shift(at, &command.payload, cell),
                Payload::TextDelete { from, to, .. } => {
                    shift(from, &command.payload, cell);
                    shift(to, &command.payload, cell);
                }
                Payload::NotesInsert { at, .. } => shift(at, &command.payload, cell),
                Payload::NotesDelete { from, to } => {
                    shift(from, &command.payload, cell);
                    shift(to, &command.payload, cell);
                }
                _ => {}
            }
        }
        match &mut op.payload {
            Payload::TextSet { cell: at, .. } | Payload::TextInsert { cell: at, .. } | Payload::TextDelete { cell: at, .. } => *at = cell,
            _ => {}
        }
        let empty = match op.payload {
            Payload::TextDelete { from, to, .. } => to <= from,
            Payload::NotesDelete { from, to } => to <= from,
            _ => false,
        };
        if empty {
            return Err(refuse(op, Rejection::invalid("the deleted text was already changed", Some(self.version))));
        }
        Ok(())
    }

    /// What a reconnecting client gets: the base (when the journal can no longer rebuild its
    /// version) and the journal tail above it (ADR-0005, RFC §4.5).
    ///
    /// A tail answer is used while the journal still covers the client. Once compaction removed
    /// it, the client replaces its replica with the native base, so the session takes a snapshot
    /// on demand when the stored one is too old or missing. A base larger than
    /// [`SessionConfig::max_base_inline`] travels as a cursor without bytes; the client then has
    /// to reopen the file from the cabinet instead of rebuilding from the wire.
    pub fn resync(&mut self, last_version: u64) -> SnapshotMsg {
        let head = self.version;
        if self.tail_covers(last_version) {
            return SnapshotMsg {
                version: head,
                snapshot: SnapshotRef { mode: SnapshotMode::BaseAndTail, cursor: format!("tail@{}", last_version.min(head)) },
                tail: self.tail_after(last_version),
                base: None,
            };
        }
        // The journal cannot rebuild this client's state: a base is needed, and the tail must
        // still reach the head. Otherwise take a fresh base at the head (compaction).
        let usable = self.snapshot.as_ref().is_some_and(|snapshot| self.tail_covers(snapshot.version));
        if !usable {
            let _ = self.take_snapshot();
        }
        if !self.snapshot.as_ref().is_some_and(|snapshot| self.tail_covers(snapshot.version)) {
            // The base could not be written; the tail-only answer is all this session has.
            return SnapshotMsg {
                version: head,
                snapshot: SnapshotRef { mode: SnapshotMode::BaseAndTail, cursor: format!("tail@{}", last_version.min(head)) },
                tail: self.tail_after(last_version),
                base: None,
            };
        }
        let Some((base_version, cursor, bytes_base64, keys)) = self.base_transfer() else {
            // No base is stored; the tail-only answer is all this session has.
            return SnapshotMsg {
                version: head,
                snapshot: SnapshotRef { mode: SnapshotMode::BaseAndTail, cursor: format!("tail@{}", last_version.min(head)) },
                tail: self.tail_after(last_version),
                base: None,
            };
        };
        SnapshotMsg {
            version: head,
            snapshot: SnapshotRef { mode: SnapshotMode::Base, cursor },
            tail: self.tail_after(base_version),
            base: Some(BaseSnapshot { version: base_version, bytes_base64, keys }),
        }
    }

    /// The stored base in wire form, with the key map every replica needs to address it.
    fn base_transfer(&self) -> Option<(u64, String, Option<String>, deckcraft_collab_proto::SnapshotKeys)> {
        let snapshot = self.snapshot.as_ref()?;
        let bytes_base64 =
            (snapshot.bytes.len() <= self.config.max_base_inline).then(|| base64::engine::general_purpose::STANDARD.encode(&snapshot.bytes));
        Some((snapshot.version, snapshot.cursor.clone(), bytes_base64, self.adapter.snapshot_keys()))
    }

    /// Does the tail hold every step after `version` (so replaying it rebuilds the head)?
    fn tail_covers(&self, version: u64) -> bool {
        version >= self.version || self.tail.first().is_some_and(|first| first.version <= version.saturating_add(1))
    }

    fn tail_after(&self, version: u64) -> Vec<Applied> {
        self.tail.iter().filter(|applied| applied.version > version).cloned().collect()
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
            // The cabinet may have accepted a version since this file was built; the cached bytes
            // are still current, but the version they were built against has moved.
            let mut done = done.clone();
            done.file_version = self.base_file_etag.clone();
            return Ok(done);
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
    use deckcraft_collab_proto::{OpKind, Payload, RejectCode, ShapeKey, SlideKey, TableAxis, Target};

    fn deck() -> Presentation {
        let mut s = Session::with_new();
        let title = s.doc().expect("doc").doc.slides[0].id;
        s.apply_remote("slide.new", &json!({"layout": "blank", "at": 1})).expect("blank");
        s.apply_remote("slide.delete", &json!({"ids": [title.0]})).expect("drop title");
        s.apply_remote("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 100]})).expect("insert");
        s.doc().expect("doc").doc.as_ref().clone()
    }

    /// One blank slide with a 2×2 table whose top-left cell holds "ab".
    fn table_deck() -> Presentation {
        let mut s = Session::with_new();
        let title = s.doc().expect("doc").doc.slides[0].id;
        s.apply_remote("slide.new", &json!({"layout": "blank", "at": 1})).expect("blank");
        s.apply_remote("slide.delete", &json!({"ids": [title.0]})).expect("drop title");
        s.apply_remote("insert.table", &json!({"rows": 2, "cols": 2, "rect": [100, 100, 300, 100]})).expect("table");
        let table = s.doc().expect("doc").doc.slides[0].shapes[0].id;
        s.apply_remote("text.set", &json!({"id": table.0, "cell": [0, 0], "text": "ab"})).expect("seed");
        s.doc().expect("doc").doc.as_ref().clone()
    }

    fn table_keys(session: &DocSession) -> (SlideKey, ShapeKey) {
        let doc = session.document().expect("doc");
        let slide = doc.slides.first().expect("slide");
        let key = session.adapter.keys();
        (
            key.slide_key(slide.id).expect("slide key").clone(),
            key.shape_key(slide.id, slide.shapes.first().expect("table").id).expect("shape key").clone(),
        )
    }

    fn cell(session: &DocSession, r: usize, c: usize) -> String {
        let doc = session.document().expect("doc");
        match &doc.slides[0].shapes[0].kind {
            deckcraft_model::ShapeKind::Table(t) => t.cell(r, c).expect("cell").text.text(),
            other => panic!("not a table: {other:?}"),
        }
    }

    fn cell_op(slide: &SlideKey, shape: &ShapeKey, payload: Payload, base: u64, op_id: &str) -> OpEnvelope {
        OpEnvelope {
            op_id: op_id.into(),
            doc: "d".into(),
            base_version: base,
            kind: OpKind::Command,
            target: Target { slide: Some(slide.clone()), shape: Some(shape.clone()), ..Target::default() },
            payload,
            history: None,
        }
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
    fn a_cell_resend_after_reconnect_applies_once() {
        let mut s = DocSession::from_document("d", table_deck(), SessionConfig::default()).expect("session");
        let (slide, shape) = table_keys(&s);
        let op = cell_op(&slide, &shape, Payload::TextInsert { at: 2, text: "X".into(), cell: Some((0, 0)) }, 0, "op-1");
        s.apply("a", &op).expect("first");
        assert_eq!(cell(&s, 0, 0), "abX");
        let again = s.apply("a", &op).expect("resend after reconnect");
        assert_eq!(again.version, 1);
        assert_eq!(s.version(), 1, "the resend does not bump the version");
        assert_eq!(cell(&s, 0, 0), "abX", "the character is not inserted twice");
    }

    #[test]
    fn a_cell_insert_rebases_past_an_inserted_row() {
        let mut s = DocSession::from_document("d", table_deck(), SessionConfig::default()).expect("session");
        let (slide, shape) = table_keys(&s);
        // A client at version 0 types before the "b" of the seeded cell.
        let queued = cell_op(&slide, &shape, Payload::TextInsert { at: 1, text: "X".into(), cell: Some((0, 0)) }, 0, "typed");
        // Someone else inserts a row above it; the queued operation's address must move down.
        let row = OpEnvelope {
            op_id: "row".into(),
            doc: "d".into(),
            base_version: 0,
            kind: OpKind::Structural,
            target: Target { slide: Some(slide), shape: Some(shape), ..Target::default() },
            payload: Payload::TableEdit { axis: TableAxis::Row, at: 0, insert: true },
            history: None,
        };
        s.apply("b", &row).expect("insert row");
        s.apply("a", &queued).expect("the queued cell insert applies after the shift");
        assert_eq!(cell(&s, 1, 0), "aXb");
        assert_eq!(cell(&s, 0, 0), "", "the inserted row stays empty");
        assert_eq!(s.version(), 2);
    }

    #[test]
    fn a_cell_insert_is_refused_when_its_row_is_deleted() {
        let mut s = DocSession::from_document("d", table_deck(), SessionConfig::default()).expect("session");
        let (slide, shape) = table_keys(&s);
        let target = Target { slide: Some(slide.clone()), shape: Some(shape.clone()), ..Target::default() };
        let queued = cell_op(&slide, &shape, Payload::TextInsert { at: 1, text: "X".into(), cell: Some((0, 0)) }, 0, "typed");
        let row = OpEnvelope {
            op_id: "row".into(),
            doc: "d".into(),
            base_version: 0,
            kind: OpKind::Structural,
            target,
            payload: Payload::TableEdit { axis: TableAxis::Row, at: 0, insert: false },
            history: None,
        };
        s.apply("b", &row).expect("delete row");
        let err = s.apply("a", &queued).expect_err("the cell is gone");
        assert_eq!(err.code, RejectCode::Stale);
        assert!(err.retryable);
        assert_eq!(s.version(), 1, "the refused operation does not bump the version");
    }

    #[test]
    fn a_queued_cell_set_moves_with_an_inserted_row() {
        let mut s = DocSession::from_document("d", table_deck(), SessionConfig::default()).expect("session");
        let (slide, shape) = table_keys(&s);
        let queued = cell_op(&slide, &shape, Payload::TextSet { text: "replaced".into(), cell: Some((0, 0)) }, 0, "set");
        let row = OpEnvelope {
            op_id: "row".into(),
            doc: "d".into(),
            base_version: 0,
            kind: OpKind::Structural,
            target: Target { slide: Some(slide), shape: Some(shape), ..Target::default() },
            payload: Payload::TableEdit { axis: TableAxis::Row, at: 0, insert: true },
            history: None,
        };
        s.apply("b", &row).expect("insert row");
        s.apply("a", &queued).expect("the set follows its cell");
        assert_eq!(cell(&s, 1, 0), "replaced");
        assert_eq!(cell(&s, 0, 0), "", "the inserted row stays empty");
    }

    #[test]
    fn two_people_typing_in_one_cell_keep_both_insertions() {
        let mut s = DocSession::from_document("d", table_deck(), SessionConfig::default()).expect("session");
        let (slide, shape) = table_keys(&s);
        let x = cell_op(&slide, &shape, Payload::TextInsert { at: 1, text: "X".into(), cell: Some((0, 0)) }, 0, "x");
        let y = cell_op(&slide, &shape, Payload::TextInsert { at: 1, text: "Y".into(), cell: Some((0, 0)) }, 0, "y");
        s.apply("a", &x).expect("x");
        s.apply("b", &y).expect("y is rebased past x");
        assert_eq!(cell(&s, 0, 0), "aXYb");

        // The same offsets in two different cells do not shift each other.
        let other = cell_op(&slide, &shape, Payload::TextInsert { at: 0, text: "Q".into(), cell: Some((1, 1)) }, 0, "q");
        s.apply("b", &other).expect("another cell");
        assert_eq!(cell(&s, 0, 0), "aXYb", "the other cell's offset did not move this one");
        assert_eq!(cell(&s, 1, 1), "Q");
    }

    fn notes_op(slide: &SlideKey, payload: Payload, base: u64, op_id: &str) -> OpEnvelope {
        OpEnvelope {
            op_id: op_id.into(),
            doc: "d".into(),
            base_version: base,
            kind: OpKind::Command,
            target: Target { slide: Some(slide.clone()), ..Target::default() },
            payload,
            history: None,
        }
    }

    fn notes_key(session: &DocSession, index: usize) -> SlideKey {
        let doc = session.document().expect("doc");
        let slide = doc.slides.get(index).expect("slide");
        session.adapter.keys().slide_key(slide.id).expect("slide key").clone()
    }

    /// Two slides; the first one carries seeded notes.
    fn two_slide_deck() -> Presentation {
        let mut s = Session::with_new();
        let title = s.doc().expect("doc").doc.slides[0].id;
        s.apply_remote("slide.new", &json!({"layout": "blank", "at": 1})).expect("blank");
        s.apply_remote("slide.delete", &json!({"ids": [title.0]})).expect("drop title");
        s.apply_remote("slide.new", &json!({"layout": "blank", "at": 1})).expect("second");
        s.apply_remote("slide.notes", &json!({"index": 0, "text": "ab"})).expect("notes");
        s.doc().expect("doc").doc.as_ref().clone()
    }

    #[test]
    fn two_people_typing_in_one_slides_notes_keep_both_insertions() {
        let mut s = DocSession::from_document("d", two_slide_deck(), SessionConfig::default()).expect("session");
        let slide = notes_key(&s, 0);
        let x = notes_op(&slide, Payload::NotesInsert { at: 0, text: "X".into() }, 0, "x");
        let y = notes_op(&slide, Payload::NotesInsert { at: 0, text: "Y".into() }, 0, "y");
        s.apply("a", &x).expect("x");
        s.apply("b", &y).expect("y is rebased past x");
        assert_eq!(s.document().expect("doc").slides[0].notes.text(), "XYab");
        assert_eq!(s.version(), 2);

        // A notes body on another slide has no address in common: its offsets do not shift.
        let second = notes_key(&s, 1);
        let other = notes_op(&second, Payload::NotesInsert { at: 0, text: "Q".into() }, 1, "q");
        s.apply("b", &other).expect("the second slide's notes");
        assert_eq!(s.document().expect("doc").slides[0].notes.text(), "XYab");
        assert_eq!(s.document().expect("doc").slides[1].notes.text(), "Q", "the base 0 operation did not move this one");
    }

    #[test]
    fn a_queued_notes_insert_rebases_across_a_slide_insert() {
        let mut s = DocSession::from_document("d", two_slide_deck(), SessionConfig::default()).expect("session");
        let first = notes_key(&s, 0);
        let typed = notes_op(&first, Payload::NotesInsert { at: 1, text: "X".into() }, 0, "typed");
        let new_slide = OpEnvelope {
            op_id: "new".into(),
            doc: "d".into(),
            base_version: 0,
            kind: OpKind::Structural,
            target: Target { slide: Some(notes_key(&s, 1)), ..Target::default() },
            payload: Payload::SlideInsert { before: None, layout: Some("blank".into()) },
            history: None,
        };
        s.apply("b", &new_slide).expect("insert a slide");
        s.apply("a", &typed).expect("the queued notes insert applies to the same slide");
        assert_eq!(s.document().expect("doc").slides[0].notes.text(), "aXb");
        assert_eq!(s.document().expect("doc").slides.len(), 3);
    }

    #[test]
    fn a_queued_notes_insert_is_refused_after_a_whole_body_replacement() {
        let mut s = DocSession::from_document("d", two_slide_deck(), SessionConfig::default()).expect("session");
        let first = notes_key(&s, 0);
        let typed = notes_op(&first, Payload::NotesInsert { at: 1, text: "X".into() }, 0, "typed");
        let body = s.document().expect("doc").slides[0].notes.clone();
        let replaced = notes_op(&first, Payload::NotesSet { notes: serde_json::to_value(&body).expect("json") }, 0, "set");
        s.apply("b", &replaced).expect("the whole body");
        let err = s.apply("a", &typed).expect_err("its offset no longer means this body");
        assert_eq!(err.code, RejectCode::Stale);
        assert!(err.retryable, "the client resyncs and re-reads the body");
        assert_eq!(s.version(), 1, "the refused operation does not bump the version");
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
        // A snapshot was taken at version 4, so a client from nothing rebuilds from the base.
        assert_eq!(s.snapshot().map(|x| x.version), Some(4));
        let at_base = s.resync(0);
        assert_eq!(at_base.version, 4);
        assert!(at_base.tail.is_empty(), "the base already covers every operation");
        assert_eq!(at_base.snapshot.mode, SnapshotMode::Base);
        assert!(at_base.snapshot.cursor.starts_with("base@4"));
        let base = at_base.base.expect("the compacted client gets the base");
        assert_eq!(base.version, 4);
        assert!(base.bytes_base64.is_some(), "a small base travels inline");
        assert!(!base.keys.shapes.is_empty(), "the base carries the session key map");

        // One more operation: a client at 4 gets exactly that one, from the journal.
        s.apply("a", &move_op(&slide, &shape, 500.0, 4, "op-4")).expect("apply");
        let caught = s.resync(4);
        assert_eq!(caught.version, 5);
        assert_eq!(caught.tail.len(), 1);
        assert_eq!(caught.tail[0].version, 5);
        assert!(caught.base.is_none(), "a covered client needs no base");
        assert!(s.resync(5).tail.is_empty(), "up to date");
    }

    #[test]
    fn a_compacted_tail_sends_a_base_that_reloads_into_the_snapshot_document() {
        let mut s = DocSession::from_document("d", deck(), SessionConfig { snapshot_every: 2, ..SessionConfig::default() }).expect("session");
        let (slide, shape) = keys(&s);
        for i in 0..4 {
            s.apply("a", &move_op(&slide, &shape, 200.0 + i as f64 * 10.0, i, &format!("op-{i}"))).expect("apply");
        }
        let at_four = s.document().expect("doc").clone();
        s.apply("a", &move_op(&slide, &shape, 500.0, 4, "op-4")).expect("apply");

        // The client missed 1..4 (the tail starts at 5): base at 4 plus the step above it.
        let snapshot = s.resync(0);
        assert_eq!(snapshot.snapshot.mode, SnapshotMode::Base);
        let base = snapshot.base.expect("a base");
        assert_eq!(base.version, 4);
        let bytes = base64::engine::general_purpose::STANDARD.decode(base.bytes_base64.as_deref().expect("inline")).expect("base64");
        let reloaded = DocSession::open("d", &bytes).expect("the base reloads");
        assert_eq!(*reloaded.document().expect("doc"), at_four, "the base is the document at its version");
        assert_eq!(snapshot.tail.len(), 1);
        assert_eq!(snapshot.tail[0].version, 5);
        assert_eq!(snapshot.version, 5, "the tail above the base reaches the head");
    }

    #[test]
    fn a_base_larger_than_the_limit_travels_without_bytes() {
        let mut s = DocSession::from_document("d", deck(), SessionConfig { snapshot_every: 1, max_base_inline: 1, ..SessionConfig::default() })
            .expect("session");
        let (slide, shape) = keys(&s);
        s.apply("a", &move_op(&slide, &shape, 300.0, 0, "op-1")).expect("apply");
        let snapshot = s.resync(0);
        let base = snapshot.base.expect("a base cursor");
        assert_eq!(base.version, 1);
        assert!(base.bytes_base64.is_none(), "over the inline limit the client reopens the file");
        assert!(!base.keys.shapes.is_empty(), "the key map is small and still travels");
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
