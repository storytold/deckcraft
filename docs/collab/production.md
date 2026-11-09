# DeckCraft collaboration: production notes

This document is the operator's view of the collaboration feature: how it is deployed, what it
promises, and what it deliberately does not do yet. The protocol itself is in
[`rfc.md`](rfc.md); the current implementation state, test evidence and the next-session
checklist are in [`STATUS.md`](STATUS.md); the shared decisions are in
`.craft/protocol/ARCHITECTURE.md` and ADR 0001–0010.

## Topology

```text
cabinet (mail.lzmkm.ru)
  │  service token, session control, commit callback
  ▼
nginx / ingress  ── TLS termination, WebSocket upgrade, message-size limit
  │
  ▼
deckcraft-collab-server (this repository, apps/deckcraft-collab-server)
  │  one Hub per document, one adapter per room
  ▼
deckcraft-collab (wire types, adapter, client state machine, hub)
  │
  ▼
deckcraft-engine (authoritative model; commands are the only mutations)
```

- Desktop: `deckcraft` opens a WebSocket to `DECKCRAFT_COLLAB_URL` with the short-lived token in
  the first frame.
- Web: the same `SlideApp` behind the host bridge; `crates/collab-web` is the browser's transport,
  over the page's `WebSocket` and `fetch` (see "Browser mode").
- One room per `fileId`. The room's state is the engine document plus the session key map; the
  server is authoritative.

## Server

Binary: `deckcraft-collab-server`.

| Variable | Meaning | Default |
|---|---|---|
| `COLLAB_BIND` | listen address | `0.0.0.0:8080` |
| `COLLAB_SERVICE_TOKEN` | cabinet service token for `/v1/sessions` (required) | — |
| `COLLAB_JWT_SECRET` | HS256 secret (local runs only) | — |
| `COLLAB_JWT_PEM` | cabinet public key (ES256/RS256/Ed25519) | — |
| `COLLAB_AUDIENCE` | token audience | `office-sync` |
| `COLLAB_MAX_PARTICIPANTS` | participants per room | `16` |
| `COLLAB_MAX_MESSAGE_BYTES` | largest client frame | `262144` |
| `COLLAB_MAX_OPS_PER_SEC` | operations per second per connection | `50` |
| `COLLAB_SNAPSHOT_EVERY` | operations between base snapshots (memory-only rooms included) | `200` |
| `COLLAB_MAX_BASE_INLINE` | largest native base sent inline to a client rebuilt after compaction | `8388608` |
| `COLLAB_STORE_DIR` | room persistence (native base + journal + media); absent keeps rooms in memory | — |
| `COLLAB_CABINET_URL` | the cabinet's commit endpoint; `{sessionId}`/`{fileId}` are substituted | — |
| `COLLAB_CABINET_TOKEN` | service token the callback is authenticated with (required with the URL) | — |
| `COLLAB_CABINET_TIMEOUT_MS` | deadline for one callback | `10000` |

TLS is terminated by the ingress. The server speaks plain HTTP/WS on a private network and must
not be exposed directly.

### HTTP

```text
POST   /v1/sessions                 Authorization: Bearer <service token>
       { "fileId", "app": "deckcraft", "initialBytes"?: base64, "initialName"?: string,
         "expectedVersion"? }
       → 201 { sessionId, fileId, wsUrl, participants, version, expiresAt }

GET    /v1/sessions/{id}            → session state (participants, version)
POST   /v1/sessions/{id}/commit     Authorization: Bearer <service token>
       { "expectedVersion"?: string }
       → 200 { fileId, name?, version, sha256, fileVersion, warnings[], bytes? }
       → 409 / 500 / 502            see "Commit" below
DELETE /v1/sessions/{id}            Authorization: Bearer <service token>
GET    /healthz, /readyz            → { "ok": true, "version" }
GET    /metrics                     → Prometheus text format
```

`initialBytes` is the file the cabinet stores: `.pptx` or native `.deckcraft`. The server sniffs
the bytes; an unreadable file is a `400` and never becomes a room.

### WebSocket

```text
GET /v1/sessions/{id}/ws
```

1. The first **text frame** is `hello` with the JWT, app, document id and the last version the
   client has. The token never appears in the URL (ADR 0008).
2. The server checks the signature, `exp`, `aud`, app, `fileId` and session id; the actor and
   role come from the token, never from a message field.
