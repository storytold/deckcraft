//! Slide show navigation: click steps, slide order (hidden slides, custom shows, ranges), loop
//! and automatic advance.

use deckcraft_model::Presentation;
use deckcraft_model::text::Action;

use crate::Timeline;

/// What the UI should do after a navigation call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShowAction {
    /// Nothing changed (e.g. back on the first slide).
    None,
    /// Start playing main step `n` of the current slide (clock from 0).
    PlayStep(usize),
    /// Step `n` jumped to its end (it was playing); nothing plays now.
    FinishStep(usize),
    /// Went back before step `n` (showing the state before it).
    StepBack(usize),
    /// Moved to slide index `n` (play its transition; `ShowState::playing` tells whether step 0
    /// starts by itself).
    Slide(usize),
    /// Past the last slide: show the end screen.
    End,
    /// Leave the show (next on the end screen).
    Exit,
}

/// What a clicked link does in the show (`ShowState::follow`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkJump {
    /// Navigation inside the show (already applied to the state).
    Show(ShowAction),
    /// Open this URL (web page, file, `mailto:`) outside the show.
    Open(String),
    /// End the show.
    Exit,
    /// Nothing to follow.
    None,
}

/// Where a slide show is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ShowState {
    /// Index into `presentation.slides`.
    pub slide: usize,
    /// Main step that plays next or is playing; `== steps` when the slide is fully built.
    pub step: usize,
    /// Step `step` is playing (its clock runs). With `step`, gives `Timeline::state(.., step, t)`.
    pub playing: bool,
    /// On the end screen.
    pub ended: bool,
}

/// Slide indices the show visits in order: the custom show or range from Set Up Slide Show, minus
/// hidden slides.
pub fn show_order(pres: &Presentation) -> Vec<usize> {
    let n = pres.slides.len();
    let base: Vec<usize> = match pres.show.custom_show.as_ref().and_then(|name| pres.custom_shows.iter().find(|c| &c.name == name)) {
        Some(cs) => cs.slides.iter().filter_map(|id| pres.slides.iter().position(|s| s.id == *id)).collect(),
        None => match pres.show.range {
            Some((a, b)) => {
                let a = (a.max(1) as usize).saturating_sub(1);
                let b = (b as usize).min(n);
                (a..b).collect()
            }
            None => (0..n).collect(),
        },
    };
    base.into_iter().filter(|i| pres.slides.get(*i).is_some_and(|s| !s.hidden)).collect()
}

fn looping(pres: &Presentation) -> bool {
    pres.show.loop_until_esc || pres.show.show_type == "kiosk"
}

impl ShowState {
    /// Start at the first slide of the show order.
    pub fn new(pres: &Presentation) -> ShowState {
        match show_order(pres).first() {
            Some(&i) => ShowState::start_at(pres, i),
            None => ShowState { ended: true, ..Default::default() },
        }
    }

    /// Start at slide `i` (hidden slides allowed: "from current slide").
    pub fn start_at(pres: &Presentation, i: usize) -> ShowState {
        let mut s = ShowState::default();
        if i < pres.slides.len() {
            s.enter(pres, i);
        } else {
            s.ended = true;
        }
        s
    }

    /// The timeline of slide `i`.
    pub fn timeline(pres: &Presentation, i: usize) -> Timeline {
        match pres.slides.get(i) {
            Some(s) => Timeline::new(s, pres.slide_size.width, pres.slide_size.height),
            None => Timeline::default(),
        }
    }

    /// Main steps of slide `i` (0 with Show without animation).
    pub fn steps(pres: &Presentation, i: usize) -> usize {
        if pres.show.without_animation { 0 } else { ShowState::timeline(pres, i).steps() }
    }

    fn enter(&mut self, pres: &Presentation, i: usize) {
        self.slide = i;
        self.step = 0;
        self.ended = false;
        self.playing = !pres.show.without_animation && ShowState::timeline(pres, i).step_is_auto(0);
    }

    fn next_index(&self, pres: &Presentation) -> Option<usize> {
        let order = show_order(pres);
        match order.iter().position(|&i| i == self.slide) {
            Some(p) => order.get(p + 1).copied(),
            None => order.iter().copied().find(|&i| i > self.slide),
        }
    }

    fn prev_index(&self, pres: &Presentation) -> Option<usize> {
        let order = show_order(pres);
        match order.iter().position(|&i| i == self.slide) {
            Some(p) => p.checked_sub(1).and_then(|q| order.get(q).copied()),
            None => order.iter().copied().rev().find(|&i| i < self.slide),
        }
    }

