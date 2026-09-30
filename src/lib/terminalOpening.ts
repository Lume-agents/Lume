export class TerminalOpeningTimeoutError extends Error {
  constructor() {
    super("The terminal window did not finish loading.");
    this.name = "TerminalOpeningTimeoutError";
  }
}

/** Native creation returns before the hidden WebView is ready to be shown. */
export function waitForTerminalWindow<T extends { label: string }>(
  label: string,
  loadWindows: () => Promise<T[]>,
  { timeoutMs = 18_000, pollIntervalMs = 150 } = {},
): Promise<T[]> {
  return new Promise((resolve, reject) => {
    let pending = true;
    let pollTimer: ReturnType<typeof setTimeout> | undefined;
    const deadline = setTimeout(() => {
      stop();
      reject(new TerminalOpeningTimeoutError());
    }, timeoutMs);

    function stop() {
      pending = false;
      clearTimeout(deadline);
      if (pollTimer !== undefined) clearTimeout(pollTimer);
    }

    async function poll() {
      try {
        const windows = await loadWindows();
        if (!pending) return;
        if (windows.some((window) => window.label === label)) {
          stop();
          resolve(windows);
        } else {
          pollTimer = setTimeout(() => void poll(), pollIntervalMs);
        }
      } catch (error) {
        if (!pending) return;
        stop();
        reject(error);
      }
    }

    void poll();
  });
}
