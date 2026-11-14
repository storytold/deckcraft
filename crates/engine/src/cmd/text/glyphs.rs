//! Fonts as drawn: which face draws each run of the slides' text. A run names a family; the
//! layout draws it in that family when this machine has it (installed or bundled), else in a
//! stand-in (`FontDb::document_face`: a metric twin, or Carlito for Calibri), and draws a
//! character its face has no glyph for in another face (`FontDb::cascade`), or as `.notdef`
//! (glyph id 0, an empty box) when no face has one. The layout records the face of every glyph
//! run and the glyph id of every glyph; this module reads them back per model run for
//! `text.glyphs`, `font.used {detail: true}` and the answer of `format.font`.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use deckcraft_fonts::{FontDb, FontFace, FontSlot, ScriptTag};
use deckcraft_geom::Rect;
use deckcraft_model::resolve::{self, Ctx};
use deckcraft_model::text::{Run, RunProps, TextBody};
use deckcraft_model::{Presentation, Shape, ShapeId, ShapeKind};
use deckcraft_text::Fields;
use serde_json::{Value, json};

use super::{Result, Selection, Session, ctx_for};
use crate::cmd::{bad, opt_usize_param};
use crate::{DocState, Target};

const CMD: &str = "text.glyphs";

/// The frame a shape whose size is not finite is laid out in (faces do not depend on the frame).
const STAND_IN_FRAME: Rect = Rect::new(0.0, 0.0, 720.0, 540.0);

/// Per drawn cluster, keyed by (paragraph, index of its first character): the face drawing it
/// and whether one of its glyphs is `.notdef`.
type Heads = BTreeMap<(usize, usize), (Arc<FontFace>, bool)>;

/// One model run as the layout draws it.
struct DrawnRun {
    para: usize,
    run: usize,
    /// The run's text as laid out (a field's value as the layout fills it in).
    text: String,
    /// The families the run's characters ask for, Latin typeface first.
    requested: Vec<String>,
    /// Face families drawing the run's visible characters, with how many each draws.
    faces: Vec<(String, usize)>,
    /// Visible characters drawn in a face none of `requested` is drawn in, each once.
    fallback: Vec<char>,
    /// Visible characters drawn as `.notdef`, each once.
    missing: Vec<char>,
}

/// One text body as the layout draws it: its runs holding a visible character, in paragraph and
/// run order, and the face family of each paragraph's drawn bullet.
struct DrawnBody {
    runs: Vec<DrawnRun>,
    bullets: BTreeMap<usize, String>,
}

impl DrawnBody {
    fn run(&self, para: usize, run: usize) -> Option<&DrawnRun> {
        self.runs.binary_search_by_key(&(para, run), |r| (r.para, r.run)).ok().and_then(|i| self.runs.get(i))
    }
}

/// A shape's text body as drawn: its own text (`cell: None`) or a table cell's.
struct Body {
    cell: Option<(usize, usize)>,
    drawn: DrawnBody,
}

/// Where a run sits: its paragraph and run index, the index of its first character in the
/// paragraph's laid-out text, and the script and slot the layout resolves for each of its
/// characters.
#[derive(Clone, Copy)]
struct RunAt<'a> {
    para: usize,
    run: usize,
    start: usize,
    scripts: &'a [(Option<ScriptTag>, FontSlot)],
}

/// A visible character of a run: its index in the paragraph's laid-out text, and its script and
/// slot as the layout resolves them.
#[derive(Clone, Copy)]
struct Visible {
    at: usize,
    c: char,
    script: Option<ScriptTag>,
    slot: FontSlot,
}

/// Distinct characters in the order they first appear.
#[derive(Default)]
struct Chars {
    seen: BTreeSet<char>,
    list: Vec<char>,
}

impl Chars {
    fn add(&mut self, c: char) {
        if self.seen.insert(c) {
            self.list.push(c);
        }
    }
}

