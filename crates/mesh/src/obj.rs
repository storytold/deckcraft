//! Wavefront OBJ, with diffuse colours from its `.mtl` libraries.
//!
//! Read: `v` (positions, extra colour components ignored), `vn` (normals), `f` (polygons of
//! `v`, `v/vt`, `v//vn` or `v/vt/vn` corners, 1-based or negative from the end, fanned into
//! triangles), `usemtl` (one primitive per material) and `mtllib`. From MTL: `newmtl`, `Kd`, and
//! opacity from `d` or `Tr`. Texture coordinates, lines, points, groups and smoothing groups are
//! left out. OBJ is Y-up, like glTF.

use std::collections::HashMap;

use crate::{Budget, Builder, DEFAULT_COLOR, Error, Primitive, Result, Scene, invalid, parse_f32};

const FORMAT: &str = "OBJ";
/// Most `.mtl` libraries one model may name.
const MAX_LIBRARIES: usize = 16;

/// The `.mtl` libraries `bytes` names, in order (for a caller that wants to fetch them first).
pub fn libraries(bytes: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(bytes);
    let mut out = Vec::new();
    for line in logical_lines(&text) {
        if let Some(rest) = keyword(&line, "mtllib") {
            for name in split_libraries(rest) {
                if out.len() < MAX_LIBRARIES && !out.contains(&name) {
                    out.push(name);
                }
            }
        }
    }
    out
}

/// `mtllib a.mtl b.mtl`, or one name with spaces in it when that is all the line holds.
fn split_libraries(rest: &str) -> Vec<String> {
    let parts: Vec<&str> = rest.split_ascii_whitespace().collect();
    if parts.len() > 1 && parts.iter().all(|p| p.to_ascii_lowercase().ends_with(".mtl")) {
        parts.into_iter().map(String::from).collect()
    } else if rest.trim().is_empty() {
        Vec::new()
    } else {
        vec![rest.trim().to_string()]
    }
}

/// Lines with `\` continuations joined and `#` comments cut.
fn logical_lines(text: &str) -> impl Iterator<Item = String> + '_ {
    let mut lines = text.lines();
    std::iter::from_fn(move || {
        let mut out = String::new();
        loop {
            let line = lines.next()?;
            let line = line.split('#').next().unwrap_or_default();
            match line.strip_suffix('\\') {
                Some(head) => {
                    out.push_str(head);
                    out.push(' ');
                }
                None => {
                    out.push_str(line);
                    return Some(out);
                }
            }
        }
    })
}

/// The rest of `line` after `word` and whitespace, when `line` starts with that keyword.
fn keyword<'a>(line: &'a str, word: &str) -> Option<&'a str> {
    let t = line.trim_start();
    let rest = t.strip_prefix(word)?;
    (rest.is_empty() || rest.starts_with(|c: char| c.is_ascii_whitespace())).then(|| rest.trim())
}

/// Materials by name: their diffuse colour and opacity.
pub fn read_mtl(bytes: &[u8], into: &mut HashMap<String, [f32; 4]>) {
    let text = String::from_utf8_lossy(bytes);
    let mut current: Option<String> = None;
    for line in logical_lines(&text) {
        if let Some(name) = keyword(&line, "newmtl") {
            current = Some(name.to_string());
            into.entry(name.to_string()).or_insert(DEFAULT_COLOR);
            continue;
        }
        let Some(name) = current.as_ref() else { continue };
        let Some(color) = into.get_mut(name) else { continue };
        if let Some(rest) = keyword(&line, "Kd") {
            let v: Vec<f32> = rest.split_ascii_whitespace().filter_map(parse_f32).collect();
            if let [r, g, b, ..] = v[..] {
                color[0] = r.clamp(0.0, 1.0);
                color[1] = g.clamp(0.0, 1.0);
                color[2] = b.clamp(0.0, 1.0);
            }
        } else if let Some(d) = keyword(&line, "d").and_then(|r| r.split_ascii_whitespace().find_map(parse_f32)) {
            color[3] = d.clamp(0.0, 1.0);
        } else if let Some(tr) = keyword(&line, "Tr").and_then(|r| r.split_ascii_whitespace().find_map(parse_f32)) {
            color[3] = (1.0 - tr).clamp(0.0, 1.0);
        }
    }
}

