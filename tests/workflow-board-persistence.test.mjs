import assert from "node:assert/strict";
import { WorkflowBoardSaveQueue } from "../src/lib/workflowBoardPersistence.ts";

const group = (id, version) => ({ id, terminalGroupId: id, steps: [], connections: [], version });
const writes = [];
let release;
const queue = new WorkflowBoardSaveQueue(async (value) => {
  writes.push(value);
  if (writes.length === 1) await new Promise((resolve) => { release = resolve; });
});
const first = queue.enqueue(group("a", 1));
queue.enqueue(group("a", 2));
queue.enqueue(group("a", 3));
queue.enqueue(group("b", 1));
assert.equal(writes.length, 1, "only one preference write is in flight");
assert.equal(queue.hasPending("a"), true);
release();
await first;
assert.deepEqual(writes.map(({ id, version }) => [id, version]), [["a", 1], ["a", 3], ["b", 1]]);
assert.equal(queue.hasPending("a"), false);
assert.equal(queue.hasPending("b"), false);

let fail = true;
const recovered = [];
const retryQueue = new WorkflowBoardSaveQueue(async (value) => {
  if (fail) throw new Error("disk unavailable");
  recovered.push(value);
});
await assert.rejects(retryQueue.enqueue(group("a", 1)), /disk unavailable/);
assert.equal(retryQueue.hasPending("a"), true, "unsaved edits survive a failed write");
fail = false;
await retryQueue.enqueue(group("a", 2));
assert.equal(recovered[0].version, 2, "retry writes the newest edit");
assert.equal(retryQueue.hasPending("a"), false);

let releaseFinalWrite;
const finalWrite = new Promise((resolve) => { releaseFinalWrite = resolve; });
const lateWrites = [];
const lateQueue = new WorkflowBoardSaveQueue((value) => {
  lateWrites.push(value.version);
  return value.version === 1 ? finalWrite : Promise.resolve();
});
const completed = lateQueue.enqueue(group("a", 1));
finalWrite.then(() => lateQueue.enqueue(group("a", 2)));
releaseFinalWrite();
await completed;
assert.deepEqual(lateWrites, [1, 2], "edits at the end of a write are not stranded in the queue");
console.log("Workflow board persistence tests passed");
