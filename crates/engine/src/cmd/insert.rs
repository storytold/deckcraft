//! Insert tab: shapes, text boxes, pictures, tables, charts, audio, video, WordArt, fields.

use deckcraft_color::SchemeSlot;
use deckcraft_geom::Xfrm;
use deckcraft_model::chart::ChartType;
use deckcraft_model::style::PictureFill;
use deckcraft_model::text::{AutoFit, RunKind, TextBody};
use deckcraft_model::{Chart, ColorRef, Fill, Geom, Line, MediaClip, Shape, ShapeId, ShapeKind, ShapeStyle, Table};
use serde_json::{Value, json};

use super::{
    CommandSpec, NO_ID_SPACE, always, bad, base64_decode, base64_encode, bool_or, bool_param, cmd, color_value, edit_shapes, has_doc, has_selection,
    has_slide, has_text_or_shapes, ok, rect_param, str_param, usize_param, xfrm_of,
};
use crate::{EngineError, Result, Session};

mod album;
mod chart;
mod links;
mod picture;
mod text_fields;

use album::{photo_album, photo_album_edit};
pub use chart::chart_type;
use chart::{chart, chart_data, chart_options, chart_type_cmd};
use links::{action_button, hyperlink, shape_action};
pub use picture::content_type;
pub(crate) use picture::media_bytes;
use picture::{audio, picture, video};
use text_fields::{date_time, slide_number, symbol, word_art};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "shape.insert",
            "Shapes",
            ["Insert", "Illustrations"],
            None,
            "{preset: rect|roundRect|ellipse|triangle|rightArrow|star5|… (see shape.presets), rect: [x,y,w,h] pt, text?: string}",
            has_slide,
            insert_shape
        ),
        cmd!(
            "insert.icon",
            "Icons",
            ["Insert", "Illustrations"],
            None,
            "{name: star|arrow-right|… (see insert.icons), rect?: [x,y,w,h]}",
            has_slide,
            insert_icon
        ),
        cmd!(query "insert.icons", "Icon Gallery", [], None, "{} → [{name, label, category}]", always, list_icons),
        cmd!(query "shape.presets", "Shape Gallery", [], None, "{} → [{name, label, category}]", always, presets),
        cmd!("insert.textBox", "Text Box", ["Insert", "Text"], None, "{rect?: [x,y,w,h], text?: string}", has_slide, text_box),
        cmd!(
            "insert.picture",
            "Picture from File…",
            ["Insert", "Images", "Pictures"],
            None,
            "{path? | data: base64, name?, rect?: [x,y,w,h]}",
            has_slide,
            picture
        ),
        cmd!(
            "insert.photoAlbum",
            "Photo Album…",
            ["Insert", "Images"],
            None,
            "{pictures?: [{path?|data: base64, name?, rot?: deg, brightness?: -1..1, contrast?: -1..1} | {text}], layout?: fit|1|2|4|1title|2title|4title, frame?: none|rect|rounded|simple, captions?: bool, grayscale?: bool, theme?: name} — omit pictures to open the dialog",
            has_doc,
            photo_album
        ),
        cmd!(
            "insert.photoAlbumEdit",
            "Edit Photo Album…",
            ["Insert", "Images"],
            None,
            "{album?: id, pictures?: same as insert.photoAlbum, layout?, frame?, captions?, grayscale?, theme?} — omit pictures to reopen the dialog on the current album",
            has_doc,
            photo_album_edit
        ),
        cmd!("insert.table", "Table…", ["Insert", "Tables"], None, "{rows, cols, rect?}", has_slide, table),
        cmd!(
            "insert.chart",
            "Chart…",
            ["Insert", "Illustrations"],
            None,
            "{type?: column|bar|line|pie|doughnut|area|scatter|stackedColumn|…, rect?, categories?: [..], series?: [{name, values}]}",
            has_slide,
            chart
        ),
        cmd!("insert.audio", "Audio from File…", ["Insert", "Media", "Audio"], None, "{path? | data: base64, name?, rect?}", has_slide, audio),
        cmd!("insert.video", "Video from File…", ["Insert", "Media", "Video"], None, "{path? | data: base64, name?, rect?}", has_slide, video),
        cmd!("insert.wordArt", "WordArt", ["Insert", "Text"], None, "{text?, style?: 0..}", has_slide, word_art),
        cmd!("insert.slideNumber", "Slide Number", ["Insert", "Text"], None, "{}", has_slide, slide_number),
        cmd!(
            "insert.dateTime",
            "Date & Time",
            ["Insert", "Text"],
            None,
            "{format?: datetime1..datetime13, update?: bool, language?: en-US|en-GB|fr-FR|ja-JP|sv-SE} — no keys opens the dialog",
            has_slide,
            date_time
        ),
        cmd!("insert.symbol", "Symbol", ["Insert", "Symbols"], None, "{text: character(s)}", has_slide, symbol),
        cmd!(
            "shape.action",
            "Action Settings",
            ["Insert", "Links"],
            None,
            "{on?: click|hover, action: none|next|previous|first|last|lastViewed|endShow|playMedia|slide|url|customShow|program, slide?: index, url?, show?: custom show name, path?, highlight?: bool, tooltip?, ids?}",
            has_selection,
            shape_action
        ),
        cmd!(
            "insert.hyperlink",
            "Link",
            ["Insert", "Links"],
            Some("Cmd+K"),
            "{url? | slide?: index, tooltip?, text?: shown when nothing is selected, highlightClick?, ids?}",
            has_text_or_shapes,
            hyperlink
        ),
        cmd!("chart.type", "Change Chart Type", ["Chart Design", "Type"], None, "{type, id?}", has_selection, chart_type_cmd),
        cmd!(
            "chart.data",
            "Edit Data",
            ["Chart Design", "Data"],
            None,
            "{categories?: [..], levels?: [[..]] (outer category levels, innermost first), series?: [{name, values}], id?}",
            has_selection,
            chart_data
        ),
        cmd!(
            "chart.options",
            "Chart Elements",
            ["Chart Design", "Chart Layouts"],
            None,
            "{title?: string|null, legend?: b|t|l|r|null, dataLabels?: bool, gridlines?: bool, palette?, binCount?: n|null, binSize?: n|null, subtotals?: [point index], quartiles?: exclusive|inclusive, upDownBars?: bool, textColor?: #RRGGBB|accent1..6|tx1…|null (title, axes, legend and labels; null for PowerPoint's default: the theme's text colour at 65 % plus 35 %, 595959 on a light theme), id?}",
            has_selection,
            chart_options
        ),
        cmd!(
            "insert.smartArt",
            "SmartArt",
            ["Insert", "Illustrations"],
            None,
            "{kind: list|process|cycle|hierarchy|pyramid|matrix, items?: [text] (leading tabs = sub-points), nodes?: [{text, level}]} → {id}",
            has_slide,
            smart_art
        ),
        cmd!(
            "insert.actionButton",
            "Action Buttons",
            ["Insert", "Illustrations"],
            None,
            "{kind: back|forward|beginning|end|home|information|return|movie|document|sound|help|custom, rect?}",
            has_slide,
            action_button
        ),
    ]
}

