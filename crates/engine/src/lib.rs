//! The DeckCraft engine façade.
//!
//! Every user-visible action is a command with a stable id (`slide.new`, `shape.insert`,
//! `text.insert`, `format.bold`…) and JSON parameters. The egui UI, the CLI, the control channel
//! and MCP all go through [`Session::execute`]. Pointer tools reduce to commands too, so every
//! gesture is journaled, undoable and replayable.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod cmd;
mod comment_identity;
mod comment_lifecycle;
mod comment_ops;
mod comment_transfer;
pub mod compare;
pub mod connect;
pub mod drawtable;
pub mod file_access;
pub mod fonts;
pub mod gif_export;
pub mod guard;
pub mod icons;
pub mod inkshape;
pub mod io;
pub mod keys;
pub mod links;
pub mod media_refs;
mod notes_transfer;
pub mod recovery;
pub mod sample;
pub mod sounds;
pub mod spell;
mod table_calc;
mod template_transfer;
pub mod templates;
pub mod thesaurus;
pub mod tools;
mod transfer_policy;

use std::sync::Arc;

use deckcraft_model::{Presentation, Shape, ShapeId, Slide, SlideId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use cmd::{CommandInfo, CommandSpec, command_specs, find_command};
pub use deckcraft_geom as geom;
pub use deckcraft_media as media;
pub use deckcraft_model as model;
pub use deckcraft_pdf as pdf;
pub use deckcraft_render as render;
pub use tools::{Mods, PointerEvent, PointerKind, ToolKind};

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("unknown command `{0}`")]
    UnknownCommand(String),
    #[error("command `{0}` is not available right now: {1}")]
    Disabled(String, String),
    #[error("invalid parameters for `{cmd}`: {msg}")]
    BadParams { cmd: String, msg: String },
    #[error("no active presentation")]
    NoDocument,
    #[error("{0}")]
    Other(String),
    #[error("internal error in `{0}` (the presentation was kept as it was): {1}")]
    Internal(String, String),
}

