import type { SessionActivity } from "$lib/domain";
import type { Language } from "$lib/i18n";
// Node executes this module directly in the focused presentation test; Vite resolves the same source in the app.
// @ts-expect-error TypeScript's bundler mode disallows the explicit source extension used by Node.
import { gitActivityInfo, gitEventTitle } from "./gitEvents.ts";

export type ActivityCategory = "edit" | "git" | "read" | "search" | "test" | "command" | "tool" | "plan";
export type ActivityThinkingState = "working" | "searching" | "solving" | "listening" | "connecting" | "weaving" | "composing" | "breathing" | "shaping";

function firstLine(value?: string): string {
  return String(value ?? "").split(/\n/, 1)[0].trim();
}

function normalizedToolTitle(value: string): string {
  return value
    .replace(/^functions\s*[·:]\s*/i, "")
    .replace(/^functions[.:/]/i, "")
    .trim();
}

export function activityCategory(activity: SessionActivity): ActivityCategory {
  const title = normalizedToolTitle(activity.title).toLowerCase();
  const detail = firstLine(activity.detail).toLowerCase();
  const searchable = `${title} ${detail}`;
  if (activity.kind === "plan" || /(?:^|[._/-])update_plan$/.test(title)) return "plan";
  if (activity.kind === "file" || /apply_patch|patch|edit(?:ed)?\s+file/.test(title)) return "edit";
  // Before test/search/read: `git grep` and `git diff` are git work first.
  if (gitActivityInfo(activity)) return "git";
  if (activity.kind === "test") return "test";
  if (/web.?search|search_query|pesquisa na web/.test(searchable)) return "search";
  if (/^\s*(?:cat|sed\s+-n|head|tail|bat|type|ls|stat)\b/.test(title)) return "read";
  if (/\b(?:rg|grep|find|fd)\b/.test(searchable) || /search|searched|buscar|procurar/.test(title)) return "search";
  if (/view_image|read|inspect|open file|imagem inspecionada/.test(title)) return "read";
  if (/^\s*(?:cat|sed\s+-n|head|tail|bat|type|ls|stat)\b/.test(title) || /^\s*(?:cat|sed\s+-n|head|tail|bat|type|ls|stat)\b/.test(detail)) return "read";
  if (
    activity.kind === "command"
    || /^(?:exec|exec_command|shell|terminal)$/.test(title)
    || /functions[.:/]exec/.test(activity.title.toLowerCase())
  ) {
    return /\b(?:test|check|lint|build|pytest|vitest|jest)\b/.test(detail) ? "test" : "command";
  }
  return "tool";
}

export function activityThinkingState(activity: SessionActivity): ActivityThinkingState {
  if (["prompt", "queued_prompt", "codex_queued_prompt"].includes(activity.kind)) return "listening";
  if (activity.kind === "message") return "composing";
  if (activity.kind === "analysis") return "breathing";
  switch (activityCategory(activity)) {
    case "edit": return "shaping";
    case "read": return "listening";
    case "search": return "searching";
    case "test": return "solving";
    case "command": return "working";
    case "git": return "working";
    case "plan": return "weaving";
    default: return "connecting";
  }
}

export function activityThinkingLabel(state: ActivityThinkingState, language: Language): string {
  const portuguese = language === "pt-BR";
  const labels: Record<ActivityThinkingState, [string, string]> = {
    working: ["Running…", "Executando…"],
    searching: ["Searching…", "Procurando…"],
    solving: ["Solving…", "Resolvendo…"],
    listening: ["Listening…", "Ouvindo…"],
    connecting: ["Connecting…", "Conectando…"],
    weaving: ["Planning…", "Planejando…"],
    composing: ["Composing…", "Escrevendo…"],
    breathing: ["Thinking…", "Pensando…"],
    shaping: ["Shaping…", "Editando…"],
  };
  return labels[state][portuguese ? 1 : 0];
}

