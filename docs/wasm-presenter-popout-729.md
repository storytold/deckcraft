# WASM presenter-view popout (#729)

Tracks https://github.com/storytold/deckcraft/issues/729 (the reporter moved duplicate #726 here).

## Implementation

The browser presenter opens the same DeckCraft web build in a separate, same-origin window.
That window starts its own eframe runner and renders only presenter controls, current/next
previews and notes. The audience window remains the sole owner of slideshow state, animations,
media and navigation. Presenter keyboard input uses the same navigation handler as the native
presenter, so an input is applied once rather than advancing two independent shows.

Messages check the exact origin, source window and a per-opening session identifier. Input
queues and preview dimensions are bounded. Previews use the audience's rendered current frame;
unchanged preview pixels are not retransmitted. Presenter updates are limited to five per second.

If the browser blocks the initial popup, an explicit Open presenter window button calls the
browser API directly from its click handler. Browser policy can still require the user to allow
popups, or choose to open a tab instead of a window. Closing the presenter leaves the audience
running and offers the same button to reopen it. Ending the show closes the presenter. The
presenter reports a closed or unresponsive audience rather than continuing an independent show.
Native presenter view continues to use its existing egui viewport.

No native second-display placement, Linux display-counting policy, laser/rehearsal changes or
other contributors' unmerged presenter work is included.

## Validation

Candidate implementation; not yet qualified for release. Compilation and automated tests are
pending. The original bug was reproduced on the deployed 0.7.0 browser build on 2026-10-14;
that deployment's exact commit is unknown.

Required runtime checks with a synthetic deck:

- Popup allowed and blocked, explicit retry, and independent window positioning.
- Next/Previous, Home/End, numeric navigation, and no double advance on Space/Enter.
- Current/next previews, notes, pause/reset, black/white screens, All Slides and zoom.
- Navigation from both windows, including animation steps and the end screen.
- Close/reopen the presenter, reload it, end/restart the show, and close/reload the audience.
- Rejection of messages from the wrong origin, source window or prior session.
- Native presenter regressions, WASM compilation and the full repository quality gates.
