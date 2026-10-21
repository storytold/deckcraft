//! 3MF reader: the 3MF Consortium's core specification and its materials and properties
//! extension.
//!
//! Read: the root model part's objects (meshes and components), build items with their transforms,
//! the model unit, base materials (`displaycolor`), colour groups (per-vertex colours) and texture
//! coordinate groups with their textures. 3MF is Z-up; models are turned to glTF's Y-up and scaled
//! to metres. Composite materials, multi-properties, slices and beam lattices are skipped.

use std::collections::HashMap;
use std::io::{Cursor, Read};

use quick_xml::events::{BytesStart, Event};

use crate::util::hex_color;
use crate::{Error, Material, Primitive, Result, Scene, Texture, Wrap, bad};

const MAX_PART: u64 = 512 << 20;
const MAX_DEPTH: usize = 16;
/// Objects placed (components followed), over the whole build: fan-out and cycles stop here.
const MAX_PLACEMENTS: usize = 1_000_000;

/// A 3MF transform: `p' = [x y z 1] · M` with M's rows `m[0..3]` and translation `m[3]`.
type M43 = [[f64; 3]; 4];
const IDENTITY: M43 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, 0.0]];

#[derive(Default)]
struct Object {
    vertices: Vec<[f64; 3]>,
    /// v1, v2, v3, pid, p1, p2, p3.
    triangles: Vec<([u32; 3], Option<u32>, [Option<u32>; 3])>,
    pid: Option<u32>,
    pindex: Option<u32>,
    components: Vec<(u32, M43)>,
}

#[derive(Default)]
struct Model {
    unit: f64,
    base: HashMap<u32, Vec<[f32; 4]>>,
    colors: HashMap<u32, Vec<[f32; 4]>>,
    /// Texture id → (path, wrap u, wrap v).
    textures: HashMap<u32, (String, Wrap, Wrap)>,
    /// Coordinate group id → (texture id, coordinates).
    coords: HashMap<u32, (u32, Vec<[f32; 2]>)>,
    objects: HashMap<u32, Object>,
    build: Vec<(u32, M43)>,
}

pub fn read(bytes: &[u8]) -> Result<Scene> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| bad(format!("3MF: {e}")))?;
    if zip.len() > 10_000 {
        return Err(bad("3MF: too many parts"));
    }
    let names: Vec<String> = zip.file_names().map(String::from).collect();
    let part = |zip: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str| -> Option<Vec<u8>> {
        let want = name.trim_start_matches('/');
        let real = names.iter().find(|n| n.eq_ignore_ascii_case(want))?.clone();
        let f = zip.by_name(&real).ok()?;
        let mut out = Vec::new();
        f.take(MAX_PART).read_to_end(&mut out).ok()?;
        Some(out)
    };
    let root = part(&mut zip, "_rels/.rels")
        .and_then(|r| root_target(&r))
        .or_else(|| names.iter().find(|n| n.to_ascii_lowercase().ends_with(".model")).cloned())
        .ok_or_else(|| bad("3MF without a model part"))?;
    let xml = part(&mut zip, &root).ok_or_else(|| bad("3MF model part missing"))?;
    let model = parse(&xml)?;
    // Textures are read when triangles first use them (`prim_for`).
    let mut read = |path: &str| part(&mut zip, path);
    build(&model, &mut read)
}

/// The target of the package's 3-D model relationship.
fn root_target(rels: &[u8]) -> Option<String> {
    let mut r = quick_xml::Reader::from_reader(rels);
    let mut buf = Vec::new();
    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Start(e) | Event::Empty(e)) if e.local_name().as_ref() == b"Relationship" => {
                let ty = attr(&e, b"Type").unwrap_or_default();
                if ty.ends_with("/3dmodel") {
                    return attr(&e, b"Target");
                }
            }
            Ok(Event::Eof) | Err(_) => return None,
            _ => {}
        }
        buf.clear();
    }
}

fn attr(e: &BytesStart, name: &[u8]) -> Option<String> {
    e.attributes().with_checks(false).flatten().find(|a| a.key.local_name().as_ref() == name).map(|a| String::from_utf8_lossy(&a.value).into_owned())
}

fn num<T: std::str::FromStr>(e: &BytesStart, name: &[u8]) -> Option<T> {
    attr(e, name).and_then(|s| s.trim().parse::<T>().ok())
}

