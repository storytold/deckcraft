use deckcraft_mesh::{Material, Primitive, Scene};
use deckcraft_model::{ShapeId, ShapeKind};
use serde_json::json;

use crate::cmd::base64_encode;
use crate::tools::{Hit, Mods, PointerEvent, PointerKind};
use crate::*;

/// A box from `lo` to `hi` with one face per colour.
fn boxed(lo: [f32; 3], hi: [f32; 3]) -> Scene {
    let mut p = Primitive { material: Some(0), ..Default::default() };
    let c = |sx: usize, sy: usize, sz: usize| [[lo[0], hi[0]][sx], [lo[1], hi[1]][sy], [lo[2], hi[2]][sz]];
    let faces: [[(usize, usize, usize); 4]; 6] = [
        [(0, 0, 1), (1, 0, 1), (1, 1, 1), (0, 1, 1)],
        [(1, 0, 0), (0, 0, 0), (0, 1, 0), (1, 1, 0)],
        [(1, 0, 1), (1, 0, 0), (1, 1, 0), (1, 1, 1)],
        [(0, 0, 0), (0, 0, 1), (0, 1, 1), (0, 1, 0)],
        [(0, 1, 1), (1, 1, 1), (1, 1, 0), (0, 1, 0)],
        [(0, 0, 0), (1, 0, 0), (1, 0, 1), (0, 0, 1)],
    ];
    for f in faces {
        let b = p.positions.len() as u32;
        for (x, y, z) in f {
            p.positions.push(c(x, y, z));
        }
        p.indices.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
    }
    Scene { primitives: vec![p], materials: vec![Material { base_color: [0.8, 0.3, 0.2, 1.0], ..Default::default() }], textures: vec![] }
}

fn glb(lo: [f32; 3], hi: [f32; 3]) -> Vec<u8> {
    deckcraft_mesh::to_glb(&boxed(lo, hi))
}

fn session() -> Session {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    s
}

fn insert_box(s: &mut Session, lo: [f32; 3], hi: [f32; 3]) -> ShapeId {
    let r = s.execute("insert.model3d", &json!({"name": "box.glb", "data": base64_encode(&glb(lo, hi))})).unwrap();
    ShapeId(r["id"].as_u64().unwrap() as u32)
}

fn model(s: &Session, id: ShapeId) -> (deckcraft_model::Model3d, deckcraft_geom::Xfrm) {
    let sh = s.doc().unwrap().shape(id).unwrap().clone();
    let ShapeKind::Model3d(m) = sh.kind else { panic!("not a model") };
    (*m, sh.xfrm.unwrap())
}

fn close(a: f64, b: f64, eps: f64) -> bool {
    (a - b).abs() <= eps
}

#[test]
fn insert_centres_the_model_at_powerpoints_size() {
    let mut s = session();
    // PowerPoint's probe: a 2 × 1 × 0.5 box inserts as a 375.43 × 187.71 pt frame in the middle of
    // a 16:9 slide.
    let id = insert_box(&mut s, [2.0, 0.0, -0.25], [4.0, 1.0, 0.25]);
    let (m, x) = model(&s, id);
    assert!(close(x.w, 375.4273, 0.05) && close(x.h, 187.7137, 0.05), "{x:?}");
    assert!(close(x.center().x, 480.0, 1e-6) && close(x.center().y, 270.0, 1e-6));
    assert!(close(m.span(x.h), 426.6667, 0.01));
    assert_eq!(m.rot, [0.0; 3]);
    assert!(m.auto_fit);
    let st = s.doc().unwrap();
    let sh = st.shape(id).unwrap();
    assert_eq!(sh.name, "3D Model 1");
    let media = st.doc.media(m.media).unwrap();
    assert_eq!(media.content_type, "model/gltf.binary");
    assert!(media.data.starts_with(b"glTF"));
    assert_eq!(st.selection.shapes, vec![id]);
    // One undo step removes it.
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.doc().unwrap().shape(id).is_none());
}

