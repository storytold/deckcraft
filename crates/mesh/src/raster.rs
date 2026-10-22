//! A small software renderer for [`Scene`]s: z-buffered triangles, perspective or orthographic,
//! textures sampled bilinearly in linear light, Lambert diffuse plus a Blinn-Phong highlight from
//! ambient, point, spot and directional lights, and supersampled edges.
//!
//! Two passes: the first rasterizes every opaque triangle into a depth buffer that remembers which
//! triangle is nearest at each sample; the second shades each covered sample once, finding the
//! surface point where the sample's view ray meets that triangle's plane (so attributes are
//! perspective-correct and triangles cut by the near plane need no special case). See-through
//! (`BLEND`) triangles are then drawn back to front over the result.

use crate::{Alpha, Material, Primitive, Scene, Texture, Wrap, linear_to_srgb, srgb_to_linear};

/// Most samples (pixels × supersamples) rendered at once; larger requests render smaller.
const MAX_SAMPLES: u64 = 4_000_000;

/// How light reaches the picture, in the units PowerPoint's 3-D models give their lights
/// (candela for point lights, illuminance for ambient light), measured against PowerPoint on a
/// grey sphere under one light at a time: direct light through Lambert's law times this gain…
const DIRECT_GAIN: f32 = 2.5;
/// …ambient light times this one, for a surface facing the viewer; ambient light is brighter
/// from above and a little from the right ([`ambient_shape`])…
const AMBIENT_GAIN: f32 = 3.55;

/// How much ambient light reaches a surface whose normal (in view space) is `n`, relative to one
/// facing the viewer: PowerPoint's ambient light is brighter from above and from the right.
fn ambient_shape(n: V3) -> f32 {
    (1.0 + 0.31 * n[1] + 0.22 * n[0] + 0.1 * (n[0] * n[0] + n[1] * n[1])).max(0.0)
}

/// …and the sum through a curve (per channel, in linear light): a soft toe, a straight part 0.027
/// below the input, and from 0.35 a roll-off towards white.
fn tone(u: f32) -> f32 {
    const TOE: f32 = 0.027;
    const KNEE: f32 = 0.35;
    if !u.is_finite() {
        return if u > 0.0 { 1.0 } else { 0.0 };
    }
    if u <= 2.0 * TOE {
        return u.max(0.0) * u.max(0.0) / (4.0 * TOE);
    }
    if u <= KNEE {
        return u - TOE;
    }
    let room = 1.0 - (KNEE - TOE);
    1.0 - room * (-(u - KNEE) / room).exp()
}
const NONE: u32 = u32::MAX;

/// How a scene is seen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// Model units → camera space (camera at the origin looking down −Z, Y up), row-major.
    pub model_view: [[f64; 4]; 4],
    /// Perspective: pixels per unit of `x / −z` (the focal length in pixels). Orthographic: pixels
    /// per camera-space unit.
    pub focal: f64,
    pub ortho: bool,
    /// Where the optical axis meets the image, in pixels from its top-left corner.
    pub center: (f64, f64),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LightKind {
    /// Light from everywhere.
    Ambient,
    /// Light from a point, fading with the square of the distance.
    Point,
    /// Parallel light travelling along `dir`.
    Directional,
    /// A point light limited to a cone around `dir` (cosines of the inner and outer half-angles).
    Spot { cos_inner: f32, cos_outer: f32 },
}

/// A light, in camera space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Light {
    pub kind: LightKind,
    /// Linear RGB, intensity included.
    pub color: [f32; 3],
    pub pos: [f64; 3],
    /// The direction the light travels.
    pub dir: [f64; 3],
}

/// A rendered image: premultiplied sRGB RGBA, rows top to bottom.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

