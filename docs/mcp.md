# MCP server

`deckcraft-cli mcp` runs a Model Context Protocol server on stdio. By default it drives its own
headless engine (no window); with `--connect PORT` it drives a running DeckCraft started with
`--control PORT`, so an agent and a person can work on the same deck. `--sample` opens the sample
deck.

Connected mode accepts only newline-terminated replies of at most 16 MiB (including the newline),
with the current request's numeric `id`, a boolean `ok`, and either `result` on success or a string
`error` on failure. CRLF is accepted. Invalid replies close the connection and report that the
command may or may not have run; they never cause a replay. Check the document before retrying a
mutation. Before sending another request, unsolicited bytes in the reader's buffer or on the
socket cause a fresh connection to be opened. Large image replies must fit this budget; render
to a file or use a smaller scale when necessary.

Regression coverage for #718 lives in `crates/mcp/src/backend.rs`: synthetic loopback peers test
reply correlation, envelopes, framing, the size cap and buffered unsolicited bytes, alongside the
lost-reply and reconnect tests.

```json
{
  "mcpServers": {
    "deckcraft": { "command": "deckcraft-cli", "args": ["mcp", "--connect", "7990"] }
  }
}
```

## Confining file access

```sh
deckcraft-cli mcp --automation-read-root /work/project --automation-write-root /work/project/out
```

```json
{"mcpServers": {"deckcraft": {"command": "deckcraft-cli",
  "args": ["mcp", "--automation-read-root", "/work/project", "--automation-write-root", "/work/project/out"]}}}
```

The two flags (VectorCraft, PhotoCraft and the other Craft apps take the same ones, so one client
config works across the suite) keep the agent to the folders you give it:

- Every file a command reads must lie inside the **read root**: `open_presentation` and
  `file.open`, and `insert_picture`, `insert.picture`, `picture.change`, `insert.audio` and
  `insert.video` with a `path`. Pictures, audio and video are copied into the deck when they are
  inserted, so playing them later reads nothing from disk.
- Every file a command writes must lie inside the **write root**: `save_presentation`, `export`
  (every format, and every slide of an image export), `file.save`, `file.saveAs`, `file.export`,
  and a slide rendered to a `path` (`call_app` with `ui.render` or `app.renderSlide`; `render_slide`
  itself returns the picture and takes no `path`), plus the `.saving` temporary file a save goes
  through.
- The write root itself, and a folder, are not files to write: a save's `.saving` temporary file
  sits next to its destination, and next to the root it would be outside it.
- Read and write access are separate. Give the same folder twice for both. With one root given, the
  access the other would grant is denied (with only `--automation-read-root`, nothing can be written
  to disk); without either flag, files are not confined.
- A path is made absolute against the server's working directory, then its links are followed: a
  file (or folder) link that leads out of a root is outside it. For a file that doesn't exist yet,
  its nearest existing folder's links are followed and the rest must be plain names, so `..` below a
  folder that doesn't exist is refused, as is a link that leads nowhere (writing through it would
  create its target). The result is compared with the root by whole folder names: `/work2/x` is not
  inside `/work`. On Windows case is ignored, UNC paths (`\\server\share\...`) are resolved like
  drive-letter ones, and device names (`CON`, `NUL`, `COM1`...) are refused.
- A refused path is an ordinary tool error the agent can read and correct, for example
  `automation path rejected: outside the write root /work/project/out: /etc/x.pptx` or
  `automation filesystem access is not granted: write authority is absent: /work/project/out/x.pptx`.
- The roots must be existing folders; a missing one stops the server with an error. Each flag is
  given once (`--flag <dir>` or `--flag=<dir>`).

The flags confine the in-process server, so `--connect` together with them is an error: the server
would otherwise talk to an app that isn't confined. DeckCraft's desktop app has no equivalent flags
yet, so a running app (`deckcraft --control`) is not confined. `--sample` is the operator's own
choice and is opened before the roots apply.

What the roots do not cover: commands that take no path still read and change the open deck, and
`inspect_document` and `inspect_slide` return it. The system font scan and the app's own recovery
folder are fixed paths, not agent input, and are not checked. Anything that reads or writes a path
goes through one place in the engine (`io::read_file` and `io::write_file`); a test keeps the rest of the
engine from touching the file system on its own.

The checks hold for every thread of the server process. A path is checked, then opened: another
program that swaps a link in between can still win that race, so the roots keep an agent to the
files it was given rather than guarding against other software on the computer.

