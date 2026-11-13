//! Ribbon and UI icons, drawn in code.
//!
//! Every icon here is an original drawing made from egui painter primitives on a 24×24 design
//! grid (contributor-original, `MIT OR Apache-2.0`). Nothing is traced from or modelled on any
//! vendor's artwork (AGENTS.md §1).
//!
//! Style: line icons in the theme's ink colour with one or two fixed accent colours. When an icon
//! is disabled everything is drawn in ink at ~35% alpha.

use std::f32::consts::PI;

use egui::epaint::{Mesh, PathShape, TextShape};
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, pos2, vec2};

/// Accent blue.
pub const BLUE: Color32 = Color32::from_rgb(0x2E, 0x6F, 0xD8);
/// Accent orange.
pub const ORANGE: Color32 = Color32::from_rgb(0xF2, 0x6B, 0x1D);
/// Accent green.
pub const GREEN: Color32 = Color32::from_rgb(0x2E, 0xA3, 0x6F);
/// Accent purple.
pub const PURPLE: Color32 = Color32::from_rgb(0x8A, 0x5C, 0xD8);
/// Accent red.
pub const RED: Color32 = Color32::from_rgb(0xD8, 0x3F, 0x6B);
/// Accent gold.
pub const GOLD: Color32 = Color32::from_rgb(0xE3, 0xB2, 0x1C);
/// Fixed dark ink used for marks drawn on top of gold fills (readable in both themes).
const DARK: Color32 = Color32::from_rgb(0x2B, 0x2B, 0x2B);

