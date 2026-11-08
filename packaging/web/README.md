# Hosting DeckCraft for the web

`deckcraft-web-<version>.zip` (from the GitHub release, or `packaging/web/package.sh`) holds a
static site in `deckcraft-web-<version>/`:

| File | What it is |
|---|---|
| `index.html` | The page. It loads everything through relative URLs. |
| `deckcraft-web-<hash>.js` | wasm-bindgen glue (generated, ES module) |
| `deckcraft-web-<hash>_bg.wasm` | The app, size is printed by `packaging/web/package.sh` |
| `_headers`, `.htaccess` | Sample header rules for Netlify/Cloudflare Pages and Apache |

There is no server-side code. Upload the folder's contents anywhere that serves static files.

## Any path works

All URLs in `index.html` are relative (`public_url = "./"` in `apps/deckcraft-web/Trunk.toml`),
so the site works at a domain root (`https://example.com/`), under a prefix
(`https://example.com/tools/deckcraft/`) and from a CDN bucket. The asset names carry a content
hash, so they can be cached forever. Only `index.html` needs revalidation.

## Required server settings

- **MIME type:** serve `.wasm` as `application/wasm`. Browsers refuse to stream-compile it under
  any other type, and the app then loads slowly or not at all. Serve `.js` as `text/javascript`.
  Most hosts already do both. For nginx, check that `mime.types` has `application/wasm wasm;`.
- **Compression:** turn on gzip or Brotli for `.wasm`, `.js` and `.html`. That shrinks the
  download by roughly 60%. You can also precompress (`brotli -k *.wasm`) and let
  the server send `Content-Encoding: br`.
- **Caching:** `Cache-Control: public, max-age=31536000, immutable` on the hashed `.wasm` and
  `.js` files, and `no-cache` on `index.html`.
- **HTTPS:** WebGPU (and the clipboard) only work in a secure context, which means `https://`
  or `http://localhost`. Over plain HTTP elsewhere, the app falls back to WebGL2.
- **No special isolation headers:** DeckCraft doesn't use `SharedArrayBuffer`, so it doesn't
  need `Cross-Origin-Opener-Policy` or `Cross-Origin-Embedder-Policy`. If your site already sends
  COEP `require-corp`, also send `Cross-Origin-Resource-Policy: same-origin` (or `cross-origin`
  when the files live on a CDN) on the app's files.

nginx example:

```nginx
location /deckcraft/ {
    types { application/wasm wasm; text/javascript js; text/html html; }
    gzip on;
    gzip_types application/wasm text/javascript text/html;
    location ~* \.(wasm|js)$ { add_header Cache-Control "public, max-age=31536000, immutable"; }
    location ~* index\.html$ { add_header Cache-Control "no-cache"; }
}
```

Local test: `python3 -m http.server 8765` inside the folder, then open http://localhost:8765/.

## Embedding in a page (iframe)

```html
<iframe
  src="https://example.com/deckcraft/"
  title="DeckCraft presentations"
  style="width: 100%; height: 720px; border: 0;"
  allow="fullscreen; clipboard-read; clipboard-write"
  allowfullscreen>
</iframe>
```

- The app fills the iframe and follows its size, so size the iframe and not the app.
- Keyboard shortcuts go to the iframe after the user clicks into it, as with any embedded app.
- **Cross-origin embeds** work. The web build keeps no preferences between visits; it starts
  with the sample deck (or `?blank`) every time. Opening and saving through the page
  (`?host=parent`, below) needs the same origin.
- **Sandboxed iframes** need at least
  `sandbox="allow-scripts allow-same-origin allow-downloads allow-popups"`. Without `allow-downloads`, Save and Export (browser
  downloads) are blocked. With `?host=parent`, `allow-scripts allow-same-origin` is enough for
  opening and saving (without `allow-same-origin` the frame's origin is `null` and there is no
  bridge), and `allow-downloads` isn't needed.
- Don't send `X-Frame-Options: DENY` or a `frame-ancestors` CSP that excludes the embedding page.

## Opening and saving through the page (`?host=parent`)

A page that keeps the files itself (a Nextcloud app, a file server) can frame DeckCraft with
`?host=parent` on the iframe `src`, hand it a presentation and take back what it saves, instead of
the user downloading. GridCraft, WordCraft and PdfCraft have the same bridge with their own prefix.
The page and DeckCraft must be on the **same origin**: DeckCraft only hears and answers its parent
window on its own origin, and without one it downloads as usual.

Messages are plain objects with a `type`:

| Message | Direction | Fields |
|---|---|---|
| `deckcraft:ready` | to the page | `version`, `appVersion`. The app runs and listens: send the file now. |
| `deckcraft:open` | from the page | `name` (a bare file name, such as `Talk.pptx`), `bytes` (`ArrayBuffer` or `Uint8Array`). Opens like File › Open: a presentation (.pptx, .potx, .ppsx, .deckcraft) has that name as its file; an outline (.txt, .md, .rtf), a file sent without a name and a damaged file open untitled. |
| `deckcraft:save` | to the page | `name`, `saveAs`, `bytes` (a transferred `ArrayBuffer`), for every file DeckCraft would otherwise download. The page stores it. |
| `deckcraft:dirty` | to the page | `dirty`, whenever unsaved changes appear or go away. |
| `deckcraft:failed` | to the page | `error`, when the app couldn't start (no WebGPU or WebGL2) or refused a file of 2 GiB or larger. |

