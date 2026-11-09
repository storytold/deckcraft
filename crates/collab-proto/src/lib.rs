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

/// A character range `[from, to)` inside one text body, a paragraph break counting as one
/// character — the same coordinates as [`Payload::TextInsert`] and [`Payload::TextDelete`]
/// (RFC §4.4). The body is the operation's own address (`target.shape`, plus `cell` for a table
/// cell), so the range never carries a shape of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextRange {
    pub from: u32,
    pub to: u32,
}

/// One body a `format.*` command addresses, and which characters of it change.
///
/// `cell` names a table cell; without it the address is the shape's own text body. `range` is a
/// whole-body character range and `caret` a collapsed position (a caret without a selection):
/// exactly one of them may be present, and neither means the whole body of that address.
///
/// A caret is an address in its own right, not a range widened to a word: every replica puts the
/// same collapsed selection on the same authoritative body, so the engine's own rule decides
/// which characters the command touches (run formatting widens a caret inside a word, paragraph
/// formatting takes the paragraph), and both replicas get the same result. A character offset is
/// deterministic where a run index is not (RFC §4.4).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FormatAddress {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell: Option<(u32, u32)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<TextRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caret: Option<u32>,
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
    /// Inserts at a character offset in the shape's text body or in one table cell (`\n` is a
    /// paragraph break). The server rebases the offset against operations that arrived after
    /// `baseVersion`, so two people typing in one paragraph both keep their text. `cell`
    /// addresses one table cell: the offset is inside that cell's own body, and a row or column
    /// inserted or deleted above/left of it moves the address and not the text.
    TextInsert {
        at: u32,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cell: Option<(u32, u32)>,
    },
    /// Removes the characters in `[from, to)` of the shape's text body or of one table cell.
    TextDelete {
        from: u32,
        to: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cell: Option<(u32, u32)>,
    },
    /// Inserts a preset shape with final geometry (and optional text).
    ShapeInsert {
        preset: String,
        rect: [f64; 4],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
    },
    /// Inserts a picture that was uploaded to the session's media channel. The bytes travel on
    /// the media channel, not in the operation: `sha256` names a content-addressed object the
    /// server stores and every replica fetches, so an operation stays small and the same picture
    /// inserted twice is stored once. `rect` is the final box; when it is absent the server
    /// computes the natural size from the bytes and the slide size, which every replica can
    /// reproduce.
    PictureInsert {
        sha256: String,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rect: Option<[f64; 4]>,
    },
    /// Inserts audio or video that was uploaded to the session's media channel. Like a picture,
    /// the bytes travel on the channel and the operation names them by `sha256`; `rect` is the
    /// final box (absent: the frame size the engine derives from the probed media).
    MediaInsert {
        kind: MediaKind,
        sha256: String,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rect: Option<[f64; 4]>,
    },
    /// Replaces one shape's fill with a final value. Absolute only: no "next theme colour", no
    /// transparency relative to an inherited style.
    ShapeFill { fill: FillSpec },
    /// Replaces one shape's outline with the fields given. Every field is a final value; a field
    /// left out keeps the shape's value on every replica because the confirmed command carries
    /// the same arguments.
    ShapeLine { line: LineSpec },
    /// Sets one slide's transition to a complete final value (`None`: no transition). The whole
    /// value travels, not the arguments of `transition.set`: `transition.set` keeps the advance
    /// timings and the embedded sound of the value it replaces, so a replica that merged the same
    /// arguments against its own state could keep a different value. The command is also the
    /// exact-value restore of a transition step (ADR 0010), checked like `shape_field`.
    SlideTransition { transition: Option<TransitionSpec> },
    /// Replaces one shape's animation steps with the exact ordered list given (`[]`: none).
    /// `target.slide` and `target.shape` address the shape; every value is one whole model
    /// `Animation` (`crates/model/src/anim.rs`) as JSON, the same way a `ShapeRestoreItem` carries
    /// a whole shape. A mirror struct in this crate would duplicate six model enums, a colour and
    /// two ids, so the model value travels as JSON and the engine deserialises it before the first
    /// write. The list is the shape's **whole** steps: `animation.add` normalises to the current
    /// list plus the new step, `animation.remove` to the list without the removed ones. The
    /// command is also the exact-image restore of an animation step (ADR 0010), because a file
    /// can carry preset ids, timings and colours the `animation.add` arguments cannot repeat.
    ShapeAnimations { animations: Vec<serde_json::Value> },
    /// Replaces one slide's **whole** animation sequence with the exact ordered list given (`[]`:
    /// none) and is the confirmed command of the normalised `animation.set`, `animation.timing`
    /// and `animation.move`. `target.slide` is the address; every value is one whole model
    /// `Animation` as JSON and carries its own shape id, because a step may move past steps of
    /// another shape, which the per-shape `ShapeAnimations` cannot express. It is also the
    /// exact-image restore of one of those steps (ADR 0010), checked like `shape_animations`.
    SlideAnimations { animations: Vec<serde_json::Value> },
    /// Replaces one slide's speaker notes with a complete final value: the whole model text body
    /// (`crates/model/src/text.rs`) as JSON, so runs, paragraph levels and bullets travel with the
    /// text. `target.slide` is the address — notes belong to a slide, not to a shape, so no shape
    /// key is named. This is the normalised form of `slide.notes` (with `body`, or with `text`
    /// turned into a body by the normaliser) and the exact-image restore of a notes step
    /// (ADR 0010), because the value a text edit replaced cannot be rebuilt from the typed string.
    NotesSet { notes: serde_json::Value },
    /// Inserts `text` at character offset `at` of `target.slide`'s notes (the offsets are whole
    /// body, paragraph breaks counted as one `\n`). The server rebases the offset against notes
    /// operations that arrived after `baseVersion`, so two people typing in one slide's notes
    /// both keep their text (RFC §4.4).
    NotesInsert { at: u32, text: String },
    /// Removes the characters in `[from, to)` of `target.slide`'s notes.
    NotesDelete { from: u32, to: u32 },
    /// Embeds one face of a document font (`p:embeddedFontLst`) in the presentation, creating the
    /// entry for `typeface` when the document has none and pointing the named face slot at the
    /// new object otherwise. The bytes travel on the media channel and the operation names them
    /// by `sha256`, like a picture, so an operation stays small and the same font is stored once.
    ///
    /// This is a **presentation-level** value: `target` names no slide or shape, so a replica
    /// applies it whatever slide it is looking at, and it changes no object address or key. The
    /// font's faces live in the document's media list on every replica (see
    /// [`Payload::FontRestore`] for the undo image).
    FontCreate { typeface: String, face: FontFace, sha256: String, name: String },
    /// Sets the descriptor fields of one embedded font entry to final values (`panose`,
    /// `pitchFamily`, `charset`). A field left out keeps the entry's value on every replica,
    /// because the confirmed command carries the same arguments. Presentation-level, like
    /// [`Payload::FontCreate`]; an entry the document does not have is refused before any write.
    FontSet {
        typeface: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        panose: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pitch_family: Option<u8>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        charset: Option<u8>,
    },
    /// Drops one embedded font entry from the presentation (`p:embeddedFontLst`). The face
    /// objects stay in the document's media list; a save writes only the objects the model still
    /// references. Presentation-level and undoable like [`Payload::FontCreate`].
    FontRemove { typeface: String },
    /// Puts back one embedded font entry, exactly, from the server's whole-entry image (ADR
    /// 0010). `font` is the model `EmbeddedFont` (`crates/model/src/lib.rs`) as JSON — its
    /// descriptors and the media ids of its four face slots — and `null` means the document had
    /// no entry for that typeface. The entry is presentation-level, so the restore names no
    /// slide; media ids are document ids every replica allocated in the same order, so a restore
    /// puts back the same objects.
    ///
    /// Like [`Payload::ShapeField`], the server builds this from the actor's history only.
    FontRestore {
        typeface: String,
        #[serde(default)]
        font: Option<serde_json::Value>,
    },
    /// One allowlisted `format.*` command with absolute arguments, aimed at one or more text
    /// bodies (§15): the whole body of `target.shape`, a whole table cell, a character range
    /// inside one body, or a caret in one body. Every body is one [`FormatAddress`] — a table
    /// cell by `(row, column)`, the shape's own body without a cell — so an operation either
    /// formats one body or **N cells of one table** as one atomic step.
    ///
    /// A table's own `text` body is empty, so formatting a table requires cell addresses; a
    /// table addressed without one, a shape that is not a table addressed by cell, an empty
    /// address list, a repeated address, an address that names no cell of that table and a
    /// range or caret past the end of the addressed body are refused before anything is
    /// written (RFC §15).
    ///
    /// `range` is a whole-body character range (`\n` counts one), the same coordinates as
    /// [`Payload::TextInsert`]/[`Payload::TextDelete`]: the server rebases every address against
    /// edits that arrived since `baseVersion`, and every replica turns the two offsets (or the
    /// caret) into the same `(paragraph, character)` pair with the same helper, so the run range
    /// that is formatted is a deterministic function of the authoritative body. Run indices are
    /// not an address: a text edit can split and merge runs on another replica.
    FormatSet {
        format: FormatCmd,
        args: serde_json::Value,
        /// The bodies this command formats, one entry each; never empty.
        addresses: Vec<FormatAddress>,
    },
    /// Inserts or deletes one row or column of the table in `target.shape`. Structural: every
    /// cell at or after `at` on the axis moves, so a queued text operation in a cell rebases its
    /// address before it applies. `insert` puts an empty line before `at`; otherwise the line
    /// `at` is removed.
    TableEdit { axis: TableAxis, at: u32, insert: bool },
    /// Replaces one table's style with a final style name (`table.style`). A style is a value of
    /// the table, not of a cell, so the operation addresses the table shape alone; no cell
    /// address moves and a style step is merge class.
    TableStyle { style: String },
    /// Replaces the fill of an explicit list of cells of one table with one final value
    /// (`table.cellFill`). `cells` is never empty and names cells the table has: the sender's
    /// normaliser expands the user's selection into that list, so a replica's own selection never
    /// decides which cells change, and a cell that is not listed is never written. A row or
    /// column edit that arrived since `baseVersion` moves the addresses like a cell text edit's.
    TableCellFill { fill: CellFillSpec, cells: Vec<(u32, u32)> },
    /// Merges the rectangle `from`–`to` (inclusive; either order of the corners) into its top-left
    /// cell (`table.merge`). The rectangle grows over merged cells it overlaps, as the single-user
    /// command does, and the merged cells' text joins the anchor's. A queued operation addressed
    /// to a cell the merge covered is refused after it, so its text is never written where nobody
    /// can see it. Merge class: no cell index moves.
    TableMerge { from: (u32, u32), to: (u32, u32) },
    /// Splits the merged cell at `cell` (`table.split`). Cells keep their indices, so no address
    /// moves; a cell that is not merged is refused before anything is written.
    TableSplit { cell: (u32, u32) },
    /// Sets one column's width to a final value in points (`table.columnWidth`). With
    /// `keep_total` the column to its right absorbs the difference so the table keeps its total
    /// width, as dragging an inner border does; the pair is recomputed against the authoritative
    /// widths, so every replica reaches the same pair.
    TableColumnWidth {
        col: u32,
        width: f64,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        keep_total: bool,
    },
    /// Sets one row's height to a final minimum in points (`table.rowHeight`). A row still grows
    /// to fit its text, as in single-user editing.
    TableRowHeight { row: u32, height: f64 },
    /// Replaces a table's six style options (`table.options`): header/total row, banded rows,
    /// first/last column and banded columns. They are independent booleans rather than one value,
    /// so the operation carries **all six as final values** — the sender computes the set it
    /// wants from the table it sees, and every replica applies that set. A partial value would
    /// make the result depend on which replica's stale copy a later merge started from. The
    /// operation addresses the table shape alone: no cell address moves and no cell content
    /// changes, so it is merge class.
    TableOptions { header_row: bool, total_row: bool, banded_rows: bool, first_column: bool, last_column: bool, banded_columns: bool },
    /// Replaces a SmartArt graphic's **whole** diagram value and regenerates its shapes
    /// (`smartArt.setDiagram`), and is the confirmed command of `smartArt.setNodes`, `addNode`,
    /// `removeNode`, `promote`, `demote`, `layout`, `style` and `colors`. The sender applies the
    /// command to the diagram it sees and sends the whole resulting value — layout, outline
    /// (text, levels, assistants, titles, pictures), style and colours — so every replica
    /// regenerates from the same final value and no replica's stale copy decides an unnamed
    /// field. `diagram` is the model `Diagram` (`crates/model/src/lib.rs`) as JSON; the value is
    /// size-bounded and the layout, style and colours are checked before the first write, like
    /// any other hostile input. `target.shape` addresses the diagram group.
    SmartArtSet { diagram: serde_json::Value },
    /// Inserts a chart from its **whole** value (`insert.chart`): the author's normaliser runs the
    /// single-user command on the chart it would create and sends the resulting value, so every
    /// replica creates exactly that chart (its type, data, options and sample-derived fields) and
    /// no replica's own sample decides a field. `rect` is the final box in slide points; without
    /// it the command places the same default box on every replica. Structural: a shape appears.
    ChartInsert { chart: serde_json::Value, rect: Option<[f64; 4]> },
    /// Replaces a chart's **whole** value (`chart.setWhole`) and is the confirmed command of
    /// `chart.type`, `chart.data` and `chart.options`. The sender applies the command to the chart
    /// it sees and sends the whole resulting `Chart` model value as JSON — type, categories,
    /// series, options and any kept part — so every replica ends with the same chart and no
    /// replica's stale copy decides a field. `target.shape` addresses the chart shape; the value
    /// is size-bounded and checked before the first write, like any other hostile input.
    ChartSet { chart: serde_json::Value },
    /// Puts back the exact value an undoable step replaced (ADR 0010). The server builds this
    /// from the actor's history and never a client from a user edit: `value` is the model value
    /// of `field` as JSON (`null` = unset), because the user-facing fill, line and format
    /// commands accept only a lossy subset of the model. A replica runs it through
    /// `shape.setField`, so every replica puts the same value back.
    ShapeField { field: ShapeField, value: serde_json::Value },
    /// Puts back one table cell's whole text body, exactly (ADR 0010). A `format.*` step edits
    /// runs and paragraphs inside one cell, and its arguments cannot describe the value they
    /// replaced, so the image is that cell's body itself — not the table's, and not the shape's
    /// (a table's own `text` is empty). `target.slide` and `target.shape` address the table,
    /// `cell` the cell inside it; an address that names no cell of that table is refused before
    /// anything is written. Merge class: it replaces one body and changes no structure, so a
    /// neighbouring cell keeps whatever anyone else wrote there.
    ///
    /// Like [`Payload::ShapeField`], the server builds this from the actor's history only.
    CellText { cell: (u32, u32), body: serde_json::Value },
    /// Puts back the whole text bodies of several table cells in one operation (ADR 0010). A
    /// `format.*` step can address N cells of one table, so its undo restores exactly those
    /// cells, atomically: `target.slide` and `target.shape` address the table, every item names
    /// one cell and carries its body. A cell the table does not have, an empty list and a
    /// repeated cell are refused before anything is written, and a neighbour is never touched.
    ///
    /// Like [`Payload::CellText`], the server builds this from the actor's history only.
    CellsText { cells: Vec<CellBody> },
    /// Puts back the exact values of several shapes in one operation. A group's `shape.fill` or
    /// `shape.line` also formats every visible member, so its undo restores the group and each
    /// member atomically: `target.slide` names the slide, every item names one shape on it.
    /// Like [`Payload::ShapeField`], the server builds this from the actor's history only.
    ShapeFields { items: Vec<ShapeFieldItem> },
    /// Puts back shapes a step removed (undo of `shape.delete`, or the redo of an insert) from the
    /// server's whole-shape images. Each item is one shape the step changed, with its model value
    /// as JSON (its id included, so every replica restores the same object), its place among its
    /// siblings and the animations that name it or a descendant. `remove` names shapes the same
    /// atomic step takes out first: the undo of `arrange.group` removes the group and puts its
    /// members back at the top level, the redo removes the members and puts the whole group back.
    /// Structural: a restored shape shifts the z-order of its siblings and can carry a whole group.
    ///
    /// Like [`Payload::ShapeField`], the server builds this from the actor's history only.
    ShapeRestore {
        items: Vec<ShapeRestoreItem>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        remove: Vec<ShapeKey>,
    },
    /// Puts back a deleted slide from the server's whole-slide image (undo of `slide.delete`, or
    /// the redo of `slide.new`/`slide.duplicate`). `slide` is the model serialised as JSON; the
    /// membership lists return it to the sections and custom shows it belonged to. Structural:
    /// it changes every index after `index`.
    SlideRestore {
        index: u32,
        slide: serde_json::Value,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        sections: Vec<(u32, u32)>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        shows: Vec<(u32, u32)>,
    },
    /// Final boxes for several shapes of one slide, in one atomic step. `arrange.align` and
    /// `arrange.distribute` move every target at once and must reach every replica as one
    /// undoable step; each item carries the final box, so no replica recomputes the reference
    /// (a slide edge, a selection frame or the gaps between objects). Like [`Payload::ShapeMove`],
    /// the values are absolute and the delta is the changed geometry.
    ShapesSetBounds { items: Vec<ShapeBounds> },
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
    /// Copies the slide in `target.slide` and inserts the copy right after it. Structural: it
    /// changes every index after the copy and allocates ids.
    SlideDuplicate {},
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

