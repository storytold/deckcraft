//! Which display the slide show and Presenter View use. With more than one display the show asks
//! which one to fill (the one chosen last time first), and Presenter View opens in a window that
//! stays on top of the show and comes back where it was last left. Swap Displays trades them.
//!
//! The desktop app reports the displays ([`crate::SlideApp::displays`]) in physical pixels, in the
//! order `egui::ViewportCommand::SetMonitor` counts them. Window rectangles are `[x, y, w, h]`: the
//! outer position and the inner size, in physical pixels.

/// A display's area on the desktop, in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Display {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Display {
    /// How much of `r` lies on this display, in square pixels.
    fn overlap(&self, r: [i32; 4]) -> i64 {
        let w = (self.x + self.w).min(r[0] + r[2]) - self.x.max(r[0]);
        let h = (self.y + self.h).min(r[1] + r[3]) - self.y.max(r[1]);
        if w > 0 && h > 0 { i64::from(w) * i64::from(h) } else { 0 }
    }

    /// A window of `size` centred on this display, shrunk to leave a margin around it.
    fn centre(&self, size: [i32; 2]) -> [i32; 4] {
        let w = size[0].clamp(1, (self.w - 2 * MARGIN).max(1));
        let h = size[1].clamp(1, (self.h - 2 * MARGIN).max(1));
        [self.x + (self.w - w) / 2, self.y + (self.h - h) / 2, w, h]
    }
}

/// Room kept around a window placed on a display, in physical pixels (title bar, taskbar).
const MARGIN: i32 = 40;
/// A window's title bar height, and how much of its width must show to grab it (physical pixels).
const TITLE_BAR: i32 = 32;
const GRAB: i32 = 160;

/// The displays, and the one the editor window is on.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Displays {
    pub list: Vec<Display>,
    pub editor: Option<usize>,
}

impl Displays {
    /// The display most of `r` is on.
    fn under(&self, r: [i32; 4]) -> Option<usize> {
        let (i, area) = self.list.iter().enumerate().map(|(i, d)| (i, d.overlap(r))).max_by_key(|&(i, a)| (a, std::cmp::Reverse(i)))?;
        (area > 0).then_some(i)
    }

    /// A display other than `not`, the editor's first.
    fn other(&self, not: Option<usize>) -> Option<usize> {
        self.editor.filter(|&e| Some(e) != not && e < self.list.len()).or_else(|| (0..self.list.len()).find(|&i| Some(i) != not))
    }

    /// The display offered first when the show asks where to go: the one chosen last time while
    /// it is still there, else the editor's.
    pub fn first_choice(&self, last: Option<usize>) -> usize {
        let valid = |i: &usize| *i < self.list.len();
        last.filter(valid).or(self.editor.filter(valid)).unwrap_or(0)
    }

    /// Can the window at `r` still be grabbed: is enough of its title bar on one display?
    fn reachable(&self, r: [i32; 4]) -> bool {
        let bar = [r[0], r[1], r[2], TITLE_BAR];
        self.list.iter().any(|d| d.overlap(bar) >= i64::from(GRAB.min(r[2])) * i64::from(TITLE_BAR))
    }

    /// Where Presenter View's window opens: where it was left (`saved`) while its title bar can
    /// still be grabbed, even over the show (it stays on top); else (a display gone, a resolution
    /// changed) centred on a display other than the show's when there is one, with the saved size
    /// or `size`. `None` leaves it to the system.
    pub fn presenter(&self, saved: Option<[i32; 4]>, show: Option<usize>, size: [i32; 2]) -> Option<[i32; 4]> {
        let saved = saved.filter(|r| r[2] > 0 && r[3] > 0);
        if self.list.is_empty() || saved.is_some_and(|r| self.reachable(r)) {
            return saved;
        }
        let d = self.other(show).unwrap_or(0);
        let size = saved.map_or(size, |r| [r[2], r[3]]);
        self.list.get(d).map(|d| d.centre(size))
    }

