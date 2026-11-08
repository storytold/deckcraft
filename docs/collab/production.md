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
- Web: the same `SlideApp` behind the host bridge; the browser link is a separate transport
  (the iframe contract is external to this repository).
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
| `text.set` | command | whole body or one table cell |
| `text.insert`, `text.delete` | command | character offsets; the server rebases them against operations since the base, so two people typing in one paragraph keep both texts |
| `shape.insert` | structural | preset, final geometry, optional text; ids are deterministic on every replica |
| `insert.picture` | structural | the operation carries a sha256 reference, not bytes; see "Media channel" |
| `insert.video`, `insert.audio` | structural | same reference; the clip's bytes are allowlisted separately |
| `shape.fill` | command | a final fill: solid colour, `none` or `background`; gradients, pictures, patterns and relative transparency are refused |
| `shape.line` | command | final outline values (`color`, `width`, `dash`, `cap`, `join`, `compound`, heads) or `none` |
| `format.*` (17 commands) | command | `format.bold/italic/underline/strikethrough/superscript/subscript/font/size/color/highlight/alignLeft/alignCenter/alignRight/justify/caps/spacing/clear` on one explicit shape's text; toggles need an explicit `on`, `args` cannot carry an address |
| `edit.delete` (shapes) | structural | explicit shape keys |
| `slide.new`, `slide.delete`, `slide.move` | structural | anchor, not index |
| `slide.duplicate` | structural | the source slide key; the copy is inserted after it |
| `arrange.group`, `arrange.ungroup`, `arrange.reorder` | structural | explicit sibling anchor |
| `shape.restore` | structural | server-built undo/redo image of whole shapes (model JSON, parent, z-order index, animations); a client cannot send it as an edit |

Anything else that changes the document is refused with a "not available in a shared session
yet" message instead of being applied locally and silently diverging. The full command surface is
the next slice; it needs a final-value normalisation for each command, not a raw `execute` on the
wire.

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

## Verification and tests

| Suite | What it covers |
|---|---|
| `cargo test -p deckcraft-collab` | 122 unit + 8 integration: convergence, dedup, structural exclusion, tombstones, undo per actor (move, bounds, align/distribute, text, fill, outline, format, group fill, slide insert/delete, shape insert/delete with z-order and animations), text rebase, shape insert, pictures/audio/video through the media channel, fill/line/format/duplicate slices, presence, snapshots, commit |
| `cargo test -p deckcraft-collab-server` | 10 unit + 6 real-socket + 4 commit tests: two editors, viewer refusal, health/metrics, bad token, restart journal replay (including slide and shape undo), media upload/fetch (picture and audio allowlists), picture insert; the cabinet callback (two copies, original versions kept, `409` on an external change, a client commit answered after the cabinet took the file, the no-callback development mode) |
| `cargo test -p deckcraft-ui-egui collab::` | 17 tests: flag off, status, viewer gate, presence TTL/marks, participants, indicator/pane, warnings, command through the pipe, engine queue |
| `cargo test -p deckcraft-engine` | 569 (1 ignored): the collaboration queue, read-only gate, `shape.setField`/`shape.setFields`/`shape.setBoundsMany`/`shape.restore` and `slide.restore` did not change single-user behaviour; `arrange.align`/`distribute` keep their numbers after the plan refactor |
| `cargo clippy … --all-targets -- -D warnings`, `cargo fmt --check`, `cargo xtask layers` | clean |

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
2. **P1 — Command surface**: fill, line and 17 `format.*` commands over explicit shapes and
   `slide.duplicate` are normalised now; fonts, animations, transitions, tables, diagrams and
   charts are still refused (see "What v1 syncs"). The largest remaining product gap; add
   commands in vertical slices, one normalisation each.
3. **P1 — Media refinement**: pictures, video and audio are on the channel, but fonts and 3D
   models are not, and the browser link needs the same HTTP bridge (the desktop `wss://` path
   now has a TLS client).
4. **P1 — Text in tables/notes and rich runs** are not normalised; shape text is. Table cells and
   notes need their own addressing and rebase; formatting runs need a run-level payload.
5. **P2 — Browser transport**: the web build shows the UI but has no `web-sys` WebSocket link; the
   host page must also forward media fetches and the session token.
6. **P2 — Optimistic preview**: edits become visible on `applied`; correctness first, preview and
   rollback are the next latency slice.
7. **P2 — Base transfer for a compacted journal tail**: restart and tail replay work, but a client
   that missed the base needs a chunked `snapshot`.
8. **P2 — Large-deck gate**: a 100+ slide/media performance run and a LibreOffice open/repair gate
   for commits are still to add; the three-client soak is short.
9. **Human action — Upstream PR**: the work is in the fork branch `copoka/deckcraft:collab/pr10`
   and open as PR #1975; the local head holds commits the PR head did not have at the last check
   (`63ec1ebe`), and the push is manual because this workspace cannot push. The body is
   [`pr10.md`](pr10.md); merging requires a human decision. Nothing was pushed to
   `storytold/deckcraft`.
