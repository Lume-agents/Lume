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

const effortNames: Record<string, [string, string]> = {
  none: ["None", "Nenhum"], minimal: ["Minimal", "Mínimo"], low: ["Low", "Baixo"], medium: ["Medium", "Médio"],
  high: ["High", "Alto"], xhigh: ["Extra high", "Muito alto"], max: ["Max", "Máximo"],
};

/** A reasoning effort as the person reads it ("xhigh" is "Muito alto"). */
export function effortName(value: string, translate: Translate): string {
  const name = effortNames[value.toLowerCase()];
  return name ? translate(name[0], name[1]) : value.charAt(0).toUpperCase() + value.slice(1);
}

/** What the chat says when the model or the reasoning effort changed; `detail` is `{ model?, effort? }` as JSON. */
export function modelChangeNoticeText(detail: string | undefined, translate: Translate): string {
  let change: { model?: string; effort?: string } = {};
  try { change = JSON.parse(detail ?? "{}") ?? {}; } catch { /* an old or odd record: say it plainly */ }
  const parts = [
    change.model ? `${translate("Model", "Modelo")}: ${change.model}` : "",
    change.effort ? `${translate("Effort", "Esforço")}: ${effortName(change.effort, translate)}` : "",
  ].filter(Boolean);
  return parts.length ? parts.join(" · ") : translate("Model changed", "Modelo alterado");
}

export function compactNoticeText(translate: Translate): string {
  return translate("Context compacted", "Contexto compactado");
}
