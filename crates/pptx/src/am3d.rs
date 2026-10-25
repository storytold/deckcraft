//! `am3d:model3d` ([MS-ODRAWXML] 2.31): a 3-D model's camera, transform, rasterized picture,
//! viewport and lights, read into [`Model3dPose`] and written back from it.
//!
//! Angles are 60,000ths of a degree; positions and vectors are EMU; ratios are `n`/`d`. The
//! specification gives the transform's order (pre-translation, scale, rotation, post-translation)
//! but not the order of `am3d:rot`'s three turns; DeckCraft reads and writes them as turns about X,
//! then Y, then Z, which is still to be checked against PowerPoint.

use deckcraft_model::{Model3dCamera, Model3dPose};

use crate::xml::{A, El, W};

pub const NS_AM3D: &str = "http://schemas.microsoft.com/office/drawing/2017/model3d";

/// The camera a model PowerPoint inserts gets: about 2.26 m in front of it, upright, with a 45°
/// perspective (EMU and 60,000ths of a degree).
const DEFAULT_CAMERA: Model3dCamera =
    Model3dCamera { pos: [0, 0, 81_360_000], up: [0, 36_000_000, 0], look_at: [0, 0, 0], fov: Some(2_700_000), ortho: None };

const ANGLE: f64 = 60_000.0;
/// Most an imported coordinate may be (well past any slide), so arithmetic on it can't overflow.
const MAX_COORD: i64 = 1 << 50;

fn coord(e: &El, name: &str) -> i64 {
    e.i64(name).unwrap_or(0).clamp(-MAX_COORD, MAX_COORD)
}

fn point(e: Option<&El>, names: [&str; 3]) -> Option<[i64; 3]> {
    let e = e?;
    Some([coord(e, names[0]), coord(e, names[1]), coord(e, names[2])])
}

fn ratio(e: Option<&El>) -> Option<(u64, u64)> {
    let e = e?;
    let (n, d) = (e.attr("n")?.trim().parse::<u64>().ok()?, e.attr("d")?.trim().parse::<u64>().ok()?);
    (d > 0).then_some((n, d))
}

fn signed_ratio(e: Option<&El>) -> Option<(i64, i64)> {
    let e = e?;
    let (n, d) = (e.i64("n")?, e.i64("d")?);
    (d != 0).then_some((n, d))
}

/// The pose of an `am3d:model3d`.
pub fn read_pose(m: &El) -> Model3dPose {
    let mut pose = Model3dPose::default();
    if let Some(c) = m.child("camera") {
        let xyz = ["x", "y", "z"];
        let d = ["dx", "dy", "dz"];
        if let (Some(pos), Some(up), Some(look_at)) = (point(c.child("pos"), xyz), point(c.child("up"), d), point(c.child("lookAt"), xyz)) {
            pose.camera = Some(Model3dCamera {
                pos,
                up,
                look_at,
                fov: c.child("perspective").and_then(|p| p.i64("fov")).map(|f| f.clamp(0, 10_800_000)),
                ortho: ratio(c.path(&["orthographic", "sz"])),
            });
        }
    }
    if let Some(t) = m.child("trans") {
        pose.meter_per_unit = ratio(t.child("meterPerModelUnit"));
        let d = ["dx", "dy", "dz"];
        pose.pre_trans = point(t.child("preTrans"), d);
        pose.post_trans = point(t.child("postTrans"), d);
        pose.scale = t.child("scale").and_then(|s| Some([signed_ratio(s.child("sx"))?, signed_ratio(s.child("sy"))?, signed_ratio(s.child("sz"))?]));
        if let Some(r) = t.child("rot") {
            let a = |k: &str| r.f64(k).filter(|v| v.is_finite()).map_or(0.0, |v| (v / ANGLE).rem_euclid(360.0));
            pose.rot = [a("ax"), a("ay"), a("az")];
        }
    }
    pose.viewport = m.child("objViewport").and_then(|v| v.i64("viewportSz")).map(|v| v.clamp(0, MAX_COORD));
    let lights: String =
        m.elements().filter(|e| matches!(e.local(), "ambientLight" | "ptLight" | "spotLight" | "dirLight" | "unkLight")).map(El::to_xml).collect();
    pose.lights = (!lights.is_empty()).then_some(lights);
    pose
}

