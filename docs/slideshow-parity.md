# Slide show and presenter view: parity with PowerPoint

> **Last reviewed:** 2026-10-14 · **Last updated:** 2026-10-24 · **Change:** minor (Ctrl+wheel zooms the show around the pointer, #1286; previous: major, community PR wave 2026-10-14: interactive decks, laser pointer, See All Slides and zoom, go-to-slide number, custom show playback, presenter timer, clock, notes size and keyboard, Show Media Controls, swipe; breadth 68%; duplicated rows and headers folded; previous: major, created) · **Target:** Microsoft PowerPoint for Mac 16.113 (Microsoft 365)

Presenting is the job PowerPoint is hired for. This doc covers the show window, presenter view,
rehearsal and recording. Transition and animation playback are in
[animation-parity.md](animation-parity.md); display hardware in
[hardware-parity.md](hardware-parity.md). Area score: checklist breadth **68%** (measured), **~60%
ready** (estimated), **10–16 h** to parity.

## Running the show

| Feature | PowerPoint | DeckCraft | Notes |
|---|---|---|---|
| Play from Start / from Current Slide (F5, Shift+F5, Cmd+Enter) | yes | yes | |
| Next/previous by click, arrows, Space, Page Up/Down, Enter, Backspace | yes | yes | #17 reported arrows not working in v0.3.0 AppImage; recheck |
| Go to slide (number + Enter), first/last | yes | yes: the digits show as "Go to slide …" until Enter, Backspace, Esc, or 3 seconds | Slide numbers are 1-based; 0 and a number past the end do not move |
| Next/previous by trackpad swipe or mouse wheel | yes | yes | One flick turns one page. The pen ignores the flick |
| Go to slide (number + Enter), first/last | yes | yes | |
| Black / white screen (B / W) | yes | yes | |
| End screen, Esc to leave | yes | yes | |
| Step clock: a click after a long pause plays its step (#11) | yes | yes | |
| Hidden slides skipped | yes | yes | |
| Hyperlinks and action settings followed (slides, web, mail, End Show, Last Viewed) | yes | yes | Only `http(s)` and `mailto` open outside the show, by design (`safe_url`) |
| A shape with a triggered animation plays it when clicked; shapes an animation has hidden cannot be clicked; a shape without an action on top of one with an action blocks it; the pointer is a hand over what can be clicked | yes | yes | Click boxes are bounding boxes; "play sound" actions (`ppaction://media`) do nothing yet |
| A slide the show returns to (link, arrow keys) looks as it was left; a triggered animation goes on where it stopped | yes | yes | A main-sequence step still playing when the show leaves is finished |
| "Advance on click" off: a click never leaves the slide (animation steps still play, keys still move on) | yes | yes | |
| Hyperlinks and action settings followed (slides, web, mail, End Show, Last Viewed, Custom Show) | yes | yes | Only `http(s)` and `mailto` open outside the show, by design (`safe_url`). A Custom Show action plays that show |
| Media playback with controls, play across slides, full-screen video, Play/Pause/Stop in the click sequence | yes | yes | WMA/WMV not decodable. Click sequence since #116. Slide Show ▸ Show Media Controls (on by default) hides the playback bar. Seeks and loops preserve new-generation video frames and end-of-stream; queue regression and repeated seeks across H.264/HEVC/VP9/AV1 in `crates/media` |
| Animated GIFs play; pause from their corner button | yes | yes (#39) | The corner button hides with Show Media Controls |
| See All Slides grid during the show | yes | yes: G, or All on the show bar; click a thumbnail, Esc closes the grid | Hidden slides are labelled; the first 48 slides are shown |
| Zoom into part of a slide during the show | yes | yes: Z, then click the slide (or the presenter slide); 2×, Esc or another click leaves it. Ctrl+wheel (Cmd+wheel on a Mac) over the slide zooms in and out around the pointer, four notches per doubling, up to 4× (#1286) | The presenter keeps the whole slide and draws a frame on the zoomed part. The wheel zooms in the audience window only; no panning while zoomed |
| Pen and highlighter ink | yes | pen only (Cmd+P pen, Cmd+A arrow, E erases all ink) | No highlighter, per-stroke eraser, ink colours, or keep/discard annotations prompt |
| Laser pointer | yes | yes in the show | Cmd/Ctrl+L and the on-screen button draw the show's laser colour and leave no ink. The presenter window does not move that dot |
| Subtitles and live captions | yes (cloud speech) | no | AI dimension |
| Custom shows | yes | yes: Show plays that list in order and does not change Set Up; Set Up Slide Show can keep one for Play from Start | Hidden slides in the list are skipped |
| Set Up Show: loop until Esc, kiosk (browsed at a kiosk), show without animation/narration, pen colour, slides range | yes | yes, most options | |
| Use rehearsed timings | yes | yes | |

## Presenter view

| Feature | PowerPoint | DeckCraft | Notes |
|---|---|---|---|
| Separate presenter window with current slide, next slide, notes | yes | yes (an egui viewport, 1100×700); the notes keep their bullets, numbers, indents and bold/italic, as in the notes pane | |
| Elapsed timer | yes | yes | |
| Pause / reset timer, clock | yes | yes: Pause/Resume and Reset in presenter view (T pauses, R resets); the clock is local time, `--:--` where there is no clock | Pausing the timer does not pause the show, animations or rehearsal |
| Slide counter, previous/next/end buttons | yes | yes | |
| Keyboard navigation while Presenter View has focus | yes | yes | Shares the audience window's key handling; handled keys are consumed before buttons, and the presenter stays available at the end screen |
| Notes font size, scrolling | yes | yes: A- / A+ (and `[` / `]`), 12–48 pt in steps of 4, default 18; the notes scroll | |
| Placed on the presenter's display automatically, show on the projector | yes | **no**: opens wherever the OS puts it | Beta blocker, 3–5 h. Open PR #433 places the show on a second display when one is attached. This change does not |
| Swap displays | yes | no | Still open on #433 |
| Pen, laser, black screen, zoom from presenter view | yes | Black, White and Zoom buttons; the audience zoom is framed on the presenter slide | Pen and laser are driven from the audience window only; the presenter window does not move the laser dot |
| See all slides from presenter view | yes | yes: All slides (or G) opens the grid in presenter view only, the audience keeps the slide; click a thumbnail to go there, Esc closes it | |
| Rehearse with Coach (Speaker Coach) | yes (cloud) | no | AI dimension |

## Rehearse and record

| Feature | PowerPoint | DeckCraft |
|---|---|---|
| Rehearse Timings, keep timings | yes | yes |
| Record Slide Show (narration, ink, laser, camera; Cameo) | yes | no |
| Clear timing / narration | yes | timings only |
| Export the recorded show to video | yes | no (see [file-format-parity.md](file-format-parity.md)) |

## Reading View

Reading View in a window with navigation: present (checklist row done).

## Estimates

| Work | Hours |
|---|---|
| Highlighter, eraser, keep-annotations prompt, laser from presenter view | 2–4 |
| Presenter view on the right display and a swap-displays command | 2–4 |
| Record slide show with narration (no camera) | 5–8 |
| **Total** | **~9–16** (the slide-show row of the feature table carries 10–16 h; recording overlaps the Media row) |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-22 | minor | See All Slides from presenter view opens in presenter view, not on the audience screen (#1130) |
| 2026-10-14 | trivial | Community PR wave folded: one media playback row, the presenter pen/laser row matches the laser that exists, estimates keep only the open work (~9–16 h); checklist breadth 68% |
| 2026-10-13 | minor | Click handling for interactive decks: triggers, per-slide state kept across visits, visible-shape hit testing with hand cursor, advance on click |
| 2026-10-13 | minor | Video seek handoff no longer discards freshly decoded frames or end-of-stream (upstream #416); deterministic queue and four-codec seek regression tests |
| 2026-10-14 | minor | Presenter View handles slideshow keys in its own viewport, including returning from the end screen and Escape; regression tests cover navigation and avoiding duplicate button activation |
| 2026-10-11 | minor | Media effects (Play, Pause, Stop) in the click sequence play with their step (#116) |
| 2026-10-13 | minor | The show has a laser pointer (Cmd/Ctrl+L, the show's laser colour, no ink). Presenter view still does not drive it |
| 2026-10-14 | trivial | Show Media Controls hides the show's playback bar and GIF button |
| 2026-10-14 | minor | Typing a slide number during the show shows it, and Enter goes there. Esc clears the number before it leaves the show. Checklist breadth stays 66% |
| 2026-10-14 | minor | A custom show plays only its slides, from the Custom Shows dialog, from Set Up Slide Show, or from a click action. Checklist breadth stays 66% |
| 2026-10-14 | minor | Presenter view pauses and resets its timer (T, R) and changes notes size (A-/A+, brackets). The clock shows local time. Second-display placement and swap stay with #433. Checklist breadth stays 66% |
| 2026-10-14 | minor | See All Slides (G) and click-to-zoom (Z) during the show. Presenter view opens that grid and sets black, white and zoom. Checklist breadth stays 66% |
| 2026-10-24 | minor | Ctrl+wheel (Cmd+wheel on a Mac) zooms the show in and out around the pointer, up to 4×; the plain wheel still turns the page (#1286) |
| 2026-10-23 | trivial | Presenter view draws the notes with their formatting (bullets, numbers, indents, bold/italic) instead of plain text (#1128) |
| 2026-10-10 | major | Created: show, presenter view, rehearse and record, compared with PowerPoint 16.113's Slide Show tab and menu |
