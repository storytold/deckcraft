use super::*;
use deckcraft_model::Fill;
use deckcraft_model::defaults;
use deckcraft_model::text::{Paragraph, Run};
use deckcraft_model::{LayoutType, Presentation, Xfrm};

fn setup(text: &str, w: f64, h: f64) -> (Presentation, Shape) {
    let mut p = Presentation::default();
    let lay = defaults::layout_of_kind(&p, LayoutType::TitleAndContent).unwrap();
    let mut s = defaults::new_slide(&mut p, lay).expect("slide");
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

/// Glyph count and width of `text` laid out in one line of `family` at 40 pt, kern threshold `kern`.
fn glyphs_and_width(family: &str, text: &str, kern: Option<f64>) -> (usize, f64) {
    let (p, mut sh) = setup(text, 2000.0, 300.0);
    let para = &mut sh.text.as_mut().unwrap().paragraphs[0];
    para.props.bullet = Some(Bullet::None);
    for r in &mut para.runs {
        r.props.font = Some(family.into());
        r.props.size = Some(40.0);
        r.props.kern = kern;
    }
    let l = lay(&p, &sh, 2000.0, 300.0);
    let n = l.runs.iter().map(|r| r.glyphs.len()).sum();
    (n, l.lines[0].extent.1 - l.lines[0].extent.0)
}

/// The first installed or bundled family of `families` for which `pick` holds.
fn first_family(families: &[&str], pick: impl Fn(&FontFace) -> bool) -> Option<String> {
    let db = FontDb::global();
    families.iter().filter(|f| db.has_family(f)).map(|f| db.face(f, "Regular")).find(|f| pick(f)).map(|f| f.family.clone())
}

const LATIN_FAMILIES: &[&str] = &["Carlito", "Calibri", "DejaVu Sans", "Liberation Sans", "Inter", "Arial", "Cambria", "Times New Roman"];

#[test]
fn latin_text_draws_no_ligatures() {
    // PowerPoint keeps "fi", "ffi" and Calibri's "ti" apart, although the fonts have them as
    // standard ligatures.
    let ligates = |f: &FontFace| deckcraft_fonts::shape(f, "office", &[], |c| c).len() < 6;
    let Some(family) = first_family(LATIN_FAMILIES, ligates) else {
        eprintln!("skipped: no font with an ffi ligature");
        return;
    };
    let (n, _) = glyphs_and_width(&family, "office fifth", Some(0.0));
    assert_eq!(n, 12, "{family}: one glyph per character");
}

#[test]
fn pairs_are_kerned_only_from_the_kern_size() {
    let kerns = |f: &FontFace| {
        let w = |feat: &[deckcraft_fonts::Feature]| deckcraft_fonts::shape(f, "AVATAR", feat, |c| c).iter().map(|g| g.x_advance).sum::<i32>();
        w(&[]) != w(&deckcraft_fonts::feature("-kern").into_iter().collect::<Vec<_>>())
    };
    let Some(family) = first_family(LATIN_FAMILIES, kerns) else {
        eprintln!("skipped: no font that kerns AVATAR");
        return;
    };
    // `kern` is the smallest size kerned; 0 is no kerning.
    let (_, off) = glyphs_and_width(&family, "AVATAR WAVE", Some(0.0));
    let (_, on) = glyphs_and_width(&family, "AVATAR WAVE", Some(12.0));
    let (_, at) = glyphs_and_width(&family, "AVATAR WAVE", Some(40.0));
    let (_, above) = glyphs_and_width(&family, "AVATAR WAVE", Some(48.0));
    assert!(on < off - 2.0, "{family}: kerned {on} vs {off}");
    assert!((at - on).abs() < 0.01, "kerned at exactly the threshold size");
    assert!((above - off).abs() < 0.01, "40 pt is below a 48 pt threshold");
}

/// Width of the first paragraph "AVATAR WAVE" (kerned from 12 pt) when a second paragraph's run
/// has character spacing `spc`.
fn kerned_width_beside_spacing(family: &str, spc: f64) -> f64 {
    let (p, mut sh) = setup("AVATAR WAVE", 2000.0, 300.0);
    let t = sh.text.as_mut().unwrap();
    let mut second = t.paragraphs[0].clone();
    second.runs[0].props.spacing = Some(spc);
    t.paragraphs.push(second);
    for para in &mut t.paragraphs {
        para.props.bullet = Some(Bullet::None);
        for r in &mut para.runs {
            r.props.font = Some(family.into());
            r.props.size = Some(40.0);
            r.props.kern = Some(12.0);
        }
    }
    let l = lay(&p, &sh, 2000.0, 300.0);
    l.lines[0].extent.1 - l.lines[0].extent.0
}

#[test]
fn character_spacing_in_the_body_keeps_kerning() {
    let kerns = |f: &FontFace| {
        let w = |feat: &[deckcraft_fonts::Feature]| deckcraft_fonts::shape(f, "AVATAR", feat, |c| c).iter().map(|g| g.x_advance).sum::<i32>();
        w(&[]) != w(&deckcraft_fonts::feature("-kern").into_iter().collect::<Vec<_>>())
    };
    let Some(family) = first_family(LATIN_FAMILIES, kerns) else {
        eprintln!("skipped: no font that kerns AVATAR");
        return;
    };
    let (_, off) = glyphs_and_width(&family, "AVATAR WAVE", Some(0.0));
    let (_, on) = glyphs_and_width(&family, "AVATAR WAVE", Some(12.0));
    assert!(on < off);
    // Spacing elsewhere in the box leaves the pairs kerned (Calibri, Segoe UI, Times New Roman in
    // PowerPoint; only Arial drops them).
    for spc in [0.0, 0.5, -1.0, 0.6, 2.0] {
        let w = kerned_width_beside_spacing(&family, spc);
        assert!((w - on).abs() < 0.01, "{family} spc {spc}: {w} vs {on}");
    }
}

#[test]
fn ligatures_stay_for_scripts_that_need_them() {
    let has = |f: &[deckcraft_fonts::Feature], tag: &str| f.iter().any(|x| Some(*x) == deckcraft_fonts::feature(tag));
    let cells = |s: &str| {
        s.chars()
            .map(|ch| Cell { ch, style: 0, face: 0, level: 0, glyphs: vec![], adv: 0.0, head: 0, boundary: true, upright: false })
            .collect::<Vec<_>>()
    };
    let latin = shaping_features(&cells("office"), &[], Some(*b"Latn"));
    assert!(has(&latin, "-liga") && has(&latin, "-clig") && has(&latin, "-kern"));
    // Arabic needs its ligatures; emoji sequences are joined by them.
    assert!(!has(&shaping_features(&cells("\u{644}\u{627}"), &[], Some(*b"Arab")), "-liga"));
    assert!(!has(&shaping_features(&cells("a\u{1F469}\u{200D}\u{1F4BB}"), &[], Some(*b"Latn")), "-liga"));
    assert!(!has(&shaping_features(&cells("12"), &[], None), "-liga"));
}

#[test]
fn only_the_first_character_of_a_bullet_is_drawn() {
    // "••" (seen in the wild) draws one dot in PowerPoint.
    for (char, glyphs) in [("\u{2022}\u{2022}", 1), ("\u{2022}", 1), ("-->", 1)] {
        let (p, mut sh) = setup("One", 600.0, 300.0);
        sh.text.as_mut().unwrap().paragraphs[0].props.bullet = Some(Bullet::Char { char: char.into() });
        let l = lay(&p, &sh, 600.0, 300.0);
        let bullet = l.runs.iter().find(|r| r.chars == (0, 0)).expect("bullet run");
        assert_eq!(bullet.glyphs.len(), glyphs, "{char:?}");
    }
    // The first grapheme cluster is kept whole.
    assert_eq!(map_symbol_bullet("e\u{301}x", None), "e\u{301}");
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
fn saved_shrink_scale_is_drawn() {
    // A file saved with fontScale 62.5% and lnSpcReduction 20% draws at that scale even though
    // the text would fit at full size, as PowerPoint draws it.
    let (p, mut sh) = setup("Short", 600.0, 300.0);
    sh.text.as_mut().unwrap().body.autofit = Some(AutoFit::Shrink { font_scale: Some(0.625), line_reduction: 0.2 });
    let l = lay(&p, &sh, 600.0, 300.0);
    assert_eq!((l.font_scale, l.line_reduction), (0.625, 0.2));
    // Text our metrics find overflowing at the saved scale is still drawn at it, as PowerPoint
    // draws it until the text changes; with no saved scale it shrinks to fit.
    let text = (0..30).map(|i| format!("Line number {i}")).collect::<Vec<_>>().join("\n");
    let (p, mut sh) = setup(&text, 600.0, 200.0);
    sh.text.as_mut().unwrap().body.autofit = Some(AutoFit::Shrink { font_scale: Some(0.9), line_reduction: 0.1 });
    let l = lay(&p, &sh, 600.0, 200.0);
    assert_eq!((l.font_scale, l.line_reduction), (0.9, 0.1));
    sh.text.as_mut().unwrap().body.autofit = Some(AutoFit::Shrink { font_scale: Some(1.0), line_reduction: 0.0 });
    let l = lay(&p, &sh, 600.0, 200.0);
    assert!(l.font_scale < 0.9, "{}", l.font_scale);
    // Nonsense saved values are ignored.
    let (p, mut sh) = setup("Short", 600.0, 300.0);
    sh.text.as_mut().unwrap().body.autofit = Some(AutoFit::Shrink { font_scale: Some(f64::NAN), line_reduction: 5.0 });
    let l = lay(&p, &sh, 600.0, 300.0);
    assert_eq!((l.font_scale, l.line_reduction), (1.0, 0.2));
}

/// A body whose file has `<a:normAutofit/>` with no `fontScale` is the schema default 100 %:
/// PowerPoint draws it as it is, even overflowing. DeckCraft draws it the same (#724).
#[test]
fn bare_normautofit_draws_at_full_size() {
    let text = (0..30).map(|i| format!("Line number {i}")).collect::<Vec<_>>().join("\n");
    let (p, mut sh) = setup(&text, 600.0, 200.0);
    sh.text.as_mut().unwrap().body.autofit = Some(AutoFit::Shrink { font_scale: None, line_reduction: 0.0 });
    let l = lay(&p, &sh, 600.0, 200.0);
    assert_eq!(l.font_scale, 1.0, "a body with no saved fontScale draws at 100%");
    assert!(l.overflow, "it overflows, as PowerPoint draws it");
}

#[test]
fn exact_line_spacing_ends_at_the_last_descent() {
    // 12 pt text at exactly 26.59 pt per line (a table cell PowerPoint sizes to 24.3 pt of text):
    // the baseline at 80 % of the pitch, the text ending at the last line's descent.
    let (p, mut sh) = setup("One\nTwo", 600.0, 300.0);
    for para in &mut sh.text.as_mut().unwrap().paragraphs {
        para.props.line_spacing = Some(Spacing::Pts(26.59));
        para.props.bullet = Some(Bullet::None);
        for r in &mut para.runs {
            r.props.size = Some(12.0);
        }
    }
    let l = lay(&p, &sh, 600.0, 300.0);
    let first = &l.lines[0];
    assert!((first.baseline - first.top - 26.59 * 0.8).abs() < 0.01, "{:?}", first.baseline - first.top);
    // The text ends at the last line's descent: above its full pitch by 20 % of it less the
    // descent (~2.3 pt for 12 pt type).
    let short = (l.lines[1].bottom - l.lines[0].top) - l.content_height;
    assert!(short > 1.0 && short < 26.59 * 0.2, "{short}");
    // Proportional spacing keeps full pitches.
    for para in &mut sh.text.as_mut().unwrap().paragraphs {
        para.props.line_spacing = Some(Spacing::Pct(1.0));
    }
    let l = lay(&p, &sh, 600.0, 300.0);
    assert!((l.content_height - (l.lines[1].bottom - l.lines[0].top)).abs() < 0.01);
}

#[test]
fn line_spacing_reduction_comes_off_the_percentage() {
    // 90 % line spacing and 10 pt before paragraphs, shrunk with a 20 % line spacing reduction:
    // PowerPoint draws 70 % lines and keeps the 10 pt.
    let (p, mut sh) = setup("One\nTwo\nThree", 600.0, 400.0);
    for (i, para) in sh.text.as_mut().unwrap().paragraphs.iter_mut().enumerate() {
        para.props.line_spacing = Some(Spacing::Pct(0.9));
        para.props.space_before = Some(Spacing::Pts(10.0));
        para.props.bullet = Some(Bullet::None);
        if i == 0 {
            para.runs[0].text.push_str(" wraps onto a second line because this paragraph is long enough to wrap in a narrow box");
        }
    }
    let ctx = Ctx::for_slide(&p, &p.slides[0]).unwrap();
    let body = sh.text.clone().unwrap();
    let bp = resolve::body(&ctx, &sh);
    let opts = Opts { rect: Rect::new(0.0, 0.0, 200.0, 400.0), fields: &NoFields, prompt_color: None, no_shrink: true };
    let full = layout_scaled(&ctx, &sh, &body, &bp, &opts, 1.0, 0.0);
    let reduced = layout_scaled(&ctx, &sh, &body, &bp, &opts, 1.0, 0.2);
    let pitch = |l: &TextLayout| l.lines[1].top - l.lines[0].top;
    // Lines within the first paragraph: 0.7 / 0.9 of the full pitch.
    assert!(full.lines[1].para == 0 && reduced.lines[1].para == 0);
    assert!((pitch(&reduced) / pitch(&full) - 0.7 / 0.9).abs() < 0.005, "{} {}", pitch(&reduced), pitch(&full));
    // The space before the next paragraph is the same 10 pt.
    let gap = |l: &TextLayout| {
        let k = l.lines.iter().position(|x| x.para == 1).unwrap();
        l.lines[k].top - l.lines[k - 1].bottom
    };
    assert!((gap(&full) - 10.0).abs() < 1e-6 && (gap(&reduced) - 10.0).abs() < 1e-6, "{} {}", gap(&full), gap(&reduced));
}

#[test]
fn shrink_gives_up_at_once_when_even_the_smallest_scale_overflows() {
    // Found by fuzzing: a tiny box with 20,000 paragraphs and shrink on overflow took 15 s to
    // open, laying out the whole body 13 times in a search that could never fit.
    let text = (0..400).map(|i| format!("Line number {i}")).collect::<Vec<_>>().join("\n");
    let (p, sh) = setup(&text, 100.0, 20.0);
    crate::LAYOUT_PASSES.with(|n| n.set(0));
    let l = lay(&p, &sh, 100.0, 20.0);
    assert!(l.overflow && l.font_scale <= 0.11, "scale {}", l.font_scale);
    assert!(crate::LAYOUT_PASSES.with(|n| n.get()) <= 2, "{} passes", crate::LAYOUT_PASSES.with(|n| n.get()));
    // Text that fits somewhere in the range still gets the search.
    let (p, sh) = setup(&(0..12).map(|i| format!("Line number {i}")).collect::<Vec<_>>().join("\n"), 600.0, 200.0);
    let l = lay(&p, &sh, 600.0, 200.0);
    assert!(l.font_scale > 0.11 && l.font_scale < 1.0 && !l.overflow, "scale {} overflow {}", l.font_scale, l.overflow);
}

#[test]
fn space_before_the_first_paragraph_with_spc_first_last_para() {
    // 10 pt before every paragraph: skipped on the first one, unless the body asks for it.
    let (p, mut sh) = setup("One\nTwo", 600.0, 300.0);
    for para in &mut sh.text.as_mut().unwrap().paragraphs {
        para.props.space_before = Some(Spacing::Pts(10.0));
        para.props.space_after = Some(Spacing::Pts(6.0));
    }
    let plain = lay(&p, &sh, 600.0, 300.0);
    sh.text.as_mut().unwrap().body.space_first_last = Some(true);
    let both = lay(&p, &sh, 600.0, 300.0);
    assert!((both.lines[0].top - plain.lines[0].top - 10.0).abs() < 1e-6);
    assert!((both.content_height - plain.content_height - 16.0).abs() < 1e-6);
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

/// #101: the text an input method is composing is underlined in place, under the baseline of each
/// line it occupies, and only over the composed characters.
#[test]
fn composition_is_underlined_per_line() {
    let (p, sh) = setup("Hello 한국어 world", 600.0, 100.0);
    let l = lay(&p, &sh, 600.0, 100.0);
    let (a, b) = (Pos { para: 0, ch: 6 }, Pos { para: 0, ch: 9 });
    let u = l.composition_underlines(a, b);
    assert_eq!(u.len(), 1, "{u:?}");
    let (x0, x1, y) = u[0];
    let (cx0, ..) = l.caret(a).unwrap();
    let (cx1, ..) = l.caret(b).unwrap();
    assert!((x0 - cx0).abs() < 0.01 && (x1 - cx1).abs() < 0.01, "{u:?} vs carets {cx0} {cx1}");
    let line = &l.lines[0];
    assert!(y > line.baseline && y <= line.bottom, "under the baseline: {y} ({} – {})", line.baseline, line.bottom);
    // Reversed ends give the same; an empty composition, nothing.
    assert_eq!(l.composition_underlines(b, a), u);
    assert!(l.composition_underlines(a, a).is_empty());

    // Wrapped: one underline per line.
    let (p, sh) = setup("가나다라마바사아자차카타파하 가나다라마바사아자차카타파하 가나다라마바사", 120.0, 400.0);
    let l = lay(&p, &sh, 120.0, 400.0);
    assert!(l.lines.len() >= 2);
    let u = l.composition_underlines(Pos { para: 0, ch: 0 }, Pos { para: 0, ch: 40 });
    assert!(u.len() >= 2, "{u:?}");
    assert!(u.windows(2).all(|w| w[1].2 > w[0].2), "each lower than the last: {u:?}");
    // Positions past the text never panic.
    let _ = l.composition_underlines(Pos { para: 3, ch: 0 }, Pos { para: 9, ch: 99 });
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
    // An Arabic run (PowerPoint tags typed Arabic `ar-…`): its digits and brackets go with the
    // right-to-left text.
    let mut run = Run::new(MIXED);
    run.props.lang = Some("ar-SA".into());
    let (p, sh) = bidi_setup(vec![run], 2000.0, true, Align::Right);
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
fn an_english_run_keeps_digits_and_brackets_left_to_right() {
    // The same text in a run without a right-to-left language: PowerPoint keeps "2024 (Retrait) "
    // one left-to-right stretch, its number on the left.
    let (p, sh) = bidi_setup(vec![Run::new(MIXED)], 2000.0, true, Align::Right);
    let l = lay(&p, &sh, 2000.0, 300.0);
    let line = &l.lines[0];
    let mid = |k: usize| (line.edges[k].0 + line.edges[k].1) / 2.0;
    assert!(mid(0) > mid(10), "first Arabic word right of the Latin");
    assert!(mid(4) < mid(10), "number left of the Latin word");
    assert!(mid(9) < mid(10) && mid(17) > mid(16), "brackets around Retrait, unmirrored");
    assert!(mid(19) < mid(4), "last Arabic word on the left");
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

/// Width of `word` laid out in a run with `baseline` (0 = normal, 0.3 = superscript …).
fn run_width(word: &str, baseline: Option<f64>) -> f64 {
    let mut run = Run::new(word);
    run.props.size = Some(24.0);
    run.props.baseline = baseline;
    let (p, sh) = bidi_setup(vec![run], 2000.0, false, Align::Left);
    let l = lay(&p, &sh, 2000.0, 300.0);
    let line = &l.lines[0];
    line.caret_x.last().unwrap() - line.caret_x.first().unwrap()
}

#[test]
fn superscript_and_subscript_advance_at_their_drawn_size() {
    // The glyphs are drawn at two thirds of the size; advancing by the full size spread the
    // letters apart ("s u p e r").
    let normal = run_width("superscript", None);
    for b in [Some(0.3), Some(-0.25)] {
        let w = run_width("superscript", b);
        assert!((w / normal - SUB_SUPER).abs() < 0.02, "{b:?}: {w} vs {normal}");
    }
}

#[test]
fn superscript_glyphs_sit_inside_their_cells() {
    let mut run = Run::new("super");
    run.props.size = Some(24.0);
    run.props.baseline = Some(0.3);
    let (p, sh) = bidi_setup(vec![run], 2000.0, false, Align::Left);
    let l = lay(&p, &sh, 2000.0, 300.0);
    let line = &l.lines[0];
    let r = l.runs.iter().find(|r| !r.cells.is_empty()).unwrap();
    assert!((r.size - 24.0 * SUB_SUPER).abs() < 1e-6);
    for (g, (ci, _)) in r.glyphs.iter().zip(&r.cells) {
        let (x0, x1) = line.edges[*ci - line.start];
        assert!(g.1 >= x0 - 0.01 && g.1 < x1, "glyph at {} outside its cell {x0}..{x1}", g.1);
    }
}

/// A wrapping numbered paragraph with `marL` and `indent`; returns (bullet x, first line's text
/// x, second line's text x).
fn hanging(margin: f64, indent: f64, bullet: Bullet) -> (Option<f64>, f64, f64) {
    let text = "Wrapping words go on and on so the paragraph needs at least two or three lines here";
    let (p, mut sh) = setup(text, 300.0, 300.0);
    let para = &mut sh.text.as_mut().unwrap().paragraphs[0];
    para.props.bullet = Some(bullet);
    para.props.margin_left = Some(margin);
    para.props.indent = Some(indent);
    let l = lay(&p, &sh, 300.0, 300.0);
    assert!(l.lines.len() >= 2, "{} lines", l.lines.len());
    let bx = l.runs.iter().find(|r| r.cells.is_empty() && !r.glyphs.is_empty()).map(|r| r.glyphs[0].1);
    (bx, l.lines[0].caret_x[0], l.lines[1].caret_x[0])
}

#[test]
fn a_hang_past_the_box_edge_keeps_its_width_on_the_first_line() {
    // PowerPoint, marL 0 / indent -36pt: the number sits at the box's edge, the first line's text
    // 36pt in, and the wrapped lines at the margin (observed with PowerPoint's slide export).
    let num = || Bullet::AutoNum { scheme: "arabicPeriod".into(), start_at: 1 };
    let (bx, first, rest) = hanging(0.0, -36.0, num());
    assert!((first - rest - 36.0).abs() < 0.5, "first {first} rest {rest}");
    assert!((bx.unwrap() - rest).abs() < 0.5, "bullet {bx:?} rest {rest}");
    // marL 18 / indent -54: number at the edge, first line at 54, the rest at 18.
    let (bx, first, rest) = hanging(18.0, -54.0, num());
    assert!((first - rest - 36.0).abs() < 0.5, "first {first} rest {rest}");
    assert!((rest - bx.unwrap() - 18.0).abs() < 0.5, "bullet {bx:?} rest {rest}");
    // A character bullet behaves the same.
    let (_, first, rest) = hanging(0.0, -36.0, Bullet::Char { char: "•".into() });
    assert!((first - rest - 36.0).abs() < 0.5, "first {first} rest {rest}");
    // Without a bullet the first line just starts at the edge.
    let (_, first, rest) = hanging(0.0, -36.0, Bullet::None);
    assert!((first - rest).abs() < 0.5, "first {first} rest {rest}");
}

#[test]
fn an_ordinary_hang_still_puts_the_text_at_the_margin() {
    let (bx, first, rest) = hanging(36.0, -36.0, Bullet::AutoNum { scheme: "arabicPeriod".into(), start_at: 1 });
    assert!((first - rest).abs() < 0.5, "first {first} rest {rest}");
    assert!((rest - bx.unwrap() - 36.0).abs() < 0.5);
}

#[test]
fn single_spaced_lines_are_1_2_times_the_size_apart_whatever_the_font() {
    // PowerPoint spaces single-spaced lines 1.2 × the font size apart in every font, with the
    // baseline where the font's Windows ascent/descent split puts it (observed on twenty fonts).
    for size in [18.0, 40.0] {
        let mut run = Run::new("HHH");
        run.props.size = Some(size);
        let (p, mut sh) = bidi_setup(vec![run], 2000.0, false, Align::Left);
        let t = sh.text.as_mut().unwrap();
        t.body.autofit = Some(AutoFit::None);
        let second = t.paragraphs[0].clone();
        t.paragraphs.push(second.clone());
        t.paragraphs.push(second);
        for para in &mut t.paragraphs {
            para.props.line_spacing = Some(Spacing::Pct(1.0));
            para.props.space_before = Some(Spacing::Pts(0.0));
            para.props.space_after = Some(Spacing::Pts(0.0));
        }
        let l = lay(&p, &sh, 2000.0, 600.0);
        assert_eq!(l.lines.len(), 3);
        // Each line is 1.2 × the size tall (the face is not compared across lines: another
        // test's thread can load the family between them, see #252).
        for line in &l.lines {
            assert!((line.bottom - line.top - size * LINE_PITCH).abs() < 1e-6, "{size}: {}", line.bottom - line.top);
        }
        let face = &l.runs[0].face;
        let first = &l.lines[0];
        assert!((first.baseline - first.top - size * LINE_PITCH * face.line_split).abs() < 1e-6);
    }
}

/// The lines of `text` in an Arial-free (whatever face resolves) box `w` wide, as strings.
fn wrapped(text: &str, w: f64, set: impl Fn(&mut ParaProps)) -> Vec<String> {
    let (p, mut sh) = bidi_setup(vec![Run::new(text)], w, false, Align::Left);
    let t = sh.text.as_mut().unwrap();
    t.body.inset_l = Some(0.0);
    t.body.inset_r = Some(0.0);
    set(&mut t.paragraphs[0].props);
    let l = lay(&p, &sh, w, 600.0);
    let chars: Vec<char> = text.chars().collect();
    l.lines.iter().map(|li| chars[li.start..li.end].iter().collect()).collect()
}

/// Width of `text` on one line.
fn one_line(text: &str) -> f64 {
    let (p, sh) = bidi_setup(vec![Run::new(text)], 5000.0, false, Align::Left);
    let l = lay(&p, &sh, 5000.0, 300.0);
    l.lines[0].caret_x.last().unwrap() - l.lines[0].caret_x[0]
}

#[test]
fn a_word_does_not_break_after_a_slash_or_backslash() {
    // PowerPoint (latinLnBrk off, its default) keeps "alpha/beta/gamma" whole and only breaks it
    // where it no longer fits; Unicode line breaking would allow a break after each slash.
    for sep in ["/", "\\"] {
        let text = ["alpha", "beta", "gamma", "delta", "epsilon"].join(sep);
        let w = one_line(&text) * 0.6;
        let lines = wrapped(&text, w, |_| {});
        assert!(lines.len() >= 2, "{lines:?}");
        assert!(lines[0].chars().count() > "alpha/beta".len(), "filled to the edge, not cut at a separator: {lines:?}");
    }
    // latinLnBrk on: Unicode's break after a slash is allowed again.
    let text = "alpha/beta/gamma/delta/epsilon";
    let lines = wrapped(text, one_line(text) * 0.6, |p| p.latin_line_break = Some(true));
    assert!(lines[0].ends_with('/'), "{lines:?}");
    // A slash followed by a space still breaks, and hyphens still break.
    let lines = wrapped("alpha-beta-gamma-delta-epsilon", one_line("alpha-beta-gamma-") + 1.0, |_| {});
    assert_eq!(lines[0], "alpha-beta-gamma-");
}

#[test]
fn korean_wraps_between_words() {
    let text = "한국어 안녕하세요 반갑습니다 감사합니다";
    let w = one_line("한국어 안녕하세요 반") + 0.5;
    let lines = wrapped(text, w, |_| {});
    assert_eq!(lines.iter().map(|l| l.trim_end()).collect::<Vec<_>>(), ["한국어 안녕하세요", "반갑습니다", "감사합니다"], "{lines:?}");
    // Japanese and Chinese still break between characters.
    let lines = wrapped("日本語の文章は空白なしで続きます", one_line("日本語の文章") + 0.5, |_| {});
    assert_eq!(lines[0], "日本語の文章");
}

#[test]
fn closing_punctuation_hangs_past_the_end_with_hanging_punct() {
    // PowerPoint (hangingPunct on, as its default text styles set it) lets "。" hang past the
    // edge instead of pulling the character before it onto the next line.
    let text = "白なしで続きます。句読点も含めて。";
    let w = one_line("白なしで続きます") + 0.5;
    let lines = wrapped(text, w, |p| p.hanging_punct = Some(true));
    assert_eq!(lines[0], "白なしで続きます。", "{lines:?}");
    // Without it, "。" can't start a line, so "す。" moves down together.
    let lines = wrapped(text, w, |p| p.hanging_punct = Some(false));
    assert_eq!(lines[0], "白なしで続きま", "{lines:?}");
}

#[test]
fn emoji_take_a_colour_face_when_one_is_installed() {
    // PowerPoint draws emoji (and ZWJ sequences, skin tones, flags) from the colour emoji font
    // even when the run's font has a plain glyph for some of them. Skipped without an installed
    // colour emoji font.
    let db = FontDb::global();
    let probe = db.cascade(&['😀'], &[], "Regular");
    if !probe.is_color() {
        return;
    }
    for s in ["😀", "👍🏽", "❤\u{FE0F}", "👨\u{200D}👩\u{200D}👧"] {
        let (p, sh) = bidi_setup(vec![Run::new(format!("a{s}b"))], 2000.0, false, Align::Left);
        let l = lay(&p, &sh, 2000.0, 300.0);
        let emoji = l.runs.iter().find(|r| r.cells.iter().any(|(_, c)| !c.is_ascii())).unwrap();
        assert!(emoji.face.is_color(), "{s:?} drawn with {}", emoji.face.family);
        assert!(l.runs.iter().filter(|r| r.cells.iter().all(|(_, c)| c.is_ascii())).all(|r| !r.face.is_color()), "the letters keep a text face");
    }
}

#[test]
fn a_bullet_the_text_font_lacks_falls_back_to_a_face_that_has_it() {
    // circleNumDbPlain numbers ①②…: the text font usually has no circled digits; PowerPoint draws
    // them from a fallback font instead of a missing-glyph box. Skipped when no installed or
    // bundled face has ①.
    let db = FontDb::global();
    if !db.cascade(&['①'], &[], "Regular").covers('①') {
        return;
    }
    let (p, mut sh) = setup("One\nTwo", 600.0, 300.0);
    for para in &mut sh.text.as_mut().unwrap().paragraphs {
        para.props.bullet = Some(Bullet::AutoNum { scheme: "circleNumDbPlain".into(), start_at: 1 });
    }
    let l = lay(&p, &sh, 600.0, 300.0);
    let bullets: Vec<_> = l.runs.iter().filter(|r| r.cells.is_empty()).collect();
    assert_eq!(bullets.len(), 2);
    for b in bullets {
        assert!(b.face.covers('①'), "drawn with {}, which lacks the circled digit", b.face.family);
        assert!(b.glyphs.iter().all(|g| g.0 != 0), "no missing-glyph box");
    }
}

#[test]
fn symbol_font_bullets_map_by_font() {
    // Wingdings 2 code 0xEA is a star, Wingdings 3 0x7D a right-pointing triangle; plain
    // Wingdings keeps its own table (0xA7 is a small square).
    let star = String::from_utf8(vec![0xEF, 0x83, 0xAA]).unwrap(); // U+F0EA
    assert_eq!(map_symbol_bullet(&star, Some("Wingdings 2")), "★");
    assert_eq!(map_symbol_bullet("\u{7d}", Some("Wingdings 3")), "►");
    assert_eq!(map_symbol_bullet("\u{f0a7}", Some("Wingdings")), "▪");
    assert_eq!(map_symbol_bullet("•", Some("Arial")), "•");
    assert!(is_symbol_font("Wingdings 2") && is_symbol_font("Symbol") && !is_symbol_font("Arial"));
}

#[test]
fn an_installed_symbol_font_draws_its_own_bullet() {
    // Where the deck's symbol font is installed, its glyph is drawn (as PowerPoint does), not a
    // look-alike. Skipped without Wingdings 2.
    let db = FontDb::global();
    if !db.has_family("Wingdings 2") {
        return;
    }
    let face = db.face("Wingdings 2", "Regular");
    assert_eq!(symbol_font_char(&face, "\u{ea}", Some("Wingdings 2")), Some('\u{f0ea}'));
    assert_eq!(symbol_font_char(&face, "\u{ea}", Some("Arial")), None);
}

#[test]
fn symbol_char_maps_symbol_font_codes() {
    // Inter is not a symbol font, so the codes map to look-alikes.
    let face = FontDb::global().face("Inter", "Regular");
    assert_eq!(symbol_char(&face, "Arial", 'n'), None, "not a symbol font");
    assert_eq!(symbol_char(&face, "Webdings", 'n'), Some('\u{25CF}'));
    assert_eq!(symbol_char(&face, "Webdings", '\u{F06E}'), Some('\u{25CF}'), "the private-use form of the code");
    assert_eq!(symbol_char(&face, "Wingdings", '\u{F06E}'), Some('\u{25A0}'));
    assert_eq!(symbol_char(&face, "Wingdings", '\u{100}'), None, "not a symbol code");
}

#[test]
fn symbol_font_characters_draw_in_the_sym_font() {
    // PowerPoint stores a Symbol-font character as U+F0xx with `<a:sym typeface="Symbol"/>`:
    // U+F0DE is the Symbol font's double arrow (⇒). It must come from that font, not from
    // whichever installed font happens to map U+F0DE (Wingdings draws an arrow at another angle).
    let symbol = "Symbol";
    let db = FontDb::global();
    // Some installed Symbol faces use Unicode mappings only, without the legacy U+F0xx cmap.
    // This integration test needs a face that can draw the symbol code used by the document.
    if !db.has_family(symbol) || !db.face(symbol, "Regular").covers('\u{F0DE}') {
        eprintln!("skipped: no installed Symbol face covers U+F0DE");
        return;
    }
    let (p, mut sh) = setup("x\u{F0DE}y", 600.0, 300.0);
    let para = &mut sh.text.as_mut().unwrap().paragraphs[0];
    para.props.bullet = Some(Bullet::None);
    para.runs[0].props.font = Some("Arial".into());
    para.runs[0].props.font_sym = Some(symbol.into());
    let l = lay(&p, &sh, 600.0, 300.0);
    let families: Vec<(String, (usize, usize))> = l.runs.iter().map(|r| (r.face.family.clone(), r.chars)).collect();
    assert!(families.iter().any(|(f, c)| f.eq_ignore_ascii_case(symbol) && c.0 == 1), "{families:?}");
    // The letters around it keep the run's font.
    assert!(families.iter().all(|(f, c)| c.0 == 1 || !f.eq_ignore_ascii_case(symbol)), "{families:?}");
}

#[test]
fn small_capitals_draw_smaller_than_the_capitals_beside_them() {
    let (p, mut sh) = setup("HxHx", 2000.0, 300.0);
    let para = &mut sh.text.as_mut().unwrap().paragraphs[0];
    para.props.bullet = Some(Bullet::None);
    para.runs[0].props.caps = Some(Caps::Small);
    para.runs[0].props.size = Some(40.0);
    let l = lay(&p, &sh, 2000.0, 300.0);
    let sizes: Vec<(usize, f64)> = l.runs.iter().map(|r| (r.glyphs.len(), r.size)).collect();
    // H at 40 pt, x as a capital at 80 % (four runs: the size changes at every letter).
    assert_eq!(sizes, vec![(1, 40.0), (1, 32.0), (1, 40.0), (1, 32.0)]);
}

#[test]
fn capitals_keep_characters_whose_capital_is_a_sequence() {
    assert_eq!(caps_char('a'), 'A');
    assert_eq!(caps_char('ǆ'), 'Ǆ');
    // PowerPoint draws "ß", "ﬁ" and "ŉ" unchanged in all caps.
    assert_eq!(caps_char('ß'), 'ß');
    assert_eq!(caps_char('\u{FB01}'), '\u{FB01}');
    assert_eq!(caps_char('ŉ'), 'ŉ');
    let (p, mut sh) = setup("aß", 2000.0, 300.0);
    let para = &mut sh.text.as_mut().unwrap().paragraphs[0];
    para.runs[0].props.caps = Some(Caps::All);
    let l = lay(&p, &sh, 2000.0, 300.0);
    let r = l.runs.iter().find(|r| !r.cells.is_empty()).unwrap();
    let face = &r.face;
    if face.covers('ß') {
        assert_eq!(r.glyphs[1].0, face.glyph_for('ß'), "ß is not drawn as S");
    }
}

#[test]
fn transparent_text_is_faded_once() {
    let (p, mut sh) = setup("Faded", 2000.0, 300.0);
    let run = &mut sh.text.as_mut().unwrap().paragraphs[0].runs[0];
    run.props.fill = Some(deckcraft_model::Fill::solid(
        deckcraft_model::ColorRef::rgb(deckcraft_color::Rgba::rgb(255, 0, 0)).with(deckcraft_color::ColorTransform::Alpha(40000)),
    ));
    let l = lay(&p, &sh, 2000.0, 300.0);
    let r = l.runs.iter().find(|r| !r.cells.is_empty()).unwrap();
    // 60 % transparency: 40 % opaque in all, not 40 % of 40 %.
    let opacity = r.color.a as f64 / 255.0 * r.alpha;
    assert!((opacity - 0.4).abs() < 0.01, "{opacity}");
}

/// One paragraph `text` with tab stops `tabs` (position, `algn`), no bullet or indent.
fn tabbed(text: &str, tabs: &[(f64, &str)], w: f64) -> TextLayout {
    let (p, mut sh) = setup(text, w, 300.0);
    let para = &mut sh.text.as_mut().unwrap().paragraphs[0];
    for r in &mut para.runs {
        r.props.size = Some(18.0);
    }
    let pp = &mut para.props;
    pp.bullet = Some(Bullet::None);
    pp.margin_left = Some(0.0);
    pp.indent = Some(0.0);
    pp.default_tab = Some(72.0);
    pp.tabs = Some(tabs.iter().map(|(pos, a)| deckcraft_model::text::TabStop { pos: *pos, align: a.to_string() }).collect());
    lay(&p, &sh, w, 300.0)
}

#[test]
fn tab_stops_align_the_text_after_them() {
    let x0 = |l: &TextLayout| l.inner.x0;
    // Right: the text ends at the stop. Centre: centred on it. Decimal: the "." at the stop.
    let l = tabbed("a\tright", &[(200.0, "r")], 600.0);
    assert!((l.lines[0].extent.1 - x0(&l) - 200.0).abs() < 0.01, "{:?}", l.lines[0].extent);
    let l = tabbed("a\tmid", &[(200.0, "ctr")], 600.0);
    let (s, e) = (l.lines[0].edges[2].0, l.lines[0].edges[4].1);
    assert!(((s + e) / 2.0 - x0(&l) - 200.0).abs() < 0.01);
    let l = tabbed("x\t1234.567", &[(150.0, "dec")], 600.0);
    assert!((l.lines[0].edges[6].0 - x0(&l) - 150.0).abs() < 0.01, "the point sits at the stop");
    // Text without a point ends at a decimal stop.
    let l = tabbed("x\t12,5", &[(150.0, "dec")], 600.0);
    assert!((l.lines[0].extent.1 - x0(&l) - 150.0).abs() < 0.01);
    // Text wider than the room before a right stop starts where the tab is; it never moves back.
    let l = tabbed("x\twider than the gap", &[(20.0, "r")], 600.0);
    assert!((l.lines[0].edges[2].0 - l.lines[0].edges[0].1).abs() < 0.01);
    // A left stop and then default stops every 72 pt.
    let l = tabbed("a\tb\tc", &[(30.0, "l")], 600.0);
    assert!((l.lines[0].edges[2].0 - x0(&l) - 30.0).abs() < 0.01);
    assert!((l.lines[0].edges[4].0 - x0(&l) - 72.0).abs() < 0.01);
}

#[test]
fn a_tab_past_the_end_moves_to_the_next_line() {
    // PowerPoint: "abcdefghij\tk\tl\tm…" in a narrow box puts the tab that doesn't fit on the next
    // line, so the text after it lands on that line's first stop.
    let l = tabbed("abcdefghij\tk", &[], 130.0);
    assert_eq!(l.lines.len(), 2);
    assert_eq!(l.lines[1].start, 10, "the tab starts the second line: {:?}", l.lines.iter().map(|x| (x.start, x.end, x.extent)).collect::<Vec<_>>());
    assert!((l.lines[1].edges[1].0 - l.inner.x0 - 72.0).abs() < 0.01);
}

/// Underline decorations of "ab cd" underlined `u` at 40 pt.
fn underline_decos(u: &str) -> (TextLayout, Vec<Deco>) {
    let (p, mut sh) = setup("ab cd", 2000.0, 300.0);
    let para = &mut sh.text.as_mut().unwrap().paragraphs[0];
    para.props.bullet = Some(Bullet::None);
    para.runs[0].props.size = Some(40.0);
    para.runs[0].props.underline = Some(u.into());
    let l = lay(&p, &sh, 2000.0, 300.0);
    let d = l.decos.iter().filter(|d| !d.behind).cloned().collect();
    (l, d)
}

#[test]
fn underlines_sit_at_the_font_underline_in_their_style() {
    let (l, sng) = underline_decos("sng");
    let r = l.runs.iter().find(|r| !r.cells.is_empty()).unwrap();
    let k = 40.0 / r.face.upem;
    let base = l.lines[0].baseline;
    assert_eq!(sng.len(), 1);
    assert!((sng[0].rect.y0 - base - r.face.underline.0 * k).abs() < 0.01, "top at the font's underline position");
    assert!((sng[0].rect.height() - (r.face.underline.1 * k).max(0.5)).abs() < 0.01);
    let t = sng[0].rect.height();
    // Double: two thin lines spanning from one thickness above the single line to its bottom.
    let (_, dbl) = underline_decos("dbl");
    assert_eq!(dbl.len(), 2);
    assert!((dbl[0].rect.y0 - (sng[0].rect.y0 - t)).abs() < 0.01 && (dbl[1].rect.y1 - sng[0].rect.y1).abs() < 0.01);
    // Heavy is thicker; dotted is dots as long as the line is thick.
    let (_, heavy) = underline_decos("heavy");
    assert!(heavy[0].rect.height() > t * 1.4);
    let (_, dotted) = underline_decos("dotted");
    assert_eq!(dotted[0].line, DecoLine::Dashed(Dash::Dot));
    let pieces = dotted[0].pieces();
    assert!(pieces.len() > 3);
    assert!((pieces[0].width() - t).abs() < 0.01 && (pieces[1].x0 - pieces[0].x1 - t).abs() < 0.01);
    // Wavy: a zigzag between the box's top and bottom.
    let (_, wavy) = underline_decos("wavy");
    let (pts, w) = wavy[0].wave().expect("a wave");
    assert!(pts.len() > 4 && w > 0.0 && w < t);
    assert!(pts[0].y < pts[1].y);
    // "words" underlines the spaces too, as PowerPoint draws it: one line.
    let (_, words) = underline_decos("words");
    assert_eq!(words.len(), 1);
}

#[test]
fn no_break_after_a_zero_width_space_or_before_an_em_dash() {
    let (allowed, _) = break_opportunities("ab\u{200B}cd", false);
    assert!(!allowed[3], "PowerPoint does not break after a zero-width space");
    let (allowed, _) = break_opportunities("ab\u{2014}cd", false);
    assert!(!allowed[2] && allowed[3], "an em dash ends a line, never starts one");
}

#[test]
fn a_line_broken_at_a_soft_hyphen_shows_a_hyphen() {
    let (p, mut sh) = setup("Soft\u{AD}hyphenated\u{AD}words", 150.0, 300.0);
    let para = &mut sh.text.as_mut().unwrap().paragraphs[0];
    para.props.bullet = Some(Bullet::None);
    para.runs[0].props.size = Some(24.0);
    let l = lay(&p, &sh, 150.0, 300.0);
    assert!(l.lines.len() >= 2);
    let first = &l.lines[0];
    let shy = first.end - 1;
    assert_eq!(first.end, 5, "breaks after the soft hyphen: {:?}", l.lines.iter().map(|x| (x.start, x.end)).collect::<Vec<_>>());
    let (x0, x1) = first.edges[shy - first.start];
    assert!(x1 - x0 > 2.0, "the hyphen takes room");
    let r = l.runs.iter().find(|r| r.cells.iter().any(|c| c.0 == shy)).expect("a glyph for the soft hyphen");
    assert_eq!(r.glyphs[r.cells.iter().position(|c| c.0 == shy).unwrap()].0, r.face.glyph_for('-'));
    // Soft hyphens draw only where a line ends; in a wide box, nowhere.
    let ends: Vec<usize> = l.lines.iter().map(|x| x.end.saturating_sub(1)).collect();
    for r in &l.runs {
        for (g, c) in r.glyphs.iter().zip(&r.cells) {
            assert!(c.1 != '\u{AD}' || ends.contains(&c.0) || g.0 != r.face.glyph_for('-'), "mid-line soft hyphen drawn");
        }
    }
    let wide = lay(&p, &sh, 2000.0, 300.0);
    assert_eq!(wide.lines.len(), 1);
    assert!(wide.runs.iter().all(|r| r.cells.iter().all(|c| c.1 != '\u{AD}' || r.glyphs.iter().all(|g| g.0 != r.face.glyph_for('-')))));
}

#[test]
fn small_kana_and_closing_brackets_follow_powerpoint_kinsoku() {
    // "っ" and "ー" never start a line: the character before them moves down too.
    let w = one_line("あいうえお") + 0.5;
    for (text, first) in [("あいうえおっかき", "あいうえ"), ("あいうえおーかき", "あいうえ"), ("あいうえおかき", "あいうえお")]
    {
        let lines = wrapped(text, w, |p| p.hanging_punct = Some(true));
        assert_eq!(lines[0], first, "{text}: {lines:?}");
    }
    // A closing bracket doesn't hang like "。" does: "「。」" overflowing by "」" moves down whole.
    let w = one_line("あいうえ「。") + 0.5;
    let lines = wrapped("あいうえ「。」かき", w, |p| p.hanging_punct = Some(true));
    assert_eq!(lines[0], "あいうえ", "{lines:?}");
}

#[test]
fn autonumber_schemes_follow_powerpoint() {
    // As PowerPoint's export draws them (start values as in the probe deck).
    for (scheme, n, want) in [
        ("ea1ChsPlain", 4, "四"),
        ("ea1ChsPeriod", 10, "十．"),
        ("ea1ChtPeriod", 21, "二十一．"),
        ("ea1JpnKorPeriod", 10, "一〇．"),
        ("ea1JpnChsDbPeriod", 13, "一三．"),
        ("arabicDbPlain", 10, "１０"),
        ("arabicDbPeriod", 13, "１３．"),
        ("circleNumDbPlain", 13, "⑬"),
        ("circleNumWdWhitePlain", 13, "③"),
        ("circleNumWdBlackPlain", 2, "❷"),
        ("thaiAlphaPeriod", 1, "ก."),
        ("thaiAlphaParenR", 4, "ง)"),
        ("thaiAlphaParenBoth", 7, "(ช)"),
        ("thaiNumPeriod", 10, "๑๐."),
        ("hindiAlphaPeriod", 4, "ई."),
        ("hindiAlpha1Period", 1, "क."),
        ("hindiNumParenR", 13, "१३)"),
        ("hebrew2Minus", 1, "א-"),
        ("arabic1Minus", 13, "ش-"),
        ("arabic2Minus", 10, "ي-"),
        ("arabicPlain", 10, "10"),
    ] {
        assert_eq!(autonum(scheme, n), want, "{scheme} {n}");
    }
    assert_eq!(chinese_num(105), "一百零五");
    assert_eq!(chinese_num(20), "二十");
    // Past the end of an alphabet the letter doubles, like "aa".
    assert_eq!(autonum("hebrew2Minus", 23), "אא-");
    assert_eq!(autonum("thaiAlphaPeriod", 0), ".");
}

#[test]
fn east_asian_vertical_text_marks_cjk_upright() {
    let (p, mut sh) = setup("日本、ABC 12", 300.0, 600.0);
    sh.text.as_mut().unwrap().body.vert = Some(deckcraft_model::text::TextDir::EaVertical);
    let l = lay(&p, &sh, 300.0, 600.0);
    assert_eq!(l.rotation, 90.0);
    let upright: Vec<bool> = l
        .runs
        .iter()
        .filter(|r| !r.cells.is_empty())
        .flat_map(|r| r.cells.iter().map(move |c| (c.1, r.upright)))
        .filter(|(c, _)| *c != ' ')
        .map(|(_, u)| u)
        .collect();
    assert_eq!(upright, vec![true, true, true, false, false, false, false, false]);
    // In vert nothing stands upright.
    sh.text.as_mut().unwrap().body.vert = Some(deckcraft_model::text::TextDir::Vertical);
    let l = lay(&p, &sh, 300.0, 600.0);
    assert!(l.runs.iter().all(|r| !r.upright));
    assert!(is_upright('あ') && is_upright('、') && is_upright('Ａ') && !is_upright('A') && !is_upright('1'));
}

#[test]
fn mongolian_lines_run_left_to_right_and_stacked_letters_stand_one_em_apart() {
    let (p, mut sh) = setup("one\ntwo", 200.0, 400.0);
    sh.text.as_mut().unwrap().body.vert = Some(deckcraft_model::text::TextDir::Vertical);
    let v = lay(&p, &sh, 200.0, 400.0);
    sh.text.as_mut().unwrap().body.vert = Some(deckcraft_model::text::TextDir::MongolianVertical);
    let m = lay(&p, &sh, 200.0, 400.0);
    assert_eq!(m.rotation, 90.0);
    // In layout space (turned a quarter turn) vert puts the first line at the top, which shows on
    // the right; mongolianVert mirrors the line places, so the first line shows on the left.
    assert!(v.lines[0].top < v.lines[1].top);
    assert!(m.lines[0].top > m.lines[1].top);
    let mid = (m.inner.y0 + m.inner.y1) / 2.0;
    assert!(((m.lines[0].top + m.lines[0].bottom) / 2.0 - mid + ((v.lines[0].top + v.lines[0].bottom) / 2.0 - mid)).abs() < 0.01);
    // Stacked: every letter upright, one em apart.
    let (p, mut sh) = setup("Lil", 200.0, 400.0);
    let t = sh.text.as_mut().unwrap();
    t.body.vert = Some(deckcraft_model::text::TextDir::Stacked);
    t.paragraphs[0].runs[0].props.size = Some(20.0);
    let s = lay(&p, &sh, 200.0, 400.0);
    assert!(s.runs.iter().filter(|r| !r.cells.is_empty()).all(|r| r.upright));
    // Its lines run left to right too.
    let (p2, mut sh2) = setup("one\ntwo", 200.0, 400.0);
    sh2.text.as_mut().unwrap().body.vert = Some(deckcraft_model::text::TextDir::Stacked);
    let st = lay(&p2, &sh2, 200.0, 400.0);
    assert!(st.lines[0].top > st.lines[1].top);
    // wordArtVertRtl stacks the same way with lines from right to left.
    sh2.text.as_mut().unwrap().body.vert = Some(deckcraft_model::text::TextDir::StackedRtl);
    let sr = lay(&p2, &sh2, 200.0, 400.0);
    assert!(sr.lines[0].top < sr.lines[1].top);
    assert!(sr.runs.iter().filter(|r| !r.cells.is_empty()).all(|r| r.upright));
    let e = &s.lines[0].edges;
    assert!((e[1].1 - e[1].0 - 20.0).abs() < 0.01 && (e[2].1 - e[2].0 - 20.0).abs() < 0.01, "{e:?}");
}

#[test]
fn symbols_after_an_emoji_keep_a_text_face() {
    // "✅ ⚠ ™": PowerPoint draws ✅ (emoji presentation) in colour and the symbols after it as text.
    let db = FontDb::global();
    if !db.cascade(&['✅'], &[], "Regular").is_color() {
        return;
    }
    let (p, sh) = bidi_setup(vec![Run::new("✅ ⚠ ™")], 2000.0, false, Align::Left);
    let l = lay(&p, &sh, 2000.0, 300.0);
    for r in &l.runs {
        for (_, c) in &r.cells {
            assert_eq!(r.face.is_color(), *c == '✅', "{c:?} drawn with {}", r.face.family);
        }
    }
}

#[test]
fn fallback_faces_do_not_move_the_baseline() {
    // PowerPoint keeps a line's baseline and pitch from the run's own font when an emoji, a symbol
    // or a script the font lacks is drawn from another face.
    let base = |text: &str| {
        let (p, sh) = bidi_setup(vec![Run::new(text)], 2000.0, false, Align::Left);
        let l = lay(&p, &sh, 2000.0, 300.0);
        (l.lines[0].baseline, l.lines[0].bottom)
    };
    let plain = base("Hxg Hxg");
    for text in ["Hxg \u{1F600} Hxg", "Hxg \u{0E2A}\u{0E27}\u{0E31}\u{0E2A}", "Hxg \u{222B}\u{2211}"] {
        let b = base(text);
        assert!((b.0 - plain.0).abs() < 0.01 && (b.1 - plain.1).abs() < 0.01, "{text:?}: {b:?} vs {plain:?}");
    }
}

#[test]
fn strikethrough_sits_at_the_font_strikeout_position() {
    let (p, mut sh) = setup("Struck", 2000.0, 300.0);
    let para = &mut sh.text.as_mut().unwrap().paragraphs[0];
    para.props.bullet = Some(Bullet::None);
    para.runs[0].props.size = Some(96.0);
    para.runs[0].props.strike = Some(Strike::Single);
    let l = lay(&p, &sh, 2000.0, 300.0);
    let r = l.runs.iter().find(|r| !r.cells.is_empty()).unwrap();
    let k = 96.0 / r.face.upem;
    let d = l.decos.iter().find(|d| !d.behind).expect("a strikethrough");
    assert!((l.lines[0].baseline - d.rect.y0 - r.face.strikeout.0 * k).abs() < 0.01, "top at the strikeout position");
    assert!((d.rect.height() - (r.face.strikeout.1 * k).max(0.5)).abs() < 0.01);
}

#[test]
fn rtl_columns_start_on_the_right_and_just_anchoring_sits_at_the_bottom() {
    let text = "word ".repeat(200);
    let (p, mut sh) = setup(&text, 400.0, 200.0);
    let t = sh.text.as_mut().unwrap();
    t.body.columns = Some(2);
    t.body.col_spacing = Some(20.0);
    t.paragraphs[0].runs[0].props.size = Some(12.0);
    let ltr = lay(&p, &sh, 400.0, 200.0);
    sh.text.as_mut().unwrap().body.rtl_col = Some(true);
    let rtl = lay(&p, &sh, 400.0, 200.0);
    let first = |l: &TextLayout| l.lines.iter().find(|x| x.column == 0).map(|x| x.extent.0).unwrap();
    let second = |l: &TextLayout| l.lines.iter().find(|x| x.column == 1).map(|x| x.extent.0).unwrap();
    assert!(first(&ltr) < second(&ltr));
    assert!(first(&rtl) > second(&rtl), "with rtlCol the first column is on the right");
    // anchor="just" and "dist" draw like "b".
    let (p, mut sh) = setup("one line", 400.0, 300.0);
    sh.text.as_mut().unwrap().body.anchor = Some(Anchor::Bottom);
    let b = lay(&p, &sh, 400.0, 300.0).lines[0].top;
    for a in [Anchor::Justified, Anchor::Distributed] {
        sh.text.as_mut().unwrap().body.anchor = Some(a);
        assert!((lay(&p, &sh, 400.0, 300.0).lines[0].top - b).abs() < 0.01, "{a:?}");
    }
}

#[test]
fn stacked_combining_marks_take_a_face_that_draws_them() {
    // A letter with marks the run's font lacks draws in a face that has them all (no boxes).
    let db = FontDb::global();
    let cluster = ['Z', '\u{0334}', '\u{0321}', '\u{031B}'];
    let run_face = db.face("Inter", "Regular");
    let f = db.cascade(&cluster, std::slice::from_ref(&run_face), "Regular");
    if !deckcraft_fonts::covers_cluster(&run_face, &cluster) && ["DejaVu Sans", "Segoe UI"].iter().any(|fam| db.has_family(fam)) {
        // Make sure a face with the marks is loaded, then the cluster must take one.
        for fam in ["DejaVu Sans", "Segoe UI"] {
            if db.has_family(fam) {
                let _ = db.face(fam, "Regular");
            }
        }
        let f = db.cascade(&cluster, std::slice::from_ref(&run_face), "Regular");
        assert!(deckcraft_fonts::covers_cluster(&f, &cluster), "{} lacks the marks", f.family);
    }
    let _ = f;
}

/// #736: `anchorCtr` centres the text block horizontally, whatever the paragraphs' own
/// alignment, and overflowing content centres symmetrically past both edges.
#[test]
fn anchorctr_centres_the_text_block() {
    let text = "The shape is top - center - overriding paragraph right alignment.";
    // The block's centre: (min left + max right) over the laid-out lines, in box coordinates.
    let block_centre = |l: &TextLayout| -> f64 {
        // Empty lines carry no ink and sit where their alignment left them: not part of the block.
        let ink = l.lines.iter().filter(|li| li.end > li.start);
        let (min, max) = ink.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), li| (a.min(li.extent.0), b.max(li.extent.1)));
        (min + max) / 2.0
    };
    let para = |align: Option<Align>, text: &str| Paragraph {
        props: ParaProps { align, ..Default::default() },
        runs: vec![Run::new(text)],
        ..Default::default()
    };
    let anchored =
        |paragraphs: Vec<Paragraph>| TextBody { body: BodyProps { anchor_ctr: Some(true), ..Default::default() }, paragraphs, ..Default::default() };
    // One right-aligned line in a wide box: the block is centred, not pushed past the right edge.
    let body_right = anchored(vec![para(Some(Align::Right), text)]);
    let (p, mut sh) = setup("", 828.0, 104.0);
    sh.text = Some(body_right);
    let l = lay(&p, &sh, 828.0, 104.0);
    assert!((block_centre(&l) - 414.0).abs() <= 1.0, "right-aligned: centre {}, want 414", block_centre(&l));
    // Left-aligned text centres the same way.
    let body_left = anchored(vec![para(None, text)]);
    let (p, mut sh) = setup("", 828.0, 104.0);
    sh.text = Some(body_left);
    let l = lay(&p, &sh, 828.0, 104.0);
    assert!((block_centre(&l) - 414.0).abs() <= 1.0, "left-aligned: centre {}, want 414", block_centre(&l));
    // A trailing empty paragraph carries no ink and must not skew the block.
    let body_empty = anchored(vec![para(Some(Align::Right), text), para(None, "")]);
    let (p, mut sh) = setup("", 828.0, 104.0);
    sh.text = Some(body_empty);
    let l = lay(&p, &sh, 828.0, 104.0);
    assert!((block_centre(&l) - 414.0).abs() <= 1.0, "trailing empty paragraph: centre {}, want 414", block_centre(&l));
    // Mixed alignments: the drawn block (not either alignment edge) is what centres.
    let body_mixed = anchored(vec![para(Some(Align::Right), text), para(None, "short")]);
    let (p, mut sh) = setup("", 828.0, 104.0);
    sh.text = Some(body_mixed);
    let l = lay(&p, &sh, 828.0, 104.0);
    assert!((block_centre(&l) - 414.0).abs() <= 1.0, "mixed: centre {}, want 414", block_centre(&l));
    // Content wider than the box overflows symmetrically past both edges (no wrap).
    let body_wide = TextBody {
        body: BodyProps { anchor_ctr: Some(true), wrap: Some(false), ..Default::default() },
        paragraphs: vec![para(Some(Align::Right), text)],
        ..Default::default()
    };
    let (p, mut sh) = setup("", 300.0, 104.0);
    sh.text = Some(body_wide);
    let l = lay(&p, &sh, 300.0, 104.0);
    assert!((block_centre(&l) - 150.0).abs() <= 2.0, "overflowing: centre {}, want 150", block_centre(&l));
    // Control: without anchorCtr a right-aligned line still ends at the box's right edge.
    let body_control = TextBody { body: BodyProps::default(), paragraphs: vec![para(Some(Align::Right), text)], ..Default::default() };
    let (p, mut sh) = setup(text, 828.0, 104.0);
    sh.text = Some(body_control);
    let l = lay(&p, &sh, 828.0, 104.0);
    assert!(
        l.lines.iter().all(|li| li.extent.1 > 810.0),
        "control: right-aligned at the box edge, got {:?}",
        l.lines.iter().map(|li| li.extent).collect::<Vec<_>>()
    );
}

#[test]
fn empty_bullets_reserve_the_typed_indent_without_becoming_content() {
    for rtl in [false, true] {
        for level in [0, 1, 2] {
            let (p, mut sh) = setup("First\n", 600.0, 300.0);
            for para in &mut sh.text.as_mut().unwrap().paragraphs {
                para.level = level;
                para.props.rtl = Some(rtl);
                para.props.align = Some(if rtl { Align::Right } else { Align::Left });
            }
            let empty = lay(&p, &sh, 600.0, 300.0);
            assert_eq!(empty.empty_bullets.len(), 1);
            assert_eq!(empty.empty_bullets[0].para, 1);
            assert!(empty.runs.iter().all(|r| r.para == 0), "empty bullet must not be exported");
            let caret = empty.caret(Pos { para: 1, ch: 0 }).unwrap();
            let bx = empty.empty_bullets[0].glyphs[0].1;
            assert!(if rtl { bx >= caret.0 } else { bx < caret.0 });
            deckcraft_model::edit::insert(sh.text.as_mut().unwrap(), (1, 0), if rtl { "אבג" } else { "Next" }, None);
            let typed = lay(&p, &sh, 600.0, 300.0);
            assert!(typed.empty_bullets.is_empty());
            let after = typed.caret(Pos { para: 1, ch: 0 }).unwrap();
            assert!(
                (caret.0 - after.0).abs() < 1e-6 && (caret.1 - after.1).abs() < 1e-6 && (caret.2 - after.2).abs() < 1e-6,
                "typing must not move the line's start: {caret:?} vs {after:?}"
            );
        }
    }
}

#[test]
fn empty_numbered_paragraph_previews_the_next_number_without_consuming_it() {
    let (p, mut sh) = setup("One\n\nTwo", 600.0, 300.0);
    for para in &mut sh.text.as_mut().unwrap().paragraphs {
        para.props.bullet = Some(Bullet::AutoNum { scheme: "arabicPeriod".into(), start_at: 4 });
    }
    let l = lay(&p, &sh, 600.0, 300.0);
    let preview = &l.empty_bullets[0];
    let next = l.runs.iter().find(|r| r.para == 2 && r.cells.is_empty()).unwrap();
    let ids = |r: &GlyphRun| r.glyphs.iter().map(|g| g.0).collect::<Vec<_>>();
    assert_eq!(preview.para, 1);
    assert_eq!(ids(preview), ids(next), "both should use 5.");
    let expected = measure_str(&preview.face, preview.size, "5.").1;
    assert_eq!(ids(preview), expected.iter().map(|g| g.0).collect::<Vec<_>>());
}

#[test]
fn empty_plain_paragraphs_and_soft_breaks_have_no_bullet_preview() {
    let (p, mut sh) = setup("", 600.0, 300.0);
    sh.text.as_mut().unwrap().paragraphs[0].props.bullet = Some(Bullet::None);
    assert!(lay(&p, &sh, 600.0, 300.0).empty_bullets.is_empty());
    sh.text = Some(TextBody::from_text("First"));
    deckcraft_model::edit::insert(sh.text.as_mut().unwrap(), (0, 5), "\u{b}", None);
    let l = lay(&p, &sh, 600.0, 300.0);
    assert_eq!(l.lines.len(), 2);
    assert!(l.empty_bullets.is_empty(), "a soft line break is not a new list item");
}
