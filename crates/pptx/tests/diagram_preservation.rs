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

fn uncached_fixture() -> Vec<u8> {
    let mut s = Session::with_new();
    s.execute("insert.smartArt", &serde_json::json!({"kind":"process"})).unwrap();
    s.execute(
        "smartArt.setNodes",
        &serde_json::json!({"lines":["Research","\tObserve","\tMeasure","Build","\tPrototype","\tTest","Deliver","\tReview","\tShip"]}),
    )
    .unwrap();
    let mut parts = files(&deckcraft_pptx::export(&s.doc().unwrap().doc).unwrap());
    // An original nested card layout: a parent label and its ordered child labels. Deliberately
    // unrelated to the layout name, so template-name dispatch cannot make this test pass.
    let layout = r#"<dgm:layoutDef xmlns:dgm="http://schemas.openxmlformats.org/drawingml/2006/diagram" uniqueId="urn:deckcraft:test:uncached-original" minVer="12.0">
 <dgm:title val="Original nested cards"/><dgm:desc val="Three parent cards with child labels"/><dgm:catLst><dgm:cat type="process" pri="99"/></dgm:catLst>
 <dgm:layoutNode name="canvas"><dgm:alg type="lin"/><dgm:presOf/><dgm:shape type="none"/>
 <dgm:constrLst><dgm:constr type="w" for="ch" refType="w" fact="0.25"/><dgm:constr type="h" for="ch" refType="h" fact="0.8"/></dgm:constrLst><dgm:ruleLst/>
 <dgm:forEach axis="ch" ptType="node"><dgm:layoutNode name="card"><dgm:alg type="composite"/><dgm:presOf/><dgm:shape type="none"/>
 <dgm:constrLst><dgm:constr type="w" for="ch" forName="header" refType="w" fact="0.9"/><dgm:constr type="w" for="ch" forName="details" refType="w" fact="0.9"/><dgm:constr type="l" for="ch" forName="header" refType="w" fact="0.05"/><dgm:constr type="l" for="ch" forName="details" refType="w" fact="0.05"/>
 <dgm:constr type="h" for="ch" forName="header" refType="h" fact="0.25"/><dgm:constr type="t" for="ch" forName="header" val="0"/>
 <dgm:constr type="h" for="ch" forName="details" refType="h" fact="0.65"/><dgm:constr type="t" for="ch" forName="details" refType="h" fact="0.35"/>
 </dgm:constrLst><dgm:ruleLst/>
 <dgm:layoutNode name="header" styleLbl="node1"><dgm:alg type="tx"/><dgm:presOf axis="self"/><dgm:shape type="roundRect"/>
 <dgm:constrLst><dgm:constr type="primFontSz" val="18"/></dgm:constrLst><dgm:ruleLst/></dgm:layoutNode>
 <dgm:layoutNode name="details" styleLbl="node1"><dgm:alg type="tx"/><dgm:presOf axis="ch" ptType="node"/><dgm:shape type="rect"/>
 <dgm:constrLst><dgm:constr type="primFontSz" val="14"/></dgm:constrLst><dgm:ruleLst/></dgm:layoutNode></dgm:layoutNode></dgm:forEach></dgm:layoutNode></dgm:layoutDef>"#;
    let drawing: Vec<_> = parts.iter().filter(|(_, b)| parse_xml(b).ok().is_some_and(|d| d.root.is("drawing"))).map(|(n, _)| n.clone()).collect();
    parts.retain(|(n, _)| !drawing.contains(n));
    for (_, bytes) in &mut parts {
        let Ok(mut doc) = parse_xml(bytes) else { continue };
        if doc.root.is("Relationships") {
            let mut xml = doc.root.to_xml();
            for e in doc.root.elements().filter(|e| e.attr("Type").is_some_and(|t| t.ends_with("diagramDrawing"))) {
                xml = xml.replace(&e.to_xml(), "");
            }
            *bytes = xml.into_bytes();
        } else if doc.root.is("Types") {
            let mut xml = doc.root.to_xml();
            for e in doc.root.elements().filter(|e| e.attr("ContentType").is_some_and(|t| t.contains("diagramDrawing"))) {
                xml = xml.replace(&e.to_xml(), "");
            }
            *bytes = xml.into_bytes();
        } else if doc.root.is("layoutDef") {
            *bytes = layout.as_bytes().to_vec();
        } else if doc.root.is("dataModel") {
            if let Some(points) = doc.root.elements_mut().find(|e| e.is("ptLst")) {
                for point in points.elements_mut() {
                    if point.attr("type") == Some("doc")
                        && let Some(pr) = point.elements_mut().find(|e| e.is("prSet"))
                    {
                        pr.set_attr("loTypeId", "urn:deckcraft:test:uncached-original");
                    }
                }
            }
            if let Some(ext) = doc.root.elements_mut().find(|e| e.is("extLst")) {
                ext.children.clear();
            }
            *bytes = doc.root.to_xml().replace("<a:rPr lang=\"en-US\"/>", "<a:rPr lang=\"en-US\"><a:latin typeface=\"Inter\"/></a:rPr>").into_bytes();
        }
    }
    zip(parts)
}