/// Default size for a click-inserted shape (1 inch square, PowerPoint style).
fn default_rect(s: &Session, w: f64, h: f64) -> Xfrm {
    let size = s.active().map(|d| d.doc.slide_size).unwrap_or(deckcraft_model::defaults::WIDE);
    Xfrm::new((size.width - w) / 2.0, (size.height - h) / 2.0, w, h)
}

/// The word an automatic shape name starts with ("Rectangle" in "Rectangle 3").
pub(crate) fn auto_name_base(shape: &Shape) -> String {
    match &shape.kind {
        ShapeKind::Picture { .. } => "Picture".to_string(),
        ShapeKind::Table(_) => "Table".to_string(),
        ShapeKind::Chart(_) => "Chart".to_string(),
        ShapeKind::Media(m) if m.video => "Video".to_string(),
        ShapeKind::Media(_) => "Audio".to_string(),
        ShapeKind::Model3d { .. } => "3D Model".to_string(),
        _ if shape.text_box => "TextBox".to_string(),
        _ => shape
            .geom
            .preset_name()
            .and_then(deckcraft_geom::preset::info)
            .map(|i| i.label.split(':').next().unwrap_or(i.label).trim().to_string())
            .unwrap_or_else(|| "Shape".into()),
    }
}

/// "{base} n" with the first `n` from the shape count + 1 that no shape on the slide already uses,
/// so a gap left by a deleted shape never makes a second shape with the same name.
pub(crate) fn first_free_name(base: &str, taken: &std::collections::HashSet<String>) -> String {
    (taken.len() + 1..).map(|n| format!("{base} {n}")).find(|c| !taken.contains(c)).unwrap_or_default()
}