type V3 = [f32; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn norm(a: V3) -> Option<V3> {
    let l = dot(a, a).sqrt();
    (l.is_finite() && l > 1e-20).then(|| [a[0] / l, a[1] / l, a[2] / l])
}

/// The largest `|x / −z|` and `|y / −z|` (orthographic: `|x|`, `|y|`) of the scene's vertices in
/// camera space: how far the model reaches from the optical axis. `None` when no vertex is in
/// front of the camera.
pub fn extent(scene: &Scene, model_view: &[[f64; 4]; 4], ortho: bool) -> Option<(f64, f64)> {
    let mut hx: f64 = 0.0;
    let mut hy: f64 = 0.0;
    let mut any = false;
    for p in &scene.primitives {
        for v in &p.positions {
            let c = xf(model_view, *v);
            let (x, y, z) = (c[0] as f64, c[1] as f64, c[2] as f64);
            let (px, py) = if ortho {
                (x, y)
            } else {
                if z >= -1e-9 {
                    continue;
                }
                (x / -z, y / -z)
            };
            if px.is_finite() && py.is_finite() {
                hx = hx.max(px.abs());
                hy = hy.max(py.abs());
                any = true;
            }
        }
    }
    any.then_some((hx, hy))
}

fn xf(m: &[[f64; 4]; 4], v: V3) -> V3 {
    let (x, y, z) = (v[0] as f64, v[1] as f64, v[2] as f64);
    [0, 1, 2].map(|r| (m[r][0] * x + m[r][1] * y + m[r][2] * z + m[r][3]) as f32)
}

/// The inverse transpose of the model-view's 3×3 part, for normals.
fn normal_matrix(m: &[[f64; 4]; 4]) -> [[f32; 3]; 3] {
    let a = [[m[0][0], m[0][1], m[0][2]], [m[1][0], m[1][1], m[1][2]], [m[2][0], m[2][1], m[2][2]]];
    let det = a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1]) - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
        + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]);
    if !det.is_finite() || det.abs() < 1e-300 {
        return a.map(|r| r.map(|v| v as f32));
    }
    let c = |r0: usize, c0: usize, r1: usize, c1: usize| a[r0][c0] * a[r1][c1] - a[r0][c1] * a[r1][c0];
    [[c(1, 1, 2, 2), -c(1, 0, 2, 2), c(1, 0, 2, 1)], [-c(0, 1, 2, 2), c(0, 0, 2, 2), -c(0, 0, 2, 1)], [c(0, 1, 1, 2), -c(0, 0, 1, 2), c(0, 0, 1, 1)]]
        .map(|row| row.map(|v| (v / det) as f32))
}

/// One primitive, moved into camera space and projected.
struct Prep<'a> {
    prim: &'a Primitive,
    mat: Material,
    tex: Option<&'a Texture>,
    cam: Vec<V3>,
    nrm: Vec<V3>,
    /// Sample-space x, y and the depth value interpolated across the screen (1/−z, or −z when
    /// orthographic).
    scr: Vec<[f32; 3]>,
}

struct Target {
    w: usize,
    h: usize,
    focal: f32,
    cx: f32,
    cy: f32,
    ortho: bool,
}

impl Target {
    /// The view ray through sample (sx, sy): origin and direction in camera space.
    fn ray(&self, sx: f32, sy: f32) -> (V3, V3) {
        let x = (sx - self.cx) / self.focal;
        let y = -(sy - self.cy) / self.focal;
        if self.ortho { ([x, y, 0.0], [0.0, 0.0, -1.0]) } else { ([0.0; 3], [x, y, -1.0]) }
    }
}

