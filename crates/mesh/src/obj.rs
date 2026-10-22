//! Wavefront OBJ reader with MTL materials.
//!
//! Read: `v` (with the common `v x y z r g b` vertex colour extension), `vt`, `vn`, `f` (any
//! polygon, triangulated as a fan; negative indices count back), `usemtl`, `mtllib`; in MTL files
//! `newmtl`, `Kd`, `Ke`, `Ns`, `d`, `Tr`, `Pr`, `Pm` and `map_Kd` (with its options). Groups,
//! smoothing groups, lines, points and free-form surfaces are skipped.

use std::collections::HashMap;

use crate::{Alpha, Error, Material, Primitive, Resolve, Result, Scene, Texture, srgb_to_linear};

/// Material libraries read for one model.
const MAX_LIBRARIES: usize = 64;

/// One face corner: position, texture coordinate and normal indices (0-based, `None` when absent).
type Corner = (usize, Option<usize>, Option<usize>);

#[derive(Default)]
struct Builder {
    prim: Primitive,
    map: HashMap<Corner, u32>,
}

pub fn read(bytes: &[u8], resolve: Resolve) -> Result<Scene> {
    let text = String::from_utf8_lossy(bytes);
    let mut pos: Vec<[f32; 3]> = Vec::new();
    let mut col: Vec<Option<[f32; 4]>> = Vec::new();
    let mut tex: Vec<[f32; 2]> = Vec::new();
    let mut nrm: Vec<[f32; 3]> = Vec::new();
    let mut scene = Scene::default();
    let mut names: HashMap<String, usize> = HashMap::new();
    let mut tex_cache: HashMap<String, Option<usize>> = HashMap::new();
    let mut libs: Vec<String> = Vec::new();
    // Primitive per material (`None`: faces before any `usemtl`).
    let mut builders: Vec<(Option<usize>, Builder)> = Vec::new();
    let mut current: Option<usize> = None;
    let mut any_colors = false;
    let mut indices = 0usize;
    // Vertices made for faces (one per distinct position/uv/normal corner), over every material.
    let mut made = 0usize;

    let mut joined = String::new();
    for raw in text.lines() {
        // A trailing backslash continues the line.
        if let Some(head) = raw.strip_suffix('\\') {
            joined.push_str(head);
            joined.push(' ');
            continue;
        }
        joined.push_str(raw);
        let line = std::mem::take(&mut joined);
        let line = line.split('#').next().unwrap_or("").trim();
        let mut it = line.split_whitespace();
        let Some(kw) = it.next() else { continue };
        match kw {
            "v" => {
                let f: Vec<f32> = it.filter_map(|t| t.parse::<f32>().ok()).collect();
                if f.len() < 3 {
                    continue;
                }
                if pos.len() >= crate::MAX_VERTICES {
                    return Err(Error::TooLarge);
                }
                pos.push([f[0], f[1], f[2]]);
                // `v x y z r g b`: colours in 0..1 (sRGB).
                col.push(if f.len() >= 6 {
                    any_colors = true;
                    Some([srgb_to_linear(f[3]), srgb_to_linear(f[4]), srgb_to_linear(f[5]), 1.0])
                } else {
                    None
                });
            }
            "vt" => {
                let f: Vec<f32> = it.filter_map(|t| t.parse::<f32>().ok()).collect();
                if tex.len() < crate::MAX_VERTICES {
                    // OBJ's v runs up from the bottom of the image; glTF's down from the top.
                    tex.push([f.first().copied().unwrap_or(0.0), 1.0 - f.get(1).copied().unwrap_or(0.0)]);
                }
            }
            "vn" => {
                let f: Vec<f32> = it.filter_map(|t| t.parse::<f32>().ok()).collect();
                if f.len() >= 3 && nrm.len() < crate::MAX_VERTICES {
                    nrm.push(unit([f[0], f[1], f[2]]));
                }
            }
            "f" | "fo" => {
                let corners: Vec<Corner> = it.filter_map(|t| corner(t, pos.len(), tex.len(), nrm.len())).collect();
                if corners.len() < 3 {
                    continue;
                }
                let b = match builders.iter().position(|(m, _)| *m == current) {
                    Some(i) => i,
                    None => {
                        builders.push((current, Builder::default()));
                        builders.len() - 1
                    }
                };
                let Some((_, b)) = builders.get_mut(b) else { continue };
                let before = b.prim.positions.len();
                let ids: Vec<u32> = corners.iter().map(|c| vertex(b, *c, &pos, &tex, &nrm)).collect();
                made = made.saturating_add(b.prim.positions.len() - before);
                if made > crate::MAX_VERTICES {
                    return Err(Error::TooLarge);
                }
                for k in 1..ids.len() - 1 {
                    if let (Some(a), Some(x), Some(y)) = (ids.first(), ids.get(k), ids.get(k + 1)) {
                        b.prim.indices.extend_from_slice(&[*a, *x, *y]);
                        indices += 3;
                    }
                }
                if indices > crate::MAX_INDICES {
                    return Err(Error::TooLarge);
                }
            }
            "usemtl" => {
                let name = rest_of(line, kw);
                current = Some(match names.get(&name) {
                    Some(i) => *i,
                    None => {
                        // A material no library defines: a plain one under that name.
                        scene.materials.push(Material { name: name.clone(), ..Default::default() });
                        names.insert(name, scene.materials.len() - 1);
                        scene.materials.len() - 1
                    }
                });
            }
            "mtllib" => {
                for lib in mtl_names(&rest_of(line, kw)) {
                    // Each library once, and only so many: a file can't make us read one for ever.
                    if libs.contains(&lib) || libs.len() >= MAX_LIBRARIES {
                        continue;
                    }
                    libs.push(lib.clone());
                    match resolve(&lib) {
                        Some(data) => mtl(&data, resolve, &mut scene, &mut names, &mut tex_cache),
                        None => log::warn!("OBJ: material library {lib} not found"),
                    }
                }
            }
            _ => {}
        }
    }
    for (m, mut b) in builders {
        // Vertex colours on some vertices only: the rest are white.
        if any_colors {
            b.prim.colors = vec![[1.0; 4]; b.prim.positions.len()];
            for ((p, _, _), i) in &b.map {
                if let (Some(Some(c)), Some(slot)) = (col.get(*p), b.prim.colors.get_mut(*i as usize)) {
                    *slot = *c;
                }
            }
        }
        b.prim.material = m;
        scene.primitives.push(b.prim);
    }
    // Faces without a material get the plain grey.
    if scene.primitives.iter().any(|p| p.material.is_none()) {
        scene.materials.push(Material::default_grey());
        let g = scene.materials.len() - 1;
        for p in scene.primitives.iter_mut().filter(|p| p.material.is_none()) {
            p.material = Some(g);
        }
    }
    Ok(scene)
}

