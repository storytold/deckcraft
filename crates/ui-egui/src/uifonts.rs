//! Interface fonts for text in any script (#101). The interface faces (Inter, BIZ UDPGothic,
//! Noto Sans Arabic and egui's own) leave Hangul, Simplified Chinese, Thai, Devanagari… without
//! glyphs, so a Korean file name, title, speaker note or comment would be drawn as boxes. For each
//! character on screen that they lack, a face that has it is borrowed from the font database (the
//! craft-fonts faces, then installed fonts such as Malgun Gothic, Apple SD Gothic Neo or Noto
//! Sans CJK) and added after the interface faces. Nothing is bundled for it: on the web, where
//! there are no installed fonts, only the embedded faces are used.
//!
//! Ported from WordCraft's IME composition and window-caption fallback (#565, #840), widened from
//! the composition and caption to every text the interface draws from the presentation.

use std::collections::HashSet;
use std::sync::Arc;

use deckcraft_fonts::{FontDb, FontFace};

/// Faces larger than this are left out (egui keeps a copy of each face it is given). Large enough
/// for the stock macOS CJK collections the font database picks: `AppleSDGothicNeo.ttc` (about
/// 55 MB, the only Hangul face) and `PingFang.ttc` (about 78 MB, Simplified Chinese).
const MAX_FACE_BYTES: usize = 128 << 20;
/// Aggregate bytes of every borrowed UI face. `MAX_FACE_BYTES` alone bounded one face but left
/// `faces` unbounded; three full-size CJK collections are enough for the supported scripts.
const MAX_AGGREGATE_BYTES: usize = 3 * MAX_FACE_BYTES;

fn accepts_face(byte_len: usize, color: bool) -> bool {
    !color && byte_len <= MAX_FACE_BYTES
}

/// Accept a face only when checked addition stays within the aggregate interface-font byte
/// budget.
fn aggregate_accepts(total: usize, byte_len: usize) -> bool {
    total.checked_add(byte_len).is_some_and(|n| n <= MAX_AGGREGATE_BYTES)
}

/// Faces borrowed for the interface, and the characters already looked at.
#[derive(Default)]
pub struct UiFonts {
    /// Faces added after the interface fonts, in the order they were found.
    pub faces: Vec<Arc<FontFace>>,
    /// Characters already looked at (drawn, given a face, or found in no face).
    seen: HashSet<char>,
    /// The presentation last looked through: (uid, revision).
    doc: Option<(u64, u64)>,
    /// Faces were added since the fonts were last installed.
    changed: bool,
}

/// Characters worth a lookup: not ASCII, a space, a control or a private-use character.
fn wanted(c: char) -> bool {
    !(c.is_ascii() || c.is_whitespace() || c.is_control() || matches!(c, '\u{E000}'..='\u{F8FF}' | '\u{F0000}'..))
}

/// The faces needed to draw `text`, beyond `have`: one for each character that `covered` says the
/// interface fonts can't draw, found with `lookup`, skipping characters already `seen`. Colour and
/// very large faces are skipped (egui draws neither well).
pub fn faces_for_text(
    text: &str,
    have: &[Arc<FontFace>],
    seen: &mut HashSet<char>,
    mut covered: impl FnMut(char) -> bool,
    lookup: impl Fn(char) -> Option<Arc<FontFace>>,
) -> Vec<Arc<FontFace>> {
    let mut out: Vec<Arc<FontFace>> = Vec::new();
    for c in text.chars().filter(|c| wanted(*c)) {
        if !seen.insert(c) || covered(c) || have.iter().chain(&out).any(|f| f.covers(c)) {
            continue;
        }
        let Some(f) = lookup(c) else { continue };
        if !accepts_face(f.data().len(), f.is_color()) || have.iter().chain(&out).any(|h| h.id() == f.id()) {
            continue;
        }
        out.push(f);
    }
    out
}

impl UiFonts {
    /// Look at `text` the interface is about to draw; borrow faces for what it can't draw.
    pub fn note(&mut self, ctx: &egui::Context, text: &str) {
        if !text.chars().any(|c| wanted(c) && !self.seen.contains(&c)) {
            return;
        }
        let font = egui::FontId::proportional(13.0);
        let new = ctx.fonts_mut(|f| {
            faces_for_text(
                text,
                &self.faces,
                &mut self.seen,
                |c| f.has_glyph(&font, c),
                |c| FontDb::global().fallback_for_filtered(c, u32::MAX, |face| accepts_face(face.data().len(), face.is_color())),
            )
        });
        let mut total: usize = self.faces.iter().map(|f| f.data().len()).sum();
        let accepted: Vec<_> = new
            .into_iter()
            .filter(|f| {
                let n = f.data().len();
                if aggregate_accepts(total, n) {
                    total += n;
                    true
                } else {
                    log::debug!("interface fallback face {} skipped: aggregate font budget", f.family);
                    false
                }
            })
            .collect();
        if !accepted.is_empty() {
            log::debug!("interface fallback fonts: {}", accepted.iter().map(|f| f.family.as_str()).collect::<Vec<_>>().join(", "));
            self.faces.extend(accepted);
            self.changed = true;
        }
    }

    /// Look through the text of a presentation the interface shows (once per revision): its
    /// title, slide text and names, notes, comments, sections and layout names.
    pub fn note_doc(&mut self, ctx: &egui::Context, st: &deckcraft_engine::DocState) {
        if self.doc == Some((st.uid, st.revision)) {
            return;
        }
        self.doc = Some((st.uid, st.revision));
        let text = ui_text(st);
        self.note(ctx, &text);
    }

