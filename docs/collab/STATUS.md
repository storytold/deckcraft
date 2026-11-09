# Collaboration status and handoff

Updated: 2026-11-09. Branch: `collab/pr11`, **rebased onto `origin/main` at `0623b430`** (heads
`collab/pr10` = `7459d798` plus its rebase note, `collab/pr11` = `c895183a` plus this docs
commit; pre-rebase hashes are kept in the slice notes below and mapped in
[Rebase onto origin/main](#rebase-onto-originmain-2026-11-09)), from
`collab/pr10` at `922a4bf4` (media slice `7fe75711`, feature slices `0a57b61e`…`69c2ff22`, undo
of fill/line/format `41fe1ad9`, undo of a group's fill/line and structural slide undo `4e666b95`,
structural undo of shapes `3ec81d70`, cabinet commit publication `31842050`, undo of
group/ungroup `98d9e9ce`, align/distribute as final bounds `31d353fe`, the TASK.md §8 gates and
the non-active-slide fix `5b078c7d`, base transfer and the three-client soak `e007e965`, the
browser link `66d4658c`, the transition slice `927e773c`, docs commits on top), plus the
table-text slice `2a91d49e`, its docs commit, the cell-set rebase `b54eed3d`, the animation-step
slice `f562aea4` with its docs commit on top, the notes-text slice `b7025f9a` with its docs
commit on top, the table-cell format slice
`a48c12de` with its docs commit on top, the run-range format slice `b4f13167` with its docs
commit on top, the N-cell/caret format slice `6ddc0c4f` with its docs commit on top, the
whole-slide animation slice `b31bff47` with its docs commit on top, the
rest-of-the-animation-surface slice `e2a5ef41` with its docs commit on top, the
document-font slice `fd4e54e8` with its docs commit on top, and the rest-of-the-table-surface
slice `c895183a` with its docs commit on top.
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
| `collab/pr10` | `7fe75711` + `0a57b61e`…`69c2ff22`, `41fe1ad9`, `4e666b95`, `3ec81d70`, `31842050`, `98d9e9ce`, `f28ee048`, `e007e965`, `927e773c` | media channel (pictures by `sha256`, upload/fetch, persistence); audio/video, TLS media client, `shape.fill`/`shape.line`/`format.*`/`slide.duplicate`, `arrange.align`/`arrange.distribute` as final bounds, `transition.set` as a whole final value; undo of fill, line, `format.*`, a group's fill/line, slide insert/delete, shape insert/delete, group/ungroup, align/distribute and transition; commit published to the cabinet as a new file version (`409` on an external change); base transfer after compaction with the key map, and a three-client soak gate |
| `collab/pr11` | `2a91d49e`, `b54eed3d` (+ cell-set rebase), `f562aea4` (+ docs), `b7025f9a` (+ docs), `a48c12de` (+ docs), `b4f13167` (+ docs), `6ddc0c4f` (+ docs), `b31bff47` (+ docs), `e2a5ef41` (+ docs), `c895183a` (+ docs) | table cell text (`text.set`/`text.insert`/`text.delete` with a `(row, col)` cell address, rebased past row/column edits), `table.edit` for row/column insert/delete with the whole table as undo image; `animation.add`/`animation.remove` as one shape's whole step list (`slide.animation.set`) with the exact list as the undo image; slide notes text as an addressable body (`notes.set`/`notes.insert`/`notes.delete`, address `(slide, notes body)`, rebased, whole-body undo image, the receiver's selection kept); `format.*` on one table cell (`FormatSet { cell: [row, col] }`, applied to that cell's own body, refused before any write for a cell the table does not have) with the cell's whole body as the undo image (`cell_text`); `format.*` on a character range of a body (`FormatSet { range }`, whole-body offsets rebased like `text.insert`/`text.delete`, unaddressable range refused before any write, the whole body as the undo image); `format.*` over N table cells as **one** operation with one address per cell (`FormatSet { addresses }`, a neighbour never touched, empty/repeated/missing addresses and ranges past the table refused before any write with no version move, each addressed cell's whole body as the undo image in one `cells_text`, `undo-conflict` on a foreign edit of any addressed cell) and a caret in a shape's body as its own address (`caret: offset`, the single-user command decides what it touches — word for run commands, paragraph for paragraph commands — never pre-widened by the replica); `animation.set`/`animation.timing`/`animation.move` as one slide's whole ordered sequence (`slide.animations.set`) with the whole sequence as the undo image; `animation.options`/`animation.clear` and the Animation Painter as the same slide-wide sequence computed with the engine's `options_plan`/`painter_plan` (the painter's clipboard stays a session value, a no-op sends nothing, a one-shot pick is disarmed after sending), with the whole sequence as the undo image; and the document-font slice `fd4e54e8`: `font.create`/`font.set`/`font.remove` on the presentation's embedded-font registry (`FontCreate` names one face by digest, `FontSet` sets the descriptors, `FontRemove` drops the entry), a presentation-level merge step with no slide address, a font face on the media channel (`MediaType::Font` with an sfnt allowlist and its own budget) and the whole `EmbeddedFont` entry (`font_restore`) as the undo image; and the rest-of-the-table-surface slice `c895183a`: `table.style`, `table.cellFill` over an explicit cell list, `table.merge`/`table.split` and `table.columnWidth`/`table.rowHeight` as final-value commands addressed by the table, the whole table as the undo image (the `table.edit` image and `shape.setField{field: table}`, whose box now follows the table), a queued cell operation rebased past a row/column edit and refused `stale` when a merge covered its cell, and the merged rectangle recorded so a covered address is never written |

### Rebase onto origin/main (2026-11-09)

Both branches were rebased onto `origin/main` at `0623b430` (23 commits ahead of the old
merge-base `159ed07e`) so that [PR #1975](https://github.com/storytold/deckcraft/pull/1975) is no
longer `CONFLICTING`: `collab/pr10` (55 commits) → `7459d798`, then `collab/pr11` (13 commits,
`--onto`) → `da3199bf`. `git merge-tree --write-tree origin/main collab/pr10` and the same for
`collab/pr11` both exit 0 with an empty conflict list. The pre-rebase branches are kept locally as
`backup/pr10-pre-rebase` and `backup/pr11-pre-rebase`. The PR head is `collab/pr10`, so that
branch carries a short note of the rebase as well; this file on `collab/pr11` is the
authoritative one (it also has the later slices).

Every hash in this file that predates the rebase refers to the pre-rebase series; the named
slices map as follows:

| Pre-rebase | Rebased | Subject |
|---|---|---|
| `98d9e9ce` | `9d824224` | undo group and ungroup from whole-subtree images |
| `e007e965` | `a7da3d8f` | base transfer after compaction and a three-client soak gate |
| `927e773c` | `85babbaa` | `slide.transition.set` as a final-value command slice |
| `922a4bf4` | `7459d798` | docs: transition slice (old `collab/pr10` head) |
| `2a91d49e` | `f7ae5a94` | text in table cells and row/column edits |
| `b54eed3d` | `649f6e4a` | rebase a whole-cell `text.set` past a table edit |
| `f562aea4` | `e7e7b601` | shape animation steps as an exact-value command slice |
| `b7025f9a` | `5edd85b3` | slide notes as an addressable body slice |
| `a48c12de` | `0178cc6a` | format a table cell's own body, exact cell-body undo |
| `b4f13167` | `a90a3e31` | format a run range of a text body, exact-body undo |
| `d6ea4051` | `538649c9` | docs: run-range format slice |
| `6ddc0c4f` | `b2e976bf` | format several table cells and a caret in one step |
| `de498801` | `da3199bf` | docs: N-cell and caret format slice (new `collab/pr11` head) |

Main moved 29 files in those 23 commits (a `craft-web-api` page command API, 3-D body/group XML,
trendline fits, sentence-case and paragraph-alignment fixes, ribbon accessibility and the updater
UI, plus a craft-libs pin bump). Four files conflicted; each was resolved as a union, not a
choice:

- `crates/engine/src/cmd/shape.rs` — main's `opt_usize_param` (used by
  `cmd/shape/appearance.rs`) and the collab-side `current_shapes` both stay; rustfmt keeps one
  wrapped list.
- `apps/deckcraft-web/Cargo.toml` — main's `craft-web-api` at
  `storytold/craft-libs@fd5e2fe` and the collab `deckcraft-collab-web`/`craft-web-host` stay; the
  host crate remains pinned to `copoka/craft-libs@7666e2b` (the session messages are not in
  `storytold/craft-libs@fd5e2fe`, ADR-0016 §3), and `Cargo.lock` names that source.
- `apps/deckcraft-web/src/web.rs` — `crate::api::install(...)` is kept and the `WebShell`
  literal keeps the collaboration fields (`ctx`, `host_events`, `host_state`,
  `last_host_state`, `last_presence_ms`).
- `Cargo.lock` — the only remaining difference from main's own lock diff is the pinned host
  source; offsets differ because the collab crates add packages.

The code patch main brought in is `origin/main`'s own `159ed07e..origin/main` patch: compared with
`git diff backup/pr11-pre-rebase collab/pr11`, the only differences are `Cargo.lock`, the host pin
in `apps/deckcraft-web/Cargo.toml`, this file, and the one union import line in `cmd/shape.rs`
(`current_shapes` stays next to `opt_usize_param`). Hunk offsets differ because the collab crates
add lines above them. So the rebase added nothing to the collaboration slices and dropped nothing.

The Commit column names the slice commits; each branch also carries documentation commits. Heads
after the rebase: `collab/pr10` = `7459d798` (+ its note), `collab/pr11` = `da3199bf` (+ docs). In the pre-rebase series:
`collab/pr9` = `47ec08fe`, `collab/pr10` = `7fe75711`, `0a57b61e`, `58eee059`, `69c2ff22`,
`41fe1ad9`, `4e666b95`, `f6281736` (journal replay of history operations), `3ec81d70` (shape
insert/delete undo), `31842050` (cabinet commit publication), `f28ee048` (docs), `98d9e9ce`
(group/ungroup undo), `31d353fe` (align/distribute as one final-bounds step), `5b078c7d`
(the §8 gates and the non-active-slide fix), `556c00d9` (three-session gate and the reconnect
catch-up fix), `e007e965` (base transfer after compaction and the three-client soak), `927e773c`
(`slide.transition.set` as a final-value slice with its undo), plus the docs commits on top,
`collab/pr11` = `2a91d49e` (table cell text and row/column edits) plus its docs commit, the
cell-set rebase `b54eed3d`, the animation-step slice `f562aea4` with its docs commit on top, the
notes-text slice `b7025f9a` with its docs commit on top, the table-cell format slice
`a48c12de` with its docs commit on top, the run-range format slice `b4f13167` with its docs
commit on top, and the N-cell/caret format slice `6ddc0c4f` with its docs commit on top. The working tree is clean: the chair's
uncommitted finding of 2026-11-08 is `7fe75711`; the second slice added `0a57b61e` (media kinds,
TLS, command slices, tests, docs), `69c2ff22` hardened the format command, `41fe1ad9` added undo
of fill/line/format steps, `4e666b95` added undo of a group's fill/line and of slide
insert/delete, `3ec81d70` added undo of a shape insert and a shape delete (whole-shape image with
z-order, animations and triggers), `31842050` sends a commit to the cabinet, which creates the new
version and keeps the original, `98d9e9ce` makes `arrange.group`/`arrange.ungroup` undoable
with whole-subtree images on both sides, and `31d353fe` carries `arrange.align`/`arrange.distribute`
as one final-bounds operation with one undo step. `927e773c` makes `transition.set`
collaborative as one whole final value with an exact-value undo step, and `2a91d49e` (on
`collab/pr11`) carries a table cell address through text operations and adds `table.edit` for
row/column insert/delete.

The web build now has its side of the cabinet session (`66d4658c`, `crates/collab-web` +
`apps/deckcraft-web`): the `deckcraft:session` message opens the session, the page's `WebSocket`
carries the protocol, `session-state`/`presence` go back to the cabinet, Save asks for a commit
and `deckcraft:committed` records the stored version. Nothing there talks to the network without a
session message.

The fork branch `copoka/deckcraft:collab/pr10` is published at `922a4bf4`, the pre-rebase head,
and `collab/pr11` at `d6ea4051`; PR #1975 is open from the fork on `collab/pr10` (head
`922a4bf4`, `CONFLICTING` against `main`). The local branches were rebased onto `origin/main`
(`0623b430`) and now carry rewritten history (`collab/pr10` = `7459d798` plus its rebase note,
`collab/pr11` = `fd4e54e8` plus documentation commits, 75 commits ahead of main), so publishing
them is a **force-push** of
the two fork branches and not a fast-forward. A `git push --dry-run` from this workspace is
refused by the environment policy, so the publish is a manual step (the chair does it): `git push
--force-with-lease fork collab/pr10 collab/pr11`. Nothing was pushed to `storytold/deckcraft`;
the PR comes from the fork and a human decision still governs merging it. The PR body is prepared
in [`pr10.md`](pr10.md): title `feat(collab): realtime collaborative editing`, one description, a
review order over the slice stack and the `PR-CHECKLIST.md` mapping. Do not push it without the
human decision.

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
- Base transfer: when compaction removed the journal below a client's version, `resync`
  (`snapshot`) carries the native `.deckcraft` base, the session key map and the steps above the
  base. Keys are issued in walk order and cannot be re-derived after structural edits, so the map
  (`slides`, `shapes`, `dead`, `next`) travels with the base and the replica continues the
  server's numbering. Above `max_base_inline` (8 MiB, `COLLAB_MAX_BASE_INLINE`) the message names
  the base without bytes and the client reopens the file from the cabinet.
- Per-actor undo: server-side before/after images, `undo-conflict` when someone else changed the
  object, undo is an ordinary new operation. The images cover `xfrm`, text, fill, outline, a
  slide's whole transition, one shape's whole animation step list, a slide's whole ordered
  animation sequence, the whole `TextBody` of a
  `format.*` step (one body or each of N table cells) and a slide's whole notes body; these travel
  as an exact model value
  (`shape_field`/`shape_animations`/`slide_animations`/`notes_set`/`cells_text`) because the
  user-facing fill/line/format/animation/notes commands cannot express the whole model, and the
  adapter accepts that payload only as an undo/redo of the actor's own latest step.
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
- Notes text is its own address: `notes.set` carries a slide's whole notes body as the model
  `TextBody` (so runs, levels and bullets survive), `notes.insert`/`notes.delete` carry character
  offsets into it, and `target.slide` is the whole address — notes belong to a slide, not to a
  shape, and a shape key in the target is refused. The normaliser turns `slide.notes` into the
  complete body and typing in the notes pane (`text.insert`/`text.delete`/`edit.delete` with
  `notes: true`) into the offset operations. The journal rebase is keyed by the slide address as
  well, so another slide's notes never shift it; a whole-body replacement after the base sends a
  queued insert or delete to `stale` instead of writing in the wrong place. The adapter resolves
  the slide before the first write and restores the receiving user's selection, moving a caret
  that was in the same notes body by the characters inserted or removed before it.
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
- Browser link (`crates/collab-web`, `Transport` over the page's `WebSocket`): the same
  `CollabDoc`, dialled from `deckcraft:session` (`sessionId`, `wsUrl`, token, role). Reconnect
  backoff is the desktop link's; unconfirmed operations keep their ids and are resent after the
  next welcome. Media: the digests a frame names are `fetch`ed first and the frame waits for them,
  in order, for at most 10 s; uploads are `fetch`ed with the same token. The token never reaches
  a URL or a log (`HostSession` prints `<redacted>`), and the shell posts `session-state` /
  `presence` to the cabinet, asks for `commit` on Save in a live session, and records
  `committed`'s version and etag. `Transport` is no longer `Send` — a browser link holds JS
  closures and `Rc`, which never cross a thread on wasm32.
- `Session::set_collab_queue`: with a room attached, every mutating path (ribbon, keys, pointer,
  IME, control channel) queues the command instead of executing it; `CollabUi::pump` sends it.
  `apply_remote` still applies the server's command directly.
- Text: `text.insert`/`text.delete` carry character offsets; the server rebases them against the
  journal tail since `baseVersion` (inserts shift right, deletes pull left and clamp), so two
  people typing in one paragraph keep both texts. Confirmed commands carry the rebased offset.
  `text.set`, `text.insert` and `text.delete` also carry an optional `(row, col)` table cell: the
  offset is inside that cell's own body, the local queue reads it from the caret or the cell
  selection, and the adapter selects the cell before running the command. A `table.edit`
  (row/column insert/delete above/left of a cell) moves the queued address during rebase; a
  deletion of the cell's line refuses the queued operation. Two cells of one table never shift
  each other's offsets.
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
- `transition.set`: one slide's **whole** transition as a final value (`slide_transition`: effect,
  option, duration, advance timings, embedded sound, kept XML; `None` = no transition). The client
  computes it with the engine's `transition.set_plan`, the same function the single-user command
  runs, so the value keeps the advance timings and sound the user did not name; no replica merges
  its own state. The adapter runs `slide.transition.set`, which deserialises and validates the
  model value (an effect the model does not know is accepted only with its kept XML, the sound must
  be in the document) before the first write.
- `animation.add`/`animation.remove`: one shape's **whole** ordered step list as a final value
  (`shape_animations`: every step is the model `Animation` as JSON), addressed by slide key and
  shape key. The client builds a new step with the engine's `animation.add_plan` (the same `build`
  the single-user command runs), appends it to the steps the shape already has, and removes one by
  index or all of them for `animation.remove`; a multi-shape command is refused. The adapter
  resolves the address, forces the shape id on every value and runs `slide.animation.set`, which
  bounds the list (256 steps, 64 KiB), accepts only the model's own fields, requires a known
  effect, and replaces the shape's steps where the first one stood before the first write. Two
  steps in a row are two absolute values, so the later accepted list wins; a shape that cannot be
  addressed (unknown key, another slide, no shape) is refused before anything is written.
- `animation.set`/`animation.timing`/`animation.move`: one slide's **whole ordered animation
  sequence** as a final value (`slide_animations`: every step is the model `Animation` as JSON and
  carries its own shape id), addressed by slide key. The client computes the resulting sequence
  with the engine's own plans — `set_plan`, `timing_plan`, `move_plan`, the same functions the
  single-user commands run — so a style/`none` replacement keeps each shape's place, timing
  changes exactly the named steps, and a move may cross another shape's step. The adapter runs
  `slide.animations.set`, which bounds the list (4096 steps, 192 KiB), accepts only the model's
  own fields, requires a known effect and a shape still on that slide, and replaces the whole list
  before the first write; a list that changes nothing sends no operation. This is a per-slide
  value, so the whole-sequence payload, not the per-shape `shape_animations`, is what keeps
  interleaved steps exact.
- `animation.options`/`animation.clear`/`animation.painterApply`: the same slide-wide
  `slide_animations` value. `options` changes exactly the named steps' option codes (resolved in
  each effect's own terms by the engine's `options_plan`, so no replica guesses a direction),
  motion path and text build; `clear` sends an empty sequence for the slide it addresses (`index`
  is a slide, not a step); the painter's clipboard (`Session.anim_painter`) is a session value, so
  `painterApply` sends the resulting sequence computed with the engine's `painter_plan` — each
  target's own steps replaced by the picked ones, a paragraph the target does not have becoming
  the whole shape — and no replica needs the clipboard. A command that changes nothing sends no
  operation, a missing clipboard or an unaddressable target is refused before anything is sent,
  and the UI disarms a one-shot pick after it is sent, as the single-user command does.
- `font.create`/`font.set`/`font.remove`: the presentation's embedded font registry
  (`p:embeddedFontLst`). `font.create` embeds one face from a font file: the bytes go to the
  media channel and the operation carries their `sha256`, the typeface and the slot
  (`regular`/`bold`/`italic`/`boldItalic`), so an entry the document lacks is created and an
  existing one only gains or replaces that face. A font is presentation-level — it names no slide
  or shape, so every replica applies it whatever it is looking at — and it changes no address or
  key, so it is a merge-class operation. `font.set` sets the entry's descriptors (`panose`,
  `pitchFamily`, `charset`) as final values; `font.remove` drops the whole entry and leaves the
  face objects in the media list for an undo image. The engine validates the file (sfnt header and
  table directory, 16 MiB ceiling), stores it as `application/x-fontdata`, installs the face in
  the session's font database so the text draws immediately, treats a typeface name
  case-insensitively, and refuses an unknown entry or a non-font file before the first write.
