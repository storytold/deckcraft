# Localization parity with PowerPoint

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-10 · **Change:** minor (Ukrainian UI catalog and language selection) · **Target:** Microsoft PowerPoint for Mac 16.113 (Microsoft 365)

DeckCraft's interface supports English and Ukrainian through `crates/ui-egui/src/i18n/uk.tsv`.
Preferences offers the system language or an explicit choice; desktop choices persist across
restarts. Menus, ribbon, command palette, panels, dialogs and presenter controls are translated,
with English fallback. Presentation content, custom placeholder prompts and command IDs keep
their original values. This addresses #15 (follow the system language) and #30 (Ukrainian).
Dimension score: **~8% ready** (estimated: English complete, slide-text script
support partly there), **55–90 h** to parity plus native-speaker review.

## How the numbers are measured

- **UI strings:** ~1,050 user-visible string literals (measured 2026-10-10: distinct
  capitalized literals in `crates/ui-egui/src`, `crates/engine/src/cmd` and `crates/model/src`,
  including the 226 command labels). Ukrainian catalog tests check command, menu, gallery and
  literal translation-call coverage, unique keys, placeholders and bundled Cyrillic glyphs.
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
| Ukrainian | uk | UI catalog; coverage checked by tests | UI captions; diagnostics retain original wording | Cyrillic; bundled UI fonts tested | no | available; English fallback | no | — |
| Simplified Chinese (Mandarin) | zh-Hans | 0 (0%) | no | renders with system CJK fonts (asset policy excludes Noto CJK from builds); UAX #14 breaking; **no IME composition**; no true vertical layout | no | none | yes | 6–10 (incl. IME) |
| Spanish | es | 0 (0%) | no | full (Latin) | no | none | yes (es, es-MX) | 3–5 |
| Hindi | hi | 0 (0%) | no | Devanagari shaping through HarfRust, untested; no bundled Devanagari font | no | none | **no** (Windows only) | 4–7 |
| Arabic | ar | 0 (0%) | no | bidi, contextual shaping, kashida justification, RTL bullets and caret (#4); **UI not mirrored**; browser export shows boxes (#81) | no | none | yes | 10–16 (incl. UI mirroring) |
| French | fr | 0 (0%) | no | full (Latin) | no | none | yes (fr, fr-CA) | 3–5 |
| Portuguese | pt | 0 (0%) | no | full (Latin) | no | none | yes (pt-BR, pt-PT) | 3–5 |
| Indonesian | id | 0 (0%) | no | full (Latin) | no | none | yes | 3–5 |
| Japanese | ja | 0 (0%) | no | renders with system or craft-fonts Japanese faces; no IME composition; vertical text is rotated, not laid out vertically | no | none | yes | 4–7 |
| German | de | 0 (0%) | no | full (Latin) | no | none | yes | 3–5 |
| Korean | ko | 0 (0%) | no | system Hangul fonts; no IME composition | no | none | yes | 4–7 |
| Vietnamese | vi | 0 (0%) | no | stacked diacritics through HarfRust | no | none | **no** (Windows only) | 3–5 |

Other languages shipped: **1 (Ukrainian)**. PowerPoint for Mac ships 16 more
than the twelve above: Czech, Danish, Greek, Finnish, Hebrew, Hungarian, Italian, Dutch, Norwegian,
Polish, Russian, Slovak, Swedish, Thai, Turkish and Traditional Chinese.

The UI catalog, system-language setting and Ukrainian integer-count formatting now exist.
Additional languages still need catalogs, appropriate plural/number formatting and native review.
IME composition (3–6 h) is counted once, under Simplified Chinese.

## Proofing languages

PowerPoint checks spelling in dozens of languages and lets each run carry a proofing language.
DeckCraft keeps `a:rPr/@lang` on round trip but its spelling check is a small English list of
common misspellings (`crates/engine/src/cmd/review.rs`); see [gaps.md](gaps.md#proofing-spelling-thesaurus-autocorrect).

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-10 | minor | Added Ukrainian catalog, language selection, English fallback, preserved custom prompts and bundled-font coverage |
| 2026-10-10 | major | Created: twelve-language table, ~1,050 UI strings measured, PowerPoint for Mac's 26 languages from its bundle |
