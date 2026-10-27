use super::*;

/// Keeps a side length in `0..=MAX_PT` so a huge value cannot exhaust memory when rendering.
/// An overflow to infinity lands on the limit; NaN becomes 0.
pub(super) fn clamp_side(v: f64) -> f64 {
    if v.is_nan() { 0.0 } else { v.clamp(0.0, MAX_PT) }
}

/// Keeps a coordinate within `±POS_LIMIT`.
pub(super) fn clamp_pos(v: f64) -> f64 {
    if v.is_nan() { 0.0 } else { v.clamp(-POS_LIMIT, POS_LIMIT) }
}

const SIZE_POS: &str = "shape.sizePosition";

/// Smallest and largest side the dialog will write, in points (220 in).
const MIN_PT: f64 = 0.01;

const MAX_PT: f64 = 15_840.0;

const POS_LIMIT: f64 = 100_000.0;

const MAX_SCALE: f64 = 10_000.0;

#[derive(Clone, Copy)]
struct SizeSpec {
    pub(crate) x: Option<f64>,
    pub(crate) y: Option<f64>,
    pub(crate) w: Option<f64>,
    pub(crate) h: Option<f64>,
    pub(crate) rot: Option<f64>,
    scale_w: Option<f64>,
    scale_h: Option<f64>,
    lock: bool,
}

impl SizeSpec {
    fn width_set(self) -> bool {
        self.w.is_some() || self.scale_w.is_some()
    }
    fn height_set(self) -> bool {
        self.h.is_some() || self.scale_h.is_some()
    }
    fn any(self) -> bool {
        self.x.is_some() || self.y.is_some() || self.width_set() || self.height_set() || self.rot.is_some()
    }
}

fn finite_key(p: &Value, key: &str) -> Result<Option<f64>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => match v.as_f64() {
            Some(n) if n.is_finite() => Ok(Some(n)),
            _ => Err(bad(SIZE_POS, format!("`{key}` must be a finite number"))),
        },
    }
}

fn in_range(v: Option<f64>, key: &str, lo: f64, hi: f64) -> Result<Option<f64>> {
    if let Some(n) = v
        && !(lo..=hi).contains(&n)
    {
        return Err(bad(SIZE_POS, format!("`{key}` is out of range")));
    }
    Ok(v)
}

fn size_spec(p: &Value) -> Result<SizeSpec> {
    let lock = match p.get("lockAspect") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(bad(SIZE_POS, "`lockAspect` must be true or false")),
    };
    Ok(SizeSpec {
        x: in_range(finite_key(p, "x")?, "x", -POS_LIMIT, POS_LIMIT)?,
        y: in_range(finite_key(p, "y")?, "y", -POS_LIMIT, POS_LIMIT)?,
        w: in_range(finite_key(p, "w")?, "w", MIN_PT, MAX_PT)?,
        h: in_range(finite_key(p, "h")?, "h", MIN_PT, MAX_PT)?,
        rot: finite_key(p, "rot")?,
        scale_w: in_range(finite_key(p, "scaleW")?, "scaleW", f64::MIN_POSITIVE, MAX_SCALE)?,
        scale_h: in_range(finite_key(p, "scaleH")?, "scaleH", f64::MIN_POSITIVE, MAX_SCALE)?,
        lock,
    })
}

fn norm_deg(r: f64) -> f64 {
    let r = r % 360.0;
    if r < 0.0 { r + 360.0 } else { r }
}

/// Width wins when both sides are set with the aspect locked, matching `shape.resize`.
fn apply_size(x: &mut deckcraft_geom::Xfrm, spec: SizeSpec) {
    let ar = if x.h > 0.0 { x.w / x.h } else { 1.0 };
    let mut nw = x.w;
    let mut nh = x.h;
    if let Some(p) = spec.scale_w {
        nw = x.w * p / 100.0;
    }
    if let Some(p) = spec.scale_h {
        nh = x.h * p / 100.0;
    }
    if let Some(w) = spec.w {
        nw = w;
    }
    if let Some(h) = spec.h {
        nh = h;
    }
    if spec.lock && (spec.width_set() || spec.height_set()) {
        if spec.width_set() {
            nh = if ar > 0.0 { nw / ar } else { nh };
        } else {
            nw = nh * ar;
        }
    }
    x.w = nw.clamp(MIN_PT, MAX_PT);
    x.h = nh.clamp(MIN_PT, MAX_PT);
    if let Some(px) = spec.x {
        x.x = px;
    }
    if let Some(py) = spec.y {
        x.y = py;
    }
    if let Some(r) = spec.rot {
        x.rot = norm_deg(r);
    }
}

