use super::{
    Arc, NO_ID_SPACE, PhType, Result, Run, RunKind, Session, Shape, ShapeId, Size, TextBody, Value, bad, bool_or, bool_param, defaults, f64_param,
    json, ok, str_param,
};

pub(super) fn scale_shapes(shapes: &mut [Shape], sx: f64, sy: f64, k: f64, ox: f64, oy: f64) {
    for sh in shapes {
        if let Some(x) = sh.xfrm.as_mut() {
            x.x = x.x * sx + ox;
            x.y = x.y * sy + oy;
            x.w *= sx;
            x.h *= sy;
        }
        if let Some(t) = sh.text.as_mut() {
            deckcraft_model::edit::format_all(t, &|r| {
                if let Some(sz) = r.size.as_mut() {
                    *sz = (*sz * k).max(1.0);
                }
            });
        }
    }
}

pub(super) fn slide_size(s: &mut Session, p: &Value) -> Result<Value> {
    let size = if let Some(pre) = str_param(p, "preset") {
        let k = pre.to_ascii_lowercase();
        let found = defaults::SLIDE_SIZES.iter().find(|(l, _, _)| l.to_ascii_lowercase().starts_with(&k) || l.to_ascii_lowercase().contains(&k));
        let (_, w, h) = found.ok_or_else(|| bad("design.slideSize", format!("unknown preset `{pre}`")))?;
        Size::new(*w, *h)
    } else {
        Size::new(
            f64_param(p, "w").ok_or_else(|| bad("design.slideSize", "missing `w`"))?.clamp(72.0, 4032.0),
            f64_param(p, "h").ok_or_else(|| bad("design.slideSize", "missing `h`"))?.clamp(72.0, 4032.0),
        )
    };
    let size = match str_param(p, "orientation") {
        Some("portrait") if size.width > size.height => Size::new(size.height, size.width),
        Some("landscape") if size.height > size.width => Size::new(size.height, size.width),
        Some("portrait" | "landscape") | None => size,
        Some(o) => return Err(bad("design.slideSize", format!("unknown orientation `{o}`"))),
    };
    let mode = str_param(p, "scale").unwrap_or("ensureFit").to_string();
    s.edit(|doc, _| {
        let old = doc.slide_size;
        let (sx, sy) = (size.width / old.width.max(1.0), size.height / old.height.max(1.0));
        let (sx, sy, k, ox, oy) = match mode.as_str() {
            "none" => (1.0, 1.0, 1.0, 0.0, 0.0),
            "maximize" => {
                let k = sx.max(sy);
                (k, k, k, (size.width - old.width * k) / 2.0, (size.height - old.height * k) / 2.0)
            }
            _ => {
                let k = sx.min(sy);
                (k, k, k, (size.width - old.width * k) / 2.0, (size.height - old.height * k) / 2.0)
            }
        };
        // Masters and layouts stretch to the new size; slides scale by mode.
        let (msx, msy) = (size.width / old.width.max(1.0), size.height / old.height.max(1.0));
        for m in &mut doc.masters {
            let m = Arc::make_mut(m);
            scale_shapes(&mut m.shapes, msx, msy, 1.0, 0.0, 0.0);
            for l in &mut m.layouts {
                scale_shapes(&mut l.shapes, msx, msy, 1.0, 0.0, 0.0);
            }
        }
        for sl in &mut doc.slides {
            scale_shapes(&mut Arc::make_mut(sl).shapes, sx, sy, k, ox, oy);
        }
        doc.slide_size = size;
        Ok(())
    })?;
    Ok(json!({"w": size.width, "h": size.height}))
}

