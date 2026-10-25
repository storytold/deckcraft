//! glTF 2.0 binary (`.glb`): a 12-byte header, a JSON chunk, then an optional binary chunk.
//!
//! [`write`] lays a [`Scene`] out as one node holding one mesh, a primitive per material, with
//! `POSITION` (and `NORMAL` when every vertex has one), `u16` or `u32` indices and a
//! double-sided, non-metallic material of the primitive's colour.
//!
//! [`read`] takes the triangles (mode 4) of the default scene, with node matrices or TRS applied:
//! float `POSITION` / `NORMAL` accessors with any stride, unsigned byte/short/int indices, and
//! `baseColorFactor`. Data in external files or data URIs, sparse or quantized accessors and
//! compressed meshes are skipped.

use serde_json::{Value, json};

use crate::math::{Mat4, apply3, normalize};
use crate::{DEFAULT_COLOR, Error, Primitive, Result, Scene, invalid};

const FORMAT: &str = "glTF binary";
const MAGIC: &[u8; 4] = b"glTF";
const CHUNK_JSON: u32 = 0x4E4F_534A;
const CHUNK_BIN: u32 = 0x004E_4942;
const FLOAT: u64 = 5126;
const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

/// The JSON and binary chunks of a glTF binary.
fn chunks(bytes: &[u8]) -> Result<(Value, &[u8])> {
    if !bytes.starts_with(MAGIC) {
        return Err(invalid(FORMAT, "missing the `glTF` signature"));
    }
    match u32_at(bytes, 4) {
        Some(2) => {}
        Some(v) => return Err(Error::Unsupported(format!("glTF version {v} (DeckCraft reads version 2)"))),
        None => return Err(invalid(FORMAT, "shorter than its header")),
    }
    let total = u32_at(bytes, 8).map(|n| n as usize).filter(|&n| n <= bytes.len()).ok_or_else(|| invalid(FORMAT, "its length is wrong"))?;
    let bytes = bytes.get(..total).unwrap_or_default();
    let (mut json, mut bin) = (None, None);
    let mut at = 12usize;
    while at.checked_add(8).is_some_and(|e| e <= bytes.len()) {
        let len = u32_at(bytes, at).unwrap_or_default() as usize;
        let kind = u32_at(bytes, at + 4).unwrap_or_default();
        let start = at + 8;
        let data = start.checked_add(len).and_then(|end| bytes.get(start..end)).ok_or_else(|| invalid(FORMAT, "a chunk runs past the end"))?;
        match kind {
            CHUNK_JSON if json.is_none() => json = Some(data),
            CHUNK_BIN if bin.is_none() => bin = Some(data),
            _ => {}
        }
        at = start + len;
    }
    let json = json.ok_or_else(|| invalid(FORMAT, "no JSON chunk"))?;
    let doc: Value = serde_json::from_slice(json).map_err(|e| invalid(FORMAT, format!("its JSON can't be read: {e}")))?;
    if !doc.is_object() {
        return Err(invalid(FORMAT, "its JSON is not an object"));
    }
    Ok((doc, bin.unwrap_or_default()))
}

/// Whether `bytes` is a glTF 2.0 binary with readable JSON (meshes are not looked at).
pub fn check(bytes: &[u8]) -> Result<()> {
    chunks(bytes).map(|_| ())
}

struct Reader<'a> {
    doc: &'a Value,
    bin: &'a [u8],
}

