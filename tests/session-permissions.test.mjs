import assert from "node:assert/strict";
import { test } from "node:test";
import { permissionDescription, permissionLabel, permissionTone } from "../src/lib/sessionPermissions.ts";

const pt = (_english, portuguese) => portuguese;
const en = (english) => english;

test("Claude Code and Codex name their modes the same way", () => {
  assert.equal(permissionLabel("default", en), "Normal");
  assert.equal(permissionLabel("auto", en), "Approve for me");
  assert.equal(permissionLabel("auto_review", en), "Approve for me");
  assert.equal(permissionLabel("bypassPermissions", en), "Full access");
  assert.equal(permissionLabel("full_access", en), "Full access");
  assert.equal(permissionLabel("auto_review", pt), "Aprovar por mim");
  assert.equal(permissionLabel("full_access", pt), "Acesso total");
  assert.equal(permissionLabel("read_only", pt), "Somente leitura");
});

test("a mode Lume does not list keeps its name instead of vanishing", () => {
  assert.equal(permissionLabel("someFutureMode", en), "someFutureMode");
  assert.equal(permissionDescription("someFutureMode", en), "");
});

test("only the modes that never ask are flagged as dangerous", () => {
  assert.equal(permissionTone("bypassPermissions"), "danger");
  assert.equal(permissionTone("full_access"), "danger");
  assert.equal(permissionTone("auto"), "auto");
  assert.equal(permissionTone("auto_review"), "auto");
  assert.equal(permissionTone("default"), "normal");
  assert.match(permissionDescription("full_access", en), /safe place/);
});

test("a Claude plan is approved or kept in planning", async () => {
  const { permissionChoices, planChoiceLabel } = await import("../src/lib/sessionPermissions.ts");
  const actions = ["allow_once", "allow_session", "deny"];
  assert.deepEqual(permissionChoices("plan", actions), ["allow_once", "deny"]);
  assert.deepEqual(permissionChoices("command", actions), actions);
  assert.equal(planChoiceLabel("plan", "allow_once", en), "Approve");
  assert.equal(planChoiceLabel("plan", "deny", pt), "Continuar planejando");
  assert.equal(planChoiceLabel("command", "deny", en), null);
});
