//! FBX meshes, from binary (6.1–7.7) and ASCII files.
//!
//! Both forms parse to the same tree of [`Node`]s (name, properties, children). From it:
//!
//! - `Objects/Geometry` (7.x) or `Objects/Model` holding `Vertices` (6.x): `Vertices`,
//!   `PolygonVertexIndex` (a negative index `!i` closes a polygon), the first
//!   `LayerElementNormal` (`ByPolygonVertex` or `ByVertice`, `Direct` or `IndexToDirect`) and the
//!   first `LayerElementMaterial` (`AllSame` or `ByPolygon`).
//! - `Objects/Model`: `Lcl Translation`, `PreRotation`, `Lcl Rotation` (XYZ Euler) and
//!   `Lcl Scaling`, chained through parent models. Pivots and rotation offsets are left out.
//! - `Objects/Material`: `DiffuseColor` (or 6.x `Diffuse`), matched to a polygon's material slot
//!   in the order the materials connect to its model.
//! - `Connections` (`C` or 6.x `Connect`, object-to-object): geometry to model, material to model,
//!   model to parent model.
//! - `GlobalSettings`: a Z-up file (`UpAxis` 2) is turned to Y-up.

mod ascii;
mod binary;

use std::collections::HashMap;

use crate::math::{Mat4, apply3};
use crate::{Budget, Builder, DEFAULT_COLOR, Error, Primitive, Result, Scene};

/// The first bytes of a binary FBX file.
pub const BINARY_MAGIC: &[u8] = b"Kaydara FBX Binary  \0";

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Prop {
    Int(i64),
    Float(f64),
    Str(String),
    /// Raw bytes (only their length is kept).
    Bytes(usize),
    Ints(Vec<i64>),
    Floats(Vec<f64>),
}

impl Prop {
    fn num(&self) -> Option<f64> {
        match self {
            Prop::Int(i) => Some(*i as f64),
            Prop::Float(f) => Some(*f),
            _ => None,
        }
    }

    fn str(&self) -> Option<&str> {
        match self {
            Prop::Str(s) => Some(s),
            _ => None,
        }
    }

    /// An object id or name, as the key connections use.
    fn key(&self) -> Option<String> {
        match self {
            Prop::Int(i) => Some(i.to_string()),
            Prop::Str(s) => Some(s.clone()),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Node {
    pub name: String,
    pub props: Vec<Prop>,
    pub children: Vec<Node>,
}

impl Node {
    fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }

    fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }

    fn first_str(&self) -> Option<&str> {
        self.props.first().and_then(Prop::str)
    }

    /// An array's values: a binary array property, an ASCII `a:` child, or plain values.
    fn floats(&self) -> Vec<f64> {
        for p in &self.props {
            match p {
                Prop::Floats(v) => return v.clone(),
                Prop::Ints(v) => return v.iter().map(|&i| i as f64).collect(),
                _ => {}
            }
        }
        let from = self.child("a").unwrap_or(self);
        from.props.iter().filter_map(Prop::num).collect()
    }

    fn ints(&self) -> Vec<i64> {
        for p in &self.props {
            match p {
                Prop::Ints(v) => return v.clone(),
                Prop::Floats(v) => return v.iter().map(|&f| f as i64).collect(),
                _ => {}
            }
        }
        let from = self.child("a").unwrap_or(self);
        from.props
            .iter()
            .filter_map(|p| match p {
                Prop::Int(i) => Some(*i),
                Prop::Float(f) => Some(*f as i64),
                _ => None,
            })
            .collect()
    }

    /// Properties by name, with their numeric values (`Properties70/P` or 6.x
    /// `Properties60/Property`).
    fn properties(&self) -> HashMap<String, Vec<f64>> {
        let mut out = HashMap::new();
        for (block, item) in [("Properties70", "P"), ("Properties60", "Property")] {
            for b in self.children_named(block) {
                for p in b.children_named(item) {
                    if let Some(name) = p.first_str() {
                        out.insert(name.to_string(), p.props.iter().skip(1).filter_map(Prop::num).filter(|v| v.is_finite()).collect());
                    }
                }
            }
        }
        out
    }
}

fn parse(bytes: &[u8]) -> Result<Vec<Node>> {
    if bytes.starts_with(BINARY_MAGIC) { binary::parse(bytes) } else { ascii::parse(bytes) }
}

/// `Name\0\x01Model` (binary) or `Model::Name` (ASCII) to `Name`.
fn display_name(raw: &str) -> String {
    let n = raw.split('\0').next().unwrap_or(raw);
    n.split_once("::").map_or(n, |(_, rest)| rest).to_string()
}

fn vec3(props: &HashMap<String, Vec<f64>>, name: &str, default: f64) -> [f64; 3] {
    match props.get(name).map(Vec::as_slice) {
        Some([x, y, z, ..]) => [*x, *y, *z],
        _ => [default; 3],
    }
}

struct Doc<'a> {
    models: HashMap<String, &'a Node>,
    model_parent: HashMap<String, String>,
    geometry_owner: HashMap<String, String>,
    model_materials: HashMap<String, Vec<[f32; 4]>>,
}