fn transform(s: Option<String>) -> M43 {
    let Some(s) = s else { return IDENTITY };
    let v: Vec<f64> = s.split_whitespace().filter_map(|t| t.parse::<f64>().ok()).filter(|f| f.is_finite()).collect();
    if v.len() != 12 {
        return IDENTITY;
    }
    let g = |i: usize| v.get(i).copied().unwrap_or(0.0);
    [[g(0), g(1), g(2)], [g(3), g(4), g(5)], [g(6), g(7), g(8)], [g(9), g(10), g(11)]]
}

/// `a` then `b`.
fn compose(a: &M43, b: &M43) -> M43 {
    let mut m = [[0.0; 3]; 4];
    for (r, row) in m.iter_mut().enumerate() {
        for (c, v) in row.iter_mut().enumerate() {
            *v = a[r][0] * b[0][c] + a[r][1] * b[1][c] + a[r][2] * b[2][c] + if r == 3 { b[3][c] } else { 0.0 };
        }
    }
    m
}

fn apply(m: &M43, p: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|c| p[0] * m[0][c] + p[1] * m[1][c] + p[2] * m[2][c] + m[3][c])
}

fn det(m: &M43) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

fn wrap(s: Option<String>) -> Wrap {
    match s.as_deref() {
        Some("mirror") => Wrap::Mirror,
        Some("clamp") | Some("none") => Wrap::Clamp,
        _ => Wrap::Repeat,
    }
}

