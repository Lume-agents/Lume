// Exercise the real Workspace pane with simulated native IPC; no CLI or agent is started.
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
const output = mkdtempSync(join(tmpdir(), "lume-codex-settings-ui-"));
await build({
  stdin: { contents: 'import "./src/app.css"; import { mount } from "svelte"; import Pane from "./src/lib/WorkspaceSessionPane.svelte"; import Dialog from "./src/lib/CodexTakeoverDialog.svelte"; const isDialog = new URLSearchParams(location.search).has("dialog"); mount(isDialog ? Dialog : Pane, { target: document.getElementById("app"), props: { session: window.__session, language: "pt-BR", ...(isDialog ? { onClose: () => window.__closed = true, onTaken: () => window.__taken = true } : {}) } });', resolveDir: root },
  external: ["/fonts/*"], define: { "import.meta.env": '{"DEV":true}' }, outfile: join(output, "app.js"), bundle: true, format: "esm", platform: "browser", conditions: ["browser", "svelte"],
  loader: { ".woff2": "file", ".woff": "file", ".png": "file", ".svg": "file" },
  plugins: [{ name: "svelte-fixture", setup(builder) {
    builder.onResolve({ filter: /^\$lib\// }, ({ path }) => ({ path: join(root, "src/lib", path.slice(5) + (extname(path) ? "" : ".ts")) }));
    builder.onLoad({ filter: /\.svelte$/ }, ({ path }) => ({ contents: compile(readFileSync(path, "utf8"), { filename: path, css: "injected" }).js.code, loader: "js", resolveDir: dirname(path) }));
  } }],
});

function fixture() {
  window.__calls = [];
  const scenario = new URLSearchParams(location.search);
  const owned = scenario.has("owned");
  const unidentified = scenario.has("unidentified");
  window.__session = {
    id: "codex-cli", nativeSessionId: "thread", processId: 123, sessionName: "CLI", agent: "codex", agentLabel: "Codex", project: "Lume", workingDirectory: "/fixture",
    source: "cli", controlOrigin: owned ? "lume" : "external", status: scenario.has("running") ? "running" : "waiting_for_input", statusLabel: "Esperando ação", startedAt: new Date().toISOString(), updatedAt: Date.now(),
    results: [], activities: [], activityTotal: 0, permissionProfile: { canRespondFromLume: true, mode: "workspace_write" }, workSummary: { files: [], checks: [], todos: [], sources: [] },
    capabilities: { canPrompt: owned, canTakeControl: !owned && !unidentified, canAttachImages: true, promptDeliveries: ["new_turn"], canReadResults: false },
  };
  window.__TAURI_INTERNALS__ = {
    transformCallback: () => 1, unregisterCallback: () => {},
    invoke: async (command, args = {}) => {
      window.__calls.push([command, args]);
      if (command === "get_session_permission_mode") return { mode: "workspace_write", modes: ["workspace_write", "full_access"] };
      if (command === "get_session_environments") return { environments: [], error: null };
      if (command === "get_session_model_settings") return { model: "gpt-test", reasoningEffort: "medium", serviceTier: "default", models: [{ model: "gpt-test", displayName: "Modelo de teste", defaultReasoningEffort: "medium", supportedReasoningEfforts: [{ value: "medium", description: "Medium" }] }] };
      if (command === "get_session_fast_mode") return false;
      if (command === "plugin:event|listen") return 1;
      return command === "take_control_session" ? null : [];
    },
  };
}

const server = createServer((request, response) => {
  const path = new URL(request.url, "http://localhost").pathname;
  if (path === "/") return response.end('<html><body><div id="app" style="height:680px;width:820px"></div><script type="module" src="/app.js"></script></body></html>');
  const file = join(output, path.slice(1));
  if (!file.startsWith(output + sep) || !existsSync(file)) return response.writeHead(404).end();
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
    const timer = setTimeout(() => reject(new Error("Chromium startup timeout: " + log)), 15000);
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
  await call("Page.addScriptToEvaluateOnNewDocument", { source: "(" + fixture.toString() + ")()" });
  const evaluate = async (expression) => {
    const result = await call("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
    if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description || result.exceptionDetails.text);
    return result.result.value;
  };
  const delay = () => new Promise((resolve) => setTimeout(resolve, 60));
  async function wait(expression) {
    const deadline = Date.now() + 10000;
    while (Date.now() < deadline) { if (await evaluate(expression)) return; await delay(); }
    throw new Error("Timed out: " + expression + "\n" + failures.join("\n"));
  }
  await call("Page.navigate", { url: "http://127.0.0.1:" + server.address().port });
  await wait("document.querySelector('.model-trigger') && document.querySelector('textarea')");
  await evaluate("(() => { const input = document.querySelector('textarea'); input.value = 'Rascunho preservado'; input.dispatchEvent(new Event('input', { bubbles: true })); })()");
  await evaluate("document.querySelector('.model-trigger').click()");
  await wait("document.querySelector('.takeover-dialog')");
  assert.match(await evaluate("document.querySelector('.takeover-dialog').textContent"), /rascunho será preservado/i);
  await evaluate("document.querySelector('.takeover-dialog footer button').click()");
  await wait("!document.querySelector('.takeover-dialog')");
  assert.equal(await evaluate("window.__calls.filter(([command]) => command === 'take_control_session').length"), 0, "cancel does not transfer the CLI");
  await evaluate("document.querySelector('.model-trigger').click()");
  await wait("document.querySelector('.takeover-primary')");
  await evaluate("document.querySelector('.takeover-primary').click()");
  await wait("!document.querySelector('.takeover-dialog')");
  assert.equal(await evaluate("document.querySelector('textarea').value"), "Rascunho preservado");
  assert.deepEqual(await evaluate("window.__calls.filter(([command]) => command === 'take_control_session').map(([, args]) => args)"), [{ sessionId: "codex-cli", prompt: "", attachments: [], force: false }]);
  assert.equal(await evaluate("window.__calls.filter(([command]) => /^set_session_(model|fast|permission|collaboration)/.test(command)).length"), 0, "settings are not changed before ownership transfers");
  await wait("document.querySelector('.permission-trigger')");
  await evaluate("document.querySelector('.permission-trigger').click()");
  await wait("document.querySelector('.takeover-dialog')");
  await evaluate("document.querySelector('.takeover-dialog footer button').click()");
  await wait("!document.querySelector('.takeover-dialog')");
  await evaluate("(() => { const input = document.querySelector('textarea'); input.value = '/plan'; input.dispatchEvent(new Event('input', { bubbles: true })); document.querySelector('form.composer').requestSubmit(); })()");
  await wait("document.querySelector('.takeover-dialog')");
  assert.equal(await evaluate("document.querySelector('textarea').value"), "/plan", "a local settings command stays in the draft");
  assert.equal(await evaluate("window.__calls.filter(([command]) => /^set_session_(model|fast|permission|collaboration)/.test(command)).length"), 0);

  await call("Page.navigate", { url: "http://127.0.0.1:" + server.address().port + "/?unidentified" });
  await wait("document.querySelector('.model-trigger')");
  await evaluate("document.querySelector('.model-trigger').click()");
  await wait("document.querySelector('.takeover-dialog')");
  assert.equal(await evaluate("document.querySelector('.takeover-primary').disabled"), true);
  assert.match(await evaluate("document.querySelector('.takeover-dialog').textContent"), /não pode ser assumida automaticamente/i);

  await call("Page.navigate", { url: "http://127.0.0.1:" + server.address().port + "/?running" });
  await wait("document.querySelector('.model-trigger')");
  await evaluate("document.querySelector('.model-trigger').click()");
  await wait("document.querySelector('.takeover-dialog')");
  assert.match(await evaluate("document.querySelector('.takeover-dialog').textContent"), /interromper a tarefa atual/i);

  await call("Page.navigate", { url: "http://127.0.0.1:" + server.address().port + "/?owned" });
  await wait("document.querySelector('.model-trigger')");
  await evaluate("document.querySelector('.model-trigger').click()");
  await wait("document.querySelector('.agent-controls-popover')");
  assert.equal(await evaluate("!!document.querySelector('.takeover-dialog')"), false, "sessions already controlled by Lume open their settings normally");

  await call("Page.navigate", { url: "http://127.0.0.1:" + server.address().port + "/?dialog&running" });
  await wait("document.querySelector('[role=alertdialog]')");
  assert.match(await evaluate("document.querySelector('[role=alertdialog]').textContent"), /interromper a tarefa atual/i);
  assert.equal(await evaluate("document.activeElement.textContent.trim()"), "Assumir controle", "Enter can confirm takeover");
  const { data } = await call("Page.captureScreenshot", { format: "png" });
  writeFileSync(join(output, "takeover-dialog.png"), Buffer.from(data, "base64"));
  assert.equal(await evaluate("window.__calls.filter(([command]) => command === 'take_control_session').length"), 0);
  await evaluate("document.activeElement.click()");
  await wait("window.__taken === true");
  assert.deepEqual(await evaluate("window.__calls.filter(([command]) => command === 'take_control_session').map(([, args]) => args)"), [{ sessionId: "codex-cli", prompt: "", attachments: [], force: false }]);
  assert.deepEqual(failures, []);
  console.log("Codex CLI settings takeover UI passed. Screenshot: " + output);
} finally {
  socket?.close();
  browser.kill();
  server.close();
}
