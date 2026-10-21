//! `am3d:model3d`: 3-D models in a graphic frame (the Office drawing extension namespace
//! `http://schemas.microsoft.com/office/drawing/2017/model3d`).
//!
//! Lengths in the 3-D scene are EMU at 36 000 000 per metre, angles 60 000ths of a degree.

use deckcraft_model::MediaId;
use deckcraft_model::model3d::{Camera3d, Light3d, LightKind, Model3d};

use super::{Imp, Part};
use crate::xml::El;

pub const EMU_PER_M: f64 = 36_000_000.0;

fn emu_m(e: Option<&El>, k: &str) -> f64 {
    e.and_then(|e| e.f64(k)).filter(|v| v.is_finite()).map(|v| v / EMU_PER_M).unwrap_or(0.0)
}

fn point(e: Option<&El>) -> [f64; 3] {
    [emu_m(e, "x"), emu_m(e, "y"), emu_m(e, "z")]
}

fn vector(e: Option<&El>) -> [f64; 3] {
    [emu_m(e, "dx"), emu_m(e, "dy"), emu_m(e, "dz")]
}

/// A ratio `n / d` (`a:CT_Ratio`, `am3d:CT_PositiveRatio`).
fn ratio(e: Option<&El>, or: f64) -> f64 {
    let Some(e) = e else { return or };
    let (n, d) = (e.f64("n").unwrap_or(0.0), e.f64("d").unwrap_or(0.0));
    let v = n / d;
    if v.is_finite() { v } else { or }
}

fn angle(e: Option<&El>, k: &str) -> f64 {
    e.and_then(|e| e.f64(k)).filter(|v| v.is_finite()).map(|v| deckcraft_geom::space::normalize_deg(v / 60_000.0)).unwrap_or(0.0)
}

/// An `am3d:clr` as linear RGB (scRGB is linear already; sRGB colours are converted).
fn color(e: Option<&El>) -> [f64; 3] {
    let Some(c) = e.and_then(|e| e.elements().next()) else { return [1.0; 3] };
    match c.local() {
        "scrgbClr" => ["r", "g", "b"].map(|k| c.f64(k).filter(|v| v.is_finite()).map(|v| (v / 100_000.0).max(0.0)).unwrap_or(1.0)),
        "srgbClr" => {
            let hex = c.attr("val").unwrap_or("FFFFFF");
            let byte = |i: usize| hex.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok()).unwrap_or(255) as f64 / 255.0;
            let lin = |v: f64| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
            [lin(byte(0)), lin(byte(2)), lin(byte(4))]
        }
        _ => [1.0; 3],
    }
}

fn light(e: &El) -> Option<Light3d> {
    let kind = match e.local() {
        "ambientLight" => LightKind::Ambient,
        "ptLight" => LightKind::Point,
        "dirLight" => LightKind::Directional,
        "spotLight" => LightKind::Spot,
        _ => return None,
    };
    let strength = e.child("intensity").or_else(|| e.child("illuminance"));
    // Whatever child carries a position or a direction.
    let pos = e.elements().find(|c| c.has_attr("x") && c.has_attr("z"));
    let dir = e.elements().find(|c| c.has_attr("dx") && c.has_attr("dz"));
    let ang = e.elements().find(|c| c.local().contains("ngle")).and_then(|c| c.f64("val").or_else(|| c.f64("ang")));
    Some(Light3d {
        kind,
        color: color(e.child("clr")),
        intensity: ratio(strength, 1.0).max(0.0),
        pos: point(pos),
        dir: vector(dir),
        angle: ang.filter(|v| v.is_finite()).map(|v| v / 60_000.0).unwrap_or(30.0),
    })
}

/// A model file's bounds (model units) and, placed as on insert, its outline (`x / −z` units).
#[derive(Clone, Copy)]
pub struct Fit {
    lo: [f64; 3],
    hi: [f64; 3],
    extent: Option<(f64, f64)>,
}

fn fit(imp: &Imp, media: MediaId) -> Option<Fit> {
    let item = imp.media.iter().find(|it| it.id == media)?;
    let scene = deckcraft_mesh::load("model.glb", &item.data, &deckcraft_mesh::no_files).ok()?;
    let (lo, hi) = scene.bounds()?;
    let m = Model3d::fitted(media, lo, hi);
    Some(Fit { lo, hi, extent: deckcraft_mesh::extent(&scene, &m.model_view(), m.is_ortho()) })
}

