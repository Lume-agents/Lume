// Browser smoke test of the real Workspace/Board, with native IPC and chat contents stubbed.
// No provider prompt is sent. Run with LUME_TEST_BROWSER pointing to Chromium if needed.
import assert from "node:assert/strict";
import { build } from "esbuild";
import { compile } from "svelte/compiler";
import { readFileSync, existsSync, readdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { spawn } from "node:child_process";
import { join, dirname, extname } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import { once } from "node:events";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const output = mkdtempSync(join(tmpdir(), "lume-workflow-board-ui-"));
const cache = join(process.env.HOME, ".cache/puppeteer/chrome-headless-shell");
const cached = existsSync(cache) ? readdirSync(cache).sort().reverse().map((version) => join(cache, version, "chrome-headless-shell-linux64/chrome-headless-shell")).find(existsSync) : undefined;
const executable = process.env.LUME_TEST_BROWSER || cached;
assert.ok(executable, "Set LUME_TEST_BROWSER to a Chromium executable");
await build({
  stdin: {
    contents: 'import "./src/app.css"; import { mount } from "svelte"; import Workspace from "./src/lib/WorkspaceWindow.svelte"; import { defaultPreferences } from "./src/lib/lume"; window.__fixtureDefaults = defaultPreferences; window.__workspace = mount(Workspace, { target: document.getElementById("app") });',
    resolveDir: root,
  },
  outfile: join(output, "app.js"), bundle: true, format: "esm", platform: "browser", conditions: ["browser", "svelte"],
  loader: { ".woff2": "file", ".woff": "file", ".png": "file", ".svg": "file" },
  plugins: [{
    name: "svelte-browser-fixture",
    setup(builder) {
      builder.onResolve({ filter: /^\$lib\// }, ({ path }) => ({ path: join(root, "src/lib", path.slice(5) + (extname(path) ? "" : ".ts")) }));
      builder.onLoad({ filter: /\.svelte$/ }, ({ path }) => {
        let source = readFileSync(path, "utf8");
        if (path.endsWith("WorkspaceSessionPane.svelte")) source = '<script>let { session } = $props();</script><section data-workspace-pane={session.id}><input aria-label="Preserved chat draft" /></section>';
        if (path.endsWith("WorkspaceInspector.svelte") || path.endsWith("WorkspaceReviewCenter.svelte")) source = '<div>Test inspector</div>';
        return { contents: compile(source, { filename: path, css: "injected" }).js.code, loader: "js", resolveDir: dirname(path) };
      });
    },
  }],
});

function nativeFixture() {
  const callbacks = new Map();
  const listeners = new Map();
  let nextId = 1;
  const contract = { instruction: "Trabalhe no papel definido.", expectedInput: "Objetivo e contexto.", producedOutput: "Resultado para a próxima etapa.", completionCondition: "Entregar o resultado revisado." };
  const sessions = ["Pesquisa", "Implementação", "Revisão"].map((sessionName, index) => ({
    id: "session-" + index, nativeSessionId: "native-" + index, sessionName, agent: ["codex", "claude_code", "opencode"][index],
    agentLabel: ["Codex", "Claude Code", "OpenCode"][index], project: "Lume · teste", workingDirectory: "/fixture/lume",
    source: "desktop", controlOrigin: "lume", status: "waiting_for_input", statusLabel: "Esperando ação",
    startedAt: new Date().toISOString(), updatedAt: Date.now(), results: [], activities: [], activityTotal: 0,
    permissionProfile: { canRespondFromLume: true, mode: "full_access" },
    capabilities: { canPrompt: true, canReadResults: true, canTakeControl: false, promptDeliveries: ["new_turn"] },
    workSummary: { files: [], checks: [], todos: [], sources: [] },
  }));
  const steps = sessions.slice(0, 2).map((session, index) => ({ id: "step-" + session.nativeSessionId, sessionNativeId: session.nativeSessionId, role: index ? "implementer" : "planner", customRoleLabel: "", attempt: 0, ...contract }));
  const group = { id: "orb-fixture", terminalGroupId: "orb-fixture-group", steps, connections: [{ id: "connection-fixture", fromStepId: steps[0].id, toStepId: steps[1].id, includeResponse: true, includeFiles: true, includeTests: true, contextPolicy: "standard", contextSelection: { response: true, files: true, checks: true, plan: false, activity: false, diffs: false }, requiresApproval: true, advanceMode: "manual", additionalInstruction: "" }] };
  const fake = window.__fixture = { sessions, runs: {}, starts: 0, saved: 0, calls: [], preferences: null };
  fake.emit = (event, payload) => {
    for (const [id, listener] of listeners) if (listener.event === event) callbacks.get(listener.handler)?.({ event, id, payload: structuredClone(payload) });
  };
  fake.status = (status, patch = {}) => {
    const current = fake.runs["orb-fixture"];
    const next = { ...current, status, updatedAt: Math.max(Date.now(), current.updatedAt + 1), ...(status === "completed" ? { steps: current.steps.map(step => ({ ...step, status: "completed" })) } : {}), ...patch };
    fake.runs[next.workflowId] = next;
    fake.emit("lume://workflow-run-changed", next);
    return structuredClone(next);
  };
  localStorage.setItem("lume:workflow-board:open", "true");
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: (_, id) => listeners.delete(id) };
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "workspace" }, currentWebview: { label: "workspace" } },
    transformCallback(callback) { const id = nextId++; callbacks.set(id, callback); return id; },
    unregisterCallback(id) { callbacks.delete(id); },
    async invoke(command, args = {}) {
      // Match Tauri's JSON boundary, including proxied Svelte state in event payloads.
      args = JSON.parse(JSON.stringify(args));
      fake.calls.push(command);
      if (command === "plugin:event|listen") { const id = nextId++; listeners.set(id, args); return id; }
      if (command === "plugin:event|unlisten") { listeners.delete(args.eventId); return; }
      if (command === "plugin:event|emit") { fake.emit(args.event, args.payload); return; }
      if (command === "get_preferences") {
        fake.preferences ??= JSON.parse(localStorage.getItem("fixture:preferences") || "null") || { ...structuredClone(window.__fixtureDefaults), language: "pt-BR", darkMode: true, workflowGroups: [group] };
        return structuredClone(fake.preferences);
      }
      if (command === "set_preferences") { fake.preferences = args.preferences; localStorage.setItem("fixture:preferences", JSON.stringify(fake.preferences)); fake.saved++; return; }
      if (command === "get_hub_snapshot") return { protocolVersion: 1, desktopVersion: "0.15.3", generatedAt: Date.now(), features: [], sessions, internalServices: [], workflowGroups: [group], workflowHistory: [] };
      if (command === "get_workflow_role_contract") return { ...contract, instruction: "Papel: " + args.role };
      if (command === "get_workflow_run") return structuredClone(fake.runs[args.workflowId] ?? null);
      if (command === "start_workflow_run") {
        assertSaved();
        fake.starts++;
        const now = Date.now();
        const run = { id: "run-" + fake.starts, workflowId: args.group.id, objective: args.objective, status: "running", currentStepId: args.group.steps[0].id, steps: args.group.steps.map((step, index) => ({ stepId: step.id, status: index ? "pending" : "running", attempt: 1 })), handoffApproved: false, recovering: false, transitionCount: 0, createdAt: now, updatedAt: now };
        fake.runs[run.workflowId] = run;
        return structuredClone(run);
        function assertSaved() {
          if (!fake.preferences.workflowGroups.some((saved) => JSON.stringify(saved) === JSON.stringify(args.group))) throw new Error("Execution began before saving the definition");
        }
      }
      const actions = { pause_workflow_run: "paused", resume_workflow_run: "running", approve_workflow_handoff: "ready", advance_workflow_run: "running", retry_workflow_step: "running", skip_workflow_step: "ready", cancel_workflow_run: "cancelled" };
      if (actions[command]) return fake.status(actions[command]);
      if (command === "preview_workflow_context") return { markdown: "# Contexto do teste\n\nResposta e arquivos selecionados.", estimatedTokens: 42, files: [], checks: [], redactions: [] };
      if (command.includes("version")) return "0.15.3";
      if (command.includes("scale_factor")) return 1;
      if (command.includes("position")) return { x: 0, y: 0 };
      if (command.includes("is_maximized")) return false;
      if (command === "get_shortcut_registration_error") return null;
      return [];
    },
  };
}

