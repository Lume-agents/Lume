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
