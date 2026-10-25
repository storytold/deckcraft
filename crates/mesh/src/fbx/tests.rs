use super::*;

/// Writes `nodes` as a binary FBX of `version` (arrays compressed when `zip`).
pub(crate) fn binary(nodes: &[Node], version: u32, zip: bool) -> Vec<u8> {
    let mut out = BINARY_MAGIC.to_vec();
    out.extend([0x1a, 0x00]);
    out.extend(version.to_le_bytes());
    let wide = version >= 7500;
    for n in nodes {
        record(&mut out, n, wide, zip);
    }
    out.extend(vec![0u8; if wide { 25 } else { 13 }]);
    out
}

fn field(out: &mut Vec<u8>, v: usize, wide: bool) {
    if wide {
        out.extend((v as u64).to_le_bytes());
    } else {
        out.extend((v as u32).to_le_bytes());
    }
}

fn record(out: &mut Vec<u8>, n: &Node, wide: bool, zip: bool) {
    let start = out.len();
    let mut props = Vec::new();
    for p in &n.props {
        match p {
            Prop::Int(i) => {
                props.push(b'L');
                props.extend(i.to_le_bytes());
            }
            Prop::Float(f) => {
                props.push(b'D');
                props.extend(f.to_le_bytes());
            }
            Prop::Str(s) => {
                props.push(b'S');
                props.extend((s.len() as u32).to_le_bytes());
                props.extend(s.as_bytes());
            }
            Prop::Bytes(n) => {
                props.push(b'R');
                props.extend((*n as u32).to_le_bytes());
                props.extend(vec![0u8; *n]);
            }
            Prop::Ints(v) => array(&mut props, b'i', v.len(), v.iter().flat_map(|&i| (i as i32).to_le_bytes()).collect(), zip),
            Prop::Floats(v) => array(&mut props, b'd', v.len(), v.iter().flat_map(|f| f.to_le_bytes()).collect(), zip),
        }
    }
    field(out, 0, wide);
    field(out, n.props.len(), wide);
    field(out, props.len(), wide);
    out.push(n.name.len() as u8);
    out.extend(n.name.as_bytes());
    out.extend(props);
    if !n.children.is_empty() {
        for c in &n.children {
            record(out, c, wide, zip);
        }
        out.extend(vec![0u8; if wide { 25 } else { 13 }]);
    }
    let end = out.len();
    if wide {
        out[start..start + 8].copy_from_slice(&(end as u64).to_le_bytes());
    } else {
        out[start..start + 4].copy_from_slice(&(end as u32).to_le_bytes());
    }
}

fn array(out: &mut Vec<u8>, kind: u8, count: usize, raw: Vec<u8>, zip: bool) {
    out.push(kind);
    out.extend((count as u32).to_le_bytes());
    let data = if zip { miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6) } else { raw };
    out.extend(u32::from(zip).to_le_bytes());
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(data);
}

fn n(name: &str, props: Vec<Prop>, children: Vec<Node>) -> Node {
    Node { name: name.into(), props, children }
}

fn s(v: &str) -> Prop {
    Prop::Str(v.into())
}

fn p70(name: &str, kind: &str, values: &[f64]) -> Node {
    let mut props = vec![s(name), s(kind), s(""), s("A")];
    props.extend(values.iter().map(|&v| Prop::Float(v)));
    n("P", props, vec![])
}

fn layer(name: &str, mapping: &str, reference: &str, data: Node) -> Node {
    n(
        name,
        vec![Prop::Int(0)],
        vec![n("MappingInformationType", vec![s(mapping)], vec![]), n("ReferenceInformationType", vec![s(reference)], vec![]), data],
    )
}

