//! PLY (Stanford polygon file) reader: ASCII, binary little- and big-endian.
//!
//! Read: vertex positions, normals, colours (`red`/`green`/`blue`/`alpha`, `r`/`g`/`b`,
//! `diffuse_red`…; bytes or floats) and texture coordinates (`s`/`t`, `u`/`v`, `texture_u`…);
//! faces (`vertex_indices` or `vertex_index`, any polygon) with optional per-corner `texcoord`
//! lists; the texture named by a `comment TextureFile` line. Other elements are skipped.

use crate::{Error, Material, Primitive, Resolve, Result, Scene, Texture, bad, srgb_to_linear};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Ty {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    F32,
    F64,
}

impl Ty {
    fn parse(s: &str) -> Option<Ty> {
        Some(match s {
            "char" | "int8" => Ty::I8,
            "uchar" | "uint8" => Ty::U8,
            "short" | "int16" => Ty::I16,
            "ushort" | "uint16" => Ty::U16,
            "int" | "int32" => Ty::I32,
            "uint" | "uint32" => Ty::U32,
            "float" | "float32" => Ty::F32,
            "double" | "float64" => Ty::F64,
            _ => return None,
        })
    }
    fn size(self) -> usize {
        match self {
            Ty::I8 | Ty::U8 => 1,
            Ty::I16 | Ty::U16 => 2,
            Ty::I32 | Ty::U32 | Ty::F32 => 4,
            Ty::F64 => 8,
        }
    }
    /// The largest value of an integer type (to scale colours); 1 for floats.
    fn unit(self) -> f64 {
        match self {
            Ty::U8 => 255.0,
            Ty::I8 => 127.0,
            Ty::U16 => 65535.0,
            Ty::I16 => 32767.0,
            Ty::U32 => u32::MAX as f64,
            Ty::I32 => i32::MAX as f64,
            Ty::F32 | Ty::F64 => 1.0,
        }
    }
}

#[derive(Clone, Debug)]
struct Prop {
    name: String,
    ty: Ty,
    /// The count type of a list property.
    list: Option<Ty>,
}

#[derive(Clone, Debug)]
struct Element {
    name: String,
    count: usize,
    props: Vec<Prop>,
}

#[derive(Clone, Copy, PartialEq)]
enum Enc {
    Ascii,
    Le,
    Be,
}

/// Reads values in any of the three encodings.
struct Values<'a> {
    enc: Enc,
    data: &'a [u8],
    at: usize,
    words: std::str::SplitAsciiWhitespace<'a>,
}

impl Values<'_> {
    fn next(&mut self, ty: Ty) -> Option<f64> {
        if self.enc == Enc::Ascii {
            return self.words.next()?.parse::<f64>().ok();
        }
        let n = ty.size();
        let b = self.data.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        let le = self.enc == Enc::Le;
        macro_rules! num {
            ($t:ty) => {{
                let a = b.try_into().ok()?;
                (if le { <$t>::from_le_bytes(a) } else { <$t>::from_be_bytes(a) }) as f64
            }};
        }
        Some(match ty {
            Ty::I8 => *b.first()? as i8 as f64,
            Ty::U8 => *b.first()? as f64,
            Ty::I16 => num!(i16),
            Ty::U16 => num!(u16),
            Ty::I32 => num!(i32),
            Ty::U32 => num!(u32),
            Ty::F32 => num!(f32),
            Ty::F64 => num!(f64),
        })
    }
}

