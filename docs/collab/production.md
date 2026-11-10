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
| `COLLAB_MAX_BASE_CHUNK` | bytes of one chunk of a base above the inline limit (base64 in one frame) | `1048576` |
| `COLLAB_MAX_BASE_CHUNKS` | deepest chunk chain for one base; a bigger base is only named in the snapshot | `1024` |
| `COLLAB_STORE_DIR` | room persistence (native base + journal + media); absent keeps rooms in memory | — |
| `COLLAB_CABINET_URL` | the cabinet's commit endpoint; `{sessionId}`/`{fileId}` are substituted | — |
| `COLLAB_CABINET_TOKEN` | service token the callback is authenticated with (required with the URL) | — |
| `COLLAB_CABINET_TIMEOUT_MS` | deadline for one callback | `10000` |
| `DECKCRAFT_CORS_ORIGINS` | origins a browser page may call this server from, comma separated (`https://cabinet.example`); empty or absent = no CORS headers | — |

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

CORS (browser mode): with `DECKCRAFT_CORS_ORIGINS` set, every HTTP answer carries
`Vary: Origin`, and an answer to an origin on the list also carries
`Access-Control-Allow-Origin: <that origin>` — the origin itself, never `*`, so the browser may
send the `Authorization` header the media channel needs. A foreign origin gets no
`Access-Control-*` header (the body may still be served; the browser refuses to hand it to the
page). `OPTIONS` with an `Origin` is answered as a preflight — `204` with
`Access-Control-Allow-Methods: GET, POST, PUT, OPTIONS`,
`Access-Control-Allow-Headers: Authorization, Content-Type` and a `600`s max-age for an admitted
origin, and no `Access-Control-*` header otherwise. `PUT` is on the list because a browser
uploads media with it. With the variable empty the server behaves exactly as before: no
`Access-Control-*` and no `Vary` header, and `OPTIONS` keeps falling through to the router.
The list is compared case-insensitively and without a trailing slash; `*` is not a wildcard, and
`Origin: null` must be listed literally.

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
| `table.style` | command | one table's style as a final name (`table.style`); a table shape is the address, no cell moves |
| `table.options` | command | the six style options (`headerRow`, `totalRow`, `bandedRows`, `firstColumn`, `lastColumn`, `bandedColumns`) as **all final values** in one `table.options`; the client applies the flags it names to the table it sees and sends the whole set, so no replica's own copy decides an unnamed option; an unknown or non-boolean flag is refused before anything is sent (and by the engine before the first write) |
| `table.cellFill` | command | one final fill (solid colour, `none`, or `inherit` to drop the explicit fill) over an **explicit** cell list the sender's normaliser built from its selection (`table.cellFill`); a cell that is not listed is never written |
| `table.merge`, `table.split` | command | final merge corners or the merge anchor (`table.merge`/`table.split`); cells keep their indices, so a queued cell operation is only moved by a row/column edit, and a merge that covered an addressed cell refuses it `stale` |
| `table.columnWidth`, `table.rowHeight` | command | one final column width (`keepTotal` takes the difference from the next column) or row height in points (`table.columnWidth`/`table.rowHeight`) |
| `smartArt.setNodes`, `smartArt.addNode`, `smartArt.removeNode`, `smartArt.promote`, `smartArt.demote`, `smartArt.layout`, `smartArt.style`, `smartArt.colors` | command | the graphic's **whole** diagram value — layout, outline (text, levels, assistants, titles, pictures), style and colours — in one `smartArt.setDiagram`; the client runs the engine's `smartArt::plan`, the same function the single-user command uses, so it sends the value that command would leave (a layout change drops the file's saved layout, a style/colour change its own XML); an unknown layout/style/colour, an unaddressable node and a too-large value are refused before sending (and by the engine before the first write); the undo image is the whole group (`shape_field{field: smartArt}`), because an outline edit regenerates the children with fresh ids and replaces an imported file's drawing |
| `insert.smartArt` | structural | the **whole diagram** the insertion would create — layout, the node tree, style and colours — plus the final box, in a `smart_art_insert`; the client builds it with the engine's `smartart::insert_plan` (the same function the single-user command runs), the engine checks layout/style/colours, node count, text and JSON size before the first write, and every replica regenerates the same drawing instead of running its own sample; a hostile tree adds nothing and moves no version; the undo is the ordinary shape-delete step, and the redo restores the whole shape (diagram and generated children) |
| `smartArt.fromText` | structural | the **whole diagram** the conversion would create — layout, nodes with their text and outline levels — plus the final box, in a `smart_art_from_text`; the client builds it with the engine's `smartart::from_text_plan` (the same plan the single-user command runs, including the grown box), the engine checks the value and box before the first write, and the source text shape is replaced in place; the undo image is the whole source shape, and the redo puts the converted graphic back |
| `smartArt.toText` | structural | the **whole text body** the outline becomes — paragraphs with their levels, bullets, runs and title/bullet sizes — plus the final box, in a `smart_art_to_text`; the client builds it with the engine's `smartart::to_text_plan`, the engine checks the JSON size, paragraph/run counts and box before the first write, and the graphic is replaced by a text box; the undo image is the whole graphic (diagram and generated children with their ids) |
| `smartArt.toShapes` | structural | no value: the drawing stays and only the outline is dropped, in a `smart_art_to_shapes` addressed by the graphic; the engine refuses a shape that is not a SmartArt graphic before the first write; the undo image is the whole prior graphic and the redo puts the detached group back |
| `smartArt.picture` | command | one top-level node's picture as a **final value** — the media channel's digest, the file name and the picture's final place (source crop and opacity) — in one `smart_art_picture`; the bytes go to the media channel first, as `insert.picture` runs, and the operation stays small; the normaliser maps a selected sub-point to its top-level node and refuses a plain shape, a nested graphic, an unknown node, a digest that is not a sha256 and a place without a visible area before sending; the engine checks the top-level group, the node, the picture allowlist and its 8 MiB budget, the place and that the layout did not change before the first write, then installs the bytes with the same `add_media` call (the same id on every replica, deduplicated by content); the undo image is the whole group (`shape_field{field: smartArt}`), so an undo puts the original node back with its children and picture and the redo returns the result; the place lives on the node and round-trips as `a:srcRect` in the diagram data part |
| `chart.type`, `chart.data`, `chart.options` | command | the chart's **whole** value — type, categories (with outer levels), series, element options and any kept part — in one `chart.setWhole`; the client runs the engine's `chart::plan`, the same plan the single-user command uses, so it sends the value that command would leave; the adapter requires a chart shape and bounds the value (category/series/point counts, text, JSON size) before the first write; the undo image is the whole chart shape (`shape_field{field: chart}`), because a kept chart part cannot be replayed from arguments |
| `insert.chart` | structural | the **whole chart** the insertion would create (its sample and typed data) plus the final box, in a `chart_insert`; every replica creates that chart instead of running its own sample; the undo is the ordinary shape-delete step |
| `shape.insert` | structural | preset, final geometry, optional text; ids are deterministic on every replica |
| `insert.picture` | structural | the operation carries a sha256 reference, not bytes; see "Media channel" |
| `insert.video`, `insert.audio` | structural | same reference; the clip's bytes are allowlisted separately |
| `insert.model3d` | structural | same reference (`kind: model`); the bytes are one binary glTF by signature with a budget of their own, and the shape draws its poster or a stand-in cube |
| `model3d.camera` | command | the model's **whole final view** — `position`, `target`, `up` in slide EMU and `fov` in degrees, or `null` for the reader's default view — in one `model3d_camera`; there is no relative form, the client validates it with the engine's own `camera_param` before sending, the wire type refuses unknown fields and the engine re-checks every number before the first write; the camera is written into the slide part as `am3d:camera`, and the undo image is the whole model shape |
| `model3d.scene` | command | the model's **whole final scene** — one light (`type` `directional`/`point`/`spot`, a non-zero `direction`, `intensity` 0–1 and a spot's cone `angle` in degrees 1–179) and the environment (`ambient` 0–1 plus an optional sRGB `background`) — or `null` for the reader's default scene — in one `model3d_scene`; there is no relative form, the client validates it with the engine's own `scene_param` before sending, the wire type refuses unknown fields, and the engine re-checks every number and that the angle belongs to a spot before the first write; the scene is written into the slide part as `am3d:scene`, and the undo image is the whole model shape |
| `shape.fill` | command | a final fill: solid colour, `none` or `background`; gradients, pictures, patterns and relative transparency are refused |
| `shape.line` | command | final outline values (`color`, `width`, `dash`, `cap`, `join`, `compound`, heads) or `none` |
| `format.*` (17 commands) | command | `format.bold/italic/underline/strikethrough/superscript/subscript/font/size/color/highlight/alignLeft/alignCenter/alignRight/justify/caps/spacing/clear` on one explicit shape's whole text body or on N table cells of one table (`FormatSet { addresses: [{ cell: [row, col] }] }`); a table without a cell address, an empty address list, a repeated cell and a cell the table does not have are refused before any write instead of being widened; toggles need an explicit `on`, `args` cannot carry an address |
| `edit.delete` (shapes) | structural | explicit shape keys |
| `slide.new`, `slide.delete`, `slide.move` | structural | anchor, not index |
| `slide.duplicate` | structural | the source slide key; the copy is inserted after it |
| `transition.set` | command | the slide's **whole** transition as a final value (effect, option, duration, advance timings, embedded sound, kept XML) in one `slide.transition.set`; a `none` with nothing else to keep is the model's "no transition" |
| `animation.add`, `animation.remove` | command | one shape's **whole** ordered animation step list as a final value in one `slide.animation.set` (`shape_animations`, the model `Animation` values as JSON); the client builds a step with the engine's `animation.add_plan`, appends it, and sends the resulting list; an unaddressable shape is refused before any write |
| `animation.set`, `animation.timing`, `animation.move` | command | the slide's **whole** ordered animation sequence as a final value in one `slide.animations.set` (`slide_animations`, every model `Animation` carrying its own shape id); the client computes it with the engine's `set_plan`/`timing_plan`/`move_plan`, so styles keep each shape's place, a move may cross another shape's step and a no-op sends nothing; the undo image is the whole sequence |
| `animation.options`, `animation.clear`, `animation.painterApply` | command | the same slide-wide `slide_animations` value: `options` changes the option codes, motion path and text build of the named steps with the engine's `options_plan` (each effect resolves its own option, so no replica guesses the codes); `clear` sends an empty sequence for the slide it addresses; the painter's clipboard is a session value, so `painterApply` sends the resulting sequence computed with the engine's `painter_plan` (the targets' own steps replaced, a paragraph the target does not have becoming the whole shape) and no replica needs the clipboard; a no-op sends nothing and the one-shot pick is disarmed after sending; the undo image is the whole sequence |
| `notes.set` | command | the slide's **whole** speaker-notes body as a final value (`notes_set`, the model `TextBody` as JSON) in one `slide.notes`; the notes pane's `body` travels as it is and its `text` is applied to the current body, so runs, levels and bullets survive |
| `notes.insert`, `notes.delete` | command | character offsets inside one slide's notes body (`notes_insert`/`notes_delete`); the server rebases them against notes operations since the base and against the slide address, so two people typing in one slide's notes keep both texts and another slide's notes never shift them |
| `format.*` over a run range or a caret | command | a whole-body character range (`addresses: [{ range: {from, to} }]`, a paragraph break counted as one, the same coordinates `text.insert`/`text.delete` use) or a collapsed caret (`addresses: [{ caret: offset }]`, a shape's own body) inside a shape's body or one table cell; every replica derives the same `(paragraph, character)` pair and the same collapsed selection, the server rebases every offset, an empty or past-the-end range is refused `invalid` before any write, a caret lets the engine command decide what it touches (a word for run commands, the paragraph for paragraph commands; no pre-widening), and the undo image is the whole body it changed |
| `format.*` over N cells | command | one operation with one address per cell of one table and one undo step: exactly those cells change, a neighbour is never touched, a foreign edit of any addressed cell makes its undo `undo-conflict`, and the restore is their whole bodies in one `cells_text` |
| `arrange.group`, `arrange.ungroup`, `arrange.reorder` | structural | explicit sibling anchor |
| `shape.restore` | structural | server-built undo/redo image of whole shapes (model JSON, parent, z-order index, animations); a client cannot send it as an edit |
| `font.create` | command | embeds one face of a document font: the file goes to the media channel, the operation carries its `sha256`, the typeface and the face slot (`regular`/`bold`/`italic`/`boldItalic`); a typeface the document has no entry for gets one, an existing entry only gains/replaces that slot. Presentation-level: no slide is named, so a replica on any slide applies it |
| `font.set`, `font.remove` | command | the entry's descriptor fields (`panose`, `pitchFamily`, `charset`) as final values, or the whole entry dropped; both refuse an entry the document does not have and change no address |
| `font.restore` | command | server-built undo/redo image of one whole entry (model JSON with its four face media ids, or `null` for "no entry"); a client cannot send it as an edit |

Anything else that changes the document is refused with a "not available in a shared session
yet" message instead of being applied locally and silently diverging. The design rule stands: only
commands whose result is a deterministic function of the authoritative state, expressed as final
values, go on the wire. What is still refused: relative effects. A model's camera and scene are
collaborative now (`model3d.camera` and `model3d.scene`, each the whole final value with the whole
model shape as the undo image); the model itself is still drawn as its poster or a stand-in cube.

A few commands are **queries**, not operations: they answer a read-only question and change no
document. `smartArt.nodes` is one of them, and the server answers only queries on an explicit
allowlist (`smartArt.nodes` today) — an arbitrary engine query could render, read the server's
files or list its dictionaries, so it is never reachable this way. A client sends
`query {id, cmd, target, params}`; the server resolves the slide and shape from the session keys,
runs the engine's query on the authoritative session, restores the engine selection and answers
`query_result {id, version, result | error}` to the asker alone. No version moves, nothing enters
the journal, and a viewer may ask. A command that changes the document, a command outside the
allowlist and an unknown command are refused in the answer — they belong on the operation path,
where they get a version and an undo image.
The full command surface is the next slice; it needs a final-value normalisation for each
command, not a raw `execute` on the wire.

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
  server must be reachable from the page (same origin, or its origin listed in
  `DECKCRAFT_CORS_ORIGINS`, which makes the server answer with `Access-Control-Allow-Origin`).
- Without a `session` message the app is the ordinary single-user editor: no socket, no media
  request, and `?collab=1` alone only shows the UI in its offline state.
- Reconnect backoff is the desktop link's (250 ms doubling to 10 s); unconfirmed operations are
  resent with the same `opId` after the next welcome.

## Media channel

Pictures, audio, video and 3-D models do not travel in an operation: the socket limit is 256 KiB
and the same picture can be inserted twice. The bytes go to a content-addressed store and the
operation carries only the digest.

- `PUT /v1/sessions/{id}/media/{sha256}` with the session token (a viewer is refused). The body is
  the object, the path must match its digest. The allowlist is by signature: PNG, JPEG, GIF, WebP,
  BMP, TIFF and SVG up to 8 MiB; TrueType/OpenType, collections and Embedded OpenType (`fntdata`)
  up to 16 MiB; MP4/MOV/WebM/AVI and WAV/FLAC/Ogg/MP3/M4A/AIFF/MIDI up to 32 MiB; a binary glTF
  (`.glb`: `glTF` magic, version 2) up to 32 MiB. Pictures keep
  512 objects / 64 MiB per session, fonts their own 32 objects / 64 MiB, audio and video their
  own 32 objects / 192 MiB, and 3-D models their own 16 objects / 128 MiB, so a font, a large clip
  or a model cannot evict the pictures.
- `GET /media/{sha256}` with a session or service token returns the bytes with their content type.
  The path is normalised first — a query string, a trailing slash and an extension (`{sha}.glb`)
  are not part of the name — and the content type comes from the bytes' signature, so an object
  answers the same way from memory and after a restart.
- One object per digest per session; a repeated upload is a no-op.
- Wire: `Payload::PictureInsert { sha256, name, rect }`,
  `Payload::MediaInsert { kind: video|audio|model, sha256, name, rect }` and
  `Payload::FontCreate { typeface, face, sha256, name }`. `rect` is optional: without it the server
  and every replica compute the frame from the same bytes and slide size (a model gets a default
  box). A font has no frame: the
  entry points at one media object per face, and the `.pptx` writer emits it as
  `ppt/fonts/fontN.fntdata` (`p:embeddedFontLst`). A model is a `p:contentPart` with a `model3d`
  relationship and is copied through the package unchanged.
- With `COLLAB_STORE_DIR` the objects live under `<store>/media/<sha256>` and a journal replay
  loads them before the operation runs.
- The desktop link derives the HTTP endpoint from its WebSocket URL: `ws://` opens a plain socket,
  `wss://` a `rustls` connection with the platform root store (`rustls-native-certs`; the
  `SSL_CERT_FILE`/`SSL_CERT_DIR` variables are honoured). A custom CA must be in the system store.
- The browser link derives the same endpoints (`http://` for `ws://`, `https://` for `wss://`) and
  uses `fetch` with the token in the `Authorization` header. The page must be allowed to reach the
  server: same origin, or its origin in `DECKCRAFT_CORS_ORIGINS` (the server then answers with
  `Access-Control-Allow-Origin` and the preflight, see "HTTP").

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

Save in a live session is a commit on the desktop: the client sends `ClientMsg::Commit` with the
cabinet version the session opened from (or the one the last commit was accepted with) and does
not write the local file; the document stays dirty until `commit-result`, a `409` keeps the edits
and the banner offers Save a copy, and a viewer cannot commit (refused before the request). Save
As and Export stay local. The host-bridge messages (`deckcraft:session` / `presence` / `commit`)
remain the external contract with the cabinet for a browser page.

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
`COLLAB_MAX_BASE_INLINE` the base travels as a **chunk chain** instead: the snapshot announces
its `sha256`, length, chunk size and depth, the client asks for one chunk at a time
(`base-chunk`, RFC §4.5.1) and installs the base only after the digest matches
(`base_chunks_total` counts the chunks; `base_bytes_total` stays at 0 for a chain). The chunk size
and the chain's depth are `COLLAB_MAX_BASE_CHUNK`/`COLLAB_MAX_BASE_CHUNKS`: a base beyond them is
named but not sent, and that client has to reopen the file from the cabinet — a fallback for a
client build without `base-chunk`, not the expected path. Measured on the build host: a 120-slide
deck with 120 distinct 160×160 pictures has a native base of **11.8 MiB**, which a viewer rebuilt
in 48 chunks of 256 KiB in 168 ms; the same deck with 320×320 pictures reaches 46.9 MiB, still
inside a 1024-chunk chain at the default 1 MiB chunk.

## Verification and tests

| Suite | What it covers |
|---|---|
| `cargo test -p deckcraft-collab` | 269 unit + 8 integration: convergence, dedup, structural exclusion, tombstones, undo per actor (move, bounds, align/distribute, text, fill, outline, format on a shape, one table cell, N cells of one table and a run range or caret, group fill, slide insert/delete, shape insert/delete with z-order and animations, transition, animation step lists and whole slide sequences from set/timing/move/options/clear/painter, a model camera), text rebase, shape insert, pictures/audio/video/3-D models through the media channel (a model is one `.glb` signature with its own budget, inserted on both replicas and undone as one structural step, its camera a final value with the whole model shape as the undo image), fill/line/format/duplicate/transition/animation/cell-format/multi-cell-format/caret slices, presence, snapshots, resync (tail, base after compaction, key map, the inline limit, a base sent as a chunk chain — order, one request at a time, resume at the missing index, a second snapshot for the same chain, a stale chain, the protocol ceilings, a short chunk and a wrong digest, steps collected by a catch-up), commit |
| `cargo test -p deckcraft-collab-server` | 10 unit + 17 real-socket + 4 commit + 5 gate tests: two editors, viewer refusal, health/metrics, bad token, restart journal replay (including slide and shape undo), media upload/fetch (picture and audio allowlists), picture insert, a 3-D model by its `.glb` signature fetched by digest, with an extension and with a query (the path is normalised, the content type comes from the signature), and its camera as a final view reaching both editors, the viewer and the committed file (a relative field is a bad frame and a degenerate camera is `invalid`, neither moving the version); the cabinet callback (two copies, original versions kept, `409` on an external change, a client commit answered after the cabinet took the file, the no-callback development mode); 120 slides with media, LibreOffice on the commit, three sessions across a conflict, a three-client soak across compaction with a base transfer, and the big-deck soak: 322 edits by three editors on 120 slides with media, a viewer rebuilt from an 11.8 MiB base sent in 48 chunks plus its tail, a fifth participant that drops mid-chain and resumes at the missing chunk, and a commit at the head whose file reimports with every replica's values |
| `cargo test -p deckcraft-ui-egui collab::` | 20 tests: flag off, status, viewer gate, presence TTL/marks, participants, indicator/pane, warnings, command through the pipe, engine queue, a run-range format and a caret in a shape body carrying their character offsets, the painter's sequence over the link disarming a one-shot pick |
| `cargo test -p deckcraft-collab-web` | 15 tests: the host contract (`session` → connect, `committed` → recorded, the token redacted, role words), the media endpoints from the session URL, the digest scan of `applied`/snapshot, the ordered frame queue (a picture waits for its bytes, a later frame never overtakes it, a missing one is released after 10 s), the handshake being the first frame, frame delivery through the link |
| `cargo test -p deckcraft-web` | 3 tests: the headless page-host scenario (one `session` message, handshake against a real in-process `Hub`, two edits to version 2, presence published, Save → `commit`, `committed` recorded), single-user mode without the message, the status words |
| `cargo test -p deckcraft-engine` | 580 (1 ignored): the collaboration queue, read-only gate, a base transfer replacing the active document in place, `shape.setField`/`shape.setFields`/`shape.setBoundsMany`/`shape.restore`, `slide.restore`, `slide.transition.set`, `slide.animation.set` and `slide.animations.set` did not change single-user behaviour; `transition.set`/`set_plan`, `animation.add`/`add_plan`, `animation.set`/`timing`/`move` with their plans and `animation.options`/`painterApply` with `options_plan`/`painter_plan` (a paragraph the target lacks becomes the whole shape) leave the same values; `arrange.align`/`distribute` keep their numbers after the plan refactor |
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
2. **P1 — Media refinement**: pictures, video, audio, font faces and 3-D models are on the
   channel. A model is one binary glTF by signature (`.glb`, `glTF` v2) with its own budget —
   `insert.model3d` on the operation stream, `MediaType::Model` in the cache — its view is
   `model3d.camera` (a final `position`/`target`/`up`/`fov`, written as `am3d:camera`) and its
   scene is `model3d.scene` (one final light and environment, written as `am3d:scene`), and it
   draws its `mc:Fallback` poster when the file has one, else a stand-in cube; `GET /media` normalises the
   path (extension, query, trailing slash) and answers with the content type the signature
   implies. The browser link fetches and uploads them with `fetch` plus the session token; the
   sync server answers CORS for the origins in `DECKCRAFT_CORS_ORIGINS` (see "HTTP"), so a page on
   another origin can use the channel once its origin is listed.
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
   the address is never pre-widened, and both replicas get the same result. The animation surface
   is complete now: `animation.options`, `animation.clear` and the Animation Painter send the
   slide's whole resulting sequence, the painter's clipboard stays a session value, and each step
   is one undo. Document fonts are in too (`font.create`/`font.set`/`font.remove`, the whole entry
   as the undo image) but have no UI entry point yet: the commands are on the control channel and
   the MCP surface. The rest of the table surface is in now (style, the six style options as one
   final set, a cell fill over an explicit cell list, merge/split and column/row sizing; the undo
   image is the whole table, as for a row/column edit), and Table Layout ▸ Cell Size has the
   Column Width and Row Height fields, which carry the selected column/row as final values and run
   the same `table.columnWidth`/`table.rowHeight` commands a border drag does. A SmartArt graphic
   can be inserted now (`insert.smartArt` as one whole diagram value, the whole shape as the undo
   image), the three conversions are in (`smartArt.fromText`/`toText`/`toShapes`, each one
   structural step with the whole source shape as its undo image), a node's picture is in
   (`smartArt.picture`: the media channel's digest and the picture's final place, the whole group
   as the undo image) and `smartArt.nodes` is
   answered by the server as a read-only query; charts are in too
   (type/data/options and an insertion as one whole chart value, the whole chart shape as the undo
   image). A model's camera and scene are in; what is left in this line is relative effects.
4. **Done — Browser transport**: the web build gets its session from the host bridge and dials the
   sync server over the page's `WebSocket` (`crates/collab-web`); the token comes from
   `deckcraft:session`, media goes over `fetch` with it, and Save asks the cabinet to commit.
   What remains is deployment detail: the page must reach the sync server (same origin, or its
   origin in `DECKCRAFT_CORS_ORIGINS`), and the cabinet must implement its end of
   `POST /api/v1/internal/office/sessions/{sessionId}/commit` (`INTEGRATION.md` §5.2).
5. **P2 — Optimistic preview**: edits become visible on `applied`; correctness first, preview and
   rollback are the next latency slice.
6. **Done — Base transfer above the inline limit**: a client that missed a compacted journal tail
   is rebuilt from the native base and the key map the `snapshot` carries (up to 8 MiB,
   `COLLAB_MAX_BASE_INLINE`). Above that the base is announced as a chunk chain
   (`base.chunks`, `base-chunk`, §4.5.1) and the replica is rebuilt from the wire, without
   reopening the file from the cabinet; a socket that drops mid-chain resumes at the missing
   chunk. The chain is capped on both sides (`COLLAB_MAX_BASE_CHUNK`/`COLLAB_MAX_BASE_CHUNKS`,
   the client's 8 MiB/4096/64 MiB ceilings), and a base beyond the chain's depth still falls back
   to "named without bytes".
7. **P3 — Soaks**: the 120-slide/media performance gate, the LibreOffice open/repair gate, the
   three-client soak (26 move/bounds/text/fill edits across compaction, a viewer rebuilt from the
   base, a reconnect from the journal, commit at the final version) and the big-deck soak (322
   edits by three editors on 120 slides with media, a viewer rebuilt from an 11.8 MiB base in 48
   chunks, a fifth participant resuming a chain mid-way, commit at the head) live in
   `apps/deckcraft-collab-server/tests/gates.rs`
   (`cargo test -p deckcraft-collab-server --test gates -- --nocapture --test-threads=1`); the
   LibreOffice test skips itself with a printed line where `soffice` is absent. The big-deck soak
   takes ~16 s of the suite; what remains is running the LibreOffice gate in CI.
8. **Human action — Upstream PR**: the work is in the fork branch `copoka/deckcraft:collab/pr10`
   and open as PR #1975; the local head holds commits the PR head did not have at the last check
   (`63ec1ebe`), and the push is manual because this workspace cannot push. The body is
   [`pr10.md`](pr10.md); merging requires a human decision. Nothing was pushed to
   `storytold/deckcraft`.
