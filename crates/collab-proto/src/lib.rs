//! Wire types for DeckCraft real-time collaboration.
//!
//! One envelope for all five Craft editors (`.craft/protocol/ARCHITECTURE.md` §2): the
//! WebSocket handshake, operations, their authoritative results, presence and the
//! service messages around them. This crate carries **no** engine or model types: only
//! the bytes that cross the wire, so the server, the client and the tests can share them
//! without linking a document engine.
//!
//! Protocol version 1. The rules the types encode are ADR 0001 (one normalised
//! operation, the server executes and answers authoritatively), 0002 (session-scoped
//! keys, tombstones), 0003 (one server version, `opId` dedup) and 0006 (presence is
//! ephemeral and never journaled).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

/// Version of this envelope. A client and a server that disagree do not talk.
pub const PROTO_VERSION: u16 = 1;

/// App id in the handshake (`deckcraft`).
pub const APP_ID: &str = "deckcraft";

/// A session-scoped slide key issued by the server (ADR 0002). Never a file id: the
/// `.pptx` writer renumbers slides (`p:sldId@id = 256 + i`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SlideKey(pub String);

/// A session-scoped shape key: the file's `cNvPr` when it is unique within its slide,
/// otherwise (and for inserted shapes) a key the server issues.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ShapeKey(pub String);

impl std::fmt::Display for SlideKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::fmt::Display for ShapeKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for SlideKey {
    fn from(s: &str) -> Self {
        SlideKey(s.to_string())
    }
}

impl From<&str> for ShapeKey {
    fn from(s: &str) -> Self {
        ShapeKey(s.to_string())
    }
}

/// Who is acting. `role` comes from the JWT, never from an operation body (ADR 0008).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Owner,
    Editor,
    Viewer,
}

impl Role {
    /// Whether the role may send an operation that changes the document.
    pub fn may_write(self) -> bool {
        !matches!(self, Role::Viewer)
    }
}

/// Form of an operation. The merge/structural **conflict** class is a separate decision
/// the adapter makes by invariant (ADR 0004); `structural` here always means the exclusive
/// class.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpKind {
    /// A normalised application command (`shape.move`, `text.set`, …).
    Command,
    /// Changes structure or addresses: exclusive, `baseVersion == head`.
    Structural,
}

/// Address of an operation. Every field is an explicit value; nothing is read from a
/// participant's selection (ADR 0001 §2).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Target {
    /// Slide the operation acts on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slide: Option<SlideKey>,
    /// One shape: `shape.move`, `text.set`, `shape.delete`, `arrange.reorder`, …
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shape: Option<ShapeKey>,
    /// Several shapes: `arrange.group`, `edit.delete`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub shapes: Vec<ShapeKey>,
    /// Text position inside `shape` (or inside one table cell).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<TextCursor>,
    /// Insert or move before this object; `None` appends. Order is an anchor, not an
    /// index (ADR 0002 §5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<ShapeKey>,
}

/// A position in a text body: `(paragraph, character offset)` within `cell` when the shape
/// is a table (`crates/model/src/edit.rs`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TextCursor {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell: Option<(u32, u32)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor: Option<(u32, u32)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caret: Option<(u32, u32)>,
}

/// What the operation does. Values are final: no relative `dx/dy`, no "current caret".
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Payload {
    /// Absolute top-left in slide points.
    ShapeMove { x: f64, y: f64 },
    /// Absolute box: position and size in slide points.
    ShapeSetBounds { x: f64, y: f64, w: f64, h: f64 },
    /// Replaces a text body (or one table cell) and keeps the first run's formatting.
    TextSet {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cell: Option<(u32, u32)>,
    },
    /// Removes one shape (`target.shape`) or several (`target.shapes`).
    ShapeDelete {},
    /// Inserts a slide before `target.slide` (`None`: at the end).
    SlideInsert {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        before: Option<SlideKey>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        layout: Option<String>,
    },
    /// Removes the slide in `target.slide`.
    SlideDelete {},
    /// Moves the slide in `target.slide` before `before` (`None`: to the end).
    SlideMove {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        before: Option<SlideKey>,
    },
    /// Groups `target.shapes` into one group.
    Group {},
    /// Ungroups `target.shape`.
    Ungroup {},
    /// Moves `target.shape` before `target.before` among its siblings (same group).
    Reorder {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        before: Option<ShapeKey>,
    },
}

