//! DeckCraft meshes: 3-D model files to glTF binary (`.glb`), the form a presentation embeds.
//!
//! - [`stl::read`]: binary and ASCII STL. STL is Z-up (CAD and 3-D printing), so it is turned
//!   to glTF's Y-up.
//! - [`obj::read`]: Wavefront OBJ, with diffuse colours from the `.mtl` libraries it names
//!   (read through a caller-supplied lookup, so the crate never touches the file system).
//! - [`fbx::read`]: binary (6.1–7.7) and ASCII FBX meshes, their model transforms and diffuse
//!   colours. Animation, skinning, cameras, lights and textures are left out.
//! - [`glb::read`] / [`glb::write`]: glTF 2.0 binary, the triangles of its default scene.
//! - [`import`]: any of these to `.glb` bytes plus the model's bounds.
//! - [`raster::render`]: a software z-buffer picture of a model through a [`raster::View`] (the
//!   slide drawing and the fallback picture a `.pptx` carries for older readers).
//!
//! Input is hostile: every count, offset and index is checked, allocations are capped
//! ([`MAX_TRIANGLES`], [`MAX_VERTICES`]) and recursion is bounded.
//!
//! Layer L0: no workspace dependencies. Builds for `wasm32-unknown-unknown`.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod fbx;
pub mod glb;
mod math;
pub mod obj;
pub mod raster;
pub mod stl;

use std::collections::HashMap;

/// Most triangles a model may have.
pub const MAX_TRIANGLES: usize = 4_000_000;
/// Most vertices a model may have.
pub const MAX_VERTICES: usize = 12_000_000;
/// Most nested nodes (FBX records, glTF scene nodes) followed.
pub const MAX_DEPTH: usize = 64;

/// The colour of a surface with no material: a light neutral grey.
pub const DEFAULT_COLOR: [f32; 4] = [0.8, 0.8, 0.8, 1.0];

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum Error {
    #[error("{0} is not a 3-D model DeckCraft can read (use .stl, .obj, .fbx or .glb)")]
    UnknownFormat(String),
    #[error("not a valid {format} file: {why}")]
    Invalid { format: &'static str, why: String },
    #[error("the model has no triangles")]
    Empty,
    #[error("the model is too large: {0}")]
    TooLarge(String),
    #[error("{0}")]
    Unsupported(String),
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn invalid(format: &'static str, why: impl Into<String>) -> Error {
    Error::Invalid { format, why: why.into() }
}

/// A file format [`import`] reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Stl,
    Obj,
    Fbx,
    Glb,
}

impl Format {
    pub fn name(self) -> &'static str {
        match self {
            Format::Stl => "STL",
            Format::Obj => "OBJ",
            Format::Fbx => "FBX",
            Format::Glb => "glTF binary",
        }
    }

    /// The format of `bytes`, from their signature first and then from the name's extension.
    pub fn detect(name: &str, bytes: &[u8]) -> Option<Format> {
        if bytes.starts_with(b"glTF") {
            return Some(Format::Glb);
        }
        if bytes.starts_with(fbx::BINARY_MAGIC) {
            return Some(Format::Fbx);
        }
        let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
        match ext.as_str() {
            "stl" => Some(Format::Stl),
            "obj" => Some(Format::Obj),
            "fbx" => Some(Format::Fbx),
            "glb" => Some(Format::Glb),
            _ => None,
        }
    }
}

/// Triangles that share one material.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Primitive {
    pub name: String,
    pub positions: Vec<[f32; 3]>,
    /// One per position, or empty (the surface is then drawn flat).
    pub normals: Vec<[f32; 3]>,
    /// Three per triangle, each `< positions.len()`.
    pub indices: Vec<u32>,
    /// Linear RGBA base colour.
    pub color: [f32; 4],
}

impl Primitive {
    pub fn new(name: impl Into<String>, color: [f32; 4]) -> Self {
        Primitive { name: name.into(), color, ..Default::default() }
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Whether the indices, normals and numbers hold together (what the writer relies on).
    pub fn is_valid(&self) -> bool {
        let n = self.positions.len();
        self.indices.len().is_multiple_of(3)
            && (self.normals.is_empty() || self.normals.len() == n)
            && self.indices.iter().all(|&i| (i as usize) < n)
            && self.positions.iter().chain(&self.normals).all(|p| p.iter().all(|c| c.is_finite()))
            && self.color.iter().all(|c| c.is_finite())
    }
}

/// A model: its primitives in world space (Y up).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scene {
    pub primitives: Vec<Primitive>,
}

impl Scene {
    pub fn triangle_count(&self) -> usize {
        self.primitives.iter().map(Primitive::triangle_count).sum()
    }

