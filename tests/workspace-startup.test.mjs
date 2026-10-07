import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { WorkspaceStartup } from "../src/lib/workspaceStartup.ts";

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}

function fixture() {
  const startup = new WorkspaceStartup();
  const calls = [];
  return {
    startup, calls,
    options: {
      ready: async () => { calls.push("ready"); },
      failed: async (reason) => { calls.push(["failed", reason]); },
      onLoaded: () => { calls.push("loaded"); },
      onError: (reason) => { calls.push(["error", reason]); },
      timeoutMessage: "Startup timed out",
      timeoutMs: 500,
    },
  };
}

test("successful startup waits for initialization before acknowledging readiness", async () => {
  const { startup, calls, options } = fixture();
  const loading = deferred();
  const run = startup.run(() => loading.promise, options);
  assert.deepEqual(calls, []);
  loading.resolve();
  await run;
  assert.deepEqual(calls, ["loaded", "ready"]);
  startup.dispose();
});

for (const stage of ["preferences", "listener", "snapshot"]) {
  test(`${stage} failure clears loading and reports the original error`, async () => {
    const { startup, calls, options } = fixture();
    let stopped = 0;
    await startup.run(async () => {
      await startup.subscribe(async () => () => { stopped++; });
      throw new Error(`${stage} unavailable`);
    }, options);
    assert.deepEqual(calls, ["loaded", ["error", `${stage} unavailable`], ["failed", `${stage} unavailable`]]);
    assert.equal(stopped, 1);
    assert.equal(startup.active, false);
    startup.dispose();
    assert.equal(stopped, 1);
  });
}

test("a rejected native acknowledgement is recoverable", async () => {
  const { startup, calls, options } = fixture();
  options.ready = async () => { throw new Error("Native acknowledgement failed"); };
  await startup.run(async () => {}, options);
  assert.deepEqual(calls.at(-1), ["failed", "Native acknowledgement failed"]);
  assert.equal(startup.active, false);
});

test("failed recovery IPC does not leave an unhandled rejection", async () => {
  const { startup, calls, options } = fixture();
  options.failed = async () => { throw new Error("IPC offline"); };
  await startup.run(async () => { throw new Error("Load failed"); }, options);
  assert.deepEqual(calls, ["loaded", ["error", "Load failed"]]);
});

test("startup timeout cannot later acknowledge a stale window", async () => {
  const { startup, calls, options } = fixture();
  const loading = deferred();
  await startup.run(() => loading.promise, { ...options, timeoutMs: 5 });
  assert.deepEqual(calls.at(-1), ["failed", "Startup timed out"]);
  loading.resolve();
  await Promise.resolve();
  assert.ok(!calls.includes("ready"));
});

test("closing during initialization cancels without reporting a spurious error", async () => {
  const { startup, calls, options } = fixture();
  const loading = deferred();
  const run = startup.run(() => loading.promise, options);
  startup.dispose();
  await run;
  loading.resolve();
  await Promise.resolve();
  assert.deepEqual(calls, []);
});

test("a listener registered after disposal is immediately removed", async () => {
  const { startup, options } = fixture();
  const registration = deferred();
  let stopped = 0;
  const run = startup.run(() => startup.subscribe(() => registration.promise), options);
  startup.dispose();
  await run;
  registration.resolve(() => { stopped++; });
  await Promise.resolve();
  await Promise.resolve();
  assert.equal(stopped, 1);
});

test("active subscriptions remain until successful workspace disposal", async () => {
  const { startup, options } = fixture();
  let stopped = 0;
  await startup.run(() => startup.subscribe(async () => () => { stopped++; }), options);
  assert.equal(stopped, 0);
  startup.dispose();
  assert.equal(stopped, 1);
});

