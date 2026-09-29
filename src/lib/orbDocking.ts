export type OrbPosition = { x: number; y: number };
export type OrbSize = { width: number; height: number };
export type OrbWorkArea = OrbPosition & OrbSize;
export type OrbDock = {
  horizontal: "left" | "right" | null;
  vertical: "top" | "bottom" | null;
};
export type OrbEdgePressure = { left: number; right: number; top: number; bottom: number };

export const ORB_PROXIMITY_DISTANCE = 72;
export const ORB_SNAP_DISTANCE = 28;

export function freeOrbDock(): OrbDock {
  return { horizontal: null, vertical: null };
}

function limits(size: OrbSize, area: OrbWorkArea, scale: number) {
  return {
    left: area.x,
    top: area.y,
    right: Math.max(area.x, area.x + area.width - Math.round(size.width * scale)),
    bottom: Math.max(area.y, area.y + area.height - Math.round(size.height * scale)),
  };
}

/** Positions and work areas are physical pixels; sizes and thresholds are logical pixels. */
export function clampOrbPosition(
  position: OrbPosition,
  size: OrbSize,
  area: OrbWorkArea,
  scale: number,
): OrbPosition {
  const bounds = limits(size, area, scale);
  return {
    x: Math.max(bounds.left, Math.min(position.x, bounds.right)),
    y: Math.max(bounds.top, Math.min(position.y, bounds.bottom)),
  };
}

export function pinOrbPosition(
  position: OrbPosition,
  dock: OrbDock,
  size: OrbSize,
  area: OrbWorkArea,
  scale: number,
): OrbPosition {
  const bounds = limits(size, area, scale);
  return clampOrbPosition({
    x: dock.horizontal ? bounds[dock.horizontal] : position.x,
    y: dock.vertical ? bounds[dock.vertical] : position.y,
  }, size, area, scale);
}

export function snapOrbPosition(
  position: OrbPosition,
  size: OrbSize,
  area: OrbWorkArea,
  scale: number,
  distance = ORB_SNAP_DISTANCE,
): { position: OrbPosition; dock: OrbDock } {
  const clamped = clampOrbPosition(position, size, area, scale);
  const bounds = limits(size, area, scale);
  const threshold = distance * scale;
  const left = clamped.x - bounds.left;
  const right = bounds.right - clamped.x;
  const top = clamped.y - bounds.top;
  const bottom = bounds.bottom - clamped.y;
  const dock: OrbDock = {
    horizontal: Math.min(left, right) <= threshold ? (left <= right ? "left" : "right") : null,
    vertical: Math.min(top, bottom) <= threshold ? (top <= bottom ? "top" : "bottom") : null,
  };
  return { position: pinOrbPosition(clamped, dock, size, area, scale), dock };
}

/** Only an actual edge contact restores docking; a saved nearby position stays free. */
export function orbDockAtPosition(
  position: OrbPosition,
  size: OrbSize,
  area: OrbWorkArea,
  scale: number,
): OrbDock {
  return snapOrbPosition(position, size, area, scale, 1 / scale).dock;
}

export function orbDockPressure(dock: OrbDock): OrbEdgePressure {
  return {
    left: Number(dock.horizontal === "left"),
    right: Number(dock.horizontal === "right"),
    top: Number(dock.vertical === "top"),
    bottom: Number(dock.vertical === "bottom"),
  };
}

export function orbEdgePressure(
  position: OrbPosition,
  size: OrbSize,
  area: OrbWorkArea,
  scale: number,
): OrbEdgePressure {
  const bounds = limits(size, area, scale);
  const left = Math.max(0, position.x - bounds.left);
  const right = Math.max(0, bounds.right - position.x);
  const top = Math.max(0, position.y - bounds.top);
  const bottom = Math.max(0, bounds.bottom - position.y);
  const strength = (distance: number) => {
    const progress = Math.max(0, Math.min(1, 1 - distance / (ORB_PROXIMITY_DISTANCE * scale)));
    return progress * progress * (3 - 2 * progress);
  };
  return {
    left: left <= right ? strength(left) : 0,
    right: right < left ? strength(right) : 0,
    top: top <= bottom ? strength(top) : 0,
    bottom: bottom < top ? strength(bottom) : 0,
  };
}

export function orbCornerRadii(pressure: OrbEdgePressure): [number, number, number, number] {
  const radius = (a: number, b: number) => 22 - 19 * Math.max(a, b);
  return [
    radius(pressure.left, pressure.top),
    radius(pressure.right, pressure.top),
    radius(pressure.right, pressure.bottom),
    radius(pressure.left, pressure.bottom),
  ];
}

export function expandedPositionForOrb(
  position: OrbPosition,
  compact: OrbSize,
  panel: OrbSize,
  area: OrbWorkArea,
  scale: number,
  dock: OrbDock,
): OrbPosition {
  const bounds = limits(compact, area, scale);
  const right = dock.horizontal === "right" || bounds.right - position.x <= 18 * scale;
  const bottom = dock.vertical === "bottom" || bounds.bottom - position.y <= 18 * scale;
  const centered = dock.vertical !== null && dock.horizontal === null;
  const widthDelta = Math.round(panel.width * scale) - Math.round(compact.width * scale);
  const heightDelta = Math.round(panel.height * scale) - Math.round(compact.height * scale);
  return clampOrbPosition({
    x: right ? position.x - widthDelta
      : centered ? position.x - widthDelta / 2 : position.x,
    y: bottom ? position.y - heightDelta : position.y,
  }, panel, area, scale);
}

export function compactPositionForPanel(
  position: OrbPosition,
  panel: OrbSize,
  compact: OrbSize,
  area: OrbWorkArea,
  scale: number,
): OrbPosition {
  const bounds = limits(panel, area, scale);
  const widthDelta = Math.round(panel.width * scale) - Math.round(compact.width * scale);
  const heightDelta = Math.round(panel.height * scale) - Math.round(compact.height * scale);
  return clampOrbPosition({
    x: bounds.right - position.x <= 18 * scale
      ? position.x + widthDelta : position.x,
    y: bounds.bottom - position.y <= 18 * scale
      ? position.y + heightDelta : position.y,
  }, compact, area, scale);
}
