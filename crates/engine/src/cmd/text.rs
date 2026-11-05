//! In-place text editing: entering/leaving text, typing, deleting, caret movement and selection.

use std::sync::Arc;

use deckcraft_geom::{Affine, Point, Rect};
use deckcraft_model::edit::{self as ed, Pos};
use deckcraft_model::resolve::{self, Ctx};
use deckcraft_model::text::{Align, AutoFit, Hyperlink, Paragraph, Run, RunKind, RunProps, TextBody};
use deckcraft_model::{Presentation, Shape, ShapeId, ShapeKind};
use serde_json::{Value, json};

use super::{
    CommandSpec, NO_TEXT_POINT, SELECT_TEXT_OR_SHAPE, always, bad, bool_or, cmd, edit_shapes, has_doc, has_slide, has_text, id_param, ok, shape_of,
    str_param, xfrm_of,
};
use crate::{DocState, HistoryEntry, Result, Selection, Session, Target, TextSel, push_undo};

mod autocorrect;
mod caret;
mod editing;
mod formatting;
mod ime;
mod layout;

use autocorrect::autocorrect_options;
pub use autocorrect::{AUTOCORRECT, AutoList, correct_word};
pub(crate) use caret::{apply_leading_tabs, delete_dir};
use caret::{delete, delete_surrounding, get, insert, move_caret, select, select_all, select_para, select_word, set};
use editing::{edit, exit};
pub(crate) use editing::{insert_field, insert_text, paste_body};
pub use formatting::format_range;
use formatting::{fonts_used, replace_font};
pub(crate) use formatting::{format_body, format_para, format_run, format_run_levels, link_text};
pub use ime::{IME_SURROUNDING_MAX_BYTES, ImeSurrounding, cell_range_underlines, composition_underlines, ime_surrounding, range_underlines};
use ime::{commit, compose};
pub(crate) use layout::ctx_for;
pub use layout::{caret_color, caret_line, fit_text_box, layout_for, text_frame, text_pos_at};

/// Catalogue text-editing, selection, autocorrect, font, IME, and surrounding-deletion commands
/// with their parameter docs and enabled predicates.
pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "font.used", "Fonts in Presentation", [], None, "{} → [family] (theme fonts resolved)", has_doc, fonts_used),
        cmd!(
            "font.replace",
            "Replace Fonts",
            ["Home", "Editing", "Replace"],
            None,
            "{from, to} — every explicit use of family `from`, theme fonts included → {replaced}",
            has_doc,
            replace_font
        ),
        cmd!(noundo "text.edit", "Edit Text", [], Some("Enter"), "{id?, at?: [paragraph, char], end?: bool, cell?: [row, col], notes?: bool}", has_slide, edit),
        cmd!(noundo "text.exit", "Stop Editing Text", [], None, "{}", has_doc, exit),
        cmd!(
            noundo "text.autocorrect",
            "AutoCorrect Options",
            ["Tools"],
            None,
            "{on?, twoInitials?, sentenceCaps?, days?, capsLock?, fractions?, ordinals?, lists?: bool, add?: [from, to], remove?: from} → the options and the replacement list",
            always,
            autocorrect_options
        ),
        cmd!("text.insert", "Type Text", [], None, "{text} (\\n = new paragraph, \\u000b = line break)", has_text, insert),
        // The input method's composition: shown in place, an undo step only once committed.
        CommandSpec {
            id: "text.compose",
            label: "Compose Text",
            menu: &[],
            shortcut: None,
            params: "{text, active?: [start, end]} — an input method's composition (preedit) at the insertion point; empty text cancels it",
            enabled: has_text,
            run: compose,
            journal: false,
            undoable: false,
        },
        cmd!(
            noundo "text.commit",
            "Commit Composition",
            [],
            None,
            "{text} — the input method's result replaces its composition, as one undo step",
            has_text,
            commit
        ),
        cmd!(
            "text.delete",
            "Delete Text",
            [],
            Some("Backspace"),
            "{dir?: backward|forward|wordBackward|wordForward} — the word directions are Ctrl+Backspace/Delete: they work from the start of a selection without deleting it",
            has_text,
            delete
        ),
        // An input method asks for text around the caret to be removed (macOS press-and-hold,
        // dictation, Wayland text-input). Internal: the keyboard and control protocol keep the
        // existing text.delete spelling. `bytes` says the counts are UTF-8 bytes (winit's
        // `Ime::DeleteSurrounding`), otherwise characters, both in the logical text
        // `ime_surrounding` publishes: a paragraph mark or soft line break is one `\n` byte, so a
        // range may cross paragraphs. The selection and pre-edit are kept; byte ranges round
        // inward to whole scalars. A deletion while composing stays in the composition's single
        // undo step.
        //
        // `undoable: false` is deliberate even though a non-composing deletion is undoable: the
        // handler pushes its own history entry from a snapshot taken before the composition is
        // replaced, so the generic post-run undo hook must not add a duplicate. `journal` stays
        // off for the same reason an input-method edit is not a replayable document command.
        CommandSpec {
            id: "text.deleteSurrounding",
            label: "Delete Surrounding Text",
            menu: &[],
            shortcut: None,
            params: "{before, after, bytes?: bool}",
            enabled: has_text,
            run: delete_surrounding,
            journal: false,
            undoable: false,
        },
        cmd!(noundo "text.move", "Move Insertion Point", [], None, "{to: left|right|up|down|wordLeft|wordRight|lineStart|lineEnd|paraStart|paraEnd|paraUp|paraDown|start|end, extend?: bool} — left/right are on screen: in a right-to-left paragraph left goes forward", has_text, move_caret),
        cmd!(noundo "text.select", "Select Text", [], None, "{anchor: [p, c], caret: [p, c]}", has_text, select),
        cmd!(noundo "text.selectAll", "Select All Text", [], None, "{}", has_text, select_all),
        cmd!(noundo "text.selectWord", "Select Word", [], None, "{at?: [p, c]}", has_text, select_word),
        cmd!(noundo "text.selectParagraph", "Select Paragraph", [], None, "{at?: [p, c]}", has_text, select_para),
        cmd!(
            "text.set",
            "Set Text",
            [],
            None,
            "{id?, text, cell?: [r, c]} — replaces the whole text, keeping the first run's formatting",
            has_slide,
            set
        ),
        cmd!(query "text.get", "Get Text", [], None, "{id?} → {text, paragraphs}", has_slide, get),
    ]
}