impl From<deckcraft_model::ModelError> for EngineError {
    fn from(e: deckcraft_model::ModelError) -> Self {
        EngineError::Other(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, EngineError>;

/// Text being edited in place: a shape's body (or a table cell, or the slide's notes).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextSel {
    pub shape: ShapeId,
    /// Selection anchor and caret: (paragraph, char).
    pub anchor: (usize, usize),
    pub caret: (usize, usize),
    /// Editing a table cell (row, col).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell: Option<(usize, usize)>,
    /// Editing the slide's speaker notes (shape is ignored).
    #[serde(default)]
    pub notes: bool,
    /// The caret position that sits at the end of a wrapped line (after End) rather than at the
    /// start of the next line. Ignored once the caret moves anywhere else.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_end: Option<(usize, usize)>,
}

impl TextSel {
    pub fn is_range(&self) -> bool {
        self.anchor != self.caret
    }
    pub fn ordered(&self) -> ((usize, usize), (usize, usize)) {
        if self.anchor <= self.caret { (self.anchor, self.caret) } else { (self.caret, self.anchor) }
    }
    /// Is the caret drawn at the end of the line before (upstream), not the start of the next?
    pub fn at_line_end(&self) -> bool {
        self.line_end == Some(self.caret)
    }
}

/// Which shape tree is being edited: slides (normal), or a master/layout (Slide Master view).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Target {
    #[default]
    Slides,
    Master {
        master: usize,
    },
    Layout {
        master: usize,
        layout: usize,
    },
    /// The handout master (View ▸ Handout Master), a page at the notes size.
    Handout,
    /// The notes master (View ▸ Notes Master), a page at the notes size.
    NotesMaster,
    /// An imported notes master, retaining its native page size.
    ImportedNotesMaster {
        master: usize,
    },
}

impl Target {
    /// A notes-size master page (Handout or Notes Master view).
    pub fn is_page_master(self) -> bool {
        matches!(self, Target::Handout | Target::NotesMaster | Target::ImportedNotesMaster { .. })
    }
}

/// The handout or notes master edited in that view.
pub fn page_master(doc: &Presentation, t: Target) -> Option<&Arc<deckcraft_model::Master>> {
    match t {
        Target::Handout => doc.handout_master.as_ref(),
        Target::NotesMaster => doc.notes_master.as_ref(),
        Target::ImportedNotesMaster { master } => doc.notes_masters.get(master).map(|m| &m.master),
        _ => None,
    }
}

pub fn page_master_mut(doc: &mut Presentation, t: Target) -> Option<&mut Arc<deckcraft_model::Master>> {
    match t {
        Target::Handout => doc.handout_master.as_mut(),
        Target::NotesMaster => doc.notes_master.as_mut(),
        Target::ImportedNotesMaster { master } => doc.notes_masters.get_mut(master).map(|m| &mut m.master),
        _ => None,
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Selection {
    /// Current slide index.
    pub slide: usize,
    /// Slides selected in the thumbnail pane / sorter (includes the current one when non-empty).
    pub slides: Vec<SlideId>,
    /// Selected shapes on the current slide (top-level or inside groups).
    pub shapes: Vec<ShapeId>,
    pub text: Option<TextSel>,
    /// Selected table cells: (table, (r0, c0), (r1, c1)).
    pub cells: Option<(ShapeId, (usize, usize), (usize, usize))>,
    /// Nested table owner under the selected top-level shape.
    #[serde(default, skip_serializing_if = "deckcraft_model::table::TablePath::is_empty")]
    pub table_path: deckcraft_model::table::TablePath,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table_owner: Option<ShapeId>,
    /// Additional text block in the active cell; absent means its first text body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell_block: Option<u32>,
    pub target: Target,
}

#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub label: String,
    pub doc: Arc<Presentation>,
    pub selection: Selection,
}

#[derive(Clone, Debug, Default)]
pub struct History {
    pub undo: Vec<HistoryEntry>,
    pub redo: Vec<HistoryEntry>,
    pub limit: usize,
    /// Most memory (estimated bytes) the undo steps may keep beyond the current document;
    /// 0 means [`UNDO_BYTE_BUDGET`].
    pub byte_limit: usize,
    /// Estimated bytes each undo step keeps that the document after it doesn't share, by the
    /// step's document address.
    pub costs: std::collections::HashMap<usize, usize>,
}

/// A live pointer interaction: the document before it began (one undo step at the end).
#[derive(Clone, Debug)]
pub struct Interaction {
    pub label: String,
    pub doc: Arc<Presentation>,
    pub selection: Selection,
}

/// Text an input method is composing (#101): its preedit sits in the edited text body, at `start`
/// for `len` characters, until it is committed, replaced or cancelled. The presentation from
/// before it began makes the whole composition one undo step.
#[derive(Clone, Debug)]
pub struct Composition {
    /// Where the composed text starts: (paragraph, char).
    pub start: (usize, usize),
    /// Its length in characters (it never spans paragraphs).
    pub len: usize,
    /// The part the input method is converting, relative to `start` (drawn with a thicker line).
    pub active: Option<(usize, usize)>,
    /// The formatting the composed text is typed with (as `text.insert` would choose).
    pub props: Option<deckcraft_model::text::RunProps>,
    /// The presentation and selection before the composition began.
    pub before: Arc<Presentation>,
    pub before_selection: Selection,
}

#[derive(Clone, Debug)]
pub struct DocState {
    pub doc: Arc<Presentation>,
    pub selection: Selection,
    pub history: History,
    pub path: Option<String>,
    pub revision: u64,
    pub saved_doc: Arc<Presentation>,
    pub interaction: Option<Interaction>,
    pub uid: u64,
    pub title: String,
    /// Fonts the file's text uses that aren't installed, found when it was opened (empty for new
    /// documents); the app mentions them once.
    pub missing_fonts: Vec<String>,
    /// An input method's composition in progress ([`Composition`]).
    pub composition: Option<Composition>,
    /// Review › Compare in progress: the revised copy.
    pub compare: Option<compare::Compare>,
}

static NEXT_UID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A document id unique across processes, not just within this one: recovery entries are named
/// by it in a folder every running instance shares. Ids are this process's random base (see
/// [`recovery::instance_id`]) plus a counter.
fn next_uid() -> u64 {
    recovery::instance_id().wrapping_add(NEXT_UID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)) & recovery::UID_MASK
}

impl DocState {
    pub fn new(doc: Presentation, path: Option<String>, title: String) -> Self {
        let doc = Arc::new(doc);
        DocState {
            saved_doc: doc.clone(),
            doc,
            selection: Selection::default(),
            history: History { limit: 500, ..Default::default() },
            path,
            revision: 1,
            interaction: None,
            uid: next_uid(),
            title,
            missing_fonts: Vec::new(),
            composition: None,
            compare: None,
        }
    }
    pub fn is_dirty(&self) -> bool {
        !Arc::ptr_eq(&self.doc, &self.saved_doc)
    }
    pub fn title(&self) -> String {
        self.path
            .as_deref()
            .and_then(|p| std::path::Path::new(p).file_stem())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| self.title.clone())
    }
    pub fn current_slide(&self) -> Option<&Slide> {
        self.doc.slides.get(self.selection.slide).map(|s| s.as_ref())
    }
    /// Shapes of the edited tree (current slide, or the master/layout in Slide Master view).
    pub fn shapes(&self) -> &[Shape] {
        match self.selection.target {
            Target::Slides => self.current_slide().map(|s| s.shapes.as_slice()).unwrap_or(&[]),
            Target::Master { master } => self.doc.masters.get(master).map(|m| m.shapes.as_slice()).unwrap_or(&[]),
            Target::Handout | Target::NotesMaster | Target::ImportedNotesMaster { .. } => {
                page_master(&self.doc, self.selection.target).map(|m| m.shapes.as_slice()).unwrap_or(&[])
            }
            Target::Layout { master, layout } => {
                self.doc.masters.get(master).and_then(|m| m.layouts.get(layout)).map(|l| l.shapes.as_slice()).unwrap_or(&[])
            }
        }
    }
    pub fn shape(&self, id: ShapeId) -> Option<&Shape> {
        deckcraft_model::find_shape(self.shapes(), id)
    }
    pub fn selected_shapes(&self) -> Vec<&Shape> {
        self.selection.shapes.iter().filter_map(|id| self.shape(*id)).collect()
    }
}

