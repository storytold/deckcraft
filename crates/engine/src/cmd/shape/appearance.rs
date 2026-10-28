use super::{
    ColorRef, ColorTransform, Dash, Effects, Fill, Geom, Glow, Gradient, GradientShape, GradientStop, Line, LineEnd, PictureFill, PictureMode,
    Reflection, Result, Rgba, SchemeSlot, Session, Shadow, ShapeStyle, Value, apply_fill, bad, bool_or, color_param, color_value, edit_geometry,
    edit_shapes, f64_param, inherited_fills, json, requested_fill, str_param, usize_param,
};

pub(super) fn adjust(s: &mut Session, p: &Value) -> Result<Value> {
    let i = usize_param(p, "index").unwrap_or(0).min(16);
    let v = f64_param(p, "value").ok_or_else(|| bad("shape.adjust", "missing `value`"))?;
    edit_geometry(s, p, "shape.adjust", |sh| {
        if let Geom::Preset { name, adj } = &mut sh.geom {
            let defaults = deckcraft_geom::preset::info(name).map(|x| x.defaults).unwrap_or(&[]);
            while adj.len() <= i {
                let k = adj.len();
                adj.push(defaults.get(k).copied().unwrap_or(0.0));
            }
            if let Some(slot) = adj.get_mut(i) {
                *slot = v;
            }
        }
        Ok(())
    })
}

/// Rectangles and rounded rectangles become custom geometry whose corners are smoothed
/// ([`deckcraft_geom::squircle`]); the radius is the rounded rectangle's own unless `radius` says.
pub(super) fn smooth_corners(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "shape.smoothCorners";
    let smoothing =
        f64_param(p, "smoothing").filter(|v| v.is_finite()).ok_or_else(|| bad(CMD, "missing `smoothing` (0–100)"))?.clamp(0.0, 100.0) / 100.0;
    let radius = f64_param(p, "radius").filter(|v| v.is_finite()).map(|v| v.max(0.0));
    edit_geometry(s, p, CMD, |sh| {
        let Some(x) = sh.xfrm else { return Err(bad(CMD, "the shape has no size of its own")) };
        let ss = x.w.min(x.h).max(0.0);
        let own = match &sh.geom {
            Geom::Preset { name, adj } if name == "roundRect" => Some(ss * adj.first().copied().unwrap_or(16667.0).clamp(0.0, 50000.0) / 100000.0),
            Geom::Preset { name, .. } if name == "rect" => Some(ss * 16667.0 / 100000.0),
            _ => None,
        };
        // A custom outline qualifies only when it is a squircle this command drew with `radius`
        // (re-smoothing as the slider moves), never an arbitrary freeform.
        let custom = match (&sh.geom, radius) {
            (Geom::Custom { paths }, Some(r)) => match paths.as_slice() {
                [only] => (0..=100).any(|k| deckcraft_geom::squircle::path_d(only.w, only.h, r, f64::from(k) / 100.0) == only.d),
                _ => false,
            },
            _ => false,
        };
        let r = match (radius, own) {
            (Some(r), _) if own.is_some() || custom => r,
            (None, Some(r)) => r,
            _ => return Err(bad(CMD, "only rectangles, rounded rectangles and shapes already smoothed with this `radius` can be smoothed")),
        };
        sh.geom = Geom::Custom {
            paths: vec![deckcraft_model::CustomPath {
                w: x.w,
                h: x.h,
                d: deckcraft_geom::squircle::path_d(x.w, x.h, r, smoothing),
                ..Default::default()
            }],
        };
        Ok(())
    })
}

pub(super) fn change(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "preset").ok_or_else(|| bad("shape.change", "missing `preset`"))?;
    let info = deckcraft_geom::preset::info(name).ok_or_else(|| bad("shape.change", format!("unknown preset `{name}`")))?;
    edit_geometry(s, p, "shape.change", |sh| {
        sh.geom = Geom::preset(info.name);
        Ok(())
    })
}

