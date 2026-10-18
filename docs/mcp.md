# MCP server

`deckcraft-cli mcp` runs a Model Context Protocol server on stdio. By default it drives its own
headless engine (no window); with `--connect PORT` it drives a running DeckCraft started with
`--control PORT`, so an agent and a person can work on the same deck. `--sample` opens the sample
deck. Stdio request frames are limited to 16 MiB including the newline; oversized frames receive a
JSON-RPC error and are drained so the next request can still be handled. Invalid UTF-8 frames receive
a parse error.

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

The two flags keep the agent to the folders you give it. Repeat either flag to allow access to more
folders:

- Every file a command reads must lie inside the **read root**: `open_presentation` and
  `file.open`, and `insert_picture`, `insert.picture`, `picture.change`, `insert.audio` and
  `insert.video` with a `path`. Pictures, audio and video are copied into the deck when they are
  inserted, so playing them later reads nothing from disk.
- Every file a command writes must lie inside the **write root**: `save_presentation`, `export`
  (every format, and every slide of an image export), `file.save`, `file.saveAs`, `file.export`,
  and `render_slide` with a `path`, plus the `.saving` temporary file a save goes through.
- Read and write access are separate. Give the same folder twice for both; a root left out grants
  none of its access (with only `--automation-read-root`, nothing can be written to disk). Without
  either flag nothing changes.
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
- The roots must be existing folders; a missing one stops the server with an error. Repeat either flag
  to allow access to another folder; both `--flag <dir>` and `--flag=<dir>` forms work.

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

## Tools

| Tool | What it does |
|---|---|
| `list_commands` | Every command (id, label, menu, shortcut, params doc), optionally filtered |
| `run_command` | Run one command: `{command, params}` — every menu item, ribbon button and gesture is a command |
| `batch` | Run several commands in order and stop at the first error |
| `new_presentation`, `open_presentation`, `save_presentation`, `export` | Files: native `.deckcraft`, PowerPoint `.pptx`, PDF (slides, notes pages, handouts), PNG/JPEG, outline, animated GIF (size, seconds per slide, loop, slides) |
| `inspect_document`, `inspect_slide` | Structure of the deck / one slide (`index` or `slide`; shapes, text, placeholders, and when present `textColor`, `line: {color, width}`, a connector's `from`/`to: {id, site}` and `links: [{text, url\|slide}]`, a scheme colour given as its slot name so a theme change is seen as such; animations as `{count, clickSteps}`, the full list with `animations: true` or `animation.get`) |
| `render_slide` | A slide (`index` or `slide`, as `export` and `inspect_slide` take it too) as an image the model can look at (image content; a client that cannot read images: `export {path, format: png, slide}` and view the file) |
| `add_slide`, `go_to_slide` | Slides by layout |
| `add_shape`, `add_text_box`, `set_text` | Shapes and text |
| `insert_picture`, `insert_table`, `insert_chart` | Content |
| `select`, `pointer`, `select_tool`, `key`, `type_text` | The same selection, mouse and keyboard paths as the UI; a `pointer` gesture and an arrow-key nudge answer `selected: [{id, rect}]`, the selection's boxes afterwards (`edit.selection {}` answers the same at any time) |
| `screenshot` | The whole window, UI included (connected mode); image content, or with `path` the PNG is written there and the answer is text `{path, width, height}` for a client that cannot read images |
| `call_app` | Any [control protocol](control-protocol.md) method (connected mode) |

A tool refuses an argument that is not in its schema (`insert_chart: unknown argument "data" (expected: categories, h, series, title, type, w, x, y)`). Keys inside `run_command`'s `params` are that command's parameters, and a command refuses a key it doesn't take, naming the ones it does (`` `shape.insert` was given `shape`, which it doesn't take. `shape.insert` takes {preset: …, rect: …} ``). An unknown command id names the commands like it (`` unknown command `new`; commands named like it: file.new, slide.new ``), and a command that is not available says how to make it so (`` nothing selected: edit.select {"ids": […]} selects shapes, or pass `ids` ``). An `ids` entry that is not an integer id (a `$name` outside a batch, a string) is refused naming it, instead of being dropped.

