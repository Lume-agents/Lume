import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { usagePollInterval } from "../src/lib/usagePolling.ts";

const read = (path) => readFileSync(new URL(`../${path}`, import.meta.url), "utf8");

// A running task is read on a timer; an idle or finished one is not, and Codex announces its own changes.
assert.equal(usagePollInterval("claude_code", "running"), 15_000);
assert.equal(usagePollInterval("antigravity", "running"), 30_000);
for (const status of ["waiting_for_input", "completed", "failed", "permission_required", "idle"]) {
  assert.equal(usagePollInterval("claude_code", status), null, status);
}
for (const agent of ["codex", "opencode", "grok", "omp", "gemini", "deepseek"]) {
  assert.equal(usagePollInterval(agent, "running"), null, agent);
}

// The Inspector reads the usage on that timer (only while the window is visible) and has a refresh button.
const inspector = read("src/lib/WorkspaceInspector.svelte");
assert.match(inspector, /usagePollInterval\(session\.agent, session\.status\)/);
assert.match(inspector, /if \(!document\.hidden\) void refreshUsage/);
assert.match(inspector, /clearInterval\(timer\)/, "the timer is cleared when the session stops running or changes");
assert.match(inspector, /class="inspector-refresh"[^>]*onclick=\{refreshInspector\}/);
assert.match(inspector, /usageRead\.delete\(key\)/, "a refresh ignores what was read once for the session");
assert.match(inspector, /mcpCache\.delete\(id\)/);
// The button sits in the header, before the close button.
assert.ok(inspector.indexOf('class="inspector-refresh"') < inspector.indexOf('"Close inspector"'));

// While a message is in flight the usage is asked of the process that runs it, never of a new one.
const backend = read("src-tauri/src/lib.rs");
assert.match(backend, /claude_session::usage_from_live\(\)/);
assert.match(backend, /None if prompt_in_flight => return/);

console.log("Usage polling and inspector refresh passed.");
