//! Placeholder inheritance in decks written by PowerPoint.

use std::io::{Cursor, Read, Write};

use deckcraft_model::{Geom, defaults};

/// `bytes` with `edit` applied to every slide part.
fn edit_slides(bytes: &[u8], edit: impl Fn(String) -> String) -> Vec<u8> {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let o = zip::write::SimpleFileOptions::default();
    for i in 0..z.len() {
        let mut f = z.by_index(i).expect("entry");
        let mut d = vec![];
        f.read_to_end(&mut d).expect("read");
        let name = f.name().to_string();
        if name.starts_with("ppt/slides/slide") && name.ends_with(".xml") {
            d = edit(String::from_utf8(d).expect("utf8")).into_bytes();
        }
        out.start_file(name, o).expect("start");
        out.write_all(&d).expect("write");
    }
    out.finish().expect("finish").into_inner()
}

#[test]
fn a_placeholder_without_geometry_takes_its_layouts() {
    // PowerPoint leaves a slide placeholder's spPr empty and draws it with the layout
    // placeholder's geometry (a picture placeholder cut into a shape keeps the shape).
    let mut p = defaults::new_presentation(None);
    let layout = p.slides[0].layout;
    let m = std::sync::Arc::make_mut(&mut p.masters[0]);
    let l = m.layouts.iter_mut().find(|l| l.id == layout).expect("layout");
    for s in &mut l.shapes {
        if s.ph.is_some() {
            s.geom = Geom::Preset { name: "ellipse".into(), adj: vec![] };
        }
    }
    let bytes = deckcraft_pptx::export(&p).expect("export");
    // Strip the slide's own geometry, as PowerPoint writes inheriting placeholders.
    let bare = edit_slides(&bytes, |s| {
        let mut s = s;
        while let Some(i) = s.find("<a:prstGeom") {
            let Some(j) = s[i..].find("</a:prstGeom>").map(|j| j + i + "</a:prstGeom>".len()) else { break };
            s.replace_range(i..j, "");
        }
        s
    });
    let q = deckcraft_pptx::import(&bare).expect("import");
    let phs: Vec<_> = q.slides[0].shapes.iter().filter(|s| s.ph.is_some()).collect();
    assert!(!phs.is_empty());
    for s in phs {
        assert!(matches!(&s.geom, Geom::Preset { name, .. } if name == "ellipse"), "{:?}", s.geom);
    }
    // A placeholder with its own geometry keeps it.
    let first = p.slides[0].id;
    for s in &mut p.slide_mut(first).expect("slide").shapes {
        if s.ph.is_some() {
            s.geom = Geom::Preset { name: "triangle".into(), adj: vec![] };
        }
    }
    let q = deckcraft_pptx::import(&deckcraft_pptx::export(&p).expect("export")).expect("import");
    assert!(q.slides[0].shapes.iter().filter(|s| s.ph.is_some()).all(|s| matches!(&s.geom, Geom::Preset { name, .. } if name == "triangle")));
}