fn unit(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l.is_finite() && l > 1e-20 { [v[0] / l, v[1] / l, v[2] / l] } else { [0.0; 3] }
}

/// The text after the keyword (names may contain spaces).
fn rest_of(line: &str, kw: &str) -> String {
    line.get(kw.len()..).unwrap_or("").trim().to_string()
}

/// `mtllib a.mtl b.mtl`; a single name with spaces when no name in the list ends in `.mtl`.
fn mtl_names(rest: &str) -> Vec<String> {
    let parts: Vec<&str> = rest.split_whitespace().collect();
    if parts.len() > 1 && parts.iter().all(|p| p.to_ascii_lowercase().ends_with(".mtl")) {
        parts.into_iter().map(String::from).collect()
    } else if rest.is_empty() {
        vec![]
    } else {
        vec![rest.to_string()]
    }
}

/// One `v/vt/vn` token, resolved to 0-based indices.
fn corner(t: &str, np: usize, nt: usize, nn: usize) -> Option<Corner> {
    let mut parts = t.split('/');
    let at = |s: Option<&str>, n: usize| -> Option<usize> {
        let v: i64 = s.filter(|s| !s.is_empty())?.parse().ok()?;
        let i = if v < 0 { (n as i64).checked_add(v)? } else { v.checked_sub(1)? };
        usize::try_from(i).ok().filter(|i| *i < n)
    };
    let p = at(parts.next(), np)?;
    let vt = at(parts.next(), nt);
    let vn = at(parts.next(), nn);
    Some((p, vt, vn))
}

fn vertex(b: &mut Builder, c: Corner, pos: &[[f32; 3]], tex: &[[f32; 2]], nrm: &[[f32; 3]]) -> u32 {
    if let Some(i) = b.map.get(&c) {
        return *i;
    }
    let i = b.prim.positions.len() as u32;
    b.prim.positions.push(pos.get(c.0).copied().unwrap_or([0.0; 3]));
    // Attributes present on some corners only: zeros elsewhere (a zero normal draws flat).
    let has_uv = c.1.is_some() || !b.prim.uvs.is_empty();
    if has_uv {
        b.prim.uvs.resize(i as usize, [0.0; 2]);
        b.prim.uvs.push(c.1.and_then(|t| tex.get(t)).copied().unwrap_or([0.0; 2]));
    }
    let has_n = c.2.is_some() || !b.prim.normals.is_empty();
    if has_n {
        b.prim.normals.resize(i as usize, [0.0; 3]);
        b.prim.normals.push(c.2.and_then(|n| nrm.get(n)).copied().unwrap_or([0.0; 3]));
    }
    b.map.insert(c, i);
    i
}