/// The face families `FontDb` gives requested families, looked up once per query.
#[derive(Default)]
struct FaceCache(BTreeMap<(bool, String, &'static str), Option<String>>);

impl FaceCache {
    /// The family of the face the layout draws `family` in, in `style`. The Latin typeface
    /// (`latin`) is drawn in itself or its stand-in (`FontDb::document_face`); another slot's
    /// typeface or a theme script font only when this machine has the family (the layout skips
    /// a missing one), else `None`.
    fn drawing(&mut self, latin: bool, family: &str, style: &'static str) -> Option<String> {
        let key = (latin, family.to_string(), style);
        if let Some(v) = self.0.get(&key) {
            return v.clone();
        }
        let db = FontDb::global();
        let v = if latin {
            Some(db.document_face(family, style).family.clone())
        } else if db.has_family(family) {
            Some(db.face(family, style).family.clone())
        } else {
            None
        };
        self.0.insert(key, v.clone());
        v
    }
}

/// Characters that draw nothing of their own, left out of the report: whitespace, control
/// characters (a line break is U+000B), zero-width and direction marks, variation selectors, and
/// the soft hyphen (drawn only as the hyphen of a line it breaks).
fn ignorable(c: char) -> bool {
    c.is_whitespace()
        || c.is_control()
        || matches!(c as u32, 0xAD | 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x2069 | 0xFE00..=0xFE0F | 0xFEFF | 0xE0100..=0xE01EF)
}

/// The face style a run asks for, as the layout names it.
fn style_name(props: &RunProps) -> &'static str {
    match (props.bold.unwrap_or(false), props.italic.unwrap_or(false)) {
        (true, true) => "Bold Italic",
        (true, false) => "Bold",
        (false, true) => "Italic",
        (false, false) => "Regular",
    }
}

/// Field values as a slide draws them: its number and the presentation's footer text. Date
/// fields keep their saved text, where the slide shows the current date.
struct DrawnFields {
    num: u32,
    footer: String,
}

impl DrawnFields {
    fn for_slide(doc: &Presentation, index: usize) -> Self {
        DrawnFields { num: doc.slide_number(index), footer: doc.header_footer.footer_text.clone() }
    }

    /// Field values as the page being edited draws them: a slide's, or on a master, layout or
    /// notes page number 1 and no footer text, as the renderer draws those pages.
    fn for_page(doc: &Presentation, sel: &Selection) -> Self {
        if matches!(sel.target, Target::Slides) { Self::for_slide(doc, sel.slide) } else { DrawnFields { num: 1, footer: String::new() } }
    }
}

impl Fields for DrawnFields {
    fn field(&self, kind: &str) -> Option<String> {
        match kind {
            "slidenum" => Some(self.num.to_string()),
            "footer" => Some(self.footer.clone()),
            _ => None,
        }
    }
}

/// The families a run's visible characters ask for, as the layout reads them, without
/// duplicates: the Latin typeface (theme references resolved); the East Asian or complex-script
/// typeface (the run's, else the theme's) when a character is in that slot; the theme's typeface
/// for each script among the characters (`a:font[@script]`); and the symbol typeface when a
/// character is a symbol-font code (U+F000–U+F0FF). The typefaces come from the layout's own
/// rules (`deckcraft_text::run_fonts`, `script_family` and `is_symbol_code`).
fn requested(ctx: &Ctx, props: &RunProps, visible: &[Visible]) -> Vec<String> {
    let fonts = deckcraft_text::run_fonts(ctx, props);
    let mut out = vec![resolve::font_family(ctx, props)];
    let mut push = |f: Option<String>| {
        if let Some(f) = f
            && !out.iter().any(|o| o.eq_ignore_ascii_case(&f))
        {
            out.push(f);
        }
    };
    if visible.iter().any(|v| v.slot == FontSlot::EastAsian) {
        push(fonts.ea);
    }
    if visible.iter().any(|v| v.slot == FontSlot::ComplexScript) {
        push(fonts.cs);
    }
    let mut scripts: Vec<ScriptTag> = vec![];
    for v in visible {
        if let Some(tag) = v.script
            && !scripts.contains(&tag)
        {
            scripts.push(tag);
            push(deckcraft_text::script_family(ctx, fonts.major, tag).map(str::to_string));
        }
    }
    if visible.iter().any(|v| deckcraft_text::is_symbol_code(v.c)) {
        push(fonts.sym);
    }
    out
}

/// Adds `n` characters drawn in `face` to `faces`.
fn add_face(faces: &mut Vec<(String, usize)>, face: &str, n: usize) {
    match faces.iter_mut().find(|(f, _)| f.as_str() == face) {
        Some((_, count)) => *count = count.saturating_add(n),
        None => faces.push((face.to_string(), n)),
    }
}

/// Face families, the one drawing the most characters first (ties in the order they appear).
fn by_count(faces: &[(String, usize)]) -> Vec<String> {
    let mut v = faces.to_vec();
    v.sort_by_key(|f| std::cmp::Reverse(f.1));
    v.into_iter().map(|(f, _)| f).collect()
}

fn char_list(chars: &[char]) -> Vec<String> {
    chars.iter().map(ToString::to_string).collect()
}

/// How the layout draws one run, from the faces it gave the paragraph's clusters: `None` when the
/// run holds no visible character. A character is missing when its cluster has a `.notdef` glyph
/// and its face has no glyph for it; else it is a fallback character when its face is none of the
/// faces the run's requested families are drawn in.
fn drawn_run(ctx: &Ctx, heads: &Heads, at: RunAt<'_>, text: String, props: &RunProps, cache: &mut FaceCache) -> Option<DrawnRun> {
    let visible: Vec<Visible> = text
        .chars()
        .enumerate()
        .filter(|(_, c)| !ignorable(*c))
        .map(|(k, c)| {
            let (script, slot) = at.scripts.get(k).copied().unwrap_or((None, FontSlot::Latin));
            Visible { at: at.start.saturating_add(k), c, script, slot }
        })
        .collect();
    if visible.is_empty() {
        return None;
    }
    let style = style_name(props);
    let names = requested(ctx, props, &visible);
    let drawing: Vec<String> = names.iter().enumerate().filter_map(|(i, n)| cache.drawing(i == 0, n, style)).collect();
    let mut faces: Vec<(String, usize)> = vec![];
    let mut fallback = Chars::default();
    let mut missing = Chars::default();
    for v in &visible {
        // The cluster holding the character starts at it or before it.
        let Some((_, (face, notdef))) = heads.range((at.para, 0)..=(at.para, v.at)).next_back() else { continue };
        add_face(&mut faces, &face.family, 1);
        if *notdef && !face.covers(v.c) {
            missing.add(v.c);
        } else if !drawing.iter().any(|d| d.eq_ignore_ascii_case(&face.family)) {
            fallback.add(v.c);
        }
    }
    Some(DrawnRun { para: at.para, run: at.run, text, requested: names, faces, fallback: fallback.list, missing: missing.list })
}

/// How the layout draws `body`, the text of `shape` laid out in `rect` with field values from
/// `fields`. Faces and glyph ids do not depend on the shrink scale, so the body is laid out once,
/// at its set size.
fn drawn_body(ctx: &Ctx, shape: &Shape, body: &TextBody, rect: Rect, fields: &DrawnFields, cache: &mut FaceCache) -> DrawnBody {
    let opts = deckcraft_text::Opts { rect, fields, prompt_color: None, no_shrink: true };
    let layout = deckcraft_text::layout(ctx, shape, body, &opts);
    let mut heads = Heads::new();
    let mut bullets = BTreeMap::new();
    for gr in &layout.runs {
        if gr.cells.is_empty() {
            // A bullet: glyphs that draw no paragraph character.
            if !gr.glyphs.is_empty() {
                bullets.entry(gr.para).or_insert_with(|| gr.face.family.clone());
            }
            continue;
        }
        for (&(gid, _, _), &(ci, _)) in gr.glyphs.iter().zip(&gr.cells) {
            let head = heads.entry((gr.para, ci)).or_insert_with(|| (gr.face.clone(), false));
            head.1 |= gid == 0;
        }
    }
    let mut runs = vec![];
    for (pi, para) in body.paragraphs.iter().enumerate() {
        let texts: Vec<(RunProps, String)> = para
            .runs
            .iter()
            .map(|run| {
                let props = resolve::run(ctx, shape, para, &run.props);
                let text = deckcraft_text::run_text(run, props.lang.as_deref(), fields);
                (props, text)
            })
            .collect();
        // Scripts are resolved over the whole paragraph, as the layout does: a digit or a space
        // takes the script of the text before it.
        let chars: Vec<char> = texts.iter().flat_map(|(_, t)| t.chars()).collect();
        let scripts = deckcraft_fonts::script::resolve(&chars);
        let mut start = 0usize;
        for (ri, (props, text)) in texts.into_iter().enumerate() {
            let end = start.saturating_add(text.chars().count());
            let at = RunAt { para: pi, run: ri, start, scripts: scripts.get(start..end).unwrap_or_default() };
            if let Some(r) = drawn_run(ctx, &heads, at, text, &props, cache) {
                runs.push(r);
            }
            start = end;
        }
    }
    DrawnBody { runs, bullets }
}

/// The frame `sh`'s own text is laid out in: its geometry's text rectangle.
fn frame(ctx: &Ctx, sh: &Shape) -> Rect {
    let x = resolve::xfrm(ctx, sh);
    if x.is_finite() { deckcraft_render::shape_geometry(sh, x.w, x.h).text_rect } else { STAND_IN_FRAME }
}

/// How the layout draws each text body of `sh`, as the slide draws them: a table's cells, or the
/// shape's own text (charts and ink draw none).
fn shape_bodies(ctx: &Ctx, sh: &Shape, fields: &DrawnFields, cache: &mut FaceCache) -> Vec<Body> {
    let mut out = vec![];
    match &sh.kind {
        ShapeKind::Table(t) => {
            // Row heights move cells, not faces: the stored heights stand in for the drawn ones,
            // which would lay every cell out a second time.
            let heights: Vec<f64> = t.rows.iter().map(|r| if r.height.is_finite() { r.height.max(0.0) } else { 0.0 }).collect();
            for (r, c, rect) in deckcraft_render::table_cell_rects(t, &heights) {
                let Some(cell) = t.cell(r, c).filter(|cell| !cell.text.is_empty()) else { continue };
                let body = deckcraft_render::table_cell_body(ctx, t, r, c, cell);
                let tmp = Shape::table_cell(body.clone());
                out.push(Body { cell: Some((r, c)), drawn: drawn_body(ctx, &tmp, &body, rect, fields, cache) });
            }
        }
        ShapeKind::Chart(_) | ShapeKind::Ink { .. } => {}
        _ => {
            if let Some(body) = sh.text.as_ref().filter(|b| !b.is_empty()) {
                out.push(Body { cell: None, drawn: drawn_body(ctx, sh, body, frame(ctx, sh), fields, cache) });
            }
        }
    }
    out
}

/// Calls `f` for each shape of `shapes` and its group members, in drawing order, hidden shapes
/// included; with `wanted`, only for the shapes it lists and the members of groups it lists.
fn visit(shapes: &[Shape], depth: usize, inside: bool, wanted: Option<&BTreeSet<ShapeId>>, f: &mut dyn FnMut(&Shape)) {
    if depth > 64 {
        return;
    }
    for sh in shapes {
        let take = inside || wanted.is_none_or(|w| w.contains(&sh.id));
        if take {
            f(sh);
        }
        visit(sh.children(), depth.saturating_add(1), take, wanted, f);
    }
}

/// `ids`: `None` when absent, else the listed shape ids.
fn shape_ids(p: &Value) -> Result<Option<BTreeSet<ShapeId>>> {
    match p.get("ids") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(a)) => {
            let mut ids = BTreeSet::new();
            for v in a {
                let id = v.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or_else(|| bad(CMD, format!("`ids` has an invalid shape id: {v}")))?;
                ids.insert(ShapeId(id));
            }
            Ok(Some(ids))
        }
        Some(v) => Err(bad(CMD, format!("`ids` is not a list of shape ids: {v}"))),
    }
}