- Undo of a font step: the whole entry before and after (the model `EmbeddedFont` as JSON — its
  descriptors and the media ids of its four face slots — or `null` when the document had none),
  restored with `font_restore` by the engine's `font.restore`. A later change to the same entry by
  anyone else is `undo-conflict`; a change to another typeface is not. The restore names no slide:
  `Restore.slide` became optional for this one presentation-level value.
- `format.*`: 17 allowlisted commands (`bold`, `italic`, `underline`, `strikethrough`,
  `superscript`, `subscript`, `font`, `size`, `color`, `highlight`, `alignLeft/Center/Right`,
  `justify`, `caps`, `spacing`, `clear`) over one explicit shape's whole text body, over the
  whole bodies of **N table cells of one table** addressed by
  `FormatSet { addresses: [{ cell: [row, col] }] }`, over a character range inside one of those
  bodies (`addresses: [{ cell?, range: {from, to} }]`) or at a caret (`addresses: [{ cell?, caret }]`).
  A selection of several cells is one operation with one address per cell, never one address
  widened to the table; a neighbouring cell that is not listed is never touched (a table without
  an address, an empty address list, a cell the table does not have, a cell addressed twice, a
  shape that is not a table, more than one body on a shape, an empty range and a range or caret
  past the end of the body are refused before any write). Toggles need an explicit `on`; `args`
  cannot carry `id`/`ids`/`index`/`cell`/`row`/`col`/`range`/`caret`. The range is a whole-body
  character range — paragraph breaks counted as one — the same coordinates
  `text.insert`/`text.delete` use, because run indices differ between replicas once someone edits
  the text. A caret in a shape's own body is its own address (`caret: offset`) rather than a
  refusal: the server places the same collapsed selection on the same authoritative body and runs
  the single-user command, so a run command widens it to the word it is in and a paragraph
  command takes the paragraph — the replica never widens it up front. The server rebases every
  address against the journal tail (a row/column edit moves each cell address); a range whose
  characters were removed is refused `invalid` before any write. The command runs on the engine
  selection placed on each addressed span, and the receiving user's selection is put back
  afterwards.
- `table.insertRowAbove`/`insertRowBelow`/`deleteRow` and the column variants: one structural
  `table.edit` payload (`axis`, `at`, `insert`) addressed by the table's shape key. The adapter
  runs the engine's own row/column command with the table id, so every replica makes the same
  change; the undo image is the whole table (`shape.setField` `field: table`), because a
  structural edit changes both the cell indices and their values.
- The rest of the table surface: `table.style` (a final style name), `table.cellFill` (one
  final fill — a solid colour, `none` or `inherit` — over an **explicit** cell list, which the
  normaliser expands from the user's selection and bounds at 4096 cells), `table.merge`/
  `table.split` (final corners or the merge anchor; cells keep their indices, so no address
  moves) and `table.columnWidth`/`table.rowHeight` (final numbers; `keepTotal` takes the
  difference from the next column, as dragging a border does). Each is one `Command` operation
  addressed by the table shape, so a replica's own selection decides nothing, and the adapter
  checks the table, every cell, column and row before the first write. A queued fill's cell
  addresses rebase past a row/column edit like a cell text offset; a merge that covered a queued
  operation's cell refuses it `stale` instead of writing where nobody can see it. The undo image
  is the whole table (`shape_field` `field: table`, the image a row/column edit records), and
  the engine's `shape.setField` now sets the shape's box to the restored table's size, so an
  undo of a width or height step moves the box back too.
- `slide.duplicate`: structural, addressed by the source slide key; the copy is inserted after it.
- `arrange.align`/`arrange.distribute`: one `shapes_set_bounds` operation with the final box of
  every moved shape. The client takes the values from the engine's own plan (`align_plan`,
  `distribute_plan`), so no replica recomputes the slide edge, the selection frame or the gaps;
  the adapter runs `shape.setBoundsMany`, which checks every id and duplicate before the first
  write and moves the whole selection in one `edit`. A shape whose box does not change is left out;
  an align that moves nothing sends nothing.
