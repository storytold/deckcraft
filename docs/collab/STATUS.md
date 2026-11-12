# Collaboration status and handoff

Updated: 2026-11-12. Branch: `collab/pr15`, branched from `collab/pr14` at `6e42a7ab` (the desktop
commit and its docs commit): **optimistic preview** (RFC §3) — with `DECKCRAFT_COLLAB_PREVIEW=1`
one local operation is shown before the room answers, settled by `applied`, rolled back exactly on
a refusal and abandoned for a `resync` when another participant's step lands in between; the wire
carries an optional `preview` local id that the server never stores, so a client without it behaves
exactly as before. On top of `1b057cbf` are two review-pass commits — `5b6fcfcd` (code) and
`9b43c611` (docs) — then the STATUS sync `fd1ee71a`, then four commits: `d56272e6`, `082b7c7b` and
`2bbe723f` (code, the second review) and `4a64e764` (docs, the second review's triage and the
restart limits), then `9ad80547` (code, tests and docs: the fourth review's triage), then
`f6239baf` (the docs-only STATUS sync), `891187f5` (code and tests: the fifth review's triage,
§Fifth CodeRabbit review), `38a20a9c` (docs: the triage table, the session log,
`production.md`'s close-failure contract) and `1f78cadb` (docs: name `38a20a9c` the delivered head
and record the unrun gate), then `fd5914e8` (the **presence heartbeat**: an unchanged presence is
repeated every 10 s, inside the room's 30 s TTL, and a moved one stays capped at 20 Hz; the test
drives the pump over an in-process pipe and fails without the heartbeat).
**The head is `1d92f763`**, two `origin/main` merges on top of `fd5914e8` and delivered to the fork
and PR #2012 (§PR publication); the §8 gate on it is green (§Verification, §Session log).
**`aaecc6db` is the newest code slice: one operation, one identity.** The normaliser gave every
operation of one command the command's single `op_id`; the room dedups on `op_id`, so it applied the
first operation of a multi-object command and answered the rest with the stored result — a
multiple-selection move or resize moved one shape and deleting several slides deleted one, on every
replica and with no rejection to notice it. Each operation now carries its own id (`<id>` for the
first, `<id>#<n>` after it), and a structural operation waits for the answer to everything sent
before it, so the N structural steps of one command go out one answer at a time instead of arriving
stale and costing a `resync` each. Details and consequences: §One operation, one identity.
**`51643c59` is the newest code slice: an input method's composition is settled before anything
leaves the client.** `Session::execute_unguarded` queued a shared session's command and returned
before it settled the composition, and the composition's own commands were queued as well, where
the normaliser has no form for them: a text typed through an input method could not reach a room
at all, and a command sent while a composition was open would have been planned against a document
that did not yet hold the composing text. The input method's commands now run on the replica, every
other command settles the composition before it is queued, and the settle hands the room the body
it typed into as one whole value — `text.setBody`, runs and paragraph properties included, one undo
step over the body before it. Details: §A composition is settled before anything leaves the client.
**`b314feef` is the newest code slice: an input method's deletion outside a composition is one
`text.delete`.** A shared session refused `text.deleteSurrounding` whenever no composition was open,
because the command had no wire form: a press-and-hold or a dictation asking for the characters
around the caret did nothing at all in a room. Outside a composition that request is an ordinary
deletion of the body, so the engine now resolves its range with the same walk `text.delete` uses
(`surrounding_delete`, shared by the single-user command and the normaliser) and the normaliser
sends exactly that range as one `TextDelete` (`NotesDelete` for notes, the cell address included)
with the body before the step as its undo image. One operation carries one range: a non-empty
selection with text deleted on both sides is two, and is still refused at the gate before the
queue, with its reason. Details: §A composition is settled before anything leaves the client.
**As of the push at 2026-11-12T03:13:50Z the delivered head is `5a105c61`** — the docs commit of
the op_id slice, on top of the code commit `aaecc6db` (and of the `origin/main` merge `c9803277`).
The fork (`copoka/deckcraft`) and PR #2012 hold exactly that commit: `gh pr view 2012 --json
headRefOid` returned `5a105c61a48d1484d8693332f54e42f95112f759` with `MERGEABLE` / `UNSTABLE` and
`updatedAt` 2026-11-12T03:13:50Z, and the local branch head was the same commit before this STATUS
commit; local HEAD now adds this docs commit on top of `5a105c61` and that commit is not pushed,
so the PR head lags the local branch by one docs commit again until a person pushes. Since then
five local commits are unpushed: `bc16494a` (docs), `51643c59` (code, §A composition is settled
before anything leaves the client), `95b5feef` (docs, that slice), `b314feef` (code, §A — an input
method's deletion outside a composition is one `text.delete`) and this STATUS commit.
**No workflow run exists for `5a105c61` or `aaecc6db`** (`gh run list --commit` returns
nothing for either), so the rollup on the delivered head carries no executed job and
`mergeStateStatus` is `UNSTABLE` for that reason alone. These statements are tied to that
timestamp: any later push moves the delivered head, and the live value is
`gh pr view 2012 --json headRefOid`.
**The delivered head has moved since that timestamp: `ebbecf08`** — the merge of `origin/main`
onto `collab/pr15` (the video-decode drop from main), pushed at 2026-11-12T05:28:24Z. At 05:44Z
the fork head, the PR head and the local branch head were all `ebbecf08`, so the four commits the
paragraph above calls unpushed (`bc16494a`, `51643c59`, `95b5feef`, `b314feef`) are inside the PR
now; the four workflow runs created on that head (CI, Nix, House rules, SonarCloud) are
`completed / action_required` and executed no job, so the rollup still carries no executed job.
On top of `ebbecf08` are local, **unpushed** commits: `42544987` (code, tests and the RFC: the
HTTP base slice, §A base above the inline limit is one HTTP request) and this docs commit (this
file, `production.md` and `rfc.md`).
**The delivered head has moved again: `1d92f763`** — the second merge of `origin/main` onto
`collab/pr15` (the chart-legend, chartEx-budget and guide-list drops from main), pushed
2026-11-12T12:13Z. At 2026-11-12T13:00Z the local head, `git ls-remote fork
refs/heads/collab/pr15` and `gh pr view 2012 --json headRefOid` all named
`1d92f763128ef7381df36fb444ece4b7d258910d` (`state` `OPEN`, `mergeStateStatus` `UNSTABLE`), so
`42544987` and the docs commits above it are delivered and nothing on the branch was unpushed
before this STATUS commit. The §8 gate was re-run on that head (§ Session log, 2026-11-12 — the
presence heartbeat is already on the branch).
The push delivered `fd5914e8` on top of `1f78cadb`, so every review fix up to that head is
delivered: the fifth review's
triage is delivered too — a room
whose snapshot fails stays live and answers `500` instead of closing, a snapshot that does not
cover a rewind is refused and not installed, and a move dropped by a role change is cancelled
exactly. The triage of `5852391245` was already delivered in `9ad80547`: `4a64e764` is its parent
and the commit adds 7 files, +498/−116. `7f93069e` — the connector-glue commit the earlier notes
named — is a dangling commit with the same subject as `2bbe723f` and is **not** in the branch. GitHub's
workflow runs on `38a20a9c` were created 2026-11-11T11:31:32Z and are all
`completed / action_required`; no later head has been approved for a run either — `gh run list
--commit 5a105c61` and `--commit aaecc6db` are empty — so the rollup carries no executed job. The
local TASK.md §8 gate on `fd5914e8` **did** run on
the build host and is green: `cargo fmt --all -- --check` clean, `cargo clippy -p
deckcraft-collab -p deckcraft-ui-egui --all-targets -- -D warnings` clean, `cargo test -p
deckcraft-collab` 288 + 8 passed, and `cargo test -p deckcraft-ui-egui --lib` 522 passed + 4
ignored with the one known host-font test skipped — it fails identically on `1f78cadb` (no CJK
glyph on the host), so it is an environment gap, not this change. Three PRs for this one feature
are open in `storytold/deckcraft` — #2012 (`fd5914e8` + docs, the leading PR), #1998 (`6e9d10e7`,
`CLEAN`) and #1975 (`be706a11`, open) — and closing the superseded two is the maintainer's
decision, not an agent's.
The CodeRabbit findings of #2012 (11 Major + 7 Minor) are triaged and fixed, with that table in §PR
publication. The second review's commits close the second review (§Second CodeRabbit review): a
shape drag now goes through the room, a client commit
is bound to the hub it was authorized against, a base reopens at its own version, one resync is on
the wire at a time, and the restart/recovery wording is corrected. The fourth review
(`5852391245`, on `4a64e764`) is triaged in §Fourth CodeRabbit review and the fifth
(`5855491108`, on `9ad80547`) in §Fifth CodeRabbit review. `collab/pr14` is branched from
`collab/pr13` at `baa845ec` (the
chunked-base slice and its docs commit): Save in a live desktop session now commits the room —
one commit in flight, the document stays dirty until `commit-result`, a refusal keeps the edits
and offers a copy — with its docs commit on top. `collab/pr13`, branched from `collab/pr12` at
`5c150c93` (the 3-D
model scene `7b51e65d` and its docs commit): a native base above the inline limit now travels as a
chain of chunks, so a client that missed a compacted journal is rebuilt from an 11.8 MiB base over
the session socket, resuming mid-chain after a drop. `collab/pr12` is two commits from
`collab/pr11` at `fe7bcbca` (the 3-D
model camera slice `c406a329` and its docs commit). `collab/pr11` is **rebased onto `origin/main`
at `0623b430`** (heads `collab/pr10` = `7459d798` plus its rebase note, `collab/pr11` =
`6e9d10e7` plus the publication
docs commit, the 3-D model slice, the CORS slice, the SmartArt insertion slice, the SmartArt
conversions slice, the SmartArt node-picture slice and this docs
commit; pre-rebase hashes are kept in the slice
notes below and mapped in
[Rebase onto origin/main](#rebase-onto-originmain-2026-11-09)), from
`collab/pr10` at `922a4bf4` (media slice `7fe75711`, feature slices `0a57b61e`…`69c2ff22`, undo
of fill/line/format `41fe1ad9`, undo of a group's fill/line and structural slide undo `4e666b95`,
structural undo of shapes `3ec81d70`, cabinet commit publication `31842050`, undo of
group/ungroup `98d9e9ce`, align/distribute as final bounds `31d353fe`, the TASK.md §8 gates and
the non-active-slide fix `5b078c7d`, base transfer and the three-client soak `e007e965`, the
browser link `66d4658c`, the transition slice `927e773c`, docs commits on top), plus the
table-text slice `2a91d49e`, its docs commit, the cell-set rebase `b54eed3d`, the animation-step
slice `f562aea4` with its docs commit on top, the notes-text slice `b7025f9a` with its docs
commit on top, the table-cell format slice
`a48c12de` with its docs commit on top, the run-range format slice `b4f13167` with its docs
commit on top, the N-cell/caret format slice `6ddc0c4f` with its docs commit on top, the
whole-slide animation slice `b31bff47` with its docs commit on top, the
rest-of-the-animation-surface slice `e2a5ef41` with its docs commit on top, the
document-font slice `fd4e54e8` with its docs commit on top, and the rest-of-the-table-surface
slice `c895183a` with its docs commit on top, and — after `02e87716` — the table-options
slice: the six style toggles as one final value and the Table Layout ▸ Cell Size fields for
column width and row height, with its docs commit on top, and — after `f3945ddf` — the SmartArt
slice: outline, layout, style and colour edits as one whole `Diagram` value, with its docs commit
on top, and — after `d9312c03` — the chart slice: chart type, data and options edits and an
insertion as one whole `Chart` value, with its docs commit on top, and — after `6745633d` — the
3-D model slice: `insert.model3d` on the media channel as one binary glTF signature with its own
budget, drawn as its poster or a stand-in cube, and then the CORS slice `f996865d`: the sync
server answers a browser page whose origin is in `DECKCRAFT_CORS_ORIGINS` with
`Access-Control-Allow-Origin`/`Vary` and a preflight, and sends no `Access-Control-*` header to
anyone else, and then the SmartArt insertion slice `cfd40da2`: `insert.smartArt` as one whole
`Diagram` value with the whole inserted shape as the undo image, and `smartArt.nodes` answered
as a read-only query by the server, and then the SmartArt conversion slice `db413e5d`:
`smartArt.fromText`, `smartArt.toText` and `smartArt.toShapes` as **one structural step each** —
the sender runs the engine's `from_text_plan`/`to_text_plan`/`to_shapes_plan`, the operation
carries the whole resulting diagram or text body and the final box (or just names the graphic to
detach), the engine checks the value before the first write, and the undo image is the whole
source shape, and then the SmartArt node-picture slice `44489f34`: `smartArt.picture` as **one
final value** — the operation addresses the top-level graphic and one top-level node and carries
the media channel's digest and the picture's final place (crop and opacity), the engine checks
the target, the node, the picture allowlist and budget, the place's finite positive area and that
the layout does not change before the first write, the picture is installed with the same
`add_media` call `insert.picture` runs so every replica allocates the same media id, and the undo
image is the whole group, and then the 3-D model camera slice `c406a329`: `model3d.camera` as
**one final view** — `position`, `target`, `up` in slide EMU and `fov` in degrees, or `null` for
the reader's default view, never a relative edit — checked by the engine's own `camera_param` on
the client and again before the first write, written into the slide part as `am3d:camera`, with
the **whole model shape** as the undo image, and then the 3-D model scene slice `7b51e65d`:
`model3d.scene` as **one final scene** — one light (`type` directional/point/spot, a non-zero
`direction`, `intensity` 0–1 and a spot's cone `angle` in degrees 1–179) and the environment
(`ambient` 0–1 and an optional sRGB `background`), or `null` for the reader's default — never a
relative edit, checked by the engine's own `scene_param` on the client and again before the first
write, written into the slide part as `am3d:scene`, with the **whole model shape** as the undo
image — and then the chunked-base slice: a base above `max_base_inline` is announced in the
snapshot as `base.chunks` (digest, length, chunk size, depth) and fetched one chunk at a time with
`base-chunk`, assembled and digest-checked by the client, resumable at the missing index, capped on
both sides, with the "named without bytes" path kept as the fallback — with this docs commit on
top — and then the HTTP-base slice `42544987`: a base above `max_base_inline` is also announced as
one request (`base.http = {path, sha256, len}`, RFC §4.5.2) that the desktop link answers from
`GET /v1/sessions/{id}/base/{sha256}` in the media reader's pattern — session token, digest in the
normalised path, `application/vnd.deckcraft.native`, a `401` for another file and a `404` for a
base the room no longer holds — with the chain kept in the same snapshot for a client without an
HTTP side, size and digest checked before the replica is replaced, and no reopen from the cabinet
below 256 MiB. Details: §A base above the inline limit is one HTTP request.
The production operator view is
[`production.md`](production.md); the protocol design is [`rfc.md`](rfc.md); the prepared PR
description is [`pr10.md`](pr10.md).

## Merge of `origin/main` (2026-11-11)

`origin/main` moved to `c6846f6a` — 28 commits over `0623b430` — after this branch was delivered,
so PR #2012 went `CONFLICTING` / `mergeStateStatus: DIRTY`. The branch was **merged, not
rebased**: merge commit **`c9803277`** has `da8ba866` as its first parent and `c6846f6a` as its
second, so every slice hash named in this file still names its commit and no history was
rewritten. The tree is clean; local HEAD was that merge commit with a STATUS commit on top, and at
the time of the merge the fork and the PR still held `da8ba866`; the merge was pushed later and the
delivered head has since moved on (see the header — `5a105c61` at 2026-11-12T03:13:50Z).

What main brought: the media codecs and containers move to craft-libs, so the `opus`, `ogg`,
`isobmff` and `matroska` crates leave the workspace (`git diff --stat HEAD^1 HEAD` on the merge:
94 files, +1 955/−7 865), together with the photo-album reopen limits, the metafile and PPTX
budget work and the `ptLst`-order fallback in the SmartArt outline reader.

`git merge-tree --write-tree HEAD origin/main` named two content conflicts before the merge. Both
are resolved as a union that keeps main's change **and** the collaboration side, not as a choice:

| File | Conflict | Resolution |
|---|---|---|
| `Cargo.lock` | one hunk: main puts the new `craft-audio` package (`storytold/craft-libs@75895c03`) where the branch's `cpufeatures 0.3.1` entry sits | both entries are kept. The first build on `agents` then rewrote exactly four lines — the workspace version main bumped to 0.18.0 in the four collaboration crates' own lock entries — and that lock is the committed one. The `copoka/craft-libs` host pin (ADR-0016 §3) is untouched and no `opus`/`ogg`/`isobmff`/`matroska` package is left |
| `docs/architecture.md` | one hunk over the L0/L1/L2 rows | those rows take main's wording (the `bitstream`/`media` split, the craft-libs decoders) with `collab-proto` added back to L0; the L5 row keeps `collab`. The header and the changelog row are main's |

One file auto-merged into a compile error and is fixed in the same commit, not left for later:
main's new `ptLst`-order fallback in `crates/pptx/src/dgm.rs` (`outline_ids`) builds a
`DiagramNode` that predates this branch's `picture_place` field, so the fallback node carries
`picture_place: None` — a node built there has no node picture, and `read/shapes.rs` fills the
place from the picture element exactly as before. Nothing else needed a hand edit: the other
files main touched (`crates/engine/src/tools/*`, `crates/engine/src/cmd/*`, `crates/pptx`, the
workflows, `xtask/src/layers.rs`) merged cleanly and compile with the collaboration code.

The TASK.md §8 gate was run on the merge commit on `agents` (20 cores, Ubuntu 24.04, rust 1.99.1):

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | clean (EXIT=0) |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean (EXIT=0) |
| `cargo test -p deckcraft-collab-proto` | 42 passed |
| `cargo test -p deckcraft-collab` | 288 + 8 passed |
| `cargo test -p deckcraft-collab-server` | 19 unit + 5 commit + 18 socket passed |
| `cargo test -p deckcraft-collab-server --test gates -- --test-threads=1` | 5 passed in 19.1 s (LibreOffice skipped on the build host) |
| `cargo test -p deckcraft-collab-web` | 15 passed |
| `cargo test -p deckcraft-engine` | 612 + 1 ignored, plus 6 + 8 integration |
| `cargo test -p deckcraft-pptx` | 70 + 5 + 146 passed |
| `cargo test -p deckcraft-ui-egui --lib` | 526 passed, 4 ignored, 1 failed — the known host-font case `ime_tests::korean_composes_in_the_notes_pane` (`No glyph for "ㄴ" (U+3134)`, the build host has no `craft-fonts`); it fails identically on the pre-merge head, so it is an environment gap, not this change |
| `cargo xtask pdf-deps` | EXIT=0 |
| `cargo xtask assets` | EXIT=0, all 48 asset files attributed |
| `cargo xtask layers` | EXIT=0, 28 crates, no violations (32 before the merge: main removed the four codec crates) |
| `cargo xtask wasm` | EXIT=0, wasm32 check of 22 crates (one wasm-only `dead_code` warning for `SlideApp::opening_generation`, which `origin/main` carries too) |
| `cargo test --workspace --doc` | EXIT=0, the workspace's 3 doctests pass |

`cargo xtask ci` as a whole cannot be EXIT=0 on that host for one reason only: its `test` step is
the workspace test run, which includes the same host-font failure above. Every other step of it
was run on its own and is green, as the table shows.

**`mergeStateStatus` must be re-checked after the push.** GitHub computes `mergeable` and
`mergeStateStatus` from the pushed head, and locally the only verdict available is that the merge
applies and builds. Pushing `collab/pr15` to the fork — and approving the workflow runs on the new
head — is the owner's step (§PR publication); an agent does not push.

## PR publication (2026-11-11)

**[PR #2012](https://github.com/storytold/deckcraft/pull/2012)** is open from
`copoka/deckcraft:collab/pr15` into `storytold/deckcraft:main` and is **ready** (not a draft); its
body is [`pr10.md`](pr10.md). At 2026-11-12T03:13:50Z the fork head **is** the PR head **is** the
local branch head: `5a105c61`, the docs commit of the op_id slice on top of the code commit
`aaecc6db` (`gh pr view 2012 --json headRefOid` and `git rev-parse HEAD` agree). Everything the
branch delivered is there: the second review's commits, the fourth review's triage, the STATUS
sync, the fifth review's triage (`891187f5`), its docs commit (`38a20a9c`), the delivered-head note
(`1f78cadb`), the presence heartbeat (`fd5914e8`), the `origin/main` merge `c9803277` (§Merge of
`origin/main`) and the op_id slice (`aaecc6db` + `5a105c61`, §One operation, one identity). GitHub
reads the branch `MERGEABLE` / `mergeStateStatus: UNSTABLE` against today's `main`; the merge
commit resolved the earlier `CONFLICTING` / `DIRTY` verdict, and no conflict has been reported
since.

The **workflow gate has not run on any of these heads**, including the delivered one: `gh run list
--commit 5a105c61` and `--commit aaecc6db` are both empty, the four workflow runs (CI, House rules,
Nix, SonarCloud) on `38a20a9c` were created 2026-11-11T11:31:32Z and are all
`completed / action_required` — they wait for a maintainer's approval and executed no job — and the
runs for the later heads wait for the same approval. The combined status for `38a20a9c` is empty
(`state: pending`, no statuses) and the only check run is CodeRabbit's, `completed / skipped`
(started 11:33:45Z), so the rollup carries no other signal; GitHub's verdict is
`mergeStateStatus: UNSTABLE` for exactly that reason. The **host gate** on the delivered head is
recorded in `.craft/logs/gate-deckcraft-2012-5a105c61.log` (`cargo xtask ci -j 12` on `agents`):
`fmt`, `pdf-deps` and `clippy` are `ok`, and the run stops at `test` on the known host-font
failure `render::tests::east_asian_vertical_text_stands_ideographs_upright`
(`crates/render/src/tests.rs:2396`, no CJK glyph on the host: "eaVert: upright stroke 39x70"),
which fails identically on earlier heads. The narrower gate on `aaecc6db` is green:
`cargo fmt --all -- --check` clean, `cargo clippy -p deckcraft-collab -p deckcraft-collab-server
--all-targets -- -D warnings` EXIT=0, `collab` 294 lib + 8 two_clients, `collab-server` 19 unit +
5 commit + 5 gates + 18 socket, `ui-egui --lib` 526 passed + 4 ignored with only that host-font
failure. CodeRabbit's
last completed check run is `completed / success` on `4a64e764`; its run on `9ad80547` was
submitted as review `5855491108` at 07:49:58Z (see §Fifth CodeRabbit review). Its five submitted
reviews are
all on record: `5845265345` (16:15Z, on `1b057cbf`, 11 Major + 7 Minor — fixed in `5b6fcfcd` +
`9b43c611`), `5846427839` (17:33Z, on `9b43c611a`, five actionable comments — triaged and closed
in `d56272e6`, §Second CodeRabbit review), `5848775355` (20:22Z, on `fd1ee71a`), which
reviewed only this file ("Files not fully reviewed: 1"), posted **no inline comment and no
actionable finding**, `5852391245` (2026-11-11T01:35Z, on `4a64e764`, four Major and one Minor
— triaged in §Fourth CodeRabbit review) and `5855491108` (07:49:58Z, on `9ad80547`, four Major
and one Minor — triaged in §Fifth CodeRabbit review).

Merging is a human step. Closing the superseded PRs is a human decision as well.

**Three pull requests for one feature are open in `storytold/deckcraft`** (checked
2026-11-12T05:44Z):

| PR | Branch | Head | GitHub state |
|---|---|---|---|
| [#2012](https://github.com/storytold/deckcraft/pull/2012) | `copoka/deckcraft:collab/pr15` | `1d92f763` (the second `origin/main` merge) — fork = PR = local head at 2026-11-12T13:00Z; the `42544987` base slice and the later docs commits are inside it, and only this STATUS commit is new | leading PR; `MERGEABLE` / `UNSTABLE` (no workflow run on the head has been approved, so nothing executed; approval is the next step); local §8 gate green on `1d92f763` (6 gates; the LibreOffice half skipped, no `soffice` on the host) |
| [#1998](https://github.com/storytold/deckcraft/pull/1998) | `copoka/deckcraft:collab/pr11` | `abffb229` | `MERGEABLE` / `CLEAN`, superseded; **no unique code** (`pr15...pr11` = 32/4, and its `picture_place` fix is already on pr15 at `crates/pptx/src/dgm.rs:528`) |
| [#1975](https://github.com/storytold/deckcraft/pull/1975) | `copoka/deckcraft:collab/pr10` | `6109fe1c` | `CONFLICTING` / `DIRTY`, superseded |

#2012 is the top of the stack and supersedes the other two: #1998 carries the earlier
`collab/pr11` head and #1975 the earlier `collab/pr10` head, both already contained in #2012's
history. The overlap is resolved by the maintainer, not by this project — **the agent does not
close them**; a person decides which pull request survives.

### CodeRabbit triage on #2012 (`1b057cbf`, 11 Major + 7 Minor)

Every finding was checked against the code first. All 18 are addressed in `5b6fcfcd` (code) and
`9b43c611` (docs), now the PR head: 13 code fixes with a regression test where one is meaningful,
4 documentation corrections and one deliberately documented behaviour (the journal order).
"Fixed" means the change is in those two commits; they are pushed as `9b43c611a`.

| # | Finding (path:line) | Severity | Disposition |
|---|---|---|---|
| 1 | `apps/deckcraft-collab-server/src/lib.rs:394` — a 16 MiB reader cap refuses media the channel accepts (audio/video and 3-D models are 32 MiB) | Major | **Fixed**: `MAX_REQUEST_BODY` is the widest media kind (32 MiB), and a 20 MiB `PUT` regression test reads through the real socket path |
| 2 | `apps/deckcraft-collab-server/src/lib.rs:647` — `DELETE /v1/sessions/{id}` leaves an orphan room writing to the store | Major | **Fixed**: `Hub::close()` is set before the room leaves the map; `receive` refuses, the pump exits on the closed flag. Unit test |
| 3 | `apps/deckcraft-collab-server/src/store.rs:85` — a replay preloads bytes only for `PictureInsert` | Major | **Fixed**: `MediaInsert`, `FontCreate` and `SmartArtPicture` preload too. An audio object across a restart is a new store test |
| 4 | `crates/collab/src/client.rs:426` — ops sent after a `Resync` are dropped as "covered" | Major | **Fixed**: `resync_in_flight` holds the queue until the snapshot; new test submits after the resync and checks the op is sent with the snapshot's base |
| 5 | `crates/collab/src/hub.rs:166` — two connections of one actor share a peer slot and outbox | Major | **Fixed**: `peers`/`out` are keyed by a per-connection id; the server uses `actor#n`, the actor stays on `Peer` for authorship, history and presence; `participants()` counts people. Two-connection and close tests |
| 6 | `crates/engine/src/lib.rs:712` — the read-only/collab-queue gates also hit nested `execute` | Major | **Fixed**: the gates apply at `depth == 0`; a typed `* ` list lead runs `format.bullets` in place on a viewer and is not queued as a local edit. Two engine tests |
| 7 | `crates/pptx/src/read/shapes.rs:323` — camera `pos`/`lookAt`/`up` accept `NaN`/`inf` | Major | **Fixed**: finite and bounded (±1e9) members only; a camera with one is dropped whole. Test |
| 8 | `crates/pptx/src/dgm.rs:434` — crop `srcRect` accepts `NaN`/`inf` and overlapping sides | Major | **Fixed**: finite members only, a crop whose sides overlap is dropped. Test extended |
| 9 | `docs/collab/production.md:286` — the media replay guarantee was too narrow | Major | **Fixed in code** (#3) and the doc now names every media-bearing payload |
| 10 | `docs/collab/production.md:341` — the journal is written after the operation is applied | Major | **Documented**: the applied result is the server's decision and is broadcast before the append. A failed append is logged, the room keeps serving, and the doc now states the durability loss and the recovery step |
| 11 | `docs/collab/production.md:357` — the 64 MiB base ceiling is unstated | Major | **Fixed in docs**: `BaseChunks::MAX_LEN` caps an assembled base too, so a larger one is named even inside the chunk depth |
| 12 | `apps/deckcraft-collab-server/src/cabinet.rs:179` — `size + 2` on a cabinet-supplied chunk size | Minor | **Fixed**: `checked_add`; test |
| 13 | `crates/collab/src/transport.rs:108` — `Drop` blocks while the worker is inside `tungstenite::connect` | Minor | **Fixed**: a `connecting` flag makes `Drop` detach instead of joining a connect with no timeout. No deterministic test (timing-only) |
| 14 | `crates/pptx/src/dgm.rs:360` — the SmartArt node picture's `alpha` is lost on save | Minor | **Fixed**: written as `a:alphaModFix amt` and read back; round-trip test |
| 15 | `crates/ui-egui/src/app/commands.rs:25` — `file.save` answers `{"committed": true}` even when the commit was refused | Minor | **Fixed**: `save()` returns `Result`; the command maps it. Call sites updated |
| 16 | `crates/ui-egui/src/ribbon/table.rs:221` — a focus loss runs the cell-size command with no change | Minor | **Fixed**: the value is compared with the field before the command runs |
| 17 | `docs/collab/production.md:104` — token validation overstated (`sessionId`, `app`/`fileId`) | Minor | **Fixed in docs**: signature/`exp`/`sub`/`aud` checked, `app`/`fileId` only when present, session id not checked |
| 18 | `docs/collab/study.md:281` — option A/B labels differ from `options.md` | Minor | **Fixed in docs**: an explicit mapping note (study B = options A) |

Verification of this pass on the `agents` host, before the push: `cargo fmt --all` clean; `cargo clippy
--workspace --all-targets -- -D warnings` clean; `collab` 283 + 8; `collab-server` 13 unit + 5
commit + 5 gates + 18 socket; `engine` 594 + 1 ignored (+ 6 + 8 integration); `pptx` 69 + 5 + 145;
`ui-egui collab::` 24 and `host_save` 12; `deckcraft-web` 6; `xtask layers` 32 crates. The whole
`ui-egui` library run has one environment failure — `ime_tests::korean_composes_in_the_notes_pane`
needs a CJK face and the build host has no `craft-fonts` — unrelated to this diff and present
before it.

### Second CodeRabbit review (`5846427839`, 17:33Z on `9b43c611a`), triaged and closed

The review posted at 17:33Z on `9b43c611a` reported five actionable comments (four Major, one
Minor), a "SafeArt" typo nit and two trivial test nits. Each was opened against this tree first;
**six are live and fixed here**, each with a test that fails without the fix (all six were run
against the un-fixed code and failed). One finding's wording was narrower than the code: the
comment is anchored at `lib.rs:846` (`fn rejected`), but the commit path it names is
`commit_from_client` — the anchor was found by `ServerMsg::Commit`, not by the line number.

| # | Finding (path:line) | Severity | Disposition |
|---|---|---|---|
| 1 | `crates/engine/src/tools/drag.rs:225-240` — a shape drag bypasses the ordered path: a viewer's drag moves locally, an editor's release submits nothing | Major | **Fixed** (`d56272e6`, `crates/engine/src/tools/pointer.rs`): a viewer's press no longer starts a writing gesture (the drag is dropped after the selection, before any `edit`), and an editor's release submits the boxes the drag left as one `shape.setBoundsMany` with final values — the same final-value form `arrange.align` uses — which the client normalises into one `ShapesSetBounds` (`crates/collab/src/local.rs`, new `drag_bounds`). Alt-drag duplication makes new objects and has no command yet, so a shared session treats the gesture as a move instead of leaving a copy on one replica. Ungluing a dragged connector from shapes that stay put is a document change the box value cannot carry, so `2bbe723f` leaves the glue alone in a shared session (every replica keeps it) and keeps the single-user unglue. Tests: `a_viewer_drag_moves_nothing_and_a_shared_drag_is_one_operation`, `a_shared_connector_drag_keeps_its_glue` (engine) and `a_finished_drag_normalises_to_one_operation_with_its_final_boxes` (collab) |
| 2 | `apps/deckcraft-collab-server/src/lib.rs:846` — a commit is authorized against one hub and built from another, so a socket that outlived `DELETE` can publish the room that replaced it | Major | **Fixed** (`d56272e6`): `build_and_publish_on` takes the hub the caller was authorized against and refuses when it is no longer the live room (`still_live`: same `Arc` in the map and not closed); the accepted etag is adopted only by that hub. Test: `a_commit_is_bound_to_the_hub_it_was_authorized_against` |
| 3 | `docs/collab/production.md:348` — the replay claim: `apply_confirmed` does not rebuild actor history | Major | **Fixed in docs** (`production.md`) and in the `store.rs` module doc: replay brings back the document, the version and the dedup window; a step taken before a restart cannot be undone after it |
| 4 | `docs/collab/production.md:348` — a room reopened from a base at version 200 reports version 0 | Major | **Fixed** (`d56272e6`): the base carries the document only, so `Store::load` now continues the numbering from `meta.json` (`DocSession::set_version`) before replaying the tail. Test: `a_base_reopens_at_the_version_it_was_taken_at` |
| 5 | `docs/collab/production.md:353` — recreating a room does not reopen the cabinet file, and a damaged journal answers `500` | Major | **Fixed in docs**: new §Recovering a damaged store — keep and name the store directory, force a snapshot while the process lives, move the damaged directory aside, then create the session with the cabinet's committed bytes |
| 6 | `crates/collab/src/client.rs:441` — a second `Resync` while one is in flight can drop an edit | Minor | **Fixed** (`d56272e6`): one resync on the wire at a time (`resync_in_flight` also gates the request); the snapshot that answers it clears the flag a gap raised meanwhile. Test: `a_gap_during_an_outstanding_resync_never_puts_a_second_request_on_the_wire` |
| — | `docs/collab/STATUS.md:1350-1351` — "SafeArt" | Minor | **Fixed in docs**: the typo was already corrected in `fd1ee71a`; no occurrence is left |
| — | `crates/ui-egui/src/host_save_tests.rs:128`, `crates/ui-egui/src/collab.rs:1726` — the tests ignore the `Result` of `save()` | Trivial | **Fixed** (`082b7c7b`): both assert the `Err` the UI reports |

### Fourth CodeRabbit review (`5852391245`, 01:35Z on `4a64e764`), triaged

Five actionable comments (four Major, one Minor). Each was opened against this tree before any
change. **Four are live and fixed here**; the third is a documentation claim that was stronger than
the code and is corrected in the docs. Every code fix has a test that fails without it: the three
tests were run against the reverted production change and failed on the intended assertion (the
reversion was applied and reverted on the build host, the tree was restored afterwards).

| # | Finding (path) | Severity | Disposition |
|---|---|---|---|
| 1 | `apps/deckcraft-collab-server/src/store.rs` — `snapshot` writes `meta.json` (version N) before the base (version M); a crash between the two leaves `load` skipping the journal entries M+1..N, so the room reopens without acknowledged edits | Major | **Fixed**: the base is `base-<version>.deckcraft` (temporary file + rename), so its bytes and the version they were taken at are one unit; `meta.json` follows it through a temporary file and a rename, and the journal is trimmed last. `load` takes the version from the base's name and tolerates a missing or torn `meta.json` (a legacy `base.deckcraft` still loads with the version from its metadata). Test: `a_newer_meta_beside_an_older_base_keeps_the_acknowledged_tail` |
| 2 | `crates/collab/src/client.rs` — a `rewind_replica` during an in-flight resync is lost: the snapshot clears `resync_wanted` and advances `replica_version` past the rewound version, so the missing tail is never asked for | Major | **Fixed**: `rewind_pending` remembers the rewound version; a snapshot that does not cover it (no base at or above it, tail starting above it) is refused — the replica stays where it is and the tail is asked for again from there. Tests: `a_rewind_during_an_outstanding_resync_keeps_asking_from_the_rewound_version`, `a_base_at_or_above_the_rewound_version_answers_the_rewind` |
| 3 | `docs/collab/production.md:348` — the deduplication guarantee is stronger than the code: `Store::snapshot` trims through the base version and `DocSession::open` restores only the replayed tail's ids | Major | **Fixed in docs**: §Restart and persistence states that the window is not persisted beyond the replayed tail, what a retry after compaction would do, and why the client does not normally send one. No code change: persisting 10 000 ids in every base is not worth its size for a retry the client already avoids |
| 4 | `docs/collab/production.md:370` — `DELETE /v1/sessions/{id}` closes the room without a snapshot, and recovery step 4 reopened from the cabinet and discarded a newer base in the quarantined directory | Major | **Fixed in code and docs**: `close_session` takes a store snapshot before the hub leaves the map (a failed snapshot is logged, the close still succeeds), and §Recovering a damaged store closes the room first and recovers the newest base from the quarantined directory instead of the cabinet's older bytes |
| 5 | `crates/engine/src/tools/pointer.rs:445` — a viewer can start an Ink drag (also a shape/text tool, the eraser, a freeform and an Edit Points vertex) on a path that returns before the read-only gate, so `add_ink` writes the replica with no room operation | Minor | **Fixed**: `down` drops a writing drag and skips the writing tool arms and the Edit Points block when the session is read-only, keeping a text selection and a marquee; the gate at the end of `down` stays as a backstop and now also clears the ink and path previews. Test: `a_viewer_tool_press_writes_nothing` |

Verification of this pass on the `agents` host (rust 1.99.1): `cargo fmt --all --check` clean;
`cargo clippy --workspace --all-targets -- -D warnings` EXIT=0; `collab-server` 16 unit (8 of them
`store::`, the new crash-order test among them) + 5 commit + 5 gates (14.8 s, LibreOffice skipped on
the build host) + 18 socket; `collab` 287 + 8; `engine` 597 + 1 ignored, plus 6 + 8 integration. The
reverted-code runs failed exactly as intended: `the replica stays at the version it was rewound to`,
`the acknowledged step was skipped`, `a viewer's ink stroke changed the slide`.

### Fifth CodeRabbit review (`5855491108`, submitted 07:49:58Z on `9ad80547`), triaged

The check run that started at 2026-11-11T07:32:03Z submitted review `5855491108` on
`repos/storytold/deckcraft/pulls/2012/reviews` at 07:49:58Z: five actionable inline comments
(four Major, one Minor), all on files this PR touches (`lib.rs`, `store.rs` twice, `client.rs`,
`pointer.rs`); its summary also names a docstring-coverage pre-merge check, which is advisory and
not a finding. Each comment was opened against the tree at `9ad80547` before any change; **all
five are live and fixed in `891187f5`**, each with a test that fails without the fix. The four
reverted-code runs were made on the build host by undoing the production hunks only and running
the new tests: `a viewer kept a position the room never accepted` (200.0 against 100.0),
`a refused snapshot moved the host's version` (4 against 0), `a directory that cannot be read is
not an empty room` (the old `Ok(None)`) and `a close without its recovery copy is not a close`
(`200 {"closed": true}` against `500`); the durability test's old code simply returned `Ok(())`.

| # | Finding (path:line) | Severity | Disposition |
|---|---|---|---|
| 1 | `apps/deckcraft-collab-server/src/lib.rs:679-682` — a failed `store.snapshot` in `close_session` still removes the hub and answers `{"closed": true}`, so the documented recovery copy does not exist | Major | **Fixed**: the snapshot is taken while the room is still in the map and still serving; a failure answers `500` with `closed: false` and leaves the room live — still answering its sockets and still the hub a commit is bound to (`still_live`). A poisoned hub lock is treated the same way. Only a successful snapshot closes the room and removes it. Test: `a_close_whose_snapshot_fails_keeps_the_room_live` (the room stays in the map, is not closed, still serves `build_and_publish_on`, and the retry after the store is writable leaves a `base-` file) |
| 2 | `apps/deckcraft-collab-server/src/store.rs:161-162` — `latest_base` swallows `read_dir`/entry errors, so a storage fault reads as "no versioned base" and `create_session` opens `initialBytes` (or a blank deck) over the stored room | Major | **Fixed**: `read_dir` and every directory entry propagate their error, and the legacy `base.deckcraft` check runs only after the directory was listed in full and held no versioned base. Test: `a_room_directory_that_cannot_be_read_is_an_error_not_a_new_room` (the room path is a file, so `read_dir` is `ENOTDIR` for every user, root included) |
| 3 | `apps/deckcraft-collab-server/src/store.rs:146` — the base is renamed into place but its directory is never synced, so a crash can lose the new base while the journal has already been trimmed to the steps above it | Major | **Fixed**: `sync_dir` (`File::open(dir).sync_all()`, a no-op on platforms without directory handles) runs after the base rename and again after `meta.json`, before `trim_journal`; a sync failure propagates and the snapshot stops with the journal intact. Test: `#[cfg(unix)] a_base_that_cannot_be_made_durable_stops_the_snapshot_before_the_journal_is_trimmed` (a directory that can be written and entered but not opened: `0o300`; the test returns early where permissions are not enforced, e.g. root) |
| 4 | `crates/collab/src/client.rs:452-461` — a snapshot refused for not covering a rewind still advances the host: `CollabDoc::on_msg` applies the base/tail and takes `snapshot.version`, so the missing steps are skipped as already seen | Major | **Fixed**: `ClientSession` records the refusal (`take_refused_snapshot`) and `CollabDoc::on_msg` returns before installing anything; the replica stays at the rewound version and the tail is asked for again from there. Test: `a_snapshot_that_does_not_cover_a_rewind_is_not_installed_by_the_host` (a resync in flight, a preview abandoned by a foreign step, then an empty tail at version 4 with no base: version stays 0 and a `Resync { last_version: 0 }` goes out) |
| 5 | `crates/engine/src/tools/pointer.rs:478-483` — a move dropped because the role turned read-only keeps the local boxes and an open interaction, so the viewer shows a position the room never accepted and the next gesture inherits a stale before-image | Minor | **Fixed**: `drop_writing_drag` cancels a move (`Drag::Move` carries the offset it last applied): only a box this gesture wrote is put back to its press-time value, a confirmed step another participant settled meanwhile stays, and the interaction closes without an undo step; the release path (`up`) does the same instead of offering the boxes to the room. Alt-drag duplicates are left alone (they make objects no operation carries). Tests: `a_role_change_during_a_move_puts_the_local_box_back` (an editor's drag, a remote step on another shape, then `set_read_only` and a new press: the dragged box is back, the remote box stays, no interaction, no undo step) |

The Minor finding's wording named only the new-press path; the release path had the same hole (a
viewer whose pointer came up after the role change kept the local move and got an undo step for
it), so both are closed. What is **not** covered: a resize, rotate or Edit Points drag dropped by
a role change still keeps the geometry it wrote — those gestures carry no stored "last written"
value, so the same exact cancel needs one, and the move is the gesture the review named.

Verification of this pass on the `agents` host: `cargo fmt --all --check` clean; `cargo clippy
--workspace --all-targets -- -D warnings` EXIT=0; `engine` 598 + 1 ignored (+ 6 + 8 integration);
`collab` 288 + 8; `collab-server` 19 unit + 5 commit + 5 gates (16.9 s, LibreOffice skipped on the
build host) + 18 socket.

This file is the handoff between sessions: what is done, how it was verified, what is not done,
and what to do next. Update it at the end of every collaboration session.

## Branch map

| Branch | Commit | Slice |
|---|---|---|
| `collab/study` | — | Phase 0 study/options |
| `collab/rfc` | `bcd68aa2` | RFC from ADR 0001–0010 |
| `collab/pr1` | `78de01b8` | wire types, adapter, `apply_remote` |
| `collab/pr2` | `92577c30` | per-user undo/redo |
| `collab/pr3` | `e9a18903` | `DocSession`: versions, dedup, snapshots, commit |
| `collab/pr4` | `62652fb1` | `ClientSession`: queue, dedup, resync, viewer |
| `collab/pr5` | `4ec6b3a8` | commit/409, presence, two-client e2e |
| `collab/pr6` | `1d07ce21` | UI: flag, indicator, participants, presence, viewer gate |
| `collab/pr7` | `d28c8ae5` | `CollabDoc` command glue + in-memory `Hub` |
| `collab/pr8` | `759b6124` | WebSocket server, desktop link, UI wiring, undo requests, persistence |
| `collab/pr9` | `149d9840` | text insert/delete with rebase, shape insert, engine command queue |
| `collab/pr10` | `7fe75711` + `0a57b61e`…`69c2ff22`, `41fe1ad9`, `4e666b95`, `3ec81d70`, `31842050`, `98d9e9ce`, `f28ee048`, `e007e965`, `927e773c` | media channel (pictures by `sha256`, upload/fetch, persistence); audio/video, TLS media client, `shape.fill`/`shape.line`/`format.*`/`slide.duplicate`, `arrange.align`/`arrange.distribute` as final bounds, `transition.set` as a whole final value; undo of fill, line, `format.*`, a group's fill/line, slide insert/delete, shape insert/delete, group/ungroup, align/distribute and transition; commit published to the cabinet as a new file version (`409` on an external change); base transfer after compaction with the key map, and a three-client soak gate |
| `collab/pr12` | `7b51e65d` (+ docs) | `model3d.scene` as one final scene — one light (type, direction, intensity and a spot's angle) and the environment (ambient and an optional sRGB background) — checked on the client and in the engine before the first write, written as `am3d:scene` into the slide part, with the whole model shape as the undo image |
| `collab/pr13` | `5c150c93` + this slice | a native base above `max_base_inline` travels as a chain of chunks (`base.chunks`, `base-chunk`): the client assembles it, checks the digest and the limits, resumes at the missing chunk after a dropped socket, and the snapshot is applied only when the base is whole; the big-deck soak (322 edits, 120 slides with media, an 11.8 MiB base in 48 chunks) is the §8 gate |
| `collab/pr14` | `baa845ec` + this slice | Save in a live desktop session sends `ClientMsg::Commit` instead of writing the local file: one commit in flight (a repeated Save is the same request), `expected` is the cabinet version the session opened from or the one the last commit was accepted with, the document stays dirty until `commit-result` (dirty is cleared only if no edit landed meanwhile), a `409` keeps the edits and the banner offers Save a copy, viewers and commenters are refused locally without a request |
| `collab/pr15` | `6e42a7ab` + this slice | optimistic preview: an optional `preview` local id on the operation (never stored, required or echoed by the room) and a client that shows one `command`-class operation at submit behind `DECKCRAFT_COLLAB_PREVIEW`; `applied` settles it without a second run, a refusal restores exactly that one edit, a foreign step in between abandons it and asks for the tail, and a preview never enters the undo history |
| `collab/pr15` | `51643c59` (+ `95b5feef`) | an input method's composition is settled before anything leaves the client: `text.compose`/`text.commit` run on the replica, every other command settles first, and the settle sends the body it typed into as one whole value (`text.setBody`, undo image = the body before it); a composition outside slides is refused before the first write with its reason |
| `collab/pr15` | `42544987` | a base above `max_base_inline` is one HTTP request (`base.http`, `GET /v1/sessions/{id}/base/{sha256}` in the media reader's pattern, RFC §4.5.2) beside the unchanged chunk chain: the desktop link fetches it once, checks size and digest, and the host rebuilds the replica and takes the key map without reopening the cabinet's file; 256 MiB ceiling, a token for another file refused, a base the room no longer holds a `404` |
| `collab/pr15` | `b314feef` (+ docs) | an input method's deletion outside a composition is one `text.delete`: the engine resolves the range with the same walk `text.delete` uses (`surrounding_delete`), the normaliser sends it as one `TextDelete` (`NotesDelete` for notes, the cell address kept) with the body before the step as its undo image, and the two ranges a non-empty selection with text on both sides would remove are still refused at the gate before the queue, with their reason |
| `collab/pr11` | `2a91d49e`, `b54eed3d` (+ cell-set rebase), `f562aea4` (+ docs), `b7025f9a` (+ docs), `a48c12de` (+ docs), `b4f13167` (+ docs), `6ddc0c4f` (+ docs), `b31bff47` (+ docs), `e2a5ef41` (+ docs), `c895183a` (+ docs), `05abfe28` (+ docs), `d6c2df6c` (+ docs), `f67b0858` (+ docs), `cfd40da2` (+ docs), `db413e5d` (+ docs), `44489f34` (+ docs), `c406a329` (+ docs) | table cell text (`text.set`/`text.insert`/`text.delete` with a `(row, col)` cell address, rebased past row/column edits), `table.edit` for row/column insert/delete with the whole table as undo image; `animation.add`/`animation.remove` as one shape's whole step list (`slide.animation.set`) with the exact list as the undo image; slide notes text as an addressable body (`notes.set`/`notes.insert`/`notes.delete`, address `(slide, notes body)`, rebased, whole-body undo image, the receiver's selection kept); `format.*` on one table cell (`FormatSet { cell: [row, col] }`, applied to that cell's own body, refused before any write for a cell the table does not have) with the cell's whole body as the undo image (`cell_text`); `format.*` on a character range of a body (`FormatSet { range }`, whole-body offsets rebased like `text.insert`/`text.delete`, unaddressable range refused before any write, the whole body as the undo image); `format.*` over N table cells as **one** operation with one address per cell (`FormatSet { addresses }`, a neighbour never touched, empty/repeated/missing addresses and ranges past the table refused before any write with no version move, each addressed cell's whole body as the undo image in one `cells_text`, `undo-conflict` on a foreign edit of any addressed cell) and a caret in a shape's body as its own address (`caret: offset`, the single-user command decides what it touches — word for run commands, paragraph for paragraph commands — never pre-widened by the replica); `animation.set`/`animation.timing`/`animation.move` as one slide's whole ordered sequence (`slide.animations.set`) with the whole sequence as the undo image; `animation.options`/`animation.clear` and the Animation Painter as the same slide-wide sequence computed with the engine's `options_plan`/`painter_plan` (the painter's clipboard stays a session value, a no-op sends nothing, a one-shot pick is disarmed after sending), with the whole sequence as the undo image; and the document-font slice `fd4e54e8`: `font.create`/`font.set`/`font.remove` on the presentation's embedded-font registry (`FontCreate` names one face by digest, `FontSet` sets the descriptors, `FontRemove` drops the entry), a presentation-level merge step with no slide address, a font face on the media channel (`MediaType::Font` with an sfnt allowlist and its own budget) and the whole `EmbeddedFont` entry (`font_restore`) as the undo image; and the rest-of-the-table-surface slice `c895183a`: `table.style`, `table.cellFill` over an explicit cell list, `table.merge`/`table.split` and `table.columnWidth`/`table.rowHeight` as final-value commands addressed by the table, the whole table as the undo image (the `table.edit` image and `shape.setField{field: table}`, whose box now follows the table), a queued cell operation rebased past a row/column edit and refused `stale` when a merge covered its cell, and the merged rectangle recorded so a covered address is never written; and the table-options slice: `table.options` as one final set of the six independent style booleans (an unknown or non-boolean flag refused before any write, the whole table as the undo image) and the Table Layout ▸ Cell Size fields that run `table.columnWidth`/`table.rowHeight`; and the SmartArt slice `d6c2df6c`: `smartArt.setNodes`/`addNode`/`removeNode`/`promote`/`demote`/`layout`/`style`/`colors` as **one whole `Diagram` value** (`smartArt.setDiagram`: layout, outline with text/levels/assistants/titles/pictures, style and colours), computed on the sender with the same engine `smartArt::plan` the single-user command runs, checked in the engine before the first write (layout, style, colours, node count and JSON size), with the **whole group** as the undo image (`shape_field{field: smartArt}`: the diagram and the generated children with their ids), because an outline edit regenerates the children with fresh ids and replaces an imported file's drawing; and the chart slice `f67b0858`: `chart.type`, `chart.data`, `chart.options` and `insert.chart` as **one whole `Chart` value** (`chart.setWhole` and the `chart_insert` payload), computed on the sender with the same engine `chart::plan` the single-user command runs, checked in the engine before the first write (unknown type, category/series/point counts, text bounds and the JSON size), refused for a shape that is not a chart, with the **whole chart shape** as the undo image (`shape_field{field: chart}`); and the SmartArt insertion slice `cfd40da2`: `insert.smartArt` as **one whole `Diagram` value** (`smartArtInsert`), computed on the sender with the same engine `smartart::insert_plan` the single-user command runs, checked in the engine before the first write (layout, style, colours, node count, text and JSON size), with the **whole inserted shape** as the undo image (an insertion is one structural step, removed by `shape.delete` and put back whole by `shape.restore`), and `smartArt.nodes` answered by the server as a **read-only query** (no version, no journal, a viewer may ask), and the SmartArt conversion slice `db413e5d`: `smartArt.fromText`/`toText`/`toShapes` as **one structural step each** — the sender computes the whole resulting diagram or text body with the same engine plan the single-user command runs, the engine checks the value and box before the first write, the source shape is replaced in place, and the undo image is the whole source shape (`shape_restore`, or a same-id remove-then-insert for `toShapes`); the SmartArt node-picture slice `44489f34`: `smartArt.picture` as one final value (the top-level graphic, one top-level node, the media channel's digest and the picture's final place), the engine checks the target, the node, the picture allowlist and budget, the place's finite positive area and that the layout does not change before the first write, and the whole group is the undo image; and the 3-D model camera slice `c406a329`: `model3d.camera` as one final view (`position`/`target`/`up` in EMU, `fov` in degrees, or `null`), relative edits refused on the client, on the wire and in the engine before the first write, the view written as `am3d:camera`, and the whole model shape as the undo image |

### Rebase onto origin/main (2026-11-09)

Both branches were rebased onto `origin/main` at `0623b430` (23 commits ahead of the old
merge-base `159ed07e`) so that [PR #1975](https://github.com/storytold/deckcraft/pull/1975) is no
longer `CONFLICTING`: `collab/pr10` (55 commits) → `7459d798`, then `collab/pr11` (13 commits,
`--onto`) → `da3199bf`. `git merge-tree --write-tree origin/main collab/pr10` and the same for
`collab/pr11` both exit 0 with an empty conflict list. The pre-rebase branches are kept locally as
`backup/pr10-pre-rebase` and `backup/pr11-pre-rebase`. The PR head is `collab/pr10`, so that
branch carries a short note of the rebase as well; this file on `collab/pr11` is the
authoritative one (it also has the later slices).

Every hash in this file that predates the rebase refers to the pre-rebase series; the named
slices map as follows:

| Pre-rebase | Rebased | Subject |
|---|---|---|
| `98d9e9ce` | `9d824224` | undo group and ungroup from whole-subtree images |
| `e007e965` | `a7da3d8f` | base transfer after compaction and a three-client soak gate |
| `927e773c` | `85babbaa` | `slide.transition.set` as a final-value command slice |
| `922a4bf4` | `7459d798` | docs: transition slice (old `collab/pr10` head) |
| `2a91d49e` | `f7ae5a94` | text in table cells and row/column edits |
| `b54eed3d` | `649f6e4a` | rebase a whole-cell `text.set` past a table edit |
| `f562aea4` | `e7e7b601` | shape animation steps as an exact-value command slice |
| `b7025f9a` | `5edd85b3` | slide notes as an addressable body slice |
| `a48c12de` | `0178cc6a` | format a table cell's own body, exact cell-body undo |
| `b4f13167` | `a90a3e31` | format a run range of a text body, exact-body undo |
| `d6ea4051` | `538649c9` | docs: run-range format slice |
| `6ddc0c4f` | `b2e976bf` | format several table cells and a caret in one step |
| `de498801` | `da3199bf` | docs: N-cell and caret format slice (new `collab/pr11` head) |

Main moved 29 files in those 23 commits (a `craft-web-api` page command API, 3-D body/group XML,
trendline fits, sentence-case and paragraph-alignment fixes, ribbon accessibility and the updater
UI, plus a craft-libs pin bump). Four files conflicted; each was resolved as a union, not a
choice:

- `crates/engine/src/cmd/shape.rs` — main's `opt_usize_param` (used by
  `cmd/shape/appearance.rs`) and the collab-side `current_shapes` both stay; rustfmt keeps one
  wrapped list.
- `apps/deckcraft-web/Cargo.toml` — main's `craft-web-api` at
  `storytold/craft-libs@fd5e2fe` and the collab `deckcraft-collab-web`/`craft-web-host` stay; the
  host crate remains pinned to `copoka/craft-libs@7666e2b` (the session messages are not in
  `storytold/craft-libs@fd5e2fe`, ADR-0016 §3), and `Cargo.lock` names that source.
- `apps/deckcraft-web/src/web.rs` — `crate::api::install(...)` is kept and the `WebShell`
  literal keeps the collaboration fields (`ctx`, `host_events`, `host_state`,
  `last_host_state`, `last_presence_ms`).
- `Cargo.lock` — the only remaining difference from main's own lock diff is the pinned host
  source; offsets differ because the collab crates add packages.

The code patch main brought in is `origin/main`'s own `159ed07e..origin/main` patch: compared with
`git diff backup/pr11-pre-rebase collab/pr11`, the only differences are `Cargo.lock`, the host pin
in `apps/deckcraft-web/Cargo.toml`, this file, and the one union import line in `cmd/shape.rs`
(`current_shapes` stays next to `opt_usize_param`). Hunk offsets differ because the collab crates
add lines above them. So the rebase added nothing to the collaboration slices and dropped nothing.

The Commit column names the slice commits; each branch also carries documentation commits. Heads
after the rebase: `collab/pr10` = `7459d798` (+ its note), `collab/pr11` = `da3199bf` (+ docs). In the pre-rebase series:
`collab/pr9` = `47ec08fe`, `collab/pr10` = `7fe75711`, `0a57b61e`, `58eee059`, `69c2ff22`,
`41fe1ad9`, `4e666b95`, `f6281736` (journal replay of history operations), `3ec81d70` (shape
insert/delete undo), `31842050` (cabinet commit publication), `f28ee048` (docs), `98d9e9ce`
(group/ungroup undo), `31d353fe` (align/distribute as one final-bounds step), `5b078c7d`
(the §8 gates and the non-active-slide fix), `556c00d9` (three-session gate and the reconnect
catch-up fix), `e007e965` (base transfer after compaction and the three-client soak), `927e773c`
(`slide.transition.set` as a final-value slice with its undo), plus the docs commits on top,
`collab/pr11` = `2a91d49e` (table cell text and row/column edits) plus its docs commit, the
cell-set rebase `b54eed3d`, the animation-step slice `f562aea4` with its docs commit on top, the
notes-text slice `b7025f9a` with its docs commit on top, the table-cell format slice
`a48c12de` with its docs commit on top, the run-range format slice `b4f13167` with its docs
commit on top, and the N-cell/caret format slice `6ddc0c4f` with its docs commit on top. The working tree is clean: the chair's
uncommitted finding of 2026-11-08 is `7fe75711`; the second slice added `0a57b61e` (media kinds,
TLS, command slices, tests, docs), `69c2ff22` hardened the format command, `41fe1ad9` added undo
of fill/line/format steps, `4e666b95` added undo of a group's fill/line and of slide
insert/delete, `3ec81d70` added undo of a shape insert and a shape delete (whole-shape image with
z-order, animations and triggers), `31842050` sends a commit to the cabinet, which creates the new
version and keeps the original, `98d9e9ce` makes `arrange.group`/`arrange.ungroup` undoable
with whole-subtree images on both sides, and `31d353fe` carries `arrange.align`/`arrange.distribute`
as one final-bounds operation with one undo step. `927e773c` makes `transition.set`
collaborative as one whole final value with an exact-value undo step, and `2a91d49e` (on
`collab/pr11`) carries a table cell address through text operations and adds `table.edit` for
row/column insert/delete.

The web build now has its side of the cabinet session (`66d4658c`, `crates/collab-web` +
`apps/deckcraft-web`): the `deckcraft:session` message opens the session, the page's `WebSocket`
carries the protocol, `session-state`/`presence` go back to the cabinet, Save asks for a commit
and `deckcraft:committed` records the stored version. Nothing there talks to the network without a
session message.

The fork branch `copoka/deckcraft:collab/pr10` is published at `922a4bf4`, the pre-rebase head,
and `collab/pr11` at `d6ea4051`; PR #1975 is open from the fork on `collab/pr10` (head
`922a4bf4`, `CONFLICTING` against `main`). The local branches were rebased onto `origin/main`
(`0623b430`) and now carry rewritten history (`collab/pr10` = `7459d798` plus its rebase note,
`collab/pr11` = `fd4e54e8` plus documentation commits, 75 commits ahead of main), so publishing
them is a **force-push** of
the two fork branches and not a fast-forward. A `git push --dry-run` from this workspace is
refused by the environment policy, so the publish is a manual step (the chair does it): `git push
--force-with-lease fork collab/pr10 collab/pr11`. Nothing was pushed to `storytold/deckcraft`;
the PR comes from the fork and a human decision still governs merging it. The PR body is prepared
in [`pr10.md`](pr10.md): title `feat(collab): realtime collaborative editing`, one description, a
review order over the slice stack and the `PR-CHECKLIST.md` mapping. Do not push it without the
human decision.

## What is implemented

### Protocol and adapter (`crates/collab-proto`, `crates/collab`)

- Envelope `hello/welcome/op/applied/rejected/presence/resync/snapshot/commit-result/error/ping`
  with the DeckCraft payloads; `undo`/`redo` are client requests, the server owns the inverse.
- Session keys (`slideKey`, `shapeKey`) derived identically on the server and the client from
  `fileId` plus the same document walk; tombstones refuse late operations.
- Authoritative ordering: one server `version`, `opId` dedup with a 10 000-entry window,
  structural operations exclusive at the head, `applied` broadcast to everyone **with the
  confirmed command** so every replica runs the same command in the same order. Dedup is per
  **operation**, not per command: a command that normalises into N operations carries N ids (the
  first keeps the id the preview and the sender's settle are keyed on, the rest are `<id>#<n>`),
  and a structural operation is only sent once everything before it has been answered.
- Delta of changed fields (no values, no derived state): group frames, connector routes and
  autofit are recomputed on each replica.
- Base transfer: when compaction removed the journal below a client's version, `resync`
  (`snapshot`) carries the native `.deckcraft` base, the session key map and the steps above the
  base. Keys are issued in walk order and cannot be re-derived after structural edits, so the map
  (`slides`, `shapes`, `dead`, `next`) travels with the base and the replica continues the
  server's numbering. Above `max_base_inline` (8 MiB, `COLLAB_MAX_BASE_INLINE`) the base travels
  as a chain of chunks (`base.chunks`: digest, length, chunk size, depth) that the client pulls one
  at a time with `base-chunk`, so a deck whose native base is tens of megabytes still rebuilds a
  replica without a frame to hold it and without reopening the file from the cabinet; a chain
  interrupted mid-way resumes at the chunk the client is missing, a base compacted under it is a
  retryable `stale`, and a base beyond `max_base_chunks` is still fetched in one HTTP request
  (RFC §4.5.2); only a base above the HTTP ceiling is named without bytes
  (§A base above the inline limit is one HTTP request).
- Per-actor undo: server-side before/after images, `undo-conflict` when someone else changed the
  object, undo is an ordinary new operation. The images cover `xfrm`, text, fill, outline, a
  slide's whole transition, one shape's whole animation step list, a slide's whole ordered
  animation sequence, the whole `TextBody` of a
  `format.*` step (one body or each of N table cells) and a slide's whole notes body; these travel
  as an exact model value
  (`shape_field`/`shape_animations`/`slide_animations`/`notes_set`/`cells_text`) because the
  user-facing fill/line/format/animation/notes commands cannot express the whole model, and the
  adapter accepts that payload only as an undo/redo of the actor's own latest step.
  A group's fill/line step records one image per changed member and restores them atomically with
  `shape_fields` (engine `shape.setFields`, values validated before the first write). An
  `arrange.align`/`distribute` step records one `xfrm` image per moved shape and restores them in
  one `shapes_set_bounds` (engine `shape.setBoundsMany`, ids and values validated before the first
  write), so a selection-wide undo is one operation, not one per object. A
  `slide.delete`, `slide.new` or `slide.duplicate` records the whole slide (model JSON, position,
  section and custom-show membership); undo restores it with `slide_restore`, redo deletes it.
  A shape insert (`shape.insert`, `insert.picture`, `insert.video`/`insert.audio`) and
  `edit.delete` record the **whole shape**: model JSON with its id, the group that held it, its
  position in the z-order and the animations that named it (including foreign effects it
  triggered). Undo `edit.delete` is one `shape_restore` (`items` of whole shapes, a group image
  carrying its children), redo is `shape_delete`; undo of an insert is `shape_delete` and its redo
  is `shape_restore`. The engine command `shape.restore` validates and deserialises every image
  before the first insert, refuses a duplicate id, and clamps the recorded sibling position to the
  list it finds, so neighbours that came or went do not block an undo.
  Every history move is checked against the recorded image and address (`check_restore`), so a
  client cannot send an undo envelope for another object or with another value.
- Notes text is its own address: `notes.set` carries a slide's whole notes body as the model
  `TextBody` (so runs, levels and bullets survive), `notes.insert`/`notes.delete` carry character
  offsets into it, and `target.slide` is the whole address — notes belong to a slide, not to a
  shape, and a shape key in the target is refused. The normaliser turns `slide.notes` into the
  complete body and typing in the notes pane (`text.insert`/`text.delete`/`edit.delete` with
  `notes: true`) into the offset operations. The journal rebase is keyed by the slide address as
  well, so another slide's notes never shift it; a whole-body replacement after the base sends a
  queued insert or delete to `stale` instead of writing in the wrong place. The adapter resolves
  the slide before the first write and restores the receiving user's selection, moving a caret
  that was in the same notes body by the characters inserted or removed before it.
- Presence: ephemeral, latest-state-wins, 30 s TTL, FNV colours, never enters a version.

### Client (`CollabDoc`, `CollabUi`, `WsTransport`)

- `CollabDoc` normalises commands to absolute/explicit operations and applies the confirmed
  commands; keys are derived, not transferred.
- Optimistic preview (`DECKCRAFT_COLLAB_PREVIEW=1`, off by default; RFC §3). One `command`-class
  operation whose payload the room will not rewrite (move/bounds/align, fill/line, a whole-body
  `format.*`, transition, animations, a whole chart, table style/options/sizing, a whole notes
  body, camera/scene, font set/remove) is applied at submit — an `Arc` copy-on-write before-image,
  through `apply_confirmed`, so no undo step is recorded — marked `preview: <opId>` on the wire and
  drawn in the same frame. Text offsets, cell addresses/ranges/carets, structural steps,
  `SmartArtSet` and `FontCreate` are never previewed: the room owns the offsets, addresses, keys
  and generated ids. While a preview is outstanding every later local command and undo/redo wait
  (bounded at `MAX_DEFERRED = 64`) and are normalised against confirmed state, so no payload ever
  contains an edit the room lacks. `applied` with the same payload and target settles it (the
  command is not run a second time); a different form restores the before-image and applies the
  authoritative command; a refusal restores exactly that one edit and leaves accepted steps alone;
  a step from another participant in between abandons the preview, reports the queued edits and
  asks for the tail from the version the replica holds (`ClientSession::rewind_replica`). The room
  strips `preview` in `DocSession::apply` before dedup, journal, undo and commit, and answers with
  no trace of it (proto and socket tests pin that).
- Unanswered operations go back to the front of the queue on a disconnect and are resent with
  the same `opId`; the server dedups them.
- Desktop `WsTransport`: one background thread, two channels, 50 ms read timeout, rustls for
  `wss://`, reconnects with the same handshake.
- UI behind a flag (`DECKCRAFT_COLLAB`/`?collab=1`): status indicator, participants pane, remote
  selection frames and carets, commit warnings banner, viewer lock.
- Browser link (`crates/collab-web`, `Transport` over the page's `WebSocket`): the same
  `CollabDoc`, dialled from `deckcraft:session` (`sessionId`, `wsUrl`, token, role). Reconnect
  backoff is the desktop link's; unconfirmed operations keep their ids and are resent after the
  next welcome. Media: the digests a frame names are `fetch`ed first and the frame waits for them,
  in order, for at most 10 s; uploads are `fetch`ed with the same token. The token never reaches
  a URL or a log (`HostSession` prints `<redacted>`), and the shell posts `session-state` /
  `presence` to the cabinet, asks for `commit` on Save in a live session, and records
  `committed`'s version and etag. `Transport` is no longer `Send` — a browser link holds JS
  closures and `Rc`, which never cross a thread on wasm32.
- `Session::set_collab_queue`: with a room attached, every mutating path (ribbon, keys, pointer,
  IME, control channel) queues the command instead of executing it; `CollabUi::pump` sends it.
  The input method is the one exception: `text.compose`/`text.commit` run on the replica and the
  settle queues the whole body once (§A composition is settled before anything leaves the client).
  `apply_remote` still applies the server's command directly. A pointer gesture is a command too:
  a viewer's press starts no writing drag at all, and an editor's finished shape drag submits the
  boxes it left as one `shape.setBoundsMany` (final values, normalised to one `ShapesSetBounds`),
  so a drag is one version and one undo step and never a local-only move. Alt-drag duplication
  makes new objects and is not on the wire yet, so a shared session treats that gesture as a move;
  a connector dragged away from shapes that stay put keeps its glue there too, because the
  operation carries boxes and not glue.
- Text: `text.insert`/`text.delete` carry character offsets; the server rebases them against the
  journal tail since `baseVersion` (inserts shift right, deletes pull left and clamp), so two
  people typing in one paragraph keep both texts. Confirmed commands carry the rebased offset.
  `text.set`, `text.insert` and `text.delete` also carry an optional `(row, col)` table cell: the
  offset is inside that cell's own body, the local queue reads it from the caret or the cell
  selection, and the adapter selects the cell before running the command. A `table.edit`
  (row/column insert/delete above/left of a cell) moves the queued address during rebase; a
  deletion of the cell's line refuses the queued operation. Two cells of one table never shift
  each other's offsets.
- `shape.insert`: preset, final geometry, optional text; ids are deterministic on all replicas.
- `insert.picture`: the operation carries `sha256`, `name` and an optional final rect, never bytes.
  The server stores one object per digest (`PUT /v1/sessions/{id}/media/{sha256}`), every replica
  fetches it (`GET /media/{sha256}`), and a journal replay loads the object before the operation.
- `insert.video` / `insert.audio`: the same media channel with their own allowlist; the payload adds
  `kind`. The media cache keeps separate budgets for pictures and for audio/video, so a clip cannot
  evict pictures. The desktop link derives the HTTP scheme from the WebSocket link: `ws://` is a
  plain socket, `wss://` a `rustls` connection with the platform root store.
- `insert.model3d`: a 3-D model on the same channel — one binary glTF (`.glb`) by signature
  (`glTF` magic, version 2), carried as `media_insert { kind: model, sha256, name, rect? }` and run
  by the new engine command `insert.model3d`, which stores the part as `model/gltf-binary` and adds
  a `Model3d { media, poster }` shape (`p:contentPart` with a `model3d` relationship). Models have
  their **own** budget in the cache (32 MiB per object, 16 objects, 128 MiB per session), so a big
  model never evicts pictures, audio or fonts. The renderer draws the `mc:Fallback` poster when the
  file has one and a stand-in cube otherwise, so the frame is never an empty box; the commit
  warning says the model is drawn as a poster or a placeholder, not as a rendered model. Undo of
  the insertion is the ordinary structural shape step.
- `shape.fill`: a final fill (solid colour, `none`, `background`), one operation per shape.
- `shape.line`: final outline values (colour, width, dash, cap, join, compound, heads) or `none`.
- `transition.set`: one slide's **whole** transition as a final value (`slide_transition`: effect,
  option, duration, advance timings, embedded sound, kept XML; `None` = no transition). The client
  computes it with the engine's `transition.set_plan`, the same function the single-user command
  runs, so the value keeps the advance timings and sound the user did not name; no replica merges
  its own state. The adapter runs `slide.transition.set`, which deserialises and validates the
  model value (an effect the model does not know is accepted only with its kept XML, the sound must
  be in the document) before the first write.
- `animation.add`/`animation.remove`: one shape's **whole** ordered step list as a final value
  (`shape_animations`: every step is the model `Animation` as JSON), addressed by slide key and
  shape key. The client builds a new step with the engine's `animation.add_plan` (the same `build`
  the single-user command runs), appends it to the steps the shape already has, and removes one by
  index or all of them for `animation.remove`; a multi-shape command is refused. The adapter
  resolves the address, forces the shape id on every value and runs `slide.animation.set`, which
  bounds the list (256 steps, 64 KiB), accepts only the model's own fields, requires a known
  effect, and replaces the shape's steps where the first one stood before the first write. Two
  steps in a row are two absolute values, so the later accepted list wins; a shape that cannot be
  addressed (unknown key, another slide, no shape) is refused before anything is written.
- `animation.set`/`animation.timing`/`animation.move`: one slide's **whole ordered animation
  sequence** as a final value (`slide_animations`: every step is the model `Animation` as JSON and
  carries its own shape id), addressed by slide key. The client computes the resulting sequence
  with the engine's own plans — `set_plan`, `timing_plan`, `move_plan`, the same functions the
  single-user commands run — so a style/`none` replacement keeps each shape's place, timing
  changes exactly the named steps, and a move may cross another shape's step. The adapter runs
  `slide.animations.set`, which bounds the list (4096 steps, 192 KiB), accepts only the model's
  own fields, requires a known effect and a shape still on that slide, and replaces the whole list
  before the first write; a list that changes nothing sends no operation. This is a per-slide
  value, so the whole-sequence payload, not the per-shape `shape_animations`, is what keeps
  interleaved steps exact.
- `animation.options`/`animation.clear`/`animation.painterApply`: the same slide-wide
  `slide_animations` value. `options` changes exactly the named steps' option codes (resolved in
  each effect's own terms by the engine's `options_plan`, so no replica guesses a direction),
  motion path and text build; `clear` sends an empty sequence for the slide it addresses (`index`
  is a slide, not a step); the painter's clipboard (`Session.anim_painter`) is a session value, so
  `painterApply` sends the resulting sequence computed with the engine's `painter_plan` — each
  target's own steps replaced by the picked ones, a paragraph the target does not have becoming
  the whole shape — and no replica needs the clipboard. A command that changes nothing sends no
  operation, a missing clipboard or an unaddressable target is refused before anything is sent,
  and the UI disarms a one-shot pick after it is sent, as the single-user command does.
- `font.create`/`font.set`/`font.remove`: the presentation's embedded font registry
  (`p:embeddedFontLst`). `font.create` embeds one face from a font file: the bytes go to the
  media channel and the operation carries their `sha256`, the typeface and the slot
  (`regular`/`bold`/`italic`/`boldItalic`), so an entry the document lacks is created and an
  existing one only gains or replaces that face. A font is presentation-level — it names no slide
  or shape, so every replica applies it whatever it is looking at — and it changes no address or
  key, so it is a merge-class operation. `font.set` sets the entry's descriptors (`panose`,
  `pitchFamily`, `charset`) as final values; `font.remove` drops the whole entry and leaves the
  face objects in the media list for an undo image. The engine validates the file (sfnt header and
  table directory, 16 MiB ceiling), stores it as `application/x-fontdata`, installs the face in
  the session's font database so the text draws immediately, treats a typeface name
  case-insensitively, and refuses an unknown entry or a non-font file before the first write.
- Undo of a font step: the whole entry before and after (the model `EmbeddedFont` as JSON — its
  descriptors and the media ids of its four face slots — or `null` when the document had none),
  restored with `font_restore` by the engine's `font.restore`. A later change to the same entry by
  anyone else is `undo-conflict`; a change to another typeface is not. The restore names no slide:
  `Restore.slide` became optional for this one presentation-level value.
- `format.*`: 17 allowlisted commands (`bold`, `italic`, `underline`, `strikethrough`,
  `superscript`, `subscript`, `font`, `size`, `color`, `highlight`, `alignLeft/Center/Right`,
  `justify`, `caps`, `spacing`, `clear`) over one explicit shape's whole text body, over the
  whole bodies of **N table cells of one table** addressed by
  `FormatSet { addresses: [{ cell: [row, col] }] }`, over a character range inside one of those
  bodies (`addresses: [{ cell?, range: {from, to} }]`) or at a caret (`addresses: [{ cell?, caret }]`).
  A selection of several cells is one operation with one address per cell, never one address
  widened to the table; a neighbouring cell that is not listed is never touched (a table without
  an address, an empty address list, a cell the table does not have, a cell addressed twice, a
  shape that is not a table, more than one body on a shape, an empty range and a range or caret
  past the end of the body are refused before any write). Toggles need an explicit `on`; `args`
  cannot carry `id`/`ids`/`index`/`cell`/`row`/`col`/`range`/`caret`. The range is a whole-body
  character range — paragraph breaks counted as one — the same coordinates
  `text.insert`/`text.delete` use, because run indices differ between replicas once someone edits
  the text. A caret in a shape's own body is its own address (`caret: offset`) rather than a
  refusal: the server places the same collapsed selection on the same authoritative body and runs
  the single-user command, so a run command widens it to the word it is in and a paragraph
  command takes the paragraph — the replica never widens it up front. The server rebases every
  address against the journal tail (a row/column edit moves each cell address); a range whose
  characters were removed is refused `invalid` before any write. The command runs on the engine
  selection placed on each addressed span, and the receiving user's selection is put back
  afterwards.
- `table.insertRowAbove`/`insertRowBelow`/`deleteRow` and the column variants: one structural
  `table.edit` payload (`axis`, `at`, `insert`) addressed by the table's shape key. The adapter
  runs the engine's own row/column command with the table id, so every replica makes the same
  change; the undo image is the whole table (`shape.setField` `field: table`), because a
  structural edit changes both the cell indices and their values.
- The rest of the table surface: `table.style` (a final style name), `table.cellFill` (one
  final fill — a solid colour, `none` or `inherit` — over an **explicit** cell list, which the
  normaliser expands from the user's selection and bounds at 4096 cells), `table.merge`/
  `table.split` (final corners or the merge anchor; cells keep their indices, so no address
  moves) and `table.columnWidth`/`table.rowHeight` (final numbers; `keepTotal` takes the
  difference from the next column, as dragging a border does). Each is one `Command` operation
  addressed by the table shape, so a replica's own selection decides nothing, and the adapter
  checks the table, every cell, column and row before the first write. A queued fill's cell
  addresses rebase past a row/column edit like a cell text offset; a merge that covered a queued
  operation's cell refuses it `stale` instead of writing where nobody can see it. The undo image
  is the whole table (`shape_field` `field: table`, the image a row/column edit records), and
  the engine's `shape.setField` now sets the shape's box to the restored table's size, so an
  undo of a width or height step moves the box back too.
- `table.options`: the six table style options (`headerRow`, `totalRow`, `bandedRows`,
  `firstColumn`, `lastColumn`, `bandedColumns`) as one operation carrying **all six final
  values** (`table_options`). They are independent booleans, so a partial set would let whichever
  replica merged first decide an option the sender did not name; the sender applies the flags the
  user toggled to the table it sees and sends the whole set. An unknown or misspelt flag, a value
  that is not a boolean and a call that names no option are refused by the normaliser before
  anything is queued, and the engine command refuses the same before the first write. The undo
  image is the whole table (`shape_field` `field: table`), like the rest of the table surface.
- Table Layout ▸ Cell Size: the ribbon's Column Width and Row Height fields show the selected
  column's width and row's height (the caret's cell, the start of the cell selection, or the
  table's first cell) and run `table.columnWidth`/`table.rowHeight` with the final number, the
  same commands a border drag runs, so a live session queues them like any other edit.
- Charts: `chart.type`, `chart.data` and `chart.options` travel as **one whole `Chart` value**
  (`chart.setWhole`: the model chart as JSON — type, categories, series, options and any kept
  part). The sender applies the command to the chart it sees with the engine's `chart::plan` (the
  same plan the single-user command runs), so the value sent is the value that command would
  leave; the adapter requires a chart shape and bounds the value before the first write, and the
  engine command deserialises the model type and checks the type, the category/series/point
  counts, the text lengths and the JSON size before it replaces anything. `insert.chart` is
  collaborative as `chart_insert`, carrying the whole chart its command would create plus the
  optional box, so every replica creates the same chart; its undo is the ordinary shape-delete
  step. The undo image of an edit is the **whole chart shape** (`shape_field` `field: chart`, the
  model `Shape` as JSON), because the chart commands express only part of the model and a kept
  part cannot be replayed from them; a foreign edit of the chart after the step makes the undo
  `undo-conflict`.
- `slide.duplicate`: structural, addressed by the source slide key; the copy is inserted after it.
- `arrange.align`/`arrange.distribute`: one `shapes_set_bounds` operation with the final box of
  every moved shape. The client takes the values from the engine's own plan (`align_plan`,
  `distribute_plan`), so no replica recomputes the slide edge, the selection frame or the gaps;
  the adapter runs `shape.setBoundsMany`, which checks every id and duplicate before the first
  write and moves the whole selection in one `edit`. A shape whose box does not change is left out;
  an align that moves nothing sends nothing.
- Undo/redo: move, bounds, align/distribute, text (shape text and table cells), fill, outline,
  `format.*` (a shape's body, one cell's body, N cells of one table, or a character range/caret of
  a body), transition, animation (a shape's step list for add/remove, a slide's whole sequence
  for set/timing/move/options/clear/painter), a document font entry (create/set/remove) and the
  whole table for style/options/fill/merge/split/sizing, and a chart edit (the whole chart
  shape). A restore
  carries the exact model value (`shape_field`) and runs through the `shape.setField` engine
  command, which bounds the JSON and deserialises it into the model type. A cell text step records
  the cell's whole body text with its address and restores it with `text.set{cell}`, so the
  shape's own (empty) body is not touched. A format step on a cell records that cell's whole body
  and restores it with `cell_text` / `shape.setField { field: cellText, cell }`, so a neighbouring
  cell is never written; a step over N cells records each cell's whole body and restores exactly
  them in one `cells_text` (`shape.setCellBodies`, one `shape.setField { field: cellText }` per
  cell, all validated before the first write), so a foreign edit of **any** addressed cell makes
  the undo `undo-conflict`. A `table.edit` step records the whole table (`shape_field` `field: table`) and
  restores rows, columns and cell text in one write. A group's fill/outline step records an image
  per changed member and restores all of them in one `shape_fields` operation (`shape.setFields`
  validates every value before the first write). `slide.delete`, `slide.new` and `slide.duplicate`
  record the whole slide and undo through `slide_restore` / `slide_delete`. `shape.insert`,
  `insert.picture`, `insert.video`/`insert.audio` and `edit.delete` record the whole shape and undo
  through `shape_restore` / `shape_delete` (z-order, animations and triggers included, group images
  carry their children). `arrange.group` and `arrange.ungroup` record a whole-subtree image on both
  sides (the group with its children, or every former member at the top level) and move between
  them in one `shape_restore{items, remove}`: the engine removes the other side's shapes first
  without dropping animations, then inserts the recorded images, all validated before the first
  write. A `transition.set` step records the slide's whole transition (effect, option, duration,
  advance timings, sound and kept XML) and restores it through the same `slide_transition` payload,
  applied by `slide.transition.set`. An `animation.add`/`animation.remove` step records the
  shape's whole step list before and after (model animations in order) and restores it through the
  same `shape_animations` payload, applied by `slide.animation.set`; a later list from another
  participant refuses the undo with `undo-conflict`, while a change to another shape's steps does
  not. An `animation.set`/`animation.timing`/`animation.move`/`animation.options`/
  `animation.clear`/`animation.painterApply` step records the slide's whole
  ordered sequence before and after and restores it through the same `slide_animations` payload,
  applied by `slide.animations.set`; any animation change on that slide after the step (including
  another shape's) refuses the undo with `undo-conflict`, because the whole order is the value.

### Server (`apps/deckcraft-collab-server`)

- HTTP: `POST/GET/DELETE /v1/sessions`, `GET /healthz`, `/readyz`, `/metrics`.
- WS: first frame is `hello` with the JWT; signature/`exp`/`aud`/app/file/session checks; actor
  and role from the token. Viewer `op` is refused before the engine. Viewer `commit` is refused too.
- Limits: message size, operations/second, participants.
- Commit: `.pptx` from the authoritative document, `sha256`, `warnings[]`, `409` on a stale
  `expectedVersion`. The server POSTs it to the cabinet (`COLLAB_CABINET_URL`, service token):
  `{fileId, name?, bytes(base64), sha256, version, expectedVersion?}` → `{version, etag}`; the
  cabinet creates a new file version and keeps the previous one. A cabinet `409` (the file changed
  outside the session) is the caller's `409`, and no version is created. The accepted `etag` is
  stored in the session and in `meta.json` (a restart keeps comparing against it). Without a
  callback, `POST /v1/sessions/{id}/commit` returns the bytes inline (development mode); a client
  `commit` is answered only after the cabinet took the file.
- Media: `PUT /v1/sessions/{id}/media/{sha256}` (session token, writer only, signature allowlist
  for pictures, fonts, audio/video and 3-D models, 8 MiB per picture, 16 MiB per font face, 32 MiB
  per clip or model), `GET /media/{sha256}` (session or service token). The HTTP reader accepts a
  body up to the widest kind (`MAX_REQUEST_BODY`, 32 MiB), so an upload the endpoint accepts is
  not refused a layer earlier. Content-addressed under
  `<store>/media/<sha256>`; 512 pictures / 64 MiB, 32 fonts / 64 MiB and 32 clips / 192 MiB per
  session, so a font cannot evict a picture; upload and byte counters in metrics.
- CORS: `DECKCRAFT_CORS_ORIGINS` is the allowlist (comma separated, compared case-insensitively
  and without a trailing slash). While it is set, every HTTP answer carries `Vary: Origin` and an
  admitted origin also gets `Access-Control-Allow-Origin` — the origin itself, never `*`, so the
  browser may send `Authorization`; a foreign origin gets no `Access-Control-*` header. `OPTIONS`
  with an `Origin` is a preflight: `204` with `GET, POST, PUT, OPTIONS` and
  `Authorization, Content-Type` for an admitted origin (`PUT` because the browser uploads media
  with it), `405` for a method outside the list. An empty or absent variable keeps the pre-CORS
  behaviour: no `Access-Control-*`, no `Vary`, and `OPTIONS` falls through to the router.
- Persistence: native base + append-only journal, snapshot every N ops, replay on restart
  through the same `apply` path (loading every media-bearing operation's object first). The append
  follows the in-memory apply; a failed append is logged and the room keeps serving, so a restart
  can lose that acknowledged tail (see `production.md`). Without `COLLAB_STORE_DIR` the server is
  memory-only.
- `COLLAB_SNAPSHOT_EVERY` now also drives a memory-only room (it used to reach the store only),
  so compaction and a base transfer can be forced in a test or a deployment. `resync` takes a
  snapshot on demand when the stored base does not cover the client; `base_transfers_total`,
  `base_chunks_total` and `base_bytes_total` (inline bytes only) count what went out, and
  `COLLAB_MAX_BASE_CHUNK`/`COLLAB_MAX_BASE_CHUNKS` bound a chained base.
- A hub is keyed per connection (`actor#n`) with the actor kept on the peer: two tabs of one
  person get their own outbox, and closing one leaves the other joined. `DELETE` marks the hub
  closed before it leaves the map, so its sockets stop instead of appending to a room that may be
  re-created on the same store directory.
- Logs/metrics contain ids, versions and counts; no slide content, text, media or tokens.

## 8. Gates (TASK.md §8)

All five gates are tests, not scripts: `apps/deckcraft-collab-server/tests/gates.rs`. Run them with

```bash
cargo test -p deckcraft-collab-server --test gates -- --nocapture --test-threads=1
```

- **Large deck with media.** 120 slides, each with a decoded picture (four content-addressed media
  objects, one per slide reused) and an autoshape, opened as an authoritative room; two editor
  sessions submit a `shape.move` for every slide. On the build host (`agents`, 20 cores, Ubuntu
  24.04, rust 1.99.1, test profile: `opt-level = 1`, dependencies at `opt-level = 2`) the
  authoritative apply took 0.1 ms p50 / 0.1 ms p95 / 0.2 ms max over 120 steps, the room opened in
  4 ms, the commit built a 154 KiB `.pptx` in 15 ms, and an unchanged commit came from the cache
  in 0.3 ms (the same remote-built test binary, run where LibreOffice exists, measured 0.2/0.2/0.7
  ms and a 63 ms commit). Both clients settle at version 120 and every picture moved. The
  assertions use generous ceilings (2 s per apply, 60 s per commit), so the test catches a
  pathological regression rather than machine noise; the numbers above are the evidence.
- **LibreOffice open/repair.** A 120-slide deck with media goes through the real commit endpoint
  (`POST /v1/sessions/gate/commit`, bytes inline, no cabinet), then LibreOffice 26.2.6.3
  (`soffice --headless --convert-to`) opens the result, renders 121 pages (the 120 slides plus the
  slide inserted in the session, counted with `pdfinfo`) in 4.2 s, and a `pptx → pptx` round trip
  takes 5.3 s and reimports as 121 slides with 120 pictures. A non-zero exit, an `Error:` line, a
  missing or empty output, or a page-count mismatch fails the test. The build host has no
  `soffice` or `pdfinfo`, so there the test prints a skip line and passes; the gate was run with
  the remote-built binary on the workstation, which has LibreOffice 26.2.6.3.
- **Three sessions: conflict and reconnect.** Two editor sockets and one viewer socket join one
  room built from a two-slide deck (one rectangle per slide). Alice moves the first slide's
  rectangle; Bob moves the *same* shape before seeing that step. The payload is absolute, so the
  later accepted value wins deterministically (x = 300) on all three replicas, and Alice's undo is
  refused with `undo-conflict` because it would overwrite Bob's later change; the refused step
  moves no version and loses no data. Bob then edits the second slide (the editors are not forced
  onto one active slide), drops his socket, Alice edits the second slide again (version 4), the
  viewer's op is refused `forbidden` without moving the version, and Bob reconnects with the
  version his replica has. The server answers the resync with the journal tail after that version;
  Bob catches up to version 4. All three clients and all three local replicas settle on one
  version and an equal `Presentation`, and a commit at version 4 reimports with the conflict's
  winner and the later move.
- **Three-client soak: compaction, base transfer and reconnect.** Two editors and one viewer on a
  two-slide deck (one rectangle per slide); the server is opened with `snapshot_every = 5`, so the
  journal is compacted during the run. The editors make 26 edits — six rounds of `shape.move`
  (Alice), `shape.setBounds` (Bob), `text.set` (Alice) and `shape.fill` (Bob), then a move to 400
  and a `text.set` — and both replicas follow every step. On the build host the 26 steps took
  0.67 s. At that point the base is the snapshot at version 25 and the tail is version 26 only.
  A viewer then joins at version 0: the server answers `resync` with the native base at 25
  (5 KiB inline), the session key map and the one-step tail; the viewer rebuilds its model, keys
  and version in 21.4 ms and reaches version 26, equal to both editors. `base_transfers_total`
  moved by one. Bob drops off, Alice moves the first rectangle to 420 and repaints it (versions
  27–28), and Bob reconnects with version 26: the answer is a tail of two steps, no base, and he
  lands on 28. All three replicas and the room settle on version 28 with an equal `Presentation`,
  and the commit at 28 reimports with the final boxes (420 / 350).
- **Big deck with media: hundreds of steps, a chained base and a fifth participant.** 120 slides,
  each with its own 160×160 picture (noise, so it does not compress) and a rectangle; the initial
  `.pptx` is 12 193 KiB and the native base is 11.8 MiB — above the 8 MiB inline limit by
  construction. The server runs with `snapshot_every = 40` and `max_base_chunk = 256 KiB`. Three
  editors make **322 edits** (move and bounds on the picture, text and fill on the rectangle, on a
  slide that comes round again) in 15.1 s on the build host — 46.8 ms per step, dominated by the
  server's 50 ms socket-read slice, not by the apply — and the journal is compacted seven times.
  A viewer then joins at version 0: the base at 320 travels as **48 chunks of 256 KiB** plus the
  two steps above it, and the viewer reaches version 322 in 168 ms. A fifth participant joins the
  same chain, drops its socket after three chunks and reconnects: it asks for chunk 3, not for
  chunk 0, finishes the chain and the tail, and converges. Both newcomers receive the two last
  edits as broadcasts; all five replicas and the room settle on one `Presentation` at version 324,
  and the commit at 324 (12 195 KiB, 223 ms) reimports with 120 slides, 120 pictures and every
  slide's picture transform equal to the replicas'. `base_chunks_total` counts the chunks served
  and `base_bytes_total` stays 0 for the chain. The five gates together run in about 19 s with
  `--test-threads=1` on the build host (LibreOffice skipped there).

The gates found a real adapter bug: `shape.move`, `shape.setBounds` and `text.set` ran against the
engine *selection*, so a replica whose active slide was another one refused a remote edit on
slide 5 with `no such shape`. Those payloads now select the target shape (and its slide) first.
`crates/collab/src/apply.rs`
`an_operation_on_another_slide_applies_whatever_the_replica_is_looking_at` fails without the fix,
and the 120-slide gate now moves a picture on every slide.

The three-session gate found a reconnect bug in `ClientSession`: a `welcome` ahead of the local
replica was adopted as the current head before the resync, so the client asked for the tail after
the head and kept a stale document while claiming to be current. The welcome no longer moves the
version when there is a gap, and the resync names the version the replica actually has
(`a_reconnect_asks_for_the_tail_from_the_version_the_replica_has`). The gate exercises the real
path: a replica that missed version 4 replays it from the snapshot tail and lands on the same
document as the editors.

## Verification

| Command | Result |
|---|---|
| `cargo test -p deckcraft-collab` | 166 unit + 8 integration |
| `cargo test -p deckcraft-collab-server --test gates -- --nocapture --test-threads=1` (on `agents`) | 4 passed in about 2.5 s: the 120-slide/media room and commit; three sessions (two editors on different slides and a viewer) across a conflicting edit, an undo-conflict, a forbidden viewer op and a reconnect with journal catch-up, all converging on version 4; the three-client soak (26 move/bounds/text/fill edits, compaction every 5 steps, a viewer rebuilt from a 5 KiB base at version 25 plus one tail step, a reconnect from a two-step tail, all three at version 28 and the commit reimporting); LibreOffice 26.2 renders 121 pages and round-trips the commit (run on the workstation, see below); on the build host the LibreOffice test skips itself (no `soffice`) |
| `cargo test -p deckcraft-collab --lib apply::` | 52 (a remote edit on a slide the replica is not looking at applies; two replicas converge on different table cells) |
| `cargo test -p deckcraft-collab-server` | 10 unit + 12 socket tests (two editors, viewer, health/metrics, invalid token, restart replay, media upload/fetch, picture insert, audio allowlist, shape-undo replay, CORS preflight, CORS GET, foreign origin, empty allowlist) + 4 commit tests (two copies with their originals kept, an external change is a `409`, a client commit answered after publication, the no-callback mode) + 4 gates |
| `cargo test -p deckcraft-collab-proto` | 26 |
| `cargo test -p deckcraft-ui-egui collab::` | 17 |
| `cargo test -p deckcraft-engine` | 575 passed, 1 ignored |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo fmt --all -- --check` | clean |
| `cargo xtask layers` | 32 crates (`collab-web` L6), no violations |
| `cargo test -p deckcraft-collab-web` | 15 passed |
| `cargo test -p deckcraft-web` | 3 passed (the headless page-host scenario) |
| `cargo check -p deckcraft-web --target wasm32-unknown-unknown` | ok |
| `trunk build` in `apps/deckcraft-web` | ok: `dist/web`, 49 MB `_bg.wasm` in the dev profile |
| after `collab/pr13` (chunked base): `fmt --check` clean, `clippy --workspace --all-targets -D warnings` clean, `collab-proto` 41, `collab` 269 + 8, `collab-server` 10 unit + 4 commit + 5 gates (19.4 s, LibreOffice skipped on the build host) + 17 socket, `collab-web` 15, `deckcraft-web` 6, `ui-egui collab::` 20 |
| after `c895183a`: `fmt --check` clean, clippy `--workspace --all-targets -D warnings` clean, `collab` 215 + 8, `collab-proto` 32, `collab-server` 10 unit + 4 commit + 4 gates (2.43 s, LibreOffice skipped on the build host) + 7 socket, `collab-web` 15, `deckcraft-web` 6, `ui-egui collab::` 20, `deckcraft-engine` 584 + 1 ignored, `xtask layers` 32 crates, wasm32 check of `deckcraft-web` ok; `cargo test --workspace --no-fail-fast` green except the two known host-without-`craft-fonts` failures (`ui-egui ime_tests::korean_composes_in_the_notes_pane`, `render tests::east_asian_vertical_text_stands_ideographs_upright`) |
| after the merge of `origin/main` (`c9803277`): `fmt --check` clean, clippy `--workspace --all-targets -D warnings` EXIT=0, `collab-proto` 42, `collab` 288 + 8, `collab-server` 19 unit + 5 commit + 18 socket + 5 gates (19.1 s, LibreOffice skipped on the build host), `collab-web` 15, `engine` 612 + 1 ignored (+ 6 + 8 integration), `pptx` 70 + 5 + 146, `ui-egui --lib` 526 passed + 4 ignored with the one known host-font failure (`ime_tests::korean_composes_in_the_notes_pane`), `xtask` `pdf-deps`/`assets` (48 files)/`layers` (28 crates)/`wasm` (22 crates) all EXIT=0, `cargo test --workspace --doc` pass; `cargo xtask ci` as a whole stops only at its `test` step, which is the same workspace run and the same host-font failure |
| after the second `origin/main` merge, head `1d92f763` (2026-11-12T13:00Z, build host `agents`): `cargo fmt --all -- --check` clean, `cargo clippy -j 4 -p deckcraft-collab -p deckcraft-ui-egui --all-targets -- -D warnings` clean, `collab` 305 lib + 8 `two_clients`, `ui-egui --lib collab::` 26 (the presence heartbeat test passes alone with `--exact`), `collab-server --test gates` **6 passed in 19.15 s** (the LibreOffice half skipped: no `soffice` on that host) — the §8 gate on the delivered head |

Re-run on 2026-11-08 after `3ec81d70`: `fmt` clean, `collab-proto` 19, `collab` 113 + 7,
`collab-server` 5 + 6, `ui-egui collab::` 17, `deckcraft-engine` 565 + 1 ignored; clippy over the
four collab crates and `ui-egui` with `-D warnings` clean; `xtask layers` 31 crates.

Re-run on 2026-11-08 after `31842050`: `fmt` clean, `collab-proto` 19, `collab` 113 + 7,
`collab-server` 10 + 6 + 4 (`tests/commit.rs`), `ui-egui collab::` 17, `deckcraft-engine` 565 + 1
ignored; clippy over the four collab crates and `ui-egui` with `-D warnings` clean; `xtask layers`
31 crates.

Re-run on 2026-11-09 after `98d9e9ce`: `fmt` clean, `collab-proto` 19, `collab` 118 + 7,
`collab-server` 10 + 6 + 4, `ui-egui collab::` 17, `deckcraft-engine` 567 + 1 ignored; clippy over
the four collab crates and `ui-egui` with `-D warnings` clean; `xtask layers` 31 crates.

Re-run on 2026-11-09 after the `arrange.align`/`arrange.distribute` slice: `fmt` clean,
`collab-proto` 20, `collab` 122 + 8, `collab-server` 10 + 6 + 4, `ui-egui collab::` 17,
`deckcraft-engine` 569 + 1 ignored; clippy over the four collab crates and `ui-egui` with
`-D warnings` clean; `xtask layers` 31 crates.

Re-run on 2026-11-09 after the three-session gate (`556c00d9`): `fmt --check` clean,
`collab-proto` 20, `collab` 124 + 8, `collab-server` 10 unit + 4 commit + 3 gates + 6 socket,
`ui-egui collab::` 17, `deckcraft-engine` 569 + 1 ignored; clippy over the four collab crates and
`ui-egui` with `-D warnings` clean; `xtask layers` 31 crates. The three session gate runs in
0.95 s on `agents` (it includes two 300 ms waits for the server to drop the old peer and two
round trips); its document comparisons are `Presentation` equality on three local replicas, and
the commit at version 4 reimports with the expected two slides.

Re-run on 2026-11-09 after the base transfer and the three-client soak (`e007e965`, on `agents`,
rust 1.99.1): `fmt --check` clean, `collab-proto` 22, `collab` 129 + 8, `collab-server` 10 unit
+ 4 commit + 4 gates + 6 socket, `ui-egui collab::` 17, `deckcraft-engine` 570 + 1 ignored;
clippy over the four collab crates, `engine` and `ui-egui` with `-D warnings` clean; `xtask
layers` 31 crates. The four gates run in about 2.5 s with `--test-threads=1`; the soak gate itself
prints 26 edits in 0.67 s, a 5 KiB base at version 25 rebuilt in 21.4 ms and a two-step tail on
the reconnect. `git status` is clean after the docs commit; nothing was pushed and CI was not
touched.

Re-run on 2026-11-09 after the transition slice (`927e773c`, on `agents`, rust 1.99.1): `fmt
--check` clean, `collab-proto` 23, `collab` 138 + 8, `collab-server` 10 unit + 4 commit + 4 gates
+ 6 socket, `deckcraft-engine` 572 + 1 ignored; clippy over `collab-proto`, `collab`,
`collab-server` and `engine` with `-D warnings` clean; `xtask layers` 32 crates. The gates were
run without `soffice` on the build host. `git status` is clean after the docs commit; nothing was
pushed and CI was not touched.

Re-run on 2026-11-09 after the table-text slice (`2a91d49e`, on `agents`, rust 1.99.1): `fmt
--check` clean, `collab-proto` 24, `collab` 147 + 8, `collab-server` 10 unit + 4 commit + 4 gates
+ 6 socket, `deckcraft-engine` 573 + 1 ignored; clippy over `collab-proto`, `collab`,
`collab-server` and `engine` with `-D warnings` clean; `xtask layers` 32 crates. The gates were
run without `soffice`. The new tests cover two replicas editing different cells of one table, a
cell operation resent after a reconnect (one version bump, one insertion), a queued cell insert
rebased past an inserted row, a queued whole-cell `text.set` following its cell after an inserted
row, a queued insert refused after its row is deleted, two people in one
cell keeping both insertions without moving another cell, and undo/redo of cell text and of a
whole-table row edit. `git status` is clean after the docs commit; nothing was pushed and CI was
not touched.

Re-run on 2026-11-09 after the animation-step slice (`f562aea4`, on `agents`, rust 1.99.1): `fmt
--check` clean, `collab-proto` 25, `collab` 156 + 8, `collab-server` 10 unit + 4 commit + 4 gates
+ 6 socket, `deckcraft-engine` 575 + 1 ignored; clippy over `collab-proto`, `collab`,
`collab-server` and `engine` with `-D warnings` clean; `xtask layers` 32 crates. The gates were
run without `soffice` (4 passed in 2.44 s). The new tests cover a step on two replicas, two steps
in a row keeping their order, an undo that keeps the shape's other step on both replicas, a later
list from another participant refusing the undo with `undo-conflict`, an unaddressable shape
(unknown key, another slide, no shape) refused before any write, a forged restore, the
normaliser's whole-list and index-addressed removal, the wire round trip, the engine setter, and
the whole template through the in-memory hub (two `animation.add` submissions and an undo on two
replicas). The engine soak no longer picks the two exact-value setters: its generator cannot build
their whole-value parameters, so they only bounced off their validation and skewed the edit-rate
assertion. `git status` is clean after the docs commit; nothing was pushed and CI was not touched.

Re-run on 2026-11-09 after the notes-text slice (`b7025f9a`, on `agents`, rust 1.99.1): `cargo fmt
--all -- --check` clean, `cargo clippy --workspace --all-targets -- -D warnings` clean,
`collab-proto` 26, `collab` 166 + 8, `collab-server` 10 unit + 4 commit + 4 gates + 6 socket;
`xtask layers` 32 crates. The new tests cover two replicas converging on a notes insert after a
slide was inserted (the structural step does not move the address), an undo and redo that restore
the previous body and its bold run, a foreign notes edit refusing the undo with `undo-conflict`,
an unknown, deleted, unaddressed or shape-addressed slide refused before any write together with
an unreadable body, the receiving caret moving with the inserted and deleted prefix while a
shape's text selection on the slide stays put, a whole-body replacement sending a queued insert to
`stale`, the journal rebase keeping two typists' text in one slide's notes and never shifting
another slide's notes, and the normaliser plus the wire form.

Re-run on 2026-11-09 after the table-cell format slice (`a48c12de`, on `agents`, rust 1.99.1):
`cargo fmt --all -- --check` clean, `cargo clippy --workspace --all-targets -- -D warnings` clean,
`collab-proto` 27, `collab` 175 + 8, `collab-server` 10 unit + 4 commit + 4 gates + 6 socket;
`deckcraft-engine` 575 + 1 ignored; `ui-egui` full suite 509 + 1 known environment failure
(`ime_tests::korean_composes_in_the_notes_pane`, no `craft-fonts`/Korean font on the build host);
`xtask layers` 32 crates. The new tests cover two replicas converging on one cell's format with a
neighbour untouched, an undo and redo that put back and reapply the exact cell body (the old
formatting included) without touching a neighbouring cell's step, an undo refused with
`undo-conflict` after another participant typed in the addressed cell, a queued cell format
landing on the cell as it is after someone typed there, the same format following an inserted row
and refused `stale` when its row is deleted, and an unaddressable cell (no such row/column, a
table without an address, a non-table shape, a deleted slide) refused before any write with no
undo step recorded; the normaliser and both wire forms are pinned. `git status` is clean after the
docs commit; nothing was pushed and CI was not touched.

Re-run on 2026-11-09 after the run-range format slice (`b4f13167`, on `agents`, rust 1.99.1):
`cargo fmt --all -- --check` clean, `cargo clippy --workspace --all-targets -- -D warnings` clean,
`collab-proto` 28, `collab` 183 + 8, `collab-server` 10 unit + 4 commit + 4 gates + 6 socket
(4 gates in 1.5–2.5 s, LibreOffice skipped on the build host), `ui-egui collab::` 18. The new tests
cover two replicas converging on a bold range of a shape's body (text kept, the run boundary at
the range's end, the rest unformatted), the same for a range inside one table cell with a
neighbour untouched, an undo and redo that put back and reapply the previous runs while the text
after the range is untouched, an undo refused with `undo-conflict` after another participant typed
in the same body, an empty, reversed, past-the-end or missing-cell range refused `invalid` before
any write with no undo step, the receiving user's selection kept across a remote range format, a
queued range following text typed before it (offsets shifted, the typed characters not bold) and
refused `invalid` when its characters were deleted, the normaliser's offsets for a range in a
shape, across a paragraph break and inside a cell (and a caret or a conflicting explicit cell
refused), and both wire forms (with and without `cell`/`range`). `git status` is clean after the
docs commit; nothing was pushed and CI was not touched.

Re-run on 2026-11-09 after the N-cell/caret format slice (`6ddc0c4f`, on `agents`, rust 1.99.1):
`cargo fmt --all -- --check` clean, `cargo clippy --workspace --all-targets -- -D warnings` clean,
`collab-proto` 29, `collab` 192 + 8, `collab-server` 10 unit + 4 commit + 4 gates + 6 socket
(LibreOffice skipped on the build host), `ui-egui collab::` 19, `ui-egui` full 511 + the known
`ime_tests::korean_composes_in_the_notes_pane` environment failure (no Korean font on the build
host), `xtask layers` 32 crates, no layering violations. The new tests cover two replicas
converging on one operation over two cells of a 2×2 table (the first whole, the second over two
characters; a neighbouring cell keeps its text and formatting; both deltas equal), an undo that
restores exactly the two addressed cells' whole bodies with one `cells_text` and a redo that
reapplies them while the neighbour never moves, an undo refused `undo-conflict` after someone
typed in one of the addressed cells (history not moved, nothing written), an empty address list, a
missing cell, an out-of-body range and a repeated cell refused before any write with the version
unchanged (and a table addressed without a cell refused `unsupported`), a remote N-cell format
that leaves the receiving user's selection alone, a queued N-cell format whose **every** address
follows an inserted row, a caret inside a word of a shape body converging on both replicas to the
same word-wide bold (undo restores the whole body, redo reapplies), a caret at a word boundary
that does not bold any existing character (no pre-widening) and a paragraph command at a caret that
takes only its paragraph, a caret past the body refused `invalid` before any write with no step,
the normaliser emitting one address per selected cell and a `caret` for a caret in a shape's body,
and both wire forms (`addresses` with `cell`/`range`/`caret`).

Re-run on 2026-11-09 after the **rebase onto `origin/main` (`0623b430`)** (on `agents`, rust
1.99.1; `collab/pr10` = `7459d798`, `collab/pr11` = `da3199bf`): `cargo fmt --all -- --check`
clean, `cargo clippy --workspace --all-targets -- -D warnings` clean (22 s warm), `collab-proto`
29, `collab` 192 + 8, `collab-web` 15, `collab-server` 10 unit + 4 commit + 4 gates + 6 socket
(the four gates in 2.52 s with `--test-threads=1`, LibreOffice skipped on the build host),
`deckcraft-web` 6 (main's page-API tests included), `ui-egui collab::` 19, `ui-egui` full 515
passed + 4 ignored + the known `ime_tests::korean_composes_in_the_notes_pane` environment failure
(no `craft-fonts`/Korean font on the build host), `deckcraft-engine` 577 + 1 ignored, `xtask
layers` 32 crates with no violations, `cargo check -p deckcraft-web --target
wasm32-unknown-unknown` ok. `git status` is clean after the docs commit; nothing was pushed, CI
was not touched, and no collab slice changed (the code patch is main's own, see the rebase
section).

Re-run on 2026-11-09 after the whole-sequence animation slice (on `agents`, rust 1.99.1):
`cargo fmt --all -- --check` clean, `cargo clippy --workspace --all-targets -- -D warnings`
clean, `collab-proto` 30, `collab` 195 + 8, `collab-web` 15, `collab-server` 10 unit + 4 commit
+ 4 gates + 6 socket (LibreOffice skipped on the build host), `deckcraft-web` 6, `ui-egui
collab::` 19, `deckcraft-engine` 579 + 1 ignored, `xtask layers` 32 crates with no violations,
`cargo check -p deckcraft-web --target wasm32-unknown-unknown` ok. The new tests cover two
replicas converging on `animation.set` and on the undo of the second shape's set, `animation.timing`
changing one step with an undo that restores the exact duration, `animation.move` crossing another
shape's step on both replicas with an undo that restores the whole order, a foreign animation edit
on the same slide making the undo `undo-conflict` without a version move, the engine keeping
interleaved steps through an exact list and refusing a missing shape, an unknown effect/field and
too many steps before any write, the plans matching the single-user commands, the normaliser
emitting the exact sequence and nothing for a no-op, and both wire forms. `git status` is clean
after the docs commit; nothing was pushed and CI was not touched.

Re-run on 2026-11-09 after the rest-of-the-animation-surface slice (`e2a5ef41`, on `agents`, rust
1.99.1): `cargo fmt --all -- --check` clean, `cargo clippy --workspace --all-targets -- -D
warnings` clean, `collab-proto` 30, `collab` 197 + 8, `collab-web` 15, `collab-server` 10 unit +
4 commit + 4 gates + 6 socket (the four gates in 2.45 s with `--test-threads=1`, LibreOffice
skipped on the build host), `deckcraft-web` 6, `ui-egui collab::` 20, `ui-egui` full 516 passed +
4 ignored + the known `ime_tests::korean_composes_in_the_notes_pane` environment failure (no
`craft-fonts`/Korean font on the build host), `deckcraft-engine` 580 + 1 ignored, `xtask layers`
32 crates with no violations, `cargo check -p deckcraft-web --target wasm32-unknown-unknown` ok
(two pre-existing dead-code warnings on wasm32, in untouched fields of `collab-web`/`ui-egui`).
The new tests cover `animation.options` and `animation.painterApply` through the engine plans
(options by index leave exactly the planned codes and text build; the painter copies a
paragraph-level step onto a shape without that paragraph and it becomes a whole-shape step); the
normaliser turning options, clear and the painter into the slide's whole sequence, sending
nothing for a no-op and refusing a missing clipboard, an empty target, an unknown target and an
out-of-range slide; the hub running options, the painter (clipboard in the session) and clear on
two replicas with an undo of each; and the UI sending the painted sequence over the link and
disarming a one-shot pick. `git status` is clean after the docs commit; nothing was pushed, CI
was not touched, and PR #1975 is unchanged.

Re-run on 2026-11-09 after the document-font slice (`fd4e54e8`, on `agents`, rust 1.99.1):
`cargo fmt --all -- --check` clean, `cargo clippy --workspace --all-targets -- -D warnings`
clean, `collab-proto` 31, `collab` 205 + 8, `collab-web` 15, `collab-server` 10 unit + 4 commit
+ 4 gates + 7 socket, `deckcraft-web` 6, `ui-egui collab::` 20, `deckcraft-engine` 584 + 1
ignored, `xtask layers` 32 crates with no violations, `cargo check -p deckcraft-web --target
wasm32-unknown-unknown` ok, `cargo check --workspace --all-targets` clean (the E0502 reported on
`57da8f76` in `crates/collab/src/local.rs` does not reproduce at `07b6f1c8`, the base of this
slice, nor after it). `cargo test --workspace --no-fail-fast` is green except the known environment
failures of a host without `craft-fonts`: `ui-egui ime_tests::korean_composes_in_the_notes_pane`
and `render tests::east_asian_vertical_text_stands_ideographs_upright` (no CJK face on the
host; both draw a glyph the machine does not have). The four §8 gates run 4/4 in 2.44 s. The new
tests cover two replicas converging on `font.create` (the bytes by digest on the media channel,
the same media id on both, the face installed), `font.set` and `font.remove`; an undo of a
descriptor step and of the create itself broadcast through the hub to both replicas, with the
face object staying for the redo; an undo refused `undo-conflict` after another participant
changed the entry; a step addressed by a slide, a digest that is not a font, a missing upload
(retryable), an unknown entry and a descriptor set with nothing to set all refused before any
write with no version move; the normaliser's digest-and-face form, its upload (`take_uploads`),
and its refusals; the media channel's sfnt allowlist and its own budget; and a real socket:
upload/fetch of a face, a viewer refused, a `font_create` applied to the room and a commit that
reimports with the entry. `git status` is clean after the docs commit; nothing was pushed, CI was
not touched, and PR #1975 is unchanged.

Re-run on 2026-11-10 after the chart slice (`f67b0858`, on `agents`, rust 1.99.1): `cargo fmt
--all -- --check` clean, `cargo clippy -p deckcraft-engine -p deckcraft-collab
-p deckcraft-collab-proto -p deckcraft-ui-egui --all-targets -- -D warnings` clean,
`collab-proto` 34, `collab` 230 + 8, `collab-server` 10 unit + 4 commit + 4 gates + 7 socket,
`ui-egui collab::` 20, `deckcraft-engine` 587 + 1 ignored, `xtask layers` 32 crates (no
violations). The new tests cover two replicas converging on one whole chart value (the type, the
data and the captions in one step, both deltas equal), an undo that restores the whole chart shape
and a redo that reapplies it on both, an undo refused `undo-conflict` after another participant
edited the same chart, a value on a plain shape refused `invalid` with no history step, an
insertion (the whole chart and its box) reaching both replicas as the same shape and its undo
removing it, the normaliser's whole-chart form for `chart.type`/`chart.data`/`chart.options` and
for `insert.chart` (unknown parameters refused), the checks that refuse a bad bin width, a bad
text colour, a value that is not a chart, too many series and an oversized value before the first
write, the exact `shape.setField{field: chart}` restore, and the same steps and undo through the
in-memory hub. `git status` is clean after the docs commit; nothing was pushed, CI was not
touched, and PR #1975 is unchanged.

Re-run on 2026-11-10 after the model-scene slice (`7b51e65d`, on `agents`, rust 1.99.1): `cargo
fmt --all -- --check` clean, `cargo clippy --workspace --all-targets -- -D warnings` clean,
`collab-proto` 39, `collab` 258 + 8, `collab-server` 10 unit + 4 commit + 4 gates + 17 socket,
`ui-egui collab::` 20, `deckcraft-engine` 592 + 1 ignored, `pptx` 67 + 145 + 5, `render` 209 + the
known host-without-`craft-fonts` failure, `collab-web` 15, `deckcraft-web` 6, `xtask layers` 32
crates. The new tests cover two replicas converging on one whole scene value (both deltas equal),
an undo that restores the whole model shape and a redo that reapplies it on both, an undo refused
`undo-conflict` after another participant edited the same model, a scene on a plain shape refused
`invalid` with no history step, the normaliser's whole-scene form and its refusals (a relative
field, a spot without an angle, an angle on a point light, a missing scene, a plain shape, a
viewer), the wire round trip and its refusal of unknown fields, the engine's checks (unknown type,
zero direction, intensity/ambient out of range, the angle/type mismatch, a bad background) before
any write, a camera and a scene on one model written as `am3d:camera`/`am3d:scene` and read back
through two pptx round trips, and a real socket: upload the `.glb`, insert the model, set the
scene on two editors and the viewer, commit and reimport the file with the same numbers, refuse a
viewer's step `forbidden`, a relative frame as a bad frame and a spot without an angle `invalid`,
with the version staying at 2. `git status` is clean after the docs commit; nothing was pushed,
CI was not touched, and PR #1975/#1998 are unchanged.

The socket tests were run against a real loopback server; the restart test creates a room, moves
the version, shuts the server down, starts a new one on the same store and asserts the version
and document came back. The media tests cover a rejected upload (viewer, wrong digest, no token),
one object for repeated bytes, a picture applied from a reference, an audio upload with its own
allowlist, and a journal replay that loads the object before running the operation. The two-client
convergence tests compare the delta of a fill/line/format/duplicate step on two adapters and the
same MediaInsert applied on both replicas. Undo of a fill, an outline and a `format.*` step is
tested on one replica and on two (the restore is an ordinary confirmed command, so both replicas
converge); a foreign fill or a foreign format on the same object is refused with `undo-conflict`,
a forged `shape_field` (no history step, or a value other than the recorded one) is refused with
`invalid`. Undo of a group's fill restores the group and both members with one `shape_fields`
operation on two replicas, and a foreign repaint of one member is refused with `undo-conflict`.
Undo of `slide.delete` restores the whole slide (position, content, section membership) on two
replicas and redo deletes it again; undo of `slide.new` deletes the slide and redo restores it;
a forged `slide_restore` aimed at another slide is refused with `invalid`. A shape insert is
undone after a foreign edit of a *neighbour* (both replicas equal, the neighbour's move stays) and
refused after a foreign edit of the *inserted* shape itself (`undo-conflict`); a shape delete is
undone after a foreign edit of the neighbour that is left, restoring the shape at its z-order with
its animation, and a deleted group comes back as one image carrying its children. Replay of a
`shape_restore` after a restart is covered by the store test, which also proves the restored shape
keeps its key. Grouping is undone on two replicas as one `shape_restore` that removes the group and
puts both members back at their z-order; redo removes the members and restores the whole group
image. Ungrouping is undone the other way round (the group image comes back, the members go out),
and a foreign move of a member refuses the group undo with `undo-conflict`. The engine tests cover
`slide.restore` (index, section, duplicate id), `shape.setFields` (a bad value refuses the whole
merge), `shape.restore` (z-order, animation, group child, duplicate id, missing parent) and
`shape.restore` with `remove` (group ↔ members in one step, a missing removal refuses before any
write), plus `shape.setBoundsMany` (one undo step for the whole selection, a missing or repeated
id refuses before the first write, a locked target is left alone).

The commit tests run the server against a fake cabinet on loopback that keeps versions like the
cabinet does. Two copies of the same original file are opened as two rooms; each commit posts a
`.pptx` that reimports with its own edit, both cabinets keep their original version byte for byte,
and a name passed at session creation reaches the callback. The `409` test changes the cabinet's
file under a live room: the server posts the commit anyway, the cabinet refuses it, no version is
created, the original stays, and the conflict counters move (`cabinet_conflicts_total`). A commit
whose `expectedVersion` does not match the room is refused before any callback is made.

Re-run on 2026-11-10 after the optimistic-preview slice (`collab/pr15`, on `agents`, rust 1.99.1):
`cargo fmt --all -- --check` clean (EXIT=0); `cargo clippy --workspace --all-targets -- -D
warnings` clean (EXIT=0); `cargo test -p deckcraft-collab-proto` 42 passed / 0 failed / 0 ignored
(EXIT=0); `cargo test -p deckcraft-collab` 280 + 8 passed / 0 failed / 0 ignored (EXIT=0);
`cargo test -p deckcraft-ui-egui collab::` 24 passed / 0 failed / 0 ignored (502 filtered out,
EXIT=0); `cargo test -p deckcraft-collab-server` 10 unit + 5 commit + 5 gates (15.78 s,
LibreOffice skipped on the build host) + 18 socket passed / 0 failed / 0 ignored (EXIT=0);
`cargo test -p deckcraft-collab-web -p deckcraft-web` 15 + 6 passed (EXIT=0); `cargo check -p
deckcraft-web --target wasm32-unknown-unknown` ok (EXIT=0); `cargo xtask layers` 32 crates, no
violations (EXIT=0). The new tests cover a previewed move visible in the local model before the
answer, settled by `applied` without running the command twice and outside the engine undo stack;
a refusal that restores exactly the refused edit and leaves an accepted one; a foreign step
between preview and answer that gives a `resync` from the replica's version, after which the tail
lands in the room's order on both replicas; a viewer and an offline replica that show nothing; a
command submitted during a preview that waits and is normalised only against confirmed state; the
flag-off path (no preview, no `preview` on the wire); the wire round trip of the optional field and
a frame without it; a room that applies an operation carrying `preview`, dedups a resend with
another local id and never stores or echoes the id (session, proto and socket tests); and the UI's
`DECKCRAFT_COLLAB_PREVIEW` path over a pipe (drawn before the answer, settled on it). `git status`
is clean after the docs commit; nothing was pushed, CI was not touched, and PR #1975/#1998 are
unchanged.

## TASK.md acceptance matrix

| Criterion | State | Where / why |
|---|---|---|
| Two or more clients edit one presentation | ✅ for the v1 command set; a three-client soak (two editors and a viewer, 26 edits) converges across compaction | hub, socket test, UI pipe test, `tests/gates.rs` |
| A 100+ slide deck with media stays responsive and its commit opens in LibreOffice | ✅ 120 slides/120 pictures, 4 media objects; commit renders 121 pages and survives an LO round trip; and a big-deck soak of 322 edits by three editors whose 11.8 MiB native base reaches a late client in 48 chunks, a fifth participant resuming the chain mid-way, and a commit at the head that reimports with every replica's values | `tests/gates.rs`; adapter fix for edits on a non-active slide |
| Insert a picture in a shared session | ✅ pictures, audio, video and 3-D models over `ws://` and `wss://` | media channel, adapter and socket tests |
| Cursors/selections/participants visible | ✅ | `CollabUi::marks`, participants pane |
| Slides, shapes and order converge | ✅ for move/bounds/align/distribute/insert/delete/slide/group/order/fill/line/format (a shape's body, one table cell, N cells of one table, a run range or a caret)/duplicate/transition/animation (a shape's step list, a slide's whole sequence from set/timing/move/options/clear and the painter), table cell text, table row/column edits, the rest of the table surface (style, cell fill, merge/split, column/row sizing), slide notes text, document fonts (the embedded-font registry: one face by digest, descriptors, removal), SmartArt (outline, layout, style and colours as one whole diagram value, an insertion of a whole diagram, the three conversions `fromText`/`toText`/`toShapes` as one structural step each, and a node's picture as one final value with its place) and charts (edits and insertion as one whole chart value) | collab tests, socket test, commit reimport |
| Reconnect works | ✅ within the retained journal tail; after compaction the client is rebuilt from the native base and the key map — inline up to 8 MiB, above it as a chunk chain that resumes after a dropped socket | client queue + resync; three-session gate; three-client soak; big-deck gate; `DocSession::resync` / `base_chunk` |
| Per-user undo/redo | ✅ text (shape, table cell and slide notes), shape values, align/distribute, fill, outline, `format.*` (a shape's body, one cell's body, N cells of one table, a run range or a caret), group fill/line, slide insert/delete, shape insert/delete, group/ungroup, transition, animation (a shape's step list and a slide's whole sequence for set/timing/move/options/clear/painter), table row/column edits, the whole table for style/fill/merge/split/sizing, document fonts (the whole entry), a SmartArt step (the whole group, including a node's picture), a SmartArt insertion (the whole inserted shape) and a SmartArt conversion (the whole source shape, whatever its id) | server history, `undo-conflict`, `shape_field`/`cell_text`/`cells_text`/`shape_fields`/`shapes_set_bounds`/`shape_restore` (an insertion or a conversion too)/`slide_restore` (the last with `remove`)/`slide_transition`/`shape_animations`/`slide_animations`/`notes_set`/`font_restore`/whole-table `shape_field` |
| Viewer cannot edit | ✅ server + engine gate, also inside the three-session gate | socket test, UI test |
| Commit creates a new version, original kept | ✅ server and desktop: `POST /v1/sessions/{id}/commit` and `ClientMsg::Commit` → cabinet callback with the bytes, `409` on an external change, the accepted version stored in `meta.json`; Save in a live desktop session commits instead of writing the local copy, stays dirty until the answer and offers a copy on a refusal | `apps/deckcraft-collab-server` (`cabinet.rs`, `lib.rs`), `tests/commit.rs`, `crates/collab/src/client.rs`, `crates/ui-egui/src/app/files.rs`; the cabinet endpoint remains external |
| Single-user mode unchanged; feature behind a flag | ✅ | flag-off tests; a page without `deckcraft:session` makes no network call (`deckcraft-web`); optimistic preview is a second opt-in on top (`DECKCRAFT_COLLAB_PREVIEW`, one operation at a time) |
| Browser: session, presence, commit through the host bridge | ✅ code + headless gate; the cabinet endpoint remains external (CORS is in the server, `DECKCRAFT_CORS_ORIGINS`) | `crates/collab-web`, `apps/deckcraft-web/src/host_session.rs`, `cargo test -p deckcraft-web` |
| RFC, protocol, tests, docs | ✅ RFC v1 accepted 4/5 (ADR-0015, 2026-11-08); formal `proposal` `…-47b7` | this tree, bus decision `20261108T143417Z-craft-8036` |
| PR in `storytold/deckcraft` | ✅ open and **ready** as [#2012](https://github.com/storytold/deckcraft/pull/2012) from the fork (`copoka/deckcraft:collab/pr15`, delivered head `5a105c61` = docs on code `aaecc6db` = the fork head = the PR head = the local branch head at 2026-11-12T03:13:50Z), rebased onto `origin/main` and merged with it; every review pass, the STATUS sync, the presence heartbeat and the op_id slice are delivered; no workflow run exists on `5a105c61`/`aaecc6db`, and the four runs on `38a20a9c` are `action_required` (created 11:31:32Z, awaiting approval, nothing executed), so approval is the human step; the second review's five findings are closed, the third review found nothing in code, the fourth review's four Major and one Minor are delivered in `9ad80547` and the fifth's five in `891187f5` | [`pr10.md`](pr10.md); merge is a human decision; #1998/#1975 are superseded and closed by a person, not by this project |

Re-run on 2026-11-10 after the CodeRabbit review pass on #2012 (`5b6fcfcd`, the code commit of the
pass pushed as `9b43c611a`, on
`agents`, rust 1.99.1): `cargo fmt --all` clean; `cargo clippy --workspace --all-targets -- -D
warnings` clean (EXIT=0); `cargo test -p deckcraft-collab` 283 + 8 passed / 0 failed / 0 ignored
(EXIT=0); `cargo test -p deckcraft-collab-server` 13 unit + 5 commit + 5 gates (17.1 s,
LibreOffice skipped on the build host) + 18 socket passed (EXIT=0); `cargo test -p
deckcraft-engine` 594 + 1 ignored, plus 6 + 8 integration (EXIT=0); `cargo test -p deckcraft-pptx`
69 + 5 + 145 passed (EXIT=0); `cargo test -p deckcraft-ui-egui --lib collab::` 24 and `host_save`
12 passed (EXIT=0); `cargo test -p deckcraft-web` 6 passed (EXIT=0); `cargo xtask layers` 32
crates, no violations (EXIT=0). The whole `ui-egui` library run has one failure before and after
this diff — `ime_tests::korean_composes_in_the_notes_pane` needs a CJK face and the build host has
no `craft-fonts`. The new tests cover the 20 MiB HTTP body, the closed room, the audio replay, an
operation held across a resync, two connections of one actor, a nested list lead on a viewer and
with the collab queue, a non-finite camera and crop, the cabinet chunk size and the SmartArt
picture's opacity round trip.

Re-run on 2026-11-11 after the second review pass (`d56272e6`, `082b7c7b` and `2bbe723f`, code, on
`agents`, rust 1.99.1): `cargo fmt --all -- --check` clean (EXIT=0); `cargo clippy --workspace
--all-targets -- -D warnings` clean (EXIT=0); `cargo test -p deckcraft-engine -p deckcraft-collab
-p deckcraft-collab-proto -p deckcraft-collab-server -j 12`: engine 596 + 1 ignored, collab 285 +
8, collab-proto 42, collab-server 15 unit + 5 commit + 5 gates (16.8 s, LibreOffice skipped on the
build host) + 18 socket, all passed, 0 failed (EXIT=0); `cargo test -p deckcraft-ui-egui --lib
collab::` 24 and `host_save` 12 passed; `cargo test -p deckcraft-collab-web -p deckcraft-web` 15 +
6 passed; `cargo xtask layers` 32 crates, no violations (EXIT=0). The six new tests were each run
against the un-fixed code first and failed there: `a_viewer_drag_moves_nothing_and_a_shared_drag_is_one_operation`,
`a_shared_connector_drag_keeps_its_glue`,
`a_finished_drag_normalises_to_one_operation_with_its_final_boxes`,
`a_gap_during_an_outstanding_resync_never_puts_a_second_request_on_the_wire`,
`a_base_reopens_at_the_version_it_was_taken_at`,
`a_commit_is_bound_to_the_hub_it_was_authorized_against`. Those three code commits and the
`4a64e764` docs commit sit under the delivered head `9ad80547` (fork and PR head), which is in
turn under `891187f5` and `38a20a9c`.

Re-run on 2026-11-11 after the fourth review pass (this session, on `4a64e764`, before the
commit): `cargo fmt --all --check` clean; `cargo clippy --workspace --all-targets -- -D warnings`
EXIT=0; `cargo test -p deckcraft-collab-server` 16 unit (8 `store::`) + 5 commit + 5 gates (14.8 s)
+ 18 socket, 0 failed; `cargo test -p deckcraft-collab` 287 + 8; `cargo test -p deckcraft-engine`
597 + 1 ignored plus 6 + 8 integration. The three new tests were run against the reverted
production change and failed on the intended assertion; the tree was restored from a copy on the
build host afterwards.

Re-run on 2026-11-10 after the SmartArt node-picture slice (`44489f34`, on `agents`, rust 1.99.1):
`cargo fmt --all -- --check` clean, `cargo clippy --workspace --all-targets -- -D warnings`
clean, `cargo test -p deckcraft-collab-proto -p deckcraft-collab -p deckcraft-collab-server`:
`collab-proto` 37 passed, `collab` 252 + 8 passed, `collab-server` 10 unit + 0 in `main.rs` + 4
commit + 4 gates + 15 socket, all passed, 0 failed, 0 ignored, EXIT=0. The engine's filtered
`smartart` tests also pass (14 passed), the full engine suite is 589 passed + 1 ignored, and
model/pptx/format are green (model 111, pptx 67 + 5, format 48 + 145 integration; 0 failed, 0
ignored, EXIT=0). The new tests cover two replicas converging on one node's picture and its place
(the full client normaliser, the adapter and the in-memory hub), the picture installed from the
media channel's bytes with the same `add_media` call on every replica; the undo restoring the
**whole original node** (its diagram, the generated children with their ids and no picture) and
the redo returning the exact result; a viewer's client-side `submit` refused `ReadOnly` and a
hand-built operation refused `Forbidden` by the server with the version and document unchanged,
also over a real socket; hostile input — a plain shape, a nested graphic, a node the outline does
not have, media the session does not hold or an object the channel says is not a picture, a place
whose crop leaves no visible area and an opacity out of range — refused before the first write
with the document unchanged and no history step. `git status` is clean after the docs commit;
nothing was pushed, CI was not touched, and PR #1975/#1998 are unchanged.

Re-run on 2026-11-10 after the SmartArt conversion slice (`db413e5d`, on `agents`, rust 1.99.1):
`cargo fmt --all -- --check` clean, `cargo clippy --workspace --all-targets -- -D warnings`
clean, `cargo test -p deckcraft-collab-proto -p deckcraft-collab -p deckcraft-collab-server`:
`collab-proto` 36 passed, `collab` 247 + 8 passed, `collab-server` 10 unit + 0 in `main.rs` + 4
commit + 4 gates + 14 socket, all
passed, 0 failed, 0 ignored, EXIT=0. The engine's filtered `smartart` tests also pass (13
passed). The new tests cover two replicas converging on `fromText`, `toText` and `toShapes`
(full client normaliser, the whole diagram/body/box, the resulting text box or plain group) and
each undo/redo in the adapter, the in-memory hub and the engine: the undo removes the converted
shape and puts the **whole source shape** back (a text box, or a SmartArt group with its diagram
and generated children and their ids), the redo reapplies the exact result; a viewer's
client-side `submit` is refused `ReadOnly`, a hand-built operation is refused `Forbidden` by the
server with the version and document unchanged, and the socket test proves the same over a real
connection; hostile values (an unknown layout, 5000 nodes, a zero-size box, a body that is not a
`TextBody`, `toText`/`toShapes` on a plain shape) are refused `invalid` before the first write
with the document unchanged and no history step. `git status` is clean after the docs commit;
nothing was pushed, CI was not touched, and PR #1975 is unchanged.

Re-run on 2026-11-10 after the SmartArt insertion slice (`cfd40da2`, on `agents`, rust 1.99.1):
`cargo fmt --all -- --check` clean, `cargo clippy --workspace --all-targets -- -D warnings`
clean, `collab-proto` 36, `collab` 240 + 8, `collab-server` 10 unit + 0 in `main.rs` + 4 commit +
4 gates (LibreOffice present, no skip) + 13 socket (one new: an insertion over the socket, the
viewer's `smartArt.nodes` query, a hostile tree refused with no version move, and the query still
answering the unchanged graphic), `deckcraft-web` 6, `ui-egui collab::` 20, `deckcraft-engine`
588 + 1 ignored (plus 6 integration + 8 show-paragraph), `xtask layers` 32 crates with no
violations. The new tests cover two replicas converging on one inserted graphic (layout, node
tree and box), its undo removing the whole shape as one structural step and its redo putting the
diagram and generated children back on both; a hostile tree (unknown layout/style/colours, a
value that is not a diagram) refused `invalid` with no shape added and no history step; the
normaliser building the same value as the command (`insert_plan`), its defaults and its refusals;
the engine inserting exactly the given value, refusing hostile input before the first write; a
viewer's client-side refusal and a hand-built operation refused by the server with the version
unchanged; and the authoritative `smartArt.nodes` query answering the asker alone, for a viewer
too, with an unknown, mutating or non-allowlisted query refused as an error and no version move. `git status` is
clean after the docs commit; nothing was pushed, CI was not touched, and PR #1975 is unchanged.

Re-run on 2026-11-10 after the CORS slice (`f996865d`, on `agents`): `cargo fmt --all -- --check`
clean, `cargo clippy --workspace --all-targets -- -D warnings` clean, `cargo test -p
deckcraft-collab-server` 10 unit + 0 in `main.rs` + 4 commit + 4 gates (LibreOffice present, no
skip) + 12 socket (four of them new: preflight, an allowed GET, a foreign origin, an empty
allowlist) + 0 doctests = 30 passed, 0 failed, 0 ignored, EXIT=0.


Re-run on 2026-11-09 after the browser link (`66d4658c`, on `agents`): `fmt --check` clean,
`collab` 129 + 8, `collab-proto` 22, `collab-server` 10 unit + 4 commit + 4 gates + 6 socket,
`collab-web` 15, `deckcraft-web` 3 (the headless page-host scenario), `ui-egui collab::` 17,
`ui-egui` full suite 509 + 1 failed (`ime_tests::korean_composes_in_the_notes_pane`: the build
host has no `craft-fonts` and no Korean-capable font, so egui panics on a missing glyph — an
environment gap, not this change), `deckcraft-engine` not re-run (untouched); clippy over
`collab`, `collab-proto` (unchanged), `collab-web`, `ui-egui` and `deckcraft-web` with
`-D warnings` clean; `xtask layers` 32 crates; `xtask wasm` 26 crates;
`cargo check -p deckcraft-web --target wasm32-unknown-unknown` ok; `trunk build` (0.22.1) ok in
4m7s cold (3m44s of it rustc), `dist/web` with a 49 MB `_bg.wasm` and the JS glue.

Re-run on 2026-11-09 after the table-options slice (`05abfe28`, on `agents`, rust 1.99.1):
`cargo fmt --all -- --check` clean, `cargo clippy --workspace --all-targets -- -D warnings`
clean, `collab-proto` 32 (the new table wire assertions extend an existing test), `collab` 218 +
8, `collab-server` 10 unit + 4 commit + 4 gates (2.45 s with `--test-threads=1`, LibreOffice
skipped on the build host) + 7 socket, `collab-web` 15, `deckcraft-web` 6, `ui-egui collab::` 20,
`ui-egui` full 517 passed + 4 ignored + the known
`ime_tests::korean_composes_in_the_notes_pane` environment failure, `deckcraft-engine` 585 + 1
ignored, `xtask layers` 32 crates with no violations, `cargo check -p deckcraft-web --target
wasm32-unknown-unknown` ok. `cargo test --workspace --no-fail-fast` is green except the two known
host-without-`craft-fonts` failures (`render
tests::east_asian_vertical_text_stands_ideographs_upright` and `ui-egui
ime_tests::korean_composes_in_the_notes_pane`). `git status` is clean after the docs commit;
nothing was pushed, CI was not touched, and PR #1975 is unchanged.

Re-run on 2026-11-09 after the SmartArt slice (`d6c2df6c`, on `agents`, rust 1.99.1):
`cargo fmt --all -- --check` clean, `cargo clippy --workspace --all-targets -- -D warnings`
clean, `collab-proto` 33, `collab` 223 + 8, `collab-server` 10 unit + 4 commit + 4 gates (2.43 s
with `--test-threads=1`, LibreOffice skipped on the build host) + 7 socket, `collab-web` 15,
`deckcraft-web` 6, `ui-egui collab::` 20, `deckcraft-engine` 586 + 1 ignored, `xtask layers` 32
crates with no violations, `cargo check -p deckcraft-web --target wasm32-unknown-unknown` ok. The
new tests cover two replicas converging on one whole diagram value (layout, outline, style and
colours in one step), its undo restoring the previous diagram **and the generated children with
their own ids** and its redo reapplying it on both replicas; an undo refused `undo-conflict` after
another participant edited the same graphic; a step addressed to a plain shape refused `invalid`
with no history step; the normaliser running the engine's plan for `setNodes`, `promote`,
`layout`, `style`, `colors`, `addNode` and `removeNode` (addressing the group through a child's
id, keeping the outline through a layout change), and refusing an unknown layout/style/colour, a
node that does not exist, a plain shape, an empty selection and the structural/convert commands
this slice does not carry; the engine regenerating from a whole value, the exact group restore
(children and ids) and refusals (unknown layout/style/colours, > 4096 nodes, a value that is not a
diagram, a group image that names another shape); and the same step and its undo through the
in-memory hub on two replicas. `git status` is clean after the docs commit; nothing was pushed, CI
was not touched, and PR #1975 is unchanged.

## What is NOT done (ordered by production impact)

1. **Command surface beyond the new slices.** Fill, line, 17 `format.*` commands over whole shape
   text, over N table cells of one table (`FormatSet { addresses: [{ cell: [row, col] }] }`,
   applied to each cell's own body), over a character range of a shape or cell body and at a caret
   (`addresses: [{ cell?, range | caret }]`, rebased, exact-body undo), `slide.duplicate`,
   `transition.set`, table cell text, table
   row/column edits, the rest of the table surface (`table.style`, the six style options as one
   final set, `table.cellFill` over an
   explicit cell list, `table.merge`/`table.split` and `table.columnWidth`/`table.rowHeight`, each
   with the whole table as the undo image), `animation.add`/`animation.remove` (one shape's whole
   step list),
   `animation.set`/`animation.timing`/`animation.move`/`animation.options`/`animation.clear` and
   the Animation Painter (one slide's whole ordered sequence) and
   slide notes text (`notes.set`/`notes.insert`/`notes.delete`, address `(slide, notes body)`) and
   document fonts (`font.create`/`font.set`/`font.remove`, the whole entry as the undo image) are
   collaborative now, and so are charts (`chart.type`/`chart.data`/`chart.options` and
   `insert.chart` as one whole `Chart` value, the whole chart shape as the undo image), and so is
   inserting a 3-D model (`insert.model3d`, a binary glTF on the media channel), inserting a
   SmartArt graphic (`insert.smartArt` as one whole `Diagram` value, the whole inserted shape as
   the undo image) and the three SmartArt conversions (`smartArt.fromText`/`toText`/`toShapes`,
   each one structural step with the whole source shape as its undo image), and a node's picture
   (`smartArt.picture` as one final value — the media channel's digest and the picture's place,
   with the whole group as the undo image), a 3-D model's camera (`model3d.camera` as one
   final view — `position`/`target`/`up` in EMU and `fov` in degrees, or `null`, with the whole
   model shape as the undo image and `am3d:camera` written into the slide part) and its scene
   (`model3d.scene` as one final scene — one light with a type, a non-zero direction, an
   intensity and a spot's cone angle, plus the environment with an ambient level and an optional
   sRGB background, or `null`, with the same whole model shape as the undo image and
   `am3d:scene` written into the slide part); `smartArt.nodes`
   is a read-only query the server answers against the authoritative document. Still refused:
   relative effects stay off the wire by design. The design
   rule stands: only
   commands whose result is a deterministic function of the authoritative state, expressed as
   final values, go on the wire; a raw `execute` is not safe (toggles and relative edits
   diverge). The command surface of this line is complete: a model's camera and scene are both in.
2. **Media refinement.** Pictures, video, audio, font faces and 3-D models are all in the media
   allowlist, each with its own budget; `GET /media` normalises the path and answers with the
   content type the signature implies. The channel's deployment gap for a browser page is closed:
   with `DECKCRAFT_CORS_ORIGINS` set the server answers an admitted origin with
   `Access-Control-Allow-Origin`/`Vary` and a preflight for the media and session endpoints, and
   sends no `Access-Control-*` header to a foreign origin. What remains is the transport below
   the browser: the desktop `wss://` path has a TLS client with system roots (a private CA must be
   in the store or `SSL_CERT_FILE`), and the ingress still terminates TLS.
3. **Rich runs are in; cell formatting, N-cell ranges and carets are done.** Shape text,
   table-cell text and slide notes are addressed and rebased. A notes edit addresses
   `(slide key, notes body)`, is refused for an unaddressable slide before any write, keeps the
   receiving user's selection, and its undo image is the slide's whole text body. A `format.*`
   command addresses a list of bodies (`FormatSet { addresses }`): a table cell is one address,
   runs against that cell's own body rather than the table's empty shape text, is refused before
   any write for a cell the table does not have (and for a table with no address), and its undo
   is the whole body of exactly that cell (`cell_text`, `shape.setField { field: cellText }`) with
   an `undo-conflict` when someone else changed that cell after the step — a neighbouring cell is
   never touched. A selection of several cells is one operation with N addresses, one per cell:
   the same checks apply to every address, an empty list, a repeated cell and a range past the
   table are refused `invalid` before any write with no version move, the undo restores exactly
   those cells' bodies in one `cells_text`, and a foreign edit of any addressed cell is
   `undo-conflict`. A range (`addresses: [{ range: {from, to} }]`) addresses whole-body character
   offsets, rebased against `text.insert`/`text.delete`, and is refused `invalid` before any write
   for an empty range or one past the body; a caret (`addresses: [{ caret }]`) in a shape's body is
   its own address and the single-user command decides what it touches (a run command widens it to
   its word, a paragraph command takes the paragraph), so the replica never widens it. Undo of
   either restores the whole body it changed (the format splits runs at the range's ends), so a
   later edit of that body by someone else is `undo-conflict`. The other table commands (style,
   the six style options as one final set, cell fill, merge/split, sizing) are in now as final
   values with the whole table as the undo image (item 1).
4. **Structural undo is covered; size limits remain.** A group's fill/outline (one image per
   changed member, one `shape_fields`), `slide.delete`/`slide.new`/`slide.duplicate` (whole-slide
   image, `slide_restore`), `shape.insert`/`insert.picture`/`insert.video`/`insert.audio`/
   `edit.delete` (whole-shape image, `shape_restore`/`shape_delete`), `arrange.group`/
   `arrange.ungroup` (whole-subtree images on both sides, one `shape_restore{items, remove}`) and
   `transition.set` (the whole transition, `slide_transition`) are undoable. Merge operations
   (move, bounds, text set/insert/delete, fill/line/format, group appearance) are covered too. A slide image is capped at 192 KiB and a shape restore at 192 KiB /
   64 images, so an undo of an unusually large object is refused `invalid` instead of breaking the
   wire limit.
5. **Cabinet callback / host bridge.** The server side and the desktop Save are wired:
   `POST /v1/sessions/{id}/commit` (service token) builds the `.pptx` and posts it to
   `COLLAB_CABINET_URL` (`{fileId, name?, bytes, sha256, version, expectedVersion?}` →
   `{version, etag}`); a cabinet `409` is the caller's `409`, the original version is kept, and the
   accepted `etag` is stored in `meta.json` so a restart still refuses an external change. On the
   desktop, Save in a live session sends `ClientMsg::Commit` with the cabinet version the session
   was opened from (or the one the last commit was accepted with, from `meta.json` on the server
   side): one commit in flight, a repeated Save is the same request, the document stays dirty until
   `commit-result`, a refusal keeps the edits and offers Save a copy (ADR-0009 §4), and Save
   As/Export stay local. What remains is outside this repository: the cabinet's endpoint
   (`INTEGRATION.md` §5.2) and the host-bridge messages
   (`session`/`presence`/`commit`/`committed`). Without
   `COLLAB_CABINET_URL` a deployment only builds verified bytes in the process.
6. **Browser transport is in (`collab-web`); deployment is not.** `apps/deckcraft-web` opens a
   session from `deckcraft:session`, dials it with the page's `WebSocket`, posts
   `session-state`/`presence`, asks for a commit on Save and records `committed`. What remains is
   outside the code: the cabinet's commit endpoint (`INTEGRATION.md` §5.2) and a token-renewal
   message (a fresh token today means a new `session` message). A page on another origin can use
   the channel once its origin is listed in `DECKCRAFT_CORS_ORIGINS` (the sync server answers the
   media and session requests with `Access-Control-Allow-Origin` and the preflight).
7. **Soaks are in; CI still skips LibreOffice.** Five soaks and gate scenarios run in
   `tests/gates.rs` (see §8): the 26-edit three-client soak, the big-deck soak — 322 edits by three
   editors over a 120-slide deck with media, a viewer rebuilt from the 11.8 MiB base sent in 48
   chunks, a fifth participant whose socket drops mid-chain and resumes at the missing chunk, and a
   commit at the head that reimports with every replica's values — and the HTTP-base gate: a base
   above the inline limit that the chain cannot carry, fetched in one request by a viewer below the
   compacted journal (§A base above the inline limit is one HTTP request). What remains is running
   the LibreOffice gate in CI, where LibreOffice is not installed (there it skips itself, printing
   a line).
8. **Base transfer above the inline limit is done, and its transport is settled.** A client below
   a compacted journal is rebuilt from the native base and the key map the `snapshot` carries, and
   above `max_base_inline` (8 MiB, `COLLAB_MAX_BASE_INLINE`) the same snapshot offers two ways to
   get it: one HTTP request (`base.http`, the media reader's pattern, RFC §4.5.2) and a chain of
   chunks over the session socket (`base.chunks`, `base-chunk`, RFC §4.5.1). The desktop link
   installs a fetch (`WsTransport::base_fetcher`) and rebuilds in one round trip whatever the deck
   weighs; a build without an HTTP side keeps the chain byte for byte as it was. Both check the
   announced size and digest before the replica is replaced, and both end at the head through the
   same `take_ready_base` path. Ceilings: `BaseHttp::MAX_LEN` (256 MiB) on a fetched base, 8 MiB
   per chunk / 4096 chunks / 64 MiB assembled on the client, `COLLAB_MAX_BASE_CHUNK`/
   `COLLAB_MAX_BASE_CHUNKS` on the server, and a validated relative path and hex digest on both
   sides. Only a base above 256 MiB, or a room whose file id cannot be a path segment, is named
   with no way to fetch it; the client then reopens the cabinet's file — the fallback, not the
   expected path. Nothing is left on this line.
9. **Upstream PR head.** The PR is open from the fork; nothing was pushed to
    `storytold/deckcraft`. Both branches were rebased onto `main` (§ Rebase onto origin/main), so
    the publish was a `--force-with-lease`; that push is done. At 2026-11-12T05:44Z the fork head,
    the PR head and the local branch head were `ebbecf08` (the `origin/main` merge with the
    video-decode drop, on top of the IME slices), with `MERGEABLE` / `UNSTABLE`; the four runs on
    that head were created 05:28:24Z and are all `completed / action_required`, so GitHub's rollup
    carries no executed job (approval is the maintainer's step; see §PR publication). On top of
    `ebbecf08` the local branch has unpushed commits — `42544987` (the HTTP base slice, code,
    tests and RFC) and this docs commit — and a push is a person's step.
    **#1998 carries no unique code:** `git rev-list --left-right --count collab/pr15...collab/pr11`
    is **32/4** (the four are `cf9b71cd` and three `origin/main` merges), and the fix older notes
    credited to it — the empty-diagram fallback setting `picture_place: None` in
    `crates/pptx/src/dgm.rs` — is already on `collab/pr15`, at `crates/pptx/src/dgm.rs:528`, so
    nothing "moves over as its own slice" and the older **24/9** count is stale. #1998
    (`abffb229`) and #1975 (`6109fe1c`, `CONFLICTING` / `DIRTY`) are superseded by #2012; a person
    closes them, not an agent session.

## How to run locally

```bash
# server
export COLLAB_SERVICE_TOKEN=dev-service
export COLLAB_JWT_SECRET=dev-secret-at-least-32-bytes
export COLLAB_STORE_DIR=/tmp/deckcraft-collab
cargo run -p deckcraft-collab-server

# create a room (bytes optional: a native/pptx base64)
curl -s -X POST localhost:8080/v1/sessions \
  -H 'Authorization: Bearer dev-service' \
  -H 'Content-Type: application/json' \
  -d '{"fileId":"demo","app":"deckcraft"}'

# desktop client
DECKCRAFT_COLLAB_URL=ws://127.0.0.1:8080/v1/sessions/demo/ws \
DECKCRAFT_COLLAB_TOKEN=<jwt> \
DECKCRAFT_COLLAB_SESSION=demo \
cargo run -p deckcraft

# the §8 gates (the LibreOffice half needs `soffice` and `pdfinfo` on the machine that runs it)
cargo test -p deckcraft-collab-server --test gates -- --nocapture --test-threads=1

# the browser build (trunk 0.22.1, wasm32-unknown-unknown)
cd apps/deckcraft-web && trunk build           # dist/web
# the page-host scenario, headless (no socket: an in-process Hub is the server)
cargo test -p deckcraft-web
```

The cabinet's token carries `aud: "office-sync"`, `sub`, `role`, `fileId`, `app: "deckcraft"`,
`sessionId` and `exp`. The server requires the signature, `exp`, `sub` and `aud`; it checks `app`
and `fileId` when the token names them, and does not check `sessionId`. Local runs may use HS256;
for a production cabinet use an ES256/RS256/Ed25519 PEM in `COLLAB_JWT_PEM` and a short-lived
token from the cabinet.

## Next session: start here

1. Run the verification table above; if the tree is clean, continue on `collab/pr15` (or branch
   `collab/pr16` from it).
2. **PR #2012 is open and ready from `copoka/deckcraft:collab/pr15`** — the delivered head is
   `1d92f763` (the second `origin/main` merge, pushed 2026-11-12T12:13Z): at 2026-11-12T13:00Z the
   fork head, the PR head and the local branch head are that commit, so every review pass, the
   STATUS sync, the presence heartbeat (`fd5914e8`), both `origin/main` merges, the op_id slice,
   the IME slices and `42544987` (the HTTP base slice) are delivered and nothing on the branch is
   unpushed; only this STATUS commit is new. The §8 gate on `1d92f763` is green (§ Session log,
   2026-11-12 — the presence heartbeat is already on the branch; 6 gates in 19.15 s, the
   LibreOffice half skipped on that host). The paragraph below keeps the 05:44Z reading of the same PR; read `ebbecf08`
   there as "the head at that time", and the live value is `gh pr view 2012 --json headRefOid`.
   `gh pr view 2012` reports `MERGEABLE` / `UNSTABLE`; **no workflow run has executed on any of
   these heads** — the four runs on `38a20a9c` (created 2026-11-11T11:31:32Z) are
   `completed / action_required` and the runs for the later heads wait for the same approval, so
   the next step for a person is that approval and then review and merge. The host gate on
   `5a105c61`
   (`.craft/logs/gate-deckcraft-2012-5a105c61.log`) is green on `fmt`/`pdf-deps`/`clippy` and stops
   at `test` only on the known host-font render test
   (`render::tests::east_asian_vertical_text_stands_ideographs_upright`, no CJK glyph on the host);
   the local TASK.md §8 gate is green on `aaecc6db`: fmt clean,
   clippy `-D warnings` for `collab` and `collab-server` EXIT=0, `collab` 294 + 8,
   `collab-server` 19 unit + 5 commit + 5 gates + 18 socket, `ui-egui --lib` 526 + 4 ignored with
   only the known host-font failure; the `--workspace` gate (engine 612, pptx 70 + 5 + 146,
   `collab-proto` 42, `xtask` pdf-deps/assets/layers/wasm) is the one recorded for `c9803277` and
   was not re-run for a change confined to `crates/collab`. **The local §8 gate is green again on
   `51643c59`** (engine, collab, collab-server, collab-proto and collab-web plus `xtask wasm`;
   numbers in §A composition is settled before anything leaves the client) — this slice touched
   `crates/engine`, `crates/collab-proto` and `crates/ui-egui` as well, and it needs no push to be
   useful: `51643c59` is local. The **workflow** gate still has not run:
   CI, House
   rules, Nix and SonarCloud on `38a20a9c` are `action_required` (created 2026-11-11T11:31:32Z)
   and wait for a maintainer's approval; the runs for `fd5914e8` wait for the same approval.
   CodeRabbit's second review is closed, the third (`fd1ee71a`) found nothing in code, the fourth
   (`5852391245`, on `4a64e764`) is delivered in `9ad80547`, and the fifth (`5855491108`,
   submitted 07:49:58Z on `9ad80547`) is delivered in `891187f5` with its docs in `38a20a9c`; the
   next step for a person is the maintainer's approval of the runs, then review and merge.
   #1998 (`abffb229`, `CLEAN`) and #1975 (`6109fe1c`, `CONFLICTING` / `DIRTY`) are superseded by
   #2012 and stay open until a person closes them — **do not close them from an agent session**.
   #1998 carries no unique code (`pr15...pr11` = 32/4) and its credited `picture_place` fix is
   already on pr15 at `crates/pptx/src/dgm.rs:528`. Do not touch CI and do not mention agents.
3. The browser side is in (`66d4658c`): `deckcraft:session` → the page's `WebSocket`,
   `session-state`/`presence` back, Save → `deckcraft:commit`, `committed` recorded, media over
   `fetch` with the session token. The desktop side is in too (`collab/pr14`): Save in a live
   native session sends `ClientMsg::Commit`, one commit in flight, and the local file is no longer
   written. The cabinet must still implement its side:
   `POST /api/v1/internal/office/sessions/{sessionId}/commit` (`INTEGRATION.md` §5.2, answers
   `{version, etag}`, keeps the previous version); the server already posts the bytes, refuses on
   `409` and stores the accepted version. For a page on another origin, list that origin in
   `DECKCRAFT_CORS_ORIGINS`: the server answers `/media/{sha256}` and the session endpoints with
   `Access-Control-Allow-Origin`/`Vary` and a preflight (`f996865d`).
4. Extend the command set in vertical slices with the same pattern: payload → normalize →
   adapter run → confirmed command → test on two clients. Document fonts, the whole table
   surface, SmartArt (the outline, layout, style and colours as one whole `Diagram` value,
   `insert.smartArt` as one whole diagram with the whole shape as its undo image, the three
   conversions as one structural step each, and a node's picture as one final value with its
   place), charts (type,
   data, options and insertion as one whole `Chart` value) and the insertion of a 3-D model
   (`insert.model3d`, a binary glTF on the media channel, drawn as its poster or a stand-in cube)
   are done; `smartArt.nodes` is answered by the server as a read-only query, and a model's
   camera and scene are both in. Optimistic preview is in too (`collab/pr15`,
   `DECKCRAFT_COLLAB_PREVIEW`). Nothing is left on this line; the next slice comes from items
   2–3 (media transport and the cabinet callback).
5. Undo is covered for move/bounds/text (shape, table cell and slide notes)/fill/line/format
   (shape body, one table cell, N cells of one table, a run range or a caret — each with an exact
   body image)/transition/animation (a shape's step list for add/remove, a slide's whole sequence
   for set/timing/move/options/clear/painter) steps, a group's fill/line, slide
   insert/delete/duplicate, shape insert/delete (a SmartArt insertion too), group/ungroup, table
   row/column edits (whole-table image), the whole table for style/options/fill/merge/split/sizing,
   a document font entry, a SmartArt outline/layout/style/colour or node-picture step (the whole
   group), a
   SmartArt insertion (the whole shape), a SmartArt conversion (the whole source shape) and a
   chart type/data/options step (the whole chart shape) and a model camera or scene step (the
   whole model shape). Every command on the wire has its undo image; relative effects are not on
   the wire by design.
6. Keep the §8 gates green (`cargo test -p deckcraft-collab-server --test gates -- --nocapture --test-threads=1`); they now cover the long run (322 edits, media, a chained base, a resume mid-chain, and the same 11.8 MiB base *offered* over HTTP), a base fetched over HTTP by a viewer below the compacted journal (2.4 MiB, one request, key map, head, no reopen) as well as the three-client soak, so the remaining gate work is running LibreOffice where it is installed.
7. Update `production.md` and this file at the end of the session; post a bus status with the
   commit and test numbers.
8. **Closed in `51643c59`: the IME finding** (`5866843261` on #1998). With `collab_queue` set,
   `Session::execute_unguarded` queued the command and returned before `settle_composition()`, and
   the composition's own commands were queued too, where the normaliser has no form for them — so a
   text typed through an input method could not reach a room at all, and a command sent while a
   composition was open would have been planned against a document that did not yet hold the
   composing text. The slice runs the input method's commands on the replica, settles the
   composition before any other command is queued, and hands the room the body as one whole value
   (`text.setBody`, one undo step). The rule, its refusals and the limits are §A composition is
   settled before anything leaves the client, `rfc.md` §10.1 and `production.md`. **The next code
   item from that line is closed in `b314feef`:** `text.deleteSurrounding` outside a composition no
   longer has to be refused in a room — the engine resolves the range it removes and the normaliser
   sends it as one `TextDelete` (`NotesDelete` for notes, the cell kept), with the body before the
   step as its undo image; only a non-empty selection with text deleted on both sides stays refused
   at the gate (two ranges, one operation), and the two-replica tests are in §A and the session log.

## A base above the inline limit is one HTTP request

A client that missed the compaction was rebuilt from a base that the snapshot either carried
inline (up to `max_base_inline`, 8 MiB) or offered as a chain of chunks over the session socket. A
deck with hundreds of megabytes of media therefore took hundreds of round trips, and a base deeper
than the configured chain was only named: the client had to reopen the file from the cabinet. This
slice gives the same base a second, cheaper route.

**Wire.** `BaseSnapshot` gains `http: {path, sha256, len}` (RFC §4.5.2), announced **beside**
`chunks` in the same snapshot whenever the bytes do not travel inline. `BaseHttp::is_well_formed`
accepts a relative path only: it starts with `/`, carries no `..`, `?`, `#`, control character or
whitespace, names a 64-character hex digest and a size inside `BaseHttp::MAX_LEN` (256 MiB). An
absolute URL is never sent and never followed, and `PROTO_VERSION` stays 1: a client that does not
know the field sees exactly the frame it saw before.

**Server.** `GET /v1/sessions/{id}/base/{sha256}` repeats the media reader's pattern: the session
token names the caller (a viewer may read it — the base is the document, not an edit), the bytes
are addressed by digest, the answer is `application/vnd.deckcraft.native`, and the path is checked
before anything is read. A token that names another `fileId` is a `401`; a base the room no longer
holds — compaction moved on, or the room is not loaded — is a `404`, and the client asks for the
session again rather than installing bytes the room does not have. `base_http_total` and
`base_http_bytes_total` count the reads. `DocSession::base_http` refuses to name a request for a
file id a path would split (`/`, `\`, `?`, `#`, a control character): that room's base stays on the
chain, where the id never appears.

**Client.** `ClientSession::set_base_fetcher` takes the host's fetch (the desktop link builds it in
`WsTransport::base_fetcher` and `CollabUi::start_native` installs it). With a fetch and no inline
bytes, the snapshot is answered by one synchronous request; the client checks the size and the
digest before anything is installed, then hands the bytes and the snapshot on through the same
`take_ready_base` path the chain uses, so the host replaces its replica, takes the key map
(`restore_keys`) and replays the tail **once**, with no second catch-up. A wrong size or digest is
not installed: the reason lands in `last_error` and the chain the server announced beside the
request is the fallback — and where neither exists, the old "reopen from the cabinet" message
stays. The host had to learn one thing: a snapshot whose base was installed in the same call must
not be read again as "the client has no bytes" — `CollabDoc::on_msg` now returns after
`install_base`, which is what kept the desktop from reporting a reopen failure for an answer that
worked.

**The fetch is synchronous, in the host's pump** — the same place a media fetch already runs
(`CollabDoc`'s `media_fetcher`), with the link's 10 s read and write timeouts as the bound. A base
is the largest object fetched this way (pictures, audio and 3-D models are up to 32 MiB), so the
cost is one blocking request of the deck's size, not a per-chunk round trip; an asynchronous fetch
would need the host to pump a new message kind and is not part of this slice.

**What did not change.** The chain, its resume-at-the-missing-index rule, the 8 MiB/4096/64 MiB
ceilings, `base-chunk` staleness and the "named without bytes" fallback are untouched: a client
without an HTTP side behaves exactly as before, which is what the big-deck soak still proves.

**Verified on `42544987` (host `agents`).** `cargo fmt --all -- --check` clean; `cargo clippy
--workspace --all-targets -- -D warnings` EXIT=0; `collab` 305 lib + 8 two_clients; `collab-proto`
43; `collab-server` 20 unit + 5 commit + 6 gates (20.8 s, LibreOffice skipped on the host) + 18
socket; `collab-web` 15; `deckcraft-web` 6; `cargo xtask wasm` ok (17 crates); `ui-egui --lib` 527
passed + 4 ignored with the one known host-font failure
(`ime_tests::korean_composes_in_the_notes_pane`, no CJK glyph on the host — an environment gap, not
this change).

**The new §8 gate** is `a_base_above_the_inline_limit_is_fetched_over_http_without_reopening_the_file`:
a 24-slide deck with media, `COLLAB_MAX_BASE_INLINE` lowered to 256 KiB and the chain made unable
to carry the base (`max_base_chunk` 256 KiB, `max_base_chunks` 1), 12 edits and two compactions, then
a viewer joins at version 0 and is rebuilt from a 2.4 MiB base in **one request, 55–72 ms** over
the runs — no
chunk, no reopen — takes the key map, applies the next edit by key and converges with both
editors; the gate also checks the endpoint's `401`/`404` answers and that the metrics counted one
HTTP base and zero chunks. The big-deck soak now additionally asserts that its 11.8 MiB base is
**offered** over HTTP while it still travels as 48 chunks to a client without one.

## One operation, one identity

`normalize` turns one UI command into a list of operations. Until `aaecc6db` every operation of a
command carried the single `op_id` the caller passed in (the `make` closure in
`crates/collab/src/local.rs`), while `DocSession::apply` dedups on that id and `Hub::receive`
broadcasts the stored `Applied` for a repeat (`crates/collab/src/session.rs`, `crates/collab/src/hub.rs`).
A command naming several objects — `shape.move` with `ids`, `shape.setBounds` with `ids`,
`slide.delete` of several slides (`slide_delete_ops`) — therefore ended as one version: the room ran
the first operation and answered the following ones with it.

Both consequences were silent. Every replica — the author's included, because a multi-operation
command never previews — showed only the first change; and a resend could not repair the rest,
because the ids were the same. The preview was never wrong here: `submit_prepared` previews only when
`ops.len() == 1`, and that single operation's id is the command's id.

**The rule.** One operation is one identity. The `make` closure numbers the operations of its
command: the first keeps the caller's id, every later one is `<id>#<n>` in emit order. The numbering
counts from the first operation a command emits and is deterministic, so normalising the same command
twice (a deferred command, a retry) gives the same ids and a resend is still one operation per id —
dedup by `opId` is untouched, and `ops[0].op_id` remains the id the optimistic preview and the
sender's settle are keyed on.

**Structural steps go one at a time.** `Adapter::apply` accepts a `structural` operation only at the
head (ADR 0004 §2). With unique ids, the second `slide.delete` of one command would arrive with the
base the first one already moved: `rejected{stale, retryable}` and a `resync` for every extra
operation. `ClientSession::next_outgoing` therefore does not send a structural operation while an
operation it sent before it is unanswered — the command's structural steps leave one answer at a
time, in order, each with the head. A structural step from *another* participant still takes the
stale + resync path, which is the ADR's intent.

**What changes for the user.** A multiple-selection move or resize is now N versions and N undo steps
rather than one silent no-op: the correct result, one step per object. A drag is unaffected — the UI
sends it as `shape.setBoundsMany`, one operation with every final box — and so are
`arrange.align`/`arrange.distribute`, which are carried as one final-bounds step. Grouping a
multi-selection nudge into one undo step would need a history hint on the wire, which the protocol
does not have: it changes nothing about convergence and is not part of this slice.

**Verification** (host `agents`, on `aaecc6db`):

```bash
cargo fmt --all -- --check                              # clean
cargo clippy -p deckcraft-collab -p deckcraft-collab-server --all-targets -- -D warnings   # EXIT=0
cargo test -p deckcraft-collab -p deckcraft-collab-server
# collab: 294 lib + 8 two_clients
# collab-server: 19 unit + 5 commit + 5 gates + 18 socket
cargo test -p deckcraft-ui-egui --lib                   # 526 passed, 4 ignored, the one known
                                                        # host-font (CJK) failure: no ㄴ glyph
```

New tests: `local::tests::a_move_of_a_multiple_selection_gives_each_operation_its_own_id`,
`a_resize_of_a_multiple_selection_gives_each_operation_its_own_id` and
`deleting_several_slides_gives_each_operation_its_own_id` (ids, targets, deterministic repeat);
`hub::tests::a_move_of_a_multiple_selection_applies_every_operation_on_both_replicas` (two replicas,
two versions, both shapes moved, a resend of the same ids deduplicated),
`deleting_several_slides_applies_every_operation_on_both_replicas`, and
`client::tests::a_structural_operation_waits_for_the_answer_to_the_one_before_it`. Without the fix the
two hub tests fail with the room at version 1 and one shape/slide changed. `rfc.md` §4.3 now states
the rule.

## A composition is settled before anything leaves the client

An input method's composition (the preedit of #101: Korean, Japanese, Chinese) is the one edit a
shared session makes **on the replica, before the room**. It belongs to one text body — a shape's
own body, one table cell of it, or a slide's notes. `text.compose` and `text.commit` therefore run
locally even with `collab_queue` on; they never enter `take_pending_commands()`, because a preedit
is not text yet and every replica does not need to see every keystroke of a conversion.

**The rule.** No operation leaves the client before the composition is settled.

- Every other command — a click, a key, undo, a ribbon action — settles the composition first
  (`Session::settle_composition`) and is queued only after it. Before this slice the queue branch
  returned first, so the command was planned against a document that did not yet hold the
  composing text: the operation carried a text its author never saw, and the undo step that
  followed put back something else.
- The settle itself queues exactly **one** command: the body it typed into, as a whole value —
  `shape.setField { field: "text" }` for a shape body, `shape.setField { field: "cellText", cell }`
  for a table cell, `slide.notes` for a slide's notes. The normaliser turns it into one
  `text.setBody` operation (`body`, `cell?`), the server runs it through `shape.setField`, and its
  undo image is that body before the step. A whole value converges on every replica, keeps the
  runs, their formatting and the paragraph properties (which `text.set` would flatten to the first
  run's), and gives the room one undo step.
- The address is captured where the composition began — the slide by id, the shape, the cell — so
  clicking to another slide between the preedit and the settle cannot send the text into another
  body.
- A second settle adds nothing and creates no second undo step: the second call finds no
  composition.
- Where the room has no operation to carry the text, the command is **refused before the first
  write**, with its reason and without moving the version: an ambiguous input-method deletion (see
  below) and any composition outside slides (a master or layout view is not synced).

**A deletion around the input point is not a composition.** Outside a composition
`text.deleteSurrounding` — a platform's press-and-hold or dictation asking for the characters around
the caret — removes an ordinary range of the body, so it does not run on the replica at all. The
engine resolves the range with the same walk `text.delete` uses (`crates/engine/src/cmd/text/ime.rs`:
`walk_backward`/`walk_forward`, characters or UTF-8 bytes, a paragraph break counting as one) and
the normaliser turns exactly that range into one `text.delete` (`TextDelete { from, to, cell? }`, or
`NotesDelete` for the notes pane), with the body before the step as its undo image: one version, one
undo step, the same step a typed `text.delete` makes, in a shape's body, in one table cell and in a
slide's notes. One operation carries **one** range: a collapsed caret with text on both sides is one
range spanning it, while a non-empty selection with text deleted on both sides is two ranges and is
refused at the gate — before the queue, with its reason, without an undo step and without touching
the revision — as are no text selection and a body that is gone. Inside a composition the deletion is
part of the pre-edit and leaves with the settle.

The rule lives in one function, so the single-user command and the normaliser cannot resolve
different characters: `text::surrounding_delete` returns the walked ranges, `delete_surrounding`
deletes them as before, and `text::shared_surrounding_delete` classifies the request for a shared
session (`Range`, `Nothing`, `Refused(why)`). `local::normalize` applies the same rule to the
replica's state as a second line of defence.

**Verified on `b314feef` (host `agents`).** `cargo fmt --all -- --check` clean; `cargo clippy -p
deckcraft-engine -p deckcraft-collab -p deckcraft-collab-server -p deckcraft-collab-proto -p
deckcraft-ui-egui --all-targets -- -D warnings` EXIT=0; engine 615 passed + 1 ignored (+ 6 + 8
integration); collab 298 lib + 8 two_clients; collab-server 19 unit + 5 commit + 5 gates (16.8 s,
LibreOffice skipped on the host) + 18 socket; collab-proto 42; collab-web 15; `cargo xtask wasm` ok
(22 crates); `ui-egui --lib` 527 passed + 4 ignored with the one known host-font failure
(`ime_tests::korean_composes_in_the_notes_pane`, no ㄴ glyph on the host — an environment gap, not
this change).

`ShapeField` and `CellText` stay the server's own undo images — the adapter still refuses a
client-sent one, because a restore must match the step it restores — and `text.setBody` is the one
payload with which a client sets a whole body. The wire carries the body as JSON, so the engine
decodes it into the model and bounds it (`MAX_FIELD_JSON`, 256 KiB) before the first write, like any
other hostile input.

**Limits, stated in `rfc.md` §10.1 and `production.md`.** A settled body must fit one operation
message (`COLLAB_MAX_MESSAGE_BYTES`, 256 KiB by default), so a pre-edit inside a body of roughly
100k characters cannot be shared: the server refuses the operation and the replica resynchronises.
A text step's undo image is that body's plain text (as for typing), so undo restores the text, not
the runs a composition typed into. A whole body is last-writer-wins, as `text.set` already was.

**Verification** (host `agents`, on `51643c59`, plus the docs commit on top):

```bash
cargo fmt --all -- --check                                   # clean
cargo clippy -p deckcraft-engine -p deckcraft-collab -p deckcraft-collab-server \
  -p deckcraft-collab-proto -p deckcraft-ui-egui --all-targets -- -D warnings   # EXIT=0
cargo test -p deckcraft-engine                               # 615 lib + 6 it + 8 show_paragraph_builds
cargo test -p deckcraft-collab -p deckcraft-collab-server -p deckcraft-collab-proto \
  -p deckcraft-collab-web
# collab: 295 lib + 8 two_clients
# collab-server: 19 unit + 5 commit + 5 gates + 18 socket
# collab-proto: 42
# collab-web: 15
cargo xtask wasm                                             # ok (22 crates)
cargo test -p deckcraft-ui-egui --lib                        # 527 passed, 4 ignored, the one known
                                                             # host-font failure
                                                             # (ime_tests::korean_composes_in_the_notes_pane,
                                                             # no ㄴ glyph on the host)
```

New tests: `tests::a_shared_session_settles_the_composition_before_it_queues_the_next_command`
(the pre-edit queues nothing; the settle queues the whole body with its bold run and the command
follows it; a repeated settle adds no operation and no undo step),
`tests::a_shared_input_method_deletion_outside_a_composition_queues_its_range_and_refuses_two`
(renamed and widened by the next slice: the word before the caret is queued as its range instead of
being refused, two ranges around a selection are still refused before the first write with the
revision and the undo history unchanged and nothing queued, and inside a composition the deletion
runs and leaves with the settle),
`tests::a_shared_composition_in_a_cell_and_in_the_notes_queues_that_body` (`cellText` with its
cell, `slide.notes` with the slide's index),
`hub::tests::a_settled_composition_reaches_both_replicas_as_one_whole_body_and_undo_restores_it`
(two replicas through the normaliser, the adapter and the in-memory hub: the room does not see the
pre-edit, both hold the author's text after the settle, one undo puts the original text back) and
`collab::tests::a_composition_in_a_live_session_leaves_only_when_it_settles` (the live UI path: no
operation while composing, one `text.setBody` on commit, nothing on a second settle). The proto
round-trip test now covers `text_body_set` with and without a cell. Without the fix the engine test
fails on its first assertion — the pre-edit is not applied at all and both the compose and the next
command end up in the queue — and the hub test fails because the commit is queued beside the
compose command, so the room never receives `한`.

## Session log

### 2026-11-12 — the presence heartbeat is already on the branch; verified on the head `1d92f763`

- **What was asked.** Fold the stranded `collab/presence-heartbeat` (`9c93ae66`, cut from
  `6e9d10e7`: "collab: repeat an unchanged presence before the room's TTL expires") into
  `collab/pr15` — rebase it, keep the TTL test, run the §8 gates and record the slice here.
- **Already on the branch: nothing to rebase.** The same change is `fd5914e8`, cherry-picked
  onto `1f78cadb` conflict-free (§2026-11-11 — the presence heartbeat delivered on top of
  `1f78cadb`). Compared region by region, the constant block (`PRESENCE_MIN_GAP_MS = 50`,
  `PRESENCE_HEARTBEAT_MS = 10_000`, both with their comments), `presence_due` and
  `collab::tests::an_unchanged_presence_is_repeated_before_the_room_forgets_it` are identical on
  `9c93ae66` and on `1d92f763` (`git show <rev>:crates/ui-egui/src/collab.rs` diffed region by
  region is empty), and `git log -S PRESENCE_HEARTBEAT_MS -- crates/ui-egui/src/collab.rs` finds
  exactly one commit, `fd5914e8`. Applying `9c93ae66` again would add a second copy of the same
  lines, so it was not applied. The chair's 09:45Z bus note already records `9c93ae66` as a
  duplicate of `fd5914e8`, not a loss. The stranded branch itself can be deleted by a person;
  this project does not delete branches (a destructive git operation).
- **Head and push.** At 2026-11-12T13:00Z the local head,
  `git ls-remote fork refs/heads/collab/pr15` and `gh pr view 2012 --json headRefOid` all named
  `1d92f763`, so every commit up to the second `origin/main` merge is delivered. This session
  changed no code and pushed nothing; the only new local commit is this STATUS entry.
- **Gate on `1d92f763`** (build host `agents`, rust 1.99.1): `cargo fmt --all -- --check` clean;
  `cargo clippy -j 4 -p deckcraft-collab -p deckcraft-ui-egui --all-targets -- -D warnings`
  clean; `cargo test -p deckcraft-collab --lib` 305 passed; `cargo test -p deckcraft-collab
  --test two_clients` 8 passed; `cargo test -p deckcraft-ui-egui --lib collab::` 26 passed, and
  the heartbeat test alone (`--exact`) passes; the TASK.md §8 gate `cargo test -p
  deckcraft-collab-server --test gates -- --test-threads=1` is **6 passed in 19.15 s** — the
  120-slide/media room and its commit, the three-session
  conflict/reconnect gate, the three-client soak with its base transfer, the 120-slide soak with
  the chained base and the HTTP base above the inline limit. The LibreOffice half **skipped
  itself**: `which soffice` prints nothing on the build host and the gate prints its skip line, so
  the `pptx` open/repair evidence for this head is the workstation run recorded in §8 above, not a
  host run. The heartbeat's own behaviour is unchanged from
  `fd5914e8` and is still the reference: an unchanged presence is repeated every 10 s inside the
  room's 30 s TTL, a moved one stays capped at 20 Hz, and the test drives the pump over an
  in-process `PipeTransport` and fails without the repetition.
- **Not done, deliberately.** No rebase of `collab/presence-heartbeat` (it would duplicate
  `fd5914e8`), no push, no branch deletion, no CI change. Workflow approval on `1d92f763` is the
  maintainer's step.

### 2026-11-12 — an input method's deletion outside a composition is one `text.delete`

- **What was asked.** The slice left one gap: `text.deleteSurrounding` outside a composition was
  refused in a room ("an input method's deletion outside a composition is not shared",
  `crates/engine/src/lib.rs:748`) because it had no wire form. Normalise it into the `TextDelete`
  range it really removes — the offsets the engine already computes — as one operation with the
  same undo image `text.delete` has, notes and table cells included; an ambiguous address (no
  selection, several ranges) stays a refusal with a reason before the first write. Tests on two
  replicas: a word deleted through the input method reaches the neighbour as `TextDelete`, undo puts
  it back in one step, and a composition over that deletion does not lose the body.
- **What changed (code `b314feef`).** The walk in `delete_surrounding` moved into
  `crates/engine/src/cmd/text/ime.rs::surrounding_delete`, which returns the resolved
  `SurroundingDelete { start, end, from, to, before, after }` and its `span()`: the removed
  characters as one contiguous range, or `None` when they are two ranges with a non-empty selection
  between them. `delete_surrounding` (`cmd/text/caret.rs`) now deletes exactly those ranges, so the
  single-user path is unchanged and both paths resolve the same characters.
  `shared_surrounding_delete` classifies the request for a shared session (`Range`, `Nothing`,
  `Refused(why)`), and `Session::execute_unguarded` uses it at the input-method gate: `Range` falls
  through to the queue (it is no longer in the "runs on the replica" set outside a composition),
  `Nothing` answers `{deleted: 0}` as a single-user session does, `Refused` keeps the refusal before
  the first write with its reason. `local::normalize` gained the `text.deleteSurrounding` branch:
  it resolves the same range against the replica and sends one `TextDelete { from, to, cell? }`
  (whole-body character offsets, `position_offset`) or `NotesDelete` for the notes pane, and refuses
  the two-range/no-selection/gone-body cases as a second line of defence. `apply` already ran a
  `TextDelete` through `text.delete` on a selection, so the neighbour's step and its undo image are
  the ones a typed deletion has without new wire types.
- **Tests.** `tests::a_shared_input_method_deletion_outside_a_composition_queues_its_range_and_refuses_two`
  (replaces the refusal test: one range queued with the revision and the undo history unchanged, a
  selection with text on both sides refused with its reason and nothing queued, `{deleted: 0}` for
  nothing, a composition still keeping its deletion local);
  `local::tests::an_input_methods_deletion_normalises_to_the_range_it_removes` (a shape body, a
  table cell with its address, the notes as `NotesDelete`, the two-range refusal, a caret with text
  on both sides as one range, nothing as no operation);
  `hub::tests::an_input_methods_deletion_outside_a_composition_reaches_the_neighbour_as_one_delete`
  (two replicas through the normaliser, the adapter and the hub: the neighbour gets exactly `Beta`
  removed, one undo step puts it back, version 1 then 2) and
  `a_composition_over_an_input_methods_deletion_keeps_the_body` (the settle after the deletion
  carries `Alpha\n한`, both replicas agree, the composition's own undo step leaves the deletion
  standing). The ime unit tests of `delete_surrounding` (chars, bytes, composition boundaries, the
  one undo step) pass unchanged on the refactored walk.
- **Gate (host `agents`, on `b314feef`).** `cargo fmt --all -- --check` clean; `cargo clippy -p
  deckcraft-engine -p deckcraft-collab -p deckcraft-collab-server -p deckcraft-collab-proto -p
  deckcraft-ui-egui --all-targets -- -D warnings` EXIT=0; engine 615 passed + 1 ignored (+ 6 + 8
  integration); collab 298 lib + 8 two_clients; collab-server 19 unit + 5 commit + 5 gates (16.8 s,
  LibreOffice skipped on the host) + 18 socket; collab-proto 42; collab-web 15; `cargo xtask wasm`
  ok (22 crates); `ui-egui --lib` 527 passed + 4 ignored with the one known host-font failure
  (`ime_tests::korean_composes_in_the_notes_pane`, no ㄴ glyph on the host — the same environment gap
  recorded before, not this change). `rfc.md` §10.1, `production.md` (the client paragraph and the
  sync table row) and §A were updated. Nothing was pushed, CI was not touched, `.opencode/agents`
  was not touched and no PR was commented on.

### 2026-11-12 — an input method's composition now settles before anything leaves the client

- **What was asked.** Take the code item from the previous handoff: with `collab_queue` set,
  `Session::execute_unguarded` returns before `settle_composition()`, so an edit typed through a
  composition (IME layouts, CJK) reaches the room under-formed — the operation's text is not the
  text its author sees, an undo step brings back something else, and the settle arrives after the
  send. The rule to implement: no operation may leave the client before the composition is settled
  (settle before planning/applying, as in the single-user path), or the command is refused with an
  explicit reason before the first write and without changing the version; a repeated settle must
  not duplicate the edit or create a second undo step. Tests: the edit after a composition carries
  the final text and matches on two replicas through the normaliser, the adapter and the in-memory
  hub; undo restores the original text in one step; a send during an active composition does not
  leave raw or is refused with its reason; the existing composition tests are rewritten to the
  rule.
- **What was found, in that order.** (1) The composition's own commands were *also* queued — and
  the normaliser has no form for `text.compose`/`text.commit`, so in a room an IME user got
  `unsupported command` per pre-edit and the typed text was lost outright, not merely
  under-formed. (2) `shape.setField`/`CellText` are restore-only on the wire: the adapter refuses a
  client-sent one (`check_restore`), so the whole body needed its own payload. (3) `Image::Text`
  is the undo image of every text step and holds the plain text, so undo restores the text, not the
  runs — and that is true of typing too, not of this slice.
- **What landed** (`51643c59`, code; this docs commit on top, neither pushed):
  `CompositionOwner` (slide id, shape, cell, notes) captured at the start of a composition;
  `text.compose`/`text.commit` run locally in a shared session; every other command settles the
  composition before it is queued (the settle block moved above the queue branch);
  `end_composition` queues one whole-body command when the body changed; `composition_settle_command`
  builds it (`shape.setField` text/cellText, `slide.notes`); `local::text_body_ops` normalises it to
  the new `Payload::TextBodySet { body, cell? }`; `apply` runs it through `shape.setField` and
  records the body as the undo image; `text.deleteSurrounding` outside a composition (superseded
  for that command by `b314feef`, which sends the range it removes as one `text.delete`) and any
  composition outside slides are refused before the first write with their reason. `rfc.md` gained
  §10.1, `production.md` the client paragraph, the sync table row and the limits.
- **Evidence.** fmt clean; clippy `-D warnings` for engine, collab, collab-server, collab-proto and
  ui-egui `--all-targets`; engine 615 + 6 + 8; collab 295 + 8; collab-server 19 + 5 + 5 + 18;
  collab-proto 42; collab-web 15; `xtask wasm` ok (22 crates); ui-egui `--lib` 527 passed + 4
  ignored with the one known host-font failure
  (`ime_tests::korean_composes_in_the_notes_pane`, no ㄴ glyph on the host — the same environment gap
  the earlier gate recorded for the render test). Nothing was pushed, CI was not touched and no PR
  was commented on.

### 2026-11-12 — the SmartArt node-picture line was already closed; the delivery record moved to `5a105c61`

- **What was asked.** Treat `smartArt.picture` as the next slice of the SmartArt line — a picture in
  a node as one structural step over the media channel: normalised operation, engine check before
  the first write (media in the session, crop and opacity, the node is really a graphic), undo
  image = the whole source node, redo = the result, dedup by `op_id`, a viewer refused `ReadOnly`
  and a hand-built operation refused `Forbidden` by the server without moving the version; two
  replicas converging through the normaliser, the adapter and the in-memory hub; hostile input
  refused before the first write; a socket test for the viewer.
- **What was found.** All of it is delivered and has been since `44489f34` ("put a picture in a
  SmartArt node as one final value"), which is an ancestor of the current head; the note
  §2026-11-10 — a picture in a SmartArt node as one final value describes exactly that design. The
  code is `Payload::SmartArtPicture` (`crates/collab-proto/src/lib.rs:435`), the address and the
  pre-write refusals are `crates/collab/src/apply.rs:1363` (top-level graphic, node in the outline,
  media present in the session, a picture object within `MAX_NODE_PICTURE_BYTES`), the engine command
  is `smartArt.setPicture` (`crates/engine/src/cmd/smartart/edit.rs:714`) with its own crop/area/
  opacity/layout checks, and the tests are `apply::tests::two_replicas_converge_on_a_node_picture_and_undo_restores_the_whole_node`,
  `a_node_picture_refuses_a_plain_shape_an_unknown_node_and_a_hostile_place`,
  `hub::tests::a_node_picture_reaches_both_replicas_through_the_hub_and_a_viewer_is_refused` (the
  viewer's `Forbidden` with the version and document unchanged),
  `local::tests::a_node_picture_normalises_to_the_media_reference_and_its_place`,
  `a_node_picture_command_uploads_its_bytes_and_sends_only_a_reference` and
  `apps/deckcraft-collab-server/tests/socket.rs:772` (`a_smartart_node_picture_travels_over_the_socket_and_a_viewer_is_refused`).
  The "What remains" sentence that named this slice belonged to the *conversions* note written
  before it; there was nothing left to implement, so **this pass wrote no code**.
- **Done (docs only).** The two forward-pointers that still called the line open are closed with
  the commits that answered them (`44489f34` for the picture, `c406a329`/`7b51e65d` for the model's
  camera and scene). The delivery record now names the head the pr-agent pushed at
  2026-11-12T03:13:50Z — `5a105c61` (docs) on `aaecc6db` (code) — and every "delivered head"
  statement in the header, §PR publication, the PR table, the coverage table and item 2 of the next
  session is tied to that timestamp, with the live value pointed at
  `gh pr view 2012 --json headRefOid`. `gh run list --commit 5a105c61` and `--commit aaecc6db` are
  both empty: no workflow has run on the delivered head.
- **Gate, re-run on the branch head (`5a105c61` / code `aaecc6db`, host `agents`).**
  `cargo fmt --all -- --check` clean; `cargo clippy -p deckcraft-collab -p deckcraft-collab-server
  --all-targets -- -D warnings` EXIT=0; `cargo test -p deckcraft-collab -p deckcraft-collab-server`
  294 lib + 8 two_clients + 19 unit + 5 commit + 5 gates (16.7 s) + 18 socket, all passed, 0 failed;
  `cargo test -p deckcraft-collab-web` 15 passed; `cargo xtask wasm` ok (22 crates). The wider
  `cargo xtask ci` on this head is `.craft/logs/gate-deckcraft-2012-5a105c61.log`: fmt/pdf-deps/
  clippy ok, and `test` stops only on the known host-font render test
  (`render::tests::east_asian_vertical_text_stands_ideographs_upright`,
  `crates/render/src/tests.rs:2396` — no CJK glyph on the host, identical on earlier heads).
- **Not done.** No push (the owner's step), no CI change, no PR comment. This STATUS commit sits on
  top of the delivered head and is local only, so the PR head is one docs commit behind the branch
  until a person pushes. The next code item stays
  the IME finding from item 8: with `collab_queue` set, `Session::execute_unguarded`
  (`crates/engine/src/lib.rs`) returns before `settle_composition()`.

### 2026-11-12 — one operation, one identity; multi-object commands reach the room whole

- **What was wrong.** `crates/collab/src/local.rs`'s `make` closure stamped the command's single
  `op_id` on every operation the command normalised into. The room's `opId` dedup (`session.rs`),
  which answers a repeat with the stored `Applied`, then swallowed everything after the first
  operation: a multiple-selection `shape.move`/`shape.setBounds` moved one shape and `slide.delete`
  of several slides deleted one, on every replica, with no `rejected` to notice it. Found by
  CodeRabbit (review `5866843261` on #1998) and confirmed on `96783106`.
- **Done.** The normaliser numbers the operations of one command (`<id>`, `<id>#1`, …), the first
  keeping the id the preview and the settle use; the numbering is deterministic so a resend keeps
  its ids. `ClientSession` holds a structural operation until the operations sent before it are
  answered, so the N structural steps of one command do not arrive stale. RFC §4.3 extended; the
  consequences (N undo steps for a multi-selection move; a drag is still one step) are written down
  in §One operation, one identity.
- **Gate.** On `aaecc6db`: fmt clean, clippy `-D warnings` for collab and collab-server EXIT=0,
  collab 294 lib + 8 two_clients, collab-server 19 unit + 5 commit + 5 gates + 18 socket, ui-egui
  `--lib` 526 passed + 4 ignored with only the known host-font failure.
- **Not done.** The second finding of the same round is untouched: with `collab_queue` the engine
  returns the command from `execute_unguarded` (`crates/engine/src/lib.rs`) before
  `settle_composition()`, so an input method's composing text is not settled before the command is
  queued. It is a separate fix with its own IME tests.
- **Delivery (as of this writing).** `aaecc6db` (code) plus `5a105c61` (this STATUS commit) are
  the delivered head: the pr-agent pushed `5a105c61` at 2026-11-12T03:13:50Z, so fork = PR = local
  head at that moment. No workflow run was created on it (approval is a maintainer's step); the
  host gate on `5a105c61` is `.craft/logs/gate-deckcraft-2012-5a105c61.log` (fmt/pdf-deps/clippy
  ok, `test` stops only on the known host-font render test). A later push moves the head again —
  read the live value from `gh pr view 2012 --json headRefOid`.

### 2026-11-11 — `origin/main` merged into `collab/pr15`; PR #2012 no longer conflicts

- **What was wrong.** `origin/main` moved 28 commits past `0623b430` after the branch was
  delivered, so GitHub reported PR #2012 as `CONFLICTING` / `mergeStateStatus: DIRTY` on head
  `da8ba866`.
- **Done.** A normal merge commit, `c9803277`, with `da8ba866` and `c6846f6a` as parents: no
  rebase, no force, no history rewritten, so every slice hash in this file still holds. Two
  content conflicts (`Cargo.lock`, `docs/architecture.md`) resolved as a union that keeps main's
  change and the collaboration side; main's new `ptLst`-order fallback in `crates/pptx/src/dgm.rs`
  auto-merged into a compile error and is fixed in the same commit (`picture_place: None`). The
  details and the full gate table are in §Merge of `origin/main`.
- **Gate.** `fmt --check` clean, clippy `--workspace --all-targets -- -D warnings` EXIT=0,
  `collab-proto` 42, `collab` 288 + 8, `collab-server` 19 unit + 5 commit + 18 socket + 5 §8 gates
  (19.1 s), `collab-web` 15, `engine` 612 + 1 ignored (+ 6 + 8), `pptx` 70 + 5 + 146,
  `ui-egui --lib` 526 + 4 ignored with only the known host-font failure, `xtask`
  `pdf-deps`/`assets`/`layers` (28 crates)/`wasm` (22 crates) EXIT=0, workspace doctests pass.
- **Not done.** Nothing was pushed: the fork and the PR still hold `da8ba866`, so
  `mergeStateStatus` will only be recomputed after the owner's push. CI approval is the owner's
  step too.
- Tree clean; docs-only change on top of the merge (this file).

### 2026-11-11 — the presence heartbeat delivered on top of `1f78cadb`; the §8 gate is green

- **What was wrong.** A client sent presence only when it changed, but the room keeps a
  participant only while their last message is younger than the 30 s TTL and never echoes a
  presence back to its author. An idle editor therefore vanished from every peer's list after
  30 s, and a client that joined later never learned about anyone — the cabinet's bar showed one
  participant while edits converged. The fix had been committed as `9c93ae66` on a branch cut
  from `6e9d10e7`, which is an ancestor of the delivered head; it was not on `1f78cadb`.
- **Delivered.** Cherry-picked onto `1f78cadb` as `fd5914e8` (conflict-free; the only file is
  `crates/ui-egui/src/collab.rs`, +65/−8): `PRESENCE_HEARTBEAT_MS = 10_000` repeats an unchanged
  presence, `PRESENCE_MIN_GAP_MS = 50` keeps a moved one at 20 Hz, and
  `an_unchanged_presence_is_repeated_before_the_room_forgets_it` drives the pump over
  `PipeTransport` and fails without the heartbeat.
- **Gate on `fd5914e8`** (build host, TASK.md §8 verification): `cargo fmt --all -- --check`
  clean; `cargo clippy -p deckcraft-collab -p deckcraft-ui-egui --all-targets -- -D warnings`
  clean; `cargo test -p deckcraft-collab` 288 + 8 passed; `cargo test -p deckcraft-ui-egui
  --lib` 522 passed + 4 ignored with `ime_tests::korean_composes_in_the_notes_pane` skipped —
  that test fails identically on `1f78cadb` with `No glyph for "ㄴ"`, so it is the known
  host-without-CJK-font gap, not this change.
- **Delivered to the fork.** `git push fork collab/pr15` → `1f78cadb..fd5914e8`;
  `git ls-remote fork refs/heads/collab/pr15` and `gh pr view 2012 --json headRefOid` both name
  `fd5914e8`, so #2012 carries the heartbeat. No history was rewritten, no force-push.
- **Three PRs.** #2012 (`fd5914e8`, leading), #1998 (`6e9d10e7`, `CLEAN`) and #1975
  (`be706a11`) stay open; closing the superseded two is the maintainer's decision.
- **Commits.** `fd5914e8` (code + test) and this docs commit on `collab/pr15`; both pushed.

### 2026-11-11 — STATUS moved to the delivered head `38a20a9c`; the gate has not run

- **What was wrong.** The header, §PR publication, the acceptance matrix and the next-session
  notes still called `9ad80547` the delivered head and kept `f6239baf`/`891187f5` "local, not
  pushed". The push had happened: `git ls-remote fork refs/heads/collab/pr15`, `gh pr view 2012
  --json headRefOid` and the local branch all name `38a20a9c`, and the tree is clean.
- **Delivered.** `891187f5` — code and tests: a room whose snapshot fails stays live and answers
  `500` instead of closing, a snapshot that does not cover a rewind is refused and not installed,
  and a move dropped by a role change is cancelled exactly — and `38a20a9c` — docs: the fifth
  review's triage table, the session log and `production.md`'s close-failure contract. The fourth
  review's triage stays delivered in `9ad80547`.
- **CI on `38a20a9c`.** The four workflow runs (CI, House rules, Nix, SonarCloud) were created
  2026-11-11T11:31:32Z and are all `completed / action_required`: none executed, they wait for a
  maintainer's approval. The only check run is CodeRabbit `completed / skipped` (11:33:45Z), the
  combined status is empty (`state: pending`, no statuses), so the rollup is that one `skipping`
  and nothing else — hence `mergeStateStatus: UNSTABLE` (`mergeable: MERGEABLE`).
- **Three PRs, one feature.** #2012 (`38a20a9c`, `UNSTABLE`), #1998 (`6e9d10e7`, `CLEAN`) and
  #1975 (`be706a11`, `UNSTABLE`) are all open, #2012 at the top of the stack. Closing the two
  superseded ones is the maintainer's decision; an agent session does not close them.
- **Commit.** One docs-only commit on `collab/pr15` on top of `38a20a9c`; not pushed, so the fork
  and the PR still point at `38a20a9c`. No code changed, no gate was rerun, the fork was not
  touched.

### 2026-11-11 — the fifth CodeRabbit review of #2012 (`5855491108`), triaged

- **What was reviewed.** The review submitted at 07:49:58Z on `9ad80547` (the head the check run
  started at 07:32:03Z): five actionable inline comments, four Major and one Minor. Every one was
  opened against the tree first; all five are live and fixed in `891187f5`, with a test that fails
  without the fix. The table is in §Fifth CodeRabbit review.
- **Server, close (finding 1).** `DELETE /v1/sessions/{id}` snapshotted the room after removing it
  from the map and logged a failure, so a store that could not be written lost the only live copy
  of the accepted tail while the answer said the close succeeded. The snapshot now happens while
  the room is still live and still in the map; a failure answers `500 {"closed": false}` and
  leaves it serving (and still the hub a `commit` binds to). `production.md` §Recovering a damaged
  store says so: a `500` is retried, never followed by quarantining a live room.
- **Server, store (findings 2 and 3).** `latest_base` turned an unreadable room directory into
  "no stored room" and let `create_session` open `initialBytes` or a blank deck over the stored
  document; it now propagates `read_dir` and entry errors and reads the legacy `base.deckcraft`
  name only after the directory was listed in full. The base rename is synced with its directory
  (and again after `meta.json`) before the journal is trimmed, and a failed sync stops the
  snapshot with the journal intact.
- **Client (finding 4).** A snapshot that does not cover a rewind was refused by `ClientSession`
  but installed by `CollabDoc::on_msg` all the same, which took `snapshot.version` and left the
  steps between missing; the state machine now reports the refusal (`take_refused_snapshot`) and
  the host returns before installing anything.
- **Engine (finding 5).** A move dropped because the role turned read-only kept the local boxes
  and an open interaction. `Drag::Move` now carries the offset it last applied, so the cancel puts
  back exactly the boxes the gesture wrote (a step another replica settled meanwhile stays) and
  closes the interaction without an undo step; `up` does the same when the release comes after the
  role change.
- **Gates on `agents`** (rust 1.99.1): `cargo fmt --all --check` clean; `cargo clippy --workspace
  --all-targets -- -D warnings` EXIT=0; `engine` 598 + 1 ignored, plus 6 + 8 integration;
  `collab` 288 + 8; `collab-server` 19 unit + 5 commit + 5 gates (16.9 s) + 18 socket. The four
  new tests were run against the reverted production hunks and failed on the intended assertions
  (list in §Fifth CodeRabbit review).
- **Commit.** `891187f5` on `collab/pr15`, code+tests+STATUS, plus this docs commit; neither is
  pushed, so the fork and the PR still point at `9ad80547`. CI untouched.

### 2026-11-11 — STATUS synced to the delivered head `9ad80547`; fifth review still running

- **What was wrong.** The header and §PR publication still called `4a64e764` the delivered head
  and the fourth review's triage "one local commit, not pushed". The push had happened: the fork
  head, `gh pr view 2012 --json headRefOid` and the local branch all named `9ad80547`, one commit
  on top of `4a64e764` (7 files, +498/−116 — `store.rs`, `client.rs`, `pointer.rs`,
  `deckcraft-collab-server/src/lib.rs`, the engine tests and both docs files).
- **Numbers.** The PR carries 112 commits, 99 files, +40 739/−562 (PR API `commits` /
  `additions` / `deletions` / `changed_files`; the earlier 100-commit figure was the page size of
  `gh pr view --json commits`). Merge state `MERGEABLE` / `UNSTABLE`, empty `reviewDecision`.
- **Fifth review.** CodeRabbit's check run on `9ad80547` started 2026-11-11T07:32:03Z and was
  still `in_progress` at 07:38Z and 07:46Z; no review object, no inline comment, no issue comment
  since 07:00Z. It was not waited on — §Fifth CodeRabbit review is the handoff for it.
- **Gates on `agents`** (rust 1.99.1, on the committed tree; only docs differ in the working
  copy): `cargo test -p deckcraft-collab -p deckcraft-engine -p deckcraft-collab-server -j 12`
  EXIT=0 — collab 287 + 8; collab-server 16 unit + 5 commit + 5 gates (14.6 s, LibreOffice
  skipped on the build host) + 18 socket; engine 597 + 1 ignored, plus 6 + 8 integration.
- **Commit.** One docs-only commit on `collab/pr15` on top of `9ad80547`; not pushed, so the fork
  and the PR still point at `9ad80547`. CI was not touched.

### 2026-11-11 — the fourth CodeRabbit review of #2012 (`5852391245`), triaged

- **What was reviewed.** The review at 01:35Z on `4a64e764`: four Major and one Minor, none of
  them marked addressed. Each was opened against the tree first; the table is in §Fourth
  CodeRabbit review. `4a64e764` was the head at review time; the fixes are delivered in `9ad80547`
  (`git ls-remote fork refs/heads/collab/pr15`, `gh pr view 2012 --json headRefOid`), whose parent
  it is.
  `7f93069e`, which the earlier notes named, is a dangling commit with the same subject as
  `2bbe723f` and is not in the branch; every hash in this file that named it now names
  `2bbe723f`.
- **Store (finding 1).** `snapshot` wrote `meta.json` with the new version before the base, so a
  crash in between made `load` skip the journal entries that carry those edits. The base is now
  `base-<version>.deckcraft` — the version is in the name, so the bytes and the version are one
  unit — `meta.json` follows through a temporary file and a rename, the journal is trimmed last,
  older bases are pruned, and `load` takes the version from the base's name (tolerating a
  missing or torn `meta.json`; a legacy `base.deckcraft` still loads from its metadata).
- **Client (finding 2).** A `rewind_replica` during an in-flight resync was lost: the answering
  snapshot cleared `resync_wanted` and advanced `replica_version` past the rewound version. The
  rewound version is remembered (`rewind_pending`) and a snapshot that does not cover it is
  refused, so the tail is asked for again from the version the replica holds.
- **Docs (findings 3 and 4).** §Restart and persistence no longer claims the dedup window comes
  back after a restart (only the replayed tail's ids do), and §Recovering a damaged store now
  closes the room first (`DELETE` takes a snapshot) and recovers the newest base from the
  quarantined directory instead of reopening from the cabinet's older bytes. Finding 4 also has a
  code half: `close_session` snapshots the live document before the hub leaves the map.
- **Pointer (finding 5).** A viewer could start an Ink stroke, a shape/text create drag, the table
  eraser, a freeform or an Edit Points vertex on a path that returns before the read-only gate,
  so `add_ink`/`edit` changed the replica with no room operation. `down` now drops a writing drag
  and skips those arms and the Edit Points block for a read-only session, keeping a text selection
  and a marquee.
- **Gates on `agents`.** fmt clean; clippy `-D warnings` EXIT=0; collab-server 16 unit (8
  `store::`) + 5 commit + 5 gates (14.8 s) + 18 socket; collab 287 + 8; engine 597 + 1 ignored
  plus 6 + 8 integration. The three new tests were run against the reverted production change and
  failed on the intended assertion.
- **Commit.** `9ad80547` on `collab/pr15` on top of `4a64e764` (code, tests and docs), pushed:
  the fork and the PR head point at it. CI was not touched.

### 2026-11-11 — the second CodeRabbit review of #2012, triaged and closed

- **What was reviewed.** CodeRabbit's review `5846427839` at 17:33Z on `9b43c611a`: five
  actionable comments (drag through the ordered path, the commit path's hub binding, the undo
  history claim, the restart version, the recovery procedure, plus one Minor on a second resync),
  a "SafeArt" typo and two test assertions. Each was checked against the tree at `fd1ee71a`
  first; the five code findings were live, the typo was already fixed, the two assertions were a
  real gap. The table is in §Second CodeRabbit review.
- **Drag.** `Session::pointer` mutated the document through `edit` for every gesture, so a
  viewer's drag moved a shape no room ordered and an editor's release sent nothing. The press of
  a viewer no longer starts a writing drag; an editor's release submits the final boxes of the
  shapes the drag moved as one `shape.setBoundsMany`, and the client turns that into one
  `ShapesSetBounds` (new `drag_bounds` in `crates/collab/src/local.rs`). Alt-drag duplication
  stays a local-only copy, so a shared session treats the gesture as a move until duplication has
  a command.
- **Commit path.** `commit_from_client` authorized against the socket's hub and `build_and_publish`
  re-looked the file id up, so a socket that outlived `DELETE` could publish the room that
  replaced it. `build_and_publish_on` takes the authorized hub and refuses unless it is still the
  live room (`still_live`: same `Arc`, not closed); the accepted etag is adopted only there.
- **Restart.** A base opens at version 0, and `meta.json`'s version was never applied, so a room
  reopened from a base at version 200 answered 0 with an empty tail. `Store::load` now continues
  the numbering before replaying; the `store.rs` module doc and `production.md` say that replay
  brings back the document, the version and the dedup window but not the per-actor undo history.
- **Recovery.** `production.md` has a new §Recovering a damaged store: keep and name the store
  directory, force a snapshot while the process lives, move the damaged directory aside, then
  create the session with the cabinet's committed bytes — a plain re-create reopens the damaged
  store and a damaged journal answers `500`.
- **Resync.** A gap raised while a resync was on the wire sent a second `Resync`; the first
  snapshot released the queue and the second could drop an operation the server never applied.
  One resync is in flight at a time, and the answering snapshot clears the flag a gap raised
  meanwhile.
- **Gates on `agents`.** fmt clean; clippy `-D warnings` clean; engine 596 + 1 ignored; collab
  285 + 8; collab-proto 42; collab-server 15 unit + 5 commit + 5 gates (16.8 s, LibreOffice
  skipped) + 18 socket; `ui-egui collab::` 24 and `host_save` 12; `collab-web` 15; `deckcraft-web`
  6; `xtask layers` 32 crates. The six new tests were run against the un-fixed code first and
  failed there.
- **Commits.** `d56272e6` (code and tests), `082b7c7b` (the two Save assertions) and `2bbe723f`
  (a connector's glue in a shared drag) on `collab/pr15`, then the docs commit `4a64e764` that
  records this triage; `4a64e764` was the branch, fork and PR head at that time (the counts here
  came from `gh pr view --json commits`, which pages at 100; the live API counts are in §PR
  publication). CI was not touched and its four runs on the head remain
  `action_required` (created 2026-11-11T01:24:39Z). CodeRabbit's third review `5848775355`
  (20:22Z, on `fd1ee71a`) read only this file and left no actionable comment, and its fourth
  `5852391245` (01:35Z, on `4a64e764`) is the pass logged above.

- **What was reviewed.** The CodeRabbit review on PR #2012 at head `1b057cbf`: 11 Major and
  7 Minor. Each finding was opened against the code first; the table in §PR publication records
  all 18 with the disposition.
- **Code fixes.** The HTTP reader now accepts a body as large as the widest media kind (32 MiB)
  instead of 16 MiB; a closed room is marked before it leaves the server's map, so its sockets
  stop and cannot keep appending to a store a new room may load; a replay preloads bytes for every
  media-bearing payload, not only a picture; an operation submitted after a `Resync` waits for
  the snapshot instead of being dropped as covered; `Hub` keys its peers and outboxes by a
  per-connection id with the actor kept for authorship/history/presence, so two tabs of one person
  no longer share one queue; the read-only and collab-queue gates apply only at `depth == 0` so a
  remote list lead formats in place; the model camera's `pos`/`lookAt`/`up` and a SmartArt picture
  crop reject non-finite values; a chunk size from the cabinet uses checked arithmetic;
  `WsTransport::Drop` detaches instead of blocking on a connect without a timeout; a SmartArt node
  picture's opacity round-trips as `a:alphaModFix`; `file.save` reports a refused commit as a
  refusal; a focus loss with no changed value no longer runs a cell-size command.
- **Docs.** `production.md` states the token validation exactly (signature/`exp`/`sub`/`aud`;
  `app`/`fileId` only when present; no session-id check), the journal-after-apply durability
  limit, the 64 MiB assembled-base ceiling and the wider replay preload. `study.md` maps its
  A/B labels to `options.md`.
- **Gates on `agents`, run before the push.** fmt clean; clippy `-D warnings` clean; `collab` 283 + 8;
  `collab-server` 13 unit + 5 commit + 5 gates + 18 socket; `engine` 594 + 1 ignored; `pptx`
  69 + 5 + 145; `ui-egui collab::` 24 and `host_save` 12; `deckcraft-web` 6; `xtask layers` 32
  crates. The full `ui-egui` library run still has the Korean IME test failing for a missing CJK
  face on the build host (no `craft-fonts`), before and after this diff.
- Commits `5b6fcfcd` (code and tests) + `9b43c611` (docs) on `collab/pr15`, **pushed**: the fork
  branch and the PR head are `9b43c611a` (106 commits, 97 files, +39 834/−522). CI was not
  touched; the head's four runs are `action_required` (awaiting approval), and CodeRabbit's
  second review on `9b43c611a` left five actionable findings for the next session. The STATUS
  correction that records this is local and unpushed.

### 2026-11-10 — optimistic preview: a local edit shown before the room confirms it

- The last item of §What is NOT done that was neither out of this repository nor a CI gap was the
  latency one: every edit appeared on `applied`. The RFC already described an optional `preview`
  local id (§3, §4.2) and `ClientSession` already distinguished `Effect::Settled` from
  `Effect::Rollback`, but no client used either.
- **Wire.** `OpEnvelope::preview: Option<String>` (`serde(default, skip_serializing_if)`) is the
  sender's local id for its optimistic copy. A frame without it decodes and behaves exactly as
  before; the room strips it in `DocSession::apply` before dedup, journal, undo images and commit,
  so nothing stores or echoes it (proto, session and real-socket tests pin all three).
- **Client.** `CollabDoc::set_preview` / `CollabUi::set_preview` (desktop flag
  `DECKCRAFT_COLLAB_PREVIEW`, off by default). Only a `command`-class operation whose payload the
  room will not rewrite is previewed: text offsets, table cell addresses/ranges/carets,
  structural steps, `SmartArtSet` (generated child ids) and `FontCreate` (session font database)
  are excluded. One preview at a time: it is applied at submit through `apply_confirmed` with an
  `Arc` copy-on-write before-image (one pointer copy; the engine's `edit` is already copy-on-write)
  and no undo step. Every later local command and undo/redo waits in a bounded queue
  (`MAX_DEFERRED = 64`) and is normalised only against confirmed state, so a payload can never
  include an edit the room does not have — the reason a chained preview would double-count text
  offsets under the server's rebase.
- **Resolution.** `applied` with the same payload and target settles the preview and does not run
  the command a second time; a different form (a rebase the client did not expect) restores the
  before-image and applies the authoritative command; a refusal restores exactly that edit and
  leaves accepted steps alone; a step from another participant between preview and answer abandons
  the preview, reports the commands that waited and asks for the tail from the version the replica
  holds (`ClientSession::rewind_replica`), so the two orders are never mixed. A preview moves no
  version and no engine undo step.
- **UI.** The preview is applied inside `CollabUi::pump` before the frame is drawn, so the canvas
  shows the change without waiting for the server; the flag follows the host (`attach` forwards
  it, `SlideApp::enable_collab` reads the environment), and a viewer or an offline session shows
  nothing.
- **Tests.** `collab` 280 + 8 (six new: visible before `applied` and settled without a second run,
  outside the undo stack; a refusal that restores exactly one edit and keeps an accepted one; a
  foreign step that gives a `resync` and then the tail in the room's order; a viewer and an
  offline replica that preview nothing; a command during a preview that waits for confirmed state;
  the flag-off path); `collab-proto` 42 (the optional field's round trip and a frame without it);
  `collab-server` 10 unit + 5 commit + 5 gates + 18 socket (new socket test: the room applies an
  operation carrying `preview`, dedups a resend under another local id and its `resync` tail
  contains no `local-*` string); `ui-egui collab::` 24 (the flag reaches the client, the move is
  drawn before the answer and settled on it); `collab-web` 15, `deckcraft-web` 6, wasm32 check ok.
- **Gates on `agents`.** `cargo fmt --all -- --check` EXIT=0; `cargo clippy --workspace
  --all-targets -- -D warnings` EXIT=0; `collab-proto` 42/0/0 EXIT=0; `collab` 280 + 8/0/0 EXIT=0;
  `ui-egui collab::` 24/0/0 EXIT=0; `collab-server` 10 + 5 + 5 + 18/0/0 EXIT=0; `xtask layers` 32
  crates EXIT=0. The §8 gates still run 5/5 in 15.78 s with LibreOffice skipped on the build host.
- Commit `a65d1948` plus this docs commit on `collab/pr15`, branched from `collab/pr14` at
  `6e42a7ab`; `rfc.md`, `production.md` and this file updated. Nothing pushed, CI untouched,
  PR #1975 and #1998 unchanged.

### 2026-11-10 — Save in a live desktop session commits the room

- The last in-repo gap of §What is NOT done item 5 was a desktop "save to cabinet" command.
  `ClientMsg::Commit` existed in the wire, the server answered it and the hub handled the `409`,
  but no client ever sent it: Save wrote the local file, so the cabinet never saw the session.
- **Wire.** `ClientSession::commit(expected)` (`CommitError::{ReadOnly, Offline}`) queues one
  commit; a Save while one is queued or in flight is the same request, so it creates no second
  file version. It waits until the journal is drained and every operation is acknowledged, so the
  file the server builds includes the last keystroke. `next_outgoing` sends it; `commit-result`
  records the accepted etag and the next Save expects it. A `commit` rejection clears only the
  in-flight marker — the expected version is kept, so a retry against a file that moved on is
  refused again instead of overwriting it (ADR-0009 §4). A dropped socket clears the marker too;
  the user saves again.
- **Desktop.** `SlideApp::save` (and `file.save` by id) commits instead of `file.save` when
  `CollabUi::owns_save()`; Save As and Export stay local. `CommitState` gives "Committing…",
  "Committed vN" and "Commit refused: …". The document is marked saved only on `commit-result`,
  and only if no edit landed since the commit was asked for; a refusal leaves it dirty and the
  banner says "Save a copy (File › Save As) keeps your edits". A viewer is refused before anything
  is sent; an empty journal still commits (a version is created from unchanged bytes).
- **Tests.** `collab` 273 + 8 (four new: waits for the journal and travels once, an empty journal
  still commits, a viewer and an offline session are refused before the wire, a refusal is not
  retried and keeps its expected version); `ui-egui collab::` 23 (three new: one commit and dirty
  until the result, then a new expected version; a refusal keeps the edits and offers a copy; a
  viewer's Save never reaches the cabinet); `collab-server --test commit` 5 (new: the accepted
  etag written to `meta.json` survives a restart and still refuses an external change, and a
  commit with no expected version compares against the restored one).
- **Gates on `agents`.** `cargo fmt --all -- --check` clean; clippy `-p deckcraft-collab -p
  deckcraft-collab-server -p deckcraft-ui-egui --all-targets -- -D warnings` clean;
  `collab-proto` 41 passed / 0 failed / 0 ignored, `collab` 273 + 8, `ui-egui collab::` 23,
  `collab-server --test commit` 5 — EXIT=0 for each. The full `ui-egui` suite is 520 passed, 1
  failed, 4 ignored: `ime_tests::korean_composes_in_the_notes_pane` fails on the build host
  because `deckcraft-fonts` is built without `craft-fonts` (no Korean glyph; the build log prints
  that warning), reproduces in isolation and is unrelated to this diff.
- Nothing was pushed; CI was not touched; PR #1975 and #1998 are unchanged.

### 2026-11-10 — a base above the inline limit travels as a chunk chain

- The gate TASK.md §8 asks for at the end of §8 — a 100+ slide deck with media and *hundreds* of
  steps — exists: `a_hundred_and_twenty_slide_deck_with_media_soaks_for_hundreds_of_steps_and_a_chained_base`.
  It builds 120 slides, each with its own 160×160 noise picture (12 193 KiB of `.pptx`, **11.8 MiB**
  of native base) and a rectangle, makes **322 edits** with three editors (move and bounds on the
  picture, text and fill on the rectangle, 46.8 ms per step on the build host — the server's 50 ms
  read slice, not the apply), compacts the journal seven times, and commits at the head.
- That base is above the 8 MiB inline limit, so the second half of the item — a client below a
  compacted journal without reopening the file from the cabinet — needed the wire to carry it.
  `BaseSnapshot.chunks {sha256, len, chunkSize, count}` announces the chain; `base-chunk
  {version, index}` → `base-chunk {version, index, count, data}` moves one chunk (base64, 256 KiB
  decoded in the gate); the client assembles, checks `len` and the digest, and only then installs
  the base and replays the tail above it. The snapshot is not applied while the base is being
  assembled: `Applied` in that window is not replayed onto the stale replica, and after installing
  the base the client asks for the tail from the base's version, so nothing is skipped. `submit`
  answers `rebuilding` in that window — normalising an edit against a document the room no longer
  has would be worse than a moment's pause.
- **Resume, not restart.** The client keeps its assembled prefix and the index it is missing; a
  dropped socket clears only the outstanding request, so the next connection asks for that chunk.
  A second snapshot naming the same base (the catch-up after a reconnect) continues the chain; a
  chunk of a base compaction replaced is `rejected{code:"stale", retryable:true}` and the client
  asks for a fresh snapshot instead of mixing two bases' bytes. In the gate a fifth participant
  joins the chain, drops after three chunks, reconnects and asks for chunk 3 — and finishes.
- **Limits on both sides.** Server: `max_base_chunk` (`COLLAB_MAX_BASE_CHUNK`, 1 MiB) and
  `max_base_chunks` (`COLLAB_MAX_BASE_CHUNKS`, 1024). Client: `BaseChunks::MAX_CHUNK_SIZE` (8 MiB),
  `MAX_COUNT` (4096), `MAX_LEN` (64 MiB) and the consistency check `count == ceil(len/chunkSize)`,
  all enforced before a chunk is asked for or a byte allocated. A base that does not fit the chain
  is fetched in one HTTP request by a client that has an HTTP side; a build without one, or a base
  above the 256 MiB HTTP ceiling, is named without bytes and reopens the cabinet's file — the
  fallback, not the expected path.
- Measured on `agents`: 322 edits in 15.1 s; the viewer rebuilt 11.8 MiB in **48 chunks** plus two
  tail steps in 168 ms; the commit at version 324 built 12 195 KiB in 223 ms and reimported with
  120 slides, 120 pictures and every slide's picture transform equal to the replicas'. The five §8
  gates run 5/5 in 19.4 s with `--test-threads=1` (LibreOffice skipped on the build host).
- Tests: `collab` 269 + 8 (chain order, one request at a time, resume at the missing index, same
  chain continued, stale chain, protocol ceilings, short chunk, wrong digest, steps collected by a
  catch-up, the CollabDoc host path resuming across a reconnect, the server's chunk API and the
  hub's viewer-allowed chunk with `stale`/`invalid` refusals), `collab-proto` 41 (wire round trip,
  `is_well_formed`, `chunk_len`), `collab-server` 10 unit + 4 commit + 5 gates + 17 socket.

### 2026-11-10 — a 3-D model's scene as one final value

- `model3d.scene` is collaborative on top of `insert.model3d`. The operation addresses one
  `Model3d` shape and carries the **whole final scene** — one light and the environment — or
  `null` for the reader's default scene. The light is `type` (`directional`, `point` or `spot`), a
  non-zero `direction` vector, `intensity` 0–1 and, for a spot only, the cone's `angle` in degrees
  1–179; the environment is `ambient` 0–1 and an optional sRGB `background` as three 0–255
  channels. There is deliberately no relative form: two replicas applying "dimmer", "warmer" or
  "turn the light 10°" to their own scenes would end in two different scenes, which is what the
  final-value rule exists to prevent (RFC §3.2.1).
- Validation happens in three places, all before the first write. The client normaliser runs the
  engine's own `scene_param`, so a relative field (`dimmer`, `rotate`, `zoom`), an unknown light
  type, a zero or non-finite direction, intensity/ambient out of 0–1, a background that is not
  three integers 0–255, an angle on a non-spot or a spot without one and a plain shape are
  refused before an operation is sent. The wire type (`collab_proto::ModelScene`) is
  `deny_unknown_fields` on the scene, the light and the environment, so a hostile frame with a
  relative field is a bad frame instead of a silently merged value. The engine command re-checks
  every number and the angle/type pairing and addresses every target before it writes; a failing
  `edit` clones, so a refusal leaves the document untouched.
- The scene is part of the model: `ShapeKind::Model3d { media, poster, camera, scene }` with a
  new `ModelScene` value. The writer puts it into the slide part next to the camera as
  `am3d:scene` (`am3d:light` with `type`, `intensity` and a spot's `angle`, and `am3d:dir` with
  `dx`/`dy`/`dz`; `am3d:environment` with `ambient` and `color`; intensity and ambient in
  thousandths of a percent, the angle in 1/60000 degree like `fov`) and the reader parses it back
  exactly; a scene with a malformed member is dropped whole rather than half-read, so a scene this
  build cannot reproduce is never written back as a different one. An absent scene is the
  reader's default lighting.
- Undo: the step image is the **whole model shape** (`Image::Model3d` / `ShapeField::Model3d`,
  restored by `shape.setField { field: "model3d" }` of the recorded `Shape`, which checks the id
  and that the value is a model). One undo puts the scene, the camera and every other field of the
  object back, a redo returns the stated scene, and a foreign edit of the model after the step
  makes the undo `undo-conflict` with nothing written.
- Tests: two replicas converge on one scene (a 3-D model inserted from the media channel first)
  and the undo/redo restore the whole shape; a rectangle is refused `invalid` before any write
  with no history step, and a foreign move of the model turns the undo into `undo-conflict` with
  nothing written; the client normaliser pins the whole final scene, the `null` reset and the
  refused relative/spot/plain-shape cases; the proto round trip pins the wire form and the
  refusal of a relative field; the engine sets the scene, refuses a relative key, an unknown
  type, a zero direction, intensity/ambient out of range, the angle/type mismatch, a bad
  background and a plain shape without writing, and clears with `null`; the pptx corpus writes a
  camera and a scene on a model and reads both back through two round trips; the socket gate
  uploads the `.glb`, inserts the model, sets the scene on two editors and the viewer, commits
  and reimports the file with the same numbers, refuses a viewer's step `forbidden`, a relative
  field as a bad frame and a spot without an angle `invalid`, with the version staying at 2.
- Gates on `agents`: `cargo fmt --all -- --check` clean; `cargo clippy --workspace --all-targets
  -- -D warnings` clean; `collab` 258 + 8, `collab-proto` 39, `collab-server` 10 unit + 4 commit
  + 4 gates + 17 socket, `engine` 592 + 1 ignored, `pptx` 67 + 145 + 5, `render` 209 + the known
  host-without-`craft-fonts` failure (`east_asian_vertical_text_stands_ideographs_upright`),
  `ui-egui collab::` 20, `collab-web` 15, `deckcraft-web` 6, `xtask layers` 32 crates.
- Commit `7b51e65d` plus this docs commit on `collab/pr12`, branched from `collab/pr11` at
  `fe7bcbca`; `rfc.md`, `production.md` and this file updated. Nothing pushed, CI untouched,
  PR #1975 and #1998 unchanged.

### 2026-11-10 — a 3-D model's camera as one final view

- `model3d.camera` is collaborative on top of `insert.model3d`. The operation addresses one
  `Model3d` shape and carries the **whole final view** — `position`, `target`, `up` in slide EMU
  (the `am3d:pos`/`am3d:lookAt`/`am3d:up` units) and `fov` in degrees — or `null` for the
  reader's default view. There is deliberately no relative form: two replicas applying "turn 15°"
  or "zoom in" to their own views would end in two different views, which is exactly what the
  final-value rule exists to prevent (RFC §3.2.1).
- Validation happens in three places, all before the first write. The client normaliser runs the
  engine's own `camera_param`, so a relative field (`dx`, `dy`, `zoom`, `orbit`), a missing
  camera, a degenerate view (`position == target`, a zero `up`, an `up` parallel to the view
  direction) or a plain shape is refused before an operation is sent. The wire type
  (`collab_proto::ModelCamera`) is `deny_unknown_fields`, so a hostile frame with a relative
  field is a bad frame instead of a silently merged value. The engine command re-checks every
  number (finite, within ±1e9 EMU, `fov` 1–179°, up not degenerate) and addresses every target
  before it writes; a failing `edit` clones, so a refusal leaves the document untouched.
- The camera is part of the model: `ShapeKind::Model3d { media, poster, camera }` with a new
  `ModelCamera` value. The writer puts it into the slide part next to the content part as
  `am3d:camera` (`am3d:pos`, `am3d:up`, `am3d:lookAt`, `am3d:perspective fov` in 1/60000 degree,
  the layout Office keeps a 3-D model's rendering information in) and the reader parses it back
  exactly; a camera with a malformed member is dropped whole rather than half-read, so a view
  this build cannot reproduce is never written back as a different one. An absent camera is the
  reader's default view.
- Undo: the step image is the **whole model shape** (`Image::Model3d` / `ShapeField::Model3d`,
  restored by `shape.setField { field: "model3d" }` of the recorded `Shape`, which checks the id
  and that the value is a model). One undo puts the view and every other field of the object
  back, a redo returns the stated view, and a foreign edit of the model after the step makes the
  undo `undo-conflict` with nothing written.
- Tests: two replicas converge on one view (a 3-D model inserted from the media channel first)
  and the undo/redo restore the whole shape; a rectangle is refused `invalid` before any write
  with no history step, and a foreign move of the model turns the undo into `undo-conflict` with
  nothing written; the client normaliser pins the whole final view, the `null` reset and the
  refused relative/degenerate/plain-shape cases; the proto round trip pins the wire form and the
  refusal of a relative field; the engine sets the view, refuses relative keys, degenerate views,
  `fov` 0/180, an out-of-range coordinate and a plain shape without writing, clears with `null`,
  and refuses a forged whole-model restore (another shape's value, a non-model); the pptx corpus
  writes a camera on a model and reads it back through two round trips; the socket gate uploads
  the `.glb`, inserts the model, sets the view on two editors and the viewer, commits and
  reimports the file with the same numbers, refuses a viewer's step `forbidden`, a degenerate
  camera `invalid` and a relative field as a bad frame, with the version staying at 2.
- Gates on `agents`: `cargo fmt --all -- --check` clean; `cargo clippy --workspace --all-targets
  -- -D warnings` clean; `collab` 255 + 8, `collab-proto` 38, `collab-server` 10 unit + 4 commit
  + 4 gates + 16 socket, `engine` 591 + 1 ignored, `pptx` 67 + 145 + 5 + 6 + 8, `render` 209 + the
  known host-without-`craft-fonts` failure (`east_asian_vertical_text_stands_ideographs_upright`),
  `ui-egui collab::` 20, `collab-web` 15, `deckcraft-web` 6, `xtask layers` 32 crates.
- Commit `c406a329` plus this docs commit on `collab/pr11`; `rfc.md`, `production.md` and this
  file updated. Nothing pushed, CI untouched, PR #1975 and #1998 unchanged.

### 2026-11-10 — a picture in a SmartArt node as one final value

- `smartArt.picture` is collaborative as **one final value**: the operation
  (`smart_art_picture`, confirmed command `smartArt.setPicture`) addresses the top-level graphic
  (`target.slide` + `target.shape`, also when the user selected a child shape) and one top-level
  node of its outline, and carries only the media channel's digest, the file name and the
  picture's final place — the source crop (left, top, right, bottom as fractions of the image;
  negative pads) and the opacity — not the bytes. `submit` puts the bytes on the media channel
  first, as `insert.picture` does, so the operation stays small and the same object is stored
  once. The normaliser maps a selected sub-point to its top-level node and refuses a plain shape,
  a nested graphic, a node the outline does not have, a digest that is not a sha256, an unknown
  argument and a place that leaves no area before anything is queued.
- The engine command installs the bytes with `doc.add_media` — deduplicated by content and in the
  same order on every replica, exactly like a picture insertion — and regenerates the drawing, so
  every replica gets the same media id and the same picture. Before the first write it checks
  that the target is a **top-level** SmartArt group, that the node exists, that the object is a
  picture this document accepts (allowlist and the media channel's 8 MiB picture budget), that
  the place's crop is finite and leaves a positive visible area with an opacity in 0..=1, and that
  the layout does not change; `reset` clears the node's picture and place with no media at all.
- The place lives in the model (`DiagramNode.picture_place`) and is drawn on the generated
  placeholder's `PictureFill`; it round-trips through the diagram data part as `a:srcRect` on the
  node's presentation point, so a saved file keeps the crop.
- The step is merge class. The undo image is the **whole group** (`shape_field{field: smartArt}`):
  an undo puts the original node back with its diagram, its generated children and their ids and
  its previous picture (or none), and the redo returns the exact result; a foreign edit of the
  same graphic makes the undo `undo-conflict`.
- Tests: two replicas converge on one node's picture and place through the normaliser, the
  adapter and the in-memory hub; the undo restores the whole original node and the redo returns
  the result; a viewer's `submit` is refused `ReadOnly` and a hand-built operation is refused
  `Forbidden` by the server with the version and document unchanged, also over a real socket;
  hostile input (a plain shape, a nested graphic, an unknown node, media the session does not
  hold or an object that is not a picture, a crop that leaves no area, an opacity out of range) is
  refused before the first write with the document unchanged and no history step.
- Commit `44489f34` on `collab/pr11`, with this docs commit on top; `rfc.md` (§3.2.1, §6, §13,
  §14.1, §15), `production.md` and this file updated. Nothing pushed, CI untouched, PR
  #1975/#1998 unchanged. What remains in this line: a model's camera/scene — delivered later in
  `c406a329` and `7b51e65d` — then relative effects, which stay local by design.

### 2026-11-10 — SmartArt conversions: from text, to text, to shapes

- The three conversions are collaborative as **one structural step each**:
  `smartArt.fromText`, `smartArt.toText` and `smartArt.toShapes`. The sender runs the same plan
  the single-user command runs — `smartArt::from_text_plan` (paragraphs with their indent levels
  and the grown box), `smartArt::to_text_plan` (the outline as a whole `TextBody` with its levels,
  bullets and the graphic's own title/bullet sizes) and `smartArt::to_shapes_plan` (only the
  graphic's id) — so the operation carries the whole result rather than arguments a replica would
  apply to its own copy. The confirmed engine commands are `smartArt.replaceWithDiagram`,
  `smartArt.replaceWithText` and `smartArt.detach`.
- Before the first write the engine checks the value: the layout, node count, node text, style,
  colours and JSON size of a diagram; the JSON size, paragraph count and run count of a text body;
  the box (four finite numbers, a positive and sane size); and that the target is a top-level text
  shape (`fromText`) or (inside) a SmartArt graphic (`toText`/`toShapes`). A refusal writes
  nothing and moves no version.
- The result replaces the source shape in its place in the z-order as one structural step. The
  undo image is the **whole source shape** — a text box, or the graphic with its diagram and
  generated children and their ids — and the redo image is the whole converted shape. A new
  `ShapeReplacement` history image holds both sides; when `toShapes` keeps the id, the restore
  removes the slot before inserting the image back, so a same-id replacement is not a clash.
- Tests: two replicas converge on each conversion through the normaliser and the adapter, and on
  the in-memory hub; each undo restores the source shape whole and each redo reapplies the exact
  result; a viewer's `submit` is refused `ReadOnly` and a hand-built operation is refused
  `Forbidden` by the server (the version and document stay); a hostile diagram/body/box and a
  plain shape addressed as a graphic are refused `invalid` with no document change and no history
  step; the socket test sends a conversion and gets a viewer's refusal over a real connection.
- Commit `db413e5d` on `collab/pr11`, with this docs commit on top; `rfc.md` (§3.2.1, §6, §13,
  §14.1, §15), `production.md` and this file updated. Nothing pushed, CI untouched, PR #1975
  unchanged. What remains in this line: nothing — `smartArt.picture` is delivered in `44489f34`
  (the note above) and the model's camera and scene in `c406a329` and `7b51e65d`; only relative
  effects stay off the wire by design.

### 2026-11-10 — SmartArt insertion as one whole value and the authoritative nodes query

- `insert.smartArt` is collaborative: the normaliser builds the graphic its command would create
  with the engine's `smartart::insert_plan` (layout, every node with text/level/assistant/title/
  picture, the default three `Text` nodes when the list is empty, the default `process` layout for
  an unknown kind) and sends that **whole `Diagram` value** in the new `smart_art_insert` payload,
  with the optional final box. The engine command accepts the value, checks layout, style,
  colours, at most 4096 nodes, 64 KiB of node text and 128 KiB of JSON before the first write, and
  regenerates the group's children from it — so every replica draws the same picture instead of
  running its own sample, and a hostile tree adds no shape and moves no version.
- The address is the slide (`target.slide`); an insertion is structural. Its undo image is the
  **whole inserted shape** (`shape_image`, children and their ids included): an undo is one
  structural `shape.delete` step and a redo is one `shape.restore` that puts the diagram and the
  generated children back. No relative argument or per-node patch is replayed.
- `smartArt.nodes` is now a **server-side read-only query** from an explicit allowlist:
  `client → query {id, cmd, target, params}` → the server resolves the slide and shape by session
  key, runs the engine's query on the authoritative session, restores the engine selection and
  answers `query_result {id, version, result | error}` to the asker alone. It moves no version,
  enters neither the journal nor an undo history, and a viewer may ask it; a mutating, unknown or
  non-allowlisted command is an error in the answer. The allowlist matters because an arbitrary
  engine query could render or read the server's files. `CollabDoc::query` /
  `take_query_answer` expose the same path to a client, and capabilities list `insert.smartArt`
  and `smartArt.nodes`.
- Tests: two replicas converge on one inserted graphic (layout, node tree, box) and its undo
  removes the whole shape / its redo restores it on both; a hostile tree refused `invalid` with the
  document unchanged and no history step; the normaliser's defaults and refusals; the engine's
  whole-value insertion, its param path and its refusals; a viewer refused on the client and a
  hand-built operation refused by the server without a version move; the `smartArt.nodes` query
  answered to the asker alone (a viewer too), with an unknown/mutating command refused and no
  version move; and the same insertion, viewer query and hostile-tree refusal over a real socket.
- Commit `cfd40da2` on `collab/pr11`, with this docs commit on top; `rfc.md` (§3.2.1, §6, §14.1,
  §15), `production.md` and this file updated. Nothing pushed, CI untouched, PR #1975 unchanged.

### 2026-11-10 — CORS on the sync server for the browser media channel

- The last deployment gap of the media channel is closed: `DECKCRAFT_CORS_ORIGINS` (comma
  separated) is the origin allowlist. While it is set every HTTP answer carries `Vary: Origin`;
  an admitted origin also gets `Access-Control-Allow-Origin: <origin>` — the origin itself, never
  `*`, so the browser may attach the `Authorization` header the media `fetch`/upload needs — and a
  foreign origin gets no `Access-Control-*` header. An empty or absent variable keeps the earlier
  behaviour exactly: no `Access-Control-*`, no `Vary`, and `OPTIONS` falls through to the router.
- `OPTIONS` with an `Origin` is answered as a preflight: `204` with
  `Access-Control-Allow-Methods: GET, POST, PUT, OPTIONS` and
  `Access-Control-Allow-Headers: Authorization, Content-Type`, plus
  `Access-Control-Max-Age: 600` (`PUT` is listed because the browser uploads media with it); a
  method outside the list is `405`, a foreign origin gets `Vary` only. A preflight carries no
  token, so the answer names methods and headers and never a room, a media object or a version.
- Tests in `tests/socket.rs`: preflight for an admitted origin, a `GET /media/{sha}` from an
  admitted origin (header and `Vary`, bytes unchanged), a foreign origin (no `Access-Control-*`
  on either the GET or the preflight), and the empty allowlist (no header at all, the old
  `OPTIONS` routing). On `agents`: `cargo fmt --all -- --check` clean,
  `cargo clippy --workspace --all-targets -- -D warnings` clean, `cargo test -p
  deckcraft-collab-server` 30 passed (10 unit + 4 commit + 4 gates + 12 socket), 0 failed,
  0 ignored, EXIT=0.
- `production.md` documents the variable, the preflight and the three deployment mentions; the
  server-side CORS gap is gone from the known gaps below. Not pushed; PR #1998 and #1975 are
  untouched.

### 2026-11-10 — 3-D models on the media channel, drawn instead of hidden

- The last media gap from `production.md` P1 is closed: a 3-D model is a media object like a
  picture. `MediaType::Model` accepts exactly one signature — a binary glTF (`glTF` magic, version
  2, at least the 12-byte header) — with a **budget of its own** (`MAX_MODEL_BYTES` 32 MiB per
  object, 16 objects and 128 MiB per session), counted apart from pictures, fonts and audio/video,
  so a model cannot evict them. The name is never the allowlist: a `.glb` name with other bytes is
  refused, and a replay with no name is still recognised by the signature.
- The operation is the existing `media_insert` with a new `kind: model` (`MediaKind::Model`), so
  the bytes travel on the channel and only the digest, the name and the final box go on the wire;
  the adapter runs the new engine command `insert.model3d`, which stores the part as
  `model/gltf-binary` and adds a `Model3d { media, poster }` shape (`p:contentPart` with a
  `model3d` relationship). The writer already copies such a part through; nothing about the model
  itself is edited in v1, so there is no partial-value problem to solve.
- A model is now **drawn** instead of warned about: the renderer paints the `mc:Fallback` poster
  when the file carries one, else a stand-in isometric cube on the frame (a new `paint::model_glyph`),
  so a shared session never shows an empty box for a model. The commit warning changed from
  "kept in the file, not drawn" to "drawn as its poster or a placeholder, not as a rendered model" —
  the honest limitation left is the missing glTF tessellator, not a silent drop.
- `GET /media` normalises the path (query, fragment, trailing slash and an extension such as
  `{sha}.glb` are not part of the name; another path segment or a non-digest is `404`) and keeps
  answering with the content type the signature implies, so a model uploaded once is served as
  `model/gltf-binary` from memory and after a restart.
- Undo is the ordinary structural step a shape insert has: the image is the inserted shape
  (`shape_delete` on undo, `shape_restore` on redo), because the insertion adds one shape on top of
  the z-order and no model attribute is editable yet.
- Tests: two replicas converge on one `media_insert { kind: model }` (same delta, same shape, same
  box, the part keeps `model/gltf-binary`) and a picture object named as a model is refused
  `invalid` before any write; its undo removes the model on both replicas and leaves the slide
  exactly as it was; the normaliser uploads the `.glb` and sends a reference only; the channel
  test covers the signature (version 1 and a short header refused), the per-object cap and the
  separate model budget; a real socket test uploads a model and fetches it by digest, with an
  extension and with a query, checking the declared content type, and refuses a `.glb` name with
  other bytes; the renderer test draws the poster when present and the cube when not, with nothing
  outside the frame.
- Gates on the `agents` host, no push: `cargo fmt --all -- --check` clean; `cargo clippy
  --workspace --all-targets -- -D warnings` clean; `cargo test -p deckcraft-collab` 234 + 8,
  `-p deckcraft-collab-proto` 34, `-p deckcraft-collab-server` 10 unit + 8 socket + 4 commit +
  4 gates (LibreOffice skipped on the build host) — 0 failed, 0 ignored; `cargo test -p
  deckcraft-render a_3d_model` and `-p deckcraft-engine` green.
- Commit `3ccd815f` on `collab/pr11`, with this docs commit on top; `rfc.md` (§3.2.1, §4.1,
  §14.1), `production.md` (media channel, the P1 item, the test table) and this file updated.
  Nothing pushed; CI untouched; PR #1998's head is unchanged.

### 2026-11-10 — charts: type, data, options and insertion as one whole chart value

- Three commands are collaborative: `chart.type`, `chart.data` and `chart.options`. A chart edit
  leaves the chart as **one value**, so the new `chart_set` payload carries the **whole resulting
  `Chart`** — type, categories (with outer levels), series (names, values and formatting), the
  element options, the style/palette and any kept part — and the address is the chart shape.
- One source of truth: the engine's `insert.chart` sample-and-apply code and the three edit
  commands moved into a public `chart::plan`, which the single-user commands now call; the
  normaliser takes the chart the author sees, runs the same plan and sends the result. On the
  receiver the adapter runs the new engine command `chart.setWhole`, which deserialises the value
  into the model `Chart`, checks the type, at most 4096 categories, series and 100 000 points in
  a series, at most 64 KiB of chart text and 192 KiB of JSON, and only then replaces the chart.
  A value on a shape that is not a chart, too many series and an oversized value are refused
  before the first write and move no version.
- `insert.chart` is collaborative too: `chart_insert` carries the whole chart its command would
  create (the normaliser builds the sample and applies the typed data through the same plan) plus
  the optional final box, so every replica creates exactly that chart instead of running its own
  sample; the adapter calls `insert.chart` with the value and the engine checks it with the same
  function. Its undo is the ordinary structural shape-delete step.
- Undo: the image is the **whole chart shape** (`shape_field{field: chart}` — the model `Shape`
  as JSON, box included), because the user-facing chart commands express only part of the model
  and a kept chart part (an embedded workbook or drawing) cannot be replayed from arguments; the
  engine's `shape.setField` gained a `chart` field that puts the shape back in place (and refuses
  an image that names another shape). A foreign edit of the same chart makes the undo
  `undo-conflict`.
- Capabilities list `insert.chart`, `chart.type`, `chart.data` and `chart.options`.
- Not in this slice: a chart whose kept parts do not fit the 192 KiB value bound (refused with a
  clear message; moving those parts onto the media channel is the later fix), the chart's own
  `chartEx`-specific editors beyond the three commands, and `insert.smartArt`.
- Tests: two replicas converge on one whole value (type, data and captions in one step); its undo
  restores the whole chart shape and its redo reapplies it on both; an undo refused
  `undo-conflict` after another participant edited the chart; a value on a plain shape refused
  `invalid` with no history step; an insertion reaching both replicas as the same shape with the
  same box, and its undo removing it; the normaliser for all three commands and for
  `insert.chart` (an unknown parameter refused); the engine refusals (unknown type, too many
  series, an oversized value, a plain shape) and the exact `shape.setField{field: chart}`
  restore; the same step and undo through the in-memory hub.
- Commit `f67b0858` on `collab/pr11`, with this docs commit on top; `rfc.md` (§3.2.1, §6, §11,
  §13, §14.1), `production.md` and this file updated. Nothing pushed, CI untouched, PR #1975
  unchanged.

### 2026-11-09 — SmartArt: outline, layout, style and colours as one whole diagram value

- Eight commands are collaborative: `smartArt.setNodes` (every form: `nodes`, `lines`,
  `index`+`text`, `index`+`title`), `addNode`, `removeNode`, `promote`, `demote`, `layout`,
  `style` and `colors`. Each leaves the graphic as **one value**, so the new `smart_art_set`
  payload carries the **whole resulting `Diagram`** — layout, outline (text, level, assistant,
  title, picture), style and colours — and the address is the group (`target.slide` +
  `target.shape`, also when the user selected a child shape).
- One source of truth: the engine's outline/layout/style/colour mutations moved into a public
  `smartArt::plan`, which the single-user commands now call; the normaliser takes the graphic's
  diagram (with text typed into its shapes synced back, as before every regeneration), runs the
  same plan and sends the result. A layout change drops the file's saved layout and a style/colour
  change its own XML exactly as the single-user command does, so the sent value is the value that
  command would leave.
- The adapter resolves the group, checks that the target is (inside) a SmartArt graphic and the
  value fits one message, then runs the new engine command `smartArt.setDiagram`: it
  deserialises the value, checks the layout, style, colours, at most 4096 nodes and at most
  64 KiB of node text and 128 KiB of JSON, and only then regenerates the group's children.
  Refusals write nothing and move no version.
- Undo: the image is the **whole group** (`shape_field{field: smartArt}` — the model `Shape` as
  JSON, diagram and generated children with their ids), because an outline edit regenerates the
  children with fresh ids and replaces any drawing an imported file brought; the engine's
  `shape.setField` gained a `smartArt` field that puts the group back in place (and refuses an
  image that names another shape). A foreign edit of the same graphic makes the undo
  `undo-conflict`.
- Capabilities list the eight command names; the payload command on the wire is
  `smartArt.setDiagram`. Not in this slice: `insert.smartArt`, `fromText`/`toText`/`toShapes`
  (structural), `smartArt.picture` (needs the media channel for the node image) and the
  `smartArt.nodes` query; charts are the next slice.
- Tests: two replicas converge on one whole value (layout, outline, style and colours in one
  step); its undo restores the previous diagram **and the generated children with their own ids**
  and its redo reapplies it on both; an undo refused `undo-conflict` after another participant
  edited the graphic; a step on a plain shape refused `invalid` with no history step; the
  normaliser running the engine's plan for all eight commands (a child id addresses the group, a
  layout change keeps the outline) and refusing an unknown layout/style/colour, a missing node,
  a plain shape, an empty selection and the structural commands outside the slice; the engine
  regenerating from a whole value, the exact group restore and the value/size refusals; the same
  step and undo through the in-memory hub.
- Commit `d6c2df6c` on `collab/pr11`, with this docs commit on top; `rfc.md` (§3.2.1, §6, §13,
  §14.1), `production.md` and this file updated. Nothing pushed, CI untouched, PR #1975 unchanged.

### 2026-11-09 — table style options and the Cell Size fields

- `table.options` is collaborative as **one final set**: the six independent booleans
  (`headerRow`, `totalRow`, `bandedRows`, `firstColumn`, `lastColumn`, `bandedColumns`) travel
  together in the new `table_options` payload, addressed by the table shape. The sender applies
  the flags it names to the table it sees and sends the whole resulting set, so every replica
  ends with the same six values and no replica's stale copy decides an unnamed option.
- Refusals before any write: an unknown or misspelt flag (`bandedRow`), a value that is not a
  boolean, and a call that names no option at all are refused by the client's normaliser **and**
  by the engine command, which now validates every key and value before `with_table` runs. The
  old engine command ignored unknown keys and non-boolean values silently, so a typo looked like
  a successful no-op; the command test pins the refusal and the unchanged table.
- Undo: the step image is the whole table (`shape_field` `field: table`), the same image the rest
  of the table surface records, restored by `shape.setField`; a foreign edit of the table makes
  the undo `undo-conflict`. The adapter classifies `table_options` as `Command` (a merge-class
  value: no cell address moves).
- Ribbon: Table Layout ▸ Cell Size gains "Column Width" and "Row Height" number fields. They show
  the selected column's width and row's height (the caret's cell, the start of the cell selection,
  or the table's first cell) and run `table.columnWidth`/`table.rowHeight` — the same commands a
  border drag runs, so a live session queues them like any other edit.
- Tests: two replicas converge on one options step (all six flags final), its undo restores the
  whole table and its redo reapplies the set on both; the same reaches both replicas through the
  hub with one undo step; the normaliser emits the whole set and refuses an unknown flag, a
  non-boolean and an empty call; the engine command refuses them without touching the table; the
  wire form carries all six fields; the ribbon's two fields change the column and the row and the
  shape box follows.
- Commit `05abfe28` on `collab/pr11`, with this docs commit on top; `rfc.md` (§3.2.1, §6, §13,
  §14.1), `production.md` and this file updated. Nothing pushed, CI untouched, PR #1975 unchanged.
- Not in this slice: SmartArt/charts and 3-D models remain refused; a per-cell undo image for the
  table (the whole table is exact and matches `table.edit`) and the style options' own UI beyond
  the Table Design checkboxes would be later refinements.

### 2026-11-09 — the rest of the table surface: style, cell fill, merge/split and sizing

- Four command groups are collaborative as final values addressed by the table: `table.style`
  (one style name), `table.cellFill` (one fill — solid, `none` or `inherit` — over an explicit
  cell list), `table.merge`/`table.split` (final corners or the merge anchor) and
  `table.columnWidth`/`table.rowHeight` (final numbers, `keepTotal` taking the difference from
  the next column). New payloads `TableStyle`, `TableCellFill`, `TableMerge`, `TableSplit`,
  `TableColumnWidth`, `TableRowHeight` map 1:1 to the engine commands; `TableEdit` keeps its
  structural class, the six new ones are `Command` because a merge or a split moves no cell index.
- Normalise: the sender resolves the table (explicit `id`, the caret's cell, the selected cell
  range or the one selected shape) and, for a fill, expands the selection into an **explicit**
  cell list (the engine's own `selected_range` rule for merged cells, bounded at 4096 cells), so
  the receiver's selection never decides what changes. Width and height travel clamped to the
  engine's 4–4000 pt; a style name is trimmed and bounded.
- Adapter: one `table_of` helper resolves the slide and shape and requires a table; every cell,
  column and row is validated before the first write; the table shape is selected and the
  single-user command runs with the table id, so all replicas reach the same result.
- Undo: because a style, a fill, a merge or a size changes the table without naming a cell that
  could be restored on its own (and the `table.*` commands cannot express the previous value —
  a cell fill is an `Option<Fill>`, a merge moves text), the step image is the **whole table**,
  the same image `table.edit` records, restored with `shape_field{field: table}`. The engine's
  `shape.setField` now also sets the shape's box to the restored table's size, so undoing a
  width or a row height moves the box back (the engine test pins it). A foreign edit of the
  table after a step — including text typed in one of its cells — makes the undo `undo-conflict`.
- Rebase: a queued `table.cellFill` carries its cell addresses, so a row/column edit moves them
  like a cell text offset and deleting their line refuses the operation `stale`. A merge that
  covered an addressed cell (it is covered now, its text is in the anchor) refuses the queued
  text or fill operation `stale` instead of writing into an invisible cell; split moves nothing.
- Server: the welcome's `capabilities.commands` lists the six command names.
- Tests: two replicas converge on a style and a fill (the neighbour untouched, `inherit` clearing
  the explicit fill and `none` setting an empty one), on a merge and a split (spans, covered
  cells, anchor text) and on a column width with `keepTotal` (the table's total unchanged) and a
  row height (the box follows); undo of a style, a fill, a width, a height and a merge, one whole
  table per step, on two replicas with redo; an undo refused `undo-conflict` after someone typed
  in the table; unknown cells, an empty cell list, a merge past the table or of one cell, a split
  of a cell that is not merged, a missing column/row, an empty style, a shape that is not a table
  and a target without a shape all refused before the first write with no step and no version
  move. The normaliser pins the explicit cell lists (caret, range, whole table, explicit),
  clamping and every refusal. The journal tests pin a queued fill following an inserted row and a
  queued text and fill refused `stale` after a merge; the hub test runs style, fill and row
  height on two replicas and two undos, each one step.
- Gates (on `agents`, rust 1.99.1): `cargo fmt --all -- --check` clean, `cargo clippy
  --workspace --all-targets -- -D warnings` clean, `collab` 215 + 8, `collab-proto` 32,
  `collab-server` 10 unit + 4 commit + 4 gates (2.43 s, LibreOffice skipped) + 7 socket,
  `collab-web` 15, `deckcraft-web` 6, `ui-egui collab::` 20, `deckcraft-engine` 584 + 1 ignored,
  `xtask layers` 32 crates, the wasm32 check of `deckcraft-web` ok. `cargo test --workspace
  --no-fail-fast` is green except the two known host-without-`craft-fonts` failures
  (`ui-egui ime_tests::korean_composes_in_the_notes_pane` and `render
  tests::east_asian_vertical_text_stands_ideographs_upright`).
- Commit `c895183a` plus this docs commit on `collab/pr11`; `rfc.md` (§3.2.1, §4.1, §6, §13,
  §14.1), `production.md` and this file updated. Nothing pushed, CI untouched, PR #1975 unchanged.
- Not in this slice: `table.options` (the style toggles are independent booleans, a separate
  final-value choice), a ribbon gesture or input for column width / row height (the commands are
  reachable through the control channel and MCP), and a fine-grained undo image per cell
  (the whole table is exact and matches `table.edit`; a per-cell image is a later refinement).

### 2026-11-09 — document fonts: an embedded-font registry on the wire

- A font is a **document asset**, not a shape value: `p:embeddedFontLst` carries one entry per
  typeface with up to four face slots, and the face bytes are ordinary media objects. The slice
  keeps that shape. Engine module `crates/engine/src/cmd/font_asset.rs` adds four commands:
  `font.create` (one face from a file — `typeface`, `face`, `data`/`path`), `font.set` (the
  entry's `panose`/`pitchFamily`/`charset` as final values), `font.remove` and `font.restore`
  (the exact whole entry, or `null`, for collaboration undo). The engine validates the sfnt
  header and table directory, caps the file at 16 MiB, stores it as `application/x-fontdata`,
  installs the face in the font database so text draws immediately, compares a typeface
  case-insensitively, and validates every face id of a restore before the first write.
- Protocol (`crates/collab-proto`): `Payload::FontCreate { typeface, face, sha256, name }`,
  `FontSet`, `FontRemove` and `FontRestore { typeface, font: Option<Value> }`, with
  `FontFace { regular, bold, italic, boldItalic }`. A font operation names no slide: it is
  presentation-level, merge class, and changes no address or key.
- Media channel: `MediaType::Font`, an sfnt/OpenType/collection/EOT signature allowlist, a 16 MiB
  per-face cap and its own 32-object / 64 MiB budget, so a font cannot evict pictures. The
  browser link prefetches a `FontCreate` digest like a picture's, and the desktop client fetches
  it through the same `apply_command` hook before applying.
- Normalise and adapter: `CollabDoc::submit` intercepts `font.create` and puts the bytes in the
  media cache (`take_uploads` for the host), so the operation carries only the digest; the
  normaliser validates the name, face, digest and descriptors and runs before the slide-view
  guard, because the command does not need a slide. The adapter resolves `sha256` in its media
  cache (`missing_media`, retryable, when absent), refuses a digest that is another kind of
  object and a font step that names a slide, then runs the engine command.
- Undo: `history::Change::Font` keeps the whole entry before and after (the model
  `EmbeddedFont`, media ids included) and `restores()` emits one `font_restore`.
  `Restore.slide` became `Option<SlideKey>` — a font is the first presentation-level value with
  no slide to address. `capture_font` re-reads the entry for `step_matches`, so any later change
  to that entry (another participant's face or descriptor) makes the undo `undo-conflict`, while
  a change to another typeface does not.
- Tests: eight in `deckcraft-collab` (two replicas converging on create/set/remove; the undo of a
  descriptor step and of the create broadcast through the hub to both replicas, with the face
  object kept for the redo; an undo refused after a foreign change; slide-addressed, non-font,
  missing-upload, unknown-entry and empty-set refusals before any write; the normaliser, the
  upload path and the media allowlist/budget) and a socket test that uploads a face, applies
  `font_create` to the room and commits a file that reimports with the entry. The engine module
  tests cover the four commands, the validation and the save/reimport.
- Gates (on `agents`, rust 1.99.1): `fmt --check` clean, clippy `--workspace --all-targets -D
  warnings` clean, `collab-proto` 31, `collab` 205 + 8, `collab-server` 10 + 4 commit + 4 gates
  (2.44 s) + 7 socket, `collab-web` 15, `deckcraft-web` 6, `engine` 584 + 1 ignored, `ui-egui
  collab::` 20, `xtask layers` 32 crates, wasm32 check of `deckcraft-web` ok. `cargo test
  --workspace` is green except the known host-without-`craft-fonts` failures in `ui-egui`
  (Korean IME) and `render` (east-asian vertical text). Commit `fd4e54e8` plus this docs commit
  on `collab/pr11`; nothing pushed, CI untouched, PR #1975 unchanged.
- Not in this slice: a UI entry point ("Embed font…" dialog) and embedding *every* used family in
  one step; both are follow-ups, the commands are reachable through the control channel, MCP and
  the animation-free command surface.

### 2026-11-09 — the rest of the animation surface: `animation.options`, `animation.clear`, the painter

- The three commands leave a slide's **whole ordered animation sequence**, so they ride the
  existing `slide_animations` payload and the `slide.animations.set` confirmed command: no new
  wire value and no new engine command, and the undo image is the whole sequence (the image the
  `animation.set`/`timing`/`move` family records), so any animation change on that slide after
  the step is `undo-conflict`.
- Engine: `animation.options` moved onto a new public `options_plan(p, animations, index, ids,
  slide) -> (Vec<Animation>, usize)` (the `timing_plan` pattern); the per-target body — the
  option resolved in each effect's own terms, the motion path for path-class steps, the text
  build — is unchanged, so the single-user command and the normaliser compute the same codes.
  The Animation Painter moved onto a new public `painter_plan(picked, animations, ids,
  paragraphs)` plus `paragraphs(shapes, id)`: painting replaces each target's own steps with the
  picked list, and a paragraph the target does not have becomes a whole-shape step, exactly as
  before. The then-unused `each_target` was removed.
- Normalise: `normalize_with_painter` (with `normalize` delegating to it with no clipboard)
  turns `animation.options` into the slide's whole resulting sequence (nothing when it changes
  nothing), `animation.clear` into an empty sequence for the slide it addresses (`index` is a
  slide, not a step; a slide with no steps sends nothing) and `animation.painterApply` into the
  sequence `painter_plan` computes from the session clipboard. The clipboard is a session value,
  not a document value, so it travels as the resulting sequence and no replica needs it; a
  missing clipboard, no target and an unaddressable target are refused before anything is sent.
- UI: `CollabUi::pump` disarms a one-shot pick once the paint is queued, because the confirmed
  command is `slide.animations.set` and the single-user `painterApply` never runs in a session.
- Tests: the engine plans match the single-user commands (options by index, the painter's
  paragraph fallback); the normaliser pins the three forms, their no-ops and their refusals; the
  hub runs `animation.options`, `animation.painterApply` (clipboard in the session) and
  `animation.clear`, each with its undo on two replicas; the UI test pins the sequence over the
  link and the disarm. `collab` 197 + 8, `collab-proto` 30, `engine` 580 + 1 ignored, `ui-egui
  collab::` 20.
- Gates (on `agents`, rust 1.99.1): `cargo fmt --all -- --check` clean, `cargo clippy
  --workspace --all-targets -- -D warnings` clean, the four §8 gates 4/4 in 2.45 s (LibreOffice
  skipped on the build host), `xtask layers` 32 crates, the wasm32 check of `deckcraft-web` ok.
  Commit `e2a5ef41` plus this docs commit on `collab/pr11`; `production.md`, `rfc.md` and this
  file updated. Nothing pushed, CI untouched, PR #1975 unchanged.

### 2026-11-09 — the rest of the animation surface: `animation.set`, `animation.timing`, `animation.move`

- The three commands are collaborative as **one slide's whole ordered animation sequence**
  (`Payload::SlideAnimations { animations: Vec<Value> }`, `target.slide` the address). Every value
  is the model `Animation` as JSON and carries its own shape id, because `animation.move` can put a
  step past another shape's step; the per-shape `shape_animations` list could not express that
  interleaving. A later list from another participant is the later absolute value.
- Normalise runs the same plans as the single-user commands — new public
  `deckcraft_engine::cmd::animation::set_plan`, `timing_plan`, `move_plan`, with `set`, `timing`
  and `move_anim` refactored onto them. `animation.set` on a selection is one operation (the whole
  sequence once), not one version per shape; `animation.timing` by `index` changes exactly that
  step and by shape ids exactly those shapes' steps; `animation.move` clamps `to` to the end as
  the single-user command does. A command whose result equals the current sequence sends nothing,
  and an unknown effect, a missing `index`/`to`, an out-of-range index or a timing edit that names
  no step is refused `BadParams` before anything is sent.
- Engine: new `slide.animations.set { index, animations }` replaces the whole list. It bounds the
  image (4096 steps, 192 KiB), accepts only the model's own field names, requires a known effect,
  a bounded option and motion path, and validates that **every** named shape is on that slide
  before the first write; the shared `read_steps` also serves the existing per-shape
  `slide.animation.set`. The soak skips the new exact-value setter for the same reason as the
  other two.
- Adapter: resolves the slide, refuses a shape key in the target, reads every value as its own
  model animation, and runs `slide.animations.set`. Undo records the slide's whole sequence before
  and after (`Image::SlideAnimations`, `Change::SlideAnimations`) and restores it through the same
  payload; `same_state` compares the slide and the whole list, so any animation change on that
  slide after the step (including another shape's) makes the undo `undo-conflict`, while a change
  to another slide's steps does not.
- Tests: two replicas converge on `animation.set` and on the undo of the second shape's set;
  `animation.timing` changes one step and its undo restores the exact duration; `animation.move`
  crosses another shape's step on both replicas and its undo restores the whole order; a foreign
  animation edit makes the undo a conflict and writes nothing; the engine keeps interleaved steps
  through an exact list, refuses a missing shape/unknown effect/unknown field/too many steps
  before any write, and its plans match the single-user commands; the normaliser emits the exact
  sequence for set/timing/move, sends nothing for a no-op, and both wire forms round-trip.
  `collab` 195 + 8, `collab-proto` 30. Commit `b31bff47` plus this docs commit on `collab/pr11`;
  `production.md`, `rfc.md` and this file updated. Nothing pushed, CI untouched, PR #1975
  unchanged.

### 2026-11-09 — rebase onto `origin/main` (`0623b430`)

- `collab/pr10` (55 commits) rebased onto `origin/main` → `7459d798` (+ a short rebase note on that
  branch, which is the PR head); `collab/pr11` (13 commits, `--onto collab/pr10 922a4bf4`) →
  `da3199bf` (+ this docs commit). Pre-rebase branches kept as `backup/pr10-pre-rebase` and
  `backup/pr11-pre-rebase`. `merge-tree --write-tree` against `origin/main` exits 0 with no
  conflicted path for both heads, so PR #1975 is `MERGEABLE` once the forks are force-pushed.
- Four files conflicted, all resolved as unions (rationale in the rebase section above):
  `crates/engine/src/cmd/shape.rs` (import list), `apps/deckcraft-web/Cargo.toml` (page API +
  collab deps, host crate still the fork pin), `apps/deckcraft-web/src/web.rs` (`api::install` +
  the collaboration `WebShell` literal) and `Cargo.lock` (fork source for `craft-web-host`).
- `git diff backup/pr11-pre-rebase collab/pr11` is main's own `159ed07e..origin/main` patch for the
  same 29 files, byte for byte except `Cargo.lock`; no collab code was added or dropped.
- Gates on `agents`: `fmt` clean, `clippy --workspace --all-targets -D warnings` clean,
  `collab-proto` 29, `collab` 192 + 8, `collab-web` 15, `collab-server` 10 + 4 + 4 gates + 6
  socket (gates 2.52 s), `deckcraft-web` 6, `ui-egui collab::` 19, `ui-egui` full 515 + the known
  Korean-font environment failure, `engine` 577 + 1 ignored, `xtask layers` 32 crates, wasm32
  check of `deckcraft-web` ok. Nothing pushed, CI untouched.

### 2026-11-09 — N cells and a caret in one format step, with an exact per-cell undo

- `Payload::FormatSet` now carries `addresses: Vec<FormatAddress>` instead of one `cell` and one
  `range`. Each `FormatAddress` is `{ cell?: [row, col], range?: {from, to}, caret?: offset }`:
  a table cell or the shape's own body, whole, over a character range, or at a caret. An
  operation over one shape is either one body address or **N cell addresses of one table**;
  a list that mixes the two, an empty list, a repeated cell, a cell the table does not have and
  a range or caret past the addressed body are refused before the first write (version does not
  move, no step recorded).
- A selection of several cells normalises to N addresses, one per actual cell the engine would
  format (`selected_range`/`in_range`, so merged cells follow the engine's own rule), never to
  one address widened to the table. The adapter validates every address, places the engine
  selection per address (cells for whole-cell addresses, `TextSel` for a range/caret) and runs
  the single-cell command each time, restoring the receiving user's selection once at the end.
- Undo: `capture_step` records one whole-body image per address. `restores` emits `cells_text`
  (`shape.setCellBodies`) for several cell bodies of one table, and each replica writes one
  `shape.setField { field: cellText }` per cell after validating all of them. The step's `before`
  and `after` images are compared per cell, so a foreign edit of **any** addressed cell makes the
  undo `undo-conflict`; a neighbour that is not in the list is never read or written.
- A caret in a shape's own body is now an address (`caret: offset`) instead of a refusal. The
  normaliser emits the whole-body offset of the collapsed caret; the server places a collapsed
  `TextSel` on the authoritative body and runs the single-user command, so the engine decides
  what it touches: a run command widens it to the word it is in, a paragraph command takes the
  paragraph, and a caret at a word boundary only sets the formatting for what is typed next — the
  replica never widens it up front.
- Journal rebase: `rebase_text` carries a cell address per address, so a row/column edit moves
  every addressed cell (and deleting the line of any of them refuses the queued step `stale`),
  and an insert/delete shifts a range's two offsets or a caret only in its own body. A range that
  a delete emptied is refused `invalid`; a caret never empties.
- Limits (RFC §4.1): at most 4096 addresses per `format_set`, and at most 4096 cell bodies /
  192 KiB JSON per `cells_text`; past that the operation is `invalid`, because it and its undo
  would not fit the wire limit anyway.
- Tests: two replicas converge on one operation over two cells with a neighbour untouched; undo
  restores exactly the addressed cells' bodies in one `cells_text` and redo reapplies them; an
  undo after a foreign edit of one addressed cell is `undo-conflict` and writes nothing; empty,
  missing, out-of-body and repeated addresses are refused with the version unchanged; a remote
  N-cell format keeps the receiver's selection; a queued N-cell format follows an inserted row
  for all its addresses; a caret inside a word bolds the word on both replicas (undo/redo), a
  caret at a word boundary leaves existing runs alone, a paragraph command at a caret takes only
  its paragraph, and a caret past the body is `invalid` before any write; the normaliser pins one
  address per selected cell and a `caret` for a shape body; both wire forms round-trip.
  `collab` 192 + 8, `collab-proto` 29, `ui-egui collab::` 19, gates 4.
- Gates (on `agents`, rust 1.99.1): `cargo fmt --all -- --check` clean, `cargo clippy --workspace
  --all-targets -- -D warnings` clean, `xtask layers` 32 crates. Commit `6ddc0c4f` plus this docs
  commit on `collab/pr11`; `production.md`, `rfc.md` and this file updated. Nothing pushed, CI
  untouched, PR #1975 unchanged.

### 2026-11-09 — formatting a run range, with the exact-body undo

- `format.*` can address a character range inside one text body: a shape's own body or one table
  cell. `Payload::FormatSet` gained `range: Option<TextRange>`, `TextRange { from, to }` — whole-body
  character offsets, a paragraph break counted as one, the same coordinates `text.insert`/
  `text.delete` and `notes.insert` use. The engine needed no new command: the adapter puts the
  engine's `TextSel` on the target body with `anchor`/`caret` at the two positions (the table cell
  in `cell`), runs the single-user `format.*`, and puts the receiving user's selection back —
  formatting moves no text, so it stays valid.
- Why offsets, not run indices: an edit on another replica splits and merges runs, so a run index
  is not deterministic; a character offset is. Every replica turns the same two offsets into a
  `(paragraph, character)` pair with the existing `offset_position` helper, so the formatted run
  range is a deterministic function of the authoritative body (RFC §3.2.1, §4.4).
- Normalise: a text selection (`selection.text.anchor != caret`) in the addressed body becomes
  `range`; a range in another cell than the explicit `cell` is refused (the command would format
  characters of a body it does not address); a caret in a shape's own body is refused, because the
  single-user command would widen it to a word; a caret in a table cell still addresses the whole
  cell; a range across table cells is still refused, never widened. The address never travels in
  the formatting `args`.
- Validation before the first write: `to <= from`, an offset past the body (checked with
  `position_offset(offset_position(x)) == x`, so a clamped offset is not silently accepted), a
  shape without text, a table without a cell and a non-table shape with a cell address are refused
  before the engine call; no undo step is recorded and the document is unchanged.
- Journal rebase: both offsets shift past the `text.insert`/`text.delete` operations in the same
  body that arrived after `baseVersion` (the existing `shift`, keyed by shape and cell). A deletion
  inside the range shrinks it; a deletion that removes every character of it leaves an empty
  address, and the queued operation is refused `invalid` instead of formatting other characters.
- Undo: the step image is the **whole body** (a shape's `shape_field{field: text}` or a cell's
  `cell_text`), because the format splits runs at the range's ends and only the whole body names
  the previous runs exactly. Undo puts the previous runs back; redo reapplies them; a later edit of
  that body by another participant makes `same_state` fail and the undo is `undo-conflict`.
- Tests: two replicas converge on a bold range of a shape (text kept, run boundary at the range's
  end, the rest unformatted); the same inside one table cell with a neighbour untouched; undo and
  redo restore and reapply exactly the previous runs while the text after the range is untouched;
  an undo after someone typed in the body is `undo-conflict` and writes nothing; empty, reversed,
  past-the-end and missing-cell ranges are `invalid` before any write with no undo step; a remote
  range format leaves the receiver's caret alone; a queued range follows text typed before it (the
  typed characters are not bold) and is refused `invalid` when its characters are deleted; the
  normaliser pins a range in a shape, one across a paragraph break, one in a cell, the refused
  caret and the contradictory explicit cell; both wire forms round-trip. `collab` 183 + 8,
  `collab-proto` 28, `ui-egui collab::` 18, gates 4.
- Gates (on `agents`, rust 1.99.1): `cargo fmt --all -- --check` clean, `cargo clippy --workspace
  --all-targets -- -D warnings` clean. Commit `b4f13167` plus this docs commit on `collab/pr11`;
  `production.md` and this file updated. Nothing pushed, CI untouched. Still refused: the
  animation timing/options/move/clear commands, fonts as a document asset, the rest of the table
  surface, SmartArt/charts, a range across table cells and a bare caret in a shape's body.

### 2026-11-09 — formatting a table cell, with an exact cell-body undo

- `format.*` can address one table cell. `Payload::FormatSet` gained `cell: Option<(u32, u32)>`;
  the address is `(slide key, shape key, row, column)` and the command is applied to that cell's
  `TextBody`, not to the table shape's own (empty) `text`. The engine needed no new command: the
  adapter puts the engine selection on that one cell (`selection.cells = (shape, (r, c), (r, c))`),
  which is what the single-user ribbon does, and the same `format.*` command runs.
- Validation before the first write: a table without a cell address is refused `unsupported`
  (unchanged), a cell address on a shape that is not a table is `invalid`, and an address that
  names no cell of the table is `invalid`. Rejection happens before the engine call, records no
  undo step and leaves the document byte-identical; a deleted slide is `stale` through the same
  `resolve_one` path as every other command.
- Normalise: the address comes from an explicit `cell: [row, col]` (or `row`+`col`), else from the
  one selected cell (`selection.cells`), else from the cell a collapsed caret is editing. A range
  across cells, or a range inside a cell or a shape, is still a run-range format and is refused
  instead of being widened to the whole body. The address never travels in the formatting `args`
  (they are stripped before `FormatCmd::accepts`, as `id`/`ids` already were).
- Journal rebase: a queued format carries no offset, only the cell address, so it is rebased only
  by row/column edits — an insert above/left moves it and a delete of its own line refuses it
  `stale`. Two people formatting the same cell need no offset merging.
- Undo: a format step on a cell records that cell's whole body (`Image::Format(TextBody)` on a
  `Snapshot` with `cell: Some`). The restore is the new merge payload
  `Payload::CellText { cell, body }` (`cell_text`, server-built from the actor's history only,
  `check_restore` refuses a forged one) and runs through `shape.setField { field: cellText }`,
  which the engine now accepts with a `cell: [row, col]` argument: it validates the address
  against the table, then replaces that one cell body. A neighbouring cell is therefore never
  written, and a later edit of the addressed body by another participant makes `step_matches` fail
  and the undo is refused `undo-conflict`.
- Tests: two replicas converge on one cell's bold, the neighbour cell keeps its text and
  formatting, a cell format creates no shape text; undo restores the exact previous body (italic
  kept, bold gone) and the redo reapplies it, while a neighbouring cell's later step survives
  both; an undo after someone typed in the same cell is `undo-conflict` and writes nothing; a
  queued format applies to the cell as it is after another participant typed there (every run is
  bold, including their text); it follows an inserted row and is `stale` when its row is deleted;
  an unaddressable cell, a table without an address, a non-table shape and a deleted slide are
  refused before any write. The normaliser (selected cell, collapsed caret, explicit address,
  cross-cell range refused, missing cell, table without address) and both wire forms are pinned.
- Gates (on `agents`, rust 1.99.1): `cargo fmt --all -- --check` clean, `cargo clippy --workspace
  --all-targets -- -D warnings` clean, `cargo test -p deckcraft-collab -p deckcraft-collab-proto
  -p deckcraft-collab-server` = `collab` 175 + 8, `collab-proto` 27, `collab-server` 10 unit + 4
  commit + 4 gates + 6 socket; `deckcraft-engine` 575 + 1 ignored; `ui-egui` 509 + the known
  Korean-font environment failure; `xtask layers` 32 crates.
- Commit `a48c12de` plus this docs commit on `collab/pr11`; `production.md` and this file
  updated. Nothing pushed, CI untouched. Still refused: run-range formatting (a run-level
  payload), the animation timing/options/move commands, the rest of the table surface, SmartArt
  and charts.

### 2026-11-09 — slide notes text as an addressable body slice

- Notes are collaborative as one slide body. The wire carries `notes_set` (the slide's whole
  speaker-notes body as the model `TextBody` in JSON), `notes_insert` (character offset plus text)
  and `notes_delete` (a character range). The address is `target.slide` alone: notes belong to a
  slide, not to a shape, so a shape key in the target is refused as `invalid` (ADR-0002 §2). The
  engine needed no new command: `slide.notes {index, body}` already writes a whole body, and
  `text.insert`/`text.delete` already act on a `notes: true` selection.
- Normalise: `slide.notes` becomes `notes_set` with the **complete** body — `body` from the notes
  pane as it is, `text` applied to the current body with the same `replace_text` the single-user
  command runs — so the value on the wire is never a lossy string. Typing in the notes pane
  (`text.insert`, `text.delete`, `edit.delete` with `notes: true`) becomes `notes_insert`/
  `notes_delete` with whole-body character offsets; a slide that does not exist is refused
  locally with `BadParams`.
- Adapter: `resolve_slide` before the first write (a tombstoned or unknown slide is `stale`, no
  slide at all is `invalid`), the body is deserialised before the write so a value this build
  cannot read is refused, then the single-user command runs. `call_notes` borrows the engine
  selection for the call and puts the receiving user's selection back afterwards: a remote notes
  edit does not reset the caret, and a caret in the same notes body moves by the characters the
  edit inserted or removed before it (a range that swallows it clamps to the range's start).
- Journal rebase: a notes offset shifts past the notes operations that arrived after its base,
  filtered by the slide address as well as the (absent) shape, so another slide's notes never move
  it. A whole-body `notes_set` after the base makes a queued insert or delete `stale`, and the
  client resyncs instead of writing at a meaningless offset.
- Undo: `Image::Notes(TextBody)` on a `Change::Notes(NotesImage)`; the restore is the same
  `notes_set` payload with the body the step replaced, so runs, levels and bullets come back. The
  image is compared whole, so a later body from another participant refuses the undo with
  `undo-conflict`, and `check_restore` refuses a restore that names another body.
- Tests: two replicas converge on a notes insert after a slide was inserted; undo and redo keep
  the previous body and its bold run; a foreign notes edit refuses the undo; an unknown, deleted,
  unaddressed or shape-addressed slide and an unreadable body are refused before any write; the
  receiving caret follows an insert and a delete while a shape's text selection on that slide
  stays put; a whole-body replacement sends a queued insert to `stale`; the journal keeps two
  typists' text in one slide's notes and never shifts another slide's notes; the normaliser and
  the wire form are pinned.
- Gates (on `agents`): `cargo fmt --all -- --check` clean, `cargo clippy --workspace --all-targets
  -- -D warnings` clean, `cargo test -p deckcraft-collab -p deckcraft-collab-proto
  -p deckcraft-collab-server` = `collab` 166 + 8, `collab-proto` 26, `collab-server` 10 unit + 4
  commit + 4 gates + 6 socket; `xtask layers` 32 crates.
- Commit `b7025f9a` plus this docs commit on `collab/pr11`; RFC (§3.2.1, §6, §11, §13, §14.1),
  `production.md` and this file updated. Nothing pushed, CI untouched. Still refused: run-range
  formatting, formatting a table cell, the timing/options/move animation commands, SmartArt and
  charts.

### 2026-11-09 — shape animation steps as an exact-value slice

- `animation.add` and `animation.remove` are collaborative. The wire value is one shape's
  **whole ordered step list** (`Payload::ShapeAnimations { animations: Vec<Value> }`), addressed by
  slide key and shape key; each value is the model `Animation` as JSON (the `ShapeRestoreItem`
  pattern), because the model has six enums, a colour and two ids that a mirror struct in
  `collab-proto` would duplicate.
- Normalise: one shape per operation (a selection of several is refused). `animation.add` builds
  the new step with the new public `deckcraft_engine::cmd::animation::add_plan` — the same `build`
  the single-user command runs, so the two paths cannot drift — sets the addressed shape id and
  appends it to the shape's existing steps. `animation.remove` with `index` takes the shape from
  the step at that index and removes that step; without an index it removes every step of the
  addressed shape. A shape with nothing to remove sends nothing.
- Adapter: `resolve_one` on the slide and shape keys, so an unknown key, a key of another slide or
  a missing shape is refused (`invalid`/`stale`) before anything is written. Every value is
  deserialised, its shape forced to the addressed one, and the whole list is passed to the engine
  command `slide.animation.set`.
- Engine: `slide.animation.set { index, shape, animations }` bounds the list (256 steps, 64 KiB),
  accepts only the model's own field names (an object with another key is not an animation this
  build wrote), requires a known effect, validates option and path lengths, checks the shape is on
  the slide, and only then replaces the shape's steps where the first one stood (steps of other
  shapes keep their place; a shape without steps gets them at the end). `[]` clears them. The same
  command is the exact-image restore.
- Undo: `Image::Animation(Vec<Animation>)` on a `Snapshot`; the restore is the same
  `shape_animations` payload with the recorded list. `same_state` compares the slide, the shape and
  the whole list, so a later list from another participant refuses the undo with `undo-conflict`,
  while a change to another shape's steps does not.
- Tests: two replicas converge on a step and on two steps in a row; undo keeps the shape's other
  step on both replicas and redo puts the first back; a later value from another participant
  refuses the undo; an unaddressable shape is refused before any write and a forged restore is
  `invalid`; the normaliser pins the whole-list form, the index-addressed removal, the no-op and
  the refused multi-shape case; proto round trip; the engine setter (in-place replacement, clear,
  unknown effect, unknown field, missing index, missing shape, too many steps); the whole template
  through the in-memory hub (two submissions and an undo, both replicas equal).
- The engine soak no longer picks `slide.transition.set`/`slide.animation.set`: the exact-value
  setters are built by a session, and the soak's parameter generator (built from user-facing
  commands) can never produce their whole-value images; before the change seed 1 edited 36 of 400
  steps, after it 60–76, so the rate assertion measures real edits again.
- Commit `f562aea4` plus this docs commit on `collab/pr11`; RFC (§3.2.1, §6, §13, §14.1),
  `production.md` and this file updated. Nothing pushed, CI untouched. Still
  refused: `animation.set` (styles), `animation.timing`, `animation.options`, `animation.move`,
  `animation.clear`, the painter, notes text, run-range formatting and formatting a table cell.

### 2026-11-09 — table cell text and row/column edits

- Text in a table cell now travels with its address. `text.set` already carried `cell`;
  `text.insert`/`text.delete` gained `cell: Option<(row, col)>` (absent for a shape's own body),
  so the offset is inside that cell's `TextBody`, not the table shape's empty text. The local
  queue reads the cell from the caret (`TextSel.cell`) or the cell selection and refuses a table
  command without one; `text.set` falls back to the caret's cell when the command has no `cell`.
- The adapter resolves the cell's own body before mapping the character offset and puts the
  engine's `TextSel` on the cell before running `text.insert`/`text.delete`; a cell that is out of
  range or a table without a cell address is `invalid`. `text.set` passes the cell to the engine,
  which already understood it.
- Rebase is per cell: `rebase_text` compares `(shape, cell)` and only shifts offsets of operations
  in the same cell, so two people in different cells of one table converge. Plain text keys do not
  include the cell (the shape key addresses the table); the address is part of the operation.
- Structural table edits are new because a row insert moves cell addresses:
  `Payload::TableEdit { axis: row|column, at, insert }` runs `table.insertRowAbove/Below`,
  `table.deleteRow` or the column variants on the table's shape key. Local
  `table.insertRowAbove/Below`, `table.deleteRow`, `table.insertColumnLeft/Right`,
  `table.deleteColumn` normalise to it; the address is checked against the table before sending.
  Structural, so it needs `baseVersion == head`.
- Rebase of a queued cell operation (`text.insert`, `text.delete` and a whole-cell `text.set`)
  past a `table.edit`: an insert above/left of the cell moves
  the address down/right, a delete of the very line refuses the operation `stale` (retryable,
  the text cannot be placed), a delete above/left pulls it up/left. Operations in another cell of
  the same table are untouched.
- Undo: `Change::Shape` with `Image::Text` already carried `cell`; it now gets the cell from an
  insert/delete too, and the restore is `text.set{cell}`, so the shape's own body is not created
  or changed. A `table.edit` step records the **whole table** (`Image::Table`, `ShapeField::Table`,
  engine `shape.setField field: table`), because a structural edit changes the cell indices and
  their values; the engine validates the table (1–1000 rows/columns, only a table shape) before
  the write.
- Tests: two replicas editing different cells of one table converge; a cell operation resent
  after a reconnect answers the stored result and inserts once; a queued cell insert rebases past
  an inserted row and lands in the shifted cell; a queued insert is refused after its row is
  deleted; two people typing in one cell keep both insertions and a second cell's offset does not
  move; undo/redo of an insert and a delete restores the exact cell text, and a row edit's undo
  restores the whole table. `collab` 147 + 8, `collab-proto` 24, `engine` 573 + 1 ignored.
- Commit `2a91d49e` plus its docs commit and the cell-set rebase on `collab/pr11` (from `collab/pr10` at `922a4bf4`);
  nothing pushed, CI untouched. Still refused: notes text, formatting a table cell (needs a cell
  address in `FormatSet` and a cell-aware exact body restore), other table commands (style, fill,
  merge, sizing), run-range formatting and animations.

### 2026-11-09 — transitions as a final-value slice

- `transition.set` is collaborative. `Payload::SlideTransition { transition: Option<TransitionSpec> }`
  carries one slide's **whole** transition — effect, option, duration, advance timings, embedded
  sound (`MediaId` as a number), loop/stop flags and the kept XML of an effect the model does not
  name. The whole value travels because `transition.set` keeps the parts the user did not name;
  sending its arguments would let each replica merge them against a different state.
- The client computes that value with the new public
  `deckcraft_engine::cmd::transition::set_plan` — the same function the single-user command now
  runs, so the two paths cannot drift (the `align_plan`/`distribute_plan` pattern). A
  `transition.set` on several selected slides is refused in a shared session (the wire addresses
  one slide per operation); an unknown effect or a bad `index` is refused before anything is sent.
- Engine command `slide.transition.set` (`{index, transition: model JSON | null}`) deserialises and
  bounds the value (8 KiB), accepts an effect the model cannot name only together with its kept
  XML, refuses a sound id that is not in `doc.media`, and writes one `edit`. It is both the
  confirmed command and the exact-image restore of the step.
- Undo: `Change::SlideTransition(TransitionImage { slide, transition })`; `same_state` compares the
  slide key and the whole value, and the restore is the same `slide_transition` payload with the
  recorded value, so advance timings, the sound and kept XML come back. A second participant's
  later value refuses the first undo with `undo-conflict`; a forged restore (another value) is
  `invalid`.
- Rebase: nothing to rebase — the value is absolute, so a queued operation keeps the `baseVersion`
  it was sent with and the later accepted value wins (a command does not need `baseVersion == head`).
- Tests: proto round trip and command name; engine exact setter (kept XML, unknown effect without
  XML, `null`, missing value, bad sound, bad index) and `transition.set` = `set_plan`; adapter
  two-replica convergence, a second participant plus the refused first undo and his own undo,
  undo/redo of a step that replaced a transition with an advance timing, a forged restore, an
  unknown effect on the wire; client normalisation (whole final value, the `none` collapse,
  unknown effect, bad index); history image and restore. `collab` 138 + 8, `collab-proto` 23,
  `engine` 572 + 1 ignored.
- Commit `927e773c` plus this docs commit on `collab/pr10`; nothing pushed, CI untouched.

### 2026-11-09 — the browser link: session, presence, commit over the host bridge

`apps/deckcraft-web` showed the collaboration UI but could not join a room: the desktop link is a
thread with `tungstenite`, and a page has neither. `crates/collab-web` is the browser's side of the
cabinet contract (`INTEGRATION.md` §6, ADR-0016 §3) and a `Transport` over the page's `WebSocket`
that drives the same `CollabDoc` as the desktop.

- `craft-web-host` is pinned to `copoka/craft-libs` `7666e2b1` — the revision that carries
  `session`, `session-state`, `presence`, `commit` and `committed`; `storytold/craft-libs` `main`
  does not have them. `Cargo.lock` records the same source and revision.
- `HostSession`/`HostEvent`/`HostState` hold the message contract without `web_sys`, so the
  handling and the whole page-host scenario are tested on the host. The token is a credential:
  `Debug` prints `<redacted>`, it travels in the first frame and in the media `Authorization`
  header and nowhere else.
- `Transport` no longer requires `Send`. The browser link owns JS closures and `Rc`, which never
  cross a thread on wasm32; the desktop `WsTransport` spawns its own thread and stays `Send`.
- Media: a browser cannot fetch synchronously while it applies a command, so `WebLink` scans an
  `applied` frame (and a snapshot tail) for the digests it names, fetches the missing bytes with
  `fetch`, and holds the frame in arrival order until they are here — a later frame never
  overtakes a picture that is still loading. A frame whose bytes never arrive is released after
  10 s and the adapter reports the picture it cannot place, never a silent divergence. Uploads
  (`put_media`) and fetches use the session token from the bridge; no new host message is invented.
- `apps/deckcraft-web`: the shell applies `session`/`committed` on the next frame, posts
  `session-state` on a status or participant change and `presence` at the room's pace, and Save in
  a live session asks the cabinet to commit instead of downloading a copy (the cabinet owns the
  file). `?collab=1` alone still leaves the app single-user and offline.
- The headless gate (`cargo test -p deckcraft-web`) is the page-host scenario: one `session`
  message, a handshake against a real in-process `Hub` (no socket, no TLS, no threads), two
  `shape.insert` edits reaching version 2 on both the app and the room, presence published, Save
  posting `commit`, and `committed` recording version 2 with its etag. Without the message the app
  makes no network call at all.
- Verified on `agents`: `fmt --check` clean; `collab` 129 + 8, `collab-proto` 22, `collab-server`
  10 + 4 + 4 + 6, `collab-web` 15, `deckcraft-web` 3, `ui-egui collab::` 17; clippy over the
  touched crates with `-D warnings` clean; `xtask layers` 32 crates; `xtask wasm` 26 crates;
  `cargo check -p deckcraft-web --target wasm32-unknown-unknown` ok; `trunk build` (0.22.1) ok in
  4m7s cold, `dist/web` with a 49 MB `_bg.wasm`. The full `ui-egui` suite is 509 passed + 1 failed:
  `ime_tests::korean_composes_in_the_notes_pane` needs a Korean font this build host does not have
  (`deckcraft-fonts` builds without `craft-fonts` there) — unrelated to this change.
- Commit `66d4658c` on `collab/pr10`; nothing pushed, CI untouched.

### 2026-11-09 — three-client soak: compaction, base transfer and reconnect

- A client whose version fell below the journal tail could not rebuild: `resync` returned a
  cursor without state (status item 9, RFC matrix row 12). `SnapshotMsg` now carries
  `base { version, bytesBase64?, keys }`. `SnapshotMode::Base` means the replica is replaced; the
  tail above the base is replayed on top of it. `DocSession::resync` answers from the journal
  while it covers the client, and otherwise takes a snapshot on demand and sends base + tail, so
  compaction never strands a client. Above `max_base_inline` (8 MiB, `COLLAB_MAX_BASE_INLINE`) the
  bytes are omitted and the client reopens the file from the cabinet.
- Keys travel with the base (`SnapshotKeys { slides, shapes, dead, next }`). Keys are issued in
  walk order and cannot be re-derived after structural edits (ADR-0002), so `Adapter` exports the
  map and the issuer counter and a replica that rebuilt from a base restores both; the next
  insertion gets the key the server would issue.
- Engine: `Session::replace_active_document` swaps the model in place (uid, title and path kept,
  revision bumped so the UI and render caches refresh; selection and undo history reset).
  `CollabDoc` uses it for a base and refuses more than 64 MiB of base bytes.
- Server: `Config.snapshot_every` (and the new `max_base_inline`) now reach a memory-only room,
  not only the store; `base_transfers_total` / `base_bytes_total` count transfers. The counters
  are in `/metrics`.
- The fourth §8 gate (`three_clients_soak_across_compaction_and_a_base_transfer`): two editors,
  26 move/bounds/text/fill edits with `snapshot_every = 5` (0.67 s on `agents`), a viewer joining
  below the base (5 KiB at version 25, one tail step, rebuilt in 21.4 ms), an editor reconnecting
  from a two-step tail, all three at version 28 with an equal `Presentation`, and a commit at 28
  that reimports with the final boxes (420 / 350). No base is sent when the tail still covers the
  client, which the reconnect asserts.
- Tests added: proto round trip of a base and decoding of an old snapshot without one; key map
  export/import keeps keys, tombstones and the next number; `resync` from inside the tail, after
  compaction (base reloads into the snapshot document) and above the inline limit; `ClientSession`
  adopts a base head and drops work the base already covers; `CollabDoc` rebuilds a replica from
  a base and finishes it with the tail; the engine replaces the active document in place.
- Verified on `agents`: `fmt --check` clean, `collab-proto` 22, `collab` 129 + 8, `collab-server`
  10 + 4 + 4 + 6, `ui-egui collab::` 17, `deckcraft-engine` 570 + 1 ignored; clippy over the four
  collab crates, `engine` and `ui-egui` with `-D warnings` clean; `xtask layers` 31 crates.
  Commit `e007e965` plus this docs commit on `collab/pr10`; nothing pushed, CI untouched.

### 2026-11-09 — three-session gate: conflicting edit, viewer, reconnect

Added the third §8 gate to `apps/deckcraft-collab-server/tests/gates.rs`: two real editor sockets
and one viewer socket on a two-slide deck. Both editors move the same rectangle; the second
absolute value wins deterministically and the first editor's undo is refused `undo-conflict`
without touching the document. The editors also work on different slides, the viewer's op is
refused `forbidden`, and one editor drops and reconnects: the resync returns the journal tail
after the version his replica has, and he lands on version 4 and on an equal `Presentation`.
Three local replicas (opened from the same bytes) apply every confirmed command, so convergence
is compared on presentation state; a commit at version 4 reimports with the expected values.

The gate exposed a real reconnect bug in `ClientSession`: a `welcome` ahead of the replica was
adopted as the head before the resync, so the client requested the tail after the head. The
welcome no longer moves the version across a gap, and the new unit test
`a_reconnect_asks_for_the_tail_from_the_version_the_replica_has` pins the behavior; two older
client tests were corrected to the same semantics. On `agents`: `fmt --check` clean,
`collab` 124 + 8, `collab-proto` 20, `collab-server` 10 + 4 + 3 + 6, clippy `-D warnings` clean,
`xtask layers` 31 crates. Commit `556c00d9`, not pushed.

### 2026-11-09 — TASK.md §8 gates: 120 slides with media, LibreOffice on the commit

- `apps/deckcraft-collab-server/tests/gates.rs`: two environment gates. A 120-slide deck with
  120 pictures (4 media objects) is opened as a room; two editor sessions move a picture on every
  slide (0.1 ms p50, 0.1 ms p95, 0.2 ms max on the build host), both settle at version 120, the
  commit builds 154 KiB in 15 ms, and the bytes reimport with 120 slides and 120 pictures. The
  same gate takes the commit through `POST /v1/sessions/{id}/commit`, opens it in LibreOffice 26.2
  (121 pages via `pdfinfo`, 4.2 s) and round-trips it through LibreOffice back into
  `deckcraft_pptx::import` (5.3 s, 121 slides, 120 pictures). The build host has no LibreOffice,
  so there the test skips itself with a printed line; the gate ran with the remote-built binary on
  the workstation, where LibreOffice 26.2.6.3 is installed.
- The large-deck gate exposed a real bug: `shape.move`/`shape.setBounds`/`text.set` were applied
  against the replica's own selection, so an edit on a slide that replica was not looking at was
  refused with "no such shape". The adapter now selects the target shape and its slide; a focused
  regression test in `crates/collab/src/apply.rs` covers a move and a `text.set` on slide 1 while
  the other replica looks at slide 0.
- Verification on the build host: `cargo test -j 4 -p deckcraft-collab -p
  deckcraft-collab-server` (123 + 8, 10, 4, 6, 2 gates), `cargo clippy -j 4 -p deckcraft-collab -p
  deckcraft-collab-server --all-targets -- -D warnings` clean, `cargo fmt --all -- --check` clean.
### 2026-11-09 — align and distribute as one final-bounds step

- `arrange.align` and `arrange.distribute` now travel as **one** operation
  (`shapes_set_bounds`, object `items:[{shape, x, y, w, h}]`) instead of one per shape. The step
  pattern of the other slices: payload → normalize → adapter run → confirmed command → two-client
  test (RFC §3.2.1).
- Normalize computes the final boxes through new public engine helpers `align_plan`/
  `distribute_plan` — the same arithmetic the single-user commands use (`arrange.rs` now applies
  their result, so the two paths cannot drift). Shapes whose box does not change are left out, and
  a no-op commands send nothing. Locked shapes are left alone.
- The adapter runs `shape.setBoundsMany`: every key resolved and duplicates refused before the
  first write, values clamped, the whole selection moved in one `edit`. Undo records one geometry
  image per moved shape and restores the step in one `shapes_set_bounds`; `same_state` compares
  every box, so a foreign move of a member refuses the undo with `undo-conflict`. Server
  `capabilities.commands` lists the new engine command.
- Tests: two replicas converge on one align step with one history entry; one undo moves both
  objects back and redo re-applies them; a foreign move refuses the undo; the server path
  (`DocSession` + two `ClientSession`s) settles both clients after one version and one undo;
  normalize tests pin the final numbers (centre-align 210/250, distribute 0/840) and the no-op
  path; engine tests prove one undo step, an atomic refusal of a missing or repeated id, and a
  locked target left in place.
- Verified: `fmt` clean, `collab-proto` 20, `collab` 122 + 8, `collab-server` 10 + 6 + 4,
  `ui-egui collab::` 17, `deckcraft-engine` 569 + 1 ignored; clippy over the four collab crates and
  `ui-egui` with `-D warnings` clean; `xtask layers` 31 crates.
- Commit `31d353fe` (code) plus this docs commit on `collab/pr10`; RFC (§3.2.1, §6, §13.4),
  production and this file updated. Nothing pushed; CI untouched.

### 2026-11-09 — group/ungroup undo and fork/PR reconciliation

- Reconciled the fork with the local branch: `git fetch fork` leaves `fork/collab/pr10` at
  `63ec1ebe`, the PR head; local `collab/pr10` was ahead by `3ec81d70`, `ded9d1ee`, `31842050`,
  `f28ee048`. There is no fork commit missing locally. The three-way diff (fork head, local head,
  merge base) is the four local commits above; `git push --dry-run` is refused by this workspace's
  policy, so the PR head update stays manual. This file, `pr10.md` and `production.md` now say the
  PR is open from the fork instead of "nothing pushed".
- `arrange.group` and `arrange.ungroup` are undoable (RFC §6, §13 item 4). A step records a
  `GroupImage` on both sides: the group present (model JSON carrying its children, direct-child
  keys, z-order index and animations) or absent with a whole image of every former member at the
  top level. Undo and redo are one structural `shape_restore{items, remove}`: the engine removes
  the other side's shapes first without dropping animations, validates and deserialises every
  image before the first write, and inserts what the step recorded. `same_state` compares the
  present group or every absent member's content and owner, not positions.
- Tests: two-replica undo/redo of a group (members return in their order, then the group image
  comes back with its id and children); two-replica undo/redo of an ungroup the other way round;
  a foreign move of a member refuses the undo with `undo-conflict`; forged restores remain
  refused. Engine tests move group ↔ members in one `shape.restore` and prove a missing `remove`
  id refuses before any write. History and proto tests cover both payload directions.
- Verified: `fmt` clean, `collab-proto` 19, `collab` 118 + 7, `collab-server` 10 + 6 + 4,
  `ui-egui collab::` 17, `deckcraft-engine` 567 + 1 ignored; clippy over the four collab crates
  and `ui-egui` with `-D warnings` clean; `xtask layers` 31 crates.
- Commit `98d9e9ce` (code) plus this docs commit on `collab/pr10`; RFC (§6, §13.4, §14.1),
  production and this file updated. Nothing pushed; CI untouched.

### 2026-11-08 — a commit becomes a cabinet file version

- P0 from [`production.md`](production.md) closed on the server side. `POST /v1/sessions/{id}/commit`
  (service token, optional `{expectedVersion}`) builds the `.pptx` from the authoritative room and
  posts it to the cabinet (`COLLAB_CABINET_URL`, `COLLAB_CABINET_TOKEN`): body
  `{fileId, name?, bytes(base64), sha256, version, expectedVersion?}`, answer `{version, etag}`,
  parsed through a small HTTP/TLS client (`rustls`, system roots; `Connection: close`; chunked
  answers decoded). The file version is created by the cabinet, which keeps the previous one; the
  server never writes over the original.
- `409` is preserved end to end: a room whose `expectedVersion` does not match is refused before
  the callback; a cabinet `409` (file changed through WebDAV or another client) is the caller's
  `409` and no version is created. The accepted `etag` becomes the session's `expectedVersion` and
  is written to `meta.json`, so a restart still compares against the cabinet's copy.
- A client `commit` over WebSocket is answered only after the cabinet took the file; a viewer's
  `commit` is refused. The `initialName` a session is opened with is passed through to the
  callback. Without a callback the endpoint returns the base64 bytes (development mode, as
  `INTEGRATION.md` §4.1 describes).
- Counters: `cabinet_commits_total` (accepted publications), `cabinet_conflicts_total`,
  `cabinet_failures_total`; no file content, names or tokens are logged.
- Tests (`apps/deckcraft-collab-server/tests/commit.rs`): a fake cabinet keeps versions like the
  real one. Two rooms opened from two copies of the same `.pptx`; each commit reimports with its
  own edit, each original stays byte for byte, the name reaches the callback. An external change
  under a live room makes the server post and the cabinet answer `409`, with the conflict counter
  moving and nothing overwritten; a wrong `expectedVersion` is refused without a callback. The
  WebSocket path answers `commit-result` with the cabinet's `etag-1`, a stale second commit gets
  `conflict`, a viewer gets `forbidden`. A no-callback commit returns a `.pptx` inline and
  `fileVersion` stays the room's own.
- `collab` 113 + 7, `collab-proto` 19, `collab-server` 10 + 6 + 4, `ui-egui collab::` 17,
  `engine` 565 + 1 ignored; clippy `-D warnings` and `fmt` clean; `xtask layers` 31 crates.
- Commit `31842050` plus the docs commit on `collab/pr10` (RFC §8/§14.1, production, this file).
  Nothing pushed; CI untouched.

### 2026-11-08 — structural undo of shape insert and delete

- A new step image: `ShapeImage` (key, the group that held the shape, its position among that
  group's children, the `Shape` model as JSON or `None`, and the animations that named the shape
  or a descendant, including foreign effects it triggered). A step records only the **top-most**
  shapes it changed: a group's image carries its children, so undoing a group delete is one image.
- `shape.insert`, `insert.picture`, `insert.video`/`insert.audio` and `edit.delete` are undoable.
  Undo of a delete is the structural `shape_restore{items:[{shape, parent?, index, value,
  animations?}]}` and its redo is `shape_delete`; undo of an insert is `shape_delete` and its redo
  `shape_restore`.
- The engine command `shape.restore` bounds the images (64 items, 192 KiB together), deserialises
  the model before the first insert, refuses an id already on the slide, and inserts in rounds so
  a parent comes back before its children. `index` is clamped to the list that exists, so
  neighbours that came or went do not block the undo. The restored subtree's animations come back,
  and the triggers a delete cleared are re-attached to the same effects.
- `same_state` compares a shape image's content, owner and animations but not its position, so a
  foreign edit of a *neighbour* never turns an undo into `undo-conflict`; a foreign edit of the
  shape itself still does.
- The key map keeps both directions after a delete (the key is marked dead, not forgotten) and
  `revive_shape` clears the tombstone on a restore: the shape comes back under the key it had, so
  the step that restored it stays addressable and a replayed journal keeps the same keys.
- Tests: two-replica undo of an insert after a foreign neighbour edit; undo refused after a
  foreign edit of the inserted shape; two-replica undo of a delete with a moved neighbour (order,
  animation and the neighbour's position checked); undo of a group delete as one image; a forged
  `shape_restore` refused with `invalid`; a journal replay after a restart keeps the shape and its
  key; engine tests for z-order, animations, a group child, a duplicate id and a missing parent.
- Commit `3ec81d70` (code) plus the docs commit on `collab/pr10`; RFC (§3.2.1, §6, §11, §13.4,
  §14.1) and this file updated. Nothing pushed; CI untouched.

### 2026-11-08 — undo of a group's fill/line and structural slide undo

- A step now carries a list of images on both sides (`Change::{Shape, Slide}`), so one operation
  can touch several objects. `shape.fill`/`shape.line` on a group record one image per changed
  member and restore them in one `shape_fields` payload; the engine command `shape.setFields`
  validates and deserialises every value before the first write (4096 items, 192 KiB combined).
- `slide.delete`, `slide.new` and `slide.duplicate` record the whole slide: model JSON, index,
  section and custom-show membership. Undo restores it with the structural `slide.restore`
  command (`slide_restore` payload), redo deletes it with `slide_delete`; slide and shape ids are
  kept, the image is capped at 192 KiB, and a restored slide is removed from the tombstone set.
- `check_restore` now guards **every** history move (not only `shape_field`): a client cannot send
  an undo envelope that addresses another object, names another slide or carries another value.
- Tests: two-replica undo/redo of a group fill and of a slide delete/insert, a foreign member
  repaint refused with `undo-conflict`, a forged `slide_restore` refused with `invalid`; engine
  tests for `slide.restore` (index, section, duplicate id) and `shape.setFields` (atomic refusal).
- Journal replay of a history operation: `DocSession::replay` runs the stored confirmed command
  without the history check, so an exact `shape_field`, a group's `shape_fields` or a
  `slide_restore` survives a restart; the store uses it and a test deletes a slide, journals the
  undo and reloads the room.
- `collab` 106 + 7, `collab-proto` 19, `collab-server` 4 + 6, `ui-egui collab::` 17,
  `engine` 562 + 1 ignored; clippy `-D warnings`, `fmt` and `xtask layers` clean.
- Commits `4e666b95` (undo) and `f6281736` (replay) on `collab/pr10`; RFC (§3.2.1, §6, §11,
  §13, §14.1) and this file updated. Nothing pushed.

### 2026-11-08 — undo of fill, line and format steps

- `Snapshot` gained `Image::Fill`, `Image::Line` and `Image::Format(TextBody)`; `shape.fill`,
  `shape.line` and the 17 `format.*` commands now record a before/after image of the value they
  replaced.
- The user-facing commands cannot reproduce the model they replaced (gradient tile and scaling,
  custom dash, per-run formatting), so undo/redo carry the exact model value in a new
  `shape_field{field, value}` payload. The adapter accepts it only as an undo/redo of the actor's
  own latest step and only with the value that step recorded; a forged or different value is
  `invalid`, a foreign edit of the same object is `undo-conflict`.
- New engine command `shape.setField`: bounded JSON, deserialised into `Fill`/`Line`/`TextBody`
  before it touches a shape. A group's fill/outline step is not recorded (members would keep the
  new value).
- Tests: `collab` 99 + 7, `collab-proto` 18, `collab-server` 3 + 6, `ui-egui collab::` 17,
  `engine` 560 + 1 ignored; clippy `-D warnings`, `fmt` and `xtask layers` clean.
- Commit `41fe1ad9` on `collab/pr10`; RFC (§3.2.1, §6, §13.4) and this file updated. Nothing
  pushed.

### 2026-11-08 — media kinds, TLS and four command slices

- Added audio and video to the media channel: signature allowlists (MP4/MOV/WebM/AVI,
  WAV/FLAC/Ogg/MP3/M4A/AIFF/MIDI), 32 MiB per clip, separate 32 objects / 192 MiB budgets so a
  clip cannot evict pictures; `Payload::MediaInsert { kind, sha256, name, rect }`, adapter run
  through `insert.video`/`insert.audio`, `GET /media` content type from the signature.
- Added the TLS HTTP client for media: the desktop link derives `http`/`https` from
  `ws://`/`wss://`, `rustls` + `rustls-native-certs` (system roots, `SSL_CERT_FILE`), default
  ports, `Host` without the default port, IPv6 brackets.
- Added four command slices with the same pattern (payload → normalize → adapter → confirmed →
  two-replica test): `shape.fill` (solid/none/background), `shape.line` (final outline values),
  17 `format.*` commands over one explicit shape's text (`FormatCmd` allowlist, no address in
  `args`, explicit `on`), `slide.duplicate` addressed by slide key.
- Tests: `collab-proto` 18, `collab` 91 + 7, `collab-server` 3 + 6, `ui-egui collab::` 17;
  clippy `-D warnings` and `fmt` clean; `xtask layers` 31 crates. Engine not re-run.
- Commits `0a57b61e`…`69c2ff22` on `collab/pr10`; docs (rfc §3.2.1/§4.1/§13/§14.1, production,
  pr10) updated. Nothing pushed.

### 2026-11-08 — PR prep and formal RFC proposal

- Audited the chair's finding: the tree is clean; the change it saw uncommitted is `7fe75711`
  (media channel) plus `a4aac7c7`/`e7ad45cf` (docs), 1129 insertions in the slice commit.
- Re-ran fmt, tests of `collab-proto`, `collab`, `collab-server`, `ui-egui collab::` and clippy;
  results are in the verification table above.
- Added [`pr10.md`](pr10.md): title, description, review order, `PR-CHECKLIST.md` mapping and the
  limitations. No push.
- Posted a formal `proposal` with the RFC/PR/STATUS artifacts to `rfc/deckcraft`:
  `20261108T105421Z-deckcraft-47b7` (refs `…-7302`, `…-7c9b`, `…-7c4a`).
- Re-prioritised the gaps in [`production.md`](production.md) (P0 cabinet callback, P1 command
  surface/media/text, P2 transport/preview/undo/base-transfer/perf).
