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

#[test]
fn uncached_affine_cycle_renders_all_shapes_without_an_external_converter() {
    let mut parts = files(&uncached_fixture());
    for (name, bytes) in &mut parts {
        if name.ends_with("layout1.xml") && name.contains("diagrams/") {
            let xml = String::from_utf8(bytes.clone()).unwrap();
            *bytes = xml
                .replace(
                    "type=\"w\" for=\"ch\" forName=\"header\" refType=\"w\" fact=\"0.9\"",
                    "type=\"w\" for=\"ch\" forName=\"header\" refFor=\"ch\" refForName=\"details\" refType=\"w\" fact=\"1\"",
                )
                .replace(
                    "type=\"w\" for=\"ch\" forName=\"details\" refType=\"w\" fact=\"0.9\"",
                    "type=\"w\" for=\"ch\" forName=\"details\" refFor=\"ch\" refForName=\"header\" refType=\"w\" fact=\"1\"",
                )
                .replace("forName=\"details\" refFor=\"ch\" refForName=\"header\" refType=\"w\" fact=\"1\"/>",
                    "forName=\"details\" refFor=\"ch\" refForName=\"header\" refType=\"w\" fact=\"1\"/><dgm:constr type=\"w\" for=\"ch\" forName=\"details\" op=\"gte\" val=\"30\"/><dgm:constr type=\"w\" for=\"ch\" forName=\"details\" op=\"lte\" val=\"30\"/>")
                .into_bytes();
        }
    }
    let bytes = zip(parts);
    let p = deckcraft_pptx::import(&bytes).unwrap();
    let shapes = group(&p).children();
    assert_eq!(shapes.len(), 6);
    for shape in shapes {
        assert!((shape.xfrm.unwrap().w - 30. * 72. / 25.4).abs() < 1e-5);
    }
    let back = deckcraft_pptx::import(&deckcraft_pptx::export(&p).unwrap()).unwrap();
    assert_eq!(geometry(group(&p)), geometry(group(&back)));
    if let Some(dir) = std::env::var_os("DECKCRAFT_RENDER_PROBES") {
        std::fs::write(std::path::PathBuf::from(dir).join("uncached-affine-cycle.pptx"), bytes).unwrap();
    }
}

#[test]
fn uncached_forward_reference_sizes_follow_the_named_sibling() {
    let mut parts = files(&uncached_fixture());
    for (name, bytes) in &mut parts {
        if name.ends_with("layout1.xml") && name.contains("diagrams/") {
            *bytes = String::from_utf8(bytes.clone())
                .unwrap()
                .replace(
                    "type=\"w\" for=\"ch\" forName=\"header\" refType=\"w\" fact=\"0.9\"",
                    "type=\"w\" for=\"ch\" forName=\"header\" refFor=\"ch\" refForName=\"details\" refType=\"w\" fact=\"0.8\"",
                )
                .into_bytes();
        }
    }
    let bytes = zip(parts);
    let p = deckcraft_pptx::import(&bytes).unwrap();
    let shapes = group(&p).children();
    assert_eq!(shapes.len(), 6);
    for pair in shapes.chunks_exact(2) {
        assert!((pair[0].xfrm.unwrap().w - 0.8 * pair[1].xfrm.unwrap().w).abs() < 1e-5);
    }
    if let Some(dir) = std::env::var_os("DECKCRAFT_RENDER_PROBES") {
        std::fs::write(std::path::PathBuf::from(dir).join("forward-reference.pptx"), bytes).unwrap();
    }
}

fn numeric_rule_fixture(rules: &str, height: &str, long: bool) -> Vec<u8> {
    let mut parts = files(&uncached_fixture());
    let layout = format!(
        r#"<dgm:layoutDef xmlns:dgm="http://schemas.openxmlformats.org/drawingml/2006/diagram" uniqueId="urn:deckcraft:test:uncached-original" minVer="12.0">
    <dgm:title val="Original numeric rule probe"/><dgm:desc val="Text-driven constraint changes"/><dgm:catLst><dgm:cat type="process" pri="99"/></dgm:catLst>
    <dgm:layoutNode name="canvas"><dgm:alg type="lin"/><dgm:presOf/><dgm:shape type="none"/><dgm:constrLst/><dgm:ruleLst/>
    <dgm:forEach axis="ch" ptType="node"><dgm:layoutNode name="card" styleLbl="node1"><dgm:alg type="tx"/><dgm:presOf axis="self"/><dgm:shape type="roundRect"/>
    <dgm:constrLst><dgm:constr type="w" val="50"/>{height}<dgm:constr type="primFontSz" val="36"/></dgm:constrLst>
    <dgm:ruleLst>{rules}</dgm:ruleLst></dgm:layoutNode></dgm:forEach></dgm:layoutNode></dgm:layoutDef>"#
    );
    for (_, bytes) in &mut parts {
        let Ok(doc) = parse_xml(bytes) else { continue };
        if doc.root.is("layoutDef") {
            *bytes = layout.as_bytes().to_vec();
        } else if doc.root.is("dataModel") {
            let text = if long { "Observe changes in the landscape and record every result" } else { "OK" };
            *bytes = String::from_utf8(bytes.clone()).unwrap().replace("Research", text).replace("Build", "Build a careful prototype").into_bytes();
        }
    }
    zip(parts)
}

