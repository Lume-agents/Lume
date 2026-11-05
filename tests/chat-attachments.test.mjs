import assert from "node:assert/strict";
import {
  cleanPromptTransport,
  isSyntheticPromptText,
  extractResponseFileCards,
  extractResponseFiles,
  promptTextKey,
} from "../src/lib/chatAttachments.ts";

const transportedPrompt = `Create the Rio presentation

Files attached through Lume. Inspect these local paths:
- "/home/user/Downloads/Mural de Troca — Apresentação Comercial.pdf"`;

assert.equal(cleanPromptTransport(transportedPrompt), "Create the Rio presentation");
assert.equal(promptTextKey(transportedPrompt), promptTextKey("Create the Rio presentation"));

const files = extractResponseFiles(
  "Final files: [deck](outputs/Rio presentation.pdf), [preview](/work/mural/preview.png)\nThe editable generator is in `generate.mjs`, and the [site](https://example.com/file.pdf) is online.",
  [{
    id: "preview",
    name: "preview.png",
    mimeType: "image/png",
    previewDataUrl: "",
    path: "/work/mural/preview.png",
  }],
  "/work/mural",
);

assert.deepEqual(files, [
  {
    path: "/work/mural/outputs/Rio presentation.pdf",
    name: "Rio presentation.pdf",
    mimeType: "application/pdf",
    isImage: false,
  },
  {
    path: "/work/mural/preview.png",
    name: "preview.png",
    mimeType: "image/png",
    isImage: true,
  },
]);

assert.deepEqual(
  extractResponseFiles("[secret](/work/mural/.env)", [], "/work/mural"),
  [],
);

assert.deepEqual(
  extractResponseFileCards(
    "Updated [configuration](src/config.ts). Final file: [report](outputs/report.pdf)",
    [],
    "/work/lume",
  ),
  [
    {
      path: "/work/lume/src/config.ts",
      name: "config.ts",
      mimeType: "application/octet-stream",
      isImage: false,
      downloadable: false,
    },
    {
      path: "/work/lume/outputs/report.pdf",
      name: "report.pdf",
      mimeType: "application/pdf",
      isImage: false,
      downloadable: true,
    },
  ],
);

assert.deepEqual(
  extractResponseFiles(
    "The editable generator is in `generate.mjs`.",
    [{
      id: "generator",
      name: "generate.mjs",
      mimeType: "application/octet-stream",
      previewDataUrl: "",
      path: "/work/mural/generate.mjs",
    }],
    "/work/mural",
  ),
  [],
);

assert.deepEqual(
  extractResponseFiles(
    "Main files: [workflow_runtime.rs](/work/lume/src/workflow_runtime.rs) and [WorkflowBridgeWindow.svelte](/work/lume/src/WorkflowBridgeWindow.svelte).",
    [],
    "/work/lume",
  ),
  [],
);

assert.deepEqual(
  extractResponseFiles(
    "Generated file: [workflow bundle](/work/lume/workflow_bundle.rs)",
    [],
    "/work/lume",
  ),
  [{
    path: "/work/lume/workflow_bundle.rs",
    name: "workflow_bundle.rs",
    mimeType: "application/octet-stream",
    isImage: false,
  }],
);

assert.deepEqual(
  extractResponseFiles(
    "PDF final: `/work/mural/outputs/Rio presentation.pdf`",
    [],
    "/work/mural",
  ),
  [{
    path: "/work/mural/outputs/Rio presentation.pdf",
    name: "Rio presentation.pdf",
    mimeType: "application/pdf",
    isImage: false,
  }],
);

// Text pasted into Claude Code's CLI arrives wrapped in <pasted_content> tags; only the text is the message.
assert.equal(cleanPromptTransport('olhe isto\n\n<pasted_content id="5f7b">\n<\\pasted_content id="f83e">\nLeia o AGENTS.md\n<\\/pasted_content>\n</pasted_content>'), "olhe isto\n\nLeia o AGENTS.md");
assert.equal(cleanPromptTransport('<pasted_content id="a">oi</pasted_content>'), "oi");
assert.equal(cleanPromptTransport("a < b e <pasted_contents>x</pasted_contents>"), "a < b e <pasted_contents>x</pasted_contents>");
// "Tool loaded." is Claude Code's own note beside a loaded deferred tool, not something a person wrote.
assert.equal(isSyntheticPromptText("Tool loaded."), true);
assert.equal(isSyntheticPromptText("Tool loaded. Now what?"), false);

// What the person ran with ! in Claude Code's CLI, and its output, are not messages either.
for (const text of ["<bash-input> claude mcp add atlassian</bash-input>", "<bash-stdout>Added HTTP MCP server</bash-stdout><bash-stderr></bash-stderr>"]) {
  assert.equal(isSyntheticPromptText(text), true, text);
}
assert.equal(isSyntheticPromptText("use <bash-input> to run things"), false);

console.log("chat attachment test suite passed");
