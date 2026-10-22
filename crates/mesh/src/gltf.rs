//! glTF 2.0 reader: binary (`.glb`) and JSON (`.gltf`) with embedded (`data:`) or companion
//! buffers and images. Khronos glTF 2.0 specification.
//!
//! Read: the default scene's node tree (matrices and TRS), triangle primitives (lists, strips,
//! fans), every accessor component type (normalized integers included), sparse accessors, the
//! metallic-roughness material with its base colour texture, `KHR_materials_pbrSpecularGlossiness`
//! (PowerPoint converts OBJ, STL, PLY and 3MF files to it), `KHR_materials_unlit`,
//! `KHR_mesh_quantization` and `EXT_texture_webp`. Skipped with a log line: points and lines,
//! Draco and meshopt compression, skins, morph targets and animations (the model is drawn at rest).

use std::rc::Rc;

use serde_json::Value;

use crate::util::{base64, percent_decode};
use crate::{Alpha, Error, Material, Primitive, Resolve, Result, Scene, Texture, Wrap, bad};

const MAGIC: u32 = 0x4654_6C67;
const CHUNK_JSON: u32 = 0x4E4F_534A;
const CHUNK_BIN: u32 = 0x004E_4942;
const MAX_NODE_DEPTH: usize = 64;
const MAX_JSON: usize = 64 << 20;

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at.checked_add(4)?).and_then(|s| s.try_into().ok()).map(u32::from_le_bytes)
}

/// A `.glb` file.
pub fn read_glb(bytes: &[u8], resolve: Resolve) -> Result<Scene> {
    if u32_at(bytes, 0) != Some(MAGIC) {
        return Err(bad("not a glTF binary"));
    }
    let version = u32_at(bytes, 4).unwrap_or(0);
    if version != 2 {
        return Err(Error::Unsupported(format!("glTF binary version {version}")));
    }
    let total = (u32_at(bytes, 8).unwrap_or(0) as usize).min(bytes.len());
    let mut at = 12usize;
    let mut json: Option<&[u8]> = None;
    let mut bin: Option<&[u8]> = None;
    while at.saturating_add(8) <= total {
        let len = u32_at(bytes, at).unwrap_or(0) as usize;
        let kind = u32_at(bytes, at + 4).unwrap_or(0);
        let start = at + 8;
        let end = start.checked_add(len).filter(|e| *e <= total).ok_or_else(|| bad("glTF chunk runs past the end"))?;
        let data = bytes.get(start..end).unwrap_or(&[]);
        match kind {
            CHUNK_JSON if json.is_none() => json = Some(data),
            CHUNK_BIN if bin.is_none() => bin = Some(data),
            _ => {}
        }
        // Chunks are 4-byte aligned.
        at = end.checked_add(3).map(|e| e & !3).unwrap_or(usize::MAX);
    }
    let json = json.ok_or_else(|| bad("glTF binary without JSON"))?;
    read_doc(json, bin, resolve)
}

/// Does this `.glb` name files outside itself: a buffer or an image by a URI that isn't `data:`?
pub fn glb_names_files(bytes: &[u8]) -> bool {
    // The JSON chunk comes first (glTF 2.0 § 4.4.3).
    let Some(len) = u32_at(bytes, 12).map(|l| l as usize) else { return false };
    if u32_at(bytes, 0) != Some(MAGIC) || u32_at(bytes, 16) != Some(CHUNK_JSON) || len > MAX_JSON {
        return false;
    }
    let Some(doc) = bytes.get(20..20usize.saturating_add(len)).and_then(|j| serde_json::from_slice::<Value>(j).ok()) else { return false };
    ["buffers", "images"]
        .iter()
        .any(|k| arr(&doc, k).iter().filter_map(|x| x.get("uri").and_then(Value::as_str)).any(|u| !u.trim_start().starts_with("data:")))
}

/// A `.gltf` (JSON) file.
pub fn read_json(bytes: &[u8], resolve: Resolve) -> Result<Scene> {
    read_doc(bytes, None, resolve)
}

