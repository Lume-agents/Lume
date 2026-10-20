export type PermissionTone = "normal" | "auto" | "danger";

type Translate = (english: string, portuguese: string) => string;

/**
 * What each permission mode is called. Claude Code and Codex share the names
 * people know: Normal, Approve for me and Full access.
 */
export function permissionLabel(mode: string, translate: Translate, agent?: string): string {
  if (agent === "omp") {
    switch (mode) {
      case "full_access": return translate("Full access", "Acesso total");
      case "workspace_write": return translate("Asks before running", "Pergunta antes de executar");
      case "custom": return translate("Asks before changing", "Pergunta antes de alterar");
      default: return mode;
    }
  }
  switch (mode) {
    case "default": return translate("Normal", "Normal");
    case "acceptEdits": return translate("Accept edits", "Aceitar edições");
    case "plan": return translate("Plan", "Planejar");
    case "agy_default": return translate("Antigravity default", "Padrão do Antigravity");
    case "agy_accept_edits": return translate("Accept edits", "Aceitar edições");
    case "agy_plan": return translate("Plan", "Planejar");
    case "agy_allow_all": return translate("Allow all tools", "Permitir todas as ferramentas");
    case "auto":
    case "auto_review": return translate("Approve for me", "Aprovar por mim");
    case "bypassPermissions":
    case "full_access": return translate("Full access", "Acesso total");
    case "read_only": return translate("Read only", "Somente leitura");
    case "dontAsk": return translate("Don't ask", "Não perguntar");
    default: return mode;
  }
}

export function permissionDescription(mode: string, translate: Translate, agent?: string): string {
  if (agent === "omp") {
    switch (mode) {
      case "full_access": return translate("Runs without approval prompts; commands have full access, not a sandbox.", "Executa sem pedir aprovação; os comandos têm acesso total e não ficam em sandbox.");
      case "workspace_write": return translate("Asks before running commands; approving a command gives it full access.", "Pergunta antes de executar comandos; aprovar um comando dá acesso total.");
      case "custom": return translate("Asks before changes; approved commands still have full access.", "Pergunta antes de alterações; comandos aprovados ainda têm acesso total.");
      default: return "";
    }
  }
  switch (mode) {
    case "default": return translate("Asks before edits and commands", "Pergunta antes de editar e de executar comandos");
    case "acceptEdits": return translate("Edits files freely, still asks for commands", "Edita arquivos sem perguntar e ainda pergunta pelos comandos");
    case "plan": return translate("Plans only, changes nothing", "Só planeja, não altera nada");
    case "agy_default": return translate("Uses Antigravity CLI rules; interactive approvals are denied in Lume's headless chat", "Usa as regras da CLI; pedidos de aprovação interativa são negados no chat headless do Lume");
    case "agy_accept_edits": return translate("Accepts file edits; commands still follow Antigravity rules", "Aceita edições de arquivos; comandos continuam seguindo as regras do Antigravity");
    case "agy_plan": return translate("Plans with read-only tools and makes no changes", "Planeja com ferramentas somente de leitura e não altera arquivos");
    case "agy_allow_all": return translate("Automatically approves every tool, including commands and file edits", "Aprova automaticamente todas as ferramentas, inclusive comandos e edições");
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
  if (mode === "bypassPermissions" || mode === "full_access" || mode === "agy_allow_all") return "danger";
  return mode === "auto" || mode === "auto_review" || mode === "agy_accept_edits" ? "auto" : "normal";
}
