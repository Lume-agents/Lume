import type { SessionActivity, WorkflowConnectionDefinition, WorkflowRun, WorkflowStepDefinition } from "$lib/domain";

/** One stretch of a step's work: thinking (writing, planning), a tool call, or waiting on someone else. */
export type TimelineSpanKind = "thinking" | "tool" | "waiting";

export interface TimelineSpan {
  id: string;
  rowId: string;
  kind: TimelineSpanKind;
  start: number;
  end: number;
  /** Still growing: the step is running and this is its latest stretch. */
  open: boolean;
  label: string;
  detail?: string;
  failed?: boolean;
  /** The activity the span shows, for the hover card. */
  activity?: SessionActivity;
  /** The row that handed its work to this span (the first span after a handoff). */
  receivedFrom?: string;
  /** The row this span's work went to (the last span before a handoff). */
  handedTo?: string;
}

export interface TimelineRow {
  id: string;
  stepId: string;
  status: WorkflowRun["steps"][number]["status"] | "pending";
  attempt: number;
  start?: number;
  end?: number;
  error?: string;
}

export interface TimelineHandoff {
  id: string;
  from: string;
  to: string;
  at: number;
  /** The next step only starts when the person approves. */
  approval: boolean;
  /** The work has been handed over (the target started); otherwise it is waiting. */
  done: boolean;
}

export interface WorkflowTimelineModel {
  start: number;
  end: number;
  rows: TimelineRow[];
  spans: TimelineSpan[];
  handoffs: TimelineHandoff[];
  live: boolean;
  /** Total time the steps spent working, without the waits between them. */
  workMs: number;
  retries: number;
  failed: number;
}

const toolKinds = new Set(["command", "file", "test", "tool", "subagent", "activity"]);
const thinkingKinds = new Set(["message", "analysis", "plan", "plan_document"]);
const waitingKinds = new Set(["permission", "question"]);

function spanKind(activity: SessionActivity): TimelineSpanKind | null {
  if (waitingKinds.has(activity.kind)) return "waiting";
  if (toolKinds.has(activity.kind)) return "tool";
  if (thinkingKinds.has(activity.kind)) return "thinking";
  return null;
}

function firstLine(value: string | undefined) {
  return value?.split("\n").map((line) => line.trim()).find(Boolean) ?? "";
}

/**
 * The timeline of one workflow run: a row per step in chain order, spans from each step's own activity while it
 * ran, a waiting span where a step sat ready for its turn, and the handoffs between steps.
 * Activities carry only a start, so each span lasts until the next activity of the same step.
 */