impl DrawnRun {
    fn to_json(&self, slide: usize, shape: ShapeId, cell: Option<(usize, usize)>) -> Value {
        let mut v = json!({
            "slide": slide,
            "shape": shape,
            "para": self.para,
            "run": self.run,
            "text": self.text,
            "requested": self.requested,
            "drawnWith": by_count(&self.faces),
            "fallbackChars": char_list(&self.fallback),
            "missingChars": char_list(&self.missing),
        });
        if let (Some((row, col)), Some(m)) = (cell, v.as_object_mut()) {
            m.insert("cell".into(), json!([row, col]));
        }
        v
    }
}

/// `text.glyphs {slide?, ids?}`: each run of the slides' text that holds a visible character,
/// with the families it asks for, the faces that draw it, and the characters drawn in a fallback
/// face or as `.notdef`. Read-only.
pub(super) fn text_glyphs(s: &mut Session, p: &Value) -> Result<Value> {
    let doc = &s.doc()?.doc;
    let count = doc.slides.len();
    let only = opt_usize_param(p, "slide", CMD)?;
    if let Some(i) = only
        && i >= count
    {
        return Err(bad(CMD, format!("`slide` is past the last slide ({count} in the presentation): {i}")));
    }
    let wanted = shape_ids(p)?;
    let range = only.map_or(0..count, |i| i..i.saturating_add(1));
    let searched = doc.slides.get(range.clone()).unwrap_or_default();
    if let Some(ids) = &wanted {
        // The searched slides' shape ids, collected once.
        let mut present = BTreeSet::new();
        for sl in searched {
            deckcraft_model::walk(&sl.shapes, &mut |sh, _| {
                present.insert(sh.id);
            });
        }
        if let Some(id) = ids.iter().find(|id| !present.contains(*id)) {
            return Err(bad(
                CMD,
                match only {
                    Some(i) => format!("`ids` names a shape that is not on slide {i}: {id}"),
                    None => format!("`ids` names a shape that is on no slide: {id}"),
                },
            ));
        }
    }
    let mut cache = FaceCache::default();
    let mut out = vec![];
    for (index, slide) in range.zip(searched) {
        let Some(ctx) = Ctx::for_slide(doc, slide) else { continue };
        let fields = DrawnFields::for_slide(doc, index);
        visit(&slide.shapes, 0, false, wanted.as_ref(), &mut |sh| {
            for b in shape_bodies(&ctx, sh, &fields, &mut cache) {
                for r in &b.drawn.runs {
                    out.push(r.to_json(index, sh.id, b.cell));
                }
            }
        });
    }
    Ok(Value::Array(out))
}

