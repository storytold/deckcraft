//! Slide show clicks on decks that went through a PPTX file: links, click-triggered animations and
//! what a slide looks like when the show comes back to it.

use std::sync::Arc;

use deckcraft_anim::{Click, HitShape, ShowAction, ShowState};
use deckcraft_geom::Xfrm;
use deckcraft_model::anim::{AnimClass, AnimStart, Animation, Transition};
use deckcraft_model::text::{Action, Hyperlink, TextBody};
use deckcraft_model::{LayoutType, Presentation, Shape, ShapeId, SlideId, defaults};

fn shape(p: &mut Presentation, name: &str, text: &str, x: f64, y: f64, w: f64, h: f64) -> Shape {
    Shape {
        id: ShapeId(p.alloc_id().expect("id")),
        name: name.into(),
        xfrm: Some(Xfrm::new(x, y, w, h)),
        text: Some(TextBody::from_text(text)),
        ..Default::default()
    }
}

fn link_to(slide: SlideId) -> Option<Hyperlink> {
    Some(Hyperlink {
        action: Action::Slide { slide },
        tooltip: String::new(),
        highlight_click: false,
        sound: None,
        stop_sound: false,
        zoom_return: None,
    })
}

/// Clicking `trigger` makes `target` disappear.
fn disappear_on_click(trigger: ShapeId, target: ShapeId) -> Animation {
    Animation {
        shape: target,
        class: AnimClass::Exit,
        effect: "disappear".into(),
        start: AnimStart::OnClick,
        duration_ms: 0,
        trigger: Some(trigger),
        ..Default::default()
    }
}

fn add_slide(p: &mut Presentation, shapes: Vec<Shape>, animations: Vec<Animation>, advance_on_click: bool) -> SlideId {
    let layout = defaults::layout_of_kind(p, LayoutType::Blank).unwrap_or_default();
    let mut s = defaults::new_slide(p, layout).expect("slide");
    s.shapes = shapes;
    s.animations = animations;
    s.transition = (!advance_on_click).then(|| Transition { advance_on_click: false, ..Default::default() });
    let id = s.id;
    p.slides.push(Arc::new(s));
    id
}

/// Drops the slide `new_presentation` starts with.
fn start(p: &mut Presentation) {
    p.slides.clear();
}

#[test]
fn saved_trigger_reveals_then_hides_on_separate_clicks() {
    let mut p = defaults::new_presentation(None);
    start(&mut p);
    let button = shape(&mut p, "Button", "Toggle", 20.0, 20.0, 100.0, 40.0);
    let target = shape(&mut p, "Target", "Answer", 200.0, 100.0, 200.0, 80.0);
    let animations = vec![
        Animation {
            shape: target.id,
            trigger: Some(button.id),
            effect: "appear".into(),
            start: AnimStart::OnClick,
            duration_ms: 0,
            ..Default::default()
        },
        disappear_on_click(button.id, target.id),
    ];
    add_slide(&mut p, vec![button, target], animations, false);
    let mut show = Show::open(&p);
    assert!(!show.shown("Target"));
    for _ in 0..2 {
        assert_eq!(show.click("Button"), Click::Animated);
        assert!(show.shown("Target"), "first click reveals the answer until the next click");
        assert_eq!(show.click("Button"), Click::Animated);
        assert!(!show.shown("Target"), "second click hides the answer");
    }
}

/// The slide show window's click handling, on a deck read from a PPTX file.
struct Show {
    pres: Presentation,
    state: ShowState,
}

impl Show {
    fn open(p: &Presentation) -> Show {
        let bytes = deckcraft_pptx::export(p).expect("export");
        let pres = deckcraft_pptx::import(&bytes).expect("import");
        let state = ShowState::new(&pres);
        Show { pres, state }
    }

