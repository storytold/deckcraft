//! Follow a hyperlink in the editor (Ctrl+Click, or Open Link on the context menu): a web or mail
//! address goes to the UI host to open, a link to a slide selects that slide.

use deckcraft_geom::Point;
use deckcraft_model::text::{Action, Hyperlink, TextBody, safe_url};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_slide};
use crate::{DocState, Result, Session, Target, UiRequest};

const CMD: &str = "link.follow";

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        noundo "link.follow",
        "Open Link",
        [],
        None,
        "{at?: [x, y]} → {url} | {slide}; follows the link on the text or shape at that slide point, or (without `at`) at the text caret or on the one selected shape. A web or mail address is handed to the app to open; a link to a slide selects it",
        has_slide,
        follow
    )]
}

fn follow(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let link = match p.get("at") {
        Some(at) => {
            let xy = at.as_array().and_then(|v| Some((v.first()?.as_f64()?, v.get(1)?.as_f64()?)));
            let Some((x, y)) = xy.filter(|(x, y)| x.is_finite() && y.is_finite()) else {
                return Err(bad(CMD, "`at` must be [x, y] in slide points"));
            };
            link_at(st, Point::new(x, y))
        }
        None => selected_link(st),
    };
    let Some(link) = link else { return Err(bad(CMD, "there is no link here")) };
    let n = st.doc.slides.len();
    let cur = st.selection.slide;
    let slide = match &link.action {
        Action::Url { url } => {
            if !safe_url(url) {
                return Err(bad(CMD, "only web (http, https) and mail links can be opened"));
            }
            let url = url.trim().to_string();
            s.ui_requests.push(UiRequest::OpenUrl { url: url.clone() });
            return Ok(json!({"url": url}));
        }
        Action::Slide { slide } => {
            st.doc.slide_index(*slide).ok_or_else(|| bad(CMD, "the slide this link goes to is no longer in the presentation"))?
        }
        Action::NextSlide => (cur + 1).min(n.saturating_sub(1)),
        Action::PreviousSlide => cur.saturating_sub(1),
        Action::FirstSlide => 0,
        Action::LastSlide => n.saturating_sub(1),
        Action::LastViewed | Action::EndShow | Action::CustomShow { .. } | Action::Program { .. } | Action::PlayMedia => {
            return Err(bad(CMD, "this link only works in a slide show"));
        }
    };
    s.execute("slide.go", &json!({"index": slide}))?;
    Ok(json!({"slide": slide}))
}

/// The link at slide point `p`, on the topmost of the slide's shapes there: a link on its text
/// at `p`, else its own click link or that of a group it is in. A shape drawn over a link hides
/// it. Where no slide shape is under `p`, a link on the layout or master can apply. Only on a
/// slide (not while editing a master or layout).
pub(crate) fn link_at(st: &DocState, p: Point) -> Option<Hyperlink> {
    if !matches!(st.selection.target, Target::Slides) {
        return None;
    }
    let placed = deckcraft_render::place_slide(&st.doc, st.selection.slide);
    let inside = |r: &deckcraft_geom::Rect| r.x0 <= p.x && p.x <= r.x1 && r.y0 <= p.y && p.y <= r.y1;
    let link_of = |id, text: bool| placed.links.iter().rev().find(|l| l.shape == id && l.text == text && inside(&l.rect)).map(|l| l.link.clone());
    let Some(top) = placed.shapes.iter().rposition(|s| inside(&s.rect)) else {
        // Nothing of the slide's own here: the layout's or master's links (their shapes aren't
        // in `placed.shapes`).
        let own = |id| placed.shapes.iter().any(|s| s.id == id);
        let on = |text: bool| placed.links.iter().rev().find(|l| l.text == text && !own(l.shape) && inside(&l.rect)).map(|l| l.link.clone());
        return on(true).or_else(|| on(false));
    };
    if let Some(l) = link_of(placed.shapes[top].id, true) {
        return Some(l);
    }
    // The shape's own click link, else the nearest group around it that has one.
    let mut at = Some(top);
    for _ in 0..=placed.shapes.len() {
        let s = &placed.shapes[at?];
        if let Some(l) = link_of(s.id, false) {
            return Some(l);
        }
        at = s.parent;
    }
    None
}

