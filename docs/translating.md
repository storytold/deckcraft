# Interface language catalogs

`crates/ui-egui/locales/zh-TW.json` maps original English display text to Taiwan Traditional
Chinese. `include_str!` embeds it: rebuild to update it, then select Preferences ▸ Interface
language. This setting is separate from the slide proofing-language dialog.

Use `locale::text(ui.ctx(), "Label")` for UI labels and `locale::format_text` for named values.
Keep `{placeholders}` exactly. Do not translate document content, filenames, user input, command
IDs, preference keys, JSON protocol values or logs. Unknown strings fall back to English.

Use 檔案, 儲存, 投影片, 簡報, 字型, 品質, 帳號 and 剪貼簿. Keep brand names, technical
abbreviations and keyboard shortcuts. The catalog is a machine-assisted draft, not native-reviewed;
no proprietary product translation resources were used. System fonts follow existing discovery
and memory budgets; no vendor font files are included.

Run `cargo test -p deckcraft-ui-egui --lib locale` and workspace gates. Tests cover parsing,
duplicates, static call sites, command labels, named substitutions and persisted UI state.
# Traditional Chinese display review

The `zh-TW` interface prefers installed Taiwan sans-serif faces before Japanese and Simplified Chinese fallbacks. Body and button text remain 13 px; small Traditional Chinese labels use 12 px. Font and language changes discard the previous layout pass and refresh text styles before painting. Native screenshots at Windows display scales still need review before claiming complete visual quality.