    /// Click / → / Space: finish the playing step, else play the next step, else go to the next slide.
    pub fn next(&mut self, pres: &Presentation) -> ShowAction {
        if pres.slides.is_empty() {
            self.ended = true;
            return ShowAction::Exit;
        }
        if self.ended {
            return ShowAction::Exit;
        }
        if self.playing {
            return self.step_done(pres);
        }
        let steps = ShowState::steps(pres, self.slide);
        if self.step < steps {
            self.playing = true;
            return ShowAction::PlayStep(self.step);
        }
        match self.next_index(pres) {
            Some(i) => {
                self.enter(pres, i);
                ShowAction::Slide(i)
            }
            None if looping(pres) => match show_order(pres).first() {
                Some(&i) => {
                    self.enter(pres, i);
                    ShowAction::Slide(i)
                }
                None => {
                    self.ended = true;
                    ShowAction::End
                }
            },
            None => {
                self.ended = true;
                self.playing = false;
                ShowAction::End
            }
        }
    }

    /// The playing step reached its end (the UI's clock passed `step_duration`).
    pub fn step_done(&mut self, pres: &Presentation) -> ShowAction {
        if !self.playing {
            return ShowAction::None;
        }
        let n = self.step;
        self.playing = false;
        self.step = (self.step + 1).min(ShowState::steps(pres, self.slide));
        ShowAction::FinishStep(n)
    }

    /// ← / Backspace: undo the last step, else go to the previous slide fully built.
    pub fn prev(&mut self, pres: &Presentation) -> ShowAction {
        if self.ended {
            self.ended = false;
            if self.slide >= pres.slides.len() {
                return ShowAction::None;
            }
            self.step = ShowState::steps(pres, self.slide);
            self.playing = false;
            return ShowAction::Slide(self.slide);
        }
        if self.playing {
            self.playing = false;
            return ShowAction::StepBack(self.step);
        }
        if self.step > 0 {
            self.step -= 1;
            return ShowAction::StepBack(self.step);
        }
        let target = match self.prev_index(pres) {
            Some(i) => Some(i),
            None if looping(pres) => show_order(pres).last().copied().filter(|&i| i != self.slide),
            None => None,
        };
        match target {
            Some(i) => {
                self.slide = i;
                self.step = ShowState::steps(pres, i);
                self.playing = false;
                ShowAction::Slide(i)
            }
            None => ShowAction::None,
        }
    }

    /// Jump to slide `i` (number + Enter, See All Slides); hidden slides are shown when jumped to.
    pub fn goto(&mut self, pres: &Presentation, i: usize) -> ShowAction {
        if i >= pres.slides.len() {
            return ShowAction::None;
        }
        self.enter(pres, i);
        ShowAction::Slide(i)
    }

    /// Follow a clicked hyperlink or shape action. `last_viewed` is the slide shown before the
    /// current one (Last Slide Viewed). Moves to slide targets; everything else is for the UI.
    pub fn follow(&mut self, pres: &Presentation, action: &Action, last_viewed: Option<usize>) -> LinkJump {
        let target = match action {
            Action::Url { url } => return LinkJump::Open(url.clone()),
            Action::EndShow => return LinkJump::Exit,
            Action::Slide { slide } => pres.slides.iter().position(|s| s.id == *slide),
            Action::NextSlide => self.next_index(pres),
            Action::PreviousSlide => self.prev_index(pres),
            Action::FirstSlide => show_order(pres).first().copied(),
            Action::LastSlide => show_order(pres).last().copied(),
            Action::LastViewed => last_viewed,
            // Custom shows, programs and media actions aren't followed (yet).
            Action::CustomShow { .. } | Action::Program { .. } | Action::PlayMedia => None,
        };
        match target {
            Some(i) => LinkJump::Show(self.goto(pres, i)),
            None => LinkJump::None,
        }
    }

    /// Seconds after which the current slide advances by itself (Advance Slide ▸ After), when the
    /// show uses timings.
    pub fn advance_after(&self, pres: &Presentation) -> Option<f64> {
        if !pres.show.use_timings || self.ended {
            return None;
        }
        pres.slides.get(self.slide).and_then(|s| s.transition.as_ref()).and_then(|t| t.advance_after_ms).map(|ms| ms as f64 / 1000.0)
    }

    /// Should the show advance now, `elapsed` seconds after the slide appeared? Waits for the
    /// slide's animations: auto-advance happens once nothing plays and the time is up; remaining
    /// click steps play automatically with timings.
    pub fn auto_advance_due(&self, pres: &Presentation, elapsed: f64) -> bool {
        match self.advance_after(pres) {
            Some(after) => !self.playing && elapsed.is_finite() && elapsed >= after,
            None => false,
        }
    }
}
