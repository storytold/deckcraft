//! Folder format tests on decks built in code.

use std::sync::Arc;

use deckcraft_color::Rgba;
use deckcraft_geom::Xfrm;
use deckcraft_model::chart::{Chart, ChartType};
use deckcraft_model::style::PictureFill;
use deckcraft_model::table::Table;
use deckcraft_model::text::TextBody;
use deckcraft_model::{
    Comment, CustomShow, InkStroke, LayoutType, MasterId, MediaClip, MediaId, Presentation, Section, Shape, ShapeId, ShapeKind, SlideId, defaults,
};

use super::*;

const T0: u64 = 1_760_000_000_000;

fn shape(p: &mut Presentation, name: &str, kind: ShapeKind) -> Shape {
    Shape { id: ShapeId(p.alloc_id().expect("id")), name: name.into(), xfrm: Some(Xfrm::new(10.0, 20.0, 200.0, 100.0)), kind, ..Default::default() }
}

fn add_slide(p: &mut Presentation, title: &str) -> SlideId {
    let layout = defaults::layout_of_kind(p, LayoutType::TitleAndContent).unwrap_or_default();
    let mut s = defaults::new_slide(p, layout).expect("slide");
    if let Some(t) = s.shapes.first_mut() {
        t.text = Some(TextBody::from_text(title));
    }
    let id = s.id;
    p.slides.push(Arc::new(s));
    id
}

/// A deck with every shape kind, notes, comments, sections, custom shows, media (one shared by
/// two records, one linked without bytes), a notes master and a handout master.
fn deck() -> Presentation {
    let mut p = defaults::new_presentation(None);
    p.props.title = "Folder test — ünïcödé".into();
    let img = p.add_media("image1.png", "image/png", vec![0x89, b'P', b'N', b'G', 1, 2, 3]).expect("media");
    let video = p.add_media("clip.mp4", "video/mp4", b"\x00\x00\x00\x18ftypmp42".to_vec()).expect("media");
    let same = MediaId(p.alloc_id().expect("id"));
    p.media.push(MediaItem {
        id: same,
        name: "copy.png".into(),
        content_type: "image/png".into(),
        data: Arc::new(vec![0x89, b'P', b'N', b'G', 1, 2, 3]),
        link: None,
    });
    let linked = MediaId(p.alloc_id().expect("id"));
    p.media.push(MediaItem {
        id: linked,
        name: "far.png".into(),
        content_type: "image/png".into(),
        link: Some("https://example.org/far.png".into()),
        ..Default::default()
    });

    let first = p.slides.first().map(|s| s.id).unwrap_or_default();
    if let Some(s) = p.slide_mut(first) {
        s.notes = TextBody::from_text("Speaker notes\nsecond line");
        s.comments = vec![Comment { author: "Ada".into(), initials: "AL".into(), text: "Looks good".into(), ..Default::default() }];
    }

    let s2 = add_slide(&mut p, "Shapes");
    let pic = shape(&mut p, "Picture", ShapeKind::Picture { fill: PictureFill { media: img, ..Default::default() } });
    let copy = shape(&mut p, "Copy", ShapeKind::Picture { fill: PictureFill { media: same, ..Default::default() } });
    let c1 = shape(&mut p, "Child 1", ShapeKind::Shape);
    let c2 = shape(&mut p, "Child 2", ShapeKind::Shape);
    let (a, b) = (c1.id, c2.id);
    let group = shape(&mut p, "Group", ShapeKind::Group { children: vec![c1, c2], child: Xfrm::new(0.0, 0.0, 100.0, 50.0) });
    let conn = shape(&mut p, "Connector", ShapeKind::Connector { start: Some((a, 1)), end: Some((b, 3)) });
    let ink = shape(
        &mut p,
        "Ink",
        ShapeKind::Ink {
            strokes: vec![InkStroke {
                points: vec![(1.0, 2.0, 0.5), (3.25, 4.0, 0.75)],
                color: Rgba::rgb(0, 0, 255),
                width: 2.0,
                highlighter: false,
            }],
        },
    );
    let opaque = shape(
        &mut p,
        "SmartArt",
        ShapeKind::Opaque { xml: "<dgm:x a=\"1\">\n  <y/>\n</dgm:x>".into(), preview: Some(img), label: "SmartArt".into(), parts: vec![] },
    );
    if let Some(s) = p.slide_mut(s2) {
        s.shapes.extend([pic, copy, group, conn, ink, opaque]);
    }

    let s3 = add_slide(&mut p, "Table and charts");
    let mut table = Table::new(2, 3, 300.0, 30.0);
    if let Some(c) = table.cell_mut(1, 2) {
        c.text = TextBody::from_text("cell");
    }
    let table = shape(&mut p, "Table", ShapeKind::Table(table));
    let mut untitled = Chart::sample(ChartType::Pie);
    untitled.title = None;
    untitled.legend = None;
    untitled.raw = Some("<c:chartSpace>\n  <c:chart/>\n</c:chartSpace>".into());
    let titled = shape(&mut p, "Chart", ShapeKind::Chart(Box::new(Chart::sample(ChartType::Column))));
    let untitled = shape(&mut p, "Untitled chart", ShapeKind::Chart(Box::new(untitled)));
    let clip =
        shape(&mut p, "Video", ShapeKind::Media(MediaClip { media: video, video: true, poster: Some(img), volume: 0.5, ..Default::default() }));
    if let Some(s) = p.slide_mut(s3) {
        s.shapes.extend([table, titled, untitled, clip]);
        s.hidden = true;
    }
    let s4 = add_slide(&mut p, "End");

    if let Some(m) = p.masters.first().cloned() {
        let mut notes = (*m).clone();
        notes.id = MasterId(p.alloc_id().expect("id"));
        notes.name = "Notes Master".into();
        let mut handout = (*m).clone();
        handout.id = MasterId(p.alloc_id().expect("id"));
        handout.layouts.clear();
        p.notes_master = Some(Arc::new(notes));
        p.handout_master = Some(Arc::new(handout));
    }
    p.sections = vec![Section { name: "Intro".into(), slides: vec![first, s2] }, Section { name: "Body".into(), slides: vec![s3, s4] }];
    p.custom_shows = vec![CustomShow { name: "Short".into(), slides: vec![first, s4] }];
    p.first_slide_number = 0;
    p.raw_ext = Some("<p:extLst/>".into());
    p
}