/// `{stops: [[pos 0–1, color], …], kind?: linear|radial|rectangular|path, angle?: deg (linear),
/// focus?: [l, t, r, b] fractions (radial/rectangular/path: where the first stop sits),
/// rotateWithShape?}`.
pub(crate) fn gradient_param(v: &Value) -> Option<Gradient> {
    let stops: Vec<GradientStop> = v
        .get("stops")?
        .as_array()?
        .iter()
        .filter_map(|s| {
            let a = s.as_array()?;
            Some(GradientStop { pos: a.first()?.as_f64()?.clamp(0.0, 1.0), color: color_value(a.get(1)?)? })
        })
        .collect();
    if stops.len() < 2 {
        return None;
    }
    let focus = v
        .get("focus")
        .and_then(Value::as_array)
        .and_then(|a| {
            let f = |i: usize| a.get(i).and_then(Value::as_f64).filter(|x| x.is_finite()).map(|x| x.clamp(0.0, 1.0));
            Some([f(0)?, f(1)?, f(2)?, f(3)?])
        })
        .unwrap_or([0.5, 0.5, 0.5, 0.5]);
    let mut stops = stops;
    stops.sort_by(|a, b| a.pos.total_cmp(&b.pos));
    let path = |p: &str| GradientShape::Path { path: p.into(), focus };
    let shape = match v.get("kind").and_then(Value::as_str) {
        Some("radial" | "circle") => path("circle"),
        Some("rectangular" | "rect") => path("rect"),
        Some("path" | "shape") => path("shape"),
        _ => GradientShape::Linear {
            angle: v.get("angle").and_then(Value::as_f64).filter(|a| a.is_finite()).unwrap_or(90.0).rem_euclid(360.0),
            scaled: false,
        },
    };
    Some(Gradient { tile: [0.0; 4], stops, shape, rotate_with_shape: v.get("rotateWithShape").and_then(Value::as_bool).unwrap_or(true) })
}

/// Fill and outline apply to a group's visible members as well as its container. Other edits
/// (notably geometry) still target only the selected shape. The whole traversal stays in one edit.
fn edit_group_appearance(s: &mut Session, p: &Value, cmd: &str, f: impl Fn(&mut deckcraft_model::Shape) -> Result<()>) -> Result<Value> {
    fn apply(sh: &mut deckcraft_model::Shape, depth: usize, f: &dyn Fn(&mut deckcraft_model::Shape) -> Result<()>) -> Result<()> {
        if depth > 64 {
            return Ok(());
        }
        f(sh)?;
        if let Some(children) = sh.children_mut() {
            for child in children {
                apply(child, depth + 1, f)?;
            }
        }
        Ok(())
    }
    edit_shapes(s, p, cmd, |sh| apply(sh, 0, &f))
}

pub(super) fn fill(s: &mut Session, p: &Value) -> Result<Value> {
    let new_fill = requested_fill(s, p)?;
    let transparency = f64_param(p, "transparency");
    let pic_props = p.get("pictureProps").filter(|v| v.is_object());
    if new_fill.is_none() && transparency.is_none() && pic_props.is_none() {
        return Err(bad("shape.fill", "give `color`, `none`, `gradient`, `pattern`, `picture`, `pictureProps`, `transparency` or `reset`"));
    }
    // Transparency on an inherited (style) fill needs the effective colour.
    let inherited = inherited_fills(s, p)?;
    edit_group_appearance(s, p, "shape.fill", |sh| apply_fill(sh, new_fill.as_ref(), pic_props, transparency, &inherited))
}