#[test]
fn insert_reads_every_format() {
    let png = {
        let img = image::RgbaImage::from_pixel(4, 4, image::Rgba([10, 200, 30, 255]));
        let mut b = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut b), image::ImageFormat::Png).unwrap();
        b
    };
    let obj = "mtllib m.mtl\nv 0 0 0\nv 1 0 0\nv 1 1 0\nvt 0 0\nvt 1 0\nvt 1 1\nusemtl M\nf 1/1 2/2 3/3\n";
    let mtl = "newmtl M\nKd 1 1 1\nmap_Kd t.png\n";
    let stl = "solid s\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid s\n";
    let ply = "ply\nformat ascii 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n0 0 0\n1 0 0\n0 1 0\n3 0 1 2\n";
    let mut s = session();
    let r = s
        .execute(
            "insert.model3d",
            &json!({"name": "t.obj", "data": base64_encode(obj.as_bytes()), "files": {"m.mtl": base64_encode(mtl.as_bytes()), "t.png": base64_encode(&png)}}),
        )
        .unwrap();
    assert_eq!(r["textures"], 1);
    for (name, data) in [("t.stl", stl.as_bytes()), ("t.ply", ply.as_bytes())] {
        let r = s.execute("insert.model3d", &json!({"name": name, "data": base64_encode(data)})).unwrap();
        assert_eq!(r["triangles"], 1, "{name}");
    }
    // Everything is kept as glTF binary.
    let st = s.doc().unwrap();
    let models: Vec<_> = st.shapes().iter().filter_map(|x| if let ShapeKind::Model3d(m) = &x.kind { Some(m.media) } else { None }).collect();
    assert_eq!(models.len(), 3);
    for m in models {
        assert!(st.doc.media(m).unwrap().data.starts_with(b"glTF"));
    }
    // Not a model, or broken: an error, nothing inserted.
    let n = s.doc().unwrap().shapes().len();
    assert!(s.execute("insert.model3d", &json!({"name": "a.png", "data": base64_encode(&png)})).is_err());
    assert!(s.execute("insert.model3d", &json!({"name": "a.glb", "data": base64_encode(b"glTF\x02\0\0\0garbage")})).is_err());
    assert!(s.execute("insert.model3d", &json!({"name": "a.stl", "data": base64_encode(b"solid\nendsolid\n")})).is_err());
    assert_eq!(s.doc().unwrap().shapes().len(), n);
}

