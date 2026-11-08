# Collaboration status and handoff

Updated: 2026-11-08. Branch: `collab/pr10` (media slice `7fe75711`, feature slices `0a57b61e`…`69c2ff22`, undo of fill/line/format `41fe1ad9`, docs commits on top).
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
| `collab/pr10` | `7fe75711` + `0a57b61e`…`69c2ff22` | media channel (pictures by `sha256`, upload/fetch, persistence); audio/video, TLS media client, `shape.fill`/`shape.line`/`format.*`/`slide.duplicate` |
| `collab/pr10` | `41fe1ad9` | undo of fill, line and `format.*` steps from exact before-images (`shape_field`) |

The Commit column names the slice commits; each branch also carries documentation commits. Heads:
`collab/pr9` = `47ec08fe`, `collab/pr10` = `7fe75711`, `0a57b61e`, `58eee059`, `69c2ff22`,
`41fe1ad9`, plus the docs commits on top. The working tree is clean: the chair's uncommitted
finding of 2026-11-08 is `7fe75711`; the second slice added `0a57b61e` (media kinds, TLS, command
slices, tests, docs), `69c2ff22` hardened the format command, and `41fe1ad9` added undo of
fill/line/format steps.

All work is local. Nothing was pushed to `storytold/deckcraft`; a fork and a human decision are
required for the upstream PR. The PR body is prepared in [`pr10.md`](pr10.md): title
`feat(collab): realtime collaborative editing`, one description, a review order over the slice
stack and the `PR-CHECKLIST.md` mapping. Do not push it without the human decision.

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
- Undo/redo: move, bounds, text, fill, outline and `format.*` steps. A restore carries the exact
  model value (`shape_field`) and runs through the new `shape.setField` engine command, which
  bounds the JSON and deserialises it into the model type. A group's fill/outline step is not
  recorded (the command also formats the members, and one object's image cannot put them back).
  Structural steps are still not in per-user undo.

### Server (`apps/deckcraft-collab-server`)

- HTTP: `POST/GET/DELETE /v1/sessions`, `GET /healthz`, `/readyz`, `/metrics`.
- WS: first frame is `hello` with the JWT; signature/`exp`/`aud`/app/file/session checks; actor
  and role from the token. Viewer `op` is refused before the engine.
- Limits: message size, operations/second, participants.
- Commit: `.pptx` from the authoritative document, `sha256`, base64 bytes, `warnings[]`, `409`
  on a stale `expectedVersion`.
- Media: `PUT /v1/sessions/{id}/media/{sha256}` (session token, writer only, signature allowlist
  for pictures/audio/video, 8 MiB per picture, 32 MiB per clip), `GET /media/{sha256}` (session or
  service token). Content-addressed under `<store>/media/<sha256>`; 512 pictures / 64 MiB and 32
  clips / 192 MiB per session; upload and byte counters in metrics.
- Persistence: native base + append-only journal, snapshot every N ops, replay on restart
  through the same `apply` path. Without `COLLAB_STORE_DIR` the server is memory-only.
- Logs/metrics contain ids, versions and counts; no slide content, text, media or tokens.

## Verification

| Command | Result |
|---|---|
| `cargo test -p deckcraft-collab` | 99 unit + 7 integration |
| `cargo test -p deckcraft-collab-server` | 3 unit + 6 socket tests (two editors, viewer, health/metrics, invalid token, restart replay, media upload/fetch, picture insert, audio allowlist) |
| `cargo test -p deckcraft-collab-proto` | 18 |
| `cargo test -p deckcraft-ui-egui collab::` | 17 |
| `cargo test -p deckcraft-engine` | 560 passed, 1 ignored |
| `cargo clippy -p deckcraft-engine -p deckcraft-collab -p deckcraft-collab-proto -p deckcraft-collab-server -p deckcraft-ui-egui --all-targets -- -D warnings` | clean |
| `cargo fmt --check` | clean |
| `cargo xtask layers` | 31 crates, no violations |

Re-run on 2026-11-08 after `41fe1ad9`: `fmt` clean, `collab-proto` 18, `collab` 99 + 7,
`collab-server` 3 + 6, `ui-egui collab::` 17, `deckcraft-engine` 560 + 1 ignored; clippy over the
four collab crates and `ui-egui` with `-D warnings` clean; `xtask layers` 31 crates.

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
`invalid`, and a group's fill step leaves the history empty.

## TASK.md acceptance matrix

| Criterion | State | Where / why |
|---|---|---|
| Two or more clients edit one presentation | ✅ for the v1 command set | hub, socket test, UI pipe test |
| Insert a picture in a shared session | ✅ pictures, audio and video over `ws://` and `wss://` | media channel, adapter and socket tests |
| Cursors/selections/participants visible | ✅ | `CollabUi::marks`, participants pane |
| Slides, shapes and order converge | ✅ for move/bounds/insert/delete/slide/group/order/fill/line/format/duplicate | collab tests |
| Reconnect works | ✅ within the retained journal tail | client queue + resync; base transfer for an evicted tail is still open |
| Per-user undo/redo | ✅ text/shape values, fill, outline and `format.*`; structural undo open | server history, `undo-conflict`, `shape_field` |
| Viewer cannot edit | ✅ server + engine gate | socket test, UI test |
| Commit creates a new version, original kept | ⚠️ bytes and `409` ready; cabinet callback missing | host-bridge task |
| Single-user mode unchanged; feature behind a flag | ✅ | flag-off tests |
| RFC, protocol, tests, docs | ✅ design accepted; formal `proposal` posted to `rfc/deckcraft` on 2026-11-08 | this tree, bus message |
| PR in `storytold/deckcraft` | ⏳ description prepared, nothing pushed | [`pr10.md`](pr10.md); fork + human decision |

## What is NOT done (ordered by production impact)

1. **Command surface beyond the new slices.** Fill, line, 17 `format.*` commands over whole shape
   text and `slide.duplicate` are collaborative now. Still refused: fonts as a document asset,
   animations, transitions, tables/SmartArt/charts, notes, run-range formatting and relative
   effects. The design rule stands: only commands whose result is a deterministic function of the
   authoritative state, expressed as final values, go on the wire; a raw `execute` is not safe
   (toggles and relative edits diverge). Next candidates: table cell edits, transitions,
   `arrange.align`/`distribute` as final bounds, animation steps.
2. **Media refinement.** Fonts and 3D models are not in the media allowlist, and the browser link
   still needs the same HTTP bridge. The desktop `wss://` path has a TLS client now (system roots;
   a private CA must be in the store or `SSL_CERT_FILE`).
3. **Text in tables and notes, rich runs.** Shape text is covered; table cells and notes need
   their own addressing and rebase. Formatting runs need a run-level payload, not whole-body.
4. **Undo of a group's fill/outline, and of structural steps.** Fill, line and `format.*` steps of
   one shape are undoable now (`shape_field` with the exact model value). A fill or outline on a
   group is not recorded: the command also formats the members and one object's image cannot put
   them back — the next slice needs an image per member or one merge payload. Slide/shape
   insert/delete are still out of v1.
5. **Cabinet callback / host bridge.** The cabinet must create the file version from
   `commit-result` and push `session/presence/commit` through `craft-web-host`. The desktop
   stores the bytes in `CollabUi::take_commit()`; no callback is wired. This is an external
   contract (same blocker as WordCraft).
6. **Browser transport.** `apps/deckcraft-web` has no `web-sys` WebSocket link yet; `?collab=1`
   shows the UI in a truthful offline state. A browser bridge must forward frames and media
   fetches, and the host page must send the session token.
7. **Optimistic preview.** Edits appear on `applied`, not on the keystroke/pointer release.
   Correct on a LAN, noticeable on a WAN. Preview needs a before-image and a rollback path; a
   rejected preview can be rolled back only if no other command has been applied since, otherwise
   the client must resync.
8. **Structural undo.** Undo of slide/shape insert/delete is still out of v1; merge operations
   (move, bounds, text set/insert/delete) are covered.
9. **Large-deck and compatibility gates.** A 100+ slide/media performance test, a LibreOffice
   open/repair gate for every commit, and a longer soak with three clients are still to add.
10. **Base transfer for an evicted journal tail.** `resync` sends the tail only; if the tail was
    compacted, a client that missed the base cannot rebuild. The store now survives restarts, so
    the next step is a chunked `snapshot` carrying the native base.
11. **Upstream PR.** Local only.

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
```

The JWT must be HS256 with `aud: "office-sync"`, `sub`, `role`, `fileId`, `app: "deckcraft"`,
`sessionId` and `exp`. For a production cabinet use an ES256/RS256/Ed25519 PEM in
`COLLAB_JWT_PEM` and a short-lived token from the cabinet.

## Next session: start here

1. Run the verification table above; if the tree is clean, continue on `collab/pr10` (or branch
   `collab/pr11` from it).
2. **Publish the PR only on a human decision**: the body is in [`pr10.md`](pr10.md); fork
   `storytold/deckcraft`, push the final stack as `feature/realtime-collaboration`, do not touch
   CI, do not mention agents. Nothing is pushed today.
3. Add the web-sys link once the host contract is agreed; until then keep the browser UI honest,
   and add the media bridge for `wss://` pages.
4. Extend the command set in vertical slices with the same pattern: payload → normalize →
   adapter run → confirmed command → test on two clients. Next: table cell text, transitions,
   `arrange.align`/`distribute` as final bounds, run-range formatting.
5. Undo is covered for move/bounds/text/fill/line/format. Next undo slices: an image per member
   for a group's fill/outline, then structural undo (slide/shape insert/delete).
6. Add the LibreOffice commit gate and the 100-slide performance test.
7. Update `production.md` and this file at the end of the session; post a bus status with the
   commit and test numbers.

## Session log

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
