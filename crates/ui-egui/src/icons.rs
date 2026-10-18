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
    /// A broken link (detach text from its style).
    Detach => "detach",
    /// Text box padding: left and right, top and bottom, each side.
    PadH => "pad-h",
    PadV => "pad-v",
    PadSides => "pad-sides",
    /// Corner radius: each corner on its own, and the corners themselves.
    Corners => "corners",
    CornerTl => "corner-tl",
    CornerTr => "corner-tr",
    CornerBr => "corner-br",
    CornerBl => "corner-bl",
    /// Four small circles: styles (text styles, colour styles), as canvas editors mark them.
    Styles => "styles",
    /// Two sliders: more settings for a section (type details).
    Sliders => "sliders",
    /// Text anchored at the top, middle or bottom of its box: an arrow onto a line.
    TextTop => "text-top",
    TextMiddle => "text-middle",
    TextBottom => "text-bottom",
    /// A circular arrow: put back as it was (a placeholder onto its layout's position).
    Revert => "revert",
    /// A slide on an easel: a presentation (document tabs).
    Deck => "deck",
    /// A luggage tag: what placeholder of the layout a box is.
    Tag => "tag",
    /// A row of frames with a plus: the slide's animation states.
    States => "states",
    /// Lines appearing one by one: build a text box's paragraphs one at a time.
    Build => "build",
    Eye => "eye",
    EyeOff => "eye-off",
    Close => "close",
    Pointer => "pointer",
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
    let pen = Pen { p: painter, o: origin, k: size / 24.0, sw: stroke_width(size), ink, dis: disabled, mono: false };
    draw(&pen, icon);
}

/// [`paint`] in `ink` alone: accent colours are drawn in the ink too (the canvas shell's panel,
/// whose icons are one grey, as in canvas editors).
pub fn paint_mono(painter: &egui::Painter, rect: egui::Rect, icon: Icon, ink: egui::Color32, disabled: bool) {
    let size = rect.width().min(rect.height());
    if !size.is_finite() || size <= 0.5 {
        return;
    }
    let origin = rect.center() - vec2(size * 0.5, size * 0.5);
    let pen = Pen { p: painter, o: origin, k: size / 24.0, sw: stroke_width(size), ink, dis: disabled, mono: true };
    draw(&pen, icon);
}

// ---------------------------------------------------------------------------------------------
// Drawing toolkit
// ---------------------------------------------------------------------------------------------

/// A logical colour, resolved against the theme ink and the disabled flag.
#[derive(Clone, Copy)]
enum C {
    Ink(f32),
    Acc(Color32, f32),
    White,
    None,
}

impl C {
    fn a(self, f: f32) -> C {
        match self {
            C::Ink(a) => C::Ink(a * f),
            C::Acc(c, a) => C::Acc(c, a * f),
            other => other,
        }
    }
}

const K: C = C::Ink(1.0);
const B: C = C::Acc(BLUE, 1.0);
const O: C = C::Acc(ORANGE, 1.0);
const G: C = C::Acc(GREEN, 1.0);
const PU: C = C::Acc(PURPLE, 1.0);
const R: C = C::Acc(RED, 1.0);
const Y: C = C::Acc(GOLD, 1.0);
const D: C = C::Acc(DARK, 1.0);
const W: C = C::White;
const N: C = C::None;

/// Light tint of an accent, for fills behind outlines.
fn t(c: C) -> C {
    c.a(0.25)
}

type P2 = (f32, f32);

#[derive(Clone, Copy)]
enum Glyph {
    Plus,
    Cross,
    Check,
}

struct Pen<'a> {
    p: &'a Painter,
    o: Pos2,
    /// Points per design unit.
    k: f32,
    /// Base stroke width in points.
    sw: f32,
    ink: Color32,
    dis: bool,
    /// Accents in the ink.
    mono: bool,
}

fn rad(deg: f32) -> f32 {
    deg * PI / 180.0
}

fn arc_pts(c: P2, rx: f32, ry: f32, a0: f32, a1: f32) -> Vec<P2> {
    let n = (((a1 - a0).abs() / 9.0).ceil() as usize).max(4);
    (0..=n)
        .map(|i| {
            let a = rad(a0 + (a1 - a0) * i as f32 / n as f32);
            (c.0 + rx * a.cos(), c.1 + ry * a.sin())
        })
        .collect()
}

fn cubic_pts(p0: P2, p1: P2, p2: P2, p3: P2, n: usize) -> Vec<P2> {
    (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            (a * p0.0 + b * p1.0 + c * p2.0 + d * p3.0, a * p0.1 + b * p1.1 + c * p2.1 + d * p3.1)
        })
        .collect()
}

fn quad_pts(p0: P2, p1: P2, p2: P2, n: usize) -> Vec<P2> {
    (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            (u * u * p0.0 + 2.0 * u * t * p1.0 + t * t * p2.0, u * u * p0.1 + 2.0 * u * t * p1.1 + t * t * p2.1)
        })
        .collect()
}

/// Rounds the corners of a polyline by `r` design units.
fn round_pts(pts: &[P2], r: f32, closed: bool) -> Vec<P2> {
    let n = pts.len();
    if n < 3 || r <= 0.0 {
        return pts.to_vec();
    }
    let mut out = Vec::with_capacity(n * 6);
    for i in 0..n {
        let cur = pts[i];
        let interior = closed || (i > 0 && i + 1 < n);
        if !interior {
            out.push(cur);
            continue;
        }
        let prev = pts[(i + n - 1) % n];
        let next = pts[(i + 1) % n];
        let d1 = ((prev.0 - cur.0).powi(2) + (prev.1 - cur.1).powi(2)).sqrt();
        let d2 = ((next.0 - cur.0).powi(2) + (next.1 - cur.1).powi(2)).sqrt();
        if d1 < 1e-4 || d2 < 1e-4 {
            out.push(cur);
            continue;
        }
        let rr = r.min(d1 * 0.5).min(d2 * 0.5);
        let a = (cur.0 + (prev.0 - cur.0) / d1 * rr, cur.1 + (prev.1 - cur.1) / d1 * rr);
        let b = (cur.0 + (next.0 - cur.0) / d2 * rr, cur.1 + (next.1 - cur.1) / d2 * rr);
        out.extend(quad_pts(a, cur, b, 4));
    }
    out
}

fn rrect_pts(x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> Vec<P2> {
    round_pts(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1)], r, true)
}

/// Star polygon with `n` points.
fn star_pts(c: P2, ro: f32, ri: f32, n: usize, rot: f32) -> Vec<P2> {
    (0..n * 2)
        .map(|i| {
            let r = if i % 2 == 0 { ro } else { ri };
            let a = rad(rot - 90.0 + i as f32 * 180.0 / n as f32);
            (c.0 + r * a.cos(), c.1 + r * a.sin())
        })
        .collect()
}

/// Transforms local points (u along `ang`, v across) to design coordinates around `c`.
fn tf(c: P2, ang: f32, pts: &[P2]) -> Vec<P2> {
    let (s, co) = rad(ang).sin_cos();
    pts.iter().map(|&(u, v)| (c.0 + u * co - v * s, c.1 + u * s + v * co)).collect()
}

fn point_in_tri(p: Pos2, a: Pos2, b: Pos2, c: Pos2) -> bool {
    let d1 = (p.x - b.x) * (a.y - b.y) - (a.x - b.x) * (p.y - b.y);
    let d2 = (p.x - c.x) * (b.y - c.y) - (b.x - c.x) * (p.y - c.y);
    let d3 = (p.x - a.x) * (c.y - a.y) - (c.x - a.x) * (p.y - a.y);
    let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(neg && pos)
}

/// Ear-clipping triangulation of a simple polygon.
fn triangulate(pts: &[Pos2]) -> Vec<[u32; 3]> {
    let n = pts.len();
    let mut tris = Vec::new();
    if n < 3 {
        return tris;
    }
    let area: f32 = (0..n)
        .map(|i| {
            let a = pts[i];
            let b = pts[(i + 1) % n];
            a.x * b.y - b.x * a.y
        })
        .sum();
    let mut idx: Vec<usize> = (0..n).collect();
    if area < 0.0 {
        idx.reverse();
    }
    while idx.len() > 3 {
        let m = idx.len();
        let mut clipped = false;
        for i in 0..m {
            let (ia, ib, ic) = (idx[(i + m - 1) % m], idx[i], idx[(i + 1) % m]);
            let (a, b, c) = (pts[ia], pts[ib], pts[ic]);
            let cross = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
            if cross <= 1e-7 {
                continue;
            }
            let blocked = idx.iter().any(|&j| j != ia && j != ib && j != ic && point_in_tri(pts[j], a, b, c));
            if blocked {
                continue;
            }
            tris.push([ia as u32, ib as u32, ic as u32]);
            idx.remove(i);
            clipped = true;
            break;
        }
        if !clipped {
            // Degenerate input: drop a vertex and carry on rather than loop forever.
            idx.remove(0);
        }
    }
    if let [a, b, c] = idx[..] {
        tris.push([a as u32, b as u32, c as u32]);
    }
    tris
}