impl Doc<'_> {
    fn local(model: &Node) -> Mat4 {
        let p = model.properties();
        let t = vec3(&p, "Lcl Translation", 0.0);
        let pre = vec3(&p, "PreRotation", 0.0);
        let r = vec3(&p, "Lcl Rotation", 0.0);
        let s = vec3(&p, "Lcl Scaling", 1.0);
        Mat4::translation(t).mul(&Mat4::euler_xyz(pre)).mul(&Mat4::euler_xyz(r)).mul(&Mat4::scale(s))
    }

    fn world(&self, key: &str) -> Mat4 {
        let mut chain = Vec::new();
        let mut at = Some(key.to_string());
        while let Some(k) = at {
            if chain.len() > crate::MAX_DEPTH || chain.contains(&k) {
                break;
            }
            at = self.model_parent.get(&k).cloned();
            chain.push(k);
        }
        // Root first.
        chain.iter().rev().filter_map(|k| self.models.get(k)).fold(Mat4::IDENTITY, |m, model| m.mul(&Self::local(model)))
    }
}

pub fn read(bytes: &[u8]) -> Result<Scene> {
    let top = parse(bytes)?;
    let find = |name: &str| top.iter().find(|n| n.name == name);
    let objects = find("Objects").ok_or_else(|| crate::invalid("FBX", "no `Objects` section"))?;

    let mut doc = Doc { models: HashMap::new(), model_parent: HashMap::new(), geometry_owner: HashMap::new(), model_materials: HashMap::new() };
    let mut geometries: Vec<(String, &Node)> = Vec::new();
    let mut materials: HashMap<String, [f32; 4]> = HashMap::new();
    for o in &objects.children {
        let Some(key) = o.props.first().and_then(Prop::key) else { continue };
        match o.name.as_str() {
            "Model" => {
                doc.models.insert(key.clone(), o);
                if o.child("Vertices").is_some() {
                    // 6.x: the model holds its own mesh.
                    doc.geometry_owner.insert(key.clone(), key.clone());
                    geometries.push((key, o));
                }
            }
            "Geometry" if o.child("Vertices").is_some() => geometries.push((key, o)),
            "Material" => {
                let p = o.properties();
                let c = p.get("DiffuseColor").or_else(|| p.get("Diffuse"));
                let color = match c.map(Vec::as_slice) {
                    Some([r, g, b, ..]) => [r.clamp(0.0, 1.0) as f32, g.clamp(0.0, 1.0) as f32, b.clamp(0.0, 1.0) as f32, 1.0],
                    _ => DEFAULT_COLOR,
                };
                materials.insert(key, color);
            }
            _ => {}
        }
    }
    let geometry_keys: std::collections::HashSet<&str> = geometries.iter().map(|(k, _)| k.as_str()).collect();
    if let Some(conns) = find("Connections") {
        for c in conns.children.iter().filter(|c| c.name == "C" || c.name == "Connect") {
            if c.first_str() != Some("OO") {
                continue;
            }
            let (Some(child), Some(parent)) = (c.props.get(1).and_then(Prop::key), c.props.get(2).and_then(Prop::key)) else { continue };
            if !doc.models.contains_key(&parent) {
                continue;
            }
            if doc.models.contains_key(&child) {
                // A model under a model (a 6.x model is its own geometry too).
                doc.model_parent.entry(child).or_insert(parent);
            } else if geometry_keys.contains(child.as_str()) {
                doc.geometry_owner.entry(child).or_insert(parent);
            } else if let Some(&color) = materials.get(&child) {
                doc.model_materials.entry(parent).or_default().push(color);
            }
        }
    }

    let up = find("GlobalSettings").map(Node::properties).and_then(|p| p.get("UpAxis").and_then(|v| v.first().copied()));
    let axis = if up == Some(2.0) { Mat4::axis_rotation(0, -90.0) } else { Mat4::IDENTITY };

    let mut budget = Budget::default();
    let mut vertices = 0usize;
    let mut primitives = Vec::new();
    for (key, g) in &geometries {
        let owner = doc.geometry_owner.get(key);
        let world = axis.mul(&owner.map_or(Mat4::IDENTITY, |m| doc.world(m)));
        if !world.is_finite() {
            continue;
        }
        let name = owner
            .and_then(|m| doc.models.get(m))
            .and_then(|m| m.props.iter().filter_map(Prop::str).find(|s| s.contains("::") || s.contains('\0')))
            .map_or_else(|| "Mesh".to_string(), display_name);
        let colors = owner.and_then(|m| doc.model_materials.get(m)).cloned().unwrap_or_default();
        let prims = geometry(g, &world, &name, &colors, &mut budget)?;
        vertices += prims.iter().map(|p| p.positions.len()).sum::<usize>();
        if vertices > crate::MAX_VERTICES {
            return Err(crate::too_many_vertices());
        }
        primitives.extend(prims);
    }
    if geometries.is_empty() {
        return Err(Error::Empty);
    }
    Scene { primitives }.finish()
}