/// A model that comes without a scene (`p:contentPart`): set up as PowerPoint inserts one, seen
/// so that it fills as much of its `w` × `h` point frame as it can.
pub fn unplaced(imp: &mut Imp, media: MediaId, w: f64, h: f64) -> Model3d {
    let fit = match imp.model_fits.get(&media) {
        Some(f) => *f,
        None => {
            let f = fit(imp, media);
            imp.model_fits.insert(media, f);
            f
        }
    };
    let Some(Fit { lo, hi, extent }) = fit else {
        log::warn!("pptx: 3-D model {media:?} can't be read; shown as inserted");
        return Model3d { media, ..Default::default() };
    };
    let mut m = Model3d::fitted(media, lo, hi);
    // The view's height on the slide (`viewport` frame heights) that brings the outline to the
    // frame's nearer edges.
    if let Some((hx, hy)) = extent
        && hx > 0.0
        && hy > 0.0
        && w > 0.0
        && h > 0.0
    {
        let focal = (w / (2.0 * hx)).min(h / (2.0 * hy));
        let half = (m.camera.fov.clamp(0.1, 179.0) / 2.0).to_radians().tan();
        let v = 2.0 * focal * half / h;
        if v.is_finite() && v > 0.0 {
            m.viewport = v;
        }
    }
    m
}

/// The model of a graphic frame `frame_h` points tall. `None` when its model part is missing (the
/// frame is then kept as an object with its picture).
pub fn model3d(imp: &mut Imp, part: &Part, el: &El, frame_h: f64) -> Option<Model3d> {
    let rel = part.rels.get(el.attr("r:embed")?)?;
    if rel.external {
        return None;
    }
    let target = rel.target.clone();
    let media = imp.media_for(&target)?;
    let cam = el.child("camera");
    let persp = cam.and_then(|c| c.child("perspective"));
    let ortho = cam.and_then(|c| c.child("orthographic"));
    let camera = Camera3d {
        pos: point(cam.and_then(|c| c.child("pos"))),
        up: cam.and_then(|c| c.child("up")).map(|u| vector(Some(u))).filter(|v| v.iter().any(|c| *c != 0.0)).unwrap_or([0.0, 1.0, 0.0]),
        look_at: point(cam.and_then(|c| c.child("lookAt"))),
        fov: match (persp, ortho) {
            (_, Some(_)) => 0.0,
            (Some(p), None) => p.f64("fov").filter(|v| v.is_finite() && *v > 0.0).map(|v| v / 60_000.0).unwrap_or(45.0),
            (None, None) => 45.0,
        },
        ortho: ortho.and_then(|o| o.f64("size")).filter(|v| v.is_finite() && *v > 0.0).map(|v| v / EMU_PER_M).unwrap_or(1.0),
    };
    let tr = el.child("trans");
    let scale = tr.and_then(|t| t.child("scale"));
    let rot = tr.and_then(|t| t.child("rot"));
    let poster = el.path(&["raster", "blip"]).and_then(|b| super::dml::media_ref(imp, part, b));
    let viewport = el.child("objViewport").and_then(|v| v.f64("viewportSz")).filter(|v| v.is_finite() && *v > 0.0);
    let lights: Vec<&El> = el.elements().filter(|c| c.local().ends_with("Light")).collect();
    let typed: Vec<Light3d> = lights.iter().take(64).filter_map(|l| light(l)).collect();
    let h = if frame_h.is_finite() && frame_h > 1e-6 { frame_h } else { 1.0 };
    Some(Model3d {
        media,
        poster,
        meter_per_unit: ratio(tr.and_then(|t| t.child("meterPerModelUnit")), 1.0),
        pre: vector(tr.and_then(|t| t.child("preTrans"))),
        scale: ["sx", "sy", "sz"].map(|k| ratio(scale.and_then(|s| s.child(k)), 1.0)),
        rot: [angle(rot, "ax"), angle(rot, "ay"), angle(rot, "az")],
        post: vector(tr.and_then(|t| t.child("postTrans"))),
        camera,
        viewport: viewport.map(|v| deckcraft_geom::emu_to_pt(v as i64) / h).unwrap_or(1.0),
        auto_fit: viewport.is_some(),
        lights: typed,
        lights_xml: (!lights.is_empty()).then(|| lights.iter().map(|l| part.keep(l)).collect()),
    })
}
