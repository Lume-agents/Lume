import assert from "node:assert/strict";
import test from "node:test";
import { filterNotes, noteExcerpt, noteTags, noteTitleFrom, parseNoteTags, projectLabel } from "../src/lib/notes.ts";

const note = (id, extra = {}) => ({ id, nativeSessionId: "", title: id, body: id, kind: "note", pinned: false, createdAt: 1, updatedAt: 1, ...extra });

test("notes filter by agent and tag", () => {
  const notes = [note("a", { agent: "codex", tags: ["release"] }), note("b", { agent: "claude_code", tags: ["release", "ideas"] }), note("c")];
  assert.deepEqual(filterNotes(notes, {}).map((item) => item.id), ["a", "b", "c"]);
  assert.deepEqual(filterNotes(notes, { agent: "codex" }).map((item) => item.id), ["a"]);
  assert.deepEqual(filterNotes(notes, { tag: "ideas" }).map((item) => item.id), ["b"]);
  assert.deepEqual(noteTags(notes), ["release", "ideas"], "most used first");
});

test("tags, labels and excerpts", () => {
  assert.deepEqual(parseNoteTags("#Release, ideas  ideas,,todo"), ["release", "ideas", "todo"]);
  assert.equal(parseNoteTags(Array.from({ length: 20 }, (_, index) => "t" + index).join(",")).length, 12);
  assert.equal(projectLabel("C:\\work\\lume\\"), "lume");
  assert.equal(projectLabel("/work/app"), "app");
  assert.equal(noteExcerpt("# Plan\n\n- [x] Ship **it**"), "Plan Ship it", "task boxes and marks are dropped");
  assert.equal(noteTitleFrom("\n## Fix the cart\nmore", "Note"), "Fix the cart");
  assert.equal(noteTitleFrom("   ", "Note"), "Note");
});
