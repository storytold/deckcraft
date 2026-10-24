//! Golden tests build their own tiny `.pptx` packages in code (contributor-original
//! fixtures): no binary files, no Microsoft material. Names are made up; the XML
//! shapes follow ECMA-376 Part 1 (PresentationML/DrawingML) and Part 2 (OPC).

use std::io::{Cursor, Write};

use crate::{AuditError, audit_bytes};

const P: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const RELS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const OFFICE_DOC: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";

/// Assemble a ZIP package from `(name, bytes)` entries.
fn package(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in entries {
        writer.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        writer.write_all(data).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn content_types() -> Vec<u8> {
    br#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/></Types>"#.to_vec()
}

fn root_rels() -> Vec<u8> {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="{RELS}"><Relationship Id="rId1" Type="{OFFICE_DOC}" Target="ppt/presentation.xml"/></Relationships>"#
    )
    .into_bytes()
}

fn presentation() -> Vec<u8> {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><p:presentation xmlns:p="{P}" xmlns:r="{R}"><p:sldIdLst><p:sldId id="256" r:id="rId2"/></p:sldIdLst></p:presentation>"#
    )
    .into_bytes()
}

/// `ppt/_rels/presentation.xml.rels` with the given inner relationships.
fn pres_rels(inner: &str) -> Vec<u8> {
    format!(r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="{RELS}">{inner}</Relationships>"#).into_bytes()
}

/// `ppt/slides/_rels/slide1.xml.rels` with the given inner relationships.
fn slide_rels(inner: &str) -> Vec<u8> {
    pres_rels(inner)
}

fn slide(inner: &str) -> Vec<u8> {
    format!(r#"<?xml version="1.0" encoding="UTF-8"?><p:sld xmlns:p="{P}" xmlns:a="{A}" xmlns:r="{R}"><p:cSld><p:spTree>{inner}</p:spTree></p:cSld></p:sld>"#)
        .into_bytes()
}

fn plain_slide() -> Vec<u8> {
    slide("")
}

/// Minimal package with no hidden information: nothing to find.
fn clean_package() -> Vec<u8> {
    package(&[
        ("[Content_Types].xml", &content_types()),
        ("_rels/.rels", &root_rels()),
        ("ppt/presentation.xml", &presentation()),
        (
            "ppt/_rels/presentation.xml.rels",
            &pres_rels(
                r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/>"#,
            ),
        ),
        ("ppt/slides/slide1.xml", &plain_slide()),
        ("ppt/slides/_rels/slide1.xml.rels", &slide_rels("")),
    ])
}

fn findings_of(bytes: &[u8]) -> Vec<crate::AuditFinding> {
    audit_bytes(bytes).unwrap()
}

fn ids_of(bytes: &[u8]) -> Vec<String> {
    findings_of(bytes).iter().map(|f| f.id.clone()).collect()
}

#[test]
fn finding_ids_are_stable() {
    let kinds = [
        (crate::FindingKind::NotesSlide, "notes/slide"),
        (crate::FindingKind::HiddenSlide, "hidden/slide"),
        (crate::FindingKind::CroppedImage, "image/crop-retains-data"),
        (crate::FindingKind::CommentAuthor, "author-pii/comment"),
        (crate::FindingKind::CorePropertiesAuthor, "author-pii/core-properties"),
        (crate::FindingKind::ExtendedProperties, "author-pii/extended-properties"),
        (crate::FindingKind::EmbeddedMedia, "media/metadata"),
        (crate::FindingKind::VbaProject, "active-content/vba"),
        (crate::FindingKind::OleEmbedding, "active-content/ole"),
        (crate::FindingKind::OrphanPart, "package/orphan-part"),
    ];
    for (kind, id) in kinds {
        assert_eq!(kind.id(), id, "finding ID changed");
    }
}

#[test]
fn hidden_slide_found() {
    let hidden = format!(r#"<?xml version="1.0" encoding="UTF-8"?><p:sld xmlns:p="{P}" show="0"><p:cSld><p:spTree/></p:cSld></p:sld>"#);
    let bytes = package(&[
        ("[Content_Types].xml", &content_types()),
        ("_rels/.rels", &root_rels()),
        ("ppt/presentation.xml", &presentation()),
        (
            "ppt/_rels/presentation.xml.rels",
            &pres_rels(concat!(
                r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/>"#,
                r#"<Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide2.xml"/>"#,
            )),
        ),
        ("ppt/slides/slide1.xml", &plain_slide()),
        ("ppt/slides/slide2.xml", &hidden.into_bytes()),
        ("ppt/slides/_rels/slide1.xml.rels", &slide_rels("")),
        ("ppt/slides/_rels/slide2.xml.rels", &slide_rels("")),
    ]);
    let findings = findings_of(&bytes);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].id, "hidden/slide");
    assert_eq!(findings[0].severity, "warn");
    assert!(findings[0].location.contains("ppt/slides/slide2.xml"), "{}", findings[0].location);
}

#[test]
fn shown_slide_has_no_hidden_finding() {
    // `show` omitted or true means the slide is shown: no finding.
    for show in ["", r#" show="1""#, r#" show="true""#] {
        let sld = format!(r#"<?xml version="1.0" encoding="UTF-8"?><p:sld xmlns:p="{P}"{show}><p:cSld><p:spTree/></p:cSld></p:sld>"#);
        let bytes = package(&[
            ("[Content_Types].xml", &content_types()),
            ("_rels/.rels", &root_rels()),
            ("ppt/presentation.xml", &presentation()),
            (
                "ppt/_rels/presentation.xml.rels",
                &pres_rels(
                    r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/>"#,
                ),
            ),
            ("ppt/slides/slide1.xml", &sld.into_bytes()),
            ("ppt/slides/_rels/slide1.xml.rels", &slide_rels("")),
        ]);
        assert!(findings_of(&bytes).is_empty(), "show={show:?}");
    }
}

#[test]
fn notes_slide_found() {
    let notes = format!(r#"<?xml version="1.0" encoding="UTF-8"?><p:notes xmlns:p="{P}"><p:cSld><p:spTree/></p:cSld></p:notes>"#);
    let bytes = package(&[
        ("[Content_Types].xml", &content_types()),
        ("_rels/.rels", &root_rels()),
        ("ppt/presentation.xml", &presentation()),
        (
            "ppt/_rels/presentation.xml.rels",
            &pres_rels(
                r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/>"#,
            ),
        ),
        ("ppt/slides/slide1.xml", &plain_slide()),
        ("ppt/slides/_rels/slide1.xml.rels", &slide_rels("")),
        ("ppt/notesSlides/notesSlide1.xml", &notes.into_bytes()),
    ]);
    let findings = findings_of(&bytes);
    // The notes slide is reachable (not orphaned) via the slide rels? No:
    // nothing references it here, so it is both notes and orphan.
    assert!(findings.iter().any(|f| f.id == "notes/slide" && f.location == "ppt/notesSlides/notesSlide1.xml"));
}

#[test]
fn cropped_image_found() {
    // A picture cropped 10% from the left whose blip still points at the
    // full image part through the slide's relationships.
    let pic = format!(
        r#"<p:pic xmlns:p="{P}" xmlns:a="{A}" xmlns:r="{R}"><p:blipFill><a:blip r:embed="rId2"/><a:srcRect l="10000" t="0" r="0" b="0"/></p:blipFill></p:pic>"#
    );
    let bytes = package(&[
        ("[Content_Types].xml", &content_types()),
        ("_rels/.rels", &root_rels()),
        ("ppt/presentation.xml", &presentation()),
        (
            "ppt/_rels/presentation.xml.rels",
            &pres_rels(
                r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/>"#,
            ),
        ),
        ("ppt/slides/slide1.xml", &slide(&pic)),
        (
            "ppt/slides/_rels/slide1.xml.rels",
            &slide_rels(
                r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image1.png"/>"#,
            ),
        ),
        ("ppt/media/image1.png", b"fake-png-bytes"),
    ]);
    let findings = findings_of(&bytes);
    // The crop finding plus the media part itself (reported as info).
    let crops: Vec<_> = findings.iter().filter(|f| f.id == "image/crop-retains-data").collect();
    assert_eq!(crops.len(), 1);
    assert_eq!(crops[0].severity, "warn");
    assert!(crops[0].location.contains("ppt/slides/slide1.xml"), "{}", crops[0].location);
    assert!(crops[0].detail.contains("ppt/media/image1.png"), "{}", crops[0].detail);
    assert!(crops[0].detail.contains("l=10000"), "{}", crops[0].detail);
    assert!(findings.iter().any(|f| f.id == "media/metadata" && f.location == "ppt/media/image1.png"));
}

#[test]
fn uncropped_image_is_clean() {
    // No srcRect, an all-zero srcRect, outward (negative) offsets, or a
    // percentage-formatted zero: nothing hidden, no crop finding.
    // (NaN/inf edges still report — see the case below.)
    for blip_fill in [
        r#"<p:blipFill><a:blip r:embed="rId2"/></p:blipFill>"#.to_string(),
        r#"<p:blipFill><a:blip r:embed="rId2"/><a:srcRect l="0" t="0" r="0" b="0"/></p:blipFill>"#.to_string(),
        r#"<p:blipFill><a:blip r:embed="rId2"/><a:srcRect l="-5000" t="0" r="0" b="-2000"/></p:blipFill>"#.to_string(),
        r#"<p:blipFill><a:blip r:embed="rId2"/><a:srcRect l="0%" t="0%" r="0%" b="0%"/></p:blipFill>"#.to_string(),
    ] {
        let pic = format!(r#"<p:pic xmlns:p="{P}" xmlns:a="{A}" xmlns:r="{R}">{blip_fill}</p:pic>"#);
        let bytes = package(&[
            ("[Content_Types].xml", &content_types()),
            ("_rels/.rels", &root_rels()),
            ("ppt/presentation.xml", &presentation()),
            (
                "ppt/_rels/presentation.xml.rels",
                &pres_rels(
                    r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/>"#,
                ),
            ),
            ("ppt/slides/slide1.xml", &slide(&pic)),
            (
                "ppt/slides/_rels/slide1.xml.rels",
                &slide_rels(
                    r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image1.png"/>"#,
                ),
            ),
            ("ppt/media/image1.png", b"fake-png-bytes"),
        ]);
        // The media part itself is reported (info), but no crop finding.
        let findings = findings_of(&bytes);
        assert!(findings.iter().all(|f| f.id != "image/crop-retains-data"), "{blip_fill}");
    }
}

#[test]
fn comment_authors_found() {
    let authors = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><p:cmAuthorLst xmlns:p="{P}"><p:cmAuthor id="0" name="Quill Ramsey" initials="QR" lastIdx="1" clrIdx="0"/><p:cmAuthor id="1" name="Ona Vettel" initials="OV" lastIdx="1" clrIdx="1"/></p:cmAuthorLst>"#
    );
    let comments = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><p:cmLst xmlns:p="{P}"><p:cm authorId="0" dt="2026-03-01T10:00:00Z" idx="1"><p:pos x="0" y="0"/><p:txBody><a:t xmlns:a="{A}">Check this.</a:t></p:txBody></p:cm><p:cm authorId="1" dt="2026-03-02T10:00:00Z" idx="2"><p:pos x="0" y="0"/><p:txBody><a:t xmlns:a="{A}">Agreed.</a:t></p:txBody></p:cm></p:cmLst>"#
    );
    let bytes = package(&[
        ("[Content_Types].xml", &content_types()),
        ("_rels/.rels", &root_rels()),
        ("ppt/presentation.xml", &presentation()),
        (
            "ppt/_rels/presentation.xml.rels",
            &pres_rels(
                r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/>"#,
            ),
        ),
        ("ppt/slides/slide1.xml", &plain_slide()),
        ("ppt/slides/_rels/slide1.xml.rels", &slide_rels("")),
        ("ppt/commentAuthors.xml", &authors.into_bytes()),
        ("ppt/comments/comment1.xml", &comments.into_bytes()),
    ]);
    let findings = findings_of(&bytes);
    // The comments part is not referenced by any rels here, so it is also an
    // orphan; the authors part likewise. Filter to the comment findings.
    let comment_findings: Vec<_> = findings.iter().filter(|f| f.id == "author-pii/comment").collect();
    assert_eq!(comment_findings.len(), 2);
    assert!(comment_findings.iter().all(|f| f.severity == "warn"));
    assert!(comment_findings.iter().any(|f| f.detail.contains("Quill Ramsey") && f.detail.contains("QR")));
    assert!(comment_findings.iter().any(|f| f.detail.contains("Ona Vettel")));
}

#[test]
fn core_properties_found() {
    let core = br#"<?xml version="1.0" encoding="UTF-8"?><cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:creator>Quill Ramsey</dc:creator><cp:lastModifiedBy>Ona Vettel</cp:lastModifiedBy></cp:coreProperties>"#;
    let rels = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="{RELS}"><Relationship Id="rId1" Type="{OFFICE_DOC}" Target="ppt/presentation.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/></Relationships>"#
    );
    let ct = content_types();
    let rr = rels.into_bytes();
    let pres = presentation();
    let pr = pres_rels(
        r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/>"#,
    );
    let s1 = plain_slide();
    let sr = slide_rels("");
    let core = core.to_vec();
    let bytes = package(&[
        ("[Content_Types].xml", ct.as_slice()),
        ("_rels/.rels", rr.as_slice()),
        ("ppt/presentation.xml", pres.as_slice()),
        ("ppt/_rels/presentation.xml.rels", pr.as_slice()),
        ("ppt/slides/slide1.xml", s1.as_slice()),
        ("ppt/slides/_rels/slide1.xml.rels", sr.as_slice()),
        ("docProps/core.xml", core.as_slice()),
    ]);
    let findings = findings_of(&bytes);
    assert!(findings.iter().any(|f| f.id == "author-pii/core-properties" && f.detail == "Quill Ramsey"));
    assert!(findings.iter().any(|f| f.id == "author-pii/core-properties" && f.detail == "Ona Vettel"));
}

#[test]
fn extended_properties_are_info() {
    let app = br#"<?xml version="1.0" encoding="UTF-8"?><Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"><Company>Quillworks Inc</Company></Properties>"#;
    let rels = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="{RELS}"><Relationship Id="rId1" Type="{OFFICE_DOC}" Target="ppt/presentation.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/package/2006/relationships/extended-properties" Target="docProps/app.xml"/></Relationships>"#
    );
    let ct = content_types();
    let rr = rels.into_bytes();
    let pres = presentation();
    let pr = pres_rels(
        r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/>"#,
    );
    let s1 = plain_slide();
    let sr = slide_rels("");
    let app = app.to_vec();
    let bytes = package(&[
        ("[Content_Types].xml", ct.as_slice()),
        ("_rels/.rels", rr.as_slice()),
        ("ppt/presentation.xml", pres.as_slice()),
        ("ppt/_rels/presentation.xml.rels", pr.as_slice()),
        ("ppt/slides/slide1.xml", s1.as_slice()),
        ("ppt/slides/_rels/slide1.xml.rels", sr.as_slice()),
        ("docProps/app.xml", app.as_slice()),
    ]);
    let findings = findings_of(&bytes);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].id, "author-pii/extended-properties");
    assert_eq!(findings[0].severity, "info");
    assert_eq!(findings[0].detail, "Quillworks Inc");
}

