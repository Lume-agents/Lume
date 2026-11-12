// Browser test of token usage in the chat header: the button, its popover with the per-prompt chart the Inspector
// also uses, and the tooltip on the chart's points, in light and dark. Set LUME_TEST_BROWSER for Chromium.
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
const output = mkdtempSync(join(tmpdir(), "lume-token-usage-ui-"));
const read = (path) => readFileSync(join(root, path), "utf8");

// The Inspector and the header draw the same component, so the two charts cannot drift apart.
assert.match(read("src/lib/WorkspaceInspector.svelte"), /<TokenUsageChart samples=\{tokenSamples\}/);
assert.match(read("src/lib/WorkspaceSessionPane.svelte"), /\{#if session\.promptTokenUsage\?\.length\}<ChatUsageButton usage=\{session\.promptTokenUsage\}/);
assert.ok(!/<title>/.test(read("src/lib/TokenUsageChart.svelte")), "the points use the tooltip, not a native title");

await build({
  stdin: {
    contents: 'import "./src/app.css"; import "./src/lib/workspaceTokens.css"; import { mount } from "svelte"; import Button from "./src/lib/ChatUsageButton.svelte"; import Metrics from "./src/lib/TokenMetrics.svelte"; for (const [id, usage] of Object.entries(window.__usage)) mount(Button, { target: document.getElementById(id), props: { usage, language: "pt-BR" } }); for (const id of ["metrics-light", "metrics-dark"]) mount(Metrics, { target: document.getElementById(id), props: { samples: window.__metrics, agent: "claude_code", language: "en", onJump: (target) => window.__jumps.push(target) } });',
    resolveDir: root,
  },
  external: ["/fonts/*"],
  outfile: join(output, "app.js"), bundle: true, format: "esm", platform: "browser", conditions: ["browser", "svelte"],
  loader: { ".woff2": "file", ".woff": "file", ".png": "file", ".svg": "file" },
  plugins: [{
    name: "svelte-browser-fixture",
    setup(builder) {
      builder.onResolve({ filter: /^\$lib\// }, ({ path }) => ({ path: join(root, "src/lib", path.slice(5) + (extname(path) ? "" : ".ts")) }));
      builder.onLoad({ filter: /\.svelte$/ }, ({ path }) => ({ contents: compile(readFileSync(path, "utf8"), { filename: path, css: "injected" }).js.code, loader: "js", resolveDir: dirname(path) }));
    },
  }],
});

function fixture() {
  const start = Date.UTC(2026, 10, 12, 14, 0);
  const turn = (index, totalTokens, context) => ({ turnId: "turn-" + index, totalTokens, inputTokens: Math.round(totalTokens * 0.86), outputTokens: Math.round(totalTokens * 0.14), createdAt: start + index * 240_000, ...(context ? { contextTokens: context, contextWindow: 200_000 } : {}) });
  const claude = [turn(0, 12_400, 22_000), turn(1, 31_800, 54_000), turn(2, 8_200, 61_000), turn(3, 46_100, 108_000), turn(4, 19_300, 127_000), turn(5, 27_600, 151_000)];
  window.__usage = { light: claude, dark: claude, codex: [turn(0, 5_100), turn(1, 9_400)], none: [] };
  // The Inspector's metrics: cache, prompt time and model on most turns.
  window.__metrics = claude.map((sample, index) => ({ ...sample, cacheReadTokens: Math.round(sample.inputTokens * 0.6), promptAt: sample.createdAt - 30_000 - index * 4_000, model: "claude-opus-4-1" }));
  window.__jumps = [];
}

const page = `<html><head><link rel="stylesheet" href="/app.css"></head><body style="margin:0">
<div class="workspace-tokens" style="height:300px;padding:16px 24px;display:flex;justify-content:flex-end;gap:16px;background:var(--workspace-pane)"><span id="codex"></span><span id="none"></span><span id="light"></span></div>
<div class="workspace-tokens dark" style="height:300px;padding:16px 24px;display:flex;justify-content:flex-end;background:var(--workspace-pane)"><span id="dark"></span></div>
<div style="display:flex"><div class="workspace-tokens" style="width:300px;padding:40px 14px 14px;background:var(--workspace-sidebar)"><div id="metrics-light"></div></div>
<div class="workspace-tokens dark" style="width:300px;padding:40px 14px 14px;background:var(--workspace-sidebar)"><div id="metrics-dark"></div></div></div>
<script type="module" src="/app.js"></script></body></html>`;
const server = createServer((request, response) => {
  const path = new URL(request.url, "http://localhost").pathname;
  if (path === "/") { response.setHeader("content-type", "text/html"); response.end(page); return; }
  const bundled = join(output, path.slice(1));
  const publicAsset = join(root, "static", path.slice(1));
  const file = bundled.startsWith(output + sep) && existsSync(bundled) ? bundled : publicAsset;
  if ((!file.startsWith(output + sep) && !file.startsWith(join(root, "static") + sep)) || !existsSync(file)) { response.writeHead(404).end(); return; }
  response.setHeader("content-type", { ".js": "text/javascript", ".css": "text/css", ".woff2": "font/woff2" }[extname(file)] || "application/octet-stream");
  response.end(readFileSync(file));
});
server.listen(0, "127.0.0.1");
await once(server, "listening");
const browser = spawn(executable, browserArguments(), { stdio: ["ignore", "ignore", "pipe"] });
let socket;
try {
  const endpoint = await new Promise((resolve, reject) => {
    let log = "";
    const timer = setTimeout(() => reject(new Error("Chromium startup timeout: " + log.slice(-1000))), 15000);
    browser.stderr.on("data", (chunk) => { log += chunk; const match = log.match(/DevTools listening on (ws:\/\/\S+)/); if (match) { clearTimeout(timer); resolve(match[1]); } });
    browser.once("exit", (code) => { clearTimeout(timer); reject(new Error("Chromium exited: " + code)); });
  });
  socket = new WebSocket(endpoint);
  await once(socket, "open");
  const pending = new Map();
  let sequence = 0;
  const failures = [];
  socket.addEventListener("message", ({ data }) => {
    const message = JSON.parse(data);
    if (message.id) { const callback = pending.get(message.id); pending.delete(message.id); message.error ? callback.reject(new Error(message.error.message)) : callback.resolve(message.result); }
    if (message.method === "Runtime.exceptionThrown") failures.push(message.params.exceptionDetails.exception?.description || message.params.exceptionDetails.text);
  });
  const send = (method, params = {}, sessionId) => new Promise((resolve, reject) => { const id = ++sequence; pending.set(id, { resolve, reject }); socket.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) })); });
  const { targetId } = await send("Target.createTarget", { url: "about:blank" });
  const { sessionId } = await send("Target.attachToTarget", { targetId, flatten: true });
  const call = (method, params) => send(method, params, sessionId);
  await call("Runtime.enable");
  await call("Page.enable");
  await call("Emulation.setDeviceMetricsOverride", { width: 720, height: 1000, deviceScaleFactor: 1, mobile: false });
  await call("Page.addScriptToEvaluateOnNewDocument", { source: "(" + fixture.toString() + ")()" });
  const evaluate = async (expression) => {
    const result = await call("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
    if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description || result.exceptionDetails.text);
    return result.result.value;
  };
  const delay = (time = 60) => new Promise((resolve) => setTimeout(resolve, time));
  async function wait(expression) {
    const deadline = Date.now() + 10000;
    while (Date.now() < deadline) { if (await evaluate(expression)) return; await delay(); }
    throw new Error("Timed out: " + expression + "\n" + failures.join("\n"));
  }
  async function screenshot(name) {
    await evaluate("document.fonts.ready");
    await delay(240);
    const { data } = await call("Page.captureScreenshot", { format: "png" });
    writeFileSync(join(output, name + ".png"), Buffer.from(data, "base64"));
  }
  async function hover(selector, fraction) {
    const box = await evaluate("(() => { const r = document.querySelector(" + JSON.stringify(selector) + ").getBoundingClientRect(); return { x: r.left, y: r.top, width: r.width, height: r.height }; })()");
    await call("Input.dispatchMouseEvent", { type: "mouseMoved", x: box.x + box.width * fraction, y: box.y + box.height / 2 });
    await delay();
  }
  await call("Page.navigate", { url: "http://127.0.0.1:" + server.address().port });
  await wait("document.querySelectorAll('.usage-trigger').length === 3");
  assert.equal(await evaluate("document.querySelector('#none .usage-trigger')"), null, "no button without prompt usage");
  assert.equal(await evaluate("document.querySelector('#light .usage-trigger').textContent.trim()"), "76%", "the context used after the last prompt");
  assert.equal(await evaluate("document.querySelector('#codex .usage-trigger').textContent.trim().split(String.fromCharCode(160)).join(' ')"), "9,4 mil", "without a context window, the last prompt's tokens");
  for (const theme of ["light", "dark"]) {
    await evaluate("document.querySelector('#" + theme + " .usage-trigger').click()");
    await wait("document.querySelector('#" + theme + " .usage-popover .token-graph')");
    assert.deepEqual(await evaluate("Array.from(document.querySelectorAll('#" + theme + " .usage-totals dd')).map(item => item.textContent.split(String.fromCharCode(160)).join(' '))"), ["145 mil", "125 mil", "20 mil", "76%"], "session totals");
    assert.equal(await evaluate("document.querySelectorAll('#" + theme + " .token-graph circle').length"), 6);
    await hover("#" + theme + " .token-plot", 0.62);
    await wait("document.querySelector('#" + theme + " .token-tip')");
    const tip = await evaluate("document.querySelector('#" + theme + " .token-tip').innerText");
    assert.match(tip, /46\.100 tokens/, "the hovered prompt's total");
    assert.match(tip, /Entrada 39\.646 · Saída 6\.454/, "input and output");
    assert.match(tip, /Contexto 54%/, "context use");
    assert.match(tip, /\d{2}:\d{2}/, "and its time");
    assert.equal(await evaluate("document.querySelectorAll('#" + theme + " .token-graph circle.active').length"), 1);
    await screenshot("chat-usage-" + theme);
    await call("Input.dispatchMouseEvent", { type: "mouseMoved", x: 5, y: 5 });
    await wait("!document.querySelector('#" + theme + " .token-tip')");
  }
  // The keyboard walks the prompts too, and Escape closes the popover.
  await evaluate("document.querySelector('#dark .token-plot').focus()");
  await wait("document.querySelector('#dark .token-tip')?.innerText.includes('27.600')");
  await evaluate("document.querySelector('#dark .token-plot').dispatchEvent(new KeyboardEvent('keydown', { key: 'Home', bubbles: true }))");
  await wait("document.querySelector('#dark .token-tip')?.innerText.includes('12.400')");
  await evaluate("window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))");
  await wait("!document.querySelector('#dark .usage-popover')");
  // The Inspector's metrics: every prompt as stacked bars, the context over time and the tiles, in both themes.
  await wait("document.querySelectorAll('#metrics-light .token-bar').length === 6");
  assert.equal(await evaluate("document.querySelectorAll('#metrics-dark .context-fill polyline').length"), 1, "the context fill");
  const tiles = await evaluate("Array.from(document.querySelectorAll('#metrics-light .metric-tiles dt')).map(item => item.textContent)");
  assert.deepEqual(tiles, ["Session total", "Per turn", "Largest turn", "Context now", "Cache hits", "Time per turn", "Model"]);
  assert.equal(await evaluate("document.querySelector('#metrics-light .metric-tiles .wide dd').textContent"), "claude-opus-4-1");
  await hover("#metrics-dark .token-plot", 0.58);
  await wait("document.querySelector('#metrics-dark .token-tip')");
  await screenshot("metrics");
  await evaluate("document.querySelector('#metrics-dark .token-plot').click()");
  await evaluate("document.querySelector('#metrics-light .metric-tiles button').click()");
  const jumps = await evaluate("window.__jumps");
  assert.equal(jumps.length, 2, "a bar and the largest turn open their prompts");
  assert.equal(jumps[1].promptAt, await evaluate("window.__metrics[3].promptAt"), "the largest turn is the fourth");
  await evaluate("document.querySelector('#metrics-light .mode-toggle button:last-child').click()");
  await wait("document.querySelectorAll('#metrics-light .token-graph circle').length === 6");
  assert.deepEqual(failures, []);
  console.log("Token usage UI passed. Screenshots: " + output);
} finally {
  socket?.close();
  browser.kill();
  server.close();
}