/// A new, empty folder of one test's own (never one left by an earlier run), removed when it
/// goes out of scope, even when the test fails.
struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        for _ in 0..100 {
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
            let p = std::env::temp_dir().join(format!("deckcraft-{tag}-{}-{nanos}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
            // `create_dir` fails on a folder that exists: this one is new.
            if std::fs::create_dir(&p).is_ok() {
                return TempDir(p);
            }
        }
        panic!("no new temporary folder");
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn companions_come_from_the_models_folder_only() {
    let tmp = TempDir::new("model3d");
    let dir = tmp.0.clone();
    let sub = dir.join("model");
    std::fs::create_dir_all(sub.join("tex")).unwrap();
    let img = image::RgbaImage::from_pixel(2, 2, image::Rgba([200, 10, 10, 255]));
    img.save(sub.join("tex").join("a.png")).unwrap();
    img.save(dir.join("outside.png")).unwrap();
    std::fs::write(sub.join("m.mtl"), "newmtl A\nmap_Kd tex/a.png\nnewmtl B\nmap_Kd ../outside.png\n").unwrap();
    let obj = "mtllib m.mtl\nv 0 0 0\nv 1 0 0\nv 1 1 0\nvt 0 0\nvt 1 0\nvt 1 1\nusemtl A\nf 1/1 2/2 3/3\nusemtl B\nf 1/1 3/3 2/2\n";
    std::fs::write(sub.join("m.obj"), obj).unwrap();
    let mut s = session();
    let r = s.execute("insert.model3d", &json!({"path": sub.join("m.obj").to_string_lossy()})).unwrap();
    // tex/a.png is read; ../outside.png is not followed out of the folder.
    assert_eq!(r["textures"], 1);
}

#[test]
fn rotating_refits_the_frame_around_its_centre_without_drift() {
    let mut s = session();
    let id = insert_box(&mut s, [2.0, 0.0, -0.25], [4.0, 1.0, 0.25]);
    let (m0, x0) = model(&s, id);
    s.execute("model3d.rotate", &json!({"y": 90})).unwrap();
    let (m1, x1) = model(&s, id);
    assert_eq!(m1.rot, [0.0, 90.0, 0.0]);
    // Seen end on, the 2 m long box is narrow; its size on the slide is kept.
    assert!(x1.w < x0.w * 0.6, "{x1:?}");
    assert!(close(x1.center().x, x0.center().x, 1e-6) && close(x1.center().y, x0.center().y, 1e-6));
    assert!(close(m1.span(x1.h), m0.span(x0.h), 1e-6));
    // Relative turns about the view's axes compose; turning back restores the frame exactly.
    s.execute("model3d.rotate", &json!({"dy": -45})).unwrap();
    s.execute("model3d.rotate", &json!({"dy": -45})).unwrap();
    let (m2, x2) = model(&s, id);
    assert!(m2.rot.iter().all(|v| close(*v, 0.0, 1e-6) || close(*v, 360.0, 1e-6)), "{:?}", m2.rot);
    assert!(close(x2.w, x0.w, 1e-6) && close(x2.h, x0.h, 1e-6), "{x2:?} {x0:?}");
    // Angles are kept in [0, 360).
    s.execute("model3d.rotate", &json!({"x": -30, "y": 400, "z": 0})).unwrap();
    assert_eq!(model(&s, id).0.rot, [330.0, 40.0, 0.0]);
    // Bad input changes nothing.
    assert!(s.execute("model3d.rotate", &json!({})).is_err());
    assert!(s.execute("model3d.rotate", &json!({"x": "a lot"})).is_err());
}

#[test]
fn views_and_reset() {
    let mut s = session();
    let id = insert_box(&mut s, [-0.5; 3], [0.5; 3]);
    let x0 = model(&s, id).1;
    let views = s.execute("model3d.views", &json!({})).unwrap();
    assert!(views.as_array().unwrap().len() >= 10);
    s.execute("model3d.view", &json!({"view": "top"})).unwrap();
    assert_eq!(model(&s, id).0.rot, [90.0, 0.0, 0.0]);
    s.execute("model3d.view", &json!({"view": "aboveFrontLeft"})).unwrap();
    let r = model(&s, id).0.rot;
    assert!(r[0] > 0.0 && r[1] > 0.0, "{r:?}");
    assert!(s.execute("model3d.view", &json!({"view": "sideways"})).is_err());
    s.execute("model3d.reset", &json!({})).unwrap();
    assert_eq!(model(&s, id).0.rot, [0.0; 3]);
    // Reset 3D Model and Size: back to the inserted size, where the model is now.
    s.execute("shape.resize", &json!({"w": 100, "h": 100})).unwrap();
    s.execute("model3d.reset", &json!({"size": true})).unwrap();
    let x = model(&s, id).1;
    assert!(close(x.w, x0.w, 1e-6) && close(x.h, x0.h, 1e-6), "{x:?} {x0:?}");
    // Commands need a model selected.
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [10, 10, 50, 50]})).unwrap();
    assert!(s.execute("model3d.reset", &json!({})).is_err());
    assert!(!s.commands().iter().find(|c| c.id == "model3d.view").unwrap().enabled);
}

#[test]
fn locked_models_do_not_turn() {
    let mut s = session();
    let id = insert_box(&mut s, [-0.5; 3], [0.5; 3]);
    s.edit(|doc, sel| {
        let list = crate::shapes_mut(doc, sel).unwrap();
        deckcraft_model::find_shape_mut(list, id).unwrap().locked = true;
        Ok(())
    })
    .unwrap();
    assert!(s.execute("model3d.rotate", &json!({"y": 30})).is_err());
    assert_eq!(model(&s, id).0.rot, [0.0; 3]);
}