pub(super) fn size_position(s: &mut Session, p: &Value) -> Result<Value> {
    let spec = size_spec(p)?;
    if !spec.any() {
        let (x, y, w, h, rot) = {
            let st = s.doc()?;
            let id = targets(s, p)?.first().copied().ok_or_else(|| bad(SIZE_POS, "nothing selected"))?;
            let sh = st.shape(id).ok_or_else(|| bad(SIZE_POS, "no such shape"))?;
            let x = xfrm_of(&st.doc, &st.selection, sh);
            (x.x, x.y, x.w, x.h, x.rot)
        };
        s.ui_requests.push(crate::UiRequest::Dialog {
            id: "sizePosition".into(),
            params: json!({"x": x, "y": y, "w": w, "h": h, "rot": rot, "scaleW": 100.0, "scaleH": 100.0, "lockAspect": false}),
        });
        return Ok(Value::Null);
    }
    let changed = {
        let ids = targets(s, p)?;
        let st = s.doc()?;
        let free: Vec<_> = ids.iter().copied().filter(|id| !st.shape(*id).is_some_and(|sh| sh.locked)).collect::<Vec<_>>();
        if !ids.is_empty() && free.is_empty() {
            return Err(bad(SIZE_POS, "the object is locked"));
        }
        free.iter().any(|id| {
            st.shape(*id).is_some_and(|sh| {
                let mut x = xfrm_of(&st.doc, &st.selection, sh);
                let before = x;
                apply_size(&mut x, spec);
                x != before
            })
        })
    };
    if !changed {
        return Ok(Value::Null);
    }
    edit_geometry(s, p, SIZE_POS, |sh| {
        if let Some(x) = sh.xfrm.as_mut() {
            apply_size(x, spec);
        }
        Ok(())
    })
}

pub(super) fn rotate(s: &mut Session, p: &Value) -> Result<Value> {
    // A trackpad twist sends many small steps. `gesture: move` keeps them as one undo;
    // `gesture: end` commits that step and does not rotate again.
    if p.get("gesture").and_then(Value::as_str) == Some("end") {
        s.commit_named("Rotation");
        return Ok(json!({}));
    }
    if p.get("gesture").and_then(Value::as_str) == Some("move") {
        s.begin("Rotation");
    }
    let (abs, by) = (f64_param(p, "deg"), f64_param(p, "by"));
    edit_geometry(s, p, "shape.rotate", |sh| {
        if let Some(x) = sh.xfrm.as_mut() {
            let r = abs.unwrap_or(x.rot) + by.unwrap_or(0.0);
            x.rot = ((r % 360.0) + 360.0) % 360.0;
        }
        Ok(())
    })
}

pub(super) fn flip(s: &mut Session, p: &Value) -> Result<Value> {
    let v = str_param(p, "axis").unwrap_or("horizontal").starts_with('v');
    edit_geometry(s, p, "shape.flip", |sh| {
        if let Some(x) = sh.xfrm.as_mut() {
            if v {
                x.flip_v = !x.flip_v;
            } else {
                x.flip_h = !x.flip_h;
            }
            // `Xfrm::affine` flips first and rotates second; a true mirror also negates the rotation.
            x.rot = (360.0 - x.rot_norm()) % 360.0;
        }
        Ok(())
    })
}

#[cfg(test)]
mod size_position_tests {
    use serde_json::json;

    use crate::Session;

    fn deck() -> (Session, u32) {
        let mut s = Session::with_new();
        s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
        let id = s.execute("shape.insert", &json!({"preset": "rect", "rect": [40.0, 50.0, 200.0, 100.0]})).unwrap()["id"].as_u64().unwrap() as u32;
        (s, id)
    }

    fn geom(s: &mut Session, id: u32) -> (f64, f64, f64, f64, f64) {
        let v = s.execute("shape.inspect", &json!({"id": id})).unwrap();
        let b = &v["box"];
        (b["x"].as_f64().unwrap(), b["y"].as_f64().unwrap(), b["w"].as_f64().unwrap(), b["h"].as_f64().unwrap(), b["rot"].as_f64().unwrap_or(0.0))
    }

    fn undos(s: &Session) -> usize {
        s.doc().unwrap().history.undo.len()
    }

    #[test]
    fn sets_position_size_and_rotation_in_one_undo() {
        let (mut s, id) = deck();
        let n = undos(&s);
        s.execute("shape.sizePosition", &json!({"x": 80.0, "y": 90.0, "w": 240.0, "h": 60.0, "rot": 370.0})).unwrap();
        assert_eq!(geom(&mut s, id), (80.0, 90.0, 240.0, 60.0, 10.0));
        assert_eq!(undos(&s), n + 1);
        assert_eq!(s.doc().unwrap().history.undo.last().unwrap().label, "Size and Position…");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(geom(&mut s, id), (40.0, 50.0, 200.0, 100.0, 0.0));
    }

