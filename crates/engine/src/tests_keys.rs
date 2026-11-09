//! Text-editing keys driven through `Session::key`, as the keyboard, control channel and MCP
//! send them. Every expectation is PowerPoint for Windows' observed behaviour unless it says Mac.

use serde_json::{Value, json};

use crate::Session;
use crate::tools::Mods;

const NONE: Mods = Mods { shift: false, alt: false, cmd: false };
const CTRL: Mods = Mods { shift: false, alt: false, cmd: true };
const CTRL_SHIFT: Mods = Mods { shift: true, alt: false, cmd: true };
const SHIFT: Mods = Mods { shift: true, alt: false, cmd: false };
const ALT: Mods = Mods { shift: false, alt: true, cmd: false };

type Pos = (usize, usize);

/// A session editing a new text box holding `text` (Windows keys), caret at `at`.
fn editing(text: &str, at: Pos) -> (Session, u32) {
    editing_in(text, at, [60.0, 60.0, 600.0, 300.0])
}

fn editing_in(text: &str, at: Pos, rect: [f64; 4]) -> (Session, u32) {
    let mut s = Session::with_new();
    s.mac_keys = false;
    s.prefs.autocorrect = false;
    s.prefs.smart_quotes = false;
    s.execute("insert.textBox", &json!({"rect": rect, "text": "x"})).unwrap();
    let id = s.doc().unwrap().selection.shapes[0].0;
    s.execute("text.set", &json!({"id": id, "text": text})).unwrap();
    s.execute("text.edit", &json!({"id": id, "at": [at.0, at.1]})).unwrap();
    (s, id)
}

fn sel(s: &Session) -> (Pos, Pos) {
    let t = s.doc().unwrap().selection.text.clone().unwrap();
    (t.anchor, t.caret)
}

fn caret(s: &Session) -> Pos {
    sel(s).1
}

fn press(s: &mut Session, key: &str, mods: Mods) -> Value {
    let r = s.key(key, mods).unwrap();
    assert_eq!(r["handled"], true, "{key} {mods:?} not handled");
    r
}

fn text(s: &mut Session, id: u32) -> String {
    s.execute("text.get", &json!({"id": id})).unwrap()["text"].as_str().unwrap().to_string()
}

/// Press `key` until the caret stops moving; the carets visited (in the first paragraph).
fn walk(s: &mut Session, key: &str, mods: Mods) -> Vec<usize> {
    let mut out = vec![caret(s).1];
    for _ in 0..200 {
        let before = caret(s);
        press(s, key, mods);
        if caret(s) == before {
            break;
        }
        out.push(caret(s).1);
    }
    out
}

#[test]
fn ctrl_arrows_stop_where_powerpoint_does() {
    let t = "Hello, world!  Foo...bar baz-qux it's 3.14 e.g. a_b x@y.com (end) \"q\" end.";
    let stops = vec![0, 5, 7, 12, 15, 18, 21, 25, 33, 38, 43, 44, 45, 46, 48, 49, 50, 52, 53, 54, 55, 56, 60, 61, 64, 66, 67, 68, 70, 73, 74];
    let (mut s, _) = editing(t, (0, 0));
    assert_eq!(walk(&mut s, "ArrowRight", CTRL), stops);
    let mut back = stops.clone();
    back.reverse();
    assert_eq!(walk(&mut s, "ArrowLeft", CTRL), back);
}

#[test]
fn ctrl_arrows_over_cjk_rtl_emoji_and_marks() {
    let (mut s, _) = editing("日本語のテキスト abc 한국어", (0, 0));
    assert_eq!(walk(&mut s, "Right", CTRL), vec![0, 1, 2, 3, 4, 5, 6, 7, 9, 13, 14, 15, 16]);
    // Arabic and Hebrew words inside a left-to-right paragraph: in logical order.
    let (mut s, _) = editing("Hello مرحبا بالعالم world שלום end", (0, 0));
    assert_eq!(walk(&mut s, "Right", CTRL), vec![0, 6, 12, 20, 26, 31, 34]);
    // An emoji ZWJ sequence and a combining mark are never split.
    let (mut s, _) = editing("e\u{301}te 👨\u{200d}👩\u{200d}👧 x", (0, 0));
    assert_eq!(walk(&mut s, "Right", CTRL), vec![0, 5, 11, 12]);
    assert_eq!(walk(&mut s, "Left", CTRL), vec![12, 11, 5, 0]);
}

