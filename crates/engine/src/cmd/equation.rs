//! Equations: an inline `Math` run holds the OMML (canonical, with `xmlns:m`) and its linear
//! text. The commands here create, replace and read those runs; the dialog and agents use them.

use deckcraft_math::Math;
use deckcraft_model::ShapeId;
use deckcraft_model::text::{Run, RunKind, RunProps};
use serde_json::{Value, json};

use super::*;
use crate::{EngineError, Result, Session};

/// Largest OMML document accepted from a parameter.
const MAX_OMML: usize = 1 << 20;
/// Largest linear text accepted from a parameter.
const MAX_LINEAR: usize = 1 << 15;
/// Largest `tree` (as JSON text) returned inline; a bigger one is left out (`treeOmitted`).
const MAX_TREE_JSON: usize = 1 << 20;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "insert.equation",
            "Equation",
            ["Insert", "Symbols"],
            None,
            "{linear? | omml?, display?: bool, rect?: [x,y,w,h]} → {id, omml, linear}: a new text box holding the equation",
            has_slide,
            insert_equation
        ),
        cmd!(
            "equation.update",
            "Edit Equation",
            ["Equation"],
            None,
            "{linear? | omml?, display?: bool, id?, paragraph?, run?} replaces an equation, keeping its formatting",
            has_slide,
            update
        ),
        cmd!(
            query "equation.get",
            "Get Equation",
            [],
            None,
            "{id?, paragraph?, run?} → {id, paragraph, run, omml, linear, display, tree}",
            has_slide,
            get
        ),
        cmd!(query "equation.templates", "Equation Templates", [], None, "{} → [{id, label, group, linear, omml}]", always, templates),
        cmd!(
            query "equation.fromLinear",
            "Linear Text to Equation",
            [],
            None,
            "{linear} → {omml, linear, display, tree}",
            always,
            from_linear
        ),
    ]
}

/// An optional parameter: absent or null is `None`; present but of the wrong type is an error
/// (a typo'd id must never fall back to another target).
fn opt<'a>(p: &'a Value, key: &str) -> Option<&'a Value> {
    p.get(key).filter(|v| !v.is_null())
}

fn opt_u32(cmd: &str, p: &Value, key: &str) -> Result<Option<u32>> {
    match opt(p, key) {
        None => Ok(None),
        Some(v) => {
            v.as_u64().and_then(|n| u32::try_from(n).ok()).map(Some).ok_or_else(|| bad(cmd, format!("`{key}` must be a non-negative integer")))
        }
    }
}

fn opt_bool(cmd: &str, p: &Value, key: &str) -> Result<Option<bool>> {
    match opt(p, key) {
        None => Ok(None),
        Some(v) => v.as_bool().map(Some).ok_or_else(|| bad(cmd, format!("`{key}` must be true or false"))),
    }
}

/// Largest coordinate / size accepted for `rect`, in points.
const MAX_RECT: f64 = 20_000.0;

fn opt_rect(cmd: &str, p: &Value) -> Result<Option<deckcraft_geom::Xfrm>> {
    let Some(v) = opt(p, "rect") else { return Ok(None) };
    let err = || bad(cmd, "`rect` must be [x, y, w, h]: four finite numbers in points, w and h above 0");
    let a = v.as_array().filter(|a| a.len() == 4).ok_or_else(err)?;
    let mut n = [0.0f64; 4];
    for (slot, x) in n.iter_mut().zip(a) {
        *slot = x.as_f64().filter(|f| f.is_finite() && f.abs() <= MAX_RECT).ok_or_else(err)?;
    }
    if n[2] < 1.0 || n[3] < 1.0 {
        return Err(err());
    }
    Ok(Some(deckcraft_geom::Xfrm::new(n[0], n[1], n[2], n[3])))
}

/// True when the text's root element is `m:oMath` / `m:oMathPara` (any prefix).
fn root_is_math(t: &str) -> bool {
    let mut rest = t.trim_start();
    // Skip an XML declaration or comments before the root.
    while let Some(r) = rest.strip_prefix("<?").or_else(|| rest.strip_prefix("<!--")) {
        let end = if rest.starts_with("<?") { "?>" } else { "-->" };
        match r.find(end) {
            Some(i) => rest = r[i + end.len()..].trim_start(),
            None => return false,
        }
    }
    let Some(r) = rest.strip_prefix('<') else { return false };
    let name: String = r.chars().take_while(|c| !c.is_whitespace() && *c != '>' && *c != '/').collect();
    let local = name.rsplit(':').next().unwrap_or("");
    matches!(local, "oMath" | "oMathPara")
}