    pub fn vertex_count(&self) -> usize {
        self.primitives.iter().map(|p| p.positions.len()).sum()
    }

    /// The axis-aligned box around every referenced vertex, or `None` with no triangles.
    pub fn bounds(&self) -> Option<Bounds> {
        let mut b: Option<Bounds> = None;
        for p in &self.primitives {
            for &i in &p.indices {
                if let Some(v) = p.positions.get(i as usize) {
                    b = Some(match b {
                        None => Bounds { min: *v, max: *v },
                        Some(b) => b.with(*v),
                    });
                }
            }
        }
        b
    }

    /// Drops primitives with no triangles and fails when nothing is left.
    pub(crate) fn finish(mut self) -> Result<Scene> {
        self.primitives.retain(|p| !p.indices.is_empty());
        if self.primitives.is_empty() {
            return Err(Error::Empty);
        }
        Ok(self)
    }
}

/// An axis-aligned box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl Bounds {
    fn with(self, v: [f32; 3]) -> Bounds {
        Bounds {
            min: [self.min[0].min(v[0]), self.min[1].min(v[1]), self.min[2].min(v[2])],
            max: [self.max[0].max(v[0]), self.max[1].max(v[1]), self.max[2].max(v[2])],
        }
    }

    pub fn size(&self) -> [f32; 3] {
        [self.max[0] - self.min[0], self.max[1] - self.min[1], self.max[2] - self.min[2]]
    }
}

/// Builds a primitive from corners, sharing a vertex between corners with the same key.
pub(crate) struct Builder<K> {
    pub prim: Primitive,
    map: HashMap<K, u32>,
    /// Some corner had no normal: the primitive's normals are dropped at the end.
    missing_normal: bool,
}

impl<K: std::hash::Hash + Eq> Builder<K> {
    pub fn new(prim: Primitive) -> Self {
        Builder { prim, map: HashMap::new(), missing_normal: false }
    }

    /// The index of the vertex for `key`, adding it (with `normal`) the first time.
    pub fn vertex(&mut self, key: K, pos: [f32; 3], normal: Option<[f32; 3]>) -> Result<u32> {
        if let Some(&i) = self.map.get(&key) {
            return Ok(i);
        }
        let i = u32::try_from(self.prim.positions.len()).map_err(|_| too_many_vertices())?;
        self.prim.positions.push(pos);
        match normal {
            Some(n) if !self.missing_normal => self.prim.normals.push(n),
            Some(_) => {}
            None => {
                self.missing_normal = true;
                self.prim.normals.clear();
            }
        }
        self.map.insert(key, i);
        Ok(i)
    }

    /// Adds a polygon as a triangle fan.
    pub fn polygon(&mut self, corners: &[u32], budget: &mut Budget) -> Result<()> {
        let Some((&first, rest)) = corners.split_first() else { return Ok(()) };
        for pair in rest.windows(2) {
            if let [b, c] = pair {
                budget.triangle()?;
                self.prim.indices.extend([first, *b, *c]);
            }
        }
        Ok(())
    }

    pub fn finish(mut self) -> Primitive {
        if self.missing_normal {
            self.prim.normals.clear();
        }
        self.prim
    }
}

/// Counts triangles and vertices across a whole model against the caps.
#[derive(Default)]
pub(crate) struct Budget {
    triangles: usize,
}

impl Budget {
    pub fn triangle(&mut self) -> Result<()> {
        self.triangles += 1;
        if self.triangles > MAX_TRIANGLES {
            return Err(Error::TooLarge(format!("more than {MAX_TRIANGLES} triangles")));
        }
        Ok(())
    }
}

pub(crate) fn too_many_vertices() -> Error {
    Error::TooLarge(format!("more than {MAX_VERTICES} vertices"))
}

/// A model read and converted for embedding.
#[derive(Clone, Debug, PartialEq)]
pub struct Imported {
    pub format: Format,
    /// The model as glTF binary. A `.glb` input is kept byte for byte (textures and all).
    pub glb: Vec<u8>,
    /// Triangles read (0 when a `.glb` is kept but its meshes could not be read).
    pub triangles: usize,
    /// Its extent, Y up, when its meshes could be read.
    pub bounds: Option<Bounds>,
    /// Its width and height on screen in [`raster::View::inserted`], when its meshes could be
    /// read: the proportions of the frame it is best inserted in.
    pub view_size: Option<(f32, f32)>,
}

impl Imported {
    fn of(format: Format, glb: Vec<u8>, scene: &Scene) -> Imported {
        Imported {
            format,
            glb,
            triangles: scene.triangle_count(),
            bounds: scene.bounds(),
            view_size: raster::projected_size(scene, &raster::View::inserted()),
        }
    }
}