#[test]
fn ctrl_arrows_cross_paragraphs_line_breaks_and_tabs() {
    let (mut s, id) = editing("One two\nThree four\n\nFive", (0, 0));
    let mut seen = vec![caret(&s)];
    for _ in 0..9 {
        press(&mut s, "Right", CTRL);
        seen.push(caret(&s));
    }
    assert_eq!(seen, vec![(0, 0), (0, 4), (0, 7), (1, 0), (1, 6), (1, 10), (2, 0), (3, 0), (3, 4), (3, 4)]);
    // A line break and a tab are stops of their own.
    s.execute("text.set", &json!({"id": id, "text": "x"})).unwrap();
    s.execute("text.edit", &json!({"id": id, "at": [0, 0]})).unwrap();
    s.execute("text.selectAll", &json!({})).unwrap();
    s.execute("text.insert", &json!({"text": "Five\u{b}six seven\tTab   x"})).unwrap();
    s.execute("text.move", &json!({"to": "start"})).unwrap();
    assert_eq!(walk(&mut s, "Right", CTRL), vec![0, 4, 5, 9, 14, 15, 21, 22]);
}

#[test]
fn ctrl_shift_arrows_extend_by_words() {
    let (mut s, _) = editing("Hello world  foo, bar\nnext para", (0, 2));
    let mut carets = vec![];
    for _ in 0..6 {
        press(&mut s, "Right", CTRL_SHIFT);
        carets.push(sel(&s));
    }
    let a = (0, 2);
    assert_eq!(carets.iter().map(|x| x.1).collect::<Vec<_>>(), vec![(0, 6), (0, 13), (0, 16), (0, 18), (0, 21), (1, 0)]);
    assert!(carets.iter().all(|x| x.0 == a));
    let mut back = vec![];
    for _ in 0..7 {
        press(&mut s, "Left", CTRL_SHIFT);
        back.push(caret(&s));
    }
    assert_eq!(back, vec![(0, 21), (0, 18), (0, 16), (0, 13), (0, 6), (0, 0), (0, 0)]);
    assert_eq!(sel(&s).0, a);
    // From a caret inside a word going left: the word's start, then on.
    let (mut s, _) = editing("Hello world  foo, bar\nnext para", (1, 2));
    press(&mut s, "Left", CTRL_SHIFT);
    press(&mut s, "Left", CTRL_SHIFT);
    assert_eq!(sel(&s), ((1, 2), (0, 21)));
}

#[test]
fn ctrl_arrows_leave_a_selection_from_its_ends() {
    for (anchor, caret_at) in [((0, 2), (0, 7)), ((0, 7), (0, 2))] {
        let (mut s, _) = editing("Hello world  foo, bar", anchor);
        s.execute("text.select", &json!({"anchor": [anchor.0, anchor.1], "caret": [caret_at.0, caret_at.1]})).unwrap();
        press(&mut s, "Right", CTRL);
        assert_eq!(sel(&s), ((0, 13), (0, 13)));
        s.execute("text.select", &json!({"anchor": [anchor.0, anchor.1], "caret": [caret_at.0, caret_at.1]})).unwrap();
        press(&mut s, "Left", CTRL);
        assert_eq!(sel(&s), ((0, 0), (0, 0)));
    }
}

