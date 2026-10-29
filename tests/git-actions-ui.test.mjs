import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const actions = readFileSync(new URL("../src/lib/GitActions.svelte", import.meta.url), "utf8");
const panel = readFileSync(new URL("../src/lib/RepositoryPanel.svelte", import.meta.url), "utf8");
const backend = readFileSync(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8");
const repository = readFileSync(new URL("../src-tauri/src/repository.rs", import.meta.url), "utf8");

// Nothing that can rewrite history or publish is offered.
assert.ok(!/git(Push|Rebase|Reset|Merge)/i.test(actions), "the bar offers push, rebase, reset or merge");
const section = repository.slice(repository.indexOf("// ── Git actions"), repository.indexOf("pub fn diff("));
assert.ok(!/\[\s*"(push|rebase|reset|pull|merge)"/.test(section.replace('"merge", "--ff-only"', "")), "the Git actions never push, rebase, reset or merge");
assert.match(repository, /"merge", "--ff-only"/, "a pull only moves the branch forward");
assert.match(repository, /"stash", "apply"/, "applying keeps the stash");

// Dropping a stash and switching with changes both ask first.
assert.match(actions, /role="alertdialog"[^>]*Drop stash|Drop stash/, "dropping a stash asks first");
assert.match(actions, /switchTarget = name/, "switching with changes asks first");
assert.match(actions, /Stash and switch/, "the switch offers a stash");

// What moves files waits while an agent works in the folder, in the bar and in the backend.
assert.match(actions, /disabled=\{!canPull\}/);
assert.match(actions, /agentWorking/);
assert.match(backend, /agents_working_in\(&state, &directory\)/);
assert.match(backend, /"agent_running"/);
for (const command of ["git_pull_session", "git_stash_session", "git_stash_apply_session", "git_switch_session"]) {
  const body = backend.slice(backend.indexOf(`async fn ${command}`));
  assert.match(body.slice(0, body.indexOf("}\n")), /git_action\(state, session_id, true/, `${command} must wait for the agent`);
}

// Only the session's own folder: a remote computer's session never shows the bar.
assert.match(panel, /\{#if !session\.remote\}<GitActions/);
assert.match(backend, /remote\.is_some\(\)/);

console.log("git actions ui test passed");
