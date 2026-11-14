import type { AgentSession } from "$lib/domain";

/**
 * How often, in ms, the usage of a session is read again while its task runs; null when it is not read
 * on a timer. Claude is asked of the process that runs the task, which is cheap, so it follows closely.
 * Antigravity is read through its own stream. Codex is not polled: its app-server announces every change.
 */
export function usagePollInterval(agent: AgentSession["agent"], status: AgentSession["status"]): number | null {
  if (status !== "running") return null;
  if (agent === "claude_code") return 15_000;
  if (agent === "antigravity") return 30_000;
  return null;
}
