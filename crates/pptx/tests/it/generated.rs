//! Harvey balls and checkboxes travel in a `.pptx` as ordinary groups of preset and custom shapes,
//! with what they were drawn from in the shape tag `ARTCRAFT_GENERATED`.

use std::io::{Cursor, Read, Write};

use deckcraft_engine::Session;
use deckcraft_model::generated::TAG;
use deckcraft_model::{Generated, Presentation, Shape, ShapeKind};
use serde_json::json;

/// A deck with one blank slide holding what `cmds` insert.
fn deck(cmds: &[(&str, serde_json::Value)]) -> Presentation {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).expect("slide");
    for (c, p) in cmds {
        s.execute(c, p).expect("insert");
    }
    s.doc().expect("doc").doc.as_ref().clone()
}

/// Every XML part of a package, by name.
fn parts(bytes: &[u8]) -> Vec<(String, String)> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes.to_vec())).expect("zip");
    let mut out = vec![];
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).expect("entry");
        let name = f.name().to_string();
        if name.ends_with(".xml") || name.ends_with(".rels") {
            let mut s = String::new();
            f.read_to_string(&mut s).expect("read");
            out.push((name, s));
        }
    }
    out
}

fn count_tag(bytes: &[u8]) -> usize {
    parts(bytes).iter().map(|(_, x)| x.matches(&format!("name=\"{TAG}\"")).count()).sum()
}

/// The package with each part's text passed through `f`.
fn rewrite(bytes: &[u8], f: impl Fn(&str, String) -> String) -> Vec<u8> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes.to_vec())).expect("zip");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..zip.len() {
        let mut e = zip.by_index(i).expect("entry");
        let name = e.name().to_string();
        let mut data = vec![];
        e.read_to_end(&mut data).expect("read");
        if name.ends_with(".xml")
            && let Ok(text) = String::from_utf8(data.clone())
        {
            data = f(&name, text).into_bytes();
        }
        out.start_file(name, zip::write::SimpleFileOptions::default()).expect("start");
        out.write_all(&data).expect("write");
    }
    out.finish().expect("finish").into_inner()
}

/// Replace the generated tag element in a tags part: `with` is the new element (or nothing).
fn replace_tag(xml: String, with: &str) -> String {
    let Some(at) = xml.find(&format!("<p:tag name=\"{TAG}\"")) else { return xml };
    let end = xml[at..].find("/>").map(|e| at + e + 2).expect("tag end");
    format!("{}{with}{}", &xml[..at], &xml[end..])
}

fn groups(p: &Presentation) -> Vec<&Shape> {
    p.slides.iter().flat_map(|s| s.shapes.iter()).filter(|s| s.is_group()).collect()
}

fn presets(g: &Shape) -> Vec<(String, Vec<f64>)> {
    g.children()
        .iter()
        .map(|c| match &c.geom {
            deckcraft_model::Geom::Preset { name, adj } => (name.clone(), adj.clone()),
            deckcraft_model::Geom::Custom { .. } => ("custom".into(), vec![]),
        })
        .collect()
}

/// C1 acceptance 3: save then open gives back the same description, written once as a tag; a
/// second save after opening still writes it once.
#[test]
fn a_harvey_ball_round_trips_with_its_tag_written_once() {
    let p = deck(&[("insert.harveyBall", json!({"value": 3, "of": 4, "background": "accent2", "outlineWidth": 1.5}))]);
    let g = groups(&p)[0];
    let bytes = deckcraft_pptx::export(&p).expect("export");
    assert_eq!(count_tag(&bytes), 1);
    let back = deckcraft_pptx::import(&bytes).expect("import");
    let h = groups(&back)[0];
    assert_eq!(h.generated, g.generated);
    assert!(h.tags.iter().all(|(n, _)| n != TAG), "taken out of the kept tags: {:?}", h.tags);
    assert_eq!(presets(h), presets(g));
    let again = deckcraft_pptx::export(&back).expect("export");
    assert_eq!(count_tag(&again), 1, "not written twice after a reopen");
    assert_eq!(groups(&deckcraft_pptx::import(&again).expect("import"))[0].generated, g.generated);
}