pub(crate) fn add_shape(s: &mut Session, cmd: &str, mut shape: Shape, select: bool) -> Result<ShapeId> {
    s.edit(|doc, sel| {
        shape.id = ShapeId(doc.alloc_id().ok_or_else(|| bad(cmd, NO_ID_SPACE))?);
        if shape.name.is_empty() {
            let base = auto_name_base(&shape);
            let taken = super::current_names(doc, sel);
            shape.name = first_free_name(&base, &taken);
        }
        let id = shape.id;
        let list = crate::shapes_mut(doc, sel).ok_or_else(|| bad("insert", "no slide"))?;
        list.push(shape);
        if select {
            sel.shapes = vec![id];
            sel.text = None;
        }
        Ok(id)
    })
}

fn insert_shape(s: &mut Session, p: &Value) -> Result<Value> {
    let preset = str_param(p, "preset").unwrap_or("rect");
    let info = deckcraft_geom::preset::info(preset).ok_or_else(|| bad("shape.insert", format!("unknown preset `{preset}`")))?;
    let line_like = deckcraft_geom::preset::is_line_like(preset);
    let rect = rect_param(p, "rect").unwrap_or_else(|| default_rect(s, 72.0, if line_like { 0.0 } else { 72.0 }));
    let adj: Vec<f64> = p.get("adj").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
    let mut shape = Shape {
        xfrm: Some(rect),
        geom: Geom::Preset { name: info.name.into(), adj },
        style: Some(if line_like { ShapeStyle::line(SchemeSlot::Accent1) } else { ShapeStyle::accent(SchemeSlot::Accent1) }),
        ..Default::default()
    };
    if let Some(fl) = p.get("flip").and_then(Value::as_str)
        && let Some(x) = shape.xfrm.as_mut()
    {
        x.flip_h = fl.contains('h');
        x.flip_v = fl.contains('v');
    }
    if !line_like {
        shape.text = Some(TextBody { paragraphs: vec![], ..Default::default() });
        if let Some(t) = str_param(p, "text") {
            shape.text = Some(TextBody::from_text(t));
        }
        if let Some(tb) = shape.text.as_mut() {
            for para in &mut tb.paragraphs {
                para.props.align = Some(deckcraft_model::text::Align::Center);
            }
        }
    } else {
        shape.kind = if preset.contains("Connector") { ShapeKind::Connector { start: None, end: None } } else { ShapeKind::Shape };
        if preset == "straightConnector1" {
            shape.line = Some(deckcraft_model::Line {
                tail: Some(deckcraft_model::style::LineEnd { kind: "triangle".into(), w: "med".into(), len: "med".into() }),
                ..Default::default()
            });
        }
    }
    if let (Some(look), false) = (&s.default_look, line_like) {
        super::format::apply_painted(&mut shape, look);
    }
    let id = add_shape(s, "shape.insert", shape, true)?;
    Ok(json!({"id": id}))
}

