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

TLS is terminated by the ingress. The server speaks plain HTTP/WS on a private network and must
not be exposed directly.

### HTTP

```text
POST   /v1/sessions                 Authorization: Bearer <service token>
       { "fileId", "app": "deckcraft", "initialBytes"?: base64, "expectedVersion"? }
       → 201 { sessionId, fileId, wsUrl, participants, version, expiresAt }

GET    /v1/sessions/{id}            → session state (participants, version)
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
commits, resyncs, bytes in/out. No file names, slide content, text or tokens are logged or
exported.

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

The server builds the `.pptx` from the authoritative document and answers `commit-result` with
`sha256`, `version`, inline base64 bytes and `warnings[]` (what the writer could not keep). A
stale `expectedVersion` is a `409`/`rejected{code:"conflict"}`; the session never overwrites an
externally changed file.

The desktop client stores the last committed file in `CollabUi::take_commit()` and shows the
warning banner. The **cabinet callback** that would create the new file version is not wired yet:
that is the host-bridge task (external contract). Until it exists, commit produces verified bytes
in the process, not a new cabinet version.

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
| `cargo test -p deckcraft-collab` | 99 unit + 7 integration: convergence, dedup, structural exclusion, tombstones, undo per actor (move, bounds, text, fill, outline, format), text rebase, shape insert, pictures/audio/video through the media channel, fill/line/format/duplicate slices, presence, snapshots, commit |
| `cargo test -p deckcraft-collab-server` | 3 unit + 6 real-socket tests: two editors, viewer refusal, health/metrics, bad token, restart journal replay, media upload/fetch (picture and audio allowlists), picture insert |
| `cargo test -p deckcraft-ui-egui collab::` | 17 tests: flag off, status, viewer gate, presence TTL/marks, participants, indicator/pane, warnings, command through the pipe, engine queue |
| `cargo test -p deckcraft-engine` | 560: the collaboration queue, read-only gate and `shape.setField` did not change single-user behaviour |
| `cargo clippy … --all-targets -- -D warnings`, `cargo fmt --check`, `cargo xtask layers` | clean |

## Known gaps before this is a finished product

Priorities are for the production cut, not for the upstream PR: the PR ships the v1 surface and
documents the rest. `STATUS.md` tracks the same list by slice, with test evidence.

1. **P0 — Cabinet callback and host bridge**: `session`, `presence`, `commit` in `craft-web-host`
   are an external contract; without them there is no new file version in the cabinet. This is the
   only gap that blocks a usable cabinet deployment.
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
7. **P2 — Undo of a group's fill/outline and of structural steps** (slide/shape insert/delete) is
   still out of v1, as RFC §13 records. Move, bounds, text, fill, outline and `format.*` steps of
   one shape are undoable.
8. **P2 — Base transfer for a compacted journal tail**: restart and tail replay work, but a client
   that missed the base needs a chunked `snapshot`.
9. **P2 — Large-deck gate**: a 100+ slide/media performance run and a LibreOffice open/repair gate
   for commits are still to add; the three-client soak is short.
10. **Human action — Upstream PR**: the work is local branches; a fork and a human decision are
    required. The prepared body is [`pr10.md`](pr10.md); nothing was pushed.
