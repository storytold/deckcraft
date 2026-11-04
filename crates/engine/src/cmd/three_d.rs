//! Format Shape ▸ 3-D Format and 3-D Rotation: a shape's `a:scene3d` / `a:sp3d`, kept as XML on
//! the model (`Effects::raw3d`), read into fields, changed, and written back.

use serde_json::Value;

/// Camera presets PowerPoint offers (3-D Rotation ▸ Presets).
pub const CAMERAS: &[&str] = &[
    "orthographicFront",
    "isometricTopUp",
    "isometricTopDown",
    "isometricBottomUp",
    "isometricBottomDown",
    "isometricLeftUp",
    "isometricLeftDown",
    "isometricRightUp",
    "isometricRightDown",
    "obliqueTopLeft",
    "obliqueTopRight",
    "obliqueBottomLeft",
    "obliqueBottomRight",
    "perspectiveFront",
    "perspectiveLeft",
    "perspectiveRight",
    "perspectiveAbove",
    "perspectiveBelow",
    "perspectiveAboveLeftFacing",
    "perspectiveAboveRightFacing",
    "perspectiveContrastingLeftFacing",
    "perspectiveContrastingRightFacing",
    "perspectiveHeroicLeftFacing",
    "perspectiveHeroicRightFacing",
    "perspectiveHeroicExtremeLeftFacing",
    "perspectiveHeroicExtremeRightFacing",
    "perspectiveRelaxed",
    "perspectiveRelaxedModerately",
];

/// Bevel presets (`ST_BevelPresetType`).
pub const BEVELS: &[&str] =
    &["circle", "relaxedInset", "cross", "coolSlant", "angle", "softRound", "convex", "slope", "divot", "riblet", "hardEdge", "artDeco"];

/// Materials (`ST_PresetMaterialType`).
pub const MATERIALS: &[&str] =
    &["warmMatte", "matte", "plastic", "metal", "dkEdge", "softEdge", "flat", "legacyWireframe", "powder", "translucentPowder", "clear", "softmetal"];

/// Light rigs (`ST_LightRigType`) PowerPoint offers.
pub const LIGHTS: &[&str] = &[
    "threePt",
    "balanced",
    "soft",
    "harsh",
    "flood",
    "contrasting",
    "morning",
    "sunrise",
    "sunset",
    "chilly",
    "freezing",
    "flat",
    "twoPt",
    "glow",
    "brightRoom",
];

/// Every camera preset the file format defines (`ST_PresetCameraType`); a file's camera is kept
/// only when it is one of these.
pub const ALL_CAMERAS: &[&str] = &[
    "legacyObliqueTopLeft",
    "legacyObliqueTop",
    "legacyObliqueTopRight",
    "legacyObliqueLeft",
    "legacyObliqueFront",
    "legacyObliqueRight",
    "legacyObliqueBottomLeft",
    "legacyObliqueBottom",
    "legacyObliqueBottomRight",
    "legacyPerspectiveTopLeft",
    "legacyPerspectiveTop",
    "legacyPerspectiveTopRight",
    "legacyPerspectiveLeft",
    "legacyPerspectiveFront",
    "legacyPerspectiveRight",
    "legacyPerspectiveBottomLeft",
    "legacyPerspectiveBottom",
    "legacyPerspectiveBottomRight",
    "orthographicFront",
    "isometricTopUp",
    "isometricTopDown",
    "isometricBottomUp",
    "isometricBottomDown",
    "isometricLeftUp",
    "isometricLeftDown",
    "isometricRightUp",
    "isometricRightDown",
    "isometricOffAxis1Left",
    "isometricOffAxis1Right",
    "isometricOffAxis1Top",
    "isometricOffAxis2Left",
    "isometricOffAxis2Right",
    "isometricOffAxis2Top",
    "isometricOffAxis3Left",
    "isometricOffAxis3Right",
    "isometricOffAxis3Bottom",
    "isometricOffAxis4Left",
    "isometricOffAxis4Right",
    "isometricOffAxis4Bottom",
    "obliqueTopLeft",
    "obliqueTop",
    "obliqueTopRight",
    "obliqueLeft",
    "obliqueRight",
    "obliqueBottomLeft",
    "obliqueBottom",
    "obliqueBottomRight",
    "perspectiveFront",
    "perspectiveLeft",
    "perspectiveRight",
    "perspectiveAbove",
    "perspectiveBelow",
    "perspectiveAboveLeftFacing",
    "perspectiveAboveRightFacing",
    "perspectiveContrastingLeftFacing",
    "perspectiveContrastingRightFacing",
    "perspectiveHeroicLeftFacing",
    "perspectiveHeroicRightFacing",
    "perspectiveHeroicExtremeLeftFacing",
    "perspectiveHeroicExtremeRightFacing",
    "perspectiveRelaxed",
    "perspectiveRelaxedModerately",
];

