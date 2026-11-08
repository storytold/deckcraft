# DeckCraft: egui 0.37 migration plan

Current candidate font pin: `37827adc41fd165f9a9234ac40f94dbad353aa69` restores the complete prior Latin catalog alongside the migration fallbacks. All three workflows use that published revision.

Status update (2026-10-25): the candidate includes main `80f13011d5a3c353369c6e38250df6e5a260f3e0`, preserving its table-cell selection, text AutoFormat, edit-point history, fill controls, status accessibility and GPU-test serialization. IME deletion's undo entry retains main's new point-type state. Status-control assertions locate the actual laid-out status row, avoiding the identically named ribbon control without assuming fixed UI zoom. Managed run `egui037-deckcraft-status-row-ci-20261025-0753` passed the focused status regression and all eight CI stages: 2,045 passing test executions, zero failures and six ignored across 61 summaries. These are execution counts, including repeated tests. Required font coverage was separately validated in run 0724; the merged tree retains it and its tests. Cargo dependency pins and lockfile are unchanged by this refresh. Native/browser GUI, native IME and paired application performance remain open. Publication requires separate explicit approval; human review and human merge are required.

Review follow-up (2026-10-25): managed run `egui037-deck-review-green-ci-20261025-0810` passed the required-font regression and all eight CI stages, including WASM: 2,058 passing test executions, zero failures and six ignored across 61 summaries. Native builds now require BIZ UDPGothic Bold while web builds require Regular only; the missing-Bold regression failed with the old acceptance behavior and passed with the fix. Accessibility queries now distinguish combo-box purpose from selected value. Complexity refactors preserve the existing behavior. Long-paragraph IME deletion still scans a prefix/suffix; bounded-cost and paired performance qualification remain open. Remote SonarCloud and CodeRabbit must rerun on the eventual published commit.

The original planning record below is historical.

## Earlier qualification before the combined font input — 2026-10-25

The local candidate reconciles `main` through
`38d2ade2b77de39c0e06278ddfd44d1082ff80a8` with the migration base
`770f4653bff0700b33d8475181e168da90a7ad4f`. All five source files listed in the
historical review below belong in the review commit. The source passed full CI
job `egui037-deckcraft-regression-ci-20261025-0445`: all eight stages, with 1,991
test passes, zero failures and six ignored tests across 60 test summaries.

The final regression additions exercise the Notes accessibility toggle, no-document
Open/Actions/Preferences/File-menu shortcuts, ignored document-only shortcuts, and
typing/IME isolation. Earlier validation failures and source-completeness warnings
below describe retained history; the current review packet records the complete
commit and its parents. **The current review/approval packet is stale for publication
pending a combined font pin:** `b076417…` has no Latin manifest/Inter, so the pinned
input cannot supply the default document family. Native/browser workflows and paired
app performance remain open. No publication or merge is authorized by these results.

## Historical planning and review records

> **Status:** final current local review recorded, 2026-10-25. The reviewed source is the dirty
> migration union in the worktree below: local base `770f4653bff0700b33d8475181e168da90a7ad4f`
> plus tracked edits and five untracked source files. That base HEAD alone is **not** a qualified
> release; any eventual commit must include all five untracked files (see the final status section).
> Build, test, benchmark, commit and publication remain with the parent's managed `task.py`
> workflow and its separate publication approval.
> **Implementation checkout:** `/home/codex/work/CraftApps/egui037-migration-20261022/deckcraft-implementation`
> at local base `770f4653bff0700b33d8475181e168da90a7ad4f` (dirty union). The earlier dirty
> implementation snapshot `9aef21e627cb447dd2180cd804db85343e73e5cf` and the historical planning
> baseline `b47195feec0f0fe4220d8943ad1c8add5dad8072` are retained only as review history.
> **Target:** egui-family `0.37.0`. The stock crates.io release is a local comparison experiment,
> not a staged consumer release; production uses reviewed, published, immutable storytold full SHAs
> for the complete egui family and winit. Publication of any pin requires a separate, explicit
> human approval of the exact contents and validation.
> **Review:** a fresh adversarial review of this plan, then a separate adversarial review of each
> implementation PR, is required before landing. This revision resolves the independent review in
> `plan-reviews/deckcraft-v1.md`: the lifecycle phase model, screenshot ownership, font feature,
> E21b/PRIMARY gaps, production pin provenance and performance sampling have been corrected. The
> final current local review is `final-landing/deck-final-review0045.txt`; its disposition is
> recorded in the final status section.

This plan is app-specific to DeckCraft. It is grounded in the baseline source and in the 0.37
sources available in the migration workspace; it is not a generic template. The repository rules
in `AGENTS.md` (asset policy, clean room, never crash, command architecture, quality gates) apply
in full. The cross-app engineering decision guide
(`/home/codex/work/ssd2/task-evidence/egui037-migration-20261022/local-guidance/engineering-decisions.md`)
also applies: keep app-owned commands, document state, undo, persistence and native windows; prefer
an app-owned adapter over a generic compatibility layer; validate observable behavior at the exact
reviewed revision.

---

Capture acceptance follows the suite contract: a queued-not-issued hidden request and
each settling frame must request `RequestPaintWhileHidden` from logic; only an issued
screenshot command forces paint itself. Callback arrival has no implicit wake, so the
live request owns the viewport waker. Record known issuance UI bounds and a geometry
epoch, reject/retry any intervening resize/DPI/zoom/surface change even if it changes
back, and derive pixel scale from returned dimensions only under an unchanged epoch.
Default limits are 4 requests, 128 MiB/image and 256 MiB aggregate. Count reserved
in-flight and completed images together.


## 1. Executive decision

DeckCraft already uses the 0.36 `eframe::App::logic` / `App::ui` split and the root `Ui`; the
0.34–0.36 "more `Ui`, less `Context`" transition is **not** new work here. The 0.37 migration is
therefore mostly:

1. a bounded API migration (`Role`, `Id::unique`, callback-based screenshots, `Event::MouseWheel`
   `source`, new IME/clipboard events, kittest accessibility/glyph checks);
2. a lifecycle hardening pass so hidden and multi-pass frames do not duplicate or drop app side
   effects;
3. a pinned suite fork and winit integration for bidi/language, IME, non-Latin shortcuts,
   Wayland clipboard and the egui-wgpu surface fixes;
4. measured UI regressions and performance, with explicit acceptance thresholds.

Do **not** adopt `craft-ui`, `craft-window`, `egui_dock` or a new generic compatibility layer as part
of this migration. DeckCraft does not depend on `craft-ui` today; `craft-ui` PR 199 and language
work #198 are useful precedent and the eventual shared home, not a reason to replace DeckCraft's
ribbon, panes, canvas, dialogs, commands or persistence. The only `craft-libs` dependencies in
DeckCraft remain `craft-pdf` and the `krilla` patch at their current immutable revision.

---

## 2. Baseline inventory

### 2.1 Exact revisions and dependency state

| Item | Baseline |
|---|---|
| DeckCraft | baseline snapshot of this plan `9aef21e627cb447dd2180cd804db85343e73e5cf` (superseded dirty snapshot); historical planning baseline `b47195feec0f0fe4220d8943ad1c8add5dad8072`. Current reviewed local base is `770f4653bff0700b33d8475181e168da90a7ad4f` (dirty union, not a qualified commit) |
| `Cargo.lock` | format 4; `egui`/`eframe`/`egui-wgpu`/`egui-winit`/`egui_kittest`/`epaint`/`ecolor`/`emath`/`epaint_default_fonts` all `0.36.2` |
| winit | `0.30.13` from crates.io (`a6755fa58a9f8350bd1e472d4c3fcc25f824ec358933bba33306d0b63df5978d`) |
| wgpu | `30.0.1` in this lock. Upstream egui `0.37.0` holds `30.0.0` (its `Cargo.toml` comment keeps that version for `wasm-bindgen-futures`); they are semver-compatible but a different lock, so the wasm dependency set must be re-verified after the bump |
| accesskit | `0.24.1`; `accesskit_consumer` 0.35.0, 0.36.0 and 0.38.0 both present; `kittest` 0.4.0 |
| craft-libs | `craft-pdf = "=0.1.0"` and `[patch.crates-io] krilla = "=0.8.2"`, both at rev `c3487320b2d1de657b860b3c8abb44e1f2b32bd8` |
| MSRV | workspace `rust-version = "1.95"`, same as egui 0.37.0 |
| Stock experiment target | upstream tag `0.37.0`, peeled commit `841a328ba2dd8f6996beb7189ebb7fc00e668a2a`. This is a local comparison experiment, never a staged consumer release |
| Candidate fork sources (comparison only) | `aiongg/egui` `craft/0.37` at `a4f9a17f64a6bae61350fb85804c9df0d9865916`; `aiongg/winit` `craft/0.30.13-lang` at `8b35388bd719e8cc86a95971ba0bc026c9a8f058`. These personal-fork SHAs are not production pins |
| Production pin requirement | reviewed, published, immutable **storytold** full SHAs for the complete egui family **and** winit, including W15P PRIMARY; one coherent atomically landed graph. No `aiongg`/personal fork in a production manifest and no stock-only intermediate release |
| Prior published 0.36.2 | storytold/egui `craft/0.36.2` at `767b1cad294120689a514bb4bd68c077e46679b9` |
| craft-libs reference | merged PR 199 base `3832c47ef68a0e6214f2c1e85f3526b6adcb686e` (current shared-library implementation base). Historical plan-only source `8614e4a04fae516fba16c22b31ff1008cb6be10b`/README `1764bca` and main `49190884278f16c9a9f5823d7698a8ffb5deb062` are superseded. Do not use PR head `b68de8c5f4f1c060904d211a7b1320aa0f7bce78` or audit merge `ec8840af8c5fda695cf195cdd05e5ae078415c07` as trees |

The contract's `0.37.0` annotated tag object SHA is not the peeled commit SHA; all pins in this
plan use the peeled commit. A branch name is not a publication pin. The `aiongg` revisions above
are source-review inputs; production manifests must name reviewed storytold commits, and the
stock 0.37.0 stage exists only to isolate the API diff.

### 2.2 Manifests and packages

| Manifest/package | egui-family use | Notes |
|---|---|---|
| root `Cargo.toml` | workspace `egui = "0.36"` with no feature, `egui_extras = "0.36"` (unused), `eframe = "0.36"` (`default-features = false`), `egui_kittest = "0.36"`; `[patch.crates-io] krilla` | `egui_extras` has no consumer; do not add or patch it "for completeness" |
| `crates/ui-egui/Cargo.toml` (`deckcraft-ui-egui`) | `egui = { workspace = true }` with **no `default_fonts`**; dev `egui_kittest` with `wgpu, snapshot`; build `serde_json, toml` | Library and example `examples/icon_gallery.rs`. Add `features = ["default_fonts"]` here (or to a shared workspace feature) so standalone `cargo test -p deckcraft-ui-egui` does not depend on an app's feature unification |
| `crates/fonts/Cargo.toml` (`deckcraft-fonts`) | direct `epaint_default_fonts = "0.36"` | Must move to 0.37 and be patched to the same fork source when the fork is used, or a second `epaint_default_fonts` source appears |
| `apps/deckcraft/Cargo.toml` (`deckcraft`) | `eframe` with `default_fonts, wgpu, accesskit, links, wayland, x11`; `egui`; native `rfd, arboard, image, cpal`; macOS `fmv-macos-events`; Linux/BSD direct `winit 0.30` with X11 | `eframe` defaults are off, so the feature list is explicit; winit must be patched consistently with eframe on Linux/BSD |
| `apps/deckcraft-web/Cargo.toml` (`deckcraft-web`) | wasm-only `eframe` with `default_fonts, wgpu, accesskit`; `rfd`, `wasm-bindgen`, `web-sys` | WebGPU with WebGL2 fallback; trunk build |
| `apps/deckcraft-cli`, `crates/mcp`, engine and below | no egui | Unaffected except through `deckcraft-ui-egui` tests |
| `xtask/Cargo.toml` | no egui | CI/bench/parity tooling |
| `apps/deckcraft/build.rs` | Windows icon/VERSIONINFO only | No egui change |

There are no `vendor/` directories, no nested Cargo workspaces, no `[patch]` sections outside the
root, and no `egui_extras`, `egui_plot`, `egui_dock`, `egui_tiles` or `egui_system_font_provider`
consumers. The only vendored/forked code relevant to this migration is the suite's egui/winit
forks and the existing `krilla` patch, which must be preserved.

### 2.3 Entry points and target-specific code

