use super::{
    ColorRef, Fill, Result, RunKind, SchemeSlot, Session, TextBody, Value, Xfrm, add_shape, bad, bool_or, bool_param, json, new_text_box, ok,
    str_param, usize_param,
};

pub(super) fn word_art(s: &mut Session, p: &Value) -> Result<Value> {
    let text = str_param(p, "text").unwrap_or("Your text here");
    let size = s.doc()?.doc.slide_size;
    let style = usize_param(p, "style").unwrap_or(0);
    let mut shape = new_text_box(Xfrm::new(size.width * 0.2, size.height * 0.4, size.width * 0.6, 80.0), text);
    if let Some(t) = shape.text.as_mut() {
        for para in &mut t.paragraphs {
            para.props.align = Some(deckcraft_model::text::Align::Center);
            for r in &mut para.runs {
                r.props.size = Some(54.0);
                r.props.bold = Some(style % 2 == 1);
                r.props.fill = Some(Fill::solid(ColorRef::scheme(match style % 4 {
                    0 => SchemeSlot::Tx1,
                    1 => SchemeSlot::Accent1,
                    2 => SchemeSlot::Accent2,
                    _ => SchemeSlot::Accent4,
                })));
                if style % 3 == 2 {
                    r.props.outline = Some(deckcraft_model::Line::solid(ColorRef::scheme(SchemeSlot::Bg1), 1.0));
                }
            }
        }
    }
    let id = add_shape(s, "insert.wordArt", shape, true)?;
    crate::cmd::text::fit_text_box(s, id)?;
    Ok(json!({"id": id}))
}

fn field_run(s: &mut Session, field: &str, text: &str) -> Result<Value> {
    if s.doc()?.selection.text.is_some() {
        return crate::cmd::text::insert_field(s, field, text);
    }
    // Not editing: turn on the footer field for the slide (Header & Footer).
    let key = if field == "slidenum" { "slideNumber" } else { "date" };
    s.execute("design.headerFooter", &json!({key: true}))
}

pub(super) fn slide_number(s: &mut Session, _p: &Value) -> Result<Value> {
    field_run(s, "slidenum", "‹#›")
}

const DATE_FORMATS: [&str; 13] = [
    "datetime1",
    "datetime2",
    "datetime3",
    "datetime4",
    "datetime5",
    "datetime6",
    "datetime7",
    "datetime8",
    "datetime9",
    "datetime10",
    "datetime11",
    "datetime12",
    "datetime13",
];

struct DateOpts {
    format: String,
    update: bool,
    /// BCP 47 tag when the caller set `language`; absent keeps the surrounding run's language.
    lang: Option<String>,
    sample: String,
}

/// `true` when the call carries a format, update flag or language (apply it). Empty params open
/// the dialog instead.
fn date_time_apply(p: &Value) -> bool {
    p.as_object().is_some_and(|o| o.contains_key("format") || o.contains_key("update") || o.contains_key("language"))
}

fn canon_lang(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_lowercase().replace('_', "-").as_str() {
        "en" | "en-us" => Some("en-US"),
        "en-gb" => Some("en-GB"),
        "fr" | "fr-fr" => Some("fr-FR"),
        "ja" | "ja-jp" => Some("ja-JP"),
        "sv" | "sv-se" => Some("sv-SE"),
        _ => None,
    }
}

fn date_opts(p: &Value) -> Result<DateOpts> {
    if p.get("format").is_some() {
        let f = str_param(p, "format").filter(|f| DATE_FORMATS.contains(f));
        if f.is_none() {
            return Err(bad("insert.dateTime", "`format` must be datetime1..datetime13"));
        }
    }
    if p.get("update").is_some() && bool_param(p, "update").is_none() {
        return Err(bad("insert.dateTime", "`update` must be true or false"));
    }
    let lang = if p.get("language").is_some() {
        let raw = str_param(p, "language").filter(|l| !l.trim().is_empty());
        let Some(raw) = raw else {
            return Err(bad("insert.dateTime", "`language` must be en-US, en-GB, fr-FR, ja-JP or sv-SE"));
        };
        Some(canon_lang(raw).ok_or_else(|| bad("insert.dateTime", "`language` must be en-US, en-GB, fr-FR, ja-JP or sv-SE"))?.to_string())
    } else {
        None
    };
    let format = str_param(p, "format").unwrap_or("datetime1").to_string();
    let sample = date_sample(&format, lang.as_deref())?;
    Ok(DateOpts { format, update: bool_or(p, "update", true), lang, sample })
}

/// The text `format` shows now in `lang`. Errors when that language has no names for the format
/// (month names and AM/PM are English only). An empty string where there is no clock (wasm).
fn date_sample(format: &str, lang: Option<&str>) -> Result<String> {
    use deckcraft_text::datetime::{DateTime, field_text};
    let probe = DateTime::new(2026, 10, 9, 20, 5, 7);
    if field_text(format, lang, probe).is_none() {
        return Err(bad("insert.dateTime", "this format isn't available in that language"));
    }
    Ok(DateTime::now_local().and_then(|now| field_text(format, lang, now)).unwrap_or_default())
}