/// Parse the `omml` / `linear` parameters into a math tree; the OMML reader never fails, so
/// text that is not an equation is rejected here. `display` is applied only when given.
fn math_param(cmd: &str, p: &Value) -> Result<Math> {
    let display = opt_bool(cmd, p, "display")?;
    let mut m = match (opt(p, "omml"), opt(p, "linear")) {
        (Some(x), _) => {
            let x = x.as_str().ok_or_else(|| bad(cmd, "`omml` must be a string"))?;
            if x.len() > MAX_OMML {
                return Err(bad(cmd, "`omml` is too large"));
            }
            if !root_is_math(x) {
                return Err(bad(cmd, "`omml` is not an Office Math (m:oMath) element"));
            }
            deckcraft_math::from_omml(x.trim())
        }
        (None, Some(x)) => {
            let x = x.as_str().ok_or_else(|| bad(cmd, "`linear` must be a string"))?;
            if x.len() > MAX_LINEAR {
                return Err(bad(cmd, "`linear` is too large"));
            }
            deckcraft_math::from_linear(x)
        }
        (None, None) => return Err(bad(cmd, "missing `linear` or `omml`")),
    };
    if m.is_empty() || deckcraft_math::to_linear(&m).trim().is_empty() {
        return Err(bad(cmd, "the equation is empty"));
    }
    if let Some(d) = display {
        m.para = d;
    }
    Ok(m)
}

/// The run payload for an equation: canonical OMML and the linear text of that OMML (so the run
/// text always matches what the stored equation renders; never empty, so the run keeps a
/// character width in the text).
fn payload(m: &Math) -> (String, String) {
    let omml = deckcraft_math::to_omml(m);
    let linear = deckcraft_math::run_text(&deckcraft_math::from_omml(&omml));
    (omml, linear)
}

/// Default box for a new equation: the slide centre, cascaded down-right so repeated inserts
/// do not stack exactly on top of each other.
fn cascade_rect(s: &Session) -> deckcraft_geom::Xfrm {
    let mut r = super::insert::default_rect(s, 300.0, 50.0);
    let existing: Vec<(f64, f64)> = s
        .doc()
        .ok()
        .and_then(|d| d.current_slide())
        .map(|sl| sl.shapes.iter().filter_map(|sh| sh.xfrm.as_ref()).map(|x| (x.x, x.y)).collect())
        .unwrap_or_default();
    for _ in 0..64 {
        if !existing.iter().any(|(x, y)| (x - r.x).abs() < 1.0 && (y - r.y).abs() < 1.0) {
            break;
        }
        r.x += 24.0;
        r.y += 24.0;
    }
    r
}

fn insert_equation(s: &mut Session, p: &Value) -> Result<Value> {
    let m = math_param("insert.equation", p)?;
    let rect = match opt_rect("insert.equation", p)? {
        Some(r) => r,
        None => cascade_rect(s),
    };
    let (omml, linear) = payload(&m);
    let mut shape = super::insert::new_text_box(rect, "");
    shape.name = String::new();
    if let Some(tb) = shape.text.as_mut()
        && let Some(para) = tb.paragraphs.first_mut()
    {
        let props = RunProps { size: Some(28.0), font: Some("Liberation Serif".into()), ..Default::default() };
        para.end_props = props.clone();
        para.runs = vec![Run { text: linear.clone(), props, kind: RunKind::Math { omml: omml.clone() } }];
    }
    let id = super::insert::add_shape(s, shape, true)?;
    super::text::fit_text_box(s, id)?;
    Ok(json!({"id": id, "omml": omml, "linear": linear}))
}

