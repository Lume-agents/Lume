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
assert.ok(!/zoom: var\(--lume-chat-unzoom/.test(pane), "no counter-zoom on the chat layout, which rounded icons into fractional pixels");
assert.match(pane, /--workspace-chat-font-size: calc\(calc\(12px \+ var\(--workspace-chat-font-adjust\)\) \* var\(--lume-chat-unzoom, 1\)\)/, "chat text sizes cancel the interface size");
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

// Subagents: one line from under the parent's avatar with a branch per subagent that grows with it, ten rows at most.
assert.match(workspace, /\.subagent-track::before \{[^}]*animation: subagent-rail-grow/);
assert.match(workspace, /class="subagent-branch" aria-hidden="true"/);
assert.match(workspace, /\.subagent-branch \{[^}]*border-bottom-left-radius[^}]*animation: subagent-branch-in/);
assert.match(workspace, /\.subagent-row:not\(:last-child\)::after \{[^}]*animation: subagent-rail-grow/);
assert.match(workspace, /\.subagent-list \{[^}]*max-height: calc\(16px \+ 10 \* 35px\)[^}]*overflow-y: auto/);
assert.ok(!/class="subagent-rail"/.test(workspace), "one line with branches, not a rail through the avatars");

// The chat header no longer has a maximize button; the prompt index and the text size also live in the agent's menu.
assert.ok(!/name="maximize"/.test(pane), "the header has no full-screen button");
assert.match(pane, /addEventListener\("lume:chat-command"/);
assert.match(workspace, /chatCommand\(contextSession, "prompts", true\)/);
assert.match(workspace, /chatCommand\(contextSession, "text-bigger", false\)/);
assert.match(workspace, /chatCommand\(contextSession, "text-smaller", false\)/);

// The inspector shows the chat's own settings and extensions instead of its latest prompts.
const inspector = read("src/lib/WorkspaceInspector.svelte");
assert.match(inspector, /<ChatConfiguration \{session\} \{language\} \/>/);
assert.ok(!/prompt-events|Latest prompts/.test(inspector), "the latest prompts section is gone");
const config = read("src/lib/ChatConfiguration.svelte");
for (const kind of ["plugin", "skill", "instruction", "command", "subagent"]) assert.match(config, new RegExp(`kind: "${kind}"`));

// The sections under the usage show a loading state while they are read.
assert.match(config, /class="config-loading" role="status"/);
assert.match(inspector, /class="mcp-loading" role="status"/);

// The subagent card floats over the chat instead of reserving a band of its own.
const portals = read("src/lib/SubagentPortals.svelte");
assert.match(portals, /\.portal-host \{ position: absolute;[^}]*pointer-events: none/);
assert.match(pane, /<div class="portal-anchor"><SubagentPortals/);

// "Files read" gets the open book; "inspected context" keeps the thought bubble.
const trace = read("src/lib/ActivityTraceGroup.svelte");
assert.match(trace, /run\.category === "read" \? "files-read" : run\.category/);
const icon = read("src/lib/ActivityTypeIcon.svelte");
assert.match(icon, /category === "files-read"/);
assert.match(icon, /category === "read" \|\| category === "analysis"/);
assert.ok(!/@keyframes/.test(icon), "the book is a still drawing");
assert.match(icon, /A page with a large pencil that leaves it at the lower right/, "the edited-files pencil is large and sticks out of the page");

// Small texts: "load earlier messages" and the changed-files toggle use the chat's small size, the sidebar filters 9.5px.
assert.match(pane, /\.load-earlier-chat \{[^}]*font-size: var\(--workspace-chat-small-size\)/);
assert.match(pane, /\.changed-files-toggle \{[^}]*font: 720 var\(--workspace-chat-small-size\)/);
assert.match(workspace, /\.session-filters button \{[^}]*font-size: 9\.5px/);

console.log("interface scale test passed");
