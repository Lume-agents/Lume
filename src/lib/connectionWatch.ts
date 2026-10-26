import { readable } from "svelte/store";
import type { PairedDevice } from "$lib/domain";
import { loadPairedDevices, loadRelayLumeNodeHealth, loadRelayLumeNodes, loadRemoteLumeNodeHealth, loadRemoteLumeNodes } from "$lib/lume";

/** The names of what is connected right now: phones and other computers. */
export interface ConnectionState { phones: string[]; computers: string[] }

/** A phone counts as connected while it keeps talking to the gateway (its app polls every few seconds). */
const PHONE_RECENT_MS = 90_000;
const REFRESH_MS = 15_000;
/** A check through the Relay opens an encrypted session on the hosted service, so it runs less often. */
const RELAY_REFRESH_MS = 60_000;

export function devicePlatform(name: string): "ios" | "android" {
  return /iphone|ipad|ipod|\bios\b|apple/i.test(name) ? "ios" : "android";
}

export function connectedPhones(devices: PairedDevice[], now = Date.now()) {
  return devices
    .filter((device) => device.lastSeenAt !== undefined && now - device.lastSeenAt <= PHONE_RECENT_MS)
    .map((device) => device.name);
}

type Computer = { nodeId: string; name: string };

async function answering(nodes: { nodeId: string }[], health: (nodeId: string) => Promise<{ lifecycle: string; displayName: string }>): Promise<Computer[]> {
  const answers = await Promise.all(nodes.map((node) => health(node.nodeId)
    .then((reply) => (reply.lifecycle === "running" ? { nodeId: node.nodeId, name: reply.displayName || node.nodeId } : null))
    .catch(() => null)));
  return answers.filter((computer): computer is Computer => computer !== null);
}

let relayComputers: Computer[] = [];
let relayCheckedAt = 0;

async function check(): Promise<ConnectionState> {
  const relayDue = Date.now() - relayCheckedAt >= RELAY_REFRESH_MS;
  const [phones, lan, relay] = await Promise.all([
    loadPairedDevices().then((devices) => connectedPhones(devices)).catch(() => [] as string[]),
    loadRemoteLumeNodes().then((nodes) => answering(nodes, loadRemoteLumeNodeHealth)).catch(() => [] as Computer[]),
    relayDue
      ? loadRelayLumeNodes().then((nodes) => answering(nodes, loadRelayLumeNodeHealth)).catch(() => [] as Computer[])
      : Promise.resolve(relayComputers),
  ]);
  if (relayDue) { relayComputers = relay; relayCheckedAt = Date.now(); }
  // A computer paired both on the network and through the Relay is listed once.
  const computers = [...lan, ...relay.filter((computer) => !lan.some((item) => item.nodeId === computer.nodeId))];
  return { phones, computers: computers.map((computer) => computer.name) };
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
