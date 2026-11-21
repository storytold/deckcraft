//! Insert ▸ Zoom: Summary Zoom, Section Zoom and Slide Zoom.
//!
//! A zoom is a picture of the target slide that jumps to it in the slide show. With Return to
//! Zoom (on for summary and section zooms, as in PowerPoint) the show comes back to the zoom
//! slide after the target slide, or the target section's last slide. In .pptx a zoom is saved as
//! the linked picture PowerPoint itself writes as the fallback for older versions.

use std::sync::Arc;

use deckcraft_geom::Xfrm;
use deckcraft_model::style::PictureFill;
use deckcraft_model::text::{Action, Hyperlink, TextBody};
use deckcraft_model::{LayoutType, Shape, ShapeId, ShapeKind, SlideId, defaults};
use serde_json::{Value, json};

use super::{CommandSpec, NO_ID_SPACE, bad, bool_or, cmd, has_slide, str_param};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        "insert.zoom",
        "Zoom",
        ["Insert", "Links"],
        None,
        "{kind?: slide|section|summary (default slide); slides?: [index] (slide and summary zooms); sections?: [index] (section zoom); returnToZoom?: bool (default: on for section and summary, off for slide)}",
        has_slide,
        zoom
    )]
}

/// One zoom tile: the slide it shows and jumps to, and the slide after which the show returns.
struct Target {
    slide: SlideId,
    index: usize,
    last: SlideId,
}

/// `n` tiles of `aspect` (w / h) laid out in a centred grid inside `area`.
fn grid(n: usize, area: Xfrm, aspect: f64) -> Vec<Xfrm> {
    let cols = (n as f64).sqrt().ceil().max(1.0) as usize;
    let rows = n.div_ceil(cols).max(1);
    let gap = 12.0;
    let cw = (area.w - gap * (cols as f64 - 1.0)) / cols as f64;
    let ch = (area.h - gap * (rows as f64 - 1.0)) / rows as f64;
    let (w, h) = if cw / ch > aspect { (ch * aspect, ch) } else { (cw, cw / aspect) };
    // A single slide zoom is a quarter of the slide wide, as PowerPoint inserts it.
    let (w, h) = if n == 1 { (w.min(area.w / 2.0), h.min(area.w / 2.0 / aspect)) } else { (w, h) };
    let total_w = cols.min(n) as f64 * w + gap * (cols.min(n) as f64 - 1.0);
    let total_h = rows as f64 * h + gap * (rows as f64 - 1.0);
    let (x0, y0) = (area.x + (area.w - total_w) / 2.0, area.y + (area.h - total_h) / 2.0);
    (0..n).map(|k| Xfrm::new(x0 + (k % cols) as f64 * (w + gap), y0 + (k / cols) as f64 * (h + gap), w, h)).collect()
}

fn indices(p: &Value, key: &str, n: usize, what: &str) -> Result<Vec<usize>> {
    // A malformed explicit selection must not become Summary Zoom's section defaults.
    let a = match p.get(key) {
        None | Some(Value::Null) => return Ok(vec![]),
        Some(Value::Array(a)) => a,
        Some(_) => return Err(bad("insert.zoom", format!("`{key}` must be a list of {what} indices (0-based)"))),
    };
    let mut v = vec![];
    for x in a {
        let i = x
            .as_u64()
            .and_then(|i| usize::try_from(i).ok())
            .filter(|i| *i < n)
            .ok_or_else(|| bad("insert.zoom", format!("there is no {what} {x} (indices count from 0)")))?;
        if !v.contains(&i) {
            v.push(i);
        }
    }
    Ok(v)
}