/// One family's use across the slides, for `font.used {detail: true}`.
#[derive(Default)]
struct FamilyUse {
    family: String,
    runs: usize,
    bullets: usize,
    /// Set sizes in points, ascending.
    sizes: Vec<f64>,
    /// Slide indexes, ascending.
    slides: Vec<usize>,
    faces: Vec<(String, usize)>,
}

impl FamilyUse {
    fn add_size(&mut self, size: f64) {
        let v = (size * 100.0).round() / 100.0;
        if let Err(i) = self.sizes.binary_search_by(|s| s.total_cmp(&v)) {
            self.sizes.insert(i, v);
        }
    }

    fn add_slide(&mut self, index: usize) {
        if self.slides.last() != Some(&index) {
            self.slides.push(index);
        }
    }

    fn to_json(&self) -> Value {
        let db = FontDb::global();
        let drawn_with = if self.faces.is_empty() {
            // None of the family's text is drawn (a merged-over table cell, a chart's text): the
            // face it would be drawn in.
            vec![db.document_face(&self.family, "Regular").family.clone()]
        } else {
            by_count(&self.faces)
        };
        json!({
            "family": self.family,
            "installed": db.has_family(&self.family),
            "drawnWith": drawn_with,
            "runs": self.runs,
            "bullets": self.bullets,
            "sizes": self.sizes,
            "slides": self.slides,
        })
    }
}

