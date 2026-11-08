# Collaboration status and handoff

Updated: 2026-11-08. Branch: `collab/pr9`. The production operator view is
[`production.md`](production.md); the protocol design is [`rfc.md`](rfc.md).

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
| `collab/pr10` | `7fe75711` | media channel: pictures by `sha256`, upload/fetch, persistence |

All work is local. Nothing was pushed to `storytold/deckcraft`; a fork and a human decision are
required for the upstream PR.

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
  object, undo is an ordinary new operation.
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
  The desktop link uploads and fetches over plain `http://` derived from its `ws://` URL; a
  `wss://` link needs a TLS HTTP client and refuses with a visible error until the host supplies
  one.

### Server (`apps/deckcraft-collab-server`)

- HTTP: `POST/GET/DELETE /v1/sessions`, `GET /healthz`, `/readyz`, `/metrics`.
- WS: first frame is `hello` with the JWT; signature/`exp`/`aud`/app/file/session checks; actor
  and role from the token. Viewer `op` is refused before the engine.
- Limits: message size, operations/second, participants.
- Commit: `.pptx` from the authoritative document, `sha256`, base64 bytes, `warnings[]`, `409`
  on a stale `expectedVersion`.
- Media: `PUT /v1/sessions/{id}/media/{sha256}` (session token, writer only, image allowlist,
  8 MiB), `GET /media/{sha256}` (session/service token). Content-addressed under
  `<store>/media/<sha256>`; 512 objects / 64 MiB per session; upload and byte counters in metrics.
- Persistence: native base + append-only journal, snapshot every N ops, replay on restart
  through the same `apply` path. Without `COLLAB_STORE_DIR` the server is memory-only.
- Logs/metrics contain ids, versions and counts; no slide content, text, media or tokens.

## Verification

| Command | Result |
|---|---|
| `cargo test -p deckcraft-collab` | 75 unit + 6 integration |
| `cargo test -p deckcraft-collab-server` | 3 unit + 5 socket tests (two editors, viewer, health/metrics, invalid token, restart replay, media upload/fetch) |
| `cargo test -p deckcraft-collab-proto` | 16 |
| `cargo test -p deckcraft-ui-egui collab::` | 17 |
| `cargo test -p deckcraft-engine` | 558 passed, 1 ignored |
| `cargo clippy -p deckcraft-engine -p deckcraft-collab -p deckcraft-collab-proto -p deckcraft-collab-server -p deckcraft-ui-egui --all-targets -- -D warnings` | clean |
| `cargo fmt --check` | clean |
| `cargo xtask layers` | 30 crates, no violations |

The socket tests were run against a real loopback server; the restart test creates a room, moves
the version, shuts the server down, starts a new one on the same store and asserts the version
and document came back. The media tests cover a rejected upload (viewer, wrong digest, no token),
one object for repeated bytes, a picture applied from a reference, and a journal replay that
loads the object before running the operation.

## TASK.md acceptance matrix

| Criterion | State | Where / why |
|---|---|---|
| Two or more clients edit one presentation | ✅ for the v1 command set | hub, socket test, UI pipe test |
| Insert a picture in a shared session | ✅ pictures on a plain `ws://` link | media channel, adapter and socket tests; video/audio open |
| Cursors/selections/participants visible | ✅ | `CollabUi::marks`, participants pane |
| Slides, shapes and order converge | ✅ for move/bounds/insert/delete/slide/group/order | collab tests |
| Reconnect works | ✅ within the retained journal tail | client queue + resync; base transfer for an evicted tail is still open |
| Per-user undo/redo | ✅ text/shape values; structural undo open | server history, `undo-conflict` |
| Viewer cannot edit | ✅ server + engine gate | socket test, UI test |
| Commit creates a new version, original kept | ⚠️ bytes and `409` ready; cabinet callback missing | host-bridge task |
| Single-user mode unchanged; feature behind a flag | ✅ | flag-off tests |
| RFC, protocol, tests, docs | ✅ | this tree |
| PR in `storytold/deckcraft` | ❌ | fork + human decision |

## What is NOT done (ordered by production impact)

1. **Media beyond pictures.** The channel carries images only: video, audio, 3D models and fonts
   need their own allowlists and limits, and a `wss://` desktop link needs a TLS HTTP client
   (plain `ws://` already uploads and fetches). The browser link needs a media bridge too.
2. **Full command surface.** Only the operations in `production.md` "What v1 syncs" are
   collaborative. Fill, fonts, animations, transitions, tables, charts, SmartArt, diagrams and
   most slide commands are refused in a shared session. The design rule stands: only commands
   whose result is a deterministic function of the authoritative state, expressed as final
   values, go on the wire; a raw `execute` is not safe (toggles and relative edits diverge).
   Next candidates: `shape.fill/line`, `format.*` over explicit shapes/runs, `slide.duplicate`,
   table cell edits, transitions.
3. **Text in tables and notes, rich runs.** Shape text is covered; table cells and notes need
   their own addressing and rebase. Formatting runs need a run-level payload, not whole-body.
4. **Cabinet callback / host bridge.** The cabinet must create the file version from
   `commit-result` and push `session/presence/commit` through `craft-web-host`. The desktop
   stores the bytes in `CollabUi::take_commit()`; no callback is wired. This is an external
   contract (same blocker as WordCraft).
5. **Browser transport.** `apps/deckcraft-web` has no `web-sys` WebSocket link yet; `?collab=1`
   shows the UI in a truthful offline state. A browser bridge must forward frames and media
   fetches, and the host page must send the session token.
6. **Optimistic preview.** Edits appear on `applied`, not on the keystroke/pointer release.
   Correct on a LAN, noticeable on a WAN. Preview needs a before-image and a rollback path; a
   rejected preview can be rolled back only if no other command has been applied since, otherwise
   the client must resync.
7. **Structural undo.** Undo of slide/shape insert/delete is still out of v1; merge operations
   (move, bounds, text set/insert/delete) are covered.
8. **Large-deck and compatibility gates.** A 100+ slide/media performance test, a LibreOffice
   open/repair gate for every commit, and a longer soak with three clients are still to add.
9. **Base transfer for an evicted journal tail.** `resync` sends the tail only; if the tail was
   compacted, a client that missed the base cannot rebuild. The store now survives restarts, so
   the next step is a chunked `snapshot` carrying the native base.
10. **Upstream PR.** Local only.

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

1. Run the verification table above; if the tree is clean, continue on `collab/pr9` (or branch
   `collab/pr10` from it).
2. Extend the media channel to video/audio (allowlist, limits, adapter commands) and add the TLS
   HTTP client for `wss://` links; test a large picture and a picture inserted twice.
3. Extend the command set in vertical slices with the same pattern: payload → normalize →
   adapter run → confirmed command → test on two clients. Start with `shape.fill`, `shape.line`,
   `format.*` over explicit shapes, then `slide.duplicate`.
4. Add the web-sys link once the host contract is agreed; until then keep the browser UI honest.
5. Add the LibreOffice commit gate and the 100-slide performance test.
6. Update `production.md` and this file at the end of the session; post a bus status with the
   commit and test numbers.