fn zoom(s: &mut Session, p: &Value) -> Result<Value> {
    let kind = str_param(p, "kind").unwrap_or("slide");
    let st = s.doc()?;
    let doc = &st.doc;
    let n = doc.slides.len();
    let targets: Vec<Target> = match kind {
        "slide" => indices(p, "slides", n, "slide")?
            .into_iter()
            .filter_map(|i| doc.slides.get(i).map(|sl| Target { slide: sl.id, index: i, last: sl.id }))
            .collect(),
        "section" => {
            let secs = indices(p, "sections", doc.sections.len(), "section")?;
            secs.into_iter()
                .filter_map(|k| {
                    let sec = doc.sections.get(k)?;
                    let first = *sec.slides.first()?;
                    let last = *sec.slides.last()?;
                    Some(Target { slide: first, index: doc.slide_index(first)?, last })
                })
                .collect()
        }
        "summary" => {
            let mut starts = indices(p, "slides", n, "slide")?;
            if starts.is_empty() {
                // Default: the first slide of every section, or every slide's start when there are none.
                starts = doc.sections.iter().filter_map(|sec| sec.slides.first().and_then(|id| doc.slide_index(*id))).collect();
            }
            starts.sort_unstable();
            starts
                .iter()
                .enumerate()
                .filter_map(|(k, &i)| {
                    // Each part runs to the slide before the next part (or the end).
                    let end = starts.get(k + 1).map(|j| j.saturating_sub(1)).unwrap_or(n.saturating_sub(1)).max(i);
                    Some(Target { slide: doc.slides.get(i)?.id, index: i, last: doc.slides.get(end)?.id })
                })
                .collect()
        }
        other => return Err(bad("insert.zoom", format!("unknown zoom kind `{other}` (slide, section or summary)"))),
    };
    if targets.is_empty() {
        return Err(bad("insert.zoom", "choose the slides (or sections) to zoom to"));
    }
    let back = bool_or(p, "returnToZoom", kind != "slide");
    let size = doc.slide_size;
    // Pictures of the targets, about 480 px wide.
    let scale = (480.0 / size.width.max(1.0)).clamp(0.05, 4.0);
    let pictures: Vec<(Vec<u8>, String)> = targets
        .iter()
        .map(|t| {
            let (png, _, _) = super::file::render_png(doc, t.index, scale, false);
            let title = doc.slides.get(t.index).map(|sl| sl.title()).unwrap_or_default();
            let label =
                if title.trim().is_empty() { format!("Slide {}", t.index + 1) } else { title.lines().next().unwrap_or("").trim().to_string() };
            (png, label)
        })
        .collect();
    let summary = kind == "summary";
    let first_target = targets.iter().map(|t| t.index).min().unwrap_or(0);
    s.edit(|doc, sel| {
        let aspect = size.width / size.height.max(1.0);
        let area = if summary {
            Xfrm::new(size.width * 0.06, size.height * 0.26, size.width * 0.88, size.height * 0.66)
        } else if targets.len() == 1 {
            Xfrm::new(size.width * 0.5, size.height * 0.5, size.width * 0.45, size.height * 0.45)
        } else {
            Xfrm::new(size.width * 0.08, size.height * 0.12, size.width * 0.84, size.height * 0.8)
        };
        let boxes = grid(targets.len(), area, aspect);
        let mut shapes = vec![];
        for ((t, (png, label)), b) in targets.iter().zip(pictures).zip(boxes) {
            let media = doc.add_media(&format!("zoom-{}.png", t.index + 1), "image/png", png).ok_or_else(|| bad("insert.zoom", NO_ID_SPACE))?;
            let id = ShapeId(doc.alloc_id().ok_or_else(|| bad("insert.zoom", NO_ID_SPACE))?);
            shapes.push(Shape {
                id,
                name: format!(
                    "{} Zoom {}",
                    if summary {
                        "Summary"
                    } else if kind == "section" {
                        "Section"
                    } else {
                        "Slide"
                    },
                    t.index + 1
                ),
                descr: label.clone(),
                xfrm: Some(b),
                kind: ShapeKind::Picture { fill: PictureFill { media, ..Default::default() } },
                click: Some(Hyperlink {
                    action: Action::Slide { slide: t.slide },
                    tooltip: label,
                    highlight_click: false,
                    zoom_return: back.then_some(t.last),
                    sound: None,
                    stop_sound: false,
                }),
                ..Default::default()
            });
        }
        let ids: Vec<ShapeId> = shapes.iter().map(|x| x.id).collect();
        if summary {
            // A new Title Only slide before the first part, as PowerPoint makes it.
            let layout =
                defaults::layout_of_kind(doc, LayoutType::TitleOnly).or_else(|| doc.slides.get(first_target).map(|x| x.layout)).unwrap_or_default();
            let mut slide = defaults::new_slide(doc, layout).ok_or_else(|| bad("insert.zoom", NO_ID_SPACE))?;
            if let Some(sh) = slide.shapes.iter_mut().find(|x| x.ph_type().is_some_and(|k| k.is_title())) {
                sh.text = Some(TextBody::from_text("Summary"));
            }
            slide.shapes.extend(shapes);
            let at = first_target.min(doc.slides.len());
            doc.slides.insert(at, Arc::new(slide));
            sel.slide = at;
            sel.slides.clear();
        } else {
            let list = crate::shapes_mut(doc, sel).ok_or_else(|| bad("insert.zoom", "no slide"))?;
            list.extend(shapes);
        }
        sel.shapes = ids.clone();
        sel.text = None;
        Ok(json!({"kind": kind, "ids": ids, "returnToZoom": back, "slide": sel.slide}))
    })
}