3. `welcome` names the actor, role, version and the server's limits.
4. Operations, presence, resync, ping and commit follow; the server broadcasts the authoritative
   `applied` (with the confirmed command) to every participant.
5. A viewer's `op` is refused before the engine; navigation, presence and snapshots still work.

Limits: message size, operations/second, participants. The server drops a peer that exceeds them
and answers with `error{code:"rate-limit"|"too-large"}`.

### Metrics

`/metrics` exposes counters only: rooms, sessions, connections, messages, operations, rejected,
commits, resyncs, bytes in/out, media uploads and bytes, and the cabinet publication
(`cabinet_commits_total`, `cabinet_conflicts_total`, `cabinet_failures_total`). No file names,
slide content, text or tokens are logged or exported.

## Client

| Flag | Where | Meaning |
|---|---|---|
| `DECKCRAFT_COLLAB=1` | desktop | show collaboration state; without a link, edits are refused |
| `DECKCRAFT_COLLAB_URL` | desktop | `ws://` or `wss://` session endpoint |
| `DECKCRAFT_COLLAB_TOKEN` | desktop | short-lived cabinet JWT |
| `DECKCRAFT_COLLAB_SESSION` | desktop | document id the token was issued for |
| `?collab=1` | web | enables the collaboration UI; the host supplies the link |
| `deckcraft:session` | web (host → app) | opens the session: `sessionId`, `wsUrl`, `token`, `role` |

With the flag off, the application is the ordinary single-user editor: no transport is created,
no command is normalised, and `Session::is_read_only()` is `false`.

A viewer is stopped twice: the server refuses every `op`, and the engine's read-only gate
(`Session::set_read_only` + `CommandSpec::changes_document`) refuses editing, undo/redo, file
replacement and input-method commands before they run.

An editor with an attached link runs in **collaboration queue** mode: `Session::set_collab_queue`
makes every mutating command, from every path (ribbon, keyboard, pointer, IME, control channel),
return `{"queued":true}` and land in `take_pending_commands()` instead of changing the document.
`CollabUi::pump` normalises and sends those commands once per frame. `apply_remote` still applies
the server's authoritative command directly. With the flag on but no link, the engine is
read-only: an edit is refused instead of diverging.

## What v1 syncs

The first release deliberately normalises a small, idempotent command set:

| Operation | Class | Notes |
|---|---|---|
| `shape.move`, `shape.setBounds` | command | absolute position and size |
| `arrange.align`, `arrange.distribute` | command | final boxes for the whole selection in one `shape.setBoundsMany` operation: the client computes them from the engine's own plan, so no replica recomputes the slide edge or the gaps; one version and one undo step |
| `text.set` | command | whole body or one table cell (`cell: [row, col]`) |
| `text.insert`, `text.delete` | command | character offsets inside the shape body or one table cell; the server rebases them against operations since the base, so two people typing in one paragraph keep both texts, and two cells of one table never shift each other |
| `table.edit` | structural | one row/column insert or delete (`axis`, `at`, `insert`) on a table shape; a queued cell text operation rebases its address past it, and a delete of the line holding the text refuses the operation |
| `shape.insert` | structural | preset, final geometry, optional text; ids are deterministic on every replica |
| `insert.picture` | structural | the operation carries a sha256 reference, not bytes; see "Media channel" |
| `insert.video`, `insert.audio` | structural | same reference; the clip's bytes are allowlisted separately |
| `shape.fill` | command | a final fill: solid colour, `none` or `background`; gradients, pictures, patterns and relative transparency are refused |
| `shape.line` | command | final outline values (`color`, `width`, `dash`, `cap`, `join`, `compound`, heads) or `none` |
| `format.*` (17 commands) | command | `format.bold/italic/underline/strikethrough/superscript/subscript/font/size/color/highlight/alignLeft/alignCenter/alignRight/justify/caps/spacing/clear` on one explicit shape's whole text body or on N table cells of one table (`FormatSet { addresses: [{ cell: [row, col] }] }`); a table without a cell address, an empty address list, a repeated cell and a cell the table does not have are refused before any write instead of being widened; toggles need an explicit `on`, `args` cannot carry an address |
| `edit.delete` (shapes) | structural | explicit shape keys |
| `slide.new`, `slide.delete`, `slide.move` | structural | anchor, not index |
| `slide.duplicate` | structural | the source slide key; the copy is inserted after it |
| `transition.set` | command | the slide's **whole** transition as a final value (effect, option, duration, advance timings, embedded sound, kept XML) in one `slide.transition.set`; a `none` with nothing else to keep is the model's "no transition" |
| `animation.add`, `animation.remove` | command | one shape's **whole** ordered animation step list as a final value in one `slide.animation.set` (`shape_animations`, the model `Animation` values as JSON); the client builds a step with the engine's `animation.add_plan`, appends it, and sends the resulting list; an unaddressable shape is refused before any write |
| `animation.set`, `animation.timing`, `animation.move` | command | the slide's **whole** ordered animation sequence as a final value in one `slide.animations.set` (`slide_animations`, every model `Animation` carrying its own shape id); the client computes it with the engine's `set_plan`/`timing_plan`/`move_plan`, so styles keep each shape's place, a move may cross another shape's step and a no-op sends nothing; the undo image is the whole sequence |
| `notes.set` | command | the slide's **whole** speaker-notes body as a final value (`notes_set`, the model `TextBody` as JSON) in one `slide.notes`; the notes pane's `body` travels as it is and its `text` is applied to the current body, so runs, levels and bullets survive |
| `notes.insert`, `notes.delete` | command | character offsets inside one slide's notes body (`notes_insert`/`notes_delete`); the server rebases them against notes operations since the base and against the slide address, so two people typing in one slide's notes keep both texts and another slide's notes never shift them |
| `format.*` over a run range or a caret | command | a whole-body character range (`addresses: [{ range: {from, to} }]`, a paragraph break counted as one, the same coordinates `text.insert`/`text.delete` use) or a collapsed caret (`addresses: [{ caret: offset }]`, a shape's own body) inside a shape's body or one table cell; every replica derives the same `(paragraph, character)` pair and the same collapsed selection, the server rebases every offset, an empty or past-the-end range is refused `invalid` before any write, a caret lets the engine command decide what it touches (a word for run commands, the paragraph for paragraph commands; no pre-widening), and the undo image is the whole body it changed |
| `format.*` over N cells | command | one operation with one address per cell of one table and one undo step: exactly those cells change, a neighbour is never touched, a foreign edit of any addressed cell makes its undo `undo-conflict`, and the restore is their whole bodies in one `cells_text` |
| `arrange.group`, `arrange.ungroup`, `arrange.reorder` | structural | explicit sibling anchor |
| `shape.restore` | structural | server-built undo/redo image of whole shapes (model JSON, parent, z-order index, animations); a client cannot send it as an edit |

Anything else that changes the document is refused with a "not available in a shared session
yet" message instead of being applied locally and silently diverging. The design rule stands: only
commands whose result is a deterministic function of the authoritative state, expressed as final
values, go on the wire. What is still refused: fonts as a document asset, the rest of the
animation surface (`animation.options`, `animation.clear`, the painter), the rest of the table
surface (style, fill, merge/split, sizing), SmartArt/charts and relative effects. The full
command surface is the next slice; it needs a final-value normalisation for each command, not a
raw `execute` on the wire.

## Browser mode

The page that hosts the web build opens the session; the app never dials on its own
(`INTEGRATION.md` §6, ADR-0016 §3, `apps/deckcraft-web/src/host_session.rs`).

| Message | Direction | What it does |
|---|---|---|
| `deckcraft:session` | host → app | `{ sessionId, wsUrl, token, role }`: the app attaches a `CollabDoc` and dials |
| `deckcraft:session-state` | app → host | `{ status, participants }` on every change: `synced`, `pending`, `offline`, `error` |
| `deckcraft:presence` | app → host | `{ participants }` at the room's pace (5–10 Hz, ADR-0006) |
| `deckcraft:commit` | app → host | `{ sessionId }` when Save is pressed in a live session; the cabinet owns the file |
| `deckcraft:committed` | host → app | `{ version, etag }`: the cabinet stored the session as a new version |

- `craft-web-host` is pinned to `copoka/craft-libs` `7666e2b1` (the revision that carries the
  session messages); `storytold/craft-libs` `main` does not have them yet.
- The token travels in the first WebSocket frame and in the media requests' `Authorization`
  header. It is never put in a URL, a log line, `localStorage` or a DOM attribute;
  `HostSession`'s `Debug` prints `<redacted>`.
- Media: a browser cannot fetch synchronously while it applies a command, so the link fetches the
  bytes a frame names before delivering it and holds the frame in arrival order (giving up after
  10 s, so a picture that cannot be fetched is reported by the adapter rather than silently
  skipped). Uploads and fetches are `fetch` calls with the session's bearer token, so the sync
  server must be reachable from the page (same origin, or CORS enabled: the server sends no CORS
  headers yet).
- Without a `session` message the app is the ordinary single-user editor: no socket, no media
  request, and `?collab=1` alone only shows the UI in its offline state.
- Reconnect backoff is the desktop link's (250 ms doubling to 10 s); unconfirmed operations are
  resent with the same `opId` after the next welcome.

## Media channel

Pictures, audio and video do not travel in an operation: the socket limit is 256 KiB and the same
picture can be inserted twice. The bytes go to a content-addressed store and the operation carries
only the digest.

- `PUT /v1/sessions/{id}/media/{sha256}` with the session token (a viewer is refused). The body is
  the object, the path must match its digest. The allowlist is by signature: PNG, JPEG, GIF, WebP,
  BMP, TIFF and SVG up to 8 MiB; MP4/MOV/WebM/AVI and WAV/FLAC/Ogg/MP3/M4A/AIFF/MIDI up to 32 MiB.
  Pictures keep 512 objects / 64 MiB per session; audio and video keep their own 32 objects /
  192 MiB, so a large clip cannot evict the pictures.
- `GET /media/{sha256}` with a session or service token returns the bytes with their content type.
- One object per digest per session; a repeated upload is a no-op.
- Wire: `Payload::PictureInsert { sha256, name, rect }` and
  `Payload::MediaInsert { kind: video|audio, sha256, name, rect }`. `rect` is optional: without it
  the server and every replica compute the frame from the same bytes and slide size.
- With `COLLAB_STORE_DIR` the objects live under `<store>/media/<sha256>` and a journal replay
  loads them before the operation runs.
- The desktop link derives the HTTP endpoint from its WebSocket URL: `ws://` opens a plain socket,
  `wss://` a `rustls` connection with the platform root store (`rustls-native-certs`; the
  `SSL_CERT_FILE`/`SSL_CERT_DIR` variables are honoured). A custom CA must be in the system store.
- The browser link derives the same endpoints (`http://` for `ws://`, `https://` for `wss://`) and
  uses `fetch` with the token in the `Authorization` header. The page must be allowed to reach the
  server: same origin, or CORS enabled on it (not implemented yet).

## Commit

The server builds the `.pptx` from the authoritative document; the client never sends the final
file. Two ways to ask for a commit:

- `POST /v1/sessions/{id}/commit` with the service token — the cabinet's trigger ("Save", the
  last participant leaving, a TTL). The body may name `expectedVersion`; without one the session's
  own version is used.
