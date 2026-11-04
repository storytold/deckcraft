set -e
mk() {
  title="$1"; body="$2"
  printf '%s\n' "$body" > issue-body.md
  gh issue create -R storytold/deckcraft --title "$title" --label parity --body-file issue-body.md
}
mk "Record: preview a recorded slide show" '## PowerPoint behaviour
Record ▸ Preview plays the recorded slide show with its narration and camera/screen video before exporting.

## DeckCraft today
The Record tab has no Preview button; `ribbon/slide_show.rs::record` only contains Cameo, From Beginning/Current and Learn More. Recording itself is still an open stack (#614, #624, #636).

## Scope
- After a recording ends, keep the recorded media and timings available to the session.
- Add a Preview button that starts a playback-only show with the recorded narration, ink/laser trace, slide timings and any recorded camera/screen media.
- Preview must not modify the deck and must stop cleanly on Esc.
- Tests: the preview command selects the recorded take; playback uses timings and narration; ending returns to the editor unchanged.

## Acceptance
Record something, press Preview, see/hear it exactly as recorded; Esc returns to the editor; save/reopen is unaffected.'
mk "Record: Cameo camera overlay and recorded Cameo" '## PowerPoint behaviour
Record ▸ Camera ▸ Cameo shows a live camera bubble during the show. The split button chooses Insert To: This Slide or All Slides. With recording on, the camera is captured and stored with the recording.

## DeckCraft today
The Record tab shows a disabled Cameo button. `#1608` asks for the live webcam/Cameo behaviour but has no implementation PR. Camera capture is absent.

## Scope
- Capture the default camera on a worker thread (no blocking on the UI thread).
- Draw the live Cameo bubble at a chosen corner/size during the show; the presenter can move/resize it with the existing selection gestures.
- Insert the Cameo into the current slide or all slides (the split button), and capture the camera while recording.
- Write the recorded camera as a video media shape on each slide that had a Cameo, with timing, so PowerPoint can open it. This modifies the PPTX; round-trip it both ways.
- Handle no camera, permission denied and camera unplugged without stopping the show.
- Tests: device failure, bubble geometry, insert-to scope, recorded media written/read back.

## Acceptance
Cameo previews live, can be placed on this/all slides, records with the show, and survives a PowerPoint round trip.'
mk "Record: complete From Beginning / From Current Slide recording" '## PowerPoint behaviour
Record ▸ From Beginning… and From Current Slide… record the show: slide timings, narration, pen/laser, camera and screen, then keep the recording in the deck.

## DeckCraft today
`show.record` exists in the open stack (#614, #624, #636): timings, ink, laser and microphone narration. Cameo, screen capture, audio-device selection, preview and the recording UI are missing. The Record-tab buttons currently start a rehearsal without recording in main.

## Scope
- Land the timings/ink/narration/laser stack on current main.
- Add the recording state machine: start, pause/resume, stop, and per-slide clocks; record bar with elapsed time and Stop.
- Capture microphone audio per slide and embed it as narration; Play Narrations skips it; Clear removes it.
- Keep the recording after the show; do not drop it when the show ends.
- Cover start/stop, slide changes, empty/quiet microphone, and show-end flush with tests.

## Acceptance
Recording from beginning and from the current slide keeps timings, ink, laser and narration; the deck saves and reopens with them; PowerPoint can read the narration and timings.'
mk "Record: screen recording" '## PowerPoint behaviour
Record ▸ Screen Recording captures a region of the screen and inserts it as a video on the current slide, with the recording available for the show.

## DeckCraft today
No screen-recording control or capture backend exists. The Record tab is a placeholder.

## Scope
- Add a screen-region picker and a capture worker (platform-native where possible; never block the UI thread).
- Encode captured frames with the deck video encoder and insert the result as a video media shape on the current slide.
- Record the screen alongside the show when a recording is active; stop cleanly if the capture source disappears.
- Handle permission denial and unsupported platforms with one reported error and no crash.
- Tests: capture geometry, encoder handoff with synthetic frames, media insert, and PPTX round trip.

## Acceptance
Select a region, record it, stop, and see a playable video shape on the slide; the PPTX opens in PowerPoint with the video.'
mk "Record: record audio and insert it on the current slide" '## PowerPoint behaviour
Record ▸ Audio records from the microphone and inserts the clip as an audio shape on the current slide.

## DeckCraft today
There is no Audio button. PR #624 records narration during a slide show, but there is no standalone record-audio-to-slide flow.

## Scope
- Add an Audio button that opens a small record bar, captures the default microphone (or a selected input device), and inserts the clip as an audio media shape on the current slide.
- Reuse the existing media insert/probe path so the PPTX has a normal audio part and timing; no new file format.
- Handle no device, permission denial, silence and a very short clip without crashing.
- Tests: synthetic PCM to WAV, size/duration validation, insert/undo, PPTX round trip.

## Acceptance
Record speech, stop, and the audio shape appears on the slide and plays in the show; the saved PPTX opens in PowerPoint with the audio.'
mk "Record: Clear Recording on current/all slides" '## PowerPoint behaviour
Record ▸ Edit ▸ Clear Recording offers Clear Recording on Current Slide and Clear Recordings on All Slides. It removes recorded narration, timings, ink/laser and camera/screen media.

## DeckCraft today
Open PR #636 adds clear timing and clear narration commands only. There is no clear-recording command, no camera/screen/ink cleanup, and no Record-tab menu.

## Scope
- Add `show.clearRecording` with current-slide/all-slides scope.
- Remove recorded narration audio, video/cameo media, ink, laser trace and recorded advance timings, with one undo step per scope.
- Keep ordinary (non-recorded) media and user-drawn ink untouched.
- Add the Clear Recording menu to the Record tab (current/all), disabled when there is nothing recorded.
- Tests: mixed recorded/ordinary content, undo/redo, native save/reopen.

## Acceptance
Clear Recording removes only recorded material from the requested scope, is undoable, and leaves ordinary slide content intact.'
mk "Record: Reset to Cameo on current/all slides" '## PowerPoint behaviour
Record ▸ Edit ▸ Reset to Cameo offers Reset to Cameo on Current Slide and Reset to Cameo on All Slides, restoring the default Cameo position/size after it was moved or resized.

## DeckCraft today
No Reset to Cameo control exists; Cameo itself is missing.

## Scope
- Store the Cameo placement per slide (or the deck default) when the user moves/resizes it.
- Add `show.resetCameo` with current/all scope to restore the default corner and size.
- Undoable; a slide without a Cameo is a no-op reported as zero.
- Add the Reset to Cameo menu to the Record tab.
- Tests: moved/resized Cameo, scope, undo, no-op.

## Acceptance
Moving the Cameo then Reset to Cameo restores the default placement on the chosen slides.'
mk "Record: Save as Show button" '## PowerPoint behaviour
Record ▸ Export ▸ Save as Show saves the presentation as a .ppsx that opens directly in slide-show mode.

## DeckCraft today
`.ppsx` save/open exists (#212, #864; docs), but the Record tab has no Save as Show button. Users must use File ▸ Save As.

## Scope
- Add the Save as Show button to the Record tab Export group.
- Reuse the existing .ppsx export path and the current deck; no new format.
- Show the normal save dialog and report success/failure in the status bar.
- Tests: clicking invokes the .ppsx export; the saved file declares the slideshow content type.

## Acceptance
Record ▸ Save as Show writes a .ppsx that opens in slide-show mode in DeckCraft and PowerPoint.'
mk "Record: PowerPoint round-trip coverage for recording features" '## Goal
Every recording feature that writes to the PPTX must round-trip with PowerPoint in both directions and produce no unexplained XML differences.

## Matrix
- Narration audio + timings
- Pen ink and laser trace
- Cameo video
- Screen-recording video
- Recorded audio clip
- Clear Recording / Reset to Cameo

## Procedure
For each feature: create a minimal deck, apply the feature in DeckCraft, save, open in PowerPoint, inspect/save, reopen in DeckCraft; then do the reverse from PowerPoint. Unzip both files and diff `[Content_Types].xml`, `ppt/presentation.xml`, slide XML, media parts, relationships and timing XML. Each diff is either fixed or documented with the public schema element that explains it.

## Acceptance
A checked-in round-trip script/test corpus covers the matrix, and the remaining XML differences are zero or documented per feature.'