export function buildWorkflowTimeline(input: {
  run: WorkflowRun | null;
  steps: WorkflowStepDefinition[];
  order: string[];
  connections: WorkflowConnectionDefinition[];
  activitiesFor: (step: WorkflowStepDefinition) => SessionActivity[];
  now: number;
}): WorkflowTimelineModel | null {
  const { run, steps, connections, now } = input;
  if (!run) return null;
  const byId = new Map(steps.map((step) => [step.id, step]));
  const order = [...input.order.filter((id) => byId.has(id)), ...steps.map((step) => step.id).filter((id) => !input.order.includes(id))];
  const live = ["running", "waiting_for_approval", "ready", "paused"].includes(run.status);
  const rows: TimelineRow[] = order.map((stepId) => {
    const state = run.steps.find((item) => item.stepId === stepId);
    return { id: `row:${stepId}`, stepId, status: state?.status ?? "pending", attempt: state?.attempt ?? 0, start: state?.startedAt, end: state?.completedAt, error: state?.error };
  });
  const started = rows.map((row) => row.start).filter((value): value is number => value !== undefined);
  const start = Math.min(run.createdAt, ...started);
  const ends = rows.map((row) => row.end ?? (row.status === "running" ? now : row.start)).filter((value): value is number => value !== undefined);
  const end = Math.max(start + 1_000, live ? now : Math.max(run.updatedAt, ...ends));

  const spans: TimelineSpan[] = [];
  let workMs = 0;
  for (const row of rows) {
    const step = byId.get(row.stepId);
    if (!step || row.start === undefined) continue;
    const rowEnd = row.end ?? (row.status === "running" ? now : row.start);
    workMs += Math.max(0, rowEnd - row.start);
    const activities = input.activitiesFor(step)
      .filter((activity) => activity.createdAt >= row.start! - 1_000 && activity.createdAt <= rowEnd)
      .map((activity) => ({ activity, kind: spanKind(activity) }))
      .filter((item): item is { activity: SessionActivity; kind: TimelineSpanKind } => item.kind !== null)
      .sort((left, right) => left.activity.createdAt - right.activity.createdAt);
    // Before the first recorded activity the agent was reading the handoff and thinking.
    const firstAt = activities[0]?.activity.createdAt ?? rowEnd;
    if (firstAt - row.start > 400) {
      spans.push({ id: `${row.id}:lead`, rowId: row.id, kind: "thinking", start: row.start, end: Math.min(firstAt, rowEnd), open: !activities.length && row.status === "running", label: "" });
    }
    activities.forEach(({ activity, kind }, index) => {
      const next = activities[index + 1]?.activity.createdAt ?? rowEnd;
      const spanStart = Math.max(row.start!, activity.createdAt);
      const last = index === activities.length - 1;
      spans.push({
        id: `${row.id}:${activity.id}`,
        rowId: row.id,
        kind,
        start: spanStart,
        end: Math.max(spanStart, Math.min(next, rowEnd)),
        open: last && row.status === "running",
        label: firstLine(activity.title) || activity.kind,
        detail: kind === "thinking" ? undefined : firstLine(activity.detail),
        failed: activity.status === "failed",
        activity,
      });
    });
    if (row.status === "failed" && spans.length && spans[spans.length - 1].rowId === row.id) spans[spans.length - 1].failed = true;
  }

  const handoffs: TimelineHandoff[] = [];
  for (const connection of connections) {
    const source = rows.find((row) => row.stepId === connection.fromStepId);
    const target = rows.find((row) => row.stepId === connection.toStepId);
    if (!source?.end || !target || source.status !== "completed") continue;
    const approval = !(connection.advanceMode === "automatic" && !connection.requiresApproval);
    const done = target.start !== undefined && target.start >= source.end - 1_000;
    handoffs.push({ id: connection.id, from: source.id, to: target.id, at: source.end, approval, done });
    const lastOfSource = [...spans].reverse().find((span) => span.rowId === source.id && span.kind !== "waiting");
    if (lastOfSource) lastOfSource.handedTo = target.id;
    const firstOfTarget = spans.find((span) => span.rowId === target.id && span.kind !== "waiting");
    if (done && firstOfTarget) firstOfTarget.receivedFrom = source.id;
    // The target sat ready while the handoff waited (for approval, or for the step to be picked up).
    const waitEnd = done ? target.start! : live ? now : source.end;
    if (waitEnd - source.end > 800) {
      spans.push({ id: `${target.id}:wait:${connection.id}`, rowId: target.id, kind: "waiting", start: source.end, end: waitEnd, open: !done && live, label: approval ? "approval" : "handoff" });
    }
  }

  return {
    start,
    end,
    rows,
    spans,
    handoffs,
    live,
    workMs,
    retries: rows.reduce((total, row) => total + Math.max(0, row.attempt - 1), 0),
    failed: rows.filter((row) => row.status === "failed").length,
  };
}

/** Round, readable tick marks for an axis of `duration` ms that fit about `target` labels. */
export function timelineTicks(duration: number, target = 5): number[] {
  const steps = [1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 900, 1800, 3600, 7200].map((seconds) => seconds * 1000);
  const step = steps.find((candidate) => duration / candidate <= target) ?? steps[steps.length - 1];
  const ticks: number[] = [];
  for (let at = 0; at <= duration + 1; at += step) ticks.push(at);
  return ticks;
}
