import { writable } from "svelte/store";
import { getVersion } from "@tauri-apps/api/app";
import { emit, emitTo, listen } from "@tauri-apps/api/event";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { version } from "../../package.json";
import { AppUpdateController, initialUpdateState, type AppUpdateState } from "./appUpdates";

export const appUpdates = writable(initialUpdateState(version));
const stateEvent = "lume://app-updates-state";
const actionEvent = "lume://app-updates-action";
type UpdateAction = "snapshot" | "check" | "install" | "restart" | "dismiss_notice" | "dismiss_notes" | "retry_notes";
const native = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
let controller: AppUpdateController | null = null;

export async function loadReleaseBody(installedVersion: string): Promise<string> {
  const abort = new AbortController();
  const timer = setTimeout(() => abort.abort(), 12_000);
  try {
    const response = await fetch(`https://api.github.com/repos/Lume-agents/Lume/releases/tags/v${encodeURIComponent(installedVersion)}`, {
      headers: { Accept: "application/vnd.github+json" }, signal: abort.signal,
    });
    if (!response.ok) throw new Error(`GitHub: ${response.status}`);
    const release = await response.json();
    if (release.tag_name !== `v${installedVersion}` || typeof release.body !== "string") throw new Error("Invalid release description");
    return release.body;
  } finally { clearTimeout(timer); }
}

/** Only the main/Orb window checks, downloads and installs. Workspace is a peer. */
export async function startAppUpdatesOwner(): Promise<() => void> {
  if (!native()) return () => {};
  const owner = new AppUpdateController({ version, getVersion, loadReleaseBody, relaunch,
    check: () => check({ timeout: 15_000, headers: { "Cache-Control": "no-cache", Pragma: "no-cache" } }),
    storage: { getItem: key => localStorage.getItem(key), setItem: (key, value) => localStorage.setItem(key, value) },
  });
  controller = owner;
  const publish = (state: AppUpdateState) => { appUpdates.set(state); void emit(stateEvent, state).catch(() => undefined); };
  const unsubscribe = owner.subscribe(publish);
  let unlisten: () => void;
  try {
    unlisten = await listen<UpdateAction>(actionEvent, ({ payload }) => {
      if (payload === "snapshot") publish(owner.state);
      else void runAction(owner, payload);
    });
  } catch (error) {
    unsubscribe(); owner.dispose(); controller = null;
    throw error;
  }
  void owner.initialize();
  const timer = setInterval(() => void owner.check(), 6 * 60 * 60 * 1000);
  return () => {
    clearInterval(timer); unlisten(); unsubscribe(); owner.dispose();
    if (controller === owner) controller = null;
  };
}

export async function startAppUpdatesPeer(): Promise<() => void> {
  if (!native()) return () => {};
  const unlisten = await listen<AppUpdateState>(stateEvent, ({ payload }) => { appUpdates.set(payload); });
  try { await emitTo("main", actionEvent, "snapshot"); }
  catch (error) { unlisten(); throw error; }
  return unlisten;
}

function runAction(owner: AppUpdateController, action: UpdateAction): Promise<void> | void {
  switch (action) {
    case "check": return owner.check(true);
    case "install": return owner.install();
    case "restart": return owner.restart();
    case "dismiss_notice": return owner.dismissNotice();
    case "dismiss_notes": return owner.dismissNotes();
    case "retry_notes": return owner.loadNotes();
  }
}

export async function appUpdateAction(action: Exclude<UpdateAction, "snapshot">) {
  if (controller) return runAction(controller, action);
  if (native()) return emitTo("main", actionEvent, action);
}
