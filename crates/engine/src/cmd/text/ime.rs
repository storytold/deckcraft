use super::caret::cell_on_slide;
use super::editing::unlinked_at_link_end;
use super::layout::{ctx_for, layout_for};
use super::{DocState, Paragraph, Result, Session, TextBody, TextSel, Value, bad, body_of, ed, edit_text, json, str_param, xfrm_of};

/// Maximum number of UTF-8 bytes in an outgoing IME surrounding-text query.
///
/// This matches `egui::output::MAX_IME_SURROUNDING_TEXT_BYTES`; keeping the bound here lets the
/// engine answer without copying a whole body or document.
pub const IME_SURROUNDING_MAX_BYTES: usize = 4000;

/// Bounded committed text around an IME selection, in logical (not visual) order and with UTF-8
/// byte offsets. The current input-method composition (pre-edit) is removed from both the text
/// and the offsets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImeSurrounding {
    pub text: String,
    pub cursor: usize,
    pub anchor: usize,
}

/// How a soft line break reads in the IME view: a newline, like a paragraph break.
///
/// The model stores it as `\u{b}` ([`RunKind::Break`]); publishing that control character would
/// give the platform a text its own `ime_text` filter never produces, so its idea of the lines
/// and our offsets would disagree. Both are one byte, so offsets do not otherwise change.
fn ime_char(ch: char) -> char {
    if ch == '\u{b}' { '\n' } else { ch }
}

/// Committed characters of one paragraph (the pre-edit skipped), as `(char index, character)`.
fn committed_chars(p: &Paragraph, comp: Option<(usize, usize)>) -> impl Iterator<Item = (usize, char)> + '_ {
    p.runs
        .iter()
        .flat_map(|run| run.content().chars())
        .enumerate()
        .filter_map(move |(i, ch)| (!comp.is_some_and(|(s, e)| i >= s && i < e)).then_some((i, ime_char(ch))))
}

/// The same characters, newest first, without walking the paragraph from its start.
fn committed_chars_rev(p: &Paragraph, comp: Option<(usize, usize)>) -> impl Iterator<Item = (usize, char)> + '_ {
    let mut starts = Vec::with_capacity(p.runs.len());
    let mut at = 0usize;
    for r in &p.runs {
        let n = r.char_len();
        starts.push((at, n));
        at += n;
    }
    p.runs.iter().zip(starts).rev().flat_map(move |(r, (start, n))| {
        let content = r.content();
        content.chars().rev().enumerate().filter_map(move |(k, ch)| {
            let i = start + n - 1 - k;
            (!comp.is_some_and(|(s, e)| i >= s && i < e)).then_some((i, ime_char(ch)))
        })
    })
}

/// The composition's `(paragraph, start, end)` character range, when it is inside the body.
pub(super) fn composition_range(body: &TextBody, c: Option<&crate::Composition>) -> Option<(usize, usize, usize)> {
    let c = c?;
    let para = body.paragraphs.get(c.start.0)?;
    let start = c.start.1.min(para.char_len());
    let end = start.saturating_add(c.len).min(para.char_len());
    (start < end).then_some((c.start.0, start, end))
}

/// Return the composition character range only when its paragraph index equals `pi`.
fn comp_in(comp: Option<(usize, usize, usize)>, pi: usize) -> Option<(usize, usize)> {
    comp.filter(|(p, _, _)| *p == pi).map(|(_, s, e)| (s, e))
}

/// Clip a model position to the body. An empty body only has position `(0, 0)`.
pub(super) fn model_pos(body: &TextBody, pos: (usize, usize)) -> Option<(usize, usize)> {
    if body.paragraphs.is_empty() {
        return (pos == (0, 0)).then_some((0, 0));
    }
    body.paragraphs.get(pos.0).map(|p| (pos.0, pos.1.min(p.char_len())))
}