fn read_doc(json: &[u8], bin: Option<&[u8]>, resolve: Resolve) -> Result<Scene> {
    if json.len() > MAX_JSON {
        return Err(Error::TooLarge);
    }
    let json = json.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(json);
    let doc: Value = serde_json::from_slice(json).map_err(|e| bad(format!("glTF JSON: {e}")))?;
    let version = doc.pointer("/asset/version").and_then(Value::as_str).unwrap_or("2.0");
    if !version.starts_with('2') {
        return Err(Error::Unsupported(format!("glTF {version}")));
    }
    for ext in arr(&doc, "extensionsRequired") {
        if let Some(e) = ext.as_str()
            && !matches!(
                e,
                "KHR_mesh_quantization"
                    | "KHR_materials_unlit"
                    | "KHR_materials_pbrSpecularGlossiness"
                    | "KHR_texture_transform"
                    | "EXT_texture_webp"
            )
        {
            log::warn!("glTF: required extension {e} not supported; drawing what can be read");
        }
    }
    let mut g = Gltf { doc: &doc, buffers: Vec::new(), resolve, verts: 0, indices: 0 };
    g.load_buffers(bin)?;
    let (materials, textures) = g.materials();
    let mut scene = Scene { materials, textures, ..Default::default() };
    g.nodes(&mut scene)?;
    Ok(scene)
}

fn arr<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get(key).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

fn idx(v: &Value, key: &str) -> Option<usize> {
    v.get(key).and_then(Value::as_u64).and_then(|i| usize::try_from(i).ok())
}

fn f32s<const N: usize>(v: Option<&Value>, default: [f32; N]) -> [f32; N] {
    let Some(a) = v.and_then(Value::as_array) else { return default };
    let mut out = default;
    for (o, x) in out.iter_mut().zip(a.iter()) {
        if let Some(f) = x.as_f64().filter(|f| f.is_finite()) {
            *o = f as f32;
        }
    }
    out
}

fn num(v: Option<&Value>, default: f32) -> f32 {
    v.and_then(Value::as_f64).filter(|f| f.is_finite()).map(|f| f as f32).unwrap_or(default)
}

struct Gltf<'a> {
    doc: &'a Value,
    buffers: Vec<Option<Rc<Vec<u8>>>>,
    resolve: Resolve<'a>,
    verts: usize,
    /// Triangle-list indices made so far, over every primitive.
    indices: usize,
}

/// `data:[mime];base64,...`
fn data_uri(uri: &str) -> Option<Vec<u8>> {
    let rest = uri.strip_prefix("data:")?;
    let (_, b64) = rest.split_once(";base64,")?;
    base64(b64)
}

