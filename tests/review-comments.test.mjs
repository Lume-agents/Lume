import assert from "node:assert/strict";
import { commentKey, formatCommentsForPrompt, groupByFile, indexComments, parseStoredComments } from "../src/lib/reviewComments.ts";

const comment = (patch) => ({ id: "c" + Math.random(), path: "src/a.ts", side: "new", line: 3, body: "Use a constant.", excerpt: "const x = 1;", createdAt: 1, ...patch });
const comments = [comment({ line: 9, body: "Why?" }), comment({ line: 3 }), comment({ path: "src/b.ts", side: "old", line: 1, excerpt: "", body: "Keep this\nline" })];

assert.deepEqual(groupByFile(comments).map((group) => [group.path, group.comments.map((item) => item.line)]), [["src/a.ts", [3, 9]], ["src/b.ts", [1]]]);
assert.equal(indexComments(comments).get(commentKey("src/a.ts", "new", 3)).length, 1);
assert.equal(indexComments(comments).has(commentKey("src/a.ts", "old", 3)), false);

const text = formatCommentsForPrompt(comments);
assert.match(text, /^Line comments:/);
assert.match(text, /- src\/a\.ts:3\n {2}Code: const x = 1;\n {2}Comment: Use a constant\./);
assert.match(text, /- src\/a\.ts:9/);
assert.match(text, /- src\/b\.ts:1 \(before the change\)\n {2}Comment: Keep this\n {4}line/);
assert.doesNotMatch(text, /Code: \n/, "an empty excerpt is left out");
assert.equal(formatCommentsForPrompt([]), "");

assert.deepEqual(parseStoredComments("not json"), []);
assert.deepEqual(parseStoredComments(null), []);
assert.equal(parseStoredComments(JSON.stringify([comments[0], { id: 1 }, { id: "x", path: "p", body: "b", line: 1, side: "middle" }])).length, 1);
console.log("review comments tests passed");
