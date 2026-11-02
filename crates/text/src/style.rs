use super::run::{RunEffects, Underline};
use super::shape::run_effects;
use super::{Arc, Caps, Ctx, Fill, FontDb, FontFace, FontSlot, ParaProps, Rect, Rgba, RunProps, ScriptTag, Spacing, Strike, resolve};

/// Values for fields (`slidenum`, `datetime…`, `footer`). `None` keeps the field's saved text.
pub trait Fields {
    fn field(&self, kind: &str) -> Option<String>;
    /// The value of field `kind` in a run of language `lang` (BCP 47, e.g. `en-US`): dates are
    /// written the language's way.
    fn field_in(&self, kind: &str, lang: Option<&str>) -> Option<String> {
        let _ = lang;
        self.field(kind)
    }
}

pub struct NoFields;

impl Fields for NoFields {
    fn field(&self, kind: &str) -> Option<String> {
        if kind == "slidenum" { Some("1".into()) } else { None }
    }
}

pub struct Opts<'a> {
    /// The text box: the preset's text rectangle in shape-local space.
    pub rect: Rect,
    pub fields: &'a dyn Fields,
    /// Draw this text instead of the body (placeholder prompts), in this colour.
    pub prompt_color: Option<Rgba>,
    /// Ignore autofit shrinking (used to measure natural height).
    pub no_shrink: bool,
}

pub(crate) struct Style {
    pub(crate) face: Arc<FontFace>,
    pub(crate) family: String,
    pub(crate) size: f64,
    pub(crate) color: Rgba,
    pub(crate) alpha: f64,
    pub(crate) baseline: f64,
    pub(crate) spacing: f64,
    pub(crate) caps: Caps,
    pub(crate) underline: Option<(Rgba, Underline)>,
    pub(crate) strike: Option<Strike>,
    pub(crate) highlight: Option<Rgba>,
    pub(crate) outline: Option<(Rgba, f64)>,
    pub(crate) link: bool,
    pub(crate) gradient: Option<deckcraft_model::style::Gradient>,
    pub(crate) effects: Option<RunEffects>,
    /// "Regular", "Bold"… for the slot and fallback faces.
    pub(crate) style_name: &'static str,
    /// BCP 47 language (`a:rPr/@lang`), for shaping.
    pub(crate) lang: Option<String>,
    /// The `a:ea` and `a:cs` families (theme references resolved), if the document names one.
    pub(crate) ea: Option<String>,
    pub(crate) cs: Option<String>,
    /// The `a:sym` family: private-use characters (U+F000–U+F0FF, as PowerPoint stores symbol-font
    /// characters) are drawn in it.
    pub(crate) sym: Option<String>,
    /// Heading font (`+mj-…`): the theme's major font set applies.
    major: bool,
    /// Pair kerning on: `a:rPr/@kern` is set and the size reaches it.
    pub(crate) kern: bool,
}

/// One character of a paragraph after shaping.
#[derive(Clone)]
pub(crate) struct Cell {
    pub(crate) ch: char,
    pub(crate) style: usize,
    pub(crate) face: usize,
    /// Resolved bidi embedding level (before the line rules).
    pub(crate) level: u8,
    /// Glyphs of the shaping cluster this cell heads, from the cluster's visual left edge (gid,
    /// x, y). Empty for the other cells of a cluster.
    pub(crate) glyphs: Vec<(u32, f64, f64)>,
    /// This character's advance: its share of the cluster (ligature carets); 0 inside a grapheme.
    pub(crate) adv: f64,
    /// Index of the cell heading this character's shaping cluster.
    pub(crate) head: usize,
    /// A grapheme starts here: carets stand and lines break only before such cells.
    pub(crate) boundary: bool,
    /// Drawn upright in East Asian vertical text.
    pub(crate) upright: bool,
}

impl Cell {
    /// A line may start before this cell: a grapheme that starts a shaping cluster.
    pub(crate) fn breakable(&self, i: usize) -> bool {
        self.boundary && self.head == i
    }
}

pub(crate) struct ParaShaped {
    pub(crate) cells: Vec<Cell>,
    pub(crate) styles: Vec<Style>,
    pub(crate) faces: Vec<Arc<FontFace>>,
    pub(crate) props: ParaProps,
    /// Size of the paragraph's first run (spacing percentages, empty lines).
    pub(crate) lead_size: f64,
    pub(crate) lead_face: Arc<FontFace>,
    /// Size of the paragraph mark (`a:endParaRPr`): what an empty last line is sized by.
    pub(crate) end_size: f64,
    pub(crate) end_face: Arc<FontFace>,
    pub(crate) bullet: Option<(String, Style)>,
    #[allow(dead_code)]
    pub(crate) level: u8,
    /// Right-to-left paragraph (`a:pPr/@rtl`).
    pub(crate) rtl: bool,
}

fn style_name(bold: bool, italic: bool) -> &'static str {
    match (bold, italic) {
        (true, true) => "Bold Italic",
        (true, false) => "Bold",
        (false, true) => "Italic",
        (false, false) => "Regular",
    }
}