fn write(p: &Presentation, prev: Option<&FileMap>) -> FileMap {
    to_folder_with(p, prev, &mut KeyGen::fixed(T0)).unwrap().files
}

/// The model a `.deckcraft` of `p` loads as, `nextId` aside (R9).
fn via_zip(p: &Presentation) -> Presentation {
    let mut q = crate::load(&crate::save(p).unwrap()).unwrap();
    q.next_id = 0;
    q
}

fn via_folder(p: &Presentation) -> Presentation {
    let mut q = from_folder(&write(p, None)).unwrap();
    q.next_id = 0;
    q
}

fn text(f: &FileMap, path: &str) -> String {
    String::from_utf8(f.get(path).cloned().unwrap_or_default()).unwrap()
}

fn slide_files(f: &FileMap) -> Vec<&String> {
    f.keys().filter(|k| k.starts_with("slides/")).collect()
}

#[test]
fn round_trip_is_lossless() {
    for p in [deck(), defaults::new_presentation(None), defaults::blank_presentation(defaults::WIDE, Default::default(), false)] {
        let q = via_folder(&p);
        assert!(q == via_zip(&p), "folder and zip load differently");
        assert_eq!(q.media.iter().map(|m| m.data.len()).collect::<Vec<_>>(), p.media.iter().map(|m| m.data.len()).collect::<Vec<_>>());
    }
}

