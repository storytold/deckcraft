# Feature naming

DeckCraft uses **Diagram**, **Diagram Design**, and **Diagram Styles** for its diagram feature, and **Decorative Text** / **Decorative Text Styles** for styled text. Menus, dialogs, tooltips, command labels, errors and newly generated object names use these names.

## Compatibility boundary

The existing `smartArt.*`, `insert.smartArt`, `insert.wordArt`, dialog and icon IDs remain stable. The persisted/control-channel tab key `SmartArt Design` still opens the same tab; only its displayed and accessible label is Diagram Design. Dispatch does not depend on the new display text.

Existing document-owned object names are not rewritten. PPTX diagram parts, DrawingML names, public data types, import/export code and attribution remain unchanged.

DeckCraft currently authors these interface strings directly in English. Shared DeckCraft catalogs exist in craft-i18n, but this change does not add a catalog loader. Before enabling that integration, catalog keys must include the new Diagram and Decorative Text labels rather than only their retired English names. Diagram/format round-trip tests, UI tests and visual checks remain part of application verification.

## Regression coverage

The standard-library-only `display_names` integration tests scan production Rust string literals for the retired display names and preserve explicit compatibility exceptions. They also test identifier, comment and raw-string handling. These focused checks do not replace application or visual testing.
