export type PermissionTone = "normal" | "auto" | "danger";

type Translate = (english: string, portuguese: string) => string;

/**
 * What each permission mode is called. Claude Code and Codex share the names
 * people know: Normal, Approve for me and Full access.
 */
export function permissionLabel(mode: string, translate: Translate): string {
  switch (mode) {
    case "default": return translate("Normal", "Normal");
    case "acceptEdits": return translate("Accept edits", "Aceitar edições");
    case "plan": return translate("Plan", "Planejar");
    case "auto":
    case "auto_review": return translate("Approve for me", "Aprovar por mim");
    case "bypassPermissions":
    case "full_access": return translate("Full access", "Acesso total");
    case "read_only": return translate("Read only", "Somente leitura");
    case "dontAsk": return translate("Don't ask", "Não perguntar");
    default: return mode;
  }
}

export function permissionDescription(mode: string, translate: Translate): string {
  switch (mode) {
    case "default": return translate("Asks before edits and commands", "Pergunta antes de editar e de executar comandos");
    case "acceptEdits": return translate("Edits files freely, still asks for commands", "Edita arquivos sem perguntar e ainda pergunta pelos comandos");
    case "plan": return translate("Plans only, changes nothing", "Só planeja, não altera nada");
    case "auto": return translate("A classifier approves safe actions for you", "Um classificador aprova as ações seguras por você");
    case "auto_review": return translate("A reviewer approves safe requests for you", "Um revisor aprova os pedidos seguros por você");
    case "bypassPermissions":
    case "full_access": return translate("Never asks. Use only in a safe place", "Nunca pergunta. Use só em um ambiente seguro");
    case "read_only": return translate("Reads files, changes nothing", "Lê arquivos, não altera nada");
    case "dontAsk": return translate("Denies anything not already allowed", "Nega o que não foi permitido antes");
    default: return "";
  }
}

export function permissionTone(mode: string): PermissionTone {
  if (mode === "bypassPermissions" || mode === "full_access") return "danger";
  return mode === "auto" || mode === "auto_review" ? "auto" : "normal";
}
