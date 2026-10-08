use super::*;
use deckcraft_model::Fill;
use deckcraft_model::defaults;
use deckcraft_model::text::{Paragraph, Run};
use deckcraft_model::{LayoutType, Presentation, Xfrm};

fn setup(text: &str, w: f64, h: f64) -> (Presentation, Shape) {
    let mut p = Presentation::default();
    let lay = defaults::layout_of_kind(&p, LayoutType::TitleAndContent).unwrap();
    let mut s = defaults::new_slide(&mut p, lay);
    let mut body = s.shapes.remove(1);
    body.xfrm = Some(Xfrm::new(0.0, 0.0, w, h));
    body.text = Some(TextBody::from_text(text));
    s.shapes = vec![body.clone()];
    p.slides = vec![std::sync::Arc::new(s)];
    (p, body)
}

fn lay(p: &Presentation, sh: &Shape, w: f64, h: f64) -> TextLayout {
    let ctx = Ctx::for_slide(p, &p.slides[0]).unwrap();
    layout(&ctx, sh, sh.text.as_ref().unwrap(), &Opts { rect: Rect::new(0.0, 0.0, w, h), fields: &NoFields, prompt_color: None, no_shrink: false })
}

#[test]
fn wraps_long_text_into_lines() {
    let text = "The quick brown fox jumps over the lazy dog and keeps on running far away";
    let (p, sh) = setup(text, 200.0, 400.0);
    let l = lay(&p, &sh, 200.0, 400.0);
    assert!(l.lines.len() >= 3, "{} lines", l.lines.len());
    for li in &l.lines {
        assert!(li.caret_x.last().unwrap() <= &(200.0 + 0.5), "line too wide: {:?}", li.caret_x.last());
        assert_eq!(li.caret_x.len(), li.end - li.start + 1);
    }
    // Lines cover the text without gaps (spaces at wraps are skipped).
    assert_eq!(l.lines[0].start, 0);
    assert!(!l.runs.is_empty());
}

#[test]
fn bullets_are_drawn_for_body_text() {
    let (p, sh) = setup("One\nTwo", 600.0, 300.0);
    let l = lay(&p, &sh, 600.0, 300.0);
    // Two text runs + two bullet runs.
    assert!(l.runs.len() >= 4, "{:?}", l.runs);
    assert_eq!(l.para_lines.len(), 2);
}

#[test]
fn shrink_on_overflow() {
    let text = (0..30).map(|i| format!("Line number {i}")).collect::<Vec<_>>().join("\n");
    let (p, sh) = setup(&text, 600.0, 200.0);
    let l = lay(&p, &sh, 600.0, 200.0);
    assert!(l.font_scale < 1.0, "scale {}", l.font_scale);
    assert!(!l.overflow || l.font_scale <= 0.11);
}

#[test]
fn caret_and_hit_roundtrip() {
    let (p, sh) = setup("Hello world", 600.0, 100.0);
    let l = lay(&p, &sh, 600.0, 100.0);
    for ch in 0..=11 {
        let pos = Pos { para: 0, ch };
        let (x, top, bot) = l.caret(pos).unwrap();
        let back = l.hit(Point::new(x + 0.1, (top + bot) / 2.0));
        assert_eq!(back, pos, "at {ch}");
    }
    let r = l.selection_rects(Pos { para: 0, ch: 0 }, Pos { para: 0, ch: 5 });
    assert_eq!(r.len(), 1);
}

#[test]
fn alignment_center_and_right() {
    let (p, mut sh) = setup("Hi", 400.0, 100.0);
    sh.text.as_mut().unwrap().paragraphs[0].props.align = Some(Align::Center);
    sh.text.as_mut().unwrap().paragraphs[0].props.bullet = Some(Bullet::None);
    let c = lay(&p, &sh, 400.0, 100.0);
    sh.text.as_mut().unwrap().paragraphs[0].props.align = Some(Align::Right);
    let r = lay(&p, &sh, 400.0, 100.0);
    let cx = c.lines[0].caret_x[0];
    let rx = r.lines[0].caret_x[0];
    assert!(rx > cx && cx > 100.0, "center {cx} right {rx}");
}