/// Families by name, compared without ASCII case as `font.used` compares them.
#[derive(Default)]
struct Uses {
    list: Vec<FamilyUse>,
    index: BTreeMap<String, usize>,
}

impl Uses {
    /// The entry for `family` (trimmed), added when new; `None` for an empty name, which
    /// `font.used` leaves out.
    fn entry(&mut self, family: &str) -> Option<&mut FamilyUse> {
        let family = family.trim();
        if family.is_empty() {
            return None;
        }
        let key = family.to_ascii_lowercase();
        let i = match self.index.get(&key) {
            Some(&i) => i,
            None => {
                self.list.push(FamilyUse { family: family.to_string(), ..Default::default() });
                let i = self.list.len().saturating_sub(1);
                self.index.insert(key, i);
                i
            }
        };
        self.list.get_mut(i)
    }

    /// Adds the runs and bullets of `body`, text of `sh` on slide `index`, as `font.used` counts
    /// them (`crate::fonts::body_font_uses`), with the faces `drawn` shows drawing them.
    fn tally(&mut self, ctx: &Ctx, sh: &Shape, body: &TextBody, drawn: Option<&DrawnBody>, index: usize) {
        crate::fonts::body_font_uses(ctx, sh, body, &mut |used| {
            let Some(u) = self.entry(&used.family) else { return };
            u.add_slide(index);
            match used.run {
                Some((ri, props)) => {
                    u.runs = u.runs.saturating_add(1);
                    if let Some(size) = props.size.filter(|v| v.is_finite()) {
                        u.add_size(size);
                    }
                    if let Some(r) = drawn.and_then(|d| d.run(used.para, ri)) {
                        for (face, n) in &r.faces {
                            add_face(&mut u.faces, face, *n);
                        }
                    }
                }
                None => {
                    u.bullets = u.bullets.saturating_add(1);
                    if let Some(face) = drawn.and_then(|d| d.bullets.get(&used.para)) {
                        add_face(&mut u.faces, face, 1);
                    }
                }
            }
        });
    }
}