/// The page being edited: the slide size, or the notes size in Handout Master view.
pub fn page_size(st: &DocState) -> deckcraft_geom::Size {
    page_master_size(&st.doc, st.selection.target).unwrap_or(st.doc.slide_size)
}

/// Native dimensions of the selected page master.
pub fn page_master_size(doc: &Presentation, target: Target) -> Option<deckcraft_geom::Size> {
    match target {
        Target::ImportedNotesMaster { master } => doc.notes_masters.get(master).map(|m| m.size),
        Target::Handout | Target::NotesMaster => Some(doc.notes_size),
        _ => None,
    }
}

/// The handout or notes master as a one-master presentation at the notes page size, for drawing
/// it with the layout renderer.
pub fn master_page(doc: &Presentation, t: Target) -> Option<Presentation> {
    let m = page_master(doc, t)?.clone();
    let mut p = doc.clone();
    p.slide_size = page_master_size(doc, t)?;
    p.masters = vec![m];
    p.slides.clear();
    Some(p)
}

/// Mutable access to the edited shape tree of a presentation.
pub fn shapes_mut<'a>(doc: &'a mut Presentation, sel: &Selection) -> Option<&'a mut Vec<Shape>> {
    match sel.target {
        Target::Slides => doc.slides.get_mut(sel.slide).map(|s| &mut Arc::make_mut(s).shapes),
        Target::Master { master } => doc.masters.get_mut(master).map(|m| &mut Arc::make_mut(m).shapes),
        t @ (Target::Handout | Target::NotesMaster | Target::ImportedNotesMaster { .. }) => {
            page_master_mut(doc, t).map(|m| &mut Arc::make_mut(m).shapes)
        }
        Target::Layout { master, layout } => {
            doc.masters.get_mut(master).and_then(|m| Arc::make_mut(m).layouts.get_mut(layout)).map(|l| &mut l.shapes)
        }
    }
}

/// Engine preferences (the UI keeps its own).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Prefs {
    pub snap_to_grid: bool,
    pub grid_spacing: f64,
    pub smart_guides: bool,
    /// Ctrl+D / Paste offset.
    pub duplicate_offset: f64,
    pub autocorrect: bool,
    pub smart_quotes: bool,
    /// AutoCorrect options (Tools ▸ AutoCorrect): fix TWo INitial CApitals, capitalize the first
    /// letter of sentences and the names of days, fix accidental cAPS lOCK, and replace 1/2, 1/4
    /// and 3/4 with fraction characters.
    pub ac_two_initials: bool,
    pub ac_sentence_caps: bool,
    pub ac_days: bool,
    pub ac_caps_lock: bool,
    pub ac_fractions: bool,
    /// Your own "replace … with …" entries; they win over the built-in list.
    pub ac_entries: Vec<(String, String)>,
    /// Built-in entries you deleted.
    pub ac_removed: Vec<String>,
    pub author: String,
    pub undo_limit: usize,
    /// Minutes between AutoRecover saves of unsaved presentations.
    pub recovery_minutes: f64,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            snap_to_grid: true,
            grid_spacing: 6.0,
            smart_guides: true,
            duplicate_offset: 12.0,
            autocorrect: true,
            smart_quotes: true,
            ac_two_initials: true,
            ac_sentence_caps: true,
            ac_days: true,
            ac_caps_lock: true,
            ac_fractions: true,
            ac_entries: Vec::new(),
            ac_removed: Vec::new(),
            author: "Presenter".into(),
            undo_limit: 500,
            recovery_minutes: 1.0,
        }
    }
}

impl Prefs {
    /// The preferences with every value in a range the editor can use: `prefs.json` is a file
    /// on disk, and a hand-edited or damaged one (a zero grid, a huge paste offset or undo
    /// limit) must not break editing or exhaust memory. Out-of-range numbers take their
    /// defaults or the nearest usable value.
    pub fn sanitized(mut self) -> Prefs {
        let d = Prefs::default();
        let within = |v: f64, lo: f64, hi: f64, default: f64| if v.is_finite() { v.clamp(lo, hi) } else { default };
        self.grid_spacing =
            if self.grid_spacing.is_finite() && self.grid_spacing > 0.0 { self.grid_spacing.clamp(0.5, 1000.0) } else { d.grid_spacing };
        self.duplicate_offset = within(self.duplicate_offset, -1000.0, 1000.0, d.duplicate_offset);
        self.undo_limit = self.undo_limit.min(MAX_UNDO_LIMIT);
        self.recovery_minutes = within(self.recovery_minutes, 0.1, 24.0 * 60.0, d.recovery_minutes);
        if self.author.chars().count() > 256 {
            self.author = self.author.chars().take(256).collect();
        }
        self
    }
}

/// The most undo steps [`Prefs::undo_limit`] can keep.
pub const MAX_UNDO_LIMIT: usize = 10_000;

/// Things a command or tool asks the UI to do (open a dialog, show a pane, start the show…).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum UiRequest {
    Dialog {
        id: String,
        params: Value,
    },
    Pane {
        id: String,
    },
    StartShow {
        from: usize,
    },
    Message {
        text: String,
    },
    PickFile {
        purpose: String,
    },
    /// Media playback for the UI host (`media.play` / `pause` / `stop` / `seek`): `action` is
    /// `play`, `pause`, `toggle`, `stop` or `seek`; `ms` the media time to start from / seek to.
    Media {
        action: String,
        shape: ShapeId,
        ms: Option<u64>,
    },
}