impl<'a> Reader<'a> {
    fn item(&self, kind: &str, i: &Value) -> Option<&'a Value> {
        self.doc.get(kind)?.get(usize::try_from(i.as_u64()?).ok()?)
    }

    /// The bytes and stride of an accessor's elements: `(data, stride, count, element size)`.
    fn view(&self, acc: &Value, elem: usize) -> Option<(&'a [u8], usize, usize)> {
        if acc.get("sparse").is_some() {
            return None;
        }
        let count = usize::try_from(acc.get("count")?.as_u64()?).ok()?;
        if count > crate::MAX_VERTICES * 3 {
            return None;
        }
        let view = self.item("bufferViews", acc.get("bufferView")?)?;
        // Only the binary chunk (buffer 0 with no `uri`).
        let buffer = self.item("buffers", view.get("buffer")?)?;
        if buffer.get("uri").is_some() {
            return None;
        }
        let num = |v: &Value, k: &str| v.get(k).and_then(Value::as_u64).map_or(Some(0), |n| usize::try_from(n).ok());
        let start = num(view, "byteOffset")?.checked_add(num(acc, "byteOffset")?)?;
        let stride = match num(view, "byteStride")? {
            0 => elem,
            s if s >= elem => s,
            _ => return None,
        };
        let view_end = num(view, "byteOffset")?.checked_add(usize::try_from(view.get("byteLength")?.as_u64()?).ok()?)?;
        let need = if count == 0 { 0 } else { stride.checked_mul(count - 1)?.checked_add(elem)? };
        let end = start.checked_add(need)?;
        if end > view_end {
            return None;
        }
        Some((self.bin.get(start..end)?, stride, count))
    }

    fn vec3s(&self, acc: &Value) -> Option<Vec<[f32; 3]>> {
        if acc.get("componentType")?.as_u64()? != FLOAT
            || acc.get("type")?.as_str()? != "VEC3"
            || acc.get("normalized").and_then(Value::as_bool) == Some(true)
        {
            return None;
        }
        let (data, stride, count) = self.view(acc, 12)?;
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let at = i * stride;
            let f = |o: usize| u32_at(data, at + o).map(f32::from_bits).filter(|v| v.is_finite());
            out.push([f(0)?, f(4)?, f(8)?]);
        }
        Some(out)
    }

    fn indices(&self, acc: &Value) -> Option<Vec<u32>> {
        if acc.get("type")?.as_str()? != "SCALAR" {
            return None;
        }
        let size = match acc.get("componentType")?.as_u64()? {
            5121 => 1,
            5123 => 2,
            5125 => 4,
            _ => return None,
        };
        let (data, stride, count) = self.view(acc, size)?;
        (0..count)
            .map(|i| {
                let at = i * stride;
                match size {
                    1 => data.get(at).map(|&b| b as u32),
                    2 => data.get(at..at + 2).map(|s| u16::from_le_bytes([s[0], s[1]]) as u32),
                    _ => u32_at(data, at),
                }
            })
            .collect()
    }

    fn color(&self, prim: &Value) -> [f32; 4] {
        let factor = prim
            .get("material")
            .and_then(|m| self.item("materials", m))
            .and_then(|m| m.pointer("/pbrMetallicRoughness/baseColorFactor"))
            .and_then(Value::as_array);
        let mut c = DEFAULT_COLOR;
        if let Some(f) = factor {
            for (slot, v) in c.iter_mut().zip(f) {
                if let Some(v) = v.as_f64().filter(|v| v.is_finite()) {
                    *slot = v.clamp(0.0, 1.0) as f32;
                }
            }
        }
        c
    }

    fn primitive(&self, prim: &Value, name: &str, world: &Mat4) -> Option<Primitive> {
        if prim.get("mode").and_then(Value::as_u64).unwrap_or(4) != 4 {
            return None;
        }
        if prim.pointer("/extensions/KHR_draco_mesh_compression").is_some() {
            return None;
        }
        let attrs = prim.get("attributes")?;
        let positions = self.vec3s(self.item("accessors", attrs.get("POSITION")?)?)?;
        let normals = attrs
            .get("NORMAL")
            .and_then(|n| self.item("accessors", n))
            .and_then(|a| self.vec3s(a))
            .filter(|n| n.len() == positions.len())
            .unwrap_or_default();
        let indices = match prim.get("indices") {
            Some(i) => self.indices(self.item("accessors", i)?)?,
            None => (0..u32::try_from(positions.len()).ok()?).collect(),
        };
        let mut indices: Vec<u32> =
            indices.as_chunks::<3>().0.iter().filter(|t| t.iter().all(|&i| (i as usize) < positions.len())).flatten().copied().collect();
        if world.det3() < 0.0 {
            for t in indices.as_chunks_mut::<3>().0 {
                t.swap(1, 2);
            }
        }
        let nm = world.normal_matrix();
        Some(Primitive {
            name: name.to_string(),
            positions: positions.iter().map(|p| world.point(*p)).collect(),
            normals: match nm {
                Some(nm) => normals.iter().map(|n| apply3(&nm, *n)).collect(),
                None => Vec::new(),
            },
            indices,
            color: self.color(prim),
        })
    }

    fn local(node: &Value) -> Mat4 {
        let floats = |k: &str, n: usize| -> Option<Vec<f64>> {
            let a = node.get(k)?.as_array()?;
            let v: Vec<f64> = a.iter().filter_map(Value::as_f64).filter(|v| v.is_finite()).collect();
            (v.len() == n).then_some(v)
        };
        if let Some(m) = floats("matrix", 16) {
            let mut out = [0.0; 16];
            out.copy_from_slice(&m);
            return Mat4(out);
        }
        let t = floats("translation", 3).map_or(Mat4::IDENTITY, |t| Mat4::translation([t[0], t[1], t[2]]));
        let r = floats("rotation", 4).map_or(Mat4::IDENTITY, |q| Mat4::rotation([q[0], q[1], q[2], q[3]]));
        let s = floats("scale", 3).map_or(Mat4::IDENTITY, |s| Mat4::scale([s[0], s[1], s[2]]));
        t.mul(&r).mul(&s)
    }

    /// Adds node `i` and its children. `path` holds the nodes above it (a cycle is cut there);
    /// `vertices` counts what has been added, so a mesh shared by many nodes can't blow up.
    fn node(&self, i: &Value, parent: &Mat4, path: &mut Vec<u64>, vertices: &mut usize, out: &mut Vec<Primitive>) -> Result<()> {
        let Some(id) = i.as_u64() else { return Ok(()) };
        if path.len() > crate::MAX_DEPTH || path.contains(&id) {
            return Ok(());
        }
        let Some(node) = self.item("nodes", i) else { return Ok(()) };
        let world = parent.mul(&Self::local(node));
        if !world.is_finite() {
            return Ok(());
        }
        if let Some(mesh) = node.get("mesh").and_then(|m| self.item("meshes", m)) {
            let name = mesh.get("name").or_else(|| node.get("name")).and_then(Value::as_str).unwrap_or("mesh");
            for prim in mesh.get("primitives").and_then(Value::as_array).into_iter().flatten() {
                if let Some(p) = self.primitive(prim, name, &world) {
                    *vertices += p.positions.len();
                    if *vertices > crate::MAX_VERTICES {
                        return Err(crate::too_many_vertices());
                    }
                    out.push(p);
                }
            }
        }
        path.push(id);
        for child in node.get("children").and_then(Value::as_array).into_iter().flatten() {
            self.node(child, &world, path, vertices, out)?;
        }
        path.pop();
        Ok(())
    }
}

