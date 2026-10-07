export type ConnectableAgent = "claude" | "opencode" | "antigravity" | "deepseek" | "codex" | "gemini";

export function agentConnectionMessage(error: unknown): string | null {
  const message = String(error).replace(/^Error:\s*/, "");
  const marker = "AGENT_CONNECTION_REQUIRED:";
  if (message.includes(marker)) return message.slice(message.indexOf(marker) + marker.length).trim();
  if (/\b(auth_required|authentication required|requires authentication|not authenticated|not logged in|login required|please log in|please sign in|please authenticate|no credentials|invalid api key|api key missing)\b/i.test(message)
    || /(?:n[aã]o (?:est[aá] )?conectad[oa]|n[aã]o autenticad[oa]|fa[cç]a login|credenciais? (?:ausentes?|inv[aá]lid[oa]s?))/i.test(message)) {
    return message;
  }
  return null;
}

export function agentLoginCommand(agent: ConnectableAgent): string | null {
  if (agent === "claude") return "claude auth login";
  if (agent === "opencode") return "opencode auth login";
  return null;
}