impl Pen<'_> {
    fn pt(&self, (x, y): P2) -> Pos2 {
        pos2(self.o.x + x * self.k, self.o.y + y * self.k)
    }

    fn v(&self, pts: &[P2]) -> Vec<Pos2> {
        pts.iter().map(|&p| self.pt(p)).collect()
    }

    fn col(&self, c: C) -> Color32 {
        let dim = if self.dis { 0.35 } else { 1.0 };
        match c {
            C::Ink(a) => self.ink.gamma_multiply(a * dim),
            C::Acc(col, a) => {
                if self.dis || self.mono {
                    self.ink.gamma_multiply(a * dim)
                } else {
                    col.gamma_multiply(a)
                }
            }
            C::White => {
                if self.dis {
                    // Glyphs on solid fills stay legible as a darker mark.
                    self.ink.gamma_multiply(0.6)
                } else {
                    Color32::WHITE
                }
            }
            C::None => Color32::TRANSPARENT,
        }
    }

    fn stroke(&self, c: C, w: f32) -> Stroke {
        Stroke::new(self.sw * w, self.col(c))
    }

    // --- lines ---

    fn line_w(&self, pts: &[P2], c: C, w: f32) {
        if pts.len() < 2 {
            return;
        }
        let col = self.col(c);
        if col == Color32::TRANSPARENT {
            return;
        }
        let v = self.v(pts);
        if let [a, b] = v[..] {
            // Single segments get round caps: draw them as one convex capsule.
            let d = b - a;
            let len = d.length();
            let r = self.sw * w * 0.5;
            if len < 1e-3 {
                self.p.circle_filled(a, r, col);
                return;
            }
            let ang = d.y.atan2(d.x);
            let mut cap = Vec::with_capacity(20);
            for (c, start) in [(b, ang - PI * 0.5), (a, ang + PI * 0.5)] {
                for i in 0..=8 {
                    let t = start + PI * i as f32 / 8.0;
                    cap.push(c + vec2(t.cos(), t.sin()) * r);
                }
            }
            self.p.add(PathShape::convex_polygon(cap, col, Stroke::NONE));
            return;
        }
        self.p.add(PathShape::line(v, Stroke::new(self.sw * w, col)));
    }

    fn line(&self, pts: &[P2], c: C) {
        self.line_w(pts, c, 1.0);
    }

    fn seg(&self, a: P2, b: P2, c: C) {
        self.line_w(&[a, b], c, 1.0);
    }

    fn seg_w(&self, a: P2, b: P2, c: C, w: f32) {
        self.line_w(&[a, b], c, w);
    }

    fn rline(&self, pts: &[P2], r: f32, c: C) {
        self.line(&round_pts(pts, r, false), c);
    }

    fn rline_w(&self, pts: &[P2], r: f32, c: C, w: f32) {
        self.line_w(&round_pts(pts, r, false), c, w);
    }

    fn dashed(&self, pts: &[P2], c: C, dash: f32, gap: f32) {
        let v = self.v(pts);
        self.p.extend(Shape::dashed_line(&v, self.stroke(c, 1.0), dash * self.k, gap * self.k));
    }

    fn dashed_w(&self, pts: &[P2], c: C, dash: f32, gap: f32, w: f32) {
        let v = self.v(pts);
        self.p.extend(Shape::dashed_line(&v, self.stroke(c, w), dash * self.k, gap * self.k));
    }

    fn dotted(&self, pts: &[P2], c: C, spacing: f32, r: f32) {
        let v = self.v(pts);
        self.p.extend(Shape::dotted_line(&v, self.col(c), spacing * self.k, r * self.sw));
    }

    // --- filled shapes ---

    /// Any simple polygon: filled via triangulation, stroked as a closed path.
    fn shape_w(&self, pts: &[P2], fill: C, stroke: C, w: f32) {
        if pts.len() < 3 {
            return;
        }
        let v = self.v(pts);
        let fc = self.col(fill);
        if fc != Color32::TRANSPARENT {
            let mut mesh = Mesh::default();
            for &p in &v {
                mesh.colored_vertex(p, fc);
            }
            for [a, b, c] in triangulate(&v) {
                mesh.add_triangle(a, b, c);
            }
            self.p.add(Shape::mesh(mesh));
            // A hairline in the fill colour anti-aliases the mesh edge.
            if matches!(stroke, C::None) {
                self.p.add(PathShape::closed_line(v.clone(), Stroke::new(0.6_f32.min(self.sw * 0.5), fc)));
            }
        }
        let sc = self.col(stroke);
        if sc != Color32::TRANSPARENT {
            self.p.add(PathShape::closed_line(v, Stroke::new(self.sw * w, sc)));
        }
    }

    fn shape(&self, pts: &[P2], fill: C, stroke: C) {
        self.shape_w(pts, fill, stroke, 1.0);
    }

    fn rshape(&self, pts: &[P2], r: f32, fill: C, stroke: C) {
        self.shape(&round_pts(pts, r, true), fill, stroke);
    }

    /// Convex polygon (anti-aliased fill).
    fn poly(&self, pts: &[P2], fill: C, stroke: C) {
        self.poly_w(pts, fill, stroke, 1.0);
    }

    fn poly_w(&self, pts: &[P2], fill: C, stroke: C, w: f32) {
        let v = self.v(pts);
        self.p.add(PathShape::convex_polygon(v, self.col(fill), self.stroke(stroke, w)));
    }

    fn rect_w(&self, x0: f32, y0: f32, x1: f32, y1: f32, r: f32, fill: C, stroke: C, w: f32) {
        let rect = Rect::from_min_max(self.pt((x0, y0)), self.pt((x1, y1)));
        self.p.rect(rect, r * self.k, self.col(fill), self.stroke(stroke, w), StrokeKind::Middle);
    }

    fn rect(&self, x0: f32, y0: f32, x1: f32, y1: f32, r: f32, fill: C, stroke: C) {
        self.rect_w(x0, y0, x1, y1, r, fill, stroke, 1.0);
    }

    fn circle_w(&self, c: P2, r: f32, fill: C, stroke: C, w: f32) {
        self.p.circle(self.pt(c), r * self.k, self.col(fill), self.stroke(stroke, w));
    }

    fn circle(&self, c: P2, r: f32, fill: C, stroke: C) {
        self.circle_w(c, r, fill, stroke, 1.0);
    }

    fn dot(&self, c: P2, r: f32, fill: C) {
        self.p.circle_filled(self.pt(c), r * self.k, self.col(fill));
    }

    fn ellipse(&self, c: P2, rx: f32, ry: f32, fill: C, stroke: C) {
        let mut pts = arc_pts(c, rx, ry, 0.0, 360.0);
        pts.pop();
        self.poly(&pts, fill, stroke);
    }

    // --- arcs and arrows ---

    fn arc(&self, c: P2, r: f32, a0: f32, a1: f32, col: C) {
        self.line(&arc_pts(c, r, r, a0, a1), col);
    }

    /// Filled arrow head with its tip at `tip`, pointing along `dir` degrees.
    fn head(&self, tip: P2, dir: f32, size: f32, c: C) {
        let (s, co) = rad(dir).sin_cos();
        let hw = size * 0.62;
        let back = (tip.0 - co * size, tip.1 - s * size);
        self.poly(&[tip, (back.0 - s * hw, back.1 + co * hw), (back.0 + s * hw, back.1 - co * hw)], c, N);
    }

    fn head_size(&self) -> f32 {
        (2.2 * self.sw / self.k).max(3.6)
    }

    fn arrow(&self, a: P2, b: P2, c: C) {
        let dir = (b.1 - a.1).atan2(b.0 - a.0);
        let hs = self.head_size();
        let end = (b.0 - dir.cos() * hs * 0.7, b.1 - dir.sin() * hs * 0.7);
        self.seg(a, end, c);
        self.head(b, dir.to_degrees(), hs, c);
    }

    fn arrow2(&self, a: P2, b: P2, c: C) {
        let dir = (b.1 - a.1).atan2(b.0 - a.0);
        let hs = self.head_size();
        let s = (a.0 + dir.cos() * hs * 0.7, a.1 + dir.sin() * hs * 0.7);
        let e = (b.0 - dir.cos() * hs * 0.7, b.1 - dir.sin() * hs * 0.7);
        self.seg(s, e, c);
        self.head(b, dir.to_degrees(), hs, c);
        self.head(a, dir.to_degrees() + 180.0, hs, c);
    }

    /// Polyline ending in an arrow head.
    fn path_arrow(&self, pts: &[P2], c: C) {
        let n = pts.len();
        if n < 2 {
            return;
        }
        let a = pts[n - 2];
        let b = pts[n - 1];
        let dir = (b.1 - a.1).atan2(b.0 - a.0);
        let hs = self.head_size();
        let mut body = pts.to_vec();
        body[n - 1] = (b.0 - dir.cos() * hs * 0.7, b.1 - dir.sin() * hs * 0.7);
        self.line(&body, c);
        self.head(b, dir.to_degrees(), hs, c);
    }

    /// Circular arc from `a0` to `a1` degrees with an arrow head at the `a1` end.
    fn arc_arrow(&self, c: P2, r: f32, a0: f32, a1: f32, col: C) {
        let hs = self.head_size();
        let sign = if a1 >= a0 { 1.0 } else { -1.0 };
        let back = (hs * 0.6 / r).to_degrees() * sign;
        self.line(&arc_pts(c, r, r, a0, a1 - back), col);
        let tip_a = rad(a1);
        let tip = (c.0 + r * tip_a.cos(), c.1 + r * tip_a.sin());
        // Tangent direction of travel, nudged inward to sit on the arc.
        let dir = a1 + 90.0 * sign - (hs * 0.5 / r).to_degrees() * sign;
        self.head(tip, dir, hs, col);
    }

    fn curve(&self, p0: P2, p1: P2, p2: P2, p3: P2, c: C) {
        self.line(&cubic_pts(p0, p1, p2, p3, 24), c);
    }

    fn curve_w(&self, p0: P2, p1: P2, p2: P2, p3: P2, c: C, w: f32) {
        self.line_w(&cubic_pts(p0, p1, p2, p3, 24), c, w);
    }

    // --- text ---

    fn text(&self, s: &str, at: P2, size: f32, c: C) {
        self.p.text(self.pt(at), Align2::CENTER_CENTER, s, FontId::proportional(size * self.k), self.col(c));
    }

    fn text_bold(&self, s: &str, at: P2, size: f32, c: C) {
        let d = 0.32 * self.sw / self.k;
        let reps: &[f32] = if self.dis { &[0.0] } else { &[-d, 0.0, d] };
        for &dx in reps {
            self.text(s, (at.0 + dx, at.1), size, c);
        }
    }

    fn text_rot(&self, s: &str, at: P2, size: f32, c: C, angle_deg: f32) {
        let col = self.col(c);
        let galley = self.p.layout_no_wrap(s.to_owned(), FontId::proportional(size * self.k), col);
        let rot = egui::emath::Rot2::from_angle(rad(angle_deg));
        let pos = self.pt(at) - rot * (galley.size() * 0.5);
        self.p.add(TextShape::new(pos, galley, col).with_angle(rad(angle_deg)));
    }

    // --- composite parts ---

    /// Round badge with a white glyph (outline + ink glyph when disabled).
    fn badge(&self, c: P2, r: f32, fill: C, glyph: Glyph) {
        let gc = if self.dis {
            self.circle(c, r, N, fill);
            K
        } else {
            self.circle(c, r, fill, N);
            W
        };
        let g = r * 0.5;
        let w = 0.95;
        match glyph {
            Glyph::Plus => {
                self.seg_w((c.0 - g, c.1), (c.0 + g, c.1), gc, w);
                self.seg_w((c.0, c.1 - g), (c.0, c.1 + g), gc, w);
            }
            Glyph::Cross => {
                let g = g * 0.85;
                self.seg_w((c.0 - g, c.1 - g), (c.0 + g, c.1 + g), gc, w);
                self.seg_w((c.0 + g, c.1 - g), (c.0 - g, c.1 + g), gc, w);
            }
            Glyph::Check => {
                self.line_w(&[(c.0 - g, c.1 + 0.1), (c.0 - g * 0.25, c.1 + g * 0.75), (c.0 + g, c.1 - g * 0.6)], gc, w);
            }
        }
    }

    /// Landscape slide frame.
    fn frame(&self, x0: f32, y0: f32, x1: f32, y1: f32) {
        self.rect(x0, y0, x1, y1, 1.5, N, K);
    }

    /// Document page with a folded top-right corner.
    fn page(&self, x0: f32, y0: f32, x1: f32, y1: f32, fill: C, stroke: C) {
        let f = ((x1 - x0) * 0.32).min(5.0);
        self.rshape(&[(x0, y0), (x1 - f, y0), (x1, y0 + f), (x1, y1), (x0, y1)], 1.2, fill, stroke);
        self.line(&[(x1 - f, y0), (x1 - f, y0 + f), (x1, y0 + f)], stroke);
    }

    /// Picture frame with sun and hills.
    fn picture(&self, x0: f32, y0: f32, x1: f32, y1: f32) {
        let (w, h) = (x1 - x0, y1 - y0);
        let m = w.min(h);
        self.rect(x0, y0, x1, y1, 1.5, N, K);
        self.dot((x1 - w * 0.27, y0 + h * 0.3), m * 0.12, Y);
        let base = y1 - h * 0.14;
        self.poly(&[(x0 + w * 0.12, base), (x0 + w * 0.38, y0 + h * 0.36), (x0 + w * 0.64, base)], G, N);
        self.poly(&[(x0 + w * 0.48, base), (x0 + w * 0.68, y0 + h * 0.56), (x1 - w * 0.12, base)], G.a(0.7), N);
    }

    fn magnifier(&self, c: P2, r: f32, lens_fill: C, col: C) {
        self.circle(c, r, lens_fill, col);
        let d = r * 0.707;
        self.seg_w((c.0 + d + 0.6, c.1 + d + 0.6), (c.0 + r * 1.55, c.1 + r * 1.55), col, 1.5);
    }

    fn bubble_pts(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<P2> {
        let r = 2.5;
        let mut pts = arc_pts((x0 + r, y0 + r), r, r, 180.0, 270.0);
        pts.extend(arc_pts((x1 - r, y0 + r), r, r, 270.0, 360.0));
        pts.extend(arc_pts((x1 - r, y1 - r), r, r, 0.0, 90.0));
        let tw = ((x1 - x0) * 0.22).clamp(2.5, 4.5);
        pts.push((x0 + r + 0.8 + tw, y1));
        pts.push((x0 + r - 0.2, y1 + 3.8));
        pts.push((x0 + r + 0.8, y1));
        pts.extend(arc_pts((x0 + r, y1 - r), r, r, 90.0, 180.0));
        pts
    }

    fn bubble(&self, x0: f32, y0: f32, x1: f32, y1: f32, fill: C, stroke: C) {
        self.shape(&Self::bubble_pts(x0, y0, x1, y1), fill, stroke);
    }

    fn person(&self, cx: f32, top: f32, s: f32, fill: C) {
        self.dot((cx, top + 2.6 * s), 2.6 * s, fill);
        let base = top + 12.0 * s;
        self.poly(&arc_pts((cx, base), 5.2 * s, 5.0 * s, 180.0, 360.0), fill, N);
    }

    fn star(&self, c: P2, ro: f32, fill: C, stroke: C) {
        self.shape(&star_pts(c, ro, ro * 0.47, 5, 0.0), fill, stroke);
    }

    fn sparkle(&self, c: P2, r: f32, fill: C) {
        self.shape(&star_pts(c, r, r * 0.3, 4, 0.0), fill, N);
    }

    fn gear(&self, c: P2, ro: f32, ri: f32, teeth: usize, fill: C, stroke: C) {
        let step = 360.0 / teeth as f32;
        let mut pts = Vec::with_capacity(teeth * 4);
        for i in 0..teeth {
            let base = i as f32 * step;
            for (a, r) in [(-0.3, ri), (-0.17, ro), (0.17, ro), (0.3, ri)] {
                let ang = rad(base + a * step);
                pts.push((c.0 + r * ang.cos(), c.1 + r * ang.sin()));
            }
        }
        self.shape(&pts, fill, stroke);
    }

    fn speaker(&self, x: f32, cy: f32, fill: C, stroke: C) {
        self.rshape(
            &[(x, cy - 3.0), (x + 3.8, cy - 3.0), (x + 8.5, cy - 7.5), (x + 8.5, cy + 7.5), (x + 3.8, cy + 3.0), (x, cy + 3.0)],
            0.8,
            fill,
            stroke,
        );
    }

    /// Table grid; `header` fills the first row.
    fn table(&self, x0: f32, y0: f32, x1: f32, y1: f32, rows: usize, cols: usize, header: C) {
        let rh = (y1 - y0) / rows as f32;
        let cw = (x1 - x0) / cols as f32;
        if !matches!(header, C::None) {
            self.rect(x0, y0, x1, y0 + rh, 1.2, header, N);
        }
        for i in 1..rows {
            let y = y0 + rh * i as f32;
            self.seg_w((x0, y), (x1, y), K, 0.8);
        }
        for j in 1..cols {
            let x = x0 + cw * j as f32;
            self.seg_w((x, y0), (x, y1), K, 0.8);
        }
        self.rect(x0, y0, x1, y1, 1.2, N, K);
    }

    /// Bar chart in the box.
    fn bars(&self, x0: f32, y0: f32, x1: f32, y1: f32) {
        let w = (x1 - x0) / 3.0;
        let h = y1 - y0;
        for (i, (frac, c)) in [(0.5, B), (1.0, O), (0.72, G)].into_iter().enumerate() {
            let bx = x0 + w * i as f32;
            self.rect(bx + w * 0.16, y1 - h * frac, bx + w * 0.84, y1, 0.6, c, N);
        }
        self.seg((x0 - 0.5, y1 + 0.2), (x1 + 0.5, y1 + 0.2), K);
    }

    /// A writing tool lying along `ang` degrees with its point at `tip`.
    fn tool(&self, tip: P2, ang: f32, len: f32, hw: f32, cone: f32, body: C, lead: C, cap_len: f32, cap: C) {
        let m = |pts: &[P2]| tf(tip, ang, pts);
        self.poly(&m(&[(0.0, 0.0), (cone, -hw), (cone, hw)]), N, K);
        self.poly(&m(&[(0.0, 0.0), (cone * 0.42, -hw * 0.42), (cone * 0.42, hw * 0.42)]), lead, N);
        self.poly(&m(&[(cone, -hw), (len - cap_len, -hw), (len - cap_len, hw), (cone, hw)]), body, K);
        if cap_len > 0.0 {
            self.poly(&m(&[(len - cap_len, -hw), (len, -hw), (len, hw), (len - cap_len, hw)]), cap, K);
        }
    }

    /// Paint brush lying along `ang` with bristle tip at `tip`.
    fn brush(&self, tip: P2, ang: f32, len: f32, bristle: C) {
        let m = |pts: &[P2]| tf(tip, ang, pts);
        self.shape(&m(&[(0.0, -0.4), (2.5, -2.2), (5.5, -2.2), (5.5, 2.2), (2.5, 2.2), (0.0, 0.4)]), bristle, K);
        self.poly(&m(&[(5.5, -2.0), (7.8, -2.0), (7.8, 2.0), (5.5, 2.0)]), K, K);
        self.line(&m(&[(7.8, -1.1), (len, -1.3), (len, 1.3), (7.8, 1.1)]), K);
    }

    /// Small path-drawn digit 1–3 in a `3 × h` box.
    fn digit(&self, d: u8, x: f32, y: f32, h: f32, c: C) {
        let w = 0.8;
        match d {
            1 => self.line_w(&[(x + 0.4, y + 1.2), (x + 2.0, y), (x + 2.0, y + h)], c, w),
            2 => self.line_w(&[(x, y + 1.0), (x + 0.7, y), (x + 2.4, y), (x + 3.1, y + 1.1), (x + 2.9, y + 2.2), (x, y + h), (x + 3.2, y + h)], c, w),
            _ => self.line_w(
                &[(x, y), (x + 3.0, y), (x + 1.3, y + h * 0.42), (x + 2.6, y + h * 0.5), (x + 3.1, y + h * 0.7), (x + 2.5, y + h), (x, y + h)],
                c,
                w,
            ),
        }
    }

    /// Path-drawn capital A.
    fn letter_a(&self, x0: f32, top: f32, x1: f32, bottom: f32, c: C, w: f32) {
        let mx = (x0 + x1) * 0.5;
        self.line_w(&[(x0, bottom), (mx, top), (x1, bottom)], c, w);
        let f = 0.62;
        let y = top + (bottom - top) * f;
        let dx = (x1 - x0) * 0.5 * f;
        self.seg_w((mx - dx, y), (mx + dx, y), c, w);
    }

    /// Hand-drawn scribble in the box.
    fn scribble(&self, x0: f32, y0: f32, x1: f32, y1: f32, c: C) {
        let (w, h) = (x1 - x0, y1 - y0);
        let p = |u: f32, v: f32| (x0 + w * u, y0 + h * v);
        // A loopy cursive stroke (prolate cycloid), normalised into the box.
        let raw: Vec<P2> = (0..=48)
            .map(|i| {
                let th = i as f32 / 48.0 * 4.0 * PI + 0.6;
                (th - 1.9 * th.sin(), -1.9 * th.cos())
            })
            .collect();
        let (mut lx, mut hx, mut ly, mut hy) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for &(x, y) in &raw {
            lx = lx.min(x);
            hx = hx.max(x);
            ly = ly.min(y);
            hy = hy.max(y);
        }
        let sx = (hx - lx).max(1e-3);
        let sy = (hy - ly).max(1e-3);
        let pts: Vec<P2> = raw.iter().map(|&(x, y)| p((x - lx) / sx, (y - ly) / sy)).collect();
        self.line(&pts, c);
    }

    /// Isometric "layer" diamond.
    fn layer(&self, cx: f32, cy: f32, hw: f32, hh: f32, fill: C, stroke: C) {
        self.rshape(&[(cx, cy - hh), (cx + hw, cy), (cx, cy + hh), (cx - hw, cy)], 0.8, fill, stroke);
    }

    /// Open book.
    fn book(&self, fill: C, col: C) {
        let left = {
            let mut p = quad_pts((12.0, 7.0), (7.5, 4.0), (2.0, 5.0), 8);
            p.extend(quad_pts((2.0, 18.5), (7.5, 17.5), (12.0, 20.5), 8));
            p
        };
        let right = {
            let mut p = quad_pts((12.0, 7.0), (16.5, 4.0), (22.0, 5.0), 8);
            p.extend(quad_pts((22.0, 18.5), (16.5, 17.5), (12.0, 20.5), 8));
            p
        };
        self.shape(&left, fill, col);
        self.shape(&right, fill, col);
    }

    fn monitor(&self, x0: f32, y0: f32, x1: f32, y1: f32) {
        self.rect(x0, y0, x1, y1, 1.5, N, K);
        let cx = (x0 + x1) * 0.5;
        self.seg((cx, y1), (cx, y1 + 3.2), K);
        self.seg((cx - 4.0, y1 + 3.6), (cx + 4.0, y1 + 3.6), K);
    }

    fn clock_badge(&self, c: P2, r: f32, fill: C) {
        let hc = if self.dis {
            self.circle(c, r, N, fill);
            K
        } else {
            self.circle(c, r, fill, N);
            W
        };
        self.line_w(&[(c.0, c.1 - r * 0.6), c, (c.0 + r * 0.5, c.1 + r * 0.2)], hc, 0.9);
    }

    fn gear_badge(&self, c: P2, r: f32, fill: C) {
        self.gear(c, r, r * 0.74, 8, fill, N);
        if self.dis {
            self.gear(c, r, r * 0.74, 8, N, fill);
        }
        self.dot(c, r * 0.32, if self.dis { N } else { W });
        if self.dis {
            self.circle(c, r * 0.32, N, fill);
        }
    }

    fn lock(&self, x0: f32, y0: f32, x1: f32, y1: f32) {
        let cx = (x0 + x1) * 0.5;
        let r = (x1 - x0) * 0.3;
        let mut sh = vec![(cx - r, y0)];
        sh.extend(arc_pts((cx, y0 - r * 0.7), r, r, 180.0, 360.0));
        sh.push((cx + r, y0));
        self.line(&sh, K);
        self.rect(x0, y0, x1, y1, 1.2, Y, K);
        let ky = (y0 + y1) * 0.5;
        self.dot((cx, ky - 0.4), 1.1, D);
        self.seg_w((cx, ky), (cx, ky + 2.0), D, 0.9);
    }

    fn cursor(&self, x: f32, y: f32, s: f32) {
        let p = |u: f32, v: f32| (x + u * s, y + v * s);
        let pts = [p(0.0, 0.0), p(0.0, 15.0), p(3.8, 11.6), p(6.4, 17.0), p(8.6, 16.0), p(6.1, 10.8), p(11.0, 10.8)];
        self.shape(&pts, K, K);
    }
}