/// Parse an MTL library into `scene.materials` (later definitions of a name replace earlier ones).
fn mtl(data: &[u8], resolve: Resolve, scene: &mut Scene, names: &mut HashMap<String, usize>, tex_cache: &mut HashMap<String, Option<usize>>) {
    let text = String::from_utf8_lossy(data);
    let mut cur: Option<usize> = None;
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let mut it = line.split_whitespace();
        let Some(kw) = it.next() else { continue };
        let nums: Vec<f32> = line.split_whitespace().skip(1).filter_map(|t| t.parse::<f32>().ok()).collect();
        if kw == "newmtl" {
            let name = rest_of(line, kw);
            let i = match names.get(&name) {
                Some(i) => *i,
                None => {
                    if scene.materials.len() >= 4096 {
                        cur = None;
                        continue;
                    }
                    scene.materials.push(Material { name: name.clone(), ..Default::default() });
                    names.insert(name, scene.materials.len() - 1);
                    scene.materials.len() - 1
                }
            };
            cur = Some(i);
            continue;
        }
        let Some(m) = cur.and_then(|i| scene.materials.get_mut(i)) else { continue };
        match kw {
            "Kd" if nums.len() >= 3 => {
                m.base_color = [srgb_to_linear(nums[0]), srgb_to_linear(nums[1]), srgb_to_linear(nums[2]), m.base_color[3]];
            }
            "Ke" if nums.len() >= 3 => m.emissive = [srgb_to_linear(nums[0]), srgb_to_linear(nums[1]), srgb_to_linear(nums[2])],
            "d" if !nums.is_empty() => set_alpha(m, nums[0]),
            "Tr" if !nums.is_empty() => set_alpha(m, 1.0 - nums[0]),
            // Phong exponent to roughness.
            "Ns" if !nums.is_empty() => m.roughness = (2.0 / (nums[0].max(0.0) + 2.0)).sqrt(),
            "Pr" if !nums.is_empty() => m.roughness = nums[0],
            "Pm" if !nums.is_empty() => m.metallic = nums[0],
            "map_Kd" => {
                let file = map_file(&rest_of(line, kw));
                if file.is_empty() {
                    continue;
                }
                let ti = match tex_cache.get(&file) {
                    Some(t) => *t,
                    None => {
                        // Failed reads count too (`tex_cache` keeps them), so they are bounded.
                        let t = if scene.textures.len() < crate::MAX_TEXTURES && tex_cache.len() < crate::MAX_TEXTURES * 4 {
                            resolve(&file).and_then(Texture::decode).map(|t| {
                                scene.textures.push(t);
                                scene.textures.len() - 1
                            })
                        } else {
                            None
                        };
                        if t.is_none() {
                            log::warn!("OBJ: texture {file} not found or unreadable");
                        }
                        tex_cache.insert(file, t);
                        t
                    }
                };
                // `map_Kd` multiplies `Kd`; most files set `Kd` to the texture's tint or white.
                if let Some(m) = cur.and_then(|i| scene.materials.get_mut(i)) {
                    m.texture = ti;
                }
            }
            _ => {}
        }
    }
}

fn set_alpha(m: &mut Material, a: f32) {
    let a = if a.is_finite() { a.clamp(0.0, 1.0) } else { 1.0 };
    m.base_color[3] = a;
    m.alpha = if a < 0.999 { Alpha::Blend } else { Alpha::Opaque };
}

/// The file name of a `map_*` statement: options (`-o 0 0 0`, `-bm 1`, …) skipped.
fn map_file(rest: &str) -> String {
    let toks: Vec<&str> = rest.split_whitespace().collect();
    let mut i = 0;
    while let Some(t) = toks.get(i) {
        let args = match *t {
            "-blendu" | "-blendv" | "-cc" | "-clamp" | "-imfchan" | "-texres" | "-bm" | "-boost" | "-type" => 1,
            "-mm" => 2,
            "-o" | "-s" | "-t" => 3,
            _ => break,
        };
        i += 1;
        // `-o`, `-s` and `-t` take one to three numbers.
        let mut k = 0;
        while k < args && toks.get(i).is_some_and(|a| a.parse::<f32>().is_ok() || (args == 1 && !a.starts_with('-'))) {
            i += 1;
            k += 1;
        }
    }
    toks.get(i..).unwrap_or(&[]).join(" ")
}