- `apps/deckcraft/src/main.rs:39-62` — `impl eframe::App for App`: `logic`, `raw_input_hook`,
  `ui`, `on_exit`. `on_exit` saves preferences and, on macOS, exits before eframe/AccessKit tears
  down the window (#279).
- `apps/deckcraft/src/main.rs:212-311` — `main`: `NativeOptions`, title bar/fullsize flags,
  app id/icon, X11/Wayland choice, Windows DX12/FXC configuration, macOS Apple events, run_native,
  `SlideApp::new`, preferences, AutoRecover open, control server, sample/open files, show start.
- `apps/deckcraft/src/control_server.rs` — localhost JSON-lines control server; passes a cloned
  `egui::Context` to requests; has tests for timeouts/abandonment.
- `apps/deckcraft/src/display.rs` — Linux/BSD X11 vs Wayland choice; `with_x11` event-loop hook.
- `apps/deckcraft/src/graphics.rs` — Windows DX12-only wgpu instance and FXC compiler defaults.
- `apps/deckcraft/src/apple_events.rs` — Finder/quit Apple events; calls `ctx.request_repaint`.
- `apps/deckcraft/src/pasteboard.rs` — arboard HTML and image pasteboard.
- `apps/deckcraft-web/src/main.rs` and `web.rs:76-116` — wasm shell, `WebShell::{logic, raw_input_hook, ui}`;
  browser drop-read and picker inbox; downloads.
- `crates/ui-egui/src/lib.rs` — `SlideApp` state (`:435-518`), `run` (`:656-674`), `run_ui`
  (`:838...`), `logic` (`:1540-1602`), `raw_input_hook` (`:1652-1655`), `keyboard`
  (`:1668-1800`), `ui` (`:1798-1872`), screenshot queue (`:1903-1972`), `now_ms`
  (`:2001-2011`). `canvas::ime_output` (`:321-333`) constructs a custom `IMEOutput`.
- `crates/ui-egui/src/doc_window.rs:84-117` — `show_viewport_immediate` second document window.
- `crates/ui-egui/src/show.rs:1757-1885` — presenter `show_viewport_immediate`; `show::ui`
  (`:521...`) full-screen show.
- `crates/ui-egui/src/panes.rs`, `status.rs`, `thumbs.rs`, `sorter.rs`, `backstage.rs`,
  `ribbon.rs` — root-`Ui` panels and nested panel order.
- `crates/ui-egui/src/theme.rs` and `uifonts.rs` — custom `FontDefinitions`, font fallback,
  `ctx.set_fonts`; `theme.rs` tests use `ctx.run_ui`.
- `crates/ui-egui/src/textures.rs` — `max_side`, `fit_texture`, `shrink_to`,
  `editor_texture_options`, texture cache; important for the 0.37 surface/texture limits.

### 2.4 Existing tests, examples, CI, performance

- Extensive unit/integration tests live in `crates/ui-egui/src/*_tests.rs` and inline `#[cfg(test)]`
  modules. Most UI tests build `egui_kittest::Harness` with
  `build_ui_state(|ui, app| { app.logic(ui.ctx()); app.ui(ui); }, SlideApp::new(...))`.
  `ui_soak_tests.rs` has an ignored long soak selected by `DECKCRAFT_UI_SOAK_STEPS`.
- There are no committed snapshot images or snapshot calls in the baseline UI tests; the
  `snapshot` kittest feature is enabled but only a few tests call `h.render()` to `/tmp`.
- `crates/ui-egui/examples/icon_gallery.rs` renders the icon set; it is not a UI golden test.
- `cargo xtask ci` is the repository gate: `fmt --check`, `pdf-deps`, `clippy -D warnings`,
  `cargo test --workspace`, asset attribution, layering, wasm32 checks. `cargo xtask bench`
  wraps `deckcraft-cli bench` and compares against `docs/benchmarks/baseline.json`; **that file
  does not exist in the baseline**, so the first implementation task is to create a baseline in
  evidence, not to treat a missing file as a pass.
- GitHub CI is Ubuntu-only for gates; FreeBSD is a reusable workflow; Windows ARM64 and release
  packaging are separate; macOS is release-only. The Windows build/GUI evidence must come from a
  fresh `VMs/craft-windows` clone under the server workflow, not from Linux cross-compilation.
- Historical compiler diagnostics under
  `/home/codex/work/ssd2/task-evidence/egui037-audit-20261022/{fork,stock}/deckcraft.errors`
  are clues about the old source state, not acceptance for this checkout. They identify `Role`
  renames, `UserData`/`Event::Screenshot`, and `Event::MouseWheel.source`.

Historical errors and their intended target (recheck against the current tree; the audit
checkouts are not this checkout):

| Historical diagnostic | Likely source anchor in the baseline | Intended target |
|---|---|---|
| `no variant ... TextEdit` for `Role` | `dialogs.rs:553,601,712` | `Role::TextInput` (and a real accessible name) |
| `no variant ... Other` for `Role` | `panes.rs:1077,1079` | `Role::Unknown` initially; then decide on a focusable `Role::Slider` |
| `no variant ... SelectableLabel` | `selpane.rs:160` | `Role::Button` with selected state |
| `no variant ... Checkbox` | `widgets.rs:162` | `Role::CheckBox` |
| `cannot find UserData in egui` | `lib.rs:1931` | `ScreenshotCallback`/`Context::request_screenshot` |
| `no variant ... Screenshot` in `Event` | `lib.rs:1951` | callback result channel, no event scan |
| missing `source` in `Event` initializer | `canvas.rs:1380`, `lib.rs:3300`, test wheel events | `MouseWheelSource::Wheel`/`Unknown` |

---

## 3. What actually changes in egui 0.37

### 3.1 Breaking and behavior-changing items used by DeckCraft

Verified against the stock `0.37.0` sources and the suite candidate:

| Area | 0.36.2 baseline | 0.37.0 target | DeckCraft action |
|---|---|---|---|
| Widget roles | `egui::WidgetType` | deprecated alias of `accesskit::Role`; `TextEdit -> TextInput`, `Checkbox -> CheckBox`, `SelectableLabel -> Button`, `Other -> Unknown` | Replace imports and variants; keep `ComboBox`, `Button`; choose semantic roles for custom controls where safe |
| ID API | `Id::new`, `AsId`, `Ui::id` | `Id::unique`; `Ui::scope_id` (with `Ui::id` deprecated); `UiBuilder::scope_id`; `Ui::unique_id` is globally unique but order-dependent and explicitly unstable; `Panel` constructors take `impl AsIdSalt` | Replace `Id::new` with `Id::unique` and every `ui.id()`/`UiBuilder::id` with `scope_id` (38 `ui.id()` references in the baseline; `UiBuilder::id` is not used today). `Ui::scope_id` is the stable rename of the old `Ui::id`; use `Ui::unique_id` only for transient widget/accessibility relations, never persisted state. Deprecations are fatal under `-D warnings`; `Response::id` is not affected |
| Screenshot | `ViewportCommand::Screenshot(UserData)` + `Event::Screenshot { user_data, image }` | `ViewportCommand::Screenshot(ScreenshotCallback)`; `Context::request_screenshot`; `UserData` and the event are gone | Replace the queue with one bounded completion slot per admitted request (§4.3); preserve token/path/reply/deadline semantics |
| Wheel events | `Event::MouseWheel { unit, delta, phase, modifiers }` | adds `source: MouseWheelSource` | Add `MouseWheelSource::Wheel` for wheel tests/synthetic input; `Unknown` where the source is genuinely unknown |
| Clipboard | `Paste(String)`; image workaround in `keyboard` | adds `PasteImage(Arc<ColorImage>)` and `MiddleClickPaste { pos, text }`; PRIMARY support in egui-winit | Handle `PasteImage` directly; keep the image workaround only as a compatibility fallback. Candidate E4 still routes PRIMARY through arboard; the required shared W15P winit/SCTK path is a separate prerequisite (§4.8, §5.2) |
| IME | 0.36.2 **already has** `ImeEvent::DeleteSurrounding { before_chars, after_chars }` and the deprecated `Enabled` variant | stock 0.37 keeps the same incoming events; the suite fork E21 adds `DeleteSurroundingBytes`; **new E21b** adds outgoing surrounding text to `IMEOutput` | DeckCraft silently ignores `DeleteSurrounding` today (`_ => Ok(Value::Null)`), so this is a pre-existing gap, not a new 0.37 API. Handle the stock and fork variants, and supply the E21b model-aware producer from `canvas::ime_output`; do not claim E21/E21b support without them |
| kittest | unnamed inputs accepted; missing glyphs draw tofu | unnamed visible inputs panic by default; missing glyphs panic by default (`MissingGlyphPolicy::Panic`), `allow_missing_glyphs()` opts out | Audit and label text fields; keep panic policy for curated UI screens; opt out only for arbitrary-script soak tests |
| Accessibility | `WidgetType`; fewer names/roles | `Role`; `Response::accessible_name`, `labelled_by`, `Ui::name_inputs`, panel resize handles, invisible widgets hidden; `Response::widget_info` **does** register an AccessKit role/label/value | Add names only where missing; keep and repair existing `widget_info` where it already supplies role/label/value. Do not treat `widget_info` as output-only and do not replace a working role with a bare name |
| Panels/root UI | panels on a root `Ui`, state keyed by the supplied `Id` | same model, but `Panel` stores `IdSalt` and keys by `parent_ui.scope_id().with_salt(self.id_salt)`; panel/Scene clip respects parent (#8562) | Keep the current panel order and nesting; unchanged strings still produce different effective IDs, so keep parent-salted panel identities; assert app-owned UiState dimensions survive save/reopen; no persisted egui panel reset exists (§4.5). Never call `root_ui` inside `App::ui` |
| Default fonts | egui's default features included `default_fonts` | `default_fonts` is no longer an `egui` default; eframe still enables it by default; only `apps/deckcraft` and `apps/deckcraft-web` enable it explicitly | Add `egui/default_fonts` to `deckcraft-ui-egui` itself or a shared workspace feature, keep the explicit app features, and test the library standalone; `theme::font_definitions` calls `FontDefinitions::default()`, which is empty without the feature |
| `AsId` / deprecated APIs | used | removed | Remove any remaining uses; `-D warnings` makes deprecations failures |
| root UI API | `Context::run` | `Context::run_ui`, `Context::run_pass`, `Context::root_ui`; `root_ui` panics if the root `Ui` is already borrowed (#8535) | Tests/headless use `run_ui`; app code never calls `root_ui` from inside `App::ui`/`run_ui` |
| hidden windows | eframe has `update` (logic+ui) and `update_logic_only` (logic only) | same contract | Both native and web call `App::logic` and `App::ui` inside `Context::run_ui` per pass. The minimized/occluded root uses `run_logic` only when no paint is wanted; a pending screenshot, `RequestPaintWhileHidden`, or a visible descendant forces a full UI pass. Never use visibility as a logic-only test |
| multi-pass | `request_discard` may call the UI callback more than once | same; later passes receive empty event queues after `RawInput::take` | Guard only genuinely once-per-layout-frame work. `end_pass` takes the output, so `current_pass_index()` is zero again after a completed pass; it is not a hidden/logic-only signal. Prefer consumed queues and monotonic deadlines for background work |
| app trait | `logic` + `ui` already used | unchanged shape; `on_exit(&mut self)` still used with wgpu (no glow feature) | Keep `App` wrapper; no wholesale `update -> ui` rewrite |

### 3.2 Source-verified facts that drive the lifecycle design

- Both native and web call `App::logic` then `App::ui` inside the `Context::run_ui` closure
  (`eframe/src/native/epi_integration.rs::update`, `eframe/src/web/app_runner.rs::logic`). They can
  call the pair once per layout pass, including discarded passes. The app's own `logic` is not a
  once-per-frame native timer.
- `Context::run_logic` runs no pass and replaces only `RawInput::viewports` and `RawInput::focused`;
  events, input time and layout remain those of the last UI pass. Eframe uses it for a hidden root
  only when no paint is wanted (`update_logic_only`).
- A hidden root still gets a full UI pass when a screenshot is pending
  (`ActionRequested::wants_paint`), when the app sent `ViewportCommand::RequestPaintWhileHidden`, or
  when the root is hidden but a descendant viewport is visible. That full pass calls `App::logic`
  **and** `App::ui` with current input. `visible() == Some(false)` therefore does not prove a
  logic-only callback, and `unwrap_or(true)` cannot turn stale `run_logic` input into fresh input.
- `RawInput::take()` moves `events` and `dropped_files` out of the first pass, so later passes of
  the same frame have empty event queues. That is a useful consequence, not a substitute for
  explicit phase routing.
- `Context::current_pass_index()` is `output.num_completed_passes`: zero on the first pass,
  incremented per discard, and zero again after a normally completed pass because `end_pass`
  takes the output with `mem::take`. It can guard genuinely once-per-layout-pass work under this
  pinned integration, but it cannot distinguish a fresh UI pass from `run_logic`, and it does not
  stay nonzero after completion.
- `ViewportInfo::visible()` reflects minimized/occluded state; `None` means unknown. Use it only for
  minimized/occluded presentation policy, never as an input-freshness test.
- `ViewportCommand::Screenshot` now carries a one-shot `ScreenshotCallback`; hidden windows still
  paint for a pending screenshot. The callback does not request a repaint, so the app must wake its
  own logic/viewport to deliver the result.
- `Context::root_ui` panics when called while the root `Ui` is borrowed (#8535).
  `show_viewport_immediate` callbacks receive their own root `Ui`; do not call `ctx.root_ui` there.

---

## 4. DeckCraft lifecycle and root-Ui design

### 4.1 Frame phases

Keep `SlideApp` as the owner of all app state. Route work by what consumes it, not by visibility:

1. **Background work (`SlideApp::logic`, every call)** — work that must continue when nothing is
   painted and that is safe to call more than once per frame:
   - `drain_control` (take each request once), `poll_opening`, `drain_inbox` once fonts are ready;
   - screenshot result collection, cancellation and monotonic deadlines;
   - AutoRecover deadline checks;
   - media player clock, owner cleanup and `session.media_status`;
   - slide-show time advancement (transitions, triggered animations, auto-advance, media cues),
     presenter timer and editor animation-preview clock, driven by a monotonic app clock;
   - frame/pass bookkeeping and repaint scheduling.
   These are consumed queues or monotonic-deadline work, not interpretations of UI events. A
   first-pass guard may protect a genuinely once-per-layout-pass advancement, but it must not gate
   fresh async results, control commands, screenshot results or other queue drains.
2. **Current UI phase (`SlideApp::ui`, on every real pass)** — work that consumes this pass's
   input and must also run on a forced hidden paint:
   - keyboard, clipboard, IME and pointer actions;
   - native dropped-file handling;
   - UI text/font observation (`note_ui_text`) from this pass's input and visible document;
   - drawing the current slide-show/editor-animation state;
   - commands triggered by widgets, and `drain_requests` after them (a queue drain, safe on replay).
3. **Render-adjacent helpers** (`textures`, `media` texture sync, `screenshots`) — called from `ui`
   or from the app's result-drain path and never own document state.

The `App` wrapper in `apps/deckcraft/src/main.rs` and `WebShell` in `apps/deckcraft-web/src/web.rs`
remain thin: Apple/drop events, then `app.logic(ctx)`, then `app.ui(ui)`. No generic context
bridge is added.

### 4.2 Exactly-once rules

- **Background work is drain- or deadline-driven.** Control requests, inbox events, opening
  results and screenshot results are taken once (`std::mem::take`, `try_recv`, `retain`), so
  repeated `logic` calls cannot replay them. Timers compare a monotonic deadline, not stale
  `ctx.input(|i| i.time)`.
- **UI input is handled only in the known UI phase.** Keyboard, clipboard, IME and native drops
  read `ui.input`/`ctx.input` in `ui`, where a forced hidden paint also arrives. Do not infer a
  logic-only callback from `visible()` and do not suppress input because a pass index is nonzero.
- **Autosave, media and show advancement are not visibility-gated.** They run from `logic` on every
  call (subject to a monotonic deadline or consumed queue) so a minimized window still writes
  AutoRecover, ticks audio/media and advances a show.
- **A first-pass guard is only a per-layout-pass deduplication.** Under this pinned integration it
  may protect work that would otherwise run twice in one discarded multi-pass frame. It is not a
  hidden-window test and it must not skip fresh input or background results; each such guard gets
  a regression proving a normal hidden call still performs the work.
- **Queues are drains:** a control request whose sender gave up (`ControlRequest::take`) is never
  re-run after its reply timeout.
- **Undo:** only commands create undo entries; `logic` must not create empty transactions.
  `Session`'s existing composition/undo rules stay; `text.compose` + `text.commit` remain one
  composition undo step, and E21b's outgoing state does not edit the document.
- **No layout side effects in logic:** do not call `root_ui`, do not create panels, and do not
  initialize fonts only to measure a widget from logic. Font installation may be scheduled from
  logic, but observation and layout happen in `ui`.

Concrete placement:

- `crates/ui-egui/src/lib.rs:1540` `SlideApp::logic`: run `drain_control`, `poll_opening`,
  `drain_inbox`, screenshot result/deadline work, the monotonic autosave check, media clock,
  show/animation advancement and repaint scheduling. Do **not** guard these with `visible`.
- `crates/ui-egui/src/lib.rs:1607` `note_ui_text` and the native `dropped_files` loop
  (`:1595-1604`) move into `ui` (or a method `ui` calls) and read the current pass only.
- `crates/ui-egui/src/lib.rs:1652` `raw_input_hook`: synthetic events are injected once per frame
  by eframe before the pass. `logic` does not consume those events, but it must detect an admitted
  batch in the live `InputState` and defer target-changing control/inbox/opening drains one pass:
  after `begin_pass` egui has already derived `PointerState`/`keys_down`, so retracting only the
  raw events cannot keep a click or key out of a document that same pass's logic opens.
- `crates/ui-egui/src/lib.rs:1668` `keyboard`: keep event-driven; called only from `ui`.
- `crates/ui-egui/src/lib.rs:1798` `ui`: keep panel order; run input handling once; call
  `drain_requests` after commands.
- Tests that manually drive the app must mirror the real host: call `logic` then `ui` inside
  `Context::run_ui` for real passes, and use `Context::run_logic` for hidden logic-only iterations.
  A `#[cfg(test)]` `run_frame` helper is justified because dozens of tests need identical
  semantics, but it must not become a production abstraction and must not pretend native calls
  logic once per frame outside `run_ui`.

### 4.3 Screenshots and control-channel replies

This is the largest concrete refactor because `UserData`/`Event::Screenshot` is gone. Follow the
shared completion-slot contract in `craft-libs/docs/egui-037-suite-rollout.md` rather than inventing
a second global token map.

**Target design in `crates/ui-egui/src/lib.rs`, `control.rs` and `media.rs`:**

- Admit each `ui.screenshot` request into a `PendingShot` record: numeric token, optional path,
  `Reply` sender, target `ViewportId`, issuance geometry/DPI plus geometry epoch, monotonic deadline, and one
  completion slot. The slot holds `Option<Arc<ColorImage>>` and a live wakeup handle (a cloned
  `Context` is acceptable *in the slot*, which the app owns); the callback holds only a
  `Weak` reference to the slot.
- The callback captures no strong `Context` and no owning app state. On completion it stores the
  image at most once under the slot lock, drops the lock, and wakes the viewport. If the request
  was cancelled, timed out or the window closed, the image is released and no file or reply is
  produced.
- Admit at most 4 screenshot requests, 128 MiB image bytes per request and 256 MiB
  aggregate reserved in-flight/completed bytes, independently of the control server's
  64-request bound. Checked width × height × 4 arithmetic must reject overflow and
  oversized/over-budget admission with a JSON error; never evict an earlier request.
  Reserve before issue; release on drain/cancel/timeout/close. Test equality and one-over
  boundaries, concurrent requests and eventual zero retained bytes. Transient GPU/readback
  and encoder RSS is a separate measured bound, not hidden inside the slot budget.
- Record logical content/screen rect from the target viewport's known UI phase, not
  `ViewportInfo::inner_rect` (unavailable on Wayland). The callback has pixels only.
  Track a geometry epoch across resize/DPI/zoom/surface changes; revalidate before issue
  and save, rejecting/retrying any epoch change, including a change back. Derive x/y
  image scale from returned pixel dimensions and the unchanged recorded logical bounds;
  check aspect/rounding and drop stale/oversized images. Never label old pixels with
  current geometry merely because completion happens in that frame.

- `issue_screenshots(ctx)` uses the app's monotonic clock (not `now_ms`) for the settle delay
  (for example 120 ms and at least 2 settled frames when there is a real pass) and calls
  `ctx.request_screenshot(...)`/`ViewportCommand::Screenshot(...)` once per admitted request.
- A screenshot itself requests hidden paint, so a hidden/minimized window runs `App::ui` with
  current input. Test screenshot-only hidden capture separately: the command itself forces
  paint. For a synthetic-input-only gesture, remove RequestPaintWhileHidden and assert the
  gesture stalls; restore it per step and assert one result. Geometry uses the epoch check above.
- `drain_screenshot_results` runs from logic, matches the token plus viewport/request identity,
  calls `control::save_screenshot` on the app thread and sends exactly one reply. A late callback
  after timeout/close does nothing.
- Keep the existing JSON shape (`path`, `width`, `height`, base64 when no path) and the 8 s
  deadline error (`no frame was presented ... use ui.render`), measured on the monotonic clock.
- Remove `Event::Screenshot` scanning, `UserData` and `collect_screenshots`.
- Kittest 0.37 fulfills screenshot requests from viewport output but renders one image once and
  completes every callback with that same image, in collection order
  (`crates/egui_kittest/src/lib.rs:913-946`). It therefore cannot prove token/path correlation or
  reverse-order completion. Correlation tests must capture the callbacks from viewport output and
  complete them manually with distinct images in a small test harness.

### 4.4 Commands, undo, save/reopen

No command or persistence contract changes:

- `SlideApp::run` remains the single app/UI command entry point; engine commands still go through
  `Session::execute`; UI commands remain in `UI_COMMANDS`/`run_ui`.
- `control::handle` continues to call `app.run`, `app.save_all`, `app.open_path`, etc.
- `Session` history remains snapshot-based; do not add undo entries for `logic`, screenshots,
  autosave, focus changes, copy/cut, or theme/font installation.
- `.deckcraft`, `.pptx`, recovery and settings formats are unchanged. `UiState`/`Prefs` serde
  compatibility tests continue.
- A frame replay (Grid sizing pass) must produce the same document and one undo entry. Add a
  mutation-check test that forces `request_discard` while clicking a mutating control and asserts
  one undo entry, one dirty revision change and one status.

### 4.5 Root `Ui`, panels, IDs, clipping

Current order is intentional and should be preserved:

- Root `Ui` in `SlideApp::ui`: `ribbon::title_bar` (top), `ribbon::show` (top), `status::status_bar`
  (bottom), then a `CentralPanel` that contains the start screen/backstage or the editor.
- Inside the editor central panel: `panes::show` (right), `thumbs::show` (left) or
  `sorter::outline_pane` (left), a nested `CentralPanel` for notes/canvas; `canvas::show`.
- Backstage: `Panel::left("backstage_nav")` and a `CentralPanel` inside the root central panel.
- Document window: `show_viewport_immediate`, `CentralPanel`, `Panel::top("doc_window_bar")`,
  `CentralPanel`.
- Presenter: `show_viewport_immediate`, `CentralPanel`; no root panel additions.

Actions:

- Keep the panel salts (`title_bar`, `ribbon`, `status`, `task_pane`, `thumbs`,
  `outline`, `backstage_nav`, `doc_window_bar`, `notes`). 0.37 changes effective panel
  IDs to parent-salted values. DeckCraft has no eframe persistence feature, so there is
  no saved egui PanelState blob to migrate or reset. App-owned `UiState` dimensions
  (`pane_width`, `thumbs_width`, `notes_height`) remain the durable source of truth.
  Outline width is session-local at baseline; preserve resizing within the session,
  without inventing a new durable outline preference in this port. Test actual settings
  save/reopen and 1×/2× geometry. `Ui::scope_id` is the old stable id; `unique_id` is
  not a persisted identity. No optional remap/reset decision remains.

- Do not move panels into `Context::root_ui`; the app already has the root `Ui`. The only
  legitimate `root_ui` calls are in tests/headless code that only owns a `Context`.
- 0.37's parent-clip fix (#8562) can change nested panel/Scene clipping. Assert `canvasRect`,
  `slideRect`, pane widths, and that no content draws over the title bar/status bar at 900×560
  and 1440×900, at 1× and 2× DPI.
- Never overwrite a saved preferred panel size with a clamped displayed size (craft-ui guidance);
  keep `pane_width`/`thumbs_width`/`notes_height` semantics unchanged.

### 4.6 Viewports and docking

There is no docking library. The app's viewports are:

- `view.newWindow`: immediate viewport `Id::from_hash_of("deckcraft_doc_window")`, title `name:2`,
  960×640, min 480×360; shows the live document texture, next/previous buttons, close request.
- Presenter view: immediate viewport `Id::from_hash_of("deckcraft_presenter")`, 1100×700, notes,
  clock, controls.
- Full-screen show: root viewport enters/exits fullscreen; no child viewport.

Actions:

- Keep `show_viewport_immediate`; do not replace with `root_ui`/`run_pass`.
- In 0.37 the immediate callback already receives a `&mut Ui`; keep `CentralPanel` last within
  each callback. Do not add panels after `CentralPanel`.
- Test: open document window, move its slide, confirm the editor selection is unchanged; close
  the window, confirm the document stays open; start/stop show and presenter; verify fullscreen
  command and no stuck fullscreen after exit/close.
- Test hidden parent: minimize/occlude the root (or run logic-only) and verify a visible immediate
  child still receives passes when eframe's descendant-visible rule applies; record any failure
  against upstream #8748 (closed and **not merged**; not in 0.37.0). A forced hidden paint is the
  second case: `App::ui` must run and current input must be handled. Do not silently claim docking
  behavior that the app does not have.

### 4.7 Fonts, glyph coverage and accessibility

**Font policy decision:** keep DeckCraft's custom `FontDefinitions` (`theme::font_definitions`),
keep the explicit `default_fonts` feature on **both the app and the UI library**, and keep
`UiFonts`' fallback selection. Do **not** enable eframe's `system_fonts` feature in this migration.
Reasons:

- `UiFonts` already chooses installed faces for missing UI glyphs, with a per-face size limit and a
  color-face rejection policy; enabling `egui_system_font_provider` would add a second, less
  controlled fallback path and the upstream-reported system-font startup cost (#8512 comments).
- Screenshot/layout determinism across machines is easier with the explicit face order.
- Color emoji rendering is not a DeckCraft requirement for this migration; slide rendering uses
  its own text pipeline. If color emoji is later required, make it a separate product decision
  with startup, memory and golden-image evidence.

Actions:

- Add `features = ["default_fonts"]` to `deckcraft-ui-egui`'s `egui` dependency (or a shared
  workspace feature consumed by both the library and apps). Today only `apps/deckcraft` and
  `apps/deckcraft-web` enable it; `crates/ui-egui/Cargo.toml:21` does not, and in 0.37
  `FontDefinitions::default()` is empty without the feature. `theme::font_definitions` builds the
  UI families from it, and `examples/icon_gallery.rs` is affected too. Test
  `cargo test -p deckcraft-ui-egui` standalone with and without `CRAFT_FONTS_DIR`; do not rely on
  app feature unification.
- Update `crates/fonts/Cargo.toml` `epaint_default_fonts` to `0.37.0` (or the same reviewed
  storytold source). Its constants (`HACK_REGULAR`, `UBUNTU_LIGHT`) remain the last-resort face.
- Keep `FontData::from_static`/`from_owned`, `FontDefinitions`, `FontFamily::Name("bold")`,
  `data.tweak.hinting` usage; verify against 0.37 source. `ctx.set_fonts` applies immediately
  when no text is laid out; `UiFonts::apply` must still request a repaint and must not install
  fonts on every frame.
- **Bound `UiFonts` aggregate growth.** `MAX_FACE_BYTES` is 128 MiB *per face*; `faces` is an
  unbounded vector and every newly found face is appended, so "bounded" must not be claimed
  without an aggregate byte/family budget or a deliberate reword. Add an aggregate budget and test
  adversarial multi-script input, duplicate faces and replacement behaviour.
- Production missing-glyph policy: egui's default `Tofu` is acceptable (never panic in
  production); do not set `Panic` outside tests. In tests, keep kittest's default `Panic` for
  curated DeckCraft screens so a literal UI glyph that no interface face draws is caught. Use
  `HarnessBuilder::allow_missing_glyphs()` only in the soak test that deliberately types
  arbitrary scripts.
- Inventory literal UI glyphs (`→`, `↘`, `↓`, `↙`, `←`, `↖`, `↑`, `↗`, `•`, `○`, `■`, `□`, `◆`,
  `➢`, `✓`, `★`, `◀`, `▶`, `◉`, `⟳`, `–`, etc.) on the ribbon, symbol dialog, animation pane and
  show controls. Replace a glyph with a code-drawn icon where it is decorative and not covered;
  do not add a new font file without an `ATTRIBUTION.md` row and licence review. If a glyph is
  intentionally unsupported in a test, isolate that test and document why.
- Add an accessibility name to every visible input. Use visible labels with `Response::labelled_by`
  where a label exists; use `Response::accessible_name` for unlabelled compact fields
  (`widgets::spinner`, the color hex field, the scheme name, search fields whose hint is not a
  real name, etc.). `hint_text` counts as a kittest fallback name, but a visible label is better.
- **Keep and repair `widget_info`.** In 0.37 `Response::widget_info` fills/registers the AccessKit
  node role, label, value and selected/toggled state; the premise that `WidgetInfo` events are not
  an accessibility tree label is false. Existing custom controls that already have usable
  role/label/value through `widget_info` should stay and be fixed, not be replaced wholesale with
  `accessible_name` (which may lose role/value). Use `labelled_by`/`accessible_name` only for
  genuinely unnamed inputs, and verify both through kittest.
- Role mapping:
  - `dialogs.rs:553,601,712` `WidgetType::TextEdit` -> `Role::TextInput`;
  - `panes.rs:1077,1079` `WidgetType::Other` -> `Role::Unknown` for compile parity, then decide
    whether the animation timing body/edge should become a focusable `Role::Slider` with
    `WidgetInfo::slider` and Left/Right keyboard adjustment. Do not present a slider role without
    keyboard operation; either implement that interaction or keep it as an explicitly named
    custom control.
  - `selpane.rs:160` `SelectableLabel` -> `Role::Button` (upstream's table); keep selected state.
  - `widgets.rs:162` `Checkbox` -> `Role::CheckBox`.
  - `selpane.rs`/`sorter.rs` imports: `WidgetType` -> `Role`; all `WidgetType::Button`,
    `ComboBox` etc. become `Role::Button`, `Role::ComboBox`.
- Use `Ui::label_inputs_by`/`name_inputs` for grouped editors, `Area::role` for the toast/alert if
  it is announced, and leave `AccessKit` enabled on native/web as today.
- Add tests that query roles and names:
  - every visible text input has a label, including the library's own example/test harness;
  - `Role::CheckBox` toggled state and a custom `widget_info` control's role/label/value;
  - row selection uses `Role::Button` with selected state (or the chosen list role);
  - dialog OK/Cancel/Close names; File/Back names; panel/area names;
  - invisible widgets are not in the tree (0.37 #8621);
  - a deliberate missing-name mutation makes the accessibility test fail.

### 4.8 Input, IME, RTL and clipboard

- **Paste image:** handle `egui::Event::PasteImage(Arc<ColorImage>)` in `keyboard`: convert to RGBA
  bytes and call `insert.picture` with a PNG name, as the existing clipboard-image fallback does.
  Keep the existing `Event::Paste` empty-text + `services.clipboard_image` fallback for
  integrations that do not emit `PasteImage`; ensure the same paste does not insert twice when
  both arrive.
- **PRIMARY:** `Event::MiddleClickPaste { pos, text }` is handled by egui's focused `TextEdit`
  (notes, dialogs). DeckCraft's custom canvas does not consume it. Test middle-click on notes and
  on the canvas; the canvas must not insert text implicitly. Keep regular clipboard copy/cut
  behavior (`ctx.copy_text`, HTML/image services) unchanged.
- **PRIMARY backend is not solved by E4.** Candidate E4 routes PRIMARY through arboard
  (`egui-winit/src/clipboard.rs`, `arboard::LinuxClipboardKind::Primary`) even though its ledger
  says "through arboard meanwhile". The shared contract requires a winit-owned W15P path on the
  existing Wayland connection/event queue using SCTK 0.19.2's per-seat PRIMARY objects, landed and
  reviewed with a published storytold winit revision. Treat arboard PRIMARY as an unresolved fork
  gap, not a solved dependency feature. Test keyboard-only focus, serial freshness, owner/seat
  changes, empty/absent offers, self-paste, password exclusion, no unchanged-selection reclaim and
  canceled/slow transfers on wlroots/KWin/Mutter; ordinary paste is not PRIMARY acceptance.
- **IME composition (incoming):** `ImeEvent::DeleteSurrounding` is **not** new 0.37 API; 0.36.2
  already has it and DeckCraft ignores it in `ime_event` (`_ => Ok(Value::Null)`). This is a
  pre-existing gap. Keep `Preedit`/`Commit` mapping to `Session::compose_text`/`commit_text`,
  handle `DeleteSurrounding { before_chars, after_chars }` (stock) and the fork's
  `DeleteSurroundingBytes` when the fork is pinned, and add one narrow engine range-delete API
  that deletes a character/byte range around the composition caret as part of the same composition
  undo transaction (validate the range before mutating; never panic on hostile indices).
  When the application settles a composition itself (click/key/command), reject the pre-edits and
  commits already queued for it in that input batch, then retire the tombstone in the pass whose
  `should_interrupt_composition` is actually published: that publication is the backend's
  disable/re-enable reset boundary. Do not keep an owner-keyed tombstone past it, or legitimate
  later commit-only input (punctuation/full-width characters, direct IME commits) is lost forever.
  Gate that retirement on the actual output descriptor being absent: a focused stock `TextEdit`
  can publish its own `o.ime` while the canvas has no pending rect, and that pass is not a reset.
  `egui::ImeEvent` carries no generation, so an OS-queued event crossing the reset cannot be
  distinguished from a legitimate new composition; only same-batch events are deterministically
  stale. A discarded pass publishes nothing and must neither consume the interrupt nor end the
  tombstone.
- **E21b outgoing surrounding text is a new prerequisite, not existing capability.** The candidate
  egui-winit never calls `WindowExtWayland::set_ime_surrounding_text`, and `IMEOutput` has no
  logical text. Extend `IMEOutput` with an optional bounded payload: logical committed text,
  UTF-8 byte cursor/anchor and owning widget identity. The constructor must extract at most 4,000
  bytes at valid UTF-8 boundaries without cloning a large document; if a selection cannot fit,
  emit explicit `Unavailable` rather than misleading truncation; an available empty document
  is distinct and owner/context generation exists in both states. Exclude preedit,
  remap indices after delete/commit/cursor changes, and never supply text for password input.
  `PlatformOutput::append` must use the latest completed pass's IME value including None,
  replacing earlier descriptor/payload. W27 clears cached/protocol state through a committed
  disable/enable boundary on owner/unavailable transitions and clears old pending batches.
  Current-epoch done serial mismatch applies incoming edits once but defers outgoing state
  until a match; stale focus epochs cannot edit a new owner. Test text-to-non-text/password
  multipass clearing and ordinary updates preserving composition. DeckCraft's custom canvas producer (`crates/ui-egui/src/canvas.rs:321-333`) must supply
  logical model text and convert the slide-text selection/caret to byte indices; setting the new
  field to `None` is not acceptance. Test outgoing text immediately after delete+commit and
  deletion-only batches, preedit exclusion, reversed selection, supplementary/combining/RTL text,
  cursor movement, long lines/over-limit selections, field/window switching, password/focus/reset
  clears and multi-pass final output. Measure bytes allocated on large documents. E21b is not
  claimed as working E21/W27 behavior.
- **RTL/bidi:** UI labels, titles, file names and notes can be Arabic/Hebrew/CJK even though UI
  strings are English. The fork's E5/E12/E13/E14/E15/E16/E20 and winit W22/W27 provide bidi
  shaping, visual rows, grapheme caret/delete, line breaking and fallback baselines. Test:
  Arabic/Hebrew title in the title bar and slide thumbnail; caret/selection in notes; Delete/
  Backspace at cluster boundaries; mixed-direction labels; no glyph panic; screenshots reviewed.
- **Non-Latin shortcuts:** E7/E19 and winit W23/W25 map Latin-layout letters on non-Latin
  layouts. Test Ctrl+S/Ctrl+Z/Ctrl+C with a Russian/Arabic/Hebrew layout event sequence (where
  the integration can produce it), and a letter that has no Latin mapping must not fire a
  shortcut.
- **UI language:** DeckCraft is English-only (`docs/localization-parity.md`). Do not set a global
  `Context::set_language` or `Frame::set_ui_language` in this migration unless the UI actually
  localizes; an English root tag on Arabic document titles would be wrong. The fork's E17/E18
  hooks remain available for a later localization task. Per-widget language tagging is an open
  product question, not an API to invent here.

### 4.9 GPU, textures and rendering

- DeckCraft's lock holds wgpu `30.0.1`; upstream egui 0.37.0's lock holds `30.0.0` (held back for
  `wasm-bindgen-futures`). They are semver-compatible, but do not claim they are the same version:
  verify `egui::wgpu` re-exports, `eframe::wgpu`/`egui_wgpu::WgpuSetup` paths and the wasm
  dependency set after the bump. The Windows `Dx12Compiler::Fxc` setup remains valid in shape.
- Keep `textures::max_side`/`fit_texture`/`shrink_to` and `TextureOptions` (nearest magnification,
  linear minification, clamp) because the slide canvas is device-pixel aligned (#184). Do not let
  the 0.37 texture/`ColorImage` or predictable-filtering changes silently switch filtering.
- E2+E3 (`egui-wgpu` surface fits `max_texture_dimension_2d` and setup errors are returned) are
  relevant: DeckCraft already bounds its own textures but relies on eframe/egui-wgpu for the
  surface. Test a window larger than the adapter limit, a zero-sized/minimized surface, and a
  bad adapter/device creation path; assert no panic and a logged/returned error.
- Windows DX12 FXC: 0.37's `wrap_texel_coord` makes `Renderer::new` slower on FXC (upstream #8736).
  Measure startup on the Windows clone with 20+ fresh processes. If the regression exceeds the
  threshold, profile and either take an upstream fix or add a fork patch that preserves all wrap
  modes (not a clamp-only shortcut). Keep color-glyph/texture-wrap semantics.
- Web: 0.37's `glyph_rasterizer`/canvas path and WebGL2 fallback are new behavior; test WebGPU
  and `?webgl` in a real browser, including CJK/emoji glyphs, canvas resize, screenshot/download
  and hidden-tab logic. A `cargo check --target wasm32` is compile-only, not browser acceptance.

### 4.10 Media, autosave and timers

- Add one app-owned monotonic clock: `std::time::Instant` on native and `web_time::Instant`
  (`performance.now()`) on wasm. Use it for screenshot settle/deadline, AutoRecover, media,
  animation preview and show timing. Keep `now_ms()` only for display/formatted timestamps;
  `SystemTime`/`js_sys::Date::now()` are wall clocks and can move backwards.
- `MediaHost::tick` currently reads `ctx.input(|i| i.time)` and is called only from `SlideApp::ui`.
  Split it:
  - `MediaHost::tick_clock(now)` from logic on every call: player tick, end/rewind, ownership
    cleanup, `session.media_status`, request repaint while playing. `now` is the monotonic app
    clock, so hidden logic advances it.
  - `MediaHost::sync_video_textures(ctx)` from `ui`: upload/replace frames for visible video.
    A hidden window accumulates no texture uploads; the next visible frame catches up from the
    player clock.
  - Do not tick the audio player twice in a multi-pass frame; deduplicate with the per-layout-pass
    guard only where genuinely needed.
- `SlideApp::tick_media` (owner/slide cleanup) moves to logic; it only reads document state and
  stops editor-owned media when the slide is left. Keep it idempotent.
- AutoRecover: replace `ctx.input(|i| i.time)`/`last_recovery` with the monotonic clock. Run the
  deadline check from logic on every call (visible or hidden). `file.recovery.save` stays a
  command; errors go to status. Schedule `request_repaint_after` for the next monotonic
  recovery deadline so an otherwise idle hidden app wakes; reschedule/cancel when dirty state
  changes. Test with all other timers disabled. Do not save on every pass.
- Slide-show time advancement moves out of `show::ui` into a `show::advance(app, show, doc, now)`
  helper called from `SlideApp::logic`. It advances the presenter timer, triggered animations,
  transitions, auto-advance and media cues even when no frame is painted, and carries the existing
  `request_repaint_after` scheduling so a hidden show wakes to advance. `show::ui` only applies
  pointer/keyboard actions and draws the resulting state. Audio owned by `media` continues through
  `tick_clock`. A first-pass guard may be kept for one advancement per layout pass, but it must not
  be visibility-based; test hidden auto-advance and one-step-per-time behaviour. If the product
  decision is that a minimized show must pause, make that an explicit app state with a test, not
  an accidental consequence of paint-driven code.
- `squiggles` already runs a worker and requests repaint; keep it. `opening` polls a channel and
  requests repaint; keep it in logic. `toast`/status timers use `now_ms` today; move them to the
  monotonic clock and keep them idempotent.
- `perf.fps`/`frame_ms`: compute on the first pass only so a Grid replay does not distort frame
  statistics. Add raw counters (`frames`, `passes`, `screenshots`) behind `ui.inspect` if needed
  for performance tests; do not add production churn.

---

## 5. Dependency, fork and publication plan

### 5.1 Dependency migration: stock experiment, then reviewed production pins

**Stage A — stock egui 0.37.0 experiment (not a staged release).** Change workspace versions to
`0.37`, update `crates/fonts` to `epaint_default_fonts = "0.37"`, and produce a lock that resolves
one registry egui family. This stage exists to isolate the API/behavior diff against public 0.37
and to run the stock-only comparison benchmarks. No fork patches. It is a local experiment and
must not be published as an intermediate consumer release; the production graph is the fork below.

**Stage B — reviewed storytold production pins.** Add a root `[patch.crates-io]` block pinning
every used egui-family crate and winit to reviewed, published, immutable **storytold** full SHAs,
landed as one coherent set. Cargo does not propagate the fork workspace's winit patch into
DeckCraft. The shape is:

```toml
[patch.crates-io]
winit = { git = "https://github.com/storytold/winit", rev = "<reviewed published storytold winit full SHA with W15P>" }
eframe = { git = "https://github.com/storytold/egui", rev = "<reviewed published storytold egui full SHA with E1-E21 + E21b>" }
egui = { git = "https://github.com/storytold/egui", rev = "<same reviewed storytold egui full SHA>" }
egui-winit = { git = "https://github.com/storytold/egui", rev = "<same reviewed storytold egui full SHA>" }
egui-wgpu = { git = "https://github.com/storytold/egui", rev = "<same reviewed storytold egui full SHA>" }
egui_kittest = { git = "https://github.com/storytold/egui", rev = "<same reviewed storytold egui full SHA>" }
epaint = { git = "https://github.com/storytold/egui", rev = "<same reviewed storytold egui full SHA>" }
epaint_default_fonts = { git = "https://github.com/storytold/egui", rev = "<same reviewed storytold egui full SHA>" }
ecolor = { git = "https://github.com/storytold/egui", rev = "<same reviewed storytold egui full SHA>" }
emath = { git = "https://github.com/storytold/egui", rev = "<same reviewed storytold egui full SHA>" }
krilla = { git = "https://github.com/storytold/craft-libs", rev = "c3487320b2d1de657b860b3c8abb44e1f2b32bd8", version = "=0.8.2" }
```

The `aiongg/egui` `a4f9a17f` and `aiongg/winit` `8b35388b` revisions are source-review inputs, not
publishable pins. Production waits until the reviewed series (including W15P PRIMARY and E21b) is
published under storytold with immutable full SHAs. `egui_extras` is unused today; do not add it
to the patch list unless a real consumer appears. Do not patch `egui_system_font_provider` because
`system_fonts` stays off; if enabled later, patch it to the same revision. Do not patch
`egui_plot`, `egui_dock` or `egui_glow`; they are not in DeckCraft's graph. Keep the `krilla`
patch and its `cargo xtask pdf-deps` check.

**Publication rules.** No machine-local path overrides may be committed. Only public git URLs and
immutable full SHAs are publishable. A branch name is not acceptable, and an `aiongg` personal-fork
SHAs must not appear in a production manifest or release. The `deckcraft` app is `publish = false`,
so its root patch and lock are not crates.io publication artifacts, but releases and CI must
still resolve the public storytold revisions. No stage is pushed or published without fresh human
approval of the exact contents and validation.

**Font pin gate (before publication, not local work; stale pending a combined pin).** The three
workflow pins currently name `b076417b5880bd4154bd22f00695b1d28c6bea83`, whose tree carries the
reviewed Chinese manifest but has no `fonts/latin-manifest.txt` or Inter. The earlier
`43913056ce82a3fdad0ef69f6b88d1dc922a8104` tree carried the Latin manifest/Inter but not the
Chinese input. The parent is restoring the licensed Inter faces in the central
`craft-fonts-implementation` checkout; that work is unpublished and must not be committed from the
DeckCraft checkout. Therefore this approval packet is **stale**: do not approve or publish it until
the reviewed Latin manifest/faces and Chinese manifest are committed together in `craft-fonts`, an
immutable full SHA is obtained, and **all three** pins move together:
`.github/workflows/ci.yml` (`CRAFT_FONTS_REF`), `.github/workflows/freebsd.yml` and
`.github/workflows/release.yml` (`craft-fonts-ref`). Do not invent or pre-commit a future SHA, and do
not advance only one workflow. `crates/fonts/build.rs` now validates required builds after exclusion
and, for wasm, the web filter: Inter Regular/Bold (or a supported Variable face) and BIZ UDPGothic
Regular must actually be embedded, plus BIZ UDPGothic Bold natively for the weight pair; wasm keeps
Regular only for its size budget. A partial pin fails instead of silently losing the default
document/UI face.

The required-input gate is explicit, but CI and FreeBSD qualification remain partial:

- `ci.yml` runs the `Font pin gate` as a local preflight job before the shared gate. It checks the
  Chinese manifest but not the Latin manifest, so the build script's required-mode validation is
  the guard for Inter. The shared `gate.yml` job does not accept a `CRAFT_FONTS_REQUIRED` input or
  another workflow's exported environment, so the CI job verifies the pin but cannot yet force
  the required-font environment into `cargo xtask ci`; release and FreeBSD notes below apply.
- The shared release workflow already sets `CRAFT_FONTS_REQUIRED=1` in every packaging step, so a
  release on a pin without the Chinese manifest or the required Inter/BIZ faces fails instead of
  shipping without them.
- The shared FreeBSD workflow passes `CRAFT_FONTS_DIR` to `cargo test` but currently exposes no
  `CRAFT_FONTS_REQUIRED` input. Its required-font gate therefore remains **pending** until
  `craft-common-actions` adds one (or the caller can otherwise set that environment); do not cite
  FreeBSD as required-font evidence before then.

The font assets stay in the central `storytold/craft-fonts` repository; no font file is added to
this checkout.

### 5.2 Fork patch ledger and DeckCraft decisions

The candidate fork's `PATCHES.md` (base upstream 0.37.0) lists E1–E21; E21b is the new, separately
reviewed addition. DeckCraft decisions:

| Patch | Provenance / behavior | DeckCraft decision | Test / removal |
|---|---|---|---|
| E1 `8f3abf06` eframe `viewport_hooks` | per-viewport native windows and raw input | Not used by DeckCraft; harmless fork API. Do not build craft-tablet behavior on it here. | Remove only with the fork; no app code depends on it |
| E2+E3 `9dd799ab` egui-wgpu | fit surface to texture limit; surface/renderer errors | Keep. Relevant on Windows/Linux/WebGPU and to DeckCraft's large-window texture policy. | Test oversized/minimized surface, error path; remove when upstream release contains and stock passes |
| E4 `66bacf23` egui-winit | Wayland regular clipboard via winit data device; **candidate PRIMARY still goes through arboard** | Keep the regular clipboard path. PRIMARY is an **unresolved fork gap**: require the shared W15P winit/SCTK per-seat path on the existing connection/event queue before production. | Regular copy/cut/paste, own copy, file-drop-then-paste; PRIMARY lifecycle tests on wlroots/KWin/Mutter. Do not claim #8657/PRIMARY from arboard alone |
| E5/E12/E13/E14/E15/E16/E20 | bidi shaping/rows, direction, grapheme caret/delete, fallback baseline, line breaker, visual cursor, UI text language | Keep. Relevant to titles, notes, dialogs, RTL/CJK UI text. DeckCraft's slide renderer is separate and unchanged. | Arabic/Hebrew/CJK/Thai/Vietnamese tests; grapheme Delete; screenshots; remove when upstream equivalent lands |
| E6 `0abfabcf` egui focus/IME | `request_focus` on focused widget does not interrupt IME | Keep. Relevant to notes/dialog TextEdits. | IME composition focus test; remove on upstream fix |
| E7 `0670c58a` egui-winit | Windows letter keys from virtual-key code on non-A–Z layouts | Keep. Relevant to shortcuts. | Russian/Arabic/Hebrew layout shortcut test; remove on upstream |
| E8 `1b03c10f` egui TextEdit | no clipping of glyphs above/below row | Keep. Relevant to notes/dialog text. | Thai/Vietnamese/Arabic marks; remove on upstream |
| E9 `b2f5cafc...` egui RTL style | mirrors menus, scroll bars, headers, title bars, combo boxes, popups, resize grips | Keep. No app code sets `right_to_left`, so LTR must not change. | LTR geometry golden and RTL review; remove on upstream |
| E10 `dc90c13e...` eframe web | named keys not text; `code`-based letters; `lang` on hidden input/speech | Keep. Relevant to `deckcraft-web`. | Browser keyboard/IME test; wasm compile; remove on upstream |
| E11 `92e61afd` epaint | glyphs rasterized on single-threaded vello_cpu; no multithreading panic | Keep for egui text. DeckCraft's `deckcraft-render` uses its own vello_cpu and already has a filters/threads regression test. | UI text with filters/emoji; slide renderer tests; remove on upstream |
| E17 `520435df` egui language | `Context::set_language`, `UiBuilder::language`, AccessKit language | Keep but do not enable an app locale yet. | AccessKit language test only if the app sets it; otherwise compile-only |
| E18 `89de8f3c` eframe | `Frame::set_ui_language`; web fallback | Keep available; DeckCraft has no localized UI. | No app call in this migration; revisit with localization |
| E19 `d64047bf...` egui/winit | one Latin letter-key rule; `keyboard_layout_is_latin`, `latin_layout_chars` | Keep. Relevant to shortcuts. | Shortcut tests; remove with winit API |
| E21 `a70470b0` egui/winit | `ImeEvent::DeleteSurroundingBytes` | Keep; handle it in the custom slide-text path. `DeleteSurrounding` itself already exists in 0.36.2 and is currently ignored, so this is a pre-existing gap plus the byte-range variant, not a new 0.37 API. | macOS press-and-hold/dictation, Wayland text-input; remove with upstream equivalent |
| **E21b** (new) | `IMEOutput` bounded logical surrounding-text payload, TextEdit producer and Wayland delivery through W27 | **Required new prerequisite**, not existing E21 capability. DeckCraft must implement the model-aware producer for `canvas::ime_output`, with UTF-8 byte cursor/anchor, preedit exclusion, password exclusion and a 4,000-byte bounded extraction. | Outgoing text after delete+commit/deletion-only, preedit/selection/focus/password/long-selection/two-window/multi-pass tests; native Wayland IME trace; measure allocation/time |

Winit W1–W27 are inherited through the reviewed winit series, plus the new **W15P** PRIMARY support.
DeckCraft-relevant ones are W1/W11 (DPI/X11 scaling), W2/W3/W5–W10/W12/W13/W18–W27 (IME, clipboard,
drag, shortcuts, hidden windows), W15 (regular Wayland clipboard, needed by E4) and W15P (PRIMARY).
Do not remove any of them independently; removal is tied to an upstream release and the tests
named in the craft-libs ledger. W11 can change `pixels_per_point`, so every geometry/perf run
records the exact winit SHA.

### 5.3 craft-libs PR 199 and language PR 198

- PR 199 updates `craft-ui` (and its dev dependencies) to 0.37 and applies the same `WidgetType`
  -> `Role`, `UiBuilder::id` -> `scope_id`, `Id::new` -> `Id::unique` mapping. It is useful
  precedent and should remain the shared UI library's migration, but DeckCraft does not consume
  `craft-ui`; do not add it just to align versions.
- **Use the reconciled shared-library base.** The implementation builds on merged PR 199 at
  `3832c47ef68a0e6214f2c1e85f3526b6adcb686e`; the reviewed 0.37 API changes are already part of
  that merge. Never merge or cherry-pick PR head `b68de8c`/audit merge `ec8840a` as trees. The
  older `8614e4a`/`1764bca`/`4919088` references were plan-only and are superseded. Preserve
  unrelated vendors and current-main fixes; regenerate the lock with the final reviewed
  storytold set.
- PR 199 does not add a fork pin or winit pin and does not test hidden `App::logic`. DeckCraft's
  root patch and lifecycle tests must stand on their own.
- Language work (#198) and the fork's E5/E12/E13/E17/E18/E19/E20 are the relevant source for
  RTL/IME/grapheme behavior. DeckCraft's slide text renderer already has its own bidi/line
  breaking; this migration only changes the egui UI/text-edit layer. Do not move slide layout
  policy into `craft-ui` or an egui compatibility layer.
- `craft-pdf`/`krilla` stay at their existing revision. They do not depend on egui; do not update
  them in this migration.

### 5.4 Lockfile, compatibility and rollback checks

Before merging any dependency stage:

- `cargo metadata --locked` and `cargo check --locked` must resolve without adding a second
  `egui`, `egui-winit`, `egui-wgpu`, `epaint`, `ecolor`, `emath`, `epaint_default_fonts` or
  `winit` source. Use `cargo tree -d` and `cargo tree -i egui@0.37.0`,
  `cargo tree -i winit@0.30.13`, `cargo tree -e features -i eframe`.
- Inspect `Cargo.lock` git entries: every egui-family git entry must have the exact full **storytold**
  rev and no `branch =` or path; there must be no `aiongg` source. The `krilla` entry and
  `craft-pdf` source stay unchanged.
- Check the feature graph: native `default_fonts`, `clipboard`, `wgpu`, `accesskit`, `links`, `wayland`,
  `x11`; web `default_fonts`, `wgpu`, `accesskit`; no accidental `system_fonts`, `glow`,
  `persistence`, `inspection` or `experimental`. The library's own `egui` dependency must carry
  `default_fonts`, not rely on an app root.
- Confirm accesskit/accesskit_consumer/kittest versions are the 0.37-compatible set; remove old
  duplicate `accesskit_consumer` versions only if the resolver does so naturally. Do not force
  one for its own sake.
- Run `cargo xtask pdf-deps` to prove the pinned PDF writer still matches.
- Keep a rollback commit at baseline `b47195f`, and preserve the baseline `Cargo.lock` in
  evidence. Rollback of a stage is `git revert` of that stage plus restoring the old lock; no
  document/settings migration is required because the formats do not change.
- Do not use untargeted `cargo update`; update only the egui-family/winit packages, then review
  the lock diff. Do not commit a lock generated with local path overrides.

---

## 6. Reviewable implementation stages

Dependent PRs, in order. Every stage gets its own adversarial review and its own validation; no
stage is published without fresh human approval of contents and validation.

### D0 — Baseline evidence and before/after harness (no production changes)

- The before/after harness exists on the **baseline before production**: record `b47195f` source
  identity, `Cargo.lock`, `rustc -Vv`, `cargo -V`, OS, GPU/adapter, DPI, fonts
  (`CRAFT_FONTS_REF` or embedded face list), window size and feature flags, then capture the paired
  baseline samples and raw data that every later stage compares against.
- Capture native screenshots (light/dark, 1×/2×) of: sample editor, ribbon tabs, Format Shape
  pane, Animation Pane, Backstage, start screen, slide sorter, doc window, presenter, symbol
  dialog, theme-colors dialog, notes/IME.
- Build the deterministic text-tessellation benchmark (plain ASCII labels, unchanged fixture) and
  the frame/startup measurement scripts before production changes; save raw samples and the
  `docs/benchmarks/baseline.json` candidate under evidence; do not commit machine-specific timings.
- Run the baseline gates (`cargo xtask ci`, optional Windows/wasm smoke) through `task.py` so the
  starting tree is known green.
- Output: evidence and harness only; no source change.

### D1 — atomic final-fork 0.37 compatibility PR

Dependencies: D0 and final reviewed/published storytold egui/winit including P2b, P7, P7b/E21b, W15P/W27 and P7c. Files: root `Cargo.toml`, `Cargo.lock`, `crates/fonts/Cargo.toml`,
`crates/ui-egui/Cargo.toml` and `src/*`, `apps/deckcraft*` if the App trait/feature graph requires
it, tests. Stock 0.37 remains an isolated comparison workspace, never a production stage.

- Bump `egui/eframe/egui_kittest` to 0.37; `epaint_default_fonts` to 0.37. Pin the complete immutable final egui-family/winit patches; preserve krilla.
- Explicitly enable native `eframe/clipboard` and verify cross-process copy/cut/paste.
- Include the compatible canvas E21b producer, incoming E21 byte handling and owner/clear/final-pass contract.
- Add `default_fonts` to `deckcraft-ui-egui`'s `egui` dependency (or a shared workspace feature),
  not just the apps; verify `cargo test -p deckcraft-ui-egui` standalone.
- Replace `WidgetType` imports/variants with `Role`; `TextEdit -> TextInput`,
  `Checkbox -> CheckBox`, `SelectableLabel -> Button`, `Other -> Unknown` initially.
- Replace egui `Id::new` with `Id::unique`; replace every `ui.id()` and `UiBuilder::id` with
  `ui.scope_id()`/`UiBuilder::scope_id` (38 `ui.id()` references); remove `AsId` uses. Run
  `cargo clippy -D warnings` to catch every remaining deprecated call.
- Add `source: MouseWheelSource` to synthetic wheel events in `canvas.rs`, `huge_deck_tests.rs`,
  `lib.rs` soak tests.
- Refactor screenshots to the bounded completion-slot design in §4.3; remove `UserData` and
  `Event::Screenshot`.
- Fix kittest fallout in this first green PR: required accessible names for genuinely unnamed
  inputs (keep working `widget_info`), missing-glyph/glyph-coverage replacements, the
  `default_fonts` feature fix for `deckcraft-ui-egui`, any `ATTRIBUTION.md` rows for changed
  assets, and all gate updates. Do not defer names/glyph/feature/attribution/gate work to a later
  stage, and verify app-owned settings sizes without an invented PanelState migration.
- Gate: workspace `cargo xtask ci`; UI kittest suite; wasm check; native smoke; review every
  changed golden/geometry; standalone library tests with and without `CRAFT_FONTS_DIR`. This is the first publishable coherent compatibility stage.

### D2 — Lifecycle, hidden logic, multi-pass and screenshots

Dependencies: D1. Files: `crates/ui-egui/src/lib.rs`, `control.rs`, `media.rs`, `opening.rs`,
`uifonts.rs`, `textures.rs`, tests; `apps/deckcraft/src/main.rs`, `apps/deckcraft-web/src/web.rs`.

- Add the monotonic app clock; move autosave deadlines, media clock and show/animation advancement
  into `logic` on every call; split video texture sync into `ui`.
- Move native dropped-file handling and `note_ui_text` into the known UI phase; remove visibility
  gating from background work.
- Make screenshot scheduling hidden-window-safe with the weak slot design; drain results in
  logic; preserve deadlines and control JSON; bound pending memory.
- Add tests for hidden `run_logic` with a stale drop, hidden autosave/media/show advance, a forced
  hidden paint (`RequestPaintWhileHidden`/pending screenshot) with current input, a two-pass Grid
  discard, manual screenshot callback correlation and `ui.screenshot` while hidden/minimized.
- Gate: new tests, `cargo xtask ci`, native control-channel smoke (`ui.screenshot`, `ui.resize`,
  `ui.inspect`), Windows VM smoke.

### D3 — native dependency qualification

Dependencies: D1 and D2. Use the already pinned final dependency unit; no second production
pinning stage. Exercise E2/E3 surface behavior, bounded E4/W15P transfer cancellation and
responsiveness, E19 held-key release pairing, E21/E21b IME and native child focus/close.
Run graph checks, Linux X11/Wayland, Windows clone and browser workflows with exact evidence.
No fork internals are copied into DeckCraft. Record unqualified native rows as gates.

### D4 — Language/IME/clipboard app integration

Dependencies: D2 + D3. Files: `crates/ui-egui/src/lib.rs` (`keyboard`, `ime_event`),
`crates/ui-egui/src/canvas.rs` (E21b producer), `crates/engine/src/keys.rs`/`cmd/text.rs` for
range deletion, tests, screenshots.

- Handle `PasteImage` without double-pasting; handle `MiddleClickPaste` expectations.
- Handle pre-existing `DeleteSurrounding` + fork `DeleteSurroundingBytes` in the custom text path,
  or record an explicit blocking gap.
- Implement the E21b `IMEOutput` producer for `canvas::ime_output`: logical model text, UTF-8
  byte cursor/anchor, preedit exclusion, password exclusion, bounded 4,000-byte extraction and
  field/window identity; test outgoing state after delete/commit, deletion-only, focus/password
  clears and multi-pass.
- Add bidi/RTL/CJK/Thai/Vietnamese tests, non-Latin shortcut tests and the #8763 selection-action
  regression; decide/implement the animation timing accessibility role.
- Gate: native Linux/Windows IME and clipboard tests; W15P PRIMARY on available compositors; web
  keyboard/IME smoke; no duplicate undo.

### D5 — Performance, UI regressions, docs

Dependencies: D1–D4. Files: tests/snapshots/benchmark scripts, `docs/`, `ROADMAP.md`,
`docs/gaps.md`, `docs/ui-parity.md`, `docs/architecture.md` if the lifecycle/architecture changed.

- Add curated snapshot/geometry/a11y tests and mutation checks.
- Run matched baseline/candidate performance with the counts in §8 (1,000 frames for p99, 20 fresh
  startup processes, 30 operation samples, 10 warmups); accept only within thresholds or with an
  explicit, reviewed exception.
- Update progress docs and parity rows; add a short “egui 0.37” note to architecture/roadmap.
- Preserve evidence outside the disposable build target; clean only task-owned build artifacts
  after all checks, per AGENTS.md.

Rollback restores the complete coherent `b47195f`/0.36.2 source, lock and executable,
including its app-owned settings. Stock 0.37 is only a comparison control, never a rollback
or production handoff. Later behavior/test stages may revert independently only while the
remaining tree still passes its required compatibility and data-preservation gates.

---

## 7. Regression and acceptance tests

### 7.1 Unit / kittest (fast, deterministic)

Use `egui_kittest::Harness::build_ui_state` (which drives `Context::run_ui`, not `Context::run`)
with the shared `run_frame` helper. Keep `with_size(1200, 800)` and step dt where existing tests
depend on it. The helper must mirror the real host: `logic` then `ui` inside `run_ui`, with a
separate `Context::run_logic` path for hidden iterations.

Required new/updated tests:

1. **Commands/undo:** insert shape, type text, move, undo/redo; assert document state and exactly
   one undo entry per user action. Include a Grid `request_discard` pass and assert no duplicate
   command/undo.
2. **Save/reopen:** build sample deck, save `.deckcraft` to a temp path via `Services::write`,
   close, reopen, assert slide count, shape text, selection-independent content; repeat for a
   `.pptx` round trip where existing format tests apply.
3. **Screenshot correlation:** capture the `ScreenshotCallback`s from viewport output and complete
   them manually with distinct images in reverse order (kittest reuses one image for all callbacks,
   so it cannot prove this). Assert token/path/size/base64 independence; assert one deadline error
   for a missing frame; assert a late callback after timeout/close writes no file and sends no
   reply; assert the bounded admission/aggregate-memory rejection.
4. **Hidden logic:** visible pass with a dropped file, then two `Context::run_logic` calls with
   empty input; assert one insertion, not two; assert autosave/media/show clocks advanced by the
   monotonic clock; assert a stale event/time is not consumed twice.
5. **Hidden forced paint:** with the root hidden, send `RequestPaintWhileHidden` (or issue a
   pending `ui.screenshot`) and assert `App::ui` runs, current input/drop is handled once, the
   screenshot reply arrives with capture-time geometry, and no reply is duplicated. This is the
   valid hidden-input negative test.
6. **Accessibility:** every visible input has a name; role/name/value/selected checks for File/Back,
   checkbox, selectable row, dialog fields, toast and a custom `widget_info` control; invisible
   widgets absent.
7. **Geometry/panels:** 900×560 and 1440×900, 1×/2×; `canvasRect`/`slideRect` inside the central
   area; title/ribbon/status do not overlap; pane widths persisted; clipped content not drawn
   over neighboring panels; app-owned settings dimensions survive save/reopen without an egui
   PanelState migration, and session-local outline resizing works.
8. **Fonts/glyphs:** standalone `cargo test -p deckcraft-ui-egui` renders with the library's own
   `default_fonts`, with and without `CRAFT_FONTS_DIR`; curated screens use kittest's default
   missing-glyph panic; fallback faces added once and `UiFonts` enforces its per-face and aggregate
   budget under adversarial multi-script input; soak test uses `allow_missing_glyphs()` only for
   arbitrary input.
9. **Selection:** select a subrange in the real notes TextEdit and use its context-menu
   Copy/Cut, then the equivalent keyboard path; verify the copied/cut logical range and
   intended draft edit/undo behavior. Do not use ribbon Bold as a proxy: it edits document
   text, not the notes draft. Fork P7c owns the selected #8763 fix and tests; #8764 is
   open/unmerged. D1 consumes that completed revision; this is not an optional fix choice.

10. **Input/IME/clipboard:** `PasteImage` inserts once; regular paste still HTML/image-aware;
    middle-click PRIMARY on notes works and on canvas does not insert; IME preedit/commit one
    undo; pre-existing `DeleteSurrounding` and fork `DeleteSurroundingBytes`; E21b outgoing text
    after delete/commit, deletion-only, preedit exclusion, selection/focus/password/long-selection/
    two-window/multi-pass; RTL caret/selection/Delete at grapheme boundaries; non-Latin shortcut
    does not fire unmapped letters.
11. **Textures/GPU:** oversized canvas/window fits `max_texture_side`; zero-size/minimize does not
    panic; texture filter/`editor_texture_options` preserved; screenshot size matches capture-time
    DPI.
12. **Control protocol:** `ui.inspect`, `ui.resize`, `ui.screenshot`, `app.save`, `app.quit`
    replies remain well-formed; timed-out requests do not run late.

### 7.2 Screenshots and visual regression

- Curated kittest snapshots at 1440×900 (and 2× pixels-per-point), light/dark: editor, ribbon tabs,
  Format Shape, Animation Pane, Backstage, sorter, symbol gallery, theme-colors dialog, presenter
  document window (embedded), start screen.
- Native screenshots from the Linux host and the Windows clone of the same fixture and window
  size. Do not compare across OS/DPI/renderer.
- Predefine pixel policy: channel tolerance ≤2/255, changed-pixel fraction ≤0.10%, and no
  geometry shift >1 device pixel for unchanged regions. 0.37's intentional style changes
  (roomier buttons/corner radius) require an explained baseline update; do not silently accept a
  changed golden. Review the `.diff`/`.new` images.
- Screenshots of DeckCraft are contributor-original; store them under evidence, not as untracked
  repo assets. Do not use PowerPoint screenshots; the private `plan/powerpoint/screenshots/`
  policy remains.

### 7.3 Mutation checks (prove the tests detect failures)

Run each as a temporary local patch, never commit, and record the failure output in evidence:

- Remove one `labelled_by`/`accessible_name` from a visible unnamed field → accessibility test fails.
- Remove a working `widget_info` role/label → accessibility role/value test fails.
- Remove the per-layout-pass guard around a mutating action → the Grid discard test reports two
  undo entries.
- Move dropped-file handling back into visibility-gated logic → the hidden forced-paint test
  reports a missed drop or a stale-input replay.
- Gate autosave/media/show on visibility → the hidden logic test reports a stopped clock or no
  AutoRecover.
- Let a late screenshot callback write/reply after timeout → the cancellation test fails.
- Reuse a screenshot token for two requests → screenshot correlation test reports crossed images.
- Remove `UiFonts` fallback for a CJK title → missing-glyph panic (curated screen) or tofu
  screenshot; exceed the aggregate face budget → the budget test fails.
- Change panel order, set a panel to the wrong side or claim panel-internal state survives the
  ID change → geometry/upgrade test fails.
- Change `editor_texture_options` filtering → texture/pixel test fails.

### 7.4 Platform gates

- **Linux native:** X11 and Wayland (Hyprland/KWin if available), 1×/2×, `deckcraft --sample
  --control`; `ui.screenshot`, `ui.resize`, input/IME, regular clipboard, W15P PRIMARY, viewports.
- **Windows native:** fresh clone from `VMs/craft-windows`, new ports, separate source/target,
  `Build-DesignCraft` equivalent for DeckCraft (`cargo build --release -p deckcraft` with
  `CRAFT_FONTS_DIR`), DX12/FXC, Windows IME/layout, screenshot/GUI via `vm.py`/`guest-exec.py`,
  `ui.screenshot`, doc window, save/reopen.
- **Web:** `trunk build --release` in `apps/deckcraft-web`; Chrome/Chromium with WebGPU and
  `?webgl`; load sample, open/save/download, keyboard/IME, CJK/emoji rendering, hidden-tab
  `run_logic`, no console panic. A wasm compile is not browser acceptance.
- **macOS:** release CI or real hardware only; AccessKit/on_exit, traffic lights, Apple events,
  IME. If no real evidence is available, mark explicitly unresolved rather than passing.
- **FreeBSD:** existing CI/VM if available; no egui-specific claim otherwise.
- Do not report Linux cross-compilation as Windows/macOS validation.

### 7.5 Required repository gates

For each implementation stage:

```
cargo xtask ci
cargo xtask bench          # after a baseline exists; evidence copy
cargo test -p deckcraft-ui-egui   # with the reviewed CRAFT_FONTS_DIR
cargo test -p deckcraft-ui-egui   # without CRAFT_FONTS_DIR, separate job
cargo test -p deckcraft
cargo check -p deckcraft-web --target wasm32-unknown-unknown
trunk build --release      # apps/deckcraft-web, when web behavior changed
```

Run through `task.py run/status/finish` with the stage's unique task and evidence; do not run a
build/test directly outside that workflow. Use the repository's `CRAFT_FONTS_DIR` and the same
font revision for baseline and candidate where the comparison requires fonts.

---

General performance reports include 0.36, an isolated stock-0.37 control and the final fork
for frame/startup/canvas/thumbnail/capture/save-reopen workloads where comparable. Record each
source/lock and use a bounded test adapter where fork-only APIs differ; explain unavailable
equivalence instead of attributing all changes to the fork. This applies to the first report
and investigated regressions, not only the #8731 microbenchmark.

## 8. Performance plan

### 8.1 Environment and fixtures

Record for every run: exact source SHA and lock digest, feature set, `CRAFT_FONTS_DIR`/font SHA,
fixture (sample deck, a 20-slide business deck, a 200-slide and 800-slide synthetic deck), OS and
kernel, CPU/GPU/adapter/driver, renderer (native wgpu DX12/Vulkan/GL vs Microsoft Basic Render
Driver software), DPI (`pixels_per_point`), viewport size, window state (visible/fullscreen/
hidden), release profile, and warmup/repetition counts. The harness and the 0.36.2 baseline raw
samples are captured in D0 before any production change.

### 8.2 Operations

- Headless (`cargo xtask bench` / `deckcraft-cli bench --runs N --json`): open `.deckcraft`,
  open `.pptx`, render all slides at 1×, save `.deckcraft`, save `.pptx`, export PDF; peak RSS.
- UI native via control channel:
  - cold start to first `ui.inspect` (process start → reply), 20+ fresh processes;
  - idle 60 s: CPU time, `perf.fps`, repaint count if exposed;
  - `ui.screenshot` latency and PNG size (root visible);
  - `ui.resize` geometry settle at 900×560, 1440×900, 2560×1440, 2×;
  - canvas render ms (`ui.inspect.perf.renderMs`) after edit, undo, slide change;
  - thumbnail render count/budget on the 800-slide deck (existing `textures.thumb_count` and
    `canvas_renders` can be exposed for tests);
  - typing/IME/keyboard latency via command start → document reply;
  - open/save/reopen through control;
  - show start/end and presenter window.
- **Targeted text-tessellation benchmark (#8731):** a deterministic fixture of plain ASCII labels
  through `Context::tessellate`, unchanged across 0.36.2, stock 0.37 and the selected fork.
  Record instructions (or a stable per-operation latency) with a predeclared threshold; general
  frame timing can hide a large text-tessellation regression. Compile time is not UI performance.
- Headless kittest frame timing: at least 1,000 frames per curated screen distribution; report
  median/p95/p99 and allocations if a test-only counting allocator is added. Aggregate raw
  samples across runs so p99 has a real sample count.
- GPU: separate software renderer and real GPU; do not mix. Windows VM results are software
  DX12 and must be labelled as such.

### 8.3 Sampling and noise policy

- Warm up at least 10 iterations (or the documented time-based equivalent) before measuring.
- For each operation, collect at least 30 paired repetitions; for frame distributions collect at
  least 1,000 frames per distribution, and report the actual sample count. Do not estimate p95/p99
  from 20–50 samples.
- Startup uses at least 20 fresh processes, alternating baseline/candidate.
- Report raw JSON, median, p95, p99 and max; discard only the warmup interval.
- If coefficient of variation >3% for a median, rerun with more samples or on a quieter host;
  never compare a baseline and candidate from different machines/renderers/DPI.
- Paired A/B on identical environment, alternating baseline/candidate runs to reduce drift.

### 8.4 Predefined thresholds

Investigate and block acceptance when any holds:

- UI frame **median >5% slower** or **p95 >10% slower**; with at least 1,000 frames, also
  investigate p99 >15% slower;
- targeted text-tessellation operations beyond their predeclared threshold;
- canvas/thumbnail render median >5% slower;
- cold start >5% slower over 20+ fresh processes (the #8736 DX12/FXC shader cost must be reported
  separately, not hidden in the aggregate);
- peak RSS >5% higher or >50 MiB higher;
- idle CPU >0.5 percentage points absolute, or idle repaint rate materially higher;
- screenshot latency >10% slower or PNG size unexpectedly changed;
- any pixel/geometry/accessibility regression, regardless of performance.

These percentage thresholds are the gate; do not add a larger absolute budget that would make a
known percentage regression acceptable. Do not auto-accept changed goldens or claim a performance
pass from compile duration. A regression that is an upstream issue (#8731 text tessellation,
#8736 DX12 FXC) still needs a measured baseline/candidate report and an explicit decision; prefer
an upstream/fork fix that preserves color glyph and texture wrap/filter semantics over a
clamp-only shortcut.

---

## 9. Evidence, documentation and cleanup

- Evidence root: `/home/codex/work/ssd2/task-evidence/egui037-migration-20261022/deckcraft/`
  (or the per-task evidence path returned by `task.py`). Keep baseline/candidate locks, source
  diffs, raw performance JSON, screenshots, test logs, mutation-check logs and exact font/fixture
  identities.
- Durable plan: this file. Update `docs/architecture.md` only for the lifecycle/fork facts that
  become true; update `ROADMAP.md` progress log, `docs/ui-parity.md`, `docs/gaps.md` and
  `docs/localization-parity.md` only for actual landed behavior. Do not claim a parity row or
  close a gap from a plan.
- No new assets: screenshots are evidence, not repo assets; if a font/glyph replacement adds an
  asset, follow `ATTRIBUTION.md`/`craft-fonts` rules. Do not add Microsoft/Adobe/Avid/Autodesk
  material.
- Cleanup: after all stages and reviews, remove only the task's disposable Cargo target and
  rebuildable outputs (see AGENTS.md), preserving source, evidence, screenshots, locks and any
  unique files. Do not clean other sessions, VMs, baselines or checkouts.
- No agent/model/tool attribution in any publishable PR/release text.
- Independent-review disposition: H1-H9, M10-M11 and L12-L17 were checked against source and are
  resolved in this revision. One sub-claim was rejected: the review said the `UserData` anchor was
  around `lib.rs:1952`; the actual `egui::UserData::new` call is at `lib.rs:1931` (the plan's
  cited line), while `Event::Screenshot` is at `:1951`. Only `logic` (`1539 -> 1540`) and
  `keyboard` (`1667 -> 1668`) needed anchor correction.

---

## 10. Blocking unknowns and explicit non-claims

1. **Production pins.** Production requires reviewed, published, immutable **storytold** full
   SHAs for the complete egui family and winit, including W15P PRIMARY and E21b. The `aiongg`
   `a4f9a17f`/`8b35388b` revisions are review inputs only; a branch, personal-fork SHA or
   stock-only intermediate release is not a production pin.
2. **macOS/native hardware evidence.** The plan cannot pass macOS AccessKit/IME/traffic-light/
   Apple-event behavior without a real macOS build/GUI run. Mark unresolved if unavailable.
3. **Upstream 0.37 regressions.** #8731 (text tessellation ~+60% instructions) and #8736 (DX12
   FXC pipeline ~+40 ms startup) are open reports, not this task's results; measure them with the
   targeted benchmark and 20+ fresh startup processes. #8763 (TextEdit click-away selection loss)
   is open; require the fork-owned tested fix and the DeckCraft selection-action regression.
   The separate `upstream-8764-fresh.json` snapshot verifies #8764 as open/unmerged at
   `6eb460a2a6d537d312a80579cb801895509deaed`; it is a proposal, not a released fix. #8748 is **closed and not merged**, and is not in
   0.37.0; do not claim it fixes hidden-parent/deferred-child behavior. #8512 is a closed
   PR/comment thread; its system-font discovery startup cost is motivation for keeping
   `system_fonts` off, not a fresh DeckCraft defect or an acceptance result.
4. **Accessibility inventory.** Many `TextEdit::` references and custom `interact` controls need
   names/roles; the actual kittest failures are unknown until the stock 0.37 run. Keep and repair
   working `widget_info` role/label/value rather than replacing it wholesale. Do not disable the
   check globally to hide failures.
5. **Glyph coverage and `UiFonts` bounds.** The literal symbol set and installed-font coverage
   differ by OS. A curated panic-policy screen test may require replacing decorative glyphs or
   adding an allowed fallback font; do not introduce tofu in production silently. The current
   128 MiB limit is per face; add and test an aggregate budget before calling `UiFonts` bounded.
6. **IME delete-surrounding and outgoing text (historical plan gap).** `DeleteSurrounding` already
   existed in 0.36.2 and was ignored at plan time; the custom slide-text path had no range-delete
   API then. E21b was a new prerequisite: the plan-time candidate had no logical surrounding text
   in `IMEOutput` and no Wayland delivery. **Final local status (2026-10-25):** the reviewed source
   now publishes `surrounding_text: update.surrounding_text` (`crates/ui-egui/src/lib.rs:2751`) and
   handles both `DeleteSurrounding` forms (`crates/ui-egui/src/lib.rs:2540-2549`), so that source
   gap is closed. Native/Wayland delivery and all IME qualification remain unproven; do not claim
   E21/E21b acceptance from source presence alone.
7. **`ui.screenshot` on a hidden/minimized window.** A pending screenshot/`RequestPaintWhileHidden`
   forces a UI pass; the weak-slot design needs a real native test (and Windows clone) for capture
   geometry, no late file/reply, bounded memory and input handling. Offscreen kittest is not proof
   of native focus/occlusion behavior.
8. **Wayland PRIMARY.** Candidate E4 uses arboard for PRIMARY; the required W15P winit/SCTK path
   must be reviewed and published before production. Native compositor evidence remains a gate.
9. **PR 199 / #198 merge state.** They are external work; this plan does not merge, rebase or
   publish them. The implementation is based on merged PR 199 at `3832c47ef68a0e6214f2c1e85f3526b6adcb686e`
   (the older `8614e4a`/`1764bca`/`4919088` references are superseded); never merge
   `b68de8c`/`ec8840a` as trees. DeckCraft does not depend on `craft-ui`; if that changes,
   re-plan the DAG.
10. **Nix/flake.** The repository has a `nix.yml` workflow but no local `flake.nix`/`nix` files in
    this checkout; do not claim Nix packaging validation from this plan.
11. **No tests or builds were run for this plan.** The historical compiler errors are clues, not
    evidence that the current checkout compiles or that the migration passes. The before/after
    harness must exist on the baseline before production and all validation uses the managed
    `task.py` workflow.

---

## 11. Acceptance checklist

- [ ] Stock 0.37.0 experiment resolves and all workspace tests pass; wasm checks pass; it is not
      published as an intermediate release.
- [ ] Production uses exactly one reviewed storytold egui-family source and one reviewed storytold
      winit source, including W15P PRIMARY and E21b; no branch/path/personal-fork; `krilla`
      preserved; `cargo xtask pdf-deps` green.
- [ ] `App::logic`/`App::ui` root-`Ui` semantics documented and tested on the real call order;
      `run_logic` stale input/time is never treated as fresh; no `root_ui` panic path.
- [ ] Hidden logic does not duplicate events/drops/autosave; autosave, audio/media and show clocks
      advance on a monotonic clock; no pass required for control/screenshot replies; forced hidden
      paint handles current input.
- [ ] Multi-pass/Grid discard produces one command/undo and no duplicated side effects.
- [ ] Screenshot completion slots are bounded/cancelable/weak, use capture-time geometry, never
      hold a Context reference cycle, produce no late file/reply, and pass correlation/hidden/
      timeout/overflow tests.
- [ ] Commands, undo, save/reopen and control protocol unchanged in observable behavior.
- [ ] Panel order/clipping/geometry and doc/presenter viewports tested at 1×/2× and narrow size;
      app-owned `UiState` dimensions survive save/reopen and tests use parent-salted
      in-memory keys; no persisted egui panel reset is invented.
- [ ] Every visible input has an accessible name; working `widget_info` roles/values retained;
      roles/selected states asserted; missing-name mutation check fails as expected.
- [ ] Curated glyph coverage passes with kittest panic policy; `UiFonts` per-face and aggregate
      budgets tested; standalone `deckcraft-ui-egui` tests pass with and without `CRAFT_FONTS_DIR`.
- [ ] PasteImage, W15P PRIMARY, pre-existing DeleteSurrounding + E21b outgoing text, RTL/grapheme/
      non-Latin shortcuts and the #8763 selection-action regression tested.
- [ ] GPU surface/texture limits, filtering, DX12 FXC and #8731 text tessellation measured; no panic
      on resize/minimize.
- [ ] Matched baseline/candidate performance with 1,000-frame distributions, 30 operation samples,
      10 warmups and 20+ fresh startup processes; investigate >5% median / >10% p95; no looser
      absolute budget; or an explicitly reviewed exception with raw evidence.
- [ ] Windows clone, Linux native, web browser and (if available) macOS/FreeBSD gates recorded;
      unresolved gates named honestly.
- [ ] Evidence preserved, task build artifacts cleaned, no unrelated worktrees/VMs touched.
- [ ] Fresh human approval obtained before any push/new PR; no publication from this plan.

---

## 12. Primary sources consulted

These are source references, not fresh validation results. `0.37.0` is the peeled release commit;
the fork is the reviewed candidate tip. The issue/discussion snapshots are the migration
workspace's read-only copy.

- egui `0.37.0` changelog and source: `https://github.com/emilk/egui/tree/0.37.0` (peeled
  `841a328ba2dd8f6996beb7189ebb7fc00e668a2a`), especially
  `crates/egui/src/context.rs`, `crates/egui/src/data/input/event.rs`,
  `crates/egui/src/data/screenshot_callback.rs`, `crates/egui/src/viewport.rs`,
  `crates/egui/src/response.rs`, `crates/egui/src/lib.rs` (Role migration table),
  `crates/eframe/src/epi.rs`, `crates/eframe/src/native/epi_integration.rs`,
  `crates/eframe/src/native/wgpu_integration.rs`, `crates/egui_kittest/src/lib.rs`,
  `crates/egui_kittest/src/accessibility.rs` and `crates/egui_kittest/src/builder.rs`.
- Callback screenshots: emilk/egui PR #8489,
  `https://github.com/emilk/egui/pull/8489`.
- Root `Ui`/`run_pass`/`root_ui` panic semantics: emilk/egui PR #8535,
  `https://github.com/emilk/egui/pull/8535`.
- `Role` replacing `WidgetType`: emilk/egui PR #8555,
  `https://github.com/emilk/egui/pull/8555`; `default_fonts` no longer default: PR #8583,
  `https://github.com/emilk/egui/pull/8583`; `Panel` `AsIdSalt`: PR #8592,
  `https://github.com/emilk/egui/pull/8592`.
- Accessibility behavior: #8581 (popup/menu/modal/panel trees), #8578 (panel resize handles),
  #8604/#8608/#8612 (accessible names/roles), #8621 (invisible widgets), #8589/#8611 (kittest
  unnamed input check), #8601 (missing glyph policy). Use the PR pages under
  `https://github.com/emilk/egui/pull/<number>`.
- PRIMARY/clipboard: #8428 (`https://github.com/emilk/egui/pull/8428`), #8472
  (`PasteImage`), #8657 (Wayland clipboard/PRIMARY interaction,
  `https://github.com/emilk/egui/pull/8657`).
- Bidi/language: #8577 (`https://github.com/emilk/egui/pull/8577`) and the suite's E5/E12/E13/
  E17/E18 ledger.
- Performance reports: #8731 (text tessellation,
  `https://github.com/emilk/egui/issues/8731`), #8736 (DX12 FXC shader compile,
  `https://github.com/emilk/egui/issues/8736`), #8512 comments (system font startup,
  `https://github.com/emilk/egui/pull/8512`).
- TextEdit click-away selection regression: #8763
  (`https://github.com/emilk/egui/issues/8763`, open). The separately verified open/unmerged
  [#8764](https://github.com/emilk/egui/pull/8764) is a proposed fix to review, not release behavior.
- Hidden-parent/deferred-children behavior: #8748
  (`https://github.com/emilk/egui/pull/8748`), closed and **not merged**; not in 0.37.0.
- Lifecycle source audit and IME gap evidence (local, read-only):
  `/home/codex/work/ssd2/task-evidence/egui037-migration-20261022/lifecycle-source-audit.md` and
  `.../ime-surrounding-gap.md`.
- Suite fork provenance and patch ledger (review input):
  `https://github.com/aiongg/egui/blob/a4f9a17f64a6bae61350fb85804c9df0d9865916/PATCHES.md`
  and `https://github.com/aiongg/egui/tree/a4f9a17f64a6bae61350fb85804c9df0d9865916`.
- Winit fork (review input): `https://github.com/aiongg/winit/tree/8b35388bd719e8cc86a95971ba0bc026c9a8f058`,
  with the W1–W27/W15P ledger recorded in the craft-libs migration planning note and the local
  `PATCHES.md` provenance.
- craft-libs PR 199 and language work #198 are external work under
  `https://github.com/storytold/craft-libs/pull/199` and
  `https://github.com/storytold/craft-libs/pull/198`; their merge state must be rechecked before
  any consumer relies on them. This implementation uses merged PR 199 at
  `3832c47ef68a0e6214f2c1e85f3526b6adcb686e`; the older `8614e4a`/`1764bca`/`4919088`
  references are superseded.


## Local implementation update — 2026-10-23

The user clarified that reviewed plans remain local and implementation proceeds before publication. The earlier requirement to publish/land this plan first is superseded; pushes and PR publication still require explicit approval.

Canonical `storytold/deckcraft` `main` now resolves to `0f4fffcf6c71ffa16029d07ac23752fedd783547`. The owned checkout was fast-forwarded to this revision. Earlier baseline SHAs and line anchors above are retained as historical review evidence; use this new revision and its exact lockfile for current-main integration and paired measurements. The original refs remain reachable.

No migration API redesign required by this delta; preserve the merged packaging, test and chrono changes where present.

The complete static delta review and exact old/new object identities are retained in the task evidence: `canonical-diffs-refresh-20261023.json` and `overnight-consumer-review-20261023.md`. This amendment claims no new build, native GUI or performance acceptance.

## Local implementation amendment — 2026-10-24 (v17 review findings)

The v16 review found four concrete defects still open. Implementation here stays app-owned:
DeckCraft has no `craft-ui` dependency, and the reviewed `craft-ui` PR is not part of this
checkout, so no generic compatibility component is added or copied. The existing `capture.rs`
bounded registry remains the owner of screenshot admission, memory reservation and completion.

1. **Large split-button geometry.** A two-line label used `text_w + 10` while the 14 px arrow
   strip occupied the last 14 px of the button, leaving up to 4.5 px of the visible final line
   over the arrow target. The button now reserves the arrow strip *and* the label centre in the
   remaining main area, so no visible line reaches the strip. A regression uses a real pointer on
   the last line's widest glyph pixels and separately on the arrow.
2. **Small split-button names.** Arrows now use their own command name (`"Bullets options"`,
   `"Shape Fill options"`, …) instead of a repeated `"More options"`. Empty-label bullets and
   numbering expose their real names through a named variant, while the old
   `small_button_ex` signature keeps its default for ordinary labels.
3. **Screenshot completion bounds and wake.** `CaptureSlot::complete` checks the callback
   image's checked byte size against the reservation for the issued capture geometry *before*
   storing the image. An oversized frame becomes a retryable slot result, releases no stale
   reservation twice and still fails on the normal request deadline. The one-shot callback
   upgrades its weak slot, releases the slot lock, then requests a repaint of the exact issued
   viewport. Queued/settling requests force the bounded `RequestPaintWhileHidden` passes; issued
   requests rely on the screenshot command plus this wake, with a deadline wake for a missed
   frame. No per-frame 16 ms repaint timer remains.
4. **Hidden slideshow clock.** A hidden/minimized show is not treated as paused: hidden logic
   anchors the last visible egui time to a monotonic instant, advances the show clocks and
   automatic steps from logic alone, and requests a bounded logic wake while the show is active.
   Drawing stays separate, so a hidden forced paint cannot advance the clocks a second time and
   no paused-show product policy is invented.

The four v16 repairs (first-font requeue, stale-batch separation, IME no-op/unavailable receipts,
large split-button main/arrow hit targets) are retained. Verification remains with the parent's
managed `task.py` tests; this amendment claims no build, native GUI or performance result.

## Local implementation repair — 2026-10-24 (v20 review findings)

Scope is limited to the two findings in
`egui037-deck-bounded-final-review-20261024-root-v20.md`. Capture admission, reservation, wake,
encoding and persistence code are unchanged.

1. **Visible logic-only pass no longer re-anchors a hidden show.** `SlideApp::tick_hidden_show`
   now returns without touching `hidden_anchor` when the viewport is visible. Only a full visible
   UI pass in `show::ui` records `(InputState::time, monotonic instant)`; a `run_logic` pass for a
   visible window carries the previous full frame's egui time and can therefore no longer move
   the hidden clock backwards before the window is minimized. The regression drives a full
   visible pass, an over-due visible `run_logic` pass whose `InputState::time` is still stale, and
   then a hidden `run_logic` pass whose 2.5 s automatic advance proves the original anchor was
   preserved.
2. **All 14 small split callers now have live named halves.** `widgets::split_menu` /
   `split_popup` give a dropdown-only split one stable popup id and one anchor spanning both
   named halves, so the label/icon and the chevron open and close the same menu. This covers
   Layout, Section, Add Shape, Layouts, Convert, Change Colors, SmartArt Styles, Shape Effects on
   Home and Shape Format, Text Fill, Text Outline, and Compress Pictures. The colour variants use
   `split_color_popup` to retain the Eyedropper hand-back; Compress Pictures retains its
   close-on-click-outside behavior and width. Group and Rotate remain real split controls: their
   main halves execute `arrange.group` and `arrange.rotateRight`, and only their named options
   arrows open the existing menus. Main and arrow accessible names are unchanged.
   Pointer/accessibility regressions exercise the actual Home Layout caller, Shape Effects on
   both tabs, Shape Format Text Fill, Group, and Rotate.

Remaining limitations and scope:

- No Cargo, rustc, test, build, fmt, clippy, metadata, native-window or GUI command was run for
  this repair. Compilation and test outcomes remain with the parent's managed `task.py` workflow.
- The hidden-clock regression reproduces stale input time through `Context::run_logic`; it does
  not schedule an actual eframe `PassMode::LogicOnly` callback. Native overdue-redraw timing still
  needs the managed runtime check.
- The new pointer regressions cover representative callers rather than every dropdown-only call
  site; the remaining callers share the same helper and retain their accessible names.
- Split controls with a real main command other than Group/Rotate were already live and are not
  redesigned. No new generic UI component or compatibility shim was added.


## Cleared-preedit terminal ownership repair — 2026-10-24

An accepted empty preedit restores the committed model text, but is not delivery of the separate
terminal commit/delete. The UI now retains that composition owner/generation and a small expected
selection/revision record. Pointer/caret work, target replacement or another application revision
invalidates the pending record before it can authorize a later mutation. Its cancelled owner
survives idle passes and an outgoing interrupt until an incoming terminal commit (or platform
enable/disable) is observed. A refused terminal commit releases the record immediately, permitting
a new preedit later in the same raw batch. Accepted IME deletion refreshes the expected selection
so its own caret/revision shift does not reject its paired commit. Immutable target identity,
model composition/undo, existing active-composition reset policy and direct commits remain intact.
The event payload still carries no epoch: if an old terminal is never delivered, the first
ambiguous new composition after an unobserved reset can be rejected. No timer guesses provenance.

Five new actual-app tests cover11 scenarios: separate-batch pointer/caret and target interruption,
both deletion forms with paired or split terminal delivery, unchanged pending empty-preedit
replacement, direct commits, and fresh composition after the rejected terminal boundary. They use
real RawInput pointer/key/IME events and production target-selection commands; original tests and
assertions remain unchanged. These new bytes await parent-managed formatting, focused/full CI and
fresh independent review. No new native IME or performance acceptance is claimed.

The CI font-pin preflight's CRAFT_FONTS_REF is corrected to the same reviewed b076417b5880bd4154bd22f00695b1d28c6bea83
used by gate/FreeBSD/release; the old43913056 value remains only the historical pending comparator.
This is a separate one-line workflow correction, not a dependency/lock or font-asset change.

## Final current local review status — 2026-10-25

This section records the final local state after
`/home/codex/work/ssd2/task-evidence/egui037-migration-20261022/final-landing/deck-final-review0045.txt`.
Earlier sections remain the historical plan and implementation amendments; where they describe an
earlier snapshot or an open plan-time gap, this section is the current local status.

### Current review follow-up — 2026-10-25

The candidate workflows now use `37827adc41fd165f9a9234ac40f94dbad353aa69`, combining
all prior Latin presentation fonts with the migration's core and Chinese fallbacks.
This commit must be published before these workflow pins can be fetched remotely.
The reusable CI gate explicitly sets `require-craft-fonts: true`; its current default
also enables required mode. The earlier statement that this cannot be passed to the
shared gate is obsolete.

The local base for this follow-up is `8372d51552c3ed943ffeb283f26a482e94a2c12f`.
The five source files listed in the earlier dirty-union review are already tracked in
that commit. The follow-up also includes `crates/fonts/tests/fixtures/latin-families.txt`;
its 51-row family/style inventory comes from the prior `76ba5a0` catalog, whose font
bytes are preserved by the combined candidate. Include that fixture with the code.
Full follow-up checks and the final local review commit are recorded in the next
publication packet; old dirty-union hashes below are historical evidence only.

### Final pins

| Dependency | Final local pin | Note |
|---|---|---|
| egui family (`egui`, `eframe`, `egui-winit`, `egui-wgpu`, `epaint`, `ecolor`, `emath`, `epaint_default_fonts`, `egui_kittest`, …) | `storytold/egui` `6d518fc799681571963842f95b55a6b68431b15a` | `Cargo.toml` patch entries and `Cargo.lock` agree |
| winit | `storytold/winit` `a517b3846ee8457d84ee4975110bfda6797c44dc` | `Cargo.toml` patch entry |
| craft-media | `storytold/craft-libs` `f0ad6c1e1d297908915f4be315b0fcfc9be7b5d4` | intentionally separate craft-libs pin |
| craft-pdf / krilla patch | `storytold/craft-libs` `9f75d3dc19b4b514c4f3d4847ed1ec1b0e86b4ef` | intentionally separate craft-libs pin |

The craft-media `f0ad6c1e…` and craft-pdf `9f75d3dc…` pins are intentionally separate from the
egui/winit pins; they are **not** required to move to `25da1314…`. The font inputs bundled by
`epaint_default_fonts` at `6d518fc7…` are byte-identical to those at the superseded `173cc737…`
reviewed source (all files in `crates/epaint_default_fonts/fonts/` hash the same), so
`ATTRIBUTION.md` now cites the exact current source URL without changing provenance.

### Review disposition

- **P1 (source completeness, blocking publication):** the five untracked files above are required
  source. The dirty worktree can build while a clean checkout of `770f4653` cannot. Not fixed by
  this documentation edit and not claimed as qualified.
- **P2 (documentation/attribution):** header/base identity, the stale plan-time IME gap, the final
  pins and the `ATTRIBUTION.md` font source URL are reconciled in this revision.
- **P2 (IME completion race):** an unproven race risk where an old `ImeResetCompleted` could clear
  `Awaiting` for a changed destination before the finalizer. No failing reproduction exists. No
  production or test code was changed speculatively; the risk stays unqualified.
- **P3 (`thumbs_scroll` contract):** the comment advertises the value for the control channel while
  `#[serde(skip)]` keeps it out of `ui.inspect`; only tests read it directly. Recorded as a
  comment/dead-contract defect, not fixed here.
- **P3 (unreachable IME fixture):** the `Suspended { request: Some(…) }` test state is not produced
  by any production constructor, so it gives limited assurance. Not fixed here.
- The `ScrollArea::clamped_offset()` and `IMEOutput` Option handling reviews found no concrete
  production defect; thumbnail behaviour coverage (opening at a non-zero slide, wheel/drag,
  smooth scrolling) remains incomplete.

### Validation actually performed

- Retained full CI run 2325 reached fmt, `pdf-deps` and clippy PASS, then exhausted the 8 GiB
  reservation during test compilation. It does not establish that the current tests compile or
  pass.
- Full CI, test execution, WASM, native/browser IME, real GUI behaviour, clean-checkout build and
  performance were **not** passed.
- No Cargo, build, test, fmt, clippy, VM or resource command was run for this documentation edit.
- Publication, commit, push, PR, review comment and merge remain outstanding and require separate
  explicit human approval.
