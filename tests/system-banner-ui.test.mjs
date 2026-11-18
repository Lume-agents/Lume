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
const output = mkdtempSync(join(tmpdir(), "lume-system-banner-ui-"));
const fixture = `<script>
  import SystemBanner from './src/lib/SystemBanner.svelte';
  let shown = $state(true);
  window.__show = () => { shown = true; };
  window.__dismissed = 0;
<\/script>
<main data-appearance="lume">
  {#if shown}
    <SystemBanner tone="error" inline message={"Erro de teste: " + "a mensagem inteira do erro continua por várias palavras ".repeat(6)} onDismiss={() => { window.__dismissed += 1; shown = false; }} onInspect={() => {}} />
  {/if}
</main>
<style>main { max-width: 420px; margin: 40px auto; }</style>`;
const compiled = compile(fixture, { filename: join(root, "BannerFixture.svelte"), name: "BannerFixture", css: "injected" }).js.code;
await build({
  stdin: { contents: `import './src/app.css';\n${compiled}\nimport { mount } from 'svelte'; mount(BannerFixture, { target: document.getElementById('app') });`, resolveDir: root },
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
  // The runners of some systems ask for reduced motion, and then an error banner never collapses.
  await call("Emulation.setEmulatedMedia", { features: [{ name: "prefers-reduced-motion", value: "no-preference" }] });
  await call("Emulation.setDeviceMetricsOverride", { width: 600, height: 500, deviceScaleFactor: 1, mobile: false });
  await call("Page.navigate", { url: `http://127.0.0.1:${server.address().port}` });
  const height = () => evaluate("document.querySelector('.system-banner p').getBoundingClientRect().height");
  const poll = async (expression, ms) => { const end = Date.now() + ms; while (Date.now() < end) { if (await evaluate(expression)) return true; await delay(60); } return false; };
  await wait("document.querySelector('.system-banner')");
  const full = await height();
  assert.ok(full > 30, "a new error shows its whole message: " + full);
  assert.equal(await evaluate("document.querySelector('.system-banner').classList.contains('collapsed')"), false);
  // After a short delay the message shrinks to one line.
  assert.ok(await poll("document.querySelector('.system-banner.collapsed')", 6000), "the banner collapses");
  await delay(500);
  assert.ok(await height() < full / 2, "to a single line");
  await screenshot("collapsed");
  // Pointing at it reads it in full again and keeps it open.
  await call("Input.dispatchMouseEvent", { type: "mouseMoved", x: 100, y: 58 });
  assert.ok(await poll("!document.querySelector('.system-banner.collapsed')", 2000), "hovering expands it");
  await call("Input.dispatchMouseEvent", { type: "mouseMoved", x: 580, y: 480 });
  // Then it collapses again and, later, leaves.
  assert.ok(await poll("document.querySelector('.system-banner.collapsed')", 7000), "it collapses again");
  assert.ok(await poll("!document.querySelector('.system-banner')", 14000), "and leaves after a longer delay");
  assert.equal(await evaluate("window.__dismissed"), 1);
  assert.deepEqual(errors, []);
  console.log("System banner UI passed. Screenshots: " + output);
} finally {
  socket?.close();
  browser.kill();
  server.close();
}
