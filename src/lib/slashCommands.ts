import type { AgentKind } from "$lib/domain";

export type SlashCommandAction =
  | "model"
  | "plan"
  | "default"
  | "interrupt"
  | "steer"
  | "rename"
  | "detach"
  | "fullscreen"
  | "zoom-in"
  | "zoom-out"
  | "clear"
  | "close";

/** A command as the agent itself reports it (see src-tauri/src/agent_commands.rs). */
export type AgentSlashCommand = {
  name: string;
  description: string;
  argumentHint?: string | null;
  /** `/` for commands, `$` for Codex skills. */
  prefix: string;
  kind: string;
};

export type SlashCommand = {
  name: string;
  description: string;
  source: "agent" | "lume";
  prefix: string;
  argumentHint?: string;
  action?: SlashCommandAction;
};

/**
 * Agents differ on what these do: Claude's `/clear` swaps the conversation under a new session id and
 * answers nothing, and Codex has no such command, so Lume starts the fresh conversation itself.
 */
const clearAliases = ["clear", "new", "reset"];

/** Lume owns model selection for these agents, so `/model` opens Lume's picker. */
const lumeModelAgents: AgentKind[] = ["codex", "claude_code", "opencode", "antigravity"];

export function agentSlashCommands(commands: AgentSlashCommand[], agent: AgentKind | undefined): SlashCommand[] {
  const lumeModel = agent !== undefined && lumeModelAgents.includes(agent);
  // Codex's Plan mode is switched by Lume itself, so its own `/plan` is not offered twice.
  const lumePlan = agent === "codex";
  const reported = commands
    .filter((command) => !(lumeModel && command.prefix === "/" && command.name === "model"))
    .filter((command) => !(lumePlan && command.prefix === "/" && command.name === "plan"))
    .filter((command) => !(command.prefix === "/" && clearAliases.includes(command.name)))
    .map((command): SlashCommand => ({
      name: command.name,
      description: command.description,
      source: "agent",
      prefix: command.prefix || "/",
      argumentHint: command.argumentHint || undefined,
    }));
  const owned: SlashCommand[] = [];
  if (lumeModel) {
    owned.push({
      name: "model",
      description: agent === "antigravity" ? "Choose the model and permissions" : "Choose the model and reasoning effort",
      source: "lume",
      prefix: "/",
      action: "model",
    });
  }
  owned.push({
    name: "clear",
    description: "Start a new conversation in this pane",
    source: "lume",
    prefix: "/",
    action: "clear",
  });
  if (lumePlan) {
    owned.push({ name: "plan", description: "Switch Plan mode on or off", source: "lume", prefix: "/", action: "plan" });
  }
  return [...owned, ...reported];
}

export function slashCommandQuery(prompt: string): { prefix: string; query: string } | null {
  const value = prompt.trimStart();
  const prefix = value[0];
  if ((prefix !== "/" && prefix !== "$") || /\s/.test(value)) return null;
  return { prefix, query: value.slice(1).toLowerCase() };
}

/** `/` browses every command; `$` narrows to Codex skills. */
export function filterSlashCommands(
  commands: SlashCommand[],
  query: { prefix: string; query: string } | null,
): SlashCommand[] {
  if (query === null) return [];
  return commands.filter((command) =>
    (query.prefix === "/" || command.prefix === query.prefix)
    && (
      !query.query
      || command.name.toLowerCase().includes(query.query)
      || command.description.toLowerCase().includes(query.query)
    )
  );
}

export function slashCommandText(command: SlashCommand): string {
  const text = `${command.prefix}${command.name}`;
  return command.prefix === "$" || command.argumentHint ? `${text} ` : text;
}

export function findSlashCommand(commands: SlashCommand[], prompt: string): SlashCommand | undefined {
  const value = prompt.trim().toLowerCase();
  const exact = commands.find((command) => `${command.prefix}${command.name}`.toLowerCase() === value);
  if (exact) return exact;
  return clearAliases.some((alias) => value === `/${alias}`)
    ? commands.find((command) => command.action === "clear")
    : undefined;
}

const requests = new Map<string, Promise<AgentSlashCommand[]>>();

/** One request per session; a failed one is retried the next time the menu opens. */
export function loadAgentSlashCommands(
  sessionId: string,
  list: (sessionId: string) => Promise<AgentSlashCommand[]>,
): Promise<AgentSlashCommand[]> {
  let request = requests.get(sessionId);
  if (!request) {
    request = list(sessionId).catch((error) => {
      requests.delete(sessionId);
      throw error;
    });
    requests.set(sessionId, request);
  }
  return request;
}