fn presets(_s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(Value::Array(
        deckcraft_geom::preset::CATALOG.iter().map(|i| json!({"name": i.name, "label": i.label, "category": i.category.label()})).collect(),
    ))
}

fn list_icons(_s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(Value::Array(crate::icons::icons().iter().map(|i| json!({"name": i.name, "label": i.label, "category": i.category})).collect()))
}

/// A Lucide stroke icon: custom geometry, no fill, round line scaled from the 24-unit design.
fn insert_icon(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("insert.icon", "missing `name`"))?;
    let icon = crate::icons::find(name).ok_or_else(|| bad("insert.icon", format!("unknown icon `{name}`")))?;
    let d = crate::icons::path_data(icon.name).ok_or_else(|| bad("insert.icon", format!("could not build icon `{}`", icon.name)))?;
    let rect = rect_param(p, "rect").unwrap_or_else(|| default_rect(s, 72.0, 72.0));
    if rect.w < 1.0 || rect.h < 1.0 {
        return Err(bad("insert.icon", "rect needs a positive width and height"));
    }
    let shape = Shape {
        name: icon.label.to_string(),
        xfrm: Some(rect),
        geom: Geom::Custom {
            paths: vec![deckcraft_model::CustomPath {
                w: crate::icons::VIEW,
                h: crate::icons::VIEW,
                d,
                fill: deckcraft_geom::preset::FillMode::None,
                stroke: true,
            }],
        },
        fill: Some(Fill::None),
        line: Some(Line {
            fill: Some(Fill::solid(ColorRef::scheme(SchemeSlot::Tx1))),
            width: Some(crate::icons::stroke_width(icon, rect.w, rect.h)),
            cap: Some(deckcraft_model::style::LineCap::Round),
            join: Some(deckcraft_model::style::LineJoin::Round),
            ..Default::default()
        }),
        style: Some(ShapeStyle::line(SchemeSlot::Tx1)),
        ..Default::default()
    };
    let id = add_shape(s, "insert.icon", shape, true)?;
    Ok(json!({"id": id}))
}

pub(crate) fn new_text_box(rect: Xfrm, text: &str) -> Shape {
    let mut body = TextBody::from_text(text);
    body.body.wrap = Some(true);
    body.body.autofit = Some(AutoFit::Shape);
    Shape { xfrm: Some(rect), text_box: true, fill: None, text: Some(body), geom: Geom::preset("rect"), ..Default::default() }
}

fn text_box(s: &mut Session, p: &Value) -> Result<Value> {
    let text = str_param(p, "text").unwrap_or("");
    let rect = rect_param(p, "rect").unwrap_or_else(|| default_rect(s, 216.0, 36.0));
    let mut shape = new_text_box(rect, text);
    // Click-to-create (no width) text boxes grow sideways without wrapping.
    if rect.w < 1.0 {
        shape.xfrm = Some(Xfrm { w: 100.0, ..rect });
        if let Some(t) = shape.text.as_mut() {
            t.body.wrap = Some(false);
        }
    }
    let id = add_shape(s, "insert.textBox", shape, true)?;
    super::text::fit_text_box(s, id)?;
    if text.is_empty() {
        // Start typing immediately.
        s.select(|_, sel| {
            sel.shapes = vec![id];
            sel.text = Some(crate::TextSel { shape: id, ..Default::default() });
        })?;
    }
    Ok(json!({"id": id}))
}