impl Payload {
    /// Command id this operation normalises to, for logs and allowlist checks.
    pub fn command(&self) -> &'static str {
        match self {
            Payload::ShapeMove { .. } => "shape.move",
            Payload::ShapeSetBounds { .. } => "shape.setBounds",
            Payload::TextSet { .. } => "text.set",
            Payload::ShapeDelete {} => "edit.delete",
            Payload::SlideInsert { .. } => "slide.new",
            Payload::SlideDelete {} => "slide.delete",
            Payload::SlideMove { .. } => "slide.move",
            Payload::Group {} => "arrange.group",
            Payload::Ungroup {} => "arrange.ungroup",
            Payload::Reorder { .. } => "arrange.reorder",
        }
    }
}

/// A client operation (ADR 0001). `opId` is the client's UUID; the server dedups on it and
/// applies strictly once (ADR 0003).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpEnvelope {
    pub op_id: String,
    pub doc: String,
    pub base_version: u64,
    pub kind: OpKind,
    #[serde(default)]
    pub target: Target,
    pub payload: Payload,
}

/// Identity a client sends in the handshake. Informational only: the server does not
/// authorise on it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
    pub name: String,
    pub ver: String,
}

/// First client message. The JWT travels here and nowhere else (ADR 0008).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hello {
    pub v: u16,
    pub app: String,
    pub doc: String,
    pub jwt: String,
    pub client: ClientInfo,
    /// Last version the client has, for the catch-up after a reconnect (ADR 0003).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_version: Option<u64>,
}

/// What the client may send and what the server answers (§4.2 of the RFC).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    Hello(Hello),
    Op(OpEnvelope),
    Presence(PresenceMsg),
    Resync { last_version: u64 },
    Commit { expected_file_version: Option<String> },
    Ping,
}

/// Lease of the write role on one document (ADR 0008).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lease {
    pub id: String,
    pub ttl_sec: u32,
}

/// How a reconnecting client receives the current state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotMode {
    /// A base alone; the client replaces its state.
    Base,
    /// A base plus the journal tail; the client replays the tail.
    BaseAndTail,
}

/// Where the client's catch-up starts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotRef {
    pub mode: SnapshotMode,
    pub cursor: String,
}

/// Limits advertised at join; the server refuses anything above them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limits {
    pub max_message_bytes: u64,
    pub max_ops_per_sec: u32,
    pub max_participants: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { max_message_bytes: 256 << 10, max_ops_per_sec: 50, max_participants: 16 }
    }
}

/// What the adapter accepts. `structural` is always exclusive; `command` may be merge or
/// structural depending on the adapter's invariant (ADR 0004).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub kinds: Vec<OpKind>,
    pub limits: Limits,
    /// Commands the adapter accepts on the wire (`shape.move`, `text.set`, …), so a client
    /// can disable the rest instead of learning by rejection.
    #[serde(default)]
    pub commands: Vec<String>,
}

/// First server message.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Welcome {
    pub v: u16,
    pub session_id: String,
    pub actor_id: String,
    pub role: Role,
    pub lease: Option<Lease>,
    pub version: u64,
    pub snapshot: SnapshotRef,
    pub capabilities: Capabilities,
}

/// Name of a shape field that changed, for a delta. Coarse on purpose: the client refetches
/// the shape and re-renders; the delta is not a patch format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    Xfrm,
    Text,
    Fill,
    Line,
    Effects,
    Hidden,
    Locked,
    Name,
    /// Position among siblings (z-order).
    ZOrder,
    /// Anything else the model stores on the shape (kind, geometry, diagram…).
    Other,
    /// Slide-level change that is not a shape: background, transition, notes…
    Slide,
}

/// One changed object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChangeKind {
    Inserted,
    Deleted,
    Modified {
        fields: Vec<Field>,
    },
    /// The slide's shape order changed; `fields` lists what else changed on the slide.
    Reordered {
        fields: Vec<Field>,
    },
}

/// A change in the authoritative result. `shape` is absent for slide-level changes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub slide: SlideKey,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shape: Option<ShapeKey>,
    #[serde(flatten)]
    pub kind: ChangeKind,
}

/// The changed part of the document after an operation.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Delta {
    pub changes: Vec<Change>,
}

impl Delta {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

/// The operation as accepted by the server, with the values it actually used.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmedCommand {
    pub cmd: String,
    #[serde(default)]
    pub target: Target,
    pub payload: Payload,
}