#[test]
fn the_3d_handle_turns_the_model_and_undoes_in_one_step() {
    let mut s = session();
    let id = insert_box(&mut s, [-0.5; 3], [0.5; 3]);
    let x = model(&s, id).1;
    let c = x.center();
    let ev = |kind, x: f64, y: f64| PointerEvent { kind, x, y, mods: Mods::default(), tol: 3.0, pressure: None };
    assert_eq!(s.hit_test(c, 3.0), Some(Hit::Rotate3d { id }));
    s.pointer(ev(PointerKind::Down, c.x, c.y)).unwrap();
    // A quarter of the frame's width to the right: 45° about the vertical axis.
    s.pointer(ev(PointerKind::Drag, c.x + x.w / 4.0, c.y)).unwrap();
    s.pointer(ev(PointerKind::Up, c.x + x.w / 4.0, c.y)).unwrap();
    let (m, x1) = model(&s, id);
    assert!(close(m.rot[1], 45.0, 1e-6), "{:?}", m.rot);
    // Seen corner on, the cube is wider than face on.
    assert!(x1.w > x.w);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(model(&s, id).0.rot, [0.0; 3]);
    // The rotation handle above the frame spins the model about the view's axis.
    let (_, top) = crate::tools::box_handles(&x);
    s.pointer(ev(PointerKind::Down, top.x, top.y)).unwrap();
    // A quarter turn clockwise round the centre.
    let d = (top - c).hypot();
    s.pointer(ev(PointerKind::Drag, c.x + d, c.y)).unwrap();
    s.pointer(ev(PointerKind::Up, c.x + d, c.y)).unwrap();
    let (m, x2) = model(&s, id);
    assert!(close(m.rot[2], 270.0, 1e-6), "{:?}", m.rot);
    assert_eq!(x2.rot, 0.0);
}

#[test]
fn resizing_scales_the_model_with_its_frame() {
    let mut s = session();
    let id = insert_box(&mut s, [-0.5; 3], [0.5; 3]);
    let (m0, x0) = model(&s, id);
    s.execute("shape.resize", &json!({"w": x0.w * 2.0, "h": x0.h * 2.0})).unwrap();
    let (m1, x1) = model(&s, id);
    assert!(close(m1.span(x1.h), 2.0 * m0.span(x0.h), 1e-6));
}

fn render_center(doc: &deckcraft_model::Presentation, index: usize, x: deckcraft_geom::Xfrm) -> [u8; 4] {
    let img = deckcraft_render::render_slide(doc, index, &deckcraft_render::RenderOpts { scale: 1.0, ..Default::default() });
    let c = x.center();
    img.pixel(c.x as u32, c.y as u32)
}

#[test]
fn models_render_and_survive_pptx_and_native_files() {
    let mut s = session();
    let id = insert_box(&mut s, [2.0, 0.0, -0.25], [4.0, 1.0, 0.25]);
    s.execute("model3d.rotate", &json!({"x": 20, "y": 40, "z": 60})).unwrap();
    let (m, x) = model(&s, id);
    let doc = s.doc().unwrap().doc.clone();
    let index = s.doc().unwrap().selection.slide;
    // Drawn: the middle of the frame isn't the white slide.
    let px = render_center(&doc, index, x);
    assert!(px[0] < 250 || px[1] < 250 || px[2] < 250, "{px:?}");
    for format in ["pptx", "deckcraft"] {
        let bytes = crate::cmd::file::save_bytes(&doc, format).unwrap();
        let back = crate::cmd::file::open_presentation(&format!("t.{format}"), &bytes).unwrap();
        let sl = back.slides.get(index).unwrap();
        let sh = sl.shapes.iter().find(|x| matches!(x.kind, ShapeKind::Model3d(_))).expect(format);
        let ShapeKind::Model3d(m2) = &sh.kind else { panic!("{format}: not a model") };
        for k in 0..3 {
            assert!(close(m2.rot[k], m.rot[k], 1e-3), "{format}: {:?} {:?}", m2.rot, m.rot);
            assert!(close(m2.pre[k], m.pre[k], 1e-6) && close(m2.camera.pos[k], m.camera.pos[k], 1e-6), "{format}");
        }
        assert!(close(m2.meter_per_unit, m.meter_per_unit, 1e-6));
        let x2 = sh.xfrm.unwrap();
        assert!(close(x2.w, x.w, 0.01) && close(m2.span(x2.h), m.span(x.h), 0.01), "{format}");
        assert_eq!(back.media(m2.media).unwrap().data.as_slice(), doc.media(m.media).unwrap().data.as_slice());
        assert!(m2.auto_fit);
        assert_eq!(sh.name, "3D Model 1");
        // And still drawn the same way.
        assert_eq!(render_center(&back, index, x2), px, "{format}");
    }
}