#[test]
fn ctrl_backspace_and_delete() {
    let (mut s, id) = editing("Hello world  foo, bar\nnext para", (0, 13));
    press(&mut s, "Backspace", CTRL);
    assert_eq!(text(&mut s, id), "Hello foo, bar\nnext para");
    assert_eq!(caret(&s), (0, 6));
    press(&mut s, "Delete", CTRL);
    assert_eq!(text(&mut s, id), "Hello , bar\nnext para".replace(" ,", ","));
    assert_eq!(caret(&s), (0, 5));
    // At a paragraph's ends they join paragraphs.
    s.execute("text.move", &json!({"to": "paraEnd"})).unwrap();
    press(&mut s, "Delete", CTRL);
    assert_eq!(text(&mut s, id), "Hello, barnext para");
    // A selection is not deleted: the word goes from its start (PowerPoint for Windows).
    let (mut s, id) = editing("Hello world", (0, 2));
    s.execute("text.select", &json!({"anchor": [0, 2], "caret": [0, 7]})).unwrap();
    press(&mut s, "Backspace", CTRL);
    assert_eq!((text(&mut s, id), sel(&s)), ("llo world".into(), ((0, 0), (0, 0))));
    let (mut s, id) = editing("Hello world", (0, 7));
    s.execute("text.select", &json!({"anchor": [0, 7], "caret": [0, 2]})).unwrap();
    press(&mut s, "Delete", CTRL);
    assert_eq!((text(&mut s, id), sel(&s)), ("He world".into(), ((0, 2), (0, 2))));
    // It is one undo step.
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(text(&mut s, id), "Hello world");
}

#[test]
fn ctrl_up_down_move_by_paragraph() {
    let t = "First paragraph that is long enough to wrap in this box.\nSecond\n\nFourth para";
    let (mut s, _) = editing_in(t, (0, 5), [60.0, 60.0, 200.0, 300.0]);
    let mut seen = vec![];
    for _ in 0..5 {
        press(&mut s, "Down", CTRL);
        seen.push(caret(&s));
    }
    assert_eq!(seen, vec![(1, 0), (2, 0), (3, 0), (3, 11), (3, 11)]);
    s.execute("text.move", &json!({"to": "end"})).unwrap();
    press(&mut s, "Left", NONE);
    let mut seen = vec![];
    for _ in 0..5 {
        press(&mut s, "Up", CTRL);
        seen.push(caret(&s));
    }
    assert_eq!(seen, vec![(3, 0), (2, 0), (1, 0), (0, 0), (0, 0)]);
    // With Shift they extend; back past the anchor the selection flips.
    s.execute("text.edit", &json!({"at": [0, 5]})).unwrap();
    press(&mut s, "Down", CTRL_SHIFT);
    press(&mut s, "Down", CTRL_SHIFT);
    assert_eq!(sel(&s), ((0, 5), (2, 0)));
    press(&mut s, "Up", CTRL_SHIFT);
    press(&mut s, "Up", CTRL_SHIFT);
    assert_eq!(sel(&s), ((0, 5), (0, 0)));
}

