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
  with the sample deck (or `?blank`) every time.
- **Sandboxed iframes** need at least
  `sandbox="allow-scripts allow-same-origin allow-downloads allow-popups"`. Without `allow-downloads`, Save and Export (browser
  downloads) are blocked.
- Don't send `X-Frame-Options: DENY` or a `frame-ancestors` CSP that excludes the embedding page.

## Renderer selection and fallback flags

DeckCraft renders with wgpu. It uses **WebGPU** when the browser has it and falls back to
**WebGL2** on its own. URL query flags override this, and they work on the iframe `src` too:

| Flag | Effect |
|---|---|
| *(none)* | WebGPU if available, otherwise WebGL2 |
| `?webgl` | Force the WebGL2 backend (useful when a WebGPU driver misbehaves) |
| `?blank` | Start with an empty presentation instead of the sample deck |
| `?show` | Start the slide show immediately (kiosk/embedded decks) |

For example: `<iframe src="https://example.com/deckcraft/?webgl" ...>`.

A browser with neither WebGPU nor WebGL2 gets a message in place of the app.

## craft-live (Nextcloud Office)

[craft-live](https://github.com/sirmmo/craft-live) serves this build to Nextcloud Office as its
presentation editor. Put the site's files in the folder craft-live's configuration names for
DeckCraft, and craft-live publishes `.pptx`, `.pptm` and `.odp` to Nextcloud:

```sh
packaging/web/package.sh            # or: cd apps/deckcraft-web && trunk build --release
mkdir -p /srv/craft-live/apps/deckcraft
cp -R dist/web/. /srv/craft-live/apps/deckcraft/
```

```toml
# craft-live.toml
[apps]
deckcraft = "/srv/craft-live/apps/deckcraft"
```

craft-live serves the folder at `/apps/deckcraft/` (the relative URLs above make that work) and
loads `index.html#craft-live=1&…` in its editor page; the build sees the fragment and becomes a
live editor of that one presentation (docs/collab.md, "In Nextcloud"). Nothing else to set: the
WebSocket and the API are craft-live's own origin, and the build needs no headers beyond the
MIME types above. Check with `craft-live check --config craft-live.toml`. Without the fragment the
same build is the ordinary web app.

Without trunk (what `trunk build` does, by hand):

```sh
cargo build --release -p deckcraft-web --target wasm32-unknown-unknown
wasm-bindgen --target web --no-typescript --out-dir dist/web --out-name deckcraft-web \
  target/wasm32-unknown-unknown/release/deckcraft-web.wasm
# then an index.html like apps/deckcraft-web/index.html whose module script imports
# ./deckcraft-web.js and calls its default export (trunk writes that line itself)
```
