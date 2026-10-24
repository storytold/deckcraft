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

## Fonts

The wasm carries the faces it was built with (the craft-fonts families and egui's own), so a deck
that needs another typeface is drawn with a substitute. A host can offer faces of its own: put a
`fonts/manifest.txt` next to `index.html`, one face per line —

```
Family | Style | path/to/file.otf
Brand Sans | Regular | fonts/brand/BrandSans-Regular.otf
Brand Sans | Bold | fonts/brand/BrandSans-Bold.otf
```

- Only the first three fields are read; blank lines and lines starting with `#` are ignored, and
  the path is relative to the page.
- The files are served as they are: no conversion, no preprocessing.
- A face is filed under the family **it declares** (its `name` table: id 16 when the file has one,
  else id 1), not under the name the manifest gives it. When the two differ the browser console
  says so and the file's own name is used — that is the name a deck's text asks for.
- A page without a manifest starts as it always has, with the embedded faces only.

DeckCraft and the other crafting apps use the same file name and layout, so one `fonts/` directory
can serve several of them.

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