/// Playback state of a media shape as the UI host reports it (see [`Session::media_status`]).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaStatus {
    /// `loading`, `playing`, `paused` or `ended`.
    pub state: String,
    pub position_ms: u64,
    pub duration_ms: u64,
    /// Why the media is silent / blank, when it can't be decoded.
    pub error: Option<String>,
}

/// Clipboard contents copied within the app.
#[derive(Clone, Debug, Default)]
pub struct Clipboard {
    pub shapes: Vec<Shape>,
    pub slides: Vec<Slide>,
    /// Exact selected source owners, retained even if the originating document is closed.
    pub(crate) templates: template_transfer::TemplateClosure,
    pub(crate) notes: Option<notes_transfer::NotesClosure>,
    pub(crate) origin: transfer_policy::ClipboardOrigin,
    /// Media the copied shapes/slides reference.
    pub media: Vec<deckcraft_model::MediaItem>,
    pub text: Option<deckcraft_model::TextBody>,
    pub plain: String,
    /// The copied shapes' theme colours (colour map applied) and fonts, for Paste Special ›
    /// Keep Source Formatting.
    pub source_theme: Option<(deckcraft_color::ColorScheme, deckcraft_model::theme::FontScheme)>,
    /// How many times the shapes were pasted (each paste offsets further).
    pub pastes: u32,
}

pub struct Session {
    docs: Vec<DocState>,
    active: Option<usize>,
    pub prefs: Prefs,
    pub journal: Vec<(String, Value)>,
    pub clipboard: Clipboard,
    pub ui_requests: Vec<UiRequest>,
    pub tool: tools::ToolState,
    pub(crate) untitled: u32,
    /// Format Painter: copied shape/text formatting and whether it stays on (double-click).
    pub painter: Option<(cmd::format::Painted, bool)>,
    /// Animation Painter: copied shape animations and whether it stays on (double-click).
    pub anim_painter: Option<(Vec<deckcraft_model::anim::Animation>, bool)>,
    /// Set as Default Shape: the look new shapes get.
    pub default_look: Option<cmd::format::Painted>,
    /// Nesting of commands run by commands: only the outermost one is an undo step and a journal
    /// entry.
    depth: u32,
    formula_budget: deckcraft_model::table::FormulaBudget,
    /// Crash-recovery folder (the app sets it; saving or closing a presentation removes its entry).
    pub recovery_dir: Option<std::path::PathBuf>,
    /// Media playback state, written by the UI host every frame (empty without a host).
    pub media_status: std::collections::HashMap<ShapeId, MediaStatus>,
    /// Animated GIF pictures paused with `media.gifPlay` (the others play).
    pub gif_paused: std::collections::HashSet<ShapeId>,
    /// Text keys follow the Mac (Option+arrows move by word, Cmd+arrows to the line or text
    /// ends) instead of Windows and Linux (Ctrl+arrows by word or paragraph). Defaults to the
    /// platform DeckCraft was built for.
    pub mac_keys: bool,
    /// Command panics caught by the guard (the document was rolled back); soak tests require 0.
    pub panics_caught: u64,
    /// Presentations a host read off the UI thread, waiting for `file.adopt` (by token).
    loaded: Vec<(u64, cmd::file::LoadedFile)>,
    next_loaded: u64,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    pub fn new() -> Self {
        guard::install_panic_hook();
        Session {
            docs: vec![],
            active: None,
            prefs: Prefs::default(),
            journal: vec![],
            clipboard: Clipboard::default(),
            ui_requests: vec![],
            tool: tools::ToolState::default(),
            untitled: 0,
            default_look: None,
            painter: None,
            anim_painter: None,
            depth: 0,
            formula_budget: Default::default(),
            recovery_dir: None,
            media_status: Default::default(),
            gif_paused: Default::default(),
            mac_keys: cfg!(target_os = "macos"),
            panics_caught: 0,
            loaded: vec![],
            next_loaded: 0,
        }
    }
    /// Hand over a presentation read on another thread ([`cmd::file::read_presentation_file`]);
    /// `file.adopt` with the returned token adds it as a document, through the command path like
    /// `file.open` (journaled, panic-guarded). Only the latest few wait; older ones are dropped.
    pub fn stage_loaded(&mut self, f: cmd::file::LoadedFile) -> u64 {
        const KEEP: usize = 4;
        self.next_loaded = self.next_loaded.wrapping_add(1);
        if self.loaded.len() >= KEEP {
            self.loaded.remove(0);
        }
        self.loaded.push((self.next_loaded, f));
        self.next_loaded
    }
    /// The staged presentation for `token`, removed from the session.
    pub(crate) fn take_loaded(&mut self, token: u64) -> Option<cmd::file::LoadedFile> {
        let i = self.loaded.iter().position(|(t, _)| *t == token)?;
        Some(self.loaded.remove(i).1)
    }
    /// A session with one new presentation open.
    pub fn with_new() -> Self {
        let mut s = Self::new();
        let _ = s.execute("file.new", &Value::Null);
        s
    }
    pub fn documents(&self) -> &[DocState] {
        &self.docs
    }
    pub fn active_index(&self) -> Option<usize> {
        self.active
    }
    pub fn active(&self) -> Option<&DocState> {
        self.active.and_then(|i| self.docs.get(i))
    }
    pub fn active_mut(&mut self) -> Option<&mut DocState> {
        self.active.and_then(|i| self.docs.get_mut(i))
    }
    pub fn doc(&self) -> Result<&DocState> {
        self.active().ok_or(EngineError::NoDocument)
    }
    pub fn doc_mut(&mut self) -> Result<&mut DocState> {
        self.active_mut().ok_or(EngineError::NoDocument)
    }
    pub fn set_active(&mut self, i: usize) {
        if i < self.docs.len() {
            self.active = Some(i);
        }
    }
    pub fn add_document(&mut self, d: DocState) -> usize {
        self.docs.push(d);
        self.active = Some(self.docs.len() - 1);
        self.docs.len() - 1
    }
    pub fn close_document(&mut self, i: usize) {
        if i < self.docs.len() {
            let d = self.docs.remove(i);
            if let Some(dir) = &self.recovery_dir {
                recovery::discard(dir, d.uid);
            }
            self.active = if self.docs.is_empty() { None } else { Some(i.min(self.docs.len() - 1)) };
        }
    }
    pub fn next_untitled(&mut self) -> String {
        self.untitled += 1;
        format!("Presentation{}", self.untitled)
    }

