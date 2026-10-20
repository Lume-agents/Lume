export type UpdateStatus = "idle" | "checking" | "up_to_date" | "available" | "downloading" | "ready" | "error";
export type UpdateProgress = { event: "Started"; data: { contentLength?: number } }
  | { event: "Progress"; data: { chunkLength: number } } | { event: "Finished" };
export type InstallableUpdate = {
  version: string;
  body?: string;
  downloadAndInstall: (onProgress: (event: UpdateProgress) => void) => Promise<void>;
  close: () => Promise<void>;
};
export type ReleaseNotes = { version: string; body: string; url: string; loading: boolean; error: string };
export type AppUpdateState = {
  status: UpdateStatus;
  currentVersion: string;
  availableVersion: string | null;
  progress: number | null;
  error: string;
  errorOperation: "check" | "install" | "restart" | null;
  notify: boolean;
  dismissedVersion: string | null;
  notes: ReleaseNotes | null;
};
export const releaseNotesSeenKey = "lume:updates:release-notes-seen";
export const releaseNotesCacheKey = "lume:updates:release-notes-cache";
export const releaseUrl = (version: string) => `https://github.com/Lume-agents/Lume/releases/tag/v${encodeURIComponent(version)}`;
export const initialUpdateState = (version: string): AppUpdateState => ({
  status: "idle", currentVersion: version, availableVersion: null, progress: null,
  error: "", errorOperation: null, notify: false, dismissedVersion: null, notes: null,
});

type UpdateDependencies = {
  version: string;
  getVersion: () => Promise<string>;
  check: () => Promise<InstallableUpdate | null>;
  loadReleaseBody: (version: string) => Promise<string>;
  relaunch: () => Promise<void>;
  storage: Pick<Storage, "getItem" | "setItem">;
};

/** The main window owns native updater resources; other windows receive snapshots. */
export class AppUpdateController {
  state: AppUpdateState;
  private listeners = new Set<(state: AppUpdateState) => void>();
  private pending: InstallableUpdate | null = null;
  private checking: Promise<void> | null = null;
  private starting: Promise<void> | null = null;
  private notesRevision = 0;
  private disposed = false;
  private dependencies: UpdateDependencies;

  constructor(dependencies: UpdateDependencies) {
    this.dependencies = dependencies;
    this.state = initialUpdateState(dependencies.version);
  }

  subscribe(listener: (state: AppUpdateState) => void) {
    this.listeners.add(listener);
    listener(this.state);
    return () => { this.listeners.delete(listener); };
  }

  private patch(patch: Partial<AppUpdateState>) {
    if (this.disposed) return;
    this.state = { ...this.state, ...patch };
    for (const listener of this.listeners) listener(this.state);
  }

  private read(key: string) {
    try { return this.dependencies.storage.getItem(key); } catch { return null; }
  }

  private write(key: string, value: string) {
    try { this.dependencies.storage.setItem(key, value); } catch { /* Updates still work without storage. */ }
  }

  initialize(): Promise<void> {
    return this.starting ??= (async () => {
      try { this.patch({ currentVersion: await this.dependencies.getVersion() }); }
      catch { /* Use the actual package version, never a historical hard-coded version. */ }
      if (this.disposed) return;
      if (this.read(releaseNotesSeenKey) !== this.state.currentVersion) void this.loadNotes();
      await this.check();
    })();
  }

  check(notifyErrors = false): Promise<void> {
    if (this.checking) return this.checking;
    if (this.disposed || ["downloading", "ready"].includes(this.state.status)) return Promise.resolve();
    this.patch({ status: "checking", error: "", errorOperation: null, progress: null });
    this.checking = Promise.resolve().then(async () => {
      try {
        const next = await this.dependencies.check();
        if (this.disposed) { await next?.close().catch(() => undefined); return; }
        const previous = this.pending;
        this.pending = next;
        if (previous && previous !== next) void previous.close().catch(() => undefined);
        this.patch({ status: next ? "available" : "up_to_date", availableVersion: next?.version ?? null, notify: !!next });
      } catch (error) {
        this.patch({ status: "error", error: String(error).replace(/^Error:\s*/, ""), errorOperation: "check", notify: notifyErrors });
      } finally { this.checking = null; }
    });
    return this.checking;
  }

  async install() {
    if (!this.pending || !["available", "error"].includes(this.state.status) || this.state.errorOperation === "check") return;
    const update = this.pending;
    // Cache before the installer can exit the process (notably on Windows).
    if (update.body?.trim()) this.write(releaseNotesCacheKey, JSON.stringify({ version: update.version, body: update.body }));
    this.patch({ status: "downloading", progress: null, error: "", errorOperation: null, notify: true, dismissedVersion: null });
    let downloaded = 0;
    let total: number | undefined;
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") {
          total = event.data.contentLength;
          this.patch({ progress: total && total > 0 ? 0 : null });
        } else if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          this.patch({ progress: total && total > 0 ? Math.min(99, Math.round(downloaded / total * 100)) : null });
        }
        // Finished is only the end of downloading, not proof of a successful install.
      });
      this.patch({ status: "ready", progress: 100 });
      await this.restart();
    } catch (error) {
      this.patch({ status: "error", error: String(error).replace(/^Error:\s*/, ""), errorOperation: "install", progress: null });
    }
  }

  async restart() {
    if (this.disposed || this.state.status !== "ready") return;
    try { await this.dependencies.relaunch(); }
    catch (error) {
      this.patch({ error: String(error).replace(/^Error:\s*/, ""), errorOperation: "restart", notify: true });
    }
  }

  dismissNotice() {
    this.patch({ dismissedVersion: this.state.availableVersion, notify: false });
  }

  async loadNotes() {
    if (this.disposed) return;
    const version = this.state.currentVersion;
    const revision = ++this.notesRevision;
    let cached = "";
    try {
      const value = JSON.parse(this.read(releaseNotesCacheKey) ?? "null");
      if (value?.version === version && typeof value.body === "string") cached = value.body;
    } catch { /* Ignore stale or corrupt cached descriptions. */ }
    this.patch({ notes: { version, url: releaseUrl(version), body: cached, loading: !cached, error: "" } });
    try {
      // Fetch this installed tag, never /latest (which may already describe a newer release).
      const body = await this.dependencies.loadReleaseBody(version);
      if (revision !== this.notesRevision || this.disposed) return;
      if (!body.trim()) throw new Error("Release description is empty");
      this.write(releaseNotesCacheKey, JSON.stringify({ version, body }));
      this.patch({ notes: { version, url: releaseUrl(version), body, loading: false, error: "" } });
    } catch (error) {
      if (revision !== this.notesRevision) return;
      this.patch({ notes: { version, url: releaseUrl(version), body: cached, loading: false, error: cached ? "" : String(error).replace(/^Error:\s*/, "") } });
    }
  }

  dismissNotes() {
    const notes = this.state.notes;
    // An offline/error placeholder does not count as having read the patch notes.
    if (notes?.body.trim()) this.write(releaseNotesSeenKey, notes.version);
    ++this.notesRevision;
    this.patch({ notes: null });
  }

  dispose() {
    this.disposed = true;
    ++this.notesRevision;
    this.listeners.clear();
    void this.pending?.close().catch(() => undefined);
    this.pending = null;
  }
}
