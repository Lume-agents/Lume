import assert from "node:assert/strict";
import { test } from "node:test";
import { createSystemBannerSource } from "../src/lib/systemBannerContext.ts";
import { animatedDisclosure } from "../src/lib/animatedDisclosure.ts";

test("banner sources report changes without repeating polling failures", () => {
  const reported = [];
  const publish = createSystemBannerSource((notice) => reported.push(notice));
  const initial = { id: "connection", message: "Connection unavailable", tone: "error" };
  publish([initial]);
  publish([{ ...initial }]);
  assert.deepEqual(reported, [initial]);
  publish([{ ...initial, message: "Connection reset" }]);
  publish([{ ...initial, message: "Connection reset", tone: "warning" }]);
  assert.equal(reported.length, 3);
});

test("a recovered source can report the same failure again", () => {
  const reported = [];
  const publish = createSystemBannerSource((notice) => reported.push(notice));
  const notice = { id: "remote", message: "Disconnected" };
  publish([notice]);
  publish([]);
  publish([notice]);
  assert.equal(reported.length, 2);
});

test("independent sources keep their identity and dismissal action", () => {
  const reported = [];
  let dismissed = false;
  const publish = createSystemBannerSource((notice) => reported.push(notice));
  publish([
    { id: "node-a", message: "Unavailable", onDismiss: () => { dismissed = true; } },
    { id: "node-b", message: "Unavailable" },
  ]);
  assert.deepEqual(reported.map((notice) => notice.id), ["node-a", "node-b"]);
  reported[0].onDismiss();
  assert.equal(dismissed, true);
});

test("components without a banner host keep their local fallback", () => {
  assert.doesNotThrow(() => createSystemBannerSource(undefined)([{ id: "local", message: "Failed" }]));
});

function disclosureFixture({ reduced = false, supported = true, onOpenChange, contentHeight = () => 120 } = {}) {
  const summaryListeners = new Map();
  const detailsListeners = new Map();
  const attributes = new Map();
  const summary = {
    getBoundingClientRect: () => ({ height: 40 }),
    setAttribute: (key, value) => attributes.set(key, value),
    removeAttribute: (key) => attributes.delete(key),
    addEventListener: (key, listener) => summaryListeners.set(key, listener),
    removeEventListener: (key) => summaryListeners.delete(key),
  };
  const animations = [];
  const animate = (frames, options) => {
    let resolve;
    let reject;
    const animation = {
      frames, options, cancelled: false,
      finished: new Promise((finish, fail) => { resolve = finish; reject = fail; }),
      cancel() { this.cancelled = true; reject(new Error("Cancelled")); },
      finish() { resolve(); },
    };
    animations.push(animation);
    return animation;
  };
  const content = { getBoundingClientRect: () => ({ height: contentHeight() }), animate };
  const details = {
    open: false, style: { overflow: "visible" }, offsetHeight: 42, clientHeight: 40,
    animate: supported ? animate : undefined,
    getBoundingClientRect: () => ({ height: details.open ? 162 : 42 }),
    querySelector: (selector) => selector.endsWith("summary") ? summary : content,
    addEventListener: (key, listener) => detailsListeners.set(key, listener),
    removeEventListener: (key) => detailsListeners.delete(key),
  };
  const previousWindow = globalThis.window;
  const previousComputedStyle = globalThis.getComputedStyle;
  globalThis.window = { matchMedia: () => ({ matches: reduced }) };
  globalThis.getComputedStyle = () => ({ opacity: "1", transform: "none" });
  const action = animatedDisclosure(details, onOpenChange);
  return {
    details, attributes, animations, action, summaryListeners, detailsListeners,
    click: () => summaryListeners.get("click")({ defaultPrevented: false, button: 0, preventDefault() {} }),
    cleanup() {
      action.destroy();
      globalThis.window = previousWindow;
      globalThis.getComputedStyle = previousComputedStyle;
    },
  };
}

test("disclosure slides and fades, then restores natural sizing", async () => {
  const fixture = disclosureFixture();
  try {
    await fixture.click();
    assert.equal(fixture.details.open, true);
    assert.equal(fixture.attributes.get("aria-expanded"), "true");
    assert.equal(fixture.animations.length, 2);
    assert.equal(fixture.animations[0].frames[1].height, "162px");
    assert.equal(fixture.animations[1].frames[0].opacity, 0);
    fixture.animations[0].finish();
    await Promise.resolve();
    assert.equal(fixture.details.style.overflow, "visible");
    await fixture.click();
    assert.equal(fixture.details.open, true, "content remains present during the closing slide");
    fixture.animations[2].finish();
    await Promise.resolve();
    assert.equal(fixture.details.open, false);
  } finally { fixture.cleanup(); }
});

test("lazy disclosure content mounts before opening height is measured", async () => {
  let mounted = false;
  const changes = [];
  const fixture = disclosureFixture({
    contentHeight: () => mounted ? 120 : 0,
    onOpenChange: (open) => { mounted = open; changes.push(open); },
  });
  try {
    await fixture.click();
    assert.equal(fixture.animations[0].frames[1].height, "162px");
    assert.deepEqual(changes, [true]);

    const closing = fixture.click();
    assert.deepEqual(changes, [true], "lazy content remains mounted while the section closes");
    fixture.animations[2].finish();
    await closing;
    await Promise.resolve();
    assert.deepEqual(changes, [true, false]);
  } finally { fixture.cleanup(); }
});

test("rapid reversal cannot settle an obsolete disclosure animation", async () => {
  const fixture = disclosureFixture();
  try {
    await fixture.click();
    const closing = fixture.click();
    const reopening = fixture.click();
    await Promise.all([closing, reopening]);
    fixture.animations[0].finish();
    fixture.animations[2].finish();
    await Promise.resolve();
    assert.equal(fixture.details.style.overflow, "hidden");
    fixture.animations[4].finish();
    await Promise.resolve();
    assert.equal(fixture.details.open, true);
    assert.equal(fixture.details.style.overflow, "visible");
  } finally { fixture.cleanup(); }
});

test("reduced motion and unsupported animations toggle immediately", () => {
  for (const options of [{ reduced: true }, { supported: false }]) {
    const fixture = disclosureFixture(options);
    try {
      fixture.click();
      assert.equal(fixture.details.open, true);
      fixture.click();
      assert.equal(fixture.details.open, false);
      assert.equal(fixture.animations.length, 0);
    } finally { fixture.cleanup(); }
  }
});

test("programmatic disclosure changes remain synchronized and cleanup removes listeners", async () => {
  const fixture = disclosureFixture({ reduced: true });
  try {
    fixture.details.open = true;
    fixture.detailsListeners.get("toggle")();
    await fixture.click();
    assert.equal(fixture.details.open, false);
  } finally { fixture.cleanup(); }
  assert.equal(fixture.summaryListeners.size, 0);
  assert.equal(fixture.detailsListeners.size, 0);
  assert.equal(fixture.attributes.has("aria-expanded"), false);
});
