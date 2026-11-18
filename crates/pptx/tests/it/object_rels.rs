//! Objects DeckCraft doesn't model (OLE, SmartArt without a drawing) whose kept parts have
//! relationships of their own, or that link outside the file, keep all of it on save (#1993).
//! Targets that are unsafe to carry along still fall back to the preview.

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};

use deckcraft_model::{ShapeKind, defaults};

const RT: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const SLIDE: &str = "ppt/slides/slide1.xml";
const OLE_CT: &str = "application/vnd.openxmlformats-officedocument.oleObject";
const PACKAGE_CT: &str = "application/vnd.deckcraft.test-package";
const OLE: &[u8] = b"\xD0\xCF\x11\xE0deckcraft-test-ole";
const PACKAGE: &[u8] = b"PK\x03\x04deckcraft-test-package";
const LINKED: &str = "file:///C:/Docs/Linked.xlsx";
const BUDGET: &str = "file:///C:/Docs/Budget.xlsx";

fn png(rgb: [u8; 3]) -> Vec<u8> {
    let mut out = Cursor::new(vec![]);
    image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(4, 4, image::Rgb(rgb))).write_to(&mut out, image::ImageFormat::Png).expect("png");
    out.into_inner()
}

fn rel(id: &str, ty: &str, target: &str) -> String {
    format!(r#"<Relationship Id="{id}" Type="{RT}/{ty}" Target="{target}"/>"#)
}

fn external(id: &str, ty: &str, target: &str) -> String {
    format!(r#"<Relationship Id="{id}" Type="{RT}/{ty}" Target="{target}" TargetMode="External"/>"#)
}

fn rels(items: &[String]) -> Vec<u8> {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{}</Relationships>"#,
        items.concat()
    )
    .into_bytes()
}

/// An exported deck with `parts` added, and `slide`, `slide_rels` and `types` appended to the
/// first slide's shape tree, its relationships and the content types.
fn deck(parts: &[(String, Vec<u8>)], slide: &str, slide_rels: &str, types: &str) -> Vec<u8> {
    let base = deckcraft_pptx::export(&defaults::new_presentation(None)).expect("export");
    let mut z = zip::ZipArchive::new(Cursor::new(base)).expect("zip");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let o = zip::write::SimpleFileOptions::default();
    for i in 0..z.len() {
        let mut f = z.by_index(i).expect("entry");
        let name = f.name().to_string();
        let mut d = vec![];
        f.read_to_end(&mut d).expect("read");
        let text = || String::from_utf8(d.clone()).expect("utf8");
        let d = match name.as_str() {
            SLIDE => text().replacen("</p:spTree>", &format!("{slide}</p:spTree>"), 1).into_bytes(),
            "ppt/slides/_rels/slide1.xml.rels" => text().replacen("</Relationships>", &format!("{slide_rels}</Relationships>"), 1).into_bytes(),
            "[Content_Types].xml" => text().replacen("</Types>", &format!("{types}</Types>"), 1).into_bytes(),
            _ => d,
        };
        out.start_file(name, o).expect("start");
        out.write_all(&d).expect("write");
    }
    for (name, data) in parts {
        out.start_file(name.as_str(), o).expect("start");
        out.write_all(data).expect("write");
    }
    out.finish().expect("finish").into_inner()
}

fn ct(part: &str, ty: &str) -> String {
    format!(r#"<Override PartName="/{part}" ContentType="{ty}"/>"#)
}

/// An OLE frame named `Object {id}` whose `p:oleObj` (`name`) points at `rid`, with its preview.
fn ole_frame(id: u32, name: &str, rid: &str, form: &str) -> String {
    format!(
        r#"<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="{id}" name="Object {id}"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="914400"/></p:xfrm><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/presentationml/2006/ole"><p:oleObj name="{name}" r:id="{rid}" progId="Package"><{form}/><p:pic><p:nvPicPr><p:cNvPr id="0" name=""/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="rIdIcon"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="914400"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic></p:oleObj></a:graphicData></a:graphic></p:graphicFrame>"#
    )
}

/// A slide with an embedded object (`Package`, `Object 3`) whose part has `ole_rels`, and a
/// linked one (`Linked`, `Object 4`). `extra` adds parts.
fn ole_deck(ole_rels: &[String], extra: Vec<(String, Vec<u8>)>) -> Vec<u8> {
    let mut parts = vec![
        ("ppt/embeddings/oleObject1.bin".to_string(), OLE.to_vec()),
        ("ppt/embeddings/_rels/oleObject1.bin.rels".to_string(), rels(ole_rels)),
        ("ppt/embeddings/package2.bin".to_string(), PACKAGE.to_vec()),
        // Points back at the object's own part: a cycle.
        ("ppt/embeddings/_rels/package2.bin.rels".to_string(), rels(&[rel("rId1", "oleObject", "oleObject1.bin")])),
        ("ppt/media/icon.png".to_string(), png([10, 200, 30])),
        ("ppt/media/icon2.png".to_string(), png([200, 10, 30])),
    ];
    parts.extend(extra);
    let slide = format!("{}{}", ole_frame(3, "Package", "rIdOle", "p:embed"), ole_frame(4, "Linked", "rIdLink", "p:link"));
    let slide_rels = [
        rel("rIdOle", "oleObject", "../embeddings/oleObject1.bin"),
        external("rIdLink", "oleObject", LINKED),
        rel("rIdIcon", "image", "../media/icon.png"),
    ]
    .concat();
    let types = [
        ct("ppt/embeddings/oleObject1.bin", OLE_CT),
        ct("ppt/embeddings/package2.bin", PACKAGE_CT),
        ct("ppt/media/icon.png", "image/png"),
        ct("ppt/media/icon2.png", "image/png"),
    ]
    .concat();
    deck(&parts, &slide, &slide_rels, &types)
}

/// A saved package, read back part by part.
struct Pkg(BTreeMap<String, Vec<u8>>);

impl Pkg {
    fn new(bytes: &[u8]) -> Self {
        let mut z = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
        let mut out = BTreeMap::new();
        for i in 0..z.len() {
            let mut f = z.by_index(i).expect("entry");
            let mut d = vec![];
            f.read_to_end(&mut d).expect("read");
            out.insert(f.name().to_string(), d);
        }
        Pkg(out)
    }

    fn get(&self, part: &str) -> &[u8] {
        self.0.get(part).unwrap_or_else(|| panic!("no part {part}"))
    }

    fn xml(&self, part: &str) -> deckcraft_pptx::XmlDoc {
        deckcraft_pptx::parse_xml(self.get(part)).expect("xml")
    }

    /// Relationship `id` of `part`: its type, the part it reaches (or the URL) and whether it is
    /// external.
    fn rel(&self, part: &str, id: &str) -> (String, String, bool) {
        let (dir, file) = part.rsplit_once('/').unwrap_or(("", part));
        let doc = self.xml(&format!("{dir}/_rels/{file}.rels"));
        let r = doc.root.children_named("Relationship").find(|r| r.attr("Id") == Some(id)).unwrap_or_else(|| panic!("{part} has no {id}"));
        let (ty, target) = (r.attr("Type").expect("type").to_string(), r.attr("Target").expect("target").to_string());
        if r.attr("TargetMode") == Some("External") {
            return (ty, target, true);
        }
        let mut segs: Vec<&str> = dir.split('/').collect();
        for s in target.split('/') {
            if s == ".." {
                segs.pop();
            } else {
                segs.push(s);
            }
        }
        (ty, segs.join("/"), false)
    }

    fn content_type(&self, part: &str) -> String {
        let doc = self.xml("[Content_Types].xml");
        let over = doc.root.children_named("Override").find(|o| o.attr("PartName") == Some(&format!("/{part}")));
        let ext = part.rsplit('.').next().unwrap_or("");
        let def = || doc.root.children_named("Default").find(|d| d.attr("Extension").is_some_and(|e| e.eq_ignore_ascii_case(ext)));
        over.or_else(def).and_then(|e| e.attr("ContentType")).expect("content type").to_string()
    }

    /// The relationship id the slide's `p:oleObj` named `name` uses, if it was kept.
    fn ole_rid(&self, name: &str) -> Option<String> {
        let doc = self.xml(SLIDE);
        let mut found = vec![];
        doc.root.find_all("oleObj", &mut found);
        found.into_iter().find(|o| o.attr("name") == Some(name)).and_then(|o| o.attr("r:id")).map(str::to_string)
    }

    /// The bytes of the picture the slide's frame `name` is written as, if it is one.
    fn picture_bytes(&self, name: &str) -> Option<Vec<u8>> {
        let doc = self.xml(SLIDE);
        let mut pics = vec![];
        doc.root.find_all("pic", &mut pics);
        let pic = pics.into_iter().find(|p| p.find("cNvPr").and_then(|c| c.attr("name")) == Some(name))?;
        let rid = pic.find("blip")?.attr("r:embed")?;
        let (_, part, external) = self.rel(SLIDE, rid);
        (!external).then(|| self.get(&part).to_vec())
    }
}

fn round(bytes: &[u8]) -> Vec<u8> {
    deckcraft_pptx::export(&deckcraft_pptx::import(bytes).expect("import")).expect("export")
}

fn check_objects(bytes: &[u8]) {
    let pkg = Pkg::new(bytes);
    let rid = pkg.ole_rid("Package").expect("the embedded object is kept, not written as its preview");
    let (ty, ole, external) = pkg.rel(SLIDE, &rid);
    assert_eq!((ty.as_str(), external), (format!("{RT}/oleObject").as_str(), false));
    assert_eq!(pkg.get(&ole), OLE);
    assert_eq!(pkg.content_type(&ole), OLE_CT);

    // The object's part keeps its own relationships, under the ids the part uses.
    let (ty, icon, external) = pkg.rel(&ole, "rId1");
    assert_eq!((ty.as_str(), external), (format!("{RT}/image").as_str(), false));
    assert_eq!(pkg.get(&icon), png([200, 10, 30]).as_slice());
    assert_eq!(pkg.content_type(&icon), "image/png");
    assert_eq!(pkg.rel(&ole, "rId2"), (format!("{RT}/hyperlink"), BUDGET.to_string(), true), "an external target stays a link");
    let (ty, package, external) = pkg.rel(&ole, "rId3");
    assert_eq!((ty.as_str(), external), (format!("{RT}/package").as_str(), false));
    assert_eq!(pkg.get(&package), PACKAGE);
    assert_eq!(pkg.content_type(&package), PACKAGE_CT);
    assert_eq!(pkg.rel(&package, "rId1"), (format!("{RT}/oleObject"), ole.clone(), false), "the cycle back to the object's part");

    // A linked object keeps its link.
    let rid = pkg.ole_rid("Linked").expect("the linked object is kept, not written as its preview");
    assert_eq!(pkg.rel(SLIDE, &rid), (format!("{RT}/oleObject"), LINKED.to_string(), true));
}

/// #1993: an embedded object whose part has relationships of its own (a picture, an external
/// link, another part pointing back at it), and a linked object, were written as their previews.
#[test]
fn objects_keep_their_parts_own_relationships_and_links() {
    let src = ole_deck(
        &[rel("rId1", "image", "../media/icon2.png"), external("rId2", "hyperlink", BUDGET), rel("rId3", "package", "package2.bin")],
        vec![],
    );
    // Twice: the second save starts from what the first one wrote.
    let first = round(&src);
    check_objects(&first);
    check_objects(&round(&first));
}

/// #1993 review: two parts of one file with equal bytes and different relationships of their own
/// stay two parts on save, each with its own relationships.
#[test]
fn equal_parts_with_different_relationships_stay_apart() {
    let (first, second) = (png([10, 200, 30]), png([200, 10, 30]));
    let parts = vec![
        ("ppt/embeddings/oleObject1.bin".to_string(), OLE.to_vec()),
        ("ppt/embeddings/_rels/oleObject1.bin.rels".to_string(), rels(&[rel("rId1", "image", "../media/icon.png")])),
        ("ppt/embeddings/oleObject2.bin".to_string(), OLE.to_vec()),
        ("ppt/embeddings/_rels/oleObject2.bin.rels".to_string(), rels(&[rel("rId1", "image", "../media/icon2.png")])),
        ("ppt/media/icon.png".to_string(), first.clone()),
        ("ppt/media/icon2.png".to_string(), second.clone()),
    ];
    let slide = format!("{}{}", ole_frame(3, "First", "rIdA", "p:embed"), ole_frame(4, "Second", "rIdB", "p:embed"));
    let slide_rels = [
        rel("rIdA", "oleObject", "../embeddings/oleObject1.bin"),
        rel("rIdB", "oleObject", "../embeddings/oleObject2.bin"),
        rel("rIdIcon", "image", "../media/icon.png"),
    ]
    .concat();
    let types = [
        ct("ppt/embeddings/oleObject1.bin", OLE_CT),
        ct("ppt/embeddings/oleObject2.bin", OLE_CT),
        ct("ppt/media/icon.png", "image/png"),
        ct("ppt/media/icon2.png", "image/png"),
    ]
    .concat();
    let first_save = round(&deck(&parts, &slide, &slide_rels, &types));
    for bytes in [first_save.clone(), round(&first_save)] {
        let pkg = Pkg::new(&bytes);
        let mut written = vec![];
        for (name, icon) in [("First", &first), ("Second", &second)] {
            let rid = pkg.ole_rid(name).expect("kept, not written as its preview");
            let (_, part, _) = pkg.rel(SLIDE, &rid);
            assert_eq!(pkg.get(&part), OLE);
            let (_, image, _) = pkg.rel(&part, "rId1");
            assert_eq!(pkg.get(&image), icon.as_slice(), "{name} keeps its own picture");
            written.push(part);
        }
        assert_ne!(written[0], written[1], "two parts, not one shared");
    }
}

/// #1993: SmartArt without a drawing whose data part has pictures and links of its own.
#[test]
fn smartart_data_keeps_its_pictures_and_links() {
    let dgm = "http://schemas.openxmlformats.org/drawingml/2006/diagram";
    let data = format!(
        r#"<dgm:dataModel xmlns:dgm="{dgm}" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="{RT}"><dgm:ptLst><dgm:pt modelId="1"><dgm:spPr><a:blipFill><a:blip r:embed="rId1"/></a:blipFill></dgm:spPr><dgm:t><a:p><a:r><a:rPr><a:hlinkClick r:id="rId2"/></a:rPr><a:t>Site</a:t></a:r></a:p></dgm:t></dgm:pt></dgm:ptLst></dgm:dataModel>"#
    );
    let mut parts = vec![
        ("ppt/diagrams/data1.xml".to_string(), data.into_bytes()),
        (
            "ppt/diagrams/_rels/data1.xml.rels".to_string(),
            rels(&[rel("rId1", "image", "../media/node.png"), external("rId2", "hyperlink", "https://example.com/site")]),
        ),
        ("ppt/media/node.png".to_string(), png([30, 30, 200])),
    ];
    let mut types = ct("ppt/diagrams/data1.xml", "application/vnd.openxmlformats-officedocument.drawingml.diagramData+xml")
        + &ct("ppt/media/node.png", "image/png");
    let mut slide_rels = rel("rIdDm", "diagramData", "../diagrams/data1.xml");
    for (rid, kind, root) in [("rIdLo", "Layout", "layoutDef"), ("rIdQs", "QuickStyle", "styleDef"), ("rIdCs", "Colors", "colorsDef")] {
        let name = format!("ppt/diagrams/{}1.xml", kind.to_ascii_lowercase());
        parts.push((name.clone(), format!(r#"<dgm:{root} xmlns:dgm="{dgm}"/>"#).into_bytes()));
        types += &ct(&name, &format!("application/vnd.openxmlformats-officedocument.drawingml.diagram{kind}+xml"));
        slide_rels += &rel(rid, &format!("diagram{kind}"), &format!("../diagrams/{}1.xml", kind.to_ascii_lowercase()));
    }
    let frame = format!(
        r#"<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="5" name="Diagram 5"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="914400"/></p:xfrm><a:graphic><a:graphicData uri="{dgm}"><dgm:relIds xmlns:dgm="{dgm}" r:dm="rIdDm" r:lo="rIdLo" r:qs="rIdQs" r:cs="rIdCs"/></a:graphicData></a:graphic></p:graphicFrame>"#
    );
    let src = deck(&parts, &frame, &slide_rels, &types);

    let first = round(&src);
    for bytes in [first.clone(), round(&first)] {
        let pkg = Pkg::new(&bytes);
        let doc = pkg.xml(SLIDE);
        let ids = doc.root.find("relIds").expect("the diagram is kept, not written as its preview");
        let (ty, data, _) = pkg.rel(SLIDE, ids.attr("r:dm").expect("data"));
        assert_eq!(ty, format!("{RT}/diagramData"));
        assert!(String::from_utf8_lossy(pkg.get(&data)).contains("Site"));
        let (ty, node, external) = pkg.rel(&data, "rId1");
        assert_eq!((ty.as_str(), external), (format!("{RT}/image").as_str(), false));
        assert_eq!(pkg.get(&node), png([30, 30, 200]).as_slice());
        assert_eq!(pkg.content_type(&node), "image/png");
        assert_eq!(pkg.rel(&data, "rId2"), (format!("{RT}/hyperlink"), "https://example.com/site".to_string(), true));
    }
}

/// #1993: a relationship of a kept part whose target isn't safe to carry along keeps the old
/// fallback: the object keeps no part and a save writes its preview.
#[test]
fn unsafe_part_targets_still_fall_back_to_the_preview() {
    // A chain of parts, each pointing at the next; the last has no relationships.
    let chain = |n: usize| -> Vec<(String, Vec<u8>)> {
        let mut out = vec![];
        for i in 1..=n {
            out.push((format!("ppt/embeddings/chain{i}.bin"), vec![i as u8]));
            if i < n {
                out.push((format!("ppt/embeddings/_rels/chain{i}.bin.rels"), rels(&[rel("rId1", "package", &format!("chain{}.bin", i + 1))])));
            }
        }
        out
    };
    let cases: Vec<(&str, Vec<String>, Vec<(String, Vec<u8>)>)> = vec![
        ("climbs above the package root", vec![rel("rId1", "image", "../../../../ppt/media/icon2.png")], vec![]),
        ("climbs above the root, percent-encoded", vec![rel("rId1", "image", "%2e%2e/%2e%2e/%2e%2e/ppt/media/icon2.png")], vec![]),
        ("an absolute target with an empty segment", vec![rel("rId1", "image", "//host/icon2.png")], vec![]),
        ("an absolute target climbing above the root", vec![rel("rId1", "image", "/../ppt/media/icon2.png")], vec![]),
        ("an absolute target outside the folders objects keep parts in", vec![rel("rId1", "slide", "/ppt/slides/slide1.xml")], vec![]),
        ("an absolute relationships part", vec![rel("rId1", "package", "/ppt/embeddings/_rels/package2.bin.rels")], vec![]),
        ("backslashes", vec![rel("rId1", "image", r"..\media\icon2.png")], vec![]),
        ("a scheme on an internal target", vec![rel("rId1", "image", "http://example.com/icon2.png")], vec![]),
        ("outside the folders objects keep parts in", vec![rel("rId1", "slide", "../slides/slide1.xml")], vec![]),
        ("a relationships part", vec![rel("rId1", "package", "_rels/package2.bin.rels")], vec![]),
        ("missing", vec![rel("rId1", "image", "../media/missing.png")], vec![]),
        ("deeper than the limit", vec![rel("rId1", "package", "chain1.bin")], chain(12)),
    ];
    for (why, ole_rels, extra) in cases {
        let src = ole_deck(&ole_rels, extra);
        let p = deckcraft_pptx::import(&src).expect("import");
        let s = p.slides[0].shapes.iter().find(|s| s.name == "Object 3").expect("object");
        let ShapeKind::Opaque { parts, .. } = &s.kind else { panic!("{why}: {:?}", s.kind) };
        assert!(parts.is_empty(), "{why}: {parts:?}");
        let pkg = Pkg::new(&deckcraft_pptx::export(&p).expect("export"));
        assert!(pkg.ole_rid("Package").is_none(), "{why}: written as its preview");
        assert_eq!(pkg.picture_bytes("Object 3"), Some(png([10, 200, 30])), "{why}: the preview picture is written");
        assert!(!pkg.0.values().any(|d| d == OLE), "{why}: the embedding isn't copied");
    }
}

/// #1993 review: an object's own (direct) relationship target is checked as the parts it reaches
/// are: one `kept_target` refuses keeps the fallback even though it resolves to a part.
#[test]
fn unsafe_direct_targets_fall_back_to_the_preview() {
    for target in [
        "../../../../ppt/embeddings/oleObject1.bin",
        r"..\embeddings\oleObject1.bin",
        "//host/ppt/embeddings/oleObject1.bin",
        "/../ppt/embeddings/oleObject1.bin",
        "/ppt/slides/slide1.xml",
        "/ppt/_rels/presentation.xml.rels",
    ] {
        let parts = vec![("ppt/embeddings/oleObject1.bin".to_string(), OLE.to_vec()), ("ppt/media/icon.png".to_string(), png([10, 200, 30]))];
        let slide_rels = [rel("rIdOle", "oleObject", target), rel("rIdIcon", "image", "../media/icon.png")].concat();
        let types = [ct("ppt/embeddings/oleObject1.bin", OLE_CT), ct("ppt/media/icon.png", "image/png")].concat();
        let src = deck(&parts, &ole_frame(3, "Package", "rIdOle", "p:embed"), &slide_rels, &types);
        let p = deckcraft_pptx::import(&src).expect("import");
        let s = p.slides[0].shapes.iter().find(|s| s.name == "Object 3").expect("object");
        let ShapeKind::Opaque { parts, .. } = &s.kind else { panic!("{target}: {:?}", s.kind) };
        assert!(parts.is_empty(), "{target}: {parts:?}");
        let pkg = Pkg::new(&deckcraft_pptx::export(&p).expect("export"));
        assert!(pkg.ole_rid("Package").is_none(), "{target}: written as its preview");
        assert_eq!(pkg.picture_bytes("Object 3"), Some(png([10, 200, 30])), "{target}: the preview picture is written");
        assert!(!pkg.0.values().any(|d| d == OLE), "{target}: the embedding isn't copied");
    }
}

/// #1993 review: a package-absolute target (`/ppt/…`, resolved from the package root as ECMA-376
/// Part 2 allows, and written by some generators) is kept, for the object's own part and for the
/// parts that part reaches, as #203 kept it.
#[test]
fn absolute_package_targets_are_kept() {
    let parts = vec![
        ("ppt/embeddings/oleObject1.bin".to_string(), OLE.to_vec()),
        ("ppt/embeddings/_rels/oleObject1.bin.rels".to_string(), rels(&[rel("rId1", "image", "/ppt/media/icon2.png")])),
        ("ppt/media/icon.png".to_string(), png([10, 200, 30])),
        ("ppt/media/icon2.png".to_string(), png([200, 10, 30])),
    ];
    let slide_rels = [rel("rIdOle", "oleObject", "/ppt/embeddings/oleObject1.bin"), rel("rIdIcon", "image", "../media/icon.png")].concat();
    let types = [ct("ppt/embeddings/oleObject1.bin", OLE_CT), ct("ppt/media/icon.png", "image/png"), ct("ppt/media/icon2.png", "image/png")].concat();
    let first = round(&deck(&parts, &ole_frame(3, "Package", "rIdOle", "p:embed"), &slide_rels, &types));
    for bytes in [first.clone(), round(&first)] {
        let pkg = Pkg::new(&bytes);
        let rid = pkg.ole_rid("Package").expect("the embedded object is kept, not written as its preview");
        let (_, ole, external) = pkg.rel(SLIDE, &rid);
        assert!(!external);
        assert_eq!(pkg.get(&ole), OLE);
        let (ty, icon, external) = pkg.rel(&ole, "rId1");
        assert_eq!((ty.as_str(), external), (format!("{RT}/image").as_str(), false));
        assert_eq!(pkg.get(&icon), png([200, 10, 30]).as_slice());
    }
}