/// Applies Format Shape ▸ Picture or texture fill options to a picture fill.
pub(super) fn picture_props(pf: &mut PictureFill, pp: &Value) {
    let f = |k: &str| pp.get(k).and_then(Value::as_f64).filter(|v| v.is_finite());
    if let Some(up) = pp.get("rotateWithShape").and_then(Value::as_bool) {
        pf.upright = !up;
    }
    match pp.get("tile").and_then(Value::as_bool) {
        Some(true) if !matches!(pf.mode, PictureMode::Tile { .. }) => {
            pf.mode = PictureMode::Tile { tx: 0.0, ty: 0.0, sx: 1.0, sy: 1.0, flip: "none".into(), align: "tl".into() };
        }
        Some(false) if !matches!(pf.mode, PictureMode::Stretch { .. }) => pf.mode = PictureMode::default(),
        _ => {}
    }
    match &mut pf.mode {
        PictureMode::Tile { tx, ty, sx, sy, flip, align } => {
            if let Some(v) = f("offsetX") {
                *tx = v.clamp(-4000.0, 4000.0);
            }
            if let Some(v) = f("offsetY") {
                *ty = v.clamp(-4000.0, 4000.0);
            }
            if let Some(v) = f("scaleX") {
                *sx = v.clamp(0.01, 10.0);
            }
            if let Some(v) = f("scaleY") {
                *sy = v.clamp(0.01, 10.0);
            }
            if let Some(m) = pp.get("mirror").and_then(Value::as_str).filter(|m| matches!(*m, "none" | "x" | "y" | "xy")) {
                *flip = m.to_string();
            }
            if let Some(a) =
                pp.get("align").and_then(Value::as_str).filter(|a| matches!(*a, "tl" | "t" | "tr" | "l" | "ctr" | "r" | "bl" | "b" | "br"))
            {
                *align = a.to_string();
            }
        }
        PictureMode::Stretch { fill_rect } => {
            if let Some(arr) = pp.get("stretch").and_then(Value::as_array) {
                for (slot, v) in fill_rect.iter_mut().zip(arr) {
                    if let Some(v) = v.as_f64().filter(|v| v.is_finite()) {
                        *slot = v.clamp(-10.0, 0.99);
                    }
                }
            }
        }
    }
}

fn line_end(v: &str) -> Option<LineEnd> {
    if v == "none" { None } else { Some(LineEnd { kind: v.to_string(), w: "med".into(), len: "med".into() }) }
}

pub(super) fn line(s: &mut Session, p: &Value) -> Result<Value> {
    let reset = bool_or(p, "reset", false);
    let none = bool_or(p, "none", false);
    let color = color_param(p, "color");
    let width = f64_param(p, "width").map(|w| w.clamp(0.0, 1584.0));
    let dash = str_param(p, "dash").map(|d| Dash::Preset { name: d.to_string() });
    let head = str_param(p, "head").map(line_end);
    let tail = str_param(p, "tail").map(line_end);
    let cap = str_param(p, "cap").map(|c| match c {
        "round" => deckcraft_model::style::LineCap::Round,
        "square" => deckcraft_model::style::LineCap::Square,
        _ => deckcraft_model::style::LineCap::Flat,
    });
    let compound = str_param(p, "compound").map(|c| match c {
        "double" => deckcraft_model::style::Compound::Double,
        "thickThin" => deckcraft_model::style::Compound::ThickThin,
        "thinThick" => deckcraft_model::style::Compound::ThinThick,
        "triple" => deckcraft_model::style::Compound::Triple,
        _ => deckcraft_model::style::Compound::Single,
    });
    let end_size = |k: &str| str_param(p, k).filter(|v| matches!(*v, "sm" | "med" | "lg")).map(str::to_string);
    let (head_size, tail_size) = (end_size("headSize"), end_size("tailSize"));
    let join = str_param(p, "join").map(|c| match c {
        "bevel" => deckcraft_model::style::LineJoin::Bevel,
        "miter" => deckcraft_model::style::LineJoin::Miter,
        _ => deckcraft_model::style::LineJoin::Round,
    });
    edit_group_appearance(s, p, "shape.line", |sh| {
        if reset {
            sh.line = None;
            return Ok(());
        }
        let l = sh.line.get_or_insert_with(Line::default);
        if none {
            l.fill = Some(Fill::None);
        }
        if let Some(c) = &color {
            l.fill = Some(Fill::solid(c.clone()));
        }
        if width.is_some() {
            l.width = width;
            if l.fill.is_none() || matches!(l.fill, Some(Fill::None)) && !none {
                l.fill = Some(Fill::solid(ColorRef::scheme(SchemeSlot::Tx1)));
            }
        }
        if dash.is_some() {
            l.dash.clone_from(&dash);
        }
        let keep_size = |old: &Option<LineEnd>, new: &Option<LineEnd>| {
            new.clone().map(|mut n| {
                if let Some(o) = old {
                    n.w.clone_from(&o.w);
                    n.len.clone_from(&o.len);
                }
                n
            })
        };
        if let Some(h) = &head {
            l.head = keep_size(&l.head, h);
        }
        if let Some(t) = &tail {
            l.tail = keep_size(&l.tail, t);
        }
        if cap.is_some() {
            l.cap = cap;
        }
        if join.is_some() {
            l.join = join;
        }
        if compound.is_some() {
            l.compound = compound;
        }
        // Arrowhead size (width and length together), kept when the kind changes.
        for (end, size) in [(&mut l.head, &head_size), (&mut l.tail, &tail_size)] {
            if let (Some(e), Some(sz)) = (end.as_mut(), size) {
                e.w.clone_from(sz);
                e.len.clone_from(sz);
            }
        }
        Ok(())
    })
}