fn shared_font_fixture(op: &str, parent: Option<bool>, first: &str, second: &str) -> Vec<u8> {
    let mut parts = files(&uncached_fixture());
    let mut constraints = String::new();
    let mut nodes = String::new();
    for (i, x) in [10, 120].into_iter().enumerate() {
        constraints.push_str(&format!(
            r#"<dgm:constr type="w" for="ch" forName="card{i}" val="40"/><dgm:constr type="h" for="ch" forName="card{i}" val="40"/><dgm:constr type="l" for="ch" forName="card{i}" val="{x}"/><dgm:constr type="t" for="ch" forName="card{i}" val="20"/>"#
        ));
        let rule = if i == 0 && parent == Some(false) { r#"<dgm:rule type="primFontSz" val="5"/>"# } else { "" };
        nodes.push_str(&format!(
            r#"<dgm:layoutNode name="card{i}" styleLbl="node1"><dgm:alg type="tx"/><dgm:presOf axis="ch" ptType="node" st="{}" cnt="1"/><dgm:shape type="rect"/><dgm:constrLst/><dgm:ruleLst>{rule}</dgm:ruleLst></dgm:layoutNode>"#,
            i + 1
        ));
    }
    let rule = if parent == Some(true) { r#"<dgm:rule type="primFontSz" for="ch" val="5"/>"# } else { "" };
    let layout = format!(
        r#"<dgm:layoutDef xmlns:dgm="http://schemas.openxmlformats.org/drawingml/2006/diagram" uniqueId="urn:deckcraft:test:uncached-original" minVer="12.0"><dgm:title val="Original shared font probe"/><dgm:desc val="Separate local triggers from synchronized font changes"/><dgm:catLst><dgm:cat type="process" pri="99"/></dgm:catLst><dgm:layoutNode name="canvas"><dgm:alg type="composite"/><dgm:presOf/><dgm:shape type="none"/><dgm:constrLst>{constraints}<dgm:constr type="primFontSz" for="ch" op="{op}" val="36"/></dgm:constrLst><dgm:ruleLst>{rule}</dgm:ruleLst>{nodes}</dgm:layoutNode></dgm:layoutDef>"#
    );
    for (_, bytes) in &mut parts {
        let Ok(doc) = parse_xml(bytes) else { continue };
        if doc.root.is("layoutDef") {
            *bytes = layout.as_bytes().to_vec();
        } else if doc.root.is("dataModel") {
            *bytes = String::from_utf8(bytes.clone()).unwrap().replace("Research", first).replace("Build", second).into_bytes();
        }
    }
    zip(parts)
}

#[test]
fn shared_font_rules_retain_their_trigger_scope_and_roundtrip_real_text() {
    let long = "Observe changes in the landscape and record every result";
    for (index, (op, parent, first, second, expected)) in [
        ("none", Some(false), long, "OK", [14., 36.]),
        ("equ", Some(false), long, "OK", [14., 14.]),
        ("gte", Some(false), long, "OK", [14., 14.]),
        ("none", Some(true), long, "OK", [14., 14.]),
        ("equ", Some(false), "OK", long, [36., 36.]),
        ("gte", Some(false), "OK", long, [36., 36.]),
        ("equ", Some(false), "Observe changes", long, [21., 21.]),
        ("none", Some(false), "Observe changes", long, [21., 36.]),
        ("none", Some(true), "Observe changes", long, [14., 14.]),
        ("equ", None, long, "OK", [36., 36.]),
    ]
    .into_iter()
    .enumerate()
    {
        let bytes = shared_font_fixture(op, parent, first, second);
        let p = deckcraft_pptx::import(&bytes).unwrap();
        let fonts = |p: &deckcraft_model::Presentation| -> Vec<f64> {
            group(p).children().iter().map(|s| s.text.as_ref().unwrap().paragraphs[0].runs[0].props.size.unwrap()).collect()
        };
        assert_eq!(fonts(&p), expected, "case {index}");
        for s in group(&p).children() {
            assert_eq!(s.text.as_ref().unwrap().body.autofit, Some(deckcraft_model::text::AutoFit::None));
        }
        let back = deckcraft_pptx::import(&deckcraft_pptx::export(&p).unwrap()).unwrap();
        assert_eq!(fonts(&back), expected, "roundtrip {index}");
        assert_eq!(geometry(group(&p)), geometry(group(&back)));
        if let Some(dir) = std::env::var_os("DECKCRAFT_RENDER_PROBES") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(format!("shared-font-{index}.pptx")), bytes).unwrap();
        }
    }
}

