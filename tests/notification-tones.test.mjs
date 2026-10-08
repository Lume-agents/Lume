import assert from "node:assert/strict";
import { scheduleTone, toneDuration } from "../src/lib/notificationTones.ts";

function fakeContext() {
  const log = { gains: [], oscillators: [] };
  return {
    log,
    destination: {},
    createGain() {
      const calls = [];
      const gain = { gain: { setValueAtTime: (value, at) => calls.push(["set", value, at]), linearRampToValueAtTime: (value, at) => calls.push(["linear", value, at]), exponentialRampToValueAtTime: (value, at) => calls.push(["exp", value, at]) }, connect() {}, disconnect() {}, calls };
      log.gains.push(gain);
      return gain;
    },
    createOscillator() {
      const oscillator = { type: "", frequency: { setValueAtTime() {} }, connect() {}, disconnect() {}, start(at) { oscillator.startAt = at; }, stop(at) { oscillator.stopAt = at; } };
      log.oscillators.push(oscillator);
      return oscillator;
    },
  };
}

for (const kind of ["completed", "failed", "permission", "usage"]) {
  const context = fakeContext();
  const end = scheduleTone(context, kind, 80, 2);
  assert.equal(end, 2 + toneDuration(kind));
  assert.ok(context.log.oscillators.length >= 2, kind + " has several notes");
  context.log.gains.forEach((gain, index) => {
    const [set, linear, exp] = gain.calls;
    assert.equal(set[1], 0.0001, "each note starts from silence");
    assert.ok(linear[2] > set[2] && linear[1] > 0.0001, "and has an attack");
    assert.equal(exp[1], 0.0001, "and decays to silence");
    const oscillator = context.log.oscillators[index];
    assert.ok(oscillator.stopAt > exp[2], kind + ": the oscillator stops only after its envelope is silent");
    assert.equal(oscillator.startAt, set[2]);
  });
}

// The volume scales the peak and never reaches zero (a zero target breaks the exponential ramp).
const quiet = fakeContext();
scheduleTone(quiet, "usage", 0, 0);
assert.ok(quiet.log.gains[0].calls[1][1] > 0);
const loud = fakeContext();
scheduleTone(loud, "usage", 100, 0);
assert.ok(loud.log.gains[0].calls[1][1] > quiet.log.gains[0].calls[1][1]);
console.log("notification tone tests passed");
