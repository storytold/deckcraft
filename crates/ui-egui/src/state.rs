use super::{Deserialize, LayoutId, MasterId, Serialize, SlideId, Target, qat, theme};

/// The operation the user asked for. Kept with the request so a late completion does not
/// guess a command from the file name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FilePurpose {
    Open,
    Picture,
    Audio,
    Video,
    /// Change Picture: the picture replaces the selected one (`picture.change`).
    ChangePicture,
    /// A drop, or any other purpose: decide from the name and bytes.
    Infer,
}

/// Where in a presentation a file should land. Ids, not indices: slides can move while
/// the browser is still reading.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FilePlace {
    Slide(SlideId),
    Master(MasterId),
    Layout {
        master: MasterId,
        layout: LayoutId,
    },
    /// The handout or notes master (one per presentation, so no id is needed).
    NotesPage(Target),
    /// The document was open but had no current slide, master or layout.
    Unspecified,
}

pub(crate) struct LocatedPlace {
    pub(crate) target: Target,
    pub(crate) slide: Option<usize>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ViewMode {
    #[default]
    Normal,
    Outline,
    Sorter,
    NotesPage,
    Reading,
}

/// Persisted UI state.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UiState {
    pub brightness: theme::Brightness,
    pub tab: String,
    pub ribbon_collapsed: bool,
    pub view: ViewMode,
    pub notes: bool,
    pub notes_height: f32,
    pub thumbs_width: f32,
    /// Vertical scroll offset of the thumbnail pane, recorded each frame for the control
    /// channel and tests; not persisted.
    #[serde(skip)]
    pub thumbs_scroll: f32,
    /// Open task pane: `format`, `animation`, `selection`, `comments`, `designer`, `background`,
    /// `smartart` (Text Pane).
    pub pane: Option<String>,
    pub pane_width: f32,
    pub ruler: bool,
    pub gridlines: bool,
    /// Slides per page previewed in Handout Master view (1, 2, 3, 4, 6 or 9).
    pub handout_per_page: u8,
    pub guides: bool,
    pub smart_guides: bool,
    /// None = fit to window.
    pub zoom: Option<f32>,
    pub sorter_zoom: f32,
    pub recent: Vec<String>,
    pub presenter_view: bool,
    /// Set Up Slide Show ▸ Keep mouse cursor visible: full-screen shows never hide a still pointer,
    /// for decks with clickable choices (#1445).
    pub show_keep_cursor: bool,
    pub ui_scale: f32,
    /// Format Shape pane: tab (`fill`, `effects`, `size`, `picture`, `text`).
    pub format_tab: String,
    /// The Text Pane's current SmartArt line (Add Shape, Promote and Demote act on it).
    pub smartart_row: usize,
    /// A Text Pane line to focus once it is drawn (after Enter, Tab or Backspace rebuilt the list).
    pub smartart_focus: Option<usize>,
    /// The Text Pane's line drafts, for the SmartArt group they were read from.
    pub smartart_drafts: (u32, Vec<String>),
    pub grayscale: bool,
    /// Comment the Review Previous/Next buttons last landed on: `(slide, index)`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment_focus: Option<(usize, usize)>,
    /// Quick Access Toolbar buttons, left to right.
    pub qat: Vec<qat::QatItem>,
}

impl Default for UiState {
    fn default() -> Self {
        UiState {
            brightness: theme::Brightness::Light,
            tab: "Home".into(),
            ribbon_collapsed: false,
            view: ViewMode::Normal,
            notes: true,
            notes_height: 52.0,
            thumbs_width: 236.0,
            thumbs_scroll: 0.0,
            pane: None,
            pane_width: 300.0,
            ruler: false,
            gridlines: false,
            handout_per_page: 6,
            guides: false,
            smart_guides: true,
            zoom: None,
            sorter_zoom: 1.0,
            recent: vec![],
            presenter_view: true,
            show_keep_cursor: false,
            ui_scale: 1.0,
            format_tab: "fill".into(),
            smartart_row: 0,
            smartart_focus: None,
            smartart_drafts: (0, vec![]),
            grayscale: false,
            comment_focus: None,
            qat: qat::defaults(),
        }
    }
}

/// The editor's zoom range (1 = 100 %).
pub const ZOOM_RANGE: std::ops::RangeInclusive<f32> = 0.1..=4.0;

impl UiState {
    /// The state with every value usable: `ui.json` is a file on disk, and a damaged or
    /// hand-edited one must not stop the window from opening. A zoom of 10,000 % asked the
    /// renderer for a texture larger than memory at start-up; panel sizes, scales and names are
    /// brought into range the same way, and the recent list is cut to its usual length.
    pub fn sanitized(mut self) -> UiState {
        let d = UiState::default();
        let within = |v: f32, lo: f32, hi: f32, default: f32| if v.is_finite() { v.clamp(lo, hi) } else { default };
        self.zoom = self.zoom.filter(|z| z.is_finite() && *z > 0.0).map(|z| z.clamp(*ZOOM_RANGE.start(), *ZOOM_RANGE.end()));
        self.sorter_zoom = within(self.sorter_zoom, 0.5, 3.0, d.sorter_zoom);
        self.ui_scale = within(self.ui_scale, 0.5, 3.0, d.ui_scale);
        self.notes_height = within(self.notes_height, 0.0, 2000.0, d.notes_height);
        self.thumbs_width = within(self.thumbs_width, 60.0, 1000.0, d.thumbs_width);
        self.pane_width = within(self.pane_width, 240.0, 520.0, d.pane_width);
        let short = |s: &String| s.len() <= 64;
        if !short(&self.tab) {
            self.tab = d.tab;
        }
        if !short(&self.format_tab) {
            self.format_tab = d.format_tab;
        }
        self.pane = self.pane.filter(short);
        self.recent.retain(|p| !p.is_empty() && p.len() <= 4096 && !p.contains('\0'));
        self.recent.dedup();
        self.recent.truncate(RECENT_MAX);
        self
    }
}

/// How many recent files File › Open and Home list.
pub const RECENT_MAX: usize = 12;

/// The live target/selection stamp used to detect a new application interruption without
/// discarding the retired composition's origin. Owner is optional because an interruption can
/// remove the canvas target entirely.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ImeTerminalStamp {
    pub(crate) owner: Option<egui::Id>,
    pub(crate) revision: u64,
    pub(crate) anchor: Option<(usize, usize)>,
    pub(crate) caret: Option<(usize, usize)>,
}

/// The live canvas target and caret/selection stamp a reset fence is bound to.
///
/// The retired composition's origin is deliberately absent: an app-originated interruption can
/// move the destination, and the replacement request belongs to the current destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ImeDestination {
    pub(crate) owner: egui::Id,
    pub(crate) revision: u64,
    pub(crate) anchor: Option<(usize, usize)>,
    pub(crate) caret: Option<(usize, usize)>,
}

/// The actual custom descriptor identity a reset request is allowed to acknowledge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ImeDescriptorKey {
    owner: egui::Id,
    context_generation: u64,
    purpose: egui::IMEPurpose,
}

impl ImeDescriptorKey {
    /// Construct the exact custom-IME descriptor identity from owner, context generation, and
    /// purpose.
    pub(crate) fn new(owner: egui::Id, context_generation: u64, purpose: egui::IMEPurpose) -> Self {
        Self { owner, context_generation, purpose }
    }
}