export function isPresentableTraceActivity(activity: SessionActivity): boolean {
  if (["prompt", "message", "analysis", "interrupt", "queued_prompt", "codex_queued_prompt", "plan", "plan_document", "warning"].includes(activity.kind)) return false;
  const title = normalizedToolTitle(activity.title).toLowerCase();
  return !/^(?:create_goal|get_goal|update_goal|update_plan)$/.test(title);
}

export function isHiddenAgentActivity(activity: SessionActivity): boolean {
  if (["plan", "plan_document", "queued_prompt", "codex_queued_prompt", "warning"].includes(activity.kind)) return true;
  const title = normalizedToolTitle(activity.title).toLowerCase();
  return /^(?:create_goal|get_goal|update_goal|update_plan)$/.test(title);
}

export function isGenericAnalysisPlaceholder(activity: SessionActivity): boolean {
  if (activity.kind !== "analysis") return false;
  const title = normalizedToolTitle(activity.title)
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLowerCase();
  if (title === "analisando a solicitacao" || title === "analyzing request") return true;
  const detail = activity.detail?.trim();
  return (title === "analise concluida" || title === "analysis completed")
    && (!detail || ["[]", "{}", "null"].includes(detail));
}

export function needsUserAuthorization(text?: string): boolean {
  const normalized = String(text ?? "")
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLowerCase();
  const compact = normalized.replace(/\s+/g, " ").trim();
  const directAuthorizationQuestions = [
    /\bvoce\s+(?:me\s+)?autoriza\b[^?\n]{0,240}\?/,
    /\bdo\s+you\s+(?:authorize|allow|approve|permit)\b[^?\n]{0,240}\?/,
    /\b(?:may|can)\s+i\s+(?:proceed|continue|run|execute|change|modify|delete|access|open|send|install)\b[^?\n]{0,240}\?/,
    /\b(?:me\s+)?autorizas\b[^?\n]{0,240}\?/,
    /\b(?:puedo|puedo\s+yo)\s+(?:proceder|continuar|ejecutar|cambiar|modificar|eliminar|acceder|abrir|enviar|instalar)\b[^?\n]{0,240}\?/,
    /\b(?:m['’])?autorisez-vous\b[^?\n]{0,240}\?/,
    /\best-ce\s+que\s+vous\s+(?:m['’])?autorisez\b[^?\n]{0,240}\?/,
    /\b(?:erlauben|genehmigen)\s+sie\b[^?\n]{0,240}\?/,
    /\bmi\s+autorizzi\b[^?\n]{0,240}\?/,
    /\bposso\s+(?:procedere|continuare|eseguire|modificare|eliminare|accedere|aprire|inviare|installare)\b[^?\n]{0,240}\?/,
  ];
  const explicitAuthorizationRetries = [
    /(?:^|[.!?]\s+)(?:por favor[,\s]+)?autorize\s+(?:novamente|de novo)\b.{0,240}\b(?:para|pra)\b/,
    /(?:^|[.!?]\s+)(?:please\s+)?(?:authorize|approve|allow|grant)\b.{0,160}\b(?:again|once more)\b/,
    /(?:^|[.!?]\s+)(?:por favor[,\s]+)?(?:autoriza\s+de\s+nuevo|vuelve\s+a\s+autorizar)\b.{0,240}\bpara\b/,
    /(?:^|[.!?]\s+)(?:veuillez\s+)?autorisez\b.{0,120}\b(?:a\s+nouveau|de\s+nouveau)\b/,
    /(?:^|[.!?]\s+)(?:bitte\s+)?(?:autorisieren|genehmigen)\b.{0,120}\berneut\b/,
    /(?:^|[.!?]\s+)(?:per\s+favore[,\s]+)?autorizza\s+(?:nuovamente|di\s+nuovo)\b/,
  ];
  return directAuthorizationQuestions.some((pattern) => pattern.test(normalized))
    || explicitAuthorizationRetries.some((pattern) => pattern.test(compact));
}

export function activityPreview(activity: SessionActivity): string {
  const line = firstLine(activity.detail)
    .replace(/^\{\s*"cmd"\s*:\s*"/i, "")
    .replace(/"\s*\}\s*$/, "");
  return line.length > 150 ? `${line.slice(0, 147)}…` : line;
}

export function activityDisplayTitle(activity: SessionActivity, language: Language): string {
  const pt = language === "pt-BR";
  const category = activityCategory(activity);
  if (category === "git") {
    const git = gitActivityInfo(activity);
    if (git) return gitEventTitle(git, language);
  }
  if (category === "edit") {
    const count = new Set(activity.files).size;
    if (count > 0) return pt ? `${count} arquivo${count === 1 ? " alterado" : "s alterados"}` : `${count} file${count === 1 ? " edited" : "s edited"}`;
    return pt ? "Arquivos alterados" : "Edited files";
  }
  if (category === "read") return pt ? "Contexto inspecionado" : "Inspected context";
  if (category === "search") return pt ? "Busca no projeto" : "Searched the project";
  if (category === "test") return pt ? "Validação executada" : "Ran a check";
  if (category === "command") return pt ? "Comando executado" : "Ran a command";
  const title = normalizedToolTitle(activity.title);
  return title || (pt ? "Ferramenta utilizada" : "Used a tool");
}

export type ActivityRun = { id: string; category: ActivityCategory | "analysis"; activities: SessionActivity[] };

function readCommandFiles(activity: SessionActivity): string[] {
  const title = normalizedToolTitle(activity.title);
  const command = /^(?:cat|sed|head|tail|bat|type)\s/i.test(title)
    ? title
    : /^(?:cat|sed|head|tail|bat|type)\s/i.test(firstLine(activity.detail))
      ? firstLine(activity.detail)
      : "";
  if (!command) {
    const detail = activity.detail?.trim() ?? "";
    return /^(?:read|open file)$/i.test(title) && /^[^\r\n{}<>|]+\.[\w-]+$/.test(detail)
      ? [detail]
      : [];
  }
  const tokens = [...command.matchAll(/"([^"]+)"|'([^']+)'|([^\s;|&<>]+)/g)]
    .map((match) => match[1] ?? match[2] ?? match[3]);
  return tokens.slice(1).filter((token) =>
    !token.startsWith("-")
    && !token.startsWith("$")
    && !/^\d+(?:,\d+)?p?$/.test(token)
    && /(?:[\\/]|\.[a-z\d-]{1,12}$)/i.test(token)
  );
}

export function activityRunFiles(run: ActivityRun): string[] {
  if (run.category !== "read" && run.category !== "edit") return [];
  const files = run.activities.flatMap((activity) =>
    activity.files.length ? activity.files : run.category === "read" ? readCommandFiles(activity) : [],
  );
  return [...new Set(files.filter((path) => path.length < 500 && !/[\r\n]/.test(path) && !path.includes("***")))];
}

export function groupConsecutiveTraceActivities(activities: SessionActivity[]): ActivityRun[] {
  const runs: ActivityRun[] = [];
  for (const activity of activities) {
    const category = activity.kind === "analysis" ? "analysis" : activityCategory(activity);
    // Each git operation is worth seeing on its own: a commit is not "2 commands".
    const groupable = !["analysis", "plan", "git"].includes(category)
      && !["permission", "question", "subagent"].includes(activity.kind)
      && activity.status !== "failed";
    const previous = runs.at(-1);
    if (groupable && previous?.category === category
      && previous.activities.every((item) => item.status !== "failed")
      && previous.activities.every((item) => !["permission", "question", "subagent"].includes(item.kind))) {
      previous.activities.push(activity);
    } else {
      runs.push({ id: activity.id, category, activities: [activity] });
    }
  }
  return runs;
}

export function activityRunTitle(run: ActivityRun, language: Language): string {
  const count = run.activities.length;
  const files = activityRunFiles(run);
  if (run.category === "read" && files.length) {
    return language === "pt-BR"
      ? `${files.length} arquivo${files.length === 1 ? " lido" : "s lidos"}`
      : `Read ${files.length} file${files.length === 1 ? "" : "s"}`;
  }
  if (count === 1) return activityDisplayTitle(run.activities[0], language);
  const pt = language === "pt-BR";
  if (run.category === "command") return pt ? `${count} comandos executados` : `${count} commands run`;
  if (run.category === "git") return pt ? `${count} operações git` : `${count} git operations`;
  if (run.category === "test") return pt ? `${count} validações executadas` : `${count} checks run`;
  if (run.category === "edit") {
    const total = files.length || count;
    return pt ? `${total} arquivo${total === 1 ? " alterado" : "s alterados"}` : `${total} file${total === 1 ? " edited" : "s edited"}`;
  }
  if (run.category === "read") return pt ? `${count} leituras de contexto` : `${count} context reads`;
  if (run.category === "search") return pt ? `${count} buscas no projeto` : `${count} project searches`;
  return pt ? `${count} ferramentas utilizadas` : `${count} tools used`;
}

function phrase(language: Language, category: ActivityCategory, count: number, fileCount: number): string {
  const pt = language === "pt-BR";
  if (category === "edit") {
    const total = fileCount || count;
    return pt ? `${total} arquivo${total === 1 ? " alterado" : "s alterados"}` : `${total} file${total === 1 ? " edited" : "s edited"}`;
  }
  if (category === "read") return pt ? "contexto lido" : "read context";
  if (category === "search") return pt ? "projeto pesquisado" : "searched the project";
  if (category === "test") return pt ? `${count} validaç${count === 1 ? "ão" : "ões"}` : `${count} check${count === 1 ? "" : "s"}`;
  if (category === "command") return pt ? `${count} comando${count === 1 ? "" : "s"}` : `${count} command${count === 1 ? "" : "s"}`;
  if (category === "git") return pt ? `${count} operaç${count === 1 ? "ão" : "ões"} git` : `${count} git operation${count === 1 ? "" : "s"}`;
  return pt ? `${count} ferramenta${count === 1 ? "" : "s"}` : `${count} tool${count === 1 ? "" : "s"}`;
}

export function activityGroupSummary(activities: SessionActivity[], language: Language): string {
  const counts = new Map<ActivityCategory, number>();
  const files = new Set<string>();
  for (const activity of activities) {
    const category = activityCategory(activity);
    if (category === "plan") continue;
    counts.set(category, (counts.get(category) ?? 0) + 1);
    if (category === "edit") activity.files.forEach((file) => files.add(file));
  }
  const order: ActivityCategory[] = ["edit", "git", "read", "search", "test", "command", "tool"];
  const parts = order.flatMap((category) => {
    const count = counts.get(category) ?? 0;
    return count ? [phrase(language, category, count, files.size)] : [];
  });
  if (parts.length === 0) return language === "pt-BR" ? "Atividade do agente" : "Agent activity";
  const text = parts.join(", ");
  return text.charAt(0).toUpperCase() + text.slice(1);
}

export function formatAgentDuration(durationMs: number): string {
  const totalSeconds = Math.max(0, Math.round(durationMs / 1_000));
  if (totalSeconds < 1) return "< 1s";
  const days = Math.floor(totalSeconds / 86_400);
  const hours = Math.floor((totalSeconds % 86_400) / 3_600);
  const minutes = Math.floor((totalSeconds % 3_600) / 60);
  const seconds = totalSeconds % 60;
  if (days > 0) return `${days}d ${hours}h`;
  if (hours > 0) return `${hours}h ${minutes}m`;
  if (minutes > 0) return `${minutes}m ${seconds}s`;
  return `${seconds}s`;
}