#[test]
fn pptx_package_has_the_model_part_and_a_fallback_picture() {
    let mut s = session();
    insert_box(&mut s, [-0.5; 3], [0.5; 3]);
    let bytes = crate::cmd::file::save_bytes(&s.doc().unwrap().doc, "pptx").unwrap();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut read = |n: &str| {
        use std::io::Read;
        let mut v = String::new();
        z.by_name(n).unwrap().read_to_string(&mut v).unwrap();
        v
    };
    let ct = read("[Content_Types].xml");
    assert!(ct.contains(r#"Extension="glb" ContentType="model/gltf.binary""#), "{ct}");
    let slide = read("ppt/slides/slide2.xml");
    let order = [
        "<mc:Choice",
        "Requires=\"am3d\"",
        "<am3d:model3d",
        "<am3d:spPr",
        "<am3d:camera",
        "<am3d:trans",
        "<am3d:raster",
        "<am3d:objViewport",
        "<am3d:ambientLight",
        "</mc:Choice>",
        "<mc:Fallback>",
        "<p:pic>",
    ];
    let mut at = 0;
    for t in order {
        let i = slide[at..].find(t).unwrap_or_else(|| panic!("{t} missing or out of order in {slide}"));
        at += i;
    }
    let rels = read("ppt/slides/_rels/slide2.xml.rels");
    assert!(rels.contains("http://schemas.microsoft.com/office/2017/06/relationships/model3d") && rels.contains("../media/model3d1.glb"), "{rels}");
}

#[test]
fn morph_turns_the_model_between_slides() {
    let mut s = session();
    insert_box(&mut s, [-0.5; 3], [0.5; 3]);
    s.execute("slide.duplicate", &json!({})).unwrap();
    s.execute("transition.set", &json!({"kind": "morph"})).unwrap();
    let st = s.doc().unwrap();
    let copy = st.shapes().iter().find(|x| matches!(x.kind, ShapeKind::Model3d(_))).unwrap().id;
    s.execute("selection.select", &json!({"ids": [copy.0]})).unwrap();
    s.execute("model3d.rotate", &json!({"y": 90})).unwrap();
    let doc = s.doc().unwrap().doc.clone();
    let (a, b) = (doc.slides[1].clone(), doc.slides[2].clone());
    let f = deckcraft_anim::morph_frame(&doc, &a, &b, 0.5, &|_| false);
    // One model, turned half way, not two cross-fading.
    let models: Vec<_> = f.shapes.iter().filter_map(|(x, _)| if let ShapeKind::Model3d(m) = &x.kind { Some(m.clone()) } else { None }).collect();
    assert_eq!(models.len(), 1);
    assert!(close(models[0].rot[1], 45.0, 1e-6), "{:?}", models[0].rot);
}

#[test]
fn arrange_rotate_turns_models_about_the_view_axis() {
    let mut s = session();
    let id = insert_box(&mut s, [-0.5; 3], [0.5; 3]);
    s.execute("arrange.rotateRight", &json!({})).unwrap();
    let (m, x) = model(&s, id);
    // Clockwise on the slide: −90° about the axis towards the viewer; the frame stays upright.
    assert!(close(m.rot[2], 270.0, 1e-6), "{:?}", m.rot);
    assert_eq!(x.rot, 0.0);
    s.execute("shape.rotate", &json!({"deg": 30})).unwrap();
    assert!(close(model(&s, id).0.rot[2], 330.0, 1e-6));
    // Models don't flip.
    s.execute("arrange.flipHorizontal", &json!({})).unwrap();
    assert!(!model(&s, id).1.flip_h);
    // Shapes and models together: each turns its own way.
    let r = s.execute("shape.insert", &json!({"preset": "rect", "rect": [10, 10, 50, 50]})).unwrap();
    let rect = r["id"].as_u64().unwrap();
    s.execute("shape.rotate", &json!({"ids": [id.0, rect], "by": 10})).unwrap();
    assert!(close(model(&s, id).0.rot[2], 320.0, 1e-6));
    assert_eq!(s.doc().unwrap().shape(ShapeId(rect as u32)).unwrap().xfrm.unwrap().rot, 10.0);
}

#[test]
fn three_d_animations_apply_to_models_and_round_trip() {
    let mut s = session();
    let id = insert_box(&mut s, [-0.5; 3], [0.5; 3]);
    for (effect, class) in [("arrive", "entr"), ("turntable", "emph"), ("swing", "emph"), ("jumpTurn", "emph"), ("leave", "exit")] {
        s.execute("animation.add", &json!({"effect": effect, "class": class})).unwrap();
    }
    let st = s.doc().unwrap();
    let anims = &st.doc.slides[st.selection.slide].animations;
    assert_eq!(anims.iter().map(|a| a.effect.as_str()).collect::<Vec<_>>(), ["arrive", "turntable", "swing", "jumpTurn", "leave"]);
    let tt = &anims[1];
    assert_eq!((tt.duration_ms, tt.amount, tt.smooth_start), (20_000, Some(360.0), 0.1));
    // Not for other shapes.
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [10, 10, 50, 50]})).unwrap();
    assert!(s.execute("animation.add", &json!({"effect": "turntable", "class": "emph"})).is_err());
    // Through a PowerPoint file and back: the same effects, written as PowerPoint writes them.
    let doc = s.doc().unwrap().doc.clone();
    let index = s.doc().unwrap().selection.slide;
    let bytes = crate::cmd::file::save_bytes(&doc, "pptx").unwrap();
    let back = crate::cmd::file::open_presentation("t.pptx", &bytes).unwrap();
    let got: Vec<(String, u32)> = back.slides[index].animations.iter().map(|a| (a.effect.clone(), a.duration_ms)).collect();
    assert_eq!(
        got,
        [("arrive".into(), 1000), ("turntable".into(), 20_000), ("swing".into(), 5000), ("jumpTurn".into(), 2000), ("leave".into(), 1000)]
    );
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut z.by_name("ppt/slides/slide2.xml").unwrap(), &mut xml).unwrap();
    for needle in [
        r#"presetID="60" presetClass="entr" presetSubtype="128""#,
        r#"<p:animRot by="21600000">"#,
        "3d.view.rotation.y",
        "3d.object.translation.y",
        "3d.object.scale.x",
        r#"presetID="39" presetClass="emph" presetSubtype="2""#,
    ] {
        assert!(xml.contains(needle), "{needle} missing");
    }
    let _ = id;
}

