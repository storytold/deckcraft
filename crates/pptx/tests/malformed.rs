//! Hostile input: the importer must return `Err` or a usable presentation, never panic.

use std::io::{Cursor, Write};

use deckcraft_model::defaults;

fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let o = zip::write::SimpleFileOptions::default();
    for (n, d) in files {
        z.start_file(*n, o).expect("start");
        z.write_all(d).expect("write");
    }
    z.finish().expect("finish").into_inner()
}

fn valid() -> Vec<u8> {
    deckcraft_pptx::export(&defaults::new_presentation(None)).expect("export")
}

/// Every entry of a valid export, as (name, bytes).
fn entries(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    (0..z.len())
        .map(|i| {
            let mut f = z.by_index(i).expect("entry");
            let mut d = vec![];
            std::io::Read::read_to_end(&mut f, &mut d).expect("read");
            (f.name().to_string(), d)
        })
        .collect()
}

fn rezip(es: &[(String, Vec<u8>)]) -> Vec<u8> {
    let v: Vec<(&str, &[u8])> = es.iter().map(|(n, d)| (n.as_str(), d.as_slice())).collect();
    zip_of(&v)
}

#[test]
fn random_bytes_and_empty() {
    assert!(deckcraft_pptx::import(&[]).is_err());
    assert!(deckcraft_pptx::import(b"PK\x03\x04garbage").is_err());
    let mut seed = 7u64;
    for len in [1usize, 10, 100, 1000, 10_000] {
        let v: Vec<u8> = (0..len)
            .map(|_| {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                (seed >> 33) as u8
            })
            .collect();
        assert!(deckcraft_pptx::import(&v).is_err());
    }
}

#[test]
fn truncated_zip() {
    let v = valid();
    for cut in [10, v.len() / 4, v.len() / 2, v.len() - 30, v.len() - 1] {
        let _ = deckcraft_pptx::import(&v[..cut]);
    }
}

#[test]
fn zip_without_presentation() {
    let z = zip_of(&[("[Content_Types].xml", b"<Types/>"), ("hello.txt", b"hi")]);
    assert!(deckcraft_pptx::import(&z).is_err());
}

#[test]
fn missing_parts_still_import() {
    let es = entries(&valid());
    // Drop each part in turn; import must not panic, and must succeed unless the main part is gone.
    for i in 0..es.len() {
        let mut v = es.clone();
        let (name, _) = v.remove(i);
        let r = deckcraft_pptx::import(&rezip(&v));
        if name == "ppt/presentation.xml" {
            assert!(r.is_err());
        } else if let Ok(p) = r {
            assert!(p.validate().is_empty(), "dropping {name}: {:?}", p.validate());
            // Whatever we read must export again.
            deckcraft_pptx::export(&p).expect("re-export");
        }
    }
}

#[test]
fn bad_xml_in_each_part() {
    let es = entries(&valid());
    for i in 0..es.len() {
        for bad in [&b"<p:sld><unclosed"[..], b"\xff\xfe\x00garbage", b"", b"<?xml version=\"1.0\"?>", b"<a:b c='1' c='2'/>"] {
            let mut v = es.clone();
            v[i].1 = bad.to_vec();
            if let Ok(p) = deckcraft_pptx::import(&rezip(&v)) {
                deckcraft_pptx::export(&p).expect("re-export");
            }
        }
    }
}

#[test]
fn huge_numbers_and_nonsense_attributes() {
    let es = entries(&valid());
    let mut v = es.clone();
    for (n, d) in v.iter_mut() {
        if n.ends_with(".xml") {
            let s = String::from_utf8_lossy(d)
                .replace("x=\"", "x=\"99999999999999999999")
                .replace("cx=\"", "cx=\"-")
                .replace("sz=\"", "sz=\"1e309")
                .replace("rot=\"", "rot=\"NaN")
                .replace("idx=\"", "idx=\"4294967296")
                .replace("id=\"", "id=\"0");
            *d = s.into_bytes();
        }
    }
    if let Ok(p) = deckcraft_pptx::import(&rezip(&v)) {
        for s in &p.slides {
            deckcraft_model::walk(&s.shapes, &mut |sh, _| {
                if let Some(x) = sh.xfrm {
                    assert!(x.is_finite());
                }
            });
        }
        deckcraft_pptx::export(&p).expect("re-export");
    }
}

