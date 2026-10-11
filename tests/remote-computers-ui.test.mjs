// Browser test of the Remote Computers settings with native IPC stubbed: pairing through the Relay
// and the live, read-only list of the other computer's agent sessions. Set LUME_TEST_BROWSER for Chromium.
import assert from "node:assert/strict";
import { build } from "esbuild";
import { compile } from "svelte/compiler";
import { readFileSync, existsSync, readdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { spawn } from "node:child_process";
import { join, dirname, extname, sep } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import { once } from "node:events";
import { executable, browserArguments } from "./support/browser.mjs";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const output = mkdtempSync(join(tmpdir(), "lume-remote-computers-ui-"));

await build({
  stdin: {
    contents: 'import "./src/app.css"; import { mount } from "svelte"; import Remote from "./src/lib/RemoteComputers.svelte"; mount(Remote, { target: document.getElementById("app"), props: { language: "pt-BR", dark: true } });',
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
  const now = Date.now();
  const remote = { nodeId: "node-casa", relayUrl: "https://relay.example", identity: { algorithm: "Ed25519", publicKey: "k", fingerprint: "f" }, deviceId: "d1", pairedAt: now };
  const paired = window.__paired = [];
  window.__phone = { running: "running" };
  window.__calls = [];
  window.__TAURI_INTERNALS__ = {
    transformCallback: () => 1,
    invoke: async (command, args) => {
      window.__calls.push([command, args]);
      if (command === "list_remote_lume_nodes") return [];
      if (command === "list_relay_lume_nodes") return paired.slice();
      if (command === "pair_lume_relay_node") { paired.push(remote); return remote; }
      if (command === "get_relay_lume_node_overview") {
        const session = (id, agent, status, extra = {}) => ({ id, agent, agentLabel: agent === "claude_code" ? "Claude Code" : "Codex", name: "", project: "Lume", status, statusLabel: status, startedAt: "1", updatedAt: now - 120000, needsPermission: false, needsAnswer: false, ...extra });
        return {
          health: { displayName: "PC de casa", machine: { operatingSystem: "linux", architecture: "x86_64" } },
          sessions: { generatedAt: now, appFound: true, sessions: [session("a", "codex", window.__phone.running), session("b", "claude_code", "permission_required", { name: "Corrigir login", needsPermission: true })] },
        };
      }
      if (command === "forget_relay_lume_node") { paired.length = 0; return true; }
      return [];
    },
  };
}

const server = createServer((request, response) => {
  const path = new URL(request.url, "http://localhost").pathname;
  if (path === "/") { response.setHeader("content-type", "text/html"); response.end('<html><head><link rel="stylesheet" href="/app.css"></head><body style="margin:0"><div id="app" style="width:420px;padding:16px;background:var(--lume-canvas-dark);color:var(--lume-ink-dark);font-family:var(--lume-font-ui, sans-serif)"></div><script type="module" src="/app.js"></script></body></html>'); return; }
  const file = join(output, path.slice(1));
  if (!file.startsWith(output + sep) || !existsSync(file)) { response.writeHead(404).end(); return; }
  response.setHeader("content-type", { ".css": "text/css", ".js": "text/javascript" }[extname(file)] || "application/octet-stream");
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
  await call("Emulation.setDeviceMetricsOverride", { width: 460, height: 700, deviceScaleFactor: 1, mobile: false });
  await call("Page.addScriptToEvaluateOnNewDocument", { source: "(" + fixture.toString() + ")()" });
  const evaluate = async (expression) => {
    const result = await call("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
    if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description || result.exceptionDetails.text);
    return result.result.value;
  };
  const delay = (time = 60) => new Promise((resolve) => setTimeout(resolve, time));
  async function screenshot(name) {
    await evaluate("document.fonts.ready");
    await delay(240);
    const { data } = await call("Page.captureScreenshot", { format: "png" });
    writeFileSync(join(output, name + ".png"), Buffer.from(data, "base64"));
  }
  async function wait(expression) {
    const deadline = Date.now() + 10000;
    while (Date.now() < deadline) { if (await evaluate(expression)) return; await delay(); }
    await screenshot("failure");
    throw new Error("Timed out: " + expression + "\n" + failures.join("\n") + "\nScreenshots: " + output);
  }

  await call("Page.navigate", { url: "http://127.0.0.1:" + server.address().port });
  await wait("document.querySelector('.pair-control input')");
  assert.match(await evaluate("document.body.textContent"), /Nenhum computador pareado/);
  await evaluate("const input = document.querySelector('.pair-control input'); input.value = 'lume://pair-relay?v=1&relay=x'; input.dispatchEvent(new Event('input', { bubbles: true }))");
  await wait("!document.querySelector('.pair-control button').disabled");
  await evaluate("document.querySelector('.pair-control button').click()");
  await wait("document.querySelectorAll('.session-row').length === 2");
  assert.ok(await evaluate("window.__calls.some(([command]) => command === 'pair_lume_relay_node')"), "a Relay link pairs through the Relay command");
  assert.ok(await evaluate("!window.__calls.some(([command]) => command === 'pair_lume_node')"), "and never through the LAN one");
  const text = await evaluate("document.querySelector('article').textContent");
  assert.match(text, /PC de casa/);
  assert.match(text, /Corrigir login/);
  assert.match(text, /Pede permissão/);
  assert.equal(await evaluate("document.querySelectorAll('.session-dot.attention').length"), 1);
  assert.equal(await evaluate("document.querySelectorAll('.session-dot.running').length"), 1);
  assert.equal(await evaluate("document.querySelectorAll('article button').length"), 1, "the Relay computer only offers Forget: no way to control it yet");
  await screenshot("remote-sessions");
  await evaluate("window.__phone.running = 'completed'");
  await evaluate("document.dispatchEvent(new Event('visibilitychange'))");
  await wait("document.querySelectorAll('.session-dot.idle').length === 1");
  await evaluate("Array.from(document.querySelectorAll('article button')).find((button) => /Esquecer/.test(button.textContent)).click()");
  await wait("document.querySelectorAll('article').length === 0");
  assert.deepEqual(failures, []);
  console.log("Remote computers UI passed. Screenshots: " + output);
} finally {
  socket?.close();
  browser.kill();
  server.close();
}