    fn hits(&self, x: f64, y: f64) -> (Vec<HitShape>, Vec<bool>) {
        let placed = deckcraft_render::place_slide(&self.pres, self.state.slide);
        let tl = ShowState::timeline(&self.pres, self.state.slide);
        let hits = placed
            .shapes
            .iter()
            .map(|s| HitShape {
                id: s.id,
                parent: s.parent,
                is_group: s.is_group,
                link: s.link.as_ref().map(|l| l.action.clone()),
                under_pointer: s.rect.x0 <= x && x <= s.rect.x1 && s.rect.y0 <= y && y <= s.rect.y1,
            })
            .collect();
        let visible = placed
            .shapes
            .iter()
            .map(|s| {
                let a = self.state.anim_state(&tl, s.id, 0.0);
                a.visible && a.opacity > 0.0
            })
            .collect();
        (hits, visible)
    }

    /// Click the middle of the shape called `name` on the current slide.
    fn click(&mut self, name: &str) -> Click {
        let (x, y) = self.centre(name);
        let (hits, vis) = self.hits(x, y);
        let tl = ShowState::timeline(&self.pres, self.state.slide);
        let visible = |id: ShapeId| hits.iter().zip(&vis).find(|(h, _)| h.id == id).is_none_or(|(_, v)| *v);
        self.state.click(&self.pres, &tl, &hits, &visible, 0.0, None)
    }

    fn centre(&self, name: &str) -> (f64, f64) {
        let slide = &self.pres.slides[self.state.slide];
        let s = slide.shapes.iter().find(|s| s.name == name).unwrap_or_else(|| panic!("no shape {name}"));
        let x = s.xfrm.expect("xfrm");
        (x.x + x.w / 2.0, x.y + x.h / 2.0)
    }

    fn shown(&self, name: &str) -> bool {
        let slide = &self.pres.slides[self.state.slide];
        let Some(s) = slide.shapes.iter().find(|s| s.name == name) else { return false };
        let a = self.state.anim_state(&ShowState::timeline(&self.pres, self.state.slide), s.id, 0.0);
        a.visible && a.opacity > 0.0
    }

    fn is_target(&self, name: &str) -> bool {
        let (x, y) = self.centre(name);
        let (hits, vis) = self.hits(x, y);
        let tl = ShowState::timeline(&self.pres, self.state.slide);
        let visible = |id: ShapeId| hits.iter().zip(&vis).find(|(h, _)| h.id == id).is_none_or(|(_, v)| *v);
        let id = self.pres.slides[self.state.slide].shapes.iter().find(|s| s.name == name).map(|s| s.id);
        self.state.target(&self.pres, &tl, &hits, &visible).map(|t| t.shape) == id
    }
}

/// An image that disappears when clicked, revealing a button that links to slide 2 and disappears
/// as well; slide 2 links back; the slides do not advance on a click.
fn hide_and_reveal_deck() -> Presentation {
    let mut p = defaults::new_presentation(None);
    start(&mut p);
    let s2_id = SlideId(p.alloc_id().expect("id"));
    let _ = s2_id;
    let message = shape(&mut p, "Message", "Both hidden", 100.0, 100.0, 600.0, 300.0);
    let mut button = shape(&mut p, "Button", "Image hidden", 100.0, 100.0, 600.0, 300.0);
    let picture = shape(&mut p, "Picture", "Picture", 100.0, 100.0, 600.0, 300.0);
    let animations = vec![disappear_on_click(picture.id, picture.id), disappear_on_click(button.id, button.id)];
    let first = add_slide(&mut p, vec![message, button.clone(), picture], animations, false);
    let mut back = shape(&mut p, "Back", "Go back", 100.0, 100.0, 600.0, 300.0);
    back.click = link_to(first);
    let second = add_slide(&mut p, vec![back], vec![], false);
    // The button links to slide 2.
    let s = Arc::make_mut(&mut p.slides[0]);
    button.click = link_to(second);
    if let Some(b) = s.shapes.iter_mut().find(|x| x.name == "Button") {
        b.click = button.click.clone();
    }
    p
}