fn shadow_preset(v: &Value) -> Option<Option<Shadow>> {
    let black = |a: i32| ColorRef::rgb(Rgba::BLACK).with(ColorTransform::Alpha(a));
    let base = |inner: bool| Shadow {
        color: black(40000),
        blur: 4.0,
        dist: 3.0,
        dir: 45.0,
        inner,
        sx: 1.0,
        sy: 1.0,
        kx: 0.0,
        ky: 0.0,
        align: "tl".into(),
        rotate_with_shape: false,
    };
    match v {
        Value::String(s) => Some(match s.as_str() {
            "none" => None,
            "inner" => Some(base(true)),
            "perspective" => Some(Shadow { sy: 0.23, kx: -1200000.0 / 60000.0, dir: 90.0, dist: 0.0, blur: 6.0, align: "bl".into(), ..base(false) }),
            "bottom" => Some(Shadow { dir: 90.0, ..base(false) }),
            "center" => Some(Shadow { dist: 0.0, blur: 8.0, ..base(false) }),
            _ => Some(base(false)),
        }),
        Value::Object(o) => Some(Some(Shadow {
            color: o.get("color").and_then(color_value).unwrap_or(black(40000)),
            blur: o.get("blur").and_then(Value::as_f64).unwrap_or(4.0).clamp(0.0, 200.0),
            dist: o.get("dist").and_then(Value::as_f64).unwrap_or(3.0).clamp(0.0, 200.0),
            dir: o.get("dir").and_then(Value::as_f64).unwrap_or(45.0),
            inner: o.get("inner").and_then(Value::as_bool).unwrap_or(false),
            ..base(false)
        })),
        _ => None,
    }
}