#[test]
fn empty_and_hostile_bodies_never_panic() {
    let (p, mut sh) = setup("", 10.0, 10.0);
    let _ = lay(&p, &sh, 10.0, 10.0);
    let _ = lay(&p, &sh, 0.0, 0.0);
    let t = sh.text.as_mut().unwrap();
    t.paragraphs = vec![Paragraph {
        runs: vec![
            Run::new("\t\u{b}\u{b}🙂 ﷽ 日本語"),
            Run { text: String::new(), props: Default::default(), kind: deckcraft_model::text::RunKind::Break },
        ],
        ..Default::default()
    }];
    t.paragraphs[0].props.margin_left = Some(-50.0);
    t.paragraphs[0].props.indent = Some(f64::NAN);
    t.paragraphs[0].props.line_spacing = Some(Spacing::Pct(-3.0));
    t.body.columns = Some(1000);
    let l = lay(&p, &sh, 50.0, 50.0);
    let _ = l.hit(Point::new(-100.0, 1e9));
    let _ = l.caret(Pos { para: 99, ch: 99 });
    let _ = l.selection_rects(Pos { para: 5, ch: 0 }, Pos { para: 0, ch: 3 });
}

#[test]
fn autonum_formats() {
    assert_eq!(autonum("arabicPeriod", 3), "3.");
    assert_eq!(autonum("romanUcPeriod", 4), "IV.");
    assert_eq!(autonum("alphaLcParenR", 2), "b)");
    assert_eq!(autonum("arabicParenBoth", 1), "(1)");
    assert_eq!(autonum("alphaUcPeriod", 27), "AA.");
}

#[test]
fn numbered_list_counts() {
    let (p, mut sh) = setup("a\nb\nc", 400.0, 300.0);
    for para in &mut sh.text.as_mut().unwrap().paragraphs {
        para.props.bullet = Some(Bullet::AutoNum { scheme: "arabicPeriod".into(), start_at: 1 });
    }
    let l = lay(&p, &sh, 400.0, 300.0);
    assert!(l.runs.len() >= 6);
}

// ---------- Bidirectional text and Arabic typography ----------

const MIXED: &str = "سنة 2024 (Retrait) فقط";

/// A one-paragraph body with no bullet, `rtl` and `align` set.
fn bidi_setup(runs: Vec<Run>, w: f64, rtl: bool, align: Align) -> (Presentation, Shape) {
    let (p, mut sh) = setup("x", w, 300.0);
    let t = sh.text.as_mut().unwrap();
    t.paragraphs = vec![Paragraph { runs, ..Default::default() }];
    let pp = &mut t.paragraphs[0].props;
    pp.rtl = Some(rtl);
    pp.align = Some(align);
    pp.bullet = Some(Bullet::None);
    pp.margin_left = Some(0.0);
    pp.indent = Some(0.0);
    (p, sh)
}

/// Is any face able to draw Arabic here (installed or bundled)? Shaping assertions that need
/// real Arabic glyphs are skipped without one.
fn arabic_face() -> Option<Arc<FontFace>> {
    let db = FontDb::global();
    let f = db.cascade(&['\u{0633}'], &[db.face("Inter", "Regular")], "Regular");
    f.covers('\u{0633}').then_some(f)
}

#[test]
fn rtl_mixed_string_is_reordered_per_line() {
    let (p, sh) = bidi_setup(vec![Run::new(MIXED)], 2000.0, true, Align::Right);
    let l = lay(&p, &sh, 2000.0, 300.0);
    assert_eq!(l.lines.len(), 1);
    let line = &l.lines[0];
    assert!(line.rtl);
    let n = MIXED.chars().count();
    assert_eq!(line.edges.len(), n);
    assert_eq!(line.caret_x.len(), n + 1);
    let mid = |k: usize| (line.edges[k].0 + line.edges[k].1) / 2.0;
    // Visual order right to left: سنة | 2024 | (Retrait) | فقط.
    assert!(mid(0) > mid(4), "first Arabic word right of the number");
    assert!(mid(4) > mid(10), "number right of the Latin word");
    assert!(mid(10) > mid(19), "Latin right of the last Arabic word");
    // Inside the Arabic words, logical order runs right to left; digits and Latin left to right.
    assert!(mid(0) > mid(1) && mid(1) > mid(2));
    assert!(mid(4) < mid(5) && mid(5) < mid(7));
    assert!(mid(10) < mid(16));
    // The brackets sit outside "Retrait": "(" (logically first) on its right.
    assert!(mid(9) > mid(16) && mid(17) < mid(10));
    // Right-aligned at the start edge S = W - rIns; the downstream caret at 0 is that edge.
    let right_edge = l.inner.x1;
    assert!((line.extent.1 - right_edge).abs() < 0.5, "{} vs {right_edge}", line.extent.1);
    assert!((line.caret_x[0] - line.extent.1).abs() < 0.01);
    // Levels after L1: Arabic odd, digits and Latin even.
    assert_eq!(line.levels[0], 1);
    assert_eq!(line.levels[4], 2);
    assert_eq!(line.levels[10], 2);
}