#[test]
fn hidden_shapes_stay_hidden_and_dead_when_the_show_comes_back() {
    let mut show = Show::open(&hide_and_reveal_deck());
    assert!(show.is_target("Picture") && show.shown("Picture"));
    assert_eq!(show.click("Picture"), Click::Animated);
    assert!(!show.shown("Picture"));
    // The button was under the picture; now it is on top. Its click follows the link.
    assert!(show.is_target("Button"));
    assert_eq!(show.click("Button"), Click::Show(ShowAction::Slide(1)));
    assert_eq!(show.state.slide, 1);
    assert_eq!(show.click("Back"), Click::Show(ShowAction::Slide(0)));
    // Back on slide 1: both are gone, only the message is left, and it does nothing.
    assert!(!show.shown("Picture") && !show.shown("Button") && show.shown("Message"));
    assert!(!show.is_target("Message"));
    for _ in 0..3 {
        assert_eq!(show.click("Message"), Click::None);
    }
    assert_eq!(show.state.slide, 0);
}

/// A board of three tiles, each linking to its question, which links back; the tile hides itself.
#[test]
fn a_game_board_hides_the_tiles_already_played() {
    let mut p = defaults::new_presentation(None);
    start(&mut p);
    let mut tiles: Vec<Shape> = (0..3).map(|i| shape(&mut p, &format!("Tile {i}"), "100", 50.0 + 250.0 * i as f64, 100.0, 200.0, 100.0)).collect();
    let animations: Vec<Animation> = tiles.iter().map(|t| disappear_on_click(t.id, t.id)).collect();
    let board = add_slide(&mut p, tiles.clone(), animations, true);
    let questions: Vec<SlideId> = (0..3)
        .map(|i| {
            let mut back = shape(&mut p, "Back", &format!("Question {i}"), 100.0, 100.0, 400.0, 200.0);
            back.click = link_to(board);
            add_slide(&mut p, vec![back], vec![], true)
        })
        .collect();
    let s = Arc::make_mut(&mut p.slides[0]);
    for (t, q) in tiles.iter_mut().zip(&questions) {
        if let Some(x) = s.shapes.iter_mut().find(|x| x.id == t.id) {
            x.click = link_to(*q);
        }
    }
    let mut show = Show::open(&p);
    for (i, played) in [1usize, 0, 2].into_iter().enumerate() {
        assert_eq!(show.click(&format!("Tile {played}")), Click::Show(ShowAction::Slide(1 + played)));
        assert_eq!(show.click("Back"), Click::Show(ShowAction::Slide(0)));
        // Exactly the tiles played so far are gone.
        let gone: Vec<bool> = (0..3).map(|t| !show.shown(&format!("Tile {t}"))).collect();
        assert_eq!(gone.iter().filter(|g| **g).count(), i + 1, "{gone:?}");
        assert!(gone[played]);
        assert!(!show.is_target(&format!("Tile {played}")));
    }
}

/// A score that cycles: each digit hides itself when clicked and shows the next; the last shows the
/// first again. Every digit has its own one-click trigger.
#[test]
fn a_score_ring_goes_round() {
    let mut p = defaults::new_presentation(None);
    start(&mut p);
    let n = 4usize;
    let digits: Vec<Shape> = (0..n).map(|i| shape(&mut p, &format!("Score {i}"), &i.to_string(), 400.0, 100.0, 100.0, 100.0)).collect();
    let mut anims = vec![];
    for (i, d) in digits.iter().enumerate() {
        anims.push(disappear_on_click(d.id, d.id));
        anims.push(Animation {
            shape: digits[(i + 1) % n].id,
            class: AnimClass::Entrance,
            effect: "appear".into(),
            start: AnimStart::WithPrevious,
            duration_ms: 0,
            trigger: Some(d.id),
            ..Default::default()
        });
    }
    add_slide(&mut p, digits, anims, true);
    let mut show = Show::open(&p);
    let showing = |show: &Show| -> Vec<usize> { (0..n).filter(|i| show.shown(&format!("Score {i}"))).collect() };
    assert_eq!(showing(&show), vec![0]);
    for click in 0..2 * n {
        let now = click % n;
        assert_eq!(showing(&show), vec![now], "before click {click}");
        assert!(show.is_target(&format!("Score {now}")));
        assert_eq!(show.click(&format!("Score {now}")), Click::Animated);
    }
    assert_eq!(showing(&show), vec![0]);
}
