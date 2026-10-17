//! Macro-enabled packages (.pptm / .potm / .ppsm): the VBA project and the parts it relates to
//! ride along byte for byte; macro-free kinds drop them cleanly; hostile packages never panic.

use std::io::{Cursor, Read, Write};
use std::sync::Arc;

use deckcraft_model::{Presentation, VbaPart, defaults};
use deckcraft_pptx::{PptxKind, export_as, import};

const RT_VBA: &str = "http://schemas.microsoft.com/office/2006/relationships/vbaProject";
const RT_SIG: &str = "http://schemas.microsoft.com/office/2006/relationships/vbaProjectSignature";
const CT_VBA: &str = "application/vnd.ms-office.vbaProject";
const CT_SIG: &str = "application/vnd.ms-office.vbaProjectSignature";

const ALL: [PptxKind; 6] =
    [PptxKind::Presentation, PptxKind::Template, PptxKind::Show, PptxKind::MacroPresentation, PptxKind::MacroTemplate, PptxKind::MacroShow];

/// Stand-in project bytes (an OLE compound file header followed by noise): never parsed.
fn project_bytes() -> Vec<u8> {
    let mut v = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    v.extend((0..5000u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8));
    v
}

fn macro_deck() -> Presentation {
    let mut p = defaults::new_presentation(None);
    p.vba_project = Some(Arc::new(project_bytes()));
    p.vba_parts.push(VbaPart {
        rel_type: RT_SIG.into(),
        name: "ppt/vbaProjectSignature.bin".into(),
        content_type: CT_SIG.into(),
        data: Arc::new(vec![1, 2, 3, 4, 5, 250]),
    });
    p
}

fn entries(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    (0..z.len())
        .map(|i| {
            let mut f = z.by_index(i).expect("entry");
            let mut d = vec![];
            f.read_to_end(&mut d).expect("read");
            (f.name().to_string(), d)
        })
        .collect()
}

fn entry(es: &[(String, Vec<u8>)], name: &str) -> Option<Vec<u8>> {
    es.iter().find(|(n, _)| n == name).map(|(_, d)| d.clone())
}

fn text(es: &[(String, Vec<u8>)], name: &str) -> String {
    String::from_utf8(entry(es, name).unwrap_or_default()).expect("utf8")
}

fn zip_of(es: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (n, d) in es {
        z.start_file(n.as_str(), o).expect("start");
        z.write_all(d).expect("write");
    }
    z.finish().expect("finish").into_inner()
}

fn set(es: &mut Vec<(String, Vec<u8>)>, name: &str, data: Vec<u8>) {
    es.retain(|(n, _)| n != name);
    es.push((name.to_string(), data));
}

#[test]
fn kinds_by_extension() {
    for k in ALL {
        assert_eq!(PptxKind::from_ext(k.ext()), Some(k));
        assert_eq!(PptxKind::from_ext(&k.ext().to_ascii_uppercase()), Some(k));
        assert!(k.with_macros().macros());
        assert_eq!(PptxKind::from_path(&format!("/a/Deck.{}", k.ext())), Some(k));
    }
    assert_eq!(PptxKind::from_ext("deckcraft"), None);
    assert_eq!(PptxKind::Presentation.with_macros(), PptxKind::MacroPresentation);
    assert_eq!(PptxKind::Template.with_macros(), PptxKind::MacroTemplate);
    assert_eq!(PptxKind::Show.with_macros(), PptxKind::MacroShow);
}