#[test]
fn home_end_and_ctrl_home_end() {
    let t = "First paragraph that is long enough to wrap across several lines in this narrow box.\nSecond";
    let (mut s, _) = editing_in(t, (0, 0), [60.0, 60.0, 160.0, 300.0]);
    let st = s.doc().unwrap();
    let ts = st.selection.text.clone().unwrap();
    let layout = crate::cmd::text::layout_for(st, &ts).unwrap();
    let lines: Vec<(usize, usize)> = layout.lines.iter().filter(|l| l.para == 0).map(|l| (l.start, l.end)).collect();
    assert!(lines.len() >= 3, "{lines:?}");
    let (l1, l2) = (lines[1], lines[2]);
    // End goes past a wrapped line's trailing space, to where the next line starts, as in
    // PowerPoint, and the caret stays drawn on the line it ended.
    let next = lines[2].0;
    s.execute("text.edit", &json!({"at": [0, l1.0 + 2]})).unwrap();
    press(&mut s, "Home", NONE);
    assert_eq!(caret(&s), (0, l1.0));
    press(&mut s, "End", NONE);
    assert_eq!(caret(&s), (0, next));
    assert!(s.doc().unwrap().selection.text.as_ref().unwrap().at_line_end());
    let drawn = |s: &Session| {
        let st = s.doc().unwrap();
        let ts = st.selection.text.clone().unwrap();
        let l = crate::cmd::text::layout_for(st, &ts).unwrap();
        let aff = if ts.at_line_end() { deckcraft_text::Affinity::Upstream } else { deckcraft_text::Affinity::Downstream };
        l.caret_drawn(deckcraft_text::Pos { para: ts.caret.0, ch: ts.caret.1 }, aff).unwrap()
    };
    let line1_top = layout.lines.iter().filter(|l| l.para == 0).nth(1).unwrap().top;
    assert_eq!(drawn(&s).1, line1_top);
    // End again stays on the line; Home goes back to its start, not the next line's.
    press(&mut s, "End", NONE);
    assert_eq!(caret(&s), (0, next));
    press(&mut s, "Home", SHIFT);
    assert_eq!(sel(&s), ((0, next), (0, l1.0)));
    press(&mut s, "End", SHIFT);
    assert_eq!(sel(&s), ((0, next), (0, next)));
    assert_eq!(drawn(&s).1, line1_top);
    // Down from a line's end lands on the next line, not two lines down.
    press(&mut s, "Down", NONE);
    let c = caret(&s);
    assert!(c.1 > l2.0 && c.1 <= l2.1, "{c:?} not on line {l2:?}");
    // The caret is drawn at the end of the line it belongs to.
    s.execute("text.move", &json!({"to": "lineEnd"})).unwrap();
    let ts = s.doc().unwrap().selection.text.clone().unwrap();
    assert!(ts.at_line_end());
    // Ctrl+Home / Ctrl+End, with Shift extending.
    press(&mut s, "End", CTRL);
    assert_eq!(caret(&s), (1, 6));
    press(&mut s, "Home", CTRL_SHIFT);
    assert_eq!(sel(&s), ((1, 6), (0, 0)));
    press(&mut s, "Home", CTRL);
    press(&mut s, "End", CTRL_SHIFT);
    assert_eq!(sel(&s), ((0, 0), (1, 6)));
    // A caret placed anywhere else is drawn at the start of its line again.
    s.execute("text.select", &json!({"anchor": [0, next], "caret": [0, next]})).unwrap();
    assert!(!s.doc().unwrap().selection.text.as_ref().unwrap().at_line_end());
    assert!(drawn(&s).1 > line1_top);
}

#[test]
fn right_to_left_paragraphs_swap_left_and_right() {
    let (mut s, _) = editing("مرحبا بالعالم hello world", (0, 0));
    s.execute("format.rtl", &json!({"on": true})).unwrap();
    // Ctrl+Left goes forward in a right-to-left paragraph, Ctrl+Right back.
    assert_eq!(walk(&mut s, "Left", CTRL), vec![0, 6, 14, 20, 25]);
    assert_eq!(walk(&mut s, "Right", CTRL), vec![25, 20, 14, 6, 0]);
    press(&mut s, "Left", NONE);
    assert_eq!(caret(&s), (0, 1));
    press(&mut s, "Left", CTRL_SHIFT);
    assert_eq!(sel(&s), ((0, 1), (0, 6)));
}

#[test]
fn mac_keys() {
    let (mut s, id) = editing("Hello world foo\nnext", (0, 8));
    s.mac_keys = true;
    press(&mut s, "Right", ALT);
    assert_eq!(caret(&s), (0, 12));
    press(&mut s, "Left", ALT);
    assert_eq!(caret(&s), (0, 6));
    press(&mut s, "Right", CTRL);
    assert_eq!(caret(&s), (0, 15));
    press(&mut s, "Left", CTRL);
    assert_eq!(caret(&s), (0, 0));
    press(&mut s, "Down", CTRL);
    assert_eq!(caret(&s), (1, 4));
    press(&mut s, "Up", CTRL);
    assert_eq!(caret(&s), (0, 0));
    s.execute("text.edit", &json!({"id": id, "at": [0, 11]})).unwrap();
    press(&mut s, "Backspace", ALT);
    assert_eq!(text(&mut s, id), "Hello  foo\nnext");
    // Ctrl+arrows aren't word moves on the Mac, nor Alt+arrows on Windows.
    s.mac_keys = false;
    let before = caret(&s);
    let r = s.key("Right", ALT).unwrap();
    assert_eq!((r["handled"].as_bool(), caret(&s)), (Some(false), before));
}

