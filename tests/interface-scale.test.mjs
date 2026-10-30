import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const read = (path) => readFileSync(new URL(`../${path}`, import.meta.url), "utf8");
const scale = read("src/lib/interfaceScale.ts");
const pane = read("src/lib/WorkspaceSessionPane.svelte");
const workspace = read("src/lib/WorkspaceWindow.svelte");
const terminal = read("src/lib/TerminalWindow.svelte");
const capability = JSON.parse(read("src-tauri/capabilities/default.json"));
const backend = read("src-tauri/src/lib.rs");

// The whole window zooms and the chat content undoes it, so the chat keeps its own text size.
assert.match(scale, /setZoom\(scale\)/);
assert.match(scale, /--lume-chat-unzoom", String\(1 \/ scale\)/);
assert.match(pane, /\.conversation-content \{[^}]*zoom: var\(--lume-chat-unzoom, 1\)/);
assert.ok(capability.permissions.includes("core:webview:allow-set-webview-zoom"));
assert.match(workspace, /applyInterfaceScale\(preferences\.interfaceScale\)/);
assert.match(terminal, /applyInterfaceScale\(nextPreferences\.interfaceScale\)/);
assert.match(backend, /interface_scale\.clamp\(80, 140\)/);

// The sidebar heading: new group beside the search, reload beside the launcher, no count, and searching folds both away.
assert.match(workspace, /class="search-toggle"[\s\S]*class="heading-fold"[^>]*>\s*<strong>/, "the title comes right after the search");
assert.match(workspace, /class="heading-actions"[\s\S]*name="folder-plus"[\s\S]*class="heading-icon refresh-agents"[\s\S]*class="session-launcher"/, "new group, reload and the launcher sit together on the right");
assert.match(workspace, /<small class="group-count">\{row\.count\}<\/small>/, "a group's count is the last thing in its heading");
assert.match(workspace, /\.session-context-command, \.session-context-actions button \{[^}]*background: transparent/, "context menu buttons only fill on hover");
assert.ok(!/<span>\{orderedSessions\.length\}<\/span>/.test(workspace), "the agent count is gone");
assert.match(workspace, /\.session-heading\.searching \.heading-actions \{[^}]*opacity: 0/);

console.log("interface scale test passed");