#[test]
fn checkboxes_round_trip_beside_other_tags() {
    let p = deck(&[
        ("insert.checkbox", json!({"state": "crossed", "color": "#C00000", "boxed": false})),
        ("insert.checkbox", json!({"state": "unchecked"})),
    ]);
    let mut p = p;
    let slide = std::sync::Arc::make_mut(&mut p.slides[1]);
    slide.shapes[0].tags = vec![("ADDIN".into(), "kept".into())];
    let bytes = deckcraft_pptx::export(&p).expect("export");
    assert_eq!(count_tag(&bytes), 2);
    let back = deckcraft_pptx::import(&bytes).expect("import");
    let (a, b) = (groups(&p), groups(&back));
    assert_eq!(b.iter().map(|g| &g.generated).collect::<Vec<_>>(), a.iter().map(|g| &g.generated).collect::<Vec<_>>());
    assert_eq!(b[0].tags, vec![("ADDIN".to_string(), "kept".to_string())]);
    assert!(matches!(&b[0].generated, Some(Generated::Checkbox(c)) if !c.boxed));
}

/// C1 acceptance 3: other software sees an ordinary group of presets; without the tag DeckCraft does too.
#[test]
fn without_its_tag_a_harvey_ball_opens_as_a_plain_group_of_the_same_shapes() {
    let p = deck(&[("insert.harveyBall", json!({"value": 1, "of": 3}))]);
    let bytes = rewrite(&deckcraft_pptx::export(&p).expect("export"), |_, x| replace_tag(x, ""));
    assert_eq!(count_tag(&bytes), 0);
    let back = deckcraft_pptx::import(&bytes).expect("import");
    let h = groups(&back)[0];
    assert_eq!(h.generated, None);
    assert!(matches!(h.kind, ShapeKind::Group { .. }));
    assert_eq!(presets(h), presets(groups(&p)[0]));
    assert_eq!(presets(h)[1], ("pie".to_string(), vec![16_200_000.0, 23_400_000.0 - 21_600_000.0]));
}

/// C1 acceptance 4: a value that doesn't parse, isn't version 1 or can't be drawn opens without a
/// panic as a plain group that keeps the tag (and writes it back unchanged).
#[test]
fn a_corrupt_tag_is_kept_as_an_ordinary_tag() {
    let p = deck(&[("insert.harveyBall", json!({"value": 2}))]);
    let bytes = deckcraft_pptx::export(&p).expect("export");
    for bad in [
        "not json",
        "{&quot;v&quot;:2,&quot;kind&quot;:&quot;harveyBall&quot;}",
        "{&quot;v&quot;:1,&quot;kind&quot;:&quot;harveyBall&quot;,&quot;of&quot;:0}",
        "",
    ] {
        let element = format!("<p:tag name=\"{TAG}\" val=\"{bad}\"/>");
        let file = rewrite(&bytes, |_, x| replace_tag(x, &element));
        let back = deckcraft_pptx::import(&file).expect("import");
        let h = groups(&back)[0];
        assert_eq!(h.generated, None, "{bad}");
        let kept: Vec<_> = h.tags.iter().filter(|(n, _)| n == TAG).collect();
        assert_eq!(kept.len(), 1, "{bad}");
        let resaved = deckcraft_pptx::export(&back).expect("export");
        assert_eq!(count_tag(&resaved), 1, "{bad}");
        assert_eq!(groups(&deckcraft_pptx::import(&resaved).expect("import"))[0].tags, h.tags, "{bad}");
    }
}

/// The description is written first among a shape's tags, so the cap on tags per part never drops it.
#[test]
fn the_description_survives_beside_many_other_tags() {
    let mut p = deck(&[("insert.harveyBall", json!({"value": 2}))]);
    let want = groups(&p)[0].generated.clone();
    let slide = std::sync::Arc::make_mut(&mut p.slides[1]);
    let g = slide.shapes.iter_mut().find(|s| s.is_group()).expect("group");
    g.tags = (0..10_000).map(|i| (format!("T{i}"), "x".to_string())).collect();
    let back = deckcraft_pptx::import(&deckcraft_pptx::export(&p).expect("export")).expect("import");
    assert_eq!(groups(&back)[0].generated, want);
}
