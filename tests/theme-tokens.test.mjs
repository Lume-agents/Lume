import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const themeCss = readFileSync(new URL("../src/lib/themeTokens.css", import.meta.url), "utf8");
const appCss = readFileSync(new URL("../src/app.css", import.meta.url), "utf8");
const workspace = readFileSync(new URL("../src/lib/WorkspaceWindow.svelte", import.meta.url), "utf8");
const pane = readFileSync(new URL("../src/lib/WorkspaceSessionPane.svelte", import.meta.url), "utf8");
const select = readFileSync(new URL("../src/lib/LumeSelect.svelte", import.meta.url), "utf8");

assert.match(appCss, /@import "\.\/lib\/themeTokens\.css"/);
const semanticScope = themeCss.match(/:root, \[data-appearance\] \{([^}]+)\}/)?.[1];
assert.ok(semanticScope, "semantic tokens must resolve inside each theme scope");
for (const token of ["user-light", "user-dark", "user-line-light", "user-line-dark", "ink-light", "ink-dark", "code-light", "code-dark", "accent-soft-light", "accent-soft-dark", "subtle-light", "subtle-dark"]) {
  assert.ok(semanticScope.includes(`--lume-${token}:`), `missing semantic token ${token}`);
}
for (const name of ["forest", "ocean", "violet", "ember"]) {
  const palette = themeCss.match(new RegExp(`\\[data-appearance="${name}"\\] \\{([^}]+)\\}`))?.[1];
  assert.ok(palette, `missing palette ${name}`);
  for (const token of ["accent", "surface-light", "surface-dark", "canvas-light", "canvas-dark"]) {
    assert.ok(palette.includes(`--lume-${token}:`), `missing ${token} in ${name}`);
  }
}
assert.match(workspace, /--workspace-user: var\(--lume-user-light\)/);
assert.match(workspace, /--workspace-user: var\(--lume-user-dark\)/);
assert.match(workspace, /grid-template-rows: minmax\(0, 1fr\)/);
assert.match(pane, /\.composer \{[^}]+flex: 0 0 auto/);
assert.match(pane, /class:beam=\{composerInIntroPosition && canCompose\}/);
assert.match(pane, /class:crossfade-out=\{composerTransition === "out"\}/);
assert.match(pane, /class:crossfade-in=\{composerTransition === "in"\}/);
assert.match(pane, /variant="heading" onValueChange=\{chooseModel\}/);
assert.match(pane, /class="controls-icon-button model-reset"/);
assert.match(pane, /\.controls-model-row \{[^}]+grid-template-columns: 32px minmax\(0, 1fr\) 32px/);
assert.match(pane, /\.lume-select\.heading \.lume-select-trigger\) \{[^}]+height: 32px; min-height: 32px; padding: 0 12px;[^}]+border: 1px solid transparent/);
assert.match(pane, /\.lume-select\.heading \.lume-select-trigger:hover\)[^\n]+\.lume-select\.heading \.lume-select-trigger\.open/);
assert.match(pane, /prefers-reduced-motion: reduce[^}]+\.composer-field\.beam/);
assert.match(select, /--select-surface: var\(--workspace-raised, var\(--dropdown-surface, var\(--lume-raised-light\)\)\)/);
assert.match(select, /--select-accent: var\(--workspace-accent, var\(--dropdown-accent, var\(--lume-accent-strong\)\)\)/);
assert.match(select, /--select-accent: var\(--lume-accent\)/);
assert.match(select, /in:fly=/);
assert.match(select, /out:fly=/);
assert.match(workspace, /\.workspace-wallpaper \{[^}]+position: absolute[^}]+width: 100%[^}]+height: 100%/);
assert.match(workspace, /\.workbench \{[^}]+max-width: 100%[^}]+contain: inline-size/);

console.log("theme token test suite passed");