/// Every light rig the file format defines (`ST_LightRigType`).
pub const ALL_LIGHTS: &[&str] = &[
    "legacyFlat1",
    "legacyFlat2",
    "legacyFlat3",
    "legacyFlat4",
    "legacyNormal1",
    "legacyNormal2",
    "legacyNormal3",
    "legacyNormal4",
    "legacyHarsh1",
    "legacyHarsh2",
    "legacyHarsh3",
    "legacyHarsh4",
    "threePt",
    "balanced",
    "soft",
    "harsh",
    "flood",
    "contrasting",
    "morning",
    "sunrise",
    "sunset",
    "chilly",
    "freezing",
    "flat",
    "twoPt",
    "glow",
    "brightRoom",
];

/// Light rig directions (`ST_LightRigDirection`).
pub const LIGHT_DIRS: &[&str] = &["tl", "t", "tr", "l", "r", "bl", "b", "br"];

/// Every material the file format defines (`ST_PresetMaterialType`).
pub const ALL_MATERIALS: &[&str] = &[
    "legacyMatte",
    "legacyPlastic",
    "legacyMetal",
    "legacyWireframe",
    "matte",
    "plastic",
    "metal",
    "warmMatte",
    "translucentPowder",
    "powder",
    "dkEdge",
    "softEdge",
    "clear",
    "flat",
    "softmetal",
];

/// One bevel: preset, width and height in points.
#[derive(Clone, Debug, PartialEq)]
pub struct Bevel {
    pub preset: String,
    pub w: f64,
    pub h: f64,
}

/// The 3-D settings of a shape.
#[derive(Clone, Debug, PartialEq)]
pub struct Shape3d {
    pub camera: String,
    /// Camera rotation (X, Y, Z in degrees, as PowerPoint shows them), when set.
    pub rot: Option<(f64, f64, f64)>,
    pub light: String,
    pub light_dir: String,
    pub top: Option<Bevel>,
    pub bottom: Option<Bevel>,
    /// Depth (extrusion), contour width and distance from ground, in points.
    pub depth: f64,
    pub contour: f64,
    pub z: f64,
    pub material: String,
    /// `a:extrusionClr` / `a:contourClr` XML, kept as is.
    pub colors: String,
}

impl Default for Shape3d {
    fn default() -> Self {
        Shape3d {
            camera: "orthographicFront".into(),
            rot: None,
            light: "threePt".into(),
            light_dir: "t".into(),
            top: None,
            bottom: None,
            depth: 0.0,
            contour: 0.0,
            z: 0.0,
            material: "warmMatte".into(),
            colors: String::new(),
        }
    }
}

const EMU: f64 = 12700.0;
const ANGLE: f64 = 60000.0;
/// Largest length kept, in points (PowerPoint's own limit).
const MAX_PT: f64 = 1584.0;

/// `value` when it is one of `known`, else `default`. Values from a file are written back into
/// XML as they are, so nothing outside these lists may pass.
fn known(value: Option<&str>, known: &[&str], default: &str) -> String {
    value.filter(|v| known.contains(v)).unwrap_or(default).to_string()
}

fn num(e: &deckcraft_pptx::XmlEl, name: &str) -> Option<f64> {
    e.attr(name).and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| v.is_finite())
}