/// Authoritative result of one operation (ADR 0001 §3): the confirmed command and/or the
/// changed state. `version` is the document version after it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Applied {
    pub op_id: String,
    pub version: u64,
    pub actor_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<ConfirmedCommand>,
    #[serde(default)]
    pub delta: Delta,
}

/// Why an operation was refused. `retryable` tells the client whether resending the same
/// payload can succeed (ADR 0004 §3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectCode {
    /// The address or the base version moved: refetch, then decide.
    Stale,
    /// The document changed under a merge operation in a way the adapter will not merge.
    Conflict,
    /// The role may not do this (a viewer, a locked shape, a protected document).
    Forbidden,
    /// The operation is malformed or unknown to this adapter.
    Invalid,
    /// The token expired or was revoked.
    Auth,
    /// Another participant holds the write lease.
    Lease,
    /// Undo would overwrite someone else's later change (ADR 0010 §3).
    UndoConflict,
    /// The adapter does not implement the operation in this version.
    Unsupported,
}

impl RejectCode {
    pub fn retryable(self) -> bool {
        matches!(self, RejectCode::Stale | RejectCode::Lease)
    }
}

/// Where the client is now, so it can retry against the right version.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hint {
    pub version: u64,
}

/// Refusal of one operation. The connection stays open.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rejected {
    pub op_id: String,
    pub code: RejectCode,
    pub message: String,
    pub retryable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<Hint>,
}

/// Ephemeral participant state: active slide, selection, caret, viewport (ADR 0006). Never
/// journaled and never part of `version`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresenceMsg {
    pub actor_id: String,
    pub slide: SlideKey,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shapes: Vec<ShapeKey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<TextCursor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport: Option<Viewport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// What the participant is looking at.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Viewport {
    pub zoom: f64,
    pub x: f64,
    pub y: f64,
}

/// Current state for a reconnecting client: a base and, optionally, the journal tail.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotMsg {
    pub version: u64,
    pub snapshot: SnapshotRef,
    #[serde(default)]
    pub tail: Vec<Applied>,
}

/// Result of a commit (ADR 0009). `warnings` lists what will not survive the file, by
/// slide/shape, not as a boolean.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitResult {
    pub sha256: String,
    pub version: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_version: Option<String>,
    /// Bytes inline for small decks; a blob reference (media channel) for large ones.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes_base64: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blob: Option<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// A connection-level error; the server closes the socket after it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

/// What the server sends (§4.2 of the RFC).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    Welcome(Welcome),
    Applied(Applied),
    Rejected(Rejected),
    Presence(PresenceMsg),
    Snapshot(SnapshotMsg),
    CommitResult(CommitResult),
    Error(ServerError),
    Pong,
}

/// Encode a client message for the socket.
pub fn encode_client(m: &ClientMsg) -> Result<String, serde_json::Error> {
    serde_json::to_string(m)
}

/// Decode a client message from one socket frame.
pub fn decode_client(s: &str) -> Result<ClientMsg, serde_json::Error> {
    serde_json::from_str(s)
}

/// Encode a server message for the socket.
pub fn encode_server(m: &ServerMsg) -> Result<String, serde_json::Error> {
    serde_json::to_string(m)
}

