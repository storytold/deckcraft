//! Original synthetic diagrams exercise identity/relationship retention without vendor fixtures.
use deckcraft_engine::Session;
use deckcraft_model::{Diagram, Shape, ShapeKind};
use deckcraft_pptx::{XmlEl, parse_xml};
use std::{
    io::{Cursor, Read, Write},
    sync::Arc,
};

fn files(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    (0..zip.len())
        .map(|i| {
            let mut f = zip.by_index(i).unwrap();
            let mut v = vec![];
            f.read_to_end(&mut v).unwrap();
            (f.name().into(), v)
        })
        .collect()
}
fn zip(files: Vec<(String, Vec<u8>)>) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (n, b) in files {
        z.start_file(n, zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(&b).unwrap();
    }
    z.finish().unwrap().into_inner()
}
fn by_root(bytes: &[u8], name: &str) -> XmlEl {
    files(bytes).into_iter().find_map(|(_, b)| parse_xml(&b).ok().map(|d| d.root).filter(|r| r.is(name))).unwrap()
}
fn fixture() -> Vec<u8> {
    let mut s = Session::with_new();
    s.execute("insert.smartArt", &serde_json::json!({"kind":"process"})).unwrap();
    s.execute("smartArt.setNodes", &serde_json::json!({"lines":["Same","Same","Third"]})).unwrap();
    let bytes = deckcraft_pptx::export(&s.doc().unwrap().doc).unwrap();
    let data = by_root(&bytes, "dataModel");
    let ids: Vec<_> = data
        .child("ptLst")
        .unwrap()
        .children_named("pt")
        .filter(|p| p.attr("type").is_none())
        .map(|p| p.attr("modelId").unwrap().to_string())
        .collect();
    let mut parts = files(&bytes);
    for (_, b) in &mut parts {
        let Ok(mut doc) = parse_xml(b) else {
            continue;
        };
        if doc.root.is("drawing") {
            for (i, sp) in doc
                .root
                .elements_mut()
                .find(|e| e.is("spTree"))
                .unwrap()
                .elements_mut()
                .filter(|e| e.is("sp") && e.child("txBody").is_some())
                .enumerate()
            {
                sp.set_attr("modelId", &ids[i]);
            }
            *b = doc.root.to_xml().into_bytes();
        }
    }
    zip(parts)
}
fn group(p: &deckcraft_model::Presentation) -> &Shape {
    p.slides.iter().flat_map(|s| &s.shapes).find(|s| s.diagram.is_some()).unwrap()
}
fn geometry(s: &Shape) -> Vec<deckcraft_geom::Xfrm> {
    let mut out = vec![];
    {
        deckcraft_model::walk(s.children(), &mut |s, _| {
            if let Some(x) = s.xfrm {
                out.push(x);
            }
        });
    }
    out
}
fn ids(root: &XmlEl) -> Vec<String> {
    root.child("ptLst").unwrap().children_named("pt").map(|p| p.attr("modelId").unwrap().into()).collect()
}

#[test]
fn imported_diagram_keeps_ids_drawing_bytes_and_identity_based_text_edits() {
    let bytes = fixture();
    let mut p = deckcraft_pptx::import(&bytes).unwrap();
    let g = group(&p).clone();
    assert!(g.diagram.as_ref().unwrap().kept.is_some());
    let saved = deckcraft_pptx::export(&p).unwrap();
    assert_eq!(ids(&by_root(&bytes, "dataModel")), ids(&by_root(&saved, "dataModel")));
    assert_eq!(by_root(&bytes, "drawing").to_xml(), by_root(&saved, "drawing").to_xml());
    let mut changed = g.diagram.clone().unwrap();
    changed.nodes[0].text = "First changed".into();
    let new = deckcraft_pptx::edit_imported_diagram_text(&g, &changed).unwrap().unwrap();
    assert_eq!(geometry(&new), geometry(&g));
    let shape = Arc::make_mut(&mut p.slides[0]).shapes.iter_mut().find(|s| s.diagram.is_some()).unwrap();
    *shape = new;
    let bytes = deckcraft_pptx::export(&p).unwrap();
    let back = deckcraft_pptx::import(&bytes).unwrap();
    let d = group(&back).diagram.as_ref().unwrap();
    assert_eq!(d.nodes[0].text, "First changed");
    assert_eq!(d.nodes[1].text, "Same");
    assert_eq!(geometry(group(&back)), geometry(&g));
    assert_eq!(ids(&by_root(&saved, "dataModel")), ids(&by_root(&bytes, "dataModel")));
    if let Some(dir) = std::env::var_os("DECKCRAFT_DIAGRAM_OUT") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("synthetic-original.pptx"), fixture()).unwrap();
        std::fs::write(dir.join("synthetic-preserved.pptx"), saved).unwrap();
        std::fs::write(dir.join("synthetic-text-edited.pptx"), bytes).unwrap();
    }
}

