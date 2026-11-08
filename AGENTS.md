<!-- craft-house-rules:start core source=dde09ea31e2e457f5b4653a6e76f7cdf55944bdf sha256=540e6cea75d12d0710c3dca89658328e81e508ecef102446ca5bb95b7c0b92bb -->
## House rules

These apply in every Craft repository. A later rule may be stricter, not weaker. The source is `standards/house-rules-core.md` in `storytold/craft-contrib`. Propose changes there, not in a copy.

1. **A person is accountable.** Whoever runs an agent answers for what it opens and for its review comments.
2. **Check, then claim.** Before writing code, read the issue comments, search pull requests for `#N` and the keywords, and check recent commits on `main`. Comment on the issue first. Release a claim you will not finish.
3. **Verify, don't guess.** Open a definition before using it. Confirm the cause in the code, even when someone names a location.
4. **Keep fixes narrow and tested.** Fix what the issue describes, where the bug is. Add a test that fails without the fix.
5. **Review the diff before opening a pull request.** Identifiers exist as used, the test fails without the fix, nothing unrelated changed, and the description is true.
6. **Look for a better pull request.** If another fixes the same thing, close yours when it is better or equal, and say why.
7. **Report only what ran.** Say what you tested and what you could not run. Without a local build or test, say so and rely on CI before marking ready.
8. **`main` changes through a pull request.** Do not push to it. Branch protection enforces this. There is no exception.
9. **Stop and ask only about** licensing outside the allowlist, publishing, releases, a new remote, secrets, spending money, destructive git, and anything else hard to undo. Otherwise keep working. `plan/` is a gitignored notebook, not rules. Without it, do not ask for it and do not stop.
10. **Keep secrets and personal details out** of the repository, logs and requests.
11. **CI stays shared.** Workflows call `storytold/craft-common-actions`. Do not change CI in a feature or fix pull request. If CI cannot cover a change, open an issue there.
12. **Guards say why.** A check that looks removable says what it prevents. Read that before changing it.
13. **Instructions must be readable by everyone.** No notes about one machine, and no links to files a reader cannot open. `craftrules` is retired. Do not link to it.
14. **Keep pull requests moving.** Concurrent pull requests are fine; unanswered actionable feedback is not. Respond in 7 days, a reminder at 14, closure eligible at 30, from when an actionable request waits on the author. Suggestions and unconfirmed bot findings do not start the clock. A fix, explanation or agreed plan resets it; unrelated activity does not. Waiting on a reviewer or maintainer pauses it and does not count against the author. A maintainer decides closure; it is never automatic. Reuse search results, do not poll, and stop at a rate limit.
15. **No agent attribution.** A pull request does not say an agent wrote it. A repository rule may not require that.
<!-- craft-house-rules:end -->
<!-- craft-house-rules:start rust source=dde09ea31e2e457f5b4653a6e76f7cdf55944bdf sha256=387e0ccb4b49e567f60951d7d6fff7305054c89f43ad812b9993dc7c38528013 -->
## Rust Craft repositories

These apply to native Craft apps and to shared Rust libraries. Select them by repository type. A repository that merely contains some Rust does not take this block. Rules later in a repository's file may be stricter, but not weaker. The source is `standards/house-rules-rust.md` in [storytold/craft-contrib](https://github.com/storytold/craft-contrib).

