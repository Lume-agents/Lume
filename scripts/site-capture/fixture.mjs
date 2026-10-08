// Illustrative sessions for product captures. Nothing here runs an agent: the real Svelte
// components render this data through a stubbed Tauri IPC layer inside a headless browser.
const now = Date.now();
const minute = 60_000;
const agent = (id, kind, label, name, project, status, statusLabel, extra = {}) => ({
  id, agent: kind, agentLabel: label, sessionName: name, project, source: "desktop", controlOrigin: "lume",
  status, statusLabel, startedAt: new Date(now - 38 * minute).toISOString(), updatedAt: now - 20_000,
  nativeSessionId: "native-" + id, workingDirectory: "/home/dev/" + project.replace(/ /g, "-"),
  permissionProfile: { mode: "workspace_write", label: "Project access", approvalPolicy: "on-request", canRespondFromLume: true, availableActions: ["allow_once", "allow_session", "deny"] },
  results: [], activities: [], rateLimits: [], promptTokenUsage: [],
  capabilities: { canPrompt: true, canReadResults: true, canTakeControl: false, promptDeliveries: ["new_turn", "queue", "steer"] },
  workSummary: {}, activityTotal: 0, ...extra,
});
let counter = 0;
const act = (kind, offset, detail, extra = {}) => ({ id: "a" + ++counter, kind, title: kind === "message" ? "Response" : kind, detail, status: "completed", createdAt: now - offset, files: [], attachments: [], ...extra });

const navigationReply = `The navigation now adapts to smaller screens.

**What changed**

- Mobile menu with a clear open and close state
- Keyboard navigation and visible focus
- Escape closes the menu and returns focus

\`\`\`svelte
<button aria-expanded={isOpen} onclick={() => (isOpen = !isOpen)}>
  Menu
</button>
\`\`\`

The desktop layout is preserved. The changes are ready for your review.`;
const reviewReply = `**Clear at every size**

The mobile navigation follows the same structure as desktop. Labels are short and the current page stays visible.

**Keyboard behavior**

Focus returns to the menu trigger on close. Escape and tab navigation follow the expected order.

**Ready for your decision**

The two changed files are ready to inspect. No additional files need to change.`;

