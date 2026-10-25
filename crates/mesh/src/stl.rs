//! STL, binary and ASCII.
//!
//! Binary: an 80-byte header, a little-endian `u32` triangle count, then 50 bytes per triangle
//! (normal, three vertices as `f32` triples, a 2-byte attribute). ASCII: `solid`, then
//! `facet normal … outer loop vertex x y z … endloop endfacet`, then `endsolid`.
//!
//! Some binary files start with `solid` too, so a file whose length matches its triangle count
//! exactly is read as binary whatever its header says. The file's facet normals are dropped
//! (exporters often get them wrong); identical vertices are shared, and the surface is drawn flat.

use crate::{Budget, Builder, DEFAULT_COLOR, Error, Primitive, Result, Scene, invalid, parse_f32};

const FORMAT: &str = "STL";

/// STL is Z-up; glTF is Y-up.
fn y_up(v: [f32; 3]) -> [f32; 3] {
    [v[0], v[2], -v[1]]
}

pub fn read(bytes: &[u8]) -> Result<Scene> {
    if is_binary(bytes) { read_binary(bytes) } else { read_ascii(bytes) }
}

fn binary_count(bytes: &[u8]) -> Option<usize> {
    let c = bytes.get(80..84)?;
    Some(u32::from_le_bytes([c[0], c[1], c[2], c[3]]) as usize)
}

fn is_binary(bytes: &[u8]) -> bool {
    let exact = binary_count(bytes).and_then(|n| n.checked_mul(50)).and_then(|n| n.checked_add(84)) == Some(bytes.len());
    let text =
        bytes.iter().position(|b| !b.is_ascii_whitespace()).and_then(|i| bytes.get(i..i + 5)).is_some_and(|s| s.eq_ignore_ascii_case(b"solid"));
    exact || (!text && bytes.len() >= 84)
}

fn read_binary(bytes: &[u8]) -> Result<Scene> {
    let count = binary_count(bytes).ok_or_else(|| invalid(FORMAT, "shorter than its header"))?;
    if count > crate::MAX_TRIANGLES {
        return Err(Error::TooLarge(format!("{count} triangles")));
    }
    let body = bytes.get(84..).unwrap_or_default();
    // A truncated file keeps the triangles it has.
    let have = body.len() / 50;
    if have < count {
        log::warn!("stl: {count} triangles announced, {have} present");
    }
    let mut b = Builder::new(Primitive::new("STL", DEFAULT_COLOR));
    let mut budget = Budget::default();
    for tri in body.as_chunks::<50>().0.iter().take(count) {
        let mut corners = [0u32; 3];
        let mut ok = true;
        for (k, corner) in corners.iter_mut().enumerate() {
            let at = 12 + k * 12;
            let f = |o: usize| tri.get(at + o..at + o + 4).map(|s| f32::from_le_bytes([s[0], s[1], s[2], s[3]]));
            let (Some(x), Some(y), Some(z)) = (f(0), f(4), f(8)) else {
                ok = false;
                break;
            };
            if !(x.is_finite() && y.is_finite() && z.is_finite()) {
                ok = false;
                break;
            }
            *corner = b.vertex([x.to_bits(), y.to_bits(), z.to_bits()], y_up([x, y, z]), None)?;
        }
        if ok {
            b.polygon(&corners, &mut budget)?;
        }
    }
    Scene { primitives: vec![b.finish()] }.finish()
}

fn read_ascii(bytes: &[u8]) -> Result<Scene> {
    let text = String::from_utf8_lossy(bytes);
    let mut words = text.split_ascii_whitespace();
    if !words.next().is_some_and(|w| w.eq_ignore_ascii_case("solid")) {
        return Err(invalid(FORMAT, "neither a binary STL nor one starting with `solid`"));
    }
    let mut b = Builder::new(Primitive::new("STL", DEFAULT_COLOR));
    let mut budget = Budget::default();
    let mut poly: Vec<u32> = Vec::new();
    let mut vertices = 0usize;
    while let Some(w) = words.next() {
        if w.eq_ignore_ascii_case("vertex") {
            let mut c = [0f32; 3];
            for v in &mut c {
                *v = words.next().and_then(parse_f32).ok_or_else(|| invalid(FORMAT, "a vertex needs three numbers"))?;
            }
            vertices += 1;
            if vertices > crate::MAX_VERTICES {
                return Err(crate::too_many_vertices());
            }
            poly.push(b.vertex([c[0].to_bits(), c[1].to_bits(), c[2].to_bits()], y_up(c), None)?);
        } else if w.eq_ignore_ascii_case("endloop") || w.eq_ignore_ascii_case("endfacet") {
            b.polygon(&poly, &mut budget)?;
            poly.clear();
        }
    }
    b.polygon(&poly, &mut budget)?;
    Scene { primitives: vec![b.finish()] }.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn binary(tris: &[[[f32; 3]; 3]], header: &[u8]) -> Vec<u8> {
        let mut v = header.to_vec();
        v.resize(80, b' ');
        v.extend((tris.len() as u32).to_le_bytes());
        for t in tris {
            v.extend([0u8; 12]);
            for p in t {
                for c in p {
                    v.extend(c.to_le_bytes());
                }
            }
            v.extend([0u8; 2]);
        }
        v
    }

    const TETRA: [[[f32; 3]; 3]; 4] = [
        [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        [[0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]],
        [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]],
    ];

    #[test]
    fn binary_shares_vertices_and_turns_z_up() {
        let s = read(&binary(&TETRA, b"tetra")).expect("read");
        assert_eq!(s.triangle_count(), 4);
        assert_eq!(s.vertex_count(), 4, "corners are shared");
        assert!(s.primitives.iter().all(Primitive::is_valid));
        // STL +Z (up) is glTF +Y.
        assert!(s.primitives[0].positions.contains(&[0.0, 1.0, -0.0]), "{:?}", s.primitives[0].positions);
    }

    #[test]
    fn binary_starting_with_solid_is_still_binary() {
        let s = read(&binary(&TETRA, b"solid exported by a CAD tool")).expect("read");
        assert_eq!(s.triangle_count(), 4);
    }

    #[test]
    fn ascii_with_quads_and_odd_case() {
        let text = "solid cube\n facet normal 0 0 1\n  OUTER LOOP\n   vertex 0 0 0\n   vertex 1 0 0\n   vertex 1 1 0\n   vertex 0 1 0\n  endloop\n endfacet\n\
                    facet normal 0 0 1\n outer loop\n vertex 0 0 1\n vertex 1e0 0 1\n vertex 1 1 1\n endloop\n endfacet\nendsolid cube\n";
        let s = read(text.as_bytes()).expect("read");
        assert_eq!(s.triangle_count(), 3, "the quad is a fan of two");
    }

    #[test]
    fn hostile_files_fail_cleanly() {
        // A count far beyond the data: the triangles present are kept.
        let mut v = binary(&TETRA[..1], b"");
        v[80..84].copy_from_slice(&1_000_000u32.to_le_bytes());
        assert_eq!(read(&v).map(|s| s.triangle_count()), Ok(1));
        // A count beyond the cap.
        v[80..84].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(read(&v), Err(Error::TooLarge(_))));
        // NaN corners are dropped.
        let nan = binary(&[[[f32::NAN, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]], b"");
        assert_eq!(read(&nan), Err(Error::Empty));
        assert!(read(b"solid x\nvertex 1 2\n").is_err());
        assert!(read(b"hello").is_err());
        assert_eq!(read(b"solid empty\nendsolid empty\n"), Err(Error::Empty));
    }
}
