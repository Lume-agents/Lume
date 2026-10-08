// Browser test of the review center with native IPC stubbed: file review progress, keyboard,
// folded context, word-level highlights and the decision bar. Set LUME_TEST_BROWSER for Chromium.
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
const output = mkdtempSync(join(tmpdir(), "lume-review-center-ui-"));
const cache = join(process.env.HOME, ".cache/puppeteer/chrome-headless-shell");
const cached = existsSync(cache) ? readdirSync(cache).sort().reverse().map((version) => join(cache, version, "chrome-headless-shell-linux64/chrome-headless-shell")).find(existsSync) : undefined;
const executable = process.env.LUME_TEST_BROWSER || cached;
assert.ok(executable, "Set LUME_TEST_BROWSER to a Chromium executable");

await build({
  stdin: {
    contents: 'import "./src/app.css"; import { mount } from "svelte"; import Review from "./src/lib/WorkspaceReviewCenter.svelte"; window.__review = mount(Review, { target: document.getElementById("app"), props: { session: window.__session, language: "pt-BR", onClose() { window.__closed = true; } } });',
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
  const body = Array.from({ length: 40 }, (_, index) => `line ${index + 1}();`);
  const long = ["diff --git a/src/cart.ts b/src/cart.ts", "--- a/src/cart.ts", "+++ b/src/cart.ts", "@@ -1,40 +1,40 @@",
    ...body.slice(0, 18).map((line) => " " + line), '-const total = price * 2; // old', '+const total = price * quantity; // new', ...body.slice(19, 40).map((line) => " " + line)].join("\n");
  const small = ["diff --git a/README.md b/README.md", "--- a/README.md", "+++ b/README.md", "@@ -1,1 +1,2 @@", " # Lume", "+Reviewed."].join("\n");
  const now = Date.now();
  window.__session = {
    id: "session-1", nativeSessionId: "native-1", sessionName: "Carrinho", agent: "codex", agentLabel: "Codex", project: "Lume", workingDirectory: "/fixture/lume",
    source: "desktop", controlOrigin: "lume", status: "completed", statusLabel: "Finalizado", startedAt: new Date().toISOString(), updatedAt: now,
    results: [{ id: "result-1", response: "Corrigi o cálculo do total.\n\nTambém atualizei o README.", createdAt: now, files: ["src/cart.ts", "README.md"], tests: ["npm test: 12 passed"] }],
    activities: [
      { id: "p1", kind: "prompt", title: "Prompt", detail: "Corrija o total do carrinho", status: "completed", createdAt: now - 5000, files: [] },
      { id: "f1", kind: "file", title: "Files changed", detail: long, status: "completed", createdAt: now - 4000, files: ["src/cart.ts"] },
      { id: "f2", kind: "file", title: "Files changed", detail: small, status: "completed", createdAt: now - 3000, files: ["README.md"] },
    ],
    activityTotal: 3, permissionProfile: { canRespondFromLume: true, mode: "full_access" },
    capabilities: { canPrompt: true, canReadResults: true, canTakeControl: false, promptDeliveries: ["new_turn", "queue"] },
    workSummary: { files: [], checks: [], todos: [], sources: [] },
  };
  const calls = window.__calls = [];
  window.__TAURI_INTERNALS__ = {
    transformCallback: () => 1,
    invoke: async (command, args) => {
      calls.push([command, args]);
      if (command === "set_review_decision") return { id: "d1", nativeSessionId: "native-1", resultId: args.resultId, decision: args.decision, note: args.note, createdAt: Date.now(), updatedAt: Date.now() };
      if (command.startsWith("plugin:")) return null;
      const run = { id: "run-1", workflowId: "wf-1", objective: "Entregar o carrinho", status: "waiting_for_approval", pendingConnectionId: "c1", handoffApproved: false, recovering: false, transitionCount: 1, createdAt: 1, updatedAt: 2, steps: [{ stepId: "s1", status: "completed", attempt: 0, resultId: "result-1" }, { stepId: "s2", status: "pending", attempt: 0 }] };
      if (command === "list_workflow_history") return [{ run, group: { id: "wf-1", steps: [], connections: [{ id: "c1", fromStepId: "s1", toStepId: "s2" }] }, events: [], steps: [{ stepId: "s1", roleLabel: "Planejador" }, { stepId: "s2", roleLabel: "Implementador" }] }];
      if (command === "get_workflow_run") return run;
      if (command === "approve_workflow_handoff") return { ...run, status: "running", pendingConnectionId: undefined, handoffApproved: true };
      if (command === "get_session_repository") return { root: "/fixture/lume", name: "lume", branch: "main", head: "abc", upstream: null, ahead: 0, behind: 0, staged: 0, modified: 1, untracked: 0, conflicts: 0, files: [{ path: "notes.txt", index: " ", worktree: "M", conflict: false, untracked: false }], commits: [], days: [], activityLimited: false, shallow: false, github: null, fetchedAt: 1 };
      if (command === "get_session_repository_diff") return { path: args.path, diff: "diff --git a/notes.txt b/notes.txt\n@@ -1,1 +1,2 @@\n keep\n+from git", binary: false, untracked: false };
      return [];
    },
  };
}

const server = createServer((request, response) => {
  const path = new URL(request.url, "http://localhost").pathname;
  if (path === "/") { response.setHeader("content-type", "text/html"); response.end('<html><head><link rel="stylesheet" href="/app.css"></head><body style="margin:0"><div id="app" style="width:1000px;height:720px;--workspace-sidebar:var(--lume-sidebar-dark);--workspace-pane:var(--lume-pane-dark);--workspace-raised:var(--lume-raised-dark);--workspace-line:var(--lume-line-dark);--workspace-strong:var(--lume-ink-strong-dark);--workspace-text:var(--lume-ink-dark);--workspace-muted:var(--lume-ink-muted-dark);--workspace-faint:var(--lume-ink-faint-dark);--workspace-accent:var(--lume-accent);--workspace-accent-soft:var(--lume-accent-soft-dark);--workspace-subtle:var(--lume-subtle-dark);--workspace-code:var(--lume-code-dark);--workspace-scroll-thumb:var(--lume-scroll-dark);background:var(--lume-canvas-dark);color:var(--workspace-text);font-family:var(--lume-font-ui, sans-serif)"></div><script type="module" src="/app.js"></script></body></html>'); return; }
  const file = join(output, path.slice(1));
  if (!file.startsWith(output + "/") || !existsSync(file)) { response.writeHead(404).end(); return; }
  response.setHeader("content-type", { ".css": "text/css", ".js": "text/javascript" }[extname(file)] || "application/octet-stream");
  response.end(readFileSync(file));
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
  await call("Emulation.setDeviceMetricsOverride", { width: 1000, height: 720, deviceScaleFactor: 1, mobile: false });
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
  const click = async (selector) => { await evaluate("document.querySelector(" + JSON.stringify(selector) + ").click()"); await delay(); };
  const key = async (value) => { await evaluate("document.querySelector('.review-center').dispatchEvent(new KeyboardEvent('keydown', { key: " + JSON.stringify(value) + ", bubbles: true }))"); await delay(); };

  await call("Page.navigate", { url: "http://127.0.0.1:" + server.address().port });
  await wait("document.querySelectorAll('.file-row').length === 2");
  await evaluate("Array.from(document.querySelectorAll('.file-open')).find((button) => button.title === 'src/cart.ts').click()");
  await wait("document.querySelector('.diff-path strong')?.title === 'src/cart.ts'");
  assert.match(await evaluate("document.querySelector('.progress').textContent"), /0\/2/);
  assert.match(await evaluate("document.querySelector('.totals').textContent"), /\+2.*−1/);
  await screenshot("review-unified");
  assert.equal(await evaluate("document.querySelectorAll('.fold-row').length"), 2, "long unchanged runs are folded");
  assert.ok(await evaluate("document.querySelector('.line-added .chg')?.textContent.includes('quantity')"), "the changed word is highlighted");
  assert.ok(await evaluate("document.querySelector('.tk-com')"), "comments are colored");
  await click(".fold-row");
  assert.equal(await evaluate("document.querySelectorAll('.fold-row').length"), 1, "a fold expands on click");
  await click(".mode-switch button:last-child");
  await wait("document.querySelector('.split-diff')");
  assert.ok(await evaluate("document.querySelectorAll('.split-cell').length >= 2"));
  await screenshot("review-split");
  // Line comments: add one on the changed line, see it in the file list, and send it with the request.
  await click(".line-added .add-comment");
  await wait("document.querySelector('.comment-composer textarea')");
  await evaluate("(() => { const area = document.querySelector('.comment-composer textarea'); area.value = 'Use a quantidade validada.'; area.dispatchEvent(new Event('input', { bubbles: true })); })()");
  await evaluate("document.querySelector('.comment-composer').requestSubmit()");
  await wait("document.querySelector('.comment-card p')?.textContent.includes('quantidade validada')");
  assert.match(await evaluate("document.querySelector('.comments-chip').textContent"), /1 comentário/);
  assert.equal(await evaluate("document.querySelector('.file-comments')?.textContent"), "1");
  await click(".mode-switch button:first-child");
  await click(".mode-switch button:last-child");
  await wait("document.querySelector('.split-threads .comment-card')");
  await screenshot("review-comment");
  await click(".comments-chip");
  await wait("document.querySelector('.correction-comments')");
  assert.equal(await evaluate("document.querySelector('.send-correction').disabled"), false, "comments alone are enough to send a request");
  await click(".send-correction");
  await wait("window.__calls.some(([command]) => command === 'submit_prompt')");
  const sent = await evaluate("window.__calls.find(([command]) => command === 'submit_prompt')[1].prompt");
  assert.match(sent, /Line comments:\n\n- src\/cart\.ts:19\n {2}Code: const total = price \* quantity;/);
  assert.match(sent, /Comment: Use a quantidade validada\./);
  await wait("document.querySelector('.comment-card.sent')");
  assert.equal(await evaluate("document.querySelector('.comments-chip')"), null, "sent comments are no longer pending");
  await wait("document.querySelector('.decision-state.changes_requested')");
  // Keyboard review: v marks the file and moves on, j/k move between files.
  await key("v");
  assert.match(await evaluate("document.querySelector('.progress').textContent"), /1\/2/);
  assert.equal(await evaluate("document.querySelector('.file-row.active .file-open').title"), "README.md", "marking reviewed jumps to the next unreviewed file");
  await key("k");
  assert.equal(await evaluate("document.querySelector('.file-row.active .file-open').title"), "src/cart.ts");
  await click(".file-row:nth-child(1) .file-check");
  assert.match(await evaluate("document.querySelector('.progress').textContent"), /2\/2/);
  assert.ok(await evaluate("JSON.parse(localStorage.getItem(Object.keys(localStorage).find(k => k.startsWith('lume:review:reviewed')))).length === 2"), "the marks persist");
  // Decisions and the agent summary.
  await click(".answer-strip > button");
  assert.ok(await evaluate("document.querySelector('.answer-strip .reading-copy')?.textContent.includes('README')"));
  assert.match(await evaluate("document.querySelector('.handoff-chip')?.textContent ?? ''"), /Handoff pendente.*Implementador/, "the pending handoff is shown");
  assert.match(await evaluate("document.querySelector('.approve-result').textContent"), /Aprovar e avançar/);
  await click(".approve-result");
  await wait("document.querySelector('.decision-state.approved')");
  assert.ok(await evaluate("window.__calls.some(([command, args]) => command === 'approve_workflow_handoff' && args.workflowId === 'wf-1')"), "approving releases the workflow handoff");
  await wait("!document.querySelector('.handoff-chip')");
  // The git view lists the working tree and reads each file's diff on demand.
  await click(".lume-select-trigger");
  await evaluate("Array.from(document.querySelectorAll('.lume-select-menu [role=option]')).find((option) => option.textContent.includes('Git')).click()");
  await wait("document.querySelector('.git-note')");
  await wait("document.querySelector('.diff-path strong')?.title === 'notes.txt'");
  await wait("Array.from(document.querySelectorAll('.split-cell.line-added')).some((line) => line.textContent.includes('from git'))");
  assert.ok(await evaluate("window.__calls.some(([command, args]) => command === 'get_session_repository_diff' && args.path === 'notes.txt')"));
  await screenshot("review-git");
  assert.ok(await evaluate("window.__calls.some(([command, args]) => command === 'set_review_decision' && args.decision === 'approved')"));
  await click(".close-review");
  await wait("window.__closed === true");
  assert.deepEqual(failures, []);
  console.log("Review center UI passed. Screenshots: " + output);
} finally {
  socket?.close();
  browser.kill();
  server.close();
}