#[test]
fn font_size_keys() {
    let (mut s, _) = editing("hello wOrld. foo bar", (0, 0));
    s.execute("text.select", &json!({"anchor": [0, 0], "caret": [0, 5]})).unwrap();
    let size = |s: &mut Session| s.execute("format.state", &json!({})).unwrap()["size"].as_f64();
    let base = size(&mut s).unwrap_or(18.0);
    // egui names the keys `Period`, `Comma`, `CloseBracket`, `OpenBracket`.
    press(&mut s, "Period", CTRL_SHIFT);
    let up = size(&mut s).unwrap();
    assert!(up > base);
    press(&mut s, "Comma", CTRL_SHIFT);
    assert_eq!(size(&mut s), Some(base));
    press(&mut s, "CloseBracket", CTRL);
    assert_eq!(size(&mut s), Some(up));
    press(&mut s, "OpenBracket", CTRL);
    assert_eq!(size(&mut s), Some(base));
    press(&mut s, ">", CTRL_SHIFT);
    assert_eq!(size(&mut s), Some(up));
}

#[test]
fn bold_italic_underline_and_clear_on_a_caret_in_a_word() {
    let (mut s, id) = editing("hello wOrld. foo bar", (0, 8));
    let r = press(&mut s, "b", CTRL);
    assert_eq!(r["command"], "format.bold");
    let body = |s: &Session| s.doc().unwrap().shape(deckcraft_model::ShapeId(id)).unwrap().text.clone().unwrap();
    let runs: Vec<(String, Option<bool>)> = body(&s).paragraphs[0].runs.iter().map(|r| (r.text.clone(), r.props.bold)).collect();
    assert_eq!(runs.iter().find(|r| r.1 == Some(true)).map(|r| r.0.as_str()), Some("wOrld"));
    // At a word boundary nothing visible changes.
    s.execute("text.edit", &json!({"id": id, "at": [0, 5]})).unwrap();
    press(&mut s, "i", CTRL);
    assert!(body(&s).paragraphs[0].runs.iter().all(|r| r.props.italic != Some(true)));
    // Ctrl+Space clears the word's formatting.
    s.execute("text.edit", &json!({"id": id, "at": [0, 8]})).unwrap();
    press(&mut s, "Space", CTRL);
    assert!(body(&s).paragraphs[0].runs.iter().all(|r| r.props.bold != Some(true)));
    press(&mut s, "u", CTRL);
    let under: String = body(&s).paragraphs[0].runs.iter().filter(|r| r.props.underline.is_some()).map(|r| r.text.clone()).collect();
    assert_eq!(under, "wOrld");
}

#[test]
fn alignment_keys() {
    let (mut s, _) = editing("one\ntwo", (0, 1));
    for (k, want) in [("e", "ctr"), ("r", "r"), ("j", "just"), ("l", "l")] {
        press(&mut s, k, CTRL);
        let st = s.execute("format.state", &json!({})).unwrap();
        assert_eq!(st["align"], want, "Ctrl+{k}");
    }
}

#[test]
fn shift_f3_cycles_case() {
    let (mut s, id) = editing("hello world", (0, 0));
    s.execute("text.selectAll", &json!({})).unwrap();
    let mut seen = vec![];
    for _ in 0..3 {
        press(&mut s, "F3", SHIFT);
        seen.push(text(&mut s, id));
    }
    assert_eq!(seen, vec!["Hello World", "HELLO WORLD", "hello world"]);
    s.execute("text.set", &json!({"id": id, "text": "hello wOrld. foo"})).unwrap();
    s.execute("text.edit", &json!({"id": id, "at": [0, 8]})).unwrap();
    press(&mut s, "F3", SHIFT);
    assert_eq!(text(&mut s, id), "hello WORLD. foo");
    assert_eq!(sel(&s), ((0, 6), (0, 6)));
    press(&mut s, "F3", SHIFT);
    assert_eq!(text(&mut s, id), "hello world. foo");
}