#[test]
fn layout_of_the_folder() {
    let p = deck();
    let f = write(&p, None);
    assert_eq!(slide_files(&f).len(), 4);
    let m = p.masters.first().map(|m| m.id.0).unwrap_or_default();
    for path in [DECK_JSON.to_string(), ORDER.into(), format!("masters/{m}.json"), "masters/notes.json".into(), "masters/handout.json".into()] {
        assert!(f.contains_key(&path), "{path}");
    }
    assert!(!f.contains_key(SECTIONS));
    assert!(f.keys().all(|k| is_owned(k)), "{:?}", f.keys().collect::<Vec<_>>());
    let deck = text(&f, DECK_JSON);
    assert!(
        deck.starts_with(
            "{\n  \"format\": \"deckcraft-folder\",\n  \"version\": 1,\n  \"deckcraftFormatVersion\": 1,\n  \"presentation\": {\n    \"slideSize\": {"
        ),
        "{deck}"
    );
    // R9: no volatile fields; split fields live elsewhere.
    let v: Value = serde_json::from_str(&deck).unwrap();
    for k in SPLIT_KEYS.iter().chain(&["generator"]) {
        assert!(v.get(k).is_none() && v["presentation"].get(k).is_none(), "{k} in deck.json");
    }
    assert!(v["presentation"].get("customShows").is_some() && v["presentation"].get("rawExt").is_some());
    assert!(deck.contains("ünïcödé"), "UTF-8 is written unescaped");
    let order = text(&f, ORDER);
    assert!(order.starts_with("# slides.order v1\nsection: Intro\n"), "{order}");
    assert_eq!(order.lines().filter(|l| is_key(l)).count(), 4);
    // Every file ends with one newline, LF only.
    for (k, v) in &f {
        if !k.starts_with("media/") {
            assert!(v.ends_with(b"\n") && !v.ends_with(b"\n\n") && !v.contains(&b'\r'), "{k}");
        }
    }
}

#[test]
fn unchanged_deck_resaves_byte_identical() {
    let p = deck();
    let f1 = write(&p, None);
    // Saved again from memory, and after a load: the same bytes, even with a different key source.
    assert_eq!(to_folder_with(&p, Some(&f1), &mut KeyGen::system()).unwrap().files, f1);
    let q = from_folder(&f1).unwrap();
    assert_eq!(to_folder_with(&q, Some(&f1), &mut KeyGen::system()).unwrap().files, f1);
}

#[test]
fn slide_keys_survive_add_delete_and_reorder() {
    let mut p = deck();
    let f1 = write(&p, None);
    let key_of = |f: &FileMap, id: SlideId| -> String {
        f.iter()
            .find(|(k, v)| k.starts_with("slides/") && serde_json::from_slice::<Value>(v).ok().and_then(|v| id_of(&v)) == Some(u64::from(id.0)))
            .map(|(k, _)| k.clone())
            .unwrap_or_default()
    };
    let ids: Vec<SlideId> = p.slides.iter().map(|s| s.id).collect();
    let before: Vec<String> = ids.iter().map(|id| key_of(&f1, *id)).collect();

    // Reorder: only slides.order changes.
    p.slides.swap(1, 2);
    p.sections = vec![];
    let f2 = write(&p, Some(&f1));
    let changed: Vec<&String> = f1.keys().chain(f2.keys()).filter(|k| f1.get(*k) != f2.get(*k)).collect();
    assert_eq!(changed, vec![&ORDER.to_string(), &ORDER.to_string()]);

    // Delete one, add one: the others keep their file; the new one gets a fresh key.
    p.slides.remove(0);
    let added = add_slide(&mut p, "New");
    let f3 = to_folder_with(&p, Some(&f2), &mut KeyGen::fixed(T0 + 1)).unwrap().files;
    assert!(!f3.contains_key(&before[0]), "deleted slide's file is gone");
    for (id, k) in ids.iter().zip(&before).skip(1) {
        assert_eq!(&key_of(&f3, *id), k);
        assert_eq!(f3.get(k), f2.get(k), "{k} unchanged");
    }
    let new_key = key_of(&f3, added);
    assert!(!before.contains(&new_key) && is_key(new_key.trim_start_matches("slides/").trim_end_matches(".json")));
    assert_eq!(slide_files(&f3).len(), 4);
    assert!(from_folder(&f3).unwrap().slides.iter().map(|s| s.id).eq(p.slides.iter().map(|s| s.id)));
}

