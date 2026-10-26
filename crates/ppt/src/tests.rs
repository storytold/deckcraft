//! End-to-end tests over `.ppt` files built here from records (never files made by PowerPoint).
#![allow(clippy::unwrap_used)]

use deckcraft_model::text::{Align, Bullet, RunKind};
use deckcraft_model::{Background, Fill, PhType, Rgba, ShapeKind};

use crate::doc::rt;
use crate::draw::prop;
use crate::record::build::{atom, container};
use crate::text::tests::{cf_bytes, pf_bytes};

fn le32(v: &[u32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn utf16(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn fopt(entries: &[(u16, u32)]) -> Vec<u8> {
    let mut b = vec![];
    for (id, op) in entries {
        b.extend_from_slice(&id.to_le_bytes());
        b.extend_from_slice(&op.to_le_bytes());
    }
    atom(rt::FOPT, entries.len() as u16, &b)
}

fn fsp(shape_type: u16, spid: u32, flags: u32) -> Vec<u8> {
    atom(rt::FSP, shape_type, &le32(&[spid, flags]))
}

/// A client anchor in master units: top, left, right, bottom.
fn anchor(t: i16, l: i16, r: i16, b: i16) -> Vec<u8> {
    atom(rt::CLIENT_ANCHOR, 0, &[t, l, r, b].iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>())
}

fn text_box(text_type: u32, text: &str, extra: &[Vec<u8>]) -> Vec<u8> {
    let mut kids = vec![atom(rt::TEXT_HEADER, 0, &text_type.to_le_bytes()), atom(rt::TEXT_CHARS, 0, &utf16(text))];
    kids.extend_from_slice(extra);
    container(rt::CLIENT_TEXTBOX, 0, &kids)
}

fn placeholder(kind: u8) -> Vec<u8> {
    container(rt::CLIENT_DATA, 0, &[atom(rt::PLACEHOLDER_ATOM, 0, &[0, 0, 0, 0, kind, 0, 0, 0])])
}

fn drawing(background: Option<Vec<u8>>, shapes: &[Vec<u8>]) -> Vec<u8> {
    let patriarch = container(rt::SP_CONTAINER, 0, &[atom(rt::FSPGR, 1, &[0; 16]), fsp(0, 1024, 0x5)]);
    let mut tree = vec![patriarch];
    tree.extend_from_slice(shapes);
    let mut dg = vec![container(rt::SPGR_CONTAINER, 0, &tree)];
    dg.extend(background);
    container(rt::DRAWING, 0, &[container(rt::DG_CONTAINER, 0, &dg)])
}

fn solid_background(rgb: u32) -> Vec<u8> {
    container(rt::SP_CONTAINER, 0, &[fsp(1, 1025, 0x400 | 0x800), fopt(&[(prop::FILL_COLOR, rgb), (prop::FILL_BOOLS, 0x0010_0010)])])
}

fn slide_atom(geom: u32, master_id: u32, flags: u16) -> Vec<u8> {
    let mut b = geom.to_le_bytes().to_vec();
    b.extend_from_slice(&[0; 8]);
    b.extend_from_slice(&le32(&[master_id, 0]));
    b.extend_from_slice(&flags.to_le_bytes());
    b.extend_from_slice(&[0; 2]);
    atom(rt::SLIDE_ATOM, 0, &b)
}

/// A master text style with one level: a paragraph and a character exception.
fn master_style(instance: u16, pf: &[u8], cf: &[u8]) -> Vec<u8> {
    atom(rt::TEXT_MASTER_STYLE, instance, &[&1u16.to_le_bytes()[..], pf, cf].concat())
}

const MASTER_ID: u32 = 0x8000_0001;
const SLIDE_ID: u32 = 256;

fn master() -> Vec<u8> {
    // Background, text, shadow, title, fill, accent, hyperlink, followed hyperlink.
    let scheme: Vec<u8> = [0xFFFFFFu32, 0x000000, 0x808080, 0x000099, 0x00CC00, 0x0000FF, 0x00FF00, 0xFF0000]
        .iter()
        .flat_map(|c| [(c >> 16) as u8, (c >> 8) as u8, *c as u8, 0])
        .collect();
    let decoration = container(
        rt::SP_CONTAINER,
        0,
        &[
            fsp(1, 1026, 0xA00),
            fopt(&[(prop::FILL_COLOR, 0x0800_0004), (prop::FILL_BOOLS, 0x0010_0010), (prop::LINE_BOOLS, 0x0008_0000)]),
            anchor(0, 0, 5760, 400),
        ],
    );
    container(
        rt::MAIN_MASTER,
        0,
        &[
            slide_atom(1, 0, 0),
            atom(rt::COLOR_SCHEME, 1, &scheme),
            master_style(0, &pf_bytes(1, false), &cf_bytes(true, 0, 44, [0x11, 0x22, 0x33])),
            master_style(1, &pf_bytes(0, true), &cf_bytes(false, 0, 28, [0, 0, 0])),
            drawing(Some(solid_background(0x0000_00FF)), &[decoration]),
        ],
    )
}

fn slide() -> Vec<u8> {
    let title = container(
        rt::SP_CONTAINER,
        0,
        &[fsp(1, 2049, 0xA00), fopt(&[(prop::ANCHOR_TEXT, 1)]), anchor(400, 500, 5260, 1200), placeholder(0x0D), text_box(0, "Hello\rWorld", &[])],
    );
    let picture = container(rt::SP_CONTAINER, 0, &[fsp(75, 2050, 0xA00), fopt(&[(prop::PIB, 1)]), anchor(1600, 800, 2400, 2400)]);
    let group = container(
        rt::SPGR_CONTAINER,
        0,
        &[
            container(rt::SP_CONTAINER, 0, &[atom(rt::FSPGR, 1, &le32(&[0, 0, 1000, 1000])), fsp(0, 2051, 0x201), anchor(1600, 3000, 4000, 3200)]),
            container(
                rt::SP_CONTAINER,
                0,
                &[
                    fsp(3, 2052, 0x2),
                    fopt(&[(prop::FILL_COLOR, 0x0000_FF00), (prop::LINE_WIDTH, 25400)]),
                    atom(rt::CHILD_ANCHOR, 0, &le32(&[500, 0, 1000, 500])),
                ],
            ),
        ],
    );
    // "Page *": the star is a slide number field. The first four characters are bold.
    let mut style = vec![];
    style.extend_from_slice(&7u32.to_le_bytes());
    style.extend_from_slice(&0u16.to_le_bytes());
    style.extend_from_slice(&pf_bytes(2, false));
    for (n, bold) in [(4u32, true), (3, false)] {
        style.extend_from_slice(&n.to_le_bytes());
        style.extend_from_slice(&cf_bytes(bold, 1, 12, [0xAB, 0xCD, 0xEF]));
    }
    let footer = container(
        rt::SP_CONTAINER,
        0,
        &[
            fsp(202, 2053, 0xA00),
            anchor(4000, 500, 2000, 4200),
            text_box(4, "Page *", &[atom(rt::STYLE_TEXT_PROP, 0, &style), atom(rt::SLIDE_NUMBER_MC, 0, &5u32.to_le_bytes())]),
        ],
    );
    // A hidden slide (SlideShowSlideInfoAtom fHidden).
    let mut info = vec![0u8; 16];
    info[10] = 0x4;
    container(
        rt::SLIDE,
        0,
        &[slide_atom(1, MASTER_ID, 0x7), atom(rt::SLIDE_SHOW_SLIDE_INFO, 0, &info), drawing(None, &[title, picture, group, footer])],
    )
}

fn notes() -> Vec<u8> {
    let number = container(rt::SP_CONTAINER, 0, &[fsp(1, 3073, 0xA00), anchor(0, 0, 100, 100), placeholder(0x08), text_box(2, "*", &[])]);
    let body = container(
        rt::SP_CONTAINER,
        0,
        &[fsp(1, 3074, 0xA00), anchor(100, 0, 4000, 4000), placeholder(0x0C), text_box(2, "Say hello\rThen wave", &[])],
    );
    container(rt::NOTES, 0, &[atom(rt::NOTES_ATOM, 0, &le32(&[SLIDE_ID, 0])), drawing(None, &[number, body])])
}

fn png() -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255])).write_to(&mut buf, image::ImageFormat::Png).unwrap();
    buf.into_inner()
}