pub(super) fn header_footer(s: &mut Session, p: &Value) -> Result<Value> {
    s.edit(|doc, sel| {
        let hf = &mut doc.header_footer;
        if let Some(v) = bool_param(p, "date") {
            hf.date = v;
        }
        if let Some(v) = str_param(p, "dateText") {
            hf.date_text = v.into();
        }
        if let Some(v) = str_param(p, "dateFormat") {
            // Refused rather than ignored: a format that renders nothing would leave the
            // placeholder showing its saved text for ever.
            if !deckcraft_text::datetime::DATE_KINDS.contains(&v) {
                return Err(bad("design.headerFooter", "dateFormat: one of datetime1…datetime13"));
            }
            hf.date_format = v.into();
        }
        if let Some(v) = bool_param(p, "slideNumber") {
            hf.slide_number = v;
        }
        if let Some(v) = bool_param(p, "footer") {
            hf.footer = v;
        }
        if let Some(v) = str_param(p, "footerText") {
            hf.footer_text = v.into();
            hf.footer = hf.footer || !v.is_empty();
        }
        if let Some(v) = bool_param(p, "hideOnTitle") {
            hf.hide_on_title = v;
        }
        let hf = hf.clone();
        let date_given = str_param(p, "dateText").is_some();
        let format_given = str_param(p, "dateFormat").is_some();
        let all = bool_or(p, "all", true);
        let list: Vec<usize> = if all { (0..doc.slides.len()).collect() } else { vec![sel.slide] };
        for i in list {
            let Some(layout) = doc.slides.get(i).map(|x| x.layout) else { continue };
            let lay = doc.layout(layout).map(|(_, l)| l.clone());
            let is_title = lay.as_ref().is_some_and(|l| l.kind == deckcraft_model::LayoutType::Title);
            let want = |k: PhType| {
                !(hf.hide_on_title && is_title)
                    && match k {
                        PhType::Date => hf.date,
                        PhType::SlideNum => hf.slide_number,
                        PhType::Footer => hf.footer,
                        _ => false,
                    }
            };
            let mut new_ids = vec![];
            for _ in 0..3 {
                new_ids.push(ShapeId(doc.alloc_id().ok_or_else(|| bad("design.headerFooter", NO_ID_SPACE))?));
            }
            let Some(sl) = doc.slides.get_mut(i) else { continue };
            let sl = Arc::make_mut(sl);
            for (n, k) in [PhType::Date, PhType::Footer, PhType::SlideNum].into_iter().enumerate() {
                let has = sl.shapes.iter().position(|x| x.ph_type() == Some(k));
                match (has, want(k)) {
                    (Some(pos), false) => {
                        sl.shapes.remove(pos);
                    }
                    (None, true) => {
                        let lp = lay.as_ref().and_then(|l| l.shapes.iter().find(|x| x.ph_type() == Some(k)).cloned());
                        let Some(lp) = lp else { continue };
                        let body = match k {
                            PhType::SlideNum => TextBody {
                                paragraphs: vec![deckcraft_model::Paragraph {
                                    runs: vec![Run {
                                        text: "‹#›".into(),
                                        props: Default::default(),
                                        kind: RunKind::Field { field: "slidenum".into() },
                                    }],
                                    ..Default::default()
                                }],
                                ..Default::default()
                            },
                            PhType::Date if hf.date_text.is_empty() => TextBody {
                                paragraphs: vec![deckcraft_model::Paragraph {
                                    runs: vec![Run {
                                        text: date_now(&hf.date_format),
                                        props: Default::default(),
                                        kind: RunKind::Field { field: date_kind(&hf.date_format).into() },
                                    }],
                                    ..Default::default()
                                }],
                                ..Default::default()
                            },
                            PhType::Date => TextBody::from_text(&hf.date_text),
                            _ => TextBody::from_text(&hf.footer_text),
                        };
                        sl.shapes.push(Shape {
                            id: new_ids.get(n).copied().unwrap_or_default(),
                            name: lp.name.clone(),
                            ph: lp.ph.clone(),
                            text: Some(body),
                            ..Default::default()
                        });
                    }
                    (Some(pos), true) if k == PhType::Footer => {
                        if let Some(sh) = sl.shapes.get_mut(pos) {
                            sh.text = Some(TextBody::from_text(&hf.footer_text));
                        }
                    }
                    (Some(pos), true) if k == PhType::Date && (date_given || format_given) => {
                        if let Some(sh) = sl.shapes.get_mut(pos) {
                            set_date(sh, &hf.date_text, &hf.date_format);
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    })?;
    ok()
}

/// The date field type `format` names, `datetime1` when it is empty or unknown.
fn date_kind(format: &str) -> &str {
    if deckcraft_text::datetime::DATE_KINDS.contains(&format) { format } else { "datetime1" }
}

/// The saved text of a date field of `format`, as it shows now.
fn date_now(format: &str) -> String {
    let kind = date_kind(format);
    deckcraft_text::datetime::DateTime::now_local().and_then(|now| deckcraft_text::datetime::field_text(kind, None, now)).unwrap_or_else(today)
}

/// Point a slide's date placeholder at a fixed date (its own plain text) or, for an empty
/// `fixed`, at an automatic date field (one it already has keeps its format).
fn set_date(sh: &mut Shape, fixed: &str, format: &str) {
    let runs = || sh.text.iter().flat_map(|t| &t.paragraphs).flat_map(|p| &p.runs);
    let field_now = runs().find_map(|r| match &r.kind {
        RunKind::Field { field } if deckcraft_text::datetime::is_date_field(field) => Some(field.clone()),
        _ => None,
    });
    let has_field = field_now.is_some();
    let props = runs().next().map(|r| r.props.clone()).unwrap_or_default();
    let want = date_kind(format);
    let (text, kind) = match fixed {
        // Automatic: keep the field, but follow a format that has changed.
        "" if field_now.as_deref() == Some(want) => return,
        "" if has_field && format.is_empty() => return,
        "" => (date_now(format), RunKind::Field { field: want.into() }),
        _ if !has_field && sh.text.as_ref().is_some_and(|t| t.text() == fixed) => return,
        _ => (fixed.to_string(), RunKind::Text),
    };
    let para = deckcraft_model::Paragraph { runs: vec![Run { text, props, kind }], ..Default::default() };
    sh.text = Some(TextBody { paragraphs: vec![para], ..sh.text.clone().unwrap_or_default() });
}

/// Today's local date as a `datetime1` field shows it (M/D/YYYY): the saved text of a new date
/// field. Empty where there is no clock (wasm).
pub fn today() -> String {
    date_field_text("datetime1")
}

/// What date field `kind` shows now (empty if unknown or there is no clock).
pub fn date_field_text(kind: &str) -> String {
    use deckcraft_text::datetime::{DateTime, field_text};
    DateTime::now_local().and_then(|now| field_text(kind, None, now)).unwrap_or_default()
}

/// Days since 1970-01-01 → (year, month, day), exact for every `i64`.
pub fn civil(z: i64) -> (i64, u32, u32) {
    use chrono::Datelike;
    // `NaiveDate` covers only about ±262143 years, and the Gregorian calendar repeats every 400
    // years (146_097 days). chrono converts the day within its cycle and whole cycles go back
    // into the year (an i128 for i64::MAX/MIN). 1970-01-01 is `num_days_from_ce` 719_163, and
    // 0..146_097 days from it always fits.
    let (cycles, rest) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    match i32::try_from(rest + 719_163).ok().and_then(chrono::NaiveDate::from_num_days_from_ce_opt) {
        Some(d) => {
            let y = i128::from(d.year()) + i128::from(cycles) * 400;
            (i64::try_from(y).unwrap_or(if y < 0 { i64::MIN } else { i64::MAX }), d.month(), d.day())
        }
        None => (1970, 1, 1),
    }
}

#[cfg(test)]
mod header_footer_date_tests {
    use deckcraft_model::PhType;
    use deckcraft_model::text::RunKind;
    use serde_json::json;

    use crate::Session;

    /// The date field of the Date placeholders of every slide: (saved text, field type).
    fn dates(s: &Session) -> Vec<(String, Option<String>)> {
        let d = &s.doc().unwrap().doc;
        d.slides
            .iter()
            .flat_map(|sl| sl.shapes.iter())
            .filter(|sh| sh.ph_type() == Some(PhType::Date))
            .flat_map(|sh| sh.text.iter())
            .flat_map(|t| t.paragraphs.iter())
            .flat_map(|p| p.runs.iter())
            .map(|r| {
                let kind = match &r.kind {
                    RunKind::Field { field } => Some(field.clone()),
                    _ => None,
                };
                (r.text.clone(), kind)
            })
            .collect()
    }

    fn sample(kind: &str) -> String {
        let now = deckcraft_text::datetime::DateTime::now_local().expect("a local date");
        deckcraft_text::datetime::field_text(kind, None, now).expect("a sample")
    }

    /// Runs `design.headerFooter` and checks the slide's date run is an automatic `kind` field.
    /// The clock is read before and after the command, so a second ticking over between the
    /// command and the read cannot fail the comparison. Returns the run text.
    fn run_automatic(s: &mut Session, params: serde_json::Value, kind: &str) -> String {
        let before = sample(kind);
        s.execute("design.headerFooter", &params).unwrap();
        let after = sample(kind);
        let got = dates(s);
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].1.as_deref(), Some(kind));
        assert!(got[0].0 == before || got[0].0 == after, "{got:?} is neither {before:?} nor {after:?}");
        got[0].0.clone()
    }

    /// Header & Footer's automatic date takes the format it is given. `HeaderFooter::date_format`
    /// existed for this and was never read: the placeholder was always `datetime1`, and a
    /// `dateFormat` parameter was accepted and silently dropped.
    #[test]
    fn the_automatic_date_takes_the_format_it_is_given() {
        let mut s = Session::with_new();

        // Default: the short date, as before.
        run_automatic(&mut s, json!({"date": true, "dateText": "", "all": true}), "datetime1");

        // A format: the field type changes and the saved text is that format's.
        run_automatic(&mut s, json!({"date": true, "dateFormat": "datetime5", "all": true}), "datetime5");
        assert_eq!(s.doc().unwrap().doc.header_footer.date_format, "datetime5");

        // A time format works the same way, and these two differ, so the format is really read.
        let time = run_automatic(&mut s, json!({"date": true, "dateFormat": "datetime11", "all": true}), "datetime11");
        assert_ne!(time, sample("datetime5"));

        // Asking again changes nothing (and does not rewrite the run).
        let again = run_automatic(&mut s, json!({"date": true, "dateFormat": "datetime11", "all": true}), "datetime11");
        assert_eq!(again, time);

        // A fixed date still wins: plain text, no field.
        s.execute("design.headerFooter", &json!({"date": true, "dateText": "4 July 1776", "all": true})).unwrap();
        assert_eq!(dates(&s), vec![("4 July 1776".to_string(), None)]);

        // Back to automatic: the remembered format comes back, not datetime1.
        run_automatic(&mut s, json!({"date": true, "dateText": "", "dateFormat": "datetime11", "all": true}), "datetime11");

        // An unknown format is refused rather than ignored, and changes nothing.
        for bad in ["datetime0", "datetime14", "nonsense", ""] {
            assert!(s.execute("design.headerFooter", &json!({"date": true, "dateFormat": bad})).is_err(), "{bad}");
        }
        assert_eq!(s.doc().unwrap().doc.header_footer.date_format, "datetime11");
        let got = dates(&s);
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].1.as_deref(), Some("datetime11"));
    }
}