- the `commit` WebSocket message from an editor. A viewer's `commit` is refused.

In both cases the bytes leave the process only through the **cabinet callback**: the server POSTs

```json
{ "fileId", "name"?, "bytes": "<base64 .pptx>", "sha256", "version", "expectedVersion"? }
```

to `COLLAB_CABINET_URL` (the `{sessionId}`/`{fileId}` placeholders are replaced with the room id)
with `Authorization: Bearer $COLLAB_CABINET_TOKEN`. The cabinet answers `{ "version", "etag" }`.
It creates a **new** file version and keeps the previous one: the server never overwrites the
original, and its own base/journal is not touched by a commit.

- `409` from the cabinet means the file changed outside the session (WebDAV, another client). The
  caller gets `409` (`rejected{code:"conflict"}` on the socket), no version is created, and the
  user is offered a copy. A commit whose `expectedVersion` does not match the session's own base
  is refused before the callback is made.
- The accepted `etag` becomes the session's `expectedVersion` for the next commit and is written
  to `meta.json`, so a restart still compares against the copy the cabinet holds.
- `warnings[]` (what the writer could not keep) stay in the answer; the WS `commit-result` also
  carries the bytes for the desktop, the HTTP answer does not (the callback already delivered
  them).
- Without `COLLAB_CABINET_URL` the server keeps the old behaviour: the HTTP answer contains the
  base64 `bytes` and no cabinet version is created. That is a development mode, not a save.
- A cabinet that answers `200` without an `etag` is compared by `sha256:<digest of the committed
  bytes>`.

The desktop UI has no "save to the cabinet" command yet; today the cabinet triggers the commit
through the HTTP endpoint. The host-bridge messages (`deckcraft:session` / `presence` / `commit`)
remain the external contract with the cabinet.

## Restart and persistence

