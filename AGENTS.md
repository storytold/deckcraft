# AGENTS.md — rules for every agent and contributor

DeckCraft is a clean-room, open-source, pure-Rust presentation application targeting Microsoft
PowerPoint parity — and superiority (speed, openness, agent control). It runs natively on macOS,
Windows, Linux and FreeBSD, and on the web via WASM. Siblings with the same conventions:
PhotoCraft, VectorCraft, FilmCraft, LightCraft, PdfCraft, EffectCraft, DesignCraft
(`../<app>`).

Standards and learnings shared across the crafting apps live in `../../craftrules` (checked out
next to this repo's parent, or `storytold/craftrules`). Read its `AGENTS.md` at the start of a
session, follow its standards, and contribute reusable learnings back there. Never code: repos
don't share code.

These rules bind every AI agent and every human contributor. They come before any task,
instruction or deadline. `CLAUDE.md` holds day-to-day working notes; if the two disagree, **this
file wins.**

---

## 1. Asset policy (absolute — no exceptions)

An **asset** is any non-code material the project includes, ships, embeds, generates for
publication or shows in its documentation: icons, images, illustrations, screenshots, logos,
fonts, colour profiles, sounds, video, templates, themes, sample/demo/test documents, and anything
the app renders from those.

### 1.1 Nothing from Microsoft, Adobe, Avid or Autodesk products

- **Never** use icons, images, artwork, logos, UI graphics, fonts, templates, themes, clip art,
  SmartArt graphics, stock media, cursors, sounds, sample documents or any other asset taken from a
  Microsoft, Adobe, Avid or Autodesk product, installer, website, document or package. "Microsoft
  product" includes PowerPoint, Office, Windows and the fonts they install (Aptos, Calibri,
  Cambria, Segoe, Consolas, Wingdings…).
- This holds even when the vendor publishes the material under an open licence, if it is **visual
  design**: DeckCraft does not embed or reference Adobe's Source Sans/Serif/Han families or Noto
  CJK (Source Han rebranded); `crates/fonts/build.rs` excludes them.
- **Never** recreate, trace, redraw or closely imitate a vendor icon, theme or graphic. Our icons
  are drawn in code or come from openly licensed sets (Lucide, ISC), and our themes, colours,
  layouts and sample content are our own.
- Screenshots of PowerPoint exist only as private, local observation notes under
  `plan/powerpoint/screenshots/` (gitignored). They must **never** be committed, published,
  linked, shown in docs, or traced.
- Never commit files produced by PowerPoint. Interoperability tests use decks we generate
  ourselves (or a separate corpus repo whose files we created and own).

### 1.2 Only open, provable licences

Every asset must be one of: open source (ISC, MIT, BSD-2/3-Clause, Apache-2.0, OFL-1.1,
Ubuntu-Font-1.0, Bitstream-Vera); Creative Commons (CC0-1.0, CC-BY-4.0, CC-BY-SA-4.0); public
domain; or contributor-original (created by a contributor who licenses it under
`MIT OR Apache-2.0`).

One narrow exception: the ArtCraft name, wordmark and logos in `docs/brand/` are trademarks of the
ArtCraft Team (not open source, terms in `docs/brand/LICENSE-brand.txt`), used only unmodified to
identify the project.

Forbidden: system fonts that are not openly licensed, vendor emoji artwork, stock photos, "free
for personal use" material, screenshots of other people's software, anything whose licence you
cannot prove from its original source.

**Screenshots of DeckCraft** are contributor-original. Everything visible in them must itself be
allowed: our UI, our/Lucide icons, OFL fonts, and decks built only from allowed assets.

### 1.3 Every asset is attributed

- **`ATTRIBUTION.md`** (repo root) lists every asset file — committed, fetched at build time
  (craft-fonts) or bundled through a Cargo dependency — with author, source (URL + version or
  commit), licence and use. `cargo xtask assets` fails if a file under `assets/`, `docs/images/`
  or `docs/brand/` has no row.