    /// Install the interface fonts again when faces were added. True if they were.
    pub fn apply(&mut self, ctx: &egui::Context) -> bool {
        if !std::mem::take(&mut self.changed) {
            return false;
        }
        crate::theme::install_fonts_with(ctx, &self.faces);
        ctx.request_repaint();
        true
    }
}

/// The text of a presentation that the interface can show (not the slides' pictures of it).
pub fn ui_text(st: &deckcraft_engine::DocState) -> String {
    let mut out = st.title();
    let mut add = |s: &str| {
        if s.chars().any(wanted) {
            out.push('\n');
            out.push_str(s);
        }
    };
    let body = |b: &deckcraft_model::text::TextBody, add: &mut dyn FnMut(&str)| {
        for p in &b.paragraphs {
            for r in &p.runs {
                add(&r.text);
            }
        }
    };
    let doc = &st.doc;
    add(&doc.props.title);
    for sec in &doc.sections {
        add(&sec.name);
    }
    let shapes = |list: &[deckcraft_model::Shape], add: &mut dyn FnMut(&str)| {
        deckcraft_model::walk(list, &mut |sh, _| {
            add(&sh.name);
            add(&sh.title);
            add(&sh.descr);
            if let Some(t) = &sh.text {
                body(t, add);
            }
            if let deckcraft_model::ShapeKind::Table(tb) = &sh.kind {
                for row in &tb.rows {
                    for cell in &row.cells {
                        body(&cell.text, add);
                    }
                }
            }
        });
    };
    for s in &doc.slides {
        add(&s.name);
        shapes(&s.shapes, &mut add);
        body(&s.notes, &mut add);
        let mut stack: Vec<&deckcraft_model::Comment> = s.comments.iter().collect();
        while let Some(c) = stack.pop() {
            add(&c.author);
            add(&c.text);
            stack.extend(c.replies.iter());
        }
    }
    for m in &doc.masters {
        add(&m.name);
        for l in &m.layouts {
            add(&l.name);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn face(family: &str) -> Arc<FontFace> {
        FontDb::global().face(family, "Regular")
    }

    /// The aggregate font budget accepts the exact cap and rejects overflow or over-budget
    /// additions.
    #[test]
    fn aggregate_font_budget_is_bounded_and_checked() {
        assert!(aggregate_accepts(0, MAX_FACE_BYTES), "the first face is accepted at the per-face limit");
        assert!(aggregate_accepts(2 * MAX_FACE_BYTES, MAX_FACE_BYTES), "three faces reach the aggregate limit");
        assert!(!aggregate_accepts(3 * MAX_FACE_BYTES, 1), "one byte over the aggregate limit is refused");
        assert!(!aggregate_accepts(usize::MAX, 1), "overflow is refused, not wrapped");
    }

    #[test]
    fn interface_face_policy_rejects_colour_and_oversize_faces() {
        assert!(accepts_face(MAX_FACE_BYTES, false), "the documented size boundary remains accepted");
        assert!(!accepts_face(MAX_FACE_BYTES + 1, false), "an oversized outline face is still rejected");
        assert!(!accepts_face(1, true), "even a small colour face is still rejected");
        assert!(!accepts_face(MAX_FACE_BYTES + 1, true));
    }

    /// One face for a whole string, looked up once per character, never for characters the
    /// interface already draws, and never twice.
    #[test]
    fn one_face_per_script_and_each_character_once() {
        let inter = face("Inter");
        let looked = std::cell::Cell::new(0);
        let lookup = |_: char| {
            looked.set(looked.get() + 1);
            Some(inter.clone())
        };
        let mut seen = HashSet::new();
        let got = faces_for_text("한국어 발표", &[], &mut seen, |_| false, lookup);
        assert_eq!(got.len(), 1, "one face, not one per character");
        // Inter doesn't cover Hangul, so every character was looked up once.
        assert_eq!(looked.get(), 5);
        assert!(faces_for_text("한국어 발표", &got, &mut seen, |_| false, lookup).is_empty(), "already seen");
        assert_eq!(looked.get(), 5);
        let mut seen = HashSet::new();
        assert!(faces_for_text("한국어", &[], &mut seen, |_| true, lookup).is_empty(), "the interface draws it");
        assert!(faces_for_text("abc  \n\t\u{E001}", &[], &mut HashSet::new(), |_| false, lookup).is_empty(), "ASCII, spaces, private use");
        assert!(faces_for_text("한", std::slice::from_ref(&inter), &mut HashSet::new(), |_| false, lookup).is_empty(), "already borrowed");
    }

    #[test]
    fn interface_text_of_a_presentation() {
        let mut s = deckcraft_engine::Session::with_new();
        let title = s.doc().unwrap().current_slide().unwrap().shapes[0].id.0;
        s.execute("text.set", &serde_json::json!({"id": title, "text": "한국어 제목"})).unwrap();
        s.execute("slide.notes", &serde_json::json!({"text": "発表者ノート"})).unwrap();
        let t = ui_text(s.doc().unwrap());
        assert!(t.contains("한국어 제목") && t.contains("発表者ノート"), "{t}");
        assert!(t.starts_with(&s.doc().unwrap().title()));
    }
}
