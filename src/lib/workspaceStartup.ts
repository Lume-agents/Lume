type Unsubscribe = () => void;

type StartupOptions = {
  ready: () => Promise<void>;
  failed: (reason: string) => Promise<void>;
  onLoaded: () => void;
  onError: (reason: string) => void;
  timeoutMessage: string;
  timeoutMs?: number;
};

/** Owns startup subscriptions, including registrations that resolve after close. */
export class WorkspaceStartup {
  private disposed = false;
  private failed = false;
  private subscriptions = new Set<Unsubscribe>();
  private cancel: (() => void) | undefined;

  get active() {
    return !this.disposed && !this.failed;
  }

  async subscribe(register: () => Promise<Unsubscribe>) {
    if (!this.active) return;
    const stop = await register();
    if (!this.active) stop();
    else this.subscriptions.add(stop);
  }

  async run(initialize: () => Promise<void>, options: StartupOptions): Promise<void> {
    let timer: ReturnType<typeof setTimeout> | undefined;
    const cancelled = new Promise<never>((_, reject) => {
      this.cancel = () => reject(new Error("Workspace closed during startup"));
    });
    const deadline = new Promise<never>((_, reject) => {
      timer = setTimeout(() => reject(new Error(options.timeoutMessage)), options.timeoutMs ?? 10_000);
    });
    try {
      await Promise.race([
        (async () => {
          await initialize();
          if (!this.active) return;
          options.onLoaded();
          await options.ready();
        })(),
        cancelled,
        deadline,
      ]);
    } catch (reason) {
      if (this.disposed) return;
      this.failed = true;
      this.clearSubscriptions();
      const message = String(reason).replace(/^Error:\s*/, "");
      options.onLoaded();
      options.onError(message);
      // Native timeout recovery still works if the IPC notification itself fails.
      await options.failed(message).catch(() => undefined);
    } finally {
      clearTimeout(timer);
      this.cancel = undefined;
    }
  }

  dispose() {
    this.disposed = true;
    this.cancel?.();
    this.clearSubscriptions();
  }

  private clearSubscriptions() {
    for (const stop of this.subscriptions) {
      try { stop(); } catch { /* A detached webview may already have removed it. */ }
    }
    this.subscriptions.clear();
  }
}
