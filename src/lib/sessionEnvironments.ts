import { readable, writable } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";

export interface EnvironmentPort {
  port: number;
  address: string;
  url: string | null;
}

export interface SessionEnvironment {
  id: string;
  sessionId: string;
  name: string;
  kind: "web" | "database" | "service";
  processId: number;
  startedAt: number;
  observedAt: number;
  stoppedAt: number | null;
  status: "running" | "stopping" | "idle" | "stopped" | "unknown";
  ports: EnvironmentPort[];
  canStop: boolean;
}

export interface EnvironmentSnapshot {
  environments: SessionEnvironment[];
  error: string | null;
}

const empty: EnvironmentSnapshot = { environments: [], error: null };
let publish: ((snapshot: EnvironmentSnapshot) => void) | undefined;
let inFlight: Promise<void> | null = null;
let lastSnapshot = empty;

/** One poll for the Workspace, regardless of how many cards or chats subscribe. */
export const sessionEnvironments = readable<EnvironmentSnapshot>(empty, (set) => {
  if (typeof window === "undefined") return;
  publish = set;
  const refresh = () => {
    if (!document.hidden) void refreshSessionEnvironments();
  };
  refresh();
  const interval = window.setInterval(refresh, 4500);
  document.addEventListener("visibilitychange", refresh);
  return () => {
    window.clearInterval(interval);
    document.removeEventListener("visibilitychange", refresh);
    publish = undefined;
  };
});

export async function refreshSessionEnvironments(): Promise<void> {
  if (inFlight) return inFlight;
  inFlight = invoke<EnvironmentSnapshot>("get_session_environments")
    .then((snapshot) => {
      if (Array.isArray(snapshot?.environments)) {
        lastSnapshot = snapshot;
        publish?.(snapshot);
      }
    })
    // Background polling must not re-announce the same error on every pass.
    .catch((error) => {
      lastSnapshot = {
        environments: lastSnapshot.environments.map((item) => item.status === "stopped" ? item : { ...item, status: "unknown", canStop: false }),
        error: String(error),
      };
      publish?.(lastSnapshot);
    })
    .finally(() => { inFlight = null; });
  return inFlight;
}

export async function stopSessionEnvironment(sessionId: string, environmentId: string): Promise<void> {
  await invoke("stop_session_environment", { sessionId, environmentId });
  await refreshSessionEnvironments();
}

export function environmentsForSession(snapshot: EnvironmentSnapshot, sessionId: string): SessionEnvironment[] {
  return snapshot.environments.filter((environment) => environment.sessionId === sessionId);
}

export function runningEnvironments(environments: SessionEnvironment[]): SessionEnvironment[] {
  return environments.filter((environment) => environment.status === "running" || environment.status === "stopping");
}

export function environmentElapsed(environment: SessionEnvironment, now = Date.now()): string {
  const seconds = Math.max(0, Math.floor(((environment.stoppedAt ?? now) - environment.startedAt * 1000) / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m`;
  return `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
}

const DISMISSED_KEY = "lume-dismissed-environments-v1";
function loadDismissed(): string[] {
  try {
    const value = JSON.parse(localStorage.getItem(DISMISSED_KEY) ?? "[]");
    return Array.isArray(value) ? value.filter((item): item is string => typeof item === "string") : [];
  } catch { return []; }
}

/** The environments and shells the person hid from the list once they were no longer running. */
export const dismissedEnvironments = writable<string[]>(typeof localStorage === "undefined" ? [] : loadDismissed());

export function dismissEnvironments(ids: string[]) {
  dismissedEnvironments.update((current) => {
    const next = [...new Set([...current, ...ids])].slice(-300);
    try { localStorage.setItem(DISMISSED_KEY, JSON.stringify(next)); } catch { /* hidden for this window only */ }
    return next;
  });
}

/** What the list still shows: a hidden one comes back if it runs again, and a running one cannot be hidden. */
export function withoutDismissed<T extends { id: string; status: string }>(items: T[], dismissed: string[]): T[] {
  return items.filter((item) => item.status === "running" || item.status === "stopping" || !dismissed.includes(item.id));
}
