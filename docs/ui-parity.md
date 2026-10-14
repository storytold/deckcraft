# UI parity with PowerPoint

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-12 · **Change:** minor (File tab / Backstage, #76; previous: off-slide objects in Normal view, #270) · **Target:** Microsoft PowerPoint for Mac 16.113 (Microsoft 365)

> **Last reviewed:** 2026-10-12 · **Last updated:** 2026-10-14 · **Change:** minor (cloud Open and Save As, the account badge, the Share split button and the presence row; previous: text editing keys observed in PowerPoint for Windows, #268) · **Target:** Microsoft PowerPoint for Mac 16.113 (Microsoft 365)

How DeckCraft looks and feels next to PowerPoint: the shell, the ribbon, menus, panes, dialogs,
direct manipulation, keyboard and precision. Observations of PowerPoint are kept locally in
`plan/powerpoint/01-observed-ui.md` and `menu-tree.txt` (gitignored, clean-room rules in
AGENTS.md §2). Dimension score: **~60% ready** (estimated), **30–50 h** to parity.

## Shell

| Element | PowerPoint for Mac | DeckCraft | Gap |
|---|---|---|---|
| Title bar: AutoSave, Home, Save, Undo/Redo, document title, Share, search | yes | yes, with the account badge, who else is here and Share as a split button when a cloud service is configured | #71 macOS window controls misaligned; no AutoSave |
| Native macOS menu bar (File, Edit, View, Insert, Format, Arrange, Tools, Slide Show, Window, Help) | yes | **no** (`menus::menu_tree()` is defined but not installed) | 3–5 h |
| Menu-bar items with a DeckCraft equivalent | 217 app-specific items | 128 matched by name (59%, heuristic lower bound) | See [target-app-parity.md](target-app-parity.md) |
| Ribbon tabs: Home, Insert, Draw, Design, Transitions, Animations, Slide Show, Record, Review, View | 10 | 10 | Record tab is a placeholder |
| Contextual tabs: Shape Format, Picture Format, Table Design, Table Layout, Chart Design, Chart Format, Video/Audio Format and Playback | yes | most; checklist P0 row partial | Chart Format, media tabs thinner |
| File tab (Backstage): Home, New, Open, Info, Save, Save As, Print, Share, Export, Close, Account, Options | yes | yes (#76), see [File tab](#file-tab-backstage). Open and Save As reach cloud storage. Share lives in the title bar as a split button | Video/GIF/CD exports, templates beyond the built-in themes |
| Start screen: new, themes, templates, recent, open | yes, the same page as File › Home | yes: File › Home with Home, New, Open, Account and Options (#76) | |
| Status bar: slide counter, language, notes, comments, views, zoom slider | yes | yes | |
| Command search (Tell Me) | yes | command palette | #79 default entry fails |
| Light / dark appearance | follows the system | light or dark; doesn't follow the system (#41) | 1–2 h |
| One window per presentation, New Window, Arrange All | yes | tabs in one window; no New Window | 3–5 h |
| Quick Access Toolbar customization | yes | no | P1 partial |

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
| Home | Prompt box (Copilot), New row of themes with More themes, Recent / Favorites / Shared with Me with a file search, More presentations | DeckCraft's mark, Blank Presentation and the first themes with More themes →, Recent with a file search and More presentations → (to Open). No Copilot prompt, Favorites or Shared with Me |
| New | Blank Presentation, template search with suggested searches, gallery of online templates and themes | Blank Presentation, search box, the built-in themes and the sample deck (`file.new {theme}`). No online templates |
| Open | Recent, Shared with Me, OneDrive, This PC, Add a Place, Browse; Presentations / Folders; Recover Unsaved Presentations | Recent and Browse (`app.openDialog`), recent presentations with a search box (`file.open`), Recover Unsaved Presentations (`file.recovery.open`), and a Cloud section with your own files beside what other people shared with you (`cloud.open`). No Add a Place or Folders tab |
| Info | Name and path, Upload / Share / Copy path / Open file location; Protect, Inspect, Version History, Manage Presentation; Properties, Related Dates, Related People | Name and path, Copy path, Open file location; Inspect Presentation (Accessibility Checker), Manage Presentation (Revert); Size, Slides, Hidden slides and editable Title, Subject, Tags, Categories, Company, Comments, Author (`file.info`, `file.properties`). No Protect, Version History or dates |
| Save | Saves; a presentation never saved goes to Save As | Same (`file.save`) |
| Save As | Places, folder, file name, type, Save, More options | This Computer or the cloud account; on this computer a type (PowerPoint, DeckCraft, PDF, Outline, PNG, JPEG) and Save, which opens the system Save dialog (web: Download); in the cloud a folder, a file name and Save (`cloud.saveAs`). .pptx/.deckcraft take the new file; the others are copies (`file.saveAs`, `file.export`) |
| Print | Print and Copies, Printer, Print All / Selection / Current / Custom Range, Slides box, layout (Full Page Slides, Notes Pages, Outline, Handouts 1–9), sides, collation, colour, Edit Header & Footer; page preview with pager and zoom | Print, Print All Slides / Current Slide / Custom Range, Slides box (`1-3, 5`), Full Page Slides / Notes Pages / Handouts 1–9, Print Hidden Slides, Frame Slides, Edit Header & Footer; preview with pager. DeckCraft prints through a PDF (`file.export`) opened in the system viewer, which has the printers (web: downloaded). No printer list, copies, sides or colour |
| Share | Cloud sharing | A split button in the title bar. The label opens the share dialog (people, groups, public links). The chevron offers Work together and the Save a Copy formats |
| Export | Create PDF/XPS Document, Create a Video, Create an Animated GIF, Package Presentation for CD, Create Handouts, Change File Type | Create PDF Document, Create Handouts (a PDF), Change File Type. No video, GIF or CD package |
| Close | Closes; asks Save / Don't Save / Cancel about unsaved changes | Same (`file.close` after the prompt); Ctrl+W asks too |
| Account | User information, sign-in, Office Background and Office Theme, product information, About | Appearance (light/dark, `view.dark`), product information, About DeckCraft. The title bar's account badge is where somebody signs in. It says who, and offers Sign out |
| Options | Opens the PowerPoint Options dialog | Opens Preferences |

## Panes and dialogs

| Pane / dialog | PowerPoint | DeckCraft |
|---|---|---|
| Format Shape / Picture / Text pane (Fill & Line, Effects, Size & Properties, Picture, Text Box) | full | partial: fill, gradient editor, line, some effects, size; no 3-D, thin picture/texture options |
| Format Background pane | yes | yes |
| Selection Pane | front-most first, groups fold, drag to restack, Bring Forward / Send Backward arrows, Show All / Hide All, eye, padlock (Lock), rename in place | same: drag restacks among siblings (a child stays in its group), Ctrl+click toggles, Shift+click selects a run, arrows / Home / End / F2 / Esc in the list, right-click menu |
| Animation Pane | list + advanced timeline | list, reorder, timing; no timeline |
| Comments pane (threads) | yes | yes |
| Design Ideas (Designer) | yes (cloud) | no |
| Dialogs | ~60 (Font, Paragraph, Bullets and Numbering, Find, Replace, Replace Fonts, Hyperlink, Action Settings, Header and Footer, Slide Size, Set Up Show, Custom Shows, Print, Page Setup, Insert Object, Symbol, Equation, Chart data sheet, SmartArt chooser, Preferences…) | 27 (`crates/ui-egui/src/dialogs.rs`): Table, Slide Size, Header and Footer, Hyperlink, Set Up Show, Zoom, Comment, Alt Text, Outline, Rename (section, shape, layout), Paragraph, About, Preferences, Export, Symbol, Equation (placeholder), Spelling, Accessibility, Language, Custom Shows, Chart Data, Trim Media, SmartArt chooser, Quit, Notes Pages |

## Direct manipulation

| Behaviour | PowerPoint | DeckCraft |
|---|---|---|
| Selection handles, rotation handle, yellow adjustment handles | yes | yes |
| Marquee, Shift/Cmd multi-select, Tab to cycle | yes | yes |
| Objects off the slide: drawn on the area around it in Normal view (full strength, master graphics too), selectable and movable there; clipped in the show, exports and thumbnails | yes | yes (#270) |
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
`plan/powerpoint/01-observed-ui.md`; icons are original (~240, drawn in code or Lucide) and must not
imitate Microsoft's. No pixel comparison against PowerPoint screenshots has been run since
2026-10-07. Cross-app icon consistency is open (#73, #56).

## Estimates

| Work | Hours |
|---|---|
| Native macOS menu bar from `menu_tree()`, filled out to PowerPoint's menu items | 3–5 |
| Format Shape pane depth | 6–10 |
| Missing dialogs (Font, Bullets and Numbering, Find/Replace, Replace Fonts, Action Settings, Print, Insert Object) | 8–14 |
| Window chrome, system appearance, palette fixes (#41, #71, #79) | 3–5 |
| Grid Options, edit points UI, crop depth | 4–7 |
| Multiple windows | 3–5 |
| A screenshot comparison pass on the most-used tabs | 2–4 |
| **Total** | **~30–52** |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-12 | minor | File tab (Backstage) observed in PowerPoint for Windows and matched (#76); the start screen is File › Home |
| 2026-10-12 | minor | Off-slide objects drawn and editable around the slide in Normal view (#270) |
| 2026-10-12 | minor | Selection Pane matched to PowerPoint (#133): drag and drop, arrows, groups, eye, Lock |
| 2026-10-12 | minor | Text editing keys: Ctrl+arrows, Ctrl+Backspace/Delete, Ctrl+Up/Down, Home/End on wrapped lines, Shift+F3, font size keys, as observed in PowerPoint for Windows (#268) |
| 2026-10-10 | major | Created: shell, panes and dialogs, direct manipulation, shortcuts and visual fidelity against PowerPoint 16.113 |