macro_rules! icons {
    ($($(#[$m:meta])* $v:ident => $n:literal,)*) => {
        /// Every icon DeckCraft draws.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Icon {
            $($(#[$m])* $v,)*
        }

        impl Icon {
            /// All icons, in declaration order.
            pub const ALL: &'static [Icon] = &[$(Icon::$v,)*];

            /// Stable kebab-case name, e.g. `"format-painter"`.
            pub fn name(self) -> &'static str {
                match self {
                    $(Icon::$v => $n,)*
                }
            }

            /// Inverse of [`Icon::name`].
            pub fn from_name(s: &str) -> Option<Icon> {
                match s {
                    $($n => Some(Icon::$v),)*
                    _ => None,
                }
            }
        }
    };
}

mod draw;
mod geometry;

use draw::draw;
use geometry::Pen;
#[cfg(test)]
use geometry::{star_pts, triangulate};

icons! {
    // Clipboard & slides
    Paste => "paste",
    Cut => "cut",
    Copy => "copy",
    FormatPainter => "format-painter",
    NewSlide => "new-slide",
    Layout => "layout",
    ResetSlide => "reset-slide",
    Section => "section",
    DuplicateSlide => "duplicate-slide",
    DeleteSlide => "delete-slide",
    HideSlide => "hide-slide",
    Slide => "slide",
    // Font
    FontGrow => "font-grow",
    FontShrink => "font-shrink",
    ClearFormatting => "clear-formatting",
    Bold => "bold",
    Italic => "italic",
    Underline => "underline",
    Strikethrough => "strikethrough",
    Superscript => "superscript",
    Subscript => "subscript",
    CharSpacing => "char-spacing",
    ChangeCase => "change-case",
    Highlight => "highlight",
    FontColor => "font-color",
    TextShadow => "text-shadow",
    // Paragraph
    Bullets => "bullets",
    Numbering => "numbering",
    DecreaseIndent => "decrease-indent",
    IncreaseIndent => "increase-indent",
    LineSpacing => "line-spacing",
    Columns => "columns",
    AlignLeft => "align-left",
    AlignCenter => "align-center",
    AlignRight => "align-right",
    Justify => "justify",
    TextDirection => "text-direction",
    AlignText => "align-text",
    ConvertSmartArt => "convert-smart-art",
    // Drawing
    Shapes => "shapes",
    Arrange => "arrange",
    QuickStyles => "quick-styles",
    ShapeFill => "shape-fill",
    ShapeOutline => "shape-outline",
    ShapeEffects => "shape-effects",
    TextFill => "text-fill",
    TextOutline => "text-outline",
    TextEffects => "text-effects",
    EditShape => "edit-shape",
    MergeShapes => "merge-shapes",
    // Insert
    Table => "table",
    Picture => "picture",
    Screenshot => "screenshot",
    Icons => "icons",
    Models3d => "models-3d",
    SmartArt => "smart-art",
    Chart => "chart",
    Zoom => "zoom",
    Link => "link",
    Action => "action",
    Comment => "comment",
    TextBox => "text-box",
    HeaderFooter => "header-footer",
    WordArt => "word-art",
    DateTime => "date-time",
    SlideNumber => "slide-number",
    Object => "object",
    Equation => "equation",
    Symbol => "symbol",
    Video => "video",
    Audio => "audio",
    ScreenRecording => "screen-recording",
    Cameo => "cameo",
    // Draw
    Pen => "pen",
    Pencil => "pencil",
    HighlighterPen => "highlighter-pen",
    Eraser => "eraser",
    LassoSelect => "lasso-select",
    InkToShape => "ink-to-shape",
    InkToText => "ink-to-text",
    InkToMath => "ink-to-math",
    Ruler => "ruler",
    // Design
    Themes => "themes",
    Variants => "variants",
    Colors => "colors",
    Fonts => "fonts",
    BackgroundStyles => "background-styles",
    SlideSize => "slide-size",
    FormatBackground => "format-background",
    DesignIdeas => "design-ideas",
    // Transitions / animations
    Preview => "preview",
    EffectOptions => "effect-options",
    Sound => "sound",
    Duration => "duration",
    ApplyToAll => "apply-to-all",
    AnimEntrance => "anim-entrance",
    AnimEmphasis => "anim-emphasis",
    AnimExit => "anim-exit",
    AnimPath => "anim-path",
    AnimationPane => "animation-pane",
    Trigger => "trigger",
    AnimationPainter => "animation-painter",
    AddAnimation => "add-animation",
    // Slide show
    PlayFromStart => "play-from-start",
    PlayFromCurrent => "play-from-current",
    PresenterView => "presenter-view",
    CustomShow => "custom-show",
    SetUpShow => "set-up-show",
    RehearseTimings => "rehearse-timings",
    Record => "record",
    Subtitles => "subtitles",
    Coach => "coach",
    // Review
    Spelling => "spelling",
    Thesaurus => "thesaurus",
    Accessibility => "accessibility",
    Translate => "translate",
    Language => "language",
    NewComment => "new-comment",
    DeleteComment => "delete-comment",
    PrevComment => "prev-comment",
    NextComment => "next-comment",
    ShowComments => "show-comments",
    Compare => "compare",
    ReadOnly => "read-only",
    // View
    ViewNormal => "view-normal",
    ViewOutline => "view-outline",
    ViewSorter => "view-sorter",
    ViewNotesPage => "view-notes-page",
    ViewReading => "view-reading",
    SlideMaster => "slide-master",
    HandoutMaster => "handout-master",
    NotesMaster => "notes-master",
    Gridlines => "gridlines",
    Guides => "guides",
    Notes => "notes",
    ZoomGlass => "zoom-glass",
    FitToWindow => "fit-to-window",
    Grayscale => "grayscale",
    NewWindow => "new-window",
    Macros => "macros",
    // Arrange
    BringForward => "bring-forward",
    SendBackward => "send-backward",
    BringToFront => "bring-to-front",
    SendToBack => "send-to-back",
    Group => "group",
    Ungroup => "ungroup",
    AlignObjects => "align-objects",
    AlignObjLeft => "align-obj-left",
    AlignObjCenter => "align-obj-center",
    AlignObjRight => "align-obj-right",
    AlignObjTop => "align-obj-top",
    AlignObjMiddle => "align-obj-middle",
    AlignObjBottom => "align-obj-bottom",
    DistributeH => "distribute-h",
    DistributeV => "distribute-v",
    RotateLeft => "rotate-left",
    RotateRight => "rotate-right",
    FlipH => "flip-h",
    FlipV => "flip-v",
    SelectionPane => "selection-pane",
    Rotate => "rotate",
    // Picture
    Crop => "crop",
    Corrections => "corrections",
    ColorAdjust => "color-adjust",
    ArtisticEffects => "artistic-effects",
    Transparency => "transparency",
    CompressPictures => "compress-pictures",
    ChangePicture => "change-picture",
    ResetPicture => "reset-picture",
    RemoveBackground => "remove-background",
    AltText => "alt-text",
    // Table
    InsertAbove => "insert-above",
    InsertBelow => "insert-below",
    InsertLeft => "insert-left",
    InsertRight => "insert-right",
    DeleteTable => "delete-table",
    MergeCells => "merge-cells",
    SplitCells => "split-cells",
    Borders => "borders",
    Shading => "shading",
    // Chart
    ChartElements => "chart-elements",
    ChartStyles => "chart-styles",
    SwitchRowCol => "switch-row-col",
    EditData => "edit-data",
    ChangeChartType => "change-chart-type",
    // Media
    Play => "play",
    Pause => "pause",
    Stop => "stop",
    TrimMedia => "trim-media",
    Volume => "volume",
    Loop => "loop",
    Bookmark => "bookmark",
    // Window chrome
    Home => "home",
    Save => "save",
    Undo => "undo",
    Redo => "redo",
    More => "more",
    Search => "search",
    Share => "share",
    CommentsBubble => "comments-bubble",
    File => "file",
    Folder => "folder",
    Export => "export",
    Print => "print",
    Settings => "settings",
    Help => "help",
    Info => "info",
    Warning => "warning",
    Lock => "lock",
    /// Open padlock (Selection pane: an object that isn't locked).
    Unlock => "unlock",
    Eye => "eye",
    EyeOff => "eye-off",
    Close => "close",
    Check => "check",
    Plus => "plus",
    Minus => "minus",
    ChevronDown => "chevron-down",
    ChevronUp => "chevron-up",
    ChevronLeft => "chevron-left",
    ChevronRight => "chevron-right",
    DragHandle => "drag-handle",
    Star => "star",
    Image => "image",
    Sparkle => "sparkle",
    Palette => "palette",
    Grid => "grid",
    List => "list",
    Collapse => "collapse",
    Expand => "expand",
    Pin => "pin",
    // File tab (Backstage)
    BackCircle => "back-circle",
    SaveAs => "save-as",
    Account => "account",
    Recent => "recent",
    NewFile => "new-file",
    OpenFolder => "open-folder",
    // Status bar (small, monochrome)
    NotesSmall => "notes-small",
    CommentsSmall => "comments-small",
    ViewNormalSmall => "view-normal-small",
    ViewSorterSmall => "view-sorter-small",
    ViewReadingSmall => "view-reading-small",
    ViewShowSmall => "view-show-small",
    FitSmall => "fit-small",
    ZoomIn => "zoom-in",
    ZoomOut => "zoom-out",
}

/// Stroke width (in points) for an icon drawn at `size` points: ≈ size/16 at 16 pt, size/20 at
/// 32 pt and above, never thinner than 1.0.
fn stroke_width(size: f32) -> f32 {
    let t = ((size - 16.0) / 16.0).clamp(0.0, 1.0);
    (size / (16.0 + 4.0 * t)).max(1.0)
}

/// Paint `icon` into `rect` (square or not; draw centred in the largest square). `ink` is the
/// foreground (text) colour for the current theme; accents are fixed colours (see above), but when
/// `disabled` is true draw everything in `ink` at ~35% alpha.
pub fn paint(painter: &egui::Painter, rect: egui::Rect, icon: Icon, ink: egui::Color32, disabled: bool) {
    let size = rect.width().min(rect.height());
    if !size.is_finite() || size <= 0.5 {
        return;
    }
    let origin = rect.center() - vec2(size * 0.5, size * 0.5);
    let pen = Pen { p: painter, o: origin, k: size / 24.0, sw: stroke_width(size), ink, dis: disabled };
    draw(&pen, icon);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn names_round_trip_and_are_unique() {
        let mut seen = HashSet::new();
        let mut names = HashSet::new();
        for &icon in Icon::ALL {
            assert!(seen.insert(icon), "duplicate icon {icon:?} in ALL");
            let name = icon.name();
            assert!(names.insert(name), "duplicate name {name}");
            assert_eq!(Icon::from_name(name), Some(icon));
            assert!(name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'), "{name} is not kebab-case");
        }
        assert_eq!(Icon::from_name("no-such-icon"), None);
    }

    #[test]
    fn stroke_width_scales() {
        assert!((stroke_width(16.0) - 1.0).abs() < 1e-6);
        assert!((stroke_width(32.0) - 1.6).abs() < 1e-6);
        assert!((stroke_width(8.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn triangulates_concave_polygon() {
        let pts: Vec<Pos2> = star_pts((0.0, 0.0), 10.0, 4.0, 5, 0.0).into_iter().map(|(x, y)| pos2(x, y)).collect();
        assert_eq!(triangulate(&pts).len(), pts.len() - 2);
    }
}