impl Gltf<'_> {
    fn uri_bytes(&self, uri: &str) -> Option<Vec<u8>> {
        if uri.starts_with("data:") {
            return data_uri(uri);
        }
        if uri.contains("://") {
            log::warn!("glTF: external URL {uri} not fetched");
            return None;
        }
        (self.resolve)(&percent_decode(uri))
    }

    /// Each buffer's bytes; a URI named twice is read once, and all of them together stay
    /// within [`crate::MAX_FILE`].
    fn load_buffers(&mut self, bin: Option<&[u8]>) -> Result<()> {
        let list = arr(self.doc, "buffers");
        let mut out = Vec::with_capacity(list.len().min(1024));
        let mut by_uri: Vec<(&str, Option<Rc<Vec<u8>>>)> = Vec::new();
        let mut total = 0usize;
        for (i, b) in list.iter().enumerate().take(1024) {
            let data = match b.get("uri").and_then(Value::as_str) {
                Some(uri) => match by_uri.iter().find(|(u, _)| *u == uri) {
                    Some((_, d)) => d.clone(),
                    None => {
                        let d = self.uri_bytes(uri).map(Rc::new);
                        total = total.saturating_add(d.as_ref().map_or(0, |d| d.len()));
                        by_uri.push((uri, d.clone()));
                        d
                    }
                },
                None if i == 0 => bin.map(|b| {
                    total = total.saturating_add(b.len());
                    Rc::new(b.to_vec())
                }),
                None => None,
            };
            if total > crate::MAX_FILE {
                return Err(Error::TooLarge);
            }
            if data.is_none() {
                log::warn!("glTF: buffer {i} is missing");
            }
            out.push(data);
        }
        self.buffers = out;
        Ok(())
    }

    /// The bytes of buffer view `i` and its stride.
    fn view(&self, i: usize) -> Option<(&[u8], Option<usize>)> {
        let v = arr(self.doc, "bufferViews").get(i)?;
        let buf = self.buffers.get(idx(v, "buffer")?)?.as_ref()?;
        let off = idx(v, "byteOffset").unwrap_or(0);
        let len = idx(v, "byteLength")?;
        let data = buf.get(off..off.checked_add(len)?)?;
        let stride = idx(v, "byteStride").filter(|s| (4..=252).contains(s));
        Some((data, stride))
    }

    /// Accessor `i` as floats (normalized integers scaled to 0..1 or −1..1), `comps` per element.
    fn floats(&self, i: usize, comps: usize) -> Option<Vec<f32>> {
        let a = arr(self.doc, "accessors").get(i)?;
        let have = match a.get("type").and_then(Value::as_str)? {
            "SCALAR" => 1,
            "VEC2" => 2,
            "VEC3" => 3,
            "VEC4" => 4,
            _ => return None,
        };
        let count = idx(a, "count")?;
        if count > crate::MAX_VERTICES {
            return None;
        }
        let ct = a.get("componentType").and_then(Value::as_u64)?;
        let norm = a.get("normalized").and_then(Value::as_bool).unwrap_or(false);
        let size = comp_size(ct)?;
        let view = match idx(a, "bufferView") {
            Some(vi) => {
                let (data, stride) = self.view(vi)?;
                let off = idx(a, "byteOffset").unwrap_or(0);
                let elem = size.checked_mul(have)?;
                let stride = stride.unwrap_or(elem);
                // The whole accessor must lie in its view (checked before allocating for it).
                if count > 0 && off.checked_add((count - 1).checked_mul(stride)?)?.checked_add(elem)? > data.len() {
                    return None;
                }
                Some((data, off, stride))
            }
            None => None,
        };
        let mut out = vec![0f32; count.checked_mul(comps)?];
        if let Some((data, off, stride)) = view {
            for e in 0..count {
                let base = off.checked_add(e.checked_mul(stride)?)?;
                for c in 0..have.min(comps) {
                    let at = base.checked_add(c * size)?;
                    let v = read_comp(data, at, ct, norm)?;
                    if let Some(o) = out.get_mut(e * comps + c) {
                        *o = v;
                    }
                }
                // A missing alpha is opaque.
                if comps == 4
                    && have == 3
                    && let Some(o) = out.get_mut(e * comps + 3)
                {
                    *o = 1.0;
                }
            }
        }
        if let Some(sp) = a.get("sparse") {
            self.sparse(sp, ct, norm, have, comps, &mut out);
        }
        Some(out)
    }

    fn sparse(&self, sp: &Value, ct: u64, norm: bool, have: usize, comps: usize, out: &mut [f32]) {
        let n = idx(sp, "count").unwrap_or(0);
        let (Some(ix), Some(vals)) = (sp.get("indices"), sp.get("values")) else { return };
        let (Some((idata, _)), Some((vdata, _))) =
            (idx(ix, "bufferView").and_then(|v| self.view(v)), idx(vals, "bufferView").and_then(|v| self.view(v)))
        else {
            return;
        };
        let ict = ix.get("componentType").and_then(Value::as_u64).unwrap_or(5125);
        let (ioff, voff) = (idx(ix, "byteOffset").unwrap_or(0), idx(vals, "byteOffset").unwrap_or(0));
        let (isz, vsz) = (comp_size(ict).unwrap_or(4), comp_size(ct).unwrap_or(4));
        for k in 0..n.min(out.len()) {
            let Some(target) = read_index(idata, ioff.saturating_add(k.saturating_mul(isz)), ict) else { return };
            for c in 0..have.min(comps) {
                let at = voff.saturating_add((k.saturating_mul(have) + c).saturating_mul(vsz));
                if let (Some(v), Some(o)) = (read_comp(vdata, at, ct, norm), out.get_mut((target as usize).saturating_mul(comps) + c)) {
                    *o = v;
                }
            }
        }
    }

    fn indices(&self, i: usize) -> Option<Vec<u32>> {
        let a = arr(self.doc, "accessors").get(i)?;
        let count = idx(a, "count")?;
        if count > crate::MAX_INDICES {
            return None;
        }
        let ct = a.get("componentType").and_then(Value::as_u64)?;
        let size = comp_size(ct)?;
        let (data, stride) = self.view(idx(a, "bufferView")?)?;
        let off = idx(a, "byteOffset").unwrap_or(0);
        let stride = stride.unwrap_or(size);
        if count > 0 && off.checked_add((count - 1).checked_mul(stride)?)?.checked_add(size)? > data.len() {
            return None;
        }
        let mut out = Vec::with_capacity(count);
        for e in 0..count {
            out.push(read_index(data, off.checked_add(e.checked_mul(stride)?)?, ct)?);
        }
        Some(out)
    }

    fn image_bytes(&self, i: usize) -> Option<Vec<u8>> {
        let im = arr(self.doc, "images").get(i)?;
        if let Some(v) = idx(im, "bufferView") {
            return self.view(v).map(|(d, _)| d.to_vec());
        }
        self.uri_bytes(im.get("uri").and_then(Value::as_str)?)
    }

    /// Materials, and the textures they use (renumbered densely). Only textures a material uses
    /// are decoded, each image once however many textures share it; each texture keeps its own
    /// sampler's wrapping.
    fn materials(&self) -> (Vec<Material>, Vec<Texture>) {
        let defs = arr(self.doc, "textures");
        let samplers = arr(self.doc, "samplers");
        let mut used: Vec<Texture> = Vec::new();
        let mut map: Vec<(usize, usize)> = Vec::new();
        // Per image: the first texture made from it (`None`: it couldn't be decoded).
        let mut decoded: Vec<(usize, Option<usize>)> = Vec::new();
        let mut take = |ti: Option<usize>| -> Option<usize> {
            let ti = ti?;
            if let Some((_, n)) = map.iter().find(|(o, _)| *o == ti) {
                return Some(*n);
            }
            if used.len() >= crate::MAX_TEXTURES {
                return None;
            }
            let t = defs.get(ti)?;
            let src = idx(t, "source").or_else(|| t.pointer("/extensions/EXT_texture_webp/source").and_then(Value::as_u64).map(|v| v as usize))?;
            let mut tex = match decoded.iter().find(|(s, _)| *s == src) {
                Some((_, first)) => used.get((*first)?)?.clone(),
                None => {
                    // Failed decodes count too, so a file can't ask for endless attempts.
                    if decoded.len() >= crate::MAX_TEXTURES * 4 {
                        return None;
                    }
                    let img = self.image_bytes(src).and_then(Texture::decode);
                    decoded.push((src, img.is_some().then_some(used.len())));
                    img?
                }
            };
            // This texture's own sampler, not the one of the texture the image came with.
            (tex.wrap_s, tex.wrap_t) = (Wrap::Repeat, Wrap::Repeat);
            if let Some(s) = idx(t, "sampler").and_then(|s| samplers.get(s)) {
                tex.wrap_s = wrap(s.get("wrapS"));
                tex.wrap_t = wrap(s.get("wrapT"));
            }
            used.push(tex);
            map.push((ti, used.len() - 1));
            Some(used.len() - 1)
        };
        let mats = arr(self.doc, "materials")
            .iter()
            .take(4096)
            .map(|m| {
                let pbr = m.get("pbrMetallicRoughness");
                let sg = m.pointer("/extensions/KHR_materials_pbrSpecularGlossiness");
                let mut out = Material {
                    name: m.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                    base_color: f32s(pbr.and_then(|p| p.get("baseColorFactor")), [1.0; 4]),
                    texture: take(pbr.and_then(|p| p.pointer("/baseColorTexture/index")).and_then(Value::as_u64).map(|v| v as usize)),
                    metallic: num(pbr.and_then(|p| p.get("metallicFactor")), 1.0),
                    roughness: num(pbr.and_then(|p| p.get("roughnessFactor")), 1.0),
                    emissive: f32s(m.get("emissiveFactor"), [0.0; 3]),
                    double_sided: m.get("doubleSided").and_then(Value::as_bool).unwrap_or(false),
                    alpha: match m.get("alphaMode").and_then(Value::as_str) {
                        Some("MASK") => Alpha::Mask(num(m.get("alphaCutoff"), 0.5)),
                        Some("BLEND") => Alpha::Blend,
                        _ => Alpha::Opaque,
                    },
                    unlit: m.pointer("/extensions/KHR_materials_unlit").is_some(),
                };
                if let Some(sg) = sg {
                    out.base_color = f32s(sg.get("diffuseFactor"), [1.0; 4]);
                    out.texture = take(sg.pointer("/diffuseTexture/index").and_then(Value::as_u64).map(|v| v as usize));
                    out.metallic = 0.0;
                    out.roughness = 1.0 - num(sg.get("glossinessFactor"), 1.0);
                }
                out
            })
            .collect();
        (mats, used)
    }

    /// Which texture coordinate set a material's colour texture uses.
    fn uv_set(&self, material: Option<usize>) -> usize {
        let Some(m) = material.and_then(|m| arr(self.doc, "materials").get(m)) else { return 0 };
        m.pointer("/pbrMetallicRoughness/baseColorTexture/texCoord")
            .or_else(|| m.pointer("/extensions/KHR_materials_pbrSpecularGlossiness/diffuseTexture/texCoord"))
            .and_then(Value::as_u64)
            .map(|v| v.min(8) as usize)
            .unwrap_or(0)
    }

    fn nodes(&mut self, scene: &mut Scene) -> Result<()> {
        let nodes = arr(self.doc, "nodes");
        let scenes = arr(self.doc, "scenes");
        let si = idx(self.doc, "scene").unwrap_or(0);
        let roots: Vec<usize> = match scenes.get(si) {
            Some(s) => arr(s, "nodes").iter().filter_map(Value::as_u64).map(|v| v as usize).collect(),
            None => {
                // No scene: every node that is nobody's child.
                let mut child = vec![false; nodes.len()];
                for n in nodes {
                    for c in arr(n, "children").iter().filter_map(Value::as_u64) {
                        if let Some(f) = child.get_mut(c as usize) {
                            *f = true;
                        }
                    }
                }
                (0..nodes.len()).filter(|i| !child.get(*i).copied().unwrap_or(true)).collect()
            }
        };
        if roots.is_empty() && nodes.is_empty() {
            // Meshes without nodes: draw each once, untransformed.
            for m in 0..arr(self.doc, "meshes").len() {
                self.mesh(m, &IDENTITY, scene)?;
            }
            return Ok(());
        }
        let mut seen = vec![false; nodes.len()];
        for r in roots {
            self.node(r, &IDENTITY, 0, &mut seen, scene)?;
        }
        Ok(())
    }

    fn node(&mut self, i: usize, parent: &M4, depth: usize, seen: &mut [bool], scene: &mut Scene) -> Result<()> {
        if depth > MAX_NODE_DEPTH {
            return Ok(());
        }
        match seen.get_mut(i) {
            Some(s) if !*s => *s = true,
            _ => return Ok(()),
        }
        let Some(n) = arr(self.doc, "nodes").get(i) else { return Ok(()) };
        let m = mul(parent, &local(n));
        if let Some(mesh) = idx(n, "mesh") {
            if n.get("skin").is_some() {
                log::info!("glTF: skinned mesh drawn in its rest pose");
            }
            self.mesh(mesh, &m, scene)?;
        }
        let children: Vec<usize> = arr(n, "children").iter().filter_map(Value::as_u64).map(|v| v as usize).collect();
        for c in children {
            self.node(c, &m, depth + 1, seen, scene)?;
        }
        Ok(())
    }

    fn mesh(&mut self, mi: usize, m: &M4, scene: &mut Scene) -> Result<()> {
        let Some(mesh) = arr(self.doc, "meshes").get(mi) else { return Ok(()) };
        for p in arr(mesh, "primitives") {
            let mode = p.get("mode").and_then(Value::as_u64).unwrap_or(4);
            if !matches!(mode, 4..=6) {
                continue;
            }
            if p.pointer("/extensions/KHR_draco_mesh_compression").is_some() {
                log::warn!("glTF: Draco-compressed primitive skipped");
                continue;
            }
            let attrs = p.get("attributes");
            let Some(pos_i) = attrs.and_then(|a| idx(a, "POSITION")) else { continue };
            let Some(pos) = self.floats(pos_i, 3) else { continue };
            let n = pos.len() / 3;
            self.verts = self.verts.saturating_add(n);
            if self.verts > crate::MAX_VERTICES {
                return Err(Error::TooLarge);
            }
            let material = idx(p, "material");
            let uv_key = format!("TEXCOORD_{}", self.uv_set(material));
            let normals = attrs.and_then(|a| idx(a, "NORMAL")).and_then(|i| self.floats(i, 3)).filter(|v| v.len() == n * 3);
            let uvs = attrs.and_then(|a| idx(a, &uv_key)).and_then(|i| self.floats(i, 2)).filter(|v| v.len() == n * 2);
            let colors = attrs.and_then(|a| idx(a, "COLOR_0")).and_then(|i| self.floats(i, 4)).filter(|v| v.len() == n * 4);
            let raw = match idx(p, "indices") {
                Some(i) => self.indices(i).unwrap_or_default(),
                None => (0..n as u32).collect(),
            };
            // What `triangles` will make of them, counted before it allocates.
            let made = match mode {
                5 | 6 => raw.len().saturating_sub(2).saturating_mul(3),
                _ => raw.len() / 3 * 3,
            };
            self.indices = self.indices.saturating_add(made);
            if self.indices > crate::MAX_INDICES {
                return Err(Error::TooLarge);
            }
            let flip = det3(m) < 0.0;
            let indices = triangles(&raw, mode, flip);
            let nm = normal_matrix(m);
            let prim = Primitive {
                positions: pos.as_chunks::<3>().0.iter().map(|c| xform_point(m, *c)).collect(),
                normals: normals.map(|v| v.as_chunks::<3>().0.iter().map(|c| xform_normal(&nm, *c)).collect()).unwrap_or_default(),
                uvs: uvs.map(|v| v.as_chunks::<2>().0.to_vec()).unwrap_or_default(),
                colors: colors.map(|v| v.as_chunks::<4>().0.to_vec()).unwrap_or_default(),
                indices,
                material,
            };
            scene.primitives.push(prim);
        }
        Ok(())
    }
}

