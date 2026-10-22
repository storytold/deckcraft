//! `am3d:model3d`: a 3-D model in a graphic frame, its elements in the order PowerPoint writes
//! and reads them.

use deckcraft_model::model3d::{LightKind, Model3d};

use crate::xml::{A, W};

pub const NS_AM3D: &str = "http://schemas.microsoft.com/office/drawing/2017/model3d";
const EMU_PER_M: f64 = 36_000_000.0;

fn emu(m: f64) -> i64 {
    let v = (m * EMU_PER_M).round();
    if v.is_finite() { v.clamp(-1e15, 1e15) as i64 } else { 0 }
}

fn point(w: &mut W, tag: &str, p: [f64; 3]) {
    w.empty(tag, A::new().a("x", emu(p[0])).a("y", emu(p[1])).a("z", emu(p[2])));
}

fn vector(w: &mut W, tag: &str, v: [f64; 3]) {
    w.empty(tag, A::new().a("dx", emu(v[0])).a("dy", emu(v[1])).a("dz", emu(v[2])));
}

/// A ratio as `n` over a million (positive where the schema wants it).
fn ratio(w: &mut W, tag: &str, v: f64, positive: bool) {
    let n = (v * 1e6).round();
    let n = if n.is_finite() { n.clamp(-1e15, 1e15) as i64 } else { 1_000_000 };
    let n = if positive && n <= 0 { 1 } else { n };
    w.empty(tag, A::new().a("n", n).a("d", 1_000_000));
}

/// 60 000ths of a degree in `[0, 21 600 000)`.
fn angle(deg: f64) -> i64 {
    (deckcraft_geom::space::normalize_deg(deg) * 60_000.0).round().clamp(0.0, 21_599_999.0) as i64
}

fn scrgb(w: &mut W, c: [f64; 3]) {
    let p = |v: f64| if v.is_finite() { (v.max(0.0) * 100_000.0).round().min(1e9) as i64 } else { 100_000 };
    w.open0("am3d:clr");
    w.empty("a:scrgbClr", A::new().a("r", p(c[0])).a("g", p(c[1])).a("b", p(c[2])));
    w.close("am3d:clr");
}

/// The `am3d:model3d` element for model `m` in a frame `cx` × `cy` EMU (`h_pt` points tall), its
/// model part at relationship `model_rid` and its picture, if any, at `raster_rid`.
pub fn model3d(w: &mut W, m: &Model3d, model_rid: &str, raster_rid: Option<&str>, cx: i64, cy: i64, h_pt: f64) {
    w.open("am3d:model3d", A::new().a("r:embed", model_rid));
    w.open0("am3d:spPr");
    w.open0("a:xfrm");
    w.empty("a:off", A::new().a("x", 0).a("y", 0));
    w.empty("a:ext", A::new().a("cx", cx).a("cy", cy));
    w.close("a:xfrm");
    w.open("a:prstGeom", A::new().a("prst", "rect"));
    w.empty0("a:avLst");
    w.close("a:prstGeom");
    w.close("am3d:spPr");

    let c = &m.camera;
    w.open0("am3d:camera");
    point(w, "am3d:pos", c.pos);
    vector(w, "am3d:up", c.up);
    point(w, "am3d:lookAt", c.look_at);
    if m.is_ortho() {
        // Without `objViewport` the view spans the frame's height: fold the viewport in.
        let size = if m.auto_fit { c.ortho } else { c.ortho / m.span(1.0).max(1e-6) };
        w.empty("am3d:orthographic", A::new().a("size", emu(size).max(1)));
    } else {
        // Without `objViewport` the field of view spans the frame's height: fold the viewport in.
        let fov = if m.auto_fit {
            c.fov
        } else {
            let half = (c.fov.clamp(0.1, 179.0) / 2.0).to_radians().tan() / m.span(1.0).max(1e-6);
            (2.0 * half.atan()).to_degrees()
        };
        w.empty("am3d:perspective", A::new().a("fov", (fov.clamp(0.1, 179.0) * 60_000.0).round() as i64));
    }
    w.close("am3d:camera");

    w.open0("am3d:trans");
    ratio(w, "am3d:meterPerModelUnit", m.meter_per_unit, true);
    vector(w, "am3d:preTrans", m.pre);
    w.open0("am3d:scale");
    ratio(w, "am3d:sx", m.scale[0], false);
    ratio(w, "am3d:sy", m.scale[1], false);
    ratio(w, "am3d:sz", m.scale[2], false);
    w.close("am3d:scale");
    let [ax, ay, az] = m.rot.map(angle);
    w.empty("am3d:rot", A::new().o("ax", (ax != 0).then_some(ax)).o("ay", (ay != 0).then_some(ay)).o("az", (az != 0).then_some(az)));
    vector(w, "am3d:postTrans", m.post);
    w.close("am3d:trans");

    if let Some(rid) = raster_rid {
        w.open("am3d:raster", A::new().a("rName", "DeckCraft").a("rVer", env!("CARGO_PKG_VERSION")));
        w.empty("am3d:blip", A::new().a("r:embed", rid));
        w.close("am3d:raster");
    }
    if m.auto_fit {
        let size = deckcraft_geom::pt_to_emu(m.span(h_pt)).max(1);
        w.empty("am3d:objViewport", A::new().a("viewportSz", size));
    }

    match &m.lights_xml {
        Some(xml) => w.raw(xml),
        None => {
            for l in &m.lights {
                let k = if l.intensity.is_finite() { l.intensity.max(0.0) } else { 0.0 };
                match l.kind {
                    LightKind::Ambient => {
                        w.open0("am3d:ambientLight");
                        scrgb(w, l.color);
                        ratio(w, "am3d:illuminance", k, true);
                        w.close("am3d:ambientLight");
                    }
                    LightKind::Point => {
                        w.open("am3d:ptLight", A::new().a("rad", 0));
                        scrgb(w, l.color);
                        ratio(w, "am3d:intensity", k, true);
                        point(w, "am3d:pos", l.pos);
                        w.close("am3d:ptLight");
                    }
                    // Only ever read from files, which keep their XML.
                    LightKind::Directional | LightKind::Spot => {}
                }
            }
        }
    }
    w.close("am3d:model3d");
}