pub fn read(bytes: &[u8], resolve: Resolve) -> Result<Scene> {
    let end = find(bytes, b"end_header").ok_or_else(|| bad("PLY without end_header"))?;
    let head = String::from_utf8_lossy(bytes.get(..end).unwrap_or(&[])).to_string();
    // The body starts after the end_header line's newline.
    let mut body = end + "end_header".len();
    while bytes.get(body).is_some_and(|c| *c == b'\r' || *c == b' ') {
        body += 1;
    }
    if bytes.get(body) == Some(&b'\n') {
        body += 1;
    }
    let mut enc = Enc::Ascii;
    let mut elements: Vec<Element> = Vec::new();
    let mut texture_file: Option<String> = None;
    for line in head.lines() {
        let mut t = line.split_whitespace();
        match t.next() {
            Some("format") => {
                enc = match t.next() {
                    Some("ascii") => Enc::Ascii,
                    Some("binary_little_endian") => Enc::Le,
                    Some("binary_big_endian") => Enc::Be,
                    other => return Err(bad(format!("PLY format {other:?}"))),
                }
            }
            Some("comment") | Some("obj_info") => {
                let rest: Vec<&str> = t.collect();
                if rest.first().is_some_and(|w| w.eq_ignore_ascii_case("TextureFile")) {
                    texture_file = Some(rest.get(1..).unwrap_or(&[]).join(" "));
                }
            }
            Some("element") => {
                let name = t.next().unwrap_or("").to_string();
                let count = t.next().and_then(|c| c.parse::<usize>().ok()).ok_or_else(|| bad("PLY element without count"))?;
                elements.push(Element { name, count, props: vec![] });
            }
            Some("property") => {
                let Some(e) = elements.last_mut() else { continue };
                let p = match t.next() {
                    Some("list") => {
                        let ct = t.next().and_then(Ty::parse).ok_or_else(|| bad("PLY list count type"))?;
                        let it = t.next().and_then(Ty::parse).ok_or_else(|| bad("PLY list item type"))?;
                        Prop { name: t.next().unwrap_or("").to_string(), ty: it, list: Some(ct) }
                    }
                    Some(ty) => {
                        Prop { name: t.next().unwrap_or("").to_string(), ty: Ty::parse(ty).ok_or_else(|| bad("PLY property type"))?, list: None }
                    }
                    None => continue,
                };
                e.props.push(p);
            }
            _ => {}
        }
    }
    let data = bytes.get(body..).unwrap_or(&[]);
    let text = if enc == Enc::Ascii { std::str::from_utf8(data).unwrap_or("") } else { "" };
    let mut v = Values { enc, data, at: 0, words: text.split_ascii_whitespace() };

    let mut pos: Vec<[f32; 3]> = Vec::new();
    let mut nrm: Vec<[f32; 3]> = Vec::new();
    let mut col: Vec<[f32; 4]> = Vec::new();
    let mut uv: Vec<[f32; 2]> = Vec::new();
    // Faces: corner vertex indices, and per-corner texture coordinates when the face has them.
    let mut faces: Vec<(Vec<u32>, Vec<[f32; 2]>)> = Vec::new();
    // Triangle indices the faces will make, over every face (each face's list is capped alone).
    let mut made = 0usize;
    for e in &elements {
        // An element without properties takes no bytes: skipping it is reading it (and its
        // count, which nothing bounds, could otherwise spin for ever).
        if e.props.is_empty() {
            continue;
        }
        let is_vertex = e.name == "vertex";
        let is_face = e.name == "face";
        if is_vertex && e.count > crate::MAX_VERTICES {
            return Err(Error::TooLarge);
        }
        if is_face && e.count > crate::MAX_INDICES / 3 {
            return Err(Error::TooLarge);
        }
        let find = |names: &[&str]| e.props.iter().position(|p| p.list.is_none() && names.contains(&p.name.as_str()));
        let (ix, iy, iz) = (find(&["x"]), find(&["y"]), find(&["z"]));
        let (inx, iny, inz) = (find(&["nx"]), find(&["ny"]), find(&["nz"]));
        let ir = find(&["red", "r", "diffuse_red"]);
        let ig = find(&["green", "g", "diffuse_green"]);
        let ib = find(&["blue", "b", "diffuse_blue"]);
        let ia = find(&["alpha", "a", "diffuse_alpha"]);
        let iu = find(&["s", "u", "texture_u", "texture_s", "tx"]);
        let iv = find(&["t", "v", "texture_v", "texture_t", "ty"]);
        let has_n = inx.is_some() && iny.is_some() && inz.is_some();
        let has_c = ir.is_some() && ig.is_some() && ib.is_some();
        let has_uv = iu.is_some() && iv.is_some();
        let mut vals: Vec<f64> = vec![0.0; e.props.len()];
        for _ in 0..e.count {
            let mut corners: Vec<u32> = Vec::new();
            let mut tc: Vec<[f32; 2]> = Vec::new();
            for (k, p) in e.props.iter().enumerate() {
                match p.list {
                    None => {
                        let x = v.next(p.ty).ok_or_else(|| bad("PLY data ends early"))?;
                        if let Some(slot) = vals.get_mut(k) {
                            *slot = x;
                        }
                    }
                    Some(ct) => {
                        let n = v.next(ct).ok_or_else(|| bad("PLY data ends early"))?;
                        let n = if n.is_finite() && (0.0..=1_000_000.0).contains(&n) { n as usize } else { return Err(bad("PLY list too long")) };
                        let wanted = is_face && matches!(p.name.as_str(), "vertex_indices" | "vertex_index");
                        let coords = is_face && p.name == "texcoord";
                        let mut items = Vec::with_capacity(if wanted || coords { n } else { 0 });
                        for _ in 0..n {
                            let x = v.next(p.ty).ok_or_else(|| bad("PLY data ends early"))?;
                            if wanted || coords {
                                items.push(x);
                            }
                        }
                        if wanted {
                            corners =
                                items.iter().map(|x| if x.is_finite() && *x >= 0.0 && *x < u32::MAX as f64 { *x as u32 } else { u32::MAX }).collect();
                            made = made.saturating_add(corners.len().saturating_sub(2).saturating_mul(3));
                            if made > crate::MAX_INDICES {
                                return Err(Error::TooLarge);
                            }
                        } else if coords {
                            tc = items.as_chunks::<2>().0.iter().map(|[u, v]| [*u as f32, 1.0 - *v as f32]).collect();
                        }
                    }
                }
            }
            if is_vertex {
                let g = |i: Option<usize>| i.and_then(|i| vals.get(i)).copied().unwrap_or(0.0);
                pos.push([g(ix) as f32, g(iy) as f32, g(iz) as f32]);
                if has_n {
                    nrm.push([g(inx) as f32, g(iny) as f32, g(inz) as f32]);
                }
                if has_c {
                    let unit = |i: Option<usize>| i.and_then(|i| e.props.get(i)).map(|p| p.ty.unit()).unwrap_or(1.0);
                    let c = |i: Option<usize>| srgb_to_linear((g(i) / unit(i)) as f32);
                    let a = if ia.is_some() { (g(ia) / unit(ia)) as f32 } else { 1.0 };
                    col.push([c(ir), c(ig), c(ib), a]);
                }
                if has_uv {
                    // PLY's t runs up from the bottom of the image.
                    uv.push([g(iu) as f32, 1.0 - g(iv) as f32]);
                }
            } else if is_face && corners.len() >= 3 {
                faces.push((corners, if tc.len() >= 3 { tc } else { vec![] }));
            }
        }
    }
    if faces.is_empty() {
        return Err(Error::Empty);
    }
    let texture = texture_file.filter(|f| !f.is_empty()).and_then(|f| resolve(&f)).and_then(Texture::decode);
    let per_corner_uv = texture.is_some() && faces.iter().any(|(_, tc)| !tc.is_empty());
    let mut material = Material::default_grey();
    if !col.is_empty() || texture.is_some() {
        material.base_color = [1.0; 4];
    }
    if texture.is_some() {
        material.texture = Some(0);
    }
    let prim = if per_corner_uv {
        // Texture coordinates per face corner: every corner becomes its own vertex.
        let mut p = Primitive { material: Some(0), ..Default::default() };
        for (corners, tc) in &faces {
            let base = p.positions.len() as u32;
            for (k, c) in corners.iter().enumerate() {
                let i = *c as usize;
                p.positions.push(pos.get(i).copied().unwrap_or([f32::NAN; 3]));
                if !nrm.is_empty() {
                    p.normals.push(nrm.get(i).copied().unwrap_or([0.0; 3]));
                }
                if !col.is_empty() {
                    p.colors.push(col.get(i).copied().unwrap_or([1.0; 4]));
                }
                p.uvs.push(tc.get(k).copied().unwrap_or([0.0; 2]));
            }
            for k in 1..corners.len() as u32 - 1 {
                p.indices.extend_from_slice(&[base, base + k, base + k + 1]);
            }
            if p.positions.len() > crate::MAX_VERTICES {
                return Err(Error::TooLarge);
            }
        }
        p
    } else {
        let mut indices = Vec::new();
        for (corners, _) in &faces {
            for k in 1..corners.len() - 1 {
                if let (Some(a), Some(b), Some(c)) = (corners.first(), corners.get(k), corners.get(k + 1)) {
                    indices.extend_from_slice(&[*a, *b, *c]);
                }
            }
        }
        let uvs = if texture.is_some() { uv } else { vec![] };
        Primitive { positions: pos, normals: nrm, uvs, colors: col, indices, material: Some(0) }
    };
    Ok(Scene { primitives: vec![prim], materials: vec![material], textures: texture.into_iter().collect() })
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).take(1 << 20).position(|w| w == needle)
}