/// The streams of a one-slide presentation, with `edit` choosing the current user's header token.
fn streams(token: u32) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let pictures = atom(0xF01E, 0x6E0, &[&[0u8; 17][..], &png()].concat());
    let mut fbse = vec![0u8; 36];
    fbse[0] = 6;
    fbse[1] = 6;
    fbse[20..24].copy_from_slice(&(pictures.len() as u32).to_le_bytes());
    let bstore = container(rt::BSTORE, 1, &[atom(rt::FBSE, 6, &fbse)]);
    let mut font = utf16("Arial");
    font.resize(68, 0);
    let mut wing = utf16("Wingdings");
    wing.resize(68, 0);
    let mut doc_atom = le32(&[5760, 4320, 4320, 5760, 1, 2, 0, 0]);
    doc_atom.extend_from_slice(&1u16.to_le_bytes());
    doc_atom.resize(40, 0);
    let persist = |id: u32| {
        atom(
            rt::SLIDE_PERSIST_ATOM,
            0,
            &le32(&[
                id,
                0,
                0,
                if id == 2 {
                    MASTER_ID
                } else if id == 3 {
                    SLIDE_ID
                } else {
                    SLIDE_ID + 1
                },
                0,
            ]),
        )
    };
    let document = container(
        rt::DOCUMENT,
        0,
        &[
            atom(rt::DOCUMENT_ATOM, 0, &doc_atom),
            container(rt::ENVIRONMENT, 0, &[container(rt::FONT_COLLECTION, 0, &[atom(rt::FONT_ENTITY, 0, &font), atom(rt::FONT_ENTITY, 1, &wing)])]),
            container(rt::DRAWING_GROUP, 0, &[container(rt::DGG_CONTAINER, 0, &[bstore])]),
            container(rt::SLIDE_LIST_WITH_TEXT, 1, &[persist(2)]),
            container(rt::SLIDE_LIST_WITH_TEXT, 0, &[persist(3)]),
            container(rt::SLIDE_LIST_WITH_TEXT, 2, &[persist(4)]),
        ],
    );
    let mut stream = vec![];
    let mut offsets = vec![];
    for part in [document, master(), slide(), notes()] {
        offsets.push(stream.len() as u32);
        stream.extend_from_slice(&part);
    }
    let dir_at = stream.len() as u32;
    let mut dir = vec![1u32 | (4 << 20)];
    dir.extend_from_slice(&offsets);
    stream.extend_from_slice(&atom(rt::PERSIST_DIRECTORY, 0, &le32(&dir)));
    let edit_at = stream.len() as u32;
    let mut edit = le32(&[0, 0x0300_03F4, 0, dir_at, 1, 5]);
    edit.extend_from_slice(&[0; 4]);
    stream.extend_from_slice(&atom(rt::USER_EDIT, 0, &edit));
    let current_user = atom(rt::CURRENT_USER, 0, &le32(&[0x14, token, edit_at, 0, 0]));
    (current_user, stream, pictures)
}

