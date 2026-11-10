import {
  getClaudeSessionModelSettings,
  getSessionModelSettings,
  setClaudeSessionModelSettings,
  setSessionModelSettings,
  type CodexModelOption,
} from "$lib/lume";

export type ModelControls = { models: CodexModelOption[]; model: string; effort: string };

/** The agents whose model and reasoning effort Lume can read and change. */
const AGENTS = ["codex", "claude_code", "opencode", "grok", "antigravity", "omp"];
export const supportsModelControls = (agent: string) => AGENTS.includes(agent);

/** The model and effort a conversation runs with, whichever agent it is. */
export async function loadModelControls(sessionId: string, agent: string): Promise<ModelControls> {
  if (agent === "claude_code") {
    const settings = await getClaudeSessionModelSettings(sessionId);
    return { models: settings.models ?? [], model: settings.model, effort: settings.reasoningEffort ?? "" };
  }
  const settings = await getSessionModelSettings(sessionId);
  const option = (settings.models ?? []).find((candidate) => candidate.model === settings.model);
  return {
    models: settings.models ?? [],
    model: settings.model,
    effort: settings.reasoningEffort ?? option?.defaultReasoningEffort ?? option?.supportedReasoningEfforts[0]?.value ?? "",
  };
}

/** Changes them, and returns what the agent then reports. */
export async function saveModelControls(sessionId: string, agent: string, model: string, effort: string): Promise<ModelControls> {
  if (agent === "claude_code") {
    const settings = await setClaudeSessionModelSettings(sessionId, model.trim() || undefined, effort || undefined);
    return { models: settings.models ?? [], model: settings.model, effort: settings.reasoningEffort ?? "" };
  }
  const settings = await setSessionModelSettings(sessionId, model, effort);
  const option = (settings.models ?? []).find((candidate) => candidate.model === settings.model);
  return { models: settings.models ?? [], model: settings.model, effort: settings.reasoningEffort ?? option?.defaultReasoningEffort ?? "" };
}
