use std::io::Write as _;

use super::*;

/// Solid colours of the six faces of the test die, in the 3 × 2 texture atlas: 1, 2, 3 on top.
const FACE: [[u8; 3]; 6] = [[230, 60, 50], [40, 200, 100], [50, 120, 220], [240, 200, 20], [150, 80, 180], [230, 120, 30]];

fn atlas_png() -> Vec<u8> {
    let mut img = image::RgbaImage::new(48, 32);
    for (x, y, p) in img.enumerate_pixels_mut() {
        let face = (y / 16 * 3 + x / 16) as usize;
        let c = FACE[face];
        *p = image::Rgba([c[0], c[1], c[2], 255]);
    }
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
    out
}

/// A unit cube centred on the origin; face value v (1..6) on: +Z 1, −Z 6, +X 2, −X 5, +Y 3, −Y 4.
fn die() -> Scene {
    let faces: [(usize, [f32; 3], [f32; 3], [f32; 3]); 6] = [
        (1, [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        (6, [0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        (2, [1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
        (5, [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
        (3, [0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
        (4, [0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
    ];
    let mut p = Primitive { material: Some(0), ..Default::default() };
    for (v, n, r, u) in faces {
        let (u0, v0) = (((v - 1) % 3) as f32 / 3.0, ((v - 1) / 3) as f32 / 2.0);
        let base = p.positions.len() as u32;
        for (sr, su, tu, tv) in [(-1.0, -1.0, 0.0, 1.0), (1.0, -1.0, 1.0, 1.0), (1.0, 1.0, 1.0, 0.0), (-1.0, 1.0, 0.0, 0.0)] {
            p.positions.push([0, 1, 2].map(|k| 0.5 * (n[k] + sr * r[k] + su * u[k])));
            p.normals.push(n);
            // Inset a little so bilinear lookups stay inside the face's cell.
            p.uvs.push([u0 + (0.1 + 0.8 * tu) / 3.0, v0 + (0.1 + 0.8 * tv) / 2.0]);
        }
        p.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    let tex = Texture::decode(atlas_png()).unwrap();
    Scene { primitives: vec![p], materials: vec![Material { texture: Some(0), double_sided: false, ..Default::default() }], textures: vec![tex] }
}

fn camera(dist: f64, size: u32) -> Camera {
    // Looking at the origin from +Z; 45° field of view over the image height.
    let focal = size as f64 / 2.0 / (22.5f64).to_radians().tan();
    Camera {
        model_view: [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, -dist], [0.0, 0.0, 0.0, 1.0]],
        focal,
        ortho: false,
        center: (size as f64 / 2.0, size as f64 / 2.0),
    }
}

fn lights() -> Vec<Light> {
    vec![
        Light { kind: LightKind::Ambient, color: [0.4; 3], pos: [0.0; 3], dir: [0.0; 3] },
        Light { kind: LightKind::Directional, color: [2.5; 3], pos: [0.0; 3], dir: [0.0, 0.0, -1.0] },
    ]
}

fn px(r: &Raster, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * r.width + x) * 4) as usize;
    [r.pixels[i], r.pixels[i + 1], r.pixels[i + 2], r.pixels[i + 3]]
}

/// The dominant channel of a pixel: 0 red, 1 green, 2 blue.
fn hue(p: [u8; 4]) -> usize {
    (0..3).max_by_key(|k| p[*k]).unwrap()
}

#[test]
fn glb_round_trip_keeps_geometry_texture_and_material() {
    let s = die();
    let glb = to_glb(&s);
    let back = load("die.glb", &glb, &no_files).unwrap();
    assert_eq!(back.vertex_count(), 24);
    assert_eq!(back.triangle_count(), 12);
    assert_eq!(back.textures.len(), 1);
    assert_eq!(back.textures[0].mime, "image/png");
    let p = &back.primitives[0];
    assert_eq!(p.uvs.len(), 24);
    assert_eq!(p.normals.len(), 24);
    assert_eq!(back.materials[p.material.unwrap()].texture, Some(0));
    let (lo, hi) = back.bounds().unwrap();
    assert_eq!((lo, hi), ([-0.5; 3], [0.5; 3]));
}

#[test]
fn render_shows_the_front_face_with_its_texture() {
    let s = die();
    let r = render(&s, &camera(3.0, 64), &lights(), 64, 64, 2);
    assert_eq!((r.width, r.height), (64, 64));
    // The front face (value 1, red) fills the centre; the corners are empty.
    let c = px(&r, 32, 32);
    assert_eq!(c[3], 255);
    assert_eq!(hue(c), 0, "{c:?}");
    assert_eq!(px(&r, 1, 1)[3], 0);
    // Turned −90° about Y the right face (2, green) comes to the front.
    let mut cam = camera(3.0, 64);
    cam.model_view = [[0.0, 0.0, -1.0, 0.0], [0.0, 1.0, 0.0, 0.0], [1.0, 0.0, 0.0, -3.0], [0.0, 0.0, 0.0, 1.0]];
    let r = render(&s, &cam, &lights(), 64, 64, 1);
    assert_eq!(hue(px(&r, 32, 32)), 1);
}

#[test]
fn texture_top_is_up() {
    // The atlas's top row holds 1, 2, 3: the top face (3, blue) is in the top row, the bottom (4,
    // yellow) in the bottom row; looking from above shows blue.
    let s = die();
    let mut cam = camera(3.0, 32);
    // Turn the model 90° about X so its top faces the camera.
    cam.model_view = [[1.0, 0.0, 0.0, 0.0], [0.0, 0.0, -1.0, 0.0], [0.0, 1.0, 0.0, -3.0], [0.0, 0.0, 0.0, 1.0]];
    let r = render(&s, &cam, &lights(), 32, 32, 1);
    assert_eq!(hue(px(&r, 16, 16)), 2);
}

#[test]
fn back_faces_of_one_sided_materials_are_culled() {
    let mut s = die();
    // Only the back face: seen from the front it is turned away.
    let p = &mut s.primitives[0];
    p.indices = p.indices[6..12].to_vec();
    let r = render(&s, &camera(3.0, 32), &lights(), 32, 32, 1);
    assert_eq!(px(&r, 16, 16)[3], 0);
    s.materials[0].double_sided = true;
    let r = render(&s, &camera(3.0, 32), &lights(), 32, 32, 1);
    assert_eq!(px(&r, 16, 16)[3], 255);
}

#[test]
fn extent_measures_the_silhouette() {
    let s = die();
    let mv = camera(3.0, 32).model_view;
    let (hx, hy) = extent(&s, &mv, false).unwrap();
    // The front face is nearest: 0.5 / 2.5.
    assert!((hx - 0.2).abs() < 1e-6 && (hy - 0.2).abs() < 1e-6, "{hx} {hy}");
    let (ox, oy) = extent(&s, &mv, true).unwrap();
    assert!((ox - 0.5).abs() < 1e-6 && (oy - 0.5).abs() < 1e-6);
}

#[test]
fn near_plane_cuts_without_trouble() {
    // The camera inside the cube: nothing panics, some samples are drawn.
    let mut s = die();
    s.materials[0].double_sided = true;
    let r = render(&s, &camera(0.2, 32), &lights(), 32, 32, 1);
    assert!(r.pixels.as_chunks::<4>().0.iter().any(|p| p[3] == 255));
}

#[test]
fn huge_requests_render_smaller() {
    let r = render(&die(), &camera(3.0, 10_000), &lights(), 10_000, 10_000, 4);
    assert!(r.width as u64 * r.height as u64 <= 4_000_000);
    assert_eq!(r.width, r.height);
}

const OBJ: &str = "\
# a textured quad and a coloured triangle
mtllib quad.mtl
v -1 -1 0
v 1 -1 0
v 1 1 0
v -1 1 0
v 0 0 1 1 0 0
vt 0 0
vt 1 0
vt 1 1
vt 0 1
vn 0 0 1
usemtl Tex
f 1/1/1 2/2/1 3/3/1 4/4/1
usemtl Plain
f -1 -4 -3
";

const MTL: &str = "\
newmtl Tex
Kd 1 1 1
map_Kd -s 1 1 1 tex.png
newmtl Plain
Kd 0.5 0.5 0.5
d 0.5
";

#[test]
fn obj_reads_polygons_materials_and_companions() {
    let png = atlas_png();
    let resolve = |name: &str| match name {
        "quad.mtl" => Some(MTL.as_bytes().to_vec()),
        "tex.png" => Some(png.clone()),
        _ => None,
    };
    let s = load("quad.obj", OBJ.as_bytes(), &resolve).unwrap();
    assert_eq!(s.triangle_count(), 3);
    assert_eq!(s.textures.len(), 1);
    let tex = s.materials.iter().find(|m| m.name == "Tex").unwrap();
    assert_eq!(tex.texture, Some(0));
    let plain = s.materials.iter().find(|m| m.name == "Plain").unwrap();
    assert_eq!(plain.alpha, Alpha::Blend);
    assert!((plain.base_color[0] - srgb_to_linear(0.5)).abs() < 1e-4);
    // OBJ texture v runs up: (0, 0) is the image's bottom-left, glTF's (0, 1).
    let q = s.primitives.iter().find(|p| !p.uvs.is_empty()).unwrap();
    assert!(q.uvs.contains(&[0.0, 1.0]));
    // Vertex colours: the fifth vertex is red, the others white.
    let t = s.primitives.iter().find(|p| p.material.map(|m| s.materials[m].name.as_str()) == Some("Plain")).unwrap();
    assert!(t.colors.iter().any(|c| c[0] > 0.99 && c[1] < 0.01));
    // Without its companions the model still loads, untextured.
    let s = load("quad.obj", OBJ.as_bytes(), &no_files).unwrap();
    assert!(s.textures.is_empty());
}

#[test]
fn stl_ascii_and_binary() {
    let ascii = "solid t\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid t\n";
    let s = load("t.stl", ascii.as_bytes(), &no_files).unwrap();
    assert_eq!(s.triangle_count(), 1);
    let mut bin = vec![0u8; 80];
    // A header starting with "solid" must still read as binary when the size matches.
    bin[..5].copy_from_slice(b"solid");
    bin.extend_from_slice(&2u32.to_le_bytes());
    for t in 0..2 {
        bin.extend_from_slice(&[0u8; 12]);
        for v in [[0.0f32, 0.0, t as f32], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
            for c in v {
                bin.extend_from_slice(&c.to_le_bytes());
            }
        }
        bin.extend_from_slice(&[0, 0]);
    }
    let s = load("b.stl", &bin, &no_files).unwrap();
    assert_eq!(s.triangle_count(), 2);
    assert_eq!(s.materials.len(), 1);
}

#[test]
fn ply_ascii_and_binary_with_colours() {
    let ascii = "ply\nformat ascii 1.0\nelement vertex 4\nproperty float x\nproperty float y\nproperty float z\nproperty uchar red\nproperty uchar green\nproperty uchar blue\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n0 0 0 255 0 0\n1 0 0 255 0 0\n1 1 0 0 255 0\n0 1 0 0 0 255\n4 0 1 2 3\n";
    let s = load("q.ply", ascii.as_bytes(), &no_files).unwrap();
    assert_eq!(s.triangle_count(), 2);
    let p = &s.primitives[0];
    assert_eq!(p.colors.len(), 4);
    assert!(p.colors[0][0] > 0.99 && p.colors[3][2] > 0.99);
    for (enc, be) in [("binary_little_endian", false), ("binary_big_endian", true)] {
        let mut b = format!("ply\nformat {enc} 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar uint vertex_indices\nend_header\n").into_bytes();
        for v in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
            for c in v {
                b.extend_from_slice(&if be { c.to_be_bytes() } else { c.to_le_bytes() });
            }
        }
        b.push(3);
        for i in 0u32..3 {
            b.extend_from_slice(&if be { i.to_be_bytes() } else { i.to_le_bytes() });
        }
        let s = load("t.ply", &b, &no_files).unwrap();
        assert_eq!(s.triangle_count(), 1, "{enc}");
        assert_eq!(s.bounds().unwrap().1, [1.0, 1.0, 0.0]);
    }
}

fn threemf_zip(model: &str, extra: &[(&str, &[u8])]) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let o = zip::write::SimpleFileOptions::default();
    z.start_file("_rels/.rels", o).unwrap();
    z.write_all(br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Target="/3D/3dmodel.model" Id="r" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/></Relationships>"#).unwrap();
    z.start_file("3D/3dmodel.model", o).unwrap();
    z.write_all(model.as_bytes()).unwrap();
    for (n, b) in extra {
        z.start_file(*n, o).unwrap();
        z.write_all(b).unwrap();
    }
    z.finish().unwrap().into_inner()
}

#[test]
fn threemf_units_axes_materials_and_textures() {
    let model = r##"<?xml version="1.0" encoding="UTF-8"?>
<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02" xmlns:m="http://schemas.microsoft.com/3dmanufacturing/material/2015/02">
<resources>
<basematerials id="1"><base name="Red" displaycolor="#FF0000"/><base name="Blue" displaycolor="#0000FF80"/></basematerials>
<m:texture2d id="2" path="/3D/Textures/t.png" contenttype="image/png" tilestyleu="clamp"/>
<m:texture2dgroup id="3" texid="2"><m:tex2coord u="0" v="0"/><m:tex2coord u="1" v="0"/><m:tex2coord u="0" v="1"/></m:texture2dgroup>
<object id="4" type="model" pid="1" pindex="0"><mesh>
<vertices><vertex x="0" y="0" z="0"/><vertex x="10" y="0" z="0"/><vertex x="0" y="10" z="0"/><vertex x="0" y="0" z="20"/></vertices>
<triangles><triangle v1="0" v2="1" v3="2"/><triangle v1="0" v2="1" v3="3" p1="1"/><triangle v1="0" v2="2" v3="3" pid="3" p1="0" p2="1" p3="2"/></triangles>
</mesh></object>
<object id="5" type="model"><components><component objectid="4" transform="1 0 0 0 1 0 0 0 1 100 0 0"/></components></object>
</resources>
<build><item objectid="5"/></build>
</model>"##;
    let png = atlas_png();
    let s = load("m.3mf", &threemf_zip(model, &[("3D/Textures/t.png", &png)]), &no_files).unwrap();
    assert_eq!(s.triangle_count(), 3);
    // Millimetres to metres, Z up to Y up: z = 20 mm becomes y = 0.02 m; the component moved x by 100 mm.
    let (lo, hi) = s.bounds().unwrap();
    assert!((hi[1] - 0.02).abs() < 1e-6, "{lo:?} {hi:?}");
    assert!((lo[0] - 0.1).abs() < 1e-6);
    assert!((lo[2] + 0.01).abs() < 1e-6);
    assert!(s.materials.iter().any(|m| m.base_color[0] > 0.99 && m.base_color[2] < 0.01));
    assert!(s.materials.iter().any(|m| m.alpha == Alpha::Blend));
    assert_eq!(s.textures.len(), 1);
    assert_eq!(s.textures[0].wrap_s, Wrap::Clamp);
    assert!(s.primitives.iter().any(|p| p.uvs.len() == 3));
}

#[test]
fn gltf_json_with_data_uri_and_node_transforms() {
    // One triangle, scaled ×2 and moved by a parent node, positions in a data: URI.
    let mut b = Vec::new();
    for v in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
        for c in v {
            b.extend_from_slice(&c.to_le_bytes());
        }
    }
    let uri = format!("data:application/octet-stream;base64,{}", b64(&b));
    let doc = serde_json::json!({
        "asset": {"version": "2.0"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"children": [1], "translation": [10, 0, 0]}, {"mesh": 0, "scale": [2, 2, 2]}],
        "meshes": [{"primitives": [{"attributes": {"POSITION": 0}}]}],
        "accessors": [{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3"}],
        "bufferViews": [{"buffer": 0, "byteLength": 36}],
        "buffers": [{"byteLength": 36, "uri": uri}],
    });
    let s = load("t.gltf", doc.to_string().as_bytes(), &no_files).unwrap();
    assert_eq!(s.bounds().unwrap(), ([10.0, 0.0, 0.0], [12.0, 2.0, 0.0]));
}

fn b64(d: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for c in d.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for (i, sh) in [18, 12, 6, 0].iter().enumerate() {
            s.push(if i <= c.len() { A[((n >> sh) & 63) as usize] as char } else { '=' });
        }
    }
    s
}

#[test]
fn sniffing_by_content_and_name() {
    assert_eq!(Format::sniff("x.bin", b"glTF\x02\0\0\0"), Some(Format::Glb));
    assert_eq!(Format::sniff("x", b"ply\nformat ascii 1.0\n"), Some(Format::Ply));
    assert_eq!(Format::sniff("x.3mf", b"PK\x03\x04"), Some(Format::ThreeMf));
    assert_eq!(Format::sniff("X.OBJ", b"v 0 0 0"), Some(Format::Obj));
    assert_eq!(Format::sniff("x", b"v 0 0 0\nf 1 1 1\n"), Some(Format::Obj));
    assert_eq!(Format::sniff("notes.txt", b"hello"), None);
    assert!(!is_model("photo.png", b"\x89PNG"));
}

/// Deterministic pseudo-random bytes.
fn noise(seed: u64, n: usize) -> Vec<u8> {
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect()
}

#[test]
fn hostile_input_never_panics() {
    let glb = to_glb(&die());
    let png = atlas_png();
    let tmf = threemf_zip(
        "<model><resources><object id=\"1\"><mesh><vertices><vertex x=\"0\" y=\"0\" z=\"0\"/></vertices><triangles><triangle v1=\"0\" v2=\"7\" v3=\"99999999\"/></triangles></mesh></object></resources><build><item objectid=\"1\"/></build></model>",
        &[],
    );
    assert!(load("x.3mf", &tmf, &no_files).is_err());
    let seeds: Vec<(&str, Vec<u8>)> = vec![
        ("a.glb", glb.clone()),
        ("a.obj", OBJ.as_bytes().to_vec()),
        ("a.ply", b"ply\nformat binary_little_endian 1.0\nelement vertex 4000000000\nproperty float x\nend_header\n".to_vec()),
        ("a.stl", b"solid x\nfacet\nouter loop\nvertex 1 2\n".to_vec()),
        ("a.3mf", tmf),
        ("a.png", png),
    ];
    for (name, base) in &seeds {
        // Truncated at many points.
        for cut in (0..base.len()).step_by((base.len() / 97).max(1)) {
            let _ = load(name, base.get(..cut).unwrap(), &no_files);
        }
        // Bytes flipped.
        for seed in 0..40u64 {
            let mut b = base.clone();
            let r = noise(seed, 8);
            for k in 0..4 {
                if !b.is_empty() {
                    let at = (r[k] as usize * 131 + seed as usize * 7919) % b.len();
                    b[at] ^= r[k + 4] | 1;
                }
            }
            let _ = load(name, &b, &no_files);
        }
    }
    for seed in 0..50u64 {
        let b = noise(seed, 300);
        for name in ["n.glb", "n.obj", "n.stl", "n.ply", "n.3mf", "n.gltf"] {
            let _ = load(name, &b, &no_files);
        }
    }
    // A GLB whose accessor claims far more data than its buffer holds.
    let doc = serde_json::json!({
        "asset": {"version": "2.0"},
        "meshes": [{"primitives": [{"attributes": {"POSITION": 0}}]}],
        "nodes": [{"mesh": 0}],
        "accessors": [{"bufferView": 0, "componentType": 5126, "count": 9_000_000, "type": "VEC3"}],
        "bufferViews": [{"buffer": 0, "byteLength": 12}],
        "buffers": [{"byteLength": 12, "uri": "data:;base64,AAAAAAAAAAAAAAAA"}],
    });
    assert!(load("x.gltf", doc.to_string().as_bytes(), &no_files).is_err());
}

#[test]
fn empty_and_unknown_files_are_errors() {
    assert_eq!(load("x.stl", b"solid x\nendsolid x\n", &no_files).unwrap_err(), Error::Format("STL without facets".into()));
    assert!(matches!(load("x.txt", b"hello", &no_files), Err(Error::Format(_))));
    assert!(matches!(load("x.glb", b"glTF\x01\0\0\0\x0c\0\0\0", &no_files), Err(Error::Unsupported(_))));
}

/// Two textures sharing one image keep their own samplers; a texture no material uses (here with
/// an image that can't be read) is never decoded and leaves nothing behind.
#[test]
fn gltf_textures_share_an_image_and_keep_their_samplers() {
    let mut b = Vec::new();
    for v in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
        for c in v {
            b.extend_from_slice(&c.to_le_bytes());
        }
    }
    let png = atlas_png();
    let doc = serde_json::json!({
        "asset": {"version": "2.0"},
        "meshes": [{"primitives": [{"attributes": {"POSITION": 0}, "material": 0}, {"attributes": {"POSITION": 0}, "material": 1}]}],
        "accessors": [{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3"}],
        "bufferViews": [{"buffer": 0, "byteLength": 36}],
        "buffers": [{"byteLength": 36, "uri": format!("data:application/octet-stream;base64,{}", b64(&b))}],
        "images": [{"uri": format!("data:image/png;base64,{}", b64(&png))}, {"uri": "data:image/png;base64,AAAA"}],
        "samplers": [{"wrapS": 33071, "wrapT": 33648}, {}],
        "textures": [{"source": 0, "sampler": 0}, {"source": 0, "sampler": 1}, {"source": 1}],
        "materials": [
            {"pbrMetallicRoughness": {"baseColorTexture": {"index": 1}}},
            {"pbrMetallicRoughness": {"baseColorTexture": {"index": 0}}},
        ],
    });
    let s = load("t.gltf", doc.to_string().as_bytes(), &no_files).unwrap();
    assert_eq!(s.textures.len(), 2);
    assert_eq!(s.materials[0].texture, Some(0));
    assert_eq!(s.materials[1].texture, Some(1));
    assert_eq!((s.textures[0].wrap_s, s.textures[0].wrap_t), (Wrap::Repeat, Wrap::Repeat));
    assert_eq!((s.textures[1].wrap_s, s.textures[1].wrap_t), (Wrap::Clamp, Wrap::Mirror));
}

/// A texture with no sampler of its own repeats, even when its image was first decoded for a
/// texture that clamps.
#[test]
fn gltf_a_shared_image_does_not_lend_its_sampler() {
    let mut b = Vec::new();
    for v in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
        for c in v {
            b.extend_from_slice(&c.to_le_bytes());
        }
    }
    let doc = serde_json::json!({
        "asset": {"version": "2.0"},
        "meshes": [{"primitives": [{"attributes": {"POSITION": 0}, "material": 0}]}],
        "accessors": [{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3"}],
        "bufferViews": [{"buffer": 0, "byteLength": 36}],
        "buffers": [{"byteLength": 36, "uri": format!("data:application/octet-stream;base64,{}", b64(&b))}],
        "images": [{"uri": format!("data:image/png;base64,{}", b64(&atlas_png()))}],
        "samplers": [{"wrapS": 33071, "wrapT": 33071}],
        "textures": [{"source": 0, "sampler": 0}, {"source": 0}],
        "materials": [
            {"pbrMetallicRoughness": {"baseColorTexture": {"index": 0}}},
            {"pbrMetallicRoughness": {"baseColorTexture": {"index": 1}}},
        ],
    });
    let s = load("t.gltf", doc.to_string().as_bytes(), &no_files).unwrap();
    assert_eq!((s.textures[0].wrap_s, s.textures[0].wrap_t), (Wrap::Clamp, Wrap::Clamp));
    assert_eq!((s.textures[1].wrap_s, s.textures[1].wrap_t), (Wrap::Repeat, Wrap::Repeat));
}
