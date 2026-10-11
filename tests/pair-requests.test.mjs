import assert from "node:assert/strict";
import test from "node:test";
import { pairRequestBanner, spacedPairCode } from "../src/lib/pairRequests.ts";

const request = { requestId: "r1", displayName: "Celular da Ana", clientFingerprint: "f", code: "120045", createdAt: 0, expiresAt: 120_000 };

test("the code is shown in two groups of three", () => {
  assert.equal(spacedPairCode("120045"), "120 045");
  assert.equal(spacedPairCode("12"), "12");
});

test("the banner names the device, shows the code and says where to answer", () => {
  const banner = pairRequestBanner(request, "pt-BR", 0, () => {});
  assert.equal(banner.id, "pair-request:r1");
  assert.match(banner.message, /Celular da Ana/);
  assert.match(banner.message, /120 045/);
  assert.match(banner.message, /Computadores remotos/);
  assert.equal(banner.tone, "warning");
  assert.match(pairRequestBanner(request, "en", 0, () => {}).message, /Remote computers/);
});

test("the banner outlives a glance but not the request", () => {
  assert.equal(pairRequestBanner(request, "en", 0, () => {}).duration, 45_000);
  assert.equal(pairRequestBanner(request, "en", 119_000, () => {}).duration, 8_000);
  assert.equal(pairRequestBanner({ ...request, expiresAt: 30_000 }, "en", 0, () => {}).duration, 30_000);
});
