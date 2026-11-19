import type { AgentSession, PromptTokenUsage, SessionActivity } from "./domain";

/** The context limit a sample ran against: the one reported, or Claude's legacy fallback. Null when unknown. */
export function contextWindowFor(agent: AgentSession["agent"], sample: Pick<PromptTokenUsage, "contextTokens" | "contextWindow">): number | null {
  if (sample.contextWindow != null) return sample.contextWindow > 0 ? sample.contextWindow : null;
  // Preserve Claude's existing legacy fallback; Codex must report its effective limit.
  if (agent === "claude_code" && sample.contextTokens != null) return sample.contextTokens > 200_000 ? 1_000_000 : 200_000;
  return null;
}

/** How full the context was after one prompt, 0–100, or null when the agent does not say. */
export function sampleContextPercent(agent: AgentSession["agent"], sample: PromptTokenUsage): number | null {
  if (!["claude_code", "codex", "antigravity"].includes(agent) || !sample.contextTokens) return null;
  const window = contextWindowFor(agent, sample);
  if (!window || !Number.isFinite(sample.contextTokens)) return null;
  return Math.max(0, Math.min(100, Math.round(sample.contextTokens / window * 100)));
}

/**
 * Context is the last request's footprint, not the sum of tokens spent across a conversation. A sample taken
 * before the latest compaction no longer says anything: the context is unknown until the next answer.
 */
export function sessionContextUsage(
  session: Pick<AgentSession, "agent" | "promptTokenUsage"> & { activities?: Pick<SessionActivity, "kind" | "createdAt">[] } | null | undefined,
) {
  if (!session || !["claude_code", "codex", "antigravity"].includes(session.agent)) return null;
  const usage = session.promptTokenUsage?.findLast((sample) =>
    (sample.contextWindow ?? 0) > 0 || (sample.contextTokens ?? 0) > 0,
  );
  if (!usage || usage.contextTokens == null) return null;
  const compactedAt = (session.activities ?? []).reduce((latest, activity) =>
    activity.kind === "compact" ? Math.max(latest, activity.createdAt) : latest, 0);
  if (compactedAt > usage.createdAt) return null;
  const tokens = usage.contextTokens;
  const window = contextWindowFor(session.agent, usage) ?? 0;
  if (!Number.isFinite(window) || window <= 0 || !Number.isFinite(tokens) || tokens < 0) return null;
  const percent = Math.min(100, Math.round(tokens / window * 100));
  return { tokens, window, percent, level: percent >= 92 ? "danger" : percent >= 75 ? "warn" : "ok" };
}