#[test]
fn rtl_paragraph_starts_at_the_right_with_the_bullet_outside() {
    let (p, mut sh) = bidi_setup(vec![Run::new("مرحبا بكم")], 400.0, true, Align::Right);
    let pp = &mut sh.text.as_mut().unwrap().paragraphs[0].props;
    pp.bullet = Some(Bullet::Char { char: "•".into() });
    pp.margin_left = Some(36.0);
    pp.indent = Some(-36.0);
    let l = lay(&p, &sh, 400.0, 300.0);
    let line = &l.lines[0];
    // Text starts marL from the right edge; the bullet hangs at S - (marL + indent) = S.
    let s_edge = l.inner.x1;
    assert!((line.extent.1 - (s_edge - 36.0)).abs() < 0.5, "text right {} expected {}", line.extent.1, s_edge - 36.0);
    let bullet = l.runs.iter().find(|r| r.chars == (0, 0)).expect("bullet run");
    let bx = bullet.glyphs[0].1;
    assert!(bx > line.extent.1 - 0.5, "bullet {bx} right of the text {}", line.extent.1);
    assert!(bx < s_edge, "bullet inside the box");
    // An LTR paragraph keeps the bullet on the left.
    sh.text.as_mut().unwrap().paragraphs[0].props.rtl = Some(false);
    sh.text.as_mut().unwrap().paragraphs[0].props.align = Some(Align::Left);
    let l = lay(&p, &sh, 400.0, 300.0);
    let bullet = l.runs.iter().find(|r| r.chars == (0, 0)).expect("bullet run");
    assert!(bullet.glyphs[0].1 < l.lines[0].extent.0);
}

#[test]
fn rtl_left_and_center_alignment() {
    let (p, sh) = bidi_setup(vec![Run::new("سلام")], 400.0, true, Align::Left);
    let l = lay(&p, &sh, 400.0, 300.0);
    assert!((l.lines[0].extent.0 - l.inner.x0).abs() < 0.5, "algn=l is flush left in RTL too");
    let (p, sh) = bidi_setup(vec![Run::new("سلام")], 400.0, true, Align::Center);
    let l = lay(&p, &sh, 400.0, 300.0);
    let c = (l.lines[0].extent.0 + l.lines[0].extent.1) / 2.0;
    assert!((c - (l.inner.x0 + l.inner.x1) / 2.0).abs() < 0.5);
}

#[test]
fn bidi_caret_stops_round_trip_through_hit_testing() {
    for rtl in [true, false] {
        let (p, sh) = bidi_setup(vec![Run::new(MIXED)], 2000.0, rtl, if rtl { Align::Right } else { Align::Left });
        let l = lay(&p, &sh, 2000.0, 300.0);
        let line = &l.lines[0];
        for (x, ch, aff) in line.visual_stops() {
            let (cx, ..) = l.caret_at(Pos { para: 0, ch }, aff).unwrap();
            assert!((cx - x).abs() < 0.01, "stop {ch} {aff:?}: caret {cx} vs stop {x}");
        }
        // Clicking just inside either side of each grapheme puts the caret on that side.
        for (a, b) in line.graphemes() {
            let Some((x0, x1)) = line.grapheme_box(a, b) else { continue };
            if x1 - x0 < 1.0 {
                continue;
            }
            let y = (line.top + line.bottom) / 2.0;
            for x in [x0 + 0.2, x1 - 0.2] {
                let (pos, aff) = l.hit_affinity(Point::new(x, y));
                let (cx, ..) = l.caret_at(pos, aff).unwrap();
                let side = if x < (x0 + x1) / 2.0 { x0 } else { x1 };
                assert!((cx - side).abs() < 0.01, "rtl={rtl} grapheme {a}: hit {x} -> {pos:?} {aff:?} caret {cx}, expected {side}");
            }
        }
    }
}

