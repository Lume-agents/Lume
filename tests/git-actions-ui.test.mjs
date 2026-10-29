import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const actions = readFileSync(new URL("../src/lib/GitActions.svelte", import.meta.url), "utf8");
const panel = readFileSync(new URL("../src/lib/RepositoryPanel.svelte", import.meta.url), "utf8");
const graph = readFileSync(new URL("../src/lib/CommitGraph.svelte", import.meta.url), "utf8");
const backend = readFileSync(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8");
const repository = readFileSync(new URL("../src-tauri/src/repository.rs", import.meta.url), "utf8");

// Nothing that can publish, reset or merge is offered; a rebase is cancelled when it conflicts.
assert.ok(!/git(Push|Reset|Merge)\b/i.test(actions), "the bar offers push, reset or merge");
const section = repository.slice(repository.indexOf("// ── Git actions"), repository.indexOf("pub fn diff("));
assert.ok(!/\[\s*"(push|reset|pull|merge)"/.test(section.replace('"merge", "--ff-only"', "")), "the Git actions never push, reset or merge");
assert.match(repository, /"merge", "--ff-only"/, "a pull only moves the branch forward");
assert.match(section, /"rebase", "--abort"/, "a conflicting rebase is aborted");
assert.match(repository, /"stash", "apply"/, "applying keeps the stash");
assert.match(section, /\["fetch", "--prune", "--quiet"\]/, "fetch can prune");

// Dropping a stash, rebasing and switching with changes all ask first.
assert.match(actions, /kind: "drop"/);
assert.match(actions, /kind: "rebase"/);
assert.match(actions, /confirm = \{ kind: "switch", target: name \}/, "switching with changes asks first");
assert.match(actions, /Stash and switch/, "the switch offers a stash");

// The menus float and animate in and out.
assert.match(actions, /class="menu branches"[^>]*transition:fly/);
assert.match(actions, /class="menu actions"[^>]*transition:fly/);
assert.match(graph, /class="detail-wrap" transition:slide/, "a commit opens and closes with a transition");
assert.match(panel, /\{#key tab\}\s*<div class="tab-body" in:fly/, "a tab change animates");

// What moves files waits while an agent works in the folder, in the bar and in the backend.
assert.match(actions, /disabled=\{!canPull \|\| !!working\}/);
assert.match(actions, /agentWorking/);
assert.match(backend, /agents_working_in\(&state, &directory\)/);
assert.match(backend, /"agent_running"/);
for (const command of ["git_pull_session", "git_rebase_session", "git_stash_session", "git_stash_apply_session", "git_switch_session"]) {
  const body = backend.slice(backend.indexOf(`async fn ${command}`));
  assert.match(body.slice(0, body.indexOf("}\n")), /git_action\(state, session_id, true/, `${command} must wait for the agent`);
}

// Only the session's own folder: a remote computer's session never shows the bar.
assert.match(panel, /\{#if !session\.remote\}<GitActions/);
assert.match(backend, /remote\.is_some\(\)/);

console.log("git actions ui test passed");