With `COLLAB_STORE_DIR` set, a room is a native base (`base.deckcraft`, written atomically) plus
an append-only `journal.jsonl`. Every accepted operation is appended and flushed before the next
one is applied; a base is taken every `COLLAB_SNAPSHOT_EVERY` operations (default 200) and the
journal is trimmed to the tail after it. On start the latest base is loaded and the tail replayed
through the same `apply` path, so versions, dedup and undo history come back.

The room directory is the SHA-256 of the cabinet's file id, and `meta.json` names the file; a
file id never becomes a path component. Without `COLLAB_STORE_DIR` the server is memory-only and
must be treated as a development instance.

A client that reconnects inside the retained journal receives the tail. Once compaction removed
it, `resync` sends the native base, the session key map and the steps above the base; the client
replaces its replica and replays them (`base_transfers_total`, `base_bytes_total`). Above
`COLLAB_MAX_BASE_INLINE` the base is named but not sent: the client must reopen the file from the
cabinet, so a 100-slide deck with media needs the chunked/base-fetch follow-up.

## Verification and tests

| Suite | What it covers |
|---|---|
| `cargo test -p deckcraft-collab` | 195 unit + 8 integration: convergence, dedup, structural exclusion, tombstones, undo per actor (move, bounds, align/distribute, text, fill, outline, format on a shape, one table cell, N cells of one table and a run range or caret, group fill, slide insert/delete, shape insert/delete with z-order and animations, transition, animation step lists and whole slide sequences), text rebase, shape insert, pictures/audio/video through the media channel, fill/line/format/duplicate/transition/animation/cell-format/multi-cell-format/caret slices, presence, snapshots, resync (tail, base after compaction, key map, the inline limit), commit |
| `cargo test -p deckcraft-collab-server` | 10 unit + 6 real-socket + 4 commit + 4 gate tests: two editors, viewer refusal, health/metrics, bad token, restart journal replay (including slide and shape undo), media upload/fetch (picture and audio allowlists), picture insert; the cabinet callback (two copies, original versions kept, `409` on an external change, a client commit answered after the cabinet took the file, the no-callback development mode); 120 slides with media, LibreOffice on the commit, three sessions across a conflict, and a three-client soak across compaction with a base transfer |
| `cargo test -p deckcraft-ui-egui collab::` | 19 tests: flag off, status, viewer gate, presence TTL/marks, participants, indicator/pane, warnings, command through the pipe, engine queue, a run-range format and a caret in a shape body carrying their character offsets |
| `cargo test -p deckcraft-collab-web` | 15 tests: the host contract (`session` → connect, `committed` → recorded, the token redacted, role words), the media endpoints from the session URL, the digest scan of `applied`/snapshot, the ordered frame queue (a picture waits for its bytes, a later frame never overtakes it, a missing one is released after 10 s), the handshake being the first frame, frame delivery through the link |
| `cargo test -p deckcraft-web` | 3 tests: the headless page-host scenario (one `session` message, handshake against a real in-process `Hub`, two edits to version 2, presence published, Save → `commit`, `committed` recorded), single-user mode without the message, the status words |
| `cargo test -p deckcraft-engine` | 579 (1 ignored): the collaboration queue, read-only gate, a base transfer replacing the active document in place, `shape.setField`/`shape.setFields`/`shape.setBoundsMany`/`shape.restore`, `slide.restore`, `slide.transition.set`, `slide.animation.set` and `slide.animations.set` did not change single-user behaviour; `transition.set`/`set_plan`, `animation.add`/`add_plan` and `animation.set`/`timing`/`move` with their plans leave the same values; `arrange.align`/`distribute` keep their numbers after the plan refactor |
| `cargo clippy … --all-targets -- -D warnings`, `cargo fmt --check`, `cargo xtask layers` | clean; `xtask layers` 32 crates (`collab-web` L6), `cargo xtask wasm` 26 crates |
| `cargo check -p deckcraft-web --target wasm32-unknown-unknown` | ok |
| `trunk build` in `apps/deckcraft-web` (trunk 0.22.1) | ok in 4m7s (dev profile, cold): `dist/web` with `index.html`, the JS glue and a 49 MB `_bg.wasm` |

## Known gaps before this is a finished product

Priorities are for the production cut, not for the upstream PR: the PR ships the v1 surface and
documents the rest. `STATUS.md` tracks the same list by slice, with test evidence.