fn wrap(v: Option<&Value>) -> Wrap {
    match v.and_then(Value::as_u64) {
        Some(33071) => Wrap::Clamp,
        Some(33648) => Wrap::Mirror,
        _ => Wrap::Repeat,
    }
}

fn comp_size(ct: u64) -> Option<usize> {
    Some(match ct {
        5120 | 5121 => 1,
        5122 | 5123 => 2,
        5125 | 5126 => 4,
        _ => return None,
    })
}

fn read_comp(d: &[u8], at: usize, ct: u64, norm: bool) -> Option<f32> {
    let b = |n: usize| d.get(at..at.checked_add(n)?);
    Some(match ct {
        5120 => {
            let v = *b(1)?.first()? as i8 as f32;
            if norm { (v / 127.0).max(-1.0) } else { v }
        }
        5121 => {
            let v = *b(1)?.first()? as f32;
            if norm { v / 255.0 } else { v }
        }
        5122 => {
            let v = i16::from_le_bytes(b(2)?.try_into().ok()?) as f32;
            if norm { (v / 32767.0).max(-1.0) } else { v }
        }
        5123 => {
            let v = u16::from_le_bytes(b(2)?.try_into().ok()?) as f32;
            if norm { v / 65535.0 } else { v }
        }
        5125 => u32::from_le_bytes(b(4)?.try_into().ok()?) as f32,
        5126 => f32::from_le_bytes(b(4)?.try_into().ok()?),
        _ => return None,
    })
}

