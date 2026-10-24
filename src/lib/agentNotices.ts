import type { SessionActivity } from "./domain";

export function isUsageLimitText(text: string): boolean {
  return /(?:rate|usage|token|session|weekly|daily|account)[_\s-]+limit|quota|limit[_\s-]+(?:reached|exceeded)|limite\s+(?:de uso|atingido|excedido)/i.test(text);
}

function isDirectUsageNotice(text: string): boolean {
  return /^\s*(?:you(?:['’]ve| have| ve)\s+(?:hit|reached|exceeded)\s+(?:your|the)\s+(?:(?:session|usage|weekly|daily|account)\s+)?limit\b|(?:usage|session|rate)[_\s-]+limit\s+(?:reached|exceeded)\b)/i.test(text);
}

/** Provider notices belong in the usage banner, including notices saved by older Lume versions. */
export function isUsageLimitActivity(activity: SessionActivity): boolean {
  if (["prompt", "queued_prompt", "codex_queued_prompt", "command", "test", "file"].includes(activity.kind)) return false;
  if (activity.kind === "tool") return isDirectUsageNotice(activity.title);
  if (activity.kind === "error" || activity.status === "failed") {
    return isUsageLimitText(`${activity.title}\n${activity.detail ?? ""}`);
  }
  return ["message", "warning"].includes(activity.kind) && isDirectUsageNotice(activity.detail || activity.title);
}