#[test]
fn local_reference_fonts_and_parent_order_match_real_text_and_survive_roundtrip() {
    let long = "Observe changes in the landscape and record every result";
    for (index, (op, factor, target, parent, first, second, expected)) in [
        ("none", 1., 0, false, "Observe changes", long, [21., 36.]),
        ("none", 0.5, 0, false, "Observe changes", long, [21., 18.]),
        ("none", 0.5, 1, false, "Observe changes", long, [36., 14.]),
        ("equ", 1., 0, false, "Observe changes", long, [21., 21.]),
        ("equ", 0.5, 0, false, "Observe changes", long, [21., 11.]),
        ("equ", 0.5, 1, false, "Observe changes", long, [28., 14.]),
        ("lte", 0.5, 0, false, "Observe changes", long, [21., 11.]),
        ("gte", 0.5, 0, false, "Observe changes", long, [21., 36.]),
        ("none", 1., 0, true, long, "OK", [14., 36.]),
        ("equ", 1., 0, true, long, "OK", [14., 14.]),
    ]
    .into_iter()
    .enumerate()
    {
        let mut parts = files(&shared_font_fixture("none", None, first, second));
        for (_, bytes) in &mut parts {
            let Ok(mut doc) = parse_xml(bytes) else { continue };
            if !doc.root.is("layoutDef") {
                continue;
            }
            let root = doc.root.elements_mut().find(|e| e.is("layoutNode")).unwrap();
            let list = root.elements_mut().find(|e| e.is("constrLst")).unwrap();
            list.elements_mut().find(|e| e.attr("type") == Some("primFontSz")).unwrap().set_attr("forName", "card0");
            list.children.extend(parse_xml(format!(r#"<root><dgm:constr type="primFontSz" for="ch" forName="card1" refFor="ch" refForName="card0" refType="primFontSz" op="{op}" fact="{factor}"/></root>"#).as_bytes()).unwrap().root.children);
            if parent {
                *root.elements_mut().find(|e| e.is("ruleLst")).unwrap() =
                    parse_xml(br#"<dgm:ruleLst><dgm:rule type="primFontSz" for="ch" forName="card0" val="20"/></dgm:ruleLst>"#).unwrap().root;
            }
            let target = format!("card{target}");
            let node = root.elements_mut().find(|e| e.attr("name") == Some(&target)).unwrap();
            *node.elements_mut().find(|e| e.is("ruleLst")).unwrap() =
                parse_xml(br#"<dgm:ruleLst><dgm:rule type="primFontSz" val="5"/></dgm:ruleLst>"#).unwrap().root;
            *bytes = doc.root.to_xml().into_bytes();
        }
        let bytes = zip(parts);
        let p = deckcraft_pptx::import(&bytes).unwrap();
        let fonts = |p: &deckcraft_model::Presentation| -> Vec<f64> {
            group(p).children().iter().map(|s| s.text.as_ref().unwrap().paragraphs[0].runs[0].props.size.unwrap()).collect()
        };
        assert_eq!(fonts(&p), expected, "case {index}");
        let back = deckcraft_pptx::import(&deckcraft_pptx::export(&p).unwrap()).unwrap();
        assert_eq!(fonts(&back), expected);
        assert_eq!(geometry(group(&p)), geometry(group(&back)));
        if let Some(dir) = std::env::var_os("DECKCRAFT_RENDER_PROBES") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(format!("local-reference-{index}.pptx")), bytes).unwrap();
        }
    }
}

#[test]
fn composite_shape_and_text_ownership_survive_import_and_export() {
    for (index, (mapped, fit)) in [(1, false), (2, false), (1, true), (2, true)].into_iter().enumerate() {
        let mut parts = files(&shared_font_fixture("none", None, "Observe changes", "Build a careful prototype"));
        for (_, bytes) in &mut parts {
            let Ok(mut doc) = parse_xml(bytes) else { continue };
            if !doc.root.is("layoutDef") {
                continue;
            }
            let root = doc.root.elements_mut().find(|e| e.is("layoutNode")).unwrap();
            *root.elements_mut().find(|e| e.is("presOf")).unwrap() =
                parse_xml(format!("<dgm:presOf axis='ch' ptType='node' st='{mapped}' cnt='1'/>").as_bytes()).unwrap().root;
            root.elements_mut().find(|e| e.is("shape")).unwrap().set_attr("type", "rect");
            if fit {
                for node in root.elements_mut().filter(|e| e.is("layoutNode")) {
                    *node.elements_mut().find(|e| e.is("ruleLst")).unwrap() =
                        parse_xml(b"<dgm:ruleLst><dgm:rule type='primFontSz' val='5'/></dgm:ruleLst>").unwrap().root;
                }
            }
            *bytes = doc.root.to_xml().into_bytes();
        }
        let bytes = zip(parts);
        let p = deckcraft_pptx::import(&bytes).unwrap();
        let children = group(&p).children();
        assert_eq!(children.len(), 3);
        let rect = children[0].xfrm.unwrap();
        let a = children[1].xfrm.unwrap();
        let b = children[2].xfrm.unwrap();
        assert!((rect.x - a.x).abs() < 1e-6);
        assert!((rect.x + rect.w - b.x - b.w).abs() < 1e-6);
        let body = children[0].text.as_ref().unwrap();
        let target = children[mapped].text.as_ref().unwrap();
        assert_eq!(body.paragraphs[0].runs[0].props.size, target.paragraphs[0].runs[0].props.size);
        assert_eq!(body.paragraphs[0].runs[0].text, target.paragraphs[0].runs[0].text);
        let inset = if mapped == 1 { body.body.inset_r } else { body.body.inset_l };
        assert!(inset.unwrap() > 200., "mapped text must use the target box, not the large outer shape");
        let back = deckcraft_pptx::import(&deckcraft_pptx::export(&p).unwrap()).unwrap();
        assert_eq!(geometry(group(&p)), geometry(group(&back)));
        let back_body = group(&back).children()[0].text.as_ref().unwrap();
        assert_eq!(body.paragraphs[0].runs[0].props.size, back_body.paragraphs[0].runs[0].props.size);
        assert!((body.body.inset_l.unwrap() - back_body.body.inset_l.unwrap()).abs() < 0.001);
        assert!((body.body.inset_r.unwrap() - back_body.body.inset_r.unwrap()).abs() < 0.001);
        if let Some(dir) = std::env::var_os("DECKCRAFT_RENDER_PROBES") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(format!("composite-mapped-text-{index}.pptx")), bytes).unwrap();
        }
    }
}

#[test]
fn ordered_size_rules_ignore_local_edges_and_fit_real_text_to_the_container() {
    let rules = r#"<dgm:rule type="w" val="80"/><dgm:rule type="h" val="160"/>"#;
    let constraints = r#"<dgm:constr type="h" val="40"/><dgm:constr type="l" refType="w"/><dgm:constr type="r" refType="h"/>"#;
    let mut parts = files(&numeric_rule_fixture(rules, constraints, true));
    for (_, bytes) in &mut parts {
        if parse_xml(bytes).ok().is_some_and(|d| d.root.is("layoutDef")) {
            *bytes = String::from_utf8(bytes.clone())
                .unwrap()
                .replace(r#"type="w" val="50""#, r#"type="w" val="20""#)
                .replace(r#"<dgm:forEach axis="ch" ptType="node">"#, r#"<dgm:forEach axis="ch" ptType="node" cnt="1">"#)
                .into_bytes();
        }
    }
    let bytes = zip(parts);
    let p = deckcraft_pptx::import(&bytes).unwrap();
    let ShapeKind::Group { children, .. } = &group(&p).kind else { panic!("diagram group") };
    assert_eq!(children.len(), 1);
    let rect = children[0].xfrm.unwrap();
    // A text node's own edges do not impose an affine size relationship. Width grows
    // to its rule limit, then height is independently fitted to the enclosing frame.
    assert!((rect.h - 324.).abs() < 0.01, "{rect:?}");
    assert!((rect.w - 80. * 72. / 25.4).abs() < 0.01, "{rect:?}");
    assert_eq!(children[0].text.as_ref().unwrap().paragraphs[0].runs[0].props.size, Some(36.));
    if let Some(dir) = std::env::var_os("DECKCRAFT_RENDER_PROBES") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("rules-coupled-real-text.pptx"), bytes).unwrap();
    }
}

#[test]
fn numeric_rules_measure_real_fonts_after_import_and_preserve_order_on_roundtrip() {
    let grow = r#"<dgm:rule type="h" val="80"/>"#;
    let shrink = r#"<dgm:rule type="primFontSz" val="12"/>"#;
    let height = r#"<dgm:constr type="h" val="25"/>"#;
    let a_bytes = numeric_rule_fixture(&format!("{grow}{shrink}"), height, true);
    let b_bytes = numeric_rule_fixture(&format!("{shrink}{grow}"), height, true);
    let a = deckcraft_pptx::import(&a_bytes).unwrap();
    let b = deckcraft_pptx::import(&b_bytes).unwrap();
    let first = |p: &deckcraft_model::Presentation| {
        let s = &group(p).children()[0];
        (s.xfrm.unwrap(), s.text.as_ref().unwrap().paragraphs[0].runs[0].props.size.unwrap())
    };
    let (ra, fa) = first(&a);
    let (rb, fb) = first(&b);
    assert!(ra.h > rb.h + 50.0 && fa > fb && fb >= 12.0, "grow first {ra:?}/{fa}, shrink first {rb:?}/{fb}");
    let short = deckcraft_pptx::import(&numeric_rule_fixture(&format!("{grow}{shrink}"), height, false)).unwrap();
    let (rs, fs) = first(&short);
    assert!(rs.h < ra.h && fs >= fa);
    let factor = deckcraft_pptx::import(&numeric_rule_fixture(
        &format!(r#"<dgm:rule type="h" fact="1.6"/>{shrink}"#),
        r#"<dgm:constr type="h" refType="w" fact="0.5"/>"#,
        true,
    ))
    .unwrap();
    assert!((first(&factor).0.h - ra.h).abs() < 0.05);
    let back = deckcraft_pptx::import(&deckcraft_pptx::export(&a).unwrap()).unwrap();
    assert_eq!(geometry(group(&a)), geometry(group(&back)));
    assert_eq!(first(&a).1, first(&back).1);
    if let Some(dir) = std::env::var_os("DECKCRAFT_RENDER_PROBES") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("rules-height-first.pptx"), a_bytes).unwrap();
        std::fs::write(dir.join("rules-font-first.pptx"), b_bytes).unwrap();
    }
}

