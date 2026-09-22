import type { AgentSession } from "$lib/domain";

export type WorkspaceSubagent = {
  id: string;
  label: string;
  path: string;
  status: "running" | "completed" | "failed" | "waiting" | "interrupted";
  startedAt: number;
  updatedAt: number;
};

const COMPLETED_SUBAGENT_PROMPT_WINDOW = 5;
const lastInteraction = new Map<string, number>();

export function noteSubagentInteraction(id: string, at = Date.now()): void {
  lastInteraction.delete(id);
  lastInteraction.set(id, at);
  if (lastInteraction.size > 256) lastInteraction.delete(lastInteraction.keys().next().value!);
}

/** Subagents remain attached to the parent chat; they are not extra sessions. */
export function subagentsForSession(session: Pick<AgentSession, "activities" | "status"> & Partial<Pick<AgentSession, "results">>): WorkspaceSubagent[] {
  const byId = new Map<string, WorkspaceSubagent>();
  const laterPrompts = session.activities
    .filter((activity) => activity.kind === "prompt")
    .map((activity) => activity.createdAt);
  for (const activity of session.activities) {
    if (activity.kind !== "subagent") continue;
    const agentPath = activity.title.replace(/^(?:Subagente|Subagent)\s*[·:]\s*/i, "").trim() || "Subagent";
    const label = agentPath.split(/[\\/]/).filter(Boolean).at(-1) || agentPath;
    // Both providers keep the same activity id across lifecycle updates. A new
    // invocation gets a new id even when it reuses the same role/path.
    const id = activity.id;
    const status = activity.status === "running" && session.status === "failed"
      ? "interrupted"
      : activity.status === "running" && !["running", "permission_required"].includes(session.status)
      ? "completed"
      : activity.status;
    const previous = byId.get(id);
    if (!previous || previous.updatedAt <= activity.createdAt) {
      byId.set(id, { id, label, path: agentPath, status, startedAt: previous?.startedAt ?? activity.createdAt, updatedAt: activity.createdAt });
    }
  }
  return [...byId.values()]
    .filter((agent) => {
      if (agent.status === "running" || agent.status === "waiting") return true;
      // The prompt that launched a child is not an idle prompt. Start the
      // countdown only after the parent turn has produced its result.
      const parentFinishedAt = session.results?.find((result) => result.createdAt >= agent.startedAt)?.createdAt;
      if (parentFinishedAt === undefined) return true;
      const idleSince = Math.max(parentFinishedAt, lastInteraction.get(agent.id) ?? 0);
      return laterPrompts.filter((createdAt) => createdAt > idleSince).length < COMPLETED_SUBAGENT_PROMPT_WINDOW;
    })
    .sort((left, right) => left.startedAt - right.startedAt || left.id.localeCompare(right.id))
    .slice(-64);
}

/** A Codex turn can stay open while its parent is only waiting for children. */
export function parentWaitingForSubagents(
  session: Pick<AgentSession, "activities" | "status">,
  children = subagentsForSession(session),
): boolean {
  if (session.status !== "running") return false;
  const activeChildren = children.filter((child) => child.status === "running");
  if (!activeChildren.length) return false;
  const newestChildStart = Math.max(...activeChildren.map((child) => child.startedAt));
  return !session.activities.some((activity) =>
    activity.kind !== "subagent"
    && activity.status === "running"
    && activity.createdAt > newestChildStart
    && !/\b(?:wait_agent|aguardando comando|collabAgentToolCall)\b/i.test(activity.title)
  );
}