## Confining commands

```sh
deckcraft-cli mcp --automation-read-root /work/project --automation-write-root /work/project/out \
    --allow-commands "slide.*, text.*, format.*, insert.textBox, edit.select, document.inspect, file.new, file.save, file.export"
deckcraft-cli mcp --allow-commands @allowed.txt   # one id or group.* per line, # comments
```

`--allow-commands` keeps the agent to the commands it was given, the way the roots keep it to
folders. Use both together.

- Every command a client asks for must be on the list: `run_command`, each command of `batch`
  (which stops at the first one off the list, or skips it with `keepGoing`), `call_app`, and the
  commands behind the dedicated tools (`new_presentation` is `file.new`, `open_presentation`
  `file.open`, `save_presentation` `file.save`, `export` `file.export`, `inspect_document`
  `document.inspect`, `add_slide` `slide.new`, `add_shape` `shape.insert`, `add_text_box`
  `insert.textBox`…). A command off the list is an ordinary tool error:
  ``command `slide.delete` is not allowed: it is not on this session's --allow-commands list``.
- Gestures (`pointer`, `key`, `type_text`, `select_tool`) are refused while a list is in force:
  they change the deck without naming a command (a drag moves a shape, a shortcut runs whatever it
  is bound to), so the list could not hold them. Use the commands behind them by name
  (`text.insert`, `shape.insert`, `edit.select`…).
- Commands a command runs itself (typing `* ` turning a paragraph into a list) are part of the
  command that was allowed and aren't checked again. `render_slide` draws the deck without a
  command and stays available when it returns the picture; writing it to a `path` (`call_app`
  with `ui.render` or `app.renderSlide`) is an export and needs `file.export` on the list.
- `list_commands` shows only the commands the client may run.
- A list names ids (`format.bold`) and groups (`format.*`: every command whose id starts with
  `format.`), separated by commas, spaces or new lines. An unknown id, a group that matches no
  command, an empty list, or the flag without a list or given twice stops the server at startup.
- An `@FILE` list must be a regular file (or a link to one). A pipe, socket or device found at the
  first check stops the server without being opened; the check and the open are not atomic, so
  another program that swaps a FIFO in between can still make the open wait. It is read up to
  256 KiB and no further, whatever the file claims to be; a longer one stops the server too.
- Like the roots, the list holds the in-process server: `--connect` with it is an error.

## Tools

| Tool | What it does |
|---|---|
| `list_commands` | Every command (id, label, menu, shortcut, params doc), optionally filtered |
| `run_command` | Run one command: `{command, params}` — every menu item, ribbon button and gesture is a command |
| `batch` | Run several commands in order and stop at the first error |
| `new_presentation`, `open_presentation`, `save_presentation`, `export` | Files: native `.deckcraft`, PowerPoint `.pptx`, PDF (slides, notes pages, handouts), PNG/JPEG, outline, animated GIF (size, seconds per slide, loop, slides) |
| `inspect_document`, `inspect_slide` | Structure of the deck / one slide (shapes, text, placeholders, animations) |
| `render_slide` | A slide as an image the model can look at |
| `add_slide`, `go_to_slide` | Slides by layout |
| `add_shape`, `add_text_box`, `set_text` | Shapes and text |
| `insert_picture`, `insert_table`, `insert_chart` | Content |
| `select`, `pointer`, `select_tool`, `key`, `type_text` | The same selection, mouse and keyboard paths as the UI |
| `screenshot` | The whole window, UI included (connected mode) |
| `call_app` | Any [control protocol](control-protocol.md) method (connected mode) |

A tool refuses an argument that is not in its schema (`insert_chart: unknown argument "data" (expected: categories, series, title, type)`). Keys inside `run_command`'s `params` are that command's parameters.

Useful commands for building decks: `slide.new {layout, title, body}`, `shape.insert {preset,
rect, text}`, `shape.connect {from, to, preset}`, `shape.freeform {points, closed, smooth}`,
`shape.merge {op, ids}`, `format.*`, `design.theme`, `transition.set`, `animation.set`,
`chart.*` (`chart.options {textColor}` colours a chart's title, axes, legend and labels), `table.*`, `file.export {path, layout}` (PDF) or `{path, width, secondsPerSlide, loop}`
(GIF). `deckcraft-cli describe COMMAND` prints any
command's parameters.