    /// Run command `id`. A panic inside it becomes [`EngineError::Internal`] and leaves the
    /// presentation as it was.
    pub fn execute(&mut self, id: &str, params: &Value) -> Result<Value> {
        self.guarded(id, |s| s.execute_unguarded(id, params))
    }

    fn execute_unguarded(&mut self, id: &str, params: &Value) -> Result<Value> {
        if self.depth == 0 {
            self.formula_budget = Default::default();
        }
        let spec = find_command(id).ok_or_else(|| EngineError::UnknownCommand(id.into()))?;
        // Any other edit (or a click, a key, undo) settles an input method's composition first.
        if self.depth == 0 && (spec.journal || spec.undoable) && !matches!(id, "text.compose" | "text.commit") {
            self.end_composition();
        }
        if let Err(e) = (spec.enabled)(self) {
            let named = (e == cmd::NOTHING_SELECTED || e == cmd::SELECT_TWO) && (params.get("ids").is_some() || params.get("id").is_some());
            if !named {
                return Err(EngineError::Disabled(id.into(), e));
            }
        }
        let before = self.active().map(|d| (d.uid, d.doc.clone(), d.selection.clone()));
        let outer = self.depth == 0;
        self.depth += 1;
        let r = (spec.run)(self, params);
        self.depth -= 1;
        let r = match r {
            Ok(r) => r,
            Err(e) => {
                // A command made of several edits can fail after some of them went through; the
                // outermost one puts the presentation back, so a failed command changes nothing.
                if outer
                    && let (Some((uid, old, sel)), Some(st)) = (before, self.active_mut())
                    && st.uid == uid
                    && !Arc::ptr_eq(&old, &st.doc)
                {
                    st.doc = old;
                    st.selection = sel;
                    st.revision += 1;
                }
                return Err(e);
            }
        };
        if let Some(st) = self.active_mut() {
            clamp_selection(st);
        }
        if let (Some((uid, old, sel)), Some(st)) = (before, self.active_mut())
            && st.uid == uid
            && !Arc::ptr_eq(&old, &st.doc)
            && st.interaction.is_none()
            && spec.undoable
            && outer
        {
            push_undo(st, HistoryEntry { label: spec.label.to_string(), doc: old, selection: sel });
        }
        if spec.journal && outer {
            self.journal.push((id.to_string(), params.clone()));
            if self.journal.len() > 10_000 {
                self.journal.drain(..1000);
            }
        }
        Ok(r)
    }

    pub fn commands(&self) -> Vec<CommandInfo> {
        command_specs().iter().map(|c| c.info(self)).collect()
    }

    /// Mutate the active presentation (copy-on-write) and bump the revision.
    pub fn edit<T>(&mut self, f: impl FnOnce(&mut Presentation, &mut Selection) -> Result<T>) -> Result<T> {
        self.edit_transaction(true, true, f)
    }

    /// Edit document resources without changing connector geometry or detaching slide owners.
    pub(crate) fn edit_resources<T>(&mut self, f: impl FnOnce(&mut Presentation, &mut Selection) -> Result<T>) -> Result<T> {
        self.edit_transaction(false, true, f)
    }

    /// IME preedit is provisional, including its first event before Composition exists.
    pub(crate) fn edit_preedit<T>(&mut self, f: impl FnOnce(&mut Presentation, &mut Selection) -> Result<T>) -> Result<T> {
        self.edit_transaction(true, false, f)
    }

    pub(crate) fn edit_calculated<T>(
        &mut self,
        f: impl FnOnce(&mut Presentation, &mut Selection, &mut deckcraft_model::table::FormulaBudget) -> Result<T>,
    ) -> Result<T> {
        self.edit_calculated_transaction(true, true, f)
    }