fn parse(xml: &[u8]) -> Result<Model> {
    let mut r = quick_xml::Reader::from_reader(xml.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(xml));
    r.config_mut().check_end_names = false;
    let mut m = Model { unit: 0.001, ..Default::default() };
    let mut buf = Vec::new();
    // The group or object being read.
    let mut group: Option<(u8, u32)> = None;
    let mut object: Option<(u32, Object)> = None;
    let mut verts = 0usize;
    let mut tris = 0usize;
    loop {
        let ev = r.read_event_into(&mut buf).map_err(|e| bad(format!("3MF XML: {e}")))?;
        match &ev {
            Event::Start(e) | Event::Empty(e) => {
                let empty = matches!(ev, Event::Empty(_));
                match e.local_name().as_ref() {
                    b"model" => {
                        m.unit = match attr(e, b"unit").as_deref() {
                            Some("micron") => 1e-6,
                            Some("centimeter") => 0.01,
                            Some("inch") => 0.0254,
                            Some("foot") => 0.3048,
                            Some("meter") => 1.0,
                            _ => 0.001,
                        }
                    }
                    b"basematerials" if !empty => group = num(e, b"id").map(|id| (0, id)),
                    b"colorgroup" if !empty => group = num(e, b"id").map(|id| (1, id)),
                    b"texture2dgroup" if !empty => {
                        group = num(e, b"id").map(|id| (2, id));
                        if let (Some(id), Some(tex)) = (num::<u32>(e, b"id"), num::<u32>(e, b"texid")) {
                            m.coords.insert(id, (tex, vec![]));
                        }
                    }
                    b"base" => {
                        if let Some((0, id)) = group {
                            let c = attr(e, b"displaycolor").and_then(|s| hex_color(&s)).unwrap_or([0.8, 0.8, 0.8, 1.0]);
                            m.base.entry(id).or_default().push(c);
                        }
                    }
                    b"color" => {
                        if let Some((1, id)) = group {
                            let c = attr(e, b"color").and_then(|s| hex_color(&s)).unwrap_or([1.0; 4]);
                            m.colors.entry(id).or_default().push(c);
                        }
                    }
                    b"tex2coord" => {
                        if let Some((2, id)) = group
                            && let Some((_, list)) = m.coords.get_mut(&id)
                        {
                            // 3MF's v runs up from the bottom of the image.
                            list.push([num(e, b"u").unwrap_or(0.0), 1.0 - num::<f32>(e, b"v").unwrap_or(0.0)]);
                        }
                    }
                    b"texture2d" => {
                        if let (Some(id), Some(path)) = (num::<u32>(e, b"id"), attr(e, b"path")) {
                            m.textures.insert(id, (path, wrap(attr(e, b"tilestyleu")), wrap(attr(e, b"tilestylev"))));
                        }
                    }
                    b"object" if !empty => {
                        let o = Object { pid: num(e, b"pid"), pindex: num(e, b"pindex"), ..Default::default() };
                        object = num(e, b"id").map(|id| (id, o));
                    }
                    b"vertex" => {
                        if let Some((_, o)) = object.as_mut() {
                            verts += 1;
                            if verts > crate::MAX_VERTICES {
                                return Err(Error::TooLarge);
                            }
                            o.vertices.push([num(e, b"x").unwrap_or(0.0), num(e, b"y").unwrap_or(0.0), num(e, b"z").unwrap_or(0.0)]);
                        }
                    }
                    b"triangle" => {
                        if let Some((_, o)) = object.as_mut()
                            && let (Some(a), Some(b), Some(c)) = (num(e, b"v1"), num(e, b"v2"), num(e, b"v3"))
                        {
                            // Over every object, not each alone.
                            tris += 1;
                            if tris > crate::MAX_INDICES / 3 {
                                return Err(Error::TooLarge);
                            }
                            o.triangles.push(([a, b, c], num(e, b"pid"), [num(e, b"p1"), num(e, b"p2"), num(e, b"p3")]));
                        }
                    }
                    b"component" => {
                        if let (Some((_, o)), Some(id)) = (object.as_mut(), num::<u32>(e, b"objectid")) {
                            if attr(e, b"path").is_some() {
                                log::warn!("3MF: component in another model part skipped");
                            } else {
                                o.components.push((id, transform(attr(e, b"transform"))));
                            }
                        }
                    }
                    b"item" => {
                        if let Some(id) = num::<u32>(e, b"objectid") {
                            m.build.push((id, transform(attr(e, b"transform"))));
                        }
                    }
                    _ => {}
                }
            }
            Event::End(e) => match e.local_name().as_ref() {
                b"basematerials" | b"colorgroup" | b"texture2dgroup" => group = None,
                b"object" => {
                    if let Some((id, o)) = object.take() {
                        m.objects.insert(id, o);
                    }
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(m)
}

/// What a triangle is drawn with.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Key {
    Plain,
    Base(u32, u32),
    Colors,
    Texture(u32),
}

struct Out<'a> {
    scene: Scene,
    prims: HashMap<Key, usize>,
    tex_index: HashMap<u32, Option<usize>>,
    /// Reads a part of the package (a texture image).
    read: &'a mut dyn FnMut(&str) -> Option<Vec<u8>>,
    /// Texture images read so far, readable or not.
    tex_reads: usize,
    verts: usize,
    placements: usize,
    /// Triangle-list indices emitted so far, over every placement.
    indices: usize,
}

fn build(m: &Model, read: &mut dyn FnMut(&str) -> Option<Vec<u8>>) -> Result<Scene> {
    let mut out =
        Out { scene: Scene::default(), prims: HashMap::new(), tex_index: HashMap::new(), read, tex_reads: 0, verts: 0, placements: 0, indices: 0 };
    let items: Vec<(u32, M43)> = if m.build.is_empty() { m.objects.keys().map(|id| (*id, IDENTITY)).collect() } else { m.build.clone() };
    for (id, t) in items {
        emit(m, id, &t, 0, &mut out)?;
    }
    Ok(out.scene)
}

fn emit(m: &Model, id: u32, t: &M43, depth: usize, out: &mut Out) -> Result<()> {
    if depth > MAX_DEPTH {
        return Ok(());
    }
    out.placements += 1;
    if out.placements > MAX_PLACEMENTS {
        return Err(Error::TooLarge);
    }
    let Some(o) = m.objects.get(&id) else { return Ok(()) };
    // Counted before anything is pushed: placing one object many times repeats its triangles.
    out.indices = out.indices.saturating_add(o.triangles.len().saturating_mul(3));
    if out.indices > crate::MAX_INDICES {
        return Err(Error::TooLarge);
    }
    for (cid, ct) in &o.components {
        emit(m, *cid, &compose(ct, t), depth + 1, out)?;
    }
    if o.triangles.is_empty() {
        return Ok(());
    }
    let flip = det(t) < 0.0;
    // Units to metres, Z-up to Y-up: (x, y, z) → (x, z, −y).
    let place = |p: [f64; 3]| -> [f32; 3] {
        let q = apply(t, p);
        [(q[0] * m.unit) as f32, (q[2] * m.unit) as f32, (-q[1] * m.unit) as f32]
    };
    // Each object vertex's index in each indexed primitive.
    let mut shared: HashMap<(Key, u32), u32> = HashMap::new();
    for (v, pid, ps) in &o.triangles {
        let pid = pid.or(o.pid);
        let p1 = ps[0].or(o.pindex).unwrap_or(0);
        let key = match pid {
            Some(g) if m.base.contains_key(&g) => Key::Base(g, p1),
            Some(g) if m.colors.contains_key(&g) => Key::Colors,
            Some(g) if m.coords.contains_key(&g) => match m.coords.get(&g).map(|c| c.0) {
                Some(tex) => Key::Texture(tex),
                None => Key::Plain,
            },
            _ => Key::Plain,
        };
        let pi = prim_for(m, key, out);
        let order: [usize; 3] = if flip { [0, 2, 1] } else { [0, 1, 2] };
        let Some(prim) = out.scene.primitives.get_mut(pi) else { continue };
        for k in order {
            let vi = v[k];
            let Some(p) = o.vertices.get(vi as usize) else { break };
            let corner_prop = ps[k].unwrap_or(p1);
            match key {
                Key::Plain | Key::Base(..) => {
                    let idx = match shared.get(&(key, vi)) {
                        Some(i) => *i,
                        None => {
                            let i = prim.positions.len() as u32;
                            prim.positions.push(place(*p));
                            shared.insert((key, vi), i);
                            out.verts += 1;
                            i
                        }
                    };
                    prim.indices.push(idx);
                }
                Key::Colors => {
                    let c = pid.and_then(|g| m.colors.get(&g)).and_then(|l| l.get(corner_prop as usize)).copied().unwrap_or([1.0; 4]);
                    prim.indices.push(prim.positions.len() as u32);
                    prim.positions.push(place(*p));
                    prim.colors.push(c);
                    out.verts += 1;
                }
                Key::Texture(_) => {
                    let uv = pid.and_then(|g| m.coords.get(&g)).and_then(|(_, l)| l.get(corner_prop as usize)).copied().unwrap_or([0.0; 2]);
                    prim.indices.push(prim.positions.len() as u32);
                    prim.positions.push(place(*p));
                    prim.uvs.push(uv);
                    out.verts += 1;
                }
            }
        }
        // A triangle with a bad vertex index: drop what was pushed of it.
        let rem = prim.indices.len() % 3;
        if rem != 0 {
            prim.indices.truncate(prim.indices.len() - rem);
        }
        if out.verts > crate::MAX_VERTICES {
            return Err(Error::TooLarge);
        }
    }
    Ok(())
}

/// The primitive (created on first use) for triangles drawn with `key`.
fn prim_for(m: &Model, key: Key, out: &mut Out) -> usize {
    if let Some(i) = out.prims.get(&key) {
        return *i;
    }
    let material = match key {
        Key::Plain => Material::default_grey(),
        Key::Base(g, i) => {
            let c = m.base.get(&g).and_then(|l| l.get(i as usize)).copied().unwrap_or([0.8, 0.8, 0.8, 1.0]);
            Material { base_color: c, alpha: if c[3] < 0.999 { crate::Alpha::Blend } else { crate::Alpha::Opaque }, ..Default::default() }
        }
        Key::Colors => Material::default(),
        Key::Texture(t) => {
            let ti = match out.tex_index.get(&t) {
                Some(i) => *i,
                None => {
                    let tex = match m.textures.get(&t) {
                        Some((path, ws, wt)) if out.scene.textures.len() < crate::MAX_TEXTURES && out.tex_reads < crate::MAX_TEXTURES * 4 => {
                            out.tex_reads += 1;
                            let tex = (out.read)(path).and_then(Texture::decode);
                            if tex.is_none() {
                                log::warn!("3MF: texture {path} missing or unreadable");
                            }
                            tex.map(|mut x| {
                                (x.wrap_s, x.wrap_t) = (*ws, *wt);
                                x
                            })
                        }
                        _ => None,
                    };
                    let i = tex.map(|tex| {
                        out.scene.textures.push(tex);
                        out.scene.textures.len() - 1
                    });
                    out.tex_index.insert(t, i);
                    i
                }
            };
            Material { texture: ti, ..Default::default() }
        }
    };
    out.scene.materials.push(material);
    out.scene.primitives.push(Primitive { material: Some(out.scene.materials.len() - 1), ..Default::default() });
    let i = out.scene.primitives.len() - 1;
    out.prims.insert(key, i);
    i
}
