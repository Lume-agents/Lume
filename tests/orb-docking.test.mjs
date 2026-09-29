import assert from "node:assert/strict";
import {
  clampOrbPosition,
  compactPositionForPanel,
  expandedPositionForOrb,
  freeOrbDock,
  orbCornerRadii,
  orbDockAtPosition,
  orbDockPressure,
  orbEdgePressure,
  pinOrbPosition,
  snapOrbPosition,
} from "../src/lib/orbDocking.ts";

const orb = { width: 78, height: 44 };
const panel = { width: 392, height: 340 };
const area = { x: 0, y: 32, width: 1280, height: 720 };
const dock = (horizontal = null, vertical = null) => ({ horizontal, vertical });

for (const [from, position, edges] of [
  [{ x: 18, y: 300 }, { x: 0, y: 300 }, dock("left")],
  [{ x: 1188, y: 300 }, { x: 1202, y: 300 }, dock("right")],
  [{ x: 550, y: 48 }, { x: 550, y: 32 }, dock(null, "top")],
  [{ x: 550, y: 692 }, { x: 550, y: 708 }, dock(null, "bottom")],
  [{ x: 12, y: 42 }, { x: 0, y: 32 }, dock("left", "top")],
  [{ x: 1190, y: 42 }, { x: 1202, y: 32 }, dock("right", "top")],
  [{ x: 12, y: 696 }, { x: 0, y: 708 }, dock("left", "bottom")],
  [{ x: 1190, y: 696 }, { x: 1202, y: 708 }, dock("right", "bottom")],
]) {
  assert.deepEqual(snapOrbPosition(from, orb, area, 1), { position, dock: edges });
  assert.deepEqual(orbDockAtPosition(position, orb, area, 1), edges,
    "a persisted edge contact must restore the same dock after reopening");
}

assert.deepEqual(snapOrbPosition({ x: 500, y: 300 }, orb, area, 1), {
  position: { x: 500, y: 300 }, dock: freeOrbDock(),
}, "a drop in the center must stay where the user puts it");
assert.equal(snapOrbPosition({ x: 29, y: 300 }, orb, area, 1).dock.horizontal, null,
  "the proximity animation must not make every nearby drop magnetic");
assert.deepEqual(orbDockAtPosition({ x: 500, y: 44 }, orb, area, 1), freeOrbDock(),
  "a saved free position near the top must not silently become docked");

const nearLeft = (x) => orbEdgePressure({ x, y: 300 }, orb, area, 1);
assert.equal(nearLeft(72).left, 0);
assert.equal(nearLeft(36).left, 0.5);
assert.equal(nearLeft(0).left, 1);
assert.equal(nearLeft(0).right, 0, "opposing edges must not squash the Orb simultaneously");
assert.deepEqual(orbCornerRadii(orbDockPressure(dock("left", "top"))), [3, 3, 22, 3],
  "a corner must flatten both contact sides and preserve the inner rounded corner");
assert.deepEqual(orbCornerRadii(orbDockPressure(freeOrbDock())), [22, 22, 22, 22],
  "pulling away must restore the original capsule contour");

const highDpiArea = { x: 16, y: 72, width: 2560, height: 1368 };
assert.deepEqual(snapOrbPosition({ x: 2370, y: 1320 }, orb, highDpiArea, 2), {
  position: { x: 2420, y: 1352 }, dock: dock("right", "bottom"),
}, "magnetic distance, Orb size and work area offsets must use the same DPI units");
assert.equal(snapOrbPosition({ x: 2362, y: 500 }, orb, highDpiArea, 2).dock.horizontal, null);
assert.equal(orbEdgePressure({ x: 88, y: 500 }, orb, highDpiArea, 2).left, 0.5);
assert.deepEqual(snapOrbPosition({ x: 1810, y: 995 }, orb,
  { x: 0, y: 30, width: 1920, height: 1020 }, 1.25), {
  position: { x: 1822, y: 995 }, dock: dock("right", "bottom"),
}, "fractional display scales must round the window size before touching the edge");
assert.deepEqual(clampOrbPosition({ x: -300, y: 2000 }, orb, highDpiArea, 2), { x: 16, y: 1352 },
  "dragging must stay clear of reserved desktop bars on every side");
assert.deepEqual(pinOrbPosition({ x: 1202, y: 300 }, dock("right"), orb, highDpiArea, 2),
  { x: 2420, y: 300 }, "an attached side must stay attached when the monitor geometry changes");

assert.deepEqual(expandedPositionForOrb({ x: 550, y: 32 }, orb, panel, area, 1, dock(null, "top")),
  { x: 393, y: 32 }, "a top dock must open downward with the panel centered on the Orb");
assert.deepEqual(expandedPositionForOrb({ x: 550, y: 708 }, orb, panel, area, 1, dock(null, "bottom")),
  { x: 393, y: 412 }, "a bottom dock must open upward");
const corner = { x: 1202, y: 708 };
const expanded = expandedPositionForOrb(corner, orb, panel, area, 1, dock("right", "bottom"));
assert.deepEqual(expanded, { x: 888, y: 412 });
assert.deepEqual(compactPositionForPanel(expanded, panel, orb, area, 1), corner,
  "expanding inward from a corner must return to the original compact anchor");
assert.deepEqual(clampOrbPosition({ x: 400, y: 800 }, panel, { x: 20, y: 40, width: 100, height: 90 }, 1),
  { x: 20, y: 40 }, "small work areas must not produce a negative window origin");

console.log("Orb docking geometry test suite passed");
