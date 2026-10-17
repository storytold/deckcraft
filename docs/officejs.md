# Office add-ins (Office.js)

> **Last reviewed:** 2026-10-15 · **Last updated:** 2026-10-15 · **Change:** major (first version: what DeckCraft's Office.js supports, the `office.sync` bridge, keeping Microsoft's `office.js` from running, function commands, colour names, tags, `insertSlidesFromBase64`, image and slide-range coercions, `goToByIdAsync`, events, limits) · **Target:** Microsoft PowerPoint (Microsoft 365)

DeckCraft runs **Office add-ins**, the web add-ins built with the Office JavaScript API
(`Office.js`), next to its own add-ins ([plugins.md](plugins.md)). An Office add-in is a web app
on its vendor's server, described by an XML manifest. DeckCraft reads the manifest into an add-in:
its ribbon buttons and right-click items are ordinary add-in items, listed in the Add-ins window,
and its commands open its **task pane** (or call a function of its function file) instead of
starting a program.

The task pane shows the add-in's page in the system web view (WKWebView on macOS, WebView2 on
Windows). Before the page's own scripts run, DeckCraft gives it **DeckCraft's Office.js**
(`crates/office-js`), an implementation of the public API written from Microsoft's published
reference (clean room). Microsoft's `office.js` is never read or shipped, and never runs in the
pane: add-in pages reference it (every one of Microsoft's samples does), and DeckCraft answers
those references with an empty script ([below](#microsofts-officejs)). Each `context.sync()` of the
page becomes one `office.sync` command against the presentation.

```
 add-in page (vendor's https)              DeckCraft
 ┌────────────────────────────────┐      ┌──────────────────────────────────────────┐
 │ the add-in's own scripts       │      │ office_pane (ui-egui): the pane, pump    │
 │   PowerPoint.run(ctx => …)     │ JSON │ office.sync (engine): paths → objects,   │
 │   ctx.sync()                   │ ───► │   DeckCraft commands, one undo step,     │
 │ DeckCraft's Office.js (wasm)   │ ◄─── │   the manifest's permissions             │
 └────────────────────────────────┘      └──────────────────────────────────────────┘
   wry web view over the task pane (apps/deckcraft/src/office_web.rs)
```

## Installing an Office add-in

- Load its `manifest.xml` with Load from folder in the Add-ins window, or
  `plugins.load {"path": ".../manifest.xml"}`.
- The manifest must be for PowerPoint (`<Host Name="Presentation"/>`), and its pages `https://`.
- It becomes the add-in `office.<its Id>`. Its default task pane (`<DefaultSettings><SourceLocation>`)
  is the command `taskpane`; each add-in command (`<VersionOverrides>`: buttons on Office tabs or
  custom tabs, menus, the text right-click menu) is a command of its own.

| Manifest | DeckCraft |
|---|---|
| `<OfficeTab id="TabHome">` … `TabInsert`, `TabDesign`, `TabTransitions`, `TabAnimations`, `TabSlideShow`, `TabReview`, `TabView`, `TabDeveloper` | That built-in tab; other ids → an **Add-ins** tab |
| `<CustomTab>` | A tab of its own, named by its `<Label>` |
| `<Control xsi:type="Button">`, the items of `xsi:type="Menu"` | Buttons (a menu's items are its group's buttons) |
| `<Action xsi:type="ShowTaskpane">` | Opens the page in the task pane (`TaskpaneId`s that match share a pane) |
| `<Action xsi:type="ExecuteFunction">` | Calls the function in the function file's page, with no pane ([function commands](#function-commands-executefunction)) |
| `<OfficeMenu id="ContextMenuText">` | Items in the text right-click menu |
| `<Permissions>` | Restricted: settings only; ReadDocument / ReadAllDocument: read; WriteDocument: write; ReadWriteDocument: both |

Icons from the manifest aren't shown (the buttons get DeckCraft's script icon). The unified JSON
manifest isn't read yet. **Requirements** (`<Requirements>`) are checked when the add-in loads, as
[plugins.md](plugins.md#office-add-ins) describes: an add-in that needs more than the table below
is blocked, with the reason.

## What the API covers

Requirement sets reported as supported (`isSetSupported`, and the manifest check):

| Set | Version | How much of it |
|---|---|---|
| PowerPointApi | 1.4 | **A subset**: the objects in the table below, which cover most of 1.1–1.4 plus the selection members of later sets (`getSelectedSlides`, `getSelectedShapes`, `getSelectedTextRange`, `setSelectedSlides`, `setSelectedShapes`, `TextRange.setSelected`). Missing: `PowerPoint.createPresentation` (1.1, `NotImplemented`), hyperlinks, tables, pictures through `shapes`, placeholders' formats. 1.5 isn't reported because not all of it is there |
| AddinCommands | 1.1 | All: ShowTaskpane and ExecuteFunction commands |
| DocumentEvents | 1.1 | `addHandlerAsync`/`removeHandlerAsync` with `DocumentSelectionChanged` |
| ImageCoercion | 1.1 | `setSelectedDataAsync(base64, {coercionType: Image})` |
| Settings, Selection, TextCoercion | 1.1 | All |

Methods (`<Requirements><Methods>`): `Document.getSelectedDataAsync`, `setSelectedDataAsync`,
`goToByIdAsync`, `Settings.get`, `set`, `remove`, `saveAsync`. Other sets (DialogApi,
SharedRuntime, RibbonApi, IdentityAPI, PowerPointApi 1.5 and later…) answer false. The table is
one, `deckcraft_plugin::office::API_SETS`, and a test keeps the library's copy equal to it.

| Object | Properties and methods |
|---|---|
| `Office` | `onReady` (waits for the library), `initialize` (called once the library is ready, also when assigned later), `actions.associate`, `context.host`, `context.platform`, `context.diagnostics`, `context.requirements.isSetSupported`, `context.document` (below); the enumerations `AsyncResultStatus`, `CoercionType`, `EventType`, `HostType`, `PlatformType`, `Index`, `GoToType`, `BindingType`, `StartupBehavior`, `VisibilityMode`, `InitializationReason`, `ValueFormat`, `FilterType`, `SelectionMode`, `DocumentMode`, `ActiveView`, `FileType`, `Table` |
| `Office.context.document` | `getSelectedDataAsync` (`Text`; `SlideRange`: `{slides: [{id, title, index}]}`, `index` from 1), `setSelectedDataAsync` (`Text`: into the text being edited, else a new text box; `Image`: base64 picture on the current slide, `imageLeft`/`imageTop`/`imageWidth`/`imageHeight` in points, one size keeps the aspect), `goToByIdAsync(id, GoToType.Slide | GoToType.Index)` (`Index.First/Last/Next/Previous` or a slide number from 1), `addHandlerAsync`/`removeHandlerAsync` (`DocumentSelectionChanged`), `settings` (`get`, `set`, `remove`, `saveAsync`) |
| `PowerPoint.run`, `new PowerPoint.RequestContext()` | `presentation`, `sync()` (one undo step; a failure takes the whole batch back), `trackedObjects` |
| `Presentation` | `title`, `slides`, `slideMasters`, `tags`, `getSelectedSlides()`, `getSelectedShapes()`, `getSelectedTextRange()`, `setSelectedSlides(ids)`, `insertSlidesFromBase64(base64, {formatting, sourceSlideIds, targetSlideId})` |
| `SlideCollection` | `getCount()`, `getItem(id)`, `getItemAt(i)`, `add({layoutId?, index?})`, `items` |
| `Slide` | `id`, `index`, `shapes`, `layout`, `slideMaster`, `tags`, `delete()`, `setSelectedShapes(ids)` |
| `SlideMasterCollection` / `SlideMaster` / `SlideLayoutCollection` / `SlideLayout` | `id`, `name`, `layouts`, `shapes`, `getItem`, `getItemAt`, `items` |
| `ShapeCollection` | `getCount()`, `getItem(id)`, `getItemAt(i)`, `addTextBox(text, box)`, `addGeometricShape(type, box)`, `addLine(type, box)`, `items` |
| `Shape` | `id`, `name`, `type`, `left`, `top`, `width`, `height`, `textFrame`, `fill`, `lineFormat`, `tags`, `delete()` |
| `TextFrame` | `hasText`, `wordWrap`, `autoSizeSetting`, `textRange`, `deleteText()`, `getParentShape()` |
| `TextRange` | `text`, `start`, `length`, `font`, `paragraphFormat`, `getSubstring(start, length)`, `setSelected()` |
| `ShapeFont` | `bold`, `italic`, `underline`, `size`, `name`, `color` (`null` when mixed) |
| `ShapeFill` | `type`, `foregroundColor`, `transparency`, `setSolidColor(color)`, `clear()` |
| `ShapeLineFormat` | `color`, `weight`, `visible`, `dashStyle` |
| `TagCollection` / `Tag` | `add(key, value)` (keys are kept upper case, as PowerPoint does; an existing key's value is replaced), `delete(key)`, `getItem(key)` (any case), `getItemAt(i)`, `getCount()`, `items`; `key`, `value` (settable). Saved in the file (`p:custDataLst`) |

Every proxy object also has `load(...)`, `toJSON()` and `set({...})`, which follows navigation
properties (`shape.set({fill: {foregroundColor: "red"}, textFrame: {textRange: {font: {bold: true}}}})`).

**Colours** are `#RRGGBB` (also `#RGB`) or an HTML colour name, in any case: the 148 CSS named
colours (`"yellow"`, `"LightSeaGreen"`, `"rebeccapurple"`). Reading a colour answers `#RRGGBB`.

**`insertSlidesFromBase64`** is Reuse Slides (`slide.reuse`): `formatting` `KeepSourceFormatting`
brings the source masters along, `UseDestinationTheme` (the default) moves the slides to this
presentation's layouts; `sourceSlideIds` (`"267#"` or `"267#1234"`) picks slides of the inserted
file; the slides go after `targetSlideId`, else after the current slide.

**Not there, but harmless:** the common API's `*Async` methods DeckCraft lacks (`getFileAsync`,
`getFilePropertiesAsync`, `bindings.*`, `addHandlerAsync` for other events, other coercion types)
call back with a failed result whose error code is `NotSupported`; `ui.displayDialogAsync` with
`NotImplemented`. The Promise-based `Office.addin.*` (`showAsTaskpane`, `hide`,
`getStartupBehavior`, `setStartupBehavior`, `onVisibilityModeChanged`), `Office.ribbon.*` and the
keyboard-shortcut methods of `Office.actions` reject with `NotImplemented`. A page that calls them
gets an error it can handle, never a `TypeError`.

One difference from PowerPoint: a `sync()` is all or nothing. If an action fails, the earlier ones
of the same batch are taken back too, and the error says which action failed (`debugInfo`).

## Function commands (ExecuteFunction)

A button or menu item with `<Action xsi:type="ExecuteFunction">` calls a function of the add-in's
**function file** (`<FunctionFile resid="…"/>`), a page with no pane:

1. DeckCraft loads the function file in a hidden web view (once; later commands reuse it), with
   its Office.js like any pane.
2. When the library has started there, it tells DeckCraft (`{"event": "ready"}`); then DeckCraft
   calls `__craftOfficeRunFunction({"call", "name", "control"})`. The library waits for the page
   to finish loading (its scripts have run), then finds the function: the one the page gave
   `Office.actions.associate(name, fn)`, else a global function of that name (older function files).
3. The function gets an event: `event.source.id` is the control's id in the manifest
   (`<Control id="Contoso.Button">`), and `event.completed()` tells DeckCraft it has finished
   (`{"event": "functionDone", "call"}`).
4. DeckCraft waits up to **5 minutes** for `completed()`. A function that isn't there, that throws,
   or that never calls `completed()` is reported in the status bar; nothing else is held up
   meanwhile.

The function's `PowerPoint.run` and common-API calls go through `office.sync` with the add-in's
permissions, as a pane's do. `DocumentSelectionChanged` reaches every ready page as
`__craftOfficeEvent({"type": "documentSelectionChanged"})` when the slide, shape or text
selection changes.

## Microsoft's office.js

Add-in pages load Microsoft's `office.js` with a `<script>` (from
`appsforoffice.microsoft.com`, `officeapis.public.onecdn.static.microsoft` or a copy of the npm
package `@microsoft/office-js`). It must not run in DeckCraft: outside Office it never reports the
host, and it would replace the `Office` object the page's `Office.onReady` uses. So the bootstrap,
which runs before the page:

- gives every `<script>` whose source is one of those an empty script instead, whether it is in
  the page's HTML (the bootstrap watches the document as the parser builds it, and changes the
  source before the script would run) or made by the page's code (`script.src = …`,
  `setAttribute("src", …)`). The script's `load` event still fires, so loaders that wait for it
  carry on;
