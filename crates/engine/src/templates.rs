//! Original starter presentations for File › New.
//!
//! Each deck is a few slides of our own text on one of our themes. Nothing here is copied from
//! another product's templates.

use std::sync::Arc;

use deckcraft_geom::Size;
use deckcraft_model::text::TextBody;
use deckcraft_model::{Presentation, defaults};
use serde_json::Value;

use crate::Result;
use crate::cmd::{bad, slide::find_layout};

/// One slide of a starter: a layout, a title, text for each non-title placeholder in order, and
/// optional speaker notes.
pub struct SlideSeed {
    pub layout: &'static str,
    pub title: &'static str,
    pub bodies: &'static [&'static str],
    pub notes: &'static str,
}

/// A starter deck shown in the template gallery.
pub struct Starter {
    pub name: &'static str,
    pub summary: &'static str,
    pub theme: &'static str,
    pub slides: &'static [SlideSeed],
}

const fn seed(layout: &'static str, title: &'static str, bodies: &'static [&'static str], notes: &'static str) -> SlideSeed {
    SlideSeed { layout, title, bodies, notes }
}

const STARTERS: &[Starter] = &[
    Starter {
        name: "Briefing",
        summary: "A weekly check-in: what changed, what's stuck, and who does the next thing",
        theme: "Harbor",
        slides: &[
            seed("title", "Weekly briefing", &["What changed, what's stuck, and what we do next"], "Open with what changed since last week."),
            seed(
                "titleAndContent",
                "This week's agenda",
                &["Shipped since last time\n\tOne concrete change\nStill open\n\tThe decision we need\nNext\n\tWho does it, and by when"],
                "",
            ),
            seed("sectionHeader", "Where we are", &["A short picture before the details"], ""),
            seed(
                "twoContent",
                "Done and next",
                &[
                    "Done\n\tThe change is in people's hands\n\tWe wrote down what we learned",
                    "Next\n\tPick the one risk to retire\n\tSay who owns it",
                ],
                "",
            ),
            seed("title", "See you next week", &["Bring one number and one question"], ""),
        ],
    },
    Starter {
        name: "Lesson",
        summary: "A short class that ends with the learner explaining the idea",
        theme: "Meadow",
        slides: &[
            seed("title", "How a lock works", &["A short lesson for people new to the idea"], ""),
            seed(
                "titleAndContent",
                "By the end",
                &["You can say what a lock protects\n\tAnd what it does not\nYou can sketch one in three steps"],
                "",
            ),
            seed("sectionHeader", "The idea", &["One metaphor, then the real thing"], ""),
            seed(
                "titleAndContent",
                "Three steps",
                &["Name the thing being shared\nDecide who may change it\nLeave it so the next person can tell"],
                "",
            ),
            seed("title", "Try it", &["Explain a lock to someone who has never seen one"], ""),
        ],
    },
    Starter {
        name: "Pitch",
        summary: "A ten-minute conversation about one promise, not a tour of features",
        theme: "Ember",
        slides: &[
            seed("title", "A quieter inbox", &["Ten minutes. One promise."], ""),
            seed(
                "titleAndContent",
                "The mess",
                &[
                    "People miss the one message that mattered\n\tIt arrived between newsletters\nThey spend the morning sorting\n\tAnd still feel behind",
                ],
                "",
            ),
            seed("sectionHeader", "A smaller promise", &["Show the one message. Hide the rest until asked."], ""),
            seed(
                "comparison",
                "Today and after",
                &[
                    "Today",
                    "The important note is buried\nFinding it takes a search",
                    "After",
                    "It sits at the top until you deal with it\nEverything else waits",
                ],
                "",
            ),
            seed("title", "The ask", &["Twenty people, two weeks, one question: did the right note surface?"], ""),
        ],
    },
    Starter {
        name: "Field Notes",
        summary: "What an outing set out to see, and what it actually found",
        theme: "Coral Reef",
        slides: &[
            seed("title", "Saturday on the ridge", &["What we went to see, and what we found"], ""),
            seed("titleAndContent", "Plan", &["Leave at dawn\n\tCold, clear, little wind\nWalk the west path\nBe back before dusk"], ""),
            seed("sectionHeader", "What we saw", &["Notes taken on the spot"], ""),
            seed(
                "twoContent",
                "Expected and found",
                &[
                    "Expected\n\tA clear view from the ridge\n\tThe creek on the map",
                    "Found\n\tLow cloud on the ridge\n\tThe creek, and a better pencil",
                ],
                "",
            ),
            seed("title", "Pack list for next time", &["Warmer gloves. A second pencil. The map that shows the creek."], ""),
        ],
    },
    Starter {
        name: "Quarterly",
        summary: "A report meant to be read aloud in half an hour",
        theme: "Paper",
        slides: &[
            seed("title", "Spring report", &["Read aloud in thirty minutes"], ""),
            seed(
                "titleAndContent",
                "Headlines",
                &["The waiting list got shorter\n\tBy the amount we promised\nTwo things slipped\n\tBoth have a new date"],
                "",
            ),
            seed("sectionHeader", "The numbers", &["Only the ones that change a decision"], ""),
            seed("titleAndContent", "What we will stop", &["The report nobody reads\nThe meeting that repeats the report"], ""),
            seed("title", "Questions we want", &["Where should the next quarter refuse work?"], ""),
        ],
    },
    Starter {
        name: "Evening Talk",
        summary: "A public talk the audience can check on the way home",
        theme: "Nocturne",
        slides: &[
            seed("title", "Lights out", &["A talk about how cities sleep"], ""),
            seed("titleAndContent", "Road map", &["Why the sky looks orange\nWhat a dark street is for\nOne change a block can make"], ""),
            seed("sectionHeader", "Look up", &["The part you can check on the way home"], ""),
            seed(
                "titleAndContent",
                "One change",
                &["Shield the lamp so light goes down\n\tNot into bedroom windows\nLeave a path you can still walk"],
                "",
            ),
            seed("title", "Walk home", &["Count the lamps you can see from your pillow"], ""),
        ],
    },
];

