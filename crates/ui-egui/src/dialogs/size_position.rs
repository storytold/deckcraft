use super::*;

fn put_dim(d: &mut Dialog, key: &str, exact: f64, shown: String) {
    d.fields.insert(key.into(), shown.clone());
    d.fields.insert(format!("seed{key}"), shown.clone());
    d.fields.insert(format!("prev{key}"), shown);
    d.fields.insert(format!("{key}Pt"), exact.to_string());
}

fn publish_dim(d: &mut Dialog, key: &str, exact: f64, shown: String) {
    d.fields.insert(key.into(), shown.clone());
    d.fields.insert(format!("seed{key}"), shown);
    d.fields.insert(format!("{key}Pt"), exact.to_string());
}

fn selection_xfrm(app: &SlideApp) -> Option<deckcraft_geom::Xfrm> {
    let st = app.session.active()?;
    let id = *st.selection.shapes.first()?;
    let sh = st.shape(id)?;
    Some(deckcraft_engine::cmd::xfrm_of(&st.doc, &st.selection, sh))
}

fn seed_size_position(app: &SlideApp, d: &mut Dialog) {
    if d.fields.contains_key("baseW") {
        return;
    }
    let num = |k: &str| d.params.get(k).and_then(Value::as_f64).filter(|v| v.is_finite());
    let sel = selection_xfrm(app);
    let x = num("x").or_else(|| sel.map(|b| b.x)).unwrap_or(0.0);
    let y = num("y").or_else(|| sel.map(|b| b.y)).unwrap_or(0.0);
    let w = num("w").or_else(|| sel.map(|b| b.w)).unwrap_or(72.0);
    let h = num("h").or_else(|| sel.map(|b| b.h)).unwrap_or(72.0);
    let rot = num("rot").or_else(|| sel.map(|b| b.rot)).unwrap_or(0.0);
    let sw = num("scaleW").unwrap_or(100.0);
    let sh = num("scaleH").unwrap_or(100.0);
    let lock = d.params.get("lockAspect").and_then(Value::as_bool).unwrap_or(false);
    put_dim(d, "x", x, num_text(x / 72.0));
    put_dim(d, "y", y, num_text(y / 72.0));
    put_dim(d, "w", w, num_text(w / 72.0));
    put_dim(d, "h", h, num_text(h / 72.0));
    put_dim(d, "rot", rot, num_text(rot));
    put_dim(d, "scaleW", sw, num_text(sw));
    put_dim(d, "scaleH", sh, num_text(sh));
    d.fields.insert("baseW".into(), w.to_string());
    d.fields.insert("baseH".into(), h.to_string());
    let bit = if lock { "1" } else { "0" };
    d.fields.insert("lock".into(), bit.into());
    d.fields.insert("prevlock".into(), bit.into());
}

fn scale_of(pt: f64, base: f64) -> f64 {
    if base > 0.0 { pt / base * 100.0 } else { 100.0 }
}

fn current_points(d: &Dialog, key: &str) -> Option<f64> {
    if edited(d, key) { parse_shown(&field_of(d, key)).map(|inches| inches * 72.0) } else { exact_of(d, key) }
}

/// Keep scale tied to size, and the other side tied when aspect is locked.
fn reconcile_size_fields(d: &mut Dialog) {
    let lock = field_of(d, "lock") == "1";
    let was_locked = field_of(d, "prevlock") == "1";
    let base_w = parse_shown(&field_of(d, "baseW")).unwrap_or(1.0);
    let base_h = parse_shown(&field_of(d, "baseH")).unwrap_or(1.0);
    let ar = if base_h > 0.0 { base_w / base_h } else { 1.0 };
    let (w_s, h_s, sw_s, sh_s) = (field_of(d, "w"), field_of(d, "h"), field_of(d, "scaleW"), field_of(d, "scaleH"));
    let (prev_w, prev_h, prev_sw, prev_sh) = (field_of(d, "prevw"), field_of(d, "prevh"), field_of(d, "prevscaleW"), field_of(d, "prevscaleH"));
    if w_s != prev_w {
        if let Some(inches) = parse_shown(&w_s) {
            let w_pt = inches * 72.0;
            d.fields.insert("wPt".into(), w_pt.to_string());
            let pct = scale_of(w_pt, base_w);
            publish_dim(d, "scaleW", pct, num_text(pct));
            if lock {
                let h_pt = if ar > 0.0 { w_pt / ar } else { w_pt };
                publish_dim(d, "h", h_pt, num_text(h_pt / 72.0));
                let hp = scale_of(h_pt, base_h);
                publish_dim(d, "scaleH", hp, num_text(hp));
            }
        }
    } else if h_s != prev_h {
        if let Some(inches) = parse_shown(&h_s) {
            let h_pt = inches * 72.0;
            d.fields.insert("hPt".into(), h_pt.to_string());
            let pct = scale_of(h_pt, base_h);
            publish_dim(d, "scaleH", pct, num_text(pct));
            if lock {
                let w_pt = h_pt * ar;
                publish_dim(d, "w", w_pt, num_text(w_pt / 72.0));
                let wp = scale_of(w_pt, base_w);
                publish_dim(d, "scaleW", wp, num_text(wp));
            }
        }
    } else if sw_s != prev_sw {
        if let Some(pct) = parse_shown(&sw_s) {
            let w_pt = base_w * pct / 100.0;
            d.fields.insert("scaleWPt".into(), pct.to_string());
            publish_dim(d, "w", w_pt, num_text(w_pt / 72.0));
            if lock {
                let h_pt = if ar > 0.0 { w_pt / ar } else { w_pt };
                publish_dim(d, "h", h_pt, num_text(h_pt / 72.0));
                publish_dim(d, "scaleH", pct, num_text(pct));
            }
        }
    } else if sh_s != prev_sh {
        if let Some(pct) = parse_shown(&sh_s) {
            let h_pt = base_h * pct / 100.0;
            d.fields.insert("scaleHPt".into(), pct.to_string());
            publish_dim(d, "h", h_pt, num_text(h_pt / 72.0));
            if lock {
                let w_pt = h_pt * ar;
                publish_dim(d, "w", w_pt, num_text(w_pt / 72.0));
                publish_dim(d, "scaleW", pct, num_text(pct));
            }
        }
    } else if lock
        && !was_locked
        && let Some(w_pt) = current_points(d, "w")
    {
        let h_pt = if ar > 0.0 { w_pt / ar } else { w_pt };
        publish_dim(d, "h", h_pt, num_text(h_pt / 72.0));
        let hp = scale_of(h_pt, base_h);
        publish_dim(d, "scaleH", hp, num_text(hp));
        let wp = scale_of(w_pt, base_w);
        publish_dim(d, "scaleW", wp, num_text(wp));
    }
    for key in ["x", "y", "w", "h", "rot", "scaleW", "scaleH"] {
        let v = field_of(d, key);
        d.fields.insert(format!("prev{key}"), v);
    }
    d.fields.insert("prevlock".into(), if lock { "1".into() } else { "0".into() });
}