- Undo/redo: move, bounds, align/distribute, text (shape text and table cells), fill, outline,
  `format.*` (a shape's body, one cell's body, N cells of one table, or a character range/caret of
  a body), transition, animation (a shape's step list for add/remove, a slide's whole sequence
  for set/timing/move/options/clear/painter), a document font entry (create/set/remove) and the
  whole table for style/fill/merge/split/sizing. A restore
  carries the exact model value (`shape_field`) and runs through the `shape.setField` engine
  command, which bounds the JSON and deserialises it into the model type. A cell text step records
  the cell's whole body text with its address and restores it with `text.set{cell}`, so the
  shape's own (empty) body is not touched. A format step on a cell records that cell's whole body
  and restores it with `cell_text` / `shape.setField { field: cellText, cell }`, so a neighbouring
  cell is never written; a step over N cells records each cell's whole body and restores exactly
  them in one `cells_text` (`shape.setCellBodies`, one `shape.setField { field: cellText }` per
  cell, all validated before the first write), so a foreign edit of **any** addressed cell makes
  the undo `undo-conflict`. A `table.edit` step records the whole table (`shape_field` `field: table`) and
  restores rows, columns and cell text in one write. A group's fill/outline step records an image
  per changed member and restores all of them in one `shape_fields` operation (`shape.setFields`
  validates every value before the first write). `slide.delete`, `slide.new` and `slide.duplicate`
  record the whole slide and undo through `slide_restore` / `slide_delete`. `shape.insert`,
  `insert.picture`, `insert.video`/`insert.audio` and `edit.delete` record the whole shape and undo
  through `shape_restore` / `shape_delete` (z-order, animations and triggers included, group images
  carry their children). `arrange.group` and `arrange.ungroup` record a whole-subtree image on both
  sides (the group with its children, or every former member at the top level) and move between
  them in one `shape_restore{items, remove}`: the engine removes the other side's shapes first
  without dropping animations, then inserts the recorded images, all validated before the first
  write. A `transition.set` step records the slide's whole transition (effect, option, duration,
  advance timings, sound and kept XML) and restores it through the same `slide_transition` payload,
  applied by `slide.transition.set`. An `animation.add`/`animation.remove` step records the
  shape's whole step list before and after (model animations in order) and restores it through the
  same `shape_animations` payload, applied by `slide.animation.set`; a later list from another
  participant refuses the undo with `undo-conflict`, while a change to another shape's steps does
  not. An `animation.set`/`animation.timing`/`animation.move`/`animation.options`/
  `animation.clear`/`animation.painterApply` step records the slide's whole
  ordered sequence before and after and restores it through the same `slide_animations` payload,
  applied by `slide.animations.set`; any animation change on that slide after the step (including
  another shape's) refuses the undo with `undo-conflict`, because the whole order is the value.

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
  for pictures, font faces and audio/video, 8 MiB per picture, 16 MiB per font face, 32 MiB per
  clip), `GET /media/{sha256}` (session or service token). Content-addressed under
  `<store>/media/<sha256>`; 512 pictures / 64 MiB, 32 fonts / 64 MiB and 32 clips / 192 MiB per
  session, so a font cannot evict a picture; upload and byte counters in metrics.
- Persistence: native base + append-only journal, snapshot every N ops, replay on restart
  through the same `apply` path. Without `COLLAB_STORE_DIR` the server is memory-only.
- `COLLAB_SNAPSHOT_EVERY` now also drives a memory-only room (it used to reach the store only),
  so compaction and a base transfer can be forced in a test or a deployment. `resync` takes a
  snapshot on demand when the stored base does not cover the client; `base_transfers_total` and
  `base_bytes_total` count what went out.
- Logs/metrics contain ids, versions and counts; no slide content, text, media or tokens.

## 8. Gates (TASK.md §8)

