# UI parity with PowerPoint

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-10 · **Change:** major (created; ribbon, menus, panes, dialogs and interaction compared with PowerPoint 16.113 observations in `plan/powerpoint/`) · **Target:** Microsoft PowerPoint for Mac 16.113 (Microsoft 365)

How DeckCraft looks and feels next to PowerPoint: the shell, the ribbon, menus, panes, dialogs,
direct manipulation, keyboard and precision. Observations of PowerPoint are kept locally in
`plan/powerpoint/01-observed-ui.md` and `menu-tree.txt` (gitignored, clean-room rules in
AGENTS.md §2). Dimension score: **~60% ready** (estimated), **30–50 h** to parity.

## Shell

| Element | PowerPoint for Mac | DeckCraft | Gap |
|---|---|---|---|
| Title bar: AutoSave, Home, Save, Undo/Redo, document title, Share, search | yes | yes, no Share (cloud) | #71 macOS window controls misaligned |
| Native macOS menu bar (File, Edit, View, Insert, Format, Arrange, Tools, Slide Show, Window, Help) | yes | **no** (`menus::menu_tree()` is defined but not installed) | 3–5 h |
| Menu-bar items with a DeckCraft equivalent | 217 app-specific items | 128 matched by name (59%, heuristic lower bound) | See [target-app-parity.md](target-app-parity.md) |
| Ribbon tabs: Home, Insert, Draw, Design, Transitions, Animations, Slide Show, Record, Review, View | 10 | 10 | Record tab is a placeholder |
| Contextual tabs: Shape Format, Picture Format, Table Design, Table Layout, Chart Design, Chart Format, Video/Audio Format and Playback | yes | most; checklist P0 row partial | Chart Format, media tabs thinner |
| Start screen: new, themes, templates, recent, open | yes | yes, thin (#17 broken layout, #76 missing overlay) | 3–5 h |
| Status bar: slide counter, language, notes, comments, views, zoom slider | yes | yes | |
| Command search (Tell Me) | yes | command palette | #79 default entry fails |
| Light / dark appearance | follows the system | light or dark; doesn't follow the system (#41) | 1–2 h |
| One window per presentation, New Window, Arrange All | yes | tabs in one window; no New Window | 3–5 h |
| Quick Access Toolbar customization | yes | no | P1 partial |

## Panes and dialogs

| Pane / dialog | PowerPoint | DeckCraft |
|---|---|---|
| Format Shape / Picture / Text pane (Fill & Line, Effects, Size & Properties, Picture, Text Box) | full | partial: fill, gradient editor, line, some effects, size; no 3-D, thin picture/texture options |
| Format Background pane | yes | yes |
| Selection Pane | yes | yes |
| Animation Pane | list + advanced timeline | list, reorder, timing; no timeline |
| Comments pane (threads) | yes | yes |
| Design Ideas (Designer) | yes (cloud) | no |
| Dialogs | ~60 (Font, Paragraph, Bullets and Numbering, Find, Replace, Replace Fonts, Hyperlink, Action Settings, Header and Footer, Slide Size, Set Up Show, Custom Shows, Print, Page Setup, Insert Object, Symbol, Equation, Chart data sheet, SmartArt chooser, Preferences…) | 27 (`crates/ui-egui/src/dialogs.rs`): Table, Slide Size, Header and Footer, Hyperlink, Set Up Show, Zoom, Comment, Alt Text, Outline, Rename (section, shape, layout), Paragraph, About, Preferences, Export, Symbol, Equation (placeholder), Spelling, Accessibility, Language, Custom Shows, Chart Data, Trim Media, SmartArt chooser, Quit, Notes Pages |

## Direct manipulation

| Behaviour | PowerPoint | DeckCraft |
|---|---|---|
| Selection handles, rotation handle, yellow adjustment handles | yes | yes |
| Marquee, Shift/Cmd multi-select, Tab to cycle | yes | yes |
| Smart guides (edges, centres, equal spacing) | yes | yes |
| Snap to grid, grid options, drawing guides | yes | gridlines, guides and snap to grid (engine setting); no Grid Options dialog or menu toggle |
| Nudge (arrows), fine nudge (Option+arrows) | yes | yes |
| Shift-constrained move, Option-drag to duplicate | yes | yes |
| Connector glue and rerouting | yes | yes |
| Edit points on freeforms | yes | **no** |
| Crop handles, crop to shape, aspect ratio | yes | partial |
| Text caret, selection, double/triple click, drag-select | yes | yes; Tab in text boxes ends editing (#78) |
| Exact size/position, lock aspect | yes | yes |
| Rulers with indent markers and tab stops | yes | rulers yes; tab stops partial |
| Context menus on shapes, slides, text, thumbnails | yes | yes, shorter |
| Drag and drop of files and between thumbnails | yes | yes |
| Image paste from the clipboard | yes | macOS yes; Windows and Linux no (#53) |
| Font menu: every installed family with its faces | yes | families yes; only Regular/Bold/Italic faces (#69) |

## Keyboard shortcuts

Every common PowerPoint for Mac shortcut is bound (checklist row done): new slide, duplicate,
group/ungroup, bold/italic/underline, alignment, font size, list levels, Format Painter, show from
start/current, view switching. Not yet: Cmd+F Find opening a find bar on the slide, Option+drag
precision without snapping, F6 pane cycling, ribbon KeyTips (Windows).

## Visual fidelity

Colours, spacing, ribbon card and tab geometry follow the observations in
`plan/powerpoint/01-observed-ui.md`; icons are original (~240, drawn in code or Lucide) and must not
imitate Microsoft's. No pixel comparison against PowerPoint screenshots has been run since
2026-10-07. Cross-app icon consistency is open (#73, #56).

## Estimates

| Work | Hours |
|---|---|
| Native macOS menu bar from `menu_tree()`, filled out to PowerPoint's menu items | 3–5 |
| Format Shape pane depth | 6–10 |
| Missing dialogs (Font, Bullets and Numbering, Find/Replace, Replace Fonts, Action Settings, Print, Insert Object) | 8–14 |
| Start screen, window chrome, system appearance, palette fixes (#17, #41, #71, #76, #79) | 4–7 |
| Grid Options, edit points UI, crop depth | 4–7 |
| Multiple windows | 3–5 |
| A screenshot comparison pass on the most-used tabs | 2–4 |
| **Total** | **~30–52** |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-10 | major | Created: shell, panes and dialogs, direct manipulation, shortcuts and visual fidelity against PowerPoint 16.113 |