#[test]
fn media_parts_found() {
    let bytes = package(&[
        ("[Content_Types].xml", &content_types()),
        ("_rels/.rels", &root_rels()),
        ("ppt/presentation.xml", &presentation()),
        (
            "ppt/_rels/presentation.xml.rels",
            &pres_rels(
                r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/>"#,
            ),
        ),
        ("ppt/slides/slide1.xml", &plain_slide()),
        (
            "ppt/slides/_rels/slide1.xml.rels",
            &slide_rels(concat!(
                r#"<Relationship Id="rId5" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/video" Target="../media/movie.mp4"/>"#,
                r#"<Relationship Id="rId6" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/audio" Target="../media/narration.mp3"/>"#,
            )),
        ),
        ("ppt/media/movie.mp4", b"fake-mp4-bytes"),
        ("ppt/media/narration.mp3", b"fake-mp3-bytes"),
    ]);
    let findings = findings_of(&bytes);
    let media: Vec<_> = findings.iter().filter(|f| f.id == "media/metadata").collect();
    assert_eq!(media.len(), 2);
    assert!(media.iter().all(|f| f.severity == "info"));
    assert!(media.iter().any(|f| f.location == "ppt/media/movie.mp4" && f.detail.contains("video")));
    assert!(media.iter().any(|f| f.location == "ppt/media/narration.mp3" && f.detail.contains("audio")));
}

