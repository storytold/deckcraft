use super::*;

pub(super) fn hyperlink(s: &mut Session, p: &Value) -> Result<Value> {
    use deckcraft_model::text::{Action, Hyperlink};
    // `display`: what a caret with nothing to link inserts, the address or the slide's title.
    let (action, display) = if let Some(u) = str_param(p, "url") {
        (Action::Url { url: u.to_string() }, u.strip_prefix("mailto:").unwrap_or(u).to_string())
    } else if let Some(i) = usize_param(p, "slide") {
        let slide = s.doc()?.doc.slides.get(i).ok_or_else(|| bad("insert.hyperlink", "no such slide"))?;
        let title = slide.title();
        (Action::Slide { slide: slide.id }, if title.is_empty() { format!("Slide {}", i + 1) } else { title })
    } else {
        return Err(bad("insert.hyperlink", "missing `url` or `slide`"));
    };
    let display = str_param(p, "text").map_or(display, str::to_string);
    let link = Hyperlink {
        action,
        tooltip: str_param(p, "tooltip").unwrap_or("").to_string(),
        highlight_click: bool_or(p, "highlightClick", false),
        sound: None,
        stop_sound: false,
        zoom_return: None,
    };
    if s.doc()?.selection.text.is_some() {
        return crate::cmd::text::link_text(s, link, &display);
    }
    edit_shapes(s, p, "insert.hyperlink", |sh| {
        sh.click = Some(link.clone());
        Ok(())
    })
}

/// Insert ▸ Action (Action Settings): what clicking the selected shapes, or pointing at them, does
/// in a show.
pub(super) fn shape_action(s: &mut Session, p: &Value) -> Result<Value> {
    use deckcraft_model::text::{Action, Hyperlink};
    const CMD: &str = "shape.action";
    let hover = match str_param(p, "on").unwrap_or("click") {
        "click" => false,
        "hover" | "over" => true,
        o => return Err(bad(CMD, format!("`on` is click or hover, not `{o}`"))),
    };
    let need = |k: &str| str_param(p, k).map(str::to_string).filter(|v| !v.trim().is_empty()).ok_or_else(|| bad(CMD, format!("missing `{k}`")));
    let action = match str_param(p, "action").unwrap_or("none") {
        "none" => None,
        "next" => Some(Action::NextSlide),
        "previous" => Some(Action::PreviousSlide),
        "first" => Some(Action::FirstSlide),
        "last" => Some(Action::LastSlide),
        "lastViewed" => Some(Action::LastViewed),
        "endShow" => Some(Action::EndShow),
        "playMedia" => Some(Action::PlayMedia),
        "url" => Some(Action::Url { url: need("url")? }),
        "customShow" => {
            let name = need("show")?;
            if !s.doc()?.doc.custom_shows.iter().any(|c| c.name == name) {
                return Err(bad(CMD, format!("no custom show `{name}`")));
            }
            Some(Action::CustomShow { name, returns: true })
        }
        "program" => Some(Action::Program { path: need("path")? }),
        "slide" => {
            let i = usize_param(p, "slide").ok_or_else(|| bad(CMD, "missing `slide`"))?;
            let slide = s.doc()?.doc.slides.get(i).ok_or_else(|| bad(CMD, "no such slide"))?;
            Some(Action::Slide { slide: slide.id })
        }
        a => return Err(bad(CMD, format!("unknown action `{a}`"))),
    };
    let highlight = bool_param(p, "highlight").unwrap_or(true);
    let tooltip = str_param(p, "tooltip").unwrap_or("").to_string();
    let link = action.map(|action| Hyperlink { action, tooltip, highlight_click: highlight, sound: None, stop_sound: false, zoom_return: None });
    edit_shapes(s, p, CMD, |sh| {
        if hover {
            sh.hover = link.clone();
        } else {
            sh.click = link.clone();
        }
        Ok(())
    })
}

pub(super) fn action_button(s: &mut Session, p: &Value) -> Result<Value> {
    use deckcraft_model::text::{Action, Hyperlink};
    let kind = str_param(p, "kind").unwrap_or("custom");
    let (preset, action) = match kind {
        "back" | "previous" => ("actionButtonBackPrevious", Some(Action::PreviousSlide)),
        "forward" | "next" => ("actionButtonForwardNext", Some(Action::NextSlide)),
        "beginning" => ("actionButtonBeginning", Some(Action::FirstSlide)),
        "end" => ("actionButtonEnd", Some(Action::LastSlide)),
        "home" => ("actionButtonHome", Some(Action::FirstSlide)),
        "information" => ("actionButtonInformation", None),
        "return" => ("actionButtonReturn", Some(Action::LastViewed)),
        "movie" => ("actionButtonMovie", None),
        "document" => ("actionButtonDocument", None),
        "sound" => ("actionButtonSound", None),
        "help" => ("actionButtonHelp", None),
        _ => ("actionButtonBlank", None),
    };
    let rect = rect_param(p, "rect").unwrap_or_else(|| default_rect(s, 48.0, 48.0));
    let shape = Shape {
        xfrm: Some(rect),
        geom: Geom::preset(preset),
        style: Some(ShapeStyle::accent(SchemeSlot::Accent1)),
        click: action.map(|a| Hyperlink {
            action: a,
            tooltip: String::new(),
            highlight_click: true,
            sound: None,
            stop_sound: false,
            zoom_return: None,
        }),
        ..Default::default()
    };
    let id = add_shape(s, "insert.actionButton", shape, true)?;
    Ok(json!({"id": id}))
}
