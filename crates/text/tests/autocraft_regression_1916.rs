use deckcraft_fonts::FontDb;
use deckcraft_geom::Rect;
use deckcraft_model::defaults;
use deckcraft_model::resolve::Ctx;
use deckcraft_model::text::{Bullet, TextBody};
use deckcraft_model::{LayoutType, Presentation, Xfrm};
use deckcraft_text::{GlyphRun, NoFields, Opts, layout};

fn lay_sym(text: &str, sym: &str) -> Vec<GlyphRun> {
    let mut p = Presentation::default();
    let lay = defaults::layout_of_kind(&p, LayoutType::TitleAndContent).expect("layout");
    let mut s = defaults::new_slide(&mut p, lay).expect("slide");
    let mut body = s.shapes.remove(1);
    body.xfrm = Some(Xfrm::new(0.0, 0.0, 600.0, 300.0));
    body.text = Some(TextBody::from_text(text));
    let para = &mut body.text.as_mut().expect("text").paragraphs[0];
    para.props.bullet = Some(Bullet::None);
    para.runs[0].props.font = Some("Arial".into());
    para.runs[0].props.font_sym = Some(sym.into());
    s.shapes = vec![body.clone()];
    p.slides = vec![std::sync::Arc::new(s)];
    let ctx = Ctx::for_slide(&p, &p.slides[0]).expect("ctx");
    let rect = Rect::new(0.0, 0.0, 600.0, 300.0);
    layout(&ctx, &body, body.text.as_ref().expect("text"), &Opts { rect, fields: &NoFields, prompt_color: None, no_shrink: false }).runs
}

#[test]
fn wingdings_arrow_without_the_sym_font_draws_the_unicode_arrow() {
    let db = FontDb::global();
    if db.has_family("Wingdings") {
        eprintln!("skipped: Wingdings is installed");
        return;
    }
    let arrow = '\u{2794}';
    let has_arrow = ["DejaVu Sans", "Noto Sans Symbols", "Noto Sans Symbols2", "Apple Symbols", "Segoe UI Symbol"]
        .iter()
        .any(|f| db.has_family(f) && db.face(f, "Regular").covers(arrow));
    if !has_arrow {
        eprintln!("skipped: no installed font draws U+2794");
        return;
    }
    // U+F0E8 is Wingdings' right arrow (the issue's "F0 E8" box).
    let runs = lay_sym("\u{F0E8} x", "Wingdings");
    let (run, i) = runs.iter().find_map(|r| r.cells.iter().position(|c| c.0 == 0).map(|i| (r, i))).expect("the symbol is drawn");
    let gid = run.glyphs.get(i).map(|g| g.0).expect("a glyph");
    assert_ne!(gid, 0, "the private-use code drew a missing-glyph box");
    assert_eq!(gid, run.face.glyph_for(arrow), "the code draws the arrow, not another character");
    // The Symbol font's bullet code (U+F0B7) draws its bullet, U+2022.
    let runs = lay_sym("\u{F0B7} x", "Symbol");
    if let Some((run, i)) = runs.iter().find_map(|r| r.cells.iter().position(|c| c.0 == 0).map(|i| (r, i)))
        && !db.has_family("Symbol")
    {
        assert_eq!(run.glyphs.get(i).map(|g| g.0), Some(run.face.glyph_for('\u{2022}')));
    }
}