- defines `Office`, `PowerPoint`, `OfficeExtension` and `OfficeRuntime` as properties that can't be
  replaced or deleted. Until the library has started they hold the page's early calls
  (`Office.onReady`, `PowerPoint.run`, `Office.initialize`, `Office.actions.associate`) and hand
  them over; afterwards they are the library's objects, whose members are read-only. A script that
  assigns its own `Office` anyway is ignored, except that an `initialize` function on it is kept
  and called.

Per platform: on **Windows** the web view also resolves Microsoft's two office.js hosts to nothing
(WebView2's `--host-resolver-rules`, set through wry), so the file isn't even downloaded. On
**macOS** wry has no safe hook for a page's sub-resource requests (WKWebView's content rules need
platform code with `unsafe`, which DeckCraft forbids), so the bootstrap is the guard there; the
web view's speculative preloader may still download the file, which then never runs.

## The bridge (`office.sync`)

The page's library sends `{"id": n, "actions": [...]}` through the web view; DeckCraft answers
`{"id": n, "results": [...]}` or `{"id": n, "error": {"code", "message", "action"}}`. Each action
names an object by its **path** from the presentation (`{"p": property}`, `{"m": method, "a": args}`):

```json
{"op": "call", "path": [{"p": "presentation"}, {"p": "slides"}, {"m": "getItemAt", "a": [0]}, {"p": "shapes"}], "method": "addTextBox", "args": ["Hi", {"left": 10, "top": 10}]}
{"op": "set",  "path": [{"p": "presentation"}, {"p": "slides"}, {"m": "getItem", "a": ["257"]}, {"p": "shapes"}, {"m": "getItem", "a": [{"result": 0}]}, {"p": "fill"}], "prop": "foregroundColor", "value": "yellow"}
{"op": "load", "path": [{"p": "presentation"}, {"m": "getSelectedSlides"}], "props": ["items"]}
{"op": "call", "path": [{"p": "document"}], "method": "goToById", "args": ["next", "index"]}
```