#[test]
fn a_turning_model_draws_differently_and_saves_as_a_picture() {
    let mut s = session();
    let id = insert_box(&mut s, [-0.5; 3], [0.5; 3]);
    s.execute("model3d.view", &json!({"view": "aboveFrontLeft"})).unwrap();
    let doc = s.doc().unwrap().doc.clone();
    let index = s.doc().unwrap().selection.slide;
    let still = deckcraft_render::render_slide(&doc, index, &deckcraft_render::RenderOpts { scale: 0.5, ..Default::default() });
    let turned = deckcraft_anim::ModelAnim { view_turn: [0.0, 90.0, 0.0], ..Default::default() };
    let state = move |sid: ShapeId| deckcraft_render::ShapeState { model: if sid == id { turned } else { Default::default() }, ..Default::default() };
    let moved = deckcraft_render::render_slide(&doc, index, &deckcraft_render::RenderOpts { scale: 0.5, state: Some(&state), ..Default::default() });
    assert_ne!(still.pixels, moved.pixels);
    // Save as Picture: a PNG of the model on a transparent background.
    let r = s.execute("shape.saveAsPicture", &json!({})).unwrap();
    let png = crate::cmd::base64_decode(r["data"].as_str().unwrap()).unwrap();
    let img = image::load_from_memory(&png).unwrap().to_rgba8();
    assert_eq!(img.get_pixel(0, 0)[3], 0);
    assert_eq!(img.get_pixel(img.width() / 2, img.height() / 2)[3], 255);
    assert!(r["name"].as_str().unwrap().ends_with(".png"));
}

fn effects_on(s: &mut Session, id: u64) -> Vec<String> {
    let all = s.execute("animation.get", &json!({})).unwrap();
    all.as_array().unwrap().iter().filter(|a| a["shape"].as_u64() == Some(id)).map(|a| a["effect"].as_str().unwrap().to_string()).collect()
}

/// The Animation Painter carries a 3-D effect to 3-D models only, like Add Animation.
#[test]
fn the_animation_painter_paints_3d_effects_onto_models_only() {
    let mut s = session();
    let from = insert_box(&mut s, [-0.5; 3], [0.5; 3]);
    let other = insert_box(&mut s, [-0.5; 3], [0.5; 3]);
    let rect = s.execute("shape.insert", &json!({"preset": "rect", "rect": [10, 10, 100, 80]})).unwrap()["id"].as_u64().unwrap();
    s.execute("animation.add", &json!({"effect": "fade", "class": "entrance", "ids": [from.0]})).unwrap();
    s.execute("animation.add", &json!({"effect": "turntable", "ids": [from.0]})).unwrap();
    s.execute("animation.painter", &json!({"id": from.0, "sticky": true})).unwrap();
    s.execute("animation.painterApply", &json!({"ids": [rect, other.0]})).unwrap();
    assert_eq!(effects_on(&mut s, rect), ["fade"]);
    assert_eq!(effects_on(&mut s, other.0 as u64), ["fade", "turntable"]);
}