    fn edit_transaction<T>(&mut self, reroute: bool, settle: bool, f: impl FnOnce(&mut Presentation, &mut Selection) -> Result<T>) -> Result<T> {
        self.edit_calculated_transaction(reroute, settle, |doc, sel, _| f(doc, sel))
    }

    fn edit_calculated_transaction<T>(
        &mut self,
        reroute: bool,
        settle: bool,
        f: impl FnOnce(&mut Presentation, &mut Selection, &mut deckcraft_model::table::FormulaBudget) -> Result<T>,
    ) -> Result<T> {
        let mut budget = if self.depth == 0 { Default::default() } else { std::mem::take(&mut self.formula_budget) };
        let result = (|| {
            let st = self.doc_mut()?;
            deckcraft_model::comments::validate(&st.doc, true).map_err(|error| EngineError::Other(error.to_string()))?;
            let mut doc = (*st.doc).clone();
            if doc.slides.iter().any(|slide| !slide.comments.is_empty()) {
                deckcraft_model::comments::normalize(&mut doc).map_err(|error| EngineError::Other(error.to_string()))?;
            }
            let comment_before = comment_lifecycle::Snapshot::capture(&doc)?;
            let mut sel = st.selection.clone();
            let imported = match sel.target {
                Target::ImportedNotesMaster { master } => st.doc.notes_masters.get(master).map(|m| m.master.id),
                _ => None,
            };
            let r = f(&mut doc, &mut sel, &mut budget)?;
            comment_before.reconcile(&mut doc)?;
            if settle && st.composition.is_none() {
                table_calc::reconcile_document_with_budget(&mut doc, &mut budget)?;
            }
            budget.check_limits()?;
            // A collection edit must not redirect an existing master selection to its neighbour.
            if sel.target == st.selection.target && matches!(sel.target, Target::ImportedNotesMaster { .. }) {
                sel.target = imported
                    .and_then(|id| doc.notes_masters.iter().position(|m| m.master.id == id))
                    .map_or(Target::Slides, |master| Target::ImportedNotesMaster { master });
                if sel.target == Target::Slides {
                    sel.shapes.clear();
                    sel.text = None;
                    sel.cells = None;
                }
            }
            if reroute {
                connect::reroute(&mut doc, &sel);
            }
            st.doc = Arc::new(doc);
            st.selection = sel;
            st.revision += 1;
            Ok(r)
        })();
        self.formula_budget = budget;
        result
    }

    /// Keep an input method's composition as it stands (#101): its text stays and the whole
    /// composition becomes one undo step. Nothing happens without one.
    pub fn end_composition(&mut self) {
        let mut budget = if self.depth == 0 { Default::default() } else { std::mem::take(&mut self.formula_budget) };
        self.settle_composition(&mut budget);
        self.formula_budget = budget;
    }

    fn settle_composition(&mut self, budget: &mut deckcraft_model::table::FormulaBudget) {
        let Some(st) = self.active_mut() else { return };
        let Some(c) = st.composition.take() else { return };
        let mut doc = (*st.doc).clone();
        let settlement = (|| -> Result<()> {
            if c.before_selection.text.as_ref().is_some_and(|text| !text.notes && text.cell.is_some()) {
                cmd::text::reconcile_composed_cell(&mut doc, &c.before_selection)?;
            }
            table_calc::reconcile_document_with_budget(&mut doc, budget)?;
            budget.check_limits()?;
            Ok(())
        })();
        if let Err(error) = settlement {
            log::warn!("cannot settle composed table calculation; composition restored: {error}");
            st.doc = c.before;
            st.selection = c.before_selection;
            st.revision += 1;
            return;
        }
        // Avoid replacing an unchanged document merely because the staged settlement
        // inspected it. This also keeps cancelled/equal composition history unchanged.
        if *st.doc != doc {
            st.doc = Arc::new(doc);
        }
        if !Arc::ptr_eq(&c.before, &st.doc) {
            push_undo(st, HistoryEntry { label: "Type Text".into(), doc: c.before, selection: c.before_selection });
        }
    }

    /// Change only the selection (no undo step).
    pub fn select(&mut self, f: impl FnOnce(&Presentation, &mut Selection)) -> Result<()> {
        let st = self.doc_mut()?;
        let doc = st.doc.clone();
        // A caret placed anywhere starts downstream; only End puts it back at a line's end.
        if let Some(t) = st.selection.text.as_mut() {
            t.line_end = None;
        }
        let owner = |sel: &Selection| {
            sel.text.as_ref().map(|t| (sel.slide, sel.target, t.shape, t.cell, t.notes, sel.table_owner, sel.table_path.clone(), sel.cell_block))
        };
        let before = owner(&st.selection);
        f(&doc, &mut st.selection);
        clamp_selection(st);
        st.revision += 1;
        // A composition belongs to the text body it began in: leaving that body settles it.
        if st.composition.is_some() && owner(&st.selection) != before {
            self.end_composition();
        }
        Ok(())
    }
}

