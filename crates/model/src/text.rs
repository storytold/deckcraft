//! Text bodies: body properties, paragraphs, runs, list styles.

use serde::{Deserialize, Serialize};

use crate::style::{ColorRef, Fill, Line};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Anchor {
    Top,
    Middle,
    Bottom,
    Justified,
    Distributed,
}

impl Anchor {
    pub fn xml(self) -> &'static str {
        match self {
            Anchor::Top => "t",
            Anchor::Middle => "ctr",
            Anchor::Bottom => "b",
            Anchor::Justified => "just",
            Anchor::Distributed => "dist",
        }
    }
    pub fn from_xml(s: &str) -> Option<Self> {
        Some(match s {
            "t" => Anchor::Top,
            "ctr" => Anchor::Middle,
            "b" => Anchor::Bottom,
            "just" => Anchor::Justified,
            "dist" => Anchor::Distributed,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum AutoFit {
    /// Do not autofit.
    None,
    /// Shrink text on overflow: font scale and line-spacing reduction (0–1).
    Shrink { font_scale: f64, line_reduction: f64 },
    /// Resize shape to fit text.
    Shape,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TextDir {
    Horizontal,
    Vertical,
    Vertical270,
    Stacked,
    EaVertical,
}

impl TextDir {
    pub fn xml(self) -> &'static str {
        match self {
            TextDir::Horizontal => "horz",
            TextDir::Vertical => "vert",
            TextDir::Vertical270 => "vert270",
            TextDir::Stacked => "wordArtVert",
            TextDir::EaVertical => "eaVert",
        }
    }
    pub fn from_xml(s: &str) -> Option<Self> {
        Some(match s {
            "horz" => TextDir::Horizontal,
            "vert" | "mongolianVert" => TextDir::Vertical,
            "vert270" => TextDir::Vertical270,
            "wordArtVert" | "wordArtVertRtl" => TextDir::Stacked,
            "eaVert" => TextDir::EaVertical,
            _ => return None,
        })
    }
}

/// `bodyPr`: insets in points, anchoring, wrapping, autofit, columns, direction, warp.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BodyProps {
    pub inset_l: Option<f64>,
    pub inset_t: Option<f64>,
    pub inset_r: Option<f64>,
    pub inset_b: Option<f64>,
    pub anchor: Option<Anchor>,
    pub anchor_ctr: Option<bool>,
    pub wrap: Option<bool>,
    pub autofit: Option<AutoFit>,
    pub vert: Option<TextDir>,
    /// Extra text rotation in degrees.
    pub rot: Option<f64>,
    pub columns: Option<u32>,
    pub col_spacing: Option<f64>,
    /// WordArt transform (`prstTxWarp`), e.g. `textArchUp`.
    pub warp: Option<String>,
    pub rtl_col: Option<bool>,
    /// Keep text upright when the shape rotates.
    pub upright: Option<bool>,
}

impl BodyProps {
    pub fn inherit(&mut self, p: &BodyProps) {
        self.inset_l = self.inset_l.or(p.inset_l);
        self.inset_t = self.inset_t.or(p.inset_t);
        self.inset_r = self.inset_r.or(p.inset_r);
        self.inset_b = self.inset_b.or(p.inset_b);
        self.anchor = self.anchor.or(p.anchor);
        self.anchor_ctr = self.anchor_ctr.or(p.anchor_ctr);
        self.wrap = self.wrap.or(p.wrap);
        self.autofit = self.autofit.or(p.autofit);
        self.vert = self.vert.or(p.vert);
        self.rot = self.rot.or(p.rot);
        self.columns = self.columns.or(p.columns);
        self.col_spacing = self.col_spacing.or(p.col_spacing);
        if self.warp.is_none() {
            self.warp.clone_from(&p.warp);
        }
        self.rtl_col = self.rtl_col.or(p.rtl_col);
        self.upright = self.upright.or(p.upright);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Align {
    Left,
    Center,
    Right,
    Justify,
    /// Justified with kashida (tatweel) elongation for Arabic (`justLow`).
    JustLow,
    Distributed,
}

impl Align {
    pub fn xml(self) -> &'static str {
        match self {
            Align::Left => "l",
            Align::Center => "ctr",
            Align::Right => "r",
            Align::Justify => "just",
            Align::JustLow => "justLow",
            Align::Distributed => "dist",
        }
    }
    pub fn from_xml(s: &str) -> Option<Self> {
        Some(match s {
            "l" => Align::Left,
            "ctr" => Align::Center,
            "r" => Align::Right,
            "just" | "thaiDist" => Align::Justify,
            "justLow" => Align::JustLow,
            "dist" => Align::Distributed,
            _ => return None,
        })
    }
}

/// Spacing as a percentage of the line (1.0 = single) or in points.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "v")]
pub enum Spacing {
    Pct(f64),
    Pts(f64),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Bullet {
    None,
    Char {
        char: String,
    },
    /// `arabicPeriod`, `arabicParenR`, `romanUcPeriod`, `romanLcPeriod`, `alphaUcPeriod`,
    /// `alphaLcParenR`, `alphaLcPeriod`, `circleNumDbPlain`…
    AutoNum {
        scheme: String,
        #[serde(default = "start_one")]
        start_at: u32,
    },
    Picture {
        media: crate::MediaId,
    },
}

fn start_one() -> u32 {
    1
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TabStop {
    pub pos: f64,
    /// `l`, `ctr`, `r`, `dec`.
    pub align: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ParaProps {
    pub align: Option<Align>,
    /// Left margin (points).
    pub margin_left: Option<f64>,
    pub margin_right: Option<f64>,
    /// First-line indent relative to the margin (negative = hanging).
    pub indent: Option<f64>,
    pub line_spacing: Option<Spacing>,
    pub space_before: Option<Spacing>,
    pub space_after: Option<Spacing>,
    pub bullet: Option<Bullet>,
    pub bullet_font: Option<String>,
    /// `None` = follow text colour.
    pub bullet_color: Option<ColorRef>,
    /// Bullet size as a fraction of the text size.
    pub bullet_size: Option<f64>,
    pub tabs: Option<Vec<TabStop>>,
    pub default_tab: Option<f64>,
    pub rtl: Option<bool>,
    pub font_align: Option<String>,
    pub east_asian_line_break: Option<bool>,
}

impl ParaProps {
    pub fn inherit(&mut self, p: &ParaProps) {
        self.align = self.align.or(p.align);
        self.margin_left = self.margin_left.or(p.margin_left);
        self.margin_right = self.margin_right.or(p.margin_right);
        self.indent = self.indent.or(p.indent);
        self.line_spacing = self.line_spacing.or(p.line_spacing);
        self.space_before = self.space_before.or(p.space_before);
        self.space_after = self.space_after.or(p.space_after);
        if self.bullet.is_none() {
            self.bullet.clone_from(&p.bullet);
        }
        if self.bullet_font.is_none() {
            self.bullet_font.clone_from(&p.bullet_font);
        }
        if self.bullet_color.is_none() {
            self.bullet_color.clone_from(&p.bullet_color);
        }
        self.bullet_size = self.bullet_size.or(p.bullet_size);
        if self.tabs.is_none() {
            self.tabs.clone_from(&p.tabs);
        }
        self.default_tab = self.default_tab.or(p.default_tab);
        self.rtl = self.rtl.or(p.rtl);
        if self.font_align.is_none() {
            self.font_align.clone_from(&p.font_align);
        }
        self.east_asian_line_break = self.east_asian_line_break.or(p.east_asian_line_break);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Caps {
    None,
    Small,
    All,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Strike {
    None,
    Single,
    Double,
}

/// A hyperlink or action on text or a shape.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Action {
    Url { url: String },
    Slide { slide: crate::SlideId },
    NextSlide,
    PreviousSlide,
    FirstSlide,
    LastSlide,
    EndShow,
    LastViewed,
    CustomShow { name: String },
    Program { path: String },
    PlayMedia,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hyperlink {
    pub action: Action,
    #[serde(default)]
    pub tooltip: String,
    #[serde(default)]
    pub highlight_click: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RunProps {
    /// Latin font family, or a theme reference `+mj-lt` / `+mn-lt`.
    pub font: Option<String>,
    pub font_ea: Option<String>,
    pub font_cs: Option<String>,
    pub font_sym: Option<String>,
    /// Size in points.
    pub size: Option<f64>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    /// `none`, `sng`, `dbl`, `heavy`, `dotted`, `dash`, `wavy`…
    pub underline: Option<String>,
    pub underline_color: Option<ColorRef>,
    pub strike: Option<Strike>,
    /// Baseline offset as a fraction of the size (0.3 = superscript 30%).
    pub baseline: Option<f64>,
    pub caps: Option<Caps>,
    /// Character spacing in points.
    pub spacing: Option<f64>,
    /// Kern at and above this size (points); 0 = off.
    pub kern: Option<f64>,
    pub fill: Option<Fill>,
    pub outline: Option<Line>,
    pub highlight: Option<ColorRef>,
    pub link: Option<Hyperlink>,
    pub lang: Option<String>,
    pub shadow: Option<crate::style::Shadow>,
    pub glow: Option<crate::style::Glow>,
    /// Spelling/grammar errors are not flagged.
    pub no_proof: Option<bool>,
}

impl RunProps {
    pub fn inherit(&mut self, p: &RunProps) {
        macro_rules! take {
            ($($f:ident),*) => { $( if self.$f.is_none() { self.$f.clone_from(&p.$f); } )* };
        }
        take!(
            font,
            font_ea,
            font_cs,
            font_sym,
            size,
            bold,
            italic,
            underline,
            underline_color,
            strike,
            baseline,
            caps,
            spacing,
            kern,
            fill,
            outline,
            highlight,
            link,
            lang,
            shadow,
            glow,
            no_proof
        );
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum RunKind {
    Text,
    /// A line break inside the paragraph (Shift+Enter).
    Break,
    /// A field: `slidenum`, `datetime`, `datetime1`…`datetime13`, or other; `text` holds the last value.
    Field {
        field: String,
    },
    /// An inline equation (OMML kept as XML; `text` is its linear form).
    Math {
        omml: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    #[serde(default)]
    pub text: String,
    #[serde(default, skip_serializing_if = "is_default_props")]
    pub props: RunProps,
    #[serde(default = "text_kind", skip_serializing_if = "is_text_kind")]
    pub kind: RunKind,
}

fn is_default_props(p: &RunProps) -> bool {
    *p == RunProps::default()
}
fn text_kind() -> RunKind {
    RunKind::Text
}
fn is_text_kind(k: &RunKind) -> bool {
    *k == RunKind::Text
}

impl Run {
    pub fn new(text: impl Into<String>) -> Self {
        Run { text: text.into(), props: RunProps::default(), kind: RunKind::Text }
    }
    pub fn with(text: impl Into<String>, props: RunProps) -> Self {
        Run { text: text.into(), props, kind: RunKind::Text }
    }
    /// The characters this run contributes to the paragraph (a break is one `\u{b}`).
    pub fn content(&self) -> &str {
        match self.kind {
            RunKind::Break => "\u{b}",
            _ => &self.text,
        }
    }
    pub fn char_len(&self) -> usize {
        self.content().chars().count()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Paragraph {
    /// Outline level 0–8.
    pub level: u8,
    #[serde(skip_serializing_if = "is_default_para")]
    pub props: ParaProps,
    pub runs: Vec<Run>,
    /// Properties of the paragraph end mark (used for an empty paragraph's caret).
    #[serde(skip_serializing_if = "is_default_props")]
    pub end_props: RunProps,
}

fn is_default_para(p: &ParaProps) -> bool {
    *p == ParaProps::default()
}

impl Paragraph {
    pub fn new(text: &str) -> Self {
        Paragraph { runs: if text.is_empty() { vec![] } else { vec![Run::new(text)] }, ..Default::default() }
    }
    pub fn text(&self) -> String {
        self.runs.iter().map(|r| r.content()).collect()
    }
    pub fn char_len(&self) -> usize {
        self.runs.iter().map(Run::char_len).sum()
    }
    pub fn is_empty(&self) -> bool {
        self.char_len() == 0
    }
    /// Merge adjacent text runs with identical properties.
    pub fn normalize(&mut self) {
        let mut out: Vec<Run> = Vec::with_capacity(self.runs.len());
        for r in self.runs.drain(..) {
            if r.kind == RunKind::Text && r.text.is_empty() {
                continue;
            }
            if let Some(last) = out.last_mut()
                && last.kind == RunKind::Text
                && r.kind == RunKind::Text
                && last.props == r.props
            {
                last.text.push_str(&r.text);
                continue;
            }
            out.push(r);
        }
        self.runs = out;
    }
}

/// Per-level paragraph + run defaults (`lstStyle`, master text styles, `defaultTextStyle`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LevelStyle {
    pub para: ParaProps,
    pub run: RunProps,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ListStyle {
    /// Levels 1–9 (index 0–8); `None` = not specified.
    pub levels: Vec<Option<LevelStyle>>,
}

impl ListStyle {
    pub fn level(&self, l: u8) -> Option<&LevelStyle> {
        self.levels.get(l as usize).and_then(|x| x.as_ref())
    }
    pub fn set(&mut self, l: u8, s: LevelStyle) {
        let i = (l as usize).min(8);
        if self.levels.len() <= i {
            self.levels.resize(i + 1, None);
        }
        if let Some(slot) = self.levels.get_mut(i) {
            *slot = Some(s);
        }
    }
    pub fn is_empty(&self) -> bool {
        self.levels.iter().all(Option::is_none)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TextBody {
    pub body: BodyProps,
    #[serde(skip_serializing_if = "ListStyle::is_empty")]
    pub list_style: ListStyle,
    pub paragraphs: Vec<Paragraph>,
}

impl TextBody {
    pub fn from_text(text: &str) -> Self {
        TextBody { paragraphs: text.split('\n').map(Paragraph::new).collect(), ..Default::default() }
    }
    /// Plain text with `\n` between paragraphs.
    pub fn text(&self) -> String {
        self.paragraphs.iter().map(Paragraph::text).collect::<Vec<_>>().join("\n")
    }
    pub fn is_empty(&self) -> bool {
        self.paragraphs.iter().all(Paragraph::is_empty)
    }
    /// Total characters, counting one separator between paragraphs.
    pub fn char_len(&self) -> usize {
        let n: usize = self.paragraphs.iter().map(Paragraph::char_len).sum();
        n + self.paragraphs.len().saturating_sub(1)
    }
}
