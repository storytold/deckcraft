//! STL reader: binary and ASCII. Geometry only: STL has no materials, so the model gets the plain
//! grey, and its facet normals (often wrong in the wild) are ignored in favour of the triangles'
//! own: every facet is drawn flat.

use crate::{Error, Material, Primitive, Result, Scene, bad};

pub fn read(bytes: &[u8]) -> Result<Scene> {
    let positions = if is_binary(bytes) { binary(bytes)? } else { ascii(bytes)? };
    let n = positions.len();
    let prim = Primitive { positions, indices: (0..n as u32).collect(), material: Some(0), ..Default::default() };
    Ok(Scene { primitives: vec![prim], materials: vec![Material::default_grey()], textures: vec![] })
}

fn count(bytes: &[u8]) -> Option<usize> {
    bytes.get(80..84).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes).map(|n| n as usize)
}

/// Binary when the triangle count matches the length exactly; ASCII when it starts with `solid`
/// and has facets; otherwise binary again (some writers pad the end).
fn is_binary(bytes: &[u8]) -> bool {
    if let Some(n) = count(bytes)
        && n.checked_mul(50).and_then(|b| b.checked_add(84)) == Some(bytes.len())
    {
        return true;
    }
    let head = String::from_utf8_lossy(bytes.get(..bytes.len().min(1024)).unwrap_or(&[])).to_string();
    !(head.trim_start().starts_with("solid") && (head.contains("facet") || head.contains("endsolid")))
}

fn binary(bytes: &[u8]) -> Result<Vec<[f32; 3]>> {
    let n = count(bytes).ok_or_else(|| bad("STL too short"))?;
    // A count larger than the file holds: read what is there.
    let n = n.min(bytes.len().saturating_sub(84) / 50);
    if n.saturating_mul(3) > crate::MAX_VERTICES {
        return Err(Error::TooLarge);
    }
    let mut out = Vec::with_capacity(n * 3);
    for t in 0..n {
        let base = 84 + t * 50 + 12;
        for v in 0..3 {
            let at = base + v * 12;
            let f = |k: usize| bytes.get(at + k * 4..at + k * 4 + 4).and_then(|b| b.try_into().ok()).map(f32::from_le_bytes).unwrap_or(0.0);
            out.push([f(0), f(1), f(2)]);
        }
    }
    Ok(out)
}

fn ascii(bytes: &[u8]) -> Result<Vec<[f32; 3]>> {
    let text = String::from_utf8_lossy(bytes);
    let mut out = Vec::new();
    let mut facet: Vec<[f32; 3]> = Vec::with_capacity(3);
    let mut toks = text.split_whitespace();
    while let Some(t) = toks.next() {
        match t {
            "vertex" => {
                let mut f = [0f32; 3];
                for c in f.iter_mut() {
                    *c = toks.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                }
                facet.push(f);
            }
            "endfacet" | "endloop" => {
                // Facets are triangles; a polygon loop (rare) is fanned.
                if facet.len() >= 3 {
                    for k in 1..facet.len() - 1 {
                        if let (Some(a), Some(b), Some(c)) = (facet.first(), facet.get(k), facet.get(k + 1)) {
                            out.extend_from_slice(&[*a, *b, *c]);
                        }
                    }
                }
                facet.clear();
                if out.len() > crate::MAX_VERTICES {
                    return Err(Error::TooLarge);
                }
            }
            _ => {}
        }
    }
    if out.is_empty() {
        return Err(bad("STL without facets"));
    }
    Ok(out)
}