/// Walk forward from `from` over at most `limit` committed characters (or UTF-8 bytes of whole
/// characters), stopping at the end of the body. Returns the position reached and the characters
/// and bytes consumed.
///
/// The returned window is bounded by `limit`, but locating `from` inside a paragraph still walks
/// that paragraph's runs from their start: runs do not cache character lengths, and a position is
/// a character index. A megabyte paragraph therefore costs time proportional to the caret's
/// character offset, even though the copied surrounding text stays within `limit`.
pub(super) fn walk_forward(
    body: &TextBody,
    comp: Option<(usize, usize, usize)>,
    from: (usize, usize),
    limit: usize,
    bytes: bool,
) -> ((usize, usize), usize, usize) {
    let (mut pi, mut at) = from;
    let (mut chars, mut used) = (0usize, 0usize);
    // `limit` comes from a (possibly hostile) request: every count is checked, never added
    // blindly, so an enormous limit cannot overflow.
    let fits = |used: usize, chars: usize, n: usize| if bytes { used.saturating_add(n) <= limit } else { chars < limit };
    while let Some(para) = body.paragraphs.get(pi) {
        for (i, ch) in committed_chars(para, comp_in(comp, pi)) {
            if i < at {
                continue;
            }
            let n = ch.len_utf8();
            if !fits(used, chars, n) {
                return ((pi, i), chars, used);
            }
            used += n;
            chars += 1;
            at = i + 1;
        }
        if pi + 1 >= body.paragraphs.len() {
            break;
        }
        // The paragraph separator between this paragraph and the next.
        if !fits(used, chars, 1) {
            return ((pi, at), chars, used);
        }
        used += 1;
        chars += 1;
        pi += 1;
        at = 0;
    }
    ((pi, at), chars, used)
}

/// Walk backward from `from` over at most `limit` committed characters (or UTF-8 bytes of whole
/// characters). Returns the position reached and the characters and bytes consumed.
pub(super) fn walk_backward(
    body: &TextBody,
    comp: Option<(usize, usize, usize)>,
    from: (usize, usize),
    limit: usize,
    bytes: bool,
) -> ((usize, usize), usize, usize) {
    let (mut pi, mut at) = from;
    let (mut chars, mut used) = (0usize, 0usize);
    let fits = |used: usize, chars: usize, n: usize| if bytes { used.saturating_add(n) <= limit } else { chars < limit };
    loop {
        let Some(para) = body.paragraphs.get(pi) else { return ((pi, at), chars, used) };
        let mut boundary = at.min(para.char_len());
        for (i, ch) in committed_chars_rev(para, comp_in(comp, pi)) {
            if i >= boundary {
                continue;
            }
            let n = ch.len_utf8();
            if !fits(used, chars, n) {
                return ((pi, boundary), chars, used);
            }
            used += n;
            chars += 1;
            boundary = i;
        }
        // This paragraph is consumed down to its start.
        if pi == 0 {
            return ((0, 0), chars, used);
        }
        if !fits(used, chars, 1) {
            return ((pi, 0), chars, used);
        }
        used += 1;
        chars += 1;
        pi -= 1;
        at = usize::MAX; // all of the previous paragraph is before us
    }
}

/// The committed logical text between two model positions (excluding `end`). `None` when it is
/// longer than `cap` bytes: every caller only needs a bounded answer.
fn logical_between(body: &TextBody, comp: Option<(usize, usize, usize)>, start: (usize, usize), end: (usize, usize), cap: usize) -> Option<String> {
    let (a, b) = if start <= end { (start, end) } else { (end, start) };
    let mut out = String::new();
    for pi in a.0..=b.0 {
        let Some(para) = body.paragraphs.get(pi) else { return Some(out) };
        let from = if pi == a.0 { a.1 } else { 0 };
        let to = if pi == b.0 { b.1 } else { usize::MAX };
        for (i, ch) in committed_chars(para, comp_in(comp, pi)) {
            if i < from || i >= to {
                continue;
            }
            out.push(ch);
            if out.len() > cap {
                return None;
            }
        }
        if pi < b.0 {
            out.push('\n');
            if out.len() > cap {
                return None;
            }
        }
    }
    Some(out)
}