fn read_index(d: &[u8], at: usize, ct: u64) -> Option<u32> {
    let b = |n: usize| d.get(at..at.checked_add(n)?);
    Some(match ct {
        5121 => *b(1)?.first()? as u32,
        5123 => u16::from_le_bytes(b(2)?.try_into().ok()?) as u32,
        5125 => u32::from_le_bytes(b(4)?.try_into().ok()?),
        _ => return None,
    })
}

/// Triangle list indices for a list (4), strip (5) or fan (6); `flip` reverses the winding (a
/// mirroring node transform).
fn triangles(raw: &[u32], mode: u64, flip: bool) -> Vec<u32> {
    let mut out = Vec::with_capacity(raw.len());
    let mut push = |a: u32, b: u32, c: u32| {
        if flip {
            out.extend_from_slice(&[a, c, b]);
        } else {
            out.extend_from_slice(&[a, b, c]);
        }
    };
    match mode {
        5 => {
            for (i, w) in raw.windows(3).enumerate() {
                if i % 2 == 0 {
                    push(w[0], w[1], w[2]);
                } else {
                    push(w[1], w[0], w[2]);
                }
            }
        }
        6 => {
            if let Some((&first, rest)) = raw.split_first() {
                for w in rest.windows(2) {
                    push(first, w[0], w[1]);
                }
            }
        }
        _ => {
            for [a, b, c] in raw.as_chunks::<3>().0 {
                push(*a, *b, *c);
            }
        }
    }
    out
}

