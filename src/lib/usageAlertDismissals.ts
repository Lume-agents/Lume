import { writable } from "svelte/store";

const STORAGE_KEY = "lume-dismissed-usage-alerts-v1";
const MAX_DISMISSALS = 200;
type DismissalHost = Pick<Window, "localStorage" | "addEventListener">;

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