/// Committed surrounding text for the text being edited.
///
/// Returns `None` when the body or selection is invalid, or when the selection alone is wider
/// than [`IME_SURROUNDING_MAX_BYTES`]. An empty body is `Some` with empty text, which is distinct
/// from an unavailable context. `cursor > anchor` is preserved for a reversed selection.
///
/// Only a bounded window is measured and copied: the query walks at most the budget out from the
/// selection on each side and does not copy the rest of the body. Finding the selection's
/// character offsets inside one paragraph still scans that paragraph's prefix, so the time is not
/// bounded independently of the paragraph length.
pub fn ime_surrounding(st: &DocState, t: &TextSel) -> Option<ImeSurrounding> {
    let body = body_of(st, t)?;
    let comp = composition_range(body, st.composition.as_ref());
    let caret = model_pos(body, t.caret)?;
    let anchor = model_pos(body, t.anchor)?;
    let (start, end) = if caret <= anchor { (caret, anchor) } else { (anchor, caret) };
    let selected = logical_between(body, comp, start, end, IME_SURROUNDING_MAX_BYTES)?;
    let selection_len = selected.len();
    let budget = IME_SURROUNDING_MAX_BYTES.saturating_sub(selection_len);
    let (_, _, before_available) = walk_backward(body, comp, start, budget, true);
    let (_, _, after_available) = walk_forward(body, comp, end, budget, true);
    let mut before = (budget / 2).min(before_available);
    let mut after = (budget - before).min(after_available);
    before = (budget - after).min(before_available);
    after = (budget - before).min(after_available);
    let (window_start, _, _) = walk_backward(body, comp, start, before, true);
    let (window_end, _, _) = walk_forward(body, comp, end, after, true);
    let before_text = logical_between(body, comp, window_start, start, budget)?;
    let after_text = logical_between(body, comp, end, window_end, budget)?;
    let before_len = before_text.len();
    let mut text = before_text;
    text.push_str(&selected);
    text.push_str(&after_text);
    // The budget arithmetic above already bounds this; refuse rather than publish more.
    if text.len() > IME_SURROUNDING_MAX_BYTES {
        return None;
    }
    // The caret and anchor are the selection's own endpoints, so their offsets inside the
    // window are the window's start plus either end of the selected text.
    let (cursor, anchor) = if caret >= anchor { (before_len + selection_len, before_len) } else { (before_len, before_len + selection_len) };
    Some(ImeSurrounding { text, cursor, anchor })
}

/// Text an input method sends: no paragraph or line breaks (Enter is a key of its own).
fn ime_text(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).collect()
}

/// Is the composition still where it was put (the paragraph holds all of it)?
fn composition_fits(st: &DocState, c: &crate::Composition) -> bool {
    let Some(t) = &st.selection.text else { return false };
    let Some(body) = body_of(st, t) else { return false };
    body.paragraphs.get(c.start.0).is_some_and(|p| c.start.1.saturating_add(c.len) <= p.char_len())
}

/// `text.compose`: put an input method's preedit at the insertion point (replacing the selection)
/// or replace the composition with the new preedit. An empty preedit cancels: the presentation
/// goes back to how it was before the composition began.
pub(super) fn compose(s: &mut Session, p: &Value) -> Result<Value> {
    let text = ime_text(str_param(p, "text").ok_or_else(|| bad("text.compose", "missing `text`"))?);
    let n = text.chars().count();
    let active = p.get("active").and_then(Value::as_array).and_then(|a| {
        let (x, y) = (a.first()?.as_u64()? as usize, a.get(1)?.as_u64()? as usize);
        (x < y).then_some((x.min(n), y.min(n)))
    });
    let current = s.doc()?.composition.clone();
    // A composition whose text was changed under it is kept as it is; a new one starts.
    let current = match current {
        Some(c) if !composition_fits(s.doc()?, &c) => {
            // The text changed under the pre-edit: the app settled it, so the platform's copy
            // must be cancelled too.
            s.settle_composition();
            None
        }
        c => c,
    };
    match current {
        None if text.is_empty() => Ok(json!({"composing": false})),
        None => {
            let st = s.doc()?;
            let (before, before_selection) = (st.doc.clone(), st.selection.clone());
            let (start, props) = edit_text(s, |body, t| {
                let (a, b) = t.ordered();
                let at = ed::delete(body, a, b);
                let props = if a != b { Some(ed::props_at(body, at)) } else { unlinked_at_link_end(body, at) };
                let c = ed::insert(body, at, &text, props.clone());
                t.anchor = c;
                t.caret = c;
                (at, props)
            })?;
            s.doc_mut()?.composition = Some(crate::Composition { start, len: n, active, props, before, before_selection });
            Ok(json!({"composing": true, "start": [start.0, start.1], "len": n}))
        }
        Some(c) if text.is_empty() => {
            let st = s.doc_mut()?;
            st.doc = c.before;
            st.selection = c.before_selection;
            st.composition = None;
            st.revision += 1;
            Ok(json!({"composing": false}))
        }
        Some(c) => {
            let (start, len, props) = (c.start, c.len, c.props.clone());
            edit_text(s, |body, t| {
                ed::delete(body, start, (start.0, start.1 + len));
                let e = ed::insert(body, start, &text, props);
                t.anchor = e;
                t.caret = e;
            })?;
            s.doc_mut()?.composition = Some(crate::Composition { len: n, active, ..c });
            Ok(json!({"composing": true, "start": [start.0, start.1], "len": n}))
        }
    }
}

