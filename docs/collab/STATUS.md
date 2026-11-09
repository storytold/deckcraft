# Collaboration status and handoff

Updated: 2026-11-09. Branch: `collab/pr10` (media slice `7fe75711`, feature slices `0a57b61e`…`69c2ff22`, undo of fill/line/format `41fe1ad9`, undo of a group's fill/line and structural slide undo `4e666b95`, structural undo of shapes `3ec81d70`, cabinet commit publication `31842050`, undo of group/ungroup `98d9e9ce`, align/distribute as final bounds `31d353fe`, the TASK.md §8 gates and the non-active-slide fix `5b078c7d`, docs commits on top).
The production operator view is
[`production.md`](production.md); the protocol design is [`rfc.md`](rfc.md); the prepared PR
description is [`pr10.md`](pr10.md).

This file is the handoff between sessions: what is done, how it was verified, what is not done,
and what to do next. Update it at the end of every collaboration session.

## Branch map

| Branch | Commit | Slice |
|---|---|---|
| `collab/study` | — | Phase 0 study/options |
| `collab/rfc` | `bcd68aa2` | RFC from ADR 0001–0010 |
| `collab/pr1` | `78de01b8` | wire types, adapter, `apply_remote` |
| `collab/pr2` | `92577c30` | per-user undo/redo |
| `collab/pr3` | `e9a18903` | `DocSession`: versions, dedup, snapshots, commit |
| `collab/pr4` | `62652fb1` | `ClientSession`: queue, dedup, resync, viewer |
| `collab/pr5` | `4ec6b3a8` | commit/409, presence, two-client e2e |
| `collab/pr6` | `1d07ce21` | UI: flag, indicator, participants, presence, viewer gate |
| `collab/pr7` | `d28c8ae5` | `CollabDoc` command glue + in-memory `Hub` |
| `collab/pr8` | `759b6124` | WebSocket server, desktop link, UI wiring, undo requests, persistence |
| `collab/pr9` | `149d9840` | text insert/delete with rebase, shape insert, engine command queue |
| `collab/pr10` | `7fe75711` + `0a57b61e`…`69c2ff22`, `41fe1ad9`, `4e666b95`, `3ec81d70`, `31842050`, `98d9e9ce`, `f28ee048` | media channel (pictures by `sha256`, upload/fetch, persistence); audio/video, TLS media client, `shape.fill`/`shape.line`/`format.*`/`slide.duplicate`, `arrange.align`/`arrange.distribute` as final bounds; undo of fill, line, `format.*`, a group's fill/line, slide insert/delete, shape insert/delete, group/ungroup and align/distribute; commit published to the cabinet as a new file version (`409` on an external change) |

The Commit column names the slice commits; each branch also carries documentation commits. Heads:
`collab/pr9` = `47ec08fe`, `collab/pr10` = `7fe75711`, `0a57b61e`, `58eee059`, `69c2ff22`,
`41fe1ad9`, `4e666b95`, `f6281736` (journal replay of history operations), `3ec81d70` (shape
insert/delete undo), `31842050` (cabinet commit publication), `f28ee048` (docs), `98d9e9ce`
(group/ungroup undo), `31d353fe` (align/distribute as one final-bounds step), `5b078c7d`
(the §8 gates and the non-active-slide fix), `556c00d9` (three-session gate and the reconnect
catch-up fix), plus the docs commits on top. The working tree is clean: the chair's
uncommitted finding of 2026-11-08 is `7fe75711`; the second slice added `0a57b61e` (media kinds,
TLS, command slices, tests, docs), `69c2ff22` hardened the format command, `41fe1ad9` added undo
of fill/line/format steps, `4e666b95` added undo of a group's fill/line and of slide
insert/delete, `3ec81d70` added undo of a shape insert and a shape delete (whole-shape image with
z-order, animations and triggers), `31842050` sends a commit to the cabinet, which creates the new
version and keeps the original, `98d9e9ce` makes `arrange.group`/`arrange.ungroup` undoable
with whole-subtree images on both sides, and `31d353fe` carries `arrange.align`/`arrange.distribute`
as one final-bounds operation with one undo step.

The published head of the fork branch `copoka/deckcraft:collab/pr10` is `63ec1ebe`, open as
[PR #1975](https://github.com/storytold/deckcraft/pull/1975). Local `collab/pr10` is ahead of it by
`3ec81d70`, `ded9d1ee`, `31842050`, `f28ee048`, `98d9e9ce`, `31d353fe`, `5b078c7d` and
`556c00d9`. `git fetch fork` shows no commit on the fork side that is missing locally, and a
`git push --dry-run` from this workspace is refused by the environment policy, so publishing the
new head is a manual step. Nothing was pushed to `storytold/deckcraft`; the PR comes from the
fork and a human decision still governs merging it. The PR body is prepared in [`pr10.md`](pr10.md):
title `feat(collab): realtime collaborative editing`, one description, a review order over the
slice stack and the `PR-CHECKLIST.md` mapping. Do not push it without the human decision.

## What is implemented

### Protocol and adapter (`crates/collab-proto`, `crates/collab`)

- Envelope `hello/welcome/op/applied/rejected/presence/resync/snapshot/commit-result/error/ping`
  with the DeckCraft payloads; `undo`/`redo` are client requests, the server owns the inverse.
- Session keys (`slideKey`, `shapeKey`) derived identically on the server and the client from
  `fileId` plus the same document walk; tombstones refuse late operations.
- Authoritative ordering: one server `version`, `opId` dedup with a 10 000-entry window,
  structural operations exclusive at the head, `applied` broadcast to everyone **with the
  confirmed command** so every replica runs the same command in the same order.
- Delta of changed fields (no values, no derived state): group frames, connector routes and
  autofit are recomputed on each replica.
- Per-actor undo: server-side before/after images, `undo-conflict` when someone else changed the
  object, undo is an ordinary new operation. The images cover `xfrm`, text, fill, outline and the
  whole `TextBody` of a `format.*` step; the last three travel as an exact model value
  (`shape_field`) because the user-facing fill/line/format commands cannot express the whole
  model, and the adapter accepts that payload only as an undo/redo of the actor's own latest step.
  A group's fill/line step records one image per changed member and restores them atomically with
  `shape_fields` (engine `shape.setFields`, values validated before the first write). An
  `arrange.align`/`distribute` step records one `xfrm` image per moved shape and restores them in
  one `shapes_set_bounds` (engine `shape.setBoundsMany`, ids and values validated before the first
  write), so a selection-wide undo is one operation, not one per object. A
  `slide.delete`, `slide.new` or `slide.duplicate` records the whole slide (model JSON, position,
  section and custom-show membership); undo restores it with `slide_restore`, redo deletes it.
  A shape insert (`shape.insert`, `insert.picture`, `insert.video`/`insert.audio`) and
  `edit.delete` record the **whole shape**: model JSON with its id, the group that held it, its
  position in the z-order and the animations that named it (including foreign effects it
  triggered). Undo `edit.delete` is one `shape_restore` (`items` of whole shapes, a group image
  carrying its children), redo is `shape_delete`; undo of an insert is `shape_delete` and its redo
  is `shape_restore`. The engine command `shape.restore` validates and deserialises every image
  before the first insert, refuses a duplicate id, and clamps the recorded sibling position to the
  list it finds, so neighbours that came or went do not block an undo.
  Every history move is checked against the recorded image and address (`check_restore`), so a
  client cannot send an undo envelope for another object or with another value.
- Presence: ephemeral, latest-state-wins, 30 s TTL, FNV colours, never enters a version.

### Client (`CollabDoc`, `CollabUi`, `WsTransport`)

- `CollabDoc` normalises commands to absolute/explicit operations and applies the confirmed
  commands; keys are derived, not transferred.
- Unanswered operations go back to the front of the queue on a disconnect and are resent with
  the same `opId`; the server dedups them.
- Desktop `WsTransport`: one background thread, two channels, 50 ms read timeout, rustls for
  `wss://`, reconnects with the same handshake.
- UI behind a flag (`DECKCRAFT_COLLAB`/`?collab=1`): status indicator, participants pane, remote
  selection frames and carets, commit warnings banner, viewer lock.
- `Session::set_collab_queue`: with a room attached, every mutating path (ribbon, keys, pointer,
  IME, control channel) queues the command instead of executing it; `CollabUi::pump` sends it.
  `apply_remote` still applies the server's command directly.
- Text: `text.insert`/`text.delete` carry character offsets; the server rebases them against the
  journal tail since `baseVersion` (inserts shift right, deletes pull left and clamp), so two
  people typing in one paragraph keep both texts. Confirmed commands carry the rebased offset.
- `shape.insert`: preset, final geometry, optional text; ids are deterministic on all replicas.
- `insert.picture`: the operation carries `sha256`, `name` and an optional final rect, never bytes.
  The server stores one object per digest (`PUT /v1/sessions/{id}/media/{sha256}`), every replica
  fetches it (`GET /media/{sha256}`), and a journal replay loads the object before the operation.
- `insert.video` / `insert.audio`: the same media channel with their own allowlist; the payload adds
  `kind`. The media cache keeps separate budgets for pictures and for audio/video, so a clip cannot
  evict pictures. The desktop link derives the HTTP scheme from the WebSocket link: `ws://` is a
  plain socket, `wss://` a `rustls` connection with the platform root store.
- `shape.fill`: a final fill (solid colour, `none`, `background`), one operation per shape.
- `shape.line`: final outline values (colour, width, dash, cap, join, compound, heads) or `none`.
- `format.*`: 17 allowlisted commands (`bold`, `italic`, `underline`, `strikethrough`,
  `superscript`, `subscript`, `font`, `size`, `color`, `highlight`, `alignLeft/Center/Right`,
  `justify`, `caps`, `spacing`, `clear`) over one explicit shape's text. Toggles need an explicit
  `on`; `args` cannot carry `id`/`ids`/`index`; a table or a shape without text is refused.
- `slide.duplicate`: structural, addressed by the source slide key; the copy is inserted after it.
- `arrange.align`/`arrange.distribute`: one `shapes_set_bounds` operation with the final box of
  every moved shape. The client takes the values from the engine's own plan (`align_plan`,
  `distribute_plan`), so no replica recomputes the slide edge, the selection frame or the gaps;
  the adapter runs `shape.setBoundsMany`, which checks every id and duplicate before the first
  write and moves the whole selection in one `edit`. A shape whose box does not change is left out;
  an align that moves nothing sends nothing.
- Undo/redo: move, bounds, align/distribute, text, fill, outline and `format.*` steps. A restore carries the exact
  model value (`shape_field`) and runs through the `shape.setField` engine command, which bounds
  the JSON and deserialises it into the model type. A group's fill/outline step records an image
  per changed member and restores all of them in one `shape_fields` operation (`shape.setFields`
  validates every value before the first write). `slide.delete`, `slide.new` and `slide.duplicate`
  record the whole slide and undo through `slide_restore` / `slide_delete`. `shape.insert`,
  `insert.picture`, `insert.video`/`insert.audio` and `edit.delete` record the whole shape and undo
  through `shape_restore` / `shape_delete` (z-order, animations and triggers included, group images
  carry their children). `arrange.group` and `arrange.ungroup` record a whole-subtree image on both
  sides (the group with its children, or every former member at the top level) and move between
  them in one `shape_restore{items, remove}`: the engine removes the other side's shapes first
  without dropping animations, then inserts the recorded images, all validated before the first
  write.

### Server (`apps/deckcraft-collab-server`)

- HTTP: `POST/GET/DELETE /v1/sessions`, `GET /healthz`, `/readyz`, `/metrics`.
- WS: first frame is `hello` with the JWT; signature/`exp`/`aud`/app/file/session checks; actor
  and role from the token. Viewer `op` is refused before the engine. Viewer `commit` is refused too.
- Limits: message size, operations/second, participants.
- Commit: `.pptx` from the authoritative document, `sha256`, `warnings[]`, `409` on a stale
  `expectedVersion`. The server POSTs it to the cabinet (`COLLAB_CABINET_URL`, service token):
  `{fileId, name?, bytes(base64), sha256, version, expectedVersion?}` → `{version, etag}`; the
  cabinet creates a new file version and keeps the previous one. A cabinet `409` (the file changed
  outside the session) is the caller's `409`, and no version is created. The accepted `etag` is
  stored in the session and in `meta.json` (a restart keeps comparing against it). Without a
  callback, `POST /v1/sessions/{id}/commit` returns the bytes inline (development mode); a client
  `commit` is answered only after the cabinet took the file.
- Media: `PUT /v1/sessions/{id}/media/{sha256}` (session token, writer only, signature allowlist
  for pictures/audio/video, 8 MiB per picture, 32 MiB per clip), `GET /media/{sha256}` (session or
  service token). Content-addressed under `<store>/media/<sha256>`; 512 pictures / 64 MiB and 32
  clips / 192 MiB per session; upload and byte counters in metrics.
- Persistence: native base + append-only journal, snapshot every N ops, replay on restart
  through the same `apply` path. Without `COLLAB_STORE_DIR` the server is memory-only.
- Logs/metrics contain ids, versions and counts; no slide content, text, media or tokens.

## 8. Gates (TASK.md §8)

All three gates are tests, not scripts: `apps/deckcraft-collab-server/tests/gates.rs`. Run them with

```bash
cargo test -p deckcraft-collab-server --test gates -- --nocapture --test-threads=1
```

- **Large deck with media.** 120 slides, each with a decoded picture (four content-addressed media
  objects, one per slide reused) and an autoshape, opened as an authoritative room; two editor
  sessions submit a `shape.move` for every slide. On the build host (`agents`, 20 cores, Ubuntu
  24.04, rust 1.99.1, test profile: `opt-level = 1`, dependencies at `opt-level = 2`) the
  authoritative apply took 0.1 ms p50 / 0.1 ms p95 / 0.2 ms max over 120 steps, the room opened in
  4 ms, the commit built a 154 KiB `.pptx` in 15 ms, and an unchanged commit came from the cache
  in 0.3 ms (the same remote-built test binary, run where LibreOffice exists, measured 0.2/0.2/0.7
  ms and a 63 ms commit). Both clients settle at version 120 and every picture moved. The
  assertions use generous ceilings (2 s per apply, 60 s per commit), so the test catches a
  pathological regression rather than machine noise; the numbers above are the evidence.
- **LibreOffice open/repair.** A 120-slide deck with media goes through the real commit endpoint
  (`POST /v1/sessions/gate/commit`, bytes inline, no cabinet), then LibreOffice 26.2.6.3
  (`soffice --headless --convert-to`) opens the result, renders 121 pages (the 120 slides plus the
  slide inserted in the session, counted with `pdfinfo`) in 4.2 s, and a `pptx → pptx` round trip
  takes 5.3 s and reimports as 121 slides with 120 pictures. A non-zero exit, an `Error:` line, a
  missing or empty output, or a page-count mismatch fails the test. The build host has no
  `soffice` or `pdfinfo`, so there the test prints a skip line and passes; the gate was run with
  the remote-built binary on the workstation, which has LibreOffice 26.2.6.3.
- **Three sessions: conflict and reconnect.** Two editor sockets and one viewer socket join one
  room built from a two-slide deck (one rectangle per slide). Alice moves the first slide's
  rectangle; Bob moves the *same* shape before seeing that step. The payload is absolute, so the
  later accepted value wins deterministically (x = 300) on all three replicas, and Alice's undo is
  refused with `undo-conflict` because it would overwrite Bob's later change; the refused step
  moves no version and loses no data. Bob then edits the second slide (the editors are not forced
  onto one active slide), drops his socket, Alice edits the second slide again (version 4), the
  viewer's op is refused `forbidden` without moving the version, and Bob reconnects with the
  version his replica has. The server answers the resync with the journal tail after that version;
  Bob catches up to version 4. All three clients and all three local replicas settle on one
  version and an equal `Presentation`, and a commit at version 4 reimports with the conflict's
  winner and the later move.

The gates found a real adapter bug: `shape.move`, `shape.setBounds` and `text.set` ran against the
engine *selection*, so a replica whose active slide was another one refused a remote edit on
slide 5 with `no such shape`. Those payloads now select the target shape (and its slide) first.
`crates/collab/src/apply.rs`
`an_operation_on_another_slide_applies_whatever_the_replica_is_looking_at` fails without the fix,
and the 120-slide gate now moves a picture on every slide.

The three-session gate found a reconnect bug in `ClientSession`: a `welcome` ahead of the local
replica was adopted as the current head before the resync, so the client asked for the tail after
the head and kept a stale document while claiming to be current. The welcome no longer moves the
version when there is a gap, and the resync names the version the replica actually has
(`a_reconnect_asks_for_the_tail_from_the_version_the_replica_has`). The gate exercises the real
path: a replica that missed version 4 replays it from the snapshot tail and lands on the same
document as the editors.

## Verification

| Command | Result |
|---|---|
| `cargo test -p deckcraft-collab` | 124 unit + 8 integration |
| `cargo test -p deckcraft-collab-server --test gates` (on `agents`, then the same binary where LibreOffice exists) | 3 passed: the 120-slide/media room and commit; three sessions (two editors on different slides and a viewer) across a conflicting edit, an undo-conflict, a forbidden viewer op and a reconnect with journal catch-up, all converging on version 4; LibreOffice 26.2 renders 121 pages and round-trips the commit; on the build host the LibreOffice test skips itself (no `soffice`) |
| `cargo test -p deckcraft-collab --lib apply::` | 124 (a remote edit on a slide the replica is not looking at applies) |
| `cargo test -p deckcraft-collab-server` | 10 unit + 6 socket tests (two editors, viewer, health/metrics, invalid token, restart replay, media upload/fetch, picture insert, audio allowlist, shape-undo replay) + 4 commit tests (two copies with their originals kept, an external change is a `409`, a client commit answered after publication, the no-callback mode) + 3 gates |
| `cargo test -p deckcraft-collab-proto` | 20 |
| `cargo test -p deckcraft-ui-egui collab::` | 17 |
| `cargo test -p deckcraft-engine` | 569 passed, 1 ignored |
| `cargo clippy -p deckcraft-engine -p deckcraft-collab -p deckcraft-collab-proto -p deckcraft-collab-server -p deckcraft-ui-egui --all-targets -- -D warnings` | clean |
| `cargo fmt --check` | clean |
| `cargo xtask layers` | 31 crates, no violations |

Re-run on 2026-11-08 after `3ec81d70`: `fmt` clean, `collab-proto` 19, `collab` 113 + 7,
`collab-server` 5 + 6, `ui-egui collab::` 17, `deckcraft-engine` 565 + 1 ignored; clippy over the
four collab crates and `ui-egui` with `-D warnings` clean; `xtask layers` 31 crates.

Re-run on 2026-11-08 after `31842050`: `fmt` clean, `collab-proto` 19, `collab` 113 + 7,
`collab-server` 10 + 6 + 4 (`tests/commit.rs`), `ui-egui collab::` 17, `deckcraft-engine` 565 + 1
ignored; clippy over the four collab crates and `ui-egui` with `-D warnings` clean; `xtask layers`
31 crates.

Re-run on 2026-11-09 after `98d9e9ce`: `fmt` clean, `collab-proto` 19, `collab` 118 + 7,
`collab-server` 10 + 6 + 4, `ui-egui collab::` 17, `deckcraft-engine` 567 + 1 ignored; clippy over
the four collab crates and `ui-egui` with `-D warnings` clean; `xtask layers` 31 crates.

Re-run on 2026-11-09 after the `arrange.align`/`arrange.distribute` slice: `fmt` clean,
`collab-proto` 20, `collab` 122 + 8, `collab-server` 10 + 6 + 4, `ui-egui collab::` 17,
`deckcraft-engine` 569 + 1 ignored; clippy over the four collab crates and `ui-egui` with
`-D warnings` clean; `xtask layers` 31 crates.

Re-run on 2026-11-09 after the three-session gate (`556c00d9`): `fmt --check` clean,
`collab-proto` 20, `collab` 124 + 8, `collab-server` 10 unit + 4 commit + 3 gates + 6 socket,
`ui-egui collab::` 17, `deckcraft-engine` 569 + 1 ignored; clippy over the four collab crates and
`ui-egui` with `-D warnings` clean; `xtask layers` 31 crates. The three session gate runs in
0.95 s on `agents` (it includes two 300 ms waits for the server to drop the old peer and two
round trips); its document comparisons are `Presentation` equality on three local replicas, and
the commit at version 4 reimports with the expected two slides.

The socket tests were run against a real loopback server; the restart test creates a room, moves
the version, shuts the server down, starts a new one on the same store and asserts the version
and document came back. The media tests cover a rejected upload (viewer, wrong digest, no token),
one object for repeated bytes, a picture applied from a reference, an audio upload with its own
allowlist, and a journal replay that loads the object before running the operation. The two-client
convergence tests compare the delta of a fill/line/format/duplicate step on two adapters and the
same MediaInsert applied on both replicas. Undo of a fill, an outline and a `format.*` step is
tested on one replica and on two (the restore is an ordinary confirmed command, so both replicas
converge); a foreign fill or a foreign format on the same object is refused with `undo-conflict`,
a forged `shape_field` (no history step, or a value other than the recorded one) is refused with
`invalid`. Undo of a group's fill restores the group and both members with one `shape_fields`
operation on two replicas, and a foreign repaint of one member is refused with `undo-conflict`.
Undo of `slide.delete` restores the whole slide (position, content, section membership) on two
replicas and redo deletes it again; undo of `slide.new` deletes the slide and redo restores it;
a forged `slide_restore` aimed at another slide is refused with `invalid`. A shape insert is
undone after a foreign edit of a *neighbour* (both replicas equal, the neighbour's move stays) and
refused after a foreign edit of the *inserted* shape itself (`undo-conflict`); a shape delete is
undone after a foreign edit of the neighbour that is left, restoring the shape at its z-order with
its animation, and a deleted group comes back as one image carrying its children. Replay of a
`shape_restore` after a restart is covered by the store test, which also proves the restored shape
keeps its key. Grouping is undone on two replicas as one `shape_restore` that removes the group and
puts both members back at their z-order; redo removes the members and restores the whole group
image. Ungrouping is undone the other way round (the group image comes back, the members go out),
and a foreign move of a member refuses the group undo with `undo-conflict`. The engine tests cover
`slide.restore` (index, section, duplicate id), `shape.setFields` (a bad value refuses the whole
merge), `shape.restore` (z-order, animation, group child, duplicate id, missing parent) and
`shape.restore` with `remove` (group ↔ members in one step, a missing removal refuses before any
write), plus `shape.setBoundsMany` (one undo step for the whole selection, a missing or repeated
id refuses before the first write, a locked target is left alone).

The commit tests run the server against a fake cabinet on loopback that keeps versions like the
cabinet does. Two copies of the same original file are opened as two rooms; each commit posts a
`.pptx` that reimports with its own edit, both cabinets keep their original version byte for byte,
and a name passed at session creation reaches the callback. The `409` test changes the cabinet's
file under a live room: the server posts the commit anyway, the cabinet refuses it, no version is
created, the original stays, and the conflict counters move (`cabinet_conflicts_total`). A commit
whose `expectedVersion` does not match the room is refused before any callback is made.

## TASK.md acceptance matrix

| Criterion | State | Where / why |
|---|---|---|
| Two or more clients edit one presentation | ✅ for the v1 command set | hub, socket test, UI pipe test |
| A 100+ slide deck with media stays responsive and its commit opens in LibreOffice | ✅ 120 slides/120 pictures, 4 media objects; commit renders 121 pages and survives an LO round trip | `tests/gates.rs`; adapter fix for edits on a non-active slide |
| Insert a picture in a shared session | ✅ pictures, audio and video over `ws://` and `wss://` | media channel, adapter and socket tests |
| Cursors/selections/participants visible | ✅ | `CollabUi::marks`, participants pane |
| Slides, shapes and order converge | ✅ for move/bounds/align/distribute/insert/delete/slide/group/order/fill/line/format/duplicate | collab tests |
| Reconnect works | ✅ within the retained journal tail, including the version the replica actually has | client queue + resync; three-session gate; base transfer for an evicted tail is still open |
| Per-user undo/redo | ✅ text/shape values, align/distribute, fill, outline, `format.*`, group fill/line, slide insert/delete, shape insert/delete, group/ungroup | server history, `undo-conflict`, `shape_field`/`shape_fields`/`shapes_set_bounds`/`shape_restore`/`slide_restore` (the last with `remove`) |
| Viewer cannot edit | ✅ server + engine gate, also inside the three-session gate | socket test, UI test |
| Commit creates a new version, original kept | ✅ server side: `POST /v1/sessions/{id}/commit` → cabinet callback with the bytes, `409` on an external change, the accepted version stored in `meta.json` | `apps/deckcraft-collab-server` (`cabinet.rs`, `lib.rs`), `tests/commit.rs`; the cabinet endpoint and the desktop save command remain external |
| Single-user mode unchanged; feature behind a flag | ✅ | flag-off tests |
| RFC, protocol, tests, docs | ✅ RFC v1 accepted 4/5 (ADR-0015, 2026-11-08); formal `proposal` `…-47b7` | this tree, bus decision `20261108T143417Z-craft-8036` |
| PR in `storytold/deckcraft` | ✅ open as [#1975](https://github.com/storytold/deckcraft/pull/1975) from the fork, MERGEABLE at the last check; local head is ahead of the PR head and the push is manual | [`pr10.md`](pr10.md); merge needs a human decision |

## What is NOT done (ordered by production impact)

1. **Command surface beyond the new slices.** Fill, line, 17 `format.*` commands over whole shape
   text and `slide.duplicate` are collaborative now. Still refused: fonts as a document asset,
   animations, transitions, tables/SmartArt/charts, notes, run-range formatting and relative
   effects. The design rule stands: only commands whose result is a deterministic function of the
   authoritative state, expressed as final values, go on the wire; a raw `execute` is not safe
   (toggles and relative edits diverge). Next candidates: table cell edits, transitions and
   animation steps.
2. **Media refinement.** Fonts and 3D models are not in the media allowlist, and the browser link
   still needs the same HTTP bridge. The desktop `wss://` path has a TLS client now (system roots;
   a private CA must be in the store or `SSL_CERT_FILE`).
3. **Text in tables and notes, rich runs.** Shape text is covered; table cells and notes need
   their own addressing and rebase. Formatting runs need a run-level payload, not whole-body.
4. **Structural undo is covered; size limits remain.** A group's fill/outline (one image per
   changed member, one `shape_fields`), `slide.delete`/`slide.new`/`slide.duplicate` (whole-slide
   image, `slide_restore`), `shape.insert`/`insert.picture`/`insert.video`/`insert.audio`/
   `edit.delete` (whole-shape image, `shape_restore`/`shape_delete`) and `arrange.group`/
   `arrange.ungroup` (whole-subtree images on both sides, one `shape_restore{items, remove}`) are
   undoable. Merge operations (move, bounds, text set/insert/delete, fill/line/format, group
   appearance) are covered too. A slide image is capped at 192 KiB and a shape restore at 192 KiB /
   64 images, so an undo of an unusually large object is refused `invalid` instead of breaking the
   wire limit.
5. **Cabinet callback / host bridge.** The server side is wired: `POST /v1/sessions/{id}/commit`
   (service token) builds the `.pptx` and posts it to `COLLAB_CABINET_URL`
   (`{fileId, name?, bytes, sha256, version, expectedVersion?}` → `{version, etag}`); a cabinet
   `409` is the caller's `409`, the original version is kept, and the accepted `etag` is stored in
   `meta.json` so a restart still refuses an external change. What remains is outside this
   repository: the cabinet's endpoint (`INTEGRATION.md` §5.2), the host-bridge messages
   (`session`/`presence`/`commit`/`committed`) and a desktop "save to cabinet" command. Without
   `COLLAB_CABINET_URL` a deployment only builds verified bytes in the process.
6. **Browser transport.** `apps/deckcraft-web` has no `web-sys` WebSocket link yet; `?collab=1`
   shows the UI in a truthful offline state. A browser bridge must forward frames and media
   fetches, and the host page must send the session token.
7. **Optimistic preview.** Edits appear on `applied`, not on the keystroke/pointer release.
   Correct on a LAN, noticeable on a WAN. Preview needs a before-image and a rollback path; a
   rejected preview can be rolled back only if no other command has been applied since, otherwise
   the client must resync.
8. **Three-client soak.** The 120-slide/media performance gate and the LibreOffice commit gate now
   run in `tests/gates.rs` (see §8). What remains is a longer three-client soak and running the
   LibreOffice gate in CI, where LibreOffice is not installed (there it skips itself).
9. **Base transfer for an evicted journal tail.** `resync` sends the tail only; if the tail was
   compacted, a client that missed the base cannot rebuild. The store now survives restarts, so
   the next step is a chunked `snapshot` carrying the native base.
10. **Upstream PR head.** The PR is open from the fork; the local head is ahead of it and the push
    is manual. Nothing was pushed to `storytold/deckcraft`.

## How to run locally

```bash
# server
export COLLAB_SERVICE_TOKEN=dev-service
export COLLAB_JWT_SECRET=dev-secret-at-least-32-bytes
export COLLAB_STORE_DIR=/tmp/deckcraft-collab
cargo run -p deckcraft-collab-server

# create a room (bytes optional: a native/pptx base64)
curl -s -X POST localhost:8080/v1/sessions \
  -H 'Authorization: Bearer dev-service' \
  -H 'Content-Type: application/json' \
  -d '{"fileId":"demo","app":"deckcraft"}'

# desktop client
DECKCRAFT_COLLAB_URL=ws://127.0.0.1:8080/v1/sessions/demo/ws \
DECKCRAFT_COLLAB_TOKEN=<jwt> \
DECKCRAFT_COLLAB_SESSION=demo \
cargo run -p deckcraft

# the §8 gates (the LibreOffice half needs `soffice` and `pdfinfo` on the machine that runs it)
cargo test -p deckcraft-collab-server --test gates -- --nocapture --test-threads=1
```

The JWT must be HS256 with `aud: "office-sync"`, `sub`, `role`, `fileId`, `app: "deckcraft"`,
`sessionId` and `exp`. For a production cabinet use an ES256/RS256/Ed25519 PEM in
`COLLAB_JWT_PEM` and a short-lived token from the cabinet.

## Next session: start here

1. Run the verification table above; if the tree is clean, continue on `collab/pr10` (or branch
   `collab/pr11` from it).
2. **PR #1975 is open from the fork**; the local head is ahead of the PR head. Publishing the new
   head needs a manual push from a checkout that may push; do not touch CI and do not mention
   agents. Merging needs a human decision.
3. The cabinet must implement its side: `POST /api/v1/internal/office/sessions/{sessionId}/commit`
   (`INTEGRATION.md` §5.2, answers `{version, etag}`, keeps the previous version) and the
   host-bridge messages; the server already posts the bytes, refuses on `409` and stores the
   accepted version. Then add the web-sys link once the host contract is agreed; until then keep
   the browser UI honest, and add the media bridge for `wss://` pages.
4. Extend the command set in vertical slices with the same pattern: payload → normalize →
   adapter run → confirmed command → test on two clients. Next: table cell text, transitions,
   run-range formatting.
5. Undo is covered for move/bounds/text/fill/line/format, a group's fill/line, slide
   insert/delete/duplicate, shape insert/delete and group/ungroup (whole-subtree images on both
   sides). The remaining undo work is table/notes text and run-level formatting, which wait for
   their command slices.
6. Keep the §8 gates green (`cargo test -p deckcraft-collab-server --test gates -- --nocapture --test-threads=1`); the three-session gate covers conflict, viewer and reconnect, so the next gate is a longer three-client soak (more edits, a snapshot eviction and base transfer).
7. Update `production.md` and this file at the end of the session; post a bus status with the
   commit and test numbers.

## Session log

### 2026-11-09 — three-session gate: conflicting edit, viewer, reconnect

Added the third §8 gate to `apps/deckcraft-collab-server/tests/gates.rs`: two real editor sockets
and one viewer socket on a two-slide deck. Both editors move the same rectangle; the second
absolute value wins deterministically and the first editor's undo is refused `undo-conflict`
without touching the document. The editors also work on different slides, the viewer's op is
refused `forbidden`, and one editor drops and reconnects: the resync returns the journal tail
after the version his replica has, and he lands on version 4 and on an equal `Presentation`.
Three local replicas (opened from the same bytes) apply every confirmed command, so convergence
is compared on presentation state; a commit at version 4 reimports with the expected values.

The gate exposed a real reconnect bug in `ClientSession`: a `welcome` ahead of the replica was
adopted as the head before the resync, so the client requested the tail after the head. The
welcome no longer moves the version across a gap, and the new unit test
`a_reconnect_asks_for_the_tail_from_the_version_the_replica_has` pins the behavior; two older
client tests were corrected to the same semantics. On `agents`: `fmt --check` clean,
`collab` 124 + 8, `collab-proto` 20, `collab-server` 10 + 4 + 3 + 6, clippy `-D warnings` clean,
`xtask layers` 31 crates. Commit `556c00d9`, not pushed.

### 2026-11-09 — TASK.md §8 gates: 120 slides with media, LibreOffice on the commit

- `apps/deckcraft-collab-server/tests/gates.rs`: two environment gates. A 120-slide deck with
  120 pictures (4 media objects) is opened as a room; two editor sessions move a picture on every
  slide (0.1 ms p50, 0.1 ms p95, 0.2 ms max on the build host), both settle at version 120, the
  commit builds 154 KiB in 15 ms, and the bytes reimport with 120 slides and 120 pictures. The
  same gate takes the commit through `POST /v1/sessions/{id}/commit`, opens it in LibreOffice 26.2
  (121 pages via `pdfinfo`, 4.2 s) and round-trips it through LibreOffice back into
  `deckcraft_pptx::import` (5.3 s, 121 slides, 120 pictures). The build host has no LibreOffice,
  so there the test skips itself with a printed line; the gate ran with the remote-built binary on
  the workstation, where LibreOffice 26.2.6.3 is installed.
- The large-deck gate exposed a real bug: `shape.move`/`shape.setBounds`/`text.set` were applied
  against the replica's own selection, so an edit on a slide that replica was not looking at was
  refused with "no such shape". The adapter now selects the target shape and its slide; a focused
  regression test in `crates/collab/src/apply.rs` covers a move and a `text.set` on slide 1 while
  the other replica looks at slide 0.
- Verification on the build host: `cargo test -j 4 -p deckcraft-collab -p
  deckcraft-collab-server` (123 + 8, 10, 4, 6, 2 gates), `cargo clippy -j 4 -p deckcraft-collab -p
  deckcraft-collab-server --all-targets -- -D warnings` clean, `cargo fmt --all -- --check` clean.
### 2026-11-09 — align and distribute as one final-bounds step

- `arrange.align` and `arrange.distribute` now travel as **one** operation
  (`shapes_set_bounds`, object `items:[{shape, x, y, w, h}]`) instead of one per shape. The step
  pattern of the other slices: payload → normalize → adapter run → confirmed command → two-client
  test (RFC §3.2.1).
- Normalize computes the final boxes through new public engine helpers `align_plan`/
  `distribute_plan` — the same arithmetic the single-user commands use (`arrange.rs` now applies
  their result, so the two paths cannot drift). Shapes whose box does not change are left out, and
  a no-op commands send nothing. Locked shapes are left alone.
- The adapter runs `shape.setBoundsMany`: every key resolved and duplicates refused before the
  first write, values clamped, the whole selection moved in one `edit`. Undo records one geometry
  image per moved shape and restores the step in one `shapes_set_bounds`; `same_state` compares
  every box, so a foreign move of a member refuses the undo with `undo-conflict`. Server
  `capabilities.commands` lists the new engine command.
- Tests: two replicas converge on one align step with one history entry; one undo moves both
  objects back and redo re-applies them; a foreign move refuses the undo; the server path
  (`DocSession` + two `ClientSession`s) settles both clients after one version and one undo;
  normalize tests pin the final numbers (centre-align 210/250, distribute 0/840) and the no-op
  path; engine tests prove one undo step, an atomic refusal of a missing or repeated id, and a
  locked target left in place.
- Verified: `fmt` clean, `collab-proto` 20, `collab` 122 + 8, `collab-server` 10 + 6 + 4,
  `ui-egui collab::` 17, `deckcraft-engine` 569 + 1 ignored; clippy over the four collab crates and
  `ui-egui` with `-D warnings` clean; `xtask layers` 31 crates.
- Commit `31d353fe` (code) plus this docs commit on `collab/pr10`; RFC (§3.2.1, §6, §13.4),
  production and this file updated. Nothing pushed; CI untouched.

### 2026-11-09 — group/ungroup undo and fork/PR reconciliation

- Reconciled the fork with the local branch: `git fetch fork` leaves `fork/collab/pr10` at
  `63ec1ebe`, the PR head; local `collab/pr10` was ahead by `3ec81d70`, `ded9d1ee`, `31842050`,
  `f28ee048`. There is no fork commit missing locally. The three-way diff (fork head, local head,
  merge base) is the four local commits above; `git push --dry-run` is refused by this workspace's
  policy, so the PR head update stays manual. This file, `pr10.md` and `production.md` now say the
  PR is open from the fork instead of "nothing pushed".
- `arrange.group` and `arrange.ungroup` are undoable (RFC §6, §13 item 4). A step records a
  `GroupImage` on both sides: the group present (model JSON carrying its children, direct-child
  keys, z-order index and animations) or absent with a whole image of every former member at the
  top level. Undo and redo are one structural `shape_restore{items, remove}`: the engine removes
  the other side's shapes first without dropping animations, validates and deserialises every
  image before the first write, and inserts what the step recorded. `same_state` compares the
  present group or every absent member's content and owner, not positions.
- Tests: two-replica undo/redo of a group (members return in their order, then the group image
  comes back with its id and children); two-replica undo/redo of an ungroup the other way round;
  a foreign move of a member refuses the undo with `undo-conflict`; forged restores remain
  refused. Engine tests move group ↔ members in one `shape.restore` and prove a missing `remove`
  id refuses before any write. History and proto tests cover both payload directions.
- Verified: `fmt` clean, `collab-proto` 19, `collab` 118 + 7, `collab-server` 10 + 6 + 4,
  `ui-egui collab::` 17, `deckcraft-engine` 567 + 1 ignored; clippy over the four collab crates
  and `ui-egui` with `-D warnings` clean; `xtask layers` 31 crates.
- Commit `98d9e9ce` (code) plus this docs commit on `collab/pr10`; RFC (§6, §13.4, §14.1),
  production and this file updated. Nothing pushed; CI untouched.

### 2026-11-08 — a commit becomes a cabinet file version

- P0 from [`production.md`](production.md) closed on the server side. `POST /v1/sessions/{id}/commit`
  (service token, optional `{expectedVersion}`) builds the `.pptx` from the authoritative room and
  posts it to the cabinet (`COLLAB_CABINET_URL`, `COLLAB_CABINET_TOKEN`): body
  `{fileId, name?, bytes(base64), sha256, version, expectedVersion?}`, answer `{version, etag}`,
  parsed through a small HTTP/TLS client (`rustls`, system roots; `Connection: close`; chunked
  answers decoded). The file version is created by the cabinet, which keeps the previous one; the
  server never writes over the original.
- `409` is preserved end to end: a room whose `expectedVersion` does not match is refused before
  the callback; a cabinet `409` (file changed through WebDAV or another client) is the caller's
  `409` and no version is created. The accepted `etag` becomes the session's `expectedVersion` and
  is written to `meta.json`, so a restart still compares against the cabinet's copy.
- A client `commit` over WebSocket is answered only after the cabinet took the file; a viewer's
  `commit` is refused. The `initialName` a session is opened with is passed through to the
  callback. Without a callback the endpoint returns the base64 bytes (development mode, as
  `INTEGRATION.md` §4.1 describes).
- Counters: `cabinet_commits_total` (accepted publications), `cabinet_conflicts_total`,
  `cabinet_failures_total`; no file content, names or tokens are logged.
- Tests (`apps/deckcraft-collab-server/tests/commit.rs`): a fake cabinet keeps versions like the
  real one. Two rooms opened from two copies of the same `.pptx`; each commit reimports with its
  own edit, each original stays byte for byte, the name reaches the callback. An external change
  under a live room makes the server post and the cabinet answer `409`, with the conflict counter
  moving and nothing overwritten; a wrong `expectedVersion` is refused without a callback. The
  WebSocket path answers `commit-result` with the cabinet's `etag-1`, a stale second commit gets
  `conflict`, a viewer gets `forbidden`. A no-callback commit returns a `.pptx` inline and
  `fileVersion` stays the room's own.
- `collab` 113 + 7, `collab-proto` 19, `collab-server` 10 + 6 + 4, `ui-egui collab::` 17,
  `engine` 565 + 1 ignored; clippy `-D warnings` and `fmt` clean; `xtask layers` 31 crates.
- Commit `31842050` plus the docs commit on `collab/pr10` (RFC §8/§14.1, production, this file).
  Nothing pushed; CI untouched.

### 2026-11-08 — structural undo of shape insert and delete

- A new step image: `ShapeImage` (key, the group that held the shape, its position among that
  group's children, the `Shape` model as JSON or `None`, and the animations that named the shape
  or a descendant, including foreign effects it triggered). A step records only the **top-most**
  shapes it changed: a group's image carries its children, so undoing a group delete is one image.
- `shape.insert`, `insert.picture`, `insert.video`/`insert.audio` and `edit.delete` are undoable.
  Undo of a delete is the structural `shape_restore{items:[{shape, parent?, index, value,
  animations?}]}` and its redo is `shape_delete`; undo of an insert is `shape_delete` and its redo
  `shape_restore`.
- The engine command `shape.restore` bounds the images (64 items, 192 KiB together), deserialises
  the model before the first insert, refuses an id already on the slide, and inserts in rounds so
  a parent comes back before its children. `index` is clamped to the list that exists, so
  neighbours that came or went do not block the undo. The restored subtree's animations come back,
  and the triggers a delete cleared are re-attached to the same effects.
- `same_state` compares a shape image's content, owner and animations but not its position, so a
  foreign edit of a *neighbour* never turns an undo into `undo-conflict`; a foreign edit of the
  shape itself still does.
- The key map keeps both directions after a delete (the key is marked dead, not forgotten) and
  `revive_shape` clears the tombstone on a restore: the shape comes back under the key it had, so
  the step that restored it stays addressable and a replayed journal keeps the same keys.
- Tests: two-replica undo of an insert after a foreign neighbour edit; undo refused after a
  foreign edit of the inserted shape; two-replica undo of a delete with a moved neighbour (order,
  animation and the neighbour's position checked); undo of a group delete as one image; a forged
  `shape_restore` refused with `invalid`; a journal replay after a restart keeps the shape and its
  key; engine tests for z-order, animations, a group child, a duplicate id and a missing parent.
- Commit `3ec81d70` (code) plus the docs commit on `collab/pr10`; RFC (§3.2.1, §6, §11, §13.4,
  §14.1) and this file updated. Nothing pushed; CI untouched.

### 2026-11-08 — undo of a group's fill/line and structural slide undo

- A step now carries a list of images on both sides (`Change::{Shape, Slide}`), so one operation
  can touch several objects. `shape.fill`/`shape.line` on a group record one image per changed
  member and restore them in one `shape_fields` payload; the engine command `shape.setFields`
  validates and deserialises every value before the first write (4096 items, 192 KiB combined).
- `slide.delete`, `slide.new` and `slide.duplicate` record the whole slide: model JSON, index,
  section and custom-show membership. Undo restores it with the structural `slide.restore`
  command (`slide_restore` payload), redo deletes it with `slide_delete`; slide and shape ids are
  kept, the image is capped at 192 KiB, and a restored slide is removed from the tombstone set.
- `check_restore` now guards **every** history move (not only `shape_field`): a client cannot send
  an undo envelope that addresses another object, names another slide or carries another value.
- Tests: two-replica undo/redo of a group fill and of a slide delete/insert, a foreign member
  repaint refused with `undo-conflict`, a forged `slide_restore` refused with `invalid`; engine
  tests for `slide.restore` (index, section, duplicate id) and `shape.setFields` (atomic refusal).
- Journal replay of a history operation: `DocSession::replay` runs the stored confirmed command
  without the history check, so an exact `shape_field`, a group's `shape_fields` or a
  `slide_restore` survives a restart; the store uses it and a test deletes a slide, journals the
  undo and reloads the room.
- `collab` 106 + 7, `collab-proto` 19, `collab-server` 4 + 6, `ui-egui collab::` 17,
  `engine` 562 + 1 ignored; clippy `-D warnings`, `fmt` and `xtask layers` clean.
- Commits `4e666b95` (undo) and `f6281736` (replay) on `collab/pr10`; RFC (§3.2.1, §6, §11,
  §13, §14.1) and this file updated. Nothing pushed.

### 2026-11-08 — undo of fill, line and format steps

- `Snapshot` gained `Image::Fill`, `Image::Line` and `Image::Format(TextBody)`; `shape.fill`,
  `shape.line` and the 17 `format.*` commands now record a before/after image of the value they
  replaced.
- The user-facing commands cannot reproduce the model they replaced (gradient tile and scaling,
  custom dash, per-run formatting), so undo/redo carry the exact model value in a new
  `shape_field{field, value}` payload. The adapter accepts it only as an undo/redo of the actor's
  own latest step and only with the value that step recorded; a forged or different value is
  `invalid`, a foreign edit of the same object is `undo-conflict`.
- New engine command `shape.setField`: bounded JSON, deserialised into `Fill`/`Line`/`TextBody`
  before it touches a shape. A group's fill/outline step is not recorded (members would keep the
  new value).
- Tests: `collab` 99 + 7, `collab-proto` 18, `collab-server` 3 + 6, `ui-egui collab::` 17,
  `engine` 560 + 1 ignored; clippy `-D warnings`, `fmt` and `xtask layers` clean.
- Commit `41fe1ad9` on `collab/pr10`; RFC (§3.2.1, §6, §13.4) and this file updated. Nothing
  pushed.

### 2026-11-08 — media kinds, TLS and four command slices

- Added audio and video to the media channel: signature allowlists (MP4/MOV/WebM/AVI,
  WAV/FLAC/Ogg/MP3/M4A/AIFF/MIDI), 32 MiB per clip, separate 32 objects / 192 MiB budgets so a
  clip cannot evict pictures; `Payload::MediaInsert { kind, sha256, name, rect }`, adapter run
  through `insert.video`/`insert.audio`, `GET /media` content type from the signature.
- Added the TLS HTTP client for media: the desktop link derives `http`/`https` from
  `ws://`/`wss://`, `rustls` + `rustls-native-certs` (system roots, `SSL_CERT_FILE`), default
  ports, `Host` without the default port, IPv6 brackets.
- Added four command slices with the same pattern (payload → normalize → adapter → confirmed →
  two-replica test): `shape.fill` (solid/none/background), `shape.line` (final outline values),
  17 `format.*` commands over one explicit shape's text (`FormatCmd` allowlist, no address in
  `args`, explicit `on`), `slide.duplicate` addressed by slide key.
- Tests: `collab-proto` 18, `collab` 91 + 7, `collab-server` 3 + 6, `ui-egui collab::` 17;
  clippy `-D warnings` and `fmt` clean; `xtask layers` 31 crates. Engine not re-run.
- Commits `0a57b61e`…`69c2ff22` on `collab/pr10`; docs (rfc §3.2.1/§4.1/§13/§14.1, production,
  pr10) updated. Nothing pushed.

### 2026-11-08 — PR prep and formal RFC proposal

- Audited the chair's finding: the tree is clean; the change it saw uncommitted is `7fe75711`
  (media channel) plus `a4aac7c7`/`e7ad45cf` (docs), 1129 insertions in the slice commit.
- Re-ran fmt, tests of `collab-proto`, `collab`, `collab-server`, `ui-egui collab::` and clippy;
  results are in the verification table above.
- Added [`pr10.md`](pr10.md): title, description, review order, `PR-CHECKLIST.md` mapping and the
  limitations. No push.
- Posted a formal `proposal` with the RFC/PR/STATUS artifacts to `rfc/deckcraft`:
  `20261108T105421Z-deckcraft-47b7` (refs `…-7302`, `…-7c9b`, `…-7c4a`).
- Re-prioritised the gaps in [`production.md`](production.md) (P0 cabinet callback, P1 command
  surface/media/text, P2 transport/preview/undo/base-transfer/perf).