pub(super) fn effects(s: &mut Session, p: &Value) -> Result<Value> {
    let reset = bool_or(p, "reset", false);
    let shadow = p.get("shadow").and_then(shadow_preset);
    let glow: Option<Option<Glow>> = p.get("glow").map(|g| match g {
        Value::Object(o) => Some(Glow {
            color: o.get("color").and_then(color_value).unwrap_or(ColorRef::scheme(SchemeSlot::Accent1).with(ColorTransform::Alpha(40000))),
            radius: o.get("radius").and_then(Value::as_f64).unwrap_or(8.0).clamp(0.0, 150.0),
        }),
        Value::Number(n) => Some(Glow {
            color: ColorRef::scheme(SchemeSlot::Accent1).with(ColorTransform::Alpha(40000)),
            radius: n.as_f64().unwrap_or(8.0).clamp(0.0, 150.0),
        }),
        _ => None,
    });
    let soft = p.get("softEdges").map(|v| v.as_f64().filter(|x| *x > 0.0).map(|x| x.min(100.0)));
    let refl = str_param(p, "reflection").map(|r| match r {
        "none" => None,
        "half" => Some(Reflection { blur: 0.5, start_alpha: 0.5, end_alpha: 0.0, end_pos: 0.55, dist: 4.0 }),
        "full" => Some(Reflection { blur: 0.5, start_alpha: 0.5, end_alpha: 0.0, end_pos: 0.9, dist: 4.0 }),
        _ => Some(Reflection { blur: 0.5, start_alpha: 0.5, end_alpha: 0.0, end_pos: 0.35, dist: 0.0 }),
    });
    let three_d = p.get("threeD").filter(|v| v.is_object());
    let bevel = str_param(p, "bevel").map(|b| if b == "none" { None } else { Some(b.to_string()) });
    let shadow_props = p.get("shadowProps").filter(|v| v.is_object()).cloned();
    let glow_props = p.get("glowProps").filter(|v| v.is_object()).cloned();
    let refl_props = p.get("reflectionProps").filter(|v| v.is_object()).cloned();
    edit_shapes(s, p, "shape.effects", |sh| {
        if reset {
            sh.effects = None;
            return Ok(());
        }
        let e = sh.effects.get_or_insert_with(Effects::default);
        if let Some(sw) = &shadow {
            match sw {
                Some(x) if x.inner => {
                    e.inner_shadow = Some(x.clone());
                    e.outer_shadow = None;
                }
                Some(x) => {
                    e.outer_shadow = Some(x.clone());
                    e.inner_shadow = None;
                }
                None => {
                    e.outer_shadow = None;
                    e.inner_shadow = None;
                }
            }
        }
        if let Some(g) = &glow {
            e.glow.clone_from(g);
        }
        if let Some(sf) = soft {
            e.soft_edge = sf;
        }
        if let Some(r) = &refl {
            e.reflection.clone_from(r);
        }
        if bevel.is_some() || three_d.is_some() {
            let mut s3 = e.raw3d.as_deref().map(crate::cmd::three_d::Shape3d::parse).unwrap_or_default();
            if let Some(b) = &bevel {
                s3.apply(&json!({"bevelTop": b.as_deref().unwrap_or("none")}));
            }
            if let Some(t) = three_d {
                if t.get("reset").and_then(Value::as_bool) == Some(true) {
                    s3 = crate::cmd::three_d::Shape3d::default();
                }
                s3.apply(t);
            }
            crate::cmd::three_d::store(e, &s3);
        }
        if let Some(v) = &shadow_props {
            let slot = if e.inner_shadow.is_some() { &mut e.inner_shadow } else { &mut e.outer_shadow };
            let mut x = slot.take().or_else(|| shadow_preset(&json!("outer")).flatten()).ok_or_else(|| bad("shape.effects", "no shadow preset"))?;
            if let Some(c) = v.get("color").and_then(color_value) {
                let a = x.color.alpha();
                x.color = with_alpha(c, a);
            }
            if let Some(t) = num(v, "transparency") {
                x.color = with_alpha(x.color.clone(), 1.0 - t.clamp(0.0, 1.0));
            }
            if let Some(b) = num(v, "blur") {
                x.blur = b.clamp(0.0, 100.0);
            }
            if let Some(d) = num(v, "dist") {
                x.dist = d.clamp(0.0, 200.0);
            }
            if let Some(d) = num(v, "dir") {
                x.dir = d.rem_euclid(360.0);
            }
            *slot = Some(x);
        }
        if let Some(v) = &glow_props {
            let mut g =
                e.glow.take().unwrap_or(Glow { color: ColorRef::scheme(SchemeSlot::Accent1).with(ColorTransform::Alpha(40000)), radius: 8.0 });
            if let Some(c) = v.get("color").and_then(color_value) {
                let a = g.color.alpha();
                g.color = with_alpha(c, a);
            }
            if let Some(t) = num(v, "transparency") {
                g.color = with_alpha(g.color.clone(), 1.0 - t.clamp(0.0, 1.0));
            }
            if let Some(r) = num(v, "radius") {
                g.radius = r.clamp(0.0, 150.0);
            }
            e.glow = (g.radius > 0.0).then_some(g);
        }
        if let Some(v) = &refl_props {
            let mut r = e.reflection.take().unwrap_or(Reflection { blur: 0.5, start_alpha: 0.5, end_alpha: 0.0, end_pos: 0.35, dist: 0.0 });
            if let Some(t) = num(v, "transparency") {
                r.start_alpha = 1.0 - t.clamp(0.0, 1.0);
            }
            if let Some(sz) = num(v, "size") {
                r.end_pos = sz.clamp(0.0, 1.0);
            }
            if let Some(d) = num(v, "dist") {
                r.dist = d.clamp(0.0, 100.0);
            }
            if let Some(b) = num(v, "blur") {
                r.blur = b.clamp(0.0, 100.0);
            }
            e.reflection = Some(r);
        }
        Ok(())
    })
}