/// Column-major 4×4 matrix, as glTF stores it.
type M4 = [f64; 16];
const IDENTITY: M4 = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

fn mul(a: &M4, b: &M4) -> M4 {
    let mut m = [0.0; 16];
    for c in 0..4 {
        for r in 0..4 {
            m[c * 4 + r] = (0..4).map(|k| a[k * 4 + r] * b[c * 4 + k]).sum();
        }
    }
    if m.iter().all(|v| v.is_finite()) { m } else { *a }
}

/// A node's own transform: `matrix`, or translation · rotation · scale.
fn local(n: &Value) -> M4 {
    if let Some(a) = n.get("matrix").and_then(Value::as_array)
        && a.len() == 16
    {
        let mut m = [0.0; 16];
        for (o, v) in m.iter_mut().zip(a.iter()) {
            *o = v.as_f64().filter(|f| f.is_finite()).unwrap_or(0.0);
        }
        return m;
    }
    let t = f32s(n.get("translation"), [0.0; 3]).map(f64::from);
    let [x, y, z, w] = f32s(n.get("rotation"), [0.0, 0.0, 0.0, 1.0]).map(f64::from);
    let s = f32s(n.get("scale"), [1.0; 3]).map(f64::from);
    let l = (x * x + y * y + z * z + w * w).sqrt();
    let (x, y, z, w) = if l > 1e-12 { (x / l, y / l, z / l, w / l) } else { (0.0, 0.0, 0.0, 1.0) };
    let r = [
        [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - w * z), 2.0 * (x * z + w * y)],
        [2.0 * (x * y + w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - w * x)],
        [2.0 * (x * z - w * y), 2.0 * (y * z + w * x), 1.0 - 2.0 * (x * x + y * y)],
    ];
    let mut m = [0.0; 16];
    for c in 0..3 {
        for row in 0..3 {
            m[c * 4 + row] = r[row][c] * s[c];
        }
    }
    m[12] = t[0];
    m[13] = t[1];
    m[14] = t[2];
    m[15] = 1.0;
    m
}

