# DeckCraft collaboration: production notes

This document is the operator's view of the collaboration feature: how it is deployed, what it
promises, and what it deliberately does not do yet. The protocol itself is in
[`rfc.md`](rfc.md); the shared decisions are in `.craft/protocol/ARCHITECTURE.md` and ADR 0001–0010.

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

## What v1 syncs

The first release deliberately normalises a small, idempotent command set:

| Operation | Class | Notes |
|---|---|---|
| `shape.move`, `shape.setBounds` | command | absolute position and size |
| `text.set` | command | whole body or one table cell |
| `text.insert`, `text.delete` | command | character offsets; the server rebases them against operations since the base, so two people typing in one paragraph keep both texts |
| `shape.insert` | structural | preset, final geometry, optional text; ids are deterministic on every replica |
| `edit.delete` (shapes) | structural | explicit shape keys |
| `slide.new`, `slide.delete`, `slide.move` | structural | anchor, not index |
| `arrange.group`, `arrange.ungroup`, `arrange.reorder` | structural | explicit sibling anchor |

Anything else that changes the document is refused with a "not available in a shared session
yet" message instead of being applied locally and silently diverging. The full command surface is
the next slice; it needs a final-value normalisation for each command, not a raw `execute` on the
wire.

## Commit

The server builds the `.pptx` from the authoritative document and answers `commit-result` with
`sha256`, `version`, inline base64 bytes and `warnings[]` (what the writer could not keep). A
stale `expectedVersion` is a `409`/`rejected{code:"conflict"}`; the session never overwrites an
externally changed file. The cabinet creates the new file version.

## Restart and persistence

With `COLLAB_STORE_DIR` set, a room is a native base (`base.deckcraft`, written atomically) plus
an append-only `journal.jsonl`. Every accepted operation is appended and flushed before the next
one is applied; a base is taken every `COLLAB_SNAPSHOT_EVERY` operations (default 200) and the
journal is trimmed to the tail after it. On start the latest base is loaded and the tail replayed
through the same `apply` path, so versions, dedup and undo history come back.

The room directory is the SHA-256 of the cabinet's file id, and `meta.json` names the file; a
file id never becomes a path component. Without `COLLAB_STORE_DIR` the server is memory-only and
must be treated as a development instance.

## Known gaps before this is a finished product

1. **Text editing in tables/notes** and rich formatting runs are not normalised yet; shape
   text (typing, inserting, deleting, whole-body replacement) is, with server-side rebase.
2. **Media**: large pictures/video are not uploaded once and referenced by hash yet.
3. **Host bridge**: the cabinet's iframe contract (`session`, `presence`, `commit`) lives in
   `craft-web-host` and is external to this repository.
4. **Browser transport**: the web build shows the UI but has no `web-sys` WebSocket link yet.
5. **Full command surface**: see "What v1 syncs"; the rest is refused, not desynchronised.
6. **Optimistic preview**: local edits become visible when `applied` returns. Correctness first;
   previews with rollback are a later slice.

These are tracked in the project task and are the acceptance blockers for the production cut.