All four gates are tests, not scripts: `apps/deckcraft-collab-server/tests/gates.rs`. Run them with

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
- **Three-client soak: compaction, base transfer and reconnect.** Two editors and one viewer on a
  two-slide deck (one rectangle per slide); the server is opened with `snapshot_every = 5`, so the
  journal is compacted during the run. The editors make 26 edits — six rounds of `shape.move`
  (Alice), `shape.setBounds` (Bob), `text.set` (Alice) and `shape.fill` (Bob), then a move to 400
  and a `text.set` — and both replicas follow every step. On the build host the 26 steps took
  0.67 s. At that point the base is the snapshot at version 25 and the tail is version 26 only.
  A viewer then joins at version 0: the server answers `resync` with the native base at 25
  (5 KiB inline), the session key map and the one-step tail; the viewer rebuilds its model, keys
  and version in 21.4 ms and reaches version 26, equal to both editors. `base_transfers_total`
  moved by one. Bob drops off, Alice moves the first rectangle to 420 and repaints it (versions
  27–28), and Bob reconnects with version 26: the answer is a tail of two steps, no base, and he
  lands on 28. All three replicas and the room settle on version 28 with an equal `Presentation`,
  and the commit at 28 reimports with the final boxes (420 / 350). The four gates together run in
  about 2.5 s with `--test-threads=1` on the build host (LibreOffice skipped there).

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
| `cargo test -p deckcraft-collab` | 166 unit + 8 integration |
| `cargo test -p deckcraft-collab-server --test gates -- --nocapture --test-threads=1` (on `agents`) | 4 passed in about 2.5 s: the 120-slide/media room and commit; three sessions (two editors on different slides and a viewer) across a conflicting edit, an undo-conflict, a forbidden viewer op and a reconnect with journal catch-up, all converging on version 4; the three-client soak (26 move/bounds/text/fill edits, compaction every 5 steps, a viewer rebuilt from a 5 KiB base at version 25 plus one tail step, a reconnect from a two-step tail, all three at version 28 and the commit reimporting); LibreOffice 26.2 renders 121 pages and round-trips the commit (run on the workstation, see below); on the build host the LibreOffice test skips itself (no `soffice`) |
| `cargo test -p deckcraft-collab --lib apply::` | 52 (a remote edit on a slide the replica is not looking at applies; two replicas converge on different table cells) |
| `cargo test -p deckcraft-collab-server` | 10 unit + 6 socket tests (two editors, viewer, health/metrics, invalid token, restart replay, media upload/fetch, picture insert, audio allowlist, shape-undo replay) + 4 commit tests (two copies with their originals kept, an external change is a `409`, a client commit answered after publication, the no-callback mode) + 4 gates |
| `cargo test -p deckcraft-collab-proto` | 26 |
| `cargo test -p deckcraft-ui-egui collab::` | 17 |
| `cargo test -p deckcraft-engine` | 575 passed, 1 ignored |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo fmt --all -- --check` | clean |
| `cargo xtask layers` | 32 crates (`collab-web` L6), no violations |
| `cargo test -p deckcraft-collab-web` | 15 passed |
| `cargo test -p deckcraft-web` | 3 passed (the headless page-host scenario) |
| `cargo check -p deckcraft-web --target wasm32-unknown-unknown` | ok |
| `trunk build` in `apps/deckcraft-web` | ok: `dist/web`, 49 MB `_bg.wasm` in the dev profile |
| after `c895183a`: `fmt --check` clean, clippy `--workspace --all-targets -D warnings` clean, `collab` 215 + 8, `collab-proto` 32, `collab-server` 10 unit + 4 commit + 4 gates (2.43 s, LibreOffice skipped on the build host) + 7 socket, `collab-web` 15, `deckcraft-web` 6, `ui-egui collab::` 20, `deckcraft-engine` 584 + 1 ignored, `xtask layers` 32 crates, wasm32 check of `deckcraft-web` ok; `cargo test --workspace --no-fail-fast` green except the two known host-without-`craft-fonts` failures (`ui-egui ime_tests::korean_composes_in_the_notes_pane`, `render tests::east_asian_vertical_text_stands_ideographs_upright`) |

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

Re-run on 2026-11-09 after the base transfer and the three-client soak (`e007e965`, on `agents`,
rust 1.99.1): `fmt --check` clean, `collab-proto` 22, `collab` 129 + 8, `collab-server` 10 unit
+ 4 commit + 4 gates + 6 socket, `ui-egui collab::` 17, `deckcraft-engine` 570 + 1 ignored;
clippy over the four collab crates, `engine` and `ui-egui` with `-D warnings` clean; `xtask
layers` 31 crates. The four gates run in about 2.5 s with `--test-threads=1`; the soak gate itself
prints 26 edits in 0.67 s, a 5 KiB base at version 25 rebuilt in 21.4 ms and a two-step tail on
the reconnect. `git status` is clean after the docs commit; nothing was pushed and CI was not
touched.

Re-run on 2026-11-09 after the transition slice (`927e773c`, on `agents`, rust 1.99.1): `fmt
--check` clean, `collab-proto` 23, `collab` 138 + 8, `collab-server` 10 unit + 4 commit + 4 gates
+ 6 socket, `deckcraft-engine` 572 + 1 ignored; clippy over `collab-proto`, `collab`,
`collab-server` and `engine` with `-D warnings` clean; `xtask layers` 32 crates. The gates were
run without `soffice` on the build host. `git status` is clean after the docs commit; nothing was
pushed and CI was not touched.

Re-run on 2026-11-09 after the table-text slice (`2a91d49e`, on `agents`, rust 1.99.1): `fmt
--check` clean, `collab-proto` 24, `collab` 147 + 8, `collab-server` 10 unit + 4 commit + 4 gates
+ 6 socket, `deckcraft-engine` 573 + 1 ignored; clippy over `collab-proto`, `collab`,
`collab-server` and `engine` with `-D warnings` clean; `xtask layers` 32 crates. The gates were
run without `soffice`. The new tests cover two replicas editing different cells of one table, a
cell operation resent after a reconnect (one version bump, one insertion), a queued cell insert
rebased past an inserted row, a queued whole-cell `text.set` following its cell after an inserted
row, a queued insert refused after its row is deleted, two people in one
cell keeping both insertions without moving another cell, and undo/redo of cell text and of a
whole-table row edit. `git status` is clean after the docs commit; nothing was pushed and CI was
not touched.

Re-run on 2026-11-09 after the animation-step slice (`f562aea4`, on `agents`, rust 1.99.1): `fmt
--check` clean, `collab-proto` 25, `collab` 156 + 8, `collab-server` 10 unit + 4 commit + 4 gates
+ 6 socket, `deckcraft-engine` 575 + 1 ignored; clippy over `collab-proto`, `collab`,
`collab-server` and `engine` with `-D warnings` clean; `xtask layers` 32 crates. The gates were
run without `soffice` (4 passed in 2.44 s). The new tests cover a step on two replicas, two steps
in a row keeping their order, an undo that keeps the shape's other step on both replicas, a later
list from another participant refusing the undo with `undo-conflict`, an unaddressable shape
(unknown key, another slide, no shape) refused before any write, a forged restore, the
normaliser's whole-list and index-addressed removal, the wire round trip, the engine setter, and
the whole template through the in-memory hub (two `animation.add` submissions and an undo on two
replicas). The engine soak no longer picks the two exact-value setters: its generator cannot build
their whole-value parameters, so they only bounced off their validation and skewed the edit-rate
assertion. `git status` is clean after the docs commit; nothing was pushed and CI was not touched.

Re-run on 2026-11-09 after the notes-text slice (`b7025f9a`, on `agents`, rust 1.99.1): `cargo fmt
--all -- --check` clean, `cargo clippy --workspace --all-targets -- -D warnings` clean,
`collab-proto` 26, `collab` 166 + 8, `collab-server` 10 unit + 4 commit + 4 gates + 6 socket;
`xtask layers` 32 crates. The new tests cover two replicas converging on a notes insert after a
slide was inserted (the structural step does not move the address), an undo and redo that restore
the previous body and its bold run, a foreign notes edit refusing the undo with `undo-conflict`,
an unknown, deleted, unaddressed or shape-addressed slide refused before any write together with
an unreadable body, the receiving caret moving with the inserted and deleted prefix while a
shape's text selection on the slide stays put, a whole-body replacement sending a queued insert to
`stale`, the journal rebase keeping two typists' text in one slide's notes and never shifting
another slide's notes, and the normaliser plus the wire form.

Re-run on 2026-11-09 after the table-cell format slice (`a48c12de`, on `agents`, rust 1.99.1):
`cargo fmt --all -- --check` clean, `cargo clippy --workspace --all-targets -- -D warnings` clean,
`collab-proto` 27, `collab` 175 + 8, `collab-server` 10 unit + 4 commit + 4 gates + 6 socket;
`deckcraft-engine` 575 + 1 ignored; `ui-egui` full suite 509 + 1 known environment failure
(`ime_tests::korean_composes_in_the_notes_pane`, no `craft-fonts`/Korean font on the build host);
`xtask layers` 32 crates. The new tests cover two replicas converging on one cell's format with a
neighbour untouched, an undo and redo that put back and reapply the exact cell body (the old
formatting included) without touching a neighbouring cell's step, an undo refused with
`undo-conflict` after another participant typed in the addressed cell, a queued cell format
landing on the cell as it is after someone typed there, the same format following an inserted row
and refused `stale` when its row is deleted, and an unaddressable cell (no such row/column, a
table without an address, a non-table shape, a deleted slide) refused before any write with no
undo step recorded; the normaliser and both wire forms are pinned. `git status` is clean after the
docs commit; nothing was pushed and CI was not touched.

Re-run on 2026-11-09 after the run-range format slice (`b4f13167`, on `agents`, rust 1.99.1):
`cargo fmt --all -- --check` clean, `cargo clippy --workspace --all-targets -- -D warnings` clean,
`collab-proto` 28, `collab` 183 + 8, `collab-server` 10 unit + 4 commit + 4 gates + 6 socket
(4 gates in 1.5–2.5 s, LibreOffice skipped on the build host), `ui-egui collab::` 18. The new tests
cover two replicas converging on a bold range of a shape's body (text kept, the run boundary at
the range's end, the rest unformatted), the same for a range inside one table cell with a
neighbour untouched, an undo and redo that put back and reapply the previous runs while the text
after the range is untouched, an undo refused with `undo-conflict` after another participant typed
in the same body, an empty, reversed, past-the-end or missing-cell range refused `invalid` before
any write with no undo step, the receiving user's selection kept across a remote range format, a
queued range following text typed before it (offsets shifted, the typed characters not bold) and
refused `invalid` when its characters were deleted, the normaliser's offsets for a range in a
shape, across a paragraph break and inside a cell (and a caret or a conflicting explicit cell
refused), and both wire forms (with and without `cell`/`range`). `git status` is clean after the
docs commit; nothing was pushed and CI was not touched.

Re-run on 2026-11-09 after the N-cell/caret format slice (`6ddc0c4f`, on `agents`, rust 1.99.1):
`cargo fmt --all -- --check` clean, `cargo clippy --workspace --all-targets -- -D warnings` clean,
`collab-proto` 29, `collab` 192 + 8, `collab-server` 10 unit + 4 commit + 4 gates + 6 socket
(LibreOffice skipped on the build host), `ui-egui collab::` 19, `ui-egui` full 511 + the known
`ime_tests::korean_composes_in_the_notes_pane` environment failure (no Korean font on the build
host), `xtask layers` 32 crates, no layering violations. The new tests cover two replicas
converging on one operation over two cells of a 2×2 table (the first whole, the second over two
characters; a neighbouring cell keeps its text and formatting; both deltas equal), an undo that
restores exactly the two addressed cells' whole bodies with one `cells_text` and a redo that
reapplies them while the neighbour never moves, an undo refused `undo-conflict` after someone
typed in one of the addressed cells (history not moved, nothing written), an empty address list, a
missing cell, an out-of-body range and a repeated cell refused before any write with the version
unchanged (and a table addressed without a cell refused `unsupported`), a remote N-cell format
that leaves the receiving user's selection alone, a queued N-cell format whose **every** address
follows an inserted row, a caret inside a word of a shape body converging on both replicas to the
same word-wide bold (undo restores the whole body, redo reapplies), a caret at a word boundary
that does not bold any existing character (no pre-widening) and a paragraph command at a caret that
takes only its paragraph, a caret past the body refused `invalid` before any write with no step,
the normaliser emitting one address per selected cell and a `caret` for a caret in a shape's body,
and both wire forms (`addresses` with `cell`/`range`/`caret`).

Re-run on 2026-11-09 after the **rebase onto `origin/main` (`0623b430`)** (on `agents`, rust
1.99.1; `collab/pr10` = `7459d798`, `collab/pr11` = `da3199bf`): `cargo fmt --all -- --check`
clean, `cargo clippy --workspace --all-targets -- -D warnings` clean (22 s warm), `collab-proto`
29, `collab` 192 + 8, `collab-web` 15, `collab-server` 10 unit + 4 commit + 4 gates + 6 socket
(the four gates in 2.52 s with `--test-threads=1`, LibreOffice skipped on the build host),
`deckcraft-web` 6 (main's page-API tests included), `ui-egui collab::` 19, `ui-egui` full 515
passed + 4 ignored + the known `ime_tests::korean_composes_in_the_notes_pane` environment failure
(no `craft-fonts`/Korean font on the build host), `deckcraft-engine` 577 + 1 ignored, `xtask
layers` 32 crates with no violations, `cargo check -p deckcraft-web --target
wasm32-unknown-unknown` ok. `git status` is clean after the docs commit; nothing was pushed, CI
was not touched, and no collab slice changed (the code patch is main's own, see the rebase
section).

Re-run on 2026-11-09 after the whole-sequence animation slice (on `agents`, rust 1.99.1):
`cargo fmt --all -- --check` clean, `cargo clippy --workspace --all-targets -- -D warnings`
clean, `collab-proto` 30, `collab` 195 + 8, `collab-web` 15, `collab-server` 10 unit + 4 commit
+ 4 gates + 6 socket (LibreOffice skipped on the build host), `deckcraft-web` 6, `ui-egui
collab::` 19, `deckcraft-engine` 579 + 1 ignored, `xtask layers` 32 crates with no violations,
`cargo check -p deckcraft-web --target wasm32-unknown-unknown` ok. The new tests cover two
replicas converging on `animation.set` and on the undo of the second shape's set, `animation.timing`
changing one step with an undo that restores the exact duration, `animation.move` crossing another
shape's step on both replicas with an undo that restores the whole order, a foreign animation edit
on the same slide making the undo `undo-conflict` without a version move, the engine keeping
interleaved steps through an exact list and refusing a missing shape, an unknown effect/field and
too many steps before any write, the plans matching the single-user commands, the normaliser
emitting the exact sequence and nothing for a no-op, and both wire forms. `git status` is clean
after the docs commit; nothing was pushed and CI was not touched.

Re-run on 2026-11-09 after the rest-of-the-animation-surface slice (`e2a5ef41`, on `agents`, rust
1.99.1): `cargo fmt --all -- --check` clean, `cargo clippy --workspace --all-targets -- -D
warnings` clean, `collab-proto` 30, `collab` 197 + 8, `collab-web` 15, `collab-server` 10 unit +
4 commit + 4 gates + 6 socket (the four gates in 2.45 s with `--test-threads=1`, LibreOffice
skipped on the build host), `deckcraft-web` 6, `ui-egui collab::` 20, `ui-egui` full 516 passed +
4 ignored + the known `ime_tests::korean_composes_in_the_notes_pane` environment failure (no
`craft-fonts`/Korean font on the build host), `deckcraft-engine` 580 + 1 ignored, `xtask layers`
32 crates with no violations, `cargo check -p deckcraft-web --target wasm32-unknown-unknown` ok
(two pre-existing dead-code warnings on wasm32, in untouched fields of `collab-web`/`ui-egui`).
The new tests cover `animation.options` and `animation.painterApply` through the engine plans
(options by index leave exactly the planned codes and text build; the painter copies a
paragraph-level step onto a shape without that paragraph and it becomes a whole-shape step); the
normaliser turning options, clear and the painter into the slide's whole sequence, sending
nothing for a no-op and refusing a missing clipboard, an empty target, an unknown target and an
out-of-range slide; the hub running options, the painter (clipboard in the session) and clear on
two replicas with an undo of each; and the UI sending the painted sequence over the link and
disarming a one-shot pick. `git status` is clean after the docs commit; nothing was pushed, CI
was not touched, and PR #1975 is unchanged.

Re-run on 2026-11-09 after the document-font slice (`fd4e54e8`, on `agents`, rust 1.99.1):
`cargo fmt --all -- --check` clean, `cargo clippy --workspace --all-targets -- -D warnings`
clean, `collab-proto` 31, `collab` 205 + 8, `collab-web` 15, `collab-server` 10 unit + 4 commit
+ 4 gates + 7 socket, `deckcraft-web` 6, `ui-egui collab::` 20, `deckcraft-engine` 584 + 1
ignored, `xtask layers` 32 crates with no violations, `cargo check -p deckcraft-web --target
wasm32-unknown-unknown` ok, `cargo check --workspace --all-targets` clean (the E0502 reported on
`57da8f76` in `crates/collab/src/local.rs` does not reproduce at `07b6f1c8`, the base of this
slice, nor after it). `cargo test --workspace --no-fail-fast` is green except the known environment
failures of a host without `craft-fonts`: `ui-egui ime_tests::korean_composes_in_the_notes_pane`
and `render tests::east_asian_vertical_text_stands_ideographs_upright` (no CJK face on the
host; both draw a glyph the machine does not have). The four §8 gates run 4/4 in 2.44 s. The new
tests cover two replicas converging on `font.create` (the bytes by digest on the media channel,
the same media id on both, the face installed), `font.set` and `font.remove`; an undo of a
descriptor step and of the create itself broadcast through the hub to both replicas, with the
face object staying for the redo; an undo refused `undo-conflict` after another participant
changed the entry; a step addressed by a slide, a digest that is not a font, a missing upload
(retryable), an unknown entry and a descriptor set with nothing to set all refused before any
write with no version move; the normaliser's digest-and-face form, its upload (`take_uploads`),
and its refusals; the media channel's sfnt allowlist and its own budget; and a real socket:
upload/fetch of a face, a viewer refused, a `font_create` applied to the room and a commit that
reimports with the entry. `git status` is clean after the docs commit; nothing was pushed, CI was
not touched, and PR #1975 is unchanged.

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
| Two or more clients edit one presentation | ✅ for the v1 command set; a three-client soak (two editors and a viewer, 26 edits) converges across compaction | hub, socket test, UI pipe test, `tests/gates.rs` |
| A 100+ slide deck with media stays responsive and its commit opens in LibreOffice | ✅ 120 slides/120 pictures, 4 media objects; commit renders 121 pages and survives an LO round trip | `tests/gates.rs`; adapter fix for edits on a non-active slide |
| Insert a picture in a shared session | ✅ pictures, audio and video over `ws://` and `wss://` | media channel, adapter and socket tests |
| Cursors/selections/participants visible | ✅ | `CollabUi::marks`, participants pane |
| Slides, shapes and order converge | ✅ for move/bounds/align/distribute/insert/delete/slide/group/order/fill/line/format (a shape's body, one table cell, N cells of one table, a run range or a caret)/duplicate/transition/animation (a shape's step list, a slide's whole sequence from set/timing/move/options/clear and the painter), table cell text, table row/column edits, the rest of the table surface (style, cell fill, merge/split, column/row sizing), slide notes text and document fonts (the embedded-font registry: one face by digest, descriptors, removal) | collab tests, socket test, commit reimport |
| Reconnect works | ✅ within the retained journal tail; after compaction the client is rebuilt from the native base and the key map (inline up to 8 MiB) | client queue + resync; three-session gate; three-client soak; `DocSession::resync` |
| Per-user undo/redo | ✅ text (shape, table cell and slide notes), shape values, align/distribute, fill, outline, `format.*` (a shape's body, one cell's body, N cells of one table, a run range or a caret), group fill/line, slide insert/delete, shape insert/delete, group/ungroup, transition, animation (a shape's step list and a slide's whole sequence for set/timing/move/options/clear/painter), table row/column edits, the whole table for style/fill/merge/split/sizing, document fonts (the whole entry) | server history, `undo-conflict`, `shape_field`/`cell_text`/`cells_text`/`shape_fields`/`shapes_set_bounds`/`shape_restore`/`slide_restore` (the last with `remove`)/`slide_transition`/`shape_animations`/`slide_animations`/`notes_set`/`font_restore`/whole-table `shape_field` |
| Viewer cannot edit | ✅ server + engine gate, also inside the three-session gate | socket test, UI test |
| Commit creates a new version, original kept | ✅ server side: `POST /v1/sessions/{id}/commit` → cabinet callback with the bytes, `409` on an external change, the accepted version stored in `meta.json` | `apps/deckcraft-collab-server` (`cabinet.rs`, `lib.rs`), `tests/commit.rs`; the cabinet endpoint and the desktop save command remain external |
| Single-user mode unchanged; feature behind a flag | ✅ | flag-off tests; a page without `deckcraft:session` makes no network call (`deckcraft-web`) |
| Browser: session, presence, commit through the host bridge | ✅ code + headless gate; the cabinet endpoint and CORS remain external | `crates/collab-web`, `apps/deckcraft-web/src/host_session.rs`, `cargo test -p deckcraft-web` |
| RFC, protocol, tests, docs | ✅ RFC v1 accepted 4/5 (ADR-0015, 2026-11-08); formal `proposal` `…-47b7` | this tree, bus decision `20261108T143417Z-craft-8036` |
| PR in `storytold/deckcraft` | ✅ open as [#1975](https://github.com/storytold/deckcraft/pull/1975) from the fork; locally rebased onto `origin/main` (`0623b430`) with a clean `merge-tree` (the fork needs a force-push, the chair's manual step) | [`pr10.md`](pr10.md); merge needs a human decision |

Re-run on 2026-11-09 after the browser link (`66d4658c`, on `agents`): `fmt --check` clean,
`collab` 129 + 8, `collab-proto` 22, `collab-server` 10 unit + 4 commit + 4 gates + 6 socket,
`collab-web` 15, `deckcraft-web` 3 (the headless page-host scenario), `ui-egui collab::` 17,
`ui-egui` full suite 509 + 1 failed (`ime_tests::korean_composes_in_the_notes_pane`: the build
host has no `craft-fonts` and no Korean-capable font, so egui panics on a missing glyph — an
environment gap, not this change), `deckcraft-engine` not re-run (untouched); clippy over
`collab`, `collab-proto` (unchanged), `collab-web`, `ui-egui` and `deckcraft-web` with
`-D warnings` clean; `xtask layers` 32 crates; `xtask wasm` 26 crates;
`cargo check -p deckcraft-web --target wasm32-unknown-unknown` ok; `trunk build` (0.22.1) ok in
4m7s cold (3m44s of it rustc), `dist/web` with a 49 MB `_bg.wasm` and the JS glue.

## What is NOT done (ordered by production impact)

1. **Command surface beyond the new slices.** Fill, line, 17 `format.*` commands over whole shape
   text, over N table cells of one table (`FormatSet { addresses: [{ cell: [row, col] }] }`,
   applied to each cell's own body), over a character range of a shape or cell body and at a caret
   (`addresses: [{ cell?, range | caret }]`, rebased, exact-body undo), `slide.duplicate`,
   `transition.set`, table cell text, table
   row/column edits, the rest of the table surface (`table.style`, `table.cellFill` over an
   explicit cell list, `table.merge`/`table.split` and `table.columnWidth`/`table.rowHeight`, each
   with the whole table as the undo image), `animation.add`/`animation.remove` (one shape's whole
   step list),
   `animation.set`/`animation.timing`/`animation.move`/`animation.options`/`animation.clear` and
   the Animation Painter (one slide's whole ordered sequence) and
   slide notes text (`notes.set`/`notes.insert`/`notes.delete`, address `(slide, notes body)`) and
   document fonts (`font.create`/`font.set`/`font.remove`, the whole entry as the undo image) are
   collaborative now. Still refused: the table style toggles (`table.options`), SmartArt/charts,
   3-D models and relative effects. The design rule stands: only
   commands whose result is a deterministic function of the authoritative state, expressed as
   final values, go on the wire; a raw `execute` is not safe (toggles and relative edits
   diverge). Next: SmartArt/charts. Column and row sizing has no ribbon gesture yet, so it is
   reachable through the control channel and MCP.
2. **Media refinement.** Font faces are in the media allowlist now; 3-D models are not. The
   browser link still needs the same HTTP bridge. The desktop `wss://` path has a TLS client now (system roots;
   a private CA must be in the store or `SSL_CERT_FILE`).
3. **Rich runs are in; cell formatting, N-cell ranges and carets are done.** Shape text,
   table-cell text and slide notes are addressed and rebased. A notes edit addresses
   `(slide key, notes body)`, is refused for an unaddressable slide before any write, keeps the
   receiving user's selection, and its undo image is the slide's whole text body. A `format.*`
   command addresses a list of bodies (`FormatSet { addresses }`): a table cell is one address,
   runs against that cell's own body rather than the table's empty shape text, is refused before
   any write for a cell the table does not have (and for a table with no address), and its undo
   is the whole body of exactly that cell (`cell_text`, `shape.setField { field: cellText }`) with
   an `undo-conflict` when someone else changed that cell after the step — a neighbouring cell is
   never touched. A selection of several cells is one operation with N addresses, one per cell:
   the same checks apply to every address, an empty list, a repeated cell and a range past the
   table are refused `invalid` before any write with no version move, the undo restores exactly
   those cells' bodies in one `cells_text`, and a foreign edit of any addressed cell is
   `undo-conflict`. A range (`addresses: [{ range: {from, to} }]`) addresses whole-body character
   offsets, rebased against `text.insert`/`text.delete`, and is refused `invalid` before any write
   for an empty range or one past the body; a caret (`addresses: [{ caret }]`) in a shape's body is
   its own address and the single-user command decides what it touches (a run command widens it to
   its word, a paragraph command takes the paragraph), so the replica never widens it. Undo of
   either restores the whole body it changed (the format splits runs at the range's ends), so a
   later edit of that body by someone else is `undo-conflict`. The other table commands (style,
   cell fill, merge/split, sizing) are in now as final values with the whole table as the undo
   image (item 1); the style toggles (`table.options`) remain, because they are a set of
   independent booleans rather than one value.
4. **Structural undo is covered; size limits remain.** A group's fill/outline (one image per
   changed member, one `shape_fields`), `slide.delete`/`slide.new`/`slide.duplicate` (whole-slide
   image, `slide_restore`), `shape.insert`/`insert.picture`/`insert.video`/`insert.audio`/
   `edit.delete` (whole-shape image, `shape_restore`/`shape_delete`), `arrange.group`/
   `arrange.ungroup` (whole-subtree images on both sides, one `shape_restore{items, remove}`) and
   `transition.set` (the whole transition, `slide_transition`) are undoable. Merge operations
   (move, bounds, text set/insert/delete, fill/line/format, group appearance) are covered too. A slide image is capped at 192 KiB and a shape restore at 192 KiB /
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
6. **Browser transport is in (`collab-web`); deployment is not.** `apps/deckcraft-web` opens a
   session from `deckcraft:session`, dials it with the page's `WebSocket`, posts
   `session-state`/`presence`, asks for a commit on Save and records `committed`. What remains is
   outside the code: the cabinet's commit endpoint (`INTEGRATION.md` §5.2), a page that can reach
   the sync server (same origin, or CORS headers the server does not send), and a token-renewal
   message (a fresh token today means a new `session` message).
7. **Optimistic preview.** Edits appear on `applied`, not on the keystroke/pointer release.
   Correct on a LAN, noticeable on a WAN. Preview needs a before-image and a rollback path; a
   rejected preview can be rolled back only if no other command has been applied since, otherwise
   the client must resync.
8. **Three-client soak, longer runs.** The three-client soak now runs in `tests/gates.rs` (see
   §8): 26 move/bounds/text/fill edits across compaction, a viewer rebuilt from the base, a
   reconnect from the journal, and a commit at the final version. What remains is a longer soak
   (hundreds of steps, more participants, media in the loop) and running the LibreOffice gate in
   CI, where LibreOffice is not installed (there it skips itself).
9. **Base transfer above the inline limit.** A client below a compacted journal is rebuilt from
   the native base and key map carried by `snapshot`; above `max_base_inline` (8 MiB,
   `COLLAB_MAX_BASE_INLINE`) the message names the base without bytes and the client must reopen
   the file from the cabinet. A chunked baseline or an HTTP-served base (the media endpoint
   pattern) is the next step for 100-slide decks with media.
10. **Upstream PR head.** The PR is open from the fork. Both branches were rebased onto `main`
    (§ Rebase onto origin/main), so the fork branches no longer fast-forward and the publish is a
    `--force-with-lease`. The push is manual (the chair does it). Nothing was pushed to
    `storytold/deckcraft`.

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

# the browser build (trunk 0.22.1, wasm32-unknown-unknown)
cd apps/deckcraft-web && trunk build           # dist/web
# the page-host scenario, headless (no socket: an in-process Hub is the server)
cargo test -p deckcraft-web
```

The JWT must be HS256 with `aud: "office-sync"`, `sub`, `role`, `fileId`, `app: "deckcraft"`,
`sessionId` and `exp`. For a production cabinet use an ES256/RS256/Ed25519 PEM in
`COLLAB_JWT_PEM` and a short-lived token from the cabinet.

## Next session: start here

1. Run the verification table above; if the tree is clean, continue on `collab/pr11` (or branch
   `collab/pr12` from it).
2. **PR #1975 is open from the fork.** Both branches are rebased onto `origin/main` (`0623b430`)
   and merge cleanly; publishing them is a `git push --force-with-lease fork collab/pr10
   collab/pr11` from a checkout that may push (history was rewritten), and remains the chair's
   manual step. Do not touch CI and do not mention agents. Merging needs a human decision.
3. The browser side is in (`66d4658c`): `deckcraft:session` → the page's `WebSocket`,
   `session-state`/`presence` back, Save → `deckcraft:commit`, `committed` recorded, media over
   `fetch` with the session token. The cabinet must still implement its side:
   `POST /api/v1/internal/office/sessions/{sessionId}/commit` (`INTEGRATION.md` §5.2, answers
   `{version, etag}`, keeps the previous version); the server already posts the bytes, refuses on
   `409` and stores the accepted version. For a page on another origin the sync server needs CORS
   headers on `/media/{sha256}` and the two session endpoints.
4. Extend the command set in vertical slices with the same pattern: payload → normalize →
   adapter run → confirmed command → test on two clients. Document fonts and the rest of the table
   surface are done (style, cell fill, merge/split, column/row sizing); next are the table style
   toggles (`table.options`), then SmartArt/charts.
5. Undo is covered for move/bounds/text (shape, table cell and slide notes)/fill/line/format
   (shape body, one table cell, N cells of one table, a run range or a caret — each with an exact
   body image)/transition/animation (a shape's step list for add/remove, a slide's whole sequence
   for set/timing/move/options/clear/painter) steps, a group's fill/line, slide
   insert/delete/duplicate, shape insert/delete, group/ungroup and table row/column edits
   (whole-table image), the whole table for style/fill/merge/split/sizing and a document font
   entry. What is left in this line is only the undo of commands that are not on the wire yet
   (the table style toggles, SmartArt/charts).
6. Keep the §8 gates green (`cargo test -p deckcraft-collab-server --test gates -- --nocapture --test-threads=1`); the three-client soak covers compaction, base transfer and a reconnect, so the next gate is a longer run (hundreds of steps, media) and a base above the inline limit.
7. Update `production.md` and this file at the end of the session; post a bus status with the
   commit and test numbers.

## Session log

### 2026-11-09 — the rest of the table surface: style, cell fill, merge/split and sizing

- Four command groups are collaborative as final values addressed by the table: `table.style`
  (one style name), `table.cellFill` (one fill — solid, `none` or `inherit` — over an explicit
  cell list), `table.merge`/`table.split` (final corners or the merge anchor) and
  `table.columnWidth`/`table.rowHeight` (final numbers, `keepTotal` taking the difference from
  the next column). New payloads `TableStyle`, `TableCellFill`, `TableMerge`, `TableSplit`,
  `TableColumnWidth`, `TableRowHeight` map 1:1 to the engine commands; `TableEdit` keeps its
  structural class, the six new ones are `Command` because a merge or a split moves no cell index.
- Normalise: the sender resolves the table (explicit `id`, the caret's cell, the selected cell
  range or the one selected shape) and, for a fill, expands the selection into an **explicit**
  cell list (the engine's own `selected_range` rule for merged cells, bounded at 4096 cells), so
  the receiver's selection never decides what changes. Width and height travel clamped to the
  engine's 4–4000 pt; a style name is trimmed and bounded.
- Adapter: one `table_of` helper resolves the slide and shape and requires a table; every cell,
  column and row is validated before the first write; the table shape is selected and the
  single-user command runs with the table id, so all replicas reach the same result.
- Undo: because a style, a fill, a merge or a size changes the table without naming a cell that
  could be restored on its own (and the `table.*` commands cannot express the previous value —
  a cell fill is an `Option<Fill>`, a merge moves text), the step image is the **whole table**,
  the same image `table.edit` records, restored with `shape_field{field: table}`. The engine's
  `shape.setField` now also sets the shape's box to the restored table's size, so undoing a
  width or a row height moves the box back (the engine test pins it). A foreign edit of the
  table after a step — including text typed in one of its cells — makes the undo `undo-conflict`.
- Rebase: a queued `table.cellFill` carries its cell addresses, so a row/column edit moves them
  like a cell text offset and deleting their line refuses the operation `stale`. A merge that
  covered an addressed cell (it is covered now, its text is in the anchor) refuses the queued
  text or fill operation `stale` instead of writing into an invisible cell; split moves nothing.
- Server: the welcome's `capabilities.commands` lists the six command names.
- Tests: two replicas converge on a style and a fill (the neighbour untouched, `inherit` clearing
  the explicit fill and `none` setting an empty one), on a merge and a split (spans, covered
  cells, anchor text) and on a column width with `keepTotal` (the table's total unchanged) and a
  row height (the box follows); undo of a style, a fill, a width, a height and a merge, one whole
  table per step, on two replicas with redo; an undo refused `undo-conflict` after someone typed
  in the table; unknown cells, an empty cell list, a merge past the table or of one cell, a split
  of a cell that is not merged, a missing column/row, an empty style, a shape that is not a table
  and a target without a shape all refused before the first write with no step and no version
  move. The normaliser pins the explicit cell lists (caret, range, whole table, explicit),
  clamping and every refusal. The journal tests pin a queued fill following an inserted row and a
  queued text and fill refused `stale` after a merge; the hub test runs style, fill and row
  height on two replicas and two undos, each one step.
- Gates (on `agents`, rust 1.99.1): `cargo fmt --all -- --check` clean, `cargo clippy
  --workspace --all-targets -- -D warnings` clean, `collab` 215 + 8, `collab-proto` 32,
  `collab-server` 10 unit + 4 commit + 4 gates (2.43 s, LibreOffice skipped) + 7 socket,
  `collab-web` 15, `deckcraft-web` 6, `ui-egui collab::` 20, `deckcraft-engine` 584 + 1 ignored,
  `xtask layers` 32 crates, the wasm32 check of `deckcraft-web` ok. `cargo test --workspace
  --no-fail-fast` is green except the two known host-without-`craft-fonts` failures
  (`ui-egui ime_tests::korean_composes_in_the_notes_pane` and `render
  tests::east_asian_vertical_text_stands_ideographs_upright`).
- Commit `c895183a` plus this docs commit on `collab/pr11`; `rfc.md` (§3.2.1, §4.1, §6, §13,
  §14.1), `production.md` and this file updated. Nothing pushed, CI untouched, PR #1975 unchanged.
- Not in this slice: `table.options` (the style toggles are independent booleans, a separate
  final-value choice), a ribbon gesture or input for column width / row height (the commands are
  reachable through the control channel and MCP), and a fine-grained undo image per cell
  (the whole table is exact and matches `table.edit`; a per-cell image is a later refinement).

### 2026-11-09 — document fonts: an embedded-font registry on the wire

- A font is a **document asset**, not a shape value: `p:embeddedFontLst` carries one entry per
  typeface with up to four face slots, and the face bytes are ordinary media objects. The slice
  keeps that shape. Engine module `crates/engine/src/cmd/font_asset.rs` adds four commands:
  `font.create` (one face from a file — `typeface`, `face`, `data`/`path`), `font.set` (the
  entry's `panose`/`pitchFamily`/`charset` as final values), `font.remove` and `font.restore`
  (the exact whole entry, or `null`, for collaboration undo). The engine validates the sfnt
  header and table directory, caps the file at 16 MiB, stores it as `application/x-fontdata`,
  installs the face in the font database so text draws immediately, compares a typeface
  case-insensitively, and validates every face id of a restore before the first write.
- Protocol (`crates/collab-proto`): `Payload::FontCreate { typeface, face, sha256, name }`,
  `FontSet`, `FontRemove` and `FontRestore { typeface, font: Option<Value> }`, with
  `FontFace { regular, bold, italic, boldItalic }`. A font operation names no slide: it is
  presentation-level, merge class, and changes no address or key.
- Media channel: `MediaType::Font`, an sfnt/OpenType/collection/EOT signature allowlist, a 16 MiB
  per-face cap and its own 32-object / 64 MiB budget, so a font cannot evict pictures. The
  browser link prefetches a `FontCreate` digest like a picture's, and the desktop client fetches
  it through the same `apply_command` hook before applying.
- Normalise and adapter: `CollabDoc::submit` intercepts `font.create` and puts the bytes in the
  media cache (`take_uploads` for the host), so the operation carries only the digest; the
  normaliser validates the name, face, digest and descriptors and runs before the slide-view
  guard, because the command does not need a slide. The adapter resolves `sha256` in its media
  cache (`missing_media`, retryable, when absent), refuses a digest that is another kind of
  object and a font step that names a slide, then runs the engine command.
- Undo: `history::Change::Font` keeps the whole entry before and after (the model
  `EmbeddedFont`, media ids included) and `restores()` emits one `font_restore`.
  `Restore.slide` became `Option<SlideKey>` — a font is the first presentation-level value with
  no slide to address. `capture_font` re-reads the entry for `step_matches`, so any later change
  to that entry (another participant's face or descriptor) makes the undo `undo-conflict`, while
  a change to another typeface does not.
- Tests: eight in `deckcraft-collab` (two replicas converging on create/set/remove; the undo of a
  descriptor step and of the create broadcast through the hub to both replicas, with the face
  object kept for the redo; an undo refused after a foreign change; slide-addressed, non-font,
  missing-upload, unknown-entry and empty-set refusals before any write; the normaliser, the
  upload path and the media allowlist/budget) and a socket test that uploads a face, applies
  `font_create` to the room and commits a file that reimports with the entry. The engine module
  tests cover the four commands, the validation and the save/reimport.
- Gates (on `agents`, rust 1.99.1): `fmt --check` clean, clippy `--workspace --all-targets -D
  warnings` clean, `collab-proto` 31, `collab` 205 + 8, `collab-server` 10 + 4 commit + 4 gates
  (2.44 s) + 7 socket, `collab-web` 15, `deckcraft-web` 6, `engine` 584 + 1 ignored, `ui-egui
  collab::` 20, `xtask layers` 32 crates, wasm32 check of `deckcraft-web` ok. `cargo test
  --workspace` is green except the known host-without-`craft-fonts` failures in `ui-egui`
  (Korean IME) and `render` (east-asian vertical text). Commit `fd4e54e8` plus this docs commit
  on `collab/pr11`; nothing pushed, CI untouched, PR #1975 unchanged.
- Not in this slice: a UI entry point ("Embed font…" dialog) and embedding *every* used family in
  one step; both are follow-ups, the commands are reachable through the control channel, MCP and
  the animation-free command surface.

### 2026-11-09 — the rest of the animation surface: `animation.options`, `animation.clear`, the painter

- The three commands leave a slide's **whole ordered animation sequence**, so they ride the
  existing `slide_animations` payload and the `slide.animations.set` confirmed command: no new
  wire value and no new engine command, and the undo image is the whole sequence (the image the
  `animation.set`/`timing`/`move` family records), so any animation change on that slide after
  the step is `undo-conflict`.
- Engine: `animation.options` moved onto a new public `options_plan(p, animations, index, ids,
  slide) -> (Vec<Animation>, usize)` (the `timing_plan` pattern); the per-target body — the
  option resolved in each effect's own terms, the motion path for path-class steps, the text
  build — is unchanged, so the single-user command and the normaliser compute the same codes.
  The Animation Painter moved onto a new public `painter_plan(picked, animations, ids,
  paragraphs)` plus `paragraphs(shapes, id)`: painting replaces each target's own steps with the
  picked list, and a paragraph the target does not have becomes a whole-shape step, exactly as
  before. The then-unused `each_target` was removed.
- Normalise: `normalize_with_painter` (with `normalize` delegating to it with no clipboard)
  turns `animation.options` into the slide's whole resulting sequence (nothing when it changes
  nothing), `animation.clear` into an empty sequence for the slide it addresses (`index` is a
  slide, not a step; a slide with no steps sends nothing) and `animation.painterApply` into the
  sequence `painter_plan` computes from the session clipboard. The clipboard is a session value,
  not a document value, so it travels as the resulting sequence and no replica needs it; a
  missing clipboard, no target and an unaddressable target are refused before anything is sent.
- UI: `CollabUi::pump` disarms a one-shot pick once the paint is queued, because the confirmed
  command is `slide.animations.set` and the single-user `painterApply` never runs in a session.
- Tests: the engine plans match the single-user commands (options by index, the painter's
  paragraph fallback); the normaliser pins the three forms, their no-ops and their refusals; the
  hub runs `animation.options`, `animation.painterApply` (clipboard in the session) and
  `animation.clear`, each with its undo on two replicas; the UI test pins the sequence over the
  link and the disarm. `collab` 197 + 8, `collab-proto` 30, `engine` 580 + 1 ignored, `ui-egui
  collab::` 20.
- Gates (on `agents`, rust 1.99.1): `cargo fmt --all -- --check` clean, `cargo clippy
  --workspace --all-targets -- -D warnings` clean, the four §8 gates 4/4 in 2.45 s (LibreOffice
  skipped on the build host), `xtask layers` 32 crates, the wasm32 check of `deckcraft-web` ok.
  Commit `e2a5ef41` plus this docs commit on `collab/pr11`; `production.md`, `rfc.md` and this
  file updated. Nothing pushed, CI untouched, PR #1975 unchanged.

### 2026-11-09 — the rest of the animation surface: `animation.set`, `animation.timing`, `animation.move`

- The three commands are collaborative as **one slide's whole ordered animation sequence**
  (`Payload::SlideAnimations { animations: Vec<Value> }`, `target.slide` the address). Every value
  is the model `Animation` as JSON and carries its own shape id, because `animation.move` can put a
  step past another shape's step; the per-shape `shape_animations` list could not express that
  interleaving. A later list from another participant is the later absolute value.
- Normalise runs the same plans as the single-user commands — new public
  `deckcraft_engine::cmd::animation::set_plan`, `timing_plan`, `move_plan`, with `set`, `timing`
  and `move_anim` refactored onto them. `animation.set` on a selection is one operation (the whole
  sequence once), not one version per shape; `animation.timing` by `index` changes exactly that
  step and by shape ids exactly those shapes' steps; `animation.move` clamps `to` to the end as
  the single-user command does. A command whose result equals the current sequence sends nothing,
  and an unknown effect, a missing `index`/`to`, an out-of-range index or a timing edit that names
  no step is refused `BadParams` before anything is sent.
- Engine: new `slide.animations.set { index, animations }` replaces the whole list. It bounds the
  image (4096 steps, 192 KiB), accepts only the model's own field names, requires a known effect,
  a bounded option and motion path, and validates that **every** named shape is on that slide
  before the first write; the shared `read_steps` also serves the existing per-shape
  `slide.animation.set`. The soak skips the new exact-value setter for the same reason as the
  other two.
- Adapter: resolves the slide, refuses a shape key in the target, reads every value as its own
  model animation, and runs `slide.animations.set`. Undo records the slide's whole sequence before
  and after (`Image::SlideAnimations`, `Change::SlideAnimations`) and restores it through the same
  payload; `same_state` compares the slide and the whole list, so any animation change on that
  slide after the step (including another shape's) makes the undo `undo-conflict`, while a change
  to another slide's steps does not.
- Tests: two replicas converge on `animation.set` and on the undo of the second shape's set;
  `animation.timing` changes one step and its undo restores the exact duration; `animation.move`
  crosses another shape's step on both replicas and its undo restores the whole order; a foreign
  animation edit makes the undo a conflict and writes nothing; the engine keeps interleaved steps
  through an exact list, refuses a missing shape/unknown effect/unknown field/too many steps
  before any write, and its plans match the single-user commands; the normaliser emits the exact
  sequence for set/timing/move, sends nothing for a no-op, and both wire forms round-trip.
  `collab` 195 + 8, `collab-proto` 30. Commit `b31bff47` plus this docs commit on `collab/pr11`;
  `production.md`, `rfc.md` and this file updated. Nothing pushed, CI untouched, PR #1975
  unchanged.

### 2026-11-09 — rebase onto `origin/main` (`0623b430`)

- `collab/pr10` (55 commits) rebased onto `origin/main` → `7459d798` (+ a short rebase note on that
  branch, which is the PR head); `collab/pr11` (13 commits, `--onto collab/pr10 922a4bf4`) →
  `da3199bf` (+ this docs commit). Pre-rebase branches kept as `backup/pr10-pre-rebase` and
  `backup/pr11-pre-rebase`. `merge-tree --write-tree` against `origin/main` exits 0 with no
  conflicted path for both heads, so PR #1975 is `MERGEABLE` once the forks are force-pushed.
- Four files conflicted, all resolved as unions (rationale in the rebase section above):
  `crates/engine/src/cmd/shape.rs` (import list), `apps/deckcraft-web/Cargo.toml` (page API +
  collab deps, host crate still the fork pin), `apps/deckcraft-web/src/web.rs` (`api::install` +
  the collaboration `WebShell` literal) and `Cargo.lock` (fork source for `craft-web-host`).
- `git diff backup/pr11-pre-rebase collab/pr11` is main's own `159ed07e..origin/main` patch for the
  same 29 files, byte for byte except `Cargo.lock`; no collab code was added or dropped.
- Gates on `agents`: `fmt` clean, `clippy --workspace --all-targets -D warnings` clean,
  `collab-proto` 29, `collab` 192 + 8, `collab-web` 15, `collab-server` 10 + 4 + 4 gates + 6
  socket (gates 2.52 s), `deckcraft-web` 6, `ui-egui collab::` 19, `ui-egui` full 515 + the known
  Korean-font environment failure, `engine` 577 + 1 ignored, `xtask layers` 32 crates, wasm32
  check of `deckcraft-web` ok. Nothing pushed, CI untouched.

### 2026-11-09 — N cells and a caret in one format step, with an exact per-cell undo

- `Payload::FormatSet` now carries `addresses: Vec<FormatAddress>` instead of one `cell` and one
  `range`. Each `FormatAddress` is `{ cell?: [row, col], range?: {from, to}, caret?: offset }`:
  a table cell or the shape's own body, whole, over a character range, or at a caret. An
  operation over one shape is either one body address or **N cell addresses of one table**;
  a list that mixes the two, an empty list, a repeated cell, a cell the table does not have and
  a range or caret past the addressed body are refused before the first write (version does not
  move, no step recorded).
- A selection of several cells normalises to N addresses, one per actual cell the engine would
  format (`selected_range`/`in_range`, so merged cells follow the engine's own rule), never to
  one address widened to the table. The adapter validates every address, places the engine
  selection per address (cells for whole-cell addresses, `TextSel` for a range/caret) and runs
  the single-cell command each time, restoring the receiving user's selection once at the end.
- Undo: `capture_step` records one whole-body image per address. `restores` emits `cells_text`
  (`shape.setCellBodies`) for several cell bodies of one table, and each replica writes one
  `shape.setField { field: cellText }` per cell after validating all of them. The step's `before`
  and `after` images are compared per cell, so a foreign edit of **any** addressed cell makes the
  undo `undo-conflict`; a neighbour that is not in the list is never read or written.
- A caret in a shape's own body is now an address (`caret: offset`) instead of a refusal. The
  normaliser emits the whole-body offset of the collapsed caret; the server places a collapsed
  `TextSel` on the authoritative body and runs the single-user command, so the engine decides
  what it touches: a run command widens it to the word it is in, a paragraph command takes the
  paragraph, and a caret at a word boundary only sets the formatting for what is typed next — the
  replica never widens it up front.
- Journal rebase: `rebase_text` carries a cell address per address, so a row/column edit moves
  every addressed cell (and deleting the line of any of them refuses the queued step `stale`),
  and an insert/delete shifts a range's two offsets or a caret only in its own body. A range that
  a delete emptied is refused `invalid`; a caret never empties.
- Limits (RFC §4.1): at most 4096 addresses per `format_set`, and at most 4096 cell bodies /
  192 KiB JSON per `cells_text`; past that the operation is `invalid`, because it and its undo
  would not fit the wire limit anyway.
- Tests: two replicas converge on one operation over two cells with a neighbour untouched; undo
  restores exactly the addressed cells' bodies in one `cells_text` and redo reapplies them; an
  undo after a foreign edit of one addressed cell is `undo-conflict` and writes nothing; empty,
  missing, out-of-body and repeated addresses are refused with the version unchanged; a remote
  N-cell format keeps the receiver's selection; a queued N-cell format follows an inserted row
  for all its addresses; a caret inside a word bolds the word on both replicas (undo/redo), a
  caret at a word boundary leaves existing runs alone, a paragraph command at a caret takes only
  its paragraph, and a caret past the body is `invalid` before any write; the normaliser pins one
  address per selected cell and a `caret` for a shape body; both wire forms round-trip.
  `collab` 192 + 8, `collab-proto` 29, `ui-egui collab::` 19, gates 4.
- Gates (on `agents`, rust 1.99.1): `cargo fmt --all -- --check` clean, `cargo clippy --workspace
  --all-targets -- -D warnings` clean, `xtask layers` 32 crates. Commit `6ddc0c4f` plus this docs
  commit on `collab/pr11`; `production.md`, `rfc.md` and this file updated. Nothing pushed, CI
  untouched, PR #1975 unchanged.

### 2026-11-09 — formatting a run range, with the exact-body undo

- `format.*` can address a character range inside one text body: a shape's own body or one table
  cell. `Payload::FormatSet` gained `range: Option<TextRange>`, `TextRange { from, to }` — whole-body
  character offsets, a paragraph break counted as one, the same coordinates `text.insert`/
  `text.delete` and `notes.insert` use. The engine needed no new command: the adapter puts the
  engine's `TextSel` on the target body with `anchor`/`caret` at the two positions (the table cell
  in `cell`), runs the single-user `format.*`, and puts the receiving user's selection back —
  formatting moves no text, so it stays valid.
- Why offsets, not run indices: an edit on another replica splits and merges runs, so a run index
  is not deterministic; a character offset is. Every replica turns the same two offsets into a
  `(paragraph, character)` pair with the existing `offset_position` helper, so the formatted run
  range is a deterministic function of the authoritative body (RFC §3.2.1, §4.4).
- Normalise: a text selection (`selection.text.anchor != caret`) in the addressed body becomes
  `range`; a range in another cell than the explicit `cell` is refused (the command would format
  characters of a body it does not address); a caret in a shape's own body is refused, because the
  single-user command would widen it to a word; a caret in a table cell still addresses the whole
  cell; a range across table cells is still refused, never widened. The address never travels in
  the formatting `args`.
- Validation before the first write: `to <= from`, an offset past the body (checked with
  `position_offset(offset_position(x)) == x`, so a clamped offset is not silently accepted), a
  shape without text, a table without a cell and a non-table shape with a cell address are refused
  before the engine call; no undo step is recorded and the document is unchanged.
- Journal rebase: both offsets shift past the `text.insert`/`text.delete` operations in the same
  body that arrived after `baseVersion` (the existing `shift`, keyed by shape and cell). A deletion
  inside the range shrinks it; a deletion that removes every character of it leaves an empty
  address, and the queued operation is refused `invalid` instead of formatting other characters.
- Undo: the step image is the **whole body** (a shape's `shape_field{field: text}` or a cell's
  `cell_text`), because the format splits runs at the range's ends and only the whole body names
  the previous runs exactly. Undo puts the previous runs back; redo reapplies them; a later edit of
  that body by another participant makes `same_state` fail and the undo is `undo-conflict`.
- Tests: two replicas converge on a bold range of a shape (text kept, run boundary at the range's
  end, the rest unformatted); the same inside one table cell with a neighbour untouched; undo and
  redo restore and reapply exactly the previous runs while the text after the range is untouched;
  an undo after someone typed in the body is `undo-conflict` and writes nothing; empty, reversed,
  past-the-end and missing-cell ranges are `invalid` before any write with no undo step; a remote
  range format leaves the receiver's caret alone; a queued range follows text typed before it (the
  typed characters are not bold) and is refused `invalid` when its characters are deleted; the
  normaliser pins a range in a shape, one across a paragraph break, one in a cell, the refused
  caret and the contradictory explicit cell; both wire forms round-trip. `collab` 183 + 8,
  `collab-proto` 28, `ui-egui collab::` 18, gates 4.
- Gates (on `agents`, rust 1.99.1): `cargo fmt --all -- --check` clean, `cargo clippy --workspace
  --all-targets -- -D warnings` clean. Commit `b4f13167` plus this docs commit on `collab/pr11`;
  `production.md` and this file updated. Nothing pushed, CI untouched. Still refused: the
  animation timing/options/move/clear commands, fonts as a document asset, the rest of the table
  surface, SmartArt/charts, a range across table cells and a bare caret in a shape's body.

### 2026-11-09 — formatting a table cell, with an exact cell-body undo

- `format.*` can address one table cell. `Payload::FormatSet` gained `cell: Option<(u32, u32)>`;
  the address is `(slide key, shape key, row, column)` and the command is applied to that cell's
  `TextBody`, not to the table shape's own (empty) `text`. The engine needed no new command: the
  adapter puts the engine selection on that one cell (`selection.cells = (shape, (r, c), (r, c))`),
  which is what the single-user ribbon does, and the same `format.*` command runs.
- Validation before the first write: a table without a cell address is refused `unsupported`
  (unchanged), a cell address on a shape that is not a table is `invalid`, and an address that
  names no cell of the table is `invalid`. Rejection happens before the engine call, records no
  undo step and leaves the document byte-identical; a deleted slide is `stale` through the same
  `resolve_one` path as every other command.
- Normalise: the address comes from an explicit `cell: [row, col]` (or `row`+`col`), else from the
  one selected cell (`selection.cells`), else from the cell a collapsed caret is editing. A range
  across cells, or a range inside a cell or a shape, is still a run-range format and is refused
  instead of being widened to the whole body. The address never travels in the formatting `args`
  (they are stripped before `FormatCmd::accepts`, as `id`/`ids` already were).
- Journal rebase: a queued format carries no offset, only the cell address, so it is rebased only
  by row/column edits — an insert above/left moves it and a delete of its own line refuses it
  `stale`. Two people formatting the same cell need no offset merging.
- Undo: a format step on a cell records that cell's whole body (`Image::Format(TextBody)` on a
  `Snapshot` with `cell: Some`). The restore is the new merge payload
  `Payload::CellText { cell, body }` (`cell_text`, server-built from the actor's history only,
  `check_restore` refuses a forged one) and runs through `shape.setField { field: cellText }`,
  which the engine now accepts with a `cell: [row, col]` argument: it validates the address
  against the table, then replaces that one cell body. A neighbouring cell is therefore never
  written, and a later edit of the addressed body by another participant makes `step_matches` fail
  and the undo is refused `undo-conflict`.
- Tests: two replicas converge on one cell's bold, the neighbour cell keeps its text and
  formatting, a cell format creates no shape text; undo restores the exact previous body (italic
  kept, bold gone) and the redo reapplies it, while a neighbouring cell's later step survives
  both; an undo after someone typed in the same cell is `undo-conflict` and writes nothing; a
  queued format applies to the cell as it is after another participant typed there (every run is
  bold, including their text); it follows an inserted row and is `stale` when its row is deleted;
  an unaddressable cell, a table without an address, a non-table shape and a deleted slide are
  refused before any write. The normaliser (selected cell, collapsed caret, explicit address,
  cross-cell range refused, missing cell, table without address) and both wire forms are pinned.
- Gates (on `agents`, rust 1.99.1): `cargo fmt --all -- --check` clean, `cargo clippy --workspace
  --all-targets -- -D warnings` clean, `cargo test -p deckcraft-collab -p deckcraft-collab-proto
  -p deckcraft-collab-server` = `collab` 175 + 8, `collab-proto` 27, `collab-server` 10 unit + 4
  commit + 4 gates + 6 socket; `deckcraft-engine` 575 + 1 ignored; `ui-egui` 509 + the known
  Korean-font environment failure; `xtask layers` 32 crates.
- Commit `a48c12de` plus this docs commit on `collab/pr11`; `production.md` and this file
  updated. Nothing pushed, CI untouched. Still refused: run-range formatting (a run-level
  payload), the animation timing/options/move commands, the rest of the table surface, SmartArt
  and charts.

### 2026-11-09 — slide notes text as an addressable body slice

- Notes are collaborative as one slide body. The wire carries `notes_set` (the slide's whole
  speaker-notes body as the model `TextBody` in JSON), `notes_insert` (character offset plus text)
  and `notes_delete` (a character range). The address is `target.slide` alone: notes belong to a
  slide, not to a shape, so a shape key in the target is refused as `invalid` (ADR-0002 §2). The
  engine needed no new command: `slide.notes {index, body}` already writes a whole body, and
  `text.insert`/`text.delete` already act on a `notes: true` selection.
- Normalise: `slide.notes` becomes `notes_set` with the **complete** body — `body` from the notes
  pane as it is, `text` applied to the current body with the same `replace_text` the single-user
  command runs — so the value on the wire is never a lossy string. Typing in the notes pane
  (`text.insert`, `text.delete`, `edit.delete` with `notes: true`) becomes `notes_insert`/
  `notes_delete` with whole-body character offsets; a slide that does not exist is refused
  locally with `BadParams`.
- Adapter: `resolve_slide` before the first write (a tombstoned or unknown slide is `stale`, no
  slide at all is `invalid`), the body is deserialised before the write so a value this build
  cannot read is refused, then the single-user command runs. `call_notes` borrows the engine
  selection for the call and puts the receiving user's selection back afterwards: a remote notes
  edit does not reset the caret, and a caret in the same notes body moves by the characters the
  edit inserted or removed before it (a range that swallows it clamps to the range's start).
- Journal rebase: a notes offset shifts past the notes operations that arrived after its base,
  filtered by the slide address as well as the (absent) shape, so another slide's notes never move
  it. A whole-body `notes_set` after the base makes a queued insert or delete `stale`, and the
  client resyncs instead of writing at a meaningless offset.
- Undo: `Image::Notes(TextBody)` on a `Change::Notes(NotesImage)`; the restore is the same
  `notes_set` payload with the body the step replaced, so runs, levels and bullets come back. The
  image is compared whole, so a later body from another participant refuses the undo with
  `undo-conflict`, and `check_restore` refuses a restore that names another body.
- Tests: two replicas converge on a notes insert after a slide was inserted; undo and redo keep
  the previous body and its bold run; a foreign notes edit refuses the undo; an unknown, deleted,
  unaddressed or shape-addressed slide and an unreadable body are refused before any write; the
  receiving caret follows an insert and a delete while a shape's text selection on that slide
  stays put; a whole-body replacement sends a queued insert to `stale`; the journal keeps two
  typists' text in one slide's notes and never shifts another slide's notes; the normaliser and
  the wire form are pinned.
- Gates (on `agents`): `cargo fmt --all -- --check` clean, `cargo clippy --workspace --all-targets
  -- -D warnings` clean, `cargo test -p deckcraft-collab -p deckcraft-collab-proto
  -p deckcraft-collab-server` = `collab` 166 + 8, `collab-proto` 26, `collab-server` 10 unit + 4
  commit + 4 gates + 6 socket; `xtask layers` 32 crates.
- Commit `b7025f9a` plus this docs commit on `collab/pr11`; RFC (§3.2.1, §6, §11, §13, §14.1),
  `production.md` and this file updated. Nothing pushed, CI untouched. Still refused: run-range
  formatting, formatting a table cell, the timing/options/move animation commands, SmartArt and
  charts.

### 2026-11-09 — shape animation steps as an exact-value slice

- `animation.add` and `animation.remove` are collaborative. The wire value is one shape's
  **whole ordered step list** (`Payload::ShapeAnimations { animations: Vec<Value> }`), addressed by
  slide key and shape key; each value is the model `Animation` as JSON (the `ShapeRestoreItem`
  pattern), because the model has six enums, a colour and two ids that a mirror struct in
  `collab-proto` would duplicate.
- Normalise: one shape per operation (a selection of several is refused). `animation.add` builds
  the new step with the new public `deckcraft_engine::cmd::animation::add_plan` — the same `build`
  the single-user command runs, so the two paths cannot drift — sets the addressed shape id and
  appends it to the shape's existing steps. `animation.remove` with `index` takes the shape from
  the step at that index and removes that step; without an index it removes every step of the
  addressed shape. A shape with nothing to remove sends nothing.
- Adapter: `resolve_one` on the slide and shape keys, so an unknown key, a key of another slide or
  a missing shape is refused (`invalid`/`stale`) before anything is written. Every value is
  deserialised, its shape forced to the addressed one, and the whole list is passed to the engine
  command `slide.animation.set`.
- Engine: `slide.animation.set { index, shape, animations }` bounds the list (256 steps, 64 KiB),
  accepts only the model's own field names (an object with another key is not an animation this
  build wrote), requires a known effect, validates option and path lengths, checks the shape is on
  the slide, and only then replaces the shape's steps where the first one stood (steps of other
  shapes keep their place; a shape without steps gets them at the end). `[]` clears them. The same
  command is the exact-image restore.
- Undo: `Image::Animation(Vec<Animation>)` on a `Snapshot`; the restore is the same
  `shape_animations` payload with the recorded list. `same_state` compares the slide, the shape and
  the whole list, so a later list from another participant refuses the undo with `undo-conflict`,
  while a change to another shape's steps does not.
- Tests: two replicas converge on a step and on two steps in a row; undo keeps the shape's other
  step on both replicas and redo puts the first back; a later value from another participant
  refuses the undo; an unaddressable shape is refused before any write and a forged restore is
  `invalid`; the normaliser pins the whole-list form, the index-addressed removal, the no-op and
  the refused multi-shape case; proto round trip; the engine setter (in-place replacement, clear,
  unknown effect, unknown field, missing index, missing shape, too many steps); the whole template
  through the in-memory hub (two submissions and an undo, both replicas equal).
- The engine soak no longer picks `slide.transition.set`/`slide.animation.set`: the exact-value
  setters are built by a session, and the soak's parameter generator (built from user-facing
  commands) can never produce their whole-value images; before the change seed 1 edited 36 of 400
  steps, after it 60–76, so the rate assertion measures real edits again.
- Commit `f562aea4` plus this docs commit on `collab/pr11`; RFC (§3.2.1, §6, §13, §14.1),
  `production.md` and this file updated. Nothing pushed, CI untouched. Still
  refused: `animation.set` (styles), `animation.timing`, `animation.options`, `animation.move`,
  `animation.clear`, the painter, notes text, run-range formatting and formatting a table cell.

### 2026-11-09 — table cell text and row/column edits

- Text in a table cell now travels with its address. `text.set` already carried `cell`;
  `text.insert`/`text.delete` gained `cell: Option<(row, col)>` (absent for a shape's own body),
  so the offset is inside that cell's `TextBody`, not the table shape's empty text. The local
  queue reads the cell from the caret (`TextSel.cell`) or the cell selection and refuses a table
  command without one; `text.set` falls back to the caret's cell when the command has no `cell`.
- The adapter resolves the cell's own body before mapping the character offset and puts the
  engine's `TextSel` on the cell before running `text.insert`/`text.delete`; a cell that is out of
  range or a table without a cell address is `invalid`. `text.set` passes the cell to the engine,
  which already understood it.
- Rebase is per cell: `rebase_text` compares `(shape, cell)` and only shifts offsets of operations
  in the same cell, so two people in different cells of one table converge. Plain text keys do not
  include the cell (the shape key addresses the table); the address is part of the operation.
- Structural table edits are new because a row insert moves cell addresses:
  `Payload::TableEdit { axis: row|column, at, insert }` runs `table.insertRowAbove/Below`,
  `table.deleteRow` or the column variants on the table's shape key. Local
  `table.insertRowAbove/Below`, `table.deleteRow`, `table.insertColumnLeft/Right`,
  `table.deleteColumn` normalise to it; the address is checked against the table before sending.
  Structural, so it needs `baseVersion == head`.
- Rebase of a queued cell operation (`text.insert`, `text.delete` and a whole-cell `text.set`)
  past a `table.edit`: an insert above/left of the cell moves
  the address down/right, a delete of the very line refuses the operation `stale` (retryable,
  the text cannot be placed), a delete above/left pulls it up/left. Operations in another cell of
  the same table are untouched.
- Undo: `Change::Shape` with `Image::Text` already carried `cell`; it now gets the cell from an
  insert/delete too, and the restore is `text.set{cell}`, so the shape's own body is not created
  or changed. A `table.edit` step records the **whole table** (`Image::Table`, `ShapeField::Table`,
  engine `shape.setField field: table`), because a structural edit changes the cell indices and
  their values; the engine validates the table (1–1000 rows/columns, only a table shape) before
  the write.
- Tests: two replicas editing different cells of one table converge; a cell operation resent
  after a reconnect answers the stored result and inserts once; a queued cell insert rebases past
  an inserted row and lands in the shifted cell; a queued insert is refused after its row is
  deleted; two people typing in one cell keep both insertions and a second cell's offset does not
  move; undo/redo of an insert and a delete restores the exact cell text, and a row edit's undo
  restores the whole table. `collab` 147 + 8, `collab-proto` 24, `engine` 573 + 1 ignored.
- Commit `2a91d49e` plus its docs commit and the cell-set rebase on `collab/pr11` (from `collab/pr10` at `922a4bf4`);
  nothing pushed, CI untouched. Still refused: notes text, formatting a table cell (needs a cell
  address in `FormatSet` and a cell-aware exact body restore), other table commands (style, fill,
  merge, sizing), run-range formatting and animations.

### 2026-11-09 — transitions as a final-value slice

- `transition.set` is collaborative. `Payload::SlideTransition { transition: Option<TransitionSpec> }`
  carries one slide's **whole** transition — effect, option, duration, advance timings, embedded
  sound (`MediaId` as a number), loop/stop flags and the kept XML of an effect the model does not
  name. The whole value travels because `transition.set` keeps the parts the user did not name;
  sending its arguments would let each replica merge them against a different state.
- The client computes that value with the new public
  `deckcraft_engine::cmd::transition::set_plan` — the same function the single-user command now
  runs, so the two paths cannot drift (the `align_plan`/`distribute_plan` pattern). A
  `transition.set` on several selected slides is refused in a shared session (the wire addresses
  one slide per operation); an unknown effect or a bad `index` is refused before anything is sent.
- Engine command `slide.transition.set` (`{index, transition: model JSON | null}`) deserialises and
  bounds the value (8 KiB), accepts an effect the model cannot name only together with its kept
  XML, refuses a sound id that is not in `doc.media`, and writes one `edit`. It is both the
  confirmed command and the exact-image restore of the step.
- Undo: `Change::SlideTransition(TransitionImage { slide, transition })`; `same_state` compares the
  slide key and the whole value, and the restore is the same `slide_transition` payload with the
  recorded value, so advance timings, the sound and kept XML come back. A second participant's
  later value refuses the first undo with `undo-conflict`; a forged restore (another value) is
  `invalid`.
- Rebase: nothing to rebase — the value is absolute, so a queued operation keeps the `baseVersion`
  it was sent with and the later accepted value wins (a command does not need `baseVersion == head`).
- Tests: proto round trip and command name; engine exact setter (kept XML, unknown effect without
  XML, `null`, missing value, bad sound, bad index) and `transition.set` = `set_plan`; adapter
  two-replica convergence, a second participant plus the refused first undo and his own undo,
  undo/redo of a step that replaced a transition with an advance timing, a forged restore, an
  unknown effect on the wire; client normalisation (whole final value, the `none` collapse,
  unknown effect, bad index); history image and restore. `collab` 138 + 8, `collab-proto` 23,
  `engine` 572 + 1 ignored.
- Commit `927e773c` plus this docs commit on `collab/pr10`; nothing pushed, CI untouched.

### 2026-11-09 — the browser link: session, presence, commit over the host bridge

`apps/deckcraft-web` showed the collaboration UI but could not join a room: the desktop link is a
thread with `tungstenite`, and a page has neither. `crates/collab-web` is the browser's side of the
cabinet contract (`INTEGRATION.md` §6, ADR-0016 §3) and a `Transport` over the page's `WebSocket`
that drives the same `CollabDoc` as the desktop.

- `craft-web-host` is pinned to `copoka/craft-libs` `7666e2b1` — the revision that carries
  `session`, `session-state`, `presence`, `commit` and `committed`; `storytold/craft-libs` `main`
  does not have them. `Cargo.lock` records the same source and revision.
- `HostSession`/`HostEvent`/`HostState` hold the message contract without `web_sys`, so the
  handling and the whole page-host scenario are tested on the host. The token is a credential:
  `Debug` prints `<redacted>`, it travels in the first frame and in the media `Authorization`
  header and nowhere else.
- `Transport` no longer requires `Send`. The browser link owns JS closures and `Rc`, which never
  cross a thread on wasm32; the desktop `WsTransport` spawns its own thread and stays `Send`.
- Media: a browser cannot fetch synchronously while it applies a command, so `WebLink` scans an
  `applied` frame (and a snapshot tail) for the digests it names, fetches the missing bytes with
  `fetch`, and holds the frame in arrival order until they are here — a later frame never
  overtakes a picture that is still loading. A frame whose bytes never arrive is released after
  10 s and the adapter reports the picture it cannot place, never a silent divergence. Uploads
  (`put_media`) and fetches use the session token from the bridge; no new host message is invented.
- `apps/deckcraft-web`: the shell applies `session`/`committed` on the next frame, posts
  `session-state` on a status or participant change and `presence` at the room's pace, and Save in
  a live session asks the cabinet to commit instead of downloading a copy (the cabinet owns the
  file). `?collab=1` alone still leaves the app single-user and offline.
- The headless gate (`cargo test -p deckcraft-web`) is the page-host scenario: one `session`
  message, a handshake against a real in-process `Hub` (no socket, no TLS, no threads), two
  `shape.insert` edits reaching version 2 on both the app and the room, presence published, Save
  posting `commit`, and `committed` recording version 2 with its etag. Without the message the app
  makes no network call at all.
- Verified on `agents`: `fmt --check` clean; `collab` 129 + 8, `collab-proto` 22, `collab-server`
  10 + 4 + 4 + 6, `collab-web` 15, `deckcraft-web` 3, `ui-egui collab::` 17; clippy over the
  touched crates with `-D warnings` clean; `xtask layers` 32 crates; `xtask wasm` 26 crates;
  `cargo check -p deckcraft-web --target wasm32-unknown-unknown` ok; `trunk build` (0.22.1) ok in
  4m7s cold, `dist/web` with a 49 MB `_bg.wasm`. The full `ui-egui` suite is 509 passed + 1 failed:
  `ime_tests::korean_composes_in_the_notes_pane` needs a Korean font this build host does not have
  (`deckcraft-fonts` builds without `craft-fonts` there) — unrelated to this change.
- Commit `66d4658c` on `collab/pr10`; nothing pushed, CI untouched.

### 2026-11-09 — three-client soak: compaction, base transfer and reconnect

- A client whose version fell below the journal tail could not rebuild: `resync` returned a
  cursor without state (status item 9, RFC matrix row 12). `SnapshotMsg` now carries
  `base { version, bytesBase64?, keys }`. `SnapshotMode::Base` means the replica is replaced; the
  tail above the base is replayed on top of it. `DocSession::resync` answers from the journal
  while it covers the client, and otherwise takes a snapshot on demand and sends base + tail, so
  compaction never strands a client. Above `max_base_inline` (8 MiB, `COLLAB_MAX_BASE_INLINE`) the
  bytes are omitted and the client reopens the file from the cabinet.
- Keys travel with the base (`SnapshotKeys { slides, shapes, dead, next }`). Keys are issued in
  walk order and cannot be re-derived after structural edits (ADR-0002), so `Adapter` exports the
  map and the issuer counter and a replica that rebuilt from a base restores both; the next
  insertion gets the key the server would issue.
- Engine: `Session::replace_active_document` swaps the model in place (uid, title and path kept,
  revision bumped so the UI and render caches refresh; selection and undo history reset).
  `CollabDoc` uses it for a base and refuses more than 64 MiB of base bytes.
- Server: `Config.snapshot_every` (and the new `max_base_inline`) now reach a memory-only room,
  not only the store; `base_transfers_total` / `base_bytes_total` count transfers. The counters
  are in `/metrics`.
- The fourth §8 gate (`three_clients_soak_across_compaction_and_a_base_transfer`): two editors,
  26 move/bounds/text/fill edits with `snapshot_every = 5` (0.67 s on `agents`), a viewer joining
  below the base (5 KiB at version 25, one tail step, rebuilt in 21.4 ms), an editor reconnecting
  from a two-step tail, all three at version 28 with an equal `Presentation`, and a commit at 28
  that reimports with the final boxes (420 / 350). No base is sent when the tail still covers the
  client, which the reconnect asserts.
- Tests added: proto round trip of a base and decoding of an old snapshot without one; key map
  export/import keeps keys, tombstones and the next number; `resync` from inside the tail, after
  compaction (base reloads into the snapshot document) and above the inline limit; `ClientSession`
  adopts a base head and drops work the base already covers; `CollabDoc` rebuilds a replica from
  a base and finishes it with the tail; the engine replaces the active document in place.
- Verified on `agents`: `fmt --check` clean, `collab-proto` 22, `collab` 129 + 8, `collab-server`
  10 + 4 + 4 + 6, `ui-egui collab::` 17, `deckcraft-engine` 570 + 1 ignored; clippy over the four
  collab crates, `engine` and `ui-egui` with `-D warnings` clean; `xtask layers` 31 crates.
  Commit `e007e965` plus this docs commit on `collab/pr10`; nothing pushed, CI untouched.

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
