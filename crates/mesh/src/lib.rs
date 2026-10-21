//! DeckCraft 3-D models: reading glTF 2.0 (`.glb`, `.gltf`), Wavefront OBJ (with its MTL
//! materials), STL, PLY and 3MF into one triangle [`Scene`], writing a scene as glTF binary
//! ([`to_glb`]), and drawing it with a small software renderer ([`render`]).
//!
//! These are the files Insert ▸ 3D Models accepts. A PowerPoint file keeps every model as glTF
//! binary, so the other formats are converted once, when they are inserted.
//!
//! Every reader takes hostile input: sizes, counts and indices are checked, allocations are capped
//! and nothing panics. Companion files (an OBJ's `.mtl` and textures, a PLY's texture, a glTF's
//! external buffers and images) come through a [`Resolve`] closure, so the caller decides what may
//! be read.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod gltf;
mod obj;
mod ply;
mod raster;
mod stl;
mod threemf;
mod write;

#[cfg(test)]
mod tests;

use std::sync::Arc;

pub use raster::{Camera, Light, LightKind, Raster, extent, render};
pub use write::to_glb;

/// Largest model file read.
pub const MAX_FILE: usize = 512 << 20;
/// Most vertices kept for one scene (all primitives together).
pub const MAX_VERTICES: usize = 10_000_000;
/// Most triangle indices kept for one scene.
pub const MAX_INDICES: usize = 30_000_000;
/// Most textures kept for one scene.
pub const MAX_TEXTURES: usize = 64;
/// Textures are decoded for drawing at most this many pixels on a side (the file keeps the
/// original).
pub const MAX_TEXTURE_SIDE: u32 = 2048;
/// Largest texture (pixels) decoded at all.
const MAX_TEXTURE_PIXELS: u64 = 16384 * 16384;

/// File extensions Insert ▸ 3D Models accepts.
pub const EXTENSIONS: &[&str] = &["glb", "gltf", "obj", "stl", "ply", "3mf"];

/// Why a model could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// Not a model file of a kind we read, or broken beyond repair.
    Format(String),
    /// A model we recognise but whose content we can't decode (glTF 1.0, Draco compression…).
    Unsupported(String),
    /// More vertices, triangles or bytes than we keep.
    TooLarge,
    /// The file holds no triangles.
    Empty,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Format(m) => write!(f, "not a readable 3-D model: {m}"),
            Error::Unsupported(m) => write!(f, "unsupported 3-D model: {m}"),
            Error::TooLarge => write!(f, "the 3-D model is too large"),
            Error::Empty => write!(f, "the 3-D model has no surfaces"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

fn bad(m: impl Into<String>) -> Error {
    Error::Format(m.into())
}

/// Looks a companion file up by the name the model file uses for it (relative, as written).
pub type Resolve<'a> = &'a dyn Fn(&str) -> Option<Vec<u8>>;

/// A resolver that finds nothing.
pub fn no_files(_: &str) -> Option<Vec<u8>> {
    None
}

/// The kinds of model file we read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Glb,
    Gltf,
    Obj,
    Stl,
    Ply,
    ThreeMf,
}

impl Format {
    /// By file extension.
    pub fn from_name(name: &str) -> Option<Format> {
        let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase())?;
        Some(match ext.as_str() {
            "glb" => Format::Glb,
            "gltf" => Format::Gltf,
            "obj" => Format::Obj,
            "stl" => Format::Stl,
            "ply" => Format::Ply,
            "3mf" => Format::ThreeMf,
            _ => return None,
        })
    }

    /// By content first (magic numbers), then by extension. A file named as another kind (a
    /// `.pptx` is a zip like 3MF) is not a model unless its content says so unambiguously.
    pub fn sniff(name: &str, bytes: &[u8]) -> Option<Format> {
        if bytes.starts_with(b"glTF") {
            return Some(Format::Glb);
        }
        if bytes.starts_with(b"ply\n") || bytes.starts_with(b"ply\r\n") {
            return Some(Format::Ply);
        }
        let by_name = Format::from_name(name);
        let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
        if by_name.is_none() && base.contains('.') {
            return None;
        }
        if bytes.starts_with(b"PK\x03\x04") {
            return Some(Format::ThreeMf);
        }
        by_name.or_else(|| {
            let head = String::from_utf8_lossy(bytes.get(..bytes.len().min(512)).unwrap_or(&[])).to_string();
            let t = head.trim_start();
            if t.starts_with('{') && head.contains("\"asset\"") {
                Some(Format::Gltf)
            } else if t.starts_with("solid") && head.contains("facet") {
                Some(Format::Stl)
            } else if head.lines().any(|l| l.starts_with("v ")) {
                Some(Format::Obj)
            } else {
                None
            }
        })
    }

    /// The usual extension.
    pub fn extension(self) -> &'static str {
        match self {
            Format::Glb => "glb",
            Format::Gltf => "gltf",
            Format::Obj => "obj",
            Format::Stl => "stl",
            Format::Ply => "ply",
            Format::ThreeMf => "3mf",
        }
    }
}

