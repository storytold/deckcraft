//! Scene → glTF 2.0 binary (`.glb`), the form PowerPoint keeps 3-D models in.
//!
//! One node, one mesh with a primitive per material, metallic-roughness materials, and the textures
//! embedded (PNG and JPEG as they were, other image types re-encoded as PNG).

use serde_json::{Value, json};

use crate::{Alpha, Scene, Wrap};

#[derive(Default)]
struct Bin {
    data: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}

impl Bin {
    fn view(&mut self, bytes: &[u8], target: Option<u32>) -> usize {
        while !self.data.len().is_multiple_of(4) {
            self.data.push(0);
        }
        let mut v = json!({"buffer": 0, "byteOffset": self.data.len(), "byteLength": bytes.len()});
        if let (Some(t), Some(o)) = (target, v.as_object_mut()) {
            o.insert("target".into(), json!(t));
        }
        self.data.extend_from_slice(bytes);
        self.views.push(v);
        self.views.len() - 1
    }

    fn floats<const N: usize>(&mut self, items: &[[f32; N]], kind: &str, bounds: bool) -> usize {
        let mut bytes = Vec::with_capacity(items.len() * N * 4);
        for it in items {
            for c in it {
                bytes.extend_from_slice(&c.to_le_bytes());
            }
        }
        let view = self.view(&bytes, Some(34962));
        let mut a = json!({"bufferView": view, "componentType": 5126, "count": items.len(), "type": kind});
        if bounds && let Some(o) = a.as_object_mut() {
            let lo: Vec<f32> = (0..N).map(|k| items.iter().map(|v| v[k]).fold(f32::INFINITY, f32::min)).collect();
            let hi: Vec<f32> = (0..N).map(|k| items.iter().map(|v| v[k]).fold(f32::NEG_INFINITY, f32::max)).collect();
            o.insert("min".into(), json!(lo));
            o.insert("max".into(), json!(hi));
        }
        self.accessors.push(a);
        self.accessors.len() - 1
    }

    fn indices(&mut self, idx: &[u32]) -> usize {
        let mut bytes = Vec::with_capacity(idx.len() * 4);
        for i in idx {
            bytes.extend_from_slice(&i.to_le_bytes());
        }
        let view = self.view(&bytes, Some(34963));
        self.accessors.push(json!({"bufferView": view, "componentType": 5125, "count": idx.len(), "type": "SCALAR"}));
        self.accessors.len() - 1
    }
}

fn wrap(w: Wrap) -> u32 {
    match w {
        Wrap::Repeat => 10497,
        Wrap::Clamp => 33071,
        Wrap::Mirror => 33648,
    }
}

/// PNG or JPEG bytes for a texture, with the MIME type.
fn image_bytes(t: &crate::Texture) -> Option<(Vec<u8>, &'static str)> {
    match t.mime.as_str() {
        "image/png" => return Some((t.data.to_vec(), "image/png")),
        "image/jpeg" => return Some((t.data.to_vec(), "image/jpeg")),
        _ => {}
    }
    // Another type: the full-size original as PNG, or the drawing copy when that can't be decoded.
    let img =
        image::load_from_memory(&t.data).ok().map(|i| i.to_rgba8()).or_else(|| image::RgbaImage::from_raw(t.width, t.height, t.rgba.clone()))?;
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).ok()?;
    Some((out, "image/png"))
}