#[test]
fn visual_movement_walks_the_line_left_to_right() {
    let (p, sh) = bidi_setup(vec![Run::new(MIXED)], 2000.0, true, Align::Right);
    let l = lay(&p, &sh, 2000.0, 300.0);
    let stops = l.lines[0].visual_stops();
    let (x0, ch0, aff0) = stops[0];
    let (mut pos, mut aff) = (Pos { para: 0, ch: ch0 }, aff0);
    let mut xs = vec![x0];
    for _ in 0..200 {
        let (np, na) = l.move_caret(pos, aff, true, MoveMode::Visual);
        if (np, na) == (pos, aff) {
            break;
        }
        (pos, aff) = (np, na);
        xs.push(l.caret_at(pos, aff).unwrap().0);
    }
    assert!(xs.windows(2).all(|w| w[1] > w[0]), "moving right always moves right: {xs:?}");
    let right = l.lines[0].extent.1;
    assert!((xs.last().unwrap() - right).abs() < 0.01, "reaches the right edge");
    // Right arrow at the right edge of an RTL paragraph's only line goes nowhere.
    assert_eq!(l.move_caret(pos, aff, true, MoveMode::Visual).0, pos);
    // Moving left from the right edge in an RTL paragraph is logically forward.
    let (p1, _) = l.move_caret(Pos { para: 0, ch: 0 }, Affinity::Downstream, false, MoveMode::Visual);
    assert_eq!(p1.ch, 1);
}

#[test]
fn logical_movement_steps_over_harakat() {
    // beh + fatha + teh: the fatha belongs to the beh's grapheme.
    let (p, sh) = bidi_setup(vec![Run::new("\u{0628}\u{064E}\u{062A}")], 400.0, true, Align::Right);
    let l = lay(&p, &sh, 400.0, 300.0);
    let (n1, _) = l.move_caret(Pos { para: 0, ch: 0 }, Affinity::Downstream, true, MoveMode::Logical);
    assert_eq!(n1.ch, 2);
    let (n2, _) = l.move_caret(n1, Affinity::Downstream, false, MoveMode::Logical);
    assert_eq!(n2.ch, 0);
    // No caret stop inside a grapheme.
    assert!(l.lines[0].visual_stops().iter().all(|s| s.1 != 1));
}

#[test]
fn selection_is_the_union_of_visual_boxes() {
    // LTR paragraph with an Arabic word: visually "ab ةنس cd".
    let (p, sh) = bidi_setup(vec![Run::new("ab سنة cd")], 2000.0, false, Align::Left);
    let l = lay(&p, &sh, 2000.0, 300.0);
    // Logical [1, 5) = "b", " ", "س", "ن": two separate boxes on screen.
    let r = l.selection_rects(Pos { para: 0, ch: 1 }, Pos { para: 0, ch: 5 });
    assert_eq!(r.len(), 2, "{r:?}");
    // The whole line is one box.
    assert_eq!(l.selection_rects(Pos { para: 0, ch: 0 }, Pos { para: 0, ch: 9 }).len(), 1);
}

#[test]
fn paint_only_style_changes_keep_arabic_joining() {
    let Some(_) = arabic_face() else {
        eprintln!("skipped: no Arabic face available");
        return;
    };
    let word = "سنة";
    let glyphs = |runs: Vec<Run>| {
        let (p, sh) = bidi_setup(runs, 400.0, true, Align::Right);
        let l = lay(&p, &sh, 400.0, 300.0);
        let mut g: Vec<(u32, f64)> = l.runs.iter().flat_map(|r| r.glyphs.iter().map(|g| (g.0, g.1))).collect();
        g.sort_by(|a, b| a.1.total_cmp(&b.1));
        g.into_iter().map(|g| g.0).collect::<Vec<_>>()
    };
    let whole = glyphs(vec![Run::new(word)]);
    let mut red = Run::new("سن");
    red.props.fill = Some(Fill::solid(deckcraft_model::style::ColorRef::rgb(Rgba::from_hex("C00000").unwrap())));
    let split = glyphs(vec![red, Run::new("ة")]);
    assert_eq!(whole, split, "a colour change inside a word keeps the joined forms");
    // And the joined forms differ from the isolated letters.
    let isolated: Vec<u32> = word.chars().map(|c| arabic_face().unwrap().glyph_for(c)).collect();
    assert_ne!(whole.iter().rev().copied().collect::<Vec<_>>(), isolated);
}

