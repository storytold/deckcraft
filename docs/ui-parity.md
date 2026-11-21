# UI parity with PowerPoint

> **Last reviewed:** 2026-11-02 · **Last updated:** 2026-11-10 · **Change:** trivial (File › Export › Change File Type and Save As export every slide as PNG or JPEG, #2019; scores unchanged) · **Target:** Microsoft PowerPoint for Mac 16.113 (Microsoft 365)
> **Last reviewed:** 2026-10-14 · **Last updated:** 2026-11-01 · **Change:** trivial (locked No Fill and no-outline click-through regression coverage added; No Fill hit-testing follows resolved fill instead of the presence of a style (#1621); scores unchanged) · **Target:** Microsoft PowerPoint for Mac 16.113 (Microsoft 365)
> **Last reviewed:** 2026-10-14 · **Last updated:** 2026-10-26 · **Change:** trivial (dragging several selected slides moves them all; previous: trivial, Edit Points: Shift, Ctrl/Cmd or Alt/Option on a handle drag sets the point type; previous: trivial, scroll bars on the zoomed slide pane, #1157; previous: major, community PR wave 2026-10-14: 14 new dialogs (41 in all), Eyedropper, Export button, New Window, Grid Options, WordArt gallery and handles, Slide Sorter sections, Quick Access Toolbar, Format Background and Format Shape depth, rich-text clipboard; duplicated rows and headers folded; previous: minor, File tab / Backstage, #76) · **Target:** Microsoft PowerPoint for Mac 16.113 (Microsoft 365)
> **Last reviewed:** 2026-10-14 · **Last updated:** 2026-10-22 · **Change:** trivial (group fill and outline edits reach members, #1079; scores unchanged) · **Target:** Microsoft PowerPoint for Mac 16.113 (Microsoft 365)
> **Last reviewed:** 2026-10-14 · **Last updated:** 2026-10-24 · **Change:** trivial (Edit Points: Shift, Ctrl/Cmd or Alt/Option on a handle drag sets the point type; previous: trivial, scroll bars on the zoomed slide pane, #1157; previous: major, community PR wave 2026-10-14: 14 new dialogs (41 in all), Eyedropper, Export button, New Window, Grid Options, WordArt gallery and handles, Slide Sorter sections, Quick Access Toolbar, Format Background and Format Shape depth, rich-text clipboard; duplicated rows and headers folded; previous: minor, File tab / Backstage, #76) · **Target:** Microsoft PowerPoint for Mac 16.113 (Microsoft 365)

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
| Start screen: new, themes, templates, recent, open | yes, the same page as File › Home | yes: File › Home with Home, New, Open, About and Options (#76) | |
| Status bar: slide counter, language, notes, comments, views, zoom slider | yes | yes | |
| Command search (Tell Me) | yes | command palette | |
| Light / dark appearance | follows the system | light or dark; doesn't follow the system (#41) | 1–2 h |
| One window per presentation, New Window, Arrange All | yes | New Window opens one more window on the same presentation (its own slide, live with edits); a row under the title bar shows a tab per open presentation (name, a dot for unsaved changes, close; middle-click closes; + for a new one; ⌃Tab / ⌃⇧Tab and ⌘` switch); no Arrange All | Arrange All |
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
Open at the top. DeckCraft follows all of this (`crates/ui-egui/src/backstage.rs`), except that
Account is About, with the page and
menu colours taken from the theme tokens (menu `chrome`, page `card`).

| Page | PowerPoint | DeckCraft |
|---|---|---|
| Home | Prompt box (Copilot), New row of themes with More themes, Recent / Favorites / Shared with Me with a file search, More presentations | DeckCraft's mark, Blank Presentation and the first themes with More themes →, the first starter decks with More templates →, Recent with a file search and More presentations → (to Open). No Copilot prompt, Favorites or Shared with Me |
| New | Blank Presentation, template search with suggested searches, gallery of online templates and themes | Blank Presentation, search box, the built-in themes, six original starter decks (`file.new {template}`) and the sample deck (`file.new {theme}`). No online templates |
| Open | Recent, Shared with Me, OneDrive, This PC, Add a Place, Browse; Presentations / Folders; Recover Unsaved Presentations | Recent and Browse (`app.openDialog`), recent presentations with a search box (`file.open`), Recover Unsaved Presentations (`file.recovery.open`). No cloud places or Folders tab |
| Info | Name and path, Upload / Share / Copy path / Open file location; Protect, Inspect, Version History, Manage Presentation; Properties, Related Dates, Related People | Name and path, Copy path, Open file location; Inspect Presentation (Accessibility Checker), Manage Presentation (Revert); Size, Slides, Hidden slides and editable Title, Subject, Tags, Categories, Company, Comments, Author (`file.info`, `file.properties`). No Protect, Version History or dates |
| Save | Saves; a presentation never saved goes to Save As | Same (`file.save`) |
| Save As | Places, folder, file name, type, Save, More options | Type (PowerPoint, DeckCraft, PDF, Outline, PNG, JPEG, and PNG or JPEG of every slide, #2019) and Save, which opens the system Save dialog (web: Download). .pptx/.deckcraft take the new file; the others are copies (`file.saveAs`, `file.export`) |
| Print | Print and Copies, Printer, Print All / Selection / Current / Custom Range, Slides box, layout (Full Page Slides, Notes Pages, Outline, Handouts 1–9), sides, collation, colour, Edit Header & Footer; page preview with pager and zoom | Print, Print All Slides / Current Slide / Custom Range, Slides box (`1-3, 5`), Full Page Slides / Notes Pages / Handouts 1–9, Print Hidden Slides, Frame Slides, Edit Header & Footer; preview with pager. DeckCraft prints through a PDF (`file.export`) opened in the system viewer, which has the printers (web: downloaded). No printer list, copies, sides or colour |
| Share | Cloud sharing | Not offered |
| Export | Create PDF/XPS Document, Create a Video, Create an Animated GIF, Package Presentation for CD, Create Handouts, Change File Type | Create PDF Document, Create Handouts (a PDF), Change File Type (PNG or JPEG of the current slide or of every slide, #2019; PNG with an optional transparent background, #784). No video, GIF or CD package |
| Close | Closes; asks Save / Don't Save / Cancel about unsaved changes | Same (`file.close` after the prompt); Ctrl+W asks too |
| Account | User information, sign-in, Office Background and Office Theme, product information, About | About: product information, project links, contributor/model credits and appearance (light/dark, `view.dark`). No Account entry |
| Options | Opens the PowerPoint Options dialog | Opens Preferences |

## Panes and dialogs

| Pane / dialog | PowerPoint | DeckCraft |
|---|---|---|
| Format Shape / Picture / Text pane (Fill & Line, Effects, Size & Properties, Picture, Text Box) | full | partial: fill (solid transparency follows the selection), gradient editor, picture/texture fill (source, tile, offsets, scale, alignment, mirror, transparency, rotate with shape), pattern fill (gallery of the 48 presets PowerPoint offers, foreground and background colors), slide background fill, line (dash, compound, cap, join, begin/end arrows and sizes), shadow/glow/reflection details (colour, transparency, blur, angle, distance, size), soft edges, 3-D format and rotation (bevels, depth, contour, material, lighting, camera presets, X/Y/Z rotation), size (lock aspect ratio) |
| Slide Master | Insert Layout, Rename, Delete, Preserve, Insert Placeholder, Title and Footers | Insert Layout, Rename, Delete, Insert Placeholder, and Preserve, Title and Footers checkboxes (`master.preserve`, `master.placeholders`). A master thumbnail selects the master |
| Format Background pane | yes | yes: solid transparency; picture transparency, tile, offset, scale, alignment and mirror; Apply to All copies a picture. Stretch insets and rotate-with-shape are command parameters |
| Selection Pane | front-most first, groups fold, drag to restack, Bring Forward / Send Backward arrows, Show All / Hide All, eye, padlock (Lock), rename in place | same: drag restacks among siblings (a child stays in its group), Ctrl+click toggles, Shift+click selects a run, arrows / Home / End / F2 / Esc in the list, right-click menu |
| Comments pane (threads) | yes | yes: every slide, with Previous and Next on the Review tab |
| Animation Pane | list + advanced timeline | list, reorder, timing, an advanced timeline with draggable bars, play the current slide on the canvas, and Play From starts at the selected effect |
| Design Ideas (Designer) | yes (cloud) | no |
| Dialogs | ~60 (Font, Paragraph, Bullets and Numbering, Find, Replace, Replace Fonts, Hyperlink, Action Settings, Header and Footer, Slide Size, Set Up Show, Custom Shows, Print, Page Setup, Insert Object, Symbol, Photo Album, Equation, Chart data sheet, SmartArt chooser, Preferences…) | 42 (`crates/ui-egui/src/dialogs.rs`): Font, Paragraph, Tabs, Columns (number and spacing; Home › Columns › More Columns… and Format Shape › Text Box › Columns…), Bullets and Numbering, Find and Replace, Replace Fonts, Table, Slide Size, Size and Position, Header and Footer, Date and Time, Hyperlink (address, slide and ScreenTip follow the selection), Action Settings, Set Up Show, Zoom (percent or Fit, applies on OK), Comment, Alt Text (decorative follows the selection), Outline, Rename (section, shape, layout), About, Preferences, AutoCorrect, Grid Options, Export, Symbol, Photo Album (Edit Photo Album reopens this dialog), Paste Special, Equation (placeholder), Spelling, Accessibility, Language, Custom Shows, Chart Data, Trim Media (start, end and fades follow the selection), SmartArt chooser, Create New Theme Colors, Create New Theme Fonts, Quit, Notes Pages |
| Design ▸ Effects | named theme effect schemes | DeckCraft, Soft, Crisp, Lift, Glow and Plain (`design.effects`); colours and fonts stay |
| Dialog look (after PhotoCraft) | Every dialog is a modal card (the window behind barely dimmed) with its title in semibold over a hairline; it opens centred and stays where its title is dragged; Esc cancels it (the topmost, with no menu open), a click outside does nothing. Buttons follow the platform's order (macOS: other answers, Cancel, the default last and filled with the accent); Enter is the default except where several lines are typed (⌘Enter there). Fields are the panel's grey ones: right-aligned grey labels, number fields with units, arithmetic and ↑ / ↓, dropdowns, section headings, real checkboxes. Header and Footer has its sub-fields under their boxes; Export has the format list, options and a preview of the slide; the save prompts show the app's mark | |

## Direct manipulation

| Behaviour | PowerPoint | DeckCraft |
|---|---|---|
| Selection handles, rotation handle, yellow adjustment handles | yes | yes; an empty No Fill shape passes interior clicks to underlying objects while its outline and handles remain selectable. Explicit, style and theme fills resolve as rendered; tests also cover locked outline targets and empty interiors without an outline (#1621) |
| Marquee, Shift/Cmd multi-select, Tab to cycle | yes | yes |
| Objects off the slide: drawn on the area around it in Normal view (full strength, master graphics too), selectable and movable there; clipped in the show, exports and thumbnails | yes | yes (#270) |
| Scroll bars on the slide pane when the zoomed slide is larger than the window | yes | yes, on each axis that overflows: drag the thumb or click the track; the wheel and trackpad still pan. The vertical bar does not turn the page (#1157) |
| Smart guides (edges, centres, equal spacing) | yes | yes |
| Snap to grid, grid options, drawing guides | yes | Grid Options (`view.gridOptions`): spacing, snap to grid, snap to other objects, display grid and guides; the on-screen grid uses that spacing |
| Nudge (arrows), fine nudge (Option+arrows) | yes | yes |
| Shift-constrained move, Option-drag to duplicate | yes | yes |
| Connector glue and rerouting | yes | yes |
| Edit points on freeforms | yes | partial: Edit Shape ▸ Edit Points turns any shape into a freeform; drag vertices and their Bézier handles (hold Shift, Ctrl/Cmd or Alt/Option while dragging a handle for a smooth, straight or corner point), Ctrl/Cmd-click to add or delete, right-click a point for Smooth/Straight/Corner Point, Delete Point and Open/Close Path, or a segment for Straight/Curved Segment, Add Point, Delete Segment (grayed out on an open outline) and Open/Close Path |
| WordArt adjust handles | yellow handles on transformed text | yes, one per adjust when the text has a warp; dragging sets that guide value |
| Crop handles, crop to shape, aspect ratio | yes | partial |
| Text caret, selection, double/triple click, drag-select | yes | yes |
| Exact size/position, lock aspect | yes | yes — Size and Position dialog (height, width, rotation, scale %, lock aspect, position from the top-left), one undo step; ribbon spinners still resize one side at a time |
| Drag the frame of a text box being edited to move it | yes | yes |
| Click the frame of a text box or placeholder to select it whole (then move, duplicate or delete it) | yes | yes; a click inside takes the caret |
| Table cells: caret and selection in the cell, click into another cell, drag row and column borders, drag across cells to select a range | yes | yes: a drag from the cell being edited into another cell selects the range (merged cells whole), Shift+arrow grows or shrinks it, formatting and Delete act on the selected cells |
| Context menus on shapes, slides, text, thumbnails | yes | yes, shorter; a right click selects the shape under the pointer first |
| Group fill and outline | formats the members | Shape Fill and Shape Outline reach nested members in one undo step; a transparency-only fill edit retains each member's colour (#1079) |
| WordArt transform | Transform gallery | Shape Format ▸ Text Effects ▸ Transform (`format.textWarp`): No Transform, Follow Path and the other 36 presets. Adjust handles: see WordArt adjust handles |
| Rulers with indent markers and tab stops | yes | yes: indent markers and tab stops drag on the ruler (a click adds a tab, the corner selector picks its kind); Tabs dialog from Home › Paragraph and the Paragraph dialog |
| Drag and drop of files and between thumbnails | yes | yes; several selected slides move together as one run, in one undo step, in the thumbnail pane and the Slide Sorter |
| Slide Sorter sections: a bar per section, add, rename in place, collapse, drag the whole section (its slides move with it) | yes | yes |
| Image paste from the clipboard | yes | macOS pastes a picture when the paste carries no text (TIFF), and rich text copies and pastes as HTML (bold, italic, underline). Windows image paste stays open (#1414) |
| Font menu: every installed family with its faces | yes | families yes; only Regular/Bold/Italic faces (#69) |

## Keyboard shortcuts

Every common PowerPoint for Mac shortcut is bound (checklist row done): new slide, duplicate,
group/ungroup, bold/italic/underline, alignment, font size, list levels, Format Painter, the Font
dialog (Cmd+T), show from start/current, view switching. Mac-style object shortcuts, as in Keynote and canvas editors, work
beside them in both layouts (`menus::mac_shortcut`): ⌘G group, ⇧⌘G ungroup, ⌥⌘] bring to front,
⌥⌘[ send to back, ⇧⌘L lock / unlock and ⇧⌘H hide / show the selection (any unlocked or shown
layer makes them all locked or hidden), ⇧⌘R paste to replace (Edit menu too: the clipboard's
image, else what was copied here, takes each selected object's place, centred on it and at its
place in the stack, in one undo step), ⇧⌘Z redo; ⇧⌘V and ⌥⇧⌘V paste text without its
formatting. A click on a thumbnail gives the slide list the keyboard (its selected rows are
tinted) until a click lands elsewhere or a shape is selected (`thumbs::slide_key`): ⌘D duplicates
the selected slides, Delete removes them, ⇧⌘H hides or shows them (⌘H stays the Mac's Hide app),
⌘↑ / ⌘↓ (or ⌘← / ⌘→) move them one place and ⇧⌘↑ / ⇧⌘↓ to the start or end, ↑ / ↓ go to the
previous or next slide, ⇧↑ / ⇧↓ extend the selection, ⌘A selects every slide, and ⌘C / ⌘X copy
or cut slides. With nothing selected on the slide, Delete and ⌘X do nothing (the slide isn't
removed). ⌘V pastes an image on the clipboard (a screenshot, a picture or an image file copied in
Finder) as a picture. Not yet: Cmd+F Find opening a find bar on the slide, Option+drag
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
| 2026-11-10 | trivial | Change File Type and Save As export every slide as PNG or JPEG (#2019); scores unchanged |
| 2026-11-02 | trivial | File › About replaces Account; product links, credits and appearance retained |
| 2026-10-31 | trivial | No Fill hit-testing follows resolved fill instead of the presence of a style (#1621); scores unchanged |
| 2026-10-29 | trivial | Picture Format: the Picture Styles row shows as many styles as fit beside the groups after it (at least two) and a More arrow with all 24, as the Transitions and Animations galleries do; Crop and Size are in view in the default window (#1232) |
| 2026-10-26 | trivial | Thumbnail pane and Slide Sorter: dragging one of several selected slides moves the whole selection |
| 2026-10-24 | trivial | Table cells: dragging across cells selects a range of cells |
| 2026-10-24 | trivial | Edit Points: Shift, Ctrl/Cmd or Alt/Option held while dragging a handle makes the point smooth, straight or corner |
| 2026-10-24 | trivial | Columns dialog: the number of text columns and the spacing between them. Dialog count 41 → 42 |
| 2026-10-23 | trivial | Scroll bars on the zoomed slide pane (#1157) |
| 2026-10-23 | minor | Shape Fill and Shape Outline (Home and Shape Format): the button applies the last colour picked from its menu, Accent 1 and Text 1 until one is picked. Line weight and dashes are not changed by the button; Font Color and Highlight stay as they were (#1080) |
| 2026-10-22 | trivial | Group fill and outline commands reach members, including nested groups; selection and single-step undo are preserved (#1079). The existing Arrange / Group checklist score stays D |
| 2026-10-16 | minor | Export › Change File Type › PNG and the Export… dialog offer a transparent background (#784) |
| 2026-10-15 | minor | Edit Points: the right-click menu has Straight/Curved Segment on a segment and Open/Close Path on a point |
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
| 2026-10-15 | minor | Clicking the frame of a text box or placeholder selects the box whole instead of starting text entry (#787) |
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

## Canvas shell (in progress, branch `feat/canvas-shell`)

A second window layout beside the PowerPoint-style one, for people who prefer a canvas-first
editor. Choose it in Preferences (Layout: Ribbon / Canvas), with `view.shell {shell: canvas}`, the
control channel's `ui.set {shell}`, or `deckcraft --shell canvas` (`DECKCRAFT_SHELL`). The desktop
app opens in the canvas layout until a layout is saved in its settings; the library (tests, the web
build) keeps the ribbon as its default. Both layouts drive the same commands, so files, undo and PPTX output don't change
with the layout (`crates/ui-egui/src/canvas_shell.rs`).

| Part | Landed (phases 1–5) | Later |
|---|---|---|
| Left sidebar | New slide row with a hide-panel button (the presentation's name is in the title bar, its commands in the menus; the title bar has no quick-access buttons in this layout); Slide Master view in the sidebar (from the panel's Masters toggle: the master and its layouts, Close masters, + for a new layout, and a layout's menu to rename, add or delete; `view.slideMaster` with `layout: null` opens the master itself), New slide with layouts, +, slide list with sections, Layers (the Selection pane's list: rename, restack, eye, lock; as in canvas editors the eye and lock show only while a row is hovered or when it is hidden (a closed eye, the name greyed) or locked; the selected row is highlighted in light blue, the hovered one in grey; ⌥-click on a chevron folds or opens the groups inside too; each row has its kind's icon: group, text, shape, line, picture, table, chart, video, audio). Locked layers work as in canvas editors: clicks on the slide pass through them (engine pref `lockedClickThrough`, on in this layout only) and they are selected from Layers; the ribbon keeps PowerPoint's lock (picked, formatted, not moved) | |
| Inspector | (The views are in the floating toolbar, exclusive: Single slide, Grid and Masters (clicking Masters again goes back to the slide), then Play. The panel has Design | Animate tabs at its top, as canvas editors' Design | Prototype; Animate is greyed out in the masters and the grid, and going there from it goes back to Design. The toolbar shows in every view; the states strip shows while the slide has states or the Animate panel is open; notes are in View › Presenter Notes, the command palette on ⇧⌘P.) the zoom menu is at the end of the floating toolbar and Export in the presentation's menu. A layer with text has Text and Shape tabs (text boxes and placeholders open on Text, shapes on Shape; each kind remembers its tab): Text holds Typography (with the text colour) and Text box (resizing, padding, shrink text to fit, wrap, columns); Shape holds position, size, appearance, fill, stroke (with dash), a picture's Image adjustments, effects, alt text and export. The old More options panel is gone. With a selection, Design is a property editor laid out like a canvas editor's design panel (`crates/ui-egui/src/props.rs`): a header naming the layer (Text, Rectangle, Image, Group; a slide's placeholder by its kind, "Title", "Footer"; in Slide Master view "Footer placeholder") with Link (Ungroup for a group), in Slide Master view the placeholder tag menu, and More options. Slides take their layout's placeholders, so the tag menu is only in Slide Master view (`placeholder.assign` still takes a slide's box for scripts). The Layers list shows each placeholder's kind as a pill after its name; Position (alignment, distribute, X, Y, rotation, rotate 90°, flips); Layout (text resizing: auto width, auto height, fixed; W, H, constrain proportions; a text box's padding, horizontal and vertical or each side, `format.margins`); Appearance (show / hide, opacity, corner radius of rounded rectangles); Typography: text that follows a style shows the style chip (opens the styles; detach button at its end) and alignment, with bold / italic / underline / strikethrough and lists behind the tune button; text with its own font, size, weight or italic (or detached) shows font, weight, size, line height and letter spacing (points) in grey fields and a Text styles button. The styles menu has an Update button on each style: it sets the style to the selected text's font, weight, size, italic and line height (as the style editor does, so text following it changes too) and links the text to it, in one undo step. Applying a style keeps the text's own colour, and detaching keeps its look (the shown values become its own; this session's detached text isn't changed by style edits). A shape's text has a Color row (a text box's colour is its Fill). While typing with a bare caret, the panel's text formatting reaches the whole box; Fill (hex, opacity, solid / gradient / image / none; a text box's Fill is its text colour); Stroke (colour, width, add / remove); Effects (drop shadow, inner shadow, glow, soft edges, reflection: add / remove); Export (slide as PNG or JPEG, deck as PDF). The panel is drawn as a canvas editor's: Inter (bundled in `assets/fonts`), semibold section titles, small grey captions, light grey fields and segmented controls with the chosen segment raised in white, and one-colour icons. Fill and Stroke rows put their buttons (fill type, remove) after the colour field. Numbers sit in grey fields in pixels without a unit; drag a field's prefix to scrub (whole numbers; typed values may have decimals), or press ↑ / ↓ in a field (⇧: by ten). Every field keeps to a sensible range (opacity 0–100%, sizes from 1, padding and stroke from 0, a corner radius up to half the shorter side). W and H keep a picture's proportions by default (other layers don't; each kind remembers its toggle). Rectangles get a corner radius (a rectangle becomes a rounded one), and the corners button gives each corner its own field: PPTX stores all four equal, the top and bottom pairs, the diagonal pairs or the top right alone (`roundRect`, `round2SameRect`, `round2DiagRect`, `round1Rect`), and other combinations are refused with a note. A slide's placeholder has a reset button in the header (next to the tag) that puts it back on its layout's position and size (`placeholder.reset` with `ids`); it is greyed out while the placeholder follows its layout. Nothing in the panel changes other slides' placeholders. Several layers: the header reads "N selected" with Group and Merge shapes; values they don't share show Mixed, and typing one sets it for all; Spacing shows the horizontal and vertical gap between neighbours (Mixed when uneven) and typing one spaces them evenly, keeping the first in place. With nothing selected: text styles, Slide (a reset button in its title row puts the slide's placeholders back on their layout, `slide.reset`; the layout picker) and Background (the colour row always works: picking a colour gives the slide a solid background). In Slide Master view with nothing selected: the master (background, theme colours as a scheme plus each colour's swatch picker and hex, heading and body fonts, text styles) or a layout (name, how many slides use it, its background or following the master's, Hide background graphics for the layout). Colour popovers have a custom colour square under the swatches. Animate has Add state and Play, the Morph timing into the current state and what changes, Build text for a selected text box (None, Appear, Fade, Fly in, Wipe, Zoom: its paragraphs come in one per click, PowerPoint's By Paragraph build, `animation.build`) and, folded, the Animation pane | Layer opacity for outlines and text (PPTX keeps opacity per fill) |
| Under the slide | Presenter notes card with a grip; floating toolbar: Select, Text box, Picture, Shape, Table, Chart, Comment, notes toggle, command search. Single-key tools: V select, T text box, R rectangle, O oval, L line, Esc back to Select (not while editing text). Letters don't type over a selected shape here: Enter edits its text | Pen and comment tools on the canvas |
| Document tabs | While a presentation is open (even one), a row under the title bar as in canvas editors: Home, a tab per presentation (deck icon, name fading out when long, unsaved dot, close on hover and on the current tab, middle-click closes), + for a new one; the current tab is the colour of the panel under it | Tabs can't be reordered |
| Side panels | ⌘. (or ⌘\\) hides both side panels and shows them again, ⌥⌘. the left one (`view.panels`, `view.leftPanel`, also in the View menu); the sidebar has a hide button, and a hidden panel leaves a button in its top corner of the canvas to bring it back | |
| Group resizing | Dragging a group's handles keeps its proportions; ⇧ frees them (engine pref `groupKeepsAspect`, on in this layout; the classic layout keeps PowerPoint's ⇧ to keep them). The panel's W/H lock starts on for groups and pictures. Snapping while proportions are kept moves the other side to match | |
| Number fields | From FigCraft: typed values may be arithmetic with units (`100/3`, `2*(4+1)`, `12px`) or edits of the value (`+10`, `*2`, `/4`, `50%`); on Mixed fields a relative edit changes each layer's own X, Y, W or H. Scrubbing a label: a step per point, ten with ⇧, a tenth with ⌥. Escape leaves a field unchanged. Fields outline on hover and ring while typed in | |
| Colour picker | A saturation / brightness square, hue bar (and opacity bar where a colour has one), Hex / RGB / HSB fields; the hue is kept while the colour is grey; one drag is one change (after FigCraft) | No eyedropper |
| Measuring | ⌥ held over the slide with a selection: red distances to the layer under the pointer (⌘⌥: inside groups) or the slide's edges, dashed extensions, labels in the panel's units that never overlap; ⌥ while dragging layers: live distances to their nearest neighbours (their group's other layers, else the slide's) or the slide's edges (from FigCraft) | |
| Menu bar | After PhotoCraft: on macOS the system menu bar (muda, `apps/deckcraft/src/mac_menu.rs`) with an app menu (About, Settings… ⌘,, Services, Hide, Hide Others, Show All, Quit ⌘Q), File, Edit, Insert, Slide, Format, Arrange, View (with check marks), Play, Window (Minimize, Zoom, the open presentations) and Help; items grey out when they can't run or a dialog is open; key equivalents come back to egui as the keys they were (⌘C / ⌘X / ⌘V as copy, cut, paste), so typing keeps its keys. Elsewhere (web, Windows, Linux) the same menus are drawn under the title bar in the canvas layout (`native_menu.rs`) | |
| Find | ⌘F (Edit › Find…) opens a bar over the canvas: typing finds every place the text appears on the slides (shapes, placeholders, groups; `edit.find`), with "3 of 12"; Enter / ⇧Enter and the arrows go to the next or previous one, selecting the words on their slide (`edit.showMatch`); Aa matches case, W whole words; it opens on the words selected; Esc closes it. The command palette is ⇧⌘P | No Replace yet; tables and notes aren't searched |
| Imported slides | A slide's text boxes join the masters without doubling up: the panel's Layout picker adopts the layout (`slide.layout` with `adopt`: existing placeholders take its places, nothing added or reordered); a plain box on a slide has the tag menu (Title, Body…, as the layout has them); a placeholder's reset button puts it on the layout's place and clears its own formatting so the master drives it (`placeholder.reset` with `formatting`); Reset Slide in this layout adds nothing either (`slide.reset` with `adopt`). The ribbon keeps PowerPoint's Layout and Reset, which add the layout's empty placeholders | |
| Preview in place | The Animate panel's ▶ Preview (and the ribbon's Preview buttons and the Animation pane's Play All) play in the editor's canvas, not full screen: the slide's animations run by themselves, through a slide's states (Morph), then editing carries on; Esc stops it early | |
| Notices | After PhotoCraft: up to three cards in the lower-right corner (missing fonts, and in this layout, which has no status bar, status and error messages), each gone after a few seconds or when closed | |
| Lock keys | ⌘L locks the selected layers, or unlocks them when all are locked; ⇧⌘L unlocks every layer on the slide (or the layout or master being edited), nothing selected needed. Also in Arrange › Lock / Unlock and Unlock All. The ribbon keeps ⌘L for Align Left, and so does text being typed | |
| Deep select | ⌘-click on the slide selects the innermost layer under the pointer, inside groups; with a grouped layer selected, a plain click on another layer of the same group selects it (selection context), and Esc steps out to the group, then to nothing (engine pref `cmdDeepSelect`, on in this layout); ⇧-click adds to the selection. The classic layout keeps PowerPoint's ⌘-click (add or remove) | |
| Slide list | Keeps the current slide in view when it changes (both layouts); the selected slide's outline is the accent colour while the list has the keyboard and grey otherwise, as in macOS lists; the Layers list marks (accent dot) the layers that change from the previous state. A slide and its states (the slides after it entering with Morph) are one entry with an "N states" badge; clicking it keeps the state being edited, and dragging, Delete, ⌘D, Cut, Copy and ⌘↑ / ⌘↓ take the states with it (↑ / ↓ step over them). The grid view shows every slide | A deliberate Morph between unrelated slides groups them too |
| States strip | Under the slide while it has states (or turned on with the toolbar's States button): each state's thumbnail, numbered; click one to edit it; + after one adds a state there; the chip between two states shows Morph's duration and opens its timing (duration, objects / words / characters, Stop being a state); drag a state to reorder it (a line shows where it goes), and a state's menu moves it left or right, adds one after it or deletes it (the run's first transition stays at its start, so deleting or moving the first state doesn't join it to the slide before). Dots on a thumbnail count its clicks (one per on-click effect and per built paragraph) | |

PPTX-safe choices in the inspector: a weight is offered only when PPTX can name it, as Regular or
Bold (the bold flag) or as a family name of its own ("Avenir Next Demi Bold"), so a variable font
such as Inter offers Regular and Bold, not its in-between instances. Opacity is the fill's
transparency (`shape.fill {transparency}`) or the picture's (`picture.adjust {transparency}`);
outlines and text keep their own colour's transparency. Letter spacing offers presets because
`format.state` doesn't report the current value.

Text styles (`crates/ui-egui/src/textstyles.rs`) are the deck's own inherited styles: Title (the
master's title style), Body, Body 2 and Body 3 (its body style's first three levels) and Plain text
(the presentation's default text style, which text boxes inherit). With nothing selected the
inspector lists them; click one to edit its font, weight, size, line height, italic and colour for
the whole presentation (`master.textStyle`, one undo step). A theme font (`+mj-lt` / `+mn-lt`) is
changed in the theme (`design.fonts`), so every style and shape in it follows. Size and line height
leave layouts that set their own alone (the title slide's 60 pt title), as PowerPoint does; font,
weight, italic and colour clear layout overrides (`layouts: true`). Applying a style to text whose
box inherits it clears the text's own formatting (`format.clear`, and the list level for Body 2 and
3), so it follows the style ("Title" in the menu). Elsewhere PPTX has no link, so the values are
copied ("Title (copy)"), and editing the style also updates text whose formatting still matches its
old values. Known gap (engine, both layouts): formatting an empty placeholder doesn't stick, since
the engine formats runs and an empty placeholder has none.

States (`crates/ui-egui/src/states.rs`, the Animate tab) are slides joined by the Morph transition:
consecutive slides whose later ones enter with Morph are the states of one animation, and Morph
animates each layer it pairs between two states (by `!!name`, name, text, then shape:
`deckcraft_anim::morph_pairs`) in position, size, rotation, colour and transparency, fading in or
out the layers only one state has. The tab shows the run as a strip of numbered thumbnails (click to
go), **Add state** (duplicate the slide and set Morph on the copy, one undo step), **Play** (present
from the first state), Morph's duration and Objects / Words / Characters, **Stop being a state**
(Morph off), and **What changes** from the previous state, layer by layer (moves, resizes, rotates,
restyles, text changes, fades in, fades out). A deck of states is an ordinary PPTX: PowerPoint 2019
and Microsoft 365 play it the same way, older versions show a fade. Limits: PPTX gives Morph no
easing setting, so there is none here; a layer hidden with the eye isn't animated (the tab says so
and suggests deleting it in that state, which fades it).

Units (`crates/ui-egui/src/units.rs`): the canvas layout shows pixels at 2 px per point, Figma
Slides' scale (a 16:9 slide is 1920 × 1080 px, a 44 pt title 88 px): type sizes, the text style
editor, sizes and positions, line widths, margins, glow and soft edges, and the ruler. Zoom follows
Figma's meaning (100% is one slide pixel per screen point, so a fitted slide reads about 40–50%).
The ribbon keeps PowerPoint's points and inches. Commands and files are unchanged: commands take
points, and PPTX stores EMU, 6,350 to a pixel exactly. Dialogs opened from the command palette
(Slide Size, Paragraph) still show points and inches.