/// The scene as a `.glb` file.
pub fn to_glb(scene: &Scene) -> Vec<u8> {
    let mut bin = Bin::default();
    let mut images = Vec::new();
    let mut textures = Vec::new();
    let mut samplers = Vec::new();
    // Scene texture index → glTF texture index.
    let mut tex_map: Vec<Option<usize>> = Vec::new();
    for t in &scene.textures {
        match image_bytes(t) {
            Some((bytes, mime)) => {
                let view = bin.view(&bytes, None);
                images.push(json!({"bufferView": view, "mimeType": mime}));
                samplers.push(json!({"magFilter": 9729, "minFilter": 9987, "wrapS": wrap(t.wrap_s), "wrapT": wrap(t.wrap_t)}));
                textures.push(json!({"sampler": samplers.len() - 1, "source": images.len() - 1}));
                tex_map.push(Some(textures.len() - 1));
            }
            None => tex_map.push(None),
        }
    }
    let mut materials: Vec<Value> = scene
        .materials
        .iter()
        .map(|m| {
            let mut pbr = json!({
                "baseColorFactor": m.base_color,
                "metallicFactor": m.metallic,
                "roughnessFactor": m.roughness,
            });
            if let (Some(t), Some(o)) = (m.texture.and_then(|t| tex_map.get(t).copied().flatten()), pbr.as_object_mut()) {
                o.insert("baseColorTexture".into(), json!({"index": t}));
            }
            let mut v = json!({"pbrMetallicRoughness": pbr, "doubleSided": m.double_sided});
            if let Some(o) = v.as_object_mut() {
                if !m.name.is_empty() {
                    o.insert("name".into(), json!(m.name));
                }
                if m.emissive.iter().any(|c| *c > 0.0) {
                    o.insert("emissiveFactor".into(), json!(m.emissive.map(|c| c.min(1.0))));
                }
                match m.alpha {
                    Alpha::Opaque => {}
                    Alpha::Mask(c) => {
                        o.insert("alphaMode".into(), json!("MASK"));
                        o.insert("alphaCutoff".into(), json!(c));
                    }
                    Alpha::Blend => {
                        o.insert("alphaMode".into(), json!("BLEND"));
                    }
                }
                if m.unlit {
                    o.insert("extensions".into(), json!({"KHR_materials_unlit": {}}));
                }
            }
            v
        })
        .collect();
    // Primitives without a material get the plain grey rather than glTF's metallic default.
    let mut default_mat: Option<usize> = None;
    let mut prims = Vec::new();
    for p in &scene.primitives {
        if p.indices.is_empty() || p.positions.is_empty() {
            continue;
        }
        let mut attrs = serde_json::Map::new();
        attrs.insert("POSITION".into(), json!(bin.floats(&p.positions, "VEC3", true)));
        // glTF wants unit normals: a primitive with any zero normal is written without normals
        // (readers draw it flat).
        if !p.normals.is_empty() && p.normals.iter().all(|n| (n[0] * n[0] + n[1] * n[1] + n[2] * n[2] - 1.0).abs() < 0.01) {
            attrs.insert("NORMAL".into(), json!(bin.floats(&p.normals, "VEC3", false)));
        }
        if !p.uvs.is_empty() {
            attrs.insert("TEXCOORD_0".into(), json!(bin.floats(&p.uvs, "VEC2", false)));
        }
        if !p.colors.is_empty() {
            attrs.insert("COLOR_0".into(), json!(bin.floats(&p.colors, "VEC4", false)));
        }
        let ind = bin.indices(&p.indices);
        let mat = match p.material.filter(|m| *m < materials.len()) {
            Some(m) => m,
            None => *default_mat.get_or_insert_with(|| {
                let g = crate::Material::default_grey();
                materials.push(json!({"name": "Default", "pbrMetallicRoughness": {"baseColorFactor": g.base_color, "metallicFactor": 0.0, "roughnessFactor": g.roughness}, "doubleSided": true}));
                materials.len() - 1
            }),
        };
        prims.push(json!({"attributes": attrs, "indices": ind, "material": mat}));
    }
    while !bin.data.len().is_multiple_of(4) {
        bin.data.push(0);
    }
    let mut doc = json!({
        "asset": {"version": "2.0", "generator": "DeckCraft"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0, "name": "Model"}],
        "meshes": [{"name": "Model", "primitives": prims}],
        "materials": materials,
        "accessors": bin.accessors,
        "bufferViews": bin.views,
        "buffers": [{"byteLength": bin.data.len()}],
    });
    if let Some(o) = doc.as_object_mut() {
        if !images.is_empty() {
            o.insert("images".into(), json!(images));
            o.insert("textures".into(), json!(textures));
            o.insert("samplers".into(), json!(samplers));
        }
        if scene.materials.iter().any(|m| m.unlit) {
            o.insert("extensionsUsed".into(), json!(["KHR_materials_unlit"]));
        }
    }
    let mut js = serde_json::to_vec(&doc).unwrap_or_default();
    while !js.len().is_multiple_of(4) {
        js.push(b' ');
    }
    let total = 12 + 8 + js.len() + 8 + bin.data.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(js.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x4E4F_534Au32.to_le_bytes());
    out.extend_from_slice(&js);
    out.extend_from_slice(&(bin.data.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x004E_4942u32.to_le_bytes());
    out.extend_from_slice(&bin.data);
    out
}