#[test]
fn deep_nesting_is_bounded() {
    let es = entries(&valid());
    let deep = format!(
        "<?xml version=\"1.0\"?><p:sld xmlns:p=\"x\" xmlns:a=\"y\"><p:cSld><p:spTree>{}{}</p:spTree></p:cSld></p:sld>",
        "<p:grpSp>".repeat(200),
        "</p:grpSp>".repeat(200)
    );
    let mut v = es.clone();
    if let Some(e) = v.iter_mut().find(|(n, _)| n == "ppt/slides/slide1.xml") {
        e.1 = deep.into_bytes();
    }
    let p = deckcraft_pptx::import(&rezip(&v)).expect("import");
    deckcraft_pptx::export(&p).expect("re-export");
    // Beyond the XML depth limit the part is rejected, the deck still loads.
    let deeper = format!("<p:sld>{}</p:sld>", "<a>".repeat(5000));
    if let Some(e) = v.iter_mut().find(|(n, _)| n == "ppt/slides/slide1.xml") {
        e.1 = deeper.into_bytes();
    }
    let _ = deckcraft_pptx::import(&rezip(&v));
}

#[test]
fn relationship_cycles_and_bad_targets() {
    let es = entries(&valid());
    let mut v = es.clone();
    if let Some(e) = v.iter_mut().find(|(n, _)| n == "ppt/slides/_rels/slide1.xml.rels") {
        e.1 = br#"<Relationships xmlns="x"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../../../../../etc/passwd"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slide1.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide" Target="slide1.xml"/></Relationships>"#.to_vec();
    }
    let p = deckcraft_pptx::import(&rezip(&v)).expect("import");
    assert_eq!(p.slides.len(), 1);
    assert!(p.validate().is_empty());
}

/// Fixed-seed random byte mutations of a valid export (inside the zip entries, so the XML is hit).
#[test]
fn fuzz_mutations() {
    let p = {
        let mut s = deckcraft_engine::Session::new();
        deckcraft_engine::sample::open_sample(&mut s).expect("sample");
        s.doc().expect("doc").doc.as_ref().clone()
    };
    let es = entries(&deckcraft_pptx::export(&p).expect("export"));
    let mut seed = 0x5eed_u64;
    let mut rnd = |n: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % n.max(1) as u64) as usize
    };
    let iters = if cfg!(debug_assertions) { 300 } else { 2000 };
    for iteration in 0..iters {
        let mut v = es.clone();
        let k = rnd(v.len());
        let d = &mut v[k].1;
        for _ in 0..1 + rnd(8) {
            if d.is_empty() {
                break;
            }
            let at = rnd(d.len());
            match rnd(4) {
                0 => d[at] = rnd(256) as u8,
                1 => {
                    d.truncate(at);
                }
                2 => d.insert(at, b"<>\"&="[rnd(5)]),
                _ => {
                    let end = (at + rnd(64)).min(d.len());
                    d.drain(at..end);
                }
            }
        }
        if let Ok(q) = deckcraft_pptx::import(&rezip(&v)) {
            assert!(q.validate().is_empty());
            let before = q.clone();
            match deckcraft_pptx::export(&q) {
                Ok(bytes) => {
                    if let Err(error) = deckcraft_pptx::import(&bytes) {
                        let dir = std::env::var("DECKCRAFT_PPTX_FUZZ_REPRO_DIR").unwrap_or_else(|_| "/private/tmp/deckcraft-pptx-fuzz-repro".into());
                        std::fs::create_dir_all(&dir).unwrap();
                        std::fs::write(std::path::Path::new(&dir).join("mutated.pptx"), rezip(&v)).unwrap();
                        std::fs::write(std::path::Path::new(&dir).join("regenerated.pptx"), &bytes).unwrap();
                        std::fs::write(std::path::Path::new(&dir).join("model.json"), serde_json::to_vec(&q).unwrap()).unwrap();
                        panic!(
                            "reopen regenerated fuzz model: iteration {iteration}, mutated part {}, fixed seed 0x5eed, artifacts {dir}: {error}",
                            es[k].0
                        );
                    }
                }
                // A permissively imported foreign notes placeholder can have a
                // role with no standard notes-page mapping. Refusing to publish
                // it is safer than silently flattening or changing its role.
                Err(deckcraft_pptx::PptxError::Unsupported(message)) => {
                    assert_eq!(message, "notes interchange: placeholder role is not permitted on a standard notes page");
                }
                Err(error) => panic!("unexpected re-export failure: {error}"),
            }
            assert!(q == before, "fuzz export/refusal mutated the imported model");
        }
    }
    // Raw byte flips of the whole zip, too.
    let bytes = deckcraft_pptx::export(&p).expect("export");
    for _ in 0..iters {
        let mut b = bytes.clone();
        for _ in 0..1 + rnd(16) {
            let at = rnd(b.len());
            b[at] = rnd(256) as u8;
        }
        let _ = deckcraft_pptx::import(&b);
    }
}

