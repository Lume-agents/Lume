import type { SessionActivity, SessionResult } from "$lib/domain";
import type { Language } from "$lib/i18n";
import type { HubSession } from "$lib/hubProtocol";

/** Id prefix of the example chat; `$lib/lume` turns requests about it down, since no agent stands behind it. */
export const TOUR_DEMO_PREFIX = "tour-demo:";

export const isTourDemoId = (value: unknown): value is string => typeof value === "string" && value.startsWith(TOUR_DEMO_PREFIX);

/**
 * A finished conversation to show the Workspace tour on when no chat is open: a prompt, the steps the agent took
 * (a search, a skill, an instruction file, an edit with its diff, git and a check) and its final answer.
 */
export function tourDemoSession(language: Language, now = Date.now()): HubSession {
  const pt = language === "pt-BR";
  const minute = 60_000;
  const id = `${TOUR_DEMO_PREFIX}session`;
  const at = (minutes: number) => now - minutes * minute;
  const activity = (key: string, kind: SessionActivity["kind"], minutes: number, title: string, detail = "", files: string[] = []): SessionActivity => ({
    id: `claude:${id}:${key}`,
    kind,
    title,
    detail,
    status: "completed",
    createdAt: at(minutes),
    files,
  });
  const diff = [
    "diff --git a/src/lib/Navigation.svelte b/src/lib/Navigation.svelte",
    "--- a/src/lib/Navigation.svelte",
    "+++ b/src/lib/Navigation.svelte",
    "@@ -1,6 +1,9 @@",
    ' <nav class="menu">',
    "-  <ul>",
    "+  <button aria-expanded={isOpen} onclick={() => (isOpen = !isOpen)}>Menu</button>",
    "+  <ul hidden={!isOpen}>",
    "     <li><a href=\"/\">Home</a></li>",
    "+    <li><a href=\"/docs\">Docs</a></li>",
    "   </ul>",
    " </nav>",
  ].join("\n");
  const reply = pt
    ? `**Pronto: a navegação agora funciona no celular.**\n\nO menu abre e fecha com um botão, o layout do desktop foi mantido e o foco do teclado acompanha o menu.\n\n\`\`\`svelte\n<button aria-expanded={isOpen}>Menu</button>\n\`\`\`\n\nOs testes passaram. Este é um exemplo do tour: nenhum agente está por trás desta conversa.`
    : `**Done: the navigation now works on mobile.**\n\nThe menu opens and closes with a button, the desktop layout is kept and keyboard focus follows the menu.\n\n\`\`\`svelte\n<button aria-expanded={isOpen}>Menu</button>\n\`\`\`\n\nThe checks passed. This is an example for the tour: no agent is behind this conversation.`;
  const results: SessionResult[] = [{ id: `${id}:result`, response: reply, createdAt: at(1), files: ["src/lib/Navigation.svelte"], tests: ["npm run check"] }];
  return {
    id,
    agent: "claude_code",
    agentLabel: "Claude Code",
    sessionName: pt ? "Exemplo do tour" : "Tour example",
    project: "lume-app",
    source: "desktop",
    controlOrigin: "lume",
    status: "waiting_for_input",
    statusLabel: pt ? "Exemplo" : "Example",
    startedAt: new Date(at(8)).toISOString(),
    updatedAt: now,
    workingDirectory: "/home/you/lume-app",
    permissionProfile: { mode: "workspace_write", label: "Workspace write", approvalPolicy: "on-request", canRespondFromLume: true, availableActions: ["allow_once", "deny"] },
    lastResponse: reply,
    results,
    activities: [
      activity("prompt", "prompt", 8, "Prompt", pt ? "Faça a navegação funcionar no celular. Mantenha o layout do desktop e me mostre o que mudou." : "Make the navigation work on mobile. Keep the desktop layout and show me what changed."),
      activity("instructions", "tool", 7.6, "Instructions", "/home/you/lume-app/CLAUDE.md", ["/home/you/lume-app/CLAUDE.md"]),
      activity("skill", "tool", 7.2, "Skill", "frontend-design"),
      activity("search", "command", 6.5, "rg -n nav src/lib", "rg -n nav src/lib"),
      activity("edit", "file", 5, "/home/you/lume-app/src/lib/Navigation.svelte", diff, ["/home/you/lume-app/src/lib/Navigation.svelte"]),
      activity("status", "command", 3.5, "git status --short", "git status --short"),
      activity("check", "test", 2.5, "npm run check", "npm run check"),
      activity("reply", "message", 1, "Resposta final", reply),
    ],
    capabilities: {
      canPrompt: true,
      canApprove: false,
      canAnswerQuestion: false,
      canTerminate: false,
      canOpenSource: false,
      canReadResults: true,
      canAttachImages: true,
      canInterrupt: false,
      canTakeControl: false,
      promptDeliveries: ["new_turn"],
    },
    workSummary: {},
    activityTotal: 8,
  };
}
