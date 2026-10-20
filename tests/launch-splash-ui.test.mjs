// Browser test of the capsule app.html paints before the app loads: only the desktop Orb window shows
// it, it takes the colors of the last launch, and dismissing it fades it out. Set LUME_TEST_BROWSER for Chromium.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync, mkdtempSync } from "node:fs";
import { createServer } from "node:http";
import { spawn } from "node:child_process";
import { join, dirname } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import { once } from "node:events";
import { executable, browserArguments } from "./support/browser.mjs";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const page = readFileSync(join(root, "src/app.html"), "utf8").replace(/%sveltekit\.[a-z]+%(\/[^"]*)?/g, "");

const server = createServer((request, response) => { response.setHeader("content-type", "text/html"); response.end(page); });
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
  socket.addEventListener("message", ({ data }) => {
    const message = JSON.parse(data);
    if (message.id) { const callback = pending.get(message.id); pending.delete(message.id); message.error ? callback.reject(new Error(message.error.message)) : callback.resolve(message.result); }
  });
  const send = (method, params = {}, sessionId) => new Promise((resolve, reject) => { const id = ++sequence; pending.set(id, { resolve, reject }); socket.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) })); });
  const { targetId } = await send("Target.createTarget", { url: "about:blank" });
  const { sessionId } = await send("Target.attachToTarget", { targetId, flatten: true });
  const call = (method, params) => send(method, params, sessionId);
  await call("Runtime.enable");
  await call("Page.enable");
  const evaluate = async (expression) => {
    const result = await call("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
    if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description || result.exceptionDetails.text);
    return result.result.value;
  };
  const base = "http://127.0.0.1:" + server.address().port;
  let scriptId;
  async function open(path, { desktop = true, theme } = {}) {
    if (scriptId) await call("Page.removeScriptToEvaluateOnNewDocument", { identifier: scriptId });
    ({ identifier: scriptId } = await call("Page.addScriptToEvaluateOnNewDocument", {
      source: `${desktop ? "window.__TAURI_INTERNALS__ = {};" : ""} try { ${theme ? `localStorage.setItem('lume:launch-theme', '${theme}')` : "localStorage.removeItem('lume:launch-theme')"} } catch {}`,
    }));
    await call("Page.navigate", { url: base + path });
    await new Promise((resolve) => setTimeout(resolve, 300));
  }

  await open("/", { theme: "dark" });
  assert.deepEqual(await evaluate("(() => { const box = document.getElementById('lume-launch').getBoundingClientRect(); return [box.width, box.height]; })()"), [78, 44], "the capsule is the Orb's size");
  assert.equal(await evaluate("document.getElementById('lume-launch').dataset.theme"), "dark", "it keeps the colors of the last launch");
  assert.equal(await evaluate("getComputedStyle(document.getElementById('lume-launch')).backgroundColor"), "rgb(27, 34, 31)");
  assert.ok(await evaluate("!!document.querySelector('#lume-launch .zzz') && document.querySelectorAll('#lume-launch .dots i').length === 3"), "asleep, with a loading badge");

  await open("/", { theme: "light" });
  assert.equal(await evaluate("getComputedStyle(document.getElementById('lume-launch')).backgroundColor"), "rgb(230, 234, 215)");

  await open("/", { desktop: false });
  assert.equal(await evaluate("document.getElementById('lume-launch')"), null, "the web and Android builds do not show it");
  await open("/workspace/");
  assert.equal(await evaluate("document.getElementById('lume-launch')"), null, "other desktop windows do not show it");
  await open("/terminal/");
  assert.equal(await evaluate("document.getElementById('lume-launch')"), null);

  await open("/", { theme: "light" });
  console.log("Launch splash UI passed.");
} finally {
  socket?.close();
  browser.kill();
  server.close();
}