#[test]
fn main_content_type_per_kind() {
    let expected = [
        (PptxKind::Presentation, "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"),
        (PptxKind::Template, "application/vnd.openxmlformats-officedocument.presentationml.template.main+xml"),
        (PptxKind::Show, "application/vnd.openxmlformats-officedocument.presentationml.slideshow.main+xml"),
        (PptxKind::MacroPresentation, "application/vnd.ms-powerpoint.presentation.macroEnabled.main+xml"),
        (PptxKind::MacroTemplate, "application/vnd.ms-powerpoint.template.macroEnabled.main+xml"),
        (PptxKind::MacroShow, "application/vnd.ms-powerpoint.slideshow.macroEnabled.main+xml"),
    ];
    for (k, ct) in expected {
        assert_eq!(k.main_content_type(), ct);
        let es = entries(&export_as(&defaults::new_presentation(None), k).expect("export"));
        let types = text(&es, "[Content_Types].xml");
        assert!(types.contains(&format!(r#"<Override PartName="/ppt/presentation.xml" ContentType="{ct}"/>"#)), "{k:?}: {types}");
        // No other kind's main content type sneaks in.
        assert_eq!(types.matches(".main+xml").count(), 1, "{k:?}");
        assert!(!types.contains("vbaProject"), "a deck without macros writes no project ({k:?})");
    }
}

#[test]
fn vba_project_round_trips_in_macro_kinds() {
    let p = macro_deck();
    for k in [PptxKind::MacroPresentation, PptxKind::MacroTemplate, PptxKind::MacroShow] {
        let bytes = export_as(&p, k).expect("export");
        let es = entries(&bytes);
        assert_eq!(entry(&es, "ppt/vbaProject.bin"), Some(project_bytes()), "{k:?}");
        assert_eq!(entry(&es, "ppt/vbaProjectSignature.bin"), Some(vec![1, 2, 3, 4, 5, 250]));
        let types = text(&es, "[Content_Types].xml");
        assert!(types.contains(&format!(r#"<Override PartName="/ppt/vbaProject.bin" ContentType="{CT_VBA}"/>"#)), "{types}");
        assert!(types.contains(&format!(r#"<Override PartName="/ppt/vbaProjectSignature.bin" ContentType="{CT_SIG}"/>"#)), "{types}");
        let rels = text(&es, "ppt/_rels/presentation.xml.rels");
        assert!(rels.contains(&format!(r#"Type="{RT_VBA}" Target="vbaProject.bin""#)), "{rels}");
        let vrels = text(&es, "ppt/_rels/vbaProject.bin.rels");
        assert!(vrels.contains(&format!(r#"Type="{RT_SIG}" Target="vbaProjectSignature.bin""#)), "{vrels}");

        let q = import(&bytes).expect("import");
        assert_eq!(q.vba_project.as_deref(), Some(&project_bytes()));
        assert_eq!(q.vba_parts, p.vba_parts);
        assert!(q.has_macros());
        assert_eq!(q.slides.len(), p.slides.len());
        // And again: a second save carries exactly the same bytes.
        let again = entries(&export_as(&q, k).expect("export"));
        assert_eq!(entry(&again, "ppt/vbaProject.bin"), Some(project_bytes()));
        assert_eq!(entry(&again, "ppt/vbaProjectSignature.bin"), Some(vec![1, 2, 3, 4, 5, 250]));
    }
}

#[test]
fn macro_free_kinds_drop_the_project_cleanly() {
    let p = macro_deck();
    for k in [PptxKind::Presentation, PptxKind::Template, PptxKind::Show] {
        let bytes = export_as(&p, k).expect("export");
        let es = entries(&bytes);
        for (n, d) in &es {
            assert!(!n.to_ascii_lowercase().contains("vba"), "{k:?} wrote {n}");
            let s = String::from_utf8_lossy(d);
            assert!(!s.contains("vbaProject"), "{k:?}: {n} mentions the project: {s}");
        }
        let q = import(&bytes).expect("import");
        assert_eq!(q.vba_project, None);
        assert!(q.vba_parts.is_empty());
        // Every relationship target that is internal exists.
        for (n, d) in es.iter().filter(|(n, _)| n.ends_with(".rels")) {
            let s = String::from_utf8_lossy(d);
            assert!(!s.contains(".bin\""), "{n}: {s}");
        }
    }
}

#[test]
fn model_json_keeps_the_project() {
    let p = macro_deck();
    let j = serde_json::to_value(&p).expect("json");
    assert!(j["vbaProject"].is_string());
    assert_eq!(j["vbaParts"][0]["name"], "ppt/vbaProjectSignature.bin");
    let q: Presentation = serde_json::from_value(j).expect("back");
    assert_eq!(q.vba_project, p.vba_project);
    assert_eq!(q.vba_parts, p.vba_parts);
    // A deck without macros writes neither key.
    let j = serde_json::to_value(defaults::new_presentation(None)).expect("json");
    assert!(j.get("vbaProject").is_none() && j.get("vbaParts").is_none());
}

/// A .pptm built by hand around a valid export, with `vba_rels` as the project's relationships.
fn hostile(project: Option<Vec<u8>>, vba_rels: Option<&str>, extra: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut es = entries(&export_as(&defaults::new_presentation(None), PptxKind::MacroPresentation).expect("export"));
    let rels = text(&es, "ppt/_rels/presentation.xml.rels")
        .replace("</Relationships>", &format!(r#"<Relationship Id="rId900" Type="{RT_VBA}" Target="vbaProject.bin"/></Relationships>"#));
    set(&mut es, "ppt/_rels/presentation.xml.rels", rels.into_bytes());
    if let Some(b) = project {
        set(&mut es, "ppt/vbaProject.bin", b);
    }
    if let Some(r) = vba_rels {
        set(&mut es, "ppt/_rels/vbaProject.bin.rels", r.as_bytes().to_vec());
    }
    for (n, d) in extra {
        set(&mut es, n, d.clone());
    }
    zip_of(&es)
}

#[test]
fn hostile_packages_open_without_panicking() {
    // The relationship points at a part that isn't there.
    let p = import(&hostile(None, None, &[])).expect("opens");
    assert_eq!(p.vba_project, None);
    assert!(!p.slides.is_empty());
    // An empty project is no project.
    let p = import(&hostile(Some(vec![]), None, &[])).expect("opens");
    assert_eq!(p.vba_project, None);
    // Unreadable project relationships: the project is kept on its own.
    let p = import(&hostile(Some(project_bytes()), Some("<Relationships><oops"), &[])).expect("opens");
    assert_eq!(p.vba_project.as_deref(), Some(&project_bytes()));
    assert!(p.vba_parts.is_empty());
    // Relationships that escape the package, point at the deck's own parts, at the project, at
    // a relationship part, outside ppt/, externally, or at nothing.
    let rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="r1" Type="{RT_SIG}" Target="../../../../etc/passwd"/>
<Relationship Id="r2" Type="{RT_SIG}" Target="presentation.xml"/>
<Relationship Id="r3" Type="{RT_SIG}" Target="vbaProject.bin"/>
<Relationship Id="r4" Type="{RT_SIG}" Target="_rels/presentation.xml.rels"/>
<Relationship Id="r5" Type="{RT_SIG}" Target="../docProps/core.xml"/>
<Relationship Id="r6" Type="{RT_SIG}" Target="https://example.invalid/x" TargetMode="External"/>
<Relationship Id="r7" Type="{RT_SIG}" Target="missing.bin"/>
<Relationship Id="r8" Type="{RT_SIG}" Target="sig.bin"/>
<Relationship Id="r9" Type="{RT_SIG}" Target="SIG.BIN"/>
</Relationships>"#
    );
    let bytes = hostile(Some(project_bytes()), Some(&rels), &[("ppt/sig.bin", vec![9, 9])]);
    let p = import(&bytes).expect("opens");
    assert_eq!(p.vba_project.as_deref(), Some(&project_bytes()));
    let names: Vec<&str> = p.vba_parts.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(names, ["ppt/sig.bin"], "only the real related part is kept");
    assert_eq!(p.vba_parts.first().map(|v| v.content_type.as_str()), Some("application/octet-stream"));
    // Saved again, it is still a valid deck.
    let q = import(&export_as(&p, PptxKind::MacroPresentation).expect("export")).expect("reopens");
    assert_eq!(q.vba_parts.len(), 1);

    // Far too many related parts: capped.
    let mut rels = String::from(r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#);
    let mut extra = vec![];
    for i in 0..200 {
        rels.push_str(&format!(r#"<Relationship Id="r{i}" Type="{RT_SIG}" Target="p{i}.bin"/>"#));
        extra.push((format!("ppt/p{i}.bin"), vec![i as u8]));
    }
    rels.push_str("</Relationships>");
    let extra: Vec<(&str, Vec<u8>)> = extra.iter().map(|(n, d)| (n.as_str(), d.clone())).collect();
    let p = import(&hostile(Some(project_bytes()), Some(&rels), &extra)).expect("opens");
    assert_eq!(p.vba_parts.len(), 64);
}

#[test]
fn oversized_project_is_left_out() {
    let huge = vec![0u8; 64 * 1024 * 1024 + 1];
    let p = import(&hostile(Some(huge), None, &[])).expect("opens");
    assert_eq!(p.vba_project, None);
    assert!(!p.slides.is_empty());
}

#[test]
fn hostile_model_parts_never_shadow_the_deck() {
    let mut p = macro_deck();
    for (name, ty) in [
        ("ppt/presentation.xml", RT_SIG),
        ("ppt/slides/slide1.xml", RT_SIG),
        ("ppt/vbaProject.bin", RT_SIG),
        ("ppt/_rels/presentation.xml.rels", RT_SIG),
        ("ppt/../evil.bin", RT_SIG),
        ("[Content_Types].xml", RT_SIG),
        ("ppt/a:b.bin", RT_SIG),
        ("ppt/ok.bin", "not a uri"),
        ("ppt/vbaProjectSignature.bin", RT_SIG),
    ] {
        p.vba_parts.push(VbaPart { rel_type: ty.into(), name: name.into(), content_type: "bad\ntype".into(), data: Arc::new(vec![7]) });
    }
    let bytes = export_as(&p, PptxKind::MacroPresentation).expect("export");
    let es = entries(&bytes);
    let mut names: Vec<&str> = es.iter().map(|(n, _)| n.as_str()).collect();
    names.sort_unstable();
    let before = names.len();
    names.dedup();
    assert_eq!(before, names.len(), "no part is written twice");
    assert!(!names.iter().any(|n| n.contains("evil") || n.contains("ok.bin") || n.contains(':')), "{names:?}");
    // The signature part kept its original bytes (the duplicate was skipped).
    assert_eq!(entry(&es, "ppt/vbaProjectSignature.bin"), Some(vec![1, 2, 3, 4, 5, 250]));
    let q = import(&bytes).expect("reopens");
    assert_eq!(q.slides.len(), p.slides.len());
    assert_eq!(q.vba_parts.len(), 1);
}

#[test]
fn kind_of_reads_the_main_content_type() {
    for k in ALL {
        assert_eq!(deckcraft_pptx::kind_of(&export_as(&macro_deck(), k).expect("export")), Some(k));
    }
    assert_eq!(deckcraft_pptx::kind_of(b"PK not a zip"), None);
    assert_eq!(deckcraft_pptx::kind_of(&[]), None);
}
