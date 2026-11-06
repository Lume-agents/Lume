type Translate = (english: string, portuguese: string) => string;

/**
 * What the chat says when a request was cancelled. `tool_use` is the CLI's record of
 * cancelling while a tool was running; anything else is a plain cancel.
 */
export function interruptNoticeText(reason: string | undefined, translate: Translate): string {
  return reason === "tool_use"
    ? translate("Interrupted · the tool was cancelled", "Interrompido · a ferramenta foi cancelada")
    : translate("Interrupted by you", "Interrompido por você");
}

/** The connection to the agent dropped; Codex says so while it tries again. */
export function connectionNoticeText(detail: string | undefined, translate: Translate): string {
  return /reconnect|retry/i.test(detail ?? "")
    ? translate("Connection lost · reconnecting", "Conexão perdida · reconectando")
    : translate("Connection to the agent lost", "Conexão com o agente perdida");
}

export function compactNoticeText(translate: Translate): string {
  return translate("Context compacted", "Contexto compactado");
}
