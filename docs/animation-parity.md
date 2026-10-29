# Transitions and animations: parity with PowerPoint

> **Last reviewed:** 2026-10-14 · **Last updated:** 2026-10-22 · **Change:** minor (Animation Pane ▸ Play From plays from the selected effect; previous: community PR wave 2026-10-14: interactive-deck triggers, canvas preview, the Animation Pane timeline, repeat and bounce end round-trip; duplicated rows and headers folded; previous: major, transitions measured frame by frame against PowerPoint for Windows' video export) · **Target:** Microsoft PowerPoint for Mac 16.113 (Microsoft 365)

Transitions and animations are what make a deck a presentation, and the part of PowerPoint people
notice first when it's missing. Scores roll up into [target-app-parity.md](target-app-parity.md)
(Transitions ~80% ready, Animations ~65%); gaps are in [gaps.md](gaps.md).

## Transitions: 48 of 48

`crates/model/src/anim.rs` `TRANSITIONS` lists every transition in PowerPoint's gallery, in its
three groups, with its Effect Options (now also Page Curl Single, Glitter Hexagons, Shred In/Out
and Zoom and Rotate). All 48 round-trip through PPTX with the elements, attributes and defaults
PowerPoint itself writes (`every_transition_kind_round_trips`,
`transition_xml_matches_powerpoints_elements`, `transition_defaults_read_like_powerpoint`): 145 of
145 PowerPoint-built test slides reopen in PowerPoint with the same effect after a DeckCraft
save.

**Measured against PowerPoint (2026-10-13).** Synthetic two-colour decks with asymmetric markers
were built in PowerPoint for Windows over COM with every transition and option (150 variants,
2 s each), exported with `CreateVideo` at 30 fps, and compared frame by frame with
`deckcraft-cli render-transition` at the same progress (`plan/powerpoint/tools/synth_transitions.ps1`,
`trans_compare.py`). Mean absolute difference over all variants at t = 0.1…0.9 (0–255): **35.7
before, 15.0 after**. Curves (fades are a smoothstep, moves ease in-out; Cover, Uncover and Pan
have their own measured curves), directions and option meanings (e.g. Glitter starts on the
option's side, Ripple's corner is where the wave heads, Wind/Airplane/Origami "Left" is `invX`),
soft edges (Wipe, Split, Shape, Clock, Ripple) and the Dynamic Content transitions moving the
slides' contents over cross-fading backgrounds now follow PowerPoint. 3-D transitions are
perspective-projected cards (`crates/anim/src/transition_geom.rs`) drawn with
perspective-correct subdivided meshes.

| Group | PowerPoint | DeckCraft | Fidelity |
|---|---|---|---|
| Subtle (Morph, Fade, Push, Wipe, Split, Reveal, Cut, Random Bars, Shape, Uncover, Cover, Flash) | 12 | 12 | Matches PowerPoint's frames (mean diff 1–8; Random Bars and Reveal Through Black ~15) |
| Exciting (Fall Over, Drape, Curtains, Wind, Prestige, Fracture, Crush, Peel Off, Page Curl, Airplane, Origami, Dissolve, Checkerboard, Blinds, Clock, Ripple, Honeycomb, Glitter, Vortex, Shred, Switch, Flip, Gallery, Cube, Doors, Box, Comb, Zoom, Random) | 29 | 29 | Perspective 3-D cards with measured motion; cloth, paper and particle effects (Curtains, Peel Off, Page Curl, Airplane, Origami, Fracture, Crush, Honeycomb, Vortex, Shred) are flat approximations of PowerPoint's meshes |
| Dynamic Content (Pan, Ferris Wheel, Conveyor, Rotate, Window, Orbit, Fly Through) | 7 | 7 | Contents move over cross-fading backgrounds, as in PowerPoint (mean diff 5–17) |

| Transition feature | PowerPoint | DeckCraft |
|---|---|---|
| Effect Options per transition | yes | yes |
| Duration, advance on click / after time, Apply To All | yes | yes |
| Transition sounds | yes | yes: Transitions › Sound offers six built-in sounds (synthesised, no sound files shipped), Other Sound…, [Stop Previous Sound] and Loop Until Next Sound; they play in the show and round-trip |
| Preview in the editor | yes | yes |
| Morph: objects move, resize, recolour; unmatched text boxes fade in place; a picture's crop morphs | yes | yes (#119, #118) |
| Morph: Words and Characters | yes | yes (#5) |
| Morph: geometry (circle to star) | yes | yes (#27) |
| Morph: `!!` name forcing, pictures, 3-D models, ink | yes | names partly; no 3-D or ink |
| Frame-by-frame comparison with PowerPoint | | done (Windows, 2026-10-13); headless frames: `deckcraft-cli render-transition` |

## Animations: 64 of ~190 effects

| Class | PowerPoint (gallery + More Effects) | DeckCraft | Missing (examples) |
|---|---:|---:|---|
| Entrance | ~50 | 24 | Plus, Wedge, Basic Zoom, Center Revolve, Compress, Stretch, Boomerang, Credits, Curve Up, Drop, Flip, Float, Pinwheel, Spiral In, Whip, Ascend, Descend, Unfold |
| Emphasis | ~25 | 17 | Brush Color, Bold Reveal, Contrasting Color, Flicker, Grow With Color, Shimmer, Blink |
| Exit | ~50 | 14 | Mirrors of the missing entrance effects, plus Blinds, Box, Checkerboard, Circle, Diamond, Peek Out, Strips as exits |
| Motion paths | 64 presets + custom | 40 + Custom Path (Lines, Arcs, Turns, Loops, Basic shapes such as stars, heart, moon, teardrop; Lines & Curves such as zigzag, waves, S curves, spirals, bounces; Special such as figure 8, bean, peanut, neutron) | Remaining ~24 presets (some Basic shapes, Lines & Curves variants) |
| Media (Play, Pause, Stop) | 3 (+ Seek) | 3 | Seek, bookmarks as triggers |

By how often people use them, the effects present cover nearly all real decks (Appear, Fade, Fly
In, Wipe, Zoom, Float In, Grow/Shrink, Pulse, Spin, motion lines): weighted, ~85% of use; by count,
~34%.

| Animation feature | PowerPoint | DeckCraft |
|---|---|---|
| Start On Click / With Previous / After Previous, duration, delay | yes | yes |
| Effect Options (direction, sequence by paragraph) | yes | yes |
| Effect dialog: sound, after-animation dim/hide, animate text by word/letter, smooth start/end, bounce end, repeat, rewind | yes | partly; repeat and bounce end round-trip; rewind lost on PPTX save (#77) |
| By-paragraph text builds, sub-bullets follow parents | yes | yes; builds edited in PowerPoint (own start per paragraph, other effects in between, by 2nd-level paragraphs) import as one effect per paragraph and keep their clicks (#844) |
| Triggers (on click of a shape) | yes | yes; played in the show: a step per click, the sequence starts over after its last step, a click completes a playing step. Separate click steps, mixed starts and paragraph builds survive PPTX save (`trigger_steps.rs`) |
| Triggers on a media bookmark | yes | no |
| Animation Painter | yes | yes (#54) |
| Animation Pane: list, reorder, remove | yes | yes |
| Animation Pane: advanced timeline (bars you drag) | yes | yes: bars from the show schedule, drag the body for delay, the right edge for duration, seconds ruler (`animpane_tests`) |
| Preview on the editor canvas (the current slide) | yes | yes |
| Preview from the selected effect | yes | yes: with an effect selected the pane's button reads Play From and starts at that effect, earlier effects already played (`animpane_tests`) |
| Motion path editing (edit points, reverse, lock) | yes | partly (no edit points) |
| Multiple animations on one object | yes | yes |
| Round trip of the timing tree | yes | flattened to the pane list on read, rebuilt on write (`every_animation_effect_survives`) |

## Estimates

| Work | Hours |
|---|---|
| Effect dialog depth | 3–5 |
| The remaining ~126 effects (most are parameter variations of existing ones) | 3–6 |
| Rewind and other effect options on round trip (#77) | 1–2 |
| Transition sounds: the rest of PowerPoint's built-in list (Applause, Arrow, Bomb…) | 1 |
| Morph across pictures, `!!` names, align once per transition (#67) | 2–4 |
| Cloth, paper and particle transitions as real meshes (Curtains, Peel Off, Page Curl, Airplane, Origami, Vortex, Shred, Honeycomb) | 4–8 |
| **Total** | **~14–26** (the Transitions and Animations rows of the feature table; part of the Features dimension's 140–240 h) |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-22 | minor | Animation Pane ▸ Play From: with an effect selected, the preview starts at that effect on the play-all clock; the row says yes and the estimate drops to ~14–26 h |
| 2026-10-15 | trivial | PPTX interactive sequences preserve later On Click effects and one click per paragraph; no change to playback or feature scores |
| 2026-10-16 | trivial | PPTX paragraph builds edited in PowerPoint keep their clicks; sub-points save With Previous of their parent (#844) |
| 2026-10-14 | trivial | Community PR wave folded: the Animation Pane timeline row says yes (it landed), preview split into canvas (yes) and from the selected effect (no); estimates drop the timeline and transition sounds (~15–27 h) |
| 2026-10-14 | trivial | Repeat and bounce end round-trip through PPTX; rewind is still lost (#77) |
| 2026-10-13 | minor | Animations ▸ Preview and Animation Pane ▸ Play All play the current slide's animations on the editor canvas; preview from the selected effect is still open |
| 2026-10-13 | major | Transitions measured frame by frame against PowerPoint's video export and retuned (mean diff 35.7 → 15.0); PowerPoint's XML defaults and option meanings; new options |
| 2026-10-13 | minor | Triggers now play in the slide show (they were read and written but never started), effects of different triggers apply in the order they were clicked |
| 2026-10-11 | minor | Morph leaves unrelated text boxes unpaired: they fade in place (#119) |
| 2026-10-11 | minor | Morph animates a changed picture crop (#118) |
| 2026-10-10 | major | Created: 48/48 transitions, 64 of ~190 animation effects, pane and option depth, estimates |