#[test]
fn zip_bomb_entry_is_capped() {
    // A highly compressible 64 MiB XML part: must import (or fail) without blowing up.
    let big = vec![b' '; 64 * 1024 * 1024];
    let mut es = entries(&valid());
    es.push(("ppt/big.xml".into(), big));
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (n, d) in &es {
        z.start_file(n.as_str(), o).expect("start");
        z.write_all(d).expect("write");
    }
    let bytes = z.finish().expect("finish").into_inner();
    assert!(bytes.len() < 2 * 1024 * 1024);
    let p = deckcraft_pptx::import(&bytes).expect("import");
    assert_eq!(p.slides.len(), 1);
}

#[test]
fn export_of_odd_models_is_valid() {
    use deckcraft_model::{LayoutId, Shape, ShapeId, ShapeKind, Slide, SlideId};
    use std::sync::Arc;
    // No masters, dangling layout, duplicate and zero shape ids, NaN geometry, control characters.
    let mut p = defaults::new_presentation(None);
    let mut s = Slide { id: SlideId(9999), layout: LayoutId(12345), ..Default::default() };
    for id in [0, 1, 5, 5] {
        s.shapes.push(Shape {
            id: ShapeId(id),
            name: "bad\u{0}name\u{b}".into(),
            xfrm: Some(deckcraft_geom::Xfrm::new(f64::NAN, f64::INFINITY, -5.0, 1e300)),
            text: Some(deckcraft_model::TextBody::from_text("tab\tand\u{1}control\nnew line")),
            ..Default::default()
        });
    }
    s.shapes.push(Shape {
        id: ShapeId(7),
        kind: ShapeKind::Opaque { xml: "<not xml".into(), preview: None, label: "Thing".into(), parts: vec![] },
        ..Default::default()
    });
    p.slides.push(Arc::new(s));
    let bytes = deckcraft_pptx::export(&p).expect("export");
    let q = deckcraft_pptx::import(&bytes).expect("import");
    assert_eq!(q.slides.len(), 2);
    assert!(q.validate().is_empty(), "{:?}", q.validate());
    let mut empty = p.clone();
    empty.masters.clear();
    let q = deckcraft_pptx::import(&deckcraft_pptx::export(&empty).expect("export")).expect("import");
    assert!(!q.masters.is_empty());
}

#[test]
fn empty_presentation_defaults_remain_empty_and_notes_projection_reopens() {
    use deckcraft_model::{Master, MasterId, PhType, Placeholder, Shape, ShapeId, TextBody, Xfrm};
    use std::sync::Arc;
    let mut p = defaults::new_presentation(None);
    p.default_text_style = Default::default();
    p.notes_master = None;
    p.notes_masters.clear();
    let master = Arc::new(Master {
        id: MasterId(80_000),
        shapes: vec![Shape {
            id: ShapeId(80_001),
            ph: Some(Placeholder { kind: PhType::Body, ..Default::default() }),
            xfrm: Some(Xfrm::new(20., 100., 400., 300.)),
            text: Some(TextBody::from_text("original unused master prompt")),
            ..Default::default()
        }],
        ..Default::default()
    });
    p.notes_masters.push(deckcraft_model::notes::NotesMaster { master, ..Default::default() });
    let slide = Arc::make_mut(&mut p.slides[0]);
    slide.notes = TextBody::from_text("Current canonical body with sparse inherited properties");
    assert!(slide.notes.paragraphs[0].runs[0].props.kern.is_none());
    slide.notes_page = Some(deckcraft_model::notes::NotesPage {
        master: Some(MasterId(80_000)),
        body_shape: Some(ShapeId(80_002)),
        shapes: vec![Shape { id: ShapeId(80_002), ph: Some(Placeholder { kind: PhType::Body, ..Default::default() }), ..Default::default() }],
        ..Default::default()
    });
    p.fix_next_id();
    let original = p.clone();
    let mut back = deckcraft_pptx::import(&deckcraft_pptx::export(&p).unwrap()).unwrap();
    for _ in 0..2 {
        assert!(back.default_text_style.is_empty());
        assert_eq!(back.slides[0].notes, p.slides[0].notes);
        back = deckcraft_pptx::import(&deckcraft_pptx::export(&back).unwrap()).unwrap();
    }
    assert!(p == original, "notes export changed sparse source inheritance");
}