#[test]
fn diagram_bullet_start_uses_data_hierarchy_and_secondary_font_not_paragraph_index() {
    for (start, enabled) in [(1, true), (2, true), (2, false)] {
        let mut parts = files(&numeric_rule_fixture("", r#"<dgm:constr type="h" val="80"/>"#, false));
        for (name, bytes) in &mut parts {
            if name.ends_with("layout1.xml") && name.contains("diagrams/") {
                *bytes = String::from_utf8(bytes.clone()).unwrap()
                    .replace("<dgm:alg type=\"tx\"/>", &format!("<dgm:varLst><dgm:bulletEnabled val=\"{}\"/></dgm:varLst><dgm:alg type=\"tx\"><dgm:param type=\"stBulletLvl\" val=\"{start}\"/></dgm:alg>", u8::from(enabled)))
                    .replace("<dgm:presOf axis=\"self\"/>", "<dgm:presOf axis=\"desOrSelf\" ptType=\"node\"/>")
                    .replace("type=\"primFontSz\" val=\"36\"", "type=\"primFontSz\" val=\"24\"/><dgm:constr type=\"secFontSz\" val=\"14\"")
                    .into_bytes();
            }
        }
        let bytes = zip(parts);
        let p = deckcraft_pptx::import(&bytes).unwrap();
        for s in group(&p).children() {
            let body = s.text.as_ref().unwrap();
            assert_eq!(body.paragraphs.len(), 3);
            assert_eq!(body.body.anchor, Some(deckcraft_model::text::Anchor::Top));
            for (i, p) in body.paragraphs.iter().enumerate() {
                assert_eq!(p.props.align, Some(deckcraft_model::text::Align::Left));
                let bullet = start == 1 || i > 0;
                if bullet {
                    let depth = usize::from(i > 0);
                    assert_eq!(p.props.margin_left, Some((depth + 2 - start) as f64 * 9.));
                    assert_eq!(p.props.indent, Some(-9.));
                }
                assert_eq!(p.props.bullet.as_ref().is_some_and(|b| !matches!(b, deckcraft_model::text::Bullet::None)), bullet);
                assert!(p.runs.iter().all(|r| r.props.size == Some(if bullet { 14. } else { 24. })));
            }
        }
        let back = deckcraft_pptx::import(&deckcraft_pptx::export(&p).unwrap()).unwrap();
        assert_eq!(group(&back).children()[0].text, group(&p).children()[0].text);
    }
}