/// Audio or video addressed by a digest on the media channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    Video,
    Audio,
}

/// One face slot of an embedded font: the four slots `p:embeddedFont` stores, in the order the
/// package writes them (`regular`, `bold`, `italic`, `boldItalic`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FontFace {
    Regular,
    Bold,
    Italic,
    BoldItalic,
}

impl FontFace {
    /// The name the engine command and the model use.
    pub fn as_str(self) -> &'static str {
        match self {
            FontFace::Regular => "regular",
            FontFace::Bold => "bold",
            FontFace::Italic => "italic",
            FontFace::BoldItalic => "boldItalic",
        }
    }

    /// One face slot from an engine argument (`regular`, `bold`, `italic`, `boldItalic`).
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "regular" => Some(FontFace::Regular),
            "bold" => Some(FontFace::Bold),
            "italic" => Some(FontFace::Italic),
            "boldItalic" => Some(FontFace::BoldItalic),
            _ => None,
        }
    }
}

/// The axis of a table a [`Payload::TableEdit`] changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableAxis {
    Row,
    Column,
}

/// The shape value a [`Payload::ShapeField`] restore puts back. These are exactly the values
/// the v1 command surface can replace but not reproduce: a fill or an outline with fields the
/// fill/line commands do not carry, and a whole text body with its run and paragraph formatting.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShapeField {
    Fill,
    Line,
    Text,
    /// The whole table in a table shape, for the undo of a row/column edit: a structural table
    /// step changes the cells' indices and their values, so only the whole table is exact.
    Table,
    /// The whole SmartArt group — its diagram value and the shapes generated from it — for the
    /// undo of an outline, layout, style or colour edit. An outline edit regenerates the children
    /// with fresh ids and replaces any drawing an imported file brought, so the diagram value
    /// alone cannot reproduce the step; the group can.
    SmartArt,
    /// The whole chart shape — its box and its `Chart` value (type, data, options and kept parts)
    /// — for the undo of a type, data or options edit. The user-facing chart commands express only
    /// part of the model and a kept chart part cannot be replayed from arguments, so the whole
    /// shape is the exact image of the step. The value names the shape it restores.
    Chart,
}