fn file_with(token: u32) -> Vec<u8> {
    let (cu, doc, pics) = streams(token);
    crate::cfb::build(&[("Current User", &cu), ("PowerPoint Document", &doc), ("Pictures", &pics)])
}

fn file() -> Vec<u8> {
    file_with(0xE391_C05F)
}

#[test]
fn imports_a_presentation() {
    let bytes = file();
    assert!(crate::sniff(&bytes));
    let p = crate::import(&bytes).unwrap();
    assert_eq!((p.slide_size.width, p.slide_size.height), (720.0, 540.0));
    assert_eq!(p.slides.len(), 1);
    let s = &p.slides[0];
    assert!(s.hidden);
    assert_eq!(s.notes.text(), "Say hello\nThen wave");

    // The title placeholder: its box, its text and the master's title style.
    let title = &s.shapes[0];
    assert_eq!(title.ph_type(), Some(PhType::Title));
    let x = title.xfrm.unwrap();
    assert_eq!((x.x, x.y, x.w, x.h), (62.5, 50.0, 595.0, 100.0));
    assert_eq!(title.fill, Some(Fill::None));
    let t = title.text.as_ref().unwrap();
    assert_eq!(t.text(), "Hello\nWorld");
    assert_eq!(t.body.anchor, Some(deckcraft_model::text::Anchor::Middle));
    let p0 = &t.paragraphs[0];
    assert_eq!(p0.props.align, Some(Align::Center));
    assert_eq!(p0.props.bullet, Some(Bullet::None));
    let r = &p0.runs[0].props;
    assert_eq!((r.size, r.bold, r.font.as_deref()), (Some(44.0), Some(true), Some("Arial")));
    assert_eq!(r.fill, Some(Fill::solid(deckcraft_model::ColorRef::rgb(Rgba::rgb(0x11, 0x22, 0x33)))));
    assert_eq!(s.title(), "Hello\nWorld");

    // The picture.
    let ShapeKind::Picture { fill } = &s.shapes[1].kind else { panic!("not a picture: {:?}", s.shapes[1].kind) };
    let media = p.media.iter().find(|m| m.id == fill.media).unwrap();
    assert_eq!(media.content_type, "image/png");
    assert!(media.data.starts_with(b"\x89PNG"));

    // The group: children in slide points, the child space the group's own box.
    let g = &s.shapes[2];
    let ShapeKind::Group { children, child } = &g.kind else { panic!("not a group") };
    let gx = g.xfrm.unwrap();
    assert_eq!((gx.x, gx.y, gx.w, gx.h), (375.0, 200.0, 125.0, 200.0));
    assert_eq!((child.x, child.y, child.w, child.h), (gx.x, gx.y, gx.w, gx.h));
    let e = children[0].xfrm.unwrap();
    assert_eq!((e.x, e.y, e.w, e.h, e.flip_h), (437.5, 200.0, 62.5, 100.0, false));
    assert_eq!(children[0].geom.preset_name(), Some("ellipse"));
    assert_eq!(children[0].fill, Some(Fill::solid(deckcraft_model::ColorRef::rgb(Rgba::rgb(0, 0xFF, 0)))));
    assert_eq!(children[0].line.as_ref().and_then(|l| l.width), Some(2.0));

    // The text box: runs split at the format change, the star a slide number field.
    let tb = s.shapes[3].text.as_ref().unwrap();
    let runs = &tb.paragraphs[0].runs;
    assert_eq!(tb.paragraphs[0].props.align, Some(Align::Right));
    assert_eq!(runs.len(), 3, "{runs:?}");
    assert_eq!((runs[0].text.as_str(), runs[0].props.bold), ("Page", Some(true)));
    assert_eq!((runs[1].text.as_str(), runs[1].props.bold), (" ", Some(false)));
    assert_eq!(runs[2].kind, RunKind::Field { field: "slidenum".into() });
    assert_eq!(runs[0].props.font.as_deref(), Some("Wingdings"));
    assert_eq!(s.shapes[3].fill, Some(Fill::None));

    // The master: its background, colours and decoration.
    let m = &p.masters[0];
    assert_eq!(m.background, Some(Background::Fill { fill: Fill::solid(deckcraft_model::ColorRef::rgb(Rgba::rgb(0xFF, 0, 0))) }));
    assert_eq!(m.theme.colors.get(deckcraft_model::SchemeSlot::Accent1), Rgba::rgb(0, 0xCC, 0));
    assert!(m.shapes.iter().any(|sh| sh.ph.is_none() && sh.fill == Some(Fill::solid(deckcraft_model::ColorRef::rgb(Rgba::rgb(0, 0xCC, 0))))));
    assert_eq!(m.title_style.level(0).and_then(|l| l.run.size), Some(44.0));
    assert!(p.layout(s.layout).is_some());
}