#[test]
fn vba_and_ole_found_by_relationship_type() {
    // Reachable active content at non-standard paths: caught by relationship
    // type, and reachable means neither is an orphan.
    let rels = pres_rels(concat!(
        r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/>"#,
        r#"<Relationship Id="rId7" Type="http://schemas.microsoft.com/office/2006/relationships/vbaProject" Target="custom/macros.bin"/>"#,
        r#"<Relationship Id="rId8" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/oleObject" Target="custom/object.bin"/>"#,
    ));
    let bytes = package(&[
        ("[Content_Types].xml", &content_types()),
        ("_rels/.rels", &root_rels()),
        ("ppt/presentation.xml", &presentation()),
        ("ppt/_rels/presentation.xml.rels", &rels),
        ("ppt/slides/slide1.xml", &plain_slide()),
        ("ppt/slides/_rels/slide1.xml.rels", &slide_rels("")),
        ("ppt/custom/macros.bin", b"fake-macro-bytes"),
        ("ppt/custom/object.bin", b"fake-ole-bytes"),
    ]);
    let findings = findings_of(&bytes);
    assert!(findings.iter().any(|f| f.id == "active-content/vba" && f.location == "ppt/custom/macros.bin"));
    assert!(findings.iter().any(|f| f.id == "active-content/ole" && f.location == "ppt/custom/object.bin"));
    assert!(findings.iter().all(|f| f.id != "package/orphan-part"));
}

