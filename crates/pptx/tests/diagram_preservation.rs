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
fn invalid_font_graph_recovers_editable_cards_and_preserves_original_data() {
    let mut parts = files(&uncached_fixture());
    for (_, bytes) in &mut parts {
        let Ok(doc) = parse_xml(bytes) else { continue };
        if doc.root.is("layoutDef") {
            let xml = doc.root.to_xml();
            // A parent-owned font cannot supply a child's font declaration.
            // Keep the original data graph, which includes nested bullet labels.
            *bytes = xml.replacen(
                "<dgm:constrLst>",
                "<dgm:constrLst><dgm:constr type=\"primFontSz\" val=\"24\"/><dgm:constr type=\"primFontSz\" for=\"ch\" refType=\"primFontSz\" fact=\"0.5\"/>",
                1,
            ).into_bytes();
        }
    }
    let input = zip(parts);
    let p = deckcraft_pptx::import(&input).unwrap();
    let g = group(&p);
    assert_eq!(g.children().len(), 3);
    let mut all_text = String::new();
    for shape in g.children() {
        assert!(shape.name.starts_with("Recovered diagram node"));
        let text = shape.text.as_ref().unwrap();
        assert_eq!(text.paragraphs.len(), 3);
        assert!(text.paragraphs[1].props.bullet.is_some());
        let primary = text.paragraphs[0].runs[0].props.size.unwrap();
        let secondary = text.paragraphs[1].runs[0].props.size.unwrap();
        assert!((5.0..=65.0).contains(&primary));
        assert_eq!(secondary, (primary * 0.78).round());
        let indent = (primary / 8.).round().clamp(1., 5.) * 4.5;
        assert_eq!(text.paragraphs[1].props.margin_left, Some(indent));
        all_text.push_str(&text.text());
    }
    for label in ["Research", "Observe", "Measure", "Build", "Prototype", "Test", "Deliver", "Review", "Ship"] {
        assert!(all_text.contains(label), "{label}");
    }
    let output = deckcraft_pptx::export(&p).unwrap();
    assert_eq!(ids(&by_root(&input, "dataModel")), ids(&by_root(&output, "dataModel")));
    assert_eq!(by_root(&input, "layoutDef").to_xml(), by_root(&output, "layoutDef").to_xml());
    let back = deckcraft_pptx::import(&output).unwrap();
    assert_eq!(geometry(g), geometry(group(&back)));
    let before = deckcraft_engine::render::render_slide(&p, 0, &Default::default());
    let after = deckcraft_engine::render::render_slide(&back, 0, &Default::default());
    assert_eq!(before.pixels, after.pixels);
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
fn uncached_diagram_advance_policy_survives_native_save_without_drawingml_attributes() {
    let p = deckcraft_pptx::import(&uncached_fixture()).unwrap();
    let texts: Vec<_> = group(&p).children().iter().filter_map(|s| s.text.as_ref()).collect();
    assert!(!texts.is_empty());
    for text in texts {
        assert_eq!(text.body.diagram_fine_advances, Some(true));
        assert_eq!(text.body.diagram_recovery_metrics, None);
    }
    let cached = deckcraft_pptx::import(&fixture()).unwrap();
    for text in group(&cached).children().iter().filter_map(|s| s.text.as_ref()) {
        assert_eq!(text.body.diagram_fine_advances, None, "cached DrawingML retains its own text policy");
    }
    let bytes = deckcraft_engine::cmd::file::save_bytes(&p, "deckcraft").unwrap();
    let back = deckcraft_engine::cmd::file::open_presentation("native.deckcraft", &bytes).unwrap();
    assert_eq!(group(&p), group(&back));
    let before = deckcraft_engine::render::render_slide(&p, 0, &Default::default());
    let after = deckcraft_engine::render::render_slide(&back, 0, &Default::default());
    assert_eq!(before.pixels, after.pixels);
    for (_, bytes) in files(&deckcraft_pptx::export(&back).unwrap()) {
        if let Ok(xml) = std::str::from_utf8(&bytes) {
            assert!(!xml.contains("diagramFineAdvances"));
            assert!(!xml.contains("diagramRecoveryMetrics"));
        }
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
    // Exercise the general affine solver through an auxiliary property. Native
    // width/height inequalities do not allocate dimensions. This is a solver
    // regression, not a claim about native handling of the cyclic definition.
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
                    "forName=\"details\" refFor=\"ch\" refForName=\"header\" refType=\"w\" fact=\"1\"/><dgm:constr type=\"userA\" for=\"ch\" forName=\"details\" refFor=\"ch\" refForName=\"details\" refType=\"w\"/><dgm:constr type=\"userA\" for=\"ch\" forName=\"details\" op=\"gte\" val=\"85.03937007874016\"/><dgm:constr type=\"userA\" for=\"ch\" forName=\"details\" op=\"lte\" val=\"85.03937007874016\"/>")
                .into_bytes();
        }
    }
    let bytes = zip(parts);
    let p = deckcraft_pptx::import(&bytes).unwrap();
    let shapes = group(&p).children();
    assert_eq!(shapes.len(), 6);
    // The affine solver's exact 30mm result is asserted before layout in
    // linear_root::tests::affine_cycle_solves_exact_widths_before_root_text_fitting.
    // These labels request root text fitting at that width. Independently observed
    // composite controls fit occupied geometry by 1023/1024 along the root flow.
    let ctx = deckcraft_model::resolve::Ctx::for_slide(&p, &p.slides[0]).unwrap();
    let requests_refit = shapes.iter().any(|shape| {
        let mut unscaled = shape.clone();
        let mut rect = unscaled.xfrm.unwrap();
        rect.w = 30. * 72. / 25.4;
        unscaled.xfrm = Some(rect);
        let body = unscaled.text.as_ref().unwrap();
        let bounds = if let deckcraft_model::Geom::Preset { name, adj } = &unscaled.geom {
            deckcraft_geom::preset::build(name, rect.w, rect.h, adj).unwrap().text_rect
        } else {
            deckcraft_geom::Rect::new(0., 0., rect.w, rect.h)
        };
        let measured = deckcraft_text::layout_with_metrics(
            &ctx,
            &unscaled,
            body,
            &deckcraft_text::Opts { rect: bounds, fields: &deckcraft_text::NoFields, prompt_color: None, no_shrink: true },
            deckcraft_text::LayoutMetrics { advance: deckcraft_text::AdvanceMetrics::DrawingMl, line: deckcraft_text::LineMetrics::Typographic },
        );
        measured.forced_line_breaks(body) > 0
    });
    assert!(requests_refit, "the original affine fixture must independently request a word-wrap refit");
    for shape in shapes {
        assert!((shape.xfrm.unwrap().w - 30. * 72. / 25.4 * 1023. / 1024.).abs() < 1e-5);
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
fn rotated_mapped_text_is_editable_after_fitting_and_roundtrip() {
    for (index, (parent, child, mode, invalid_rule)) in
        [(0, 90, "upr", false), (30, 90, "upr", false), (0, 90, "none", false), (30, 180, "grav", false), (30, 90, "upr", true)]
            .into_iter()
            .enumerate()
    {
        let mut parts = files(&shared_font_fixture("none", Some(false), "Observe changes", "OK"));
        for (_, bytes) in &mut parts {
            let Ok(mut doc) = parse_xml(bytes) else { continue };
            if !doc.root.is("layoutDef") {
                continue;
            }
            let root = doc.root.elements_mut().find(|e| e.is("layoutNode")).unwrap();
            *root.elements_mut().find(|e| e.is("presOf")).unwrap() = parse_xml(b"<dgm:presOf axis='ch' ptType='node' cnt='1'/>").unwrap().root;
            let shape = root.elements_mut().find(|e| e.is("shape")).unwrap();
            shape.set_attr("type", "rect");
            shape.set_attr("rot", &parent.to_string());
            let target = root.elements_mut().find(|e| e.attr("name") == Some("card0")).unwrap();
            target.elements_mut().find(|e| e.is("shape")).unwrap().set_attr("rot", &child.to_string());
            if invalid_rule {
                // Search failure must retain independently rotated/mapped text, not
                // return the provisional physical shape with text in the wrong box.
                let rules = target.elements_mut().find(|e| e.is("ruleLst")).unwrap();
                rules.elements_mut().find(|e| e.is("rule")).unwrap().set_attr("max", "1");
            }
            target
                .elements_mut()
                .find(|e| e.is("alg"))
                .unwrap()
                .children
                .extend(parse_xml(format!("<root><dgm:param type='autoTxRot' val='{mode}'/></root>").as_bytes()).unwrap().root.children);
            *bytes = doc.root.to_xml().into_bytes();
        }
        let bytes = zip(parts);
        let p = deckcraft_pptx::import(&bytes).unwrap();
        let labels: Vec<_> = group(&p).children().iter().filter_map(|s| s.text.as_ref().map(|b| (s, b))).collect();
        assert_eq!(labels.len(), 3);
        for (shape, body) in &labels {
            let first = &body.paragraphs[0].runs[0];
            assert_eq!(first.props.size, Some(if first.text == "OK" || invalid_rule { 36. } else { 21. }));
            if first.text != "OK" {
                assert_eq!(shape.xfrm.unwrap().rot.rem_euclid(360.), if mode == "none" { 90. } else { 0. });
            }
        }
        assert!(group(&p).children().iter().filter(|s| s.text.is_none()).count() >= 1);
        let back = deckcraft_pptx::import(&deckcraft_pptx::export(&p).unwrap()).unwrap();
        assert_eq!(geometry(group(&p)), geometry(group(&back)));
        let labels_back: Vec<_> = group(&back).children().iter().filter_map(|s| s.text.as_ref()).collect();
        assert_eq!(labels_back.len(), labels.len());
        for ((_, a), b) in labels.iter().zip(labels_back) {
            assert_eq!(a.paragraphs[0].runs[0].text, b.paragraphs[0].runs[0].text);
            assert_eq!(a.paragraphs[0].runs[0].props.size, b.paragraphs[0].runs[0].props.size);
        }
        if let Some(dir) = std::env::var_os("DECKCRAFT_RENDER_PROBES").filter(|_| !invalid_rule) {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(format!("rotated-text-{index}.pptx")), bytes).unwrap();
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
    // to its rule limit, then the finite linear fit reduces the width slightly;
    // height is independently fitted to the enclosing frame.
    assert!((rect.h - 324.).abs() < 0.01, "{rect:?}");
    assert!((rect.w - 79.921875 * 72. / 25.4).abs() < 0.01, "{rect:?}");
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

#[test]
fn diagram_hanging_defaults_follow_the_final_secondary_font_and_preserve_authored_sides() {
    // Same-font native controls: primary 11/12 both produce secondary 9 and a 4.5pt
    // hang. Explicit secondary 27/28 produce 18/22.5pt, independently of primary.
    for (primary, secondary, margin, indent, expected_font, expected_margin, expected_indent) in [
        (11., None, None, None, 9., 4.5, -4.5),
        (12., None, None, None, 9., 4.5, -4.5),
        (49., Some(9.), None, None, 9., 4.5, -4.5),
        (49., Some(11.), None, None, 11., 4.5, -4.5),
        (49., Some(12.), None, None, 12., 9., -9.),
        (49., Some(16.), None, None, 16., 13.5, -13.5),
        (49., Some(19.), None, None, 19., 13.5, -13.5),
        (49., Some(20.), None, None, 20., 18., -18.),
        (49., Some(21.), None, None, 21., 18., -18.),
        (24., Some(27.), None, None, 27., 18., -18.),
        (49., Some(28.), None, None, 28., 22.5, -22.5),
        (24., Some(38.), None, None, 38., 22.5, -22.5),
        (24., Some(27.), Some(9.), None, 27., 9., -18.),
        (24., Some(27.), None, Some(-9.), 27., 18., -9.),
        (24., Some(27.), None, Some(0.), 27., 18., 0.),
        (49., Some(20.), Some(0.), None, 20., 0., -18.),
        (49., Some(20.), None, Some(0.), 20., 18., 0.),
    ] {
        let mut parts = files(&numeric_rule_fixture("", r#"<dgm:constr type="h" val="80"/>"#, false));
        for (_, bytes) in &mut parts {
            let Ok(mut doc) = parse_xml(bytes) else { continue };
            if doc.root.is("layoutDef") {
                *bytes = doc
                    .root
                    .to_xml()
                    .replace("<dgm:alg type=\"tx\"/>", "<dgm:alg type=\"tx\"><dgm:param type=\"stBulletLvl\" val=\"1\"/></dgm:alg>")
                    .replace(
                        "<dgm:constr type=\"primFontSz\" val=\"36\"/>",
                        &format!(
                            "<dgm:constr type=\"primFontSz\" val=\"{primary}\"/>{}",
                            secondary.map(|s| format!("<dgm:constr type=\"secFontSz\" val=\"{s}\"/>")).unwrap_or_default()
                        ),
                    )
                    .into_bytes();
            } else if doc.root.is("dataModel") {
                for point in doc.root.elements_mut().find(|e| e.is("ptLst")).unwrap().elements_mut() {
                    if let Some(t) = point.elements_mut().find(|e| e.is("t")) {
                        for p in t.elements_mut().filter(|e| e.is("p")) {
                            let mut prefix = parse_xml(br#"<a:p><a:pPr/></a:p>"#).unwrap().root;
                            let props = prefix.elements_mut().next().unwrap();
                            if let Some(v) = margin {
                                props.set_attr("marL", &format!("{}", v * 12700.));
                            }
                            if let Some(v) = indent {
                                props.set_attr("indent", &format!("{}", v * 12700.));
                            }
                            p.children.splice(0..0, prefix.children);
                        }
                    }
                }
                *bytes = doc.root.to_xml().into_bytes();
            }
        }
        let p = deckcraft_pptx::import(&zip(parts)).unwrap();
        let paragraph = &group(&p).children()[0].text.as_ref().unwrap().paragraphs[0];
        assert_eq!(paragraph.props.margin_left, Some(expected_margin));
        assert_eq!(paragraph.props.indent, Some(expected_indent));
        assert!(paragraph.runs.iter().all(|r| r.props.size == Some(expected_font)));
    }
}

#[test]
fn customized_diagram_runs_keep_sizes_and_first_run_hanging_defaults_through_font_rules() {
    // Independent native controls separate custT from run formatting: without it,
    // the layout font wins; with it, each run retains its size even during a shrink
    // rule. The first run controls the bullet. Missing run sizes use 36pt, including
    // when pPr/defRPr supplies a different size.
    for (customized, sizes, default_size, secondary, fitting, authored_hang, expected_sizes, expected_margin) in [
        (false, vec![Some(14.)], None, 27., false, false, vec![27.], Some(18.)),
        (true, vec![Some(14.)], None, 27., false, false, vec![14.], Some(9.)),
        (true, vec![Some(27.), Some(11.)], None, 11., false, false, vec![27., 11.], Some(18.)),
        (true, vec![Some(11.), Some(27.)], None, 27., false, false, vec![11., 27.], Some(4.5)),
        // Independent native boundary pairs retain fractional custom advances and
        // lower hanging defaults, while automatic fonts resolve to the next point.
        (true, vec![Some(15.75)], None, 27., false, false, vec![15.75], Some(9.)),
        (false, vec![Some(15.75)], None, 15.75, false, false, vec![16.], Some(13.5)),
        (true, vec![Some(19.75)], None, 27., false, false, vec![19.75], Some(13.5)),
        (false, vec![Some(19.75)], None, 19.75, false, false, vec![20.], Some(18.)),
        (true, vec![Some(27.75)], None, 27., false, false, vec![27.75], Some(18.)),
        (false, vec![Some(27.75)], None, 27.75, false, false, vec![28.], Some(22.5)),
        (true, vec![None], None, 27., false, false, vec![36.], Some(22.5)),
        (true, vec![None], Some(27.), 11., false, false, vec![36.], Some(22.5)),
        (true, vec![Some(14.)], None, 27., false, true, vec![14.], Some(25.)),
        (true, vec![Some(14.)], None, 27., true, false, vec![14.], Some(9.)),
    ] {
        let rules = if fitting { r#"<dgm:rule type="secFontSz" val="8"/>"# } else { "" };
        let mut parts = files(&numeric_rule_fixture(rules, r#"<dgm:constr type="h" val="12"/>"#, false));
        for (_, bytes) in &mut parts {
            let Ok(mut doc) = parse_xml(bytes) else { continue };
            if doc.root.is("layoutDef") {
                *bytes = doc
                    .root
                    .to_xml()
                    .replace("<dgm:alg type=\"tx\"/>", "<dgm:alg type=\"tx\"><dgm:param type=\"stBulletLvl\" val=\"1\"/></dgm:alg>")
                    .replace(
                        "<dgm:constr type=\"primFontSz\" val=\"36\"/>",
                        &format!("<dgm:constr type=\"primFontSz\" val=\"49\"/><dgm:constr type=\"secFontSz\" val=\"{secondary}\"/>"),
                    )
                    .into_bytes();
            } else if doc.root.is("dataModel") {
                let props = if authored_hang { "marL=\"317500\" indent=\"-317500\"" } else { "indent=\"0\"" };
                let default = default_size.map(|v| format!("<a:defRPr sz=\"{}\"/>", v * 100.)).unwrap_or_default();
                let runs: String = sizes
                    .iter()
                    .enumerate()
                    .map(|(i, size)| {
                        let size = size.map(|v| format!(" sz=\"{}\"", v * 100.)).unwrap_or_default();
                        let text = if fitting {
                            "W".repeat(200)
                        } else if i == 0 {
                            "Quiet".into()
                        } else {
                            " Brook".into()
                        };
                        format!("<a:r><a:rPr{size}><a:latin typeface=\"Inter\"/></a:rPr><a:t>{text}</a:t></a:r>")
                    })
                    .collect();
                for point in doc.root.elements_mut().find(|e| e.is("ptLst")).unwrap().elements_mut() {
                    if let Some(pr) = point.elements_mut().find(|e| e.is("prSet")) {
                        pr.set_attr("custT", if customized { "1" } else { "0" });
                    }
                    if let Some(text) = point.elements_mut().find(|e| e.is("t")) {
                        for paragraph in text.elements_mut().filter(|e| e.is("p")) {
                            *paragraph = parse_xml(format!("<a:p><a:pPr {props}>{default}</a:pPr>{runs}</a:p>").as_bytes()).unwrap().root;
                        }
                    }
                }
                *bytes = doc.root.to_xml().into_bytes();
            }
        }
        let p = deckcraft_pptx::import(&zip(parts)).unwrap();
        let paragraph = &group(&p).children()[0].text.as_ref().unwrap().paragraphs[0];
        let actual: Vec<_> = paragraph.runs.iter().map(|r| r.props.size.unwrap()).collect();
        assert_eq!(actual, expected_sizes, "custT={customized}, fitting={fitting}");
        if let Some(expected) = expected_margin {
            assert_eq!(paragraph.props.margin_left, Some(expected));
        }
        assert_eq!(paragraph.props.indent, Some(if authored_hang { -25. } else { 0. }));
        let back = deckcraft_pptx::import(&deckcraft_pptx::export(&p).unwrap()).unwrap();
        assert_eq!(group(&back).children()[0].text, group(&p).children()[0].text);
    }
}

#[test]
fn diagram_font_rules_refresh_unauthored_hanging_defaults_without_rewriting_authored_sides() {
    // Native rule/direct endpoint pairs agree after shrinking primary35 to14 (secondary11),
    // including independently authored marL and indent. The final generated side is4.5pt;
    // a previous trial's larger margin must not persist.
    let fixture = |rule: bool, margin: Option<f64>, indent: Option<f64>| {
        let rules = if rule { r#"<dgm:rule type="primFontSz" val="14"/>"# } else { "" };
        let primary = if rule { 35 } else { 14 };
        let mut parts = files(&numeric_rule_fixture(rules, r#"<dgm:constr type="h" val="12"/>"#, false));
        for (_, bytes) in &mut parts {
            let Ok(mut doc) = parse_xml(bytes) else { continue };
            if doc.root.is("layoutDef") {
                *bytes = doc
                    .root
                    .to_xml()
                    .replace("<dgm:alg type=\"tx\"/>", "<dgm:alg type=\"tx\"><dgm:param type=\"stBulletLvl\" val=\"1\"/></dgm:alg>")
                    .replace("type=\"primFontSz\" val=\"36\"", &format!("type=\"primFontSz\" val=\"{primary}\""))
                    .into_bytes();
            } else if doc.root.is("dataModel") {
                for point in doc.root.elements_mut().find(|e| e.is("ptLst")).unwrap().elements_mut() {
                    if let Some(t) = point.elements_mut().find(|e| e.is("t")) {
                        for p in t.elements_mut().filter(|e| e.is("p")) {
                            let mut prefix = parse_xml(br#"<a:p><a:pPr/></a:p>"#).unwrap().root;
                            let props = prefix.elements_mut().next().unwrap();
                            if let Some(v) = margin {
                                props.set_attr("marL", &format!("{}", v * 12700.));
                            }
                            if let Some(v) = indent {
                                props.set_attr("indent", &format!("{}", v * 12700.));
                            }
                            p.children.splice(0..0, prefix.children);
                        }
                    }
                }
                *bytes = doc.root.to_xml().replace(">OK<", &format!(">Quiet {}<", "W".repeat(100))).into_bytes();
            }
        }
        deckcraft_pptx::import(&zip(parts)).unwrap()
    };
    for (margin, indent) in [(None, None), (Some(9.), None), (None, Some(0.)), (None, Some(-9.)), (Some(25.), Some(-25.))] {
        let fitted = fixture(true, margin, indent);
        let direct = fixture(false, margin, indent);
        let a = &group(&fitted).children()[0].text.as_ref().unwrap().paragraphs[0];
        let b = &group(&direct).children()[0].text.as_ref().unwrap().paragraphs[0];
        assert!(a.runs.iter().chain(&b.runs).all(|r| r.props.size == Some(11.)));
        assert_eq!(a.props.margin_left, b.props.margin_left, "font trial retained a previous margin");
        assert_eq!(a.props.indent, b.props.indent, "font trial retained a previous indent");
        assert_eq!(a.props.margin_left, Some(margin.unwrap_or(4.5)));
        assert_eq!(a.props.indent, Some(indent.unwrap_or(-4.5)));
    }
}

#[test]
fn nonpainting_diagram_nodes_do_not_advance_colour_lists_or_duplicate_text() {
    use deckcraft_color::SchemeSlot;
    use deckcraft_model::style::{ColorBase, Fill};
    for (mode, changed) in ["hidden", "none", "absent", "connector"].into_iter().flat_map(|m| [(m, 0), (m, 1)]) {
        let mut parts = files(&shared_font_fixture("none", None, "Alpha", "Beta"));
        for (_, bytes) in &mut parts {
            let Ok(mut doc) = parse_xml(bytes) else { continue };
            if doc.root.is("colorsDef") {
                let xml = doc.root.to_xml();
                *bytes = xml
                    .replace(
                        "<dgm:txFillClrLst/>",
                        r#"<dgm:txFillClrLst meth="repeat"><a:schemeClr val="accent4"/><a:schemeClr val="accent5"/></dgm:txFillClrLst>"#,
                    )
                    .into_bytes();
                continue;
            }
            if !doc.root.is("layoutDef") {
                continue;
            }
            let root = doc.root.elements_mut().find(|e| e.is("layoutNode")).unwrap();
            let name = format!("card{changed}");
            let card = root.elements_mut().find(|e| e.attr("name") == Some(name.as_str())).unwrap();
            let removed = if mode == "absent" { card.child("shape").map(|s| (card.to_xml(), s.to_xml())) } else { None };
            if mode != "absent" {
                let shape = card.elements_mut().find(|e| e.is("shape")).unwrap();
                match mode {
                    "hidden" => shape.set_attr("hideGeom", "1"),
                    "none" => shape.set_attr("type", "none"),
                    _ => shape.set_attr("type", "conn"),
                }
            }
            let xml = doc.root.to_xml();
            *bytes = removed.map_or_else(|| xml.clone(), |(card, shape)| xml.replacen(&card, &card.replacen(&shape, "", 1), 1)).into_bytes();
        }
        let bytes = zip(parts);
        let p = deckcraft_pptx::import(&bytes).unwrap();
        let shapes = group(&p).children();
        let shape = |i: usize| shapes.iter().find(|s| s.name == format!("card{i}")).unwrap();
        let expected = if mode == "connector" && changed == 0 { SchemeSlot::Accent2 } else { SchemeSlot::Accent1 };
        assert!(
            matches!(&shape(1 - changed).fill, Some(Fill::Solid { color }) if color.base == ColorBase::Scheme { slot: expected }),
            "{mode}/{changed}"
        );
        assert_eq!(shape(changed).hidden, matches!(mode, "none" | "absent"), "{mode}/{changed}");
        if mode == "hidden" {
            let run = &shape(changed).text.as_ref().unwrap().paragraphs[0].runs[0];
            assert!(matches!(&run.props.fill, Some(Fill::Solid { color }) if color.base == ColorBase::Scheme { slot: SchemeSlot::Accent4 }));
        }
        let back = deckcraft_pptx::import(&deckcraft_pptx::export(&p).unwrap()).unwrap();
        let image = deckcraft_engine::render::render_slide(&p, 0, &Default::default());
        let again = deckcraft_engine::render::render_slide(&back, 0, &Default::default());
        assert_eq!(image.pixels, again.pixels, "{mode} round trip");
    }
}

#[test]
fn allocation_only_nodes_affect_fit_without_becoming_exported_shapes() {
    for alg in ["sp", "tx", "conn"] {
        for shape in ["", "<dgm:shape type='none'/>"] {
            let mut parts = files(&shared_font_fixture("none", None, "Alpha", "Beta"));
            for (_, bytes) in &mut parts {
                let Ok(mut doc) = parse_xml(bytes) else { continue };
                if !doc.root.is("layoutDef") {
                    continue;
                }
                let root = doc.root.elements_mut().find(|e| e.is("layoutNode")).unwrap();
                let constraints = root.elements_mut().find(|e| e.is("constrLst")).unwrap();
                for (kind, value) in [("w", 400), ("h", 40), ("l", -40), ("t", 20)] {
                    constraints.children.extend(
                        parse_xml(format!("<root><dgm:constr type='{kind}' for='ch' forName='space' val='{value}'/></root>").as_bytes())
                            .unwrap()
                            .root
                            .children,
                    );
                }
                root.children.extend(
                    parse_xml(
                        format!("<root><dgm:layoutNode name='space'><dgm:alg type='{alg}'/><dgm:presOf/>{shape}</dgm:layoutNode></root>").as_bytes(),
                    )
                    .unwrap()
                    .root
                    .children,
                );
                *bytes = doc.root.to_xml().into_bytes();
            }
            let p = deckcraft_pptx::import(&zip(parts)).unwrap();
            let g = group(&p);
            assert_eq!(g.children().len(), 2, "{alg}/{shape}: no phantom drawing node");
            for s in g.children() {
                assert!((s.xfrm.unwrap().w - g.xfrm.unwrap().w * 0.1).abs() < 1e-6);
            }
            let back = deckcraft_pptx::import(&deckcraft_pptx::export(&p).unwrap()).unwrap();
            assert_eq!(group(&back).children().len(), 2);
            assert_eq!(geometry(g), geometry(group(&back)));
            let image = deckcraft_engine::render::render_slide(&p, 0, &Default::default());
            let again = deckcraft_engine::render::render_slide(&back, 0, &Default::default());
            assert_eq!(image.pixels, again.pixels, "{alg}/{shape} round trip");
        }
    }
}

#[test]
fn inverted_endpoints_keep_positive_editable_geometry_through_roundtrip() {
    let mut parts = files(&uncached_fixture());
    for (name, bytes) in &mut parts {
        if name.ends_with("layout1.xml") && name.contains("diagrams/") {
            *bytes = br#"<dgm:layoutDef xmlns:dgm="http://schemas.openxmlformats.org/drawingml/2006/diagram" uniqueId="urn:deckcraft:test:signed-edges" minVer="12.0">
            <dgm:title val="Original signed endpoint witness"/><dgm:desc val="Separate an endpoint reference from the displayed width"/>
            <dgm:layoutNode name="canvas"><dgm:alg type="composite"/><dgm:presOf/><dgm:shape type="none"/>
            <dgm:constrLst><dgm:constr type="w" val="20"/><dgm:constr type="h" val="20"/>
            <dgm:constr type="l" for="ch" forName="subject" val="25"/><dgm:constr type="h" for="ch" forName="subject" val="10"/>
            <dgm:constr type="l" for="ch" forName="witness" refFor="ch" refForName="subject" refType="r"/>
            <dgm:constr type="w" for="ch" forName="witness" val="4"/><dgm:constr type="h" for="ch" forName="witness" val="4"/>
            </dgm:constrLst><dgm:layoutNode name="subject" styleLbl="node1"><dgm:alg type="tx"/><dgm:presOf axis="ch" ptType="node" st="1" cnt="1"/><dgm:shape type="rtTriangle"/></dgm:layoutNode>
            <dgm:layoutNode name="witness" styleLbl="node1"><dgm:alg type="tx"/><dgm:presOf axis="ch" ptType="node" st="2" cnt="1"/><dgm:shape type="rect"/></dgm:layoutNode>
            </dgm:layoutNode></dgm:layoutDef>"#.to_vec();
        }
    }
    let input = zip(parts);
    let p = deckcraft_pptx::import(&input).unwrap();
    let g = group(&p);
    assert_eq!(g.children().len(), 2);
    let a = g.children()[0].xfrm.unwrap();
    let b = g.children()[1].xfrm.unwrap();
    let mm = 72. / 25.4;
    assert!((a.w - 5. * mm).abs() < 1e-6);
    assert!((a.x - b.x - 5. * mm).abs() < 1e-6);
    assert!(!a.flip_h && !a.flip_v);
    let output = deckcraft_pptx::export(&p).unwrap();
    assert_eq!(ids(&by_root(&input, "dataModel")), ids(&by_root(&output, "dataModel")));
    let back = deckcraft_pptx::import(&output).unwrap();
    assert_eq!(geometry(g), geometry(group(&back)));
    assert_eq!(
        deckcraft_engine::render::render_slide(&p, 0, &Default::default()).pixels,
        deckcraft_engine::render::render_slide(&back, 0, &Default::default()).pixels
    );
}

#[test]
fn measured_cycle_text_refitting_survives_editable_round_trip() {
    let layout = r#"<dgm:layoutDef xmlns:dgm="http://schemas.openxmlformats.org/drawingml/2006/diagram" uniqueId="urn:deckcraft:test:cycle-refit" minVer="12.0">
      <dgm:title val="Original cycle measurement"/><dgm:desc val="Mapped labels and geometry"/><dgm:catLst><dgm:cat type="cycle" pri="99"/></dgm:catLst>
      <dgm:layoutNode name="ring"><dgm:alg type="cycle"/><dgm:presOf/><dgm:shape type="none"/>
      <dgm:constrLst><dgm:constr type="w" for="ch" forName="card" val="25"/><dgm:constr type="h" for="ch" forName="card" val="25"/></dgm:constrLst>
      <dgm:forEach axis="ch" ptType="node"><dgm:layoutNode name="card"><dgm:alg type="tx"/><dgm:presOf axis="self"/><dgm:shape type="rect"/>
      <dgm:constrLst><dgm:constr type="primFontSz" val="8"/></dgm:constrLst></dgm:layoutNode></dgm:forEach></dgm:layoutNode></dgm:layoutDef>"#;
    let mut radii = Vec::new();
    for label in ["OK".to_string(), "Long unbreakableLongWord ".repeat(16)] {
        let mut parts = files(&uncached_fixture());
        for (_, bytes) in &mut parts {
            let Ok(doc) = parse_xml(bytes) else { continue };
            if doc.root.is("layoutDef") {
                *bytes = layout.as_bytes().to_vec();
            } else if doc.root.is("dataModel") {
                *bytes = String::from_utf8(bytes.clone())
                    .unwrap()
                    .replace("Research", &label)
                    .replace("Build", &label)
                    .replace("Deliver", &label)
                    .into_bytes();
            }
        }
        let p = deckcraft_pptx::import(&zip(parts)).unwrap();
        let shapes = group(&p).children();
        assert_eq!(shapes.len(), 3);
        let a = shapes[0].xfrm.unwrap();
        let b = shapes[1].xfrm.unwrap();
        radii.push((a.x - b.x).hypot(a.y - b.y) / 3_f64.sqrt());
        let saved = deckcraft_pptx::export(&p).unwrap();
        let reopened = deckcraft_pptx::import(&saved).unwrap();
        let restored = group(&reopened).children();
        assert_eq!(restored.len(), shapes.len());
        for (original, again) in shapes.iter().zip(restored) {
            assert_eq!(original.text.as_ref().unwrap().text(), again.text.as_ref().unwrap().text());
            let (a, b) = (original.xfrm.unwrap(), again.xfrm.unwrap());
            assert!((a.x - b.x).abs() < 0.01 && (a.y - b.y).abs() < 0.01 && (a.w - b.w).abs() < 0.01 && (a.h - b.h).abs() < 0.01);
        }
    }
    assert!((radii[1] / radii[0] - 1023. / 1024.).abs() < 1e-5);
}

#[test]
fn nested_helper_visibility_and_default_secondary_fonts_round_trip() {
    for (shape, visible_per_card) in
        [("", 0), ("<dgm:shape type='none'/>", 0), ("<dgm:shape type='rect' hideGeom='1'/>", 1), ("<dgm:shape type='rect'/>", 2)]
    {
        let layout = format!(
            r#"<dgm:layoutDef xmlns:dgm="http://schemas.openxmlformats.org/drawingml/2006/diagram" uniqueId="urn:deckcraft:test:helper-visibility" minVer="12.0">
            <dgm:title val="Original nested typography"/><dgm:desc val="Independent text nodes"/><dgm:catLst><dgm:cat type="cycle" pri="99"/></dgm:catLst>
            <dgm:layoutNode><dgm:alg type="cycle"/><dgm:presOf/><dgm:shape type="none"/>
            <dgm:constrLst><dgm:constr type="w" for="ch" val="60"/><dgm:constr type="h" for="ch" val="45"/></dgm:constrLst>
            <dgm:forEach axis="ch" ptType="node"><dgm:layoutNode name="outer"><dgm:alg type="composite"/><dgm:presOf axis="self"/><dgm:shape type="rect"/>
            <dgm:constrLst><dgm:constr type="w" for="ch" val="40"/><dgm:constr type="h" for="ch" val="30"/>
            <dgm:constr type="primFontSz" val="8"/><dgm:constr type="secFontSz" val="10"/><dgm:constr type="lMarg" val="0"/></dgm:constrLst>
            <dgm:layoutNode name="label"><dgm:alg type="tx"><dgm:param type="stBulletLvl" val="1"/></dgm:alg><dgm:presOf axis="self"/>{shape}
            <dgm:constrLst><dgm:constr type="primFontSz" val="30"/></dgm:constrLst></dgm:layoutNode></dgm:layoutNode></dgm:forEach></dgm:layoutNode></dgm:layoutDef>"#
        );
        let mut parts = files(&uncached_fixture());
        for (_, bytes) in &mut parts {
            if parse_xml(bytes).ok().is_some_and(|d| d.root.is("layoutDef")) {
                *bytes = layout.as_bytes().to_vec();
            }
        }
        let p = deckcraft_pptx::import(&zip(parts)).unwrap();
        let back = deckcraft_pptx::import(&deckcraft_pptx::export(&p).unwrap()).unwrap();
        for doc in [&p, &back] {
            let text: Vec<_> = group(doc).children().iter().filter(|s| !s.hidden).filter_map(|s| s.text.as_ref()).collect();
            assert_eq!(text.len(), visible_per_card * 3, "{shape}");
            for body in text {
                assert!(body.paragraphs.iter().flat_map(|p| &p.runs).all(|r| r.props.size == Some(23.)));
            }
        }
        assert_eq!(geometry(group(&p)), geometry(group(&back)));
        assert_eq!(
            deckcraft_engine::render::render_slide(&p, 0, &Default::default()).pixels,
            deckcraft_engine::render::render_slide(&back, 0, &Default::default()).pixels
        );
    }
}

#[test]
fn snake_packs_nested_occupied_frames_and_preserves_editability() {
    for flow in ["row", "col"] {
        let layout = format!(
            r#"<dgm:layoutDef xmlns:dgm="http://schemas.openxmlformats.org/drawingml/2006/diagram" uniqueId="urn:deckcraft:test:snake-occupied" minVer="12.0">
            <dgm:title val="Original nested packing"/><dgm:desc val="Constrained nested cards"/><dgm:catLst><dgm:cat type="list" pri="99"/></dgm:catLst>
            <dgm:layoutNode><dgm:alg type="snake"><dgm:param type="flowDir" val="{flow}"/><dgm:param type="bkpt" val="fixed"/><dgm:param type="bkPtFixedVal" val="2"/></dgm:alg>
            <dgm:presOf/><dgm:shape type="none"/><dgm:constrLst><dgm:constr type="w" for="ch" val="40"/><dgm:constr type="h" for="ch" val="30"/></dgm:constrLst>
            <dgm:forEach axis="ch" ptType="node"><dgm:layoutNode name="card"><dgm:alg type="composite"/><dgm:presOf/><dgm:shape type="none"/>
            <dgm:constrLst><dgm:constr type="w" for="ch" val="20"/><dgm:constr type="h" for="ch" val="10"/></dgm:constrLst>
            <dgm:layoutNode name="label"><dgm:alg type="tx"/><dgm:presOf axis="self"/><dgm:shape type="rect"/>
            <dgm:constrLst><dgm:constr type="primFontSz" val="8"/><dgm:constr type="lMarg" val="0"/><dgm:constr type="rMarg" val="0"/></dgm:constrLst>
            </dgm:layoutNode></dgm:layoutNode></dgm:forEach></dgm:layoutNode></dgm:layoutDef>"#
        );
        let mut parts = files(&uncached_fixture());
        for (_, bytes) in &mut parts {
            if parse_xml(bytes).ok().is_some_and(|doc| doc.root.is("layoutDef")) {
                *bytes = layout.as_bytes().to_vec();
            }
        }
        let p = deckcraft_pptx::import(&zip(parts)).unwrap();
        let cards = group(&p).children();
        assert_eq!(cards.len(), 3);
        let (a, b, c) = (cards[0].xfrm.unwrap(), cards[1].xfrm.unwrap(), cards[2].xfrm.unwrap());
        let (w, h) = (20. * 72. / 25.4, 10. * 72. / 25.4);
        for card in cards {
            let r = card.xfrm.unwrap();
            assert!((r.w - w).abs() < 0.01 && (r.h - h).abs() < 0.01);
            assert!(card.text.as_ref().is_some_and(|body| !body.text().is_empty()));
        }
        if flow == "row" {
            assert!((b.x - a.x - w).abs() < 0.01 && (c.x - a.x).abs() < 0.01 && (c.y - a.y - h).abs() < 0.01);
        } else {
            assert!((b.y - a.y - h).abs() < 0.01 && (c.y - a.y).abs() < 0.01 && (c.x - a.x - w).abs() < 0.01);
        }
        let back = deckcraft_pptx::import(&deckcraft_pptx::export(&p).unwrap()).unwrap();
        assert_eq!(geometry(group(&p)), geometry(group(&back)));
        assert_eq!(group(&p).diagram.as_ref().unwrap().nodes, group(&back).diagram.as_ref().unwrap().nodes);
        assert_eq!(group(&p).diagram.as_ref().unwrap().layout_xml, group(&back).diagram.as_ref().unwrap().layout_xml);
        assert_eq!(
            deckcraft_engine::render::render_slide(&p, 0, &Default::default()).pixels,
            deckcraft_engine::render::render_slide(&back, 0, &Default::default()).pixels
        );
    }
}