/// Where an equation lives: shape, paragraph, run.
fn locate(s: &Session, cmd: &str, p: &Value) -> Result<(ShapeId, usize, usize)> {
    let st = s.doc()?;
    let given = opt_u32(cmd, p, "id")?.map(ShapeId);
    let (para, run) = (opt_u32(cmd, p, "paragraph")?, opt_u32(cmd, p, "run")?);
    if para.is_some() != run.is_some() {
        return Err(bad(cmd, "pass `paragraph` and `run` together"));
    }
    let id = given
        .or_else(|| st.selection.text.as_ref().map(|t| t.shape))
        .or_else(|| st.selection.shapes.first().copied())
        .ok_or_else(|| bad(cmd, "select an equation or pass `id`"))?;
    let shape = st.shape(id).ok_or_else(|| EngineError::Other(format!("no shape {id}")))?;
    let body = shape.text.as_ref().ok_or_else(|| bad(cmd, "the shape has no text"))?;
    let is_math = |pi: usize, ri: usize| body.paragraphs.get(pi).and_then(|q| q.runs.get(ri)).is_some_and(|r| matches!(r.kind, RunKind::Math { .. }));
    if let (Some(pi), Some(ri)) = (para, run) {
        let (pi, ri) = (pi as usize, ri as usize);
        return if is_math(pi, ri) { Ok((id, pi, ri)) } else { Err(bad(cmd, "that run is not an equation")) };
    }
    // The equation the insertion point touches, else the first one in the shape.
    if let Some(t) = st.selection.text.as_ref().filter(|t| t.shape == id) {
        let (a, b) = t.ordered();
        if let Some(q) = body.paragraphs.get(a.0) {
            let mut at = 0;
            for (ri, r) in q.runs.iter().enumerate() {
                let end = at + r.char_len();
                if matches!(r.kind, RunKind::Math { .. }) && b.0 == a.0 && at <= b.1 && a.1 <= end {
                    return Ok((id, a.0, ri));
                }
                at = end;
            }
        }
    }
    for (pi, q) in body.paragraphs.iter().enumerate() {
        if let Some(ri) = q.runs.iter().position(|r| matches!(r.kind, RunKind::Math { .. })) {
            return Ok((id, pi, ri));
        }
    }
    Err(bad(cmd, "no equation found"))
}

fn update(s: &mut Session, p: &Value) -> Result<Value> {
    let mut m = math_param("equation.update", p)?;
    let (id, pi, ri) = locate(s, "equation.update", p)?;
    // New linear text keeps the equation's inline / display setting unless `display` says otherwise.
    if p.get("omml").is_none_or(Value::is_null) && opt_bool("equation.update", p, "display")?.is_none() {
        let old = s
            .doc()?
            .shape(id)
            .and_then(|sh| sh.text.as_ref())
            .and_then(|tb| tb.paragraphs.get(pi))
            .and_then(|q| q.runs.get(ri))
            .map(|r| r.kind.clone());
        if let Some(RunKind::Math { omml }) = old {
            m.para = deckcraft_math::from_omml(&omml).para;
        }
    }
    let (omml, linear) = payload(&m);
    let (o, l) = (omml.clone(), linear.clone());
    s.edit(|doc, sel| {
        let shapes = crate::shapes_mut(doc, sel).ok_or_else(|| bad("equation.update", "no slide"))?;
        let run = deckcraft_model::find_shape_mut(shapes, id)
            .and_then(|sh| sh.text.as_mut())
            .and_then(|tb| tb.paragraphs.get_mut(pi))
            .and_then(|q| q.runs.get_mut(ri))
            .ok_or_else(|| bad("equation.update", "the equation is gone"))?;
        run.text = l;
        run.kind = RunKind::Math { omml: o };
        Ok(())
    })?;
    super::text::fit_text_box(s, id)?;
    Ok(json!({"id": id, "paragraph": pi, "run": ri, "omml": omml, "linear": linear}))
}

fn describe(m: &Math) -> Value {
    // The tree is far larger than its source; a huge one is withheld so a client never gets
    // a response of hundreds of megabytes.
    let tree = serde_json::to_string(m).ok().filter(|t| t.len() <= MAX_TREE_JSON).and_then(|t| serde_json::from_str::<Value>(&t).ok());
    let omitted = tree.is_none();
    let mut v = json!({
        "omml": deckcraft_math::to_omml(m),
        "linear": deckcraft_math::to_linear(m),
        "display": m.para,
        "tree": tree.unwrap_or(Value::Null),
    });
    if let (true, Some(o)) = (omitted, v.as_object_mut()) {
        o.insert("treeOmitted".into(), json!(true));
    }
    v
}

