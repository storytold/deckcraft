# Control protocol

Start the app with `--control <port>`. The server listens on `127.0.0.1` only and speaks JSON
lines: one request object per line, one reply per line.

A line that is not a JSON object with a string `method` (an HTTP request, text that isn't JSON,
bytes that aren't UTF-8) gets one error reply and the server closes the connection; nothing sent
after it runs. Blank lines are skipped.

The connected MCP client requires the reply's numeric `id` to match its request, `ok` to be a
boolean, and exactly one of `result` (success) or a string `error` (failure). It caps each reply at
16 MiB including its terminating newline, accepts CRLF, and rejects EOF inside a frame. Invalid
replies invalidate the connection and leave the command's outcome unknown; the client does not
replay it. Unsolicited bytes already buffered between calls force a reconnect before sending.

The server bounds what a client can hold: 16 connections at once (more get `{"ok":false,"error":"busy: …"}`
and are closed), 16 MiB per request line (longer closes the connection), 64 queued requests (more are
answered `busy`), 10 minutes idle and 30 seconds per blocked reply write before the socket is closed, and
60 seconds waiting for the UI (`"error":"timeout"`). A request still queued when that wait expires is
abandoned: the UI skips it, so a retry cannot run it twice. If the UI had already started it, the reply
says the outcome is unknown instead of `timeout`.

```json
{"id": 1, "method": "engine.execute", "params": {"command": "shape.insert", "params": {"preset": "roundRect", "rect": [100, 100, 240, 120], "text": "Hello"}}}
{"id": 1, "ok": true, "result": {"id": 258}}
```

Coordinates are slide points (1/72 inch; a 16:9 slide is 960 × 540) unless noted.

| Method | Params | What it does |
|---|---|---|
| `engine.execute` / `ui.menu.invoke` / `command` | `{command, params}` | Run any command (see `engine.commands`; `deckcraft-cli describe COMMAND` documents one) |
| `engine.commands` | — | Every command: id, label, menu path, shortcut, params doc, enabled / disabled reason |
| `document.inspect` | — | Slides, sections, layouts, shapes, selection |
| `ui.inspect` | — | UI state, active tool, window/canvas/slide rects, dialog, `home` (the File tab over an open presentation), `backstage` (the current File page, including the start screen), slide show state (`grid`, `zoomArm`, `zoom`, `blank`, `presenterPaused`, `presenterSecs`, `notesPx`, `custom`, `goto`; `cells` when See All Slides is open), `animPreview` (canvas playback: slide, time, duration), documents, perf (fps, render ms) |
| `ui.tool.select` | `{tool, preset?}` | `select`, `text`, `shape` + preset, any preset name, `curve`, `freeform`, `scribble`, `pen`, `highlighter`, `eraser` |
| `ui.pointer` | `{events: [{kind: down\|drag\|up\|move\|doubleclick\|tripleclick\|contextclick, x, y, mods?, pressure?}], mods?}` | Send slide-point pointer events through the active tool's mouse path. `contextclick` performs the right-click selection used by context menus; `pressure` is `0..=1` for a pen and can be omitted for a mouse |
| `ui.key` | `{key, shift?, alt?, cmd?}` | Send a key press. During an active slide show, digits followed by Enter jump to that slide; G opens See All Slides, Z arms zoom, T toggles the presenter timer's pause, R resets it, and `[` / `]` resize notes. Esc clears a typed slide number or backs out of the grid/zoom before ending the active show. Close See All Slides with Esc or G before using the other listed show shortcuts. Slide-show keys ignore modifier flags; use `show.end` to leave the end-of-show screen. Outside a show, Escape stops canvas animation preview |
| `ui.text` | `{text}` | Insert into the current text edit. With one shape selected and no active text edit, begin editing and replace the shape's existing text |
| `ui.ime` | `{preedit, active?: [start, end]}` / `{commit}` | An input method's composition through egui, as a real one arrives: shown in place on the slide or in a focused field; `{"preedit": ""}` cancels |
| `ui.move` / `ui.click` / `ui.drag` | screen points | Real egui pointer input — reaches every widget, menu and pane |
| `ui.set` | `{tab?, view?, pane?, notes?, zoom?, brightness?, ruler?, gridlines?, guides?, …}` | UI state (returns the full state) |
| `ui.dialog.open` / `ui.dialog.close` | `{id, fields?}` | Open or close a dialog by id; `fields` pre-fills its fields (About: `{"tab": "contributors"}` or `"models"`) |
| `ui.screenshot` | `{path?}` | PNG of the whole window (base64 when no path) |
| `ui.render` | `{path?, slide?, scale?}` | Render a slide headlessly (`slide`: 0-based index, like every slide index here; the editor shows index + 1) |
| `ui.resize` / `ui.focus` | | Window control |
| `show.start` / `show.end` | `{from?: slide index, custom?: name, reading?}` | Slide show. `custom` plays that custom show from its first slide and does not change Set Up. With no `from` and no `custom`, playback starts at the first slide of the show order. A `from` that is not a slide of the deck (negative, not a whole number, past the end) is refused; a hidden slide plays |
| `app.open` / `app.save` / `app.export` / `app.quit` | `{path, …}` | Files (export takes the `file.export` params, e.g. `{"path": "deck.pdf", "layout": "handouts", "perPage": 3}` or `{"path": "deck.gif", "width": "large", "secondsPerSlide": 3, "loop": false}`, or `{"path": "slide.png", "transparentBackground": true}` for a PNG without the slide background) |

The MCP server (`deckcraft-cli mcp`, see [mcp.md](mcp.md)) wraps the same methods for Claude
and other agents; `deckcraft-cli app --method METHOD [PARAMS]` sends one request from a shell.

## Inspecting slide actions

Run the `slide.actions` engine command through `engine.execute`:

```json
{"id": 2, "method": "engine.execute", "params": {"command": "slide.actions", "params": {"index": 0}}}
```

`index` is zero-based and optional; omitting it inspects the selected slide. The reply's `result`
contains `{index, slideId, actions}`. Each action includes an `owner` with the shape id/name and
its group path; text actions also identify the paragraph/run and run text, plus the table row/cell
for table text. Actions report their `event` (`click`, `hover`, `textLink`, or `textHover`),
`type`, `target`, and full `hyperlink`. Inspection never runs an action. A slide without actions
returns `"actions": []`.