/// The link at the text caret (or covering the selected text), else the click link of the one
/// selected shape: what Open Link on the context menu follows.
pub fn selected_link(st: &DocState) -> Option<Hyperlink> {
    if let Some(t) = &st.selection.text {
        // A link on the text wins; unlinked text falls back to its shape's own link.
        let (a, b) = if t.anchor <= t.caret { (t.anchor, t.caret) } else { (t.caret, t.anchor) };
        if let Some(l) = super::text::body_of(st, t).and_then(|body| link_in(body, a, b)) {
            return Some(l);
        }
        return if t.notes { None } else { st.shape(t.shape).and_then(|sh| sh.click.clone()) };
    }
    match st.selected_shapes().as_slice() {
        [one] => one.click.clone(),
        _ => None,
    }
}

/// The link on the text from `a` to `b` (paragraph, char): the first linked run the span
/// touches. A caret (`a == b`) takes the character after it, or the one before at the end of a run.
fn link_in(body: &TextBody, a: (usize, usize), b: (usize, usize)) -> Option<Hyperlink> {
    let caret = a == b;
    for (pi, para) in body.paragraphs.iter().enumerate().skip(a.0).take(b.0.saturating_sub(a.0) + 1) {
        let from = if pi == a.0 { a.1 } else { 0 };
        let to = if pi == b.0 { b.1 } else { usize::MAX };
        let mut acc = 0usize;
        for run in &para.runs {
            let end = acc + run.char_len();
            // A selection takes only runs it covers part of: one ending at the start of a
            // paragraph takes nothing from that paragraph.
            let touches = if caret { acc <= from && from <= end && end > acc } else { end > from && acc < to };
            if touches && let Some(l) = &run.props.link {
                return Some(l.clone());
            }
            acc = end;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    /// Two slides, the first selected, with a text box whose second word links to the action
    /// `action` makes from the deck.
    fn deck(action: impl FnOnce(&deckcraft_model::Presentation) -> Action) -> (Session, deckcraft_model::ShapeId) {
        let mut s = Session::with_new();
        s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
        s.execute("slide.go", &json!({"index": 0})).unwrap();
        let action = action(&s.doc().unwrap().doc);
        let r = s.execute("shape.insert", &json!({"preset": "rect", "rect": [100.0, 100.0, 400.0, 100.0], "text": "go there"})).unwrap();
        let id = deckcraft_model::ShapeId(r["id"].as_u64().unwrap() as u32);
        s.edit(|doc, sel| {
            let shapes = crate::shapes_mut(doc, sel).unwrap();
            let sh = deckcraft_model::find_shape_mut(shapes, id).unwrap();
            let para = &mut sh.text.as_mut().unwrap().paragraphs[0];
            let mut linked = para.runs[0].clone();
            para.runs[0].text = "go ".into();
            linked.text = "there".into();
            linked.props.link =
                Some(Hyperlink { action, tooltip: String::new(), highlight_click: false, sound: None, stop_sound: false, zoom_return: None });
            para.runs.push(linked);
            Ok(())
        })
        .unwrap();
        (s, id)
    }

    /// The centre of the linked word, from the same layout the show clicks on.
    fn link_point(s: &Session) -> [f64; 2] {
        let st = s.doc().unwrap();
        let placed = deckcraft_render::place_slide(&st.doc, st.selection.slide);
        let r = placed.links.iter().find(|l| l.text).expect("a placed text link").rect;
        [(r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0]
    }

    #[test]
    fn a_web_link_at_a_point_is_handed_to_the_app_and_keeps_the_selection() {
        let (mut s, id) = deck(|_| Action::Url { url: "https://example.com/".into() });
        s.select(|_, sel| sel.shapes = vec![id]).unwrap();
        let before = s.doc().unwrap().selection.shapes.clone();
        let at = link_point(&s);
        let r = s.execute("link.follow", &json!({"at": at})).unwrap();
        assert_eq!(r["url"], "https://example.com/");
        assert!(s.ui_requests.iter().any(|u| matches!(u, UiRequest::OpenUrl { url } if url == "https://example.com/")));
        assert_eq!(s.doc().unwrap().selection.shapes, before, "following a link leaves the selection alone");
    }

    #[test]
    fn a_link_to_a_slide_selects_that_slide() {
        let (mut s, _) = deck(|doc| Action::Slide { slide: doc.slides[1].id });
        let at = link_point(&s);
        let r = s.execute("link.follow", &json!({"at": at})).unwrap();
        assert_eq!(r["slide"], 1);
        assert_eq!(s.doc().unwrap().selection.slide, 1, "the editor moved to the linked slide");
        assert!(s.ui_requests.iter().all(|u| !matches!(u, UiRequest::OpenUrl { .. })));
    }

    #[test]
    fn a_shape_link_is_followed_from_the_selection() {
        let (mut s, id) = deck(|_| Action::Url { url: "https://example.com/".into() });
        s.edit(|doc, sel| {
            let sh = deckcraft_model::find_shape_mut(crate::shapes_mut(doc, sel).unwrap(), id).unwrap();
            sh.text = None;
            sh.click = Some(Hyperlink {
                action: Action::Url { url: "mailto:someone@example.com".into() },
                tooltip: String::new(),
                highlight_click: false,
                sound: None,
                stop_sound: false,
                zoom_return: None,
            });
            Ok(())
        })
        .unwrap();
        s.select(|_, sel| sel.shapes = vec![id]).unwrap();
        let r = s.execute("link.follow", &json!({})).unwrap();
        assert_eq!(r["url"], "mailto:someone@example.com");
    }

    #[test]
    fn unsafe_and_missing_links_are_refused() {
        let (mut s, _) = deck(|_| Action::Url { url: "javascript:alert(1)".into() });
        let at = link_point(&s);
        assert!(s.execute("link.follow", &json!({"at": at})).is_err(), "a javascript: link is not opened");
        assert!(s.ui_requests.iter().all(|u| !matches!(u, UiRequest::OpenUrl { .. })));
        assert!(s.execute("link.follow", &json!({"at": [5.0, 5.0]})).is_err(), "no link at an empty spot");
    }

    #[test]
    fn ctrl_click_follows_a_link_and_elsewhere_toggles_the_selection() {
        let (mut s, id) = deck(|_| Action::Url { url: "https://example.com/".into() });
        let press = |s: &mut Session, at: [f64; 2]| {
            let mods = crate::Mods { cmd: true, ..Default::default() };
            let ev = |kind| crate::PointerEvent { kind, x: at[0], y: at[1], mods, ..Default::default() };
            s.pointer(ev(crate::PointerKind::Down)).unwrap();
            s.pointer(ev(crate::PointerKind::Up)).unwrap();
        };
        s.select(|_, sel| sel.shapes.clear()).unwrap();
        let at = link_point(&s);
        press(&mut s, at);
        assert!(s.ui_requests.iter().any(|u| matches!(u, UiRequest::OpenUrl { .. })), "Ctrl+Click on link text opens it");
        assert!(s.doc().unwrap().selection.shapes.is_empty(), "and does not select the shape");
        // The unlinked word of the same box: Ctrl+Click selects as before.
        s.ui_requests.clear();
        let st = s.doc().unwrap();
        let placed = deckcraft_render::place_slide(&st.doc, 0);
        let word = placed.links.iter().find(|l| l.text).unwrap().rect;
        let box_ = placed.shapes.iter().find(|p| p.id == id).unwrap().rect;
        press(&mut s, [(box_.x0 + word.x0) / 2.0, (word.y0 + word.y1) / 2.0]);
        assert!(s.ui_requests.iter().all(|u| !matches!(u, UiRequest::OpenUrl { .. })), "no link there");
        assert_eq!(s.doc().unwrap().selection.shapes, vec![id], "Ctrl+Click off the link adds the shape to the selection");
    }

    #[test]
    fn a_right_click_on_a_linked_word_offers_its_link() {
        let (mut s, id) = deck(|_| Action::Url { url: "https://example.com/".into() });
        let right_click = |s: &mut Session, at: [f64; 2]| {
            s.pointer(crate::PointerEvent { kind: crate::PointerKind::ContextClick, x: at[0], y: at[1], ..Default::default() }).unwrap();
        };
        let at = link_point(&s);
        right_click(&mut s, at);
        assert_eq!(s.tool.context_link.map(|p| [p.x, p.y]), Some(at), "the linked word under the right-click, nothing selected first");
        assert!(selected_link(s.doc().unwrap()).is_none(), "the selection alone would not have offered it");
        s.execute("link.follow", &json!({"at": at})).unwrap();
        assert!(s.ui_requests.iter().any(|u| matches!(u, UiRequest::OpenUrl { .. })));
        // The unlinked word of the same box: no link to offer.
        let st = s.doc().unwrap();
        let placed = deckcraft_render::place_slide(&st.doc, 0);
        let word = placed.links.iter().find(|l| l.text).unwrap().rect;
        let box_ = placed.shapes.iter().find(|p| p.id == id).unwrap().rect;
        right_click(&mut s, [(box_.x0 + word.x0) / 2.0, (word.y0 + word.y1) / 2.0]);
        assert!(s.tool.context_link.is_none(), "no link off the linked word");
        // With another tool, a right-click doesn't keep an earlier one's link.
        right_click(&mut s, at);
        s.set_tool(crate::ToolKind::TextBox);
        right_click(&mut s, [5.0, 5.0]);
        assert!(s.tool.context_link.is_none(), "a right-click with another tool clears the link");
    }

    fn url_link(url: &str) -> Hyperlink {
        Hyperlink {
            action: Action::Url { url: url.into() },
            tooltip: String::new(),
            highlight_click: false,
            sound: None,
            stop_sound: false,
            zoom_return: None,
        }
    }

    #[test]
    fn a_shape_drawn_over_link_text_hides_it() {
        let (mut s, _) = deck(|_| Action::Url { url: "https://example.com/".into() });
        let at = link_point(&s);
        // An unlinked shape over the whole text box: nothing to follow there.
        let cover = s.execute("shape.insert", &json!({"preset": "rect", "rect": [90.0, 90.0, 420.0, 120.0]})).unwrap();
        let cover = deckcraft_model::ShapeId(cover["id"].as_u64().unwrap() as u32);
        assert!(link_at(s.doc().unwrap(), Point::new(at[0], at[1])).is_none(), "the covered link is not followed");
        // With its own link, the covering shape's link is the one there.
        s.edit(|doc, sel| {
            deckcraft_model::find_shape_mut(crate::shapes_mut(doc, sel).unwrap(), cover).unwrap().click = Some(url_link("https://cover.example/"));
            Ok(())
        })
        .unwrap();
        let found = link_at(s.doc().unwrap(), Point::new(at[0], at[1])).map(|l| l.action);
        assert_eq!(found, Some(Action::Url { url: "https://cover.example/".into() }));
    }

    #[test]
    fn unlinked_text_being_edited_falls_back_to_its_shapes_link() {
        let (mut s, id) = deck(|_| Action::Url { url: "https://text.example/".into() });
        s.edit(|doc, sel| {
            deckcraft_model::find_shape_mut(crate::shapes_mut(doc, sel).unwrap(), id).unwrap().click = Some(url_link("https://shape.example/"));
            Ok(())
        })
        .unwrap();
        // Caret inside "go " (unlinked), then inside "there" (linked).
        s.execute("text.edit", &json!({"id": id.0, "at": [0, 1]})).unwrap();
        assert_eq!(selected_link(s.doc().unwrap()).map(|l| l.action), Some(Action::Url { url: "https://shape.example/".into() }));
        s.execute("text.edit", &json!({"id": id.0, "at": [0, 5]})).unwrap();
        assert_eq!(selected_link(s.doc().unwrap()).map(|l| l.action), Some(Action::Url { url: "https://text.example/".into() }));
    }

    #[test]
    fn a_selection_ending_at_a_paragraph_start_takes_no_link_from_it() {
        let (s, id) = deck(|_| Action::Url { url: "https://example.com/".into() });
        let st = s.doc().unwrap();
        let mut body = st.shape(id).unwrap().text.clone().unwrap();
        // Paragraph 0 plain, paragraph 1 starting with the linked word.
        let mut second = body.paragraphs[0].clone();
        second.runs.remove(0);
        body.paragraphs[0].runs.truncate(1);
        body.paragraphs.push(second);
        assert!(link_in(&body, (0, 1), (1, 0)).is_none(), "the selection covers none of paragraph 1");
        assert!(link_in(&body, (0, 1), (1, 1)).is_some(), "one character of the link is covered");
        assert!(link_in(&body, (1, 0), (1, 0)).is_some(), "a caret at the link's start takes it");
    }

    #[test]
    fn ctrl_click_in_edit_points_edits_points_rather_than_following_the_link() {
        let (mut s, id) = deck(|_| Action::Url { url: "https://example.com/".into() });
        s.edit(|doc, sel| {
            let sh = deckcraft_model::find_shape_mut(crate::shapes_mut(doc, sel).unwrap(), id).unwrap();
            sh.text = None;
            sh.click = Some(url_link("https://example.com/"));
            Ok(())
        })
        .unwrap();
        s.select(|_, sel| sel.shapes = vec![id]).unwrap();
        s.execute("shape.editPoints", &json!({"on": true})).unwrap();
        let mods = crate::Mods { cmd: true, ..Default::default() };
        // The rectangle's top-left corner is one of its points.
        s.pointer(crate::PointerEvent { kind: crate::PointerKind::Down, x: 100.0, y: 100.0, mods, ..Default::default() }).unwrap();
        s.pointer(crate::PointerEvent { kind: crate::PointerKind::Up, x: 100.0, y: 100.0, mods, ..Default::default() }).unwrap();
        assert!(s.ui_requests.iter().all(|u| !matches!(u, UiRequest::OpenUrl { .. })), "the link is not followed in Edit Points");
    }
}