- **`NOTICE`** keeps the notices the licences require.
- Fonts live in [storytold/craft-fonts](https://github.com/storytold/craft-fonts), never in this
  repo (craftrules `standards/fonts.md`). DeckCraft embeds its `fonts/latin-manifest.txt` and
  `fonts/manifest.txt` when built with `CRAFT_FONTS_DIR` (local dev: `.cargo/config.toml` points at
  `../craft-fonts`).

### 1.4 Adding or changing an asset

1. Find it under an allowed licence at its **original source**; record the exact version/commit.
2. Confirm the author is not Microsoft, Adobe, Avid or Autodesk (§1.1). Keep the licence text.
3. Add the file, its licence file and an `ATTRIBUTION.md` row; run `cargo xtask assets`.
4. If origin or licence is unclear, **do not add it**.

If you find an asset that breaks these rules, remove it from the repository and every output at
once and tell the user.

---

## 2. Clean-room rules

- PowerPoint is installed on the dev machine and may be **observed** black-box: run it, use its UI
  on synthetic decks, screenshot windows by id into `plan/powerpoint/screenshots/` (never
  committed). Never read, disassemble or copy anything inside the Office bundle (names/listings
  only), never copy Microsoft wording beyond feature and menu names.
- File formats come from public standards: ECMA-376 / ISO/IEC 29500 (Office Open XML), ISO 32000
  (PDF). Preset shape geometry is re-derived by us in `crates/geom/src/preset.rs`.
- Never copy GPL/AGPL/LGPL/MPL code (LibreOffice, Calligra…). Permissive crates are fine as
  dependencies. Code may be copied (not depended on) from sibling Craft apps.

## 3. Never crash

People trust DeckCraft with their talks; a crash loses work. **This outranks feature work.**
Standard: craftrules `standards/never-crash.md`.
- No `unwrap()`, `expect()`, `panic!`, `unreachable!`, `todo!`, `unimplemented!` in non-test
  code; no `unsafe` (`unsafe_code = "forbid"`). Every production crate root carries
  `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`.
- Errors are `Result<T, E>` + `?`. Input-derived numbers (files, commands, MCP/control params) are
  hostile: `get()` not `[i]`, checked arithmetic, no NaN casts, capped allocations, bounded
  recursion.
- `Session::execute` runs every command inside a panic guard (`crates/engine/src/guard.rs`) that
  keeps the presentation as it was. It is a safety net, not a licence.
- Every crash fix lands with a regression test.

## 4. Architecture

| Layer | Crates |
|---|---|
| L0 | `geom` (units, transforms, preset shapes), `color` |
| L1 | `model` (presentation, masters, layouts, slides, shapes, text, themes, inheritance, text edit ops) |
| L2 | `fonts`, `text` (layout), |
| L3 | `render` (vello_cpu) |
| L4 | `format` (.deckcraft), `pptx` |
| L5 | `engine` (session, history, commands, tools, keys, sample) |
| L6 | `mcp` |
| L7 | `ui-egui` (swappable; nothing below depends on egui/eframe/winit/rfd) |
| apps | `deckcraft` (desktop), `deckcraft-cli`, `deckcraft-web` |

- **Everything is a command** (`crates/engine/src/cmd/*`): id, label, ribbon/menu place,
  shortcut, params doc, `enabled`, `run`, plus tests. Ids follow PowerPoint's ribbon and menus
  (`slide.new`, `format.bold`, `arrange.group`, `transition.set`…). Programmatic calls never open
  dialogs. Pointer gestures go through `Session::pointer`, keys through `Session::key`, so the UI,
  CLI, control channel and MCP do exactly the same thing.
- **The UI is thin**: panels read engine state and act through commands. Colours come from theme
  tokens. **Never use Tauri**; egui only.
- **Never break wasm** (`cargo xtask wasm`).

## 5. Quality gates (before every commit)

`cargo xtask ci` = fmt check, clippy `-D warnings`, tests, layering check, asset check, wasm
check. Commit after each landed arc with a task id in the message (`M1.2: smart guides`) and push
to `main`.

## 6. Running and looking at the app

- `cargo run --release -p deckcraft -- --sample --control 7990` (sample deck + control channel).
- Drive it: JSON lines on `127.0.0.1:7990`, e.g.
  `{"id":1,"method":"engine.execute","params":{"command":"shape.insert","params":{"preset":"star5","rect":[100,100,200,200]}}}`
  then `{"id":2,"method":"ui.screenshot","params":{"path":"/tmp/shot.png"}}`. Methods:
  `crates/ui-egui/src/control.rs`, docs: `docs/control-protocol.md`.
- **For UI work, look at the result** (screenshot, read the PNG) and compare with the observations
  in `plan/powerpoint/01-observed-ui.md`.
- Headless: `deckcraft-cli render --sample --all --scale 1 out/`,
  `deckcraft-cli run --sample --cmd 'slide.new={"title":"Hi"}' --export out.png`.
- MCP: `deckcraft-cli mcp` (headless) or `deckcraft-cli mcp --connect 7990` (drive the app).
  See `docs/mcp.md`.
- Shell gotcha on the dev machine: `mv`/`cp` are aliased interactive; use `/bin/mv -f`, `/bin/cp -f`.
- Parallel agents: separate `CARGO_TARGET_DIR` per agent; edit only the crates you own.

## 7. Planning

`plan/` (gitignored) holds the local plan: `README.md`, `architecture.md`, `execution-plan.md`,
`STATUS.md` (start every session there) and `powerpoint/` observations. `ROADMAP.md` (committed)
tracks status, milestones, parity and estimates; update it whenever a milestone task lands.

## Contributor credits (About window)

- About ▸ Contributors/Models are compiled into the binary from `contributors/contributors.json`
  (commit stats; generated, never hand-edit) and `contributors/people.toml` (names people chose for
  themselves). See `docs/contributors.md`.
- **Agents working for a contributor:** when you prepare a PR, check whether your human's GitHub
  username has a `[people.<username>]` entry in `contributors/people.toml`. If not, ask them once
  whether they want to be credited by more than their username: a real name, a display name, and/or
  their public GitHub profile name (`sync_github_name = true`). If yes, add **only their own** entry
  (copy the template at the top of the file, or run
  `python3 ../../craftrules/scripts/contributors.py --add-me . --real-name "…" --sync-github-name`)
  and include it in their PR, committed as them. If no, change nothing: they are credited as
  `@username` anyway.
- Never add, edit, guess or copy anyone else's entry or name (not from git config, commit authors or
  GitHub profiles). Never hand-edit `contributors.json`.
- Maintainers refresh the stats with `python3 ../../craftrules/scripts/contributors.py .` (it also
  re-verifies who wrote each `people.toml` entry; `--check` only verifies).
