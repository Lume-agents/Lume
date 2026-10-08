// Renders the real Lume Svelte surfaces in headless Chromium against illustrative fixture
// sessions and writes PNG captures for the marketing site. Usage:
//   node scripts/site-capture/capture.mjs <base-url> <out-dir> [only]
// <base-url> is a running `vite dev` server of this repository. No agent is ever started.
import { spawn } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, writeFileSync } from "node:fs";
import { once } from "node:events";
import { join } from "node:path";
import { buildFixture } from "./fixture.mjs";

const [baseUrl = "http://127.0.0.1:1521", outDir = "site-captures", only] = process.argv.slice(2);
mkdirSync(outDir, { recursive: true });
const cache = join(process.env.HOME, ".cache/puppeteer/chrome-headless-shell");
const executable = process.env.LUME_TEST_BROWSER || readdirSync(cache).sort().reverse().map((version) => join(cache, version, "chrome-headless-shell-linux64/chrome-headless-shell")).find(existsSync);

function nativeFixture(data, label, extra) {
  const { sessions, group, language, dark } = data;
  const listeners = new Map(); const callbacks = new Map(); let nextId = 1;
  window.__unknown = [];
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: (_, id) => listeners.delete(id) };
  const ready = import("/src/lib/lume.ts").then((module) => module.defaultPreferences);
  const settings = { language, darkMode: dark, startupMode: "orb", workflowGroups: [group], ...extra.preferences };
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label }, currentWebview: { label } },
    transformCallback(callback) { const id = nextId++; callbacks.set(id, callback); return id; },
    unregisterCallback(id) { callbacks.delete(id); },
    async invoke(command, args = {}) {
      args = JSON.parse(JSON.stringify(args));
      if (command === "plugin:event|listen") { const id = nextId++; listeners.set(id, args); return id; }
      if (command === "plugin:event|unlisten") { listeners.delete(args.eventId); return; }
      if (command === "plugin:event|emit") return;
      if (command === "get_preferences") return { ...(await ready), ...settings };
      if (command === "set_preferences") return;
      if (command === "get_hub_snapshot") return { protocolVersion: 1, desktopVersion: "0.15.4", generatedAt: Date.now(), features: [], sessions, internalServices: [], workflowGroups: [group], workflowHistory: [] };
      if (command === "list_sessions") return sessions;
      if (command === "get_workflow_run") {
        if (!extra.run) return null;
        const t = Date.now();
        return { id: "run-demo", workflowId: group.id, objective: "Ship the responsive navigation", status: "running", currentStepId: group.steps[1].id, handoffApproved: false, recovering: false, transitionCount: 1, createdAt: t - 600000, updatedAt: t,
          steps: group.steps.map((step, index) => ({ stepId: step.id, status: ["completed", "running", "pending"][index], attempt: 1 })) };
      }
      if (command === "get_workspace_prompt_index_page") return { prompts: [], hasMore: false };
      if (command === "get_session_model_settings") {
        const efforts = ["low", "medium", "high"].map((value) => ({ value, description: value }));
        return { model: "gpt-5.5", reasoningEffort: "medium", serviceTier: null, models: [{ model: "gpt-5.5", displayName: "GPT-5.5", description: "Frontier coding model", isDefault: true, defaultReasoningEffort: "medium", supportedReasoningEfforts: efforts }] };
      }
      if (command === "get_claude_session_model_settings") {
        const efforts = ["low", "medium", "high"].map((value) => ({ value, description: value }));
        return { model: "claude-opus-5-5", reasoningEffort: "medium", models: [{ model: "claude-opus-5-5", displayName: "Opus 5.5", description: "Most capable", isDefault: true, defaultReasoningEffort: "medium", supportedReasoningEfforts: efforts }, { model: "claude-sonnet-5-5", displayName: "Sonnet 5.5", description: "Balanced", isDefault: false, defaultReasoningEffort: "medium", supportedReasoningEfforts: efforts }] };
      }
      if (command === "get_session_permission_mode") return { mode: "default", modes: ["default", "acceptEdits", "plan", "bypassPermissions"] };
      if (command === "get_session_collaboration_mode") return "default";
      if (command === "get_session_repository") {
        const days = Array.from({ length: 84 }, (_, index) => ({ date: new Date(Date.now() - (83 - index) * 864e5).toISOString().slice(0, 10), count: (index * 7) % 5, level: (index * 7) % 5 }));
        return { root: "/home/dev/Lume", name: "Lume", branch: "feature/responsive-navigation", head: "a1b2c3d", upstream: "origin/main", ahead: 2, behind: 0, staged: 0, modified: 2, untracked: 0, conflicts: 0,
          files: [{ path: "src/lib/Navigation.svelte", index: " ", worktree: "M", conflict: false, untracked: false }, { path: "src/routes/+layout.svelte", index: " ", worktree: "M", conflict: false, untracked: false }],
          commits: [{ oid: "a1b2c3d", subject: "Add a mobile navigation menu", author: "dev", date: new Date().toISOString() }], days, activityLimited: false, shallow: false, github: "acme/lume", fetchedAt: Date.now() };
      }
      if (extra.commands?.[command]) return typeof extra.commands[command] === "function" ? extra.commands[command](args) : extra.commands[command];
      if (command.includes("version")) return "0.15.4";
      if (command.includes("scale_factor")) return 1;
      if (command.includes("position")) return { x: 0, y: 0 };
      if (command.includes("is_maximized") || command.includes("is_fullscreen")) return false;
      if (command === "get_shortcut_registration_error") return null;
      window.__unknown.push(command);
      return [];
    },
  };
}