`saveAs` says whether the file becomes the presentation's file, as on the desktop:

- **Save** sends the file the page sent, under its own name and format (`Talk.pptx` comes back
  as `Talk.pptx`), with `saveAs: false`. DeckCraft keeps only the part after the last `/`, so
  `docs/Talk.pptx` also comes back as `Talk.pptx`. A presentation that has no file the page sent
  (a new one, an outline, a file sent without a name, a file the user dropped, a repaired copy of
  a damaged file) is a new file with `saveAs: true`: Ctrl+S names it `<title>.deckcraft` as Save
  suggests on the desktop, and File › Save opens the Save As page first, as on the desktop.
- **Save As** (.pptx or .deckcraft) sends `saveAs: true`. The presentation is now that file; the
  next Save sends its name with `saveAs: false`.
- **Exports and Print** (PDF, handouts, pictures, outline, GIF) send `saveAs: false` and are
  copies: the presentation keeps its file and its unsaved changes.

`saveAs: false` doesn't mean "safe to overwrite": an export is `false` under a name the page may
already hold (`Talk.pdf`, or `Talk.pptx` exported from a presentation that is now
`Talk.deckcraft`). Replace a file without asking only when the name is the one the page sent, or
the name of the last `saveAs: true` file it stored for this presentation; ask for anything else.
DeckCraft doesn't learn the name the page finally stores a Save As under, so a page that renames
on a conflict should map later Saves of the old name itself.

In host mode DeckCraft starts with an empty presentation instead of the sample, and the first
presentation the page sends takes its place unless the user has changed it; later ones open
beside it. The page guards leaving itself (DeckCraft adds no guard of its own), and
`&author=Name` signs the user's comments.

```html
<iframe id="deck" src="/deckcraft/?host=parent&author=Ada" style="width: 100%; height: 720px; border: 0;"></iframe>
<script>
  const frame = document.getElementById("deck");
  addEventListener("message", async (e) => {
    if (e.origin !== location.origin || e.source !== frame.contentWindow) return;
    if (e.data.type === "deckcraft:ready") {
      const bytes = await (await fetch("/files/Talk.pptx")).arrayBuffer();
      frame.contentWindow.postMessage({ type: "deckcraft:open", name: "Talk.pptx", bytes }, location.origin, [bytes]);
    } else if (e.data.type === "deckcraft:save") {
      // Store e.data.bytes as e.data.name. Replace silently only when !e.data.saveAs and the name
      // is the one sent (or the last saveAs file stored); ask before replacing anything else.
    }
  });
</script>
```

The messages and both transports are the shared contract of [`craft-web-host`](https://github.com/storytold/craft-libs/tree/main/crates/web-host) (storytold/craft-libs#420), the same for every Craft app: its README lists every message, including `saved`, with the compatibility notes. In `deckcraft:ready`, `version` is the contract's (`"1"`) and `appVersion` is this app's release. DeckCraft reads bytes, so a host sends `bytes`, not a `File`.

### Driving it from the page (`window.deckcraft`)

Every DeckCraft web build puts `window.deckcraft` on the page, from [`craft-web-api`](https://github.com/storytold/craft-libs/tree/main/crates/web-api) (storytold/craft-libs#413), the same shape in every Craft app. `deckcraft.request(method, params)` calls any method of the control channel the desktop app serves (`docs/control-protocol.md`), and `execute(command, params)`, `commands()`, `docInspect(params)`, `uiInspect(params)` and `screenshot(params)` are shortcuts for the common ones. Each returns a Promise that resolves with the method's result and rejects with an `Error`; a call the app does not answer within 30 seconds rejects.

```js
const deck = await deckcraft.docInspect();                 // a summary of the open presentation
await deckcraft.execute("shape.insert", { preset: "roundRect", rect: [100, 100, 240, 120], text: "Hello" });
const { data } = await deckcraft.screenshot();            // the canvas as PNG (base64)
```

The API is there in every mode, with or without a host. Opening a file, handing a saved or exported file to the page, the file name and the unsaved flag stay with the host messages above.

## Renderer selection and fallback flags

DeckCraft renders with wgpu. It uses **WebGPU** when the browser has it and falls back to
**WebGL2** on its own. URL query flags override this, and they work on the iframe `src` too:

| Flag | Effect |
|---|---|
| *(none)* | WebGPU if available, otherwise WebGL2 |
| `?webgl` | Force the WebGL2 backend (useful when a WebGPU driver misbehaves) |
| `?blank` | Start with an empty presentation instead of the sample deck |
| `?show` | Start the slide show immediately (kiosk/embedded decks) |
| `?host=parent` | Open and save through the framing page ([above](#opening-and-saving-through-the-page-hostparent)); `&author=` names the user |

For example: `<iframe src="https://example.com/deckcraft/?webgl" ...>`.

A browser with neither WebGPU nor WebGL2 gets a message in place of the app.