/// A finite number at `key` of an effect's properties.
fn num(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(Value::as_f64).filter(|x| x.is_finite())
}

/// `c` with its alpha set to `a` (0–1); fully opaque drops the transform.
fn with_alpha(mut c: ColorRef, a: f64) -> ColorRef {
    c.mods.retain(|m| !matches!(m, ColorTransform::Alpha(_)));
    if a < 1.0 {
        c.mods.push(ColorTransform::Alpha((a.clamp(0.0, 1.0) * 100_000.0).round() as i32));
    }
    c
}

/// Theme quick styles: 7 rows (outline only, solid fill variants, intense…) × 6 accents.
pub fn quick_style_for(index: usize) -> (ShapeStyle, Option<Fill>, Option<Line>) {
    let slots = [
        SchemeSlot::Dk1,
        SchemeSlot::Accent1,
        SchemeSlot::Accent2,
        SchemeSlot::Accent3,
        SchemeSlot::Accent4,
        SchemeSlot::Accent5,
        SchemeSlot::Accent6,
    ];
    let slot = slots.get(index % 7).copied().unwrap_or(SchemeSlot::Accent1);
    let row = index / 7;
    let mut st = ShapeStyle::accent(slot);
    let c = ColorRef::scheme(slot);
    match row {
        0 => {
            // Colored outline, light fill.
            st.fill_ref = (1, ColorRef::scheme(SchemeSlot::Lt1));
            st.line_ref = (2, c);
            st.font_ref.1 = Some(ColorRef::scheme(SchemeSlot::Dk1));
            (st, None, None)
        }
        1 => (st, None, None),
        2 => {
            st.fill_ref = (1, c.clone().with(ColorTransform::LumMod(20000)).with(ColorTransform::LumOff(80000)));
            st.line_ref = (0, c.clone());
            st.font_ref.1 = Some(c.with(ColorTransform::LumMod(50000)));
            (st, None, Some(Line::none()))
        }
        3 => {
            st.fill_ref = (2, c);
            (st, None, Some(Line::none()))
        }
        4 => {
            st.fill_ref = (3, c.clone());
            st.effect_ref = (3, c);
            (st, None, Some(Line::none()))
        }
        _ => {
            st.fill_ref = (1, c.clone().with(ColorTransform::LumMod(75000)));
            st.line_ref = (1, ColorRef::scheme(SchemeSlot::Lt1));
            (st, None, None)
        }
    }
}

pub(super) fn quick_style(s: &mut Session, p: &Value) -> Result<Value> {
    let i = usize_param(p, "index").unwrap_or(1).min(41);
    let (st, f, l) = quick_style_for(i);
    edit_shapes(s, p, "shape.quickStyle", |sh| {
        sh.style = Some(st.clone());
        sh.fill.clone_from(&f);
        sh.line.clone_from(&l);
        sh.effects = None;
        if let Some(t) = sh.text.as_mut() {
            deckcraft_model::edit::format_all(t, &|r| r.fill = None);
        }
        Ok(())
    })
}