const jobs = {
  workspace: { route: "/workspace", width: 1480, height: 860, wait: "document.querySelectorAll('[data-workspace-pane]').length >= 2" },
  orb: { route: "/", width: 420, height: 435, transparent: true, wait: "document.body.innerText.length > 20" },
  board: { route: "/workspace", width: 1480, height: 700, board: true, extra: { run: true }, storage: true, wait: "document.querySelector('.workflow-board:not([hidden]) .board-card')" },
};

const browser = spawn(executable, ["--no-sandbox", "--disable-dev-shm-usage", "--hide-scrollbars", "--remote-debugging-port=0", "about:blank"], { stdio: ["ignore", "ignore", "pipe"] });
let log = "";
const endpoint = await new Promise((resolve, reject) => {
  const timer = setTimeout(() => reject(new Error("Chromium startup timeout: " + log.slice(-800))), 15000);
  browser.stderr.on("data", (chunk) => { log += chunk; const match = log.match(/DevTools listening on (ws:\/\/\S+)/); if (match) { clearTimeout(timer); resolve(match[1]); } });
});
const socket = new WebSocket(endpoint);
await once(socket, "open");
const pending = new Map(); let sequence = 0;
socket.addEventListener("message", ({ data }) => { const m = JSON.parse(data); if (m.method === "Runtime.exceptionThrown") console.error("page error:", m.params.exceptionDetails.exception?.description || m.params.exceptionDetails.text); if (m.method === "Runtime.consoleAPICalled" && m.params.type === "error") console.error("console:", m.params.args.map((a) => a.value ?? a.description).join(" ").slice(0, 400)); if (m.id) { const p = pending.get(m.id); pending.delete(m.id); m.error ? p.reject(new Error(m.error.message)) : p.resolve(m.result); } });
const send = (method, params = {}, sessionId) => { const id = ++sequence; return new Promise((resolve, reject) => { pending.set(id, { resolve, reject }); socket.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) })); }); };
const delay = (ms) => new Promise((r) => setTimeout(r, ms));

try {
  for (const [name, job] of Object.entries(jobs)) {
    if (only && only !== name) continue;
    const { targetId } = await send("Target.createTarget", { url: "about:blank" });
    const { sessionId } = await send("Target.attachToTarget", { targetId, flatten: true });
    const call = (method, params) => send(method, params, sessionId);
    const evaluate = async (expression) => { const r = await call("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true }); if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description || r.exceptionDetails.text); return r.result.value; };
    await call("Runtime.enable"); await call("Page.enable");
    await call("Emulation.setDeviceMetricsOverride", { width: job.width, height: job.height, deviceScaleFactor: 2, mobile: false });
    const data = buildFixture({ language: "en", dark: true });
    const label = job.route.slice(1) || "main";
    await call("Page.addScriptToEvaluateOnNewDocument", { source: `localStorage.setItem("lume:workflow-board:open", ${job.board ? '"true"' : '"false"'}); ${job.storage ? `localStorage.setItem("lume:workflow-board:v1", ${JSON.stringify(JSON.stringify({ activeGroupId: data.group.id, layouts: { [data.group.id]: { name: "Ship the navigation", positions: Object.fromEntries(data.group.steps.map((step, index) => [step.id, { x: 90 + index * 380, y: 150 }])), view: { x: 0, y: 0, zoom: 1 } } }, objectives: { [data.group.id]: "Ship the responsive navigation" } }))});` : ""} localStorage.setItem("lume:workspace-layout:v1", ${JSON.stringify(JSON.stringify({ primaryId: "s1", secondaryId: "s2", focusedPaneId: "s1", splitRatio: 0.5, inspectorOpen: true, ...(job.layout || {}) }))}); (${nativeFixture.toString()})(${JSON.stringify(data)}, ${JSON.stringify(label)}, ${JSON.stringify(job.extra || {})});` });
    if (job.transparent) await call("Emulation.setDefaultBackgroundColorOverride", { color: { r: 0, g: 0, b: 0, a: 0 } });
    await call("Page.navigate", { url: baseUrl + job.route });
    const deadline = Date.now() + 20000;
    while (Date.now() < deadline && !(await evaluate(job.wait).catch(() => false))) await delay(150);
    await delay(900);
    await evaluate("(() => { const style = document.createElement('style'); style.textContent = '.system-banner-stack { display: none !important; }'; document.head.append(style); })()");
    await delay(500);
    const shot = await call("Page.captureScreenshot", { format: "png" });
    writeFileSync(join(outDir, name + ".png"), Buffer.from(shot.data, "base64"));
    console.log(name, "unknown IPC:", JSON.stringify(await evaluate("[...new Set(window.__unknown)]")));
    await send("Target.closeTarget", { targetId });
  }
} finally { socket.close(); browser.kill("SIGTERM"); }