/// `font.used {detail: true}`: per family `font.used` lists, in the same order, whether this
/// machine has it, the faces that draw its text (most characters first), the runs and character
/// bullets set in it, the sizes set on those runs and the slides they are on.
pub(super) fn used_detail(doc: &Presentation) -> Vec<Value> {
    let mut uses = Uses::default();
    let mut cache = FaceCache::default();
    for (index, slide) in doc.slides.iter().enumerate() {
        let Some(ctx) = Ctx::for_slide(doc, slide) else { continue };
        let fields = DrawnFields::for_slide(doc, index);
        deckcraft_model::walk(&slide.shapes, &mut |sh, _| {
            let drawn = shape_bodies(&ctx, sh, &fields, &mut cache);
            let by_cell: BTreeMap<Option<(usize, usize)>, &DrawnBody> = drawn.iter().map(|b| (b.cell, &b.drawn)).collect();
            if let Some(body) = &sh.text {
                uses.tally(&ctx, sh, body, by_cell.get(&None).copied(), index);
            }
            if let ShapeKind::Table(t) = &sh.kind {
                for (r, row) in t.rows.iter().enumerate() {
                    for (c, cell) in row.cells.iter().enumerate() {
                        uses.tally(&ctx, sh, &cell.text, by_cell.get(&Some((r, c))).copied(), index);
                    }
                }
            }
        });
    }
    let mut list = uses.list;
    list.sort_by_key(|u| u.family.to_lowercase());
    list.iter().map(FamilyUse::to_json).collect()
}