/// `text.commit`: the input method's result replaces the composition (or is typed at the
/// insertion point when there is none) and the whole composition becomes one undo step.
pub(super) fn commit(s: &mut Session, p: &Value) -> Result<Value> {
    let text = ime_text(str_param(p, "text").ok_or_else(|| bad("text.commit", "missing `text`"))?);
    if s.doc()?.composition.is_some() || !text.is_empty() {
        compose(s, &json!({"text": text}))?;
    }
    s.end_composition();
    let caret = s.doc()?.selection.text.as_ref().map(|t| t.caret).unwrap_or_default();
    Ok(json!({"caret": [caret.0, caret.1]}))
}

/// Underlines (slide coordinates) under character ranges `(para, start, end)` of shape `id`'s
/// text as laid out on the current slide: where spelling squiggles go.
pub fn range_underlines(
    st: &DocState,
    id: deckcraft_model::ShapeId,
    ranges: &[(usize, usize, usize)],
) -> Vec<(deckcraft_geom::Point, deckcraft_geom::Point)> {
    let Some(sh) = st.shape(id) else { return vec![] };
    let Some(l) = layout_for(st, &TextSel { shape: id, ..Default::default() }) else { return vec![] };
    let x = xfrm_of(&st.doc, &st.selection, sh);
    let tr = deckcraft_render::shape_geometry(sh, x.w, x.h).text_rect;
    let m = deckcraft_render::text_transform(x.affine(), tr, l.rotation);
    let mut out = vec![];
    for &(para, a, b) in ranges {
        for (x0, x1, y) in l.composition_underlines(deckcraft_text::Pos { para, ch: a }, deckcraft_text::Pos { para, ch: b }) {
            out.push((m * deckcraft_geom::Point::new(x0, y), m * deckcraft_geom::Point::new(x1, y)));
        }
    }
    out
}

/// Like [`range_underlines`], for the text of table `id`'s cell at `row`, `col`.
pub fn cell_range_underlines(
    st: &DocState,
    id: deckcraft_model::ShapeId,
    row: usize,
    col: usize,
    ranges: &[(usize, usize, usize)],
) -> Vec<(deckcraft_geom::Point, deckcraft_geom::Point)> {
    let Some(sh) = st.shape(id) else { return vec![] };
    let deckcraft_model::ShapeKind::Table(tb) = &sh.kind else { return vec![] };
    let (Some(cell), Some(ctx)) = (tb.cell(row, col), ctx_for(&st.doc, &st.selection)) else { return vec![] };
    let heights = crate::cmd::table_drawn_heights(&st.doc, &st.selection, tb);
    let Some(rect) = deckcraft_render::table_cell_rects(tb, &heights).into_iter().find(|(r, c, _)| *r == row && *c == col).map(|(_, _, r)| r) else {
        return vec![];
    };
    let body = deckcraft_render::table_cell_body(&ctx, tb, row, col, cell);
    let tmp = deckcraft_model::Shape::table_cell(body.clone());
    let l = deckcraft_text::layout(
        &ctx,
        &tmp,
        &body,
        &deckcraft_text::Opts { rect, fields: &deckcraft_text::NoFields, prompt_color: None, no_shrink: false },
    );
    let x = xfrm_of(&st.doc, &st.selection, sh);
    let m = deckcraft_render::text_transform(x.affine(), rect, l.rotation);
    let mut out = vec![];
    for &(para, a, b) in ranges {
        for (x0, x1, y) in l.composition_underlines(deckcraft_text::Pos { para, ch: a }, deckcraft_text::Pos { para, ch: b }) {
            out.push((m * deckcraft_geom::Point::new(x0, y), m * deckcraft_geom::Point::new(x1, y)));
        }
    }
    out
}

