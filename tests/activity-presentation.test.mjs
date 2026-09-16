import assert from "node:assert/strict";
import {
  activityCategory,
  activityGroupSummary,
  activityRunTitle,
  activityThinkingLabel,
  activityThinkingState,
  formatAgentDuration,
  groupConsecutiveTraceActivities,
  isGenericAnalysisPlaceholder,
  isHiddenAgentActivity,
  needsUserAuthorization,
} from "../src/lib/activityPresentation.ts";

const activity = (kind, title, detail = "", files = []) => ({
  id: `${kind}:${title}`,
  kind,
  title,
  detail,
  status: "completed",
  createdAt: 1,
  files,
  attachments: [],
  appendDetail: false,
});

const events = [
  activity("tool", "functions · exec", "rg -n workflow src"),
  activity("command", "Command", "cargo test --lib"),
  activity("file", "Files changed", "", ["src/main.rs", "src/lib.rs"]),
];

assert.equal(activityCategory(events[0]), "search");
assert.equal(activityCategory(events[1]), "test");
assert.equal(activityCategory(events[2]), "edit");
assert.equal(activityThinkingState(events[0]), "searching");
assert.equal(activityThinkingState(events[1]), "solving");
assert.equal(activityThinkingState(events[2]), "shaping");
assert.equal(activityThinkingState(activity("prompt", "User prompt")), "listening");
assert.equal(activityThinkingState(activity("message", "Agent response")), "composing");
assert.equal(activityThinkingState(activity("analysis", "Reasoning")), "breathing");
assert.equal(activityThinkingState(activity("command", "Command", "npm run dev")), "working");
assert.equal(activityThinkingState(activity("tool", "functions · notify", "ready")), "connecting");
assert.equal(activityThinkingState(activity("plan", "Plan updated")), "weaving");
assert.equal(activityThinkingLabel("solving", "en"), "Solving…");
assert.equal(activityThinkingLabel("solving", "pt-BR"), "Resolvendo…");
assert.equal(
  activityGroupSummary(events, "en"),
  "2 files edited, searched the project, 1 check",
);
assert.equal(
  activityGroupSummary(events, "pt-BR"),
  "2 arquivos alterados, projeto pesquisado, 1 validação",
);
const compactTrace = [
  { ...activity("file", "Files changed", "", ["src/main.rs"]), id: "file-1" },
  ...["echo first", "echo second", "echo third"].map((detail, index) => ({
    ...activity("command", "Command", detail), id: `command-${index}`,
  })),
  { ...activity("test", "Check", "cargo test"), id: "test-1" },
];
const compactRuns = groupConsecutiveTraceActivities(compactTrace);
assert.deepEqual(compactRuns.map((run) => run.activities.length), [1, 3, 1]);
assert.equal(activityRunTitle(compactRuns[1], "pt-BR"), "3 comandos executados");
assert.equal(activityRunTitle(compactRuns[1], "en"), "3 commands run");
assert.deepEqual(groupConsecutiveTraceActivities([
  compactTrace[1],
  { ...compactTrace[2], status: "failed" },
  compactTrace[3],
]).map((run) => run.activities.length), [1, 1, 1]);
assert.equal(isHiddenAgentActivity(activity("tool", "functions · update_plan")), true);
assert.equal(isHiddenAgentActivity(activity("tool", "functions.get_goal")), true);
assert.equal(isGenericAnalysisPlaceholder({ ...activity("analysis", "Analisando a solicitação"), status: "running" }), true);
assert.equal(isGenericAnalysisPlaceholder(activity("analysis", "Analisando a solicitação")), true);
assert.equal(isGenericAnalysisPlaceholder(activity("analysis", "Análise concluída")), false);
assert.equal(formatAgentDuration(0), "< 1s");
assert.equal(formatAgentDuration(42_400), "42s");
assert.equal(formatAgentDuration(134_000), "2m 14s");
assert.equal(formatAgentDuration(7_440_000), "2h 4m");
assert.equal(needsUserAuthorization("Você autoriza que eu continue?"), true);
assert.equal(needsUserAuthorization("voce me autoriza a executar o comando?"), true);
assert.equal(needsUserAuthorization("Do you authorize me to run this command?"), true);
assert.equal(needsUserAuthorization("May I proceed with the installation?"), true);
assert.equal(needsUserAuthorization("¿Me autorizas a eliminar este archivo?"), true);
assert.equal(needsUserAuthorization("Est-ce que vous m’autorisez à continuer ?"), true);
assert.equal(needsUserAuthorization("Erlauben Sie mir, diesen Befehl auszuführen?"), true);
assert.equal(needsUserAuthorization("Mi autorizzi a procedere?"), true);
assert.equal(needsUserAuthorization(
  "O reinício não foi executado porque a autorização do Docker expirou nas duas tentativas. As APIs permanecem no estado anterior.\n\nAutorize novamente o acesso ao daemon Docker para eu concluir o reinício.",
), true);
assert.equal(needsUserAuthorization("Please authorize access to the Docker daemon again so I can finish the restart."), true);
assert.equal(needsUserAuthorization("Autoriza de nuevo el acceso al daemon para que pueda completar el reinicio."), true);
assert.equal(needsUserAuthorization("A documentação usa a frase 'você autoriza' como exemplo."), false);
assert.equal(needsUserAuthorization("The documentation explains what 'do you authorize' means."), false);
assert.equal(needsUserAuthorization("Você autoriza esta alteração."), false);
assert.equal(needsUserAuthorization("A autorização foi concluída."), false);
assert.equal(needsUserAuthorization("A autorização expirou, mas nenhuma nova tentativa é necessária."), false);
assert.equal(needsUserAuthorization("O guia explica como autorizar novamente o Docker para desenvolvimento."), false);

console.log("activity presentation test suite passed");