/// Decode a server message from one socket frame.
pub fn decode_server(s: &str) -> Result<ServerMsg, serde_json::Error> {
    serde_json::from_str(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op() -> OpEnvelope {
        OpEnvelope {
            op_id: "6f1c".into(),
            doc: "f42".into(),
            base_version: 41,
            kind: OpKind::Command,
            target: Target { slide: Some("s1".into()), shape: Some("7".into()), ..Target::default() },
            payload: Payload::ShapeMove { x: 120.0, y: 64.0 },
        }
    }

    #[test]
    fn proto_version_is_one() {
        assert_eq!(PROTO_VERSION, 1);
        assert_eq!(APP_ID, "deckcraft");
    }

    #[test]
    fn op_round_trips() {
        let msg = ClientMsg::Op(op());
        let text = encode_client(&msg).expect("encode");
        assert!(text.contains("\"type\":\"op\""), "{text}");
        assert!(text.contains("\"cmd\":\"shape_move\""), "{text}");
        assert_eq!(decode_client(&text).expect("decode"), msg);
    }

    #[test]
    fn hello_carries_the_token_only_there() {
        let hello = Hello {
            v: PROTO_VERSION,
            app: APP_ID.into(),
            doc: "f42".into(),
            jwt: "secret".into(),
            client: ClientInfo { name: "deckcraft-web".into(), ver: "0.1".into() },
            last_version: Some(41),
        };
        let text = encode_client(&ClientMsg::Hello(hello.clone())).expect("encode");
        assert!(text.contains("\"jwt\":\"secret\""));
        assert_eq!(decode_client(&text).expect("decode"), ClientMsg::Hello(hello));
    }

    #[test]
    fn applied_round_trips_with_delta() {
        let applied = Applied {
            op_id: "6f1c".into(),
            version: 42,
            actor_id: "a1".into(),
            command: Some(ConfirmedCommand {
                cmd: "shape.move".into(),
                target: Target { slide: Some("s1".into()), shape: Some("7".into()), ..Target::default() },
                payload: Payload::ShapeMove { x: 120.0, y: 64.0 },
            }),
            delta: Delta {
                changes: vec![Change { slide: "s1".into(), shape: Some("7".into()), kind: ChangeKind::Modified { fields: vec![Field::Xfrm] } }],
            },
        };
        let text = encode_server(&ServerMsg::Applied(applied.clone())).expect("encode");
        assert_eq!(decode_server(&text).expect("decode"), ServerMsg::Applied(applied));
    }

    #[test]
    fn structural_payloads_have_no_required_fields() {
        for p in [Payload::ShapeDelete {}, Payload::SlideDelete {}, Payload::Group {}, Payload::Ungroup {}] {
            let text = serde_json::to_string(&p).expect("encode");
            assert!(text.contains("\"cmd\""), "{text}");
            assert_eq!(serde_json::from_str::<Payload>(&text).expect("decode"), p);
        }
    }

    #[test]
    fn anchor_is_optional_and_absent_when_appended() {
        let p = Payload::Reorder { before: None };
        let text = serde_json::to_string(&p).expect("encode");
        assert!(!text.contains("before"), "{text}");
    }

    #[test]
    fn reject_codes_know_when_a_retry_can_work() {
        assert!(RejectCode::Stale.retryable());
        assert!(RejectCode::Lease.retryable());
        assert!(!RejectCode::Forbidden.retryable());
        assert!(!RejectCode::Conflict.retryable());
        assert!(!RejectCode::UndoConflict.retryable());
        assert!(!RejectCode::Invalid.retryable());
        assert!(!RejectCode::Auth.retryable());
        assert!(!RejectCode::Unsupported.retryable());
    }

    #[test]
    fn only_writers_may_write() {
        assert!(Role::Owner.may_write());
        assert!(Role::Editor.may_write());
        assert!(!Role::Viewer.may_write());
    }

    #[test]
    fn payload_reports_the_command_it_normalises_to() {
        assert_eq!(Payload::ShapeMove { x: 0.0, y: 0.0 }.command(), "shape.move");
        assert_eq!(Payload::TextSet { text: "hi".into(), cell: None }.command(), "text.set");
        assert_eq!(Payload::SlideMove { before: None }.command(), "slide.move");
        assert_eq!(Payload::Reorder { before: None }.command(), "arrange.reorder");
    }

    #[test]
    fn unknown_message_types_are_refused() {
        assert!(decode_client("{\"type\":\"nope\"}").is_err());
        assert!(decode_server("{}").is_err());
    }

    #[test]
    fn presence_has_no_version_and_no_document() {
        let p = PresenceMsg {
            actor_id: "a1".into(),
            slide: "s1".into(),
            shapes: vec!["7".into()],
            text: Some(TextCursor { cell: None, anchor: Some((0, 3)), caret: Some((0, 5)) }),
            viewport: Some(Viewport { zoom: 1.0, x: 0.0, y: 0.0 }),
            color: Some("#3b82f6".into()),
        };
        let text = encode_server(&ServerMsg::Presence(p)).expect("encode");
        assert!(!text.contains("version"), "{text}");
        assert!(!text.contains("doc"), "{text}");
    }

    #[test]
    fn limits_default_matches_the_rfc() {
        let l = Limits::default();
        assert_eq!(l.max_message_bytes, 256 << 10);
        assert_eq!(l.max_ops_per_sec, 50);
        assert_eq!(l.max_participants, 16);
    }
}