/// Keep the selection pointing at things that exist.
fn clamp_selection(st: &mut DocState) {
    let n = st.doc.slides.len();
    if n == 0 {
        st.selection.slide = 0;
    } else if st.selection.slide >= n {
        st.selection.slide = n - 1;
    }
    let ids: Vec<ShapeId> = st.selection.shapes.iter().copied().filter(|id| st.shape(*id).is_some()).collect();
    st.selection.shapes = ids;
    if let Some(t) = &st.selection.text
        && !t.notes
        && st.shape(t.shape).is_none()
    {
        st.selection.text = None;
    }
    let slides: Vec<SlideId> = st.selection.slides.iter().copied().filter(|id| st.doc.slide(*id).is_some()).collect();
    st.selection.slides = slides;
    let before = st.selection.target;
    match st.selection.target {
        Target::Master { master } if master >= st.doc.masters.len() => st.selection.target = Target::Slides,
        t @ (Target::Handout | Target::NotesMaster | Target::ImportedNotesMaster { .. }) if page_master(&st.doc, t).is_none() => {
            st.selection.target = Target::Slides
        }
        Target::Layout { master, layout } if st.doc.masters.get(master).is_none_or(|m| layout >= m.layouts.len()) => {
            st.selection.target = Target::Slides
        }
        _ => {}
    }
    if before != st.selection.target {
        st.selection.shapes.clear();
        st.selection.text = None;
        st.selection.cells = None;
    }
    let valid_table_owner = st.selection.table_owner.is_some_and(|id| {
        st.selection.shapes.contains(&id)
            && st.shape(id).is_some_and(|shape| match &shape.kind {
                deckcraft_model::ShapeKind::Table(table) => table.at_path(&st.selection.table_path).is_ok_and(|leaf| {
                    st.selection.text.as_ref().is_none_or(|text| {
                        text.shape == id
                            && !text.notes
                            && text.cell.is_some_and(|(r, c)| leaf.cell(r, c).and_then(|cell| cell.text_block(st.selection.cell_block)).is_some())
                    })
                }),
                _ => false,
            })
    });
    if !valid_table_owner {
        if !st.selection.table_path.is_empty() || st.selection.cell_block.is_some() {
            st.selection.text = None;
            st.selection.cells = None;
        }
        st.selection.table_path = Default::default();
        st.selection.table_owner = None;
        st.selection.cell_block = None;
    }
}

/// Default for [`History::byte_limit`].
pub const UNDO_BYTE_BUDGET: usize = 256 << 20;

fn text_cost(body: &deckcraft_model::text::TextBody) -> usize {
    body.paragraphs.iter().fold(0usize, |n, p| p.runs.iter().fold(n.saturating_add(96), |n, r| n.saturating_add(128).saturating_add(r.text.len())))
}

/// A rough size of a shape in memory: the struct, its text and kept XML, its children.
fn shape_cost(sh: &deckcraft_model::Shape, depth: usize) -> usize {
    let kids = if depth < 64 { sh.children().iter().fold(0usize, |n, c| n.saturating_add(shape_cost(c, depth + 1))) } else { 0 };
    let table = if let deckcraft_model::ShapeKind::Table(table) = &sh.kind {
        table.rows.iter().fold(table.cols.len().saturating_mul(std::mem::size_of::<f64>()), |total, row| {
            row.cells.iter().fold(total.saturating_add(std::mem::size_of::<deckcraft_model::table::Row>()), |total, cell| {
                total
                    .saturating_add(std::mem::size_of::<deckcraft_model::Cell>())
                    .saturating_add(text_cost(&cell.text))
                    .saturating_add(cell.data.as_ref().map_or(0, |data| data.metadata_bytes()))
            })
        })
    } else {
        0
    };
    768usize
        .saturating_add(sh.text.as_ref().map_or(0, text_cost))
        .saturating_add(sh.raw_ext.as_ref().map_or(0, String::len))
        .saturating_add(kids)
        .saturating_add(table)
}

fn shapes_cost(shapes: &[deckcraft_model::Shape]) -> usize {
    shapes.iter().fold(0usize, |n, s| n.saturating_add(shape_cost(s, 0)))
}

fn notes_fields_cost(fields: &deckcraft_model::notes::NotesFields) -> usize {
    use deckcraft_model::notes::NotesDateTime;
    let date = match &fields.date_time {
        Some(NotesDateTime::Fixed { text }) => text.len(),
        Some(NotesDateTime::Current { format, lang }) => format.len().saturating_add(lang.as_ref().map_or(0, String::len)),
        None => 0,
    };
    64usize
        .saturating_add(fields.header.as_ref().map_or(0, String::len))
        .saturating_add(fields.footer.as_ref().map_or(0, String::len))
        .saturating_add(date)
}

fn document_masters(doc: &Presentation) -> impl Iterator<Item = &Arc<deckcraft_model::Master>> {
    doc.masters.iter().chain(doc.handout_master.iter()).chain(doc.notes_master.iter()).chain(doc.notes_masters.iter().map(|m| &m.master))
}