/// A unit cube's corners, its six quads and a normal per corner.
fn cube_geometry() -> Node {
    let v: Vec<f64> = (0..8).flat_map(|i| [(i & 1) as f64, ((i >> 1) & 1) as f64, ((i >> 2) & 1) as f64]).collect();
    let quads: [[i64; 4]; 6] = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
    let face_normals: [[f64; 3]; 6] = [[0.0, 0.0, -1.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
    let pvi: Vec<i64> = quads.iter().flat_map(|q| [q[0], q[1], q[2], !q[3]]).collect();
    let normals: Vec<f64> = face_normals.iter().flat_map(|f| [*f; 4]).flatten().collect();
    n(
        "Geometry",
        vec![Prop::Int(10), s("Cube\0\x01Geometry"), s("Mesh")],
        vec![
            n("Vertices", vec![Prop::Floats(v)], vec![]),
            n("PolygonVertexIndex", vec![Prop::Ints(pvi)], vec![]),
            layer("LayerElementNormal", "ByPolygonVertex", "Direct", n("Normals", vec![Prop::Floats(normals)], vec![])),
            layer("LayerElementMaterial", "ByPolygon", "IndexToDirect", n("Materials", vec![Prop::Ints(vec![0, 0, 1, 1, 1, 1])], vec![])),
        ],
    )
}

fn cube(up_axis: i64) -> Vec<Node> {
    let c = |a: i64, b: i64| n("C", vec![s("OO"), Prop::Int(a), Prop::Int(b)], vec![]);
    let material = |id: i64, rgb: [f64; 3]| {
        n("Material", vec![Prop::Int(id), s("M\0\x01Material"), s("")], vec![n("Properties70", vec![], vec![p70("DiffuseColor", "Color", &rgb)])])
    };
    vec![
        n("FBXHeaderExtension", vec![], vec![n("FBXVersion", vec![Prop::Int(7400)], vec![])]),
        n(
            "GlobalSettings",
            vec![],
            vec![n("Properties70", vec![], vec![n("P", vec![s("UpAxis"), s("int"), s("Integer"), s(""), Prop::Int(up_axis)], vec![])])],
        ),
        n(
            "Objects",
            vec![],
            vec![
                cube_geometry(),
                n(
                    "Model",
                    vec![Prop::Int(20), s("Box\0\x01Model"), s("Mesh")],
                    vec![n("Properties70", vec![], vec![p70("Lcl Translation", "Lcl Translation", &[10.0, 0.0, 0.0])])],
                ),
                n(
                    "Model",
                    vec![Prop::Int(21), s("Root\0\x01Model"), s("Null")],
                    vec![n("Properties70", vec![], vec![p70("Lcl Scaling", "Lcl Scaling", &[2.0, 2.0, 2.0])])],
                ),
                material(30, [1.0, 0.0, 0.0]),
                material(31, [0.0, 0.5, 0.0]),
            ],
        ),
        n("Connections", vec![], vec![c(10, 20), c(20, 21), c(21, 0), c(30, 20), c(31, 20)]),
    ]
}

fn check_cube(scene: &Scene, label: &str) {
    assert_eq!(scene.triangle_count(), 12, "{label}");
    assert_eq!(scene.primitives.len(), 2, "{label}: one primitive per material slot");
    assert_eq!(scene.primitives[0].color, [1.0, 0.0, 0.0, 1.0], "{label}");
    assert_eq!(scene.primitives[1].color, [0.0, 0.5, 0.0, 1.0], "{label}");
    assert_eq!(scene.primitives[0].name, "Box", "{label}");
    assert!(scene.primitives.iter().all(|p| p.is_valid() && p.normals.len() == p.positions.len()), "{label}");
    // Translated by 10 inside a parent scaled by 2.
    let b = scene.bounds().expect("bounds");
    assert_eq!((b.min, b.max), ([20.0, 0.0, 0.0], [22.0, 2.0, 2.0]), "{label}");
}

#[test]
fn binary_cube_in_every_layout() {
    for (version, zip) in [(7400, false), (7400, true), (7500, true), (6100, false)] {
        let scene = read(&binary(&cube(1), version, zip)).expect("read");
        check_cube(&scene, &format!("{version} zip={zip}"));
    }
}

#[test]
fn z_up_turns_to_y_up() {
    let scene = read(&binary(&cube(2), 7400, true)).expect("read");
    let b = scene.bounds().expect("bounds");
    assert_eq!((b.min, b.max), ([20.0, 0.0, -2.0], [22.0, 2.0, 0.0]));
}

#[test]
fn ascii_cube() {
    let geometry = cube_geometry();
    let list = |node: &Node| -> String {
        match &node.props[0] {
            Prop::Floats(v) => v.iter().map(f64::to_string).collect::<Vec<_>>().join(","),
            Prop::Ints(v) => v.iter().map(i64::to_string).collect::<Vec<_>>().join(",\n"),
            _ => String::new(),
        }
    };
    let pick = |name: &str| geometry.child(name).expect(name);
    let normals = pick("LayerElementNormal").child("Normals").expect("normals");
    let text = format!(
        "; FBX 7.4.0 project file\n; ----\nGlobalSettings:  {{\n Properties70:  {{\n  P: \"UpAxis\", \"int\", \"Integer\", \"\",1\n }}\n}}\nObjects:  {{\n\
         Geometry: 10, \"Geometry::Cube\", \"Mesh\" {{\n  Vertices: *24 {{\n   a: {}\n  }}\n  PolygonVertexIndex: *24 {{\n   a: {}\n  }}\n\
         LayerElementNormal: 0 {{\n   MappingInformationType: \"ByPolygonVertex\"\n   ReferenceInformationType: \"Direct\"\n   Normals: *72 {{\n    a: {}\n   }}\n  }}\n\
         LayerElementMaterial: 0 {{\n   MappingInformationType: \"ByPolygon\"\n   ReferenceInformationType: \"IndexToDirect\"\n   Materials: *6 {{\n    a: 0,0,1,1,1,1\n   }}\n  }}\n }}\n\
         Model: 20, \"Model::Box\", \"Mesh\" {{\n  Properties70:  {{\n   P: \"Lcl Translation\", \"Lcl Translation\", \"\", \"A\",10,0,0\n  }}\n }}\n\
         Model: 21, \"Model::Root\", \"Null\" {{\n  Properties70:  {{\n   P: \"Lcl Scaling\", \"Lcl Scaling\", \"\", \"A\",2,2,2\n  }}\n }}\n\
         Material: 30, \"Material::Red\", \"\" {{\n  Properties70:  {{\n   P: \"DiffuseColor\", \"Color\", \"\", \"A\",1,0,0\n  }}\n }}\n\
         Material: 31, \"Material::Green\", \"\" {{\n  Properties70:  {{\n   P: \"DiffuseColor\", \"Color\", \"\", \"A\",0,0.5,0\n  }}\n }}\n}}\n\
         Connections:  {{\n ;Geometry::Cube, Model::Box\n C: \"OO\",10,20\n C: \"OO\",20,21\n C: \"OO\",21,0\n C: \"OO\",30,20\n C: \"OO\",31,20\n}}\n",
        list(pick("Vertices")),
        list(pick("PolygonVertexIndex")),
        list(normals),
    );
    let scene = read(text.as_bytes()).expect("read");
    check_cube(&scene, "ascii");
}

#[test]
fn fbx6_model_holds_its_mesh() {
    let text = "; FBX 6.1.0 project file\nObjects:  {\n Model: \"Model::Tri\", \"Mesh\" {\n  Properties60:  {\n   Property: \"Lcl Translation\", \"Lcl Translation\", \"A\",0,5,0\n  }\n\
                Vertices: 0,0,0,1,0,0,0,1,0\n  PolygonVertexIndex: 0,1,-3\n }\n Material: \"Material::Blue\", \"\" {\n  Properties60:  {\n   Property: \"Diffuse\", \"Color\", \"A\",0,0,1\n  }\n }\n}\n\
                Connections:  {\n Connect: \"OO\", \"Model::Tri\", \"Model::Scene\"\n Connect: \"OO\", \"Material::Blue\", \"Model::Tri\"\n}\n";
    let scene = read(text.as_bytes()).expect("read");
    assert_eq!(scene.triangle_count(), 1);
    assert_eq!(scene.primitives[0].name, "Tri");
    assert_eq!(scene.primitives[0].color, [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(scene.bounds().map(|b| b.min), Some([0.0, 5.0, 0.0]));
}

#[test]
fn mirrored_models_keep_their_faces_outward() {
    let mut nodes = cube(1);
    let objects = nodes.iter_mut().find(|n| n.name == "Objects").expect("objects");
    let model = objects.children.iter_mut().find(|n| n.props.first() == Some(&Prop::Int(21))).expect("root");
    model.children = vec![n("Properties70", vec![], vec![p70("Lcl Scaling", "Lcl Scaling", &[-1.0, 1.0, 1.0])])];
    let plain = read(&binary(&cube(1), 7400, false)).expect("read");
    let mirrored = read(&binary(&nodes, 7400, false)).expect("read");
    assert_ne!(plain.primitives[0].indices[..3], mirrored.primitives[0].indices[..3]);
}

#[test]
fn hostile_files_fail_cleanly() {
    let good = binary(&cube(1), 7400, true);
    for len in 0..good.len() {
        let _ = read(&good[..len]);
    }
    // Every single-byte change is read or refused, never a panic.
    for at in (0..good.len()).step_by(3) {
        let mut b = good.clone();
        b[at] ^= 0xA5;
        let _ = read(&b);
    }
    // Deep nesting.
    let mut deep = n("Leaf", vec![], vec![]);
    for _ in 0..200 {
        deep = n("N", vec![], vec![deep]);
    }
    assert!(read(&binary(&[deep], 7400, false)).is_err());
    // An array that says it is huge.
    let mut huge = binary(&[n("Objects", vec![], vec![n("Vertices", vec![Prop::Floats(vec![0.0; 3])], vec![])])], 7400, false);
    if let Some(at) = huge.windows(8).position(|w| w == b"Vertices") {
        let count_at = at + 8 + 1;
        huge[count_at..count_at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    }
    assert!(read(&huge).is_err());
    assert!(read(b"Kaydara FBX Binary  \0").is_err());
    assert_eq!(read(b"Objects: {\n}\n"), Err(Error::Empty));
    // Polygon indices beyond the vertices are skipped, not followed.
    let text = "Objects: {\n Geometry: 1, \"Geometry::G\", \"Mesh\" {\n  Vertices: 0,0,0,1,0,0,0,1,0\n  PolygonVertexIndex: 0,1,99,-3\n }\n}\n";
    assert_eq!(read(text.as_bytes()).map(|s| s.triangle_count()), Ok(1));
}