fn get(s: &mut Session, p: &Value) -> Result<Value> {
    let (id, pi, ri) = locate(s, "equation.get", p)?;
    let run = s
        .doc()?
        .shape(id)
        .and_then(|sh| sh.text.as_ref())
        .and_then(|tb| tb.paragraphs.get(pi))
        .and_then(|q| q.runs.get(ri))
        .cloned()
        .ok_or_else(|| bad("equation.get", "the equation is gone"))?;
    let RunKind::Math { omml } = &run.kind else { return Err(bad("equation.get", "that run is not an equation")) };
    let mut v = describe(&deckcraft_math::from_omml(omml));
    if let Some(o) = v.as_object_mut() {
        o.insert("id".into(), json!(id));
        o.insert("paragraph".into(), json!(pi));
        o.insert("run".into(), json!(ri));
    }
    Ok(v)
}

fn templates(_s: &mut Session, _p: &Value) -> Result<Value> {
    let list: Vec<Value> = deckcraft_math::templates()
        .iter()
        .map(|t| {
            json!({
                "id": t.id,
                "label": t.label,
                "group": t.group,
                "linear": deckcraft_math::to_linear(&t.math),
                "omml": deckcraft_math::to_omml(&t.math),
            })
        })
        .collect();
    Ok(Value::Array(list))
}

