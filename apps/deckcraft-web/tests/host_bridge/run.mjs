// Headless browser test for the host bridge (apps/deckcraft-web/src/host.rs).
//
// Serves a same-origin parent page that frames deckcraft-web, then checks the round trip:
// deckcraft:save carries an id; the parent's deckcraft:saved { id } marks the document saved;
// a dropped reply leaves it dirty and reports deckcraft:failed after the 30 s deadline; and
// deckcraft:dirty is reported as unsaved changes appear and go away.
//
// Requirements (the test SKIPS with exit 0 when one is missing, so CI without them stays green):
// - wasm-bindgen 0.2.129 on PATH (`cargo install wasm-bindgen-cli --version 0.2.129 --locked`);
// - a wasm build: `cargo build --target wasm32-unknown-unknown -p deckcraft-web --release`
//   (DECKCRAFT_WASM overrides the path);
// - Playwright: DECKCRAFT_PLAYWRIGHT names its package directory, else `playwright` must resolve.
//
// Run: node apps/deckcraft-web/tests/host_bridge/run.mjs (from the repository root).

import { execFileSync } from "node:child_process";
import { createRequire } from "node:module";
import { existsSync, mkdirSync, readFileSync } from "node:fs";
import { createServer } from "node:http";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "../../../..");
const outDir = join(root, "target", "host-bridge-test");
const wasmPath = process.env.DECKCRAFT_WASM || join(root, "target/wasm32-unknown-unknown/release/deckcraft-web.wasm");
const REQUIRED_BINDGEN = "0.2.129";
const ACK_DEADLINE_MS = 30_000; // SAVE_ACK_TIMEOUT_MS in src/bridge.rs
const SKIP = (why) => {
  console.log(`SKIP host-bridge browser test: ${why}`);
  process.exit(0);
};

function bindgenVersion() {
  try {
    return execFileSync("wasm-bindgen", ["--version"], { encoding: "utf8" }).trim().split(/\s+/)[1];
  } catch {
    return null;
  }
}

function loadPlaywright() {
  const candidates = [process.env.DECKCRAFT_PLAYWRIGHT, "playwright"].filter(Boolean);
  for (const c of candidates) {
    try {
      return createRequire(import.meta.url)(c);
    } catch {
      /* try the next */
    }
  }
  return null;
}

const version = bindgenVersion();
if (version !== REQUIRED_BINDGEN) SKIP(`wasm-bindgen ${REQUIRED_BINDGEN} not on PATH (found ${version ?? "none"})`);
if (!existsSync(wasmPath)) SKIP(`no wasm build at ${wasmPath}`);
const playwright = loadPlaywright();
if (!playwright) SKIP("playwright is not installed (set DECKCRAFT_PLAYWRIGHT)");

// JS glue for the wasm, next to the page that loads it.
mkdirSync(outDir, { recursive: true });
execFileSync("wasm-bindgen", ["--target", "web", "--no-typescript", "--out-dir", outDir, wasmPath], { stdio: "inherit" });

const PARENT_HTML = `<!doctype html>
<meta charset="utf-8"><title>host parent</title>
<iframe id="app" src="/app/index.html?host=parent&blank&webgl" style="width:960px;height:640px;border:0"></iframe>
<script>
  window.messages = [];
  window.addEventListener("message", (e) => {
    if (e.source !== document.getElementById("app").contentWindow) return;
    const d = e.data || {};
    window.messages.push({ type: d.type, id: d.id, dirty: d.dirty, error: d.error, name: d.name,
      bytes: d.bytes && d.bytes.byteLength, version: d.version });
  });
  // Answer a save as the parent page would (same origin).
  window.confirmSave = (id) =>
    document.getElementById("app").contentWindow.postMessage({ type: "deckcraft:saved", id }, location.origin);
</script>`;

const APP_HTML = `<!doctype html>
<meta charset="utf-8"><title>DeckCraft</title>
<style>html,body{margin:0;height:100%;overflow:hidden;background:#262626}#deckcraft_canvas{position:absolute;inset:0;width:100%;height:100%}</style>
<canvas id="deckcraft_canvas"></canvas>
<script type="module">
  import init from "./pkg/deckcraft-web.js";
  init();
</script>`;

const TYPES = { ".js": "text/javascript", ".wasm": "application/wasm", ".html": "text/html" };
const server = createServer((req, res) => {
  const path = new URL(req.url, "http://x").pathname;
  let body = null;
  if (path === "/parent.html") body = PARENT_HTML;
  else if (path === "/app/index.html") body = APP_HTML;
  else if (path.startsWith("/app/pkg/")) {
    const rel = decodeURIComponent(path.slice("/app/pkg/".length));
    const file = join(outDir, rel);
    if (!rel.includes("..") && existsSync(file)) body = readFileSync(file);
  }
  if (body === null) {
    res.writeHead(404).end();
    return;
  }
  res.writeHead(200, { "content-type": TYPES[path.slice(path.lastIndexOf("."))] ?? "text/html" }).end(body);
});
await new Promise((ok) => server.listen(0, "127.0.0.1", ok));
const base = `http://127.0.0.1:${server.address().port}`;

