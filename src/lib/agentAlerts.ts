import type { AgentSession, SessionActivity } from "$lib/domain";
import type { Language } from "$lib/i18n";

export type AgentAlertTone = "info" | "warning" | "error";

export type AgentAlert = {
  id: string;
  message: string;
  tone: AgentAlertTone;
  duration: number;
  occurredAt: number;
  priority: number;
};

const RECENT_FAILURE_WINDOW = 10 * 60 * 1_000;
const USAGE_WARNING_REMAINING = 20;

function tr(language: Language, english: string, portuguese: string) {
  return language === "pt-BR" ? portuguese : english;
}

function sessionName(session: AgentSession) {
  return session.sessionName?.trim() || session.project?.trim() || session.agentLabel;
}

function compact(value: string, max = 120) {
  const text = value.replace(/\s+/g, " ").trim();
  return text.length > max ? `${text.slice(0, max - 1).trimEnd()}…` : text;
}

function isMcpFailure(activity: SessionActivity) {
  return activity.status === "failed" && /(?:^|\b)mcp(?:\b|\s*[·:])/i.test(`${activity.title} ${activity.detail ?? ""}`);
}

function isUsageLimitFailure(activity: SessionActivity) {
  return activity.status === "failed"
    && /(?:rate|usage|token)\s+limit|quota|limit\s+(?:reached|exceeded)|limite\s+(?:de uso|atingido|excedido)/i.test(`${activity.title} ${activity.detail ?? ""}`);
}

function failedActivityMessage(session: AgentSession, activity: SessionActivity, language: Language) {
  const name = sessionName(session);
  const title = compact(activity.title.replace(/^MCP\s*[·:]\s*/i, ""), 82);
  if (isUsageLimitFailure(activity)) {
    return tr(language, `${name} reached the agent usage limit.`, `${name} atingiu o limite de uso do agente.`);
  }
  if (isMcpFailure(activity)) {
    return tr(language, `${name}: MCP failed — ${title}.`, `${name}: falha no MCP — ${title}.`);
  }
  const subject = activity.kind === "test"
    ? tr(language, "Validation failed", "Validação falhou")
    : activity.kind === "command"
      ? tr(language, "Command failed", "Comando falhou")
      : activity.kind === "subagent"
        ? tr(language, "Subagent failed", "Subagente falhou")
        : tr(language, "Agent action failed", "Ação do agente falhou");
  return `${name}: ${subject} — ${title}.`;
}

export function collectAgentAlerts(
  sessions: AgentSession[],
  language: Language,
  now = Date.now(),
): AgentAlert[] {
  const alerts = new Map<string, AgentAlert>();
  const add = (alert: AgentAlert) => {
    const current = alerts.get(alert.id);
    if (!current || alert.occurredAt > current.occurredAt) alerts.set(alert.id, alert);
  };

  for (const session of sessions) {
    const name = sessionName(session);

    for (const limit of session.rateLimits ?? []) {
      const used = Number(limit.usedPercent);
      if (!Number.isFinite(used)) continue;
      const remaining = Math.max(0, Math.min(100, Math.round(100 - used)));
      if (remaining > USAGE_WARNING_REMAINING) continue;
      const exhausted = remaining === 0;
      const windowLabel = limit.label.trim() || tr(language, "current window", "janela atual");
      add({
        id: `usage:${session.agent}:${limit.id}:${limit.resetsAt ?? "current"}:${exhausted ? "exhausted" : "low"}`,
        message: exhausted
          ? tr(language, `${session.agentLabel} reached the ${windowLabel} usage limit.`, `${session.agentLabel} atingiu o limite de uso de ${windowLabel}.`)
          : tr(language, `Only ${remaining}% of ${session.agentLabel}'s ${windowLabel} usage remains.`, `Restam apenas ${remaining}% do uso de ${windowLabel} do ${session.agentLabel}.`),
        tone: exhausted ? "error" : "warning",
        duration: exhausted ? 0 : 12_000,
        occurredAt: limit.resetsAt ?? now,
        priority: exhausted ? 100 : 70,
      });
    }

    if (session.pendingPermission) {
      add({
        id: `permission:${session.id}:${session.pendingPermission.id}`,
        message: tr(
          language,
          `${name} is waiting for permission: ${compact(session.pendingPermission.summary)}.`,
          `${name} está aguardando permissão: ${compact(session.pendingPermission.summary)}.`,
        ),
        tone: "warning",
        duration: 0,
        occurredAt: Date.parse(session.pendingPermission.requestedAt) || session.updatedAt,
        priority: 85,
      });
    }

    if (session.pendingQuestion) {
      add({
        id: `question:${session.id}:${session.pendingQuestion.id}`,
        message: tr(language, `${name} is waiting for your response.`, `${name} está aguardando sua resposta.`),
        tone: "warning",
        duration: 0,
        occurredAt: Date.parse(session.pendingQuestion.requestedAt) || session.updatedAt,
        priority: 80,
      });
    }

    for (const activity of session.activities.filter((activity) => activity.kind === "warning").slice(-12)) {
      add({
        id: `warning:${session.id}:${activity.id}`,
        message: compact(activity.detail || activity.title, 180),
        tone: "warning",
        duration: 7_000,
        occurredAt: activity.createdAt,
        priority: 78,
      });
    }

    const recentFailures = session.activities
      .filter((activity) => activity.status === "failed" && now - activity.createdAt <= RECENT_FAILURE_WINDOW)
      .slice(-3);
    for (const activity of recentFailures) {
      add({
        id: `activity:${session.id}:${activity.id}`,
        message: failedActivityMessage(session, activity, language),
        tone: "error",
        duration: isMcpFailure(activity) || isUsageLimitFailure(activity) ? 0 : 9_000,
        occurredAt: activity.createdAt,
        priority: isUsageLimitFailure(activity) ? 100 : isMcpFailure(activity) ? 95 : 75,
      });
    }

    const latestFailure = recentFailures.at(-1);
    if (session.status === "failed" && !latestFailure) {
      add({
        id: `session:${session.id}:failed:${session.statusLabel}`,
        message: tr(language, `${name} failed: ${compact(session.statusLabel)}.`, `${name} falhou: ${compact(session.statusLabel)}.`),
        tone: "error",
        duration: 0,
        occurredAt: session.updatedAt,
        priority: 90,
      });
    }
  }

  return [...alerts.values()].sort((left, right) => right.priority - left.priority || right.occurredAt - left.occurredAt);
}