/// One shape value in a [`Payload::ShapeFields`] merge restore.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShapeFieldItem {
    pub shape: ShapeKey,
    pub field: ShapeField,
    /// The model value as JSON; `null` = unset.
    pub value: serde_json::Value,
}

/// One table cell's whole body in a [`Payload::CellsText`] restore.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CellBody {
    pub cell: (u32, u32),
    /// The model `TextBody` as JSON.
    pub body: serde_json::Value,
}

/// One shape in a [`Payload::ShapesSetBounds`] step: the final box in slide points. The shape is
/// named by key, never by id from the sender's replica.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShapeBounds {
    pub shape: ShapeKey,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// One shape in a [`Payload::ShapeRestore`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShapeRestoreItem {
    /// The key the shape comes back under: keys are stable for the session, so a restore reuses
    /// the key the deleted shape had.
    pub shape: ShapeKey,
    /// The group that holds the shape; `None` = the top level of the slide.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<ShapeKey>,
    /// Position among the parent's children (`0` = back). A neighbour that came or went may have
    /// shifted it: the engine clamps to the list it finds.
    pub index: u32,
    /// The whole `Shape` model as JSON, carrying its own id and every field.
    pub value: serde_json::Value,
    /// Animation entries that name the shape or a descendant, and effects of other shapes
    /// triggered by one of them. The delete dropped them; the restore puts them back.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub animations: Vec<serde_json::Value>,
}

/// A final fill value. Only absolute forms are on the wire: a solid colour from the palette or
/// the theme, no fill, or the slide background showing through.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FillSpec {
    Solid { color: String },
    None,
    Background,
}

impl FillSpec {
    /// The `shape.fill` arguments that reproduce this value.
    pub fn args(&self) -> serde_json::Value {
        match self {
            FillSpec::Solid { color } => serde_json::json!({"color": color}),
            FillSpec::None => serde_json::json!({"none": true}),
            FillSpec::Background => serde_json::json!({"background": true}),
        }
    }
}

/// A table cell's fill as `table.cellFill` expresses it: one solid colour, an explicit "no fill"
/// or no explicit fill at all (the table style's own fill shows; the model's `None`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CellFillSpec {
    /// One solid colour, e.g. `#ff0000`.
    Solid { color: String },
    /// An explicit empty fill on the cell.
    None,
    /// No explicit cell fill: the style's own fill shows through.
    Inherit,
}

impl CellFillSpec {
    /// The `table.cellFill` arguments that reproduce this value.
    pub fn args(&self) -> serde_json::Value {
        match self {
            CellFillSpec::Solid { color } => serde_json::json!({"color": color}),
            CellFillSpec::None => serde_json::json!({"none": true}),
            CellFillSpec::Inherit => serde_json::json!({}),
        }
    }
}

/// A final slide transition, field for field the model's `Transition` (`crates/model/src/anim.rs`):
/// the effect id, its option, the duration, the advance settings, the embedded sound and the
/// unmodelled XML a file kept for round-trip. `None` on `Payload::SlideTransition` means the
/// slide has no transition, which is the model's own representation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TransitionSpec {
    /// Effect id: `none`, `fade`, `push`, `wipe`, … (the model's `TRANSITIONS`), or the local name
    /// of an effect the model does not know, in which case `raw` carries its XML.
    pub kind: String,
    /// Effect option, e.g. a direction (`l`, `r`, `u`, `d`, `horz`, `vert`).
    pub option: String,
    pub duration_ms: u32,
    pub advance_on_click: bool,
    pub advance_after_ms: Option<u32>,
    /// Embedded sound media id; the object lives in the document's media list on every replica.
    pub sound: Option<u32>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub sound_loop: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub stop_sound: bool,
    /// Unmodelled transition XML kept for round-trip.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
}

impl Default for TransitionSpec {
    /// The model's defaults (an empty `Transition`), so a payload that omits a field means the
    /// same value on every replica.
    fn default() -> Self {
        TransitionSpec {
            kind: "none".into(),
            option: String::new(),
            duration_ms: 1000,
            advance_on_click: true,
            advance_after_ms: None,
            sound: None,
            sound_loop: false,
            stop_sound: false,
            raw: None,
        }
    }
}

/// A final outline. A field left out is not a relative change: the confirmed command carries the
/// same fields, so every replica leaves that part of the outline as it is.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LineSpec {
    /// Clears the outline; the other fields are then ignored.
    #[serde(default)]
    pub none: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cap: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub join: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compound: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head_size: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tail_size: Option<String>,
}