/// Underlines under an input method's composition (#101), in slide coordinates: `(from, to,
/// thick)` per line, thick under the part being converted. Empty without a composition, or for
/// notes (an egui text field draws its own composition).
pub fn composition_underlines(st: &DocState) -> Vec<(deckcraft_geom::Point, deckcraft_geom::Point, bool)> {
    let (Some(c), Some(t)) = (&st.composition, &st.selection.text) else { return vec![] };
    if t.notes {
        return vec![];
    }
    if let Some((row, col)) = t.cell {
        let Some(placed) = cell_on_slide(st, t.shape, row, col) else { return vec![] };
        return underline_segments(&placed.layout, placed.to_slide, c);
    }
    let Some(sh) = st.shape(t.shape) else { return vec![] };
    let Some(l) = layout_for(st, t) else { return vec![] };
    let x = xfrm_of(&st.doc, &st.selection, sh);
    let tr = deckcraft_render::shape_geometry(sh, x.w, x.h).text_rect;
    let m = deckcraft_render::text_transform(x.affine(), tr, l.rotation);
    underline_segments(&l, m, c)
}

fn underline_segments(
    layout: &deckcraft_text::TextLayout,
    to_slide: deckcraft_geom::Affine,
    c: &crate::Composition,
) -> Vec<(deckcraft_geom::Point, deckcraft_geom::Point, bool)> {
    let pos = |ch: usize| deckcraft_text::Pos { para: c.start.0, ch: c.start.1 + ch };
    let mut out = vec![];
    let mut add = |a: usize, b: usize, thick: bool| {
        for (x0, x1, y) in layout.composition_underlines(pos(a), pos(b)) {
            out.push((to_slide * deckcraft_geom::Point::new(x0, y), to_slide * deckcraft_geom::Point::new(x1, y), thick));
        }
    };
    add(0, c.len, false);
    if let Some((a, b)) = c.active {
        add(a, b.min(c.len), true);
    }
    out
}