fn xform_point(m: &M4, p: [f32; 3]) -> [f32; 3] {
    let (x, y, z) = (p[0] as f64, p[1] as f64, p[2] as f64);
    [
        (m[0] * x + m[4] * y + m[8] * z + m[12]) as f32,
        (m[1] * x + m[5] * y + m[9] * z + m[13]) as f32,
        (m[2] * x + m[6] * y + m[10] * z + m[14]) as f32,
    ]
}

fn det3(m: &M4) -> f64 {
    m[0] * (m[5] * m[10] - m[9] * m[6]) - m[4] * (m[1] * m[10] - m[9] * m[2]) + m[8] * (m[1] * m[6] - m[5] * m[2])
}

/// The inverse transpose of the upper 3×3 (row-major), for normals.
fn normal_matrix(m: &M4) -> [[f64; 3]; 3] {
    let a = [[m[0], m[4], m[8]], [m[1], m[5], m[9]], [m[2], m[6], m[10]]];
    let det = det3(m);
    if !det.is_finite() || det.abs() < 1e-30 {
        return a;
    }
    let c = |r0: usize, c0: usize, r1: usize, c1: usize| a[r0][c0] * a[r1][c1] - a[r0][c1] * a[r1][c0];
    // Cofactor matrix = (inverse transpose) · det.
    [[c(1, 1, 2, 2), -c(1, 0, 2, 2), c(1, 0, 2, 1)], [-c(0, 1, 2, 2), c(0, 0, 2, 2), -c(0, 0, 2, 1)], [c(0, 1, 1, 2), -c(0, 0, 1, 2), c(0, 0, 1, 1)]]
        .map(|row| row.map(|v| v / det))
}

fn xform_normal(nm: &[[f64; 3]; 3], n: [f32; 3]) -> [f32; 3] {
    let v = [0, 1, 2].map(|r| nm[r][0] * n[0] as f64 + nm[r][1] * n[1] as f64 + nm[r][2] * n[2] as f64);
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l.is_finite() && l > 1e-20 { v.map(|c| (c / l) as f32) } else { [0.0; 3] }
}
