//! `a:defPPr` colours in `p:defaultTextStyle` as PowerPoint writes them, kept on open (#810).

use std::io::{Cursor, Read, Write};

use deckcraft_model::defaults;
use deckcraft_model::style::{ColorRef, Fill};
use deckcraft_model::text::TextBody;

/// Imports a one-slide deck whose text box runs set no colour of their own, with
/// `ppt/presentation.xml`'s `p:defaultTextStyle` replaced by `dts`, and returns every run's
/// resolved fill (what the renderer draws).
fn resolved_fills(dts: &str) -> Vec<Option<Fill>> {
    let mut p = defaults::new_presentation(None);
    let sid = p.slides[0].id;
    let new_id = deckcraft_model::ShapeId(p.alloc_id().expect("id"));
    let s = p.slide_mut(sid).expect("slide");
    // A plain text box: no placeholder and no styles of its own, so its runs take the
    // presentation's default text style.
    s.shapes.push(deckcraft_model::Shape {
        id: new_id,
        xfrm: Some(deckcraft_geom::Xfrm::new(100.0, 100.0, 400.0, 100.0)),
        text: Some(TextBody::from_text("plain")),
        ..Default::default()
    });
    let bytes = deckcraft_pptx::export(&p).expect("export");
    let mut z = zip::ZipArchive::new(Cursor::new(&bytes)).expect("zip");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let o = zip::write::SimpleFileOptions::default();
    for i in 0..z.len() {
        let mut f = z.by_index(i).expect("entry");
        let mut d = vec![];
        f.read_to_end(&mut d).expect("read");
        if f.name() == "ppt/presentation.xml" {
            let s = String::from_utf8(d).expect("utf8");
            let t = s.find("<p:defaultTextStyle").expect("defaultTextStyle");
            let end = "</p:defaultTextStyle>";
            let b = t + s[t..].find(end).expect("end") + end.len();
            d = format!("{}{dts}{}", &s[..t], &s[b..]).into_bytes();
        }
        out.start_file(f.name(), o).expect("start");
        out.write_all(&d).expect("write");
    }
    let bytes = out.finish().expect("finish").into_inner();
    let q = deckcraft_pptx::import(&bytes).expect("import");
    let slide = &q.slides[0];
    let ctx = deckcraft_model::resolve::Ctx::for_slide(&q, slide).expect("ctx");
    let mut fills = vec![];
    for sh in slide.shapes.iter().filter_map(|s| s.text.as_ref().map(|t| (s, t))) {
        for para in sh.1.paragraphs.iter() {
            for run in para.runs.iter() {
                fills.push(deckcraft_model::resolve::run(&ctx, sh.0, para, &run.props).fill);
            }
        }
    }
    fills
}

const NS: &str =
    r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main""#;

/// The colours of text that takes its formatting from `p:defaultTextStyle` were dropped, so it
/// drew in the plain default colour instead of the colour the deck chose (#810).
#[test]
fn default_text_style_colours_are_kept() {
    let white = Some(Fill::solid(ColorRef::rgb(deckcraft_color::Rgba::WHITE)));
    let dts = format!(
        r#"<p:defaultTextStyle {NS}><a:defPPr><a:defRPr><a:solidFill><a:srgbClr val="FFFFFF"/></a:solidFill></a:defRPr></a:defPPr></p:defaultTextStyle>"#
    );
    assert_eq!(resolved_fills(&dts), vec![white.clone()], "`a:defPPr` colours reach text without its own");

    // A level's own `a:lvlNpPr` still wins over the defaults.
    let dts = format!(
        r#"<p:defaultTextStyle {NS}><a:defPPr><a:defRPr><a:solidFill><a:srgbClr val="FFFFFF"/></a:solidFill></a:defRPr></a:defPPr><a:lvl1pPr><a:defRPr><a:solidFill><a:srgbClr val="FFFF00"/></a:solidFill></a:defRPr></a:lvl1pPr></p:defaultTextStyle>"#
    );
    assert_eq!(resolved_fills(&dts), vec![Some(Fill::solid(ColorRef::rgb(deckcraft_color::Rgba::rgb(255, 255, 0))))], "the level entry wins");
}