test("window creation and hot database commands stay outside the UI thread", () => {
  const lib = readFileSync(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8");
  for (const command of ["open_workspace_window", "list_sessions", "get_hub_snapshot", "get_preferences", "get_workspace_prompt_index_page", "get_workspace_conversation_page", "get_terminal_hub_snapshot"]) {
    assert.match(lib, new RegExp(`async fn ${command}\\(`), command);
  }
  assert.ok(!lib.includes("WebviewWindowBuilder"), "callbacks must use the shared asynchronous coordinator");
  assert.equal((lib.match(/workspace_windows::schedule_open\(app\)/g) ?? []).length, 3);
});

test("frontend acknowledgement is wired after sessions load and render", () => {
  const component = readFileSync(new URL("../src/lib/WorkspaceWindow.svelte", import.meta.url), "utf8");
  assert.match(component, /const \[loadedPreferences, loadedSessions\] = await Promise\.all\(\[\s*loadPreferences\(\),\s*refresh\(\),\s*\]\)/);
  assert.match(component, /if \(!loadedSessions\) throw new Error/);
  assert.match(component, /await tick\(\); await markWorkspaceFrontendReady\(\)/);
  assert.match(component, /startup\.dispose\(\)/);
});

test("startup failure is visible in the Orb regardless of its active tab", () => {
  const orb = readFileSync(new URL("../src/routes/+page.svelte", import.meta.url), "utf8");
  assert.match(orb, /if \(workspaceOpenError\) visibleItems\.unshift\(/);
  assert.match(orb, /workspaceOpenError = String\(reason\)[\s\S]*?startupChooserOpen = false/);
  assert.match(orb, /lume:\/\/workspace-open-failed/);
});

test("Workspace settings defer section content and native reads until expanded", () => {
  const workspace = readFileSync(new URL("../src/lib/WorkspaceWindow.svelte", import.meta.url), "utf8");
  assert.ok(/let settingsSections = \$state\(\{\s*appearance: false,[\s\S]*?about: false,[\s\S]*?reset: false/.test(workspace), "all settings sections start collapsed");
  assert.ok(/function openSettings\(\) \{\s*settingsOpen = true;\s*\}/.test(workspace), "opening the drawer does not load section data");
  assert.ok(/function closeSettings\(\) \{\s*settingsOpen = false;\s*settingsSections = \{\s*appearance: false,[\s\S]*?about: false,[\s\S]*?reset: false/.test(workspace), "reopening the drawer stays lightweight after a previous visit");
  assert.ok(/function setSettingsSectionOpen\(section: SettingsSectionKey, open: boolean\) \{\s*settingsSections\[section\] = open;\s*if \(open\) void loadSettingsSectionData\(section\);/.test(workspace), "section data loads only when expanded");
  assert.ok(/preferences: \["monitors"\],[\s\S]*?agents: \["integrations"\],[\s\S]*?companions: \["vscode"\],[\s\S]*?externalDetectors: \["externalPlugins"\],[\s\S]*?mobileAccess: \["mobileStatus", "pairedDevices"\]/.test(workspace), "native reads are mapped to their owning sections");
  assert.ok(workspace.includes("{#if settingsSections.agents}") && workspace.includes("{#if settingsSections.about}"), "section bodies are conditionally mounted");
  assert.ok(!workspace.includes("loadSettingsData()"), "the old eager settings load is removed");
});

test("Workspace setting explanations use the shared tooltip instead of a second text line", () => {
  const workspace = readFileSync(new URL("../src/lib/WorkspaceWindow.svelte", import.meta.url), "utf8");
  assert.ok(workspace.includes('data-tooltip={tr("Used across Lume", "Usado em todo o Lume")}'));
  assert.ok(workspace.includes('data-tooltip={tr("Default view for the next launch", "Visualização padrão da próxima abertura")}'));
  assert.ok(workspace.includes('data-tooltip={tr("Task and permission feedback", "Retorno de tarefas e permissões")}'));
  assert.ok(workspace.includes('data-tooltip={tr("Primary display by default", "Tela principal por padrão")}'));
  assert.ok(!workspace.includes('<small>{tr("Used across Lume", "Usado em todo o Lume")}</small>'));
  assert.ok(!workspace.includes('<small>{tr("Default view for the next launch", "Visualização padrão da próxima abertura")}</small>'));
  assert.ok(workspace.includes("font-size: 10px; font-weight: 730"), "setting names are more legible without their descriptions");
});
