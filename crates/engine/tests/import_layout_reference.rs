//! Original synthetic import/layout checks and explicitly opted-in private diagnostics.
//! Diagnostics deliberately exclude document text, names, paths and parser error details.
use std::io::{Cursor, Write};

use deckcraft_engine::{DocState, Session, cmd::file};
use deckcraft_fonts::CRAFT_FONTS;
use deckcraft_geom::Rect;
use deckcraft_model::{
    Presentation, Shape,
    resolve::{self, Ctx},
    text::AutoFit,
};
use deckcraft_text::{NoFields, Opts, TextLayout};
use serde::Deserialize;
use serde_json::{Value, json};

const NS: &str = "xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0' xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0' xmlns:svg='urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0'";

fn fixture(wrap: bool, size: u32) -> Vec<u8> {
    let wrap = if wrap { "wrap" } else { "no-wrap" };
    let content = format!(
        "<office:document-content {NS} office:version='1.3'><office:automatic-styles><style:style style:name='Frame' style:family='graphic'><style:graphic-properties draw:auto-grow-height='true' draw:fit-to-size='false' style:shrink-to-fit='false' fo:wrap-option='{wrap}' fo:padding-left='7.2pt' fo:padding-right='7.2pt' fo:padding-top='3.6pt' fo:padding-bottom='3.6pt'/></style:style><style:style style:name='Paint' style:family='paragraph'><style:paragraph-properties fo:background-color='#789487'/><style:text-properties fo:font-family='Liberation Sans' fo:font-size='{size}pt' fo:color='#ffffff'/></style:style></office:automatic-styles><office:body><office:presentation><draw:page draw:name='Original synthetic layout'><draw:frame draw:style-name='Frame' svg:x='40pt' svg:y='40pt' svg:width='180pt' svg:height='160pt'><draw:text-box><text:p text:style-name='Paint'>Garden workshop planning and community projects</text:p></draw:text-box></draw:frame></draw:page></office:presentation></office:body></office:document-content>"
    );
    let manifest = format!(
        "<manifest:manifest xmlns:manifest='urn:oasis:names:tc:opendocument:xmlns:manifest:1.0' manifest:version='1.3'><manifest:file-entry manifest:full-path='/' manifest:media-type='{}'/><manifest:file-entry manifest:full-path='content.xml' manifest:media-type='text/xml'/></manifest:manifest>",
        deckcraft_odp::MIME
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, bytes) in
        [("mimetype", deckcraft_odp::MIME.as_bytes()), ("content.xml", content.as_bytes()), ("META-INF/manifest.xml", manifest.as_bytes())]
    {
        zip.start_file(name, stored).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn layout(presentation: &Presentation, shape: &Shape) -> TextLayout {
    let ctx = Ctx::for_slide(presentation, &presentation.slides[0]).unwrap();
    let xfrm = resolve::xfrm(&ctx, shape);
    let rect = deckcraft_render::shape_geometry(shape, xfrm.w, xfrm.h).text_rect;
    deckcraft_text::layout(&ctx, shape, shape.text.as_ref().unwrap(), &Opts { rect, fields: &NoFields, prompt_color: None, no_shrink: false })
}

#[test]
fn explicit_odp_wrap_and_paragraph_paint_survive_native_reopening() {
    for size in [32, 24] {
        for wrap in [false, true] {
            let p = file::open_presentation("original-layout.odp", &fixture(wrap, size)).unwrap();
            let sh = &p.slides[0].shapes[0];
            let body = sh.text.as_ref().unwrap();
            assert_eq!(body.body.wrap, Some(wrap));
            assert_eq!(body.body.autofit, Some(AutoFit::Shape));
            assert!(body.paragraphs[0].props.background.is_some());
            let before = layout(&p, sh);
            assert!(!before.backgrounds.is_empty());
            if !wrap {
                assert_eq!(before.lines.len(), 1);
            } else {
                assert!(before.lines.len() > 1);
            }
            let native = file::save_bytes(&p, "deckcraft").unwrap();
            let reopened = deckcraft_format::load(&native).unwrap();
            let restored = &reopened.slides[0].shapes[0];
            assert_eq!(restored.text, sh.text);
            let after = layout(&reopened, restored);
            assert_eq!(before.lines.len(), after.lines.len());
            assert_eq!(before.content_width, after.content_width);
            assert_eq!(before.content_height, after.content_height);
            assert_eq!(before.backgrounds[0].rect, after.backgrounds[0].rect);
        }
    }
}

#[test]
fn bundled_liberation_sans_autofit_keeps_source_wrap_and_covers_edited_text() {
    if !CRAFT_FONTS.iter().any(|font| font.family == "Liberation Sans") {
        eprintln!("skipped optional metric check: Liberation Sans is not bundled");
        return;
    }
    for size in [32, 24] {
        for wrap in [false, true] {
            let p = file::open_presentation("original-layout.odp", &fixture(wrap, size)).unwrap();
            let before = p.slides[0].shapes[0].xfrm.unwrap();
            let id = p.slides[0].shapes[0].id.0;
            let mut session = Session::new();
            session.add_document(DocState::new(p, None, "Original synthetic layout".into()));
            session.execute("text.edit", &json!({"id":id})).unwrap();
            session.execute("text.selectAll", &json!({})).unwrap();
            session.execute("text.insert", &json!({"text":"Garden workshop planning and community projects for every season"})).unwrap();
            let p = &session.active().unwrap().doc;
            let sh = &p.slides[0].shapes[0];
            assert_eq!(sh.text.as_ref().unwrap().body.wrap, Some(wrap));
            let after = sh.xfrm.unwrap();
            let laid = layout(p, sh);
            assert!(!laid.runs.is_empty());
            for run in &laid.runs {
                assert_eq!(run.face.family, "Liberation Sans");
                assert_eq!(run.size, f64::from(size));
                assert!(CRAFT_FONTS.iter().any(|font| font.family == "Liberation Sans" && font.bytes == run.face.data()));
            }
            if wrap {
                assert_eq!(after.w, before.w);
                assert!(laid.lines.len() > 1);
            } else {
                assert!(after.w > before.w);
                assert_eq!(laid.lines.len(), 1);
            }
            assert_eq!(laid.backgrounds.len(), 1);
            let paint = laid.backgrounds[0].rect;
            for line in &laid.lines {
                assert!(line.extent.0 >= paint.x0 - 0.01 && line.extent.1 <= paint.x1 + 0.01);
                assert!(line.top >= paint.y0 - 0.01 && line.bottom <= paint.y1 + 0.01);
            }
            assert!(after.h >= laid.content_height);
        }
    }
}

#[derive(Deserialize)]
struct PrivateInput {
    format: String,
    path: std::path::PathBuf,
}

fn public_family(value: &str) -> &str {
    const PUBLIC: &[&str] = &[
        "Arial",
        "Arial Nova",
        "Aptos",
        "Aptos Display",
        "Calibri",
        "Carlito",
        "Cambria",
        "Caladea",
        "Liberation Sans",
        "Liberation Serif",
        "Liberation Mono",
        "Inter",
        "Ubuntu",
        "BIZ UDPGothic",
        "Helvetica",
        "Helvetica Neue",
        "Times New Roman",
        "Courier New",
        "DejaVu Sans",
    ];
    if PUBLIC.iter().any(|known| known.eq_ignore_ascii_case(value)) { value } else { "[other family]" }
}

fn rect_json(rect: Rect) -> Value {
    json!([rect.x0, rect.y0, rect.x1, rect.y1])
}

#[test]
fn optional_private_layout_diagnostics() {
    let Some(inputs_path) = std::env::var_os("DECKCRAFT_PRIVATE_LAYOUT_INPUTS") else { return };
    let output_path = std::env::var_os("DECKCRAFT_PRIVATE_LAYOUT_REPORT").expect("private layout report destination must be set");
    let input_bytes = std::fs::read(inputs_path).unwrap_or_else(|_| panic!("private layout input list unreadable"));
    let inputs: Vec<PrivateInput> = serde_json::from_slice(&input_bytes).unwrap_or_else(|_| panic!("private layout input list invalid"));
    assert!(!inputs.is_empty() && inputs.len() <= 8, "private layout input count outside diagnostic bound");
    let mut reports = Vec::new();
    for input in inputs {
        let safe_name = match input.format.as_str() {
            "odp" => "reference.odp",
            "pptx" => "reference.pptx",
            _ => panic!("private layout format must be odp or pptx"),
        };
        let bytes = std::fs::read(input.path).unwrap_or_else(|_| panic!("private presentation unreadable"));
        let p = file::open_presentation(safe_name, &bytes).unwrap_or_else(|_| panic!("private presentation import failed"));
        let slide = p.slides.first().expect("private presentation has no slide");
        let ctx = Ctx::for_slide(&p, slide).expect("private slide layout context unavailable");
        let mut shapes: Vec<_> = slide.shapes.iter().enumerate().filter(|(_, sh)| sh.text.as_ref().is_some_and(|text| !text.is_empty())).collect();
        shapes.sort_by(|(_, a), (_, b)| {
            let a = resolve::xfrm(&ctx, a);
            let b = resolve::xfrm(&ctx, b);
            (b.w * b.h).total_cmp(&(a.w * a.h))
        });
        shapes.truncate(3);
        shapes.sort_by_key(|(ordinal, _)| *ordinal);
        let mut boxes = Vec::new();
        for (ordinal, sh) in shapes {
            let x = resolve::xfrm(&ctx, sh);
            let body = sh.text.as_ref().unwrap();
            let bp = resolve::body(&ctx, sh);
            let l = layout(&p, sh);
            let mut requested = Vec::new();
            for para in &body.paragraphs {
                for run in &para.runs {
                    let props = resolve::run(&ctx, sh, para, &run.props);
                    let family = resolve::font_family(&ctx, &props);
                    let item = json!({"family":public_family(&family),"size":props.size});
                    if !requested.contains(&item) {
                        requested.push(item);
                    }
                }
            }
            let mut selected = Vec::new();
            for run in &l.runs {
                let item = json!({"family":public_family(&run.face.family),"size":run.size,"matches_bundled_font_bytes":CRAFT_FONTS.iter().any(|font| font.bytes == run.face.data())});
                if !selected.contains(&item) {
                    selected.push(item);
                }
            }
            let rect = deckcraft_render::shape_geometry(sh, x.w, x.h).text_rect;
            boxes.push(json!({
                "shape_ordinal_one_based":ordinal+1,"geometry_points":[x.x,x.y,x.w,x.h],
                "wrap":bp.wrap,"autofit":bp.autofit,"requested_fonts":requested,"selected_fonts":selected,
                "line_count":l.lines.len(),"content_width":l.content_width,"content_height":l.content_height,
                "font_scale":l.font_scale,"inner_bounds":rect_json(l.inner),"overflow":l.overflow,
                "natural_fit_width":deckcraft_text::fit_width(&ctx,sh,body,rect,&NoFields),
                "lines":l.lines.iter().map(|line| json!({"extent":[line.extent.0,line.extent.1],"top":line.top,"baseline":line.baseline,"bottom":line.bottom})).collect::<Vec<_>>(),
                "background_bounds":l.backgrounds.iter().map(|paint|rect_json(paint.rect)).collect::<Vec<_>>()
            }));
        }
        reports.push(
            json!({"format":input.format,"slide_index_one_based":1,"slide_size_points":[p.slide_size.width,p.slide_size.height],"text_boxes":boxes}),
        );
    }
    let output = serde_json::to_vec_pretty(&json!({"scope":"DeckCraft layout diagnostics; not external-consumer fidelity","documents":reports}))
        .expect("safe layout report serialization failed");
    std::fs::write(output_path, output).unwrap_or_else(|_| panic!("private layout report write failed"));
}
