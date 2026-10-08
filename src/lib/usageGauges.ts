import type { AgentKind, AgentRateLimit } from "$lib/domain";
import type { Language } from "$lib/i18n";

export type UsageGauge = {
  id: string;
  remaining: number;
  window: string;
  title: string;
};

export type UsageGaugeGroup = {
  /** Empty for agents whose limits are not split into groups. */
  label: string;
  gauges: UsageGauge[];
};

const GROUP_SEPARATOR = " · ";
const SIDE_BY_SIDE_GAUGES = 2;

function tr(language: Language, english: string, portuguese: string) {
  return language === "pt-BR" ? portuguese : english;
}

/** How many limits the agent publishes, so loading placeholders match the final layout; 0 means no usage. */
export function expectedUsageLimitCount(agent: AgentKind) {
  if (agent === "antigravity") return 4;
  if (agent === "codex" || agent === "claude_code") return 2;
  return 0;
}

/** Past two gauges the token chart no longer fits beside them, so it moves below. */
export function stacksUsageGauges(agent: AgentKind, limitCount: number) {
  return Math.max(limitCount, expectedUsageLimitCount(agent)) > SIDE_BY_SIDE_GAUGES;
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

/** Splits `"<group> · <window>"` labels (Antigravity) into one row per group, shortest window first. */
export function usageGaugeGroups(limits: AgentRateLimit[], language: Language): UsageGaugeGroup[] {
  const resetFormat = new Intl.DateTimeFormat(language, { dateStyle: "short", timeStyle: "short" });
  const groups = new Map<string, UsageGaugeGroup & { minutes: number[] }>();
  for (const limit of limits) {
    const separator = limit.label.lastIndexOf(GROUP_SEPARATOR);
    const label = separator < 0 ? "" : limit.label.slice(0, separator);
    const window = rateWindowLabel(limit.windowMinutes, limit.label);
    const remaining = Math.max(0, Math.min(100, Math.round(100 - Number(limit.usedPercent))));
    const title = [
      `${label ? `${label}${GROUP_SEPARATOR}${window}` : limit.label}: ${remaining}% ${tr(language, "remaining", "restante")}`,
      limit.resetsAt ? `${tr(language, "resets", "reinicia")} ${resetFormat.format(new Date(limit.resetsAt))}` : "",
    ].filter(Boolean).join(GROUP_SEPARATOR);
    const group = groups.get(label) ?? { label, gauges: [], minutes: [] };
    // Antigravity lists the weekly bucket first; keep the 5h → weekly order Codex and Claude use.
    const minutes = limit.windowMinutes ?? Number.POSITIVE_INFINITY;
    const index = group.minutes.findIndex((existing) => existing > minutes);
    const at = index < 0 ? group.gauges.length : index;
    group.minutes.splice(at, 0, minutes);
    group.gauges.splice(at, 0, { id: limit.id, remaining, window, title });
    groups.set(label, group);
  }
  return [...groups.values()].map(({ label, gauges }) => ({ label, gauges }));
}