/// Is `bytes` (named `name`) a model file we read? Decides by content and extension only.
pub fn is_model(name: &str, bytes: &[u8]) -> bool {
    Format::sniff(name, bytes).is_some()
}

/// Can this model file name other files it needs (an OBJ's materials and textures, a PLY's
/// texture, a glTF's buffers and images, a GLB's external buffers or images)? A GLB is read to
/// tell: most hold everything themselves.
pub fn names_other_files(name: &str, bytes: &[u8]) -> bool {
    match Format::sniff(name, bytes) {
        Some(Format::Obj | Format::Ply | Format::Gltf) => true,
        Some(Format::Glb) => gltf::glb_names_files(bytes),
        _ => false,
    }
}

/// Read a model file of any supported kind. `resolve` supplies companion files.
pub fn load(name: &str, bytes: &[u8], resolve: Resolve) -> Result<Scene> {
    if bytes.len() > MAX_FILE {
        return Err(Error::TooLarge);
    }
    let format = Format::sniff(name, bytes).ok_or_else(|| bad(format!("{name}: unknown kind of file")))?;
    let scene = match format {
        Format::Glb => gltf::read_glb(bytes, resolve)?,
        Format::Gltf => gltf::read_json(bytes, resolve)?,
        Format::Obj => obj::read(bytes, resolve)?,
        Format::Stl => stl::read(bytes)?,
        Format::Ply => ply::read(bytes, resolve)?,
        Format::ThreeMf => threemf::read(bytes)?,
    };
    scene.finish()
}

/// Triangles to draw, in one coordinate space (node transforms applied), in the file's units.
#[derive(Clone, Debug, Default)]
pub struct Scene {
    pub primitives: Vec<Primitive>,
    pub materials: Vec<Material>,
    pub textures: Vec<Texture>,
}

/// Triangles sharing one material. Attribute lists other than `positions` are either empty or as
/// long as `positions`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Primitive {
    pub positions: Vec<[f32; 3]>,
    /// Unit normals; empty (or a zero normal) draws the triangle flat.
    pub normals: Vec<[f32; 3]>,
    /// Texture coordinates, glTF convention: (0, 0) is the image's top-left corner.
    pub uvs: Vec<[f32; 2]>,
    /// Linear RGBA vertex colours, multiplied with the material colour.
    pub colors: Vec<[f32; 4]>,
    /// Three per triangle, into the attribute lists.
    pub indices: Vec<u32>,
    pub material: Option<usize>,
}

/// How a material's alpha is used.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Alpha {
    Opaque,
    /// Fully see-through below the cutoff, opaque above.
    Mask(f32),
    Blend,
}

/// A surface's look, in glTF's metallic-roughness terms.
#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    pub name: String,
    /// Linear RGBA.
    pub base_color: [f32; 4],
    /// Base colour texture, an index into [`Scene::textures`].
    pub texture: Option<usize>,
    pub metallic: f32,
    pub roughness: f32,
    /// Linear RGB light the surface gives off by itself.
    pub emissive: [f32; 3],
    pub double_sided: bool,
    pub alpha: Alpha,
    /// Drawn in its own colour, without lighting.
    pub unlit: bool,
}

impl Default for Material {
    /// A matte white, seen from both sides (what converted files get).
    fn default() -> Self {
        Material {
            name: String::new(),
            base_color: [1.0; 4],
            texture: None,
            metallic: 0.0,
            roughness: 0.8,
            emissive: [0.0; 3],
            double_sided: true,
            alpha: Alpha::Opaque,
            unlit: false,
        }
    }
}

impl Material {
    /// The plain grey a model without materials (STL, PLY without colours) gets.
    pub fn default_grey() -> Material {
        Material { name: "Default".into(), base_color: [0.2176, 0.2176, 0.2176, 1.0], ..Default::default() }
    }
}

/// How texture coordinates outside 0..1 are treated.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Wrap {
    #[default]
    Repeat,
    Clamp,
    Mirror,
}

