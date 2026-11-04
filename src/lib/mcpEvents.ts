import type { SessionActivity } from "$lib/domain";
import type { Language } from "$lib/i18n";

/** A small label of an MCP event: the product it went to, a ticket's status, what a call changed. */
export type McpChip = { label: string; tone: "info" | "ok" | "warn" | "bad" | "muted"; title?: string };

/** What a call to an MCP tool did, read from the tool's name, its arguments and its answer (see `mcp_activity_detail`). */
export type McpEventInfo = {
  server: string;
  /** What the server is called in the chat; the Atlassian server is Jira or Confluence by the tool it was asked for. */
  serverLabel: string;
  tool: string;
  toolLabel: string;
  /** Which presenter read it: `jira:transition`, `jira:edit`… or `generic`. */
  presenter: string;
  input: Record<string, unknown>;
  result: unknown;
  /** The answer's text, with the MCP content blocks unwrapped. */
  resultText: string;
  error?: string;
  failed: boolean;
  running: boolean;
  /** A ticket key, a query, a page: what the call was about. */
  subject?: string;
  facts: Record<string, string>;
  chips: McpChip[];
  rows: Array<[string, string]>;
};

type Json = Record<string, unknown>;

const isObject = (value: unknown): value is Json => typeof value === "object" && value !== null && !Array.isArray(value);
const text = (value: unknown): string => (typeof value === "string" ? value.trim() : typeof value === "number" ? String(value) : "");

/** `mcp__server__tool`, as the agent names an MCP tool. */
export function parseMcpToolName(name: string): { server: string; tool: string } | null {
  const match = /^mcp__(.+?)__(.+)$/.exec(name.trim());
  return match ? { server: match[1], tool: match[2] } : null;
}

function humanize(value: string): string {
  const spaced = value
    .replace(/^claude_ai_/i, "")
    .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
    .replace(/[_-]+/g, " ")
    .trim();
  return spaced.charAt(0).toUpperCase() + spaced.slice(1);
}

function short(value: unknown, limit = 140): string {
  const raw = typeof value === "string" ? value : value === undefined ? "" : JSON.stringify(value);
  const line = raw.replace(/\s+/g, " ").trim();
  return line.length > limit ? `${line.slice(0, limit - 1)}…` : line;
}