/// The starter gallery, in the order File › New shows it.
pub fn starters() -> &'static [Starter] {
    STARTERS
}

/// Starters whose name, summary, theme or a slide title contains `query` (empty matches all).
pub fn matching(query: &str) -> Vec<&'static Starter> {
    let q = query.trim().to_lowercase();
    let hit = |text: &str| text.to_lowercase().contains(&q);
    starters()
        .iter()
        .filter(|t| q.is_empty() || hit(t.name) || hit(t.summary) || hit(t.theme) || t.slides.iter().any(|slide| hit(slide.title)))
        .collect()
}

pub fn find(name: &str) -> Option<&'static Starter> {
    let name = name.trim();
    starters().iter().find(|t| t.name.eq_ignore_ascii_case(name))
}

fn text_with_levels(text: &str) -> TextBody {
    let mut body = TextBody::from_text(text);
    for para in &mut body.paragraphs {
        let tabs = para.runs.first().map(|r| r.text.chars().take_while(|c| *c == '\t').count()).unwrap_or(0);
        if tabs > 0
            && let Some(run) = para.runs.first_mut()
        {
            run.text = run.text.trim_start_matches('\t').to_string();
            para.level = tabs.min(8) as u8;
        }
    }
    body
}

/// A new presentation filled with `name`'s slides. `size` is the slide size in points.
pub fn build(name: &str, size: Size) -> Result<Presentation> {
    let starter = find(name).ok_or_else(|| {
        let names = starters().iter().map(|t| t.name).collect::<Vec<_>>().join(", ");
        bad("file.new", format!("no template `{name}`; use {names}"))
    })?;
    let theme = deckcraft_model::theme::builtin_themes()
        .into_iter()
        .find(|t| t.name.eq_ignore_ascii_case(starter.theme))
        .ok_or_else(|| bad("file.new", format!("template `{}` names a missing theme `{}`", starter.name, starter.theme)))?;
    let mut doc = defaults::blank_presentation(size, theme, false);
    for seed in starter.slides {
        let key = Value::String(seed.layout.to_string());
        let layout = find_layout(&doc, &key, 0)
            .ok_or_else(|| bad("file.new", format!("template `{}` uses an unknown layout `{}`", starter.name, seed.layout)))?;
        let mut slide = defaults::new_slide(&mut doc, layout).ok_or_else(|| bad("file.new", crate::cmd::NO_ID_SPACE))?;
        if !seed.title.is_empty()
            && let Some(sh) = slide.shapes.iter_mut().find(|sh| sh.ph_type().is_some_and(|k| k.is_title()))
        {
            sh.text = Some(text_with_levels(seed.title));
        }
        let mut bodies = seed.bodies.iter();
        for sh in slide.shapes.iter_mut().filter(|sh| sh.ph_type().is_some_and(|k| !k.is_title())) {
            let Some(text) = bodies.next() else { break };
            if !text.is_empty() {
                sh.text = Some(text_with_levels(text));
            }
        }
        if bodies.next().is_some() {
            return Err(bad("file.new", format!("template `{}` has more text than the `{}` layout holds", starter.name, seed.layout)));
        }
        if !seed.notes.is_empty() {
            slide.notes = text_with_levels(seed.notes);
        }
        doc.slides.push(Arc::new(slide));
    }
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::Session;

    fn body(doc: &Presentation, slide: usize, n: usize) -> (String, u8) {
        let para_slide = &doc.slides[slide];
        let text = para_slide
            .shapes
            .iter()
            .filter(|sh| sh.ph_type().is_some_and(|k| !k.is_title()))
            .nth(n)
            .and_then(|sh| sh.text.as_ref())
            .cloned()
            .unwrap_or_default();
        let level = text.paragraphs.get(1).map(|p| p.level).unwrap_or(0);
        (text.text(), level)
    }

    #[test]
    fn every_starter_opens_on_its_theme_and_is_a_clean_document() {
        let mut s = Session::new();
        for starter in starters() {
            s.execute("file.new", &json!({"template": starter.name.to_lowercase()})).unwrap();
            let st = s.active().unwrap();
            assert_eq!(st.doc.masters[0].theme.name, starter.theme, "{}", starter.name);
            assert_eq!(st.doc.slides.len(), starter.slides.len(), "{}", starter.name);
            assert_eq!(st.doc.slides[0].title(), starter.slides[0].title, "{}", starter.name);
            assert!(!st.is_dirty(), "{}", starter.name);
            assert!(st.history.undo.is_empty(), "{}", starter.name);
            assert_eq!(st.selection.slide, 0);
        }
        let listed = s.execute("file.templates", &json!({})).unwrap();
        assert_eq!(listed["templates"].as_array().map(Vec::len), Some(starters().len()));
    }

    #[test]
    fn briefing_keeps_notes_columns_and_outline_levels() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"template": "Briefing", "size": [800.0, 450.0]})).unwrap();
        let doc = &s.active().unwrap().doc;
        assert_eq!(doc.slide_size.width, 800.0);
        assert_eq!(doc.slides[0].notes_text(), "Open with what changed since last week.");
        let (agenda, level) = body(doc, 1, 0);
        assert!(agenda.contains("One concrete change"), "{agenda}");
        assert_eq!(level, 1);
        let (done, _) = body(doc, 3, 0);
        let (next, _) = body(doc, 3, 1);
        assert!(done.starts_with("Done"), "{done}");
        assert!(next.starts_with("Next"), "{next}");
    }

    #[test]
    fn pitch_comparison_fills_both_columns() {
        let doc = build("Pitch", defaults::WIDE).unwrap();
        assert_eq!(body(&doc, 3, 0).0, "Today");
        assert!(body(&doc, 3, 1).0.contains("buried"));
        assert_eq!(body(&doc, 3, 2).0, "After");
        assert!(body(&doc, 3, 3).0.contains("top"));
    }

    #[test]
    fn search_matches_name_summary_and_theme() {
        assert_eq!(matching("ridge").iter().map(|t| t.name).collect::<Vec<_>>(), vec!["Field Notes"]);
        assert_eq!(matching("nocturne").iter().map(|t| t.name).collect::<Vec<_>>(), vec!["Evening Talk"]);
        assert_eq!(matching("").len(), starters().len());
        assert!(matching("facet").is_empty());
    }

    #[test]
    fn unknown_blank_and_theme_are_refused() {
        let mut s = Session::new();
        let unknown = s.execute("file.new", &json!({"template": "Ion"})).unwrap_err();
        assert!(unknown.to_string().contains("Briefing"), "{unknown}");
        let both = s.execute("file.new", &json!({"template": "Lesson", "theme": "Harbor"})).unwrap_err();
        assert!(both.to_string().contains("not both"), "{both}");
        let blank = s.execute("file.new", &json!({"template": "Lesson", "blank": true})).unwrap_err();
        assert!(blank.to_string().contains("blank"), "{blank}");
        assert!(s.active().is_none());
    }

    #[test]
    fn a_starter_round_trips_through_pptx() {
        let doc = build("Evening Talk", defaults::WIDE).unwrap();
        let bytes = deckcraft_pptx::export(&doc).unwrap();
        let back = deckcraft_pptx::import(&bytes).unwrap();
        assert_eq!(back.slides.len(), doc.slides.len());
        for (before, after) in doc.slides.iter().zip(back.slides.iter()) {
            assert_eq!(after.title(), before.title());
        }
        assert!(back.slides[3].shapes.iter().any(|sh| sh.text.as_ref().is_some_and(|t| t.text().contains("bedroom"))));
    }
}
