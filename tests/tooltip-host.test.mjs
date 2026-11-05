import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const read = (path) => readFileSync(new URL(`../${path}`, import.meta.url), "utf8");
const host = read("src/lib/LumeTooltipHost.svelte");

// A click also focuses a control, so only keyboard focus asks for a tooltip.
assert.match(host, /target\?\.matches\(":focus-visible"\)\) show\(target, focusDelay\)/);
assert.ok(!/if \(target\) show\(target, focusDelay\)/.test(host), "focus no longer shows a tooltip by itself");

// A title that repeats the text already on screen is skipped unless that text is cut off.
assert.match(host, /function repeatsVisibleText\(target: HTMLElement, tip: string\)/);
assert.match(host, /visible === flat\(tip\) && !isTruncated\(target\)/);
assert.match(host, /if \(!nextText \|\| repeatsVisibleText\(target, nextText\)\) return;/);
assert.match(host, /target\.dataset\.tooltip\?\.trim\(\)\) return false/, "an explicit data-tooltip is always shown");

// Hovering waits a little longer, and what a row or a number already says is not said again.
assert.match(host, /const hoverDelay = 800;/);
const windowSource = read("src/lib/WorkspaceWindow.svelte");
assert.match(windowSource, /title=\{sidebarCollapsed \? sessionName\(session\) : undefined\}/, "an open sidebar row has no tooltip; the collapsed one names the agent");
assert.ok(!/Drag to arrange this agent|Drag to reorder|Drag to resize|Import a font file/.test(windowSource));
assert.ok(!/Lines added and removed|Files marked as reviewed/.test(read("src/lib/WorkspaceReviewCenter.svelte")));
assert.ok(!/title=\{pt \? "Mudanças"|\$\{info\.fromBranch\} → \$\{info\.branch\}`\}/.test(read("src/lib/GitEventChips.svelte")));
assert.ok(!/Updated or published on this date/.test(read("src/lib/WorkspaceExtensions.svelte")));

// The labels that were only repeated are gone.
const review = read("src/lib/WorkspaceReviewCenter.svelte");
assert.ok(!/title=\{tr\("Unified"/.test(review) && !/title=\{tr\("Split"/.test(review));
const inspector = read("src/lib/WorkspaceInspector.svelte");
assert.ok(!/inspector-access-badge[^>]*title=/.test(inspector));
assert.ok(!/access-badge full-access" title=/.test(read("src/lib/TerminalWindow.svelte")));

console.log("tooltip host test passed");