const workflowRoles = ["planner", "implementer", "reviewer"];
const roleContract = {
  planner: { instruction: "Break the goal into small steps and name the files involved.", expectedInput: "The objective and the repository.", producedOutput: "An ordered plan.", completionCondition: "The plan is clear enough to implement." },
  implementer: { instruction: "Implement the plan and keep the diff focused.", expectedInput: "The plan.", producedOutput: "The changed files and a summary.", completionCondition: "The change builds and the checks pass." },
  reviewer: { instruction: "Review the change for clarity, accessibility and regressions.", expectedInput: "The summary and the diff.", producedOutput: "A short review with a decision.", completionCondition: "Every finding is addressed or accepted." },
};
export function buildFixture({ language = "en", dark = true } = {}) {
  const sessions = [
    agent("s1", "codex", "Codex", "Responsive navigation", "Lume", "waiting_for_input", "Ready for your review", {
      results: [{ id: "r1", response: navigationReply, createdAt: now - 4 * minute, files: ["src/lib/Navigation.svelte", "src/routes/+layout.svelte"], tests: ["npm run check"] }],
      activities: [
        act("prompt", 9 * minute, "Make the navigation work on mobile. Keep the desktop layout, add keyboard support, and show me what changed."),
        act("command", 8 * minute, "rg -n \"nav\" src/lib", { title: "Search" }),
        act("file", 6 * minute, "diff --git a/src/lib/Navigation.svelte b/src/lib/Navigation.svelte\n--- a/src/lib/Navigation.svelte\n+++ b/src/lib/Navigation.svelte\n@@ -1,6 +1,22 @@\n-<nav class=\"menu\">\n+<nav class=\"menu\" data-open={isOpen}>\n+  <button aria-expanded={isOpen} onclick={() => (isOpen = !isOpen)}>Menu</button>\n+  <ul hidden={!isOpen}>\n+    <li><a href=\"/\">Home</a></li>\n+  </ul>\n </nav>\ndiff --git a/src/routes/+layout.svelte b/src/routes/+layout.svelte\n--- a/src/routes/+layout.svelte\n+++ b/src/routes/+layout.svelte\n@@ -3,2 +3,5 @@\n+<svelte:window onkeydown={closeOnEscape} />\n+<Navigation />\n", { title: "Edited", files: ["src/lib/Navigation.svelte", "src/routes/+layout.svelte"] }),
        act("test", 5 * minute, "npm run check", { title: "Validation" }),
        act("message", 4 * minute, navigationReply),
      ],
      rateLimits: [{ id: "five-hour", label: "5h", usedPercent: 24, windowMinutes: 300 }],
      workSummary: { todo: { updatedAt: now, items: [{ id: "t1", text: "Audit the current menu", status: "completed" }, { id: "t2", text: "Add the mobile menu", status: "completed" }, { id: "t3", text: "Verify keyboard focus", status: "completed" }] } },
      activityTotal: 5,
    }),
    agent("s2", "claude_code", "Claude Code", "Review navigation changes", "Lume", "waiting_for_input", "Ready for your review", {
      results: [{ id: "r2", response: reviewReply, createdAt: now - 3 * minute, files: ["src/lib/Navigation.svelte"], tests: [] }],
      activities: [
        act("prompt", 7 * minute, "Review the navigation update. Focus on clarity, keyboard behavior and mobile layout."),
        act("command", 6 * minute, "git diff --stat", { title: "Read context" }),
        act("message", 3 * minute, reviewReply),
      ],
      rateLimits: [{ id: "five-hour", label: "5h", usedPercent: 38, windowMinutes: 300 }],
      activityTotal: 3,
    }),
    agent("s3", "codex", "Codex", "API response handling", "Orbit API", "running", "Working on the task", { activities: [act("prompt", 3 * minute, "Handle rate-limit responses from the billing API with exponential backoff.")], activityTotal: 1 }),
    agent("s4", "antigravity", "Antigravity", "Migrate billing webhooks", "Orbit API", "waiting_for_input", "Ready for your review", { activities: [], results: [{ id: "r4", response: "The webhook handlers now verify signatures before parsing the payload.", createdAt: now - 14 * minute, files: ["api/webhooks.ts"], tests: [] }] }),
    agent("s5", "opencode", "OpenCode", "Refactor the parser", "Parser", "waiting_for_input", "Ready for your review", { results: [{ id: "r5", response: "The tokenizer is split into small pure functions with unit tests.", createdAt: now - 21 * minute, files: ["src/tokenizer.ts"], tests: [] }] }),
    agent("s6", "claude_code", "Claude Code", "Authentication flow", "Orbit API", "waiting_for_permission", "Needs your approval", {
      pendingPermission: { id: "p1", kind: "command", summary: "Run the authentication tests", resource: "npm test -- auth", risk: "low", requestedAt: new Date(now - 30_000).toISOString() },
    }),
  ];
  const stepFor = (session, index) => ({ id: "step-" + session.nativeSessionId, sessionNativeId: session.nativeSessionId, role: workflowRoles[index], customRoleLabel: "", attempt: 0, ...roleContract[workflowRoles[index]] });
  const steps = [sessions[2], sessions[0], sessions[1]].map(stepFor);
  const connection = (from, to, extra = {}) => ({ id: "c-" + from.id + "-" + to.id, fromStepId: from.id, toStepId: to.id, includeResponse: true, includeFiles: true, includeTests: true, contextPolicy: "standard", contextSelection: { response: true, files: true, checks: true, plan: false, activity: false, diffs: false }, requiresApproval: false, advanceMode: "manual", additionalInstruction: "", ...extra });
  const group = { id: "demo-flow", terminalGroupId: "board-demo-flow", steps, connections: [connection(steps[0], steps[1], { advanceMode: "automatic" }), connection(steps[1], steps[2], { requiresApproval: true })] };
  return { sessions, group, language, dark };
}