impl Shape3d {
    /// Reads kept 3-D XML (`a:scene3d` and `a:sp3d`); unknown or invalid parts take their defaults.
    pub fn parse(raw: &str) -> Shape3d {
        let mut s = Shape3d::default();
        let Ok(doc) = deckcraft_pptx::parse_xml(format!("<r>{raw}</r>").as_bytes()) else { return s };
        let root = &doc.root;
        if let Some(cam) = root.path(&["scene3d", "camera"]) {
            s.camera = known(cam.attr("prst"), ALL_CAMERAS, "orthographicFront");
            if let Some(rot) = cam.child("rot") {
                let d = |n| num(rot, n).unwrap_or(0.0) / ANGLE;
                s.rot = Some((d("lon"), d("lat"), d("rev")));
            }
        }
        if let Some(l) = root.path(&["scene3d", "lightRig"]) {
            s.light = known(l.attr("rig"), ALL_LIGHTS, "threePt");
            s.light_dir = known(l.attr("dir"), LIGHT_DIRS, "t");
        }
        if let Some(sp) = root.child("sp3d") {
            let pt = |n, lo: f64| (num(sp, n).unwrap_or(0.0) / EMU).clamp(lo, MAX_PT);
            s.depth = pt("extrusionH", 0.0);
            s.contour = pt("contourW", 0.0);
            s.z = pt("z", -MAX_PT);
            s.material = known(sp.attr("prstMaterial"), ALL_MATERIALS, "warmMatte");
            for (name, slot) in [("bevelT", &mut s.top), ("bevelB", &mut s.bottom)] {
                if let Some(b) = sp.child(name) {
                    let len = |n| (num(b, n).unwrap_or(76200.0) / EMU).clamp(0.0, MAX_PT);
                    *slot = Some(Bevel { preset: known(b.attr("prst"), BEVELS, "circle"), w: len("w"), h: len("h") });
                }
            }
            // Colours are re-serialized by the XML writer (escaped); only `a:`-prefixed ones,
            // since they are written back inside our own `a:sp3d`.
            s.colors = sp.elements().filter(|c| (c.is("extrusionClr") || c.is("contourClr")) && c.prefix() == "a").map(|c| c.to_xml()).collect();
        }
        s
    }

    /// Whether this is a flat shape with no 3-D at all.
    pub fn is_flat(&self) -> bool {
        self.camera == "orthographicFront"
            && self.rot.is_none_or(|(x, y, z)| x == 0.0 && y == 0.0 && z == 0.0)
            && self.top.is_none()
            && self.bottom.is_none()
            && self.depth == 0.0
            && self.contour == 0.0
            && self.z == 0.0
    }

    /// The `a:scene3d` and `a:sp3d` XML, or `None` for a flat shape.
    pub fn to_xml(&self) -> Option<String> {
        if self.is_flat() {
            return None;
        }
        let e = |pt: f64| (pt * EMU).round() as i64;
        let mut x = format!("<a:scene3d><a:camera prst=\"{}\"", self.camera);
        match self.rot {
            Some((rx, ry, rz)) => {
                let a = |d: f64| (d.rem_euclid(360.0) * ANGLE).round() as i64 % 21_600_000;
                x += &format!("><a:rot lat=\"{}\" lon=\"{}\" rev=\"{}\"/></a:camera>", a(ry), a(rx), a(rz));
            }
            None => x += "/>",
        }
        x += &format!("<a:lightRig rig=\"{}\" dir=\"{}\"/></a:scene3d><a:sp3d", self.light, self.light_dir);
        if self.z != 0.0 {
            x += &format!(" z=\"{}\"", e(self.z));
        }
        if self.depth > 0.0 {
            x += &format!(" extrusionH=\"{}\"", e(self.depth));
        }
        if self.contour > 0.0 {
            x += &format!(" contourW=\"{}\"", e(self.contour));
        }
        if self.material != "warmMatte" {
            x += &format!(" prstMaterial=\"{}\"", self.material);
        }
        x += ">";
        for (tag, b) in [("bevelT", &self.top), ("bevelB", &self.bottom)] {
            if let Some(b) = b {
                x += &format!("<a:{tag} w=\"{}\" h=\"{}\" prst=\"{}\"/>", e(b.w), e(b.h), b.preset);
            }
        }
        x += &self.colors;
        x += "</a:sp3d>";
        Some(x)
    }

    /// Applies `{camera?, rotX?, rotY?, rotZ?, z?, bevelTop?, bevelTopW?, bevelTopH?, bevelBottom?,
    /// bevelBottomW?, bevelBottomH?, depth?, contour?, material?, lighting?, lightAngle?}`.
    pub fn apply(&mut self, p: &Value) {
        let f = |k: &str| p.get(k).and_then(Value::as_f64).filter(|v| v.is_finite());
        let pick = |k: &str, list: &[&str]| p.get(k).and_then(Value::as_str).filter(|v| list.contains(v)).map(str::to_string);
        if let Some(c) = pick("camera", CAMERAS) {
            self.camera = c;
            // A preset sets its own angles.
            self.rot = None;
        }
        let (rx, ry, rz) = (f("rotX"), f("rotY"), f("rotZ"));
        if rx.is_some() || ry.is_some() || rz.is_some() {
            let (ox, oy, oz) = self.rot.unwrap_or((0.0, 0.0, 0.0));
            let d = |v: Option<f64>, o: f64| v.map_or(o, |v| v.rem_euclid(360.0));
            self.rot = Some((d(rx, ox), d(ry, oy), d(rz, oz)));
        }
        if let Some(v) = f("z") {
            self.z = v.clamp(-1584.0, 1584.0);
        }
        for (key, slot) in [("bevelTop", &mut self.top), ("bevelBottom", &mut self.bottom)] {
            match p.get(key).and_then(Value::as_str) {
                Some("none") => *slot = None,
                Some(b) if BEVELS.contains(&b) => {
                    let old = slot.take().unwrap_or(Bevel { preset: String::new(), w: 6.0, h: 6.0 });
                    *slot = Some(Bevel { preset: b.to_string(), ..old });
                }
                _ => {}
            }
            if let Some(b) = slot.as_mut() {
                if let Some(w) = f(&format!("{key}W")) {
                    b.w = w.clamp(0.0, 1584.0);
                }
                if let Some(h) = f(&format!("{key}H")) {
                    b.h = h.clamp(0.0, 1584.0);
                }
            }
        }
        if let Some(v) = f("depth") {
            self.depth = v.clamp(0.0, 1584.0);
        }
        if let Some(v) = f("contour") {
            self.contour = v.clamp(0.0, 1584.0);
        }
        if let Some(m) = pick("material", MATERIALS) {
            self.material = m;
        }
        if let Some(l) = pick("lighting", LIGHTS) {
            self.light = l;
        }
    }
}