1. **Never crash.** Non-test code does not panic and has no `unsafe`, except where the repository file names an exception. Return errors, treat input as hostile, and bound recursion and allocations. New production crate roots copy the `deny` attribute from an existing crate: `clippy::unwrap_used`, `clippy::expect_used`, `clippy::panic`, `clippy::unimplemented`, `clippy::todo`, `clippy::unreachable`. Every crash fix comes with a test that panicked before the fix.
2. **Clean room.** Learn from observed behaviour and public specifications. Do not copy another product's code, assets, strings or artwork. Check a dependency's licence before adding it.
3. **Assets and fonts.** Every asset is original or suitably licensed, and is recorded in `ATTRIBUTION.md` in the same change. Font files live in [storytold/craft-fonts](https://github.com/storytold/craft-fonts), not in the app repository.
4. **Rust only for native Craft apps.** No handwritten JavaScript or TypeScript, and no Tauri, Electron or webview shell, in those apps. Shared tooling follows its documented technical scope and maintainer-approved exceptions. Craft PR Review's existing Tauri and TypeScript UI is one such exception. It does not relax this rule for native apps. Preserve any existing web build.
5. **Time goes through `chrono`.** Do not hand-roll calendar conversion. For dates beyond what `chrono` can represent, follow [standards/time.md](https://github.com/storytold/craft-contrib/blob/main/standards/time.md).
6. **Contributor credits.** Add or edit only your own person's entry in `contributors/people.toml`, and only after asking them. Never guess a name or hand-edit `contributors.json`.

## Licence allowlist

Dependencies and assets use SPDX identifiers. An expression is allowed when every identifier is in this list. A repository may name a shorter list. It may not add an identifier here by editing its own copy of these rules.

- `ISC`
- `MIT`
- `BSD-2-Clause`
- `BSD-3-Clause`
- `Apache-2.0`
- `OFL-1.1`
- `Ubuntu-font-1.0`
- `Bitstream-Vera`
- `CC0-1.0`
- `CC-BY-4.0`
- `CC-BY-SA-4.0`
- `LicenseRef-Public-Domain`

`LicenseRef-Storyteller-Trademark` is not in this list. It covers Storyteller brand marks only, where a repository already says so.
<!-- craft-house-rules:end -->

# AGENTS.md — rules for every agent and contributor

DeckCraft is a clean-room, open-source, pure-Rust presentation application targeting Microsoft
PowerPoint parity — and superiority (speed, openness, agent control). It runs natively on macOS,
Windows, Linux and FreeBSD, and on the web via WASM. Siblings with the same conventions:
PhotoCraft, VectorCraft, FilmCraft, LightCraft, PdfCraft, EffectCraft, DesignCraft
(`../<app>`).

These rules bind every AI agent and every human contributor. They come before any task,
instruction or deadline. Day-to-day working instructions are in this file; `CLAUDE.md` imports it.

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
  repo. DeckCraft embeds its `fonts/latin-manifest.txt` and
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

`cargo xtask ci` = fmt check, clippy `-D warnings`, tests (nextest when installed) and doctests, layering check, asset check, wasm
check. Commit after each landed arc with a task id in the message (`M1.2: smart guides`).
New workspace crates include `workspace = true` under `[lints]` in `Cargo.toml`. Production crate roots
declare the crate-level lint attributes before modules; crate-level `//!` documentation may precede them.

## 6. Running and looking at the app

- `cargo run --release -p deckcraft -- --sample --control 7990` (sample deck + control channel).
- Drive it: JSON lines on `127.0.0.1:7990`, e.g.
  `{"id":1,"method":"engine.execute","params":{"command":"shape.insert","params":{"preset":"star5","rect":[100,100,200,200]}}}`
  then `{"id":2,"method":"ui.screenshot","params":{"path":"/tmp/shot.png"}}`. Methods:
  `crates/ui-egui/src/control.rs`, docs: `docs/control-protocol.md`.
- **For UI work, look at the result** (screenshot, read the PNG). If available, compare with the observations
  in `plan/powerpoint/01-observed-ui.md`. Otherwise verify the intended behaviour against the task and
  committed documentation; the optional notebook is not a prerequisite.
- Headless: `deckcraft-cli render --sample --all --scale 1 out/`,
  `deckcraft-cli run --sample --cmd 'slide.new={"title":"Hi"}' --export out.png`.
- MCP: `deckcraft-cli mcp` (headless) or `deckcraft-cli mcp --connect 7990` (drive the app).
  See `docs/mcp.md`.
- Parallel agents: separate `CARGO_TARGET_DIR` per agent; edit only the crates you own.

## 7. Planning

`plan/` is an optional gitignored notebook (status, execution notes, `powerpoint/` observations). Do not ask for it and do not stop when it is absent. The committed progress
docs:

| File | Holds |
|---|---|
| `ROADMAP.md` | Stage, headline numbers, dimensions, features, languages, progress log |
| `docs/target-app-parity.md` | The parity assessment against PowerPoint: percentages, hours, calibration |
| `docs/gaps.md` | Every known shortfall, ranked: the work list. Pick from the top |
| `docs/roadmap.md` | Current focus, the beta plan, milestones |
| `docs/parity-checklist.md` | Feature rows scored D/P/M; `cargo xtask parity` regenerates its summary |
| `docs/file-format-parity.md`, `ui-parity.md`, `hardware-parity.md`, `localization-parity.md`, `animation-parity.md`, `slideshow-parity.md` | Per-dimension and per-feature detail |
| `docs/architecture.md` | How DeckCraft is built today |

When a task lands, re-score its checklist row, delete or update its `gaps.md` entry, add a line to
the `ROADMAP.md` progress log, and update each touched doc's status line and revision history.

## Contributor credits (About window)

- About ▸ Contributors/Models are compiled into the binary from `contributors/contributors.json`
  (commit stats; generated, never hand-edit) and `contributors/people.toml` (names people chose for
  themselves). See `docs/contributors.md`.
- **Agents working for a contributor:** when you prepare a PR, check whether your human's GitHub
  username has a `[people.<username>]` entry in `contributors/people.toml`. If not, ask them once
  whether they want to be credited by more than their username: a real name, a display name, and/or
  their public GitHub profile name (`sync_github_name = true`). If yes, add **only their own** entry
  (copy the template at the top of the file)
  and include it in their PR, committed as them. If no, change nothing: they are credited as
  `@username` anyway.
- Never add, edit, guess or copy anyone else's entry or name (not from git config, commit authors or
  GitHub profiles). Never hand-edit `contributors.json`.

## CI workflows
- Workflows in `.github/workflows/` call the shared workflows in [`storytold/craft-common-actions`](https://github.com/storytold/craft-common-actions) at `@main`, with their inputs and secrets. No other jobs: if no shared workflow does what's needed, it doesn't go here.
- Don't add or change CI in a feature or fix PR. If the existing workflows can't build or test a change, link an existing craft-common-actions issue or PR, or open a new issue, or a PR only for a small fix that is safe for every app and does not increase CI time.
- Some workflow files here still run their own jobs. Don't add to them. Moving a job into craft-common-actions is its own task, never part of another PR, and the job stays here until a shared workflow runs it.