pub fn read(bytes: &[u8]) -> Result<Scene> {
    let (doc, bin) = chunks(bytes)?;
    let r = Reader { doc: &doc, bin };
    let scene_index = doc.get("scene").cloned().unwrap_or(json!(0));
    let roots: Vec<Value> = match r.item("scenes", &scene_index).and_then(|s| s.get("nodes")).and_then(Value::as_array) {
        Some(n) => n.clone(),
        // No scene: every node no other node lists as a child.
        None => {
            let n = doc.get("nodes").and_then(Value::as_array).map_or(0, Vec::len);
            let children: Vec<u64> = doc
                .get("nodes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|n| n.get("children")?.as_array())
                .flatten()
                .filter_map(Value::as_u64)
                .collect();
            (0..n as u64).filter(|i| !children.contains(i)).map(Value::from).collect()
        }
    };
    let mut prims = Vec::new();
    let mut vertices = 0;
    for root in &roots {
        r.node(root, &Mat4::IDENTITY, &mut Vec::new(), &mut vertices, &mut prims)?;
    }
    let scene = Scene { primitives: prims };
    if scene.triangle_count() > crate::MAX_TRIANGLES {
        return Err(Error::TooLarge(format!("{} triangles", scene.triangle_count())));
    }
    scene.finish()
}

fn pad4(v: &mut Vec<u8>, with: u8) {
    while !v.len().is_multiple_of(4) {
        v.push(with);
    }
}

/// `scene` as a glTF binary.
pub fn write(scene: &Scene) -> Result<Vec<u8>> {
    let mut bin: Vec<u8> = Vec::new();
    let (mut views, mut accessors, mut materials, mut prims) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut view = |bin: &mut Vec<u8>, data: &[u8], target: u32| -> usize {
        pad4(bin, 0);
        views.push(json!({"buffer": 0, "byteOffset": bin.len(), "byteLength": data.len(), "target": target}));
        bin.extend_from_slice(data);
        views.len() - 1
    };
    for p in scene.primitives.iter().filter(|p| !p.indices.is_empty()) {
        if !p.is_valid() {
            return Err(invalid(FORMAT, format!("primitive `{}` is inconsistent", p.name)));
        }
        let (mut min, mut max) = ([f32::MAX; 3], [f32::MIN; 3]);
        let mut pos = Vec::with_capacity(p.positions.len() * 12);
        for v in &p.positions {
            for k in 0..3 {
                min[k] = min[k].min(v[k]);
                max[k] = max[k].max(v[k]);
                pos.extend(v[k].to_le_bytes());
            }
        }
        let pv = view(&mut bin, &pos, ARRAY_BUFFER);
        accessors.push(json!({"bufferView": pv, "componentType": FLOAT, "count": p.positions.len(), "type": "VEC3", "min": min, "max": max}));
        let mut attributes = json!({"POSITION": accessors.len() - 1});
        if !p.normals.is_empty() {
            let nb: Vec<u8> = p.normals.iter().flat_map(|n| normalize(*n)).flat_map(f32::to_le_bytes).collect();
            let nv = view(&mut bin, &nb, ARRAY_BUFFER);
            accessors.push(json!({"bufferView": nv, "componentType": FLOAT, "count": p.normals.len(), "type": "VEC3"}));
            attributes["NORMAL"] = json!(accessors.len() - 1);
        }
        let (ib, ct): (Vec<u8>, u64) = if p.positions.len() <= u16::MAX as usize {
            (p.indices.iter().flat_map(|&i| (i as u16).to_le_bytes()).collect(), 5123)
        } else {
            (p.indices.iter().flat_map(|i| i.to_le_bytes()).collect(), 5125)
        };
        let iv = view(&mut bin, &ib, ELEMENT_ARRAY_BUFFER);
        accessors.push(json!({"bufferView": iv, "componentType": ct, "count": p.indices.len(), "type": "SCALAR"}));
        materials.push(json!({
            "name": p.name,
            "pbrMetallicRoughness": {"baseColorFactor": p.color, "metallicFactor": 0.0, "roughnessFactor": 0.8},
            "doubleSided": true,
            "alphaMode": if p.color[3] < 1.0 { "BLEND" } else { "OPAQUE" },
        }));
        prims.push(json!({"attributes": attributes, "indices": accessors.len() - 1, "material": materials.len() - 1, "mode": 4}));
    }
    if prims.is_empty() {
        return Err(Error::Empty);
    }
    pad4(&mut bin, 0);
    let doc = json!({
        "asset": {"version": "2.0", "generator": "DeckCraft"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0, "name": "Model"}],
        "meshes": [{"primitives": prims, "name": "Model"}],
        "materials": materials,
        "accessors": accessors,
        "bufferViews": views,
        "buffers": [{"byteLength": bin.len()}],
    });
    let mut js = serde_json::to_vec(&doc).map_err(|e| invalid(FORMAT, e.to_string()))?;
    pad4(&mut js, b' ');
    let total = 12 + 8 + js.len() + 8 + bin.len();
    let too_big = || Error::TooLarge("over 4 GiB as glTF binary".into());
    let total32 = u32::try_from(total).map_err(|_| too_big())?;
    let mut out = Vec::with_capacity(total);
    out.extend(MAGIC);
    out.extend(2u32.to_le_bytes());
    out.extend(total32.to_le_bytes());
    out.extend(u32::try_from(js.len()).map_err(|_| too_big())?.to_le_bytes());
    out.extend(CHUNK_JSON.to_le_bytes());
    out.extend(js);
    out.extend(u32::try_from(bin.len()).map_err(|_| too_big())?.to_le_bytes());
    out.extend(CHUNK_BIN.to_le_bytes());
    out.extend(bin);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad() -> Scene {
        Scene {
            primitives: vec![
                Primitive {
                    name: "Red".into(),
                    positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 2.0, 0.0], [0.0, 2.0, 0.0]],
                    normals: vec![[0.0, 0.0, 1.0]; 4],
                    indices: vec![0, 1, 2, 0, 2, 3],
                    color: [1.0, 0.0, 0.0, 1.0],
                },
                Primitive {
                    name: "Grey".into(),
                    positions: vec![[0.0, 0.0, -1.0], [1.0, 0.0, -1.0], [0.0, 1.0, -1.0]],
                    indices: vec![0, 1, 2],
                    color: DEFAULT_COLOR,
                    ..Default::default()
                },
            ],
        }
    }

    #[test]
    fn write_then_read_round_trips() {
        let glb = write(&quad()).expect("write");
        assert_eq!(&glb[..4], b"glTF");
        assert_eq!(glb.len() % 4, 0);
        assert_eq!(u32_at(&glb, 8), Some(glb.len() as u32));
        let back = read(&glb).expect("read");
        assert_eq!(back.primitives.len(), 2);
        assert_eq!(back.primitives[0].positions, quad().primitives[0].positions);
        assert_eq!(back.primitives[0].normals, quad().primitives[0].normals);
        assert_eq!(back.primitives[0].indices, quad().primitives[0].indices);
        assert_eq!(back.primitives[0].color, [1.0, 0.0, 0.0, 1.0]);
        assert!(back.primitives[1].normals.is_empty());
        let (doc, _) = chunks(&glb).expect("chunks");
        assert_eq!(doc.pointer("/accessors/0/max"), Some(&json!([1.0, 2.0, 0.0])), "POSITION carries its bounds");
    }

    #[test]
    fn large_meshes_use_32_bit_indices() {
        let n = 70_000u32;
        let p = Primitive {
            name: "big".into(),
            positions: (0..n).map(|i| [i as f32, (i % 7) as f32, 0.0]).collect(),
            indices: (0..n - 2).flat_map(|i| [i, i + 1, i + 2]).collect(),
            color: DEFAULT_COLOR,
            ..Default::default()
        };
        let glb = write(&Scene { primitives: vec![p.clone()] }).expect("write");
        let back = read(&glb).expect("read");
        assert_eq!(back.primitives[0].indices, p.indices);
    }

    #[test]
    fn node_transforms_apply() {
        let glb = write(&quad()).expect("write");
        let (mut doc, bin) = chunks(&glb).expect("chunks");
        doc["nodes"] = json!([{"children": [1], "translation": [10.0, 0.0, 0.0]}, {"mesh": 0, "scale": [-1.0, 1.0, 1.0]}]);
        doc["scenes"] = json!([{"nodes": [0]}]);
        let glb2 = assemble(&doc, bin);
        let back = read(&glb2).expect("read");
        assert_eq!(back.primitives[0].positions[1], [9.0, 0.0, 0.0]);
        assert_eq!(back.primitives[0].indices[..3], [0, 2, 1], "a mirror flips the winding");
        assert_eq!(back.primitives[0].normals[0], [0.0, 0.0, 1.0]);
    }

    fn assemble(doc: &Value, bin: &[u8]) -> Vec<u8> {
        let mut js = serde_json::to_vec(doc).expect("json");
        pad4(&mut js, b' ');
        let mut out = MAGIC.to_vec();
        out.extend(2u32.to_le_bytes());
        out.extend(((12 + 8 + js.len() + 8 + bin.len()) as u32).to_le_bytes());
        out.extend((js.len() as u32).to_le_bytes());
        out.extend(CHUNK_JSON.to_le_bytes());
        out.extend(js);
        out.extend((bin.len() as u32).to_le_bytes());
        out.extend(CHUNK_BIN.to_le_bytes());
        out.extend(bin);
        out
    }

    #[test]
    fn hostile_files_fail_cleanly() {
        let glb = write(&quad()).expect("write");
        // Every truncation is an error or a smaller model, never a panic.
        for n in 0..glb.len() {
            let _ = read(&glb[..n]);
        }
        let (doc, bin) = chunks(&glb).expect("chunks");
        let mutate = |f: &dyn Fn(&mut Value)| {
            let mut d = doc.clone();
            f(&mut d);
            read(&assemble(&d, bin))
        };
        assert!(mutate(&|d| d["accessors"][0]["count"] = json!(u64::MAX)).is_ok_and(|s| s.primitives.len() == 1), "the broken primitive is skipped");
        assert!(mutate(&|d| d["bufferViews"][0]["byteOffset"] = json!(1u64 << 40)).is_ok());
        assert_eq!(mutate(&|d| d["nodes"] = json!([{"mesh": 0, "children": [0]}])).map(|s| s.primitives.len()), Ok(2), "a cycle is cut");
        assert!(mutate(&|d| d["buffers"][0]["uri"] = json!("data:,")).is_err());
        assert!(read(b"glTF\x01\0\0\0\x0c\0\0\0").is_err());
        assert!(read(b"glTF\x02\0\0\0\xff\xff\xff\xff").is_err());
        assert!(check(b"nope").is_err());
        let mut bad_index = quad();
        bad_index.primitives[0].indices[0] = 99;
        assert!(write(&bad_index).is_err());
    }
}