#[test]
fn changed_geometry_does_not_reuse_stale_drawing_and_ambiguous_text_fails_atomically() {
    let p = deckcraft_pptx::import(&fixture()).unwrap();
    let mut g = group(&p).clone();
    let before = g.clone();
    if let ShapeKind::Group { children, .. } = &mut g.kind {
        children[0].xfrm.as_mut().unwrap().rot += 15.0;
    }
    let mut changed: Diagram = g.diagram.clone().unwrap();
    changed.nodes[0].text = "Edit".into();
    assert!(deckcraft_pptx::edit_imported_diagram_text(&g, &changed).is_err());
    assert_eq!(group(&p), &before);
}

#[test]
fn text_pane_command_undo_and_native_save_keep_the_imported_diagram() {
    let path = std::env::temp_dir().join(format!("diagram-command-{}.pptx", std::process::id()));
    let native = path.with_extension("deckcraft");
    std::fs::write(&path, fixture()).unwrap();
    let mut s = Session::with_new();
    s.execute("file.open", &serde_json::json!({"path": path})).unwrap();
    let before = group(&s.doc().unwrap().doc).clone();
    s.execute("smartArt.setNodes", &serde_json::json!({"id": before.id.0, "index": 0, "text": "Command edit"})).unwrap();
    let changed = group(&s.doc().unwrap().doc).clone();
    assert_eq!(geometry(&changed), geometry(&before));
    assert!(changed.diagram.as_ref().unwrap().imported);
    let exported = deckcraft_pptx::export(&s.doc().unwrap().doc).unwrap();
    assert!(by_root(&exported, "drawing").text().contains("Command edit"));
    s.execute("edit.undo", &serde_json::json!({})).unwrap();
    assert_eq!(group(&s.doc().unwrap().doc), &before);
    s.execute("edit.redo", &serde_json::json!({})).unwrap();
    s.execute("file.save", &serde_json::json!({"path": native})).unwrap();
    s.execute("file.open", &serde_json::json!({"path": native})).unwrap();
    let after_native = deckcraft_pptx::export(&s.doc().unwrap().doc).unwrap();
    assert_eq!(by_root(&exported, "drawing"), by_root(&after_native, "drawing"));
    assert_eq!(ids(&by_root(&exported, "dataModel")), ids(&by_root(&after_native, "dataModel")));
    std::fs::remove_file(path).unwrap();
    std::fs::remove_file(native).unwrap();
}

#[test]
fn presentation_point_associations_update_only_the_owned_node() {
    let bytes = fixture();
    let mut parts = files(&bytes);
    let mut bindings = Vec::new();
    for (_, bytes) in &mut parts {
        let Ok(mut doc) = parse_xml(bytes) else { continue };
        if !doc.root.is("dataModel") {
            continue;
        }
        let points = doc.root.elements_mut().find(|e| e.is("ptLst")).unwrap();
        let mut new = vec![];
        for (i, pt) in points.children_named("pt").filter(|p| p.attr("type").is_none()).enumerate() {
            let original = pt.attr("modelId").unwrap().to_string();
            let pres = format!("presentation-{i}");
            let xml = format!(r#"<dgm:pt modelId="{pres}" type="pres"><dgm:prSet presAssocID="{original}" presName="nodeText"/></dgm:pt>"#);
            new.extend(parse_xml(format!("<root>{xml}</root>").as_bytes()).unwrap().root.children);
            bindings.push((original, pres));
        }
        points.children.extend(new);
        *bytes = doc.root.to_xml().into_bytes();
    }
    for (_, bytes) in &mut parts {
        let Ok(mut doc) = parse_xml(bytes) else { continue };
        if !doc.root.is("drawing") {
            continue;
        }
        for sp in doc.root.elements_mut().find(|e| e.is("spTree")).unwrap().elements_mut() {
            if let Some((_, pres)) = bindings.iter().find(|(id, _)| sp.attr("modelId") == Some(id.as_str())) {
                sp.set_attr("modelId", pres);
            }
        }
        *bytes = doc.root.to_xml().into_bytes();
    }
    let p = deckcraft_pptx::import(&zip(parts)).unwrap();
    let g = group(&p);
    let mut updated = g.diagram.clone().unwrap();
    updated.nodes[0].text = "Associated edit".into();
    let result = deckcraft_pptx::edit_imported_diagram_text(g, &updated).unwrap().unwrap();
    let d = result.diagram.as_ref().unwrap();
    assert_eq!(d.nodes[1].text, "Same");
    assert_eq!(geometry(&result), geometry(g));
    assert!(result.children().iter().filter_map(|s| s.text.as_ref()).any(|t| t.text().contains("Associated edit")));
    updated.nodes[0].title = "No mapped title shape".into();
    assert!(deckcraft_pptx::edit_imported_diagram_text(g, &updated).is_err());
}