/// Pasted into a deck whose media already uses the model's id for other bytes, the model is
/// given a fresh id and still shows its own file.
#[test]
fn a_pasted_model_keeps_its_own_file_when_its_id_is_taken() {
    let mut s = session();
    let id = insert_box(&mut s, [-0.5; 3], [0.5; 3]);
    let (m, _) = model(&s, id);
    let glb = s.doc().unwrap().doc.media(m.media).unwrap().data.clone();
    s.execute("edit.select", &json!({"ids": [id.0]})).unwrap();
    s.execute("edit.copy", &json!({})).unwrap();
    s.execute("file.new", &json!({})).unwrap();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    // A picture there uses the model's media id for its own bytes.
    let mut png = Vec::new();
    image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 20, 30, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let pic = ShapeId(s.execute("insert.picture", &json!({"name": "p.png", "data": base64_encode(&png)})).unwrap()["id"].as_u64().unwrap() as u32);
    s.edit(|doc, sel| {
        let list = crate::shapes_mut(doc, sel).unwrap();
        let ShapeKind::Picture { fill } = &mut deckcraft_model::find_shape_mut(list, pic).unwrap().kind else { panic!("not a picture") };
        let old = fill.media;
        fill.media = m.media;
        doc.media.iter_mut().find(|x| x.id == old).unwrap().id = m.media;
        Ok(())
    })
    .unwrap();
    assert_ne!(s.doc().unwrap().doc.media(m.media).unwrap().data, glb);
    let r = s.execute("edit.paste", &json!({})).unwrap();
    let pasted = ShapeId(r["ids"][0].as_u64().unwrap() as u32);
    let (m2, _) = model(&s, pasted);
    assert_ne!(m2.media, m.media);
    assert_eq!(s.doc().unwrap().doc.media(m2.media).unwrap().data, glb);
}

/// An orthographic camera without AutoFit keeps the model's size on the slide through PPTX
/// (which then has no `objViewport`, so the view is folded into the camera's size).
#[test]
fn an_orthographic_model_keeps_its_size_through_pptx() {
    let mut s = session();
    let id = insert_box(&mut s, [-0.5; 3], [0.5; 3]);
    s.edit(|doc, sel| {
        let list = crate::shapes_mut(doc, sel).unwrap();
        if let ShapeKind::Model3d(m) = &mut deckcraft_model::find_shape_mut(list, id).unwrap().kind {
            m.camera.fov = 0.0;
            m.camera.ortho = 1.5;
            m.auto_fit = false;
            m.viewport = 1.7;
        }
        Ok(())
    })
    .unwrap();
    let (m, x) = model(&s, id);
    let doc = s.doc().unwrap().doc.clone();
    let bytes = crate::cmd::file::save_bytes(&doc, "pptx").unwrap();
    let back = crate::cmd::file::open_presentation("t.pptx", &bytes).unwrap();
    let sh = back.slides.iter().flat_map(|sl| sl.shapes.iter()).find(|x| matches!(x.kind, ShapeKind::Model3d(_))).unwrap();
    let ShapeKind::Model3d(m2) = &sh.kind else { panic!("not a model") };
    assert!(m2.is_ortho());
    let (f, f2) = (m.focal(x.h), m2.focal(sh.xfrm.unwrap().h));
    assert!(close(f, f2, f * 1e-3), "{f} {f2}");
}

