# The `.deck` folder format

> **Last updated:** 2026-10-12 · **Change:** major (created with the format, issue #220)

A `.deck` folder holds the same document as a `.deckcraft` file, split into indented JSON files
so that Git (or any file-based store) can diff, review and merge it slide by slide. DeckCraft
opens and saves it directly (File ▸ Open Folder…, Save As ▸ DeckCraft folder (.deck),
`file.saveAs`/`file.export` with format `deck`, `deckcraft-cli convert x.deckcraft x.deck`).
The `.deckcraft` zip and PPTX stay the exchange formats; the folder is for repositories.

Code: `crates/format/src/folder/` (pure functions over an in-memory file map, wasm-safe) and
`crates/engine/src/folder.rs` (disk I/O, native builds only).

## Layout

```text
talk.deck/
├── deck.json                 format, version and the presentation fields not stored elsewhere
├── slides.order              slide order and sections, one slide key per line
├── sections.json             only when the sections can't be written in slides.order
├── slides/
│   ├── 01JA7Q3M5K8X9Y2Z3A4B5C6D7E.json   one slide per file
│   └── 01JA7Q4B2DQ7W1N8V3C5X6Z9KM.json
├── masters/
│   ├── 3.json                one master with its layouts and theme, named by its id
│   ├── notes.json            the notes master, if any
│   └── handout.json          the handout master, if any
├── media-meta/
│   └── 412.json              {"id": 412, "name": "image3.png", "contentType": "image/png", "sha256": "9f2c…"}
└── media/
    └── 9f2c41….png           media bytes, named by their SHA-256
```

`deck.json`:

```json
{
  "format": "deckcraft-folder",
  "version": 1,
  "deckcraftFormatVersion": 1,
  "presentation": {
    "slideSize": { "width": 960.0, "height": 540.0 },
    "notesSize": { "width": 540.0, "height": 720.0 },
    "customShows": [],
    "show": { "…": "…" },
    "headerFooter": { "…": "…" },
    "props": { "…": "…" },
    "defaultTextStyle": { "…": "…" },
    "firstSlideNumber": 1,
    "embeddedFonts": []
  }
}
```

`presentation` holds every field of the `.deckcraft` manifest's `presentation` except those
stored in their own files (`slides`, `masters`, `notesMaster`, `handoutMaster`, `sections`,
`media`) and `nextId`. `version` is the folder layout version; `deckcraftFormatVersion` the
version of the JSON model inside, as in a `.deckcraft` manifest.

`slides.order`:

```text
# slides.order v1
section: Introduction
01JA7Q3M5K8X9Y2Z3A4B5C6D7E
01JA7Q4B2DQ7W1N8V3C5X6Z9KM
section: Appendix
01JA7R0P6T3S2D9F8G7H6J5K4L
```

Empty lines and lines starting with `#` are ignored; `section: <name>` opens a section; any other
line is a slide key. Slides before the first section belong to none.

## Rules

1. **Lossless.** Reading a folder gives the model a `.deckcraft` with the same content gives,
   through the same media and `repair` path (`next_id` aside, see 8).
2. **Canonical JSON.** 2-space indentation, LF line ends, a final newline, UTF-8 written
   unescaped, keys in the order the model writes them, numbers as serde_json writes them (with
   `float_roundtrip`). Saving an unchanged deck writes identical bytes, and DeckCraft rewrites
   only the files that changed.
3. **Fewer nulls, verified.** A `key: null` pair is dropped only when reloading without it gives
   the same model. Where the default isn't null the null stays, e.g. `Chart.title` and
   `Chart.legend` (`None` means "no title"; an absent key means the default "Chart Title" and a
   bottom legend). Nulls inside arrays stay; empty arrays and objects are never dropped.
4. **Unknown keys are kept.** Keys this build doesn't know (written by a newer DeckCraft or
   another tool) are kept where they were when the file is saved again: in objects matched by
   key, in arrays by `id` (or by position when lengths match). Unknown top-level keys of
   `deck.json` stay too.
5. **Split.** One slide per file (the slide object as in the manifest, with its numeric `id`); one
   master per file with its layouts and theme; the rest in `deck.json`.
6. **Stable slide keys.** A slide's file is named by a ULID (26 upper-case Crockford base-32
   characters) given when it is first saved, and found again on later saves by the slide `id`
   written in the file. A new slide gets a new key; a deleted slide's file is removed; moving a
   slide only changes `slides.order`. Two branches that add slides never compete for a file name.
7. **Media by content.** Bytes go to `media/<sha256>.<ext>` (extension from the content type, else
   the original name, else `bin`), stored once however many media records share them; each media
   id has a record in `media-meta/<id>.json` with its `sha256`. Linked media without bytes has no
   `sha256` and no bytes file. On reading, the bytes must match their hash (a Git LFS pointer that
   wasn't pulled is reported as such).
8. **Volatile fields left out.** `generator` and `nextId` aren't written; `nextId` is recomputed on
   load as the highest id in the deck.
9. **Round-trip XML stays a string.** `raw` (charts, extensions) and `rawExt` stay JSON strings.
10. **Order.** Slide order and sections are in `slides.order`. If the sections can't be written
    there (not contiguous runs in slide order, a slide in two sections, a line break in a name),
    `slides.order` lists the slides only and `sections.json` holds the sections as in the model.
    Masters and media are read in id order; when the deck has them in another order, `deck.json`
    holds it (`mastersOrder`, `mediaOrder`). DeckCraft reports these fallbacks when saving.

## Reading

- `format` must be `deckcraft-folder` or `deckgit` (the external tool this format started as);
  a folder keeps its `format` value and its `slides.order` header line when saved again.
- A slide key listed twice in `slides.order` (an unresolved merge) is an error. A slide file that
  `slides.order` doesn't list is read after the listed ones, so nothing is lost after an
  interrupted save or a hand edit. A listed slide without its file is an error.

## Saving

- Only files the format owns are written or removed: `deck.json`, `slides.order`,
  `sections.json`, `slides/<KEY>.json`, `masters/<id>.json`, `masters/notes.json`,
  `masters/handout.json`, `media-meta/<id>.json` and `media/<sha256>[.<ext>]`. Anything else in
  the folder (a README, `.gitattributes`…) is left alone.
- Changed files are first written to hidden temporary siblings (`.<name>.saving`), then renamed
  into place (media first, `deck.json` last), then owned files that no longer belong are removed.
  A new folder is written next to its place and renamed in one step.
- Saving into an existing folder that isn't a folder deck (not empty, no `deck.json`) is refused.
- The browser build doesn't open or save folders.

## Using it with Git

- Media can go to Git LFS (`media/** filter=lfs diff=lfs merge=lfs -text` in `.gitattributes`).
- `slides.order` can use `merge=union` so concurrent slide additions never conflict; a union merge
  can duplicate a moved slide's key, which DeckCraft then reports instead of guessing.
- Two branches that both add shapes or slides may allocate the same numeric ids (the model's
  shared counter); keeping ids apart across branches, repairing them after a merge, and importing
  PowerPoint files returned by reviewers are left to external tools.
