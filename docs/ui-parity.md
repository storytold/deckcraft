# UI parity with PowerPoint

> **Last reviewed:** 2026-10-14 · **Last updated:** 2026-10-18 · **Change:** minor (File › Print: printer list, local and network printers; previous: major, community PR wave 2026-10-14: 14 new dialogs (41 in all), Eyedropper, Export button, New Window, Grid Options, WordArt gallery and handles, Slide Sorter sections, Quick Access Toolbar, Format Background and Format Shape depth, rich-text clipboard; duplicated rows and headers folded; previous: minor, File tab / Backstage, #76) · **Target:** Microsoft PowerPoint for Mac 16.113 (Microsoft 365)

How DeckCraft looks and feels next to PowerPoint: the shell, the ribbon, menus, panes, dialogs,
direct manipulation, keyboard and precision. Observations of PowerPoint are kept locally in
`plan/powerpoint/01-observed-ui.md` and `menu-tree.txt` (gitignored, clean-room rules in
AGENTS.md §2). Dimension score: **~60% ready** (estimated), **30–50 h** to parity.

## Empty list paragraphs

While editing slide text, empty bulleted and numbered paragraphs show gray markers at the list's
indent, including the first click into a content placeholder and the paragraph after Enter.
Typing gives the marker its normal text color. These temporary markers are hidden when editing
ends and are omitted from thumbnails, slide shows and exports.

## Shell

| Element | PowerPoint for Mac | DeckCraft | Gap |
|---|---|---|---|
| Title bar: AutoSave, Home, Save, Undo/Redo, document title, Share, search | yes | yes, plus Export; no Share (cloud) | #71 macOS window controls misaligned |
| Native macOS menu bar (File, Edit, View, Insert, Format, Arrange, Tools, Slide Show, Window, Help) | yes | **no** (`menus::menu_tree()` is defined but not installed) | 3–5 h |
| Menu-bar items with a DeckCraft equivalent | 217 app-specific items | 128 matched by name (59%, heuristic lower bound) | See [target-app-parity.md](target-app-parity.md) |
| Ribbon tabs: Home, Insert, Draw, Design, Transitions, Animations, Slide Show, Record, Review, View | 10 | 10 | Record tab is a placeholder |
| Colour menus: theme and standard colours, hex field, Eyedropper | yes (plus a More Colors dialog) | yes: theme and standard colours, hex field and an Eyedropper that takes the colour drawn under the pointer on the slide (#367) | More Colors dialog; Eyedropper only takes colours from the slide, not the rest of the screen |
| Contextual tabs: Shape Format, Picture Format, Table Design, Table Layout, Chart Design, Chart Format, Video/Audio Format and Playback | yes | most; checklist P0 row partial | Chart Format, media tabs thinner |
| File tab (Backstage): Home, New, Open, Info, Save, Save As, Print, Share, Export, Close, Account, Options | yes | yes (#76), see [File tab](#file-tab-backstage); no Share (cloud) | Video/GIF/CD exports, templates beyond the built-in themes |
| Start screen: new, themes, templates, recent, open | yes, the same page as File › Home | yes: File › Home with Home, New, Open, Account and Options (#76) | |
| Status bar: slide counter, language, notes, comments, views, zoom slider | yes | yes | |
| Command search (Tell Me) | yes | command palette | |
| Light / dark appearance | follows the system | light or dark; doesn't follow the system (#41) | 1–2 h |
| One window per presentation, New Window, Arrange All | yes | New Window opens one more window on the same presentation (its own slide, live with edits); tabs stay in the first window; no Arrange All | Arrange All |
| Quick Access Toolbar customization | yes | yes: Customize menu (common commands, reset), right-click any ribbon button to add, right-click a toolbar button to remove; saved with the UI state | done |

## File tab (Backstage)

Observed black-box in PowerPoint for Windows (Microsoft 365, Version 2609, October 2026) on a
synthetic presentation. Clicking **File** (or Alt+F) replaces the ribbon, slides and status bar with
a full-window view; the title bar stays. A side menu 160 points wide, a step darker than the page,
lists Back (a circled arrow), Home, New, Open, Share, a rule, Info, Save, Save As, Print, Export,
Close and, at the bottom under a rule, Account and Options. Entries are 40 points tall with an
outline icon; the current page is drawn in the accent colour and outlined. Esc goes back. Alt+F opens
on Home with the keyboard in the side menu; Up and Down move through it and a page shows as soon as
its entry has the focus. PowerPoint's start screen is the same Home page, with only Home, New and
Open at the top. DeckCraft follows all of this (`crates/ui-egui/src/backstage.rs`), with the page and
menu colours taken from the theme tokens (menu `chrome`, page `card`).

| Page | PowerPoint | DeckCraft |
|---|---|---|
| Home | Prompt box (Copilot), New row of themes with More themes, Recent / Favorites / Shared with Me with a file search, More presentations | DeckCraft's mark, Blank Presentation and the first themes with More themes →, the first starter decks with More templates →, Recent with a file search and More presentations → (to Open). No Copilot prompt, Favorites or Shared with Me |
| New | Blank Presentation, template search with suggested searches, gallery of online templates and themes | Blank Presentation, search box, the built-in themes, six original starter decks (`file.new {template}`) and the sample deck (`file.new {theme}`). No online templates |
| Open | Recent, Shared with Me, OneDrive, This PC, Add a Place, Browse; Presentations / Folders; Recover Unsaved Presentations | Recent and Browse (`app.openDialog`), recent presentations with a search box (`file.open`), Recover Unsaved Presentations (`file.recovery.open`). No cloud places or Folders tab |
| Info | Name and path, Upload / Share / Copy path / Open file location; Protect, Inspect, Version History, Manage Presentation; Properties, Related Dates, Related People | Name and path, Copy path, Open file location; Inspect Presentation (Accessibility Checker), Manage Presentation (Revert); Size, Slides, Hidden slides and editable Title, Subject, Tags, Categories, Company, Comments, Author (`file.info`, `file.properties`). No Protect, Version History or dates |
| Save | Saves; a presentation never saved goes to Save As | Same (`file.save`) |
| Save As | Places, folder, file name, type, Save, More options | Type (PowerPoint, DeckCraft, PDF, Outline, PNG, JPEG) and Save, which opens the system Save dialog (web: Download). .pptx/.deckcraft take the new file; the others are copies (`file.saveAs`, `file.export`) |
| Print | Print and Copies, Printer, Print All / Selection / Current / Custom Range, Slides box, layout (Full Page Slides, Notes Pages, Outline, Handouts 1–9), sides, collation, colour, Edit Header & Footer; page preview with pager and zoom | Print, Print All Slides / Current Slide / Custom Range, Slides box (`1-3, 5`), Full Page Slides / Notes Pages / Handouts 1–9, Print Hidden Slides, Frame Slides, Edit Header & Footer; preview with pager. DeckCraft prints through a PDF (`file.export`): opened in the system viewer (web: downloaded), handed to a printer of this computer (copies, sides, Grayscale) or sent to a network printer (WS-Print, IPP: copies, sides, orientation, quality, colour, resolution, after a query and a confirmation). No Outline layout or collation |
| Share | Cloud sharing | Not offered |
| Export | Create PDF/XPS Document, Create a Video, Create an Animated GIF, Package Presentation for CD, Create Handouts, Change File Type | Create PDF Document, Create Handouts (a PDF), Change File Type (PNG with an optional transparent background, #784). No video, GIF or CD package |
| Close | Closes; asks Save / Don't Save / Cancel about unsaved changes | Same (`file.close` after the prompt); Ctrl+W asks too |
| Account | User information, sign-in, Office Background and Office Theme, product information, About | Appearance (light/dark, `view.dark`), product information, About DeckCraft. No accounts to sign in to |
| Options | Opens the PowerPoint Options dialog | Opens Preferences |

## Panes and dialogs

| Pane / dialog | PowerPoint | DeckCraft |
|---|---|---|
| Format Shape / Picture / Text pane (Fill & Line, Effects, Size & Properties, Picture, Text Box) | full | partial: fill (solid transparency follows the selection), gradient editor, picture/texture fill (source, tile, offsets, scale, alignment, mirror, transparency, rotate with shape), line (dash, compound, cap, join, begin/end arrows and sizes), shadow/glow/reflection details (colour, transparency, blur, angle, distance, size), soft edges, 3-D format and rotation (bevels, depth, contour, material, lighting, camera presets, X/Y/Z rotation), size (lock aspect ratio) |
| Slide Master | Insert Layout, Rename, Delete, Preserve, Insert Placeholder, Title and Footers | Insert Layout, Rename, Delete, Insert Placeholder, and Preserve, Title and Footers checkboxes (`master.preserve`, `master.placeholders`). A master thumbnail selects the master |
| Format Background pane | yes | yes: solid transparency; picture transparency, tile, offset, scale, alignment and mirror; Apply to All copies a picture. Stretch insets and rotate-with-shape are command parameters |
| Selection Pane | front-most first, groups fold, drag to restack, Bring Forward / Send Backward arrows, Show All / Hide All, eye, padlock (Lock), rename in place | same: drag restacks among siblings (a child stays in its group), Ctrl+click toggles, Shift+click selects a run, arrows / Home / End / F2 / Esc in the list, right-click menu |
| Comments pane (threads) | yes | yes: every slide, with Previous and Next on the Review tab |
| Animation Pane | list + advanced timeline | list, reorder, timing, an advanced timeline with draggable bars, play the current slide on the canvas; no preview from the selected effect |
| Design Ideas (Designer) | yes (cloud) | no |
| Dialogs | ~60 (Font, Paragraph, Bullets and Numbering, Find, Replace, Replace Fonts, Hyperlink, Action Settings, Header and Footer, Slide Size, Set Up Show, Custom Shows, Print, Page Setup, Insert Object, Symbol, Photo Album, Equation, Chart data sheet, SmartArt chooser, Preferences…) | 41 (`crates/ui-egui/src/dialogs.rs`): Font, Paragraph, Tabs, Bullets and Numbering, Find and Replace, Replace Fonts, Table, Slide Size, Size and Position, Header and Footer, Date and Time, Hyperlink (address, slide and ScreenTip follow the selection), Action Settings, Set Up Show, Zoom (percent or Fit, applies on OK), Comment, Alt Text (decorative follows the selection), Outline, Rename (section, shape, layout), About, Preferences, AutoCorrect, Grid Options, Export, Symbol, Photo Album (Edit Photo Album reopens this dialog), Paste Special, Equation (placeholder), Spelling, Accessibility, Language, Custom Shows, Chart Data, Trim Media (start, end and fades follow the selection), SmartArt chooser, Create New Theme Colors, Create New Theme Fonts, Quit, Notes Pages |
| Design ▸ Effects | named theme effect schemes | DeckCraft, Soft, Crisp, Lift, Glow and Plain (`design.effects`); colours and fonts stay |

## Direct manipulation

| Behaviour | PowerPoint | DeckCraft |
|---|---|---|
| Selection handles, rotation handle, yellow adjustment handles | yes | yes |
| Marquee, Shift/Cmd multi-select, Tab to cycle | yes | yes |
| Objects off the slide: drawn on the area around it in Normal view (full strength, master graphics too), selectable and movable there; clipped in the show, exports and thumbnails | yes | yes (#270) |
| Smart guides (edges, centres, equal spacing) | yes | yes |
| Snap to grid, grid options, drawing guides | yes | Grid Options (`view.gridOptions`): spacing, snap to grid, snap to other objects, display grid and guides; the on-screen grid uses that spacing |
| Nudge (arrows), fine nudge (Option+arrows) | yes | yes |
| Shift-constrained move, Option-drag to duplicate | yes | yes |
| Connector glue and rerouting | yes | yes |
| Edit points on freeforms | yes | partial: Edit Shape ▸ Edit Points turns any shape into a freeform; drag vertices and their Bézier handles, Ctrl/Cmd-click to add or delete, right-click for Smooth/Straight/Corner Point and Delete Point; no Straight/Curved Segment or Open/Close Path yet |
| WordArt adjust handles | yellow handles on transformed text | yes, one per adjust when the text has a warp; dragging sets that guide value |
| Crop handles, crop to shape, aspect ratio | yes | partial |
| Text caret, selection, double/triple click, drag-select | yes | yes |
| Exact size/position, lock aspect | yes | yes — Size and Position dialog (height, width, rotation, scale %, lock aspect, position from the top-left), one undo step; ribbon spinners still resize one side at a time |
| Drag the frame of a text box being edited to move it | yes | yes |
| Table cells: caret and selection in the cell, click into another cell, drag row and column borders | yes | yes; no cell-range selection by dragging across cells |
| Context menus on shapes, slides, text, thumbnails | yes | yes, shorter; a right click selects the shape under the pointer first |
| WordArt transform | Transform gallery | Shape Format ▸ Text Effects ▸ Transform (`format.textWarp`): No Transform, Follow Path and the other 36 presets. Adjust handles: see WordArt adjust handles |
| Rulers with indent markers and tab stops | yes | yes: indent markers and tab stops drag on the ruler (a click adds a tab, the corner selector picks its kind); Tabs dialog from Home › Paragraph and the Paragraph dialog |
| Drag and drop of files and between thumbnails | yes | yes |
| Slide Sorter sections: a bar per section, add, rename in place, collapse, drag the whole section (its slides move with it) | yes | yes |
| Image paste from the clipboard | yes | macOS pastes a picture when the paste carries no text (TIFF), and rich text copies and pastes as HTML (bold, italic, underline). Windows and Linux image paste stays #53 |
| Font menu: every installed family with its faces | yes | families yes; only Regular/Bold/Italic faces (#69) |

## Keyboard shortcuts

Every common PowerPoint for Mac shortcut is bound (checklist row done): new slide, duplicate,
group/ungroup, bold/italic/underline, alignment, font size, list levels, Format Painter, the Font
dialog (Cmd+T), show from start/current, view switching. Not yet: Cmd+F Find opening a find bar on the slide, Option+drag
precision without snapping, F6 pane cycling, ribbon KeyTips (Windows).

### Text editing keys

Observed black-box in PowerPoint for Windows (Microsoft 365, October 2026) on synthetic text, and
pinned by tests in `crates/model/src/words.rs`, `crates/model/src/edit.rs` and
`crates/engine/src/tests_keys.rs` (#268). Windows and Linux use Ctrl; the Mac uses Option for words
and Cmd for line and text ends (`Session::mac_keys`).

| Keys | Behaviour |
|---|---|
| Ctrl+Left / Right | Previous / next word stop. A stop is the start of every word, punctuation run, symbol run, tab, line break and CJK character, plus the paragraph's end; spaces belong to the word before them. `it's`, `baz-qux`, `a-`, `3.14`, `1,000`, `3:45` are one word; `e.g.`, `a_b`, `x@y.com`, `dogs'` are not. Every CJK ideograph, kana and Hangul syllable is a word. Combining marks and emoji sequences never split. With a selection: Right goes on from its end, Left from its start |
| Ctrl+Shift+Left / Right | Moves the selection's moving end by the same stops |
| Ctrl+Up / Down | Start of the paragraph (then of the previous one) / start of the next paragraph (end of the text in the last one); Shift extends |
| Ctrl+Backspace | Deletes back to the previous stop (word plus its trailing spaces); at a paragraph's start joins it to the previous paragraph |
| Ctrl+Delete | Inside a word: the rest of it. On spaces after a word: the spaces and the next word. At a word's start: the word and its spaces. Then smart spacing: spaces left before closing punctuation (`ab cd, ef` → `ab, ef`) or at the paragraph's end after a word go; sentence punctuation left touching a word gets a space (`cd...ef gh` → `cd... gh`). At a paragraph's end joins the next paragraph |
| Ctrl+Backspace / Delete with a selection | Leave the selection and delete the word part before / after its start |
| Right-to-left paragraph | Left and Right (with or without Ctrl) swap: Left goes forward in the text. Arabic or Hebrew words in a left-to-right paragraph move in logical order |
| Home / End | Start / end of the visual line. End on a wrapped line goes past its trailing space and the caret stays drawn at that line's end; End again stays; Home from there returns to the same line's start |
| Ctrl+Home / End | Start / end of the text; Shift extends |
| Ctrl+Shift+> / <, Ctrl+] / [ | Next / previous size in the font size list (18 → 20 → 24) |
| Ctrl+B / I / U, Ctrl+Space | With a caret strictly inside a word, the whole word (without trailing spaces); at a word's edge, only what is typed next |
| Ctrl+E / L / R / J | Center / left / right / justify the paragraphs in the selection |
| Shift+F3 | Cycles lowercase → Capitalize Each Word → UPPERCASE → lowercase; other mixes go to UPPERCASE first. A caret in a word changes that word and moves to its start |
| Shift+Enter | Line break |
| Double-click / triple-click | The word without its trailing spaces (a CJK character on its own) / the paragraph including its paragraph mark |

Quirks kept on purpose because PowerPoint has them: Ctrl+Left (not Ctrl+Right) also stops at
each no-break or ideographic space, and treats an emoji followed by punctuation as one word. Tab, Shift+Tab and
Alt+Shift+arrows in text are handled elsewhere (#78, #93).

## Visual fidelity

Colours, spacing, ribbon card and tab geometry follow the observations in
`plan/powerpoint/01-observed-ui.md`; the ribbon and menu icons are original (~240, drawn in code)
and must not imitate Microsoft's. Insert › Icons places Lucide stroke icons (ISC, release 1.56.0)
on the slide as shapes. The editing canvas, thumbnails and layout previews are drawn on the
physical pixel grid, so a Retina display does not blur them between pixels (#184). No pixel
comparison against PowerPoint screenshots has been run since 2026-10-07. Cross-app icon
consistency is open (#73, #56).

## Estimates

| Work | Hours |
|---|---|
| Native macOS menu bar from `menu_tree()`, filled out to PowerPoint's menu items | 3–5 |
| Format Shape pane depth | 6–10 |
| Missing dialogs (Print, Insert Object and the rest of PowerPoint's ~60) | 4–8 |
| Window chrome and system appearance (#41, #71) | 3–5 |
| Arrange All (New Window is one extra window) | 2–3 |
| Edit points UI, crop depth | 3–6 |
| Multiple windows | 3–5 |
| A screenshot comparison pass on the most-used tabs | 2–4 |
| **Total** | **~26–46** |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-18 | minor | File › Print lists the printers of this computer (read again each time the page opens; "No printers found" when there are none) and a network printer, with their settings |
| 2026-10-16 | minor | Export › Change File Type › PNG and the Export… dialog offer a transparent background (#784) |
| 2026-10-14 | minor | Gray list markers on empty paragraphs while editing; caret keeps the list indent before typing |
| 2026-10-14 | minor | Community PR wave folded: one row each for Format Shape, Dialogs (41, listed from `dialogs.rs`), Animation Pane (timeline is in), window, Quick Access Toolbar, Format Background, rulers, image paste, context menus, exact size and edit points; estimates re-totalled (~26–46 h) |
| 2026-10-13 | trivial | Eyedropper in the colour menus (#367) |
| 2026-10-12 | minor | Title bar: an Export button next to Save opens the Export dialog, a quick-access shortcut beside File › Export |
| 2026-10-12 | minor | A right click selects the shape under the pointer before its context menu opens, as in PowerPoint; edited text keeps its selection and gets text items |
| 2026-10-13 | minor | WordArt Transform gallery on Shape Format ▸ Text Effects; `format.textWarp` applies a preset or clears it. Adjust handles remain open |
| 2026-10-14 | minor | Insert › Icons is a searchable gallery of Lucide stroke icons inserted as shapes. Ribbon chrome stays original drawings |
| 2026-10-14 | minor | Size and Position dialog on Shape Format › Size and the shape context menu. Breadth stays 79%; UI ready stays ~60% |
| 2026-10-14 | trivial | Comments pane lists every slide; Review ▸ Previous and Next walk the deck |
| 2026-10-13 | minor | View › New Window: a second window on the same presentation, with its own slide; Arrange All is still missing |
| 2026-10-14 | minor | File › Home and New show six original starter decks; online templates are still absent |
| 2026-10-14 | minor | Slide Sorter draws a bar per section: Add Section, rename in place, collapse, and drag the bar to move the section and its slides |
| 2026-10-14 | trivial | Photo Album dialog (28 dialogs). Breadth stays 79%, UI ~60% |
| 2026-10-14 | trivial | Edit Photo Album reuses the Photo Album dialog (28 dialogs). Breadth stays 79%, UI ~60% |
| 2026-10-14 | minor | Design › Colors › Customize Colors… opens Create New Theme Colors (the open master's twelve theme colours; dialogs 27 → 28) |
| 2026-10-14 | minor | Design › Fonts › Customize Fonts… opens Create New Theme Fonts (heading and body latin on the open master; dialogs 27 → 28) |
| 2026-10-14 | trivial | Alt Text dialog: Mark as decorative follows the selection |
| 2026-10-14 | trivial | Hyperlink dialog: address, email, slide and ScreenTip follow the selection |
| 2026-10-14 | minor | Insert › Date and Time: format list, language, Update automatically. 28 dialogs. Languages are the ones the date field can compute (English, plus numeric dates for en-GB, fr-FR, ja-JP, sv-SE) |
| 2026-10-14 | trivial | Zoom dialog: percent or Fit, applied on OK. View ▸ Zoom opens it; the arrow keeps the presets |
| 2026-10-13 | trivial | Command search no longer offers an Export command that fails without a path (#79) |
| 2026-10-14 | minor | Design ▸ Effects applies a named effect scheme (fills, lines and effects). Not a new dialog; dialogs stay 27 |
| 2026-10-14 | minor | Slide Master: Preserve, and Title and Footers placeholders, on the open master or layout |
| 2026-10-14 | trivial | Trim Media dialog: start, end and fades follow the selected clip |
| 2026-10-12 | minor | Dragging the frame of a text box being edited moves it, as in PowerPoint |
| 2026-10-12 | minor | Table cells: caret and selection shown in the edited cell, clicks place the caret in the clicked cell, row and column borders drag to resize |
| 2026-10-14 | minor | Format Background pane: solid transparency, picture transparency, tile, offset, scale, alignment, mirror, and Apply to All copying a picture. Not a new dialog; the dialog count stays 27 |
| 2026-10-13 | trivial | Editor slide rasters sit on the physical pixel grid (#184) |
| 2026-10-14 | minor | Copy and paste rich text (HTML: bold, italic, underline) with other apps |
| 2026-10-13 | minor | Animations ▸ Preview and Animation Pane ▸ Play All play the current slide on the editor canvas |
| 2026-10-14 | trivial | Format Shape Fill transparency slider follows the selected solid fill and does not clear it on focus loss |
| 2026-10-14 | minor | Paragraph dialog from the Home › Paragraph launcher; Tabs… opens a standalone Tabs dialog |
| 2026-10-14 | minor | Font dialog from Home › Font, the shape menu and Cmd+T |
| 2026-10-13 | minor | Grid Options dialog: spacing, snap, and whether gridlines and guides are drawn; the on-screen grid uses that spacing |
| 2026-10-13 | minor | WordArt adjust handles on a selected text warp; dragging one sets that `prstTxWarp` adjust |
| 2026-10-12 | minor | File tab (Backstage) observed in PowerPoint for Windows and matched (#76); the start screen is File › Home |
| 2026-10-12 | minor | Off-slide objects drawn and editable around the slide in Normal view (#270) |
| 2026-10-12 | minor | Selection Pane matched to PowerPoint (#133): drag and drop, arrows, groups, eye, Lock |
| 2026-10-12 | minor | Text editing keys: Ctrl+arrows, Ctrl+Backspace/Delete, Ctrl+Up/Down, Home/End on wrapped lines, Shift+F3, font size keys, as observed in PowerPoint for Windows (#268) |
| 2026-10-11 | minor | Tab in text boxes keeps editing (#78) |
| 2026-10-10 | major | Created: shell, panes and dialogs, direct manipulation, shortcuts and visual fidelity against PowerPoint 16.113 |