Paths start at `presentation`, `document` (the common API) or `settings`. An object added earlier
in the same sync is named by that action's result (`{"result": i}`). Writes go through DeckCraft's
commands (`slide.new`, `shape.setBounds`, `text.set`, `format.bold`, `shape.fill`, `slide.reuse`,
`insert.picture`…), on the slide the object is on. Agents can send the same batches with the
`office.sync` command; macro add-ins reach the same objects with `host/sync`.

Settings are kept per presentation and add-in while DeckCraft runs; saving them in the file (the
web-extension parts of the PPTX package) comes next.

## Security

- The add-in's page runs in the web view's sandbox, on its own `https://` origin; its access to
  the presentation is the bridge's, bounded by the manifest's permissions.
- The web view only navigates to `https://` pages; links that open a window go to the browser.
- DeckCraft itself never contacts Microsoft: the add-in's page is loaded from its vendor, and the
  API is DeckCraft's. On macOS a page that references Microsoft's `office.js` may still make the
  web view download it (above); it never runs.

## Limits and next steps

- **Platforms:** the desktop app on macOS and Windows. Linux and FreeBSD need WebKitGTK 4.1 in the
  packages; the web app needs an iframe host.
- **The bootstrap:** the API has to exist before the page's scripts run, and a wasm module of this
  size can't start synchronously in WebView2, so a short JavaScript file (`crates/office-js/bootstrap.js`)
  holds the first calls until the library is ready and keeps Microsoft's `office.js` from running.
  It is the one hand-written script in DeckCraft.
- **Checked by hand only:** the parser-timing part of the `office.js` blocking, the Windows host
  rules, and the function commands' hidden web view run in the real WKWebView and WebView2. The
  library's tests run in Node (`wasm-bindgen-test-runner`), the bridge's and the pane's in Rust.
- Not yet: the dialog API, the shared runtime and `Office.addin`, keyboard shortcuts
  (`ExtendedOverrides`), `Office.ribbon`, `PowerPoint.createPresentation`, hyperlinks, tables and
  pictures through the object model, `shapes.addTable`, settings saved in the file, content add-ins
  on the slide (a `ContentApp` opens in the task pane), the unified JSON manifest, single sign-on
  with Microsoft accounts, manifest icons.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-15 | major | Created: what DeckCraft's Office.js supports (PowerPointApi 1.4 subset, AddinCommands, DocumentEvents, ImageCoercion), the bridge, Microsoft's `office.js` kept from running (bootstrap; host rules on Windows), function commands end to end, colour names, tags and `insertSlidesFromBase64` in the host, image and slide-range coercions, `goToByIdAsync`, `set({...})`, the common API's enumerations, unsupported methods answering `NotSupported` |
