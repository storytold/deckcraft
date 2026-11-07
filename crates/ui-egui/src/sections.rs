//! Section grouping shared by the thumbnail pane, the Slide Sorter and the slide show.
//!
//! A section holds consecutive slides; slides added outside every section still belong to the
//! document and are grouped last, as "Other slides". Nothing here is saved: sections themselves
//! live in the model, the grouping is derived from their order.

use deckcraft_model::Presentation;

/// Which part of the deck an "All Slides" grid shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GridFilter {
    /// Every slide, in document order.
    #[default]
    All,
    /// One section, by index into `Presentation::sections`.
    Section(usize),
    /// Slides that belong to no section.
    Other,
}

/// One group of slide indices, in document order. `None` is the slides outside every section.
pub fn groups(doc: &Presentation) -> Vec<(Option<usize>, Vec<usize>)> {
    if doc.sections.is_empty() {
        return vec![(None, (0..doc.slides.len()).collect())];
    }
    let mut seen = vec![false; doc.slides.len()];
    let mut out = Vec::with_capacity(doc.sections.len() + 1);
    for (si, sec) in doc.sections.iter().enumerate() {
        let mut slides = Vec::with_capacity(sec.slides.len());
        for id in &sec.slides {
            if let Some(i) = doc.slide_index(*id)
                && let Some(flag) = seen.get_mut(i)
                && !*flag
            {
                *flag = true;
                slides.push(i);
            }
        }
        out.push((Some(si), slides));
    }
    let rest: Vec<usize> = seen.iter().enumerate().filter(|(_, on)| !**on).map(|(i, _)| i).collect();
    if !rest.is_empty() {
        out.push((None, rest));
    }
    out
}

/// The label of a group: the section's name, or "Other slides" for slides outside every section.
pub fn group_name(doc: &Presentation, index: Option<usize>) -> String {
    match index {
        Some(i) => doc.sections.get(i).map(|s| s.name.trim().to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| format!("Section {}", i + 1)),
        None => "Other slides".into(),
    }
}

/// The tabs an "All Slides" grid shows, in order: "All slides", then one per section, then
/// "Other slides" when some slide belongs to no section. Empty when tabs would add nothing
/// (one section and no orphan slide).
pub fn tabs(doc: &Presentation) -> Vec<(GridFilter, String, usize)> {
    if doc.sections.is_empty() {
        return Vec::new();
    }
    let grouped = groups(doc);
    if doc.sections.len() == 1 && grouped.len() == 1 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(grouped.len() + 1);
    out.push((GridFilter::All, "All slides".to_string(), doc.slides.len()));
    for (index, slides) in grouped {
        let filter = match index {
            Some(i) => GridFilter::Section(i),
            None => GridFilter::Other,
        };
        out.push((filter, group_name(doc, index), slides.len()));
    }
    out
}

/// Slide indices an "All Slides" grid lists for `filter`, in document order.
pub fn grid_slides(doc: &Presentation, filter: GridFilter) -> Vec<usize> {
    match filter {
        GridFilter::All => (0..doc.slides.len()).collect(),
        GridFilter::Section(i) => groups(doc).into_iter().find(|(index, _)| *index == Some(i)).map(|(_, slides)| slides).unwrap_or_default(),
        GridFilter::Other => groups(doc).into_iter().find(|(index, _)| index.is_none()).map(|(_, slides)| slides).unwrap_or_default(),
    }
}

/// The filter to select when a grid opens at `slide`: the slide's section when there is one, or
/// "All slides" when tabs add nothing.
pub fn current_filter(doc: &Presentation, slide: usize) -> GridFilter {
    if tabs(doc).is_empty() {
        return GridFilter::All;
    }
    match doc.slides.get(slide).map(|s| s.id).and_then(|id| doc.section_of(id)) {
        Some(i) => GridFilter::Section(i),
        None => GridFilter::Other,
    }
}

/// The name shown as the grid's title for `filter`.
pub fn filter_title(doc: &Presentation, filter: GridFilter) -> String {
    match filter {
        GridFilter::All => "All slides".into(),
        GridFilter::Section(i) => group_name(doc, Some(i)),
        GridFilter::Other => group_name(doc, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn deck() -> Presentation {
        let mut s = deckcraft_engine::Session::with_new();
        for _ in 0..4 {
            s.execute("slide.new", &json!({})).unwrap();
        }
        assert_eq!(s.active().unwrap().doc.slides.len(), 5);
        (*s.active().unwrap().doc).clone()
    }

    #[test]
    fn groups_keep_section_order_and_put_orphans_last() {
        let mut doc = deck();
        let ids: Vec<_> = doc.slides.iter().map(|s| s.id).collect();
        doc.sections.push(deckcraft_model::Section { name: "Intro".into(), slides: ids[..2].to_vec() });
        doc.sections.push(deckcraft_model::Section { name: "Demo".into(), slides: ids[2..4].to_vec() });
        let g = groups(&doc);
        assert_eq!(g.len(), 3);
        assert_eq!(g[0], (Some(0), vec![0, 1]));
        assert_eq!(g[1], (Some(1), vec![2, 3]));
        assert_eq!(g[2], (None, vec![4]));
        assert_eq!(group_name(&doc, Some(1)), "Demo");
        assert_eq!(group_name(&doc, None), "Other slides");
    }

    #[test]
    fn tabs_and_filters_cover_every_slide_once() {
        let mut doc = deck();
        let ids: Vec<_> = doc.slides.iter().map(|s| s.id).collect();
        doc.sections.push(deckcraft_model::Section { name: "Intro".into(), slides: ids[..2].to_vec() });
        doc.sections.push(deckcraft_model::Section { name: "Demo".into(), slides: ids[2..4].to_vec() });
        let tabs = tabs(&doc);
        assert_eq!(tabs.iter().map(|t| t.1.as_str()).collect::<Vec<_>>(), ["All slides", "Intro", "Demo", "Other slides"]);
        assert_eq!(tabs.iter().map(|t| t.2).collect::<Vec<_>>(), [5, 2, 2, 1]);
        assert_eq!(grid_slides(&doc, GridFilter::All), vec![0, 1, 2, 3, 4]);
        assert_eq!(grid_slides(&doc, GridFilter::Section(1)), vec![2, 3]);
        assert_eq!(grid_slides(&doc, GridFilter::Other), vec![4]);
        assert_eq!(current_filter(&doc, 3), GridFilter::Section(1));
        assert_eq!(current_filter(&doc, 4), GridFilter::Other);
    }

    #[test]
    fn a_single_section_with_no_orphans_has_no_tabs() {
        let mut doc = deck();
        let ids: Vec<_> = doc.slides.iter().map(|s| s.id).collect();
        doc.sections.push(deckcraft_model::Section { name: "Only".into(), slides: ids });
        assert!(tabs(&doc).is_empty());
        assert_eq!(current_filter(&doc, 2), GridFilter::All);
        assert_eq!(grid_slides(&doc, GridFilter::All), vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn no_sections_is_the_plain_deck() {
        let doc = deck();
        assert!(tabs(&doc).is_empty());
        assert_eq!(current_filter(&doc, 0), GridFilter::All);
        assert_eq!(grid_slides(&doc, GridFilter::All), vec![0, 1, 2, 3, 4]);
        assert!(grid_slides(&doc, GridFilter::Section(0)).is_empty());
    }
}