/// Render `scene` into a `width` × `height` image with `samples` × `samples` supersampling (1–4).
/// Very large requests come back smaller (same aspect); the caller stretches them.
pub fn render(scene: &Scene, cam: &Camera, lights: &[Light], width: u32, height: u32, samples: u32) -> Raster {
    let (mut width, mut height) = (width.max(1), height.max(1));
    let mut ss = samples.clamp(1, 4);
    let mut k = 1.0f64;
    while (width as u64 * ss as u64) * (height as u64 * ss as u64) > MAX_SAMPLES {
        if ss > 1 {
            ss -= 1;
        } else {
            width = (width / 2).max(1);
            height = (height / 2).max(1);
            k *= 0.5;
        }
    }
    let s = ss as f64;
    let t = Target {
        w: width as usize * ss as usize,
        h: height as usize * ss as usize,
        focal: (cam.focal * k * s) as f32,
        cx: (cam.center.0 * k * s) as f32,
        cy: (cam.center.1 * k * s) as f32,
        ortho: cam.ortho,
    };
    let mut out = Raster { width, height, pixels: vec![0; width as usize * height as usize * 4] };
    if !(t.focal.is_finite() && t.focal > 0.0 && t.cx.is_finite() && t.cy.is_finite()) {
        return out;
    }
    let nm = normal_matrix(&cam.model_view);
    let preps: Vec<Prep> = scene
        .primitives
        .iter()
        .map(|p| {
            let mat = p.material.and_then(|m| scene.materials.get(m)).cloned().unwrap_or_else(Material::default_grey);
            let tex = mat.texture.and_then(|i| scene.textures.get(i)).filter(|_| !p.uvs.is_empty());
            let cam_pos: Vec<V3> = p.positions.iter().map(|v| xf(&cam.model_view, *v)).collect();
            let nrm: Vec<V3> =
                p.normals.iter().map(|n| norm([0, 1, 2].map(|r| nm[r][0] * n[0] + nm[r][1] * n[1] + nm[r][2] * n[2])).unwrap_or([0.0; 3])).collect();
            let scr = cam_pos
                .iter()
                .map(|c| {
                    if t.ortho {
                        [t.cx + t.focal * c[0], t.cy - t.focal * c[1], -c[2]]
                    } else {
                        let iz = 1.0 / -c[2];
                        [t.cx + t.focal * c[0] * iz, t.cy - t.focal * c[1] * iz, iz]
                    }
                })
                .collect();
            Prep { prim: p, mat, tex, cam: cam_pos, nrm, scr }
        })
        .collect();
    let n = t.w * t.h;
    // Nearest depth (larger is nearer) and which triangle (primitive, triangle) is there.
    let mut depth = vec![f32::NEG_INFINITY; n];
    let mut hit = vec![(NONE, NONE); n];
    for (pi, p) in preps.iter().enumerate() {
        if p.mat.alpha == Alpha::Blend {
            continue;
        }
        for (ti, tri) in p.prim.indices.as_chunks::<3>().0.iter().enumerate() {
            raster_tri(&t, p, tri, &mut |i, d| {
                if let (Some(dv), Some(hv)) = (depth.get_mut(i), hit.get_mut(i))
                    && d > *dv
                {
                    *dv = d;
                    *hv = (pi as u32, ti as u32);
                }
            });
        }
    }
    let ambient: [f32; 3] =
        lights.iter().filter(|l| l.kind == LightKind::Ambient).fold([0.0; 3], |a, l| [a[0] + l.color[0], a[1] + l.color[1], a[2] + l.color[2]]);
    let direct: Vec<&Light> = lights.iter().filter(|l| l.kind != LightKind::Ambient).collect();
    // Linear premultiplied colour per sample.
    let mut color = vec![[0f32; 4]; n];
    for (i, (pi, ti)) in hit.iter().enumerate() {
        if *pi == NONE {
            continue;
        }
        let (sx, sy) = ((i % t.w) as f32 + 0.5, (i / t.w) as f32 + 0.5);
        if let Some(p) = preps.get(*pi as usize)
            && let Some(c) = shade(&t, p, *ti as usize, sx, sy, ambient, &direct)
            && let Some(slot) = color.get_mut(i)
        {
            let a = if matches!(p.mat.alpha, Alpha::Mask(_)) { 1.0 } else { c[3] };
            *slot = [c[0] * a, c[1] * a, c[2] * a, a];
        }
    }
    // See-through triangles, farthest first, over what is drawn; they don't hide each other.
    let mut blended: Vec<(f32, usize, usize)> = Vec::new();
    for (pi, p) in preps.iter().enumerate().filter(|(_, p)| p.mat.alpha == Alpha::Blend) {
        for (ti, tri) in p.prim.indices.as_chunks::<3>().0.iter().enumerate() {
            let z: f32 = tri.iter().filter_map(|v| p.cam.get(*v as usize)).map(|c| c[2]).sum::<f32>() / 3.0;
            blended.push((z, pi, ti));
        }
    }
    blended.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, pi, ti) in blended {
        let Some(p) = preps.get(pi) else { continue };
        let Some(tri) = p.prim.indices.get(ti * 3..ti * 3 + 3) else { continue };
        raster_tri(&t, p, tri, &mut |i, d| {
            if depth.get(i).is_none_or(|dv| d <= *dv) {
                return;
            }
            let (sx, sy) = ((i % t.w) as f32 + 0.5, (i / t.w) as f32 + 0.5);
            if let Some(c) = shade(&t, p, ti, sx, sy, ambient, &direct)
                && let Some(slot) = color.get_mut(i)
            {
                let a = c[3];
                *slot = [c[0] * a + slot[0] * (1.0 - a), c[1] * a + slot[1] * (1.0 - a), c[2] * a + slot[2] * (1.0 - a), a + slot[3] * (1.0 - a)];
            }
        });
    }
    // Average the supersamples, then to premultiplied sRGB bytes.
    let ssu = ss as usize;
    let inv = 1.0 / (ssu * ssu) as f32;
    for y in 0..height as usize {
        for x in 0..width as usize {
            let mut acc = [0f32; 4];
            for sy in 0..ssu {
                for sx in 0..ssu {
                    if let Some(c) = color.get((y * ssu + sy) * t.w + x * ssu + sx) {
                        for k in 0..4 {
                            acc[k] += c[k];
                        }
                    }
                }
            }
            let a = (acc[3] * inv).clamp(0.0, 1.0);
            let o = (y * width as usize + x) * 4;
            if let Some(px) = out.pixels.get_mut(o..o + 4)
                && a > 0.0
            {
                for k in 0..3 {
                    let straight = (acc[k] * inv / a).clamp(0.0, 1.0);
                    px[k] = (linear_to_srgb(straight) * a * 255.0 + 0.5) as u8;
                }
                px[3] = (a * 255.0 + 0.5) as u8;
            }
        }
    }
    out
}