/// How a layer element maps onto the mesh.
#[derive(Clone, Copy, PartialEq)]
enum Mapping {
    PolygonVertex,
    Vertex,
    Polygon,
    AllSame,
}

fn mapping(layer: &Node) -> Option<Mapping> {
    match layer.child("MappingInformationType")?.first_str()? {
        "ByPolygonVertex" => Some(Mapping::PolygonVertex),
        "ByVertice" | "ByVertex" | "ByControlPoint" => Some(Mapping::Vertex),
        "ByPolygon" => Some(Mapping::Polygon),
        "AllSame" => Some(Mapping::AllSame),
        _ => None,
    }
}

fn indexed(layer: &Node) -> bool {
    matches!(layer.child("ReferenceInformationType").and_then(Node::first_str), Some("IndexToDirect" | "Index"))
}

/// One geometry's triangles, a primitive per material slot.
fn geometry(g: &Node, world: &Mat4, name: &str, colors: &[[f32; 4]], budget: &mut Budget) -> Result<Vec<Primitive>> {
    let raw = g.child("Vertices").map(Node::floats).unwrap_or_default();
    if raw.len() / 3 > crate::MAX_VERTICES {
        return Err(crate::too_many_vertices());
    }
    let positions: Vec<[f32; 3]> = raw.as_chunks::<3>().0.iter().map(|[x, y, z]| world.point([*x as f32, *y as f32, *z as f32])).collect();
    if positions.iter().flatten().any(|v| !v.is_finite()) {
        return Err(crate::invalid("FBX", "a vertex is not a finite number"));
    }
    let pvi = g.child("PolygonVertexIndex").map(Node::ints).unwrap_or_default();

    let normal_layer = g.child("LayerElementNormal");
    let normals: Vec<[f32; 3]> = normal_layer
        .and_then(|l| l.child("Normals"))
        .map(Node::floats)
        .unwrap_or_default()
        .as_chunks::<3>()
        .0
        .iter()
        .map(|[x, y, z]| [*x as f32, *y as f32, *z as f32])
        .collect();
    let normal_index = normal_layer.and_then(|l| l.child("NormalsIndex").or_else(|| l.child("NormalIndex"))).map(Node::ints).unwrap_or_default();
    let normal_mapping = normal_layer.and_then(mapping);
    let normal_indexed = normal_layer.is_some_and(indexed);
    let nm = world.normal_matrix();
    let normal_at = |corner: usize, vertex: usize| -> Option<usize> {
        let at = match normal_mapping? {
            Mapping::PolygonVertex => corner,
            Mapping::Vertex => vertex,
            _ => return None,
        };
        let i = if normal_indexed { usize::try_from(*normal_index.get(at)?).ok()? } else { at };
        (i < normals.len()).then_some(i)
    };

    let mat_layer = g.child("LayerElementMaterial");
    let mat_mapping = mat_layer.and_then(mapping);
    let slots = mat_layer.and_then(|l| l.child("Materials")).map(Node::ints).unwrap_or_default();
    let slot_of = |polygon: usize| -> i64 {
        match mat_mapping {
            Some(Mapping::Polygon) => slots.get(polygon).copied().unwrap_or(0),
            _ => slots.first().copied().unwrap_or(0),
        }
    };

    let mirrored = world.det3() < 0.0;
    let mut builders: Vec<(i64, Builder<(usize, Option<usize>)>)> = Vec::new();
    let mut poly: Vec<(usize, Option<usize>)> = Vec::new();
    let mut corners: Vec<u32> = Vec::new();
    let mut polygon = 0usize;
    for (corner, &raw_index) in pvi.iter().enumerate() {
        let last = raw_index < 0;
        let v = if last { !raw_index } else { raw_index };
        match usize::try_from(v).ok().filter(|&v| v < positions.len()) {
            Some(v) => poly.push((v, normal_at(corner, v))),
            None => log::warn!("fbx: polygon index {raw_index} is out of range"),
        }
        if !last {
            continue;
        }
        let slot = slot_of(polygon);
        polygon += 1;
        let b = match builders.iter().position(|(s, _)| *s == slot) {
            Some(i) => builders.get_mut(i),
            None => {
                let color = usize::try_from(slot).ok().and_then(|s| colors.get(s)).copied().unwrap_or(DEFAULT_COLOR);
                let label = if slot == 0 { name.to_string() } else { format!("{name} {slot}") };
                builders.push((slot, Builder::new(Primitive::new(label, color))));
                builders.last_mut()
            }
        };
        let Some((_, b)) = b else { continue };
        corners.clear();
        for &(v, n) in &poly {
            let (Some(&pos), normal) = (positions.get(v), n.and_then(|n| normals.get(n)).zip(nm.as_ref()).map(|(n, m)| apply3(m, *n))) else {
                continue;
            };
            corners.push(b.vertex((v, n), pos, normal)?);
        }
        if mirrored {
            corners.reverse();
        }
        b.polygon(&corners, budget)?;
        poly.clear();
    }
    Ok(builders.into_iter().map(|(_, b)| b.finish()).collect())
}

#[cfg(test)]
mod tests;