// ---------------------------------------------------------------------------------------------
// The icons
// ---------------------------------------------------------------------------------------------

#[expect(clippy::too_many_lines)]
fn draw(p: &Pen<'_>, icon: Icon) {
    use Icon as I;
    match icon {
        // ----- Clipboard & slides -----
        I::Paste => {
            p.rline(&[(10.5, 21.5), (3.5, 21.5), (3.5, 4.5), (17.0, 4.5), (17.0, 9.0)], 2.0, K);
            p.rect(7.0, 2.5, 13.5, 6.5, 1.2, Y, K);
            p.rect(10.5, 10.0, 21.0, 22.0, 1.2, t(B), B);
            p.seg((13.0, 14.0), (18.5, 14.0), B);
            p.seg((13.0, 18.0), (17.0, 18.0), B);
        }
        I::Cut => {
            p.seg((8.6, 14.6), (16.5, 2.5), K);
            p.seg((15.4, 14.6), (7.5, 2.5), K);
            p.circle((6.8, 17.6), 3.3, N, O);
            p.circle((17.2, 17.6), 3.3, N, O);
            p.dot((12.0, 9.4), 1.0, K);
        }
        I::Copy => {
            p.rline(&[(8.0, 16.0), (3.0, 16.0), (3.0, 2.5), (14.0, 2.5), (14.0, 7.0)], 1.5, K);
            p.rect(8.0, 7.5, 20.5, 21.5, 1.5, t(B), B);
            p.seg((11.0, 12.5), (17.5, 12.5), B);
            p.seg((11.0, 16.5), (16.0, 16.5), B);
        }
        I::FormatPainter => {
            p.rect(2.5, 3.0, 17.0, 9.0, 1.5, Y, K);
            p.rline(&[(17.0, 6.0), (20.5, 6.0), (20.5, 11.5), (11.0, 11.5), (11.0, 14.0)], 1.2, K);
            p.rect(9.0, 14.0, 13.0, 21.5, 1.0, K, K);
        }
        I::NewSlide => {
            p.frame(2.0, 4.0, 19.5, 16.5);
            p.seg((5.0, 8.0), (13.0, 8.0), B);
            p.seg((5.0, 12.0), (10.0, 12.0), K.a(0.5));
            p.badge((18.0, 17.5), 5.0, G, Glyph::Plus);
        }
        I::Layout => {
            p.frame(2.0, 3.5, 22.0, 20.5);
            p.rect(5.0, 6.5, 19.0, 9.0, 0.8, B, N);
            p.rect(5.0, 11.5, 11.0, 17.5, 0.8, t(G), G);
            p.rect(13.0, 11.5, 19.0, 17.5, 0.8, t(O), O);
        }
        I::ResetSlide => {
            p.frame(2.0, 3.5, 22.0, 20.5);
            p.arc_arrow((12.0, 12.0), 4.6, -20.0, -290.0, B);
        }
        I::Section => {
            p.rshape(&[(2.5, 2.5), (13.0, 2.5), (16.5, 5.75), (13.0, 9.0), (2.5, 9.0)], 0.6, PU, N);
            p.rline(&[(5.0, 9.0), (5.0, 16.0), (7.5, 16.0)], 1.0, K);
            p.rect(7.5, 12.0, 21.5, 20.5, 1.2, N, K);
            p.seg((10.0, 15.5), (16.0, 15.5), K.a(0.5));
        }
        I::DuplicateSlide => {
            p.rline(&[(6.5, 15.5), (2.5, 15.5), (2.5, 3.5), (16.5, 3.5), (16.5, 8.0)], 1.5, K);
            p.rect(6.5, 8.0, 21.5, 20.5, 1.5, t(B), B);
            p.seg((14.0, 11.5), (14.0, 17.0), B);
            p.seg((11.25, 14.25), (16.75, 14.25), B);
        }
        I::DeleteSlide => {
            p.frame(2.0, 4.0, 19.5, 16.5);
            p.seg((5.0, 8.0), (13.0, 8.0), B);
            p.seg((5.0, 12.0), (10.0, 12.0), K.a(0.5));
            p.badge((18.0, 17.5), 5.0, R, Glyph::Cross);
        }
        I::HideSlide => {
            p.rect(2.0, 4.0, 22.0, 20.0, 1.5, N, K.a(0.55));
            p.seg((5.0, 8.0), (13.0, 8.0), K.a(0.55));
            p.seg((3.0, 21.0), (21.0, 3.0), R);
        }
        I::Slide => {
            p.frame(2.0, 4.5, 22.0, 19.5);
            p.seg((5.0, 8.5), (15.0, 8.5), B);
            p.rect(5.0, 11.5, 19.0, 16.5, 0.8, t(B), N);
        }

        // ----- Font -----
        I::FontGrow => {
            p.letter_a(2.5, 4.0, 15.0, 20.5, K, 1.15);
            p.poly(&[(16.0, 10.5), (22.5, 10.5), (19.25, 5.0)], B, N);
        }
        I::FontShrink => {
            p.letter_a(3.0, 8.0, 13.5, 20.5, K, 1.0);
            p.poly(&[(16.0, 5.0), (22.5, 5.0), (19.25, 10.5)], B, N);
        }
        I::ClearFormatting => {
            p.letter_a(2.5, 3.0, 14.0, 17.5, K, 1.1);
            let m = |pts: &[P2]| tf((17.0, 16.5), -45.0, pts);
            p.poly(&m(&[(-5.5, -2.8), (0.0, -2.8), (0.0, 2.8), (-5.5, 2.8)]), R, R);
            p.poly(&m(&[(0.0, -2.8), (5.5, -2.8), (5.5, 2.8), (0.0, 2.8)]), N, K);
        }
        I::Bold => p.text_bold("B", (12.0, 12.0), 22.0, K),
        I::Italic => {
            p.seg_w((10.5, 4.0), (18.5, 4.0), K, 1.1);
            p.seg_w((5.5, 20.0), (13.5, 20.0), K, 1.1);
            p.seg_w((14.5, 4.0), (9.5, 20.0), K, 1.1);
        }
        I::Underline => {
            let mut pts = vec![(6.5, 3.0)];
            pts.extend(arc_pts((12.0, 11.5), 5.5, 5.5, 180.0, 0.0));
            pts.push((17.5, 3.0));
            p.line_w(&pts, K, 1.1);
            p.seg_w((4.5, 21.0), (19.5, 21.0), B, 1.2);
        }
        I::Strikethrough => {
            p.text("ab", (12.0, 11.0), 19.0, K);
            p.seg_w((2.5, 13.0), (21.5, 13.0), R, 1.1);
        }
        I::Superscript => {
            p.seg((3.0, 9.0), (13.0, 21.0), K);
            p.seg((13.0, 9.0), (3.0, 21.0), K);
            p.digit(2, 16.0, 2.5, 6.5, B);
        }
        I::Subscript => {
            p.seg((3.0, 3.5), (13.0, 15.5), K);
            p.seg((13.0, 3.5), (3.0, 15.5), K);
            p.digit(2, 16.0, 15.0, 6.5, B);
        }
        I::CharSpacing => {
            p.letter_a(2.0, 3.0, 10.5, 15.0, K, 1.0);
            p.line(&[(13.5, 3.0), (17.75, 15.0), (22.0, 3.0)], K);
            p.arrow2((2.5, 20.0), (21.5, 20.0), B);
        }
        I::ChangeCase => {
            p.text("A", (8.0, 11.0), 19.0, K);
            p.text("a", (17.5, 12.5), 16.0, B);
            p.seg((3.0, 21.0), (21.0, 21.0), K.a(0.4));
        }
        I::Highlight => {
            let m = |pts: &[P2]| tf((8.0, 15.0), -45.0, pts);
            p.poly(&m(&[(0.0, -1.2), (3.0, -3.0), (3.0, 3.0), (0.0, 1.4)]), Y, K);
            p.poly(&m(&[(3.0, -3.0), (13.5, -3.0), (13.5, 3.0), (3.0, 3.0)]), N, K);
            p.rect(2.5, 19.5, 21.5, 22.5, 0.8, Y, N);
        }
        I::FontColor => {
            p.letter_a(4.5, 2.5, 19.5, 17.0, K, 1.1);
            p.rect(2.5, 19.5, 21.5, 22.5, 0.8, R, N);
        }
        I::TextShadow => {
            p.text("S", (14.0, 13.8), 22.0, K.a(0.3));
            p.text_bold("S", (11.0, 11.0), 22.0, K);
        }

        // ----- Paragraph -----
        I::Bullets => {
            for y in [5.5, 12.0, 18.5] {
                p.dot((4.5, y), 1.8, B);
                p.seg((9.0, y), (21.0, y), K);
            }
        }
        I::Numbering => {
            for (i, y) in [5.5_f32, 12.0, 18.5].into_iter().enumerate() {
                p.digit(i as u8 + 1, 2.8, y - 2.6, 5.2, B);
                p.seg((9.5, y), (21.0, y), K);
            }
        }
        I::DecreaseIndent | I::IncreaseIndent => {
            p.seg((3.0, 4.0), (21.0, 4.0), K);
            p.seg((11.0, 9.5), (21.0, 9.5), K);
            p.seg((11.0, 14.5), (21.0, 14.5), K);
            p.seg((3.0, 20.0), (21.0, 20.0), K);
            if icon == I::DecreaseIndent {
                p.poly(&[(8.0, 8.0), (8.0, 16.0), (3.0, 12.0)], B, N);
            } else {
                p.poly(&[(3.0, 8.0), (3.0, 16.0), (8.0, 12.0)], B, N);
            }
        }
        I::LineSpacing => {
            for y in [5.0, 12.0, 19.0] {
                p.seg((11.0, y), (21.0, y), K);
            }
            p.arrow2((5.0, 2.5), (5.0, 21.5), B);
        }
        I::Columns => {
            for y in [4.5, 9.5, 14.5, 19.5] {
                p.seg((2.5, y), (10.5, y), K);
                p.seg((13.5, y), (21.5, y), K);
            }
        }
        I::AlignLeft | I::AlignCenter | I::AlignRight | I::Justify => {
            let rows: [(f32, f32); 4] = match icon {
                I::AlignLeft => [(3.0, 21.0), (3.0, 15.0), (3.0, 21.0), (3.0, 13.0)],
                I::AlignCenter => [(3.0, 21.0), (7.0, 17.0), (3.0, 21.0), (8.0, 16.0)],
                I::AlignRight => [(3.0, 21.0), (9.0, 21.0), (3.0, 21.0), (11.0, 21.0)],
                _ => [(3.0, 21.0), (3.0, 21.0), (3.0, 21.0), (3.0, 14.0)],
            };
            for ((a, b), y) in rows.into_iter().zip([4.5, 9.5, 14.5, 19.5]) {
                p.seg((a, y), (b, y), K);
            }
        }
        I::TextDirection => {
            // A capital A lying on its side.
            p.line_w(&[(17.0, 5.5), (3.0, 12.0), (17.0, 18.5)], K, 1.1);
            p.seg_w((11.7, 8.3), (11.7, 15.7), K, 1.1);
            p.arrow((21.0, 3.5), (21.0, 20.5), B);
        }
        I::AlignText => {
            p.rect(2.5, 3.0, 15.0, 21.0, 1.2, N, K);
            p.seg((5.5, 10.0), (12.0, 10.0), K);
            p.seg((5.5, 14.0), (10.5, 14.0), K);
            p.arrow((19.5, 2.5), (19.5, 9.5), B);
            p.arrow((19.5, 21.5), (19.5, 14.5), B);
        }
        I::ConvertSmartArt => {
            for (y, x1) in [(4.0, 11.0), (8.0, 9.0), (12.0, 11.0)] {
                p.seg((2.5, y), (x1, y), K);
            }
            p.path_arrow(&[(14.0, 6.0), (19.0, 6.0), (19.0, 12.5)], B);
            p.rect(2.5, 15.5, 7.5, 21.5, 1.0, B, N);
            p.rect(9.5, 15.5, 14.5, 21.5, 1.0, O, N);
            p.rect(16.5, 15.5, 21.5, 21.5, 1.0, G, N);
        }

        // ----- Drawing -----
        I::Shapes => {
            p.rect(11.0, 2.5, 21.5, 13.0, 1.0, t(G), G);
            p.rshape(&[(15.25, 11.0), (21.5, 21.5), (9.0, 21.5)], 0.8, t(O), O);
            p.circle((8.0, 9.0), 5.5, t(B), B);
        }
        I::Arrange => {
            p.rect(2.5, 2.5, 11.5, 11.5, 1.0, N, K);
            p.rect(7.5, 7.5, 16.5, 16.5, 1.0, t(B), B);
            p.rect(12.5, 12.5, 21.5, 21.5, 1.0, B, B);
        }
        I::QuickStyles => {
            p.rect(3.0, 3.0, 11.0, 11.0, 1.5, B, N);
            p.rect(13.0, 3.0, 21.0, 11.0, 1.5, N, O);
            p.rect(3.0, 13.0, 11.0, 21.0, 1.5, t(G), G);
            p.rect(13.0, 13.0, 21.0, 21.0, 1.5, PU, N);
        }
        I::ShapeFill => {
            let m = |pts: &[P2]| tf((10.0, 10.0), 45.0, pts);
            p.poly(&m(&[(-5.0, 0.0), (5.0, 0.0), (5.0, 5.0), (-5.0, 5.0)]), B, N);
            p.rshape(&m(&[(-5.0, -5.0), (5.0, -5.0), (5.0, 5.0), (-5.0, 5.0)]), 0.8, N, K);
            p.line(&quad_pts((5.2, 6.4), (5.5, 1.0), (10.0, 3.0), 10), K);
            p.poly(&[(18.5, 8.5), (20.6, 12.6), (16.4, 12.6)], B, N);
            p.dot((18.5, 13.4), 2.1, B);
            p.rect(2.5, 19.5, 21.5, 22.5, 0.8, B, N);
        }
        I::ShapeOutline => {
            p.tool((4.5, 15.5), -45.0, 17.0, 2.4, 4.2, N, K, 0.0, N);
            p.rect(2.5, 19.5, 21.5, 22.5, 0.8, O, N);
        }
        I::ShapeEffects => {
            p.rect(6.0, 8.0, 18.5, 20.5, 2.0, K.a(0.25), N);
            p.rect(3.0, 5.0, 15.5, 17.5, 2.0, B, N);
            p.sparkle((19.0, 5.0), 4.0, Y);
        }
        I::TextFill => {
            p.letter_a(4.5, 2.5, 19.5, 17.0, B, 1.5);
            p.rect(2.5, 19.5, 21.5, 22.5, 0.8, B, N);
        }
        I::TextOutline => {
            let outer = [(3.5, 17.0), (9.6, 2.5), (14.4, 2.5), (20.5, 17.0), (16.6, 17.0), (15.2, 13.2), (8.8, 13.2), (7.4, 17.0)];
            p.shape_w(&outer, N, K, 0.8);
            p.poly_w(&[(10.0, 10.0), (12.0, 5.0), (14.0, 10.0)], N, K, 0.8);
            p.rect(2.5, 19.5, 21.5, 22.5, 0.8, O, N);
        }
        I::TextEffects => {
            p.letter_a(3.0, 5.0, 17.0, 21.0, PU, 1.3);
            p.sparkle((19.0, 5.0), 4.5, Y);
        }
        I::EditShape => {
            let a = (4.0, 19.0);
            let b = (11.0, 4.5);
            let c = (20.0, 17.5);
            let mut pts = vec![a, b];
            pts.extend(cubic_pts(b, (17.0, 4.0), (23.0, 11.0), c, 16));
            pts.extend(cubic_pts(c, (15.0, 22.0), (9.0, 17.0), a, 16));
            p.shape(&pts, t(B), K);
            for q in [a, b, c] {
                p.rect(q.0 - 1.6, q.1 - 1.6, q.0 + 1.6, q.1 + 1.6, 0.3, B, N);
            }
        }
        I::MergeShapes => {
            let mut lens = arc_pts((9.0, 12.0), 6.5, 6.5, -62.5, 62.5);
            lens.extend(arc_pts((15.0, 12.0), 6.5, 6.5, 117.5, 242.5));
            p.poly(&lens, PU, N);
            p.circle((9.0, 12.0), 6.5, N, K);
            p.circle((15.0, 12.0), 6.5, N, K);
        }

        // ----- Insert -----
        I::Table => p.table(2.5, 4.0, 21.5, 20.0, 3, 3, B),
        I::Picture => p.picture(2.0, 4.0, 22.0, 20.0),
        I::Screenshot => {
            for (cx, cy, sx, sy) in [(2.5, 3.5, 1.0, 1.0), (21.5, 3.5, -1.0, 1.0), (2.5, 20.5, 1.0, -1.0), (21.5, 20.5, -1.0, -1.0)] {
                p.rline(&[(cx, cy + 4.0 * sy), (cx, cy), (cx + 4.0 * sx, cy)], 1.0, K);
            }
            p.rect(6.5, 7.5, 17.5, 16.5, 1.0, t(B), B);
            p.seg((6.5, 10.0), (17.5, 10.0), B);
        }
        I::Icons => {
            p.person(6.75, 2.5, 0.75, B);
            p.star((17.0, 7.0), 5.0, Y, N);
            let heart: Vec<P2> = (0..40)
                .map(|i| {
                    let tt = i as f32 / 40.0 * 2.0 * PI;
                    let x = 16.0 * tt.sin().powi(3);
                    let y = 13.0 * tt.cos() - 5.0 * (2.0 * tt).cos() - 2.0 * (3.0 * tt).cos() - (4.0 * tt).cos();
                    (7.0 + x * 0.29, 17.0 - y * 0.29)
                })
                .collect();
            p.shape(&heart, R, N);
            p.rshape(&[(17.0, 12.5), (21.5, 17.0), (17.0, 21.5), (12.5, 17.0)], 0.8, G, N);
        }
        I::Models3d => {
            let (top, ur, lr, bot, ll, ul, mid) = ((12.0, 2.5), (20.5, 7.2), (20.5, 16.8), (12.0, 21.5), (3.5, 16.8), (3.5, 7.2), (12.0, 11.9));
            p.poly(&[top, ur, mid, ul], t(B), N);
            p.poly(&[ul, mid, bot, ll], B.a(0.55), N);
            p.poly(&[mid, ur, lr, bot], B, N);
            p.shape_w(&[top, ur, lr, bot, ll, ul], N, K, 1.0);
            p.line(&[ul, mid, ur], K);
            p.seg(mid, bot, K);
        }
        I::SmartArt => {
            p.rect(8.5, 2.5, 15.5, 8.5, 1.0, G, N);
            p.seg((12.0, 8.5), (12.0, 11.5), K);
            p.rline(&[(5.5, 14.5), (5.5, 11.5), (18.5, 11.5), (18.5, 14.5)], 1.0, K);
            p.rect(2.0, 14.5, 9.0, 20.5, 1.0, B, N);
            p.rect(15.0, 14.5, 22.0, 20.5, 1.0, O, N);
        }
        I::Chart => p.bars(2.5, 4.0, 21.5, 20.5),
        I::Zoom => {
            p.rline(&[(10.5, 15.0), (2.0, 15.0), (2.0, 3.0), (18.0, 3.0), (18.0, 9.5)], 1.5, K);
            p.seg((5.0, 6.5), (11.0, 6.5), K.a(0.5));
            p.magnifier((14.5, 14.5), 4.5, t(B), B);
        }
        I::Link => {
            let link = |c: P2, col: C| {
                let local = rrect_pts(-5.5, -2.8, 5.5, 2.8, 2.8);
                p.shape(&tf(c, -45.0, &local), N, col);
            };
            link((8.5, 15.5), K);
            link((15.5, 8.5), B);
        }
        I::Action => {
            p.rect(2.5, 3.5, 18.0, 12.5, 2.0, t(B), B);
            p.cursor(11.5, 8.5, 0.82);
        }
        I::Comment => {
            p.bubble(2.5, 3.5, 21.5, 16.5, t(Y), K);
            p.seg((6.5, 8.0), (17.5, 8.0), K);
            p.seg((6.5, 12.0), (14.0, 12.0), K);
        }
        I::TextBox => {
            p.rect(3.5, 4.5, 20.5, 19.5, 0.5, N, K);
            for (x, y) in [(3.5, 4.5), (20.5, 4.5), (3.5, 19.5), (20.5, 19.5)] {
                p.rect(x - 1.5, y - 1.5, x + 1.5, y + 1.5, 0.3, B, N);
            }
            p.seg((7.5, 9.5), (16.5, 9.5), K);
            p.seg((7.5, 14.5), (13.5, 14.5), K);
        }
        I::HeaderFooter => {
            p.rect(4.0, 2.5, 20.0, 21.5, 1.2, N, K);
            p.rect(6.5, 5.0, 17.5, 7.5, 0.6, B, N);
            p.rect(6.5, 16.5, 17.5, 19.0, 0.6, B, N);
            p.seg_w((7.0, 10.5), (17.0, 10.5), K.a(0.45), 0.8);
            p.seg_w((7.0, 13.2), (14.5, 13.2), K.a(0.45), 0.8);
        }
        I::WordArt => {
            p.text_rot("A", (13.5, 13.5), 22.0, PU, -10.0);
            p.text_rot("A", (11.5, 11.5), 22.0, O, -10.0);
            p.text_rot("A", (11.3, 11.5), 22.0, O, -10.0);
        }
        I::DateTime => {
            p.rect(3.0, 5.0, 21.0, 21.0, 1.6, N, K);
            p.rect(3.0, 5.0, 21.0, 9.5, 1.6, R, N);
            p.rect(3.0, 5.0, 21.0, 21.0, 1.6, N, K);
            p.seg((8.0, 2.8), (8.0, 6.5), K);
            p.seg((16.0, 2.8), (16.0, 6.5), K);
            for (i, (x, y)) in [(8.0, 13.5), (12.0, 13.5), (16.0, 13.5), (8.0, 17.5), (12.0, 17.5), (16.0, 17.5)].into_iter().enumerate() {
                p.dot((x, y), 1.1, if i == 4 { B } else { K });
            }
        }
        I::SlideNumber => {
            p.frame(2.0, 4.0, 22.0, 20.0);
            p.seg((10.7, 7.5), (9.7, 16.5), B);
            p.seg((15.0, 7.5), (14.0, 16.5), B);
            p.seg((7.5, 10.5), (17.0, 10.5), B);
            p.seg((7.0, 13.6), (16.5, 13.6), B);
        }
        I::Object => {
            p.page(3.0, 2.0, 16.0, 19.0, N, K);
            p.rect(11.5, 11.5, 21.5, 21.5, 1.6, B, N);
            p.rect(14.2, 14.2, 18.8, 18.8, 0.6, N, W);
        }
        I::Equation => {
            p.line_w(&[(18.5, 6.5), (18.0, 4.0), (6.0, 4.0), (12.5, 12.0), (6.0, 20.0), (18.0, 20.0), (18.5, 17.5)], B, 1.15);
        }
        I::Symbol => p.text("Ω", (12.0, 12.0), 22.0, K),
        I::Video => {
            p.rect(2.0, 6.0, 15.5, 18.0, 2.0, t(B), K);
            p.rshape(&[(16.5, 10.5), (22.0, 7.0), (22.0, 17.0), (16.5, 13.5)], 0.6, N, K);
            p.dot((6.0, 10.0), 1.2, R);
        }
        I::Audio => {
            p.speaker(2.5, 12.0, t(PU), K);
            p.arc((11.0, 12.0), 4.5, -45.0, 45.0, PU);
            p.arc((11.0, 12.0), 8.5, -45.0, 45.0, PU);
        }
        I::ScreenRecording => {
            p.monitor(2.0, 3.0, 22.0, 16.5);
            p.circle((12.0, 9.75), 3.0, R, N);
        }
        I::Cameo => {
            p.frame(2.0, 4.0, 22.0, 20.0);
            p.circle((12.0, 12.0), 5.6, t(B), B);
            p.dot((12.0, 10.4), 1.9, B);
            p.poly(&arc_pts((12.0, 16.0), 3.4, 2.6, 180.0, 360.0), B, N);
        }

        // ----- Draw tab -----
        I::Pen => {
            p.tool((3.5, 20.5), -45.0, 23.0, 2.2, 5.0, B, K, 3.5, K);
        }
        I::Pencil => {
            p.tool((3.5, 20.5), -45.0, 22.0, 2.6, 5.0, Y, K, 3.5, R);
        }
        I::HighlighterPen => {
            let m = |pts: &[P2]| tf((6.0, 15.0), -45.0, pts);
            p.poly(&m(&[(0.0, -1.0), (3.0, -3.2), (3.0, 3.2), (0.0, 2.2)]), Y, K);
            p.poly(&m(&[(3.0, -3.6), (14.5, -3.6), (14.5, 3.6), (3.0, 3.6)]), t(Y), K);
            p.rect(2.0, 19.5, 13.0, 22.5, 0.8, Y, N);
        }
        I::Eraser => {
            let m = |pts: &[P2]| tf((12.5, 11.0), -45.0, pts);
            p.poly(&m(&[(-8.0, -4.0), (-1.0, -4.0), (-1.0, 4.0), (-8.0, 4.0)]), R, N);
            p.rshape(&m(&[(-8.0, -4.0), (8.0, -4.0), (8.0, 4.0), (-8.0, 4.0)]), 1.0, N, K);
            p.seg((12.0, 21.0), (21.0, 21.0), K);
        }
        I::LassoSelect => {
            let mut e = arc_pts((13.0, 9.0), 8.5, 6.0, 120.0, 480.0);
            e.truncate(e.len().saturating_sub(1));
            p.dashed(&e, B, 2.6, 1.7);
            p.curve((8.8, 14.2), (5.5, 16.5), (10.0, 19.0), (5.5, 22.0), K);
            p.dot((8.8, 14.2), 1.4, K);
        }
        I::InkToShape => {
            p.scribble(1.5, 2.0, 12.5, 11.0, K);
            p.circle((16.5, 16.5), 5.0, t(B), B);
        }
        I::InkToText => {
            p.scribble(1.5, 2.0, 12.5, 11.0, K);
            p.letter_a(12.0, 9.0, 21.5, 21.5, B, 1.2);
        }
        I::InkToMath => {
            p.scribble(1.5, 2.0, 12.5, 11.0, K);
            p.line_w(&[(21.0, 11.0), (12.5, 11.0), (17.0, 16.25), (12.5, 21.5), (21.0, 21.5)], B, 1.1);
        }
        I::Ruler => {
            let m = |pts: &[P2]| tf((12.0, 12.0), -45.0, pts);
            p.rshape(&m(&[(-11.5, -4.0), (11.5, -4.0), (11.5, 4.0), (-11.5, 4.0)]), 0.8, Y.a(0.85), K);
            for (i, u) in [-8.0_f32, -5.0, -2.0, 1.0, 4.0, 7.0].into_iter().enumerate() {
                let l = if i % 2 == 0 { 3.2 } else { 1.8 };
                p.line_w(&m(&[(u, -4.0), (u, -4.0 + l)]), D, 0.8);
            }
        }

        // ----- Design -----
        I::Themes => {
            p.rect(9.5, 2.5, 21.5, 14.5, 2.0, O, N);
            p.rect(2.5, 8.5, 15.0, 21.5, 2.0, B, N);
            p.text("Aa", (8.75, 15.0), 10.0, W);
        }
        I::Variants => {
            p.rect(8.0, 2.5, 21.5, 11.0, 1.2, t(PU), PU);
            p.rect(5.0, 7.0, 18.5, 15.5, 1.2, t(G), G);
            p.rect(2.5, 12.0, 16.0, 21.0, 1.2, B, B);
        }
        I::Colors => {
            for (i, c) in [R, Y, G, B].into_iter().enumerate() {
                let x = 3.0 + i as f32 * 4.5;
                p.rect(x, 4.0, x + 4.5, 20.0, 0.0, c, N);
            }
            p.rect(3.0, 4.0, 21.0, 20.0, 0.0, N, K);
        }
        I::Fonts => {
            p.text("A", (8.0, 11.0), 20.0, K);
            p.text("g", (17.0, 10.0), 18.0, B);
        }
        I::BackgroundStyles => {
            p.poly(&[(2.7, 4.7), (21.3, 4.7), (2.7, 19.3)], t(B), N);
            p.poly(&[(21.3, 4.7), (21.3, 19.3), (2.7, 19.3)], PU.a(0.7), N);
            p.frame(2.0, 4.0, 22.0, 20.0);
        }
        I::SlideSize => {
            p.rline(&[(17.0, 12.0), (17.0, 19.0), (2.5, 19.0), (2.5, 7.0), (10.0, 7.0)], 1.5, K);
            p.arrow((9.0, 13.0), (21.5, 2.5), B);
        }
        I::FormatBackground => {
            p.rline(&[(10.0, 17.0), (2.0, 17.0), (2.0, 4.0), (20.0, 4.0), (20.0, 9.0)], 1.5, K);
            p.rect(4.5, 6.5, 13.5, 12.5, 0.6, t(PU), N);
            p.brush((12.0, 20.5), -45.0, 13.0, Y);
        }
        I::DesignIdeas => {
            p.circle((12.0, 9.5), 6.0, Y.a(0.5), K);
            p.seg((9.6, 15.7), (9.6, 17.0), K);
            p.seg((14.4, 15.7), (14.4, 17.0), K);
            p.seg((9.5, 18.0), (14.5, 18.0), K);
            p.seg((10.5, 21.0), (13.5, 21.0), K);
            for a in [-160.0_f32, -125.0, -55.0, -20.0] {
                let (s, c) = rad(a).sin_cos();
                p.seg((12.0 + c * 8.0, 9.5 + s * 8.0), (12.0 + c * 10.0, 9.5 + s * 10.0), Y);
            }
        }

        // ----- Transitions / animations -----
        I::Preview => {
            p.circle((12.0, 12.0), 9.5, N, K);
            p.rshape(&[(9.5, 7.0), (17.0, 12.0), (9.5, 17.0)], 0.8, G, N);
        }
        I::EffectOptions => {
            p.rect(2.5, 8.5, 15.5, 21.5, 1.5, t(B), K);
            p.arrow((9.0, 15.0), (21.5, 2.5), B);
        }
        I::Sound => {
            let head = |c: P2| {
                let pts = tf(c, -22.0, &arc_pts((0.0, 0.0), 3.0, 2.1, 0.0, 351.0));
                p.poly(&pts, PU, N);
            };
            head((6.3, 18.0));
            head((16.3, 15.5));
            p.seg((9.0, 17.0), (9.0, 5.0), PU);
            p.seg((19.0, 14.5), (19.0, 2.5), PU);
            p.poly(&[(9.0, 4.5), (19.0, 2.0), (19.0, 5.5), (9.0, 8.0)], PU, N);
        }
        I::Duration => {
            p.circle((12.0, 13.5), 8.0, N, K);
            p.rect(10.0, 2.0, 14.0, 4.0, 0.6, K, N);
            p.seg((12.0, 4.0), (12.0, 5.5), K);
            p.seg((12.0, 13.5), (15.8, 9.0), B);
            p.dot((12.0, 13.5), 1.3, B);
        }
        I::ApplyToAll => {
            p.rline(&[(6.0, 7.5), (6.0, 3.0), (21.5, 3.0), (21.5, 13.5), (17.5, 13.5)], 1.5, K);
            p.rect(2.5, 7.5, 17.5, 18.0, 1.5, N, K);
            p.badge((18.0, 18.0), 4.8, G, Glyph::Check);
        }
        I::AnimEntrance => {
            p.star((13.5, 12.5), 8.5, G, N);
            p.seg((1.5, 9.0), (4.5, 9.0), K);
            p.seg((0.8, 13.5), (4.0, 13.5), K);
        }
        I::AnimEmphasis => {
            p.star((12.0, 12.5), 9.5, Y, N);
        }
        I::AnimExit => {
            p.star((10.5, 12.5), 8.5, R, N);
            p.seg((19.5, 9.0), (22.5, 9.0), K);
            p.seg((20.0, 13.5), (23.2, 13.5), K);
        }
        I::AnimPath => {
            let pts = cubic_pts((4.5, 19.5), (3.0, 6.0), (20.0, 19.0), (19.5, 5.0), 60);
            p.dotted(&pts[6..pts.len().saturating_sub(4)], K, 2.4, 0.55);
            p.head((19.5, 4.0), -90.0, 4.0, K);
            p.circle((4.5, 19.5), 2.4, G, N);
        }
        I::AnimationPane => {
            p.rect(2.5, 3.0, 21.5, 21.0, 1.5, N, K);
            p.seg((11.0, 3.0), (11.0, 21.0), K);
            p.rect(4.5, 7.0, 8.5, 12.0, 0.5, t(B), N);
            for (y, c) in [(7.5, G), (12.0, Y), (16.5, R)] {
                p.seg_w((13.5, y), (19.0, y), c, 1.4);
            }
        }
        I::Trigger => {
            p.rshape(&[(14.5, 2.0), (5.0, 13.5), (11.5, 13.5), (9.5, 22.0), (19.0, 9.5), (12.5, 9.5)], 0.6, Y, K);
        }
        I::AnimationPainter => {
            p.star((7.5, 7.5), 6.0, G, N);
            p.brush((12.0, 12.0), 45.0, 13.0, Y);
        }
        I::AddAnimation => {
            p.star((10.5, 11.0), 9.0, Y, N);
            p.badge((18.5, 18.5), 4.8, G, Glyph::Plus);
        }

        // ----- Slide show -----
        I::PlayFromStart => {
            p.frame(2.0, 4.0, 22.0, 20.0);
            p.rect(7.0, 8.0, 9.2, 16.0, 0.5, G, N);
            p.rshape(&[(11.0, 8.0), (17.5, 12.0), (11.0, 16.0)], 0.6, G, N);
        }
        I::PlayFromCurrent => {
            p.frame(2.0, 4.0, 22.0, 20.0);
            p.rshape(&[(9.5, 8.0), (16.0, 12.0), (9.5, 16.0)], 0.6, G, N);
        }
        I::PresenterView => {
            p.monitor(2.0, 3.0, 22.0, 16.5);
            p.rect(4.5, 5.5, 13.0, 12.0, 0.6, t(B), B);
            p.seg_w((15.0, 6.5), (19.5, 6.5), K, 0.8);
            p.seg_w((15.0, 9.5), (19.5, 9.5), K, 0.8);
            p.seg_w((15.0, 12.5), (18.0, 12.5), K, 0.8);
        }
        I::CustomShow => {
            p.rect(2.5, 2.5, 12.5, 8.0, 1.0, N, K);
            p.rect(2.5, 9.5, 12.5, 15.0, 1.0, O, O);
            p.rect(2.5, 16.5, 12.5, 22.0, 1.0, N, K);
            p.rshape(&[(15.5, 8.0), (21.5, 12.25), (15.5, 16.5)], 0.6, G, N);
        }
        I::SetUpShow => {
            p.frame(2.0, 4.0, 19.5, 16.5);
            p.seg((5.0, 8.0), (13.0, 8.0), K.a(0.5));
            p.gear_badge((17.5, 17.5), 5.5, B);
        }
        I::RehearseTimings => {
            p.frame(2.0, 4.0, 19.5, 16.5);
            p.seg((5.0, 8.0), (13.0, 8.0), K.a(0.5));
            p.clock_badge((17.5, 17.5), 5.2, B);
        }
        I::Record => {
            p.circle((12.0, 12.0), 9.5, N, K);
            p.circle((12.0, 12.0), 5.5, R, N);
        }
        I::Subtitles => {
            p.rect(2.0, 4.5, 22.0, 19.5, 2.0, N, K);
            p.seg((5.5, 12.0), (11.0, 12.0), B);
            p.seg((13.5, 12.0), (18.5, 12.0), B);
            p.seg((5.5, 16.0), (8.5, 16.0), B);
            p.seg((11.0, 16.0), (18.5, 16.0), B);
        }
        I::Coach => {
            p.person(9.0, 5.0, 1.2, t(B));
            p.circle((9.0, 8.1), 3.1, N, B);
            p.line(&arc_pts((9.0, 19.4), 6.2, 6.0, 180.0, 360.0), B);
            p.star((18.5, 6.0), 4.2, Y, N);
        }

        // ----- Review -----
        I::Spelling => {
            p.text("abc", (11.5, 7.0), 12.0, K);
            p.line_w(&[(6.0, 16.0), (10.0, 20.0), (19.5, 11.0)], G, 1.3);
        }
        I::Thesaurus => {
            p.rect(8.0, 2.5, 19.5, 21.5, 1.2, t(B), N);
            p.rect(4.5, 2.5, 19.5, 21.5, 1.5, N, K);
            p.seg((8.0, 2.5), (8.0, 21.5), K);
            p.poly(&[(13.5, 2.5), (17.0, 2.5), (17.0, 9.0), (15.25, 7.5), (13.5, 9.0)], R, N);
        }
        I::Accessibility => {
            p.circle((12.0, 12.0), 9.8, N, B);
            p.dot((12.0, 6.5), 1.6, B);
            p.seg((6.8, 9.6), (17.2, 9.6), B);
            p.seg((12.0, 9.6), (12.0, 13.8), B);
            p.line(&[(9.0, 18.5), (12.0, 13.8), (15.0, 18.5)], B);
        }
        I::Translate => {
            p.rect(10.0, 9.0, 21.5, 20.5, 2.0, O, N);
            p.rect(2.5, 3.5, 14.0, 15.0, 2.0, B, N);
            p.letter_a(5.5, 5.8, 11.0, 12.8, W, 0.9);
            p.seg_w((17.75, 10.8), (17.75, 11.9), W, 0.8);
            p.seg_w((15.2, 12.9), (20.3, 12.9), W, 0.8);
            p.seg_w((15.6, 14.4), (19.9, 18.9), W, 0.8);
            p.seg_w((19.9, 14.4), (15.6, 18.9), W, 0.8);
        }
        I::Language => {
            p.circle((12.0, 12.0), 9.5, N, B);
            p.ellipse((12.0, 12.0), 4.2, 9.5, N, B);
            p.seg_w((2.5, 12.0), (21.5, 12.0), B, 0.8);
            p.seg_w((4.3, 7.0), (19.7, 7.0), B, 0.8);
            p.seg_w((4.3, 17.0), (19.7, 17.0), B, 0.8);
        }
        I::NewComment => {
            p.bubble(2.0, 3.0, 19.5, 15.0, N, K);
            p.badge((18.0, 17.5), 5.0, G, Glyph::Plus);
        }
        I::DeleteComment => {
            p.bubble(2.0, 3.0, 19.5, 15.0, N, K);
            p.badge((18.0, 17.5), 5.0, R, Glyph::Cross);
        }
        I::PrevComment => {
            p.bubble(2.5, 3.0, 21.5, 17.0, N, K);
            p.arrow((16.5, 10.0), (7.0, 10.0), B);
        }
        I::NextComment => {
            p.bubble(2.5, 3.0, 21.5, 17.0, N, K);
            p.arrow((7.5, 10.0), (17.0, 10.0), B);
        }
        I::ShowComments => {
            p.rline(&[(8.0, 7.5), (8.0, 2.5), (21.5, 2.5), (21.5, 13.0), (17.0, 13.0)], 2.0, K);
            p.bubble(2.5, 7.5, 16.5, 17.5, t(Y), K);
            p.seg((5.5, 11.0), (13.5, 11.0), K);
            p.seg((5.5, 14.2), (11.0, 14.2), K);
        }
        I::Compare => {
            p.rect(2.5, 2.5, 11.0, 15.5, 1.2, N, K);
            p.rect(13.0, 8.5, 21.5, 21.5, 1.2, t(B), B);
            p.arrow((13.0, 4.0), (21.5, 4.0), K);
            p.arrow((11.0, 20.0), (2.5, 20.0), K);
        }
        I::ReadOnly => {
            p.page(3.0, 2.0, 16.0, 20.5, N, K);
            p.lock(13.0, 15.0, 21.5, 22.0);
        }

        // ----- View -----
        I::ViewNormal => {
            p.rect(2.0, 3.5, 22.0, 20.5, 1.5, N, K);
            p.seg((8.0, 3.5), (8.0, 20.5), K);
            for y in [6.0, 10.5, 15.0] {
                p.rect(3.9, y, 6.2, y + 2.6, 0.3, K.a(0.55), N);
            }
            p.rect(10.0, 6.0, 20.0, 15.0, 0.6, t(B), B);
            p.seg_w((10.0, 18.0), (17.0, 18.0), K.a(0.5), 0.8);
        }
        I::ViewOutline => {
            p.rect(2.0, 3.5, 22.0, 20.5, 1.5, N, K);
            p.seg((11.0, 3.5), (11.0, 20.5), K);
            p.seg_w((4.0, 7.0), (9.0, 7.0), K, 0.8);
            p.seg_w((5.5, 10.0), (9.0, 10.0), K, 0.8);
            p.seg_w((5.5, 13.0), (9.0, 13.0), K, 0.8);
            p.seg_w((4.0, 16.0), (9.0, 16.0), K, 0.8);
            p.rect(13.0, 7.0, 20.0, 13.0, 0.6, t(B), B);
        }
        I::ViewSorter => {
            for (i, (x0, x1)) in [(2.5, 11.0), (13.0, 21.5)].into_iter().enumerate() {
                for (j, (y0, y1)) in [(4.0, 10.5), (13.5, 20.0)].into_iter().enumerate() {
                    if i == 0 && j == 0 {
                        p.rect(x0, y0, x1, y1, 0.8, t(B), B);
                    } else {
                        p.rect(x0, y0, x1, y1, 0.8, N, K);
                    }
                }
            }
        }
        I::ViewNotesPage => {
            p.rect(5.0, 2.0, 19.0, 22.0, 1.2, N, K);
            p.rect(7.5, 4.5, 16.5, 10.5, 0.6, t(B), B);
            p.seg_w((7.5, 14.0), (16.5, 14.0), K, 0.8);
            p.seg_w((7.5, 17.5), (14.0, 17.5), K, 0.8);
        }
        I::ViewReading => {
            p.book(t(B), K);
            p.seg((12.0, 7.0), (12.0, 20.5), K);
        }
        I::SlideMaster => {
            p.rect(2.0, 2.5, 15.0, 10.5, 1.2, t(PU), PU);
            p.seg((4.5, 5.5), (11.0, 5.5), PU);
            p.rline(&[(5.0, 10.5), (5.0, 19.5), (9.0, 19.5)], 1.0, K);
            p.seg((5.0, 14.0), (9.0, 14.0), K);
            p.rect(9.0, 12.0, 17.0, 16.0, 0.8, N, K);
            p.rect(9.0, 17.5, 17.0, 21.5, 0.8, N, K);
        }
        I::HandoutMaster => {
            p.rect(4.0, 2.0, 20.0, 22.0, 1.2, N, K);
            for (x, y) in [(6.5, 4.5), (12.75, 4.5), (6.5, 11.0), (12.75, 11.0)] {
                p.rect(x, y, x + 4.75, y + 4.5, 0.4, t(PU), PU);
            }
            p.seg_w((6.5, 19.0), (17.5, 19.0), K, 0.8);
        }
        I::NotesMaster => {
            p.rline(&[(8.0, 4.0), (8.0, 2.0), (21.0, 2.0), (21.0, 19.0), (16.0, 19.0)], 1.2, PU);
            p.rect(3.0, 4.5, 16.0, 22.0, 1.2, N, K);
            p.rect(5.5, 7.0, 13.5, 12.0, 0.5, t(PU), PU);
            p.seg_w((5.5, 15.5), (13.5, 15.5), K, 0.8);
            p.seg_w((5.5, 18.8), (11.5, 18.8), K, 0.8);
        }
        I::Gridlines => {
            for v in [9.0, 15.0] {
                p.seg_w((v, 3.0), (v, 21.0), K.a(0.6), 0.7);
                p.seg_w((3.0, v), (21.0, v), K.a(0.6), 0.7);
            }
            p.rect(3.0, 3.0, 21.0, 21.0, 1.2, N, K);
        }
        I::Guides => {
            p.rect(3.0, 3.0, 21.0, 21.0, 1.2, N, K);
            p.dashed(&[(12.0, 1.5), (12.0, 22.5)], B, 2.2, 1.6);
            p.dashed(&[(1.5, 12.0), (22.5, 12.0)], B, 2.2, 1.6);
        }
        I::Notes => {
            p.rect(4.0, 4.0, 20.0, 21.5, 1.5, t(Y), K);
            for x in [8.0, 12.0, 16.0] {
                p.seg((x, 2.3), (x, 5.8), K);
            }
            p.seg_w((7.5, 10.5), (16.5, 10.5), K, 0.85);
            p.seg_w((7.5, 14.0), (16.5, 14.0), K, 0.85);
            p.seg_w((7.5, 17.5), (13.5, 17.5), K, 0.85);
        }
        I::ZoomGlass => p.magnifier((10.0, 10.0), 7.0, t(B), K),
        I::FitToWindow => {
            for (cx, cy, sx, sy) in [(2.5, 2.5, 1.0, 1.0), (21.5, 2.5, -1.0, 1.0), (2.5, 21.5, 1.0, -1.0), (21.5, 21.5, -1.0, -1.0)] {
                p.rline(&[(cx, cy + 5.0 * sy), (cx, cy), (cx + 5.0 * sx, cy)], 0.8, K);
            }
            p.rect(7.0, 8.0, 17.0, 16.0, 1.0, t(B), B);
        }
        I::Grayscale => {
            p.rect(3.0, 3.0, 9.0, 21.0, 0.0, K.a(0.85), N);
            p.rect(9.0, 3.0, 15.0, 21.0, 0.0, K.a(0.45), N);
            p.rect(15.0, 3.0, 21.0, 21.0, 0.0, K.a(0.15), N);
            p.rect(3.0, 3.0, 21.0, 21.0, 1.2, N, K);
        }
        I::NewWindow => {
            p.rect(2.0, 3.5, 20.0, 17.0, 1.5, N, K);
            p.seg((2.0, 7.0), (20.0, 7.0), K);
            p.dot((4.5, 5.25), 0.6, K);
            p.badge((18.5, 18.0), 4.8, G, Glyph::Plus);
        }
        I::Macros => {
            p.page(3.0, 2.0, 17.0, 21.0, N, K);
            p.line(&[(8.5, 9.0), (6.0, 11.5), (8.5, 14.0)], B);
            p.line(&[(11.5, 9.0), (14.0, 11.5), (11.5, 14.0)], B);
            p.gear_badge((17.5, 17.5), 5.2, O);
        }

        // ----- Arrange -----
        I::BringForward => {
            p.layer(10.0, 15.5, 8.0, 4.0, N, K);
            p.layer(10.0, 8.5, 8.0, 4.0, B, B);
            p.arrow((21.0, 13.0), (21.0, 3.0), K);
        }
        I::SendBackward => {
            p.layer(10.0, 15.5, 8.0, 4.0, B, B);
            p.layer(10.0, 8.5, 8.0, 4.0, N, K);
            p.arrow((21.0, 11.0), (21.0, 21.0), K);
        }
        I::BringToFront => {
            p.layer(10.0, 18.5, 7.5, 3.2, N, K);
            p.layer(10.0, 12.0, 7.5, 3.2, N, K);
            p.layer(10.0, 5.5, 7.5, 3.2, B, B);
            p.arrow((20.5, 14.0), (20.5, 4.5), K);
            p.seg((18.0, 2.5), (23.0, 2.5), K);
        }
        I::SendToBack => {
            p.layer(10.0, 18.5, 7.5, 3.2, B, B);
            p.layer(10.0, 12.0, 7.5, 3.2, N, K);
            p.layer(10.0, 5.5, 7.5, 3.2, N, K);
            p.arrow((20.5, 10.0), (20.5, 19.5), K);
            p.seg((18.0, 21.5), (23.0, 21.5), K);
        }
        I::Group => {
            p.dashed(&[(2.5, 2.5), (21.5, 2.5), (21.5, 21.5), (2.5, 21.5), (2.5, 2.5)], B, 2.0, 1.6);
            p.rect(5.5, 5.5, 13.0, 13.0, 0.8, t(G), G);
            p.circle((15.5, 15.5), 4.3, t(O), O);
        }
        I::Ungroup => {
            p.dashed_w(&[(2.0, 2.0), (12.0, 2.0), (12.0, 12.0), (2.0, 12.0), (2.0, 2.0)], B, 1.6, 1.3, 0.8);
            p.rect(4.5, 4.5, 9.5, 9.5, 0.6, t(G), G);
            p.dashed_w(&[(12.0, 12.0), (22.0, 12.0), (22.0, 22.0), (12.0, 22.0), (12.0, 12.0)], B, 1.6, 1.3, 0.8);
            p.circle((17.0, 17.0), 2.9, t(O), O);
        }
        I::AlignObjects => {
            p.dashed(&[(2.0, 12.0), (22.0, 12.0)], K, 2.0, 1.5);
            p.rect(3.5, 4.0, 8.0, 20.0, 0.8, B, N);
            p.rect(10.0, 7.5, 14.0, 16.5, 0.8, O, N);
            p.rect(16.0, 5.5, 20.5, 18.5, 0.8, G, N);
        }
        I::AlignObjLeft => {
            p.rect(3.5, 5.0, 19.5, 10.5, 0.8, t(B), B);
            p.rect(3.5, 13.5, 13.5, 19.0, 0.8, t(B), B);
            p.seg((3.5, 2.5), (3.5, 21.5), K);
        }
        I::AlignObjCenter => {
            p.rect(4.0, 5.0, 20.0, 10.5, 0.8, t(B), B);
            p.rect(7.0, 13.5, 17.0, 19.0, 0.8, t(B), B);
            p.seg((12.0, 2.5), (12.0, 21.5), K);
        }
        I::AlignObjRight => {
            p.rect(4.5, 5.0, 20.5, 10.5, 0.8, t(B), B);
            p.rect(10.5, 13.5, 20.5, 19.0, 0.8, t(B), B);
            p.seg((20.5, 2.5), (20.5, 21.5), K);
        }
        I::AlignObjTop => {
            p.rect(5.0, 3.5, 10.5, 19.5, 0.8, t(B), B);
            p.rect(13.5, 3.5, 19.0, 13.5, 0.8, t(B), B);
            p.seg((2.5, 3.5), (21.5, 3.5), K);
        }
        I::AlignObjMiddle => {
            p.rect(5.0, 4.0, 10.5, 20.0, 0.8, t(B), B);
            p.rect(13.5, 7.0, 19.0, 17.0, 0.8, t(B), B);
            p.seg((2.5, 12.0), (21.5, 12.0), K);
        }
        I::AlignObjBottom => {
            p.rect(5.0, 4.5, 10.5, 20.5, 0.8, t(B), B);
            p.rect(13.5, 10.5, 19.0, 20.5, 0.8, t(B), B);
            p.seg((2.5, 20.5), (21.5, 20.5), K);
        }
        I::DistributeH => {
            p.seg((2.5, 3.0), (2.5, 21.0), K);
            p.seg((21.5, 3.0), (21.5, 21.0), K);
            p.rect(4.9, 7.0, 8.2, 17.0, 0.6, t(B), B);
            p.rect(10.35, 5.0, 13.65, 19.0, 0.6, t(B), B);
            p.rect(15.8, 8.5, 19.1, 15.5, 0.6, t(B), B);
        }
        I::DistributeV => {
            p.seg((3.0, 2.5), (21.0, 2.5), K);
            p.seg((3.0, 21.5), (21.0, 21.5), K);
            p.rect(7.0, 4.9, 17.0, 8.2, 0.6, t(B), B);
            p.rect(5.0, 10.35, 19.0, 13.65, 0.6, t(B), B);
            p.rect(8.5, 15.8, 15.5, 19.1, 0.6, t(B), B);
        }
        I::RotateLeft => {
            p.poly(&[(3.0, 21.5), (3.0, 13.0), (12.0, 21.5)], t(B), B);
            p.arc_arrow((13.0, 12.0), 7.5, 30.0, -160.0, K);
        }
        I::RotateRight => {
            p.poly(&[(21.0, 21.5), (21.0, 13.0), (12.0, 21.5)], t(B), B);
            p.arc_arrow((11.0, 12.0), 7.5, 150.0, 340.0, K);
        }
        I::FlipH => {
            p.dashed(&[(12.0, 2.0), (12.0, 22.0)], K, 1.8, 1.4);
            p.poly(&[(9.5, 5.0), (9.5, 19.0), (2.5, 19.0)], N, K);
            p.poly(&[(14.5, 5.0), (21.5, 19.0), (14.5, 19.0)], B, B);
        }
        I::FlipV => {
            p.dashed(&[(2.0, 12.0), (22.0, 12.0)], K, 1.8, 1.4);
            p.poly(&[(5.0, 9.5), (19.0, 9.5), (19.0, 2.5)], N, K);
            p.poly(&[(5.0, 14.5), (19.0, 21.5), (19.0, 14.5)], B, B);
        }
        I::SelectionPane => {
            p.rect(3.0, 2.5, 21.0, 21.5, 1.5, N, K);
            p.rect(4.6, 4.6, 19.4, 9.4, 0.8, t(B), N);
            for (y, c) in [(7.0, B), (12.0, O), (17.0, G)] {
                p.rect(6.0, y - 1.3, 8.6, y + 1.3, 0.4, c, N);
                p.seg_w((10.5, y), (17.5, y), K, 0.85);
            }
        }
        I::Rotate => {
            p.rect(9.0, 9.0, 15.0, 15.0, 0.8, t(B), B);
            p.arc_arrow((12.0, 12.0), 8.5, -70.0, 220.0, K);
        }

        // ----- Picture -----
        I::Crop => {
            p.rline_w(&[(6.5, 2.0), (6.5, 17.5), (22.0, 17.5)], 0.6, K, 1.25);
            p.rline_w(&[(2.0, 6.5), (17.5, 6.5), (17.5, 22.0)], 0.6, B, 1.25);
        }
        I::Corrections => {
            p.poly(&arc_pts((12.0, 12.0), 5.2, 5.2, -90.0, 90.0), Y, N);
            p.circle((12.0, 12.0), 5.2, N, K);
            for i in 0..8 {
                let (s, c) = rad(i as f32 * 45.0).sin_cos();
                p.seg((12.0 + c * 7.6, 12.0 + s * 7.6), (12.0 + c * 9.8, 12.0 + s * 9.8), Y);
            }
        }
        I::ColorAdjust => {
            p.circle((12.0, 8.0), 5.6, R.a(0.6), N);
            p.circle((8.3, 14.6), 5.6, G.a(0.6), N);
            p.circle((15.7, 14.6), 5.6, B.a(0.6), N);
        }
        I::ArtisticEffects => {
            p.rect(2.0, 4.0, 22.0, 20.0, 1.5, N, K);
            p.curve_w((5.5, 16.0), (8.0, 4.5), (14.0, 21.0), (18.5, 8.0), PU, 2.0);
        }
        I::Transparency => {
            let cs = 13.0 / 4.0;
            for i in 0..4 {
                for j in 0..4 {
                    if (i + j) % 2 == 0 {
                        let (x, y) = (3.0 + i as f32 * cs, 3.0 + j as f32 * cs);
                        p.rect(x, y, x + cs, y + cs, 0.0, K.a(0.3), N);
                    }
                }
            }
            p.rect(3.0, 3.0, 16.0, 16.0, 0.6, N, K.a(0.6));
            p.rect(9.0, 9.0, 21.0, 21.0, 1.2, B.a(0.55), B);
        }
        I::CompressPictures => {
            p.picture(6.5, 6.0, 17.5, 18.0);
            p.arrow((0.8, 12.0), (5.0, 12.0), K);
            p.arrow((23.2, 12.0), (19.0, 12.0), K);
        }
        I::ChangePicture => {
            p.picture(2.0, 3.0, 18.0, 15.5);
            let c = (17.5, 17.5);
            if p.dis {
                p.circle(c, 5.0, N, G);
            } else {
                p.circle(c, 5.0, G, N);
            }
            let wc = if p.dis { K } else { W };
            p.arc_arrow(c, 2.8, 200.0, 330.0, wc);
            p.arc_arrow(c, 2.8, 20.0, 150.0, wc);
        }
        I::ResetPicture => {
            p.picture(2.0, 3.0, 18.0, 15.5);
            p.arc_arrow((17.0, 17.5), 4.0, -10.0, -260.0, B);
        }
        I::RemoveBackground => {
            p.rect(2.0, 4.0, 22.0, 20.0, 1.5, PU.a(0.55), N);
            p.dot((12.0, 10.0), 3.0, B);
            p.poly(&arc_pts((12.0, 19.3), 6.0, 5.5, 180.0, 360.0), B, N);
            p.rect(2.0, 4.0, 22.0, 20.0, 1.5, N, K);
        }
        I::AltText => {
            p.picture(2.0, 2.0, 15.5, 12.5);
            p.seg((2.5, 16.5), (21.5, 16.5), K);
            p.seg((2.5, 20.5), (15.0, 20.5), K);
        }

        // ----- Table -----
        I::InsertAbove | I::InsertBelow | I::InsertLeft | I::InsertRight => {
            let (grid, add): ((f32, f32, f32, f32, usize, usize), (f32, f32, f32, f32)) = match icon {
                I::InsertAbove => ((3.0, 10.0, 21.0, 21.0, 2, 3), (3.0, 2.5, 21.0, 7.5)),
                I::InsertBelow => ((3.0, 3.0, 21.0, 14.0, 2, 3), (3.0, 16.5, 21.0, 21.5)),
                I::InsertLeft => ((10.0, 3.0, 21.0, 21.0, 3, 2), (2.5, 3.0, 7.5, 21.0)),
                _ => ((3.0, 3.0, 14.0, 21.0, 3, 2), (16.5, 3.0, 21.5, 21.0)),
            };
            p.table(grid.0, grid.1, grid.2, grid.3, grid.4, grid.5, N);
            p.rect(add.0, add.1, add.2, add.3, 1.0, t(G), G);
            let c = ((add.0 + add.2) * 0.5, (add.1 + add.3) * 0.5);
            p.seg_w((c.0 - 1.6, c.1), (c.0 + 1.6, c.1), G, 0.9);
            p.seg_w((c.0, c.1 - 1.6), (c.0, c.1 + 1.6), G, 0.9);
        }
        I::DeleteTable => {
            p.table(2.5, 3.0, 19.5, 17.0, 3, 3, B);
            p.badge((18.0, 18.0), 5.0, R, Glyph::Cross);
        }
        I::MergeCells => {
            p.rect(2.5, 6.0, 21.5, 18.0, 1.2, N, K);
            p.dashed(&[(12.0, 6.0), (12.0, 18.0)], K.a(0.6), 1.5, 1.2);
            p.arrow((4.5, 12.0), (10.2, 12.0), B);
            p.arrow((19.5, 12.0), (13.8, 12.0), B);
        }
        I::SplitCells => {
            p.rect(2.5, 6.0, 21.5, 18.0, 1.2, N, K);
            p.seg((12.0, 6.0), (12.0, 18.0), K);
            p.arrow((10.0, 12.0), (4.5, 12.0), B);
            p.arrow((14.0, 12.0), (19.5, 12.0), B);
        }
        I::Borders => {
            p.dotted(&[(12.0, 5.0), (12.0, 19.0)], K, 2.2, 0.45);
            p.dotted(&[(5.0, 12.0), (19.0, 12.0)], K, 2.2, 0.45);
            p.rect(3.0, 3.0, 21.0, 21.0, 0.6, N, B);
        }
        I::Shading => {
            p.rect(3.0, 3.0, 12.0, 12.0, 0.0, Y.a(0.8), N);
            p.rect(12.0, 12.0, 21.0, 21.0, 0.0, Y.a(0.8), N);
            p.table(3.0, 3.0, 21.0, 21.0, 2, 2, N);
        }

        // ----- Chart -----
        I::ChartElements => {
            p.bars(2.5, 3.5, 16.0, 17.5);
            p.badge((18.5, 18.5), 4.8, G, Glyph::Plus);
        }
        I::ChartStyles => {
            p.bars(2.5, 6.0, 15.5, 20.5);
            p.brush((14.0, 10.0), -45.0, 12.0, PU);
        }
        I::SwitchRowCol => {
            p.table(2.5, 2.5, 11.0, 11.0, 2, 2, B);
            p.rect(13.0, 13.0, 17.25, 21.5, 0.6, O, N);
            p.table(13.0, 13.0, 21.5, 21.5, 2, 2, N);
            p.line(&quad_pts((14.0, 4.0), (20.0, 4.0), (20.0, 9.5), 10), K);
            p.head((20.0, 11.0), 90.0, 3.6, K);
            p.line(&quad_pts((10.0, 20.0), (4.0, 20.0), (4.0, 14.5), 10), K);
            p.head((4.0, 13.0), -90.0, 3.6, K);
        }
        I::EditData => {
            p.table(2.5, 3.0, 18.0, 16.0, 3, 3, B);
            p.tool((11.0, 21.5), -45.0, 14.0, 2.2, 3.8, Y, K, 2.5, R);
        }
        I::ChangeChartType => {
            p.circle((8.0, 8.0), 5.8, t(B), B);
            let mut wedge = vec![(8.0, 8.0)];
            wedge.extend(arc_pts((8.0, 8.0), 5.8, 5.8, -90.0, 0.0));
            p.poly(&wedge, O, N);
            p.rect(13.0, 15.0, 15.8, 21.0, 0.4, G, N);
            p.rect(17.5, 11.0, 20.3, 21.0, 0.4, B, N);
            p.seg((11.5, 21.5), (22.0, 21.5), K);
        }

        // ----- Media -----
        I::Play => p.rshape(&[(6.5, 4.0), (19.5, 12.0), (6.5, 20.0)], 1.0, G, N),
        I::Pause => {
            p.rect(6.0, 4.5, 10.0, 19.5, 1.0, K, N);
            p.rect(14.0, 4.5, 18.0, 19.5, 1.0, K, N);
        }
        I::Stop => p.rect(5.0, 5.0, 19.0, 19.0, 2.0, K, N),
        I::TrimMedia => {
            p.rect(2.0, 8.0, 22.0, 16.0, 1.2, N, K);
            p.rect(7.0, 8.0, 17.0, 16.0, 0.0, t(Y), N);
            p.rect(5.8, 5.0, 8.2, 19.0, 0.8, Y, N);
            p.rect(15.8, 5.0, 18.2, 19.0, 0.8, Y, N);
        }
        I::Volume => {
            p.speaker(2.5, 12.0, N, K);
            p.arc((11.0, 12.0), 3.8, -45.0, 45.0, B);
            p.arc((11.0, 12.0), 7.0, -45.0, 45.0, B);
            p.arc((11.0, 12.0), 10.2, -40.0, 40.0, B);
        }
        I::Loop => {
            p.path_arrow(&round_pts(&[(4.0, 13.0), (4.0, 7.0), (20.0, 7.0)], 2.5, false), G);
            p.path_arrow(&round_pts(&[(20.0, 11.0), (20.0, 17.0), (4.0, 17.0)], 2.5, false), G);
        }
        I::Bookmark => {
            p.rshape(&[(6.0, 2.5), (18.0, 2.5), (18.0, 21.5), (12.0, 16.5), (6.0, 21.5)], 0.8, R, N);
        }

        // ----- Window chrome -----
        I::Home => {
            p.rect(10.0, 14.5, 14.0, 21.0, 0.8, B, N);
            p.line(&[(2.5, 11.5), (12.0, 3.0), (21.5, 11.5)], K);
            p.rline(&[(5.0, 9.5), (5.0, 21.0), (19.0, 21.0), (19.0, 9.5)], 1.2, K);
        }
        I::Save => {
            p.rshape(&[(3.0, 3.0), (17.5, 3.0), (21.0, 6.5), (21.0, 21.0), (3.0, 21.0)], 1.5, t(B), B);
            p.rect(7.0, 3.0, 15.0, 8.5, 0.6, N, B);
            p.seg((12.5, 4.5), (12.5, 7.0), B);
            p.rect(6.5, 13.0, 17.5, 21.0, 0.8, N, B);
        }
        I::Undo | I::Redo => {
            let mut pts = vec![(6.0, 8.5), (14.0, 8.5)];
            pts.extend(arc_pts((14.0, 14.0), 5.5, 5.5, -90.0, 90.0));
            pts.push((8.5, 19.5));
            let hs = p.head_size();
            let pts: Vec<P2> = if icon == I::Undo { pts } else { pts.into_iter().map(|(x, y)| (24.0 - x, y)).collect() };
            p.line(&pts, K);
            if icon == I::Undo {
                p.head((3.0, 8.5), 180.0, hs, K);
            } else {
                p.head((21.0, 8.5), 0.0, hs, K);
            }
        }
        I::More => {
            for x in [5.0, 12.0, 19.0] {
                p.dot((x, 12.0), 1.9, K);
            }
        }
        I::Search => p.magnifier((10.0, 10.0), 6.5, N, K),
        I::Share => {
            p.seg((6.0, 12.0), (18.0, 5.5), K);
            p.seg((6.0, 12.0), (18.0, 18.5), K);
            p.dot((18.0, 5.5), 3.0, B);
            p.dot((6.0, 12.0), 3.0, B);
            p.dot((18.0, 18.5), 3.0, B);
        }
        I::CommentsBubble => p.bubble(2.5, 3.5, 21.5, 17.0, N, K),
        I::File => {
            p.page(4.5, 2.0, 19.5, 22.0, N, K);
            p.seg((8.0, 12.5), (16.0, 12.5), B);
            p.seg((8.0, 16.5), (14.0, 16.5), B);
        }
        I::Folder => {
            p.rshape(&[(2.5, 4.5), (9.0, 4.5), (11.0, 7.0), (21.5, 7.0), (21.5, 19.5), (2.5, 19.5)], 1.2, t(Y), K);
            p.rshape(&[(2.5, 10.0), (21.5, 10.0), (21.5, 19.5), (2.5, 19.5)], 1.2, Y.a(0.6), K);
        }
        I::Export => {
            p.rline(&[(15.0, 8.5), (15.0, 2.5), (3.0, 2.5), (3.0, 21.5), (15.0, 21.5), (15.0, 15.5)], 1.5, K);
            p.arrow((8.5, 12.0), (22.0, 12.0), B);
        }
        I::Print => {
            p.rect(6.5, 2.5, 17.5, 9.0, 0.8, N, K);
            p.rline(&[(6.5, 17.5), (2.5, 17.5), (2.5, 9.0), (21.5, 9.0), (21.5, 17.5), (17.5, 17.5)], 1.8, K);
            p.rect(6.5, 14.0, 17.5, 21.5, 0.8, N, K);
            p.dot((18.5, 12.0), 1.1, G);
        }
        I::Settings => {
            p.gear((12.0, 12.0), 9.8, 7.3, 8, N, K);
            p.circle((12.0, 12.0), 3.0, N, K);
        }
        I::Help => {
            p.circle((12.0, 12.0), 9.8, N, K);
            let mut q = arc_pts((12.0, 9.4), 3.2, 3.0, 180.0, 400.0);
            q.push((12.0, 13.0));
            q.push((12.0, 14.6));
            p.line(&q, B);
            p.dot((12.0, 17.8), 1.25, B);
        }
        I::Info => {
            p.circle((12.0, 12.0), 9.8, N, K);
            p.dot((12.0, 7.3), 1.3, B);
            p.seg((12.0, 10.8), (12.0, 17.5), B);
        }
        I::Warning => {
            p.rshape(&[(12.0, 2.5), (22.5, 20.5), (1.5, 20.5)], 2.0, Y, N);
            p.seg((12.0, 9.0), (12.0, 14.0), D);
            p.dot((12.0, 17.2), 1.2, D);
        }
        // Selection pane toggles: monochrome outlines in the ink colour.
        I::Lock => {
            let mut sh = vec![(7.5, 10.5), (7.5, 8.0)];
            sh.extend(arc_pts((12.0, 8.0), 4.5, 4.5, 180.0, 360.0));
            sh.push((16.5, 10.5));
            p.line(&sh, K);
            p.rect(4.5, 10.5, 19.5, 21.5, 1.8, N, K);
            p.seg((12.0, 14.5), (12.0, 17.5), K);
        }
        I::Unlock => {
            // The shackle swung open: its left leg out of the body, raised.
            let mut sh = vec![(12.0, 10.5), (12.0, 6.0)];
            sh.extend(arc_pts((16.5, 6.0), 4.5, 4.5, 180.0, 360.0));
            sh.push((21.0, 8.5));
            p.line(&sh, K);
            p.rect(3.0, 10.5, 16.5, 21.5, 1.8, N, K);
            p.seg((9.75, 14.5), (9.75, 17.5), K);
        }
        I::Detach => {
            // The link's two halves pulled apart, with the break marked between them.
            let link = |c: P2| {
                let local = rrect_pts(-4.5, -2.8, 4.5, 2.8, 2.8);
                p.shape(&tf(c, -45.0, &local), N, K);
            };
            link((7.0, 17.0));
            link((17.0, 7.0));
            p.seg((9.0, 4.0), (10.0, 7.0), K);
            p.seg((4.0, 9.0), (7.0, 10.0), K);
            p.seg((15.0, 20.0), (14.0, 17.0), K);
            p.seg((20.0, 15.0), (17.0, 14.0), K);
        }
        I::Revert => {
            // Three quarters of a circle, from the left over the top round to the bottom, with
            // the arrowhead at its start: turning back.
            p.line(&arc_pts((12.0, 12.0), 7.0, 7.0, 180.0, 450.0), K);
            p.line(&[(2.5, 10.0), (5.0, 12.8), (7.5, 10.0)], K);
        }
        I::Deck => {
            p.rect(3.5, 4.5, 20.5, 15.5, 1.5, N, K);
            p.seg((12.0, 15.5), (12.0, 18.0), K);
            p.line(&[(8.0, 20.5), (12.0, 18.0), (16.0, 20.5)], K);
        }
        I::Styles => {
            for (x, y) in [(8.0, 8.0), (16.0, 8.0), (8.0, 16.0), (16.0, 16.0)] {
                p.circle((x, y), 2.6, N, K);
            }
        }
        I::Sliders => {
            // Two vertical tracks, each with a knob at a different height.
            p.seg((8.0, 4.0), (8.0, 20.0), K);
            p.seg((16.0, 4.0), (16.0, 20.0), K);
            p.circle((8.0, 14.5), 2.4, W, K);
            p.circle((16.0, 9.5), 2.4, W, K);
        }
        I::TextTop => {
            p.seg((5.0, 5.0), (19.0, 5.0), K);
            p.seg((12.0, 19.0), (12.0, 7.5), K);
            p.line(&[(8.5, 11.0), (12.0, 7.5), (15.5, 11.0)], K);
        }
        I::TextBottom => {
            p.seg((5.0, 19.0), (19.0, 19.0), K);
            p.seg((12.0, 5.0), (12.0, 16.5), K);
            p.line(&[(8.5, 13.0), (12.0, 16.5), (15.5, 13.0)], K);
        }
        I::TextMiddle => {
            p.seg((5.0, 12.0), (19.0, 12.0), K);
            p.seg((12.0, 3.5), (12.0, 9.0), K);
            p.line(&[(9.5, 6.5), (12.0, 9.0), (14.5, 6.5)], K);
            p.seg((12.0, 20.5), (12.0, 15.0), K);
            p.line(&[(9.5, 17.5), (12.0, 15.0), (14.5, 17.5)], K);
        }
        I::Tag => {
            // A tag with its corner cut to a point and a hole near the point.
            p.shape(&[(4.0, 6.0), (4.0, 12.0), (12.5, 20.5), (20.5, 12.5), (12.0, 4.0), (6.0, 4.0)], N, K);
            p.circle((8.5, 8.5), 1.5, N, K);
        }
        I::States => {
            // Three frames in a row, the last one dashed, as the strip under the slide shows them.
            p.rect(2.5, 8.0, 8.5, 16.0, 1.0, N, K);
            p.rect(10.0, 8.0, 16.0, 16.0, 1.0, N, K);
            p.seg((17.5, 12.0), (21.5, 12.0), K);
            p.seg((19.5, 10.0), (19.5, 14.0), K);
        }
        I::Build => {
            // Three bullet lines, the last one faint, appearing from the left.
            for (k, y) in [6.5f32, 12.0, 17.5].into_iter().enumerate() {
                let c = if k == 2 { C::Ink(0.4) } else { K };
                p.dot((5.0, y), 1.4, c);
                p.seg((8.5, y), (20.0, y), c);
            }
        }
        I::CornerTl | I::CornerTr | I::CornerBr | I::CornerBl => {
            // A rounded corner, drawn top-left and mirrored into place.
            let mut pts = vec![(5.0, 19.0), (5.0, 12.0)];
            pts.extend(arc_pts((12.0, 12.0), 7.0, 7.0, 180.0, 270.0));
            pts.push((19.0, 5.0));
            let (fx, fy) = match icon {
                I::CornerTr => (true, false),
                I::CornerBr => (true, true),
                I::CornerBl => (false, true),
                _ => (false, false),
            };
            let pts: Vec<P2> = pts.into_iter().map(|(x, y)| (if fx { 24.0 - x } else { x }, if fy { 24.0 - y } else { y })).collect();
            p.line(&pts, K);
        }
        I::Corners => {
            // The four corners of a rounded square, apart.
            for (fx, fy) in [(false, false), (true, false), (true, true), (false, true)] {
                let mut pts = vec![(4.0, 9.0), (4.0, 7.5)];
                pts.extend(arc_pts((7.5, 7.5), 3.5, 3.5, 180.0, 270.0));
                pts.push((9.0, 4.0));
                let pts: Vec<P2> = pts.into_iter().map(|(x, y)| (if fx { 24.0 - x } else { x }, if fy { 24.0 - y } else { y })).collect();
                p.line(&pts, K);
            }
        }
        I::PadH | I::PadV | I::PadSides => {
            p.rect(3.5, 3.5, 20.5, 20.5, 2.5, N, K);
            if icon != I::PadV {
                p.seg((7.5, 7.5), (7.5, 16.5), K);
                p.seg((16.5, 7.5), (16.5, 16.5), K);
            }
            if icon != I::PadH {
                p.seg((7.5, 7.5), (16.5, 7.5), K);
                p.seg((7.5, 16.5), (16.5, 16.5), K);
            }
        }
        I::Eye | I::EyeOff => {
            let mut e = quad_pts((2.0, 12.0), (12.0, 4.0), (22.0, 12.0), 16);
            e.extend(quad_pts((22.0, 12.0), (12.0, 20.0), (2.0, 12.0), 16).into_iter().skip(1));
            e.pop();
            p.shape(&e, N, K);
            p.circle((12.0, 12.0), 3.2, N, K);
            if icon == I::EyeOff {
                p.seg_w((4.0, 21.0), (20.0, 3.0), K, 1.1);
            }
        }
        I::Pointer => p.rshape(&[(5.5, 3.5), (19.0, 12.0), (12.5, 13.5), (9.0, 20.0)], 1.2, N, K),
        I::Close => {
            p.seg_w((5.5, 5.5), (18.5, 18.5), K, 1.1);
            p.seg_w((18.5, 5.5), (5.5, 18.5), K, 1.1);
        }
        I::Check => p.line_w(&[(4.0, 12.5), (9.5, 18.0), (20.0, 6.5)], G, 1.3),
        I::Plus => {
            p.seg((12.0, 4.5), (12.0, 19.5), K);
            p.seg((4.5, 12.0), (19.5, 12.0), K);
        }
        I::Minus => p.seg((4.5, 12.0), (19.5, 12.0), K),
        I::ChevronDown => p.line(&[(6.0, 9.0), (12.0, 15.0), (18.0, 9.0)], K),
        I::ChevronUp => p.line(&[(6.0, 15.0), (12.0, 9.0), (18.0, 15.0)], K),
        I::ChevronLeft => p.line(&[(15.0, 6.0), (9.0, 12.0), (15.0, 18.0)], K),
        I::ChevronRight => p.line(&[(9.0, 6.0), (15.0, 12.0), (9.0, 18.0)], K),
        I::DragHandle => {
            for x in [9.0, 15.0] {
                for y in [6.0, 12.0, 18.0] {
                    p.dot((x, y), 1.6, K);
                }
            }
        }
        I::Star => p.star((12.0, 12.8), 10.0, t(Y), Y),
        I::Image => {
            p.rect(2.5, 4.0, 21.5, 20.0, 1.5, N, K);
            p.circle((16.0, 9.0), 1.8, N, K);
            p.line(&[(2.5, 17.5), (8.5, 11.0), (14.0, 16.5), (16.5, 14.0), (21.5, 18.5)], K);
        }
        I::Sparkle => {
            p.sparkle((10.5, 13.0), 9.0, PU);
            p.sparkle((19.0, 5.0), 3.8, Y);
        }
        I::Palette => {
            let mut pts = arc_pts((12.0, 12.0), 9.8, 9.5, 70.0, 380.0);
            pts.extend(cubic_pts((21.2, 15.25), (19.0, 15.0), (14.5, 14.0), (15.5, 21.0), 10));
            p.shape(&pts, N, K);
            p.dot((7.5, 9.5), 1.7, R);
            p.dot((11.5, 6.3), 1.7, Y);
            p.dot((16.3, 7.8), 1.7, G);
            p.dot((7.2, 14.8), 1.7, B);
        }
        I::Grid => {
            for (x, y) in [(3.0, 3.0), (13.0, 3.0), (3.0, 13.0), (13.0, 13.0)] {
                p.rect(x, y, x + 8.0, y + 8.0, 1.5, N, K);
            }
        }
        I::List => {
            for y in [5.5, 12.0, 18.5] {
                p.dot((4.5, y), 1.5, K);
                p.seg((9.0, y), (21.0, y), K);
            }
        }
        I::Collapse => {
            p.line(&[(7.0, 3.0), (12.0, 8.0), (17.0, 3.0)], K);
            p.line(&[(7.0, 21.0), (12.0, 16.0), (17.0, 21.0)], K);
            p.seg_w((5.0, 12.0), (19.0, 12.0), K.a(0.5), 0.8);
        }
        I::Expand => {
            p.line(&[(7.0, 8.0), (12.0, 3.0), (17.0, 8.0)], K);
            p.line(&[(7.0, 16.0), (12.0, 21.0), (17.0, 16.0)], K);
            p.seg_w((5.0, 12.0), (19.0, 12.0), K.a(0.5), 0.8);
        }
        I::Pin => {
            let m = |pts: &[P2]| tf((12.0, 12.0), 35.0, pts);
            p.line(&m(&[(0.0, 2.0), (0.0, 10.5)]), K);
            p.rshape(&m(&[(-6.0, 0.0), (6.0, 0.0), (6.0, 2.5), (-6.0, 2.5)]), 0.6, R, N);
            p.poly(&m(&[(-3.0, -7.0), (3.0, -7.0), (3.6, 0.0), (-3.6, 0.0)]), R, N);
            p.rshape(&m(&[(-4.2, -10.0), (4.2, -10.0), (4.2, -7.0), (-4.2, -7.0)]), 0.6, R, N);
        }

        // ----- File tab (Backstage) navigation: outlines in the ink colour -----
        I::BackCircle => {
            p.circle((12.0, 12.0), 9.8, N, K);
            p.arrow((17.0, 12.0), (6.5, 12.0), K);
        }
        I::SaveAs => {
            p.rline(&[(11.0, 20.5), (2.5, 20.5), (2.5, 3.0), (15.0, 3.0), (18.0, 6.0), (18.0, 10.5)], 1.2, K);
            p.rect(6.0, 3.0, 13.0, 7.5, 0.6, N, K);
            p.seg_w((13.5, 21.0), (21.5, 13.0), O, 1.4);
            p.seg_w((12.5, 22.0), (13.5, 21.0), K, 1.0);
        }
        I::Account => {
            p.circle((12.0, 8.0), 4.5, N, K);
            p.line(&arc_pts((12.0, 22.0), 8.5, 7.5, 180.0, 360.0), K);
        }
        I::Recent => {
            p.circle((12.0, 12.0), 9.8, N, K);
            p.line(&[(12.0, 6.0), (12.0, 12.0), (16.0, 14.5)], K);
        }
        I::NewFile => {
            p.rline(&[(14.0, 2.5), (5.0, 2.5), (5.0, 21.5), (19.0, 21.5), (19.0, 7.5), (14.0, 2.5), (14.0, 7.5), (19.0, 7.5)], 0.8, K);
        }
        I::OpenFolder => {
            p.rline(&[(18.5, 9.0), (18.5, 6.5), (10.0, 6.5), (8.0, 4.0), (2.5, 4.0), (2.5, 19.5), (18.0, 19.5)], 1.0, K);
            p.rline(&[(2.5, 19.5), (6.0, 10.0), (22.0, 10.0), (18.0, 19.5)], 1.0, K);
        }

        // ----- Status bar (monochrome) -----
        I::NotesSmall => {
            p.rect(3.0, 4.0, 21.0, 20.0, 1.5, N, K);
            p.seg((3.0, 13.5), (21.0, 13.5), K);
            p.seg((6.5, 16.8), (15.5, 16.8), K);
        }
        I::CommentsSmall => p.bubble(3.0, 4.0, 21.0, 16.5, N, K),
        I::ViewNormalSmall => {
            p.rect(2.5, 4.0, 21.5, 20.0, 1.5, N, K);
            p.seg((8.5, 4.0), (8.5, 20.0), K);
        }
        I::ViewSorterSmall => {
            for (x, y) in [(2.5, 4.5), (13.0, 4.5), (2.5, 13.5), (13.0, 13.5)] {
                p.rect(x, y, x + 8.5, y + 6.0, 1.0, N, K);
            }
        }
        I::ViewReadingSmall => {
            p.book(N, K);
            p.seg((12.0, 7.0), (12.0, 20.5), K);
        }
        I::ViewShowSmall => {
            p.seg((2.0, 4.0), (22.0, 4.0), K);
            p.rect(3.5, 4.0, 20.5, 16.0, 0.8, N, K);
            p.seg((12.0, 16.0), (12.0, 19.0), K);
            p.line(&[(8.5, 22.0), (12.0, 19.0), (15.5, 22.0)], K);
        }
        I::FitSmall => {
            for (cx, cy, sx, sy) in [(3.0, 3.0, 1.0, 1.0), (21.0, 3.0, -1.0, 1.0), (3.0, 21.0, 1.0, -1.0), (21.0, 21.0, -1.0, -1.0)] {
                p.rline(&[(cx, cy + 5.0 * sy), (cx, cy), (cx + 5.0 * sx, cy)], 0.8, K);
            }
            p.rect(8.0, 8.5, 16.0, 15.5, 0.8, N, K);
        }
        I::ZoomIn | I::ZoomOut => {
            p.magnifier((10.0, 10.0), 7.0, N, K);
            p.seg((7.0, 10.0), (13.0, 10.0), K);
            if icon == I::ZoomIn {
                p.seg((10.0, 7.0), (10.0, 13.0), K);
            }
        }
    }
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
