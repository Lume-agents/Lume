import assert from "node:assert/strict";
import { test } from "node:test";
import { readFileSync } from "node:fs";
import { AppUpdateController, releaseNotesSeenKey, releaseNotesCacheKey } from "../src/lib/appUpdates.ts";

const turn = () => new Promise(resolve => setImmediate(resolve));
function deferred() {
  let resolve, reject;
  const promise = new Promise((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}
function fixture(overrides = {}, saved = new Map([[releaseNotesSeenKey, "0.18.2"]])) {
  const calls = { checks: 0, installs: 0, restarts: 0, closes: 0, releases: [] };
  const update = {
    version: "0.18.3", body: "## Fixes\n- Improved workspace startup.",
    close: async () => { calls.closes++; },
    downloadAndInstall: async progress => { calls.installs++; progress({ event: "Started", data: { contentLength: 100 } }); progress({ event: "Progress", data: { chunkLength: 100 } }); progress({ event: "Finished" }); },
  };
  const controller = new AppUpdateController({
    version: "0.18.2", getVersion: async () => "0.18.2",
    check: async () => { calls.checks++; return update; },
    loadReleaseBody: async version => { calls.releases.push(version); return `## Release ${version}\n- A real release description.`; },
    relaunch: async () => { calls.restarts++; },
    storage: { getItem: key => saved.get(key) ?? null, setItem: (key, value) => saved.set(key, value) },
    ...overrides,
  });
  return { controller, calls, update, saved };
}

test("startup checks once and never installs without an explicit action", async () => {
  const { controller, calls } = fixture();
  await Promise.all([controller.initialize(), controller.initialize()]);
  assert.equal(calls.checks, 1);
  assert.equal(calls.installs, 0);
  assert.equal(controller.state.status, "available");
  assert.equal(controller.state.notes, null);
  controller.dispose();
});

test("concurrent checks share the native request and close replaced resources", async () => {
  const { controller, calls, update } = fixture();
  const request = deferred();
  let checks = 0;
  const concurrent = fixture({ check: () => { checks++; return request.promise; } }).controller;
  const first = concurrent.check();
  assert.equal(concurrent.check(true), first);
  await turn();
  assert.equal(checks, 1);
  request.resolve(update);
  await first;
  concurrent.dispose();
  await controller.initialize();
  await controller.check();
  // The adapter reused the same resource, so it must not be closed under itself.
  assert.equal(calls.closes, 1); // concurrent's disposal only
  controller.dispose();
});

test("first launch loads the installed release, not the newer available release", async () => {
  const { controller, calls, saved } = fixture({}, new Map());
  await controller.initialize(); await turn();
  assert.deepEqual(calls.releases, ["0.18.2"]);
  assert.equal(controller.state.notes.version, "0.18.2");
  assert.match(controller.state.notes.url, /\/tag\/v0\.18\.2$/);
  assert.equal(saved.has(releaseNotesSeenKey), false);
  controller.dismissNotes();
  assert.equal(saved.get(releaseNotesSeenKey), "0.18.2");
  controller.dispose();
  const reopened = fixture({}, saved);
  await reopened.controller.initialize();
  assert.equal(reopened.controller.state.notes, null);
  assert.deepEqual(reopened.calls.releases, []);
  reopened.controller.dispose();
});

test("manual installers and newer versions still trigger their own patch notes", async () => {
  const { controller, saved } = fixture({ getVersion: async () => "0.18.4" });
  saved.set(releaseNotesCacheKey, JSON.stringify({ version: "0.18.3", body: "Old cached release" }));
  await controller.initialize(); await turn();
  assert.equal(controller.state.currentVersion, "0.18.4");
  assert.match(controller.state.notes.body, /0\.18\.4/);
  assert.doesNotMatch(controller.state.notes.body, /Old cached/);
  controller.dispose();
});

test("installed release cache works offline but a future cache is never shown", async () => {
  for (const cachedVersion of ["0.18.2", "0.18.3"]) {
    const saved = new Map([[releaseNotesCacheKey, JSON.stringify({ version: cachedVersion, body: "Cached patch notes" })]]);
    const { controller } = fixture({ loadReleaseBody: async () => { throw new Error("offline"); } }, saved);
    await controller.initialize(); await turn();
    assert.equal(controller.state.notes.body, cachedVersion === "0.18.2" ? "Cached patch notes" : "");
    controller.dismissNotes();
    assert.equal(saved.get(releaseNotesSeenKey), cachedVersion === "0.18.2" ? "0.18.2" : undefined);
    controller.dispose();
  }
});

test("closing while notes load prevents a late response from reopening the dialog", async () => {
  const request = deferred();
  const { controller, saved } = fixture({ loadReleaseBody: () => request.promise }, new Map());
  await controller.initialize();
  assert.equal(controller.state.notes.loading, true);
  controller.dismissNotes();
  request.resolve("Late release notes"); await turn();
  assert.equal(controller.state.notes, null);
  assert.equal(saved.has(releaseNotesSeenKey), false);
  controller.dispose();
});

test("an offline placeholder can be retried and does not mark the notes as read", async () => {
  let offline = true;
  const { controller } = fixture({ loadReleaseBody: async () => { if (offline) throw new Error("offline"); return "## Reconnected"; } }, new Map());
  await controller.initialize(); await turn();
  assert.equal(controller.state.notes.error, "offline");
  offline = false;
  await controller.loadNotes();
  assert.equal(controller.state.notes.body, "## Reconnected");
  assert.equal(controller.state.notes.error, "");
  controller.dispose();
});

test("download completion is not installation completion; repeated install clicks do not duplicate work", async () => {
  const { controller, update, calls, saved } = fixture();
  const installed = deferred();
  update.downloadAndInstall = async progress => {
    calls.installs++;
    assert.equal(JSON.parse(saved.get(releaseNotesCacheKey)).version, update.version);
    progress({ event: "Started", data: { contentLength: 100 } });
    progress({ event: "Progress", data: { chunkLength: 150 } });
    progress({ event: "Finished" });
    await installed.promise;
  };
  await controller.initialize();
  const installing = controller.install();
  await controller.install(); await controller.check();
  assert.equal(calls.installs, 1);
  assert.equal(calls.checks, 1);
  assert.equal(controller.state.status, "downloading");
  assert.equal(controller.state.progress, 99);
  assert.equal(calls.restarts, 0);
  installed.resolve(); await installing;
  assert.equal(controller.state.status, "ready");
  assert.equal(controller.state.progress, 100);
  assert.equal(calls.restarts, 1);
  controller.dispose();
});

test("unknown download length stays indeterminate until installation succeeds", async () => {
  const { controller, update } = fixture();
  update.downloadAndInstall = async progress => {
    progress({ event: "Started", data: {} });
    progress({ event: "Progress", data: { chunkLength: 900 } });
    assert.equal(controller.state.progress, null);
  };
  await controller.initialize(); await controller.install();
  assert.equal(controller.state.progress, 100);
  controller.dispose();
});

test("signature or install failures never relaunch and can be retried", async () => {
  const { controller, update, calls } = fixture();
  update.downloadAndInstall = async () => { throw new Error("signature verification failed"); };
  await controller.initialize(); await controller.install();
  assert.equal(controller.state.status, "error");
  assert.equal(controller.state.errorOperation, "install");
  assert.match(controller.state.error, /signature/);
  assert.equal(calls.restarts, 0);
  update.downloadAndInstall = async () => {};
  await controller.install();
  assert.equal(controller.state.status, "ready");
  assert.equal(calls.restarts, 1);
  controller.dispose();
});

test("relaunch failure keeps the installed state and retries restart without downloading", async () => {
  let restarts = 0;
  const { controller, calls } = fixture({ relaunch: async () => { if (++restarts === 1) throw new Error("restart failed"); } });
  await controller.initialize(); await controller.install();
  assert.equal(controller.state.status, "ready");
  assert.equal(controller.state.errorOperation, "restart");
  await controller.restart();
  assert.equal(calls.installs, 1);
  assert.equal(restarts, 2);
  controller.dispose();
});

test("declining only hides the notice, retaining the update in settings", async () => {
  const { controller, calls } = fixture();
  await controller.initialize(); controller.dismissNotice(); await controller.check();
  assert.equal(controller.state.dismissedVersion, "0.18.3");
  assert.equal(controller.state.status, "available");
  assert.equal(calls.installs, 0);
  controller.dispose();
});

test("automatic offline checks stay quiet; a manual check exposes a recoverable error", async () => {
  const { controller } = fixture({ check: async () => { throw new Error("network unavailable"); } });
  await controller.initialize();
  assert.equal(controller.state.status, "error");
  assert.equal(controller.state.notify, false);
  await controller.check(true);
  assert.equal(controller.state.notify, true);
  controller.dispose();
});

test("storage denial and teardown do not break or resurrect the update flow", async () => {
  const request = deferred();
  const { controller, update, calls } = fixture({ check: () => request.promise,
    storage: { getItem: () => { throw new Error("denied"); }, setItem: () => { throw new Error("denied"); } },
  });
  const starting = controller.initialize(); await turn();
  controller.dismissNotes(); controller.dispose(); request.resolve(update); await starting;
  assert.equal(calls.closes, 1);
  assert.notEqual(controller.state.status, "available");
});

test("Orb and Workspace place the shared update banner after Reset, outside About", () => {
  for (const file of ["src/routes/+page.svelte", "src/lib/WorkspaceWindow.svelte"]) {
    const source = readFileSync(new URL(`../${file}`, import.meta.url), "utf8");
    assert.doesNotMatch(source, /settingsSections\.about|tr\("About(?: Lume)?",/);
    assert.match(source, /<AppUpdateNotice /);
    assert.ok(source.indexOf("<UpdateBanner ") > source.indexOf('tr("Reset", "Redefinir")'));
    assert.doesNotMatch(source, /from "@tauri-apps\/plugin-updater"/);
  }
  const runtime = readFileSync(new URL("../src/lib/appUpdateRuntime.ts", import.meta.url), "utf8");
  assert.match(runtime, /emitTo\("main", actionEvent/);
  assert.match(runtime, /releases\/tags\/v/);
  assert.doesNotMatch(runtime, /api\.github\.com[^\n]*\/latest/);
});