#[cfg(test)]
mod ime_surrounding_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use serde_json::json;

    use crate::Session;

    /// Test helper: insert and enter a rectangle containing `aé😀b`, returning its shape id.
    fn edit(s: &mut Session) -> u64 {
        let r = s.execute("shape.insert", &json!({"preset": "rect", "rect": [10, 10, 240, 60], "text": "aé😀b"})).unwrap();
        let id = r["id"].as_u64().unwrap();
        s.execute("text.edit", &json!({"id": id})).unwrap();
        id
    }

    /// Test helper: return the identified shape's text, or an empty string when it is gone.
    fn text(s: &Session, id: u64) -> String {
        let id = deckcraft_model::ShapeId(u32::try_from(id).unwrap());
        s.doc().unwrap().shape(id).and_then(|sh| sh.text.as_ref()).map(deckcraft_model::text::TextBody::text).unwrap_or_default()
    }

    /// Test helper: set an in-paragraph text selection through the engine command path.
    fn select(s: &mut Session, anchor: usize, caret: usize) {
        s.execute("text.select", &json!({"anchor": [0, anchor], "caret": [0, caret]})).unwrap();
    }

    /// DeleteSurrounding removes only the requested text around the selection. The selection
    /// itself and its direction stay, so the following commit replaces it.
    #[test]
    fn selected_text_survives_the_requested_characters_or_bytes_around_it() {
        let mut s = Session::with_new();
        let id = edit(&mut s);
        select(&mut s, 3, 1); // reversed: caret before anchor, selecting é😀
        s.delete_surrounding(1, 1, false).unwrap();
        assert_eq!(text(&s, id), "é😀", "a and b go; the selected run stays");
        let t = s.doc().unwrap().selection.text.clone().unwrap();
        assert_eq!((t.anchor, t.caret), ((0, 2), (0, 0)), "both endpoints shift and stay reversed");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(text(&s, id), "aé😀b");

        select(&mut s, 1, 3);
        s.delete_surrounding(1, 1, true).unwrap();
        assert_eq!(text(&s, id), "é😀", "byte counts around the selection delete a and b too");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(text(&s, id), "aé😀b");
    }

    /// A byte range removes only whole scalars fully contained in it. A count that ends inside
    /// a character leaves that character whole (and undeleted), never splitting it.
    #[test]
    fn byte_counts_round_inward_to_whole_characters() {
        let mut s = Session::with_new();
        let id = edit(&mut s);
        select(&mut s, 2, 2); // after é: bytes 0..3
        s.delete_surrounding(1, 0, true).unwrap();
        assert_eq!(text(&s, id), "aé😀b", "one byte inside é is not the whole scalar");
        s.delete_surrounding(2, 0, true).unwrap();
        assert_eq!(text(&s, id), "a😀b", "two bytes reach é's start, so the whole é goes");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(text(&s, id), "aé😀b");

        select(&mut s, 2, 2);
        s.delete_surrounding(0, 1, true).unwrap();
        assert_eq!(text(&s, id), "aé😀b", "one byte into the emoji is not the whole scalar");
        s.delete_surrounding(0, 4, true).unwrap();
        assert_eq!(text(&s, id), "aéb", "four bytes contain the whole emoji");
    }

    /// The committed query is logical text with UTF-8 byte offsets; it keeps a reversed
    /// selection, removes the model-stored pre-edit, and treats an empty body as available.
    #[test]
    fn surrounding_text_is_committed_logical_and_bounded() {
        use super::{IME_SURROUNDING_MAX_BYTES, ime_surrounding};

        let mut s = Session::with_new();
        let id = edit(&mut s);
        let st = s.doc().unwrap();
        let t = st.selection.text.clone().unwrap();
        let q = ime_surrounding(st, &t).unwrap();
        assert_eq!((q.text.as_str(), q.cursor, q.anchor), ("aé😀b", 8, 8));

        s.execute("text.select", &json!({"anchor": [0, 3], "caret": [0, 1]})).unwrap();
        let st = s.doc().unwrap();
        let t = st.selection.text.clone().unwrap();
        let q = ime_surrounding(st, &t).unwrap();
        assert_eq!((q.cursor, q.anchor), (1, 7), "reversed selection keeps caret before anchor");
        assert_eq!(q.text, "aé😀b");

        s.execute("text.set", &json!({"id": id, "text": "ab"})).unwrap();
        s.execute("text.move", &json!({"to": "end"})).unwrap();
        s.compose_text("日本", None).unwrap();
        let st = s.doc().unwrap();
        let t = st.selection.text.clone().unwrap();
        let q = ime_surrounding(st, &t).unwrap();
        assert_eq!((q.text.as_str(), q.cursor, q.anchor), ("ab", 2, 2), "pre-edit is not committed text");

        s.execute("text.set", &json!({"id": id, "text": ""})).unwrap();
        let st = s.doc().unwrap();
        let t = st.selection.text.clone().unwrap();
        let q = ime_surrounding(st, &t).unwrap();
        assert_eq!((q.text.as_str(), q.cursor, q.anchor), ("", 0, 0), "empty body is available");

        let long = "a".repeat(IME_SURROUNDING_MAX_BYTES + 1500);
        s.execute("text.set", &json!({"id": id, "text": long})).unwrap();
        s.execute("text.move", &json!({"to": "end"})).unwrap();
        let st = s.doc().unwrap();
        let t = st.selection.text.clone().unwrap();
        let q = ime_surrounding(st, &t).unwrap();
        assert!(q.text.len() <= IME_SURROUNDING_MAX_BYTES, "{}", q.text.len());
        assert_eq!(q.cursor, q.text.len(), "caret stays at the end of the bounded window");
    }

    /// Paragraph marks are part of the logical text the platform sees (`\n`), so a deletion may
    /// remove one and merge the paragraphs, exactly as the published offsets say.
    #[test]
    fn paragraph_marks_are_deletable_logical_text() {
        use super::ime_surrounding;

        let mut s = Session::with_new();
        let id = edit(&mut s);
        s.execute("text.set", &json!({"id": id, "text": "ab\ncd"})).unwrap();
        // The caret at the start of the second paragraph: after "ab" and its paragraph mark.
        s.execute("text.select", &json!({"anchor": [1, 0], "caret": [1, 0]})).unwrap();
        let st = s.doc().unwrap();
        let q = ime_surrounding(st, &st.selection.text.clone().unwrap()).unwrap();
        assert_eq!((q.text.as_str(), q.cursor, q.anchor), ("ab\ncd", 3, 3));

        // One byte before the caret is that paragraph mark.
        s.delete_surrounding(1, 0, true).unwrap();
        assert_eq!(text(&s, id), "abcd", "the mark goes and the paragraphs merge");
        let t = s.doc().unwrap().selection.text.clone().unwrap();
        assert_eq!(t.caret, (0, 2), "the caret stays where the merged text continues");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(text(&s, id), "ab\ncd");

        // Characters count the same mark.
        s.execute("text.select", &json!({"anchor": [1, 0], "caret": [1, 0]})).unwrap();
        s.delete_surrounding(1, 0, false).unwrap();
        assert_eq!(text(&s, id), "abcd");
        s.execute("edit.undo", &json!({})).unwrap();

        // And from the other side: one character after the end of the first paragraph.
        s.execute("text.select", &json!({"anchor": [0, 2], "caret": [0, 2]})).unwrap();
        s.delete_surrounding(0, 1, false).unwrap();
        assert_eq!(text(&s, id), "abcd");
    }

    /// A soft line break (Shift+Enter) is published as a newline, like a paragraph break: the
    /// platform must never see the model's `\u{b}`, which its own text filter would reject.
    #[test]
    fn soft_line_breaks_read_as_newlines() {
        use super::ime_surrounding;

        let mut s = Session::with_new();
        let id = edit(&mut s);
        s.execute("text.autocorrect", &json!({"on": false})).unwrap(); // isolate logical line-break coordinates

        s.execute("text.set", &json!({"id": id, "text": "ab"})).unwrap();
        s.execute("text.select", &json!({"anchor": [0, 1], "caret": [0, 1]})).unwrap();
        s.execute("text.insert", &json!({"text": "\u{b}"})).unwrap();
        assert_eq!(text(&s, id), "a\u{b}b", "a line break inside one paragraph");
        let st = s.doc().unwrap();
        let q = ime_surrounding(st, &st.selection.text.clone().unwrap()).unwrap();
        assert_eq!((q.text.as_str(), q.cursor, q.anchor), ("a\nb", 2, 2));

        // The break is one logical byte, so removing it removes one character.
        s.delete_surrounding(1, 0, true).unwrap();
        assert_eq!(text(&s, id), "ab");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(text(&s, id), "a\u{b}b");
    }

    /// A count that does not fit a `usize` (32-bit wasm) is clamped, never truncated to a
    /// different number that would delete other text than the platform asked for.
    #[test]
    fn out_of_range_counts_clamp_instead_of_truncating() {
        let mut s = Session::with_new();
        let id = edit(&mut s);
        select(&mut s, 1, 1);
        s.delete_surrounding(usize::MAX, 0, false).unwrap();
        assert_eq!(text(&s, id), "é😀b", "everything before the caret goes, no panic");
        let expected = usize::try_from(4_294_967_296u64).unwrap_or(usize::MAX);
        assert_eq!(super::super::caret::count_param(&json!({"before": 4_294_967_296u64}), "before"), expected);
        assert!(expected > 0, "the count is clamped, not wrapped to nothing");
        assert_eq!(super::super::caret::count_param(&json!({}), "before"), 0);
    }

    /// A non-composing input-method deletion is exactly one undo step; the manual history entry
    /// must not be duplicated by the generic command wrapper.
    #[test]
    fn a_non_composing_ime_deletion_is_one_undo_step() {
        let mut s = Session::with_new();
        let id = edit(&mut s);
        let undo = s.doc().unwrap().history.undo.len();
        select(&mut s, 3, 3);
        s.delete_surrounding(1, 0, false).unwrap();
        assert_eq!(text(&s, id), "aéb");
        assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1, "one history entry, not two");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(text(&s, id), "aé😀b");
    }
}
