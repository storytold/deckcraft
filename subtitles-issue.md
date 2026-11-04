## PowerPoint behaviour

Slide Show ▸ Captions & Subtitles has **Always Use Subtitles** and **Subtitle Settings**: spoken language, subtitle language, microphone, position (Bottom/Top overlaid, Below/Above slide) and More Settings. During a show PowerPoint transcribes the microphone and draws live captions over or beside the slide.

## DeckCraft today

The Slide Show ribbon has no Captions & Subtitles group. `docs/slideshow-parity.md` records "Subtitles and live captions: no (AI dimension)", and `docs/parity-checklist.md` has no row for them. Nothing in `show.rs` draws captions or opens a microphone for speech.

## Scope

- Add a `show.subtitles` command and a Captions & Subtitles ribbon group with **Always Use Subtitles** and **Subtitle Settings** (spoken language, subtitle language, microphone, position, More Settings).
- Add a speech-to-text provider behind a small trait so the show can receive partial and final transcript lines without blocking the render thread.
- Draw live captions at the chosen position with the existing theme/text renderer; captions must survive slide changes and clear when the show ends.
- Persist the user settings (not the deck: PowerPoint keeps caption settings per user, not in the presentation). No PPTX change is expected.
- Cover the settings command, provider lifecycle (start/stop/error) and caption layout with tests; include a fake provider for deterministic tests.

## Acceptance

- With subtitles on, speaking produces visible captions; turning them off removes them.
- All four positions and both overlay/side placements work.
- A provider that fails or is unavailable reports once and leaves the show running.
- No crash when the show ends while a recognition stream is active.