/// The text body a text selection edits.
pub fn body_of<'a>(st: &'a DocState, t: &TextSel) -> Option<&'a TextBody> {
    if t.notes {
        return st.current_slide().map(|s| &s.notes);
    }
    let sh = st.shape(t.shape)?;
    if let (Some((r, c)), ShapeKind::Table(tb)) = (t.cell, &sh.kind) {
        return tb.cell(r, c).map(|x| &x.text);
    }
    sh.text.as_ref()
}

fn body_mut<'a>(doc: &'a mut Presentation, sel: &Selection, t: &TextSel) -> Option<&'a mut TextBody> {
    if t.notes {
        return doc.slides.get_mut(sel.slide).map(|s| &mut Arc::make_mut(s).notes);
    }
    let shapes = crate::shapes_mut(doc, sel)?;
    let sh = deckcraft_model::find_shape_mut(shapes, t.shape)?;
    if let Some((r, c)) = t.cell {
        if let ShapeKind::Table(tb) = &mut sh.kind {
            return tb.cell_mut(r, c).map(|x| &mut x.text);
        }
        return None;
    }
    Some(sh.text.get_or_insert_with(TextBody::default))
}

/// Edit the active text body and selection; refits auto-sizing text boxes afterwards.
pub(crate) fn edit_text<T>(s: &mut Session, f: impl FnOnce(&mut TextBody, &mut TextSel) -> T) -> Result<T> {
    let t0 = s.doc()?.selection.text.clone().ok_or_else(|| bad("text", NO_TEXT_POINT))?;
    // Materialise an inherited placeholder box before editing (so autofit can change it).
    let r = s.edit(|doc, sel| {
        let mut t = t0.clone();
        let body = body_mut(doc, sel, &t).ok_or_else(|| bad("text", "the text being edited is gone"))?;
        let r = f(body, &mut t);
        // The saved shrink-on-overflow scale was for the old text: fit the new text afresh. A
        // body whose file carried no `fontScale` keeps none (100 %) here; `fit_shrink_scale`
        // below fills in the computed scale when the new text overflows (#724).
        if let Some(AutoFit::Shrink { font_scale, line_reduction }) = &mut body.body.autofit {
            if font_scale.is_some() {
                *font_scale = Some(1.0);
            }
            *line_reduction = 0.0;
        }
        sel.text = Some(t);
        Ok(r)
    })?;
    if !t0.notes && t0.cell.is_none() {
        fit_text_box(s, t0.shape)?;
    }
    Ok(r)
}