/// Estimated bytes `doc` keeps alive that `newer` doesn't share. This intentionally stays a
/// coarse estimate; notes-page artwork and canonical text obey the same budget as slides.
pub fn undo_cost(doc: &Presentation, newer: &Presentation) -> usize {
    use std::collections::HashSet;
    let slides: HashSet<*const deckcraft_model::Slide> = newer.slides.iter().map(Arc::as_ptr).collect();
    let masters: HashSet<*const deckcraft_model::Master> = document_masters(newer).map(Arc::as_ptr).collect();
    let media: HashSet<*const Vec<u8>> = newer.media.iter().map(|m| Arc::as_ptr(&m.data)).collect();
    let templates: HashSet<_> = newer.presentation_templates.iter().map(Arc::as_ptr).collect();
    let mut n = 64usize.saturating_mul(doc.slides.len());
    n = n.saturating_add(doc.presentation_templates.capacity().saturating_mul(std::mem::size_of::<Arc<deckcraft_model::PresentationTemplate>>()));
    for template in doc.presentation_templates.iter().filter(|template| !templates.contains(&Arc::as_ptr(template))) {
        n = n
            .saturating_add(std::mem::size_of::<deckcraft_model::PresentationTemplate>())
            .saturating_add(2 * std::mem::size_of::<usize>())
            .saturating_add(template.name.capacity())
            .saturating_add(template.display_name.as_ref().map_or(0, String::capacity))
            .saturating_add(template.slots.capacity().saturating_mul(std::mem::size_of::<deckcraft_model::PresentationTemplateSlot>()));
    }
    for s in doc.slides.iter().filter(|s| !slides.contains(&Arc::as_ptr(s))) {
        n = n.saturating_add(512).saturating_add(shapes_cost(&s.shapes)).saturating_add(text_cost(&s.notes));
        if let Some(page) = &s.notes_page {
            n = n
                .saturating_add(256)
                .saturating_add(shapes_cost(&page.shapes))
                .saturating_add(notes_fields_cost(&page.fields))
                .saturating_add(if page.background.is_some() { 256 } else { 0 });
        }
    }
    let mut charged = HashSet::new();
    for m in document_masters(doc).filter(|m| !masters.contains(&Arc::as_ptr(m))) {
        if !charged.insert(Arc::as_ptr(m)) {
            continue;
        }
        n = n.saturating_add(4096).saturating_add(shapes_cost(&m.shapes));
        for l in &m.layouts {
            n = n.saturating_add(512).saturating_add(shapes_cost(&l.shapes));
        }
    }
    // Wrapper fields are owned values, copied even when the inner Master Arc is shared.
    for m in &doc.notes_masters {
        n = n.saturating_add(64).saturating_add(notes_fields_cost(&m.fields));
    }
    for m in doc.media.iter().filter(|m| !media.contains(&Arc::as_ptr(&m.data))) {
        n = n.saturating_add(m.data.len());
    }
    n
}

pub(crate) fn push_undo(st: &mut DocState, e: HistoryEntry) {
    let h = &mut st.history;
    h.costs.insert(Arc::as_ptr(&e.doc) as usize, undo_cost(&e.doc, &st.doc));
    h.undo.push(e);
    h.redo.clear();
    if h.undo.len() > h.limit.max(1) {
        h.undo.remove(0);
    }
    // Memory too: drop the oldest steps while the kept ones pass the budget (the newest always
    // stays, however big). A deck-wide edit keeps a copy of every slide, so 500 of them on a big
    // deck could hold hundreds of copies.
    let budget = if h.byte_limit == 0 { UNDO_BYTE_BUDGET } else { h.byte_limit };
    let cost = |h: &History, e: &HistoryEntry| h.costs.get(&(Arc::as_ptr(&e.doc) as usize)).copied().unwrap_or(0);
    let mut total: usize = h.undo.iter().fold(0usize, |n, e| n.saturating_add(cost(h, e)));
    let mut drop = 0;
    while total > budget && drop + 1 < h.undo.len() {
        total = total.saturating_sub(cost(h, &h.undo[drop]));
        drop += 1;
    }
    if drop > 0 {
        h.undo.drain(..drop);
    }
    // Costs of steps that are gone (dropped, or redo steps cleared above).
    if h.costs.len() > h.undo.len() + 16 {
        let live: std::collections::HashSet<usize> = h.undo.iter().chain(h.redo.iter()).map(|e| Arc::as_ptr(&e.doc) as usize).collect();
        h.costs.retain(|k, _| live.contains(k));
    }
}

#[cfg(test)]
mod soak_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_keys;

#[cfg(test)]
mod notes_layout_tests;
#[cfg(test)]
mod script_styles_tests;

#[cfg(test)]
mod notes_duplicate_tests;
#[cfg(test)]
mod notes_transfer_independent_tests;
#[cfg(test)]
mod notes_transfer_review_boundary_tests;
#[cfg(test)]
mod notes_transfer_tests;
#[cfg(test)]
mod presentation_template_boundary_tests;
#[cfg(test)]
mod presentation_template_compare_tests;
#[cfg(test)]
mod presentation_template_review_tests;
#[cfg(test)]
mod presentation_template_tests;
#[cfg(test)]
mod presentation_template_transfer_review_tests;
#[cfg(test)]
mod table_content_tests;
#[cfg(test)]
mod table_formula_edit_tests;
#[cfg(test)]
mod table_value_edit_tests;
#[cfg(test)]
mod table_value_review_tests;
#[cfg(test)]
mod template_transfer_tests;
#[cfg(test)]
mod transfer_policy_review_tests;
#[cfg(test)]
mod transfer_policy_tests;