#[test]
fn ole_fallback_by_directory() {
    // An embedding with no relationship at all: the `ppt/embeddings/`
    // fallback still reports it (and it is an orphan too).
    let bytes = package(&[
        ("[Content_Types].xml", &content_types()),
        ("_rels/.rels", &root_rels()),
        ("ppt/presentation.xml", &presentation()),
        (
            "ppt/_rels/presentation.xml.rels",
            &pres_rels(
                r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/>"#,
            ),
        ),
        ("ppt/slides/slide1.xml", &plain_slide()),
        ("ppt/slides/_rels/slide1.xml.rels", &slide_rels("")),
        ("ppt/embeddings/oleObject1.bin", b"fake-ole-bytes"),
    ]);
    let findings = findings_of(&bytes);
    assert!(findings.iter().any(|f| f.id == "active-content/ole" && f.location == "ppt/embeddings/oleObject1.bin"));
}

#[test]
fn orphan_part_found() {
    // ppt/slides/slide9.xml is not referenced by any relationship.
    let bytes = package(&[
        ("[Content_Types].xml", &content_types()),
        ("_rels/.rels", &root_rels()),
        ("ppt/presentation.xml", &presentation()),
        (
            "ppt/_rels/presentation.xml.rels",
            &pres_rels(
                r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/>"#,
            ),
        ),
        ("ppt/slides/slide1.xml", &plain_slide()),
        ("ppt/slides/_rels/slide1.xml.rels", &slide_rels("")),
        ("ppt/slides/slide9.xml", &plain_slide()),
    ]);
    let findings = findings_of(&bytes);
    // slide9.xml and its (nonexistent) rels: only slide9.xml is the orphan;
    // its rels path was never created so nothing references it either way.
    let orphans: Vec<_> = findings.iter().filter(|f| f.id == "package/orphan-part").collect();
    assert_eq!(orphans.len(), 1);
    assert_eq!(orphans[0].location, "ppt/slides/slide9.xml");
    assert_eq!(orphans[0].severity, "info");
}