#[test]
fn shift_enter_and_word_and_paragraph_selection() {
    let (mut s, id) = editing("Hello world,  foo bar\nnext", (0, 7));
    s.execute("text.selectWord", &json!({})).unwrap();
    assert_eq!(sel(&s), ((0, 6), (0, 11)));
    s.execute("text.selectWord", &json!({"at": [0, 12]})).unwrap();
    assert_eq!(sel(&s), ((0, 11), (0, 12)));
    // Triple-click: the paragraph and its mark.
    s.execute("text.selectParagraph", &json!({"at": [0, 3]})).unwrap();
    assert_eq!(sel(&s), ((0, 0), (1, 0)));
    s.execute("text.selectParagraph", &json!({"at": [1, 1]})).unwrap();
    assert_eq!(sel(&s), ((1, 0), (1, 4)));
    s.execute("text.edit", &json!({"id": id, "at": [1, 2]})).unwrap();
    press(&mut s, "Enter", SHIFT);
    let b = s.doc().unwrap().shape(deckcraft_model::ShapeId(id)).unwrap().text.clone().unwrap();
    assert_eq!(b.paragraphs.len(), 2);
    assert_eq!(b.paragraphs[1].text(), "ne\u{b}xt");
}

#[test]
fn keys_never_panic_on_any_text() {
    let texts = ["", " ", "\u{b}", "\t\t", "👨\u{200d}👩\u{200d}👧", "e\u{301}", "日本", "مرحبا", "a\n\nb", "  x  \n  "];
    let keys = ["Left", "Right", "Up", "Down", "Home", "End", "Backspace", "Delete"];
    for t in texts {
        for k in keys {
            for m in [CTRL, CTRL_SHIFT, NONE, SHIFT, ALT] {
                for at in [(0, 0), (0, 1), (5, 5)] {
                    let (mut s, _) = editing(t, at);
                    for _ in 0..3 {
                        let _ = s.key(k, m).unwrap();
                    }
                }
            }
        }
    }
}

#[test]
fn select_into_another_body_settles_the_composition() {
    let (mut s, a) = editing("hello", (0, 0));
    s.execute("insert.textBox", &json!({"rect": [60.0, 400.0, 600.0, 100.0], "text": "x"})).unwrap();
    let b = s.doc().unwrap().selection.shapes[0].0;
    s.execute("text.set", &json!({"id": b, "text": "0123456789"})).unwrap();
    s.execute("text.edit", &json!({"id": a, "at": [0, 0]})).unwrap();
    s.compose_text("ab", None).unwrap();
    s.select(|_, sel| {
        sel.text = Some(crate::TextSel { shape: deckcraft_model::ShapeId(b), ..Default::default() });
    })
    .unwrap();
    assert!(s.doc().unwrap().composition.is_none());
    s.commit_text("X").unwrap();
    assert_eq!(text(&mut s, b), "X0123456789");
    assert_eq!(text(&mut s, a), "abhello");
}

#[test]
fn select_into_another_table_cell_settles_the_composition() {
    let (mut s, _) = editing("x", (0, 0));
    let data = json!([["hello", "0123456789"]]);
    let r = s.execute("insert.table", &json!({"rows": 1, "cols": 2, "rect": [60.0, 400.0, 600.0, 60.0], "data": data})).unwrap();
    let id = deckcraft_model::ShapeId(r["id"].as_u64().unwrap() as u32);
    s.select(|_, sel| sel.text = Some(crate::TextSel { shape: id, cell: Some((0, 0)), ..Default::default() })).unwrap();
    s.compose_text("ab", None).unwrap();
    s.select(|_, sel| sel.text = Some(crate::TextSel { shape: id, cell: Some((0, 1)), ..Default::default() })).unwrap();
    assert!(s.doc().unwrap().composition.is_none());
    s.commit_text("X").unwrap();
    let st = s.doc().unwrap();
    let Some(deckcraft_model::ShapeKind::Table(t)) = st.shape(id).map(|sh| &sh.kind) else { panic!("not a table") };
    let cell = |c| t.cell(0, c).map(|x| x.text.text()).unwrap_or_default();
    assert_eq!((cell(0), cell(1)), ("abhello".to_string(), "X0123456789".to_string()));
}

