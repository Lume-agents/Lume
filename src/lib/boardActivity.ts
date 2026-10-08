import type { SessionActivity, WorkflowStepRun } from "$lib/domain";
import type { HubSession } from "$lib/hubProtocol";
import type { Language } from "$lib/i18n";
import { activityCategory, activityDisplayTitle, activityPreview, activityThinkingLabel, activityThinkingState, formatAgentDuration } from "$lib/activityPresentation";

/** What a workflow card shows about its agent, beyond the step's own status. */
export type StepActivity = {
  /** One line about what the agent is doing now, or the outcome of its last turn. */
  line: string | null;
  /** Time since the step started, or how long it took once finished. */
  elapsed: string | null;
  progress: { done: number; total: number } | null;
  attention: { kind: "permission" | "question"; text: string } | null;
};

// Prompts, answers and interruptions are conversation, not work in progress.
const conversation = new Set(["prompt", "queued_prompt", "codex_queued_prompt", "message", "interrupt", "warning"]);

const baseName = (path: string) => path.split(/[\\/]/).filter(Boolean).pop() ?? path;
const shorten = (text: string, max: number) => (text.length > max ? `${text.slice(0, max - 1).trimEnd()}…` : text);

function describe(activity: SessionActivity, language: Language): string {
  const pt = language === "pt-BR";
  const category = activityCategory(activity);
  if (category === "edit" && activity.files.length) {
    const first = baseName(activity.files[0]);
    const more = new Set(activity.files).size - 1;
    return `${pt ? "Editando" : "Editing"} ${shorten(first, 26)}${more > 0 ? ` +${more}` : ""}`;
  }
  if (category === "command" || category === "test") {
    const command = activityPreview(activity);
    if (command) return `$ ${shorten(command, 34)}`;
  }
  if (category === "edit" || category === "git" || category === "read" || category === "search" || category === "plan") return activityDisplayTitle(activity, language);
  return activityThinkingLabel(activityThinkingState(activity), language).replace(/…$/, "");
}

/** The agent's current work, taken from its most recent non-conversational activity. */
export function currentActivityLine(session: HubSession | undefined, language: Language): string | null {
  if (!session) return null;
  const working = session.activities.filter((activity) => !conversation.has(activity.kind));
  if (session.status === "running") {
    const live = [...working].reverse().find((activity) => activity.status === "running") ?? working[working.length - 1];
    if (live) return describe(live, language);
    return activityThinkingLabel("breathing", language).replace(/…$/, "");
  }
  const result = session.results[session.results.length - 1];
  const files = new Set(result?.files ?? []).size;
  if (files > 0) return language === "pt-BR" ? `${files} arquivo${files === 1 ? " alterado" : "s alterados"}` : `${files} file${files === 1 ? "" : "s"} changed`;
  return null;
}

export function stepProgress(session: HubSession | undefined): { done: number; total: number } | null {
  const items = session?.workSummary?.todo?.items?.length ? session.workSummary.todo.items : session?.workSummary?.plan?.items;
  if (!items?.length) return null;
  return { done: items.filter((item) => item.status === "completed").length, total: items.length };
}

export function stepElapsed(stepRun: WorkflowStepRun | undefined, now: number): string | null {
  if (!stepRun?.startedAt) return null;
  if (stepRun.status === "running") return formatAgentDuration(now - stepRun.startedAt);
  if (stepRun.completedAt && ["completed", "failed"].includes(stepRun.status)) return formatAgentDuration(stepRun.completedAt - stepRun.startedAt);
  return null;
}

export function stepActivity(session: HubSession | undefined, stepRun: WorkflowStepRun | undefined, language: Language, now: number): StepActivity {
  const pt = language === "pt-BR";
  const attention: StepActivity["attention"] = session?.pendingPermission
    ? { kind: "permission", text: session.pendingPermission.summary || (pt ? "Aguardando aprovação" : "Waiting for approval") }
    : session?.pendingQuestion
      ? { kind: "question", text: pt ? "O agente fez uma pergunta" : "The agent asked a question" }
      : null;
  return { line: attention ? null : currentActivityLine(session, language), elapsed: stepElapsed(stepRun, now), progress: stepProgress(session), attention };
}

export type FeedItem = {
  id: string;
  category: ReturnType<typeof activityCategory> | "message" | "question" | "permission";
  title: string;
  detail: string;
  status: SessionActivity["status"];
  at: number;
};

const feedHidden = new Set(["prompt", "queued_prompt", "codex_queued_prompt", "interrupt"]);

/** The agent's recent work, newest first, for the board's live panel. */
export function activityFeed(session: HubSession | undefined, language: Language, limit = 14): FeedItem[] {
  if (!session) return [];
  const pt = language === "pt-BR";
  return session.activities
    .filter((activity) => !feedHidden.has(activity.kind))
    .map((activity): FeedItem => {
      if (activity.kind === "message") {
        const text = (activity.detail ?? "").trim().replace(/\s+/g, " ");
        return { id: activity.id, category: "message", title: pt ? "Mensagem" : "Message", detail: shorten(text, 120), status: activity.status, at: activity.createdAt };
      }
      const category = activity.kind === "question" ? "question" : activity.kind === "permission" ? "permission" : activityCategory(activity);
      const files = [...new Set(activity.files)].map(baseName);
      const detail = category === "edit" && files.length
        ? shorten(files.slice(0, 2).join(", ") + (files.length > 2 ? ` +${files.length - 2}` : ""), 80)
        : category === "command" || category === "test" ? shorten(activityPreview(activity), 80) : "";
      return { id: activity.id, category, title: activityDisplayTitle(activity, language), detail, status: activity.status, at: activity.createdAt };
    })
    .sort((a, b) => b.at - a.at)
    .slice(0, limit);
}

/** The agent's latest answer, trimmed for a panel. */
export function lastResponse(session: HubSession | undefined, max = 700): string | null {
  const text = (session?.results[session.results.length - 1]?.response ?? session?.lastResponse ?? "").trim();
  return text ? shorten(text, max) : null;
}