/// Whether run `run` of paragraph `para` of `body` holds a character of the range from `a` to `b`
/// (positions as (paragraph, character), `a` first).
fn run_in_range(body: &TextBody, para: usize, run: usize, a: (usize, usize), b: (usize, usize)) -> bool {
    let Some(runs) = body.paragraphs.get(para).map(|p| &p.runs) else { return false };
    let start = runs.iter().take(run).fold(0usize, |n, r| n.saturating_add(r.char_len()));
    let end = start.saturating_add(runs.get(run).map_or(0, Run::char_len));
    start < end && (para, start) < b && (para, end) > a
}

/// Adds the faces drawing those of `runs` that ask for `family` (their Latin typeface) to `faces`.
fn add_runs<'a>(faces: &mut Vec<(String, usize)>, family: &str, runs: impl Iterator<Item = &'a DrawnRun>) {
    for r in runs.filter(|r| r.requested.first().is_some_and(|f| f.eq_ignore_ascii_case(family))) {
        for (face, n) in &r.faces {
            add_face(faces, face, *n);
        }
    }
}

/// The faces drawing the selection's runs that ask for `family`, as the page being edited draws
/// them, with how many characters each draws: the runs of the selected text (the word at the
/// caret, as `format_range` takes it; none for a caret between words, where no character is set),
/// else the text of the selected shapes (of a table, only the selected cells when a range of them
/// is selected). Speaker notes are not drawn on the page.
fn selection_faces(st: &DocState, ctx: &Ctx, family: &str) -> Vec<(String, usize)> {
    let sel = &st.selection;
    let fields = DrawnFields::for_page(&st.doc, sel);
    let mut cache = FaceCache::default();
    let mut faces = vec![];
    if let Some(t) = &sel.text {
        let (a, b) = super::format_range(st, t);
        if a != b
            && !t.notes
            && let Some(sh) = st.shape(t.shape)
            && let Some(text) = super::body_of(st, t)
        {
            for body in shape_bodies(ctx, sh, &fields, &mut cache).iter().filter(|b| b.cell == t.cell) {
                add_runs(&mut faces, family, body.drawn.runs.iter().filter(|r| run_in_range(text, r.para, r.run, a, b)));
            }
        }
        return faces;
    }
    for sh in sel.shapes.iter().filter_map(|id| st.shape(*id)) {
        let range = match &sh.kind {
            ShapeKind::Table(t) => crate::cmd::table::selected_range(sel.cells, sh.id, t),
            _ => None,
        };
        for body in shape_bodies(ctx, sh, &fields, &mut cache) {
            if body.cell.is_none_or(|(r, c)| crate::cmd::table::in_range(range, r, c)) {
                add_runs(&mut faces, family, body.drawn.runs.iter());
            }
        }
    }
    faces
}

/// `format.font`'s answer (and `format.fontDialog`'s when it sets the family), once the family is
/// set: the family a run set to `family` asks for, theme references (`+mn-lt`) resolved on the page
/// being edited; whether this machine has it; and the faces that draw the selection's text set in
/// it, as the page draws it, most characters first (a stand-in when this machine lacks the family,
/// another face for a character it has no glyph for). With none of that text drawn (a caret between
/// words, speaker notes, a shape without text), the face a run in the family is drawn in
/// (`FontDb::document_face`).
pub(crate) fn font_answer(st: &DocState, family: &str) -> Value {
    let props = RunProps { font: Some(family.to_string()), ..Default::default() };
    let ctx = ctx_for(&st.doc, &st.selection);
    let resolved = ctx.as_ref().map_or_else(|| family.to_string(), |ctx| resolve::font_family(ctx, &props));
    let faces = ctx.as_ref().map(|ctx| selection_faces(st, ctx, &resolved)).unwrap_or_default();
    let db = FontDb::global();
    let drawn_with = if faces.is_empty() { vec![db.document_face(&resolved, "Regular").family.clone()] } else { by_count(&faces) };
    json!({
        "family": resolved,
        "installed": db.has_family(&resolved),
        "drawnWith": drawn_with,
    })
}