#[test]
fn just_low_stretches_arabic_with_kashida() {
    if arabic_face().is_none() {
        eprintln!("skipped: no Arabic face available");
        return;
    }
    // Tatweel glyphs drawn, counted in each run's own face (the one the layout picked).
    let tatweels = |l: &TextLayout| {
        l.runs
            .iter()
            .map(|r| {
                let tg = r.face.glyph_for('\u{0640}');
                if tg == 0 { 0 } else { r.glyphs.iter().filter(|g| g.0 == tg).count() }
            })
            .sum::<usize>()
    };
    let text = "بسم الله الرحمن الرحيم نستعين به ونستغفره ونتوب إليه";
    let (p, sh) = bidi_setup(vec![Run::new(text)], 220.0, true, Align::JustLow);
    let l = lay(&p, &sh, 220.0, 600.0);
    assert!(l.lines.len() >= 2);
    let first = &l.lines[0];
    let w = l.inner.width();
    assert!((first.extent.1 - first.extent.0 - w).abs() < 0.5, "first line fills the box: {:?} vs {w}", first.extent);
    if l.runs.iter().all(|r| r.face.glyph_for('\u{0640}') == 0) {
        eprintln!("skipped: the Arabic face has no tatweel");
        return;
    }
    assert!(tatweels(&l) > 0, "kashida drawn");
    // The last line isn't stretched and sits at the start (right) edge.
    let last = l.lines.last().unwrap();
    assert!((last.extent.1 - l.inner.x1).abs() < 0.5);
    // Plain justify uses spaces: no tatweel.
    let (p, sh) = bidi_setup(vec![Run::new(text)], 220.0, true, Align::Justify);
    let l = lay(&p, &sh, 220.0, 600.0);
    assert_eq!(tatweels(&l), 0);
}

#[test]
fn wrapped_rtl_lines_break_in_logical_order() {
    let text = "واحد اثنان ثلاثة أربعة خمسة ستة سبعة ثمانية تسعة عشرة";
    let (p, sh) = bidi_setup(vec![Run::new(text)], 160.0, true, Align::Right);
    let l = lay(&p, &sh, 160.0, 600.0);
    assert!(l.lines.len() >= 2);
    // Lines cover the text in logical order: the first line holds the first words.
    assert_eq!(l.lines[0].start, 0);
    for w in l.lines.windows(2) {
        assert!(w[1].start >= w[0].end);
    }
    for line in &l.lines {
        assert!(line.extent.0 >= l.inner.x0 - 0.5, "line fits: {:?}", line.extent);
        assert!((line.extent.1 - l.inner.x1).abs() < 0.5, "flush right");
    }
    // Upstream affinity at a wrap keeps the caret on the earlier line.
    let end0 = l.lines[0].end;
    assert_eq!(l.line_of_affine(Pos { para: 0, ch: end0 }, Affinity::Upstream), Some(0));
}

#[test]
fn hostile_bidi_input_never_panics() {
    let nasty = format!("{}\t\u{b}\u{202E}abc\u{2069}{}(]))[ ١٢٣ \u{0640}\u{064E}\u{064E}\u{200D}", "\u{202B}".repeat(200), "\u{2067}".repeat(130));
    for rtl in [true, false] {
        for align in [Align::Left, Align::Right, Align::Center, Align::Justify, Align::JustLow, Align::Distributed] {
            let (p, sh) = bidi_setup(vec![Run::new(nasty.clone()), Run::new(MIXED)], 60.0, rtl, align);
            let l = lay(&p, &sh, 60.0, 60.0);
            for line in &l.lines {
                assert_eq!(line.caret_x.len(), line.end - line.start + 1);
                assert_eq!(line.edges.len(), line.end - line.start);
            }
            let _ = l.hit_affinity(Point::new(-1e9, 1e9));
            let _ = l.selection_rects(Pos { para: 0, ch: 0 }, Pos { para: 0, ch: 9999 });
            let mut pos = (Pos::default(), Affinity::Downstream);
            for i in 0..50 {
                pos = l.move_caret(pos.0, pos.1, i % 3 != 0, if i % 2 == 0 { MoveMode::Visual } else { MoveMode::Logical });
            }
            let _ = l.caret_at(Pos { para: 7, ch: 7 }, Affinity::Upstream);
        }
    }
}
