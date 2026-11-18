# Add-ins (plug-ins)

DeckCraft runs command plug-ins: small WebAssembly modules that work on the open presentation
through the same commands as the ribbon, the shortcuts, the CLI, the control channel and MCP.
They run in the sandbox of craft-libs' shared `craft-plugin` crate
([RFC-0046](https://github.com/storytold/craft-contrib/blob/main/rfcs/0046-shared-plugin-runtime.md)),
with an instruction budget and memory limits, and reach nothing but the commands below. The ABI,
the manifest (`craft-plugin.json`) and the `.craftplugin` bundle are described in the
[craft-plugin README](https://github.com/storytold/craft-libs/blob/main/crates/plugin/README.md).
A manifest must name `deckcraft` in `apps`. Office add-ins and VBA are a different thing and stay
out of scope.

## Using add-ins

- **Insert ▸ Add-ins ▸ Get Add-ins** opens the Add-ins window on getting more: from a
  `.craftplugin` file, from a GitHub repository's release (`owner/repo` or `owner/repo@tag`; the
  bundle is checked against the release's `SHA256SUMS`) or from a catalog (an `index.json` on
  GitHub, or a local `file://` one while you write an entry). Downloads run off the UI thread.
- Before anything is installed, a confirmation shows the add-in's name, author, kind, access
  (read or edit the presentation), the commands it may call, where it comes from and its SHA-256.
  It says plainly when the bundle is in no catalog you use.
- **Insert ▸ Add-ins ▸ My Add-ins** lists each installed add-in's commands. A command with
  parameters asks for them first, in a form made from its manifest. **Manage Add-ins…** runs,
  updates and removes them. The window is also `app.addins` (`{page?: installed|get}`).
- A run is **one undo step**, named after the add-in: the commands it calls run nested inside
  `plugin.run` and record no steps of their own. A run that fails (a trap, an exhausted budget, a
  refusal it passes on, a non-zero status) leaves the presentation, the selection, the undo
  history and the add-in's settings as they were.
- The desktop app keeps add-ins in `Plug-ins` in its settings folder, and each add-in's settings
  (at most 64 KB, keys up to 128 characters) and the catalogs added in `plugin-settings.json`
  beside it (`DECKCRAFT_NO_PREFS` turns both off). The CLI, MCP and the web build keep add-ins for
  the session only and download nothing (install from a file, `data` or a local catalog there).
- Add-in panels (`ui_show`) aren't shown yet: a run says so in its `panel` field.

## Commands

| Command | Does |
|---|---|
| `plugin.list` | The installed add-ins, problems reading the folder, the catalogs |
| `plugin.install` | `path`, `data` (base64), `repo` or `catalog` + `id`; `review: true` describes it without installing, `sha256` installs the bundle reviewed |
| `plugin.update` | Checks each add-in's catalog or repository; `apply` installs updates that ask for no other permissions |
| `plugin.remove` | Removes an add-in and its settings |
| `plugin.run` | `id`, `command?`, `params?`, `fuel?` |
| `plugin.catalog.list` | The packages DeckCraft can run in a catalog, or in every catalog added |
| `plugin.catalog.add` | Adds (or with `remove`, removes) a catalog |

## What an add-in may call

Only the commands on the allowlist in `crates/engine/src/plugin.rs` (`ALLOWED`): reading the
presentation (`slide.inspect`, `shape.inspect`, `text.get`…), the current slide and the
selection, slides and sections, shapes and their formatting, text and its formatting, tables,
charts, diagrams, pictures already in the deck, themes, backgrounds, masters and layouts,
transitions, animations, comments and guides. Every other command is refused, so a command added
to DeckCraft later stays refused until it is reviewed. Refused, among others:

- files and paths: `file.*` (open, save, export, print), pictures, video, audio and photo albums
  from files, Change Picture, themes and slides from other files, Compare, fonts, Action
  Settings (it can name a program); any parameter `path`, `paths`, `file`, `files`, `dir` or
  `folder` holding text (an animation's motion `path` is not a file and is allowed);
- the slide show, the presenter and media playback (`show.*`, `media.play`…), views and
  windows (`view.*`, `window.*`);
- Undo and Redo, the clipboard (`edit.copy`, `edit.cut`, `edit.paste*`), the Format and
  Animation Painters, Set as Default Shape;
- preferences: AutoCorrect, the spelling dictionary, the input method's composition;
- add-ins themselves (`plugin.*`).

A read-only add-in is refused commands that edit, and a call of its that changed the
presentation anyway is put back. A call that asks the window for a dialog, a pane, a file picker
or the slide show is put back and refused (pass the command's parameters instead).
