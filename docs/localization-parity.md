# Localization parity with PowerPoint

> **Last reviewed:** 2026-10-17 · **Last updated:** 2026-10-17 · **Change:** major (language catalog and Simplified Chinese landed; previous: minor, IME composition underlined in table cells; previous: major, created) · **Target:** Microsoft PowerPoint for Mac 16.113 (Microsoft 365)

The interface now has the shared Crafting-Apps language system
(`crates/ui-egui/src/i18n/`, the system WordCraft/PdfCraft/PhotoCraft use): English strings in the
code are the catalog keys, `crate::tl!(...)` looks them up per frame, and the `language` setting
(`auto`, following the OS language, or a code) lives in `ui.json` with a picker under
Backstage ▸ Options ▸ Interface language (#15). Simplified Chinese (`zh-hans.tsv`) ships with
**1,323 entries**; the audit test `i18n::audit::the_catalog_covers_every_string_the_interface_looks_up`
drives every ribbon tab, Backstage page, dialog and task pane in a pseudo-language and fails if any
string the interface looks up has no entry. Interface text in a script the interface fonts lack
borrows a system face (PingFang for Simplified Chinese on macOS), the catalog included.
Requests: #30 and PR #34 (Ukrainian, open). Dimension score: **~65% ready** (English complete,
Simplified Chinese interface complete and audited; other eleven key languages open; slide-text
script support partly there), **40–70 h** to parity plus native-speaker review.

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
| Simplified Chinese (Mandarin) | zh-Hans | 1,323 / 1,323 entries (100% of what the interface looks up) | yes / yes / no help | renders with system CJK fonts and a borrowed system face (asset policy excludes Noto CJK from builds); UAX #14 breaking; IME composition (#101); no true vertical layout | no | **interface shipped** | yes | 0 (interface); review + IME polish remain |
| Spanish | es | 0 (0%) | no | full (Latin) | no | none | yes (es, es-MX) | 3–5 |
| Hindi | hi | 0 (0%) | no | Devanagari shaping through HarfRust, untested; no bundled Devanagari font | no | none | **no** (Windows only) | 4–7 |
| Arabic | ar | 0 (0%) | no | bidi, contextual shaping, kashida justification, RTL bullets and caret (#4); **UI not mirrored**; browser export shows boxes (#81) | no | none | yes | 10–16 (incl. UI mirroring) |
| French | fr | 0 (0%) | no | full (Latin) | no | none | yes (fr, fr-CA) | 3–5 |
| Portuguese | pt | 0 (0%) | no | full (Latin) | no | none | yes (pt-BR, pt-PT) | 3–5 |
| Indonesian | id | 0 (0%) | no | full (Latin) | no | none | yes | 3–5 |
| Japanese | ja | 0 (0%) | no | renders with system or craft-fonts Japanese faces; IME composition (#101); vertical text is rotated, not laid out vertically | no | none | yes | 4–7 |
| German | de | 0 (0%) | no | full (Latin) | no | none | yes | 3–5 |
| Korean | ko | 0 (0%) | no | system Hangul fonts, in the interface too; IME composition (#101) | no | none | yes | 4–7 |
| Vietnamese | vi | 0 (0%) | no | stacked diacritics through HarfRust | no | none | **no** (Windows only) | 3–5 |

Other languages shipped: **0** (Ukrainian is in review as PR #34). PowerPoint for Mac ships 16 more
than the twelve above: Czech, Danish, Greek, Finnish, Hebrew, Hungarian, Italian, Dutch, Norwegian,
Polish, Russian, Slovak, Swedish, Thai, Turkish and Traditional Chinese.

Per-language hours assume the infrastructure exists: **8–14 h** once for a string catalog (copy a
sibling app's approach, e.g. EffectCraft's menu and panel catalogs), a language setting that follows
the system (#15), plural and number formatting, and an `xtask` check that every key has a
translation. IME composition landed with #101 (slide text, table cells, notes and text fields;
one undo step per composition, underlined in place); the hours that included it are kept until
the next re-measure.

## Proofing languages

PowerPoint checks spelling in dozens of languages and lets each run carry a proofing language.
DeckCraft keeps `a:rPr/@lang` on round trip but its spelling check is a small English list of
common misspellings (`crates/engine/src/cmd/review.rs`); see [gaps.md](gaps.md#proofing-spelling-thesaurus-autocorrect).

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-17 | major | Language catalog (`crates/ui-egui/src/i18n`, `tl!`, `language` setting, Options picker) and Simplified Chinese (`zh-hans.tsv`, 1,323 entries) landed; audit test requires every looked-up string to have an entry; interface CJK text borrows a system face |
| 2026-10-14 | minor | IME composition in a table cell is underlined in place; the candidate window sits on that cell's caret |
| 2026-10-12 | minor | IME composition for Chinese, Japanese and Korean, and interface text in any installed script (#101) |
| 2026-10-10 | major | Created: twelve-language table, ~1,050 UI strings measured, PowerPoint for Mac's 26 languages from its bundle |