/** The answer of an MCP tool as plain data: content blocks are joined and parsed when their text is JSON. */
function unwrapResult(result: unknown): { text: string; data: unknown } {
  let body = result;
  if (Array.isArray(result) && result.every((block) => isObject(block) && "type" in block)) {
    body = result.map((block) => (isObject(block) ? text(block.text) : "")).filter(Boolean).join("\n");
  } else if (isObject(result) && Array.isArray(result.content)) {
    return unwrapResult(result.content);
  }
  if (typeof body === "string") {
    const trimmed = body.trim();
    if (/^[\[{]/.test(trimmed)) {
      try { return { text: trimmed, data: JSON.parse(trimmed) }; } catch { /* Truncated or not JSON: keep the text. */ }
    }
    return { text: trimmed, data: trimmed };
  }
  return { text: short(body, 2_000), data: body };
}

function payloadOf(data: unknown): Json {
  if (isObject(data) && isObject(data.data)) return data.data;
  return isObject(data) ? data : {};
}

function statusTone(status: string): McpChip["tone"] {
  const value = status.normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase();
  if (/(done|closed|resolved|conclu|pronto|finaliz|fechad|entregue|released|merged)/.test(value)) return "ok";
  if (/(block|imped|cancel|reject|rejeit|failed|falh)/.test(value)) return "bad";
  if (/(progress|andamento|doing|review|revis|test|homolog|analis)/.test(value)) return "warn";
  return "info";
}

const issueKey = (input: Json) => text(input.issueIdOrKey) || text(input.issueKey) || text(input.issueId) || text(input.key);

type Presented = Pick<McpEventInfo, "presenter" | "subject" | "facts" | "chips" | "rows" | "serverLabel">;

/** An object argument reads as its name when it has one (`{ accountId }`, `{ name }`) instead of as JSON. */
function display(value: unknown): string {
  if (isObject(value)) {
    const named = text(value.displayName) || text(value.name) || text(value.value) || text(value.key) || text(value.accountId) || text(value.id);
    if (named) return named;
  }
  return short(value);
}

function entries(source: unknown, limit = 8): Array<[string, string]> {
  if (!isObject(source)) return [];
  return Object.entries(source)
    .filter(([, value]) => value !== undefined && value !== null && value !== "")
    .slice(0, limit)
    .map(([key, value]) => [humanize(key), display(value)]);
}

function atlassian(tool: string, input: Json, result: { text: string; data: unknown }, pt: boolean): Presented {
  const product = /confluence/i.test(tool) ? "Confluence" : /jira|issue/i.test(tool) ? "Jira" : "Atlassian";
  const data = payloadOf(result.data);
  const key = issueKey(input);
  const fields = isObject(input.fields) ? input.fields : {};
  const base = { serverLabel: product, facts: {} as Record<string, string>, chips: [] as McpChip[], rows: [] as Array<[string, string]> };
  const label = (portuguese: string, english: string) => (pt ? portuguese : english);

  if (tool === "transitionJiraIssue") {
    const status = text(data.statusName) || text(isObject(data.status) ? data.status.name : data.status);
    const chips: McpChip[] = status ? [{ label: status, tone: statusTone(status) }] : [];
    const rows: Array<[string, string]> = [[label("Ticket", "Issue"), key], [label("Novo status", "New status"), status]];
    if (data.transitionId !== undefined) rows.push([label("Transição", "Transition"), String(data.transitionId)]);
    return { ...base, presenter: "jira:transition", subject: key, facts: { key, status }, chips, rows: rows.filter(([, value]) => value) };
  }
  if (tool === "editJiraIssue") {
    const names = Object.keys(fields).map(humanize);
    return {
      ...base, presenter: "jira:edit", subject: key, facts: { key },
      chips: names.slice(0, 4).map((name) => ({ label: name, tone: "muted" as const })),
      rows: [[label("Ticket", "Issue"), key], ...entries(fields)].filter(([, value]) => value) as Array<[string, string]>,
    };
  }
  if (tool === "createJiraIssue") {
    const created = text(data.key) || key;
    const summary = text(input.summary) || text(fields.summary);
    const type = text(input.issueTypeName) || text(input.issueType) || (isObject(fields.issuetype) ? text(fields.issuetype.name) : "");
    const project = text(input.projectKey) || (isObject(fields.project) ? text(fields.project.key) : "");
    return {
      ...base, presenter: "jira:create", subject: created, facts: { key: created, summary },
      chips: [...(type ? [{ label: type, tone: "info" as const }] : []), ...(project ? [{ label: project, tone: "muted" as const }] : [])],
      rows: ([[label("Ticket", "Issue"), created], [label("Título", "Summary"), summary], [label("Tipo", "Type"), type], [label("Projeto", "Project"), project]] as Array<[string, string]>).filter(([, value]) => value),
    };
  }
  if (tool === "getJiraIssue") {
    const issueFields = isObject(data.fields) ? data.fields : {};
    const status = isObject(issueFields.status) ? text(issueFields.status.name) : "";
    const summary = text(issueFields.summary);
    return {
      ...base, presenter: "jira:get", subject: key, facts: { key, summary },
      chips: status ? [{ label: status, tone: statusTone(status) }] : [],
      rows: ([[label("Ticket", "Issue"), key], [label("Título", "Summary"), summary], [label("Status", "Status"), status]] as Array<[string, string]>).filter(([, value]) => value),
    };
  }
  if (tool === "searchJiraIssuesUsingJql") {
    const issues = Array.isArray(data.issues) ? data.issues.length : Array.isArray(result.data) ? result.data.length : undefined;
    const jql = text(input.jql);
    return {
      ...base, presenter: "jira:search", subject: jql, facts: { count: issues === undefined ? "" : String(issues), jql },
      chips: issues === undefined ? [] : [{ label: pt ? `${issues} ${issues === 1 ? "ticket" : "tickets"}` : `${issues} ${issues === 1 ? "issue" : "issues"}`, tone: "info" }],
      rows: ([["JQL", jql], [label("Máximo", "Max results"), text(input.maxResults)]] as Array<[string, string]>).filter(([, value]) => value),
    };
  }
  if (tool === "addOrEditJiraIssueComment" || tool === "listJiraIssueComments") {
    const comment = text(input.commentBody) || text(input.body) || text(input.comment);
    return {
      ...base, presenter: tool === "listJiraIssueComments" ? "jira:comments" : "jira:comment", subject: key, facts: { key },
      chips: [], rows: ([[label("Ticket", "Issue"), key], [label("Comentário", "Comment"), short(comment, 200)]] as Array<[string, string]>).filter(([, value]) => value),
    };
  }
  if (/Confluence/.test(tool)) {
    const title = text(input.title) || text(input.query) || text(input.cql) || text(input.pageId) || text(input.id);
    const action = /^search/i.test(tool) ? "search" : /^create/i.test(tool) ? "create" : /^update/i.test(tool) ? "update" : "get";
    return { ...base, presenter: `confluence:${action}`, subject: title, facts: { title }, chips: [], rows: entries(input, 6) };
  }
  return { ...base, presenter: "generic", subject: undefined, rows: entries(input) };
}

function generic(input: Json): Presented {
  const first = Object.values(input).find((value) => typeof value === "string" && value.length < 90);
  return { presenter: "generic", serverLabel: "", subject: typeof first === "string" ? first : undefined, facts: {}, chips: [], rows: entries(input) };
}

/** What an activity says about an MCP call, or null when it is not one. */
export function mcpActivityInfo(activity: SessionActivity, language: Language = "en"): McpEventInfo | null {
  const named = parseMcpToolName(activity.title);
  if (!named) return null;
  let detail: Json = {};
  try {
    const parsed = JSON.parse(activity.detail ?? "{}");
    if (isObject(parsed)) detail = parsed;
  } catch { /* The detail was cut short; the call is still known by its name. */ }
  const input = isObject(detail.input) ? detail.input : {};
  const answer = unwrapResult(detail.result);
  const error = text(detail.error) || undefined;
  const failed = activity.status === "failed" || Boolean(error);
  const pt = language === "pt-BR";
  const presented = /atlassian/i.test(named.server) ? atlassian(named.tool, input, answer, pt) : generic(input);
  const serverLabel = presented.serverLabel || humanize(named.server);
  return {
    server: named.server, tool: named.tool, toolLabel: humanize(named.tool), input, result: detail.result,
    resultText: answer.text, error, failed, running: activity.status === "running", ...presented, serverLabel,
  };
}

/** The same server and tool: calls worth showing as one run. */
export function sameMcpTool(left: SessionActivity, right: SessionActivity): boolean {
  const a = parseMcpToolName(left.title);
  const b = parseMcpToolName(right.title);
  return Boolean(a && b && a.server === b.server && a.tool === b.tool);
}

/** The line that names one call, such as `IPG-253 → REVISÃO`. */
export function mcpEventTitle(info: McpEventInfo, language: Language): string {
  const pt = language === "pt-BR";
  const { key, status, summary, count, title } = info.facts;
  switch (info.presenter) {
    case "jira:transition": return key && status ? `${key} → ${status}` : key ? (pt ? `Moveu ${key}` : `Moved ${key}`) : (pt ? "Moveu um ticket" : "Moved an issue");
    case "jira:edit": return key ? (pt ? `Editou ${key}` : `Edited ${key}`) : (pt ? "Editou um ticket" : "Edited an issue");
    case "jira:create": return key ? (pt ? `Criou ${key}${summary ? ` · ${summary}` : ""}` : `Created ${key}${summary ? ` · ${summary}` : ""}`) : (pt ? "Criou um ticket" : "Created an issue");
    case "jira:get": return key ? (pt ? `Leu ${key}${summary ? ` · ${summary}` : ""}` : `Read ${key}${summary ? ` · ${summary}` : ""}`) : (pt ? "Leu um ticket" : "Read an issue");
    case "jira:search": return count ? (pt ? `Buscou tickets no Jira · ${count}` : `Searched Jira · ${count}`) : (pt ? "Buscou tickets no Jira" : "Searched Jira");
    case "jira:comment": return key ? (pt ? `Comentou em ${key}` : `Commented on ${key}`) : (pt ? "Comentou em um ticket" : "Commented on an issue");
    case "jira:comments": return key ? (pt ? `Leu os comentários de ${key}` : `Read the comments of ${key}`) : (pt ? "Leu comentários" : "Read comments");
    case "confluence:search": return pt ? `Buscou no Confluence${title ? ` · ${title}` : ""}` : `Searched Confluence${title ? ` · ${title}` : ""}`;
    case "confluence:create": return pt ? `Criou a página${title ? ` ${title}` : ""}` : `Created the page${title ? ` ${title}` : ""}`;
    case "confluence:update": return pt ? `Atualizou a página${title ? ` ${title}` : ""}` : `Updated the page${title ? ` ${title}` : ""}`;
    case "confluence:get": return pt ? `Leu a página${title ? ` ${title}` : ""}` : `Read the page${title ? ` ${title}` : ""}`;
    default: return info.subject ? `${info.toolLabel} · ${info.subject}` : info.toolLabel;
  }
}

/** One title for several calls of the same tool in a row, such as `23 tickets movidos`. */
export function mcpRunTitle(activities: SessionActivity[], language: Language): string {
  const pt = language === "pt-BR";
  const count = activities.length;
  const first = mcpActivityInfo(activities[0], language);
  const plural = (one: string, many: string) => (count === 1 ? one : many);
  switch (first?.presenter) {
    case "jira:transition": return pt ? `${count} ${plural("ticket movido", "tickets movidos")}` : `Moved ${count} ${plural("issue", "issues")}`;
    case "jira:edit": return pt ? `${count} ${plural("ticket editado", "tickets editados")}` : `Edited ${count} ${plural("issue", "issues")}`;
    case "jira:create": return pt ? `${count} ${plural("ticket criado", "tickets criados")}` : `Created ${count} ${plural("issue", "issues")}`;
    case "jira:get": return pt ? `${count} ${plural("ticket lido", "tickets lidos")}` : `Read ${count} ${plural("issue", "issues")}`;
    case "jira:search": return pt ? `${count} buscas no Jira` : `${count} Jira searches`;
    case "jira:comment": return pt ? `${count} comentários` : `${count} comments`;
    default: return pt ? `${count} chamadas · ${first?.toolLabel ?? "MCP"}` : `${count} calls · ${first?.toolLabel ?? "MCP"}`;
  }
}

/** The chips that summarize a run: how many tickets ended in each status. */
export function mcpRunChips(activities: SessionActivity[]): McpChip[] {
  const infos = activities.map((activity) => mcpActivityInfo(activity)).filter((info): info is McpEventInfo => Boolean(info));
  const counts = new Map<string, { count: number; tone: McpChip["tone"] }>();
  for (const info of infos) {
    const status = info.presenter === "jira:transition" ? info.facts.status : "";
    if (!status) continue;
    const entry = counts.get(status) ?? { count: 0, tone: statusTone(status) };
    entry.count += 1;
    counts.set(status, entry);
  }
  const chips: McpChip[] = [...counts].map(([label, { count, tone }]) => ({ label: `${label} ×${count}`, tone }));
  const failed = infos.filter((info) => info.failed).length;
  if (failed) chips.push({ label: `×${failed}`, tone: "bad", title: "failed" });
  return chips;
}