    #[test]
    fn lock_aspect_and_scale_share_one_step() {
        let (mut s, id) = deck();
        s.execute("shape.sizePosition", &json!({"w": 300.0, "lockAspect": true})).unwrap();
        assert_eq!(geom(&mut s, id), (40.0, 50.0, 300.0, 150.0, 0.0));
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("shape.sizePosition", &json!({"h": 50.0, "lockAspect": true})).unwrap();
        assert_eq!(geom(&mut s, id), (40.0, 50.0, 100.0, 50.0, 0.0));
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("shape.sizePosition", &json!({"scaleW": 25.0, "scaleH": 150.0})).unwrap();
        assert_eq!(geom(&mut s, id), (40.0, 50.0, 50.0, 150.0, 0.0));
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("shape.sizePosition", &json!({"scaleH": 50.0, "lockAspect": true})).unwrap();
        assert_eq!(geom(&mut s, id), (40.0, 50.0, 100.0, 50.0, 0.0));
        // An explicit width wins over a scale on the same axis.
        s.execute("shape.sizePosition", &json!({"w": 80.0, "scaleW": 50.0})).unwrap();
        assert_eq!(geom(&mut s, id).2, 80.0);
    }

    #[test]
    fn partial_fields_leave_the_rest_and_a_repeat_does_not_undo() {
        let (mut s, id) = deck();
        let n = undos(&s);
        s.execute("shape.sizePosition", &json!({"y": 12.0, "rot": -90.0})).unwrap();
        assert_eq!(geom(&mut s, id), (40.0, 12.0, 200.0, 100.0, 270.0));
        assert_eq!(undos(&s), n + 1);
        let n = undos(&s);
        s.execute("shape.sizePosition", &json!({"x": 40.0, "y": 12.0, "w": 200.0, "h": 100.0, "rot": 270.0})).unwrap();
        assert_eq!(undos(&s), n, "identical geometry is not an undo step");
    }

    #[test]
    fn no_fields_opens_the_dialog_without_editing() {
        let (mut s, id) = deck();
        let n = undos(&s);
        s.execute("shape.sizePosition", &json!({})).unwrap();
        assert_eq!(geom(&mut s, id), (40.0, 50.0, 200.0, 100.0, 0.0));
        assert_eq!(undos(&s), n);
        let req = s.ui_requests.last().unwrap();
        match req {
            crate::UiRequest::Dialog { id, params } => {
                assert_eq!(id, "sizePosition");
                assert_eq!(params["w"], 200.0);
                assert_eq!(params["h"], 100.0);
                assert_eq!(params["x"], 40.0);
                assert_eq!(params["rot"], 0.0);
                assert_eq!(params["scaleW"], 100.0);
                assert_eq!(params["lockAspect"], false);
            }
            other => panic!("expected a dialog, got {other:?}"),
        }
    }

    #[test]
    fn bad_fields_do_not_edit() {
        let (mut s, id) = deck();
        let n = undos(&s);
        for p in [
            json!({"w": "wide"}),
            json!({"h": -5.0}),
            json!({"scaleW": 0.0}),
            json!({"x": 1e9}),
            json!({"rot": "spin"}),
            json!({"lockAspect": "yes"}),
        ] {
            let err = s.execute("shape.sizePosition", &p).unwrap_err();
            assert!(err.to_string().contains("sizePosition"), "{err}");
            assert_eq!(geom(&mut s, id), (40.0, 50.0, 200.0, 100.0, 0.0));
            assert_eq!(undos(&s), n);
        }
    }

    #[test]
    fn scale_is_per_shape_and_nothing_selected_is_disabled() {
        let (mut s, a) = deck();
        let b = s.execute("shape.insert", &json!({"preset": "ellipse", "rect": [10.0, 10.0, 40.0, 80.0]})).unwrap()["id"].as_u64().unwrap() as u32;
        s.execute("shape.sizePosition", &json!({"ids": [a, b], "scaleW": 200.0})).unwrap();
        assert_eq!(geom(&mut s, a).2, 400.0);
        assert_eq!(geom(&mut s, a).3, 100.0);
        assert_eq!(geom(&mut s, b).2, 80.0);
        assert_eq!(geom(&mut s, b).3, 80.0);
        assert_eq!(s.doc().unwrap().history.undo.last().unwrap().label, "Size and Position…");
        s.execute("edit.select", &json!({"ids": []})).unwrap();
        let err = s.execute("shape.sizePosition", &json!({})).unwrap_err();
        assert!(err.to_string().contains("nothing selected"), "{err}");
    }
}