#[cfg(test)]
mod target_tests {
    use super::indices;
    use serde_json::{Value, json};

    #[test]
    fn target_lists_preserve_optional_defaults_order_and_deduplication() {
        for key in ["slides", "sections"] {
            assert_eq!(indices(&json!({}), key, 3, "target").unwrap(), Vec::<usize>::new());
            let mut params = json!({});
            for value in [Value::Null, json!([])] {
                params[key] = value;
                assert_eq!(indices(&params, key, 3, "target").unwrap(), Vec::<usize>::new());
            }
            params[key] = json!([2, 0, 2, 1]);
            assert_eq!(indices(&params, key, 3, "target").unwrap(), [2, 0, 1]);
        }
    }

    #[test]
    fn target_lists_reject_wrong_outer_types_and_invalid_members() {
        for key in ["slides", "sections"] {
            let mut params = json!({});
            for value in [json!(false), json!(0), json!("0"), json!({})] {
                params[key] = value;
                let err = indices(&params, key, 3, "target").unwrap_err().to_string();
                assert!(err.contains(key), "{err}");
            }
            for value in [
                json!(-1),
                json!(1.5),
                Value::Null,
                json!(true),
                json!("1"),
                json!({}),
                json!([]),
                json!(3),
                json!(4_294_967_296u64),
                json!(u64::MAX),
            ] {
                params[key] = json!([0, value, 1]);
                assert!(indices(&params, key, 3, "target").is_err(), "{key}: {params}");
            }
        }
    }

    #[test]
    fn malformed_summary_targets_do_not_change_the_presentation() {
        let mut s = crate::Session::with_new();
        s.execute("section.add", &json!({"name": "Part one", "at": 0})).unwrap();
        let before = s.doc().unwrap().doc.clone();
        let history = (s.doc().unwrap().history.undo.len(), s.doc().unwrap().history.redo.len());
        let revision = s.doc().unwrap().revision;
        for slides in [json!(false), json!(0), json!("0"), json!({})] {
            assert!(s.execute("insert.zoom", &json!({"kind": "summary", "slides": slides})).is_err(), "{slides}");
            let st = s.doc().unwrap();
            assert_eq!(st.doc, before, "{slides}");
            assert_eq!((st.history.undo.len(), st.history.redo.len()), history, "{slides}");
            assert_eq!(st.revision, revision, "{slides}");
        }
    }
}
