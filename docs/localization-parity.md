# Localization parity with PowerPoint

> **Last reviewed:** 2026-11-01 · **Last updated:** 2026-11-01 · **Change:** minor (Simplified Chinese interface catalog from craft-i18n: 1,148 DeckCraft rows plus shared rows on 0.13.1, CJK faces borrowed for the UI's own text; previous: major, interface catalogs from craft-i18n, language setting and German) · **Target:** Microsoft PowerPoint for Mac 16.113 (Microsoft 365)

DeckCraft's interface is shown in English, German or Simplified Chinese. Strings stay English
literals in the code and are looked up in the shared
[craft-i18n](https://github.com/storytold/craft-i18n) catalogs when the UI is drawn (`tl!`,
`crates/ui-egui/src/i18n.rs`); `build.rs` embeds them from `CRAFT_I18N_DIR` at the commit in
`craft-i18n.rev`, and a build without it is English only. The interface follows the system
language (#15), or Preferences › Interface language and `app.language` (`auto`, `en`, `de`,
`zh-hans`). Command ids, engine labels returned to scripts, the control channel, the CLI and MCP
stay English. Still English: slide placeholder prompts (drawn by the renderer), status messages
from the engine, the native macOS menu bar and some dialog choice lists. Other requests: #30 and
PR #34 (Ukrainian) and the language PRs listed in storytold/craft-i18n#48, which can now land as
catalog-only PRs there. Dimension score: **~20% ready** (estimated: English complete, German
mostly, Simplified Chinese partial, slide-text script support partly there), **40–75 h** to
parity plus native-speaker review.

## How the numbers are measured

- **UI strings:** ~1,050 user-visible string literals (measured 2026-10-10: distinct
  capitalized literals in `crates/ui-egui/src`, `crates/engine/src/cmd` and `crates/model/src`,
  including the 226 command labels). A catalog will refine the count; until then percentages are
  against this figure.
- **Script support** is about slide text (what users type and import), separately from the UI:
  bidi and shaping live in `crates/text` (UAX #9, HarfRust, UAX #14 line breaking).
- **PowerPoint's languages:** `Contents/Resources/*.lproj` in PowerPoint 16.113.4: ar, cs, da, de,
  el, en, en_GB, es, es_MX, fi, fr, fr_CA, he, hu, id, it, ja, ko, nl, no, pl, pt (Brazil), pt_PT,
  ru, sk, sv, th, tr, zh_CN, zh_TW (26 languages). PowerPoint for **Mac** has no Hindi or Vietnamese
  UI; PowerPoint for Windows has both.

## The twelve key languages

| Language | Code | UI strings translated | Dialogs / tooltips / help | Script support (slide text) | Native review | Status | PowerPoint for Mac UI | Hours to full |
|---|---|---|---|---|---|---|---|---|
| English | en | ~1,050 / ~1,050 (100%) | yes / yes / no help | full | yes | **full** | yes | 0 |
| Simplified Chinese (Mandarin) | zh-Hans | 1,148 DeckCraft rows + shared `common.tsv` (partial; dynamic `format!` strings stay English) | partly / yes / no help | native candidate optionally embeds LXGW WenKai Lite via craft-fonts, with existing per-character fallback; other builds use system CJK fonts (Noto CJK remains excluded); UAX #14 breaking; IME composition (#101); no true vertical layout | no | **partial** | yes | 2–4 + review |
| Spanish | es | 0 (0%) | no | full (Latin) | no | none | yes (es, es-MX) | 3–5 |
| Hindi | hi | 0 (0%) | no | Devanagari shaping through HarfRust, untested; no bundled Devanagari font | no | none | **no** (Windows only) | 4–7 |
| Arabic | ar | 0 (0%) | no | bidi, contextual shaping, kashida justification, RTL bullets and caret (#4); **UI not mirrored**; browser export shows boxes (#81) | no | none | yes | 10–16 (incl. UI mirroring) |
| French | fr | 0 (0%) | no | full (Latin) | no | none | yes (fr, fr-CA) | 3–5 |
| Portuguese | pt | 0 (0%) | no | full (Latin) | no | none | yes (pt-BR, pt-PT) | 3–5 |
| Indonesian | id | 0 (0%) | no | full (Latin) | no | none | yes | 3–5 |
| Japanese | ja | 0 (0%) | no | renders with system or craft-fonts Japanese faces; IME composition (#101); vertical text is rotated, not laid out vertically | no | none | yes | 4–7 |
| German | de | ~840 rows (654 DeckCraft, 185 shared): ribbon, menus, palette, status bar, File tab, dialogs, panes | partly / yes / no help | full (Latin) | no | **partial** | yes | 1–3 |
| Korean | ko | 0 (0%) | no | system Hangul fonts, in the interface too; IME composition (#101) | no | none | yes | 4–7 |
| Vietnamese | vi | 0 (0%) | no | stacked diacritics through HarfRust | no | none | **no** (Windows only) | 3–5 |

Other languages shipped: **German and Simplified Chinese** (Ukrainian is in review as PR #34). PowerPoint for Mac ships 16 more
than the twelve above: Czech, Danish, Greek, Finnish, Hebrew, Hungarian, Italian, Dutch, Norwegian,
Polish, Russian, Slovak, Swedish, Thai, Turkish and Traditional Chinese.

Per-language hours assume the infrastructure exists, which it now does: the craft-i18n runtime
(lookup, fallback, plurals, locale matching), a language setting that follows the system (#15) and a
test that the embedded catalogs parse cleanly. Adding a language is a `locales/<code>/deckcraft.tsv`
in craft-i18n plus its code in `LANGUAGES`. Still to do once: an app-side report of source texts
without a translation (craft-i18n `docs/integration.md`). IME composition landed with #101 (slide text, table cells, notes and text fields;
one undo step per composition, underlined in place); the hours that included it are kept until
the next re-measure.

## Proofing languages

PowerPoint checks spelling in dozens of languages and lets each run carry a proofing language.
DeckCraft keeps `a:rPr/@lang` on round trip but its spelling check is a small English list of
common misspellings (`crates/engine/src/cmd/review.rs`); see [gaps.md](gaps.md#proofing-spelling-thesaurus-autocorrect).
Review › Language sets the proofing language of the selected text or, with *Whole presentation*,
of every text in the file and the default for new text (`format.languageAll`).

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-01 | minor | Simplified Chinese on 0.13.1: upstream refactors keep every existing translation; the new Effects presets, Size & Properties fields, picture styles, print-preview error and command labels are translated (1,148 DeckCraft rows). Still no native review. |
| 2026-10-29 | minor | Simplified Chinese interface catalog (`zh-hans/deckcraft.tsv` in craft-i18n: 1,093 DeckCraft rows plus shared rows), offered as `zh-hans` and drawn with CJK faces borrowed for the UI's own catalog text. Dynamic strings still assembled with `format!` before drawing stay English until the source uses `tl!`/`fmt`. No native review yet. |
| 2026-10-28 | major | Interface catalogs from craft-i18n, system language and Preferences › Interface language (#15); German partial (~840 rows) |
| 2026-10-24 | minor | Local 0.37 candidate adds an opt-in native LXGW WenKai Lite glyph fallback and CJK interface-test coverage. This records local implementation only: managed tests, formatting, clippy, wasm-size and native GUI qualification remain pending. Browser font budget and uniform Chinese typography are unchanged. |
| 2026-10-21 | minor | Review › Language can set the proofing language of the whole presentation |
| 2026-10-14 | minor | IME composition in a table cell is underlined in place; the candidate window sits on that cell's caret |
| 2026-10-12 | minor | IME composition for Chinese, Japanese and Korean, and interface text in any installed script (#101) |
| 2026-10-10 | major | Created: twelve-language table, ~1,050 UI strings measured, PowerPoint for Mac's 26 languages from its bundle |