#[test]
fn orphan_subtree_rels_do_not_rescue_parts() {
    // Reachability is traversed from the package root: `ppt/secret.xml` is
    // orphaned, so its own `.rels` file must not rescue `ppt/hidden.xml`.
    let secret_rels = pres_rels(
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="hidden.xml"/>"#,
    );
    let bytes = package(&[
        ("[Content_Types].xml", &content_types()),
        ("_rels/.rels", &root_rels()),
        ("ppt/presentation.xml", &presentation()),
        (
            "ppt/_rels/presentation.xml.rels",
            &pres_rels(
                r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/>"#,
            ),
        ),
        ("ppt/slides/slide1.xml", &plain_slide()),
        ("ppt/slides/_rels/slide1.xml.rels", &slide_rels("")),
        ("ppt/secret.xml", br#"<?xml version="1.0"?><secret/>"#),
        ("ppt/_rels/secret.xml.rels", &secret_rels),
        ("ppt/hidden.xml", br#"<?xml version="1.0"?><hidden/>"#),
    ]);
    let findings = findings_of(&bytes);
    let orphans: Vec<&str> = findings.iter().filter(|f| f.id == "package/orphan-part").map(|f| f.location.as_str()).collect();
    assert_eq!(findings.len(), orphans.len());
    assert!(orphans.contains(&"ppt/secret.xml"));
    assert!(orphans.contains(&"ppt/_rels/secret.xml.rels"));
    assert!(orphans.contains(&"ppt/hidden.xml"));
}

#[test]
fn external_relationship_targets_are_not_orphans() {
    let rels = pres_rels(&format!(
        "{}{}",
        r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/>"#,
        r#"<Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com/report" TargetMode="External"/>"#,
    ));
    let bytes = package(&[
        ("[Content_Types].xml", &content_types()),
        ("_rels/.rels", &root_rels()),
        ("ppt/presentation.xml", &presentation()),
        ("ppt/_rels/presentation.xml.rels", &rels),
        ("ppt/slides/slide1.xml", &plain_slide()),
        ("ppt/slides/_rels/slide1.xml.rels", &slide_rels("")),
    ]);
    assert!(findings_of(&bytes).is_empty());
}

#[test]
fn clean_file_has_no_findings() {
    assert!(findings_of(&clean_package()).is_empty());
}

#[test]
fn unusual_prefixes_still_detected() {
    // Matching is by namespace URI, not prefix.
    let hidden = format!(r#"<?xml version="1.0" encoding="UTF-8"?><p1:sld xmlns:p1="{P}" show="0"><p1:cSld><p1:spTree/></p1:cSld></p1:sld>"#);
    let bytes = package(&[
        ("[Content_Types].xml", &content_types()),
        ("_rels/.rels", &root_rels()),
        ("ppt/presentation.xml", &presentation()),
        (
            "ppt/_rels/presentation.xml.rels",
            &pres_rels(
                r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/>"#,
            ),
        ),
        ("ppt/slides/slide1.xml", &hidden.into_bytes()),
        ("ppt/slides/_rels/slide1.xml.rels", &slide_rels("")),
    ]);
    let findings = findings_of(&bytes);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].id, "hidden/slide");
}

#[test]
fn missing_presentation_xml_is_an_error() {
    let bytes = package(&[("[Content_Types].xml", &content_types()), ("_rels/.rels", &root_rels())]);
    assert!(matches!(audit_bytes(&bytes), Err(AuditError::Xml(p, _)) if p == "ppt/presentation.xml"));
}

#[test]
fn malformed_zip_is_an_error_not_a_panic() {
    assert!(matches!(audit_bytes(b"not a zip at all"), Err(AuditError::Zip(_))));
}

#[test]
fn truncated_xml_is_an_error_not_a_silent_pass() {
    // Cut mid-tag: quick-xml reports the truncation; the audit must not
    // silently pass a half-read part.
    let mut doc = presentation();
    doc.truncate(doc.len() / 2);
    let bytes = package(&[
        ("[Content_Types].xml", &content_types()),
        ("_rels/.rels", &root_rels()),
        ("ppt/presentation.xml", &doc),
        (
            "ppt/_rels/presentation.xml.rels",
            &pres_rels(
                r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/>"#,
            ),
        ),
        ("ppt/slides/slide1.xml", &plain_slide()),
        ("ppt/slides/_rels/slide1.xml.rels", &slide_rels("")),
    ]);
    assert!(matches!(audit_bytes(&bytes), Err(AuditError::Xml(_, _))));
}

#[test]
fn mismatched_end_tag_is_an_error() {
    let bad = format!(r#"<?xml version="1.0" encoding="UTF-8"?><p:presentation xmlns:p="{P}"><p:sldIdLst></p:WRONG></p:sldIdLst></p:presentation>"#);
    let bytes = package(&[
        ("[Content_Types].xml", &content_types()),
        ("_rels/.rels", &root_rels()),
        ("ppt/presentation.xml", &bad.into_bytes()),
        (
            "ppt/_rels/presentation.xml.rels",
            &pres_rels(
                r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/>"#,
            ),
        ),
        ("ppt/slides/slide1.xml", &plain_slide()),
        ("ppt/slides/_rels/slide1.xml.rels", &slide_rels("")),
    ]);
    assert!(matches!(audit_bytes(&bytes), Err(AuditError::Xml(_, _))));
}

#[test]
fn multiple_root_elements_is_an_error() {
    let doc = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><p:presentation xmlns:p="{P}"><p:sldIdLst/></p:presentation><p:presentation xmlns:p="{P}"/>"#
    );
    let bytes = package(&[
        ("[Content_Types].xml", &content_types()),
        ("_rels/.rels", &root_rels()),
        ("ppt/presentation.xml", &doc.into_bytes()),
        (
            "ppt/_rels/presentation.xml.rels",
            &pres_rels(
                r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/>"#,
            ),
        ),
        ("ppt/slides/slide1.xml", &plain_slide()),
        ("ppt/slides/_rels/slide1.xml.rels", &slide_rels("")),
    ]);
    assert!(matches!(audit_bytes(&bytes), Err(AuditError::Xml(_, _))));
}

#[test]
fn findings_serialize_to_json() {
    let hidden = format!(r#"<?xml version="1.0" encoding="UTF-8"?><p:sld xmlns:p="{P}" show="false"><p:cSld><p:spTree/></p:cSld></p:sld>"#);
    let bytes = package(&[
        ("[Content_Types].xml", &content_types()),
        ("_rels/.rels", &root_rels()),
        ("ppt/presentation.xml", &presentation()),
        (
            "ppt/_rels/presentation.xml.rels",
            &pres_rels(
                r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/>"#,
            ),
        ),
        ("ppt/slides/slide1.xml", &hidden.into_bytes()),
        ("ppt/slides/_rels/slide1.xml.rels", &slide_rels("")),
    ]);
    let findings = findings_of(&bytes);
    let json = serde_json::to_value(&findings).unwrap();
    let first = &json[0];
    assert_eq!(first["id"], serde_json::json!("hidden/slide"));
    assert_eq!(first["severity"], serde_json::json!("warn"));
    assert!(first["location"].as_str().unwrap().contains("ppt/slides/slide1.xml"));
    // Deterministic order: re-running gives the same sequence.
    assert_eq!(ids_of(&bytes), ids_of(&bytes));
}

#[test]
fn nonfinite_crop_edge_is_reported() {
    // NaN/inf edges are invalid values: fail closed, report the crop.
    for edge in ["NaN", "inf", "-inf"] {
        let blip_fill = format!(r#"<p:blipFill><a:blip r:embed="rId2"/><a:srcRect l="{edge}" t="0" r="0" b="0"/></p:blipFill>"#);
        let pic = format!(r#"<p:pic xmlns:p="{P}" xmlns:a="{A}" xmlns:r="{R}">{blip_fill}</p:pic>"#);
        let bytes = package(&[
            ("[Content_Types].xml", &content_types()),
            ("_rels/.rels", &root_rels()),
            ("ppt/presentation.xml", &presentation()),
            (
                "ppt/_rels/presentation.xml.rels",
                &pres_rels(
                    r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/>"#,
                ),
            ),
            ("ppt/slides/slide1.xml", &slide(&pic)),
            (
                "ppt/slides/_rels/slide1.xml.rels",
                &slide_rels(
                    r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image1.png"/>"#,
                ),
            ),
            ("ppt/media/image1.png", b"fake-png-bytes"),
        ]);
        let findings = findings_of(&bytes);
        assert!(findings.iter().any(|f| f.id == "image/crop-retains-data"), "{edge}");
    }
}