/// Visit every sample a triangle covers, with its depth value (larger is nearer).
fn raster_tri(t: &Target, p: &Prep, tri: &[u32], visit: &mut dyn FnMut(usize, f32)) {
    let (Some(&a), Some(&b), Some(&c)) = (tri.first(), tri.get(1), tri.get(2)) else { return };
    let (Some(ca), Some(cb), Some(cc)) = (p.cam.get(a as usize), p.cam.get(b as usize), p.cam.get(c as usize)) else { return };
    // Back faces of one-sided materials are not drawn.
    let fnorm = cross(sub(*cb, *ca), sub(*cc, *ca));
    let facing = if t.ortho { fnorm[2] } else { -dot(fnorm, *ca) };
    if !p.mat.double_sided && facing <= 0.0 {
        return;
    }
    let near = 1e-5f32;
    if t.ortho {
        let (Some(sa), Some(sb), Some(sc)) = (p.scr.get(a as usize), p.scr.get(b as usize), p.scr.get(c as usize)) else { return };
        // Orthographic depth is −z: nearer is smaller, so negate for "larger is nearer".
        fill(t, [[sa[0], sa[1], -sa[2]], [sb[0], sb[1], -sb[2]], [sc[0], sc[1], -sc[2]]], visit);
        return;
    }
    let behind = [ca[2] > -near, cb[2] > -near, cc[2] > -near];
    if behind.iter().all(|b| *b) {
        return;
    }
    if !behind.iter().any(|b| *b) {
        let (Some(sa), Some(sb), Some(sc)) = (p.scr.get(a as usize), p.scr.get(b as usize), p.scr.get(c as usize)) else { return };
        fill(t, [*sa, *sb, *sc], visit);
        return;
    }
    // Cut by the near plane: clip the polygon, project, fan.
    let poly = [*ca, *cb, *cc];
    let mut clipped: Vec<V3> = Vec::with_capacity(4);
    for i in 0..3 {
        let (u, v) = (poly[i], poly[(i + 1) % 3]);
        let (ui, vi) = (u[2] <= -near, v[2] <= -near);
        if ui {
            clipped.push(u);
        }
        if ui != vi {
            let k = (-near - u[2]) / (v[2] - u[2]);
            clipped.push([u[0] + (v[0] - u[0]) * k, u[1] + (v[1] - u[1]) * k, -near]);
        }
    }
    let proj = |c: &V3| {
        let iz = 1.0 / -c[2];
        [t.cx + t.focal * c[0] * iz, t.cy - t.focal * c[1] * iz, iz]
    };
    if let Some(first) = clipped.first() {
        let f = proj(first);
        for w in clipped.windows(2).skip(1) {
            fill(t, [f, proj(&w[0]), proj(&w[1])], visit);
        }
    }
}

