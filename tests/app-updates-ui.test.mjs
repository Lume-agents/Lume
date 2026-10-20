import assert from "node:assert/strict";
import { build } from "esbuild";
import { compile } from "svelte/compiler";
import { readFileSync, existsSync, mkdtempSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { spawn } from "node:child_process";
import { join, dirname, extname, sep } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import { once } from "node:events";
import { executable, browserArguments } from "./support/browser.mjs";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const output = mkdtempSync(join(tmpdir(), "lume-updates-ui-"));
const fixture = `<script>
  import { mockIPC, mockWindows } from '@tauri-apps/api/mocks';
  import { emit } from '@tauri-apps/api/event';
  import { appUpdates, startAppUpdatesOwner, startAppUpdatesPeer } from './src/lib/appUpdateRuntime';
  import AppUpdateNotice from './src/lib/AppUpdateNotice.svelte';
  import UpdateBanner from './src/lib/UpdateBanner.svelte';
  let dark = $state(true);
  window.__setDark = value => { dark = value; };
  window.__updates = appUpdates;
  window.__calls = [];
  window.__nativeVisible = true;
  window.__releaseBody = '## Improvements\\n- Unified updates in Orb and Workspace.\\n- Patch notes appear once per version.\\n\\n## Fixes\\n' + Array.from({ length: 30 }, (_, i) => '- Fix ' + (i + 1) + ': readable release notes.').join('\\n') + '\\n\\n<script>alert("unsafe")<\\/script>\\n[javascript](javascript:alert(1))';
  mockWindows('main', ['main', 'workspace']);
  mockIPC(async (command, args) => {
    window.__calls.push([command, args]);
    if (command === 'plugin:app|version') return '0.18.2';
    if (command === 'plugin:window|is_visible') return window.__nativeVisible;
    if (command === 'plugin:event|emit_to') return emit(args.event, args.payload);
    if (command === 'plugin:updater|check') return { rid: 1, currentVersion: '0.18.2', version: '0.18.3', body: '## Next release', rawJson: {} };
    if (command === 'plugin:updater|download_and_install') {
      args.onEvent.onmessage({ event: 'Started', data: { contentLength: 100 } });
      args.onEvent.onmessage({ event: 'Progress', data: { chunkLength: 40 } });
      await new Promise(resolve => { window.__finishInstall = resolve; });
      args.onEvent.onmessage({ event: 'Finished' });
    }
    return null;
  }, { shouldMockEvents: true });
  window.fetch = async url => { window.__releaseUrl = String(url); return new Response(JSON.stringify({ tag_name: 'v0.18.2', body: window.__releaseBody }), { status: 200 }); };
  window.__start = async () => { window.__stop = await startAppUpdatesOwner(); window.__stopPeer = await startAppUpdatesPeer(); };
</script>
<main class:dark data-appearance="lume">
  <div class="settings"><h1>Ajustes</h1><details><summary>Preferências</summary></details><details><summary>Redefinir</summary></details><UpdateBanner state={$appUpdates} language="pt-BR" {dark} /></div>
  <AppUpdateNotice language="pt-BR" {dark} />
</main>
<style>
  main { min-height:100dvh; padding:24px; background:var(--lume-canvas-light); color:var(--lume-ink-light); }
  main.dark { background:var(--lume-canvas-dark); color:var(--lume-ink-dark); }
  .settings { max-width:400px; margin:0 auto; display:grid; gap:16px; }
  h1 { margin:0; font-size:22px; }
  details { padding:12px; border-bottom:1px solid currentColor; }
</style>`;
const compiled = compile(fixture, { filename: join(root, "UpdateFixture.svelte"), name: "UpdateFixture", css: "injected" }).js.code;
await build({
  stdin: { contents: `import './src/app.css';\n${compiled}\nimport { mount } from 'svelte'; mount(UpdateFixture, { target: document.getElementById('app') });`, resolveDir: root },
  bundle: true, outfile: join(output, "app.js"), format: "esm", platform: "browser", conditions: ["browser", "svelte"],
  external: ["/fonts/*"], loader: { ".woff2": "file", ".woff": "file", ".png": "file", ".svg": "file" },
  plugins: [{ name: "svelte-fixture", setup(builder) {
    builder.onResolve({ filter: /^\$lib\// }, ({ path }) => ({ path: join(root, "src/lib", path.slice(5) + (extname(path) ? "" : ".ts")) }));
    builder.onLoad({ filter: /\.svelte$/ }, ({ path }) => ({ contents: compile(readFileSync(path, "utf8"), { filename: path, css: "injected" }).js.code, loader: "js", resolveDir: dirname(path) }));
  } }],
});
const server = createServer((request, response) => {
  const path = new URL(request.url, "http://localhost").pathname;
  if (path === "/") { response.setHeader("content-type", "text/html"); response.end('<html lang="pt-BR"><head><link rel="stylesheet" href="/app.css"></head><body><div id="app"></div><script type="module" src="/app.js"></script></body></html>'); return; }
  if (path === "/lume.svg") { response.setHeader("content-type", "image/svg+xml"); response.end(readFileSync(join(root, "static/lume.svg"))); return; }
  const file = join(output, path.slice(1));
  if (!file.startsWith(output + sep) || !existsSync(file)) { response.writeHead(404).end(); return; }
  response.setHeader("content-type", { ".js": "text/javascript", ".css": "text/css" }[extname(file)] || "application/octet-stream"); response.end(readFileSync(file));
});
server.listen(0, "127.0.0.1"); await once(server, "listening");
const browser = spawn(executable, browserArguments(), { stdio: ["ignore", "ignore", "pipe"] });
let socket;
try {
  const endpoint = await new Promise((resolve, reject) => {
    let log = "";
    const timer = setTimeout(() => reject(new Error(log)), 15000);
    browser.stderr.on("data", chunk => { log += chunk; const match = log.match(/DevTools listening on (ws:\/\/\S+)/); if (match) { clearTimeout(timer); resolve(match[1]); } });
    browser.once("exit", code => { clearTimeout(timer); reject(new Error(`Chromium: ${code}`)); });
  });
  socket = new WebSocket(endpoint); await new Promise(resolve => { socket.onopen = resolve; });
  let id = 0;
  const pending = new Map(); const errors = [];
  socket.onmessage = ({ data }) => {
    const message = JSON.parse(data);
    if (message.method === "Runtime.exceptionThrown") errors.push(message.params.exceptionDetails.exception?.description ?? message.params.exceptionDetails.text);
    if (message.id) { const request = pending.get(message.id); pending.delete(message.id); message.error ? request.reject(new Error(message.error.message)) : request.resolve(message.result); }
  };
  const send = (method, params = {}, sessionId) => new Promise((resolve, reject) => {
    const number = ++id; pending.set(number, { resolve, reject }); socket.send(JSON.stringify({ id: number, method, params, sessionId }));
  });
  const { targetId } = await send("Target.createTarget", { url: "about:blank" });
  const { sessionId } = await send("Target.attachToTarget", { targetId, flatten: true });
  const call = (method, params) => send(method, params, sessionId);
  const evaluate = async expression => {
    const result = await call("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
    if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text);
    return result.result.value;
  };
  const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
  async function wait(expression) {
    const deadline = Date.now() + 10000;
    while (Date.now() < deadline) { if (await evaluate(expression)) return; await delay(40); }
    throw new Error(`Timed out: ${expression}\n${errors.join("\n")}`);
  }
  async function screenshot(name) { await delay(200); const { data } = await call("Page.captureScreenshot", { format: "png" }); writeFileSync(join(output, `${name}.png`), Buffer.from(data, "base64")); }
  await call("Runtime.enable"); await call("Page.enable");
  await call("Emulation.setDeviceMetricsOverride", { width: 1200, height: 800, deviceScaleFactor: 1, mobile: false });
  await call("Page.navigate", { url: `http://127.0.0.1:${server.address().port}` });
  await wait("window.__start");
  await evaluate("localStorage.clear(); window.__start()");
  await wait("document.querySelector('[role=dialog]') && document.querySelector('.release-notes-body li')");
  assert.match(await evaluate("window.__releaseUrl"), /\/tags\/v0\.18\.2$/);
  assert.equal(await evaluate("window.__calls.filter(([command]) => command === 'plugin:updater|check').length"), 1, "peer startup must not issue a second native check");
  assert.equal(await evaluate("document.querySelector('.release-notes-body script, .release-notes-body a[href^=javascript]')"), null);
  assert.ok(await evaluate("document.querySelector('.release-notes-body').scrollHeight > document.querySelector('.release-notes-body').clientHeight"));
  await screenshot("notes-workspace-dark");
  await call("Emulation.setDeviceMetricsOverride", { width: 390, height: 520, deviceScaleFactor: 1, mobile: false });
  await evaluate("window.__setDark(false)"); await delay(100);
  assert.ok(await evaluate("(() => { const r = document.querySelector('[role=dialog]').getBoundingClientRect(); return r.left >= 0 && r.right <= innerWidth && r.bottom <= innerHeight; })()"));
  await screenshot("notes-orb-light");
  await call("Input.dispatchKeyEvent", { type: "keyDown", key: "Escape", code: "Escape" });
  await wait("!document.querySelector('[role=dialog]') && document.querySelector('.app-update-notice')");
  assert.equal(await evaluate("localStorage.getItem('lume:updates:release-notes-seen')"), "0.18.2");
  assert.equal(await evaluate("window.__calls.filter(([command]) => command === 'plugin:updater|download_and_install').length"), 0);
  await screenshot("update-notice-orb-light");
  await evaluate("document.querySelector('.app-update-notice button.dismiss').click()");
  await wait("!document.querySelector('.app-update-notice')");
  assert.match(await evaluate("document.querySelector('.settings [data-update-card]').textContent"), /0\.18\.3 disponível/);
  // Hidden Orb windows defer presentation until their native window becomes visible.
  await evaluate("window.__nativeVisible = false; window.__updates.update(s => ({ ...s, dismissedVersion: null, notify: true })); window.dispatchEvent(new Event('focus'))");
  await delay(250);
  assert.equal(await evaluate("document.querySelector('.app-update-notice')"), null);
  await evaluate("window.__nativeVisible = true; window.dispatchEvent(new Event('focus'))");
  await wait("document.querySelector('.app-update-notice')");
  await evaluate("document.querySelector('.app-update-notice button.primary').click()");
  await wait("document.querySelector('.app-update-notice [role=progressbar]')?.getAttribute('aria-valuenow') === '40'");
  assert.equal(await evaluate("document.querySelector('.app-update-notice button:not(.dismiss)').disabled"), true);
  await screenshot("download-orb-light");
  await evaluate("window.__finishInstall()");
  await wait("window.__calls.some(([command]) => command === 'plugin:process|restart')");
  assert.equal(await evaluate("window.__calls.filter(([command]) => command === 'plugin:updater|download_and_install').length"), 1);
  await evaluate("window.__stopPeer(); window.__stop(); window.__calls.length = 0; window.__start()");
  await wait("window.__calls.some(([command]) => command === 'plugin:updater|check')");
  await delay(200);
  assert.equal(await evaluate("document.querySelector('[role=dialog]')"), null, "the same release notes must not reopen after restart");
  assert.deepEqual(errors, []);
  console.log(`App updater browser checks passed (Orb/Workspace, dark/light, consent, progress, dismissal, safe Markdown). Screenshots: ${output}`);
} finally { socket?.close(); browser.kill("SIGTERM"); server.closeAllConnections(); server.close(); }
