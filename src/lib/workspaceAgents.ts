import type { AgentSession } from "$lib/domain";

export type WorkspaceSubagent = {
  id: string;
  label: string;
  status: "running" | "completed" | "failed" | "waiting" | "interrupted";
  updatedAt: number;
};

/** Subagents remain attached to the parent chat; they are not extra sessions. */
export function subagentsForSession(session: Pick<AgentSession, "activities" | "status">): WorkspaceSubagent[] {
  const byId = new Map<string, WorkspaceSubagent>();
  for (const activity of session.activities) {
    if (activity.kind !== "subagent") continue;
    const agentPath = activity.title.replace(/^(?:Subagente|Subagent)\s*[·:]\s*/i, "").trim() || "Subagent";
    const label = agentPath.split(/[\\/]/).filter(Boolean).at(-1) || agentPath;
    // Claude hooks carry a stable agent id. Codex activities carry an agent path.
    const id = activity.id.includes(":subagent:") ? activity.id : `codex:${agentPath}`;
    const status = activity.status === "running" && session.status === "failed"
      ? "interrupted"
      : activity.status === "running" && !["running", "permission_required"].includes(session.status)
      ? "completed"
      : activity.status;
    if (!byId.has(id) || byId.get(id)!.updatedAt <= activity.createdAt) {
      byId.set(id, { id, label, status, updatedAt: activity.createdAt });
    }
  }
  return [...byId.values()]
    .sort((left, right) => right.updatedAt - left.updatedAt)
    .slice(0, 8);
}