pub(super) fn date_time(s: &mut Session, p: &Value) -> Result<Value> {
    if !date_time_apply(p) {
        s.ui_requests.push(crate::UiRequest::Dialog { id: "dateTime".into(), params: json!({}) });
        return ok();
    }
    let opts = date_opts(p)?;
    let (doc, sel) = {
        let st = s.doc()?;
        (st.doc.clone(), st.selection.clone())
    };
    let r = apply_date(s, &opts);
    if r.is_err()
        && let Some(st) = s.active_mut()
    {
        st.doc = doc;
        st.selection = sel;
    }
    r
}

fn apply_date(s: &mut Session, o: &DateOpts) -> Result<Value> {
    if s.doc()?.selection.text.is_some() {
        if o.update {
            crate::cmd::text::insert_field(s, &o.format, &o.sample)?;
            if let Some(lang) = &o.lang {
                stamp_caret_lang(s, lang)?;
            }
        } else {
            insert_plain(s, &o.sample, o.lang.as_deref())?;
        }
        return Ok(json!({"where": "caret", "format": o.format, "update": o.update, "text": o.sample}));
    }
    // Not editing: the slide's date placeholder (this slide only). A layout with no date
    // placeholder gets a text box instead, so the chosen format is still inserted.
    s.execute("design.headerFooter", &json!({"date": true, "all": false}))?;
    if !write_date_placeholder(s, o)? {
        return insert_date_box(s, o);
    }
    Ok(json!({"where": "footer", "format": o.format, "update": o.update, "text": o.sample}))
}

/// Point the current slide's date placeholder at this format (a field, or the sample as plain text).
fn write_date_placeholder(s: &mut Session, o: &DateOpts) -> Result<bool> {
    let format = o.format.clone();
    let sample = o.sample.clone();
    let lang = o.lang.clone();
    let update = o.update;
    s.edit(|doc, sel| {
        let Some(sl) = doc.slides.get_mut(sel.slide) else { return Ok(false) };
        let sl = std::sync::Arc::make_mut(sl);
        let Some(sh) = sl.shapes.iter_mut().find(|x| x.ph_type() == Some(deckcraft_model::PhType::Date)) else {
            return Ok(false);
        };
        let mut props = sh.text.as_ref().and_then(|t| t.paragraphs.first()).and_then(|p| p.runs.first()).map(|r| r.props.clone()).unwrap_or_default();
        if let Some(lang) = lang {
            props.lang = Some(lang);
        }
        let kind = if update { RunKind::Field { field: format } } else { RunKind::Text };
        let para = deckcraft_model::Paragraph { runs: vec![deckcraft_model::Run { text: sample, props, kind }], ..Default::default() };
        let body = sh.text.clone().unwrap_or_default();
        sh.text = Some(TextBody { paragraphs: vec![para], ..body });
        Ok(true)
    })
}

fn insert_date_box(s: &mut Session, o: &DateOpts) -> Result<Value> {
    let size = s.doc()?.doc.slide_size;
    let y = (size.height - 48.0).max(0.0);
    s.execute("insert.textBox", &json!({"rect": [36.0, y, 360.0, 28.0], "text": ""}))?;
    if o.update {
        crate::cmd::text::insert_field(s, &o.format, &o.sample)?;
    } else {
        crate::cmd::text::insert_text(s, &o.sample, None)?;
    }
    if let Some(lang) = &o.lang {
        stamp_caret_lang(s, lang)?;
    }
    Ok(json!({"where": "textBox", "format": o.format, "update": o.update, "text": o.sample}))
}

/// Insert `text` at the caret. `lang`, when set, is on that run before runs with equal props merge,
/// so the date stays its own run next to the surrounding text.
fn insert_plain(s: &mut Session, text: &str, lang: Option<&str>) -> Result<()> {
    let text = text.to_string();
    let lang = lang.map(str::to_string);
    crate::cmd::text::edit_text(s, move |body, t| {
        let (a, b) = t.ordered();
        let at = deckcraft_model::edit::delete(body, a, b);
        let mut props = deckcraft_model::edit::props_at(body, at);
        if let Some(lang) = lang.clone() {
            props.lang = Some(lang);
        }
        let c = deckcraft_model::edit::insert(body, at, &text, Some(props));
        t.anchor = c;
        t.caret = c;
    })?;
    Ok(())
}

/// Tag the run that ends at the caret with `lang`, so a date field keeps that language when it updates.
fn stamp_caret_lang(s: &mut Session, lang: &str) -> Result<()> {
    let lang = lang.to_string();
    crate::cmd::text::edit_text(s, move |body, t| {
        let Some(p) = body.paragraphs.get_mut(t.caret.0) else { return };
        let mut at = 0usize;
        for r in &mut p.runs {
            let n = r.char_len();
            if n > 0 && at < t.caret.1 && t.caret.1 <= at + n {
                r.props.lang = Some(lang);
                return;
            }
            at += n;
        }
    })?;
    Ok(())
}

pub(super) fn symbol(s: &mut Session, p: &Value) -> Result<Value> {
    let t = str_param(p, "text").ok_or_else(|| bad("insert.symbol", "missing `text`"))?.to_string();
    if s.doc()?.selection.text.is_none() {
        return Err(bad("insert.symbol", "place the insertion point in text first"));
    }
    crate::cmd::text::insert_text(s, &t, None)
}
