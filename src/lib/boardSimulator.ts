import type { HubSession } from "$lib/hubProtocol";
import type { StepVisualState } from "$lib/workflowBoard";

/** What the board's simulator pretends one step is going through. */
export type DemoState = {
  state: StepVisualState;
  attention: "none" | "permission" | "question";
  /** Completed to-do items out of four, while the step runs. */
  progress: number;
};

export const demoTodoCount = 4;
const pending: DemoState = { state: "pending", attention: "none", progress: 0 };

/**
 * A scripted run over `count` steps: each one works through its plan, the middle ones stop to ask for
 * permission, the last asks a question, then everything completes and the last step fails once.
 */
export function demoFrames(count: number): DemoState[][] {
  const frames: DemoState[][] = [];
  const row = (cells: Array<DemoState | undefined>) => frames.push(Array.from({ length: count }, (_, index) => cells[index] ?? pending));
  for (let step = 0; step < count; step++) {
    const before = Array.from({ length: step }, () => ({ state: "completed", attention: "none", progress: demoTodoCount }) as DemoState);
    const running = (progress: number, attention: DemoState["attention"] = "none") => row([...before, { state: "running", attention, progress }]);
    running(0);
    running(1);
    running(2);
    if (step > 0 && step < count - 1) running(2, "permission");
    if (step === count - 1 && count > 1) running(3, "question");
    running(3);
    row([...before, { state: "completed", attention: "none", progress: demoTodoCount }]);
  }
  const done: DemoState = { state: "completed", attention: "none", progress: demoTodoCount };
  row(Array.from({ length: count }, () => done));
  row(Array.from({ length: count }, (_, index) => (index === count - 1 ? { state: "failed", attention: "none", progress: 2 } as DemoState : done)));
  return frames;
}

const todoLabels = ["Find the code", "Make the change", "Run the checks", "Summarize the result"];

/** A session that looks like the step's agent in the given state, for the cards to read. */
export function demoSession(base: HubSession | undefined, demo: DemoState, now: number): HubSession | undefined {
  if (!base) return base;
  const running = demo.state === "running";
  const doing = demo.progress >= 3
    ? { kind: "test", title: "Validation", detail: "npm run check", files: [] as string[] }
    : demo.progress === 2
      ? { kind: "command", title: "Search", detail: "rg -n \"nav\" src/lib", files: [] as string[] }
      : { kind: "file", title: "Editing", detail: "src/lib/Navigation.svelte", files: ["src/lib/Navigation.svelte", "src/routes/+layout.svelte"] };
  const activities = running ? [{ id: "demo-live", status: "running" as const, createdAt: now, ...doing }] : [];
  const items = todoLabels.map((label, index) => ({ label, status: index < demo.progress ? "completed" as const : index === demo.progress && running ? "in_progress" as const : "pending" as const }));
  return {
    ...base,
    status: running ? "running" : demo.state === "failed" ? "failed" : "waiting_for_input",
    statusLabel: running ? "Working" : demo.state === "completed" ? "Ready for your review" : base.statusLabel,
    activities,
    results: demo.state === "completed" ? [{ id: "demo-result", response: "Done", createdAt: now, files: ["src/lib/Navigation.svelte", "src/routes/+layout.svelte"], tests: [] }] : [],
    pendingPermission: demo.attention === "permission" ? { id: "demo-permission", kind: "command", summary: "Run the tests", resource: "npm test", risk: "low", requestedAt: new Date(now).toISOString() } : undefined,
    pendingQuestion: demo.attention === "question" ? { id: "demo-question", requestedAt: new Date(now).toISOString() } as unknown as HubSession["pendingQuestion"] : undefined,
    workSummary: running || demo.state === "completed" ? { todo: { updatedAt: now, items } } : {},
  } as HubSession;
}