impl LineSpec {
    /// The `shape.line` arguments that reproduce this value. `None` clears the outline.
    pub fn args(&self) -> serde_json::Value {
        let mut out = serde_json::Map::new();
        if self.none {
            out.insert("none".into(), serde_json::Value::Bool(true));
            return serde_json::Value::Object(out);
        }
        if let Some(color) = &self.color {
            out.insert("color".into(), serde_json::Value::String(color.clone()));
        }
        if let Some(width) = self.width
            && let Some(n) = serde_json::Number::from_f64(width)
        {
            out.insert("width".into(), serde_json::Value::Number(n));
        }
        for (key, value) in [
            ("dash", &self.dash),
            ("cap", &self.cap),
            ("join", &self.join),
            ("compound", &self.compound),
            ("head", &self.head),
            ("tail", &self.tail),
            ("headSize", &self.head_size),
            ("tailSize", &self.tail_size),
        ] {
            if let Some(v) = value {
                out.insert(key.into(), serde_json::Value::String(v.clone()));
            }
        }
        serde_json::Value::Object(out)
    }
}

/// The character/paragraph formatting v1 carries over the wire. Each maps to the engine command
/// of the same id; the arguments are absolute (`on: true`, an explicit colour, an explicit size).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormatCmd {
    Bold,
    Italic,
    Underline,
    Strikethrough,
    Superscript,
    Subscript,
    Font,
    Size,
    Color,
    Highlight,
    AlignLeft,
    AlignCenter,
    AlignRight,
    Justify,
    Caps,
    Spacing,
    Clear,
}

impl FormatCmd {
    /// The engine command id, also the name on the wire.
    pub fn id(self) -> &'static str {
        match self {
            FormatCmd::Bold => "format.bold",
            FormatCmd::Italic => "format.italic",
            FormatCmd::Underline => "format.underline",
            FormatCmd::Strikethrough => "format.strikethrough",
            FormatCmd::Superscript => "format.superscript",
            FormatCmd::Subscript => "format.subscript",
            FormatCmd::Font => "format.font",
            FormatCmd::Size => "format.size",
            FormatCmd::Color => "format.color",
            FormatCmd::Highlight => "format.highlight",
            FormatCmd::AlignLeft => "format.alignLeft",
            FormatCmd::AlignCenter => "format.alignCenter",
            FormatCmd::AlignRight => "format.alignRight",
            FormatCmd::Justify => "format.justify",
            FormatCmd::Caps => "format.caps",
            FormatCmd::Spacing => "format.spacing",
            FormatCmd::Clear => "format.clear",
        }
    }

    /// Parse the engine command id.
    pub fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "format.bold" => FormatCmd::Bold,
            "format.italic" => FormatCmd::Italic,
            "format.underline" => FormatCmd::Underline,
            "format.strikethrough" => FormatCmd::Strikethrough,
            "format.superscript" => FormatCmd::Superscript,
            "format.subscript" => FormatCmd::Subscript,
            "format.font" => FormatCmd::Font,
            "format.size" => FormatCmd::Size,
            "format.color" => FormatCmd::Color,
            "format.highlight" => FormatCmd::Highlight,
            "format.alignLeft" => FormatCmd::AlignLeft,
            "format.alignCenter" => FormatCmd::AlignCenter,
            "format.alignRight" => FormatCmd::AlignRight,
            "format.justify" => FormatCmd::Justify,
            "format.caps" => FormatCmd::Caps,
            "format.spacing" => FormatCmd::Spacing,
            "format.clear" => FormatCmd::Clear,
            _ => return None,
        })
    }

    /// Whether `args` is a value of this command's own shape: only the keys it reads, no address
    /// (`id`/`ids`/`index`) that would move the command off the operation's target.
    pub fn accepts(self, args: &serde_json::Value) -> bool {
        let Some(map) = args.as_object() else { return false };
        let allowed: &[&str] = match self {
            FormatCmd::Bold | FormatCmd::Italic | FormatCmd::Superscript | FormatCmd::Subscript => &["on"],
            FormatCmd::Underline => &["on", "style"],
            FormatCmd::Strikethrough => &["on", "double"],
            FormatCmd::Font => &["family"],
            FormatCmd::Size => &["size"],
            FormatCmd::Color | FormatCmd::Highlight => &["color"],
            FormatCmd::Caps => &["caps"],
            FormatCmd::Spacing => &["spacing"],
            FormatCmd::AlignLeft | FormatCmd::AlignCenter | FormatCmd::AlignRight | FormatCmd::Justify | FormatCmd::Clear => &[],
        };
        map.keys().all(|k| allowed.contains(&k.as_str()))
    }
}

impl Payload {
    /// Command id this operation normalises to, for logs and allowlist checks.
    pub fn command(&self) -> &'static str {
        match self {
            Payload::ShapeMove { .. } => "shape.move",
            Payload::ShapeSetBounds { .. } => "shape.setBounds",
            Payload::TextSet { .. } => "text.set",
            Payload::TextInsert { .. } => "text.insert",
            Payload::TextDelete { .. } => "text.delete",
            Payload::ShapeInsert { .. } => "shape.insert",
            Payload::PictureInsert { .. } => "insert.picture",
            Payload::MediaInsert { kind, .. } => match kind {
                MediaKind::Video => "insert.video",
                MediaKind::Audio => "insert.audio",
            },
            Payload::ShapeFill { .. } => "shape.fill",
            Payload::ShapeLine { .. } => "shape.line",
            Payload::SlideTransition { .. } => "slide.transition.set",
            Payload::ShapeAnimations { .. } => "slide.animation.set",
            Payload::SlideAnimations { .. } => "slide.animations.set",
            Payload::NotesSet { .. } => "notes.set",
            Payload::NotesInsert { .. } => "notes.insert",
            Payload::NotesDelete { .. } => "notes.delete",
            Payload::FontCreate { .. } => "font.create",
            Payload::FontSet { .. } => "font.set",
            Payload::FontRemove { .. } => "font.remove",
            Payload::FontRestore { .. } => "font.restore",
            Payload::FormatSet { format, .. } => format.id(),
            Payload::TableEdit { .. } => "table.edit",
            Payload::TableStyle { .. } => "table.style",
            Payload::TableCellFill { .. } => "table.cellFill",
            Payload::TableMerge { .. } => "table.merge",
            Payload::TableSplit { .. } => "table.split",
            Payload::TableColumnWidth { .. } => "table.columnWidth",
            Payload::TableRowHeight { .. } => "table.rowHeight",
            Payload::TableOptions { .. } => "table.options",
            Payload::SmartArtSet { .. } => "smartArt.setDiagram",
            Payload::ChartInsert { .. } => "insert.chart",
            Payload::ChartSet { .. } => "chart.setWhole",
            Payload::ShapeField { .. } => "shape.setField",
            Payload::CellText { .. } => "shape.setCellBody",
            Payload::CellsText { .. } => "shape.setCellBodies",
            Payload::ShapeFields { .. } => "shape.setFields",
            Payload::ShapesSetBounds { .. } => "shape.setBoundsMany",
            Payload::ShapeRestore { .. } => "shape.restore",
            Payload::SlideRestore { .. } => "slide.restore",
            Payload::ShapeDelete {} => "edit.delete",
            Payload::SlideInsert { .. } => "slide.new",
            Payload::SlideDelete {} => "slide.delete",
            Payload::SlideMove { .. } => "slide.move",
            Payload::SlideDuplicate {} => "slide.duplicate",
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
    /// Set when this operation moves the sender's history rather than making a new step. The
    /// adapter verifies the objects still hold what the targeted step left before applying, and
    /// answers `undo_conflict` otherwise (ADR 0010 §3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history: Option<HistoryAction>,
}

/// Which way one actor's history moves. Undo is a new operation in every case: it is sent with
/// `baseVersion = head`, checked like any other, and gets its own version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum HistoryAction {
    Undo {
        /// The operation being taken back; the adapter expects the objects to still hold what it
        /// left behind.
        op_id: String,
    },
    Redo {
        /// The operation being re-applied after an undo.
        op_id: String,
    },
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
    Resync {
        last_version: u64,
    },
    Commit {
        expected_file_version: Option<String>,
    },
    /// Take back this actor's latest step. The server owns the before/after image and answers with
    /// an ordinary `applied` (ADR 0010).
    Undo,
    /// Re-apply the step that this actor's latest undo took back.
    Redo,
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

/// The session key map in wire form, so a client that joins after the journal tail was
/// compacted addresses the same objects as everyone else (ADR-0002).
///
/// Keys are session-scoped strings issued in walk order; they cannot be re-derived from the
/// document after structural edits, so a base carries the map and the issuer counter.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotKeys {
    /// Key → slide id.
    #[serde(default)]
    pub slides: Vec<(String, u32)>,
    /// Key → (slide id, shape id).
    #[serde(default)]
    pub shapes: Vec<(String, u32, u32)>,
    /// Keys of shapes that were deleted; a late operation must be refused (ADR-0002 §3).
    #[serde(default)]
    pub dead: Vec<(String, u32)>,
    /// The number the issuer had reached, so the next key a replica issues is the server's next.
    #[serde(default)]
    pub next: u64,
}

