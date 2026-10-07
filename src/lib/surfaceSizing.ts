export interface SurfaceSize {
  width: number;
  height: number;
  syncLinuxSurface?: boolean;
}

/** Keep native window/WebView resizes ordered; coalesce obsolete frames. */
export function createSurfaceSizeQueue(apply: (size: SurfaceSize) => Promise<void>) {
  let pending: SurfaceSize | null = null;
  let running = false;
  let waiters: Array<{ resolve: () => void; reject: (error: unknown) => void }> = [];

  async function drain() {
    running = true;
    while (pending) {
      const size = pending;
      const current = waiters;
      pending = null;
      waiters = [];
      try {
        await apply(size);
        for (const waiter of current) waiter.resolve();
      } catch (error) {
        for (const waiter of current) waiter.reject(error);
      }
    }
    running = false;
  }

  return (size: SurfaceSize): Promise<void> => new Promise((resolve, reject) => {
    pending = { ...size, syncLinuxSurface: Boolean(size.syncLinuxSurface || pending?.syncLinuxSurface) };
    waiters.push({ resolve, reject });
    if (!running) void drain();
  });
}

export interface SurfaceMeasure {
  width: number;
  height: number;
}

/** Whether a native size is the one asked for, allowing the rounding fractional scales cause. */
export function surfaceMatches(target: SurfaceMeasure, measured: SurfaceMeasure, tolerance = 2): boolean {
  return Math.abs(measured.width - target.width) <= tolerance
    && Math.abs(measured.height - target.height) <= tolerance;
}

export interface SettleOptions {
  target: SurfaceMeasure;
  /** Every size that can clip the panel: the WebView's viewport and the window itself. */
  measure: () => Promise<SurfaceMeasure[]>;
  apply: () => Promise<void>;
  wait: (milliseconds: number) => Promise<void>;
  /** True once a newer resize or an animation makes this check obsolete. */
  shouldStop?: () => boolean;
  onMismatch?: (info: { attempt: number; target: SurfaceMeasure; measured: SurfaceMeasure[] }) => void;
  attempts?: number;
  delays?: number[];
}

/**
 * X11 applies resizes asynchronously, and during the open/close animation the window is resized
 * every frame, so the last size asked for is not always the size it ends up with. That leaves the
 * panel cut at an intermediate size. After the animation this checks the real sizes and asks
 * again until they match. Returns how many corrections it had to make.
 */
export async function settleSurfaceSize(options: SettleOptions): Promise<number> {
  const attempts = options.attempts ?? 6;
  const delays = options.delays ?? [60, 110, 160, 220, 300, 400];
  let corrections = 0;
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    await options.wait(delays[Math.min(attempt, delays.length - 1)]);
    if (options.shouldStop?.()) return corrections;
    const measured = await options.measure();
    if (measured.every((size) => surfaceMatches(options.target, size))) return corrections;
    options.onMismatch?.({ attempt, target: options.target, measured });
    corrections += 1;
    await options.apply();
  }
  return corrections;
}