1. **P0 — Cabinet callback**: the server side is in place — `POST /v1/sessions/{id}/commit`, a
   server-to-server POST with the bytes (`{fileId, name?, bytes, sha256, version,
   expectedVersion?}` → `{version, etag}`), `409` on an external change, the accepted version
   surviving a restart. What remains is outside this repository: the cabinet's endpoint
   (`INTEGRATION.md` §5.2), the host-bridge messages (`session`, `presence`, `commit`/`committed`)
   and a desktop "save to cabinet" command. Without the cabinet endpoint a deployment still
   builds verified bytes in the process, not a new cabinet version.
2. **P1 — Media refinement**: pictures, video and audio are on the channel, but fonts and 3D
   models are not. The browser link fetches and uploads them with `fetch` plus the session token;
   the sync server has no CORS headers, so a page on another origin cannot use the channel yet.
3. **P1 — Notes, cell formatting, N-cell ranges and carets are in**. Shape text, table-cell text,
   slide notes, a table cell's formatting, a run range inside either body and a caret in a shape's
   body are addressed and rebased now: a notes edit
   addresses `(slide key, notes body)` (`notes.set`/`notes.insert`/`notes.delete`), its undo image
   is the whole text body of the slide, an unaddressable slide is refused before any write, and a
   remote notes edit keeps the receiving user's selection (a caret in the same notes body moves
   with the characters inserted or removed before it). A `format.*` command addresses a list of
   bodies — `FormatSet { addresses: [{ cell?: [row, col], range?: {from, to}, caret?: offset }] }`.
   A table cell is one address and applies to that cell's own body, not to the table's empty shape
   text; a selection of several cells is **one operation with N addresses, one per cell**, so a
   range of cells formats exactly those cells and the undo image is those cells' whole bodies
   (`cells_text`, one `shape.setCellBodies` → `shape.setField { field: cellText }` per cell, one
   atomic step). An empty address list, a missing cell, a repeated cell and a range or caret past
   the addressed body are refused `invalid` before any write (the version does not move), a
   neighbouring cell is never touched, and an undo after someone changed **any** addressed cell is
   refused `undo_conflict`. A run range carries whole-body character offsets and is rebased past
   typing that arrived before it. A caret in a shape's body is its own address: the server puts
   the same collapsed selection on the same authoritative body and runs the single-user command,
   so a run command widens it to the word it is in and a paragraph command takes the paragraph —
   the address is never pre-widened, and both replicas get the same result. What is left in this
   line: the rest of the animation (`animation.options`, `animation.clear`, the painter) and table
   surfaces and fonts as a document asset.
4. **Done — Browser transport**: the web build gets its session from the host bridge and dials the
   sync server over the page's `WebSocket` (`crates/collab-web`); the token comes from
   `deckcraft:session`, media goes over `fetch` with it, and Save asks the cabinet to commit.
   What remains is deployment detail: the page must reach the sync server (same origin, or CORS
   headers the server does not send yet), and the cabinet must implement its end of
   `POST /api/v1/internal/office/sessions/{sessionId}/commit` (`INTEGRATION.md` §5.2).
5. **P2 — Optimistic preview**: edits become visible on `applied`; correctness first, preview and
   rollback are the next latency slice.
6. **P2 — Base transfer above the inline limit**: a client that missed a compacted journal tail is
   rebuilt from the native base and the key map the `snapshot` carries (up to 8 MiB,
   `COLLAB_MAX_BASE_INLINE`). Above that the message names the base without bytes and the client
   has to reopen the file from the cabinet; a chunked or HTTP-served base is the next step.
7. **P3 — Three-client soak**: the 120-slide/media performance gate, the LibreOffice open/repair
   gate and the three-client soak (26 move/bounds/text/fill edits across compaction, a viewer
   rebuilt from the base, a reconnect from the journal, commit at the final version) live in
   `apps/deckcraft-collab-server/tests/gates.rs`
   (`cargo test -p deckcraft-collab-server --test gates -- --nocapture --test-threads=1`); the
   LibreOffice test skips itself with a printed line where `soffice` is absent. What remains is a
   longer soak and running the LibreOffice gate in CI.
8. **Human action — Upstream PR**: the work is in the fork branch `copoka/deckcraft:collab/pr10`
   and open as PR #1975; the local head holds commits the PR head did not have at the last check
   (`63ec1ebe`), and the push is manual because this workspace cannot push. The body is
   [`pr10.md`](pr10.md); merging requires a human decision. Nothing was pushed to
   `storytold/deckcraft`.