/// Sets a shape's 3-D settings on its effects, keeping `bevel` (the top bevel's preset) in step.
pub fn store(e: &mut deckcraft_model::Effects, s: &Shape3d) {
    e.raw3d = s.to_xml();
    e.bevel = if e.raw3d.is_some() { s.top.as_ref().map(|b| b.preset.clone()) } else { None };
}

#[cfg(test)]
mod tests {
    use super::*;

    const NS: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main""#;

    /// Preset names from a file are written back into XML; anything outside the format's lists
    /// (here, values that would close the tag and inject markup once unescaped) falls back to the
    /// default instead.
    #[test]
    fn unknown_presets_from_a_file_are_not_written_back() {
        let evil = "x&quot;/&gt;&lt;a:evil/&gt;";
        let raw = format!(
            r#"<a:scene3d {NS}><a:camera prst="{evil}"/><a:lightRig rig="{evil}" dir="{evil}"/></a:scene3d><a:sp3d {NS} extrusionH="12700" prstMaterial="{evil}"><a:bevelT w="25400" h="25400" prst="{evil}"/><a:extrusionClr><a:srgbClr val="{evil}"/></a:extrusionClr></a:sp3d>"#
        );
        let s = Shape3d::parse(&raw);
        assert_eq!(
            (s.camera.as_str(), s.light.as_str(), s.light_dir.as_str(), s.material.as_str()),
            ("orthographicFront", "threePt", "t", "warmMatte")
        );
        assert_eq!(s.top.as_ref().map(|b| b.preset.as_str()), Some("circle"));
        assert_eq!(s.depth, 1.0);
        let xml = s.to_xml().expect("not flat");
        assert!(!xml.contains("<a:evil"), "{xml}");
        // What is written is well-formed XML with the colour's value escaped.
        let doc = deckcraft_pptx::parse_xml(format!("<r {NS}>{xml}</r>").as_bytes()).expect("well-formed");
        assert_eq!(doc.root.path(&["sp3d", "extrusionClr", "srgbClr"]).and_then(|c| c.attr("val")), Some(r#"x"/><a:evil/>"#));
    }

    #[test]
    fn known_presets_round_trip() {
        let raw = format!(
            r#"<a:scene3d {NS}><a:camera prst="legacyObliqueTopLeft"><a:rot lat="1200000" lon="600000" rev="0"/></a:camera><a:lightRig rig="legacyHarsh2" dir="tl"/></a:scene3d><a:sp3d {NS} z="-12700" contourW="12700" prstMaterial="legacyMetal"><a:bevelB w="38100" h="12700" prst="artDeco"/></a:sp3d>"#
        );
        let s = Shape3d::parse(&raw);
        assert_eq!(s.camera, "legacyObliqueTopLeft");
        assert_eq!(s.rot, Some((10.0, 20.0, 0.0)));
        assert_eq!((s.light.as_str(), s.light_dir.as_str(), s.material.as_str()), ("legacyHarsh2", "tl", "legacyMetal"));
        assert_eq!((s.z, s.contour), (-1.0, 1.0));
        assert_eq!(s.bottom, Some(Bevel { preset: "artDeco".into(), w: 3.0, h: 1.0 }));
        assert_eq!(Shape3d::parse(&s.to_xml().expect("3-D")), s);
        // Not XML at all: defaults.
        assert_eq!(Shape3d::parse("<a:scene3d><a:camera"), Shape3d::default());
    }
}
