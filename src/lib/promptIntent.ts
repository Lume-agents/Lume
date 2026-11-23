import type { AgentKind } from "$lib/domain";

/** What the person asked the agent to set up before working on a prompt: a to-do list, a goal or a plan. */
export type PromptIntent = "todo" | "goal" | "planning";

export const promptIntents: PromptIntent[] = ["todo", "goal", "planning"];

/**
 * Starts the request Lume appends to a prompt (protocol.rs LUME_INTENT_MARKER). The chat shows a chip instead,
 * and the line after it names the intent, which is how a goal is read back from the prompt.
 */
export const lumeIntentMarker = "Lume request attached to this prompt (the person chose it, follow it):";

export function promptIntentLabel(intent: PromptIntent, pt: boolean): string {
  return { todo: pt ? "Lista de tarefas" : "To-do list", goal: "Goal", planning: pt ? "Plano" : "Plan" }[intent];
}

/** What turning the option on does to the next prompt, for the menu's second line. */
export function promptIntentHint(intent: PromptIntent, pt: boolean): string {
  return {
    todo: pt ? "Lista de tarefas antes de começar" : "A to-do list before the work starts",
    goal: pt ? "Este prompt vira o objetivo do chat" : "This prompt becomes the chat's goal",
    planning: pt ? "Um plano, e espera sua aprovação" : "A plan, then waits for your approval",
  }[intent];
}

/** `/todo fix login` → the intent and the request without the command. */
export function parsePromptIntent(value: string): { intent: PromptIntent; body: string } | null {
  const match = /^\/(todo|goal|planning)(?:\s+([\s\S]*))?$/i.exec(value.trim());
  if (!match) return null;
  return { intent: match[1].toLowerCase() as PromptIntent, body: (match[2] ?? "").trim() };
}

function todoTool(agent: AgentKind | undefined): string {
  switch (agent) {
    case "claude_code": return "your TodoWrite tool";
    case "codex": return "your update_plan tool";
    case "opencode": return "your todowrite tool";
    case "antigravity": return "your task checklist artifact (task.md)";
    default: return "your to-do tool (or, without one, a Markdown checklist `- [ ] item` in your reply)";
  }
}

function instruction(intent: PromptIntent, agent: AgentKind | undefined): string {
  const tool = todoTool(agent);
  if (intent === "todo") {
    return `Before doing the work, break the request above into a to-do list with ${tool}, then carry it out, keeping each item's status current as you go.`;
  }
  if (intent === "goal") {
    const goal = agent === "codex"
      ? "Set the request above as this thread's goal with create_goal, break it into a to-do list with update_plan, and keep working until the goal is met; then mark it complete with update_goal."
      : `Treat the request above as the goal of this conversation: break it into a to-do list with ${tool}, keep it current, and keep working until every item is done.`;
    return goal;
  }
  const plan = agent === "codex"
    ? "with your update_plan tool"
    : agent === "antigravity"
      ? "in your implementation plan artifact (implementation_plan.md)"
      : `with ${tool}`;
  return `Do not change anything yet. Investigate what you need, write a step-by-step plan for the request above ${plan}, then stop and wait for my approval before implementing it.`;
}

/** The text sent to the agent: the person's words, then Lume's request, which the chat hides. */
export function promptWithIntent(body: string, intent: PromptIntent, agent: AgentKind | undefined): string {
  return `${body}\n\n${lumeIntentMarker}\nintent: ${intent}\n${instruction(intent, agent)}`;
}

/** The intent a sent prompt carried, read back from its text. */
export function promptIntentOf(value?: string): PromptIntent | null {
  const text = String(value ?? "");
  const at = text.indexOf(lumeIntentMarker);
  if (at < 0) return null;
  const match = /^\s*intent:\s*(todo|goal|planning)\b/.exec(text.slice(at + lumeIntentMarker.length));
  return match ? (match[1] as PromptIntent) : null;
}