/// A corner's position index and normal index (0-based), resolved against what has been read.
fn corner(token: &str, positions: usize, normals: usize) -> Result<(usize, Option<usize>)> {
    let mut parts = token.split('/');
    let resolve = |s: &str, len: usize| -> Result<usize> {
        let i: i64 = s.parse().map_err(|_| invalid(FORMAT, format!("bad face index `{token}`")))?;
        let len_i = i64::try_from(len).unwrap_or(i64::MAX);
        let idx = if i > 0 {
            i - 1
        } else if i < 0 {
            len_i + i
        } else {
            -1
        };
        if (0..len_i).contains(&idx) {
            Ok(idx as usize)
        } else {
            Err(invalid(FORMAT, format!("face index `{token}` points past the {len} vertices read so far")))
        }
    };
    let v = resolve(parts.next().unwrap_or_default(), positions)?;
    let _texture = parts.next();
    let n = match parts.next() {
        Some(s) if !s.is_empty() => Some(resolve(s, normals)?),
        _ => None,
    };
    Ok((v, n))
}

pub fn read(bytes: &[u8], sidecar: &mut dyn FnMut(&str) -> Option<Vec<u8>>) -> Result<Scene> {
    let mut materials = HashMap::new();
    for lib in libraries(bytes) {
        match sidecar(&lib) {
            Some(mtl) => read_mtl(&mtl, &mut materials),
            None => log::info!("obj: material library {lib} not available; using grey"),
        }
    }
    let text = String::from_utf8_lossy(bytes);
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    // One builder per material, in first-use order.
    let mut builders: Vec<Builder<(usize, Option<usize>)>> = Vec::new();
    let mut by_material: HashMap<String, usize> = HashMap::new();
    let mut current = 0usize;
    let mut budget = Budget::default();
    let mut poly = Vec::new();
    for line in logical_lines(&text) {
        let mut words = line.split_ascii_whitespace();
        match words.next() {
            Some("v") => {
                if positions.len() >= crate::MAX_VERTICES {
                    return Err(crate::too_many_vertices());
                }
                let c: Vec<f32> =
                    words.take(3).map(|w| parse_f32(w).ok_or_else(|| invalid(FORMAT, format!("bad number `{w}`")))).collect::<Result<_>>()?;
                let [x, y, z] = c[..] else { return Err(invalid(FORMAT, "a `v` line needs three numbers")) };
                positions.push([x, y, z]);
            }
            Some("vn") => {
                if normals.len() >= crate::MAX_VERTICES {
                    return Err(crate::too_many_vertices());
                }
                let c: Vec<f32> = words.take(3).filter_map(parse_f32).collect();
                let [x, y, z] = c[..] else { return Err(invalid(FORMAT, "a `vn` line needs three numbers")) };
                normals.push(crate::math::normalize([x, y, z]));
            }
            Some("usemtl") => {
                let name = keyword(&line, "usemtl").unwrap_or_default().to_string();
                current = match by_material.get(&name) {
                    Some(&i) => i,
                    None => {
                        let color = materials.get(&name).copied().unwrap_or(DEFAULT_COLOR);
                        builders.push(Builder::new(Primitive::new(name.clone(), color)));
                        by_material.insert(name, builders.len() - 1);
                        builders.len() - 1
                    }
                };
            }
            Some("f") => {
                if builders.is_empty() {
                    builders.push(Builder::new(Primitive::new("default", DEFAULT_COLOR)));
                    by_material.insert(String::new(), 0);
                    current = 0;
                }
                let Some(b) = builders.get_mut(current) else { continue };
                poly.clear();
                for token in words {
                    let (v, n) = corner(token, positions.len(), normals.len())?;
                    let (Some(&pos), normal) = (positions.get(v), n.and_then(|n| normals.get(n).copied())) else { continue };
                    poly.push(b.vertex((v, n), pos, normal)?);
                }
                b.polygon(&poly, &mut budget)?;
            }
            _ => {}
        }
    }
    if builders.is_empty() && !positions.is_empty() {
        return Err(Error::Unsupported("the OBJ file has points but no faces".into()));
    }
    Scene { primitives: builders.into_iter().map(Builder::finish).collect() }.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_files(_: &str) -> Option<Vec<u8>> {
        None
    }

    const CUBE: &str = "# a cube\nmtllib cube.mtl\no Cube\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nv 0 0 1\nv 1 0 1\nv 1 1 1\nv 0 1 1\n\
        vn 0 0 -1\nvn 0 0 1\nusemtl Red\nf 1//1 4//1 3//1 2//1\nf 5//2 6//2 7//2 8//2\nusemtl Blue\nf 1 2 6 5\nf 2 3 7 6\nf 3 4 8 7\nf -4 -8 -5 -1\n";

    #[test]
    fn faces_materials_and_normals() {
        let mtl = b"newmtl Red\nKd 1 0 0\nd 0.5\nnewmtl Blue\nKd 0 0 1\n".to_vec();
        let mut files = |n: &str| (n == "cube.mtl").then(|| mtl.clone());
        let s = read(CUBE.as_bytes(), &mut files).expect("read");
        assert_eq!(s.triangle_count(), 12);
        assert_eq!(s.primitives.len(), 2);
        let red = &s.primitives[0];
        assert_eq!((red.name.as_str(), red.color), ("Red", [1.0, 0.0, 0.0, 0.5]));
        assert_eq!(red.normals.len(), red.positions.len(), "every red corner has a normal");
        let blue = &s.primitives[1];
        assert_eq!(blue.color, [0.0, 0.0, 1.0, 1.0]);
        assert!(blue.normals.is_empty(), "blue corners have none");
        assert!(s.primitives.iter().all(Primitive::is_valid));
    }

    #[test]
    fn missing_library_falls_back_to_grey() {
        let s = read(CUBE.as_bytes(), &mut no_files).expect("read");
        assert!(s.primitives.iter().all(|p| p.color == DEFAULT_COLOR));
        assert_eq!(libraries(CUBE.as_bytes()), vec!["cube.mtl".to_string()]);
        assert_eq!(libraries(b"mtllib my materials.mtl\nmtllib a.mtl b.mtl\n"), vec!["my materials.mtl", "a.mtl", "b.mtl"]);
    }

    #[test]
    fn continuations_and_texture_corners() {
        let text = "v 0 0 0\nv 1 0 0\nv 0 1 0\nvt 0 0\nf 1/1 \\\n 2/1 3/1\n";
        assert_eq!(read(text.as_bytes(), &mut no_files).map(|s| s.triangle_count()), Ok(1));
    }

    #[test]
    fn hostile_files_fail_cleanly() {
        for bad in ["v 0 0 0\nf 1 2 3\n", "v 0 0 0\nf 0 0 0\n", "v 0 0\n", "v 0 0 nan\n", "v 1 1 1\nf -9 1 1\n", "f a b c\n"] {
            assert!(read(bad.as_bytes(), &mut no_files).is_err(), "{bad:?}");
        }
        assert!(matches!(read(b"v 0 0 0\nv 1 1 1\n", &mut no_files), Err(Error::Unsupported(_))));
        assert_eq!(read(b"", &mut no_files), Err(Error::Empty));
        // A face that repeats a corner only makes degenerate triangles, which are kept.
        assert_eq!(read(b"v 0 0 0\nf 1 1 1\n", &mut no_files).map(|s| s.triangle_count()), Ok(1));
    }
}