/// Bold the first paragraph, select part of it and arm the Format Painter.
fn painter_armed(sticky: bool) -> (Session, u32) {
    let (mut s, id) = editing("Hello\nworld", (0, 0));
    s.execute("text.select", &json!({"anchor": [0, 0], "caret": [0, 5]})).unwrap();
    s.execute("format.bold", &json!({"on": true})).unwrap();
    s.execute("format.painter", &json!({"sticky": sticky})).unwrap();
    (s, id)
}

fn bold_at(s: &mut Session, at: Pos) -> bool {
    s.execute("text.select", &json!({"anchor": [at.0, at.1], "caret": [at.0, at.1 + 1]})).unwrap();
    s.execute("format.state", &json!({})).unwrap()["bold"] == true
}

fn pointer(s: &mut Session, kind: crate::tools::PointerKind, x: f64, y: f64) {
    s.pointer(crate::tools::PointerEvent { kind, x, y, tol: 4.0, ..Default::default() }).unwrap();
}

#[test]
fn format_painter_applies_to_a_dragged_text_selection() {
    use crate::tools::PointerKind::{Down, Drag, Up};
    let (mut s, _) = painter_armed(false);
    assert!(!bold_at(&mut s, (1, 0)));
    s.execute("text.move", &json!({"to": "start"})).unwrap();
    // Drag across the second line's text.
    pointer(&mut s, Down, 68.0, 90.0);
    pointer(&mut s, Drag, 110.0, 90.0);
    pointer(&mut s, Up, 110.0, 90.0);
    let t = s.doc().unwrap().selection.text.clone().unwrap();
    assert!(t.is_range(), "the drag should select text");
    assert!(s.painter.is_none(), "the painter is used up");
    assert!(bold_at(&mut s, (1, 0)));
}

#[test]
fn format_painter_applies_to_a_triple_clicked_paragraph() {
    use crate::tools::PointerKind::TripleClick;
    let (mut s, _) = painter_armed(true);
    s.execute("text.select", &json!({"anchor": [1, 2], "caret": [1, 2]})).unwrap();
    pointer(&mut s, TripleClick, 70.0, 90.0);
    assert!(s.painter.is_some(), "a sticky painter stays armed");
    assert!(bold_at(&mut s, (1, 0)));
}

#[test]
fn format_painter_waits_for_a_range_on_a_plain_click() {
    use crate::tools::PointerKind::{Down, Up};
    let (mut s, _) = painter_armed(false);
    pointer(&mut s, Down, 68.0, 90.0);
    pointer(&mut s, Up, 68.0, 90.0);
    assert!(s.painter.is_some());
}

#[test]
fn format_painter_on_part_of_a_bulleted_line_keeps_its_bullet() {
    use crate::tools::PointerKind::{Down, Drag, Up};
    let (mut s, _) = painter_armed(false);
    s.execute("text.select", &json!({"anchor": [1, 0], "caret": [1, 5]})).unwrap();
    s.execute("format.bullets", &json!({"on": true})).unwrap();
    let bulleted = |s: &mut Session| s.execute("format.state", &json!({})).unwrap()["bullets"] == true;
    assert!(bulleted(&mut s));
    s.execute("text.select", &json!({"anchor": [0, 0], "caret": [0, 5]})).unwrap();
    s.execute("format.painter", &json!({})).unwrap();
    pointer(&mut s, Down, 68.0, 90.0);
    pointer(&mut s, Drag, 100.0, 90.0);
    pointer(&mut s, Up, 100.0, 90.0);
    assert!(s.painter.is_none());
    assert!(bold_at(&mut s, (1, 0)));
    assert!(bulleted(&mut s), "the bullet stays");
}