const server = createServer((request, response) => {
  const path = new URL(request.url, "http://localhost").pathname;
  if (path === "/") {
    response.setHeader("content-type", "text/html");
    response.end('<html><head><link rel="stylesheet" href="/app.css"></head><body><div id="app"></div><script type="module" src="/app.js"></script></body></html>');
  } else {
    const bundled = join(output, path.slice(1));
    const publicAsset = join(root, "static", path.slice(1));
    const file = bundled.startsWith(output + "/") && existsSync(bundled) ? bundled : publicAsset;
    if ((!file.startsWith(output + "/") && !file.startsWith(join(root, "static") + "/")) || !existsSync(file)) { response.writeHead(404).end(); return; }
    const mime = { ".css": "text/css", ".js": "text/javascript", ".png": "image/png", ".svg": "image/svg+xml", ".woff2": "font/woff2", ".woff": "font/woff" };
    response.setHeader("content-type", mime[extname(file)] || "application/octet-stream");
    response.end(readFileSync(file));
  }
});
server.listen(0, "127.0.0.1");
await once(server, "listening");
const browser = spawn(executable, ["--no-sandbox", "--disable-dev-shm-usage", "--remote-debugging-port=0", "about:blank"], { stdio: ["ignore", "ignore", "pipe"] });
let socket;
try {
  const endpoint = await new Promise((resolve, reject) => {
    let log = "";
    const timer = setTimeout(() => reject(new Error("Chromium startup timeout: " + log.slice(-1000))), 15000);
    browser.stderr.on("data", (chunk) => { log += chunk; const match = log.match(/DevTools listening on (ws:\/\/\S+)/); if (match) { clearTimeout(timer); resolve(match[1]); } });
    browser.once("exit", (code) => { clearTimeout(timer); reject(new Error("Chromium exited: " + code + " " + log.slice(-1000))); });
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
  function send(method, params = {}, sessionId) {
    const id = ++sequence;
    return new Promise((resolve, reject) => { pending.set(id, { resolve, reject }); socket.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) })); });
  }
  const { targetId } = await send("Target.createTarget", { url: "about:blank" });
  const { sessionId } = await send("Target.attachToTarget", { targetId, flatten: true });
  const call = (method, params) => send(method, params, sessionId);
  await call("Runtime.enable");
  await call("Page.enable");
  await call("Emulation.setDeviceMetricsOverride", { width: 1280, height: 800, deviceScaleFactor: 1, mobile: false });
  await call("Page.addScriptToEvaluateOnNewDocument", { source: "(" + nativeFixture.toString() + ")()" });
  async function evaluate(expression) {
    const result = await call("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
    if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description || result.exceptionDetails.text);
    return result.result.value;
  }
  const delay = (time = 60) => new Promise((resolve) => setTimeout(resolve, time));
  async function keypress(key, code, windowsVirtualKeyCode) {
    await call("Input.dispatchKeyEvent", { type: "keyDown", key, code, windowsVirtualKeyCode });
    await call("Input.dispatchKeyEvent", { type: "keyUp", key, code, windowsVirtualKeyCode });
    await delay();
  }
  async function checkSemanticContrast(theme) {
    const colors = await evaluate("(() => { const board = getComputedStyle(document.querySelector('.workflow-board')); return { danger: board.getPropertyValue('--board-danger').trim(), warning: board.getPropertyValue('--board-warning').trim(), background: getComputedStyle(document.querySelector('.board-panel')).backgroundColor }; })()");
    const luminance = (color) => {
      const channels = color.startsWith("#") ? color.slice(1).match(/../g).map(value => parseInt(value, 16)) : color.match(/[\d.]+/g).slice(0, 3).map(Number);
      const scale = color.startsWith("color(srgb ") ? 1 : 255;
      const linear = channels.map(value => { const c = value / scale; return c <= .04045 ? c / 12.92 : ((c + .055) / 1.055) ** 2.4; });
      return linear[0] * .2126 + linear[1] * .7152 + linear[2] * .0722;
    };
    for (const tone of ["danger", "warning"]) {
      const a = luminance(colors[tone]), b = luminance(colors.background);
      assert.ok((Math.max(a, b) + .05) / (Math.min(a, b) + .05) >= 4.5, theme + " " + tone + " foreground contrast: " + JSON.stringify(colors));
    }
  }
  async function wait(expression) {
    const deadline = Date.now() + 12000;
    while (Date.now() < deadline) { if (await evaluate(expression)) return; await delay(); }
    await screenshot("failure");
    const state = await evaluate("({ groups: window.__fixture.preferences?.workflowGroups, cards: Array.from(document.querySelectorAll('.board-card')).map(card => ({ id: card.dataset.boardCard, left: card.style.left, top: card.style.top })), canvas: document.querySelector('.board-viewport')?.className, banners: document.querySelector('.system-banner-stack')?.textContent, trace: window.__dragTrace })");
    throw new Error("Timed out: " + expression + "\n" + JSON.stringify(state) + "\n" + failures.join("\n") + "\nScreenshot: " + output);
  }
  const click = async (selector) => { await evaluate("document.querySelector(" + JSON.stringify(selector) + ").click()"); await delay(); };
  async function pointerDrag(source, target, dx = 0, dy = 0) {
    const point = (selector) => evaluate("(() => { const r = document.querySelector(" + JSON.stringify(selector) + ").getBoundingClientRect(); return { x: r.x + r.width/2, y: r.y + r.height/2 }; })()");
    const from = await point(source);
    const to = target ? await point(target) : { x: from.x + dx, y: from.y + dy };
    await evaluate("window.__dragTrace = { from: " + JSON.stringify(from) + ", to: " + JSON.stringify(to) + ", hit: document.elementFromPoint(" + from.x + "," + from.y + ")?.outerHTML.slice(0,400), events: [] }; if (!window.__traceInstalled) { window.__traceInstalled = true; for (const type of ['pointerdown', 'pointermove', 'pointerup', 'gotpointercapture', 'lostpointercapture']) document.addEventListener(type, event => window.__dragTrace?.events.push({ type, x: event.clientX, y: event.clientY, target: event.target.className?.baseVal ?? event.target.className, canvas: document.querySelector('.board-viewport')?.className }), true); }");
    await call("Input.dispatchMouseEvent", { type: "mousePressed", ...from, button: "left", buttons: 1, clickCount: 1 });
    for (let step = 1; step <= 5; step++) {
      await call("Input.dispatchMouseEvent", { type: "mouseMoved", x: from.x + (to.x - from.x) * step / 5, y: from.y + (to.y - from.y) * step / 5, button: "left", buttons: 1 });
      await delay(20);
    }
    await call("Input.dispatchMouseEvent", { type: "mouseReleased", ...to, button: "left", buttons: 0, clickCount: 1 });
    await delay(120);
  }
  async function screenshot(name) {
    await evaluate("document.fonts.ready");
    await delay(240); // Settle the panel's entrance/exit before judging the render.
    const { data } = await call("Page.captureScreenshot", { format: "png" });
    writeFileSync(join(output, name + ".png"), Buffer.from(data, "base64"));
  }
  await call("Page.navigate", { url: "http://127.0.0.1:" + server.address().port });
  await wait("document.querySelectorAll('.board-card').length === 2");
  await click('[aria-label="Aumentar zoom"]');
  assert.equal(await evaluate("document.querySelector('.zoom-label').textContent"), "120%");
  await click('[aria-label="Restaurar zoom"]');
  assert.equal(await evaluate("document.querySelector('.zoom-label').textContent"), "100%");
  assert.equal(await evaluate("document.querySelector('.board-chat-layer').inert"), true);
  await evaluate("document.querySelector('[aria-label=\"Preserved chat draft\"]').value = 'rascunho preservado'");
  await evaluate("(() => { const target = document.querySelector('.board-viewport'); const r = target.getBoundingClientRect(); const dataTransfer = new DataTransfer(); dataTransfer.setData('text/x-lume-session', 'session-2'); target.dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, dataTransfer, clientX: r.left + 820, clientY: r.top + 270 })); })()");
  await wait("document.querySelectorAll('.board-card').length === 3");
  await click('[aria-label="Fechar painel"]');
  await pointerDrag('[data-board-card="step-native-1"] .port-out', '[data-board-card="step-native-2"] .card-body');
  await wait("window.__fixture.preferences.workflowGroups[0].connections.length === 2");
  await pointerDrag('[data-board-card="step-native-2"] .port-out', '[data-board-card="step-native-0"] .card-body');
  assert.equal(await evaluate("window.__fixture.preferences.workflowGroups[0].connections.length"), 2, "cycle is refused");
  await wait("document.querySelector('.system-banner-stack').textContent.includes('laço')");
  await pointerDrag('[data-board-card="step-native-0"] .card-body', null, 37, 23);
  assert.equal(await evaluate("parseFloat(document.querySelector('[data-board-card=\"step-native-0\"]').style.left)"), 133, "cards do not snap to a grid");
  await click(".settings-button");
  await wait("document.querySelector('.workspace-settings')");
  await evaluate("document.querySelector('[aria-label=\"Fechar ajustes\"]').focus()");
  await keypress("Delete", "Delete", 46);
  assert.equal(await evaluate("window.__fixture.preferences.workflowGroups[0].steps.length"), 3, "settings keys do not delete the underlying workflow selection");
  await keypress("Escape", "Escape", 27);
  await wait("document.querySelector('.settings-button').getAttribute('aria-expanded') === 'false'");
  assert.equal(await evaluate("document.querySelector('.workflow-board').hidden"), false, "settings Escape does not close the board");
  assert.equal(await evaluate("document.querySelector('.board-card.selected')?.dataset.boardCard"), "step-native-0", "settings Escape does not clear the board selection");
  await checkSemanticContrast("dark");
  await click('[aria-label="Fechar painel"]');
  await click(".edge-policy");
  await evaluate("(() => { const field = document.querySelector('[aria-label=\"Objetivo do workflow\"]'); field.value = 'Teste de encadeamento'; field.dispatchEvent(new Event('input', { bubbles: true })); })()");
  await click(".preview-button");
  await wait("document.querySelector('.context-preview')?.textContent.includes('42')");
  await screenshot("desktop-dark");
  await click(".board-run .primary");
  await wait("document.querySelector('.board-run').dataset.status === 'running'");
  await click(".board-run .run-actions button:not(.primary):not(.danger)");
  await wait("document.querySelector('.board-run').dataset.status === 'paused'");
  await click(".board-run .primary");
  await wait("document.querySelector('.board-run').dataset.status === 'running'");
  await evaluate("window.__fixture.status('waiting_for_approval', { pendingConnectionId: 'connection-fixture' })");
  await wait("document.querySelector('.edge-policy.waiting')");
  await click(".board-run .primary");
  await wait("document.querySelector('.board-run').dataset.status === 'ready'");
  await click(".board-run .primary");
  await wait("document.querySelector('.board-run').dataset.status === 'running'");
  await evaluate("window.__fixture.status('failed', { error: 'Falha simulada' })");
  await click(".board-run .primary");
  await wait("document.querySelector('.board-run').dataset.status === 'running'");
  await evaluate("window.__fixture.status('failed', { error: 'Etapa indisponível' })");
  await evaluate("Array.from(document.querySelectorAll('.run-actions button')).find(button => button.textContent.includes('Ignorar')).click()");
  await wait("document.querySelector('.board-run').dataset.status === 'ready'");
  await evaluate("window.__fixture.status('ready', { recovering: true })");
  await wait("document.querySelector('.board-run .primary').disabled");
  await evaluate("window.__fixture.status('ready', { recovering: false })");
  await click(".board-run .danger");
  await wait("document.querySelector('.board-run').dataset.status === 'cancelled'");
  await click(".board-run .primary");
  await wait("window.__fixture.starts === 2");
  await evaluate("window.__fixture.status('completed', { error: undefined })");
  await wait("document.querySelector('.board-run').dataset.status === 'completed'");
  await evaluate("(() => { const r = window.__fixture.runs['orb-fixture']; window.__fixture.emit('lume://workflow-run-changed', { ...r, status: 'running', updatedAt: r.updatedAt - 1 }); })()");
  assert.equal(await evaluate("document.querySelector('.board-run').dataset.status"), "completed", "stale events do not rewind the run");
  await click('[data-board-card="step-native-0"] .card-body');
  await evaluate("Array.from(document.querySelectorAll('.panel-actions button')).find(button => button.textContent.includes('Abrir chat')).click()");
  await wait("document.querySelector('.workflow-board').hidden");
  assert.equal(await evaluate("document.querySelector('[aria-label=\"Preserved chat draft\"]').value"), "rascunho preservado", "returning to chats retains drafts");
  await click('[aria-label="Board de workflow"]');
  await wait("!document.querySelector('.workflow-board').hidden");
  await evaluate("(async () => { const p = window.__fixture.preferences; p.darkMode = false; await window.__TAURI_INTERNALS__.invoke('set_preferences', { preferences: p }); window.__fixture.emit('lume://preferences-changed', p); })()");
  await delay(240);
  await screenshot("desktop-light");
  await checkSemanticContrast("light");
  assert.equal(await evaluate("document.documentElement.scrollWidth <= innerWidth"), true, "board does not overflow the window");
  await call("Page.reload");
  await wait("document.querySelectorAll('.board-card').length === 3");
  assert.equal(await evaluate("parseFloat(document.querySelector('[data-board-card=\"step-native-0\"]').style.left)"), 133, "layout survives reopening");
  await click('[data-board-card="step-native-1"] .card-body');
  await evaluate("document.querySelector('.board-viewport').focus()");
  await call("Input.dispatchKeyEvent", { type: "keyDown", key: "Delete", code: "Delete", windowsVirtualKeyCode: 46 });
  await call("Input.dispatchKeyEvent", { type: "keyUp", key: "Delete", code: "Delete", windowsVirtualKeyCode: 46 });
  await wait("window.__fixture.preferences.workflowGroups[0].steps.length === 2");
  assert.equal(await evaluate("window.__fixture.preferences.workflowGroups[0].connections[0].toStepId"), "step-native-2", "removing a middle step reconnects its neighbors");
  await click(".edge-insert");
  await evaluate("Array.from(document.querySelectorAll('.agent-options button')).find(button => button.textContent.includes('Implementação')).click()");
  await wait("window.__fixture.preferences.workflowGroups[0].steps.length === 3");
  assert.equal(await evaluate("window.__fixture.preferences.workflowGroups[0].connections.length"), 2, "an optional intermediate agent splits the arrow");
  await call("Emulation.setDeviceMetricsOverride", { width: 1024, height: 720, deviceScaleFactor: 1, mobile: false });
  await wait("document.querySelector('.board-viewport').clientWidth < 800");
  await click('[aria-label="Ajustar à tela"]');
  await delay(200);
  await screenshot("compact-light");
  await evaluate("(async () => { const p = window.__fixture.preferences; p.darkMode = true; await window.__TAURI_INTERNALS__.invoke('set_preferences', { preferences: p }); window.__fixture.emit('lume://preferences-changed', p); })()");
  await screenshot("compact-dark");
  assert.equal(await evaluate("(() => { const canvas = document.querySelector('.board-viewport').getBoundingClientRect(); const panel = document.querySelector('.board-panel').getBoundingClientRect(); return Array.from(document.querySelectorAll('.board-card')).every(card => { const r = card.getBoundingClientRect(); return r.left >= canvas.left && r.right <= panel.left && r.top >= canvas.top + 65; }); })()"), true, "fit keeps cards clear of the editor and toolbar after resizing");
  assert.deepEqual(failures, [], "no browser exceptions");
  console.log("Workflow board UI passed: Orb import, sidebar drop, arrows, cycle refusal, free drag, preview, zoom, all run controls, recovery, event order, draft/layout persistence, removal, insertion, overlay keyboard ownership, fit, light/dark contrast.");
  console.log("Screenshots: " + output);
} finally {
  socket?.close();
  browser.kill("SIGTERM");
  server.close();
}