/// Scan-convert a screen-space triangle (x, y, depth) over sample centres.
fn fill(t: &Target, v: [[f32; 3]; 3], visit: &mut dyn FnMut(usize, f32)) {
    if !v.iter().flatten().all(|c| c.is_finite()) {
        return;
    }
    let area = (v[1][0] - v[0][0]) * (v[2][1] - v[0][1]) - (v[2][0] - v[0][0]) * (v[1][1] - v[0][1]);
    if area.abs() < 1e-12 {
        return;
    }
    let minx = v.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min).floor().max(0.0);
    let maxx = v.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max).ceil().min(t.w as f32);
    let miny = v.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min).floor().max(0.0);
    let maxy = v.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max).ceil().min(t.h as f32);
    if minx >= maxx || miny >= maxy {
        return;
    }
    let inv = 1.0 / area;
    let edge = |a: [f32; 3], b: [f32; 3], x: f32, y: f32| (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0]);
    for y in miny as usize..maxy as usize {
        let py = y as f32 + 0.5;
        for x in minx as usize..maxx as usize {
            let px = x as f32 + 0.5;
            // Barycentric weights, the same sign as the area inside.
            let w0 = edge(v[1], v[2], px, py) * inv;
            let w1 = edge(v[2], v[0], px, py) * inv;
            let w2 = edge(v[0], v[1], px, py) * inv;
            if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                continue;
            }
            let d = w0 * v[0][2] + w1 * v[1][2] + w2 * v[2][2];
            visit(y * t.w + x, d);
        }
    }
}

