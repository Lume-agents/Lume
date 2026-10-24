import type { AgentSession } from "./domain";

/** Context is the last request's footprint, not the sum of tokens spent across a conversation. */
export function sessionContextUsage(session: Pick<AgentSession, "agent" | "promptTokenUsage"> | null | undefined) {
  if (!session || !["claude_code", "codex"].includes(session.agent)) return null;
  const usage = session.promptTokenUsage?.findLast((sample) =>
    (sample.contextWindow ?? 0) > 0 || (sample.contextTokens ?? 0) > 0,
  );
  if (!usage || usage.contextTokens == null) return null;
  const tokens = usage.contextTokens;
  // Preserve Claude's existing legacy fallback; Codex must report its effective limit.
  const window = usage.contextWindow
    ?? (session.agent === "claude_code" ? (tokens > 200_000 ? 1_000_000 : 200_000) : 0);
  if (!Number.isFinite(window) || window <= 0 || !Number.isFinite(tokens) || tokens < 0) return null;
  const percent = Math.min(100, Math.round(tokens / window * 100));
  return { tokens, window, percent, level: percent >= 92 ? "danger" : percent >= 75 ? "warn" : "ok" };
}
