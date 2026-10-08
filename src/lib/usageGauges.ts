import type { AgentKind, AgentRateLimit } from "$lib/domain";
import type { Language } from "$lib/i18n";

export interface UsageGauge {
  id: string;
  remaining: number;
  window: string;
  title: string;
}

export interface UsageGaugeGroup {
  /** Empty for agents whose limits are not split into groups. */
  label: string;
  gauges: UsageGauge[];
}

const GROUP_SEPARATOR = " · ";

/** How many limits the agent publishes, so loading placeholders match the final layout. */
export function expectedUsageLimitCount(agent: AgentKind) {
  if (agent === "antigravity") return 4;
  if (agent === "codex" || agent === "claude_code") return 2;
  return 0;
}

export function rateWindowLabel(windowMinutes?: number, fallback = "") {
  if (windowMinutes) {
    if (windowMinutes >= 1_440) return `${Math.round(windowMinutes / 1_440)}d`;
    if (windowMinutes >= 60) return `${Math.round(windowMinutes / 60)}h`;
    return `${windowMinutes}m`;
  }
  const window = fallback.split(GROUP_SEPARATOR).at(-1) ?? fallback;
  return window.match(/\b\d+\s*[dhm]\b/i)?.[0]?.replaceAll(" ", "") ?? window;
}

/** Splits `"<group> · <window>"` labels (Antigravity) into one row per group, keeping their order. */
export function usageGaugeGroups(limits: AgentRateLimit[], language: Language): UsageGaugeGroup[] {
  const portuguese = language === "pt-BR";
  const resetFormat = new Intl.DateTimeFormat(language, { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });
  const groups = new Map<string, UsageGaugeGroup>();
  for (const limit of limits) {
    const separator = limit.label.lastIndexOf(GROUP_SEPARATOR);
    const label = separator < 0 ? "" : limit.label.slice(0, separator);
    const remaining = Math.max(0, Math.min(100, Math.round(100 - Number(limit.usedPercent))));
    const title = [
      `${limit.label}: ${remaining}% ${portuguese ? "restante" : "remaining"}`,
      limit.resetsAt ? `${portuguese ? "reseta" : "resets"} ${resetFormat.format(new Date(limit.resetsAt))}` : "",
    ].filter(Boolean).join(" · ");
    const group = groups.get(label) ?? { label, gauges: [] };
    group.gauges.push({ id: limit.id, remaining, window: rateWindowLabel(limit.windowMinutes, limit.label), title });
    groups.set(label, group);
  }
  return [...groups.values()];
}