#[test]
fn an_encrypted_presentation_says_so() {
    assert!(matches!(crate::import(&file_with(0xF3D1_C4DF)), Err(crate::PptError::Encrypted)));
}

#[test]
fn other_files_are_not_ppt() {
    assert!(!crate::sniff(b"PK\x03\x04"));
    assert!(!crate::sniff(&crate::cfb::build(&[("WordDocument", b"x")])));
    assert!(matches!(crate::import(b"not a presentation"), Err(crate::PptError::NotPpt)));
    assert!(matches!(crate::import(&crate::cfb::build(&[("Workbook", b"x")])), Err(crate::PptError::NotPpt)));
}

/// Damaged input never panics, loops or allocates without bound: every 4-byte word of the
/// document stream set to hostile values, and truncations of it.
#[test]
fn hostile_documents_never_panic() {
    let (cu, doc, pics) = streams(0xE391_C05F);
    for at in (0..doc.len().saturating_sub(4)).step_by(3) {
        for v in [0u32, 1, 0x7FFF_FFFF, 0xFFFF_FFFF, 0x0000_FFFF, 0xF00F_000F] {
            let mut d = doc.clone();
            d[at..at + 4].copy_from_slice(&v.to_le_bytes());
            let f = crate::cfb::build(&[("Current User", &cu), ("PowerPoint Document", &d), ("Pictures", &pics)]);
            let _ = crate::import(&f);
        }
    }
    for cut in (0..doc.len()).step_by(7) {
        let f = crate::cfb::build(&[("Current User", &cu), ("PowerPoint Document", &doc[..cut]), ("Pictures", &pics[..cut.min(pics.len())])]);
        let _ = crate::import(&f);
    }
}