#[test]
fn uncached_custom_layout_executes_tree_constraints_and_retains_original_graph() {
    let input = uncached_fixture();
    let original_ids = ids(&by_root(&input, "dataModel"));
    let p = deckcraft_pptx::import(&input).unwrap();
    let g = group(&p);
    let first = g.children()[0].xfrm.unwrap();
    // Independently measured from native rendering of this original uncached definition.
    for (actual, expected) in [(first.x, 124.8), (first.y, 32.4), (first.w, 172.8), (first.h, 64.8)] {
        assert!((actual - expected).abs() < 1e-6, "{actual} != {expected}");
    }
    assert_eq!(g.children().len(), 6, "layout should create two shapes per top-level node");
    let first_text = g.children()[0].text.as_ref().unwrap();
    assert_eq!(first_text.body.inset_l, Some(18.0 * 0.56));
    assert_eq!(first_text.paragraphs[0].props.line_spacing, Some(deckcraft_model::text::Spacing::Pct(0.9)));
    assert_eq!(first_text.paragraphs[0].props.space_after, Some(deckcraft_model::text::Spacing::Pct(0.35)));
    let texts: Vec<_> = g.children().iter().filter_map(|s| s.text.as_ref()).map(|t| t.text()).collect();
    assert!(texts.iter().any(|s| s.contains("Observe") && s.contains("Measure")), "{texts:?}");
    let output = deckcraft_pptx::export(&p).unwrap();
    assert_eq!(ids(&by_root(&output, "dataModel")), original_ids);
    let back = deckcraft_pptx::import(&output).unwrap();
    assert_eq!(geometry(g), geometry(group(&back)));
    let image = deckcraft_engine::render::render_slide(&p, 0, &Default::default());
    let again = deckcraft_engine::render::render_slide(&back, 0, &Default::default());
    assert_eq!(image.pixels, again.pixels);
    if let Some(dir) = std::env::var_os("DECKCRAFT_RENDER_PROBES") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("uncached-composite.pptx"), input).unwrap();
        std::fs::write(dir.join("uncached-composite-roundtrip.pptx"), output).unwrap();
        std::fs::write(dir.join("uncached-composite.png"), image.to_png()).unwrap();
    }
}
#[test]
fn uncached_shape_adjustment_keeps_unspecified_preset_defaults() {
    let mut parts = files(&uncached_fixture());
    for (name, bytes) in &mut parts {
        if name.ends_with("layout1.xml") && name.contains("diagrams/") {
            let xml = String::from_utf8(bytes.clone()).unwrap();
            *bytes = xml
                .replace(
                    "<dgm:shape type=\"roundRect\"/>",
                    "<dgm:shape type=\"leftBrace\"><dgm:adjLst><dgm:adj idx=\"2\" val=\"0.3\"/></dgm:adjLst></dgm:shape>",
                )
                .into_bytes();
        }
    }
    let p = deckcraft_pptx::import(&zip(parts)).unwrap();
    let deckcraft_model::Geom::Preset { name, adj } = &group(&p).children()[0].geom else { panic!("preset") };
    assert_eq!(name, "leftBrace");
    assert_eq!(adj, &[8333.0, 30000.0]);
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
fn missing_diagram_cache_uses_the_authored_alternate_vector_branch() {
    let bytes = fixture();
    let mut parts = files(&bytes);
    let fallback = r#"<p:sp><p:nvSpPr><p:cNvPr id="9001" name="Author fallback"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="1270000" y="1270000"/><a:ext cx="3810000" cy="1270000"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:solidFill><a:srgbClr val="2856A0"/></a:solidFill></p:spPr><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr sz="2400"/><a:t>Visible authored fallback</a:t></a:r></a:p></p:txBody></p:sp>"#;
    let mut missing_drawing = None;
    for (name, bytes) in &mut parts {
        let Ok(mut doc) = parse_xml(bytes) else { continue };
        if doc.root.is("drawing") {
            missing_drawing = Some(name.clone());
        }
        if !doc.root.is("sld") {
            continue;
        }
        let mut frames = vec![];
        doc.root.find_all("graphicFrame", &mut frames);
        let frame = frames.into_iter().find(|e| e.find("relIds").is_some()).unwrap().to_xml();
        doc.root.set_attr("xmlns:mc", "http://schemas.openxmlformats.org/markup-compatibility/2006");
        doc.root.set_attr("xmlns:a14", "http://schemas.microsoft.com/office/drawing/2010/main");
        let ac = format!(
            "<mc:AlternateContent><mc:Choice Requires=\"a14\">{frame}</mc:Choice><mc:Fallback>{fallback}</mc:Fallback></mc:AlternateContent>"
        );
        *bytes = doc.root.to_xml().replace(&frame, &ac).into_bytes();
    }
    let cached = deckcraft_pptx::import(&zip(parts.clone())).unwrap();
    assert!(cached.slides[0].shapes.iter().any(|s| s.diagram.is_some()));
    parts.retain(|(n, _)| Some(n) != missing_drawing.as_ref());
    let missing = zip(parts);
    let back = deckcraft_pptx::import(&missing).unwrap();
    assert!(back.slides[0].shapes.iter().any(|s| s.text.as_ref().is_some_and(|t| t.text().contains("Visible authored fallback"))));
    assert!(!back.slides[0].shapes.iter().any(|s| s.diagram.is_some()));
}

#[test]
fn cached_diagram_root_child_coordinates_are_not_assumed_to_equal_the_frame() {
    let mut parts = files(&fixture());
    for (_, bytes) in &mut parts {
        let Ok(mut doc) = parse_xml(bytes) else { continue };
        if !doc.root.is("drawing") {
            continue;
        }
        let tree = doc.root.elements_mut().find(|e| e.is("spTree")).unwrap();
        let props = tree.elements_mut().find(|e| e.is("grpSpPr")).unwrap();
        *props = parse_xml(br#"<dsp:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="7620000" cy="3810000"/><a:chOff x="1270000" y="2540000"/><a:chExt cx="15240000" cy="7620000"/></a:xfrm></dsp:grpSpPr>"#).unwrap().root;
        *bytes = doc.root.to_xml().into_bytes();
    }
    let p = deckcraft_pptx::import(&zip(parts)).unwrap();
    let ShapeKind::Group { child, .. } = &group(&p).kind else { panic!("group") };
    assert_eq!(*child, deckcraft_geom::Xfrm::new(100.0, 200.0, 1200.0, 600.0));
    let back = deckcraft_pptx::import(&deckcraft_pptx::export(&p).unwrap()).unwrap();
    assert_eq!(group(&p).kind, group(&back).kind);
}

#[test]
fn generated_complex_diagram_matrix_renders_cached_shapes() {
    let mut out = deckcraft_model::Presentation::default();
    out.slides.clear();
    for kind in ["hierarchy", "cycle", "radial", "matrix", "pyramid", "pictureProcess"] {
        let mut s = Session::with_new();
        s.execute("insert.smartArt", &serde_json::json!({"kind":kind})).unwrap();
        let lines = if kind == "hierarchy" {
            vec!["Root", "\tBranch A", "\t\tLeaf 1", "\t\tLeaf 2", "\tBranch B", "\t\tLeaf 3"]
        } else {
            vec!["Observe", "Plan", "Build", "Measure", "Review", "Adapt"]
        };
        s.execute("smartArt.setNodes", &serde_json::json!({"lines":lines})).unwrap();
        let p = &s.doc().unwrap().doc;
        let layout = deckcraft_model::defaults::layout_of_kind(&out, deckcraft_model::LayoutType::Blank).unwrap();
        let mut slide = deckcraft_model::defaults::new_slide(&mut out, layout).unwrap();
        // Fresh slide identities; diagrams are the only contributor-original content.
        slide.shapes = p.slides[0].shapes.iter().filter(|s| s.diagram.is_some()).cloned().collect();
        out.slides.push(Arc::new(slide));
    }
    let bytes = deckcraft_pptx::export(&out).unwrap();
    let imported = deckcraft_pptx::import(&bytes).unwrap();
    let dir = std::env::var_os("DECKCRAFT_RENDER_PROBES").map(std::path::PathBuf::from);
    if let Some(dir) = &dir {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("complex-diagrams.pptx"), &bytes).unwrap();
    }
    for (i, slide) in imported.slides.iter().enumerate() {
        assert!(slide.shapes.iter().any(|s| s.diagram.as_ref().is_some_and(|d| d.kept.is_some())));
        let img = deckcraft_engine::render::render_slide(&imported, i, &Default::default());
        assert!(img.pixels.chunks_exact(4).filter(|p| p[0] < 150).count() > 1000);
        if let Some(dir) = &dir {
            std::fs::write(dir.join(format!("diagram-{}.png", i + 1)), img.to_png()).unwrap();
        }
    }
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
