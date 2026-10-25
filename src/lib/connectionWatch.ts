import { readable } from "svelte/store";
import type { PairedDevice } from "$lib/domain";
import { loadPairedDevices, loadRemoteLumeNodeHealth, loadRemoteLumeNodes } from "$lib/lume";

/** The names of what is connected right now: phones and other computers. */
export interface ConnectionState { phones: string[]; computers: string[] }

/** A phone counts as connected while it keeps talking to the gateway (its app polls every few seconds). */
const PHONE_RECENT_MS = 90_000;
const REFRESH_MS = 15_000;

export function devicePlatform(name: string): "ios" | "android" {
  return /iphone|ipad|ipod|\bios\b|apple/i.test(name) ? "ios" : "android";
}

export function connectedPhones(devices: PairedDevice[], now = Date.now()) {
  return devices
    .filter((device) => device.lastSeenAt !== undefined && now - device.lastSeenAt <= PHONE_RECENT_MS)
    .map((device) => device.name);
}

async function check(): Promise<ConnectionState> {
  const [phones, computers] = await Promise.all([
    loadPairedDevices().then((devices) => connectedPhones(devices)).catch(() => [] as string[]),
    loadRemoteLumeNodes()
      .then((nodes) => Promise.all(nodes.map((node) => loadRemoteLumeNodeHealth(node.nodeId)
        .then((health) => (health.lifecycle === "running" ? health.displayName || node.nodeId : null))
        .catch(() => null))))
      .then((names) => names.filter((name): name is string => name !== null))
      .catch(() => [] as string[]),
  ]);
  return { phones, computers };
}

/** What is connected to this Lume right now, refreshed while the window is visible. */
export const connections = readable<ConnectionState>({ phones: [], computers: [] }, (set) => {
  let stopped = false;
  const refresh = async () => {
    if (document.hidden) return;
    const next = await check();
    if (!stopped) set(next);
  };
  void refresh();
  const timer = setInterval(() => void refresh(), REFRESH_MS);
  document.addEventListener("visibilitychange", refresh);
  return () => {
    stopped = true;
    clearInterval(timer);
    document.removeEventListener("visibilitychange", refresh);
  };
});