**Fewer turns.** `shape.insert {preset, rect, text, fill, line, size, color, bold, name}` makes a
styled box in one command. `add_slide` (and `slide.new`) answer `placeholders: {title: id, body:
id, …}` so the title or body can be set without an inspect. Shape ids are unique in the
presentation: a shape or text command given an id on another slide edits it there, makes that
slide current and answers `{"changed": 1, "onSlides": [4], "currentSlide": 4}`; the read-only
lookups (`shape.inspect`, `text.get`, `shape.sites`) find it anywhere, move nothing, and
`shape.inspect` and `text.get` answer the `slide` index it is on. Only the commands that need
several shapes on one slide (`shape.connect`, `arrange.*`, `smartArt.*`) refuse an id on another
slide, saying where the shape is and the `slide.go` that gets there; `comment.*` acts on the
current slide. `batch` items and `run_command` may name a tool (`add_shape`,
`add_text_box`, `go_to_slide`…) instead of a command, with the tool's arguments as `params`; a
`batch` answers one text array, so an item naming a tool that answers an image (`render_slide`,
`screenshot` without `path`) is refused and told to run on its own. A
`batch` item with `"as": "a"` can be referred to by any later item's params as `"$a"` (its `id`,
or its whole answer when it has none), so insert, connect and animate fit in one batch; a `$name`
with no such earlier item is refused.

Answers carry what the next step needs, so it costs no inspect: `insert.table`, `insert.chart`,
`insert.picture` and `insert.smartArt` (which takes a `rect` too) answer `{id, rect}`; the `table.insertRow*`/`insertColumn*` commands answer the
table's `rect`, `rows`, `cols` and `overflows` (true when the box runs off the slide);
`slide.duplicate` answers the copy's `index`, `placeholders` and `shapes`; `slide.move` answers
`{index, id}`; `slide.fromOutline` answers `{slides, indices, ids}` and leaves its last new slide
current, so a following `slide.new` lands after it; `section.add` answers the resulting
`sections: [{name, start, slides}]`, refusing an `at` past the end with the valid range;
`design.theme` answers the palette as `design.palette {}` does; `design.headerFooter` answers
`{slides, default: true}` and becomes the presentation's default, so slides added later carry the
same footer and number; `comment.add` answers `{index, slide}`; `animation.add` answers `{count, indices, steps}`, `steps`
being the click step of each added animation as `animation.get` reports `step` (and `path`, the
OOXML path string, for a motion path);
`comment.reply`/`comment.resolve` answer the thread's state and
`comment.list {all: true}` lists every slide's comments with their slide index (without `all` it
is the current slide only); `edit.undo`/`edit.redo` answer `{label, slide}`;
`transition.applyAll` answers `{slides, source, kind}` (kind, duration and options are all copied
from slide `source`, the current slide) and is refused when the current slide has no transition,
since copying nothing would clear every slide; `{clear: true}` does that on purpose. `file.export`
refuses `perPage` with the `notes` or `slides` PDF layout: as in PowerPoint's Print page, notes
pages (one slide with its notes per page) and handouts (several slides per page) are separate
layouts, and `perPage` belongs to handouts. `file.export {all: true, path: "d.png"}` writes one
file per slide numbered from 1, the displayed slide number (`d1.png`, `d2.png`…), while `slide`
is a 0-based index; it answers `{files, indices}`, `indices` being the 0-based slide of each file.
`chart.data` replaces the whole `categories`/`series` it is given, so pass every series to keep.
`text.replace {text, with, scope?: body|titles|notes|all}` answers `{replaced, slides}`; `body` is the
text in non-title placeholders, text boxes, shapes and tables, so a product name in body text can be
changed without touching titles or notes (the default is still everything).
`animation.timing`'s `index` is the animation's position in the slide's list as `animation.get`
numbers them; `indices: [..]` changes several and `all: true` every animation on the slide (one call
retimes a slide), answering `{changed}`; give one of the three (two are refused, as in
`animation.options`); `animation.get` reports each one's click `step`. `review.spelling` answers `{dictionary, lang, issues}`:
with `dictionary: false` no Hunspell dictionary was installed for `lang` and only the built-in
common-misspellings list ran, so an empty `issues` is not a clean deck (`review.dictionary` says
which folders are searched). An unknown
`table.style` id or a `chart.options` `palette` that is not a palette name (`colorful1`…`4`,
`monochrome1`…`13`) is refused naming the valid values.

Useful commands for building decks: `slide.new {layout, title, body}`, `shape.insert {preset,
rect, text}`, `shape.connect {from, to, preset}`, `shape.freeform {points, closed, smooth}`,
`shape.merge {op, ids}`, `format.*`, `design.theme`, `transition.set`, `animation.set`,
`chart.*` (`chart.options {textColor}` colours a chart's title, axes, legend and labels), `table.*`, `file.export {path, layout}` (PDF) or `{path, width, secondsPerSlide, loop}`
(GIF). `design.palette {}` answers the active theme's colours (`dk1`, `lt1`, `accent1`…`accent6`,
`hlink`, `folHlink`, plus the resolved `text` and `background`) and fonts, so a diagram can use
the scheme names `accent1`…`accent6` or those hex values instead of hard-coding colours.
`deckcraft-cli describe COMMAND` prints any command's parameters.