fn shown_points(d: &Dialog, key: &str, label: &str) -> Result<f64, String> {
    if edited(d, key) {
        let n = parse_shown(&field_of(d, key)).ok_or_else(|| format!("{label} must be a number"))?;
        return Ok(n * 72.0);
    }
    exact_of(d, key).ok_or_else(|| format!("{label} must be a number"))
}

fn shown_number(d: &Dialog, key: &str, label: &str) -> Result<f64, String> {
    if edited(d, key) {
        return parse_shown(&field_of(d, key)).ok_or_else(|| format!("{label} must be a number"));
    }
    exact_of(d, key).ok_or_else(|| format!("{label} must be a number"))
}

fn size_position_value(d: &Dialog) -> Result<Value, String> {
    let x = shown_points(d, "x", "Horizontal")?;
    let y = shown_points(d, "y", "Vertical")?;
    let rot = shown_number(d, "rot", "Rotation")?;
    let base_w = parse_shown(&field_of(d, "baseW")).unwrap_or(1.0);
    let base_h = parse_shown(&field_of(d, "baseH")).unwrap_or(1.0);
    let mut w = if edited(d, "w") {
        shown_points(d, "w", "Width")?
    } else if edited(d, "scaleW") {
        base_w * shown_number(d, "scaleW", "Scale width")? / 100.0
    } else {
        shown_points(d, "w", "Width")?
    };
    let mut h = if edited(d, "h") {
        shown_points(d, "h", "Height")?
    } else if edited(d, "scaleH") {
        base_h * shown_number(d, "scaleH", "Scale height")? / 100.0
    } else {
        shown_points(d, "h", "Height")?
    };
    if field_of(d, "lock") == "1" {
        let ar = if base_h > 0.0 { base_w / base_h } else { 1.0 };
        let width_drives = edited(d, "w") || edited(d, "scaleW") || (!edited(d, "h") && !edited(d, "scaleH"));
        if width_drives {
            h = if ar > 0.0 { w / ar } else { h };
        } else {
            w = h * ar;
        }
    }
    Ok(json!({"x": x, "y": y, "w": w, "h": h, "rot": rot, "lockAspect": field_of(d, "lock") == "1"}))
}

pub(super) fn size_position_dialog(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    seed_size_position(app, d);
    ui.label(egui::RichText::new("Size").strong());
    text_row(ui, d, "Height (in):", "h");
    text_row(ui, d, "Width (in):", "w");
    text_row(ui, d, "Rotation (°):", "rot");
    text_row(ui, d, "Scale height (%):", "scaleH");
    text_row(ui, d, "Scale width (%):", "scaleW");
    check(ui, d, "Lock aspect ratio", "lock", false);
    ui.add_space(8.0);
    ui.label(egui::RichText::new("Position").strong());
    ui.label("From the top-left corner of the slide");
    text_row(ui, d, "Horizontal (in):", "x");
    text_row(ui, d, "Vertical (in):", "y");
    reconcile_size_fields(d);
    let (ok, cancel) = buttons(ui, "OK");
    if ok {
        match size_position_value(d) {
            Ok(p) => return app.run("shape.sizePosition", p).is_ok(),
            Err(msg) => {
                app.set_status(msg);
                return false;
            }
        }
    }
    cancel
}