/// The lit colour (linear, straight alpha) of triangle `ti` of `p` at sample (sx, sy).
fn shade(t: &Target, p: &Prep, ti: usize, sx: f32, sy: f32, ambient: [f32; 3], lights: &[&Light]) -> Option<[f32; 4]> {
    let tri = p.prim.indices.get(ti * 3..ti * 3 + 3)?;
    let (ia, ib, ic) = (*tri.first()? as usize, *tri.get(1)? as usize, *tri.get(2)? as usize);
    let (a, b, c) = (*p.cam.get(ia)?, *p.cam.get(ib)?, *p.cam.get(ic)?);
    let n = cross(sub(b, a), sub(c, a));
    let (o, d) = t.ray(sx, sy);
    let denom = dot(n, d);
    if denom.abs() < 1e-30 {
        return None;
    }
    let s = dot(n, sub(a, o)) / denom;
    let q = [o[0] + d[0] * s, o[1] + d[1] * s, o[2] + d[2] * s];
    // Barycentric coordinates of q.
    let nn = dot(n, n);
    if nn <= 0.0 || !nn.is_finite() {
        return None;
    }
    let wa = dot(cross(sub(c, b), sub(q, b)), n) / nn;
    let wb = dot(cross(sub(a, c), sub(q, c)), n) / nn;
    let wc = 1.0 - wa - wb;
    let lerp3 = |x: V3, y: V3, z: V3| [0, 1, 2].map(|k| x[k] * wa + y[k] * wb + z[k] * wc);
    let flat = norm(n)?;
    let mut normal = match (p.nrm.get(ia), p.nrm.get(ib), p.nrm.get(ic)) {
        (Some(x), Some(y), Some(z)) => norm(lerp3(*x, *y, *z)).unwrap_or(flat),
        _ => flat,
    };
    let view = if t.ortho { [0.0, 0.0, 1.0] } else { norm([-q[0], -q[1], -q[2]]).unwrap_or([0.0, 0.0, 1.0]) };
    if dot(normal, view) < 0.0 {
        normal = [-normal[0], -normal[1], -normal[2]];
    }
    let mut base = p.mat.base_color;
    if let (Some(x), Some(y), Some(z)) = (p.prim.colors.get(ia), p.prim.colors.get(ib), p.prim.colors.get(ic)) {
        for k in 0..4 {
            base[k] *= x[k] * wa + y[k] * wb + z[k] * wc;
        }
    }
    if let (Some(tex), Some(x), Some(y), Some(z)) = (p.tex, p.prim.uvs.get(ia), p.prim.uvs.get(ib), p.prim.uvs.get(ic)) {
        let u = x[0] * wa + y[0] * wb + z[0] * wc;
        let v = x[1] * wa + y[1] * wb + z[1] * wc;
        let tc = sample(tex, u, v);
        for k in 0..4 {
            base[k] *= tc[k];
        }
    }
    if let Alpha::Mask(cut) = p.mat.alpha
        && base[3] < cut
    {
        return None;
    }
    if p.mat.unlit {
        return Some(base);
    }
    // glTF's metallic-roughness model: Lambert diffuse, and a GGX highlight with Smith
    // visibility and Schlick's Fresnel term.
    let rgb = [base[0], base[1], base[2]];
    let metal = p.mat.metallic;
    let diffuse = rgb.map(|c| c * (1.0 - metal));
    let f0 = rgb.map(|c| 0.04 + (c - 0.04) * metal);
    let r = p.mat.roughness.clamp(0.03, 1.0);
    let a2 = (r * r) * (r * r);
    let ndv = dot(normal, view).max(1e-4);
    let mut out = [0f32; 3];
    // Ambient light: diffuse, and its reflection, stronger at grazing angles on smooth surfaces.
    let amb = AMBIENT_GAIN * ambient_shape(normal);
    let graze = (1.0 - ndv).powi(5);
    for k in 0..3 {
        let fr = f0[k] + ((1.0 - r).max(f0[k]) - f0[k]) * graze;
        out[k] = amb * ambient[k] * (diffuse[k] * (1.0 - fr) + fr) + p.mat.emissive[k];
    }
    let pi = std::f32::consts::PI;
    for l in lights {
        let (dir, radiance) = match l.kind {
            LightKind::Directional => (norm([-l.dir[0] as f32, -l.dir[1] as f32, -l.dir[2] as f32]), l.color),
            LightKind::Point | LightKind::Spot { .. } => {
                let to = sub([l.pos[0] as f32, l.pos[1] as f32, l.pos[2] as f32], q);
                let d2 = dot(to, to).max(1e-6);
                let dir = norm(to);
                let mut k = 1.0 / d2;
                if let (LightKind::Spot { cos_inner, cos_outer }, Some(dv)) = (l.kind, dir) {
                    let axis = norm([l.dir[0] as f32, l.dir[1] as f32, l.dir[2] as f32]).unwrap_or([0.0, 0.0, -1.0]);
                    let cd = -dot(dv, axis);
                    k *= ((cd - cos_outer) / (cos_inner - cos_outer).max(1e-4)).clamp(0.0, 1.0);
                }
                (dir, l.color.map(|c| c * k))
            }
            LightKind::Ambient => continue,
        };
        let Some(ld) = dir else { continue };
        let ndl = dot(normal, ld);
        if ndl <= 0.0 {
            continue;
        }
        let h = norm([ld[0] + view[0], ld[1] + view[1], ld[2] + view[2]]).unwrap_or(normal);
        let ndh = dot(normal, h).max(0.0);
        let vdh = dot(view, h).max(0.0);
        let dd = ndh * ndh * (a2 - 1.0) + 1.0;
        let d = a2 / (pi * dd * dd).max(1e-12);
        let vis = 0.5 / (ndl * (ndv * ndv * (1.0 - a2) + a2).sqrt() + ndv * (ndl * ndl * (1.0 - a2) + a2).sqrt()).max(1e-6);
        let fv = (1.0 - vdh).powi(5);
        for k in 0..3 {
            let f = f0[k] + (1.0 - f0[k]) * fv;
            out[k] += DIRECT_GAIN * ((1.0 - f) * diffuse[k] / pi + f * d * vis) * radiance[k] * ndl;
        }
    }
    Some([tone(out[0]), tone(out[1]), tone(out[2]), base[3]])
}