#[test]
fn chart_title_and_legend_nulls_are_kept() {
    let p = deck();
    let f = write(&p, None);
    let slide = f.values().map(|v| String::from_utf8_lossy(v).to_string()).find(|t| t.contains("Untitled chart")).unwrap_or_default();
    // `None` differs from the default (`Some("Chart Title")`, `Some("b")`): the nulls stay.
    assert!(slide.contains("\"title\": null") && slide.contains("\"legend\": null"), "{slide}");
    // Other nulls are gone (R2), e.g. `background` and `transition` of the slide.
    assert!(!slide.contains("\"background\": null") && !slide.contains("\"transition\": null"));
    // R7: raw XML stays one JSON string.
    assert!(slide.contains("\"raw\": \"<c:chartSpace>\\n  <c:chart/>\\n</c:chartSpace>\""));
    let q = via_folder(&p);
    let chart = |p: &Presentation, name: &str| {
        p.slides.iter().flat_map(|s| s.shapes.iter()).find(|s| s.name == name).and_then(|s| match &s.kind {
            ShapeKind::Chart(c) => Some((c.title.clone(), c.legend.clone())),
            _ => None,
        })
    };
    assert_eq!(chart(&q, "Untitled chart"), Some((None, None)));
    assert_eq!(chart(&q, "Chart"), chart(&p, "Chart"));
}

#[test]
fn media_is_stored_once_per_content() {
    let p = deck();
    let f = write(&p, None);
    let bytes: Vec<&String> = f.keys().filter(|k| k.starts_with("media/")).collect();
    // image (shared by two records) + video; the linked one has no bytes.
    assert_eq!(bytes.len(), 2, "{bytes:?}");
    let png = sha256::hex(&[0x89, b'P', b'N', b'G', 1, 2, 3]);
    assert!(f.contains_key(&format!("media/{png}.png")));
    let metas: Vec<String> = f.iter().filter(|(k, _)| k.starts_with("media-meta/")).map(|(_, v)| String::from_utf8_lossy(v).to_string()).collect();
    assert_eq!(metas.len(), 4);
    assert_eq!(metas.iter().filter(|m| m.contains(&png)).count(), 2);
    let linked = metas.iter().find(|m| m.contains("far.png")).cloned().unwrap_or_default();
    assert!(linked.contains("\"link\": \"https://example.org/far.png\"") && !linked.contains("sha256"), "{linked}");
    let first = metas.first().cloned().unwrap_or_default();
    assert!(first.starts_with("{\n  \"id\": ") && first.trim_end().ends_with(&format!("\"sha256\": \"{png}\"\n}}")), "{first}");
}

#[test]
fn unknown_keys_and_files_are_kept() {
    let p = deck();
    let mut f = write(&p, None);
    let slide_path = slide_files(&f).into_iter().nth(1).cloned().unwrap_or_default();
    let edit = |f: &mut FileMap, path: &str, from: &str, to: &str| {
        let t = text(f, path);
        assert!(t.contains(from), "{path}: {from}");
        f.insert(path.to_string(), t.replacen(from, to, 1).into_bytes());
    };
    edit(&mut f, &slide_path, "  \"layout\":", "  \"futureSlideField\": {\n    \"a\": null\n  },\n  \"layout\":");
    edit(&mut f, DECK_JSON, "  \"presentation\": {\n", "  \"presentation\": {\n    \"futureDeckField\": 7,\n");
    edit(&mut f, DECK_JSON, "{\n  \"format\"", "{\n  \"tool\": \"x\",\n  \"format\"");
    let meta = f.keys().find(|k| k.starts_with("media-meta/")).cloned().unwrap_or_default();
    edit(&mut f, &meta, "  \"name\":", "  \"alt\": \"later\",\n  \"name\":");
    let mut q = from_folder(&f).unwrap();
    // A change elsewhere in the same slide: the unknown key stays where it was.
    let i = p.slides.iter().position(|s| slide_path.contains(&key_of_slide(&f, s.id.0))).unwrap_or(1);
    if let Some(s) = q.slides.get_mut(i) {
        Arc::make_mut(s).name = "Renamed".into();
    }
    let g = write(&q, Some(&f));
    let s = text(&g, &slide_path);
    assert!(s.contains("  \"futureSlideField\": {\n    \"a\": null\n  },\n  \"layout\":") && s.contains("\"name\": \"Renamed\""), "{s}");
    let d = text(&g, DECK_JSON);
    assert!(d.contains("\"presentation\": {\n    \"futureDeckField\": 7,\n") && d.contains("\"tool\": \"x\""), "{d}");
    assert!(text(&g, &meta).contains("\"alt\": \"later\",\n  \"name\":"));
}