/// Three bulleted paragraphs "one", "two", "three" and the painter armed from bold plain text.
fn painter_over_bullets() -> Session {
    let (mut s, _) = editing("one\ntwo\nthree\nplain", (0, 0));
    s.execute("text.select", &json!({"anchor": [3, 0], "caret": [3, 5]})).unwrap();
    s.execute("format.bold", &json!({"on": true})).unwrap();
    s.execute("text.select", &json!({"anchor": [0, 0], "caret": [2, 5]})).unwrap();
    s.execute("format.bullets", &json!({"on": true})).unwrap();
    s.execute("text.select", &json!({"anchor": [3, 0], "caret": [3, 5]})).unwrap();
    s.execute("format.painter", &json!({})).unwrap();
    s
}

fn bulleted(s: &mut Session, para: usize) -> bool {
    s.execute("text.select", &json!({"anchor": [para, 0], "caret": [para, 0]})).unwrap();
    s.execute("format.state", &json!({})).unwrap()["bullets"] == true
}

#[test]
fn format_painter_paints_only_the_paragraphs_a_selection_covers() {
    // Mid paragraph 0 to mid paragraph 2: only paragraph 1 is covered, so only it loses its bullet.
    let mut s = painter_over_bullets();
    s.execute("text.select", &json!({"anchor": [0, 1], "caret": [2, 2]})).unwrap();
    s.execute("format.painterApply", &json!({})).unwrap();
    assert_eq!([bulleted(&mut s, 0), bulleted(&mut s, 1), bulleted(&mut s, 2)], [true, false, true]);
    // Start of paragraph 0 to mid paragraph 2: the partly covered last paragraph keeps its bullet.
    let mut s = painter_over_bullets();
    s.execute("text.select", &json!({"anchor": [0, 0], "caret": [2, 2]})).unwrap();
    s.execute("format.painterApply", &json!({})).unwrap();
    assert_eq!([bulleted(&mut s, 0), bulleted(&mut s, 1), bulleted(&mut s, 2)], [false, false, true]);
}

#[test]
fn format_painter_triple_click_leaves_the_next_paragraph_alone() {
    use crate::tools::PointerKind::TripleClick;
    let mut s = painter_over_bullets();
    s.execute("text.select", &json!({"anchor": [0, 1], "caret": [0, 1]})).unwrap();
    pointer(&mut s, TripleClick, 70.0, 70.0);
    assert!(s.painter.is_none());
    assert_eq!([bulleted(&mut s, 0), bulleted(&mut s, 1), bulleted(&mut s, 2)], [false, true, true]);
}

#[test]
fn format_painter_ignores_a_double_click_that_selects_nothing() {
    use crate::tools::PointerKind::DoubleClick;
    let mut s = painter_over_bullets();
    s.execute("text.select", &json!({"anchor": [0, 0], "caret": [1, 2]})).unwrap();
    let before = sel(&s);
    pointer(&mut s, DoubleClick, 900.0, 520.0);
    assert_eq!(sel(&s), before);
    assert!(s.painter.is_some(), "a click that left the selection alone keeps the painter");
    assert!(bulleted(&mut s, 0));
}

#[test]
fn format_painter_ignores_a_shift_press_that_leaves_the_selection() {
    use crate::tools::PointerKind::{Down, Up};
    let (mut s, _) = painter_armed(false);
    // Find the caret position under the point, then select from the line start to it.
    pointer(&mut s, Down, 110.0, 90.0);
    pointer(&mut s, Up, 110.0, 90.0);
    let k = caret(&s);
    assert!(k.1 > 0);
    s.execute("text.select", &json!({"anchor": [k.0, 0], "caret": [k.0, k.1]})).unwrap();
    // Shift-pressing at the caret keeps the range, so there is nothing new to paint.
    let shift = Mods { shift: true, ..NONE };
    for kind in [Down, Up] {
        s.pointer(crate::tools::PointerEvent { kind, x: 110.0, y: 90.0, mods: shift, tol: 4.0, ..Default::default() }).unwrap();
    }
    assert_eq!(sel(&s), ((k.0, 0), k));
    assert!(s.painter.is_some(), "the painter stays armed");
    assert!(!bold_at(&mut s, (1, 0)));
}