    /// Swap Displays: the show moves to the display Presenter View is on (the next one when they
    /// share one) and Presenter View, keeping its size, to the display the show leaves.
    pub fn swap(&self, show: usize, presenter: [i32; 4]) -> Option<(usize, [i32; 4])> {
        let n = self.list.len();
        if n < 2 || show >= n {
            return None;
        }
        let to = self.under(presenter).filter(|&d| d != show).unwrap_or((show + 1) % n);
        Some((to, self.list[show].centre([presenter[2], presenter[3]])))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1920×1080 laptop at the origin and a 1280×720 projector to its right.
    fn two() -> Displays {
        Displays { list: vec![Display { x: 0, y: 0, w: 1920, h: 1080 }, Display { x: 1920, y: 0, w: 1280, h: 720 }], editor: Some(0) }
    }

    #[test]
    fn the_question_offers_the_last_choice_first() {
        let mut d = two();
        assert_eq!(d.first_choice(None), 0, "the editor's display");
        assert_eq!(d.first_choice(Some(1)), 1);
        d.editor = Some(1);
        assert_eq!(d.first_choice(None), 1);
        // A display that has gone: the editor's.
        assert_eq!(d.first_choice(Some(5)), 1);
        d.editor = Some(7);
        assert_eq!(d.first_choice(Some(5)), 0);
        assert_eq!(Displays::default().first_choice(Some(3)), 0);
    }

    #[test]
    fn presenter_view_comes_back_where_it_was_left() {
        let d = two();
        let left = [100, 120, 900, 600];
        assert_eq!(d.presenter(Some(left), Some(1), [1100, 700]), Some(left));
        // On the show's display too: it stays on top of the show.
        assert_eq!(d.presenter(Some([2000, 50, 900, 600]), Some(1), [1100, 700]), Some([2000, 50, 900, 600]));
        // Nothing saved, or saved on a display that is gone: centred on another display.
        assert_eq!(d.presenter(None, Some(1), [1100, 700]), Some([410, 190, 1100, 700]));
        assert_eq!(d.presenter(Some([-3000, 0, 800, 500]), Some(1), [1100, 700]), Some([560, 290, 800, 500]));
        // Left almost off screen after a resolution change, or with its title bar above the top:
        // it can't be grabbed, so it is centred again.
        assert_eq!(d.presenter(Some([3199, 100, 900, 600]), Some(1), [1100, 700]), Some([510, 240, 900, 600]));
        assert_eq!(d.presenter(Some([100, -200, 900, 600]), Some(1), [1100, 700]), Some([510, 240, 900, 600]));
        // Enough of its title bar shows: it stays where it is.
        assert_eq!(d.presenter(Some([1700, 100, 900, 600]), Some(1), [1100, 700]), Some([1700, 100, 900, 600]));
        // Larger than the display: shrunk to fit.
        assert_eq!(d.presenter(None, Some(0), [4000, 3000]), Some([1960, 40, 1200, 640]));
    }

    #[test]
    fn with_one_display_presenter_view_keeps_its_place_over_the_show() {
        let one = Displays { list: vec![Display { x: 0, y: 0, w: 1920, h: 1080 }], editor: Some(0) };
        assert_eq!(one.presenter(Some([100, 100, 800, 500]), None, [1100, 700]), Some([100, 100, 800, 500]));
        assert_eq!(one.presenter(Some([5000, 100, 800, 500]), None, [1100, 700]), Some([560, 290, 800, 500]));
        // No display list (tests, web): whatever was saved, else the system's choice.
        assert_eq!(Displays::default().presenter(Some([1, 2, 3, 4]), None, [1100, 700]), Some([1, 2, 3, 4]));
        assert_eq!(Displays::default().presenter(None, None, [1100, 700]), None);
    }

    #[test]
    fn swap_displays_trades_the_show_and_presenter_view() {
        let d = two();
        // Presenter View on the laptop, the show on the projector: they trade.
        assert_eq!(d.swap(1, [100, 100, 900, 600]), Some((0, [2110, 60, 900, 600])));
        // Both on the laptop: the show goes to the next display, Presenter View stays on the laptop.
        assert_eq!(d.swap(0, [100, 100, 900, 600]), Some((1, [510, 240, 900, 600])));
        let three = Displays { list: [0, 1920, 3840].map(|x| Display { x, y: 0, w: 1920, h: 1080 }).to_vec(), editor: Some(0) };
        assert_eq!(three.swap(2, [2000, 100, 900, 600]).map(|s| s.0), Some(1));
        assert_eq!(three.swap(2, [4000, 100, 900, 600]).map(|s| s.0), Some(0));
        assert_eq!(Displays::default().swap(0, [0, 0, 10, 10]), None);
    }
}
