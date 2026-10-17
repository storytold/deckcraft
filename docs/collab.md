# Live sessions (co-editing)

> **Last reviewed:** 2026-10-16 · **Last updated:** 2026-10-16 · **Change:** major (the web build edits live in Nextcloud Office through craft-live; previously minor: document chat) · **Target:** Microsoft PowerPoint (Microsoft 365)

Several people edit one presentation at the same time, each in their own DeckCraft, and see each
other's selections, carets and edits as they happen: DeckCraft's equivalent of PowerPoint's
co-authoring. It's **native**: DeckCraft merges the edits itself (a CRDT, Yjs through the
[`yrs`](https://crates.io/crates/yrs) crate), shows who's there and keeps a per-person undo.
Moving the bytes between people is a **transport add-in**'s job (MQTT, a local relay, a
y-websocket server, Nextcloud…), which never needs to understand a presentation. With a room key
the content is encrypted end to end, so the transport only ever sees ciphertext.

The protocol is shared with GridCraft and WordCraft:
[craft plugin protocol 2](https://github.com/sirmmo/craft-collab/blob/main/docs/protocol-2.md)
(push channel, rooms, `collab/*` messages, encryption, limits). The shared document is
**DeckCraft schema 1**, specified (normatively) in craft-collab's
[`docs/schemas/deckcraft-1.md`](https://github.com/sirmmo/craft-collab/blob/main/docs/schemas/deckcraft-1.md);
this page summarises it and says how the app behaves.

## Using it

**Share**, at the right end of the title bar. Not in a session, it opens the Share window: pick
the transport add-in, your name and colour, then **Start live session** or **Join a session…**.
A transport with its own `share` and `join` commands (craft-collab's craft-mqtt and loopback have
them) takes over from there: it asks for what it needs (a broker, an invitation link) in its own
dialog and answers with the room to join. For a transport without them, type the room and its
key (Encrypt end to end makes a new key when starting). With no transport add-in loaded, the
window says one is needed and opens the Add-ins window.

In a session, the button shows the session's state with a dot (green: **Live**; amber:
**Connecting…** or **Joining…**; grey: **Offline**, editing goes on here; red: another app's
room) and opens the session popover: the state, whether it's encrypted, the room id and
transport, the transport's message, everyone in it (circle, name, "(you)"; click a name to go to
their selection), **Copy invitation** when the transport's Share or Join gave a link
(`result.link`), and **Leave** (the presentation stays open with every edit, as an ordinary
presentation; it isn't saved automatically).

To the left of Share, the people in the session are overlapping circles with their initials on
their colour (black or white text, whichever reads), you first, at most four and then "+N"
(hovering it lists the rest; clicking it opens the popover). Hover a circle for the name ("(you)"
for yourself); click it to go to the slide that person is on and select what they selected.

On the slide, others' selections are outlined in their colours with a name tag, their caret is
drawn where they're typing (it follows the text as you both type), and shapes they change are
outlined in amber for two seconds. The status bar adds the session's state after the slide
counter ("Slide 2 of 9 · Joining…"). While the room's document hasn't arrived yet (**Joining…**)
and for viewers (`role: view`), the presentation is read-only.

Undo and Redo, while in a session, undo **your** changes only; others' edits never enter your
undo history. Saving works as usual: each person saves their own copy.

## In Nextcloud (craft-live)

[craft-live](https://github.com/sirmmo/craft-live) is a WOPI editor that Nextcloud Office uses in
place of Collabora. It opens presentations in DeckCraft's **web build**, live: everyone who opens a
`.pptx` in Nextcloud's Files edits the same presentation in their browser, and the desktop app
joins the same rooms through craft-live's nextcloud-live add-in. The web build speaks craft-live's
embed contract (craft-live's `docs/design.md` §9) natively; there are no add-ins in the browser.

**Using it.** Click a presentation in Nextcloud. It opens in DeckCraft inside Nextcloud's page,
with the people editing it as circles in the title bar, the session popover (the room, who's
there) and the Chat pane, as in any live session. The first person to open the file loads it
into the room; the next ones see "Joining…" (read-only) until it's there. Save (⌘S, the quick
access toolbar, File › Save) saves into Nextcloud now; craft-live also saves on its own (after
200 edits, 30 s without one, at least every 5 min, and soon after the last editor stops), as a
new version of the same file. **Close** at the right end of the title bar (or the popover's
Close, or File › Close) goes back to Nextcloud's Files. People with view-only access see the
presentation read-only (they chat). If the connection drops, editing goes on and the
presentation merges when it's back (it reconnects after 1 s, then up to every 30 s); an expired
session says to open the file again.

**How it works** (`deckcraft_engine::live`, the browser side in `apps/deckcraft-web/src/live.rs`):

- **Configuration.** craft-live loads `index.html#craft-live=1&ws=…&room=…&token=…&user=…&role=…&seed=…&file=…&name=…&format=…&api=…&lang=…`
  (form-encoded, in the fragment so the token never reaches a server log). The app posts
  `craft-live:hello` to the shell around it (same origin) as soon as it runs and accepts
  `craft-live:config` with the same values; messages from anyone but the parent on the same
  origin are ignored.
- **The room** is the engine's ordinary co-editing room, served in memory under the transport id
  `craft-live` (the popover says "through craft-live"). Its `collab/sync1|sync2|update|awareness`
  messages are the y-websocket frames one to one (lib0 framing, protocol 2 Appendix B), on
  `<ws>?token=<token>` with binary frames: `0/0` sync step 1, `0/1` sync step 2, `0/2` update,
  `1` awareness, `2/0` permission denied, `3` query awareness (ignored: awareness renews every
  15 s) and `100` craft-live's control messages (JSON). Every committed command is one update,
  as on the desktop; the room's `user` is Nextcloud's person (`{id, name, color}`), which also
  signs the chat.
- **Seeding** (`seed=1`): `GET <file>` with `Authorization: Bearer <token>`, opened as File ›
  Open does (a 0-byte file is a new presentation), then `collab.join` mode `share`: the room is
  built in one transaction. **Joining** (`seed=0`): mode `join`, read-only "Joining…" until the
  first sync step 2. The placeholder shown while loading is closed.
- **Connecting:** `hello {clientId, can: ["upload"], client}` at once; the room's sync step 1 and
  everything after it wait for `welcome`. A seeder whose `welcome` has neither `seed` nor
  `seeded` (the lease went to someone else) never sends its copy: it leaves, asks again
  (`POST <api>/prepare`) and seeds or joins as answered. `reseed` (the seeder left early) does
  the same.
- **Control messages:** `welcome`; `peers` (the room's people for the popover, and
  `craft-live:peers` with yourself `self: true`); `save {id, mode: "upload", url, name, format}`:
  the presentation's bytes in `format` (`pptx`, `pptm`, `potx`, `potm`, `ppsx`, `ppsm` as asked;
  anything else, `.odp`, as a `.pptx` under a `.pptx` name, which craft-live keeps as a copy
  beside the file), `PUT <url>` with the bearer token and `X-Craft-File-Name` (percent-encoded),
  without blocking editing; a failed upload, or `mode: "webdav"`, answers `saveResult {id, ok:
  false, error}`; `saved` / `saveFailed` / `modified` / `seeded` (the shell's
  `craft-live:modified`, and `craft-live:saved` after a save the person asked for: Save sends
  `saveNow`, Nextcloud's `Action_Save` arrives as `craft-live:save`); `notice` (status bar for
  info, a toast for warnings and errors); `expired`.
- **Close codes:** 4401 (expired) and 4403 (refused, with the permission-denied reason) end the
  session with a message; anything else reconnects with backoff (1 s doubling to 30 s), says
  `hello` and sync step 1 again, and both sides' edits merge.
- **To the shell:** `craft-live:loaded` once the presentation is shown (seeded, or joined),
  `modified`, `saved`, `peers`, `close` (Close, File › Close, the popover's Close) and `error`
  (the file couldn't be loaded or read, before it was shown). `craft-live:close` from the shell
  (Nextcloud closes the editor) leaves the room; `craft-live:getPeers` answers with the peers;
  `craft-live:host` messages are ignored.
- **Desktop:** `collab/snapshot {room, file: true, format}` answers in `format` when DeckCraft
  writes it (`.pptx` otherwise), with `name` and `format` in the answer, so nextcloud-live saves a
  `.pptm` or a `.potx` in its own format.

DeckCraft doesn't read or write `.odp`: craft-live lists `.odp` for DeckCraft, and opening one
says "DeckCraft can't open …" (`craft-live:error`). `lang` isn't used yet (DeckCraft's interface
is English). Building the web build for craft-live: [packaging/web/README.md](../packaging/web/README.md#craft-live-nextcloud-office).

## Commands

| Command | Params | Does |
|---|---|---|
| `collab.share` | `{plugin, room, user?, role?, key?, title?, link?}` | Starts a session from the active presentation (no params: the Share window) |
| `collab.join` | `{plugin, room, mode: share\|join, user?: {id, name, color}, role?: edit\|view, key?, path?, title?, link?}` | Joins or starts a session (no params: the Share window). `share` seeds the room from the active presentation (or `path`); `join` opens a new presentation (or `path`) filled from the room |
| `collab.leave` | `{room?}` | Leaves (default: the active presentation's room) |
| `collab.status` | `{}` | `{rooms: [{room, plugin, state, peers, listed, role, encrypted, clientId, user, title, document, message, link, canUndo, canRedo}], transports, user}` |
| `collab.setUser` | `{name?, color?}` | Your name (the author name comments carry) and colour (`#RRGGBB`) |
| `chat.list`, `chat.send`, `chat.edit`, `chat.delete`, `chat.clear`, `chat.export`, `chat.typing`, `view.chat` | | The document chat ([below](#chat)) |

A plugin command's result may carry `collab: {join: {…}}` (with protocol 1.2's `open`, `dialog`
and `message`): `plugins.run` returns it to the caller, and the interface runs `collab.join` with
it (the plugin's id as `plugin`) when a button, menu item, plugin dialog or the Share window ran
the command, keeping `result.link` as the room's invitation. Programmatic calls never open
dialogs or windows.

Rust API (for embedders, tests and the web build): `Session::collab_poll(now_ms)` runs the
sessions between frames; `Session::collab_serve_in_memory(id)` / `collab_join_in_memory`,
`collab_take_outgoing` and `collab_poll_pushed` connect a session to a transport the caller
implements; `collab_take_notices` gives the status-bar messages. The core, without the engine, is
the `deckcraft-collab` crate (L2, no egui, builds for the web).

## Schema 1 in brief

`meta` `{app: "deckcraft", schema: 1, title}`; `presentation` (presentation-wide settings, one
JSON-text entry each, and `masters`, the master ids in order); `slides` (slide ids in order);
`slide:<id>` per slide: its properties as JSON-text entries (`layout`, `transition`,
`animations`, `background`…), `shapes` (its top-level shape ids, back to front), a map
`shape:<id>` per shape (groups' children included, listed in the group's `children`), `notes`
(Yjs text) and `notesBody`; a shape's properties are JSON-text entries (`xfrm`, `geom`, `kind`,
`fill`, `line`…, last writer wins each) and its text is Yjs text; `masters`, `layouts` and
`theme` hold whole JSON values (they change rarely and together); `media` holds pictures and
other files by id (immutable).

- **Ids** are 8 characters of `0-9a-z`, the base-36 form of the object's 32-bit id in DeckCraft's
  model, so references inside values (connector ends, animation targets, links to slides,
  pictures' media, a slide's layout) are the same numbers on every peer. In a session DeckCraft
  hands out new ids from a random block (a random start from 2^24 to 2^31 with 65,536 free ids
  after it, renewed after 32,768), so two people adding at once don't meet.
- **Text** is Yjs text: each paragraph's characters then `\n`; a line break is U+000B; formatting
  is Yjs attributes, one per run property (`bold: true`, `size: 24`, `font`…), with fields
  (`field`) and equations (`math`) marked the same way; the `\n` carries the paragraph's `level`,
  its properties (`p:<name>`), its default run properties (`defRPr`) and the end mark's run
  properties. Typing is character inserts, so two people typing in one text box both keep their
  text, and formatting different properties of the same words keeps both.
- **Awareness** is `{user, selection: {slide, shapes, text?: {shape | notes, anchor, head}}}`,
  `anchor` and `head` Yjs relative positions (base64), so a caret stays put while others type.

## Behaviour

- **Push channel.** Every running add-in's standard output is read all the time; what an add-in
  sends on its own (notifications, `collab/snapshot` and `host/log` requests) waits in a queue that
  the app's main loop takes each frame, never in the middle of a command. `host/…` requests
  during one of DeckCraft's requests are still answered by it (protocol 1.1). Lines may be 64 MiB;
  an add-in sending more than 1,000 messages a second for 5 s is stopped and marked failed.
  `initialize` gets `host.features` `push` and `collab` and `host.collab = {formats: ["yjs-v1"],
  app: "deckcraft", schema: 1, encryption: ["aes-256-gcm"]}`.
- **Sharing**: the room is seeded from the presentation in one transaction (origin `seed`, not
  undoable), then `collab/joined` and `collab/sync1`. **Joining** (`mode: join`): a new
  presentation opens, read-only and showing "Joining…", until a state for DeckCraft schema 1
  arrives. A room without masters (a peer that shared only slides) is shown with DeckCraft's
  default master.
- **Local edits.** Each committed command (the outermost: a plugin command's steps are one), each
  finished drag and each committed input-method composition is **one** transaction and one
  `collab/update`, computed by comparing the presentation before and after: changed properties,
  shapes and slides added, removed and moved (the longest run already in order stays), and text
  as the characters typed, deleted or formatted. Commands that change only view state (selection,
  zoom, the current slide) send nothing; the selection goes to awareness.
- **Remote changes** are applied between frames, never during a drag or a composition (they wait
  for it to end), and only the slides they touched are read again. The caret stays where it was
  in the text, wherever others typed. A message that can't be decoded or applied is dropped and
  shown in the status bar; an update that writes another app's `meta` is dropped and says "This
  room is for <app> <schema>". Nothing a peer sends can crash DeckCraft (panics in the Yjs
  library are caught too; values a reader can't take are skipped one by one).
- **Sync.** A received `sync1` is answered with `sync2` addressed to the asker (`to` = its
  `from`); when its state vector shows changes DeckCraft lacks, DeckCraft asks back with its own
  `sync1`. `sync1` also goes out when the transport reports `online` after another state. An
  update that skips some of a peer's changes waits until they arrive (and DeckCraft asks for
  them).
- **Encryption** (a `key`, 32 bytes): AES-256-GCM per protocol 2 section 4; `sync2` with
  `snapshot: true` and the answer to `collab/snapshot` use the method name `collab/snapshot`.
  After 10 messages that don't decrypt, the status bar asks "Wrong room key?".
- **`collab/snapshot`** is answered with the whole state (encrypted when keyed) and, with
  `file: true`, the presentation as `.pptx` bytes, except in an encrypted room: there DeckCraft
  answers `fileRefused` instead, so the plaintext never reaches the transport.
- **Offline.** When the transport reports `offline` or exits, editing goes on; an exited
  transport is started again after 3 s (a crash counts as a plugin failure) and the room rejoins
  with `sync1`, which merges both sides' edits.
- **Recovery.** Every 30 s the desktop app writes each room's whole state into the AutoRecover
  folder (`rooms/room-<hash>.json`); joining that room again (`mode: join`) merges it first, so
  edits made before a crash aren't lost. Leaving deletes the file.
- **Limits.** An update is at most 16 MiB before base64 (a larger local change is announced with
  `sync1` so peers fetch it; a larger remote one is refused), awareness 64 KiB and 20 a second, 16
  rooms per transport, one room per presentation.

## Tests

- `crates/collab/src/tests.rs`: two and three replicas with random concurrent edits (slides
  added, removed and moved; shapes added, removed, moved, recoloured, grouped and reordered;
  typing, deleting and formatting text in shared text boxes; notes) and updates delivered out of
  order and twice converge, and each model equals its own document; typing in one text box
  merges and undo is per user; moves of different shapes both stay; relative positions follow
  others' typing; hostile updates are refused. `DECKCRAFT_FUZZ_SEEDS` soaks it.
- `crates/engine/tests/collab_convergence.rs`: two and three sessions run random command sequences
  (slides, shapes, typing keystroke by keystroke with others' updates arriving in between,
  formatting, grouping, notes, transitions, per-user undo and redo) with updates exchanged in
  random order and duplicated; they end identical, equal to a newcomer.
  `DECKCRAFT_COLLAB_SEEDS`, `DECKCRAFT_COLLAB_STEPS`, `DECKCRAFT_COLLAB_SEED` soak and replay it.
- `crates/engine/tests/collab_conformance.rs`: replays craft-collab's DeckCraft schema-1
  transcripts (`CRAFT_COLLAB_DIR`, or a `craft-collab-deckcraft` or `craft-collab` checkout next to
  this one).
- `crates/engine/tests/collab_push.rs`: a fake transport (the test binary) relays between two
  sessions through the push channel: joining from a plugin command's result, sync, edits both
  ways, `collab/snapshot` with the `.pptx`, awareness, per-user undo, leaving.
- `crates/engine/tests/collab_loopback.rs`: two DeckCraft processes through craft-collab's
  loopback transport (`CRAFT_COLLAB_LOOPBACK` = the built plugin).
- `crates/ui-egui/src/collab_tests.rs`: the Share button, its state in a session, the people's
  circles and "+N", going to a person's selection, the popover's Leave, the Share window.
- craft-live: `crates/engine/src/live_tests.rs` (the frames byte for byte and hostile ones,
  the fragment, the save format, and clients against a fake room that behaves as craft-live's:
  seeding with a joiner held at "Joining…", one update per command and awareness both ways, the
  upload's URL, headers and bytes, `.odp` saved as a `.pptx`, failed and WebDAV saves answered
  with `saveResult`, the shell's messages, `expired` and 4401, reconnecting with backoff and
  merging offline edits, 4403, `reseed` and a lost lease going through `prepare`, viewers, Close
  and File › Close, errors before the presentation shows, hostile frames, the desktop's snapshot
  `format`); `crates/engine/tests/live_wasm.rs` (seeding, joining, an edit, the chat and an
  upload in wasm32 under `wasm-bindgen-test-runner`); `crates/ui-egui/src/collab_tests.rs`
  (Save and Close going to the page, the popover's Close). Checked end to end against the real
  craft-live server and its fake Nextcloud with the client run natively (seeding, joining,
  editing, a viewer, Save and `Action_Save` writing new versions of the file, `.pptm` kept,
  `.odp` refused, Close, reconnecting); a real browser in a real Nextcloud is still to try (list
  in [gaps.md](gaps.md)).
- Chat: `crates/engine/tests/chat.rs` (the commands and their limits, not undo steps but
  modifying, `.pptx`/`.pptm`/`.deckcraft` round trips; in sessions: sharing seeds and joining
  brings the chat, concurrent chatting by two and three people converging in one order, replies,
  edits, deletions, clearing with messages sent meanwhile, Undo never removing a message, viewers
  chatting, typing in awareness, an encrypted room carrying ciphertext); chatting in the
  convergence fuzz (`collab_convergence.rs`); craft-collab's `deckcraft-1-chat` and
  `deckcraft-1-chat-share` transcripts in `collab_conformance.rs` (with `chat`, `typing`,
  `chatDecodesTo` lines, `fails` and variables in `run` params); `crates/pptx/tests/chat.rs` (the
  part); `deckcraft_model::chat` tests (validation, the 10,000 limit); `crates/ui-egui/src/chat_tests.rs`
  (the button and its unread count, the mention flash, the pane, Enter and Shift+Enter, "Message
  deleted", the menu's Office note, Clear chat… asking first, typing in a session).

## Chat

The people editing a presentation talk in a chat beside it: craft-collab's
[document chat](https://github.com/sirmmo/craft-collab/blob/main/docs/chat.md) (protocol 2.1),
the same in GridCraft and WordCraft. The chat belongs to the presentation: it's shared live
through the room, saved in the file, and stays there until someone clears it. Outside a live
session it works too, as notes left in the file.

**Using it.** The title bar's chat button (a speech bubble, to the left of the people's circles)
opens the **Chat** pane on the right; it's there in a session, and outside one when the
presentation has a chat. View › Chat opens it too. The button counts others' messages that
arrived while the pane was closed (a red badge), and flashes when one of them mentions you
(`@Your Name`, or your first name). In the pane each message shows its author's circle (initials
on their colour), name in their colour, time (relative today: "just now", "5 min ago", "3 h ago";
else the local date and time), the text (selectable; links clickable; `@Name` highlighted when it
names someone in the session or the chat), the message it replies to quoted, and "edited" or
"Message deleted". **Reply** under a message answers it; **Edit** and **Delete** are under your own.
The box at the bottom sends with Enter (Shift+Enter for a new line; Escape stops replying or
editing). Above the messages: "Name is typing…" while others type in the chat. The **⋯** menu has
Export as text and Export as JSON (copied to the clipboard), **Clear chat…** (it asks: "Remove the
whole chat from the document for everyone?") and the note "Kept in the file; Office drops it when
it saves". While joining, the box waits for the room's document; viewers (`role: view`) chat too.

**Commands** (everything is a command; programmatic calls never ask):

| Command | Params | Does |
|---|---|---|
| `chat.list` | `{since?, limit?}` | `{messages: [{id, author: {id, name, color}, at, text?, replyTo?, edited?, deleted?}], clearedAt?, count}`, newest last: those after the message `since` names (a number: sent after that time in ms), the newest `limit` of them; `count` is the chat's size |
| `chat.send` | `{text, replyTo?}` | `{id}` (8 random characters of `0-9a-z`). Text of 1–4,000 UTF-16 units; at 10,000 messages "The chat is full: clear it to go on" |
| `chat.edit` | `{id, text}` | The author only (their id is compared), not a deleted message |
| `chat.delete` | `{id}` | The author only: "Message deleted" stays so replies still point somewhere |
| `chat.clear` | `{}` | `{removed}`: every message goes, for everyone in a session |
| `chat.export` | `{format?: text\|json}` | `{format, text}`: `[2026-10-16 14:03 UTC] Name: text` lines, or the file part's JSON |
| `chat.typing` | `{typing}` | Your awareness says `typing: true` (the pane sends it while its box has the focus and text) |
| `view.chat` | `{on?}` | Shows or hides the pane (interface) |

The author is the room's `user` in a session, else you as `collab.setUser` and the preferences
say (a stable random id, your name and colour). Chat changes aren't undo steps (Undo never
removes a message, in a session or not) but they make the presentation modified: it has to be
saved for the chat to be kept. Others' messages make it modified too.

**In the shared document**: the root array `chat` (each message a map `{id, author, at, text,
replyTo?, edited?, deleted?}`, `author` the JSON text of `{id, name, color}`) and the root map
`chatMeta` (`clearedAt`). Messages are only appended, and read in the array's order (the same on
every peer whatever the senders' clocks say); an edit or a deletion sets keys on the author's
entry; clearing removes every entry in one transaction and sets `clearedAt`. Neither root is in
the undo manager's scope. A reader skips entries that aren't maps or have a malformed `id`,
`author`, `at` or `text`, and repeated ids, and keeps the first 10,000 left. **Sharing** seeds the
room's chat from the presentation's; **joining** brings the room's chat in (the joined presentation
starts unmodified); in an encrypted room the chat travels encrypted like every update. Awareness
may carry `typing: true`.

**In the file**: `.pptx`/`.pptm`/`.potx`… hold the part `/craft/chat.json` (content type
`application/vnd.craft.chat+json`, related from the package root's `_rels/.rels` with
`urn:craft:relationships:chat`): `{"version": 1, "clearedAt"?, "messages": [...]}`, the messages
as `chat.list` gives them. An empty chat writes no part; a malformed part is ignored with a
warning, never stopping the file from opening. The native `.deckcraft` stores the same JSON as
`chat` in `presentation.json`. PowerPoint drops the part when it saves; the chat then lives on only
in copies the craft apps saved.

Core: `deckcraft_model::chat` (the model and its validation, shared by the file, the native
format and the room), `crates/collab/src/chat.rs` (the Yjs mapping), `crates/pptx/src/chat.rs`
(the part), `crates/engine/src/chat.rs` (the commands), `crates/ui-egui/src/chat_ui.rs` (the pane
and the button).

## Not yet

- Masters, layouts and the theme are shared as whole JSON values: two people editing the same
  master at once keep the later. Tables (their cells' text) and charts are whole values inside
  their shape's `kind`; SmartArt's outline is one value (`diagram`).
- The web build has no add-ins and joins only craft-live's rooms (Nextcloud); a plain
  y-websocket server or an MQTT broker isn't reachable from it yet.
- yrs garbage collection is off (GridCraft's fuzzer crashed with it and the undo manager):
  deleted content stays in the document's history.
- A text position in a table cell or in a cell being edited isn't sent to others (only the
  table's selection is).

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-16 | major | The web build is a craft-live editor (Nextcloud Office): the embed contract (fragment, hello/config, the shell's messages), y-websocket with lib0 framing and craft-live's control messages, seeding and joining, uploads in the file's format, reconnecting, Close in the title bar, Save to Nextcloud; the desktop's `collab/snapshot` takes a `format` hint |
| 2026-10-16 | minor | Document chat (craft-collab's chat.md 1.0, protocol 2.1): the `chat` and `chatMeta` roots, the file part, `chat.*` commands and `view.chat`, the Chat pane and the title bar's chat button with the unread count, mentions, typing in awareness; tests and the chat transcripts |
| 2026-10-15 | major | Created: plugin protocol 2 (2.0.1) in DeckCraft: push channel, rooms, schema 1 (texts as Yjs text with formatting attributes, ids that are the model's), encryption, per-user undo, awareness with remote selections and carets, recovery; Share and the people in the title bar |