/// A texture image: its encoded bytes as found (written back to glTF) and its pixels for drawing.
#[derive(Clone, Debug, PartialEq)]
pub struct Texture {
    /// `image/png`, `image/jpeg`, or another image type (written to glTF as PNG).
    pub mime: String,
    pub data: Arc<Vec<u8>>,
    /// Straight sRGB RGBA, at most [`MAX_TEXTURE_SIDE`] on a side.
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub wrap_s: Wrap,
    pub wrap_t: Wrap,
}

impl Texture {
    /// Decode an encoded image (PNG, JPEG, WebP, GIF, BMP, TIFF, TGA). `None` when it can't be read.
    pub fn decode(data: Vec<u8>) -> Option<Texture> {
        let fmt = image::guess_format(&data).ok()?;
        let reader = image::ImageReader::with_format(std::io::Cursor::new(&data), fmt);
        let (w, h) = reader.into_dimensions().ok()?;
        if w == 0 || h == 0 || (w as u64) * (h as u64) > MAX_TEXTURE_PIXELS {
            return None;
        }
        let img = image::load_from_memory_with_format(&data, fmt).ok()?;
        let img = if w > MAX_TEXTURE_SIDE || h > MAX_TEXTURE_SIDE { img.thumbnail(MAX_TEXTURE_SIDE, MAX_TEXTURE_SIDE) } else { img };
        let rgba = img.to_rgba8();
        let mime = match fmt {
            image::ImageFormat::Png => "image/png",
            image::ImageFormat::Jpeg => "image/jpeg",
            image::ImageFormat::WebP => "image/webp",
            image::ImageFormat::Gif => "image/gif",
            image::ImageFormat::Bmp => "image/bmp",
            image::ImageFormat::Tiff => "image/tiff",
            image::ImageFormat::Tga => "image/x-tga",
            _ => "application/octet-stream",
        };
        Some(Texture {
            mime: mime.into(),
            data: Arc::new(data),
            width: rgba.width(),
            height: rgba.height(),
            rgba: rgba.into_raw(),
            wrap_s: Wrap::Repeat,
            wrap_t: Wrap::Repeat,
        })
    }
}

