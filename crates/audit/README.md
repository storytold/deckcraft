# deckcraft-audit

Report hidden information in a `.pptx` file before sharing it — speaker
notes, hidden slides, cropped images that still carry their full pixel data,
comment authors, creator names in document properties, embedded media, macro
and embedded-object parts, and package parts nothing references.

This crate implements upstream issue
[storytold/deckcraft#953](https://github.com/storytold/deckcraft/issues/953)
("deckcraft-cli audit: report hidden information in .pptx before sharing").
Audit only: it never modifies the file. Sanitizing (removing what it finds)
is a separate, future step.

## Design

- **Raw OPC bytes, never the document model.** The `deckcraft-pptx` model
  drops unmodeled parts on round-trip, so model-based auditing would miss
  things. `audit_bytes` opens the ZIP once and scans the parts directly.
- **Fail-closed.** Malformed input is an error, never a false "clean".
  The XML walkers enforce exactly one root element, reject non-whitespace
  text/CDATA outside the root, reject mismatched closing tags
  (`check_end_names`), and error on EOF with unclosed elements.
- **Namespace URIs, not prefixes.** A producer that spells the
  PresentationML namespace with an unusual prefix is still scanned.
- **Active content by relationship type first**, fixed names/paths
  (`ppt/vbaProject.bin`, `ppt/embeddings/`) as fallback; one part yields one
  finding.
- **Orphan reachability traversed from the package root** (`_rels/.rels`):
  only relationships in reachable parts count, so a `.rels` file inside an
  orphaned subtree cannot rescue its targets.
- **Hostile-input caps:** a 100k entry limit, a 256 MiB per-part
  decompressed cap, a 1 GiB cumulative decompressed budget, and a 2M XML
  element cap per part.

## Finding classes

| ID | Title | Severity | What it means |
|---|---|---|---|
| `notes/slide` | speaker notes present | warn | a `ppt/notesSlides/*.xml` part exists |
| `hidden/slide` | hidden slide | warn | `show="false"` (W3C boolean) on the `p:sld` root of a slide part (`CT_Slide`, ECMA-376 Part 1 §19.3.1.38); omitted `show` means shown |
| `image/crop-retains-data` | cropped image retains full pixel data | warn | an `a:srcRect` crop on a `p:pic` whose `a:blip` still references the full image part (§20.1.8.50 `CT_BlipFillProperties`, §20.1.8.51 `CT_RelativeRect`). Flag only: stripping would destroy data |
| `author-pii/comment` | author name on comment | warn | `p:cmAuthor` (`id`, `name`, `initials`) referenced by `p:cm` via `authorId` (§19.4.1/§19.4.2) |
| `author-pii/core-properties` | author name in document properties | warn | `dc:creator` / `cp:lastModifiedBy` in `docProps/core.xml` (ECMA-376 Part 2 §12.3.1) |
| `author-pii/extended-properties` | organization name in extended properties | info | `Company` / `Manager` in `docProps/app.xml` (ECMA-376 Part 2 §12.3.2) |
| `media/metadata` | embedded media | info | parts under `ppt/media/`, reported with their part names |
| `active-content/vba` | VBA macro project | warn | by `vbaProject` relationship type, or `ppt/vbaProject.bin` |
| `active-content/ole` | embedded object | warn | by `oleObject`/`package` relationship type, or under `ppt/embeddings/` |
| `package/orphan-part` | orphan package part | info | a ZIP entry no relationship targets |

The IDs are the crate's public contract: they appear in CLI output and
serialized reports. Never rename them; add a new variant instead.

## JSON report

```json
{
  "file": "deck.pptx",
  "findings": [
    {
      "id": "hidden/slide",
      "title": "hidden slide",
      "severity": "warn",
      "location": "ppt/slides/slide2.xml <p:sld show=\"0\">",
      "detail": "slide hidden from the slide show"
    }
  ],
  "count": 1
}
```

Findings are sorted by (id, location, detail), so repeated runs produce
identical output.

## CLI

```
deckcraft-cli audit FILE
```

Exit status: **0** when the deck is clean, **1** when findings are present
(usable as a CI gate), **2** when the audit could not run. Usage errors —
unknown option, missing file argument, unreadable file — are exit 2, never
1, so a script cannot mistake a typo for "findings present".

## Limits (v1)

- Speaker notes are reported by presence of notes-slide parts; the audit does
  not judge their content.
- Crops are detected on pictures in slide parts (`ppt/slides/*.xml`).
- Only the classic comment model (`ppt/commentAuthors.xml` + `ppt/comments/`)
  is scanned; the modern threaded-comments extension is not.
- Anonymized producer values are reported as-is: the finding records what a
  recipient would see.
