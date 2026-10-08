import assert from "node:assert/strict";
import { renderSafeMarkdown, stripInternalAgentMetadata } from "../src/lib/markdown.js";
import { renderWorkspaceMarkdownWithFileBadges } from "../src/lib/workspaceFileReferences.js";
import { renderSafeMarkdown as renderMobileMarkdown } from "../mobile-pwa/markdown.js";

const rendered = renderSafeMarkdown(`Resposta visível.

<oai-mem-citation>
<citation_entries>
MEMORY.md:1-2|note=[internal]
</citation_entries>
<rollout_ids>
019f8061-7032-7521-b333-84f84c744fa8
</rollout_ids>
</oai-mem-citation>`);

assert.equal(rendered, "<p>Resposta visível.</p>");
assert.equal(rendered.includes("oai-mem-citation"), false);
assert.equal(stripInternalAgentMetadata("Visible\n<oai-mem-citation>hidden"), "Visible");
assert.equal(
  renderSafeMarkdown("Arquivo: [README](https://example.com/README.md)"),
  '<p>Arquivo: <a href="https://example.com/README.md" target="_blank" rel="noopener noreferrer">README</a></p>',
);
const nestedLink = "Veja [`npx … create`](https://www.npmjs.com/package/@pixel-point/toolcraft).";
const expectedNestedLink = '<p>Veja <a href="https://www.npmjs.com/package/@pixel-point/toolcraft" target="_blank" rel="noopener noreferrer"><code>npx … create</code></a>.</p>';
assert.equal(renderSafeMarkdown(nestedLink), expectedNestedLink);
assert.equal(renderMobileMarkdown(nestedLink), expectedNestedLink);
assert.equal(renderSafeMarkdown("[`<script>`](javascript:evil)"), "<p><code>&lt;script&gt;</code></p>");
assert.equal(renderWorkspaceMarkdownWithFileBadges(nestedLink, () => "<svg></svg>"), expectedNestedLink);

const table = renderSafeMarkdown(`| Phase | Status |
| :--- | ---: |
| Context Builder | Complete |
| Safety | **Testing** |`);
assert.match(table, /<div class="markdown-table-wrap"><table>/);
assert.match(table, /<th class="align-left">Phase<\/th>/);
assert.match(table, /<th class="align-right">Status<\/th>/);
assert.match(table, /<td class="align-right"><strong>Testing<\/strong><\/td>/);
assert.equal(renderMobileMarkdown(`| Phase | Status |\n| :--- | ---: |\n| Context Builder | Complete |\n| Safety | **Testing** |`), table);
assert.equal(renderSafeMarkdown("A | B\nStill text"), "<p>A | B<br>Still text</p>");

const icon = () => '<svg class="inline-file-icon" aria-hidden="true"></svg>';
const cited = renderWorkspaceMarkdownWithFileBadges(
  "Feito no chat do workspace (Documents/Projetos/Ideias/Lume/src/lib/WorkspaceSessionPane.svelte).",
  icon,
);
assert.match(cited, /workspace \(<span class="inline-file-badge"[^>]*><svg[^>]*><\/svg><span class="inline-file-name">WorkspaceSessionPane\.svelte<\/span><\/span>\)\.<\/p>/);
assert.equal(cited.includes("Arquivo citado"), false);
assert.match(
  renderWorkspaceMarkdownWithFileBadges("Veja [o arquivo](/work/src/main.rs).", icon),
  /<span class="inline-file-name">main\.rs<\/span>/,
);
assert.match(
  renderWorkspaceMarkdownWithFileBadges("Veja [o arquivo](/work/src/main.rs).", icon, "/work"),
  /<button class="inline-file-badge" type="button" data-local-file="\/work\/src\/main\.rs"/,
);
assert.equal(
  renderWorkspaceMarkdownWithFileBadges("Veja [site](https://example.com/main.rs) e `src/main.rs`.", icon),
  renderSafeMarkdown("Veja [site](https://example.com/main.rs) e `src/main.rs`."),
);
assert.equal(
  renderWorkspaceMarkdownWithFileBadges("```\nsrc/main.rs\n```", icon),
  renderSafeMarkdown("```\nsrc/main.rs\n```"),
);
assert.equal(
  renderWorkspaceMarkdownWithFileBadges("Não destaque (src/.env).", icon),
  renderSafeMarkdown("Não destaque (src/.env)."),
);
assert.match(
  renderWorkspaceMarkdownWithFileBadges("Arquivo (src/<img onerror=x>.ts).", icon),
  /title="src\/&lt;img onerror=x&gt;\.ts"/,
);

console.log("markdown test suite passed");

{
  const html = renderSafeMarkdown("See https://github.com/Lume-agents/Lume/pull/26, and [the issue](https://github.com/o/r/issues/3). Plain https://example.com/x stays.");
  assert.match(html, /<a class="github-ref" href="https:\/\/github.com\/Lume-agents\/Lume\/pull\/26"[^>]*>Lume-agents\/Lume#26<\/a>/);
  assert.match(html, /<a class="github-ref" href="https:\/\/github.com\/o\/r\/issues\/3"/);
  assert.doesNotMatch(html, /example\.com[^<]*<\/a>/);
}