/// Reads the 3-D model in `bytes` (named `name`) and converts it to `.glb`.
///
/// `sidecar` returns the bytes of a file the model names next to itself (an OBJ's `.mtl`
/// libraries), or `None` when it can't be read; materials then fall back to grey.
pub fn import(name: &str, bytes: &[u8], sidecar: &mut dyn FnMut(&str) -> Option<Vec<u8>>) -> Result<Imported> {
    let format = Format::detect(name, bytes).ok_or_else(|| Error::UnknownFormat(name.to_string()))?;
    let scene = match format {
        Format::Stl => stl::read(bytes)?,
        Format::Obj => obj::read(bytes, sidecar)?,
        Format::Fbx => fbx::read(bytes)?,
        Format::Glb => {
            // Keep the file as it is (textures, extensions…) as long as it is a glTF binary;
            // reading its meshes only measures it.
            glb::check(bytes)?;
            return Ok(match glb::read(bytes) {
                Ok(scene) => Imported::of(format, bytes.to_vec(), &scene),
                Err(e) => {
                    log::warn!("mesh: kept {name} without measuring it: {e}");
                    Imported { format, glb: bytes.to_vec(), triangles: 0, bounds: None, view_size: None }
                }
            });
        }
    };
    let glb = glb::write(&scene)?;
    Ok(Imported::of(format, glb, &scene))
}

/// Parses an ASCII float the way model files write them (`1`, `-0.5`, `1e-3`, `nan` refused).
pub(crate) fn parse_f32(s: &str) -> Option<f32> {
    s.parse::<f32>().ok().filter(|v| v.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: &str) -> Option<Vec<u8>> {
        None
    }

    const STL: &str = "solid t\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 2 0 0\nvertex 0 0 1\nendloop\nendfacet\nendsolid t\n";
    const OBJ: &str = "mtllib t.mtl\nv 0 0 0\nv 2 0 0\nv 0 1 0\nusemtl Gold\nf 1 2 3\n";

    #[test]
    fn every_format_becomes_a_readable_glb() {
        let stl = import("part.STL", STL.as_bytes(), &mut none).expect("stl");
        assert_eq!((stl.format, stl.triangles), (Format::Stl, 1));
        let b = stl.bounds.expect("bounds");
        assert_eq!(b.size(), [2.0, 1.0, 0.0], "Z-up STL stands up in Y");
        assert_eq!(glb::read(&stl.glb).map(|s| s.triangle_count()), Ok(1));

        let mut mtl = |n: &str| (n == "t.mtl").then(|| b"newmtl Gold\nKd 1 0.8 0\n".to_vec());
        let obj = import("t.obj", OBJ.as_bytes(), &mut mtl).expect("obj");
        assert_eq!(glb::read(&obj.glb).map(|s| s.primitives[0].color), Ok([1.0, 0.8, 0.0, 1.0]));

        // A .glb is kept byte for byte.
        let kept = import("again.glb", &obj.glb, &mut none).expect("glb");
        assert_eq!((kept.format, kept.triangles, &kept.glb), (Format::Glb, 1, &obj.glb));
    }

    #[test]
    fn formats_are_told_apart() {
        assert_eq!(Format::detect("x.fbx", b"; FBX 7.4.0 project file"), Some(Format::Fbx));
        assert_eq!(Format::detect("noext", b"Kaydara FBX Binary  \0\x1a\0"), Some(Format::Fbx));
        assert_eq!(Format::detect("wrong.stl", b"glTF\x02\0\0\0"), Some(Format::Glb), "the signature wins over the name");
        assert_eq!(Format::detect("a.3ds", b""), None);
        assert!(matches!(import("a.3ds", b"", &mut none), Err(Error::UnknownFormat(_))));
        assert!(matches!(import("a.glb", b"glTF\x02\0\0\0\x0c\0\0\0", &mut none), Err(Error::Invalid { .. })));
    }

    #[test]
    fn a_glb_whose_meshes_cant_be_read_is_still_kept() {
        let json = br#"{"asset":{"version":"2.0"},"meshes":[{"primitives":[{"attributes":{},"extensions":{"KHR_draco_mesh_compression":{}}}]}]}  "#;
        let mut glb = b"glTF".to_vec();
        glb.extend(2u32.to_le_bytes());
        glb.extend(((12 + 8 + json.len()) as u32).to_le_bytes());
        glb.extend((json.len() as u32).to_le_bytes());
        glb.extend(0x4E4F_534Au32.to_le_bytes());
        glb.extend(json);
        let kept = import("draco.glb", &glb, &mut none).expect("kept");
        assert_eq!((kept.triangles, kept.bounds), (0, None));
    }
}
