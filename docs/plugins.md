# Add-ins, Office add-ins and macros

> **Last reviewed:** 2026-10-15 · **Last updated:** 2026-10-15 · **Change:** minor (Office add-ins: what Microsoft's samples need, detailed in [officejs.md](officejs.md); previously minor: manifest 1.3: add-ins that require other add-ins, with version ranges; previously minor: protocol 2: the push channel and co-editing transports; previously patch: events refuse `host/sync` and the VBA project, reads too; no-op commands are no edit; previously minor: protocol 1.2: host-drawn dialogs, opening files, document events, `context.document`; previously minor: ribbon buttons that run macros, saved as the presentation's `customUI14.xml`; Record Macro, a DeckCraft extra; previously major: first version: out-of-process add-ins, Office add-ins' task panes, VBA macros through a macro add-in) · **Target:** Microsoft PowerPoint (Microsoft 365)

DeckCraft runs three kinds of extension, all as **add-ins** listed in the Add-ins window (Insert ›
Add-ins, Developer › Add-ins):

1. **Add-ins** (`plugin.json`): programs in any language that DeckCraft starts on first use and
   talks to over JSON-RPC. They add commands, ribbon groups and right-click items. The manifest
   and protocol are the craft apps' shared ones (GridCraft, WordCraft): one add-in can serve
   several apps.
2. **Office add-ins** (`manifest.xml`): web add-ins written for PowerPoint with the Office
   JavaScript API. Their pages show in a task pane, running **DeckCraft's own Office.js**
   (`crates/office-js`); Microsoft's `office.js` is never read or shipped, and never runs in the
   pane. [officejs.md](officejs.md) has the details.
3. **Macros**: an add-in that declares `"macros": {"language": "VBA"}` (craft-vba,
   [sirmmo/craft-vba](https://github.com/sirmmo/craft-vba)) runs the presentation's VBA. The
   Developer tab appears once one is loaded.

An add-in is a program that runs with the user's permissions; the Add-ins window says so before
installing one from GitHub. The web app can't run add-ins.

## Installing

- **From GitHub:** Add-ins › Install from GitHub (`plugins.install {"url": "owner/repo"}`): the
  release zip for this platform, else the repository's source, goes into `<settings>/plugins/` and
  is loaded. `<settings>` is `~/Library/Application Support/DeckCraft` on macOS, `%APPDATA%\DeckCraft`
  on Windows, `~/.config/deckcraft` elsewhere.
- **From a folder:** Add-ins › Load from a folder, or `plugins.load {"path": …}` (a folder holding
  `plugin.json` or an Office add-in's `manifest.xml`).
- At start-up DeckCraft loads every add-in in `DECKCRAFT_PLUGIN_PATH` and `<settings>/plugins/`.
- **Activate / Deactivate** (`plugins.setEnabled`) is remembered in the preferences; **Unload**
  lasts until the next start.

| Command | Params | Does |
|---|---|---|
| `plugins.list` | `{}` | The loaded add-ins with their status, `state` (`loaded`, `running`, `failed`, `blocked`, `disabled`), `blockedBy`, `requires`, `requiredBy`, commands and ribbon |
| `plugins.load` | `{path, force?}` | Loads the add-in in a folder (or a manifest file). An add-in with the same id is replaced; with a version outside the range a turned-on add-in requires, only with `force` |
| `plugins.install` | `{url (or github), dir?, version?, withRequirements?, force?}` | Installs the add-in in a GitHub repository into `dir` (default: `<settings>/plugins/`) and loads it. The result adds `release` and `missing`; see [Requirements](#requirements-manifest-13) |
| `plugins.installMissing` | `{id, dir?, force?}` | Installs the missing required add-ins of `id` that name `github` |
| `plugins.requirements` | `{id}` | `{id, satisfied, blockedBy, tree}`: what `id` needs and what's installed |
| `plugins.unload` | `{id}` | Unloads an add-in and stops its process (until the next start); `dependentsBlocked` lists the add-ins that needed it |
| `plugins.setEnabled` | `{id, enabled}` | Turns an add-in off or back on, and remembers it in the preferences; turning off reports `dependentsBlocked` |
| `plugins.openHomepage` | `{id}` | Opens the add-in's web page in the browser |
| `plugins.run` | `{plugin, command, params?}` | Runs an add-in command (one undoable step) |

## The manifest (`plugin.json`)

Shared with the other craft apps; see GridCraft's `docs/plugins.md` for every field. What DeckCraft
reads:

| Field | DeckCraft |
|---|---|
| `id`, `name`, `version`, `description`, `author`, `homepage` | Shown in the Add-ins window |
| `apps` | Must name `deckcraft` when present (`["gridcraft", "deckcraft"]`) |
| `run` | `{command, args}`, started in the add-in's folder |
| `commands` | `[{id, label, description, params}]`, run by `plugins.run` |
| `ui` / `uiFor.deckcraft` | Ribbon groups (`tab`: Home, Insert, Draw, Design, Transitions, Animations, Slide Show, Record, Review, View, or a new tab) and right-click items (`menu`: `shape`, `slide`, `text`) |
| `macros` | `{language}`: the add-in runs macros (protocol 1.1) |
| `events` | `["documentOpened", "documentSaved", "documentClosed"]`: the document events it hears (protocol 1.2); other names are kept and ignored |
| `collab` | `{transport, label}`: the add-in is a co-editing transport (protocol 2); only such an add-in may join rooms, and the Share window lists it by `label` |
| `functions` | GridCraft's spreadsheet functions: accepted, not used |
| `requires.apps` | Manifest 1.3. Per app id, the app versions the add-in works with (`{"deckcraft": ">=0.7.0"}`); an app missing from the map isn't constrained. See [Requirements](#requirements-manifest-13) |
| `requires.protocol` | The plugin protocol versions it needs (`">=1.2"`), checked against `host.protocol` (`"2.0"`) |
| `requires.features` | Names from `host.features` (`dialogs`, `open`, `events`, `document`, `push`, `collab`) that must all be present |
| `requires.addins[]` | Other add-ins it needs: `id`, `version` (a range; default `*`), `github` (`owner/repo` to install it from), `optional` (default false), `why` (shown to the person, at most 200 characters). At most 32 |

## The protocol

JSON-RPC 2.0, one message per line on the add-in's standard input and output.

- `initialize {protocolVersion: 1, host: {id: "deckcraft", name, version, protocol, features, language, collab}, plugin: {id, dir}, addins}`.
  `features` is `["dialogs", "open", "events", "document", "push", "collab"]` (protocols 1.2 and
  2), `language` the UI's BCP 47 tag (`en`: DeckCraft's interface is English), `collab` what
  co-editing DeckCraft speaks (protocol 2, below). Since manifest 1.3, `protocol` is the protocol
  version as text (`"2.0"`, what `requires.protocol` is checked against), and `addins` lists every
  installed add-in as `{id, version, enabled}` (`enabled`: turned on, not blocked and not failed),
  so an add-in can adapt to the optional ones it found.
  The environment also has `CRAFT_HOST=deckcraft`, `CRAFT_PLUGIN_DIR` (also as `DECKCRAFT_PLUGIN_DIR`) and `DECKCRAFT_VERSION`.
- `commands/run {command, params, context}` → `{commands: [{command, params}], message?, result?}`.
  `context` is `{document, presentation, slide (0-based), slideId, slides, titles: [slide titles], selection: [shape ids], text}`.
  The answered commands are DeckCraft's own (`slide.new`, `insert.textBox`, `shape.fill`…; not
  `plugins.*` or `developer.*`) and run as **one undoable step**; if one fails, nothing changes.

**Protocol 1.1: asking the host.** While a request of DeckCraft's is waiting, the add-in may send
requests of its own; DeckCraft answers them, then waits again:

| Request | Answer |
|---|---|
| `host/sync {actions}` | `{results}`: the same actions as an Office add-in's `context.sync()` (below), on the open presentation; an error's `data` is `{code, action}` |
| `host/vbaProject/get {}` | `{base64}`: the presentation's `vbaProject.bin`, or `null` |
| `host/vbaProject/set {base64}` | `{}`: replaces it (at most 64 MiB; signatures over the old one are dropped) |
| `host/message {text}` | `{result: 1}`: shown in the status bar |
| `host/log {text}` | `{}`: written to the log |

What the add-in changes this way is part of the running command's one undo step.

**Protocol 1.2: dialogs, opening files, document events.** The craft apps' shared contract
(craft-nextcloud's `docs/host-protocol-1.2.md`); DeckCraft does it as follows.

- **`context.document`** `{path, name, dirty}` in `commands/run`'s and `macros/run`'s `context`
  (`macros/run` gets `context: {document}`): `path` the file it was opened from or last saved to,
  `null` for a new presentation (then `name` is its title, `Presentation1`).
- **`open {path, label?, readOnly?}`** in a command's result: after the step, if it succeeded,
  DeckCraft opens the file as File › Open does (in the background, in a new window, added to
  Recent). The path must be absolute, an existing regular file, and `.deckcraft`, `.pptx`, `.potx`,
  `.ppsx`, `.pptm`, `.potm`, `.ppsm`, `.txt` or `.md`; otherwise the command fails and nothing
  changes. `label` (at most 200 characters) shows as the Location in File › Info (the path is its
  tooltip) until the presentation is saved elsewhere; `file.open` and `file.adopt` take it as
  `label`, `file.info` returns it. `readOnly` is ignored: DeckCraft has no read-only mode.
- **`dialog`** in a command's result: DeckCraft draws a modal in its own widgets and theme:
  `list` (icon, label and detail per item, a filter box above 20 items, `multi` for several),
  `form` (`text`, `secret`, `number`, `checkbox`, `choice` fields) or `message`; OK, a double-click
  on an item and up to 4 extra buttons run the add-in's command the same way as a ribbon button,
  with `id`/`ids` for a list's selection and `fields` for a form, so dialogs chain. Cancel, Escape
  and a click outside close it. If the command fails, the dialog comes back with the reason. Text
  over a limit is cut; a malformed dialog, or one whose buttons run commands the add-in doesn't
  declare, fails the command.
- A result may combine fields (`open` + `message`, `dialog` + `result` + `message` + `commands`):
  each is done. A **programmatic** `plugins.run` (CLI, MCP, control channel) opens and shows
  nothing: it returns `open` and `dialog` in its result for the caller.
- **Events**: `events/documentOpened {path, name}` (File › Open, Recent, a presentation dropped
  from the file manager, an add-in's `open`), `events/documentSaved {path, name, format}` (Save,
  Save As, Save as Template; not AutoRecover) and `events/documentClosed {path, name}` (File ›
  Close, and quitting, for each open presentation that has a file), to each loaded, active add-in
  that declares it. They go out after the command, from a background thread, in order: a slow or
  failing add-in never holds up or fails the save, open or close, and an event is no undo step.
  Opened and saved time out after 30 s, closed after 5 s; quitting waits at most 5 s. The answer's
  `message` shows in the status bar, an error as a notice. During an event `host/log` and
  `host/message` work; `host/sync` and `host/vbaProject/get` and `/set` are refused, reads too
  (`AccessDenied`), as in GridCraft and WordCraft: an event is a notification, not an edit.
  `deckcraft-cli run` waits for events before it exits.
- A command whose answer changes nothing (a dialog step, even one that read the presentation
  through `host/sync` or wrote back what was there) makes no undo step and leaves a clean
  presentation clean, so a dialog chain that ends by opening a file doesn't ask to save first.

**Macros** (`"macros"` in the manifest): `macros/list`, `macros/run {name, control?: {id, tag}}`,
`macros/modules`, `macros/saveModule {name, source}`, `macros/deleteModule {name}`,
`macros/record {steps, name?}` → `{source}`, as in GridCraft. Commands: `developer.macros`,
`developer.runMacro {name}`, `developer.modules`, `developer.saveModule`, `developer.deleteModule`,
`developer.recordMacro {stop?, module?, name?}`, `developer.recordToCode {steps, name?}`,
`developer.enableMacros`, `developer.macroSecurity {level}`.

**Record Macro** (Developer › Code; a **DeckCraft extra**: PowerPoint dropped its macro recorder
in PowerPoint 2007, DeckCraft has one). Record Macro notes where the command journal stands; Stop
Recording sends what you did since to the add-in as `macros/record {steps: [{command, params}],
name?}`, with DeckCraft's own command ids and their params exactly as journaled (slide indexes
0-based, shape and slide ids the engine's numbers, colours `#RRGGBB`), and puts the `source` it
answers at the end of Module1, which opens in the editor. The Developer tab's own commands and what
add-ins do (`developer.*`, `plugins.*`, `office.*`) aren't recorded; Delete while editing text is
journaled as the text delete it is (`text.delete {dir: forward}`), not as a shape delete. craft-vba
writes PowerPoint VBA from the steps (0.1.5 and later); steps it can't express become comments.
`developer.recordToCode {steps}` turns steps given directly into code without a recording.

**Ribbon buttons** (Developer › Ribbon Buttons, or Add to Ribbon… in the Macros window): buttons
that run the presentation's macros, on a built-in tab or one of the presentation's own. They are
saved with the presentation as the Office ribbon part (`customUI/customUI14.xml`, related from the
package root), so PowerPoint shows them too, and a presentation made in PowerPoint brings its
buttons along (`customUI14.xml` or the Office 2007 `customUI.xml`). A ribbon that holds more than
tabs, groups and buttons (pictures of its own, menus, `onLoad`) is kept unchanged, pictures
included: its buttons show and run but can't be edited. A button runs its macro as PowerPoint does,
with the button: `macros/run` gets `control: {id, tag}`, which a VBA add-in passes as the
`IRibbonControl` argument (`Sub Go(control As IRibbonControl)`); a macro without parameters runs as
it is. The buttons show whether or not a macro add-in is loaded (without one, a click says so).
Commands: `developer.ribbon` → `{tabs: [{tab, id, builtin, label, groups: [{id, label, buttons:
[{id, label, macro, image, large, screentip, tag}]}]}], editable}`,
`developer.addRibbonButton {macro, label?, tab?: "Macros", group?: "Macros", image?, large?,
screentip?, tag?}` → `{id}`, `developer.editRibbonButton {id, …}`,
`developer.removeRibbonButton {id}`, `developer.pressRibbonButton {id}` → `{ran}`. Built-in tabs
by name: Home, Insert, Design, Transitions, Animations, Slide Show, Review, View, Developer
(`TabHome`…); any other name is a tab of the presentation's own.

**Macro security** (Developer › Macro Security): `prompt`, the default, runs a presentation's macros
once the user clicked **Enable Content** in the bar a presentation with macros opens with;
`disable` runs none; `enable` runs all. Agents, the CLI and MCP are bound by the same setting.

**Saving:** macros live in `.pptm` (`.potm`, `.ppsm`). A presentation with macros saved under a
`.pptx`, `.potx` or `.ppsx` name is saved as `.pptm`, `.potm` or `.ppsm` instead, and DeckCraft says
so; the asked file isn't written.

## Protocol 2: the push channel and co-editing

[Craft plugin protocol 2](https://github.com/sirmmo/craft-collab/blob/main/docs/protocol-2.md)
(2.0.1, shared with GridCraft and WordCraft) adds to 1, 1.1 and 1.2 and changes nothing in them:

- **The push channel.** DeckCraft reads every running add-in's output all the time. An add-in may
  send notifications whenever it likes, and requests to the *anytime* methods (`collab/snapshot`,
  `host/log`); DeckCraft answers them in its main loop, in any order, and may send the add-in
  notifications interleaved with its own requests, so an add-in must match answers by `id`. The
  protocol-1 rules hold for DeckCraft's requests; `host/…` requests during one of them are still
  answered by it. A line may be 64 MiB; more than 1,000 messages a second for 5 s and the add-in is
  stopped. `initialize` gets `host.features` `"push"` and `"collab"` and
  `host.collab = {formats: ["yjs-v1"], app: "deckcraft", schema: 1, encryption: ["aes-256-gcm"]}`.
- **Transports.** An add-in whose manifest has `"collab": {"transport": "mqtt", "label": "Live
  (MQTT)"}` may join rooms; it runs while it's in one. Its command answers `{"collab": {"join":
  {room, mode, user, role?, key?, path?, title?}}}` (alongside `message`, `dialog`, `open`, and
  `result.link`, an invitation); `plugins.run` returns it, and when DeckCraft's own interface ran
  the command (a button, a menu item, an add-in's dialog, the Share window) it runs `collab.join`
  with it and keeps the link for **Copy invitation**. Then the `collab/*` messages flow:
  `collab/joined`, `sync1`/`sync2`/`update`/`awareness` (Yjs, base64, encrypted when keyed),
  `collab/status`, `collab/leave`, `collab/snapshot` (with `file: true`, the presentation as
  `.pptx` bytes, refused in an encrypted room).
- The Share window runs a transport's own `share` and `join` commands when it has them, with
  `{name, color, encrypt}`.

DeckCraft's shared document (schema 1), commands and behaviour: [collab.md](collab.md). A test
transport that relays between two rooms is `crates/engine/tests/collab_push.rs`;
[craft-collab](https://github.com/sirmmo/craft-collab) has MQTT and loopback transports.

## Requirements (manifest 1.3)

An add-in can say which other add-ins it needs, in which versions, and which app versions and
protocol features. DeckCraft checks this before starting it, says what's missing, and offers to
install it. The contract is shared with GridCraft and WordCraft:
[Craft plugin manifests 1.3](https://github.com/sirmmo/craft-collab/blob/main/docs/plugin-requirements.md).

```json
{
  "id": "org.example.reports",
  "version": "1.4.0",
  "requires": {
    "apps": {"deckcraft": ">=0.7.0", "wordcraft": ">=0.7.0"},
    "protocol": ">=1.2",
    "features": ["dialogs", "events"],
    "addins": [
      {"id": "org.craft.vba", "version": "^0.1.4", "github": "sirmmo/craft-vba", "why": "Runs the report macros"},
      {"id": "org.craft.mqtt", "version": ">=0.1, <0.3", "github": "sirmmo/craft-collab", "optional": true, "why": "Live editing"}
    ]
  }
}
```

**Versions** are `MAJOR.MINOR.PATCH`; missing parts are 0, a pre-release (`1.0.0-beta.1`) sorts
before its release, `+build` is ignored, and an Office add-in's `1.0.0.0` compares on its first
three parts. **Ranges** are Cargo's: comma-separated comparators that must all hold. `^1.2.3` (also
a bare `1.2.3`) is `>=1.2.3, <2.0.0`, and for 0.x `^0.1.4` is `>=0.1.4, <0.2.0`; `~1.2.3` is
`>=1.2.3, <1.3.0`; `>=`, `>`, `<=`, `<`, `=` take a full or partial version (`>=0.1` is `>=0.1.0`;
as in Cargo, `=1.2` and `<=1.2` mean `1.2.*`, `>1.2` means `>=1.3.0`); `1.*` and `1.2.*` are
wildcards; `*` or nothing is any version, pre-releases included. Otherwise a pre-release only
matches a comparator that names a pre-release of the same version (`>=1.0.0-rc.1`). For the
protocol write `>=1.2`: Cargo's `^1.2` leaves out 2.0. Ranges are at most 128 characters.

**Blocked.** Whenever the add-ins change (start-up, install, `plugins.load`, turning one on or
off, unloading, one failing for good), DeckCraft resolves them all together. An add-in is
**blocked**, with its reasons in `blockedBy` and in the Add-ins window, when:

| `kind` | When | Message |
|---|---|---|
| `app` | `requires.apps.deckcraft` doesn't match this version | Needs DeckCraft 0.8 or later (this is 0.7.0) |
| `protocol` | `requires.protocol` doesn't match `host.protocol` | Needs plugin protocol 3.0 or later (this DeckCraft has 2.0) |
| `feature` | A name in `requires.features` isn't in `host.features`; an Office add-in's requirement set or method isn't supported | Needs `x`, which this DeckCraft doesn't have |
| `addin` | A required (not optional) add-in isn't installed, is turned off, failed, is blocked itself, or its version is out of range | Needs craft-vba ^0.1.4 (installed: 0.1.3) |
| `cycle` | Required add-ins need each other | Requirement cycle: Reports → craft-vba → Reports |
| `depth` | The chain of required add-ins below it is more than 8 deep | Its required add-ins go 9 deep (at most 8) |
| `range` | A range in its manifest doesn't parse | Its manifest's requirement for craft-vba (`soon`) isn't a version range |

A blocked add-in isn't started; its commands, ribbon groups, menu items, events, macros, task
panes and co-editing transport are absent, as for an add-in that's turned off. Installing what it
needs unblocks it at once, without restarting. Optional add-ins never block. Required add-ins
start before the add-ins that need them.

**Turning off or unloading** an add-in others need asks first in the Add-ins window ("Reports
needs craft-vba. Turn both off?"). Programmatically it just happens: the add-ins that needed it
are blocked, and the result's `dependentsBlocked` lists them. **Installing or updating** an add-in
to a version a turned-on add-in's range doesn't accept asks first ("Reports needs craft-vba
^0.1.4; 0.2.0 doesn't match. Install craft-vba 0.2.0 anyway?"); programmatically
(`plugins.install`, `plugins.load`) it fails unless `force: true`, and then that add-in is blocked.

**Installing what's missing.** After installing an add-in from GitHub, DeckCraft offers its
missing or out-of-range required add-ins that name `github` ("Reports needs craft-vba ^0.1.4 (Runs
the report macros). Install it too?"), ticked; optional ones are offered unticked. For each it
takes **the newest release whose tag is a version in range** (`v0.1.5` or `0.1.5`; drafts are
skipped, and so are pre-releases unless the range names one; one page of the newest 100 releases
is read), not just the latest, and says which. What they need in turn is offered next. A blocked
add-in whose missing add-ins name `github` shows **Install Missing** in the Add-ins window.

| Command | Does |
|---|---|
| `plugins.install {url, withRequirements?, force?, version?}` | `withRequirements` (default false here; the interface always asks) also installs the missing required add-ins that name `github`, with `installed` and `failed` in the result. The result always lists `missing` (`[{id, name, version, github?, optional, why, installed?}]`) for the caller to install. `version`, a range, picks the newest release in it |
| `plugins.installMissing {id, force?}` | Installs `id`'s missing required add-ins that name `github`, then theirs (at most 8 deep): `{installed: [{id, name, version, release}], failed: [{id, error}], missing, satisfied}` |
| `plugins.requirements {id}` | `{id, satisfied, blockedBy, tree: [{id, name, need, have, state, optional, why, github, children}]}`; `state` is `missing` for an add-in that isn't installed |
| `plugins.list` | Per add-in: `state` (`blocked` among `loaded`, `running`, `failed`, `disabled`), `blockedBy: [{id?, kind, need, have?, message}]`, `requires` (the manifest's), `requiredBy` (the ids of the add-ins that need it) |

Office add-ins' `<Requirements>` are checked the same way (below).

## Office add-ins

An Office add-in's `manifest.xml` (with `<Host Name="Presentation"/>` and `https://` pages) becomes
the add-in `office.<its Id>`: its default task pane is the command `taskpane`, and each add-in
command of `<VersionOverrides>` (buttons on Office tabs — `TabHome`, `TabInsert`, `TabDesign`,
`TabTransitions`, `TabAnimations`, `TabSlideShow`, `TabReview`, `TabView`, `TabDeveloper` — or
custom tabs, menus, the text right-click menu `ContextMenuText`) is a command of its own. The
manifest's `<Permissions>` bound what the page may do (`Restricted`, `ReadDocument`,
`ReadWriteDocument`).

The task pane is the system web view (WKWebView on macOS, WebView2 on Windows) laid over a
right-hand panel (`apps/deckcraft/src/office_web.rs`). Before the page's own scripts run, it gets
the bootstrap (`crates/office-js/bootstrap.js`, the one hand-written script, copied from GridCraft's)
and DeckCraft's Office.js built to wasm (`cargo xtask office-js`; the macOS package carries it in
`Resources/office-js`, or set `DECKCRAFT_OFFICE_JS`). Each `context.sync()` becomes one
`office.sync` command: one undo step, all or nothing. **[officejs.md](officejs.md)** is the full
reference: the API covered, the bridge, function commands (ExecuteFunction), how Microsoft's
`office.js` is kept from running, and the limits.

**Objects** (PowerPoint JavaScript API, PowerPointApi 1.1–1.4 subset; tags, `insertSlidesFromBase64`
and `set({...})` too, see officejs.md):

| Object | Properties | Methods |
|---|---|---|
| `presentation` | `title` | `slides`, `slideMasters`, `getSelectedSlides()`, `getSelectedShapes()`, `setSelectedSlides(ids)` |
| `SlideCollection` | `count`, `items` | `getItem(id)`, `getItemAt(i)`, `add({layoutId?, index?})` |
| `Slide` | `id`, `index` | `shapes`, `layout`, `slideMaster`, `delete()`, `setSelectedShapes(ids)` |
| `SlideMasterCollection` / `SlideMaster` / `SlideLayoutCollection` / `SlideLayout` | `id`, `name`, `count`, `items` | `getItem`, `getItemAt`, `layouts` |
| `ShapeCollection` | `count`, `items` | `getItem(id)`, `getItemAt(i)`, `addTextBox(text, box)`, `addGeometricShape(type, box)`, `addLine(type, box)` |
| `Shape` | `id`, `name`, `type`, `left`, `top`, `width`, `height` | `textFrame`, `fill`, `lineFormat`, `delete()` |
| `TextFrame` | `hasText`, `wordWrap`, `autoSizeSetting` | `textRange` |
| `TextRange` | `text`, `start`, `length` | `font`, `getSubstring(start, length)` |
| `ShapeFont` | `bold`, `italic`, `underline`, `size`, `name`, `color` (`null` when mixed) | |
| `ShapeFill` | `type`, `foregroundColor`, `transparency` | `setSolidColor(color)`, `clear()` |
| `ShapeLineFormat` | `color`, `weight`, `visible`, `dashStyle` | |

The common API: `Office.onReady`, `Office.initialize`, `Office.actions.associate`,
`Office.context.document.getSelectedDataAsync`/`setSelectedDataAsync` (text, slide range, image),
`goToByIdAsync`, `addHandlerAsync` (`DocumentSelectionChanged`), `settings` (kept per open
presentation until it closes), `requirements.isSetSupported`. Colours are `#RRGGBB` or CSS colour
names (`"yellow"`).
A shape added earlier in the same sync is named by that action's result (`getItem({"result": i})`).

Requirement sets reported as supported: **PowerPointApi 1.4** (a subset, officejs.md), and
AddinCommands, DocumentEvents, ImageCoercion, Settings, Selection and TextCoercion 1.1; methods:
`Document.getSelectedDataAsync`, `Document.setSelectedDataAsync`, `Document.goToByIdAsync`,
`Settings.get`, `set`, `remove`, `saveAsync`. **Requirements.** The manifest's `<Requirements>`
are checked when the add-in loads, against that one table (`deckcraft_plugin::office::API_SETS`
and `API_METHODS`, which `isSetSupported` answers from too; a test keeps them equal). `<Sets>`
lists `<Set Name="PowerPointApi" MinVersion="1.3"/>`; a set without `MinVersion` takes the
`<Sets DefaultMinVersion="…">` (else 1.1). `<Methods>` lists
`<Method Name="Document.getSelectedDataAsync"/>`. An unmet one makes the add-in **blocked**
([Requirements](#requirements-manifest-13)): it stays listed in the Add-ins window with the reason
("Needs PowerPointApi 1.9; DeckCraft supports up to 1.4", "Needs WordApi 1.1, which DeckCraft
doesn't support"), and its buttons, menu items and task panes are absent, instead of failing at its
first call. `plugins.list` reports it as `state: "blocked"` with `blockedBy` (kind `feature`).

**Not yet:** events other than `DocumentSelectionChanged`, dialogs (`displayDialogAsync`), the
shared runtime, keyboard shortcuts, hyperlinks, settings saved in the file, the unified (JSON)
manifest, and a web view on Linux.

## VBA (craft-vba)

craft-vba picks PowerPoint's object model when `host.id` is `deckcraft`: `ActivePresentation`,
`Slides` (`Count`, `Item`, `Add(index, ppLayout…)`, `For Each`), `Slide` (`Shapes`, `SlideIndex`,
`SlideID`, `Delete`), `Shapes` (`AddTextbox`, `AddShape` with the `msoShape…` constants, `AddLine`),
`Shape` (`Name`, `Left`, `Top`, `Width`, `Height`, `TextFrame.TextRange.Text`, `Font.Bold/Italic/
Size/Name/Color.RGB`, `Fill.ForeColor.RGB`, `Line…`, `Delete`), `ActiveWindow.Selection`, `MsgBox`.
A macro is one undo step; a run-time error takes everything it did back
(`Run-time error '9': Subscript out of range`). See craft-vba's README for the full list.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-15 | minor | Office add-ins: [officejs.md](officejs.md) created; Microsoft's `office.js` never runs, function commands (ExecuteFunction) end to end, colour names, tags and `insertSlidesFromBase64`, image and slide-range coercions, `goToByIdAsync`, `DocumentSelectionChanged`; AddinCommands, DocumentEvents and ImageCoercion 1.1 reported |
| 2026-10-15 | minor | Manifest 1.3: `requires` (apps, protocol, features, other add-ins with Cargo-style version ranges), blocked add-ins (`state`, `blockedBy`, `requiredBy`), start order, `host.protocol` and `addins` in `initialize`, `plugins.requirements`, `plugins.installMissing`, `plugins.install` `github`/`withRequirements`/`force`/`version` (newest release in range), `plugins.load` `force`, Install Missing, asking before turning off a needed add-in or updating out of range; Office add-ins' requirement sets and methods (PowerPointApi); a command table |
| 2026-10-15 | minor | Protocol 2: the push channel (add-ins speak any time; `host.features` `push`, `collab`; `host.collab`), manifest `collab` (transports), `collab.join` in command results; co-editing in [collab.md](collab.md) |
| 2026-10-15 | patch | Aligned with GridCraft and WordCraft: during document events `host/sync` and `host/vbaProject/*` are refused, reads too; a command that changes nothing makes no undo step |
| 2026-10-15 | minor | Protocol 1.2: `host.features` and `host.language`, `context.document`, command results `open` and `dialog` (host-drawn list, form and message dialogs that chain), manifest `events` and `events/documentOpened`, `documentSaved`, `documentClosed` |
| 2026-10-15 | minor | Ribbon buttons that run macros (`customUI14.xml`, `macros/run` gets `control`); Record Macro and `developer.recordToCode` (`macros/record`), a DeckCraft extra |
| 2026-10-14 | major | First version: add-ins (shared manifest and protocol 1 and 1.1), Office add-ins' task panes with DeckCraft's Office.js, VBA macros through craft-vba, the Developer tab |