/// A GLB whose buffer is a separate file is stored converted, so the deck holds the whole model.
#[test]
fn a_glb_with_an_external_buffer_is_stored_whole() {
    let mut bin = Vec::new();
    for v in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
        for c in v {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let mut js = json!({
        "asset": {"version": "2.0"},
        "meshes": [{"primitives": [{"attributes": {"POSITION": 0}}]}],
        "accessors": [{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0, 0, 0], "max": [1, 1, 0]}],
        "bufferViews": [{"buffer": 0, "byteLength": 36}],
        "buffers": [{"byteLength": 36, "uri": "tri.bin"}],
    })
    .to_string()
    .into_bytes();
    while !js.len().is_multiple_of(4) {
        js.push(b' ');
    }
    let mut glb = b"glTF".to_vec();
    glb.extend_from_slice(&2u32.to_le_bytes());
    glb.extend_from_slice(&((12 + 8 + js.len()) as u32).to_le_bytes());
    glb.extend_from_slice(&(js.len() as u32).to_le_bytes());
    glb.extend_from_slice(&0x4E4F_534Au32.to_le_bytes());
    glb.extend_from_slice(&js);
    let mut s = session();
    let r = s.execute("insert.model3d", &json!({"name": "tri.glb", "data": base64_encode(&glb), "files": {"tri.bin": base64_encode(&bin)}})).unwrap();
    let (m, _) = model(&s, ShapeId(r["id"].as_u64().unwrap() as u32));
    let stored = s.doc().unwrap().doc.media(m.media).unwrap().data.clone();
    assert_ne!(stored.as_slice(), glb.as_slice(), "converted, not kept as it was");
    let scene = deckcraft_mesh::load("tri.glb", &stored, &deckcraft_mesh::no_files).unwrap();
    assert_eq!(scene.triangle_count(), 1);
}

/// A link inside the model's folder that leads out of it is not followed: the texture behind it
/// stays on the disk. (Skipped where this account can't make links.)
#[test]
fn companions_do_not_follow_links_out_of_the_folder() {
    let tmp = TempDir::new("model3d-link");
    let dir = tmp.0.clone();
    let (sub, outside) = (dir.join("model"), dir.join("elsewhere"));
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 200, 10, 255])).save(outside.join("t.png")).unwrap();
    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(&outside, sub.join("link"));
    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_dir(&outside, sub.join("link"));
    // The folder is new, so a failure is the account not being allowed to make links.
    if let Err(e) = linked {
        eprintln!("skipped: can't make a link here ({e})");
        return;
    }
    std::fs::write(sub.join("m.mtl"), "newmtl A\nmap_Kd link/t.png\n").unwrap();
    std::fs::write(sub.join("m.obj"), "mtllib m.mtl\nv 0 0 0\nv 1 0 0\nv 1 1 0\nvt 0 0\nvt 1 0\nvt 1 1\nusemtl A\nf 1/1 2/2 3/3\n").unwrap();
    let mut s = session();
    let r = s.execute("insert.model3d", &json!({"path": sub.join("m.obj").to_string_lossy()})).unwrap();
    assert_eq!(r["textures"], 0);
}

/// Insert says which of the files it was given the model used.
#[test]
fn insert_names_the_companions_it_used() {
    let mtl = "newmtl A\nKd 1 0 0\n";
    let obj = "mtllib m.mtl\nv 0 0 0\nv 1 0 0\nv 1 1 0\nusemtl A\nf 1 2 3\n";
    let mut s = session();
    let r = s
        .execute(
            "insert.model3d",
            &json!({"name": "m.obj", "data": base64_encode(obj.as_bytes()), "files": {"m.mtl": base64_encode(mtl.as_bytes()), "photo.png": base64_encode(b"not used")}}),
        )
        .unwrap();
    assert_eq!(r["companions"], json!(["m.mtl"]));
}

/// On a small model the side resize handles lie inside the 3-D handle's zone: they still
/// resize, and the middle still turns the model.
#[test]
fn a_small_models_side_handles_resize() {
    let mut s = session();
    let r = s
        .execute("insert.model3d", &json!({"name": "box.glb", "data": base64_encode(&glb([-0.5; 3], [0.5; 3])), "rect": [100, 100, 16, 16]}))
        .unwrap();
    let id = ShapeId(r["id"].as_u64().unwrap() as u32);
    let x = model(&s, id).1;
    let tol = 3.0;
    let (pts, _) = crate::tools::box_handles(&x);
    // The east handle is inside the 3-D handle's reach from the centre.
    assert!((pts[3] - x.center()).hypot() <= tol * crate::tools::ROTATE3D_RADIUS);
    assert_eq!(s.hit_test(pts[3], tol), Some(Hit::Resize { id, handle: 3 }));
    assert_eq!(s.hit_test(x.center(), tol), Some(Hit::Rotate3d { id }));
}
