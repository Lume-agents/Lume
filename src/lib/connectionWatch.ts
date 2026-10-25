import { readable } from "svelte/store";
import type { PairedDevice } from "$lib/domain";
import { loadPairedDevices, loadRemoteLumeNodeHealth, loadRemoteLumeNodes } from "$lib/lume";

export interface ConnectionState { phone: boolean; computer: boolean }

/** A phone counts as connected while it keeps talking to the gateway (its app polls every few seconds). */
const PHONE_RECENT_MS = 90_000;
const REFRESH_MS = 15_000;

export function devicePlatform(name: string): "ios" | "android" {
  return /iphone|ipad|ipod|\bios\b|apple/i.test(name) ? "ios" : "android";
}

export function phoneConnected(devices: PairedDevice[], now = Date.now()) {
  return devices.some((device) => device.lastSeenAt !== undefined && now - device.lastSeenAt <= PHONE_RECENT_MS);
}

async function check(): Promise<ConnectionState> {
  const [phone, computer] = await Promise.all([
    loadPairedDevices().then((devices) => phoneConnected(devices)).catch(() => false),
    loadRemoteLumeNodes()
      .then((nodes) => Promise.all(nodes.map((node) => loadRemoteLumeNodeHealth(node.nodeId).then((health) => health.lifecycle === "running").catch(() => false))))
      .then((answers) => answers.some(Boolean))
      .catch(() => false),
  ]);
  return { phone, computer };
}

/** What is connected to this Lume right now, refreshed while the window is visible. */
export const connections = readable<ConnectionState>({ phone: false, computer: false }, (set) => {
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
