import type { AgentSession, SessionActivity } from "$lib/domain";

export type SessionShell = {
  id: string;
  /** What the shell is for, in the agent's words, or its command when it gave none. */
  title: string;
  command: string;
  status: "running" | "completed" | "failed" | "interrupted";
  startedAt: number;
};

/** How long a finished shell stays listed, so its end can still be seen. */
const FINISHED_VISIBLE_MS = 10 * 60_000;

/**
 * The shells an agent runs in the background (Claude Code's `run_in_background` commands), running ones first.
 * A session that ended has no running shells left, whatever its log last said.
 */
export function sessionShells(session: Pick<AgentSession, "status" | "activities">, now = Date.now()): SessionShell[] {
  const ended = session.status === "completed" || session.status === "failed";
  return session.activities
    .filter((activity: SessionActivity) => activity.kind === "shell")
    .map((activity): SessionShell => ({
      id: activity.id,
      title: activity.title,
      command: activity.detail ?? activity.title,
      status: (["running", "completed", "failed", "interrupted"].includes(activity.status) ? activity.status : "completed") as SessionShell["status"],
      startedAt: activity.createdAt,
    }))
    .filter((shell) => shell.status === "running" ? !ended : now - shell.startedAt < FINISHED_VISIBLE_MS)
    .sort((left, right) => Number(right.status === "running") - Number(left.status === "running") || right.startedAt - left.startedAt);
}

export function shellElapsed(shell: SessionShell, now = Date.now()): string {
  const seconds = Math.max(0, Math.floor((now - shell.startedAt) / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  return minutes < 60 ? `${minutes}m` : `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
}
