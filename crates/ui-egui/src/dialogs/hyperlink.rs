use super::{Value, json};

/// What Insert Hyperlink would edit on the current selection.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum CurrentLink {
    /// Nothing is linked. OK sets a new link (a caret at a word boundary inserts text).
    None,
    /// One link covers the text span or every selected shape.
    One(deckcraft_model::text::Hyperlink),
    /// The selection mixes different links. OK must not replace them until a new target is chosen.
    Mixed,
}

/// Fields the Insert Hyperlink dialog opens with.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct HyperlinkSeed {
    /// `url`, `email` or `slide`.
    pub(super) mode: &'static str,
    pub(super) url: String,
    pub(super) email: String,
    pub(super) slide: usize,
    pub(super) tip: String,
    pub(super) highlight: bool,
    /// The action is not a web address, email or slide this dialog edits (or the selection mixes
    /// links). OK leaves it in place until the user picks a different target.
    pub(super) preserve: bool,
}

impl HyperlinkSeed {
    /// A new link: the address starts at `https://`, as the dialog always did.
    pub(super) fn blank() -> Self {
        HyperlinkSeed { mode: "url", url: "https://".into(), email: String::new(), slide: 0, tip: String::new(), highlight: false, preserve: false }
    }
}

/// Turn one hyperlink into dialog fields. A slide that is no longer in the deck, and actions this
/// dialog does not edit (next slide, a program, …), are preserved so OK does not retarget them.
pub(super) fn seed_of_link(link: &deckcraft_model::text::Hyperlink, slide_ids: &[deckcraft_model::SlideId]) -> HyperlinkSeed {
    use deckcraft_model::text::Action;
    let mut seed = HyperlinkSeed {
        mode: "url",
        url: String::new(),
        email: String::new(),
        slide: 0,
        tip: link.tooltip.clone(),
        highlight: link.highlight_click,
        preserve: true,
    };
    match &link.action {
        Action::Url { url } => {
            seed.preserve = false;
            if let Some(email) = url.strip_prefix("mailto:") {
                seed.mode = "email";
                seed.email = email.to_string();
                seed.url = "https://".into();
            } else {
                seed.url = url.clone();
            }
        }
        Action::Slide { slide } => {
            if let Some(i) = slide_ids.iter().position(|id| id == slide) {
                seed.preserve = false;
                seed.mode = "slide";
                seed.slide = i;
                seed.url = "https://".into();
            }
        }
        _ => {}
    }
    seed
}

pub(super) fn seed_from(current: CurrentLink, slide_ids: &[deckcraft_model::SlideId]) -> HyperlinkSeed {
    match current {
        CurrentLink::None => HyperlinkSeed::blank(),
        CurrentLink::Mixed => HyperlinkSeed { preserve: true, url: String::new(), ..HyperlinkSeed::blank() },
        CurrentLink::One(link) => seed_of_link(&link, slide_ids),
    }
}

/// Links on the half-open span `[a, b)`. A point span is a new link: Insert Hyperlink inserts
/// there instead of editing a neighbouring run.
pub(super) fn link_covering(body: &deckcraft_model::text::TextBody, a: (usize, usize), b: (usize, usize)) -> CurrentLink {
    if a == b {
        return CurrentLink::None;
    }
    let mut seen: Option<Option<deckcraft_model::text::Hyperlink>> = None;
    for (pi, para) in body.paragraphs.iter().enumerate() {
        if pi < a.0 || pi > b.0 {
            continue;
        }
        let from = if pi == a.0 { a.1 } else { 0 };
        let to = if pi == b.0 { b.1 } else { usize::MAX };
        if from >= to {
            continue;
        }
        let mut acc = 0usize;
        let mut covered = false;
        for run in &para.runs {
            let len = run.char_len();
            let end = acc + len;
            if len > 0 && end > from && acc < to {
                covered = true;
                match &seen {
                    None => seen = Some(run.props.link.clone()),
                    Some(prev) if prev == &run.props.link => {}
                    Some(_) => return CurrentLink::Mixed,
                }
            }
            acc = end;
        }
        if !covered && para.char_len() == 0 {
            match &seen {
                None => seen = Some(para.end_props.link.clone()),
                Some(prev) if prev == &para.end_props.link => {}
                Some(_) => return CurrentLink::Mixed,
            }
        }
    }
    match seen {
        Some(Some(link)) => CurrentLink::One(link),
        _ => CurrentLink::None,
    }
}

pub(super) fn common_click(clicks: impl IntoIterator<Item = Option<deckcraft_model::text::Hyperlink>>) -> CurrentLink {
    let mut clicks = clicks.into_iter();
    let Some(first) = clicks.next() else { return CurrentLink::None };
    for next in clicks {
        if next != first {
            return CurrentLink::Mixed;
        }
    }
    match first {
        Some(link) => CurrentLink::One(link),
        None => CurrentLink::None,
    }
}

/// The link Insert Hyperlink would edit: text when a caret or selection is active, otherwise the
/// click action shared by the selected shapes.
pub(super) fn hyperlink_seed_of(st: Option<&deckcraft_engine::DocState>) -> HyperlinkSeed {
    let Some(st) = st else { return HyperlinkSeed::blank() };
    let ids: Vec<deckcraft_model::SlideId> = st.doc.slides.iter().map(|s| s.id).collect();
    let current = if let Some(t) = &st.selection.text {
        match deckcraft_engine::cmd::text::body_of(st, t) {
            Some(body) => {
                let (a, b) = deckcraft_engine::cmd::text::format_range(st, t);
                link_covering(body, a, b)
            }
            None => CurrentLink::None,
        }
    } else {
        common_click(st.selected_shapes().into_iter().map(|sh| sh.click.clone()))
    };
    seed_from(current, &ids)
}

/// OK writes a link unless the dialog is holding an action it does not edit and the user has not
/// chosen a new target.
pub(super) fn hyperlink_should_apply(seed: &HyperlinkSeed, mode: &str, url: &str, email: &str, slide: usize) -> bool {
    if !seed.preserve {
        return true;
    }
    if mode != seed.mode {
        return true;
    }
    match mode {
        "email" => email != seed.email,
        "slide" => slide != seed.slide,
        _ => url != seed.url,
    }
}

pub(super) fn hyperlink_command(mode: &str, url: &str, email: &str, slide: usize, tip: &str, highlight: bool) -> Value {
    match mode {
        "slide" => json!({"slide": slide, "tooltip": tip, "highlightClick": highlight}),
        "email" => json!({"url": format!("mailto:{email}"), "tooltip": tip, "highlightClick": highlight}),
        _ => json!({"url": url, "tooltip": tip, "highlightClick": highlight}),
    }
}