const results = [];
let errors = [];
const check = (name, ok, detail = "") => {
  results.push({ name, ok });
  console.log(`${ok ? "PASS" : "FAIL"} ${name}${detail ? `: ${detail}` : ""}`);
};
const waitFor = async (pred, ms, what) => {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    const v = await pred();
    if (v) return v;
    await new Promise((r) => setTimeout(r, 250));
  }
  throw new Error(`timed out waiting for ${what}`);
};

// The cached headless shell (Playwright 1228); DECKCRAFT_CHROMIUM overrides it.
const executablePath = process.env.DECKCRAFT_CHROMIUM || join(process.env.HOME, "Library/Caches/ms-playwright/chromium_headless_shell-1228/chrome-headless-shell-mac-arm64/chrome-headless-shell");
const browser = await playwright.chromium.launch({ headless: true, executablePath, args: ["--enable-unsafe-swiftshader", "--use-angle=swiftshader"] });
let exitCode = 0;
try {
  const page = await browser.newPage({ viewport: { width: 1000, height: 700 } });
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("response", (r) => { if (r.status() >= 400) errors.push(r.status() + " " + r.url()); });
  page.on("console", (m) => { if (m.type() === "error" || m.type() === "warning") errors.push(m.text()); });
  await page.goto(`${base}/parent.html`);
  const log = () => page.evaluate(() => window.messages);

  const ready = await waitFor(async () => (await log()).find((m) => m.type === "deckcraft:ready"), 120_000, "deckcraft:ready");
  check("app posts deckcraft:ready with a version", !!ready.version, ready.version ?? "");

  // The title placeholder takes keyboard input after a click on the canvas.
  const canvas = page.frameLocator("#app").locator("canvas");
  const count = async (type) => (await log()).filter((m) => m.type === type).length;
  const nextMessage = (type, from, ms, what) =>
    waitFor(async () => (await log()).filter((m) => m.type === type).slice(from).at(0), ms, what);
  const dirtyStates = async () => (await log()).filter((m) => m.type === "deckcraft:dirty").map((m) => m.dirty);

  await canvas.click({ position: { x: 480, y: 320 } });
  await page.keyboard.type("Hello");
  await waitFor(async () => (await dirtyStates()).includes(true), 30_000, "deckcraft:dirty true after an edit");
  check("an edit reports deckcraft:dirty { dirty: true }", true);

  // Save, confirm: the parent's answer marks the document saved.
  let from = await count("deckcraft:save");
  await page.keyboard.press("Control+s");
  const save = await nextMessage("deckcraft:save", from, 30_000, "deckcraft:save");
  check("Save posts deckcraft:save with an id and a .deckcraft name", Number.isInteger(save.id) && /\.deckcraft$/.test(save.name), JSON.stringify({ id: save.id, name: save.name }));
  check("deckcraft:save carries the bytes", save.bytes > 0, `${save.bytes} bytes`);
  const cleanBefore = (await dirtyStates()).length;
  await page.evaluate((id) => window.confirmSave(id), save.id);
  await waitFor(async () => (await dirtyStates()).slice(cleanBefore).includes(false), 15_000, "deckcraft:dirty false after the parent's deckcraft:saved");
  check("parent deckcraft:saved { id } marks the document saved (dirty false)", true);
  check("no deckcraft:failed for the confirmed save", !(await log()).some((m) => m.type === "deckcraft:failed" && m.id === save.id));

  // Dropped reply: edit, save, and the parent never answers. The save fails after the deadline
  // and the document stays dirty.
  await page.keyboard.type("!");
  await waitFor(async () => (await dirtyStates()).at(-1) === true, 30_000, "dirty again after the second edit");
  from = await count("deckcraft:save");
  await page.keyboard.press("Control+s");
  const dropped = await nextMessage("deckcraft:save", from, 30_000, "second deckcraft:save");
  const failed = await waitFor(
    async () => (await log()).find((m) => m.type === "deckcraft:failed" && m.id === dropped.id),
    ACK_DEADLINE_MS + 30_000,
    "deckcraft:failed for the dropped reply",
  );
  check("a dropped deckcraft:saved yields deckcraft:failed after the deadline", !!failed, failed.error ?? "");
  await new Promise((r) => setTimeout(r, 1500));
  check("the document stays dirty after the dropped reply", (await dirtyStates()).at(-1) === true, JSON.stringify(await dirtyStates()));
  check("deckcraft:dirty reports both states", (await dirtyStates()).includes(true) && (await dirtyStates()).includes(false), JSON.stringify(await dirtyStates()));
  if (errors.length) console.log("page errors:", errors.slice(0, 5));
} catch (e) {
  console.log(`ERROR ${e.message}`);
  if (typeof errors !== "undefined" && errors.length) console.log("page errors:", errors.slice(0, 5));
  exitCode = 1;
} finally {
  await browser.close();
  server.close();
}
const failedChecks = results.filter((r) => !r.ok).length;
console.log(`host-bridge: ${results.length - failedChecks}/${results.length} checks passed`);
process.exit(exitCode || (failedChecks ? 1 : 0));
