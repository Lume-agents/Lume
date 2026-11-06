import { writable } from "svelte/store";

const STORAGE_KEY = "lume-dismissed-usage-alerts-v1";
const MAX_DISMISSALS = 200;
type DismissalHost = Pick<Window, "localStorage" | "addEventListener">;

/** A provider can move the same reset by seconds, including across a rounding boundary. */
export function hasUsageAlertDismissal(ids: string[], candidate: string, now = Date.now()): boolean {
  if (ids.includes(candidate)) return true;
  const parse = (id: string) => /^usage:([^:]+):(.+):(current|\d+):(low|exhausted)$/.exec(id);
  const next = parse(candidate);
  if (!next || next[3] === "current") return false;
  const nextTime = Number(next[3]);
  return ids.some((id) => {
    const previous = parse(id);
    if (!previous || previous[1] !== next[1] || previous[2] !== next[2] || previous[4] !== next[4] || previous[3] === "current") return false;
    const value = Number(previous[3]);
    // Before this change, reset timestamps were stored as rounded hours. Retain those dismissals.
    const legacy = value < 100_000_000;
    const previousTime = legacy ? value * 3_600_000 : value;
    // Never suppress a later cycle once the previous reset has definitely passed.
    if (legacy ? now > previousTime + 1_800_000 : now >= previousTime && nextTime > previousTime) return false;
    if (!legacy && now < previousTime && nextTime > now) return true;
    return Math.abs(previousTime - nextTime) <= (legacy ? 1_920_000 : 120_000);
  });
}

export function createUsageAlertDismissals(host?: DismissalHost) {
  function read(): string[] {
    try {
      const value: unknown = JSON.parse(host?.localStorage.getItem(STORAGE_KEY) ?? "[]");
      return Array.isArray(value)
        ? value.filter((id): id is string => typeof id === "string" && id.startsWith("usage:")).slice(-MAX_DISMISSALS)
        : [];
    } catch {
      return [];
    }
  }

  const store = writable(read());
  host?.addEventListener("storage", (event) => {
    if (event.key === STORAGE_KEY || event.key === null) store.set(read());
  });

  return {
    subscribe: store.subscribe,
    dismiss(id: string) {
      if (!id.startsWith("usage:")) return;
      store.update((current) => {
        const next = [...new Set([...read(), ...current, id])].slice(-MAX_DISMISSALS);
        try {
          host?.localStorage.setItem(STORAGE_KEY, JSON.stringify(next));
        } catch {
          // Dismissal still applies to this window when storage is unavailable.
        }
        return next;
      });
    },
  };
}

export const usageAlertDismissals = createUsageAlertDismissals(typeof window === "undefined" ? undefined : window);