fn key_of_slide(f: &FileMap, id: u32) -> String {
    f.iter()
        .find(|(k, v)| k.starts_with("slides/") && serde_json::from_slice::<Value>(v).ok().and_then(|v| id_of(&v)) == Some(u64::from(id)))
        .map(|(k, _)| k.trim_start_matches("slides/").trim_end_matches(".json").to_string())
        .unwrap_or_default()
}

#[test]
fn split_sections_go_to_sections_json() {
    let mut p = deck();
    let ids: Vec<SlideId> = p.slides.iter().map(|s| s.id).collect();
    p.sections = vec![Section { name: "Odd".into(), slides: vec![ids[0], ids[2]] }, Section { name: "Even".into(), slides: vec![ids[1], ids[3]] }];
    let folder = to_folder_with(&p, None, &mut KeyGen::fixed(T0)).unwrap();
    assert!(folder.files.contains_key(SECTIONS) && !text(&folder.files, ORDER).contains("section:"));
    assert_eq!(folder.warnings.len(), 1);
    assert!(via_folder(&p) == via_zip(&p));
}

#[test]
fn masters_and_media_out_of_id_order() {
    let mut p = deck();
    let mut second = (*p.masters[0]).clone();
    second.id = MasterId(p.alloc_id().expect("id"));
    second.layouts.clear();
    p.masters.insert(0, Arc::new(second));
    p.media.reverse();
    let folder = to_folder_with(&p, None, &mut KeyGen::fixed(T0)).unwrap();
    let d = text(&folder.files, DECK_JSON);
    assert!(d.contains("\"mastersOrder\"") && d.contains("\"mediaOrder\""), "{d}");
    assert_eq!(folder.warnings.len(), 2);
    assert!(via_folder(&p) == via_zip(&p));
}

/// A folder written by the external `deckgit` tool, built from its spec (FORMAT-DECK.md):
/// format `deckgit`, its own `slides.order` header, sections, a media record.
#[test]
fn reads_and_keeps_a_deckgit_folder() {
    let p = defaults::new_presentation(None);
    let master = p.masters.first().map(|m| (**m).clone()).unwrap();
    let slide = (*p.slides[0]).clone();
    let slide_json = serde_json::to_vec_pretty(&slide).unwrap();
    let png = vec![0x89, b'P', b'N', b'G', 9];
    let sha = sha256::hex(&png);
    let mut f = FileMap::new();
    let key = "01JA7Q3M5K8X9Y2Z3A4B5C6D7E";
    f.insert(
        DECK_JSON.into(),
        b"{\n  \"format\": \"deckgit\",\n  \"version\": 1,\n  \"deckcraftFormatVersion\": 1,\n  \"presentation\": {\n    \"slideSize\": {\n      \"width\": 720.0,\n      \"height\": 540.0\n    },\n    \"firstSlideNumber\": 3\n  }\n}\n"
            .to_vec(),
    );
    f.insert(ORDER.into(), format!("# deckgit slides.order v1\nsection: Only\n{key}\n").into_bytes());
    f.insert(format!("slides/{key}.json"), slide_json);
    f.insert(format!("masters/{}.json", master.id.0), serde_json::to_vec_pretty(&master).unwrap());
    f.insert(
        "media-meta/900.json".into(),
        format!("{{\n  \"id\": 900,\n  \"name\": \"a.png\",\n  \"contentType\": \"image/png\",\n  \"sha256\": \"{sha}\"\n}}\n").into_bytes(),
    );
    f.insert(format!("media/{sha}.png"), png.clone());
    f.insert("README.md".into(), b"not ours".to_vec());

    let q = from_folder(&f).unwrap();
    assert_eq!(q.slide_size.width, 720.0);
    assert_eq!(q.first_slide_number, 3);
    assert_eq!(q.slides.len(), 1);
    assert_eq!(q.sections, vec![Section { name: "Only".into(), slides: vec![slide.id] }]);
    assert_eq!(q.media.first().map(|m| m.data.as_slice()), Some(&png[..]));
    assert!(q.next_id >= 900);

    // Saved again: the format name, the header and the key stay; README isn't ours.
    let g = write(&q, Some(&f));
    assert!(text(&g, DECK_JSON).starts_with("{\n  \"format\": \"deckgit\","));
    assert!(text(&g, ORDER).starts_with("# deckgit slides.order v1\nsection: Only\n"));
    assert!(g.contains_key(&format!("slides/{key}.json")));
    assert!(!g.contains_key("README.md") && !is_owned("README.md"));
}