fn table(s: &mut Session, p: &Value) -> Result<Value> {
    let rows = usize_param(p, "rows").unwrap_or(2).clamp(1, 75);
    let cols = usize_param(p, "cols").unwrap_or(2).clamp(1, 75);
    let size = s.doc()?.doc.slide_size;
    let w = size.width * 0.8;
    let rh = 40.0;
    let rect = rect_param(p, "rect").unwrap_or(Xfrm::new(size.width * 0.1, (size.height - rh * rows as f64).max(0.0) / 2.0, w, rh * rows as f64));
    let mut t = Table::new(rows, cols, rect.w, rect.h / rows as f64);
    if let Some(data) = p.get("data").and_then(Value::as_array) {
        for (r, row) in data.iter().enumerate() {
            for (c, v) in row.as_array().map(|a| a.as_slice()).unwrap_or(&[]).iter().enumerate() {
                if let (Some(cell), Some(text)) = (t.cell_mut(r, c), v.as_str()) {
                    cell.text = TextBody::from_text(text);
                }
            }
        }
    }
    let id = add_shape(s, "insert.table", Shape { xfrm: Some(rect), kind: ShapeKind::Table(t), ..Default::default() }, true)?;
    Ok(json!({"id": id}))
}

/// A SmartArt graphic: a group regenerated from its outline (see [`super::smartart`]). Items with
/// leading tabs are sub-points; `nodes` (`[{text, level}]`) may be given instead.
fn smart_art(s: &mut Session, p: &Value) -> Result<Value> {
    let kind = str_param(p, "kind").unwrap_or("process").to_string();
    let nodes = match p.get("nodes") {
        Some(v) => serde_json::from_value(v.clone()).map_err(|e| bad("insert.smartArt", e.to_string()))?,
        None => super::smartart::nodes_from_lines(p.get("items").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str)),
    };
    super::smartart::insert(s, &kind, nodes)
}

#[cfg(test)]
mod tests {
    use deckcraft_model::text::RunKind;
    use serde_json::json;

    use crate::Session;

    fn session() -> Session {
        Session::with_new()
    }

    fn undo(s: &Session) -> usize {
        s.doc().unwrap().history.undo.len()
    }

    fn date_run(s: &Session) -> Option<deckcraft_model::Run> {
        s.doc()
            .unwrap()
            .current_slide()
            .unwrap()
            .shapes
            .iter()
            .find(|x| x.ph_type() == Some(deckcraft_model::PhType::Date))
            .and_then(|x| x.text.as_ref())
            .and_then(|t| t.paragraphs.first())
            .and_then(|p| p.runs.first())
            .cloned()
    }

    fn title_field(s: &Session) -> deckcraft_model::Run {
        let title = s.doc().unwrap().current_slide().unwrap().shapes[0].text.clone().unwrap();
        title.paragraphs[0].runs.iter().find(|r| r.text.contains(':') || matches!(r.kind, RunKind::Field { .. })).cloned().unwrap()
    }

    fn edit_title(s: &mut Session) {
        let id = s.doc().unwrap().current_slide().unwrap().shapes[0].id.0;
        s.execute("text.edit", &json!({"id": id})).unwrap();
        s.execute("text.insert", &json!({"text": "On "})).unwrap();
    }

    #[test]
    fn no_params_opens_the_dialog_and_edits_nothing() {
        let mut s = session();
        let rev = s.doc().unwrap().revision;
        for p in [json!({}), serde_json::Value::Null] {
            s.ui_requests.clear();
            s.execute("insert.dateTime", &p).unwrap();
            assert!(matches!(&s.ui_requests[..], [crate::UiRequest::Dialog { id, .. }] if id == "dateTime"));
            assert_eq!(s.doc().unwrap().revision, rev);
            assert_eq!(undo(&s), 0);
            assert!(date_run(&s).is_none());
        }
    }

    #[test]
    fn a_format_at_the_caret_is_one_updating_field() {
        let mut s = session();
        edit_title(&mut s);
        let before = undo(&s);
        let v = s.execute("insert.dateTime", &json!({"format": "datetime10", "update": true})).unwrap();
        assert_eq!(v["where"], "caret");
        let run = title_field(&s);
        assert!(matches!(run.kind, RunKind::Field { ref field } if field == "datetime10"), "{run:?}");
        assert!(run.text.contains(':'), "{run:?}");
        assert_eq!(undo(&s), before + 1);
        assert_eq!(s.doc().unwrap().history.undo.last().unwrap().label, "Date & Time");
        s.execute("edit.undo", &json!({})).unwrap();
        let title = s.doc().unwrap().current_slide().unwrap().shapes[0].text.as_ref().unwrap().text();
        assert_eq!(title, "On ");
    }

