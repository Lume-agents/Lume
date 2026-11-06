export type UsageResetEvent = {
  type?: string;
  eventId?: string;
  occurredAt?: number;
  agent?: string;
  windowLabel?: string;
  remainingPercent?: number;
};

const names: Record<string, string> = {
  codex: "Codex", claude_code: "Claude Code", antigravity: "Antigravity",
  gemini: "Gemini", opencode: "OpenCode", deepseek: "DeepSeek", chatgpt: "ChatGPT", claude: "Claude",
};

/** No conversations or account details are needed in a reset notification. */
export function usageResetNotice(event: UsageResetEvent, language: "en" | "pt-BR", now = Date.now()) {
  if (event.type !== "usage_reset" || !event.eventId || event.eventId.length > 128) return null;
  if (!Number.isFinite(event.occurredAt) || now - Number(event.occurredAt) > 300_000 || Number(event.occurredAt) - now > 120_000) return null;
  const agent = names[event.agent ?? ""] ?? (language === "pt-BR" ? "agente" : "agent");
  const window = event.windowLabel?.replace(/\s+/g, " ").trim().slice(0, 80) || (language === "pt-BR" ? "atual" : "current");
  const exhausted = event.remainingPercent === 0;
  return {
    id: event.eventId,
    title: "Lume",
    body: language === "pt-BR"
      ? exhausted ? `A janela ${window} do ${agent} reiniciou, mas o limite já está esgotado.` : `O limite ${window} do ${agent} foi renovado.`
      : exhausted ? `${agent}'s ${window} window reset, but its limit is already exhausted.` : `${agent}'s ${window} usage limit has reset.`,
  };
}