#[test]
fn damaged_folders_are_errors_not_panics() {
    let p = deck();
    let f = write(&p, None);
    assert!(matches!(from_folder(&FileMap::new()), Err(FormatError::NotOurs(_))));
    let with = |path: &str, bytes: &[u8]| {
        let mut g = f.clone();
        g.insert(path.into(), bytes.to_vec());
        from_folder(&g)
    };
    assert!(matches!(with(DECK_JSON, b"{\"format\":\"other\",\"version\":1}"), Err(FormatError::NotOurs(_))));
    assert!(with(DECK_JSON, b"{\"format\":\"deckcraft-folder\",\"version\":2}").is_err());
    assert!(with(DECK_JSON, b"not json").is_err());
    assert!(with(ORDER, b"bad key\n").is_err());
    let order = text(&f, ORDER);
    let first = order.lines().find(|l| is_key(l)).unwrap_or_default().to_string();
    assert!(with(ORDER, format!("{order}{first}\n").as_bytes()).unwrap_err().to_string().contains("twice"));
    assert!(with(ORDER, b"01JA7Q3M5K8X9Y2Z3A4B5C6D7E\n").is_err(), "listed slide without a file");
    let slide = slide_files(&f).first().cloned().cloned().unwrap_or_default();
    assert!(with(&slide, b"{\"id\": \"x\"}").is_err());
    assert!(with(&slide, b"[1, 2").is_err());
    let media = f.keys().find(|k| k.starts_with("media/")).cloned().unwrap_or_default();
    let lfs = with(&media, b"version https://git-lfs.github.com/spec/v1\noid sha256:0\nsize 7\n").unwrap_err().to_string();
    assert!(lfs.contains("git lfs pull"), "{lfs}");
    let mut g = f.clone();
    g.remove(&media);
    assert!(from_folder(&g).is_err());
    // Truncated files anywhere: an error or a deck, never a panic.
    for k in f.keys() {
        let b = f.get(k).cloned().unwrap_or_default();
        let _ = with(k, &b[..b.len() / 2]);
    }
}

#[test]
fn unlisted_slide_files_are_kept_at_the_end() {
    let p = deck();
    let f = write(&p, None);
    let order = text(&f, ORDER);
    let last = order.lines().rev().find(|l| is_key(l)).unwrap_or_default();
    let mut g = f.clone();
    g.insert(ORDER.into(), order.replace(&format!("{last}\n"), "").into_bytes());
    let q = from_folder(&g).unwrap();
    assert_eq!(q.slides.len(), p.slides.len());
}

#[test]
fn owned_paths() {
    for ok in [
        "deck.json",
        "slides.order",
        "sections.json",
        "slides/01JA7Q3M5K8X9Y2Z3A4B5C6D7E.json",
        "masters/12.json",
        "masters/notes.json",
        "masters/handout.json",
        "media-meta/412.json",
        "media/9f2c41aa9f2c41aa9f2c41aa9f2c41aa9f2c41aa9f2c41aa9f2c41aa9f2c41aa.png",
    ] {
        assert!(is_owned(ok), "{ok}");
    }
    for no in [
        "README.md",
        ".gitattributes",
        "slides/notes.json",
        "slides/01JA7Q3M5K8X9Y2Z3A4B5C6D7E.json.saving",
        "slides/sub/01JA7Q3M5K8X9Y2Z3A4B5C6D7E.json",
        "masters/x.json",
        "media-meta/a.json",
        "media/9f2c41aa9f2c41aa9f2c41aa9f2c41aa9f2c41aa9f2c41aa9f2c41aa9f2c41aa.png.saving",
        "media/photo.png",
        "deck.json.saving",
    ] {
        assert!(!is_owned(no), "{no}");
    }
}

#[test]
fn media_extensions() {
    assert_eq!(media_ext("image/jpeg", "x.jpeg"), "jpg");
    assert_eq!(media_ext("application/octet-stream", "font.TTF"), "ttf");
    assert_eq!(media_ext("", "noext"), "bin");
    assert_eq!(media_ext("", "x.verylongext"), "bin");
}