    #[test]
    fn update_false_inserts_plain_text_in_the_chosen_language() {
        let mut s = session();
        edit_title(&mut s);
        let before = undo(&s);
        let now = deckcraft_text::datetime::DateTime::now_local().unwrap();
        let expect = deckcraft_text::datetime::field_text("datetime1", Some("en-GB"), now).unwrap();
        s.execute("insert.dateTime", &json!({"format": "datetime1", "update": false, "language": "en-GB"})).unwrap();
        let title = s.doc().unwrap().current_slide().unwrap().shapes[0].text.clone().unwrap();
        let run = title.paragraphs[0].runs.iter().find(|r| r.text == expect).cloned().unwrap();
        assert_eq!(run.kind, RunKind::Text);
        assert_eq!(run.props.lang.as_deref(), Some("en-GB"));
        assert_eq!(undo(&s), before + 1);
    }

    #[test]
    fn a_named_format_in_french_and_a_bad_value_change_nothing() {
        let mut s = session();
        edit_title(&mut s);
        let rev = s.doc().unwrap().revision;
        let undos = undo(&s);
        let err = s.execute("insert.dateTime", &json!({"format": "datetime2", "language": "fr-FR"})).unwrap_err();
        assert!(err.to_string().contains("isn't available"), "{err}");
        assert_eq!(s.doc().unwrap().revision, rev);
        assert_eq!(undo(&s), undos);
        for p in [
            json!({"format": "datetime14"}),
            json!({"format": "nope"}),
            json!({"update": "yes"}),
            json!({"language": "de-DE"}),
            json!({"language": ""}),
        ] {
            let err = s.execute("insert.dateTime", &p).unwrap_err();
            assert!(err.to_string().contains("insert.dateTime"), "{err}");
            assert_eq!(s.doc().unwrap().revision, rev);
            assert_eq!(undo(&s), undos);
        }
    }

    #[test]
    fn without_a_caret_the_date_lands_on_this_slide_only() {
        let mut s = session();
        s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
        s.execute("slide.go", &json!({"index": 0})).unwrap();
        let now = deckcraft_text::datetime::DateTime::now_local().unwrap();
        let clock = deckcraft_text::datetime::field_text("datetime10", Some("en-US"), now).unwrap();
        let v = s.execute("insert.dateTime", &json!({"format": "datetime10", "language": "en_US"})).unwrap();
        assert_eq!(v["where"], "footer");
        let run = date_run(&s).unwrap();
        assert!(matches!(run.kind, RunKind::Field { ref field } if field == "datetime10"), "{run:?}");
        assert_eq!(run.text, clock);
        assert_eq!(run.props.lang.as_deref(), Some("en-US"));
        s.execute("slide.go", &json!({"index": 1})).unwrap();
        assert!(date_run(&s).is_none(), "the other slide keeps its own footer");
        s.execute("slide.go", &json!({"index": 0})).unwrap();
        let fixed = deckcraft_text::datetime::field_text("datetime12", None, now).unwrap();
        s.execute("insert.dateTime", &json!({"format": "datetime12", "update": false})).unwrap();
        let run = date_run(&s).unwrap();
        assert_eq!((run.kind, run.text), (RunKind::Text, fixed));
        s.execute("edit.undo", &json!({})).unwrap();
        let run = date_run(&s).unwrap();
        assert!(matches!(run.kind, RunKind::Field { .. }), "one undo restores the field");
    }
}

#[cfg(test)]
mod icon_and_picture_tests;
