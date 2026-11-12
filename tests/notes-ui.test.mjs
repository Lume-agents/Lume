// Browser test of the Notes tab: list by project and tag, markdown preview, linked prompt, create, plans filter and delete,
// in light and dark, against stubbed IPC. Set LUME_TEST_BROWSER for Chromium.
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
const output = mkdtempSync(join(tmpdir(), "lume-notes-ui-"));
const read = (path) => readFileSync(join(root, path), "utf8");

await build({
  stdin: {
    contents: 'import "./src/app.css"; import "./src/lib/workspaceTokens.css"; import { mount } from "svelte"; import Notes from "./src/lib/WorkspaceNotes.svelte"; for (const id of ["light", "dark"]) mount(Notes, { target: document.getElementById(id), props: { language: "en", project: id === "light" ? "/work/lume" : undefined, canOpenLinked: (note) => note.nativeSessionId === "native-1", onOpenLinked: (note) => window.__opened.push(note.id), onClose() {} } });',
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

// A small notes store behind stubbed IPC: list_notes filters like the backend, save and delete change it.
function fixture() {
  window.__opened = [];
  const now = Date.UTC(2026, 10, 12, 14, 0);
  let notes = [
    { id: "n1", nativeSessionId: "native-1", title: "Release checklist", body: "## Release\n\n- [x] Changelog\n- [ ] Tag `v0.19`\n\nRun **npm run check** first.", kind: "note", pinned: true, createdAt: now, updatedAt: now, agent: "codex", project: "/work/lume", tags: ["release"], promptActivityId: "p7", promptExcerpt: "Prepare the release" },
    { id: "n2", nativeSessionId: "", title: "Ideas", body: "Try a dark board.", kind: "note", pinned: false, createdAt: now - 9e5, updatedAt: now - 9e5, project: "/work/lume", tags: ["ideas"] },
    { id: "p1", nativeSessionId: "native-2", title: "Saved plan", body: "1. Inspect\n2. Implement", kind: "plan", pinned: false, createdAt: now - 2e6, updatedAt: now - 2e6, agent: "claude_code", project: "/work/other", tags: [] },
  ];
  window.__notes = () => notes;
  window.__TAURI_INTERNALS__ = {
    transformCallback: () => 1,
    invoke: async (command, args) => {
      if (command === "list_notes") {
        const query = (args.query || "").toLowerCase();
        return notes.filter((note) => (!args.project || note.project === args.project) && (!args.kind || note.kind === args.kind)
          && (!query || (note.title + note.body + note.tags.join(" ")).toLowerCase().includes(query)));
      }
      if (command === "save_session_note") {
        const existing = notes.find((note) => note.id === args.noteId);
        const saved = { ...(existing ?? { id: "new-" + notes.length, nativeSessionId: "", createdAt: Date.now(), project: args.links?.project }), title: args.title || "Note", body: args.body, kind: args.kind, pinned: args.pinned, updatedAt: Date.now(), tags: args.links?.tags ?? existing?.tags ?? [] };
        notes = [saved, ...notes.filter((note) => note.id !== saved.id)];
        return saved;
      }
      if (command === "delete_session_note") { notes = notes.filter((note) => note.id !== args.id); return null; }
      return null;
    },
  };
  window.confirm = () => true;
}

const page = `<html><head><link rel="stylesheet" href="/app.css"></head><body style="margin:0">
<div class="workspace-tokens" style="position:relative;height:520px;background:var(--workspace-bg)"><div id="light"></div></div>
<div class="workspace-tokens dark" style="position:relative;height:520px;background:var(--workspace-bg)"><div id="dark"></div></div>
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
  await call("Emulation.setDeviceMetricsOverride", { width: 1000, height: 1040, deviceScaleFactor: 1, mobile: false });
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
  await wait("document.querySelectorAll('#light .note-row').length === 2");
  // The focused project is chosen first: the other project's plan is not listed.
  assert.deepEqual(await evaluate("Array.from(document.querySelectorAll('#light .note-row strong')).map(item => item.textContent)"), ["Release checklist", "Ideas"]);
  await evaluate("document.querySelector('#light .note-row').click()");
  await wait("document.querySelector('#light .preview li')");
  assert.match(await evaluate("document.querySelector('#light .preview').innerHTML"), /<strong>npm run check<\/strong>/, "markdown preview");
  assert.match(await evaluate("document.querySelector('#light .linked').innerText"), /Prepare the release[\s\S]*Open linked prompt/);
  await evaluate("document.querySelector('#light .linked button').click()");
  assert.deepEqual(await evaluate("window.__opened"), ["n1"]);
  // Tag filter, then search.
  await evaluate("Array.from(document.querySelectorAll('#light .tag-filter button')).find(item => item.textContent === '#ideas').click()");
  await wait("document.querySelectorAll('#light .note-row').length === 1");
  await evaluate("Array.from(document.querySelectorAll('#light .tag-filter button')).find(item => item.textContent === '#ideas').click()");
  // Create, edit and save a note with tags.
  await evaluate("document.querySelector('#light .new-note').click()");
  await wait("document.activeElement?.classList.contains('title-input')");
  await evaluate(`(() => { const set = (selector, value) => { const field = document.querySelector('#light ' + selector); field.value = value; field.dispatchEvent(new Event('input', { bubbles: true })); };
    set('.title-input', 'Deploy steps'); set('.tags-input', '#Ops, release'); set('.body-input', '1. Build\\n2. Ship'); })()`);
  await evaluate("document.querySelector('#light .primary').click()");
  await wait("document.querySelectorAll('#light .note-row').length === 3");
  const saved = await evaluate("window.__notes()[0]");
  assert.deepEqual([saved.title, saved.tags, saved.project], ["Deploy steps", ["ops", "release"], "/work/lume"]);
  // Plans have their own filter, across projects.
  await evaluate("document.querySelector('#dark .notes-tabs button:last-child').click()");
  await wait("document.querySelectorAll('#dark .note-row').length === 1");
  await evaluate("document.querySelector('#dark .note-row').click()");
  await wait("document.querySelector('#dark .preview ol')");
  await screenshot("notes");
  // Delete.
  await evaluate("document.querySelector('#light .note-row').click()");
  await wait("document.querySelector('#light .ghost.danger')");
  await evaluate("document.querySelector('#light .ghost.danger').click()");
  await wait("document.querySelectorAll('#light .note-row').length === 2");
  assert.deepEqual(failures, []);
  console.log("Notes UI passed. Screenshots: " + output);
} finally {
  socket?.close();
  browser.kill();
  server.close();
}
