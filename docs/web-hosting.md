# Putting DeckCraft on the web (free)

DeckCraft's web version is a folder of static files (no server code), so any free static host
works. Nothing about a deck is ever sent to the host: decks open and save as files on the
teacher's own device, and the only thing the browser stores is the teacher's preferences.

## What you get

- **One link for everyone**, on any device with a modern browser (Chrome, Edge, Safari, Firefox).
  No install, no admin rights, no Apple or Windows signing.
- **Installable and offline.** Chrome and Edge offer *Install DeckCraft* (a desktop-style window);
  on iPad, Share ▸ Add to Home Screen. After one visit it works with no internet.
- **Updates by themselves.** Publish a new version and the next time anyone opens the app with
  internet they get it (the previous version keeps working until then). Nothing to press.

## The catches (be honest with staff)

- **Saving is a download.** The browser can't write over the file you opened. *Save* downloads a
  fresh `.pptx` to Downloads, which you then move or replace. (Chrome and Edge could later be
  given true "save in place"; Safari and iPad can't.) On a Mac or Windows PC with Google Drive
  for Desktop, saving to the Drive folder works well.
- **No Google Drive sign-in in the web version yet.** The Drive feature in the Mac app uses the
  system keychain; the web version needs its own browser sign-in (later).
- **First load is big:** about 36 MB, about 14 MB once the host compresses it. After that it is cached.
- **Safari / iPad:** works in principle (WebGPU or WebGL2), but I have only tested Chromium so far.

## Where to host it, free

| Host | Free? | Who can open it | Notes |
|---|---|---|---|
| **GitHub Pages** (recommended to start) | Yes | Anyone with the link (the repository is public) | Workflow included: `.github/workflows/web.yml`. Address: `https://k-betts.github.io/deckcraft/`. Compresses the wasm for you. |
| **Cloudflare Pages** | Yes | Anyone, **or only people you allow** with *Cloudflare Access* (free for up to 50 users, e.g. only school Google accounts) | Best if the school wants it not to be public. Connect the repo, build command below, output `dist/web`. |
| **Netlify / Firebase Hosting** | Yes (generous limits) | Anyone | Same idea. `packaging/web/_headers` is already in the Netlify/Cloudflare format. |
| **Your own Raspberry Pi** | Yes | Staff with the password (see below) | The private copy with what the Mac app has built in (curriculum, questions, templates, exam library); nothing to install but Tailscale. |
| **The school's own web server** | Their cost | Whoever IT chooses | Unzip `deckcraft-web-<version>.zip` (from `packaging/web/package.sh`) and see `packaging/web/README.md` for the two settings it needs. |

The app contains no pupil data and no secrets, so a public address is not a data risk, but it
does mean anyone who finds the link can use the app. If that matters, use Cloudflare Access.

## Publishing to GitHub Pages (one time, then one click)

1. GitHub ▸ the repository ▸ **Settings ▸ Pages ▸ Build and deployment ▸ Source: GitHub Actions**.
   (Until you do this, nothing can be published. I did not change your repository settings.)
2. **Actions ▸ Publish web app ▸ Run workflow.** About 10 minutes the first time. When it finishes
   the address appears in the run summary.
3. To publish an update later: merge to `main`, then run the workflow again.

## Hosting on a Raspberry Pi (free, private, a copy of the Mac app)

A Pi suits a **private** copy for your department: one with the school's curriculum built in,
so Get Ready works straight away, behind a password that only staff know. Any Pi from a Pi 3 up
will do: it only hands out files, and the app runs in each teacher's browser.

- **Keep this copy private.** A curriculum file is the school's own and may use a provider's
  wording, so a build with it in must never go on the public GitHub Pages site (which is built
  without one). The password below and Tailscale's https address keep it to staff.
- **https matters.** Without it the browser turns off offline use, installing the app, the
  clipboard and WebGPU (the app still runs, more slowly). Tailscale Funnel gives the Pi a free,
  fixed `https://….ts.net` address that works from school and home with nothing to install.
  (Cloudflare Tunnel also works but needs a domain name, which costs money.)

### 1. Set up the Pi (once)

The Pi needs Python 3 (Raspberry Pi OS has it), SSH from the Mac, and Tailscale. Nothing else
is installed: `packaging/web/serve-private.py` is the web server.

1. Publish once from the Mac (step 2 below). It creates `~/deckcraft-web/` on the Pi, with the
   site and the server in it.
2. On the Pi, choose the staff user name and password (typed twice; only a salted hash is kept,
   in `~/deckcraft-web/password.json`):
   ```bash
   python3 ~/deckcraft-web/serve-private.py --set-password
   ```
3. Run it as a service, so it starts with the Pi. Save this as
   `~/deckcraft-web/deckcraft-web.service` (change `kieran` to your user name):
   ```
   [Unit]
   Description=DeckCraft private web app (password protected)
   After=network-online.target

   [Service]
   User=kieran
   WorkingDirectory=/home/kieran/deckcraft-web
   ExecStart=/usr/bin/python3 /home/kieran/deckcraft-web/serve-private.py /home/kieran/deckcraft-web/site
   Restart=on-failure

   [Install]
   WantedBy=multi-user.target
   ```
   then:
   ```bash
   sudo cp ~/deckcraft-web/deckcraft-web.service /etc/systemd/system/
   sudo systemctl enable --now deckcraft-web
   ```
   It listens on `127.0.0.1:8790` only, so nothing reaches it except through Tailscale.
4. Give it an https address with Tailscale Funnel. Funnel can use ports 443, 8443 and 10000;
   pick one nothing else on the Pi uses (`tailscale serve status` lists them):
   ```bash
   sudo tailscale funnel --bg --https=10000 http://127.0.0.1:8790
   ```
   The address is `https://<pi-name>.<your-tailnet>.ts.net:10000/`. Give staff that link and the
   password. `sudo tailscale funnel --https=10000 off` takes it off the internet again. (First
   time on a tailnet: turn on HTTPS certificates and Funnel in the Tailscale admin console.)

### 2. Publish a version (each time): a copy of the Mac app

**The Mac app is the master copy.** Set things up there (the curriculum in Get Ready, your own
questions, the maths tools in Teacher Mode, your lesson templates, the Question Library's folder),
then from the repository on the Mac:

```bash
CRAFT_FONTS_DIR=~/Projects/personal/craft-fonts packaging/web/deploy-private.sh kieran@100.74.218.18
```

It reads what the Mac app has now, from `~/Library/Application Support/DeckCraft/`, and builds it
into the web app (it says `building in the school material from …`):

| From the Mac app | In the web app |
| --- | --- |
| The curriculum (`ui.json`) | Get Ready plans from it straight away |
| Your own questions and question recipes, questions you took out of the bank (`ui.json`) | Get Ready and Manage Questions |
| Subject packs and Get Ready's mode, year and route (`ui.json`) | The Maths buttons on the ribbon |
| The Teacher Mode maths tools you chose (`maths-tools.json`) | The same tools in Teacher Mode |
| Lesson templates (`Templates/*.deckcraft`) | Home ▸ Templates and New, with their own layouts (a "Get Ready Task" layout is used by Get Ready as on the Mac) |
| The Question Library's folder (`ui.json` names it) | Insert ▸ Question Library: the questions (one file, about 1 MB compressed) and their pictures go in `exam/` beside the site, behind the same password; each picture downloads when it is first shown |

Nothing else is read: not the recent files list, not your name (Preferences ▸ author stays each
teacher's own), not `prefs.json` (editing preferences such as snapping and AutoCorrect are each
teacher's own). The script prints only counts for the exam library, never its contents.

It then makes compressed copies (the app downloads as about 13 MB) and copies everything to
`~/deckcraft-web/site` on the Pi. The server picks it up at once; teachers get the new version the
next time they open the app. **What a teacher changed in the browser is kept:** published things
are taken when the browser has none or when a newer version is published, and then a published
question replaces the copy with the same id while questions a teacher added in the browser stay.
A newer version does replace the curriculum and the Teacher Mode tool list.

Options:

- `deploy-private.sh --local` builds into `dist/web-private` without publishing, to try first with
  `python3 -m http.server 8765 --directory dist/web-private`.
- A second argument names another settings folder (a copy, or another teacher's).
- `DECKCRAFT_EXAM_LIBRARY=/path/to/library` uses another exam library; `DECKCRAFT_EXAM_LIBRARY=none`
  leaves it out.
- `CRAFT_FONTS_DIR` is needed when you run it from a worktree (the fonts are found by themselves in
  the main checkout). Without the fonts the app shows only a grey screen.

**Never publish this build anywhere public.** The curriculum is the school's own and the past papers
belong to the exam boards. The public GitHub Pages build is made without any of it.

The Pi needs to stay on for the address to work.

## Building it yourself

```bash
cd apps/deckcraft-web && trunk build --release     # output: dist/web
python3 -m http.server 8765 --directory dist/web   # then open http://localhost:8765/
```

The offline and update logic is `apps/deckcraft-web/sw.js`; check it with
`node apps/deckcraft-web/tests/sw.test.mjs`. `node apps/deckcraft-web/tests/smoke.mjs dist/web`
opens the built app in headless Chrome with `?smoke` (Home, Get Ready, Exam Question, the Question
Library, a template, Teacher Mode and its tools, at 1920×1080 and 3840×2160) and fails on any
console error; CI runs it on every pull request.

## What this does not do yet

- The iPad-draws-and-the-board-shows idea needs a live link between two devices (a small free relay
  or a direct browser-to-browser connection). That is the next web phase (W3 in `MATHS.md`).
- Save in place and Google Drive in the browser (see above).

## The staff question-bank page (`/bank/`)

`deploy-private.sh` publishes `/bank/` on the private site. Anyone with the staff password can
browse the built-in bank (3,753 templates, one worked example each) and *suggest* a change. People
with an **editor login** edit the live bank directly: add questions (paste the Gemini Gem's answer,
see `bank-gem-prompt.md`), replace a built-in question with their own version, take one out, edit or
delete their own, approve teachers' suggestions. **Admins** also make and remove logins on the
Logins tab. The private web app loads the live bank (`/bank/api/live.json`) every time it opens, so
an edit reaches teachers on their next page load; the Mac app does not read it yet.

- First admin, on the Pi: `python3 ~/deckcraft-web/serve-private.py --add-user` (choose role admin).
  More logins: that command again, or the Logins tab. No restart needed. A login from the first
  version (`bank-admin.json`) still works as user `admin`.
- Data: `~/deckcraft-web/bank-data/bank.json` (live bank, suggestions, history of changes) and
  `users.json` (salted hashes). Back both up.
- Fixed questions only: random-number templates are still written by the admin (or the AI prompt in
  `question-writing-guide.md`) and checked with `deckcraft-cli bank check`.
- Editors can also move questions to another skill (button or drag) and, on the **Tag to curriculum** tab, say which questions practise each curriculum objective. The deploy publishes the Mac app's curriculum as `curriculum.json` for that tab.
- Tests: `python3 -m unittest packaging/web/test_bank_portal.py`.