/// File > Open on a `.ppt` gives an untitled-to-disk copy: Save asks where to save instead of
/// writing DeckCraft's own format under the `.ppt` name.
#[test]
fn the_app_opens_a_ppt_and_save_asks_for_a_new_name() {
    let dir = std::env::temp_dir().join(format!("deckcraft-ppt-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Lecture.ppt");
    std::fs::write(&path, file()).unwrap();
    let mut s = deckcraft_engine::Session::new();
    let r = s.execute("file.open", &serde_json::json!({"path": path.to_string_lossy()})).unwrap();
    assert_eq!(r["slides"], 1);
    let st = s.doc().unwrap();
    assert_eq!(st.path, None);
    assert_eq!(st.title(), "Lecture");
    let saved = s.execute("file.save", &serde_json::json!({})).unwrap();
    assert_eq!(saved["needsPath"], true);
    // Saved as .pptx, it opens again with its slide.
    let out = dir.join("Lecture.pptx");
    s.execute("file.saveAs", &serde_json::json!({"path": out.to_string_lossy()})).unwrap();
    let back = deckcraft_engine::cmd::file::open_presentation("Lecture.pptx", &std::fs::read(&out).unwrap()).unwrap();
    assert_eq!(back.slides.len(), 1);
    assert_eq!(back.slides[0].shapes[0].text.as_ref().unwrap().text(), "Hello\nWorld");
    std::fs::remove_dir_all(&dir).ok();

    // Password-protected: the same message as for a .pptx.
    let err = deckcraft_engine::cmd::file::open_presentation("Locked.ppt", &file_with(0xF3D1_C4DF)).unwrap_err().to_string();
    assert!(err.contains("Locked.ppt is password-protected"), "{err}");
}