/// sRGB (0..1) to linear.
pub fn srgb_to_linear(c: f32) -> f32 {
    let c = if c.is_finite() { c.clamp(0.0, 1.0) } else { 0.0 };
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// Linear (0..1) to sRGB.
pub fn linear_to_srgb(c: f32) -> f32 {
    let c = if c.is_finite() { c.clamp(0.0, 1.0) } else { 0.0 };
    if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

impl Scene {
    /// Number of vertices over all primitives.
    pub fn vertex_count(&self) -> usize {
        self.primitives.iter().map(|p| p.positions.len()).sum()
    }

    /// Number of triangles over all primitives.
    pub fn triangle_count(&self) -> usize {
        self.primitives.iter().map(|p| p.indices.len() / 3).sum()
    }

    /// The box around every vertex: (lowest, highest corner).
    pub fn bounds(&self) -> Option<([f64; 3], [f64; 3])> {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for p in &self.primitives {
            for v in &p.positions {
                for k in 0..3 {
                    let c = v[k] as f64;
                    lo[k] = lo[k].min(c);
                    hi[k] = hi[k].max(c);
                }
            }
        }
        (lo.iter().chain(hi.iter()).all(|v| v.is_finite())).then_some((lo, hi))
    }

    /// Check and tidy what a reader produced: drop broken triangles and attributes of the wrong
    /// length, drop non-finite vertices' triangles, clamp material and texture references, and
    /// refuse a scene without triangles or over the limits.
    pub fn finish(mut self) -> Result<Scene> {
        self.textures.truncate(MAX_TEXTURES);
        let nt = self.textures.len();
        for m in &mut self.materials {
            if m.texture.is_some_and(|t| t >= nt) {
                m.texture = None;
            }
            for c in m.base_color.iter_mut() {
                *c = if c.is_finite() { c.clamp(0.0, 1.0) } else { 1.0 };
            }
            for c in m.emissive.iter_mut() {
                *c = if c.is_finite() { c.max(0.0) } else { 0.0 };
            }
            m.metallic = if m.metallic.is_finite() { m.metallic.clamp(0.0, 1.0) } else { 0.0 };
            m.roughness = if m.roughness.is_finite() { m.roughness.clamp(0.0, 1.0) } else { 1.0 };
        }
        let nm = self.materials.len();
        let mut verts = 0usize;
        let mut idx = 0usize;
        for p in &mut self.primitives {
            let n = p.positions.len();
            if p.normals.len() != n {
                p.normals.clear();
            }
            if p.uvs.len() != n {
                p.uvs.clear();
            }
            if p.colors.len() != n {
                p.colors.clear();
            }
            if p.material.is_some_and(|m| m >= nm) {
                p.material = None;
            }
            let ok = |i: u32| (i as usize) < n && p.positions.get(i as usize).is_some_and(|v| v.iter().all(|c| c.is_finite()));
            let tris: Vec<u32> = p.indices.as_chunks::<3>().0.iter().filter(|t| t.iter().all(|i| ok(*i))).flatten().copied().collect();
            p.indices = tris;
            // No normals in the file: our own, so every reader (PowerPoint too) shades alike.
            if p.normals.is_empty() {
                p.normals = vertex_normals(p);
            }
            for nrm in p.normals.iter_mut() {
                if !nrm.iter().all(|c| c.is_finite()) {
                    *nrm = [0.0; 3];
                }
            }
            for uv in p.uvs.iter_mut() {
                if !uv.iter().all(|c| c.is_finite()) {
                    *uv = [0.0; 2];
                }
            }
            for c in p.colors.iter_mut() {
                for v in c.iter_mut() {
                    *v = if v.is_finite() { v.clamp(0.0, 1.0) } else { 1.0 };
                }
            }
            verts = verts.saturating_add(n);
            idx = idx.saturating_add(p.indices.len());
        }
        if verts > MAX_VERTICES || idx > MAX_INDICES {
            return Err(Error::TooLarge);
        }
        self.primitives.retain(|p| !p.indices.is_empty());
        if self.primitives.is_empty() {
            return Err(Error::Empty);
        }
        Ok(self)
    }
}

/// Normals for a primitive that has none: each vertex gets the area-weighted average of the
/// triangles using it, so shared vertices shade smoothly and unshared ones (STL) flat.
fn vertex_normals(p: &Primitive) -> Vec<[f32; 3]> {
    let mut acc = vec![[0f32; 3]; p.positions.len()];
    for t in p.indices.as_chunks::<3>().0 {
        let (Some(a), Some(b), Some(c)) = (p.positions.get(t[0] as usize), p.positions.get(t[1] as usize), p.positions.get(t[2] as usize)) else {
            continue;
        };
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        for i in t {
            if let Some(s) = acc.get_mut(*i as usize) {
                for k in 0..3 {
                    s[k] += n[k];
                }
            }
        }
    }
    acc.into_iter()
        .map(|n| {
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if l.is_finite() && l > 1e-30 { [n[0] / l, n[1] / l, n[2] / l] } else { [0.0; 3] }
        })
        .collect()
}

/// Little helpers shared by the readers.
pub(crate) mod util {
    /// Standard or URL-safe base64 (padding and whitespace ignored).
    pub fn base64(s: &str) -> Option<Vec<u8>> {
        let mut out = Vec::with_capacity(s.len() / 4 * 3);
        let mut acc = 0u32;
        let mut bits = 0;
        for c in s.bytes() {
            let v = match c {
                b'A'..=b'Z' => c - b'A',
                b'a'..=b'z' => c - b'a' + 26,
                b'0'..=b'9' => c - b'0' + 52,
                b'+' | b'-' => 62,
                b'/' | b'_' => 63,
                b'=' | b'\n' | b'\r' | b' ' | b'\t' => continue,
                _ => return None,
            } as u32;
            acc = (acc << 6) | v;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push(((acc >> bits) & 0xff) as u8);
            }
        }
        Some(out)
    }

    /// `%20`-style escapes in a relative URI.
    pub fn percent_decode(s: &str) -> String {
        let b = s.as_bytes();
        let mut out = Vec::with_capacity(b.len());
        let mut i = 0;
        while i < b.len() {
            let c = b.get(i).copied().unwrap_or(0);
            if c == b'%'
                && let Some(h) = b.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok())
            {
                out.push(h);
                i += 3;
                continue;
            }
            out.push(c);
            i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    /// A `#RRGGBB` or `#RRGGBBAA` colour (sRGB) as linear RGBA.
    pub fn hex_color(s: &str) -> Option<[f32; 4]> {
        let h = s.trim().strip_prefix('#')?;
        let byte = |i: usize| h.get(i..i + 2).and_then(|x| u8::from_str_radix(x, 16).ok());
        let (r, g, b) = (byte(0)?, byte(2)?, byte(4)?);
        let a = if h.len() >= 8 { byte(6)? } else { 255 };
        let l = |v: u8| super::srgb_to_linear(v as f32 / 255.0);
        Some([l(r), l(g), l(b), a as f32 / 255.0])
    }
}