fn wrap(c: f32, n: u32, w: Wrap) -> usize {
    let n = n.max(1) as i64;
    let i = c.floor() as i64;
    let v = match w {
        Wrap::Clamp => i.clamp(0, n - 1),
        Wrap::Repeat => i.rem_euclid(n),
        Wrap::Mirror => {
            let m = i.rem_euclid(2 * n);
            if m < n { m } else { 2 * n - 1 - m }
        }
    };
    v as usize
}

/// Bilinear texture lookup in linear light (straight alpha).
fn sample(t: &Texture, u: f32, v: f32) -> [f32; 4] {
    if !(u.is_finite() && v.is_finite()) || t.width == 0 || t.height == 0 {
        return [1.0; 4];
    }
    let x = u * t.width as f32 - 0.5;
    let y = v * t.height as f32 - 0.5;
    let (fx, fy) = (x - x.floor(), y - y.floor());
    let (x0, y0) = (wrap(x, t.width, t.wrap_s), wrap(y, t.height, t.wrap_t));
    let (x1, y1) = (wrap(x + 1.0, t.width, t.wrap_s), wrap(y + 1.0, t.height, t.wrap_t));
    let texel = |xx: usize, yy: usize| -> [f32; 4] {
        let i = (yy * t.width as usize + xx) * 4;
        match t.rgba.get(i..i + 4) {
            Some(px) => [lin(px[0]), lin(px[1]), lin(px[2]), px[3] as f32 / 255.0],
            None => [1.0; 4],
        }
    };
    let (a, b, c, d) = (texel(x0, y0), texel(x1, y0), texel(x0, y1), texel(x1, y1));
    [0, 1, 2, 3].map(|k| (a[k] * (1.0 - fx) + b[k] * fx) * (1.0 - fy) + (c[k] * (1.0 - fx) + d[k] * fx) * fy)
}

/// sRGB byte to linear, by table.
fn lin(c: u8) -> f32 {
    static LUT: std::sync::OnceLock<[f32; 256]> = std::sync::OnceLock::new();
    let t = LUT.get_or_init(|| std::array::from_fn(|i| srgb_to_linear(i as f32 / 255.0)));
    t[c as usize]
}