fn angle(deg: f64) -> i64 {
    let d = if deg.is_finite() { deg.rem_euclid(360.0) } else { 0.0 };
    ((d * ANGLE).round() as i64).rem_euclid(21_600_000)
}

/// Writes `am3d:model3d` for the model part `rid`, its frame `cx` × `cy` EMU and its rasterized
/// picture `raster` (the fallback picture), in `pose`.
pub fn write_model3d(w: &mut W, rid: &str, frame: (i64, i64, i64, i64), raster: Option<&str>, pose: &Model3dPose) {
    let (x, y, cx, cy) = frame;
    w.open("am3d:model3d", A::new().a("r:embed", rid));
    w.open0("am3d:spPr");
    w.open0("a:xfrm");
    w.empty("a:off", A::new().a("x", x).a("y", y));
    w.empty("a:ext", A::new().a("cx", cx.max(0)).a("cy", cy.max(0)));
    w.close("a:xfrm");
    w.open("a:prstGeom", A::new().a("prst", "rect"));
    w.empty0("a:avLst");
    w.close("a:prstGeom");
    w.close("am3d:spPr");

    let cam = pose.camera.as_ref().unwrap_or(&DEFAULT_CAMERA);
    w.open0("am3d:camera");
    w.empty("am3d:pos", A::new().a("x", cam.pos[0]).a("y", cam.pos[1]).a("z", cam.pos[2]));
    w.empty("am3d:up", A::new().a("dx", cam.up[0]).a("dy", cam.up[1]).a("dz", cam.up[2]));
    w.empty("am3d:lookAt", A::new().a("x", cam.look_at[0]).a("y", cam.look_at[1]).a("z", cam.look_at[2]));
    match (cam.fov, cam.ortho) {
        (None, Some((n, d))) => {
            w.open0("am3d:orthographic");
            w.empty("am3d:sz", A::new().a("n", n).a("d", d));
            w.close("am3d:orthographic");
        }
        (fov, _) => w.empty("am3d:perspective", A::new().a("fov", fov.unwrap_or(2_700_000))),
    }
    w.close("am3d:camera");

    w.open0("am3d:trans");
    if let Some((n, d)) = pose.meter_per_unit {
        w.empty("am3d:meterPerModelUnit", A::new().a("n", n).a("d", d));
    }
    if let Some(t) = pose.pre_trans {
        w.empty("am3d:preTrans", A::new().a("dx", t[0]).a("dy", t[1]).a("dz", t[2]));
    }
    if let Some(s) = pose.scale {
        w.open0("am3d:scale");
        for (name, (n, d)) in ["am3d:sx", "am3d:sy", "am3d:sz"].into_iter().zip(s) {
            w.empty(name, A::new().a("n", n).a("d", d));
        }
        w.close("am3d:scale");
    }
    w.empty("am3d:rot", A::new().a("ax", angle(pose.rot[0])).a("ay", angle(pose.rot[1])).a("az", angle(pose.rot[2])));
    if let Some(t) = pose.post_trans {
        w.empty("am3d:postTrans", A::new().a("dx", t[0]).a("dy", t[1]).a("dz", t[2]));
    }
    w.close("am3d:trans");

    if let Some(r) = raster {
        w.open("am3d:raster", A::new().a("rName", "DeckCraft").a("rVer", env!("CARGO_PKG_VERSION")));
        w.empty("am3d:blip", A::new().a("r:embed", r));
        w.close("am3d:raster");
    }
    w.empty("am3d:objViewport", A::new().a("viewportSz", pose.viewport.unwrap_or(cx.max(cy).max(1))));
    if let Some(l) = &pose.lights {
        w.raw(l);
    }
    w.close("am3d:model3d");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(xml: &str) -> El {
        crate::xml::parse(xml.as_bytes()).expect("xml").root
    }

    const MODEL: &str = r#"<am3d:model3d xmlns:am3d="http://schemas.microsoft.com/office/drawing/2017/model3d" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" r:embed="rId2"><am3d:spPr><a:xfrm><a:off x="1" y="2"/><a:ext cx="3" cy="4"/></a:xfrm></am3d:spPr><am3d:camera><am3d:pos x="0" y="0" z="70000000"/><am3d:up dx="0" dy="36000000" dz="0"/><am3d:lookAt x="0" y="0" z="0"/><am3d:perspective fov="2700000"/></am3d:camera><am3d:trans><am3d:meterPerModelUnit n="25000" d="1000000"/><am3d:preTrans dx="-10" dy="20" dz="0"/><am3d:scale><am3d:sx n="1" d="2"/><am3d:sy n="1" d="2"/><am3d:sz n="1" d="2"/></am3d:scale><am3d:rot ax="1200000" ay="20160000" az="0"/><am3d:postTrans dx="5" dy="0" dz="0"/></am3d:trans><am3d:objViewport viewportSz="2000000"/><am3d:ambientLight><am3d:clr><a:scrgbClr r="50000" g="50000" b="50000"/></am3d:clr><am3d:illuminance n="1" d="2"/></am3d:ambientLight></am3d:model3d>"#;

    #[test]
    fn reads_every_part_of_the_pose() {
        let p = read_pose(&parse(MODEL));
        assert_eq!(p.rot, [20.0, 336.0, 0.0]);
        let c = p.camera.clone().expect("camera");
        assert_eq!((c.pos, c.up, c.fov, c.ortho), ([0, 0, 70_000_000], [0, 36_000_000, 0], Some(2_700_000), None));
        assert_eq!(p.meter_per_unit, Some((25_000, 1_000_000)));
        assert_eq!((p.pre_trans, p.post_trans), (Some([-10, 20, 0]), Some([5, 0, 0])));
        assert_eq!(p.scale, Some([(1, 2); 3]));
        assert_eq!(p.viewport, Some(2_000_000));
        assert!(p.lights.as_deref().is_some_and(|l| l.contains("ambientLight") && l.contains("illuminance")), "{:?}", p.lights);
    }

    #[test]
    fn writes_what_it_reads() {
        let p = read_pose(&parse(MODEL));
        let mut w = W::new();
        w.open("root", A::new().a("xmlns:am3d", NS_AM3D).a("xmlns:a", crate::opc::NS_A).a("xmlns:r", crate::opc::NS_R));
        write_model3d(&mut w, "rId9", (1, 2, 3, 4), Some("rId10"), &p);
        w.close("root");
        let xml = String::from_utf8(w.finish()).expect("utf8");
        let back = parse(&xml);
        let m = back.child("model3d").expect("model3d");
        assert_eq!(m.attr("r:embed"), Some("rId9"));
        assert_eq!(read_pose(m), p, "{xml}");
        assert_eq!(m.path(&["raster", "blip"]).and_then(|b| b.attr("r:embed")), Some("rId10"));
    }

    #[test]
    fn a_new_model_gets_a_default_camera_and_odd_values_are_tamed() {
        let mut w = W::new();
        w.open("root", A::new().a("xmlns:am3d", NS_AM3D).a("xmlns:a", crate::opc::NS_A).a("xmlns:r", crate::opc::NS_R));
        write_model3d(&mut w, "rId1", (0, 0, 100, 50), None, &Model3dPose { rot: [-90.0, f64::NAN, 725.0], ..Default::default() });
        w.close("root");
        let back = parse(&String::from_utf8(w.finish()).expect("utf8"));
        let m = back.child("model3d").expect("model3d");
        let p = read_pose(m);
        assert_eq!(p.rot, [270.0, 0.0, 5.0]);
        assert_eq!(p.camera, Some(DEFAULT_CAMERA));
        assert_eq!(p.viewport, Some(100));
        assert!(m.child("raster").is_none());

        let wild = parse(
            r#"<model3d><camera><pos x="99999999999999999999" y="a" z="1"/><up dx="0" dy="1" dz="0"/><lookAt x="0" y="0" z="0"/><perspective fov="-5"/></camera><trans><meterPerModelUnit n="1" d="0"/><rot ax="nan" ay="-60000" az="99999999999999"/></trans></model3d>"#,
        );
        let p = read_pose(&wild);
        assert_eq!(p.meter_per_unit, None, "a zero denominator is dropped");
        assert_eq!(p.camera.map(|c| (c.pos[1], c.fov)), Some((0, Some(0))));
        assert!(p.rot.iter().all(|r| r.is_finite() && (0.0..360.0).contains(r)), "{:?}", p.rot);
    }
}