pub(crate) fn make_style(ctx: &Ctx, r: &RunProps, scale: f64, prompt: Option<Rgba>) -> Style {
    let family = resolve::font_family(ctx, r);
    let bold = r.bold.unwrap_or(false);
    let italic = r.italic.unwrap_or(false);
    let face =
        embedded_face(ctx, &family, style_name(bold, italic)).unwrap_or_else(|| FontDb::global().document_face(&family, style_name(bold, italic)));
    let size = (r.size.unwrap_or(18.0) * scale).clamp(0.5, 4000.0);
    let (mut color, alpha, gradient) = match &r.fill {
        // The resolved colour carries the fill's transparency (`a:alpha`): it is applied once.
        Some(Fill::Solid { color }) => (ctx.color(color, None), 1.0, None),
        Some(Fill::Gradient(g)) => {
            // Colours resolved here (theme, colour map), so the renderer needs no colour scheme.
            let mut g = g.clone();
            for s in &mut g.stops {
                s.color = deckcraft_model::ColorRef::rgb(ctx.color(&s.color, None));
            }
            (g.stops.first().map(|s| ctx.color(&s.color, None)).unwrap_or(Rgba::BLACK), 1.0, Some(g))
        }
        Some(Fill::None) => (Rgba::TRANSPARENT, 0.0, None),
        _ => (resolve::text_color(ctx, r), 1.0, None),
    };
    if let Some(p) = prompt {
        color = p;
    }
    let major = r.font.as_deref().is_some_and(|f| f.starts_with("+mj"));
    let theme = &ctx.master.theme;
    let slot_family = |explicit: &Option<String>, theme_ref: &str| {
        let name = theme.font(explicit.as_deref().unwrap_or(theme_ref));
        (!name.trim().is_empty()).then_some(name)
    };
    let ea = slot_family(&r.font_ea, if major { "+mj-ea" } else { "+mn-ea" });
    let cs = slot_family(&r.font_cs, if major { "+mj-cs" } else { "+mn-cs" });
    let underline =
        r.underline.as_deref().and_then(Underline::from_xml).map(|u| (r.underline_color.as_ref().map(|c| ctx.color(c, None)).unwrap_or(color), u));
    let outline = r.outline.as_ref().and_then(|l| match &l.fill {
        Some(Fill::Solid { color: c }) => Some((ctx.color(c, None), l.width.unwrap_or(0.75))),
        _ => None,
    });
    Style {
        face,
        family,
        size,
        color,
        alpha,
        baseline: r.baseline.unwrap_or(0.0),
        spacing: r.spacing.unwrap_or(0.0) * scale,
        caps: r.caps.unwrap_or(Caps::None),
        underline,
        strike: r.strike.filter(|s| *s != Strike::None),
        highlight: r.highlight.as_ref().map(|c| ctx.color(c, None)),
        outline,
        link: r.link.is_some(),
        gradient,
        effects: run_effects(ctx, r, scale),
        style_name: style_name(bold, italic),
        lang: r.lang.clone().filter(|l| !l.is_empty()),
        ea,
        cs,
        sym: r.font_sym.as_deref().map(|f| ctx.master.theme.font(f)).filter(|f| !f.trim().is_empty()),
        major,
        kern: r.kern.is_some_and(|k| k > 0.0 && size + 1e-6 >= k * scale),
    }
}

/// The faces to try for a cluster of `slot`/`script` in style `st`, in order: the slot's
/// typeface, the theme's font for the script (`a:font[@script]`), then the run's Latin face.
pub(crate) fn face_chain(ctx: &Ctx, st: &Style, slot: FontSlot, script: Option<ScriptTag>) -> Vec<Arc<FontFace>> {
    let db = FontDb::global();
    let mut chain: Vec<Arc<FontFace>> = Vec::with_capacity(3);
    let mut push = |family: &str| {
        // Only families that exist: `FontDb::face` would substitute the default face.
        if let Some(f) = embedded_face(ctx, family, st.style_name).or_else(|| db.has_family(family).then(|| db.face(family, st.style_name)))
            && !chain.iter().any(|c| Arc::ptr_eq(c, &f))
        {
            chain.push(f);
        }
    };
    for family in requested_families(ctx, st, slot, script) {
        push(&family);
    }
    if !chain.iter().any(|c| Arc::ptr_eq(c, &st.face)) {
        chain.push(st.face.clone());
    }
    chain
}

pub(crate) fn embedded_face(ctx: &Ctx, family: &str, style: &str) -> Option<Arc<FontFace>> {
    let font = ctx.pres.embedded_fonts.iter().find(|f| f.typeface.eq_ignore_ascii_case(family))?;
    let preferred = match style {
        "Bold Italic" => font.bold_italic,
        "Bold" => font.bold,
        "Italic" => font.italic,
        _ => font.regular,
    };
    preferred.into_iter().chain(font.faces().into_iter().flatten()).find_map(|id| {
        let item = ctx.pres.media(id)?;
        deckcraft_fonts::embedded_face(item.data.clone(), family, style)
    })
}

/// The document's requested cascade, including unavailable families, shared with diagnostics.
pub(crate) fn requested_families(ctx: &Ctx, st: &Style, slot: FontSlot, script: Option<ScriptTag>) -> Vec<String> {
    let mut out = Vec::with_capacity(3);
    let explicit = match slot {
        FontSlot::EastAsian => st.ea.as_deref(),
        FontSlot::ComplexScript => st.cs.as_deref(),
        FontSlot::Latin => None,
    };
    if let Some(family) = explicit {
        out.push(family.to_string());
    }
    if let Some(sc) = script.and_then(|t| std::str::from_utf8(&t).ok().map(String::from)) {
        let set = if st.major { &ctx.master.theme.fonts.major } else { &ctx.master.theme.fonts.minor };
        if let Some(family) = set.script_for_language(&sc, st.lang.as_deref()) {
            out.push(family.to_string());
        }
    }
    out.push(st.family.clone());
    out.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    out
}

/// Space before or after a paragraph, points. Shrink-on-overflow's line spacing reduction
/// doesn't apply to it (PowerPoint keeps it).
pub(crate) fn spacing_pts(s: Option<Spacing>, natural_line_height: f64) -> f64 {
    match s {
        Some(Spacing::Pct(p)) => p * natural_line_height,
        Some(Spacing::Pts(v)) => v,
        None => 0.0,
    }
}
