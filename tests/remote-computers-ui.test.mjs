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
  try { localStorage.setItem("lume:tour:workspace:v1", "seen"); localStorage.setItem("lume:tour:board:v1", "seen"); localStorage.setItem("lume:tour:orb:v1", "seen"); } catch {}
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
      if (command === "get_node_share_conversations") return window.__share ?? false;
      if (command === "set_node_share_conversations") { window.__share = args.enabled; return args.enabled; }
      if (command === "get_relay_lume_session") {
        if (window.__denyChat) throw "the owner of that computer has not allowed reading its conversations";
        return { session: { id: args.sessionId, agent: "codex", agentLabel: "Codex", name: "", project: "Lume", status: "running", statusLabel: "Running", startedAt: "1", updatedAt: now, needsPermission: Boolean(window.__needsPermission), needsAnswer: false }, generatedAt: now, truncated: false,
          messages: [{ id: "u1", role: "user", text: "Corrija o total <b>do carrinho</b>", createdAt: 1 }, { id: "a1", role: "assistant", text: "Pronto, corrigi.", createdAt: 2 }] };
      }
      if (command === "get_relay_lume_permission") {
        if (window.__approvalsDenied) throw "the owner of that computer has not allowed approvals from other devices";
        return { permission: window.__permission ?? null };
      }
      if (command === "answer_relay_lume_permission") { window.__permission = null; window.__needsPermission = false; return { accepted: true }; }
      if (command === "interrupt_relay_lume_session") { window.__stopped = (window.__stopped ?? 0) + 1; return { accepted: true }; }
      if (command === "get_node_allow_approvals") return window.__approvals ?? false;
      if (command === "set_node_allow_approvals") { window.__approvals = args.enabled; return args.enabled; }
      if (command === "get_node_allow_prompts") return window.__prompts ?? false;
      if (command === "set_node_allow_prompts") { window.__prompts = args.enabled; return args.enabled; }
      if (command === "send_relay_lume_prompt") {
        if (window.__promptFails) { window.__promptFails -= 1; throw "the owner of that computer has not allowed prompts from other devices"; }
        return { accepted: true };
      }
      if (command === "forget_relay_lume_node") { paired.length = 0; return true; }
      if (command === "discover_lume_nodes") return [{ nodeId: "node-casa", address: "192.168.0.8", port: 4433, identityFingerprint: "f", certificateSha256: "c", protocolMinimum: 1, protocolMaximum: 1 }];
      if (command === "request_lume_node_pairing") return { requestId: "req-1", code: "482913", expiresAt: now + 120000 };
      if (command === "check_lume_node_pairing") {
        window.__checks = (window.__checks || 0) + 1;
        if (window.__checks < 2) return { status: "pending" };
        paired.push(remote);
        return { status: "approved", relay: remote };
      }
      if (command === "list_node_pair_requests") return window.__incoming || [];
      if (command === "decide_node_pair_request") { window.__decision = args; window.__incoming = []; return args.approve; }
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
  assert.equal(await evaluate("document.querySelectorAll('article button:not(.session-button)').length"), 1, "the Relay computer only offers Forget: no way to control it yet");
  await screenshot("remote-sessions");
  // Reading a conversation: plain text only, refused politely when the owner has not allowed it.
  await evaluate("window.__denyChat = true; document.querySelector('.session-button').click()");
  await wait("document.querySelector('.conversation .error')");
  assert.match(await evaluate("document.querySelector('.conversation .error').textContent"), /não liberou/);
  await evaluate("window.__denyChat = false; document.dispatchEvent(new Event('visibilitychange'))");
  await wait("document.querySelectorAll('.bubble').length === 2");
  assert.equal(await evaluate("document.querySelector('.bubble.user p').textContent"), "Corrija o total <b>do carrinho</b>");
  assert.equal(await evaluate("document.querySelector('.bubble p b')"), null, "remote text is never interpreted as HTML");
  await screenshot("remote-conversation");
  // Sending a prompt: refused politely, then delivered with the same request id the failed attempt used.
  await evaluate("window.__promptFails = 1; const box = document.querySelector('.prompt-form textarea'); box.value = 'Rode os testes'; box.dispatchEvent(new Event('input', { bubbles: true }))");
  await evaluate("document.querySelector('.prompt-form button').click()");
  await wait("document.querySelector('.prompt-note.error')");
  assert.match(await evaluate("document.querySelector('.prompt-note').textContent"), /não permitiu prompts/);
  assert.equal(await evaluate("document.querySelector('.prompt-form textarea').value"), "Rode os testes", "a refused prompt stays in the box");
  await evaluate("document.querySelector('.prompt-form button').click()");
  await wait("document.querySelector('.prompt-note') && !document.querySelector('.prompt-note.error')");
  const sent = await evaluate("window.__calls.filter(([command]) => command === 'send_relay_lume_prompt').map(([, args]) => args)");
  assert.equal(sent.length, 2);
  assert.equal(sent[0].requestId, sent[1].requestId, "a retry reuses the request id, so the Node runs it once");
  assert.equal(sent[1].prompt, "Rode os testes");
  assert.equal(await evaluate("document.querySelector('.prompt-form textarea').value"), "", "a delivered prompt clears the box");
  // A pending permission: refused politely without the owner's switch, then answered once.
  await evaluate("window.__needsPermission = true; window.__approvalsDenied = true; document.dispatchEvent(new Event('visibilitychange'))");
  await wait("document.querySelector('.prompt-note.error') && /aprovações/.test(document.querySelector('.prompt-note.error').textContent)");
  assert.equal(await evaluate("document.querySelector('.permission-card')"), null);
  await evaluate("window.__approvalsDenied = false; window.__permission = { id: 'perm-1', kind: 'command', summary: 'npm test', risk: 'low', requestedAt: 'now' }; document.dispatchEvent(new Event('visibilitychange'))");
  await wait("document.querySelector('.permission-card')");
  assert.match(await evaluate("document.querySelector('.permission-card p').textContent"), /npm test/);
  await screenshot("remote-permission");
  await evaluate("document.querySelector('.permission-actions .primary').click()");
  await wait("!document.querySelector('.permission-card')");
  const answered = await evaluate("window.__calls.filter(([command]) => command === 'answer_relay_lume_permission').map(([, args]) => args)");
  assert.deepEqual([answered.length, answered[0].permissionId, answered[0].action], [1, "perm-1", "allow_once"]);
  // A running session can be stopped from here.
  await evaluate("document.querySelector('.stop-session').click()");
  await wait("window.__stopped === 1");
  const stopped = await evaluate("window.__calls.filter(([command]) => command === 'interrupt_relay_lume_session').map(([, args]) => args)");
  assert.equal(stopped[0].sessionId, "a");
  assert.ok(stopped[0].requestId.length >= 8);
  await evaluate("Array.from(document.querySelectorAll('.conversation-head button')).find((button) => /Voltar/.test(button.textContent)).click()");
  await wait("document.querySelectorAll('.session-row').length === 2");
  // The owner's switch.
  await evaluate("document.querySelector('.share-switch input').click()");
  await wait("window.__share === true");
  await evaluate("window.__phone.running = 'completed'");
  await evaluate("document.dispatchEvent(new Event('visibilitychange'))");
  await wait("document.querySelectorAll('.session-dot.idle').length === 1");
  await evaluate("Array.from(document.querySelectorAll('article button')).find((button) => /Esquecer/.test(button.textContent)).click()");
  await wait("document.querySelectorAll('article').length === 0");
  // Pairing without a link: find the computer, compare the code, wait for the approval.
  await evaluate("Array.from(document.querySelectorAll('.heading button')).find((button) => /Buscar/.test(button.textContent)).click()");
  await wait("document.querySelector('.discovered button')");
  await evaluate("document.querySelector('.discovered button').click()");
  await wait("document.querySelector('.request-panel .code')");
  assert.equal(await evaluate("document.querySelector('.request-panel .code').textContent"), "482 913");
  assert.ok(await evaluate("window.__calls.some(([command]) => command === 'request_lume_node_pairing')"));
  await screenshot("remote-code");
  await wait("document.querySelectorAll('article').length === 1 && !document.querySelector('.request-panel')");
  assert.match(await evaluate("document.body.textContent"), /fora desta rede/, "the Relay access arrives with the approval");
  // The other way round: a device asks this computer, and the person approves after comparing.
  await evaluate("window.__incoming = [{ requestId: 'in-1', displayName: 'Celular da Ana', clientFingerprint: 'x', code: '120045', createdAt: 1, expiresAt: Date.now() + 100000 }]; document.dispatchEvent(new Event('visibilitychange'))");
  await wait("document.querySelector('.request-panel.attention .code')");
  assert.equal(await evaluate("document.querySelector('.request-panel.attention .code').textContent"), "120 045");
  await screenshot("incoming-request");
  await evaluate("Array.from(document.querySelectorAll('.request-panel.attention button')).find((button) => /Aprovar/.test(button.textContent)).click()");
  await wait("window.__decision && !document.querySelector('.request-panel.attention')");
  assert.deepEqual(await evaluate("window.__decision"), { requestId: "in-1", approve: true });
  assert.deepEqual(failures, []);
  console.log("Remote computers UI passed. Screenshots: " + output);
} finally {
  socket?.close();
  browser.kill();
  server.close();
}
