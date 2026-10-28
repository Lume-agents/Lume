// Browser test of the Remote Computers settings with native IPC stubbed: turning on this computer's access, its
// pairing link, devices and switches, and connecting to another computer through the Relay. Set LUME_TEST_BROWSER for Chromium.
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
  const me = window.__me = { lifecycle: "disabled", name: "Notebook", relayUrl: null, relayConnected: null, devices: [], shareConversations: false, allowPrompts: false, allowApprovals: false };
  window.__incoming = [{ requestId: "in-1", displayName: "Celular da Ana", clientFingerprint: "c", code: "120045", createdAt: now, expiresAt: now + 120000 }];
  window.__calls = [];
  window.__TAURI_INTERNALS__ = {
    transformCallback: () => 1,
    invoke: async (command, args) => {
      window.__calls.push([command, args]);
      if (command === "get_this_node") return { ...me };
      if (command === "set_this_node_on") {
        Object.assign(me, args.on ? { lifecycle: "running", relayUrl: "https://relay.lumeagents.dev", relayConnected: true, devices: [{ deviceId: "dev-1", name: "PC do trabalho", pairedAt: now, lastSeenAt: now - 120000 }] } : { lifecycle: "disabled", relayConnected: null });
        return { ...me };
      }
      if (command === "create_this_node_pairing") return { link: "lume://pair-relay?v=1&relay=https://relay.lumeagents.dev&node=n&offer=o&secret=s&fingerprint=f", expiresAt: now + 300000 };
      if (command === "revoke_this_node_device") { me.devices = me.devices.filter((device) => device.deviceId !== args.deviceId); return { ...me }; }
      if (command === "set_node_share_conversations") { me.shareConversations = args.enabled; return args.enabled; }
      if (command === "set_node_allow_prompts") { me.allowPrompts = args.enabled; return args.enabled; }
      if (command === "set_node_allow_approvals") { me.allowApprovals = args.enabled; return args.enabled; }
      if (command === "list_node_pair_requests") return me.lifecycle === "disabled" ? [] : window.__incoming.slice();
      if (command === "decide_node_pair_request") { window.__decision = args; window.__incoming = []; return true; }
      if (command === "list_remote_lume_nodes") return [];
      if (command === "list_relay_lume_nodes") return paired.slice();
      if (command === "pair_lume_relay_node") { paired.push(remote); return remote; }
      if (command === "forget_relay_lume_node") { paired.length = 0; return true; }
      if (command === "get_relay_lume_node_overview") {
        return { health: { lifecycle: "running", enabled: true, nodeId: "node-casa", displayName: "PC de casa" }, sessions: { generatedAt: now, appFound: true, live: true, sessions: [{ id: "a" }, { id: "b" }] } };
      }
      if (command === "discover_lume_nodes") return [];
      if (command.startsWith("plugin:")) return null;
      throw new Error("unexpected IPC " + command);
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
  await wait("document.querySelector('.remote-computers .block')");
  const body = () => evaluate("document.body.textContent");
  assert.match(await body(), /Este computador/);
  assert.match(await body(), /Desligado/);
  assert.doesNotMatch(await body(), /Desligado por padrão|Vale na hora/, "no explanatory paragraphs in the settings");
  // Turning on access for other computers.
  await evaluate("document.querySelector('.block .switch-row input').click()");
  await wait("/Online/.test(document.querySelector('.block .state').textContent)");
  assert.deepEqual(await evaluate("window.__calls.filter(([command]) => command === 'set_this_node_on').map(([, args]) => args.on)"), [true]);
  // A pairing link with its QR code.
  await evaluate("Array.from(document.querySelectorAll('button')).find((button) => /Link de pareamento/.test(button.textContent)).click()");
  await wait("document.querySelector('.pairing .qr img')");
  assert.match(await evaluate("document.querySelector('.pairing code').textContent"), /^lume:\/\/pair-relay/);
  // A click opens the QR code larger; Escape closes it.
  await evaluate("document.querySelector('.pairing .qr').click()");
  await wait("document.querySelector('.qr-zoom img')");
  assert.ok(await evaluate("document.querySelector('.qr-zoom img').getBoundingClientRect().width > 300"), "the large QR code is readable from a distance");
  await screenshot("qr-zoom");
  await evaluate("window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))");
  await wait("!document.querySelector('.qr-zoom')");
  // A device that asks to pair by code, then the owner's switches and the paired devices.
  await wait("document.querySelector('.request')");
  assert.match(await evaluate("document.querySelector('.request').textContent"), /Celular da Ana[\s\S]*120045/);
  await screenshot("this-computer");
  await evaluate("Array.from(document.querySelectorAll('.request button')).find((button) => /Permitir/.test(button.textContent)).click()");
  await wait("window.__decision");
  assert.deepEqual(await evaluate("window.__decision"), { requestId: "in-1", approve: true });
  await evaluate("Array.from(document.querySelectorAll('.switch-row.compact')).find((row) => /Enviar prompts/.test(row.textContent)).querySelector('input').click()");
  await wait("window.__me.allowPrompts === true");
  await evaluate("Array.from(document.querySelectorAll('.device button')).find((button) => /Revogar/.test(button.textContent)).click()");
  await wait("!document.querySelector('.device')");
  // Connecting to another computer with a link.
  await evaluate("const input = document.querySelector('.connect input'); input.value = 'lume://pair-relay?v=1&relay=x'; input.dispatchEvent(new Event('input', { bubbles: true }))");
  await evaluate("document.querySelector('.connect button').click()");
  await wait("document.querySelector('.computer') && /PC de casa/.test(document.querySelector('.computer').textContent)");
  assert.match(await evaluate("document.querySelector('.computer').textContent"), /Online · 2 sessões/);
  assert.ok(await evaluate("!window.__calls.some(([command]) => command === 'pair_lume_node')"), "a Relay link never goes through the LAN command");
  await screenshot("other-computers");
  await evaluate("document.querySelector('.computer button').click()");
  await wait("!document.querySelector('.computer')");
  assert.deepEqual(failures, []);
  console.log("Remote computers UI passed. Screenshots: " + output);
} finally {
  socket?.close();
  browser.kill();
  server.close();
}
