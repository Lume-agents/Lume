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