/// The native base a client replaces its replica with after compaction (ADR-0005 §4.5).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseSnapshot {
    /// The version the base represents; the tail carries the steps after it.
    pub version: u64,
    /// The native `.deckcraft` file. Absent when it is larger than the server's inline limit, in
    /// which case the client has to reopen the file from the cabinet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes_base64: Option<String>,
    pub keys: SnapshotKeys,
}

/// Current state for a reconnecting client: a base and, optionally, the journal tail.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotMsg {
    pub version: u64,
    pub snapshot: SnapshotRef,
    #[serde(default)]
    pub tail: Vec<Applied>,
    /// Present when the journal tail no longer covers the client's version: the replica is
    /// rebuilt from this base and the tail above it (ADR-0005, RFC §4.5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<BaseSnapshot>,
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
    use serde_json::json;

    fn op() -> OpEnvelope {
        OpEnvelope {
            op_id: "6f1c".into(),
            doc: "f42".into(),
            base_version: 41,
            kind: OpKind::Command,
            target: Target { slide: Some("s1".into()), shape: Some("7".into()), ..Target::default() },
            payload: Payload::ShapeMove { x: 120.0, y: 64.0 },
            history: None,
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
    fn a_final_bounds_step_round_trips_as_a_command() {
        let payload = Payload::ShapesSetBounds {
            items: vec![
                ShapeBounds { shape: "7".into(), x: 210.0, y: 100.0, w: 200.0, h: 100.0 },
                ShapeBounds { shape: "8".into(), x: 250.0, y: 100.0, w: 120.0, h: 100.0 },
            ],
        };
        assert_eq!(payload.command(), "shape.setBoundsMany");
        let msg = ClientMsg::Op(OpEnvelope {
            op_id: "6f1c".into(),
            doc: "f42".into(),
            base_version: 7,
            kind: OpKind::Command,
            target: Target { slide: Some("s1".into()), shapes: vec!["7".into(), "8".into()], ..Target::default() },
            payload,
            history: None,
        });
        let text = encode_client(&msg).expect("encode");
        assert!(text.contains("\"cmd\":\"shapes_set_bounds\""), "{text}");
        assert!(text.contains("\"shape\":\"7\""), "{text}");
        assert_eq!(decode_client(&text).expect("decode"), msg);
    }

    #[test]
    fn undo_and_redo_requests_round_trip() {
        for msg in [ClientMsg::Undo, ClientMsg::Redo] {
            let text = encode_client(&msg).expect("encode");
            assert_eq!(decode_client(&text).expect("decode"), msg);
        }
    }

    #[test]
    fn undo_is_an_operation_with_its_own_version() {
        let mut envelope = op();
        envelope.history = Some(HistoryAction::Undo { op_id: "op-0".into() });
        let text = encode_client(&ClientMsg::Op(envelope.clone())).expect("encode");
        assert!(text.contains("\"action\":\"undo\""), "{text}");
        assert!(text.contains("\"baseVersion\":41"), "{text}");
        assert_eq!(decode_client(&text).expect("decode"), ClientMsg::Op(envelope));
    }

    #[test]
    fn a_new_step_has_no_history_action() {
        let text = encode_client(&ClientMsg::Op(op())).expect("encode");
        assert!(!text.contains("history"), "{text}");
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
    fn a_snapshot_base_carries_the_bytes_and_the_key_map() {
        let keys = SnapshotKeys { slides: vec![("d-s1".into(), 3)], shapes: vec![("d-h2".into(), 3, 7)], dead: vec![], next: 2 };
        let msg = ServerMsg::Snapshot(SnapshotMsg {
            version: 9,
            snapshot: SnapshotRef { mode: SnapshotMode::Base, cursor: "base@4".into() },
            tail: vec![],
            base: Some(BaseSnapshot { version: 4, bytes_base64: Some("UEsD".into()), keys: keys.clone() }),
        });
        let text = encode_server(&msg).expect("encode");
        assert!(text.contains("\"mode\":\"base\""), "{text}");
        assert!(text.contains("\"next\":2"), "{text}");
        assert_eq!(decode_server(&text).expect("decode"), msg);
    }

    #[test]
    fn a_snapshot_without_a_base_still_decodes() {
        let text = r#"{"type":"snapshot","version":3,"snapshot":{"mode":"base_and_tail","cursor":"tail@3"},"tail":[]}"#;
        let msg = decode_server(text).expect("decode");
        assert!(matches!(msg, ServerMsg::Snapshot(SnapshotMsg { base: None, .. })));
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
        assert_eq!(Payload::TextInsert { at: 0, text: "hi".into(), cell: None }.command(), "text.insert");
        assert_eq!(Payload::TextDelete { from: 0, to: 1, cell: None }.command(), "text.delete");
        assert_eq!(Payload::TableEdit { axis: TableAxis::Row, at: 0, insert: true }.command(), "table.edit");
        assert_eq!(Payload::SlideMove { before: None }.command(), "slide.move");
        assert_eq!(Payload::Reorder { before: None }.command(), "arrange.reorder");
        assert_eq!(Payload::SlideTransition { transition: None }.command(), "slide.transition.set");
        assert_eq!(Payload::ShapeAnimations { animations: Vec::new() }.command(), "slide.animation.set");
        assert_eq!(Payload::SlideAnimations { animations: Vec::new() }.command(), "slide.animations.set");
    }

    #[test]
    fn a_shape_animation_list_travels_as_whole_model_values() {
        let fade = serde_json::json!({"shape": 7, "effect": "fade", "class": "entrance", "durationMs": 500});
        let pulse = serde_json::json!({"shape": 7, "effect": "pulse", "class": "emphasis", "repeat": 3});
        let payload = Payload::ShapeAnimations { animations: vec![fade.clone(), pulse.clone()] };
        let text = serde_json::to_string(&payload).expect("encode");
        assert!(text.contains("\"cmd\":\"shape_animations\""), "{text}");
        assert!(text.contains("\"effect\":\"pulse\""), "{text}");
        assert_eq!(serde_json::from_str::<Payload>(&text).expect("decode"), payload);
        let empty = Payload::ShapeAnimations { animations: Vec::new() };
        assert_eq!(serde_json::from_str::<Payload>(&serde_json::to_string(&empty).expect("encode")).expect("decode"), empty);
    }

    #[test]
    fn a_slide_animation_list_travels_with_every_steps_own_shape() {
        let steps = vec![
            serde_json::json!({"shape": 4, "effect": "fade", "class": "entrance"}),
            serde_json::json!({"shape": 9, "effect": "pulse", "class": "emphasis", "repeat": 2}),
            serde_json::json!({"shape": 4, "effect": "zoom", "class": "entrance"}),
        ];
        let payload = Payload::SlideAnimations { animations: steps.clone() };
        let text = serde_json::to_string(&payload).expect("encode");
        assert!(text.contains("\"cmd\":\"slide_animations\""), "{text}");
        assert!(text.contains("\"shape\":9"), "a step keeps its own shape: {text}");
        assert_eq!(serde_json::from_str::<Payload>(&text).expect("decode"), payload);
        let empty = Payload::SlideAnimations { animations: Vec::new() };
        assert_eq!(serde_json::from_str::<Payload>(&serde_json::to_string(&empty).expect("encode")).expect("decode"), empty);
    }

    #[test]
    fn notes_operations_address_a_slide_and_carry_a_whole_body() {
        let body = serde_json::json!({"paragraphs": [{"runs": [{"text": "hello", "props": {"bold": true}}]}]});
        let set = Payload::NotesSet { notes: body.clone() };
        let text = serde_json::to_string(&set).expect("encode");
        assert!(text.contains("\"cmd\":\"notes_set\""), "{text}");
        assert!(text.contains("\"hello\""), "{text}");
        assert_eq!(serde_json::from_str::<Payload>(&text).expect("decode"), set);
        assert_eq!(set.command(), "notes.set");

        let insert = Payload::NotesInsert { at: 4, text: "X".into() };
        assert!(serde_json::to_string(&insert).expect("encode").contains("\"cmd\":\"notes_insert\""));
        assert_eq!(serde_json::from_str::<Payload>(&serde_json::to_string(&insert).expect("encode")).expect("decode"), insert);
        assert_eq!(insert.command(), "notes.insert");

        let delete = Payload::NotesDelete { from: 1, to: 3 };
        assert!(serde_json::to_string(&delete).expect("encode").contains("\"cmd\":\"notes_delete\""));
        assert_eq!(serde_json::from_str::<Payload>(&serde_json::to_string(&delete).expect("encode")).expect("decode"), delete);
        assert_eq!(delete.command(), "notes.delete");
    }

    #[test]
    fn a_document_font_travels_as_an_entry_and_its_faces_by_digest() {
        let create =
            Payload::FontCreate { typeface: "Deck Face".into(), face: FontFace::Bold, sha256: "ab".repeat(32), name: "DeckFace-Bold.ttf".into() };
        let text = serde_json::to_string(&create).expect("encode");
        assert!(text.contains("\"cmd\":\"font_create\""), "{text}");
        assert!(text.contains("\"face\":\"bold\""), "{text}");
        assert_eq!(serde_json::from_str::<Payload>(&text).expect("decode"), create);
        assert_eq!(create.command(), "font.create");
        assert_eq!(FontFace::parse("boldItalic"), Some(FontFace::BoldItalic));
        assert_eq!(FontFace::BoldItalic.as_str(), "boldItalic");
        assert_eq!(FontFace::parse("black"), None);

        let set =
            Payload::FontSet { typeface: "Deck Face".into(), panose: Some("020B0604020202020204".into()), pitch_family: Some(2), charset: None };
        let text = serde_json::to_string(&set).expect("encode");
        assert!(text.contains("\"cmd\":\"font_set\""), "{text}");
        assert!(!text.contains("charset"), "an absent descriptor is not sent: {text}");
        assert_eq!(serde_json::from_str::<Payload>(&text).expect("decode"), set);
        assert_eq!(set.command(), "font.set");

        let remove = Payload::FontRemove { typeface: "Deck Face".into() };
        assert!(serde_json::to_string(&remove).expect("encode").contains("\"cmd\":\"font_remove\""));
        assert_eq!(serde_json::from_str::<Payload>(&serde_json::to_string(&remove).expect("encode")).expect("decode"), remove);
        assert_eq!(remove.command(), "font.remove");

        // The undo image is the whole model entry or `null` (no entry before the step).
        let restore = Payload::FontRestore { typeface: "Deck Face".into(), font: Some(serde_json::json!({"typeface": "Deck Face", "regular": 7})) };
        let text = serde_json::to_string(&restore).expect("encode");
        assert!(text.contains("\"cmd\":\"font_restore\""), "{text}");
        assert_eq!(serde_json::from_str::<Payload>(&text).expect("decode"), restore);
        assert_eq!(restore.command(), "font.restore");
        let gone = Payload::FontRestore { typeface: "Deck Face".into(), font: None };
        let text = serde_json::to_string(&gone).expect("encode");
        assert_eq!(serde_json::from_str::<Payload>(&text).expect("decode"), gone);
    }

    #[test]
    fn a_cell_address_travels_with_text_and_a_table_step() {
        let insert = Payload::TextInsert { at: 3, text: "x".into(), cell: Some((1, 2)) };
        let text = serde_json::to_string(&insert).expect("encode");
        assert!(text.contains("\"cell\":[1,2]"), "{text}");
        assert_eq!(serde_json::from_str::<Payload>(&text).expect("decode"), insert);

        let delete = Payload::TextDelete { from: 0, to: 3, cell: Some((1, 2)) };
        let text = serde_json::to_string(&delete).expect("encode");
        assert!(text.contains("\"cell\":[1,2]"), "{text}");
        assert_eq!(serde_json::from_str::<Payload>(&text).expect("decode"), delete);

        // A body operation on a shape stays free of the field.
        let shape = Payload::TextInsert { at: 0, text: "x".into(), cell: None };
        assert!(!serde_json::to_string(&shape).expect("encode").contains("cell"));

        let step = Payload::TableEdit { axis: TableAxis::Row, at: 4, insert: true };
        let text = serde_json::to_string(&step).expect("encode");
        assert!(text.contains("\"cmd\":\"table_edit\""), "{text}");
        assert!(text.contains("\"axis\":\"row\""), "{text}");
        assert_eq!(serde_json::from_str::<Payload>(&text).expect("decode"), step);
        assert_eq!(Payload::TableEdit { axis: TableAxis::Column, at: 0, insert: false }.command(), "table.edit");
    }

    #[test]
    fn the_rest_of_the_table_surface_travels_as_final_values() {
        let style = Payload::TableStyle { style: "grid".into() };
        let text = serde_json::to_string(&style).expect("encode");
        assert!(text.contains("\"cmd\":\"table_style\""), "{text}");
        assert_eq!(serde_json::from_str::<Payload>(&text).expect("decode"), style);
        assert_eq!(style.command(), "table.style");

        let fill = Payload::TableCellFill { fill: CellFillSpec::Solid { color: "#ff0000".into() }, cells: vec![(0, 0), (0, 1)] };
        let text = serde_json::to_string(&fill).expect("encode");
        assert!(text.contains("\"cmd\":\"table_cell_fill\""), "{text}");
        assert!(text.contains("\"cells\":[[0,0],[0,1]]"), "{text}");
        assert_eq!(serde_json::from_str::<Payload>(&text).expect("decode"), fill);
        assert_eq!(fill.command(), "table.cellFill");
        assert_eq!(CellFillSpec::Solid { color: "#ff0000".into() }.args(), json!({"color": "#ff0000"}));
        assert_eq!(CellFillSpec::None.args(), json!({"none": true}));
        assert_eq!(CellFillSpec::Inherit.args(), json!({}));
        assert_eq!(
            serde_json::from_str::<Payload>(
                &serde_json::to_string(&Payload::TableCellFill { fill: CellFillSpec::None, cells: vec![(1, 1)] }).expect("encode")
            )
            .expect("decode"),
            Payload::TableCellFill { fill: CellFillSpec::None, cells: vec![(1, 1)] }
        );

        let merge = Payload::TableMerge { from: (0, 0), to: (1, 1) };
        let text = serde_json::to_string(&merge).expect("encode");
        assert!(text.contains("\"cmd\":\"table_merge\""), "{text}");
        assert_eq!(serde_json::from_str::<Payload>(&text).expect("decode"), merge);
        assert_eq!(merge.command(), "table.merge");

        let split = Payload::TableSplit { cell: (0, 0) };
        assert_eq!(serde_json::from_str::<Payload>(&serde_json::to_string(&split).expect("encode")).expect("decode"), split);
        assert_eq!(split.command(), "table.split");

        let width = Payload::TableColumnWidth { col: 1, width: 120.0, keep_total: true };
        let text = serde_json::to_string(&width).expect("encode");
        assert!(text.contains("\"keep_total\":true"), "{text}");
        assert_eq!(serde_json::from_str::<Payload>(&text).expect("decode"), width);
        assert_eq!(width.command(), "table.columnWidth");
        // Without `keepTotal` the field is not on the wire and decodes to the same value.
        let plain = Payload::TableColumnWidth { col: 0, width: 80.0, keep_total: false };
        let text = serde_json::to_string(&plain).expect("encode");
        assert!(!text.contains("keep_total"), "{text}");
        assert_eq!(serde_json::from_str::<Payload>(&text).expect("decode"), plain);

        let height = Payload::TableRowHeight { row: 3, height: 40.0 };
        assert_eq!(serde_json::from_str::<Payload>(&serde_json::to_string(&height).expect("encode")).expect("decode"), height);
        assert_eq!(height.command(), "table.rowHeight");

        // All six options travel as final values: a partial set would let a replica's own copy
        // decide the rest.
        let options = Payload::TableOptions {
            header_row: true,
            total_row: false,
            banded_rows: true,
            first_column: false,
            last_column: true,
            banded_columns: false,
        };
        let text = serde_json::to_string(&options).expect("encode");
        assert!(text.contains("\"cmd\":\"table_options\""), "{text}");
        for field in [
            "\"header_row\":true",
            "\"total_row\":false",
            "\"banded_rows\":true",
            "\"first_column\":false",
            "\"last_column\":true",
            "\"banded_columns\":false",
        ] {
            assert!(text.contains(field), "{field} missing from {text}");
        }
        assert_eq!(serde_json::from_str::<Payload>(&text).expect("decode"), options);
        assert_eq!(options.command(), "table.options");
    }

    #[test]
    fn a_smartart_edit_travels_as_one_whole_diagram_value() {
        // A SmartArt edit is one whole diagram value: the layout, the outline, the style and the
        // colours travel together, so every replica regenerates from the same final value.
        let diagram = Payload::SmartArtSet {
            diagram: serde_json::json!({
                "kind": "hierarchy",
                "nodes": [{"text": "CEO", "level": 0}, {"text": "CTO", "level": 1}],
                "style": "intense",
                "colors": "accent3",
            }),
        };
        let text = serde_json::to_string(&diagram).expect("encode");
        assert!(text.contains("\"cmd\":\"smart_art_set\""), "{text}");
        assert!(text.contains("\"kind\":\"hierarchy\""), "{text}");
        assert!(text.contains("\"level\":1"), "{text}");
        assert_eq!(serde_json::from_str::<Payload>(&text).expect("decode"), diagram);
        assert_eq!(diagram.command(), "smartArt.setDiagram");

        // The exact restore of a SmartArt step names the whole group, not the diagram value.
        let restore = Payload::ShapeField { field: ShapeField::SmartArt, value: serde_json::json!({"id": 7}) };
        let text = serde_json::to_string(&restore).expect("encode");
        assert!(text.contains("\"field\":\"smart_art\""), "{text}");
        assert_eq!(serde_json::from_str::<Payload>(&text).expect("decode"), restore);
        assert_eq!(restore.command(), "shape.setField");
    }

    #[test]
    fn a_chart_edit_travels_as_one_whole_chart_value() {
        // A chart edit is one whole chart value: the type, the data and the options travel
        // together, so every replica replaces the chart instead of merging arguments against its
        // own copy.
        let chart = Payload::ChartSet {
            chart: serde_json::json!({
                "kind": "pie",
                "categories": ["Q1", "Q2"],
                "series": [{"name": "Sales", "values": [4.2, 2.8]}],
                "title": "Revenue",
                "legend": "b",
            }),
        };
        let text = serde_json::to_string(&chart).expect("encode");
        assert!(text.contains("\"cmd\":\"chart_set\""), "{text}");
        assert!(text.contains("\"kind\":\"pie\""), "{text}");
        assert!(text.contains("\"values\":[4.2,2.8]"), "{text}");
        assert_eq!(serde_json::from_str::<Payload>(&text).expect("decode"), chart);
        assert_eq!(chart.command(), "chart.setWhole");

        // An insertion carries the whole chart and, when the author's command named one, the box.
        let insert = Payload::ChartInsert {
            chart: serde_json::json!({"kind": "column", "categories": ["A"], "series": [{"name": "S", "values": [1.0]}]}),
            rect: Some([10.0, 20.0, 300.0, 200.0]),
        };
        let text = serde_json::to_string(&insert).expect("encode");
        assert!(text.contains("\"cmd\":\"chart_insert\""), "{text}");
        assert!(text.contains("\"rect\":[10.0,20.0,300.0,200.0]"), "{text}");
        assert_eq!(serde_json::from_str::<Payload>(&text).expect("decode"), insert);
        assert_eq!(insert.command(), "insert.chart");

        // The exact restore of a chart step names the whole shape, not the chart value.
        let restore = Payload::ShapeField { field: ShapeField::Chart, value: serde_json::json!({"id": 9}) };
        let text = serde_json::to_string(&restore).expect("encode");
        assert!(text.contains("\"field\":\"chart\""), "{text}");
        assert_eq!(serde_json::from_str::<Payload>(&text).expect("decode"), restore);
        assert_eq!(restore.command(), "shape.setField");
    }

    #[test]
    fn a_transition_travels_as_a_complete_value() {
        let fade = TransitionSpec {
            kind: "fade".into(),
            option: "smoothly".into(),
            duration_ms: 700,
            advance_after_ms: Some(5000),
            sound: Some(3),
            ..TransitionSpec::default()
        };
        let payload = Payload::SlideTransition { transition: Some(fade.clone()) };
        let text = serde_json::to_string(&payload).expect("encode");
        assert!(text.contains("\"cmd\":\"slide_transition\""), "{text}");
        assert!(text.contains("\"advanceAfterMs\":5000"), "{text}");
        assert!(text.contains("\"sound\":3"), "{text}");
        assert!(text.contains("\"advanceOnClick\":true"), "{text}");
        assert_eq!(serde_json::from_str::<Payload>(&text).expect("decode"), payload);

        let none = Payload::SlideTransition { transition: None };
        let text = serde_json::to_string(&none).expect("encode");
        assert_eq!(serde_json::from_str::<Payload>(&text).expect("decode"), none);
        assert_eq!(TransitionSpec::default().kind, "none", "the model's default, not Rust's");
        assert!(TransitionSpec::default().advance_on_click);
    }

    #[test]
    fn a_picture_operation_carries_a_reference_not_bytes() {
        let payload = Payload::PictureInsert { sha256: "9f2c".into(), name: "logo.png".into(), rect: Some([10.0, 20.0, 300.0, 200.0]) };
        let text = serde_json::to_string(&payload).expect("encode");
        assert!(text.contains("\"cmd\":\"picture_insert\""), "{text}");
        assert!(text.contains("\"sha256\":\"9f2c\""), "{text}");
        assert!(!text.contains("data"), "{text}");
        assert_eq!(serde_json::from_str::<Payload>(&text).expect("decode"), payload);
        assert_eq!(payload.command(), "insert.picture");
        let natural = Payload::PictureInsert { sha256: "9f2c".into(), name: "logo.png".into(), rect: None };
        let text = serde_json::to_string(&natural).expect("encode");
        assert!(!text.contains("rect"), "{text}");
    }

    #[test]
    fn media_and_format_payloads_round_trip_and_name_their_command() {
        let media =
            Payload::MediaInsert { kind: MediaKind::Video, sha256: "9f2c".into(), name: "clip.mp4".into(), rect: Some([10.0, 20.0, 300.0, 200.0]) };
        let text = serde_json::to_string(&media).expect("encode");
        assert!(text.contains("\"cmd\":\"media_insert\""), "{text}");
        assert!(text.contains("\"kind\":\"video\""), "{text}");
        assert!(!text.contains("data"), "{text}");
        assert_eq!(serde_json::from_str::<Payload>(&text).expect("decode"), media);
        assert_eq!(media.command(), "insert.video");

        let fill = Payload::ShapeFill { fill: FillSpec::Solid { color: "#ff0000".into() } };
        assert_eq!(fill.command(), "shape.fill");
        assert_eq!(FillSpec::Solid { color: "#ff0000".into() }.args(), json!({"color": "#ff0000"}));
        let text = serde_json::to_string(&fill).expect("encode");
        assert_eq!(serde_json::from_str::<Payload>(&text).expect("decode"), fill);

        let spec = LineSpec { none: false, color: Some("#0044ff".into()), width: Some(2.0), dash: Some("dash".into()), ..LineSpec::default() };
        let line = Payload::ShapeLine { line: spec.clone() };
        assert_eq!(line.command(), "shape.line");
        assert_eq!(spec.args(), json!({"color": "#0044ff", "width": 2.0, "dash": "dash"}));

        let format = Payload::FormatSet { format: FormatCmd::Bold, args: json!({"on": true}), addresses: vec![FormatAddress::default()] };
        assert_eq!(format.command(), "format.bold");
        assert_eq!(FormatCmd::from_id("format.bold"), Some(FormatCmd::Bold));
        assert_eq!(FormatCmd::from_id("format.nope"), None);
        assert_eq!(Payload::SlideDuplicate {}.command(), "slide.duplicate");
    }

    #[test]
    fn a_cell_format_carries_its_address_and_its_body_restore_round_trips() {
        let format = Payload::FormatSet {
            format: FormatCmd::Italic,
            args: json!({"on": true}),
            addresses: vec![FormatAddress { cell: Some((1, 2)), ..FormatAddress::default() }],
        };
        let text = serde_json::to_string(&format).expect("encode");
        assert!(text.contains("\"cell\":[1,2]"), "the cell address travels with the command: {text}");
        assert_eq!(serde_json::from_str::<Payload>(&text).expect("decode"), format);

        // A shape-level format still addresses its one body: the empty address, no `cell` key.
        let plain = Payload::FormatSet { format: FormatCmd::Bold, args: json!({"on": true}), addresses: vec![FormatAddress::default()] };
        let text = serde_json::to_string(&plain).expect("encode");
        assert!(!text.contains("cell"), "the shape body has no cell address: {text}");
        assert!(text.contains("\"addresses\":[{}]"), "the body is one (empty) address: {text}");
        assert_eq!(serde_json::from_str::<Payload>(&text).expect("decode"), plain);

        // The exact restore of a cell body names the same cell and carries the whole body.
        let restore = Payload::CellText { cell: (1, 2), body: json!({"paragraphs": [{"runs": [{"text": "x"}]}]}) };
        let text = serde_json::to_string(&restore).expect("encode");
        assert!(text.contains("\"cmd\":\"cell_text\""), "{text}");
        assert!(text.contains("\"cell\":[1,2]"), "{text}");
        assert_eq!(serde_json::from_str::<Payload>(&text).expect("decode"), restore);
        assert_eq!(restore.command(), "shape.setCellBody");

        // A multi-cell format's undo image is those cells' whole bodies, in one operation.
        let cells = Payload::CellsText {
            cells: vec![
                CellBody { cell: (0, 0), body: json!({"paragraphs": [{"runs": [{"text": "a"}]}]}) },
                CellBody { cell: (0, 1), body: json!({"paragraphs": [{"runs": [{"text": "b"}]}]}) },
            ],
        };
        let text = serde_json::to_string(&cells).expect("encode");
        assert!(text.contains("\"cmd\":\"cells_text\""), "{text}");
        assert_eq!(serde_json::from_str::<Payload>(&text).expect("decode"), cells);
        assert_eq!(cells.command(), "shape.setCellBodies");
    }

    #[test]
    fn a_run_range_format_carries_its_character_offsets() {
        // The range is a whole-body character range; run indices never travel because a text
        // edit splits runs differently on another replica.
        let ranged = Payload::FormatSet {
            format: FormatCmd::Bold,
            args: json!({"on": true}),
            addresses: vec![FormatAddress { range: Some(TextRange { from: 0, to: 5 }), ..FormatAddress::default() }],
        };
        assert_eq!(ranged.command(), "format.bold");
        let text = serde_json::to_string(&ranged).expect("encode");
        assert!(text.contains("\"range\":{\"from\":0,\"to\":5}"), "{text}");
        assert!(!text.contains("cell"), "a shape-level range has no cell address: {text}");
        assert_eq!(serde_json::from_str::<Payload>(&text).expect("decode"), ranged);

        let in_cell = Payload::FormatSet {
            format: FormatCmd::Italic,
            args: json!({"on": true}),
            addresses: vec![FormatAddress { cell: Some((1, 2)), range: Some(TextRange { from: 2, to: 4 }), ..FormatAddress::default() }],
        };
        let text = serde_json::to_string(&in_cell).expect("encode");
        assert!(text.contains("\"cell\":[1,2]") && text.contains("\"range\":{\"from\":2,\"to\":4}"), "{text}");
        assert_eq!(serde_json::from_str::<Payload>(&text).expect("decode"), in_cell);

        // A caret is its own address, not an empty range.
        let caret = Payload::FormatSet {
            format: FormatCmd::Bold,
            args: json!({"on": true}),
            addresses: vec![FormatAddress { caret: Some(3), ..FormatAddress::default() }],
        };
        let text = serde_json::to_string(&caret).expect("encode");
        assert!(text.contains("\"caret\":3") && !text.contains("range"), "{text}");
        assert_eq!(serde_json::from_str::<Payload>(&text).expect("decode"), caret);

        // A whole-body format has no `range` and no `caret` key at all.
        let plain = Payload::FormatSet { format: FormatCmd::Bold, args: json!({"on": true}), addresses: vec![FormatAddress::default()] };
        let text = serde_json::to_string(&plain).expect("encode");
        assert!(!text.contains("range") && !text.contains("caret"), "{text}");
    }

    #[test]
    fn a_multi_cell_format_carries_one_address_per_cell() {
        // One operation formats N cells of one table; every cell is its own address, so a
        // neighbouring cell that is not listed can never travel with it.
        let many = Payload::FormatSet {
            format: FormatCmd::Bold,
            args: json!({"on": true}),
            addresses: vec![
                FormatAddress { cell: Some((0, 0)), ..FormatAddress::default() },
                FormatAddress { cell: Some((0, 1)), range: Some(TextRange { from: 1, to: 3 }), ..FormatAddress::default() },
            ],
        };
        let text = serde_json::to_string(&many).expect("encode");
        assert!(text.contains("\"addresses\":[{\"cell\":[0,0]},{\"cell\":[0,1],\"range\":{\"from\":1,\"to\":3}}]"), "{text}");
        assert_eq!(serde_json::from_str::<Payload>(&text).expect("decode"), many);
        assert_eq!(many.command(), "format.bold");
    }

    #[test]
    fn shape_and_slide_restores_round_trip_and_name_their_command() {
        let fields = Payload::ShapeFields {
            items: vec![
                ShapeFieldItem { shape: ShapeKey("k-h1".into()), field: ShapeField::Fill, value: json!({"kind": "none"}) },
                ShapeFieldItem { shape: ShapeKey("k-h2".into()), field: ShapeField::Line, value: serde_json::Value::Null },
            ],
        };
        let text = serde_json::to_string(&fields).expect("encode");
        assert!(text.contains("\"cmd\":\"shape_fields\""), "{text}");
        assert_eq!(serde_json::from_str::<Payload>(&text).expect("decode"), fields);
        assert_eq!(fields.command(), "shape.setFields");

        let restore = Payload::ShapeRestore {
            items: vec![ShapeRestoreItem {
                shape: ShapeKey("k-h3".into()),
                parent: None,
                index: 1,
                value: json!({"id": 7, "name": "Gone", "kind": {"kind": "group", "children": []}}),
                animations: vec![json!({"shape": 7, "effect": "fade"})],
            }],
            remove: Vec::new(),
        };
        let text = serde_json::to_string(&restore).expect("encode");
        assert!(text.contains("\"cmd\":\"shape_restore\""), "{text}");
        assert_eq!(serde_json::from_str::<Payload>(&text).expect("decode"), restore);
        assert_eq!(restore.command(), "shape.restore");
        let single = Payload::ShapeRestore {
            items: vec![ShapeRestoreItem { shape: ShapeKey("k-h3".into()), parent: None, index: 0, value: json!({"id": 7}), animations: Vec::new() }],
            remove: vec![ShapeKey("k-h9".into())],
        };
        let text = serde_json::to_string(&single).expect("encode");
        assert!(!text.contains("animations"), "an empty list is omitted: {text}");
        assert!(!text.contains("parent"), "a top-level shape names no parent: {text}");
        assert!(text.contains("\"remove\":[\"k-h9\"]"), "the shapes one step takes out travel with it: {text}");
        assert_eq!(serde_json::from_str::<Payload>(&text).expect("decode"), single);

        let slide = Payload::SlideRestore { index: 2, slide: json!({"id": 7, "name": "Back"}), sections: vec![(0, 1)], shows: vec![] };
        let text = serde_json::to_string(&slide).expect("encode");
        assert!(text.contains("\"cmd\":\"slide_restore\""), "{text}");
        assert!(!text.contains("shows"), "an empty list is omitted: {text}");
        assert_eq!(serde_json::from_str::<Payload>(&text).expect("decode"), slide);
        assert_eq!(slide.command(), "slide.restore");
    }

    #[test]
    fn clearing_an_outline_carries_only_the_flag() {
        let line = LineSpec { none: true, color: Some("#000000".into()), width: Some(4.0), ..LineSpec::default() };
        assert_eq!(line.args(), json!({"none": true}));
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