fn from_linear(_s: &mut Session, p: &Value) -> Result<Value> {
    let text = opt(p, "linear").and_then(Value::as_str).ok_or_else(|| bad("equation.fromLinear", "`linear` must be a string"))?;
    if text.len() > MAX_LINEAR {
        return Err(bad("equation.fromLinear", "`linear` is too large"));
    }
    let mut m = deckcraft_math::from_linear(text);
    if let Some(d) = opt_bool("equation.fromLinear", p, "display")? {
        m.para = d;
    }
    Ok(describe(&m))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::*;

    fn session() -> Session {
        Session::with_new()
    }

    fn math_run(s: &Session, id: u64) -> deckcraft_model::text::Run {
        let sh = s.doc().unwrap().shape(deckcraft_model::ShapeId(id as u32)).unwrap().clone();
        sh.text.unwrap().paragraphs[0].runs[0].clone()
    }

    #[test]
    fn insert_creates_box_with_math_run_and_undoes() {
        let mut s = session();
        let before = s.doc().unwrap().current_slide().unwrap().shapes.len();
        let r = s.execute("insert.equation", &json!({"linear": "a^2+b^2=c^2"})).unwrap();
        let id = r["id"].as_u64().unwrap();
        assert!(r["omml"].as_str().unwrap().contains("xmlns:m="));
        let run = math_run(&s, id);
        assert!(matches!(&run.kind, deckcraft_model::text::RunKind::Math { omml } if omml.contains("xmlns:m=")));
        assert!(!run.text.is_empty());
        assert_eq!(run.props.size, Some(28.0));
        assert_eq!(s.doc().unwrap().current_slide().unwrap().shapes.len(), before + 1);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().current_slide().unwrap().shapes.len(), before);
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().current_slide().unwrap().shapes.len(), before + 1);
        assert!(math_run(&s, id).text.contains('c'));
    }

    #[test]
    fn update_get_round_trip_keeps_props_and_undoes() {
        let mut s = session();
        let id = s.execute("insert.equation", &json!({"linear": "x/2"})).unwrap()["id"].as_u64().unwrap();
        let before = math_run(&s, id);
        let r = s.execute("equation.update", &json!({"id": id, "linear": "sqrt(x)+1", "display": true})).unwrap();
        assert!(r["omml"].as_str().unwrap().contains("oMathPara"));
        let after = math_run(&s, id);
        assert_eq!(after.props, before.props);
        assert_ne!(after.text, before.text);
        let g = s.execute("equation.get", &json!({"id": id})).unwrap();
        assert_eq!(g["display"], true);
        assert_eq!(g["linear"], after.text);
        assert!(g["tree"].is_object());
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(math_run(&s, id), before);
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(math_run(&s, id), after);
    }

    #[test]
    fn update_accepts_omml() {
        let mut s = session();
        let id = s.execute("insert.equation", &json!({"linear": "1"})).unwrap()["id"].as_u64().unwrap();
        let omml = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:f><m:num><m:r><m:t>a</m:t></m:r></m:num><m:den><m:r><m:t>b</m:t></m:r></m:den></m:f></m:oMath>"#;
        s.execute("equation.update", &json!({"id": id, "omml": omml})).unwrap();
        let g = s.execute("equation.get", &json!({"id": id})).unwrap();
        assert!(g["omml"].as_str().unwrap().contains("<m:f>"));
        assert!(g["omml"].as_str().unwrap().contains("xmlns:m="));
    }

    #[test]
    fn bad_input_is_an_error_not_a_panic() {
        let mut s = session();
        let before = s.doc().unwrap().revision;
        for p in [
            json!({}),
            json!({"linear": ""}),
            json!({"linear": "   "}),
            json!({"omml": "not xml at all"}),
            json!({"omml": "<<<"}),
            json!({"omml": "<a><b/></a>"}),
            json!({"omml": "<m:oMath xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\"></m:oMath>"}),
            json!({"omml": 5}),
        ] {
            assert!(s.execute("insert.equation", &p).is_err(), "{p}");
        }
        assert_eq!(s.doc().unwrap().revision, before);
        let huge = "x".repeat(2 << 20);
        assert!(s.execute("insert.equation", &json!({"linear": huge})).is_err());
        // update/get with nothing to act on
        assert!(s.execute("equation.get", &json!({})).is_err());
        assert!(s.execute("equation.update", &json!({"id": 424242, "linear": "x"})).is_err());
        let title = s.doc().unwrap().current_slide().unwrap().shapes[0].id.0;
        assert!(s.execute("equation.get", &json!({"id": title})).is_err());
        assert!(s.execute("equation.update", &json!({"id": title, "linear": "x", "paragraph": 0, "run": 0})).is_err());
    }

    #[test]
    fn hostile_params_are_errors_not_retargets() {
        let mut s = session();
        let a = s.execute("insert.equation", &json!({"linear": "x/2"})).unwrap()["id"].as_u64().unwrap();
        let rev = s.doc().unwrap().revision;
        for bad in [json!(-1), json!("a"), json!(1.5), json!(1e30), json!(true), json!([1])] {
            for cmd in ["equation.get", "equation.update"] {
                assert!(s.execute(cmd, &json!({"id": bad, "linear": "y"})).is_err(), "{cmd} id {bad}");
                assert!(s.execute(cmd, &json!({"id": a, "paragraph": bad, "run": 0, "linear": "y"})).is_err(), "{cmd} paragraph {bad}");
                assert!(s.execute(cmd, &json!({"id": a, "paragraph": 0, "run": bad, "linear": "y"})).is_err(), "{cmd} run {bad}");
            }
        }
        assert!(s.execute("equation.get", &json!({"id": a, "paragraph": 0})).is_err());
        for d in [json!("yes"), json!("nope"), json!(1), json!([])] {
            assert!(s.execute("equation.update", &json!({"id": a, "linear": "y", "display": d})).is_err());
            assert!(s.execute("insert.equation", &json!({"linear": "y", "display": d})).is_err());
            assert!(s.execute("equation.fromLinear", &json!({"linear": "y", "display": d})).is_err());
        }
        assert_eq!(s.doc().unwrap().revision, rev);
        assert_eq!(math_run(&s, a).text, "x/2");
    }

    #[test]
    fn rect_is_validated() {
        let mut s = session();
        let rev = s.doc().unwrap().revision;
        for r in [
            json!("a"),
            json!([1]),
            json!([null, null, null, null]),
            json!([1e308, -1e308, 1e308, 1e308]),
            json!([0, 0, 0, 0]),
            json!([0, 0, -5, 10]),
            json!([0, 0, 100, 0.5]),
            json!([0, 0, 100, 100, 5]),
            json!([0, 0, 1e9, 100]),
            json!(["0", 0, 10, 10]),
        ] {
            assert!(s.execute("insert.equation", &json!({"linear": "x", "rect": r})).is_err(), "{r}");
        }
        assert_eq!(s.doc().unwrap().revision, rev);
        let ok = s.execute("insert.equation", &json!({"linear": "x", "rect": [10, 20, 300, 60]})).unwrap();
        let id = deckcraft_model::ShapeId(ok["id"].as_u64().unwrap() as u32);
        let x = s.doc().unwrap().shape(id).unwrap().xfrm.unwrap();
        assert_eq!((x.x, x.y, x.w), (10.0, 20.0, 300.0));
    }

    #[test]
    fn non_math_roots_are_rejected() {
        let mut s = session();
        for o in ["<a>oMath</a>", "<a><m:oMath/></a>", "<!-- oMath --><a/>", "<?xml version='1.0'?><b>oMathPara</b>", "<m:oMathX/>"] {
            assert!(s.execute("insert.equation", &json!({"omml": o})).is_err(), "{o}");
        }
        let id = s.execute("insert.equation", &json!({"linear": "x/2"})).unwrap()["id"].as_u64().unwrap();
        let before = math_run(&s, id);
        assert!(s.execute("equation.update", &json!({"id": id, "omml": "<a>oMath</a>"})).is_err());
        assert_eq!(math_run(&s, id), before);
        let ok = r#"<?xml version="1.0"?><m:oMathPara xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:oMath><m:r><m:t>q</m:t></m:r></m:oMath></m:oMathPara>"#;
        assert!(s.execute("insert.equation", &json!({"omml": ok})).is_ok());
    }

    #[test]
    fn update_keeps_display_unless_told() {
        let mut s = session();
        let id = s.execute("insert.equation", &json!({"linear": "x/2", "display": true})).unwrap()["id"].as_u64().unwrap();
        s.execute("equation.update", &json!({"id": id, "linear": "y^2"})).unwrap();
        assert_eq!(s.execute("equation.get", &json!({"id": id})).unwrap()["display"], true);
        s.execute("equation.update", &json!({"id": id, "linear": "z", "display": false})).unwrap();
        assert_eq!(s.execute("equation.get", &json!({"id": id})).unwrap()["display"], false);
        s.execute("equation.update", &json!({"id": id, "linear": "w", "display": true})).unwrap();
        // Replacement OMML that is itself inline sets it.
        let omml = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:r><m:t>a</m:t></m:r></m:oMath>"#;
        s.execute("equation.update", &json!({"id": id, "omml": omml})).unwrap();
        assert_eq!(s.execute("equation.get", &json!({"id": id})).unwrap()["display"], false);
    }

    #[test]
    fn control_characters_never_reach_run_text() {
        let mut s = session();
        let r = s.execute("insert.equation", &json!({"linear": "a\u{0}b\u{1}c\u{FFFE}"})).unwrap();
        let id = r["id"].as_u64().unwrap();
        let run = math_run(&s, id);
        assert_eq!(run.text, "abc");
        assert_eq!(r["linear"], "abc");
        let g = s.execute("equation.get", &json!({"id": id})).unwrap();
        assert_eq!(g["linear"], run.text);
    }

    #[test]
    fn repeated_inserts_do_not_stack() {
        let mut s = session();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..4 {
            let id = s.execute("insert.equation", &json!({"linear": "x"})).unwrap()["id"].as_u64().unwrap();
            let x = s.doc().unwrap().shape(deckcraft_model::ShapeId(id as u32)).unwrap().xfrm.unwrap();
            assert!(seen.insert((x.x as i64, x.y as i64)), "stacked at {},{}", x.x, x.y);
        }
    }

    #[test]
    fn copy_and_cut_of_a_partial_equation_keep_it() {
        let mut s = session();
        let id = s.execute("insert.equation", &json!({"linear": "x/2"})).unwrap()["id"].as_u64().unwrap();
        let text = math_run(&s, id).text;
        s.execute("text.edit", &json!({"id": id})).unwrap();
        s.execute("text.select", &json!({"anchor": [0, 1], "caret": [0, 2]})).unwrap();
        let copied = s.execute("edit.copy", &json!({})).unwrap();
        assert_eq!(copied["text"], text);
        let clip = s.clipboard.text.clone().unwrap();
        assert!(matches!(clip.paragraphs[0].runs[0].kind, deckcraft_model::text::RunKind::Math { .. }));
        let cut = s.execute("edit.cut", &json!({})).unwrap();
        assert_eq!(cut["text"], text);
        let clip = s.clipboard.text.clone().unwrap();
        assert!(matches!(clip.paragraphs[0].runs[0].kind, deckcraft_model::text::RunKind::Math { .. }));
        let sh = s.doc().unwrap().shape(deckcraft_model::ShapeId(id as u32)).unwrap().clone();
        assert!(sh.text.unwrap().paragraphs[0].runs.iter().all(|r| r.kind == deckcraft_model::text::RunKind::Text));
    }

    #[test]
    fn pptx_export_keeps_the_equation() {
        let mut s = session();
        let id = s.execute("insert.equation", &json!({"linear": "x/2", "display": true})).unwrap()["id"].as_u64().unwrap();
        let bytes = deckcraft_pptx::export(&s.doc().unwrap().doc).unwrap();
        let back = deckcraft_pptx::import(&bytes).unwrap();
        let found = back
            .slides
            .iter()
            .flat_map(|sl| sl.shapes.iter())
            .filter_map(|sh| sh.text.as_ref())
            .flat_map(|tb| tb.paragraphs.iter())
            .flat_map(|p| p.runs.iter())
            .find(|r| matches!(r.kind, deckcraft_model::text::RunKind::Math { .. }));
        let run = found.expect("equation survives");
        assert_eq!(run.text, math_run(&s, id).text);
    }

    #[test]
    fn templates_and_from_linear_are_queries() {
        let mut s = session();
        let t = s.execute("equation.templates", &json!({})).unwrap();
        let list = t.as_array().unwrap();
        assert!(list.len() > 10);
        assert!(list.iter().any(|x| x["id"] == "frac"));
        assert!(list.iter().all(|x| x["omml"].as_str().unwrap().contains("xmlns:m=")));
        let r = s.execute("equation.fromLinear", &json!({"linear": "(a+b)/c"})).unwrap();
        assert!(r["omml"].as_str().unwrap().contains("<m:f>"));
        assert!(s.execute("equation.fromLinear", &json!({})).is_err());
        // Queries leave the document and history alone.
        assert!(s.doc().unwrap().history.undo.is_empty());
    }

    #[test]
    fn text_editing_removes_a_math_run_whole() {
        let mut s = session();
        let id = s.execute("insert.equation", &json!({"linear": "a/b"})).unwrap()["id"].as_u64().unwrap();
        let n = math_run(&s, id).text.chars().count();
        s.execute("text.edit", &json!({"id": id, "end": true})).unwrap();
        s.execute("text.delete", &json!({"dir": "backward"})).unwrap();
        let sh = s.doc().unwrap().shape(deckcraft_model::ShapeId(id as u32)).unwrap().clone();
        assert!(sh.text.unwrap().paragraphs[0].runs.iter().all(|r| r.kind == deckcraft_model::text::RunKind::Text), "{n}");
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(matches!(math_run(&s, id).kind, deckcraft_model::text::RunKind::Math { .. }));
    }

    #[test]
    fn run_text_is_one_line() {
        let mut s = session();
        let r = s.execute("insert.equation", &json!({"linear": "a\nb\r\nc\td"})).unwrap();
        let id = r["id"].as_u64().unwrap();
        let t = math_run(&s, id).text;
        assert!(!t.chars().any(char::is_control), "{t:?}");
        assert_eq!(s.doc().unwrap().shape(deckcraft_model::ShapeId(id as u32)).unwrap().text.as_ref().unwrap().paragraphs.len(), 1);
    }

    #[test]
    fn huge_results_are_bounded() {
        let mut s = session();
        let big = "a_".repeat(super::MAX_LINEAR / 2);
        let r = s.execute("equation.fromLinear", &json!({"linear": big})).unwrap();
        assert!(serde_json::to_string(&r).unwrap().len() < 8 << 20);
        assert!(s.execute("equation.fromLinear", &json!({"linear": "a".repeat(super::MAX_LINEAR + 1)})).is_err());
    }

    #[test]
    fn word_delete_next_to_text_takes_only_the_equation() {
        let mut s = session();
        let id = s.execute("insert.equation", &json!({"linear": "x/y"})).unwrap()["id"].as_u64().unwrap();
        s.execute("text.edit", &json!({"id": id, "at": [0, 0]})).unwrap();
        s.execute("text.insert", &json!({"text": "hello"})).unwrap();
        s.execute("text.edit", &json!({"id": id, "end": true})).unwrap();
        s.execute("text.delete", &json!({"dir": "wordBackward"})).unwrap();
        let sh = s.doc().unwrap().shape(deckcraft_model::ShapeId(id as u32)).unwrap().clone();
        let runs = &sh.text.unwrap().paragraphs[0].runs;
        assert!(runs.iter().all(|r| r.kind == deckcraft_model::text::RunKind::Text));
        assert_eq!(runs.iter().map(|r| r.text.as_str()).collect::<String>(), "hello");
    }
}
