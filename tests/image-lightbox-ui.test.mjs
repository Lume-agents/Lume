// Browser test of the image viewer: a thumbnail opens the whole photo over a darkened window,
// and Escape, the backdrop and the close button dismiss it. Set LUME_TEST_BROWSER for Chromium.
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
const output = mkdtempSync(join(tmpdir(), "lume-image-lightbox-"));

await build({
  stdin: {
    contents: 'import { mount } from "svelte"; import Image from "./src/lib/AttachmentImage.svelte"; for (const [id, attachment] of Object.entries(window.__attachments)) mount(Image, { target: document.getElementById(id), props: { attachment, language: "pt-BR", width: 92, height: 68 } });',
    resolveDir: root,
  },
  outfile: join(output, "app.js"), bundle: true, format: "esm", platform: "browser", conditions: ["browser", "svelte"],
  plugins: [{
    name: "svelte-browser-fixture",
    setup(builder) {
      builder.onResolve({ filter: /^\$lib\// }, ({ path }) => ({ path: join(root, "src/lib", path.slice(5) + (extname(path) ? "" : ".ts")) }));
      builder.onLoad({ filter: /\.svelte$/ }, ({ path }) => ({ contents: compile(readFileSync(path, "utf8"), { filename: path, css: "injected" }).js.code, loader: "js", resolveDir: dirname(path) }));
    },
  }],
});

const pixel = (color) => `data:image/svg+xml;utf8,${encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" width="40" height="30"><rect width="40" height="30" fill="${color}"/></svg>`)}`;
function fixture() {
  window.__calls = [];
  window.__TAURI_INTERNALS__ = {
    transformCallback: () => 1,
    invoke: async (command, args) => {
      window.__calls.push([command, args.path]);
      if (command === "read_local_image_data_url") return "data:image/svg+xml;utf8," + encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="400" height="300"><rect width="400" height="300" fill="#3a7"/></svg>');
      return null;
    },
  };
  window.__attachments = {
    sent: { name: "image.jpg", previewDataUrl: "__PREVIEW__", path: "/cache/prompt-attachments/1791581411129-0-image.jpg" },
    recovered: { name: "1791581411129-0-image.jpg", previewDataUrl: "", path: "/cache/prompt-attachments/1791581411129-0-other.png" },
    file: { name: "report.xlsx", previewDataUrl: "", path: "/work/report.xlsx" },
  };
}

const server = createServer((request, response) => {
  const path = new URL(request.url, "http://localhost").pathname;
  if (path === "/") { response.setHeader("content-type", "text/html"); response.end('<html><body style="margin:0;background:#18211d;color:#dfe9e3"><div style="padding:30px;display:flex;gap:12px"><div id="sent"></div><div id="recovered"></div><div id="file"></div></div><script type="module" src="/app.js"></script></body></html>'); return; }
  const file = join(output, path.slice(1));
  if (!file.startsWith(output + sep) || !existsSync(file)) { response.writeHead(404).end(); return; }
  response.setHeader("content-type", { ".js": "text/javascript" }[extname(file)] || "application/octet-stream");
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
  await call("Emulation.setDeviceMetricsOverride", { width: 900, height: 600, deviceScaleFactor: 1, mobile: false });
  await call("Page.addScriptToEvaluateOnNewDocument", { source: "(" + fixture.toString().replace("__PREVIEW__", pixel("#c44")) + ")()" });
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
  await call("Page.navigate", { url: "http://127.0.0.1:" + server.address().port });
  await wait("document.querySelectorAll('.attachment-image').length === 2");
  assert.equal(await evaluate("document.querySelectorAll('.attachment-file').length"), 1, "a non-image stays a file chip");
  assert.ok(await evaluate("window.__calls.some(([command, path]) => command === 'read_local_image_data_url' && path.endsWith('other.png'))"), "a recovered image reads its thumbnail from the file");
  assert.equal(await evaluate("document.querySelector('.image-lightbox')"), null);

  await evaluate("document.querySelector('#sent .attachment-image').click()");
  await wait("document.body.querySelector(':scope > .image-lightbox img')");
  assert.equal(await evaluate("getComputedStyle(document.querySelector('.image-lightbox')).position"), "fixed");
  assert.match(await evaluate("getComputedStyle(document.querySelector('.image-lightbox')).backgroundColor"), /rgba\(4, 7, 6, 0\.8\d*\)/, "the window behind is darkened");
  await wait("document.querySelector('.image-lightbox img').src.includes('width%3D%22400%22')");
  assert.ok(await evaluate("window.__calls.some(([command, path]) => command === 'read_local_image_data_url' && path.endsWith('1791581411129-0-image.jpg'))"), "the whole photo is read from its file");

  await evaluate("document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))");
  await evaluate("document.querySelector('.image-lightbox').dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))");
  await wait("!document.querySelector('.image-lightbox')");

  await evaluate("document.querySelector('#sent .attachment-image').click()");
  await wait("document.querySelector('.image-lightbox')");
  await evaluate("document.querySelector('.image-lightbox').click()");
  await wait("!document.querySelector('.image-lightbox')");

  await evaluate("document.querySelector('#sent .attachment-image').click()");
  await wait("document.querySelector('.image-lightbox .close')");
  await evaluate("document.querySelector('.image-lightbox .close').click()");
  await wait("!document.querySelector('.image-lightbox')");
  assert.deepEqual(failures, []);
  console.log("Image lightbox UI passed.");
} finally {
  socket?.close();
  browser.kill();
  server.close();
}
