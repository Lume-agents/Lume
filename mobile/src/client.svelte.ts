import type { HubCommand, HubCommandResponse, HubSession, HubSnapshot } from "$lib/hubProtocol";
import type { PromptDelivery } from "$lib/domain";
import { decodeBytes, decryptPayload, encodeBytes, encryptPayload, keyFromSecret, randomBytes, type Envelope } from "./crypto";
import { isNativeShell, nativePlatform, plugin, realtimePlugin, updaterPlugin, type MonitoringStatus, type RealtimeState } from "./native";
import { pairingIntentUrl, parsePairingTarget, suggestedDeviceName, type PairingTarget } from "./pairing";
import type { PreparedImage } from "./images";

// Everything the phone keeps between launches. The keys are the first mobile app's, so an update keeps
// the pairing, the device verification and the notification choices.
const keys = {
  token: "lume-mobile-token-v1",
  gateway: "lume-mobile-gateway-v1",
  device: "lume-mobile-device-v1",
  desktop: "lume-mobile-desktop-v1",
  biometric: "lume-mobile-biometric-v1",
  notifications: "lume-mobile-notifications-v1",
  background: "lume-mobile-background-v1",
  updateCheck: "lume-mobile-update-check-v1",
  language: "lume-mobile-language-v1",
  appearance: "lume-mobile-appearance-v1",
};
const UPDATE_INTERVAL = 6 * 60 * 60 * 1000;

const read = (key: string) => { try { return localStorage.getItem(key); } catch { return null; } };
const write = (key: string, value: string | null) => {
  try { value === null ? localStorage.removeItem(key) : localStorage.setItem(key, value); } catch { /* Private mode keeps the state for this visit only. */ }
};

export type Language = "en" | "pt-BR";
/** The phone's look, with the desktop's own options (see $lib/appearance). */
export type MobileAppearance = {
  theme: "lume" | "forest" | "ocean" | "violet" | "ember";
  mode: "system" | "dark" | "light";
  darkBase: "theme" | "graphite" | "black" | "slate";
  lightBase: "theme" | "white" | "gray" | "beige";
  accentColor: string;
  uiFont: string;
  codeFont: string;
  textSize: "small" | "default" | "large";
};
const defaultAppearance: MobileAppearance = { theme: "lume", mode: "system", darkBase: "theme", lightBase: "theme", accentColor: "", uiFont: "", codeFont: "", textSize: "default" };
function storedAppearance(): MobileAppearance {
  try { return { ...defaultAppearance, ...JSON.parse(read(keys.appearance) || "{}") }; } catch { return { ...defaultAppearance }; }
}
export type Phase = "starting" | "pair" | "empty" | "loading" | "app" | "failed";
export type Device = { id: string; name: string; scopes: string[] };
export type Banner = { message: string; kind: "info" | "error" | "success"; source: string };
type CommandInput = HubCommand | { type: "take_control_session"; sessionId: string; prompt: string; attachments: PreparedImage[] };

function compareVersions(left: string | undefined, right: string | undefined) {
  const parts = (value: string | undefined) => String(value || "").split("-")[0].split(".").map((part) => Number.parseInt(part, 10) || 0);
  const [a, b] = [parts(left), parts(right)];
  for (let index = 0; index < Math.max(a.length, b.length, 4); index += 1) {
    const difference = (a[index] || 0) - (b[index] || 0);
    if (difference) return difference;
  }
  return 0;
}

export class MobileClient {
  // Connection and pairing.
  phase = $state<Phase>("starting");
  pairingCode = $state<string | null>(null);
  apiBase = $state(read(keys.gateway) || "");
  deviceName = $state(suggestedDeviceName());
  pairing = $state(false);
  pairMessage = $state("");
  installPromptDismissed = $state(false);
  connection = $state<{ state: "online" | "offline" | "idle"; label: string }>({ state: "idle", label: "Starting" });
  banner = $state<Banner | null>(null);

  // What the desktop reports.
  snapshot = $state<HubSnapshot | null>(null);
  device = $state<Device | null>(null);
  cached = $state(false);

  // Preferences that live on the phone.
  language = $state<Language>((read(keys.language) as Language) || (navigator.language?.toLowerCase().startsWith("pt") ? "pt-BR" : "en"));
  biometric = $state(Boolean(read(keys.biometric)));
  notifications = $state(read(keys.notifications) === "on");
  appearance = $state<MobileAppearance>(storedAppearance());
  background = $state(read(keys.background) === "on");

  // Updates of the mobile app itself.
  update = $state<{ busy: boolean; detail: string; version: string; available: string | null; managed: boolean }>({ busy: false, detail: "", version: "", available: null, managed: false });
  /** A screen the app asks to show (an update notification opens Settings). */
  requestedView = $state<string | null>(null);

  // Composer state per session, so a draft survives leaving the chat.
  drafts = $state<Record<string, string>>({});
  attachments = $state<Record<string, PreparedImage[]>>({});
  deliveries = $state<Record<string, PromptDelivery>>({});
  sending = $state<Record<string, boolean>>({});
  responseImages = $state<Record<string, string>>({});

  private token = read(keys.token);
  private deviceId = read(keys.device);
  private desktopId = read(keys.desktop);
  private nativeCredentials = false;
  private transportKey: Promise<CryptoKey> | undefined;
  private lastSequence = 0;
  private realtimeConnected = false;
  private realtimeListeners = false;
  private pollTimer: ReturnType<typeof setTimeout> | undefined;
  private bannerTimer: ReturnType<typeof setTimeout> | undefined;
  private autoPairTimer: ReturnType<typeof setTimeout> | undefined;
  private previousStatuses = new Map<string, string>();
  private previousPermissions = new Map<string, string>();
  private previousQuestions = new Map<string, string>();
  private tracked = false;
  private updatesReady = false;
  private companionBusy = false;
  private companionAttempts = new Map<string, number>();
  private versionReport = { key: "", at: 0 };
  private installedVersion: string | undefined;
  private responseLoads = new Set<string>();
  private rateLimitRefreshes = new Map<string, number>();
  private manifestUrl: string | null = null;
  private manifestSha: string | null = null;

  constructor() {
    const params = new URLSearchParams(location.hash.slice(1) || location.search);
    this.pairingCode = params.get("code");
  }

  get pt() { return this.language === "pt-BR"; }
  tr = (english: string, portuguese: string) => (this.language === "pt-BR" ? portuguese : english);
  get sessions(): HubSession[] { return this.snapshot?.sessions ?? []; }
  get scopes(): string[] { return this.device?.scopes ?? []; }
  get paired() { return Boolean(this.token || this.nativeCredentials); }
  get platform() { return nativePlatform(); }
  get nativeBackground() { return nativePlatform() === "android" && Boolean(realtimePlugin()); }
  get showInstallPrompt() {
    return Boolean(this.pairingCode) && /Android/i.test(navigator.userAgent) && !isNativeShell() && !this.installPromptDismissed;
  }
  get canScan() { return isNativeShell() && nativePlatform() === "android"; }

  setAppearance(patch: Partial<MobileAppearance>) {
    this.appearance = { ...this.appearance, ...patch };
    write(keys.appearance, JSON.stringify(this.appearance));
  }

  setLanguage(language: Language) {
    this.language = language;
    write(keys.language, language);
  }

  showBanner(message: string, kind: Banner["kind"] = "info", source = "general") {
    clearTimeout(this.bannerTimer);
    this.banner = { message: String(message || this.tr("Something went wrong.", "Algo deu errado.")), kind, source };
    this.bannerTimer = setTimeout(() => { if (this.banner?.source === source) this.banner = null; }, kind === "error" ? 7000 : 4500);
  }

  hideBanner(source?: string) {
    if (source && this.banner?.source !== source) return;
    clearTimeout(this.bannerTimer);
    this.banner = null;
  }

  // ---------------------------------------------------------------- transport

  private localFetch(url: string, options: RequestInit) {
    return fetch(url, { ...options, targetAddressSpace: "local" } as RequestInit);
  }

  private async securePair(body: { code: string | null; deviceName: string }) {
    const key = await keyFromSecret(this.pairingCode || body.code || "");
    const envelope = await encryptPayload(key, { deviceName: body.deviceName, timestamp: Date.now() }, "lume-pair-request-v1");
    const response = await this.localFetch(`${this.apiBase}/api/v1/pair-secure`, {
      method: "POST", cache: "no-store", headers: { "Content-Type": "application/json" }, body: JSON.stringify(envelope),
    });
    const payload = await response.json().catch(() => ({}));
    if (!response.ok) throw new Error(payload.error?.message || `Lume returned ${response.status}`);
    return decryptPayload<any>(key, payload as Envelope, "lume-pair-response-v1");
  }

  private async secureApi(path: string, method: string, body: unknown) {
    if (!this.token || !this.deviceId) {
      await this.forget();
      throw new Error(this.tr("Pair this phone with Lume again.", "Pareie este celular com o Lume de novo."));
    }
    this.transportKey ||= keyFromSecret(this.token);
    const key = await this.transportKey;
    const requestNonce = encodeBytes(randomBytes(16));
    const envelope = await encryptPayload(key, { method, path, body, timestamp: Date.now(), requestNonce }, "lume-secure-request-v1");
    const response = await this.localFetch(`${this.apiBase}/api/v1/secure`, {
      method: "POST", cache: "no-store", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ deviceId: this.deviceId, ...envelope }),
    });
    const outer = await response.json().catch(() => ({}));
    if (!response.ok) {
      if (response.status === 401) await this.forget();
      throw new Error(outer.error?.message || `Lume returned ${response.status}`);
    }
    const secure = await decryptPayload<{ status: number; body: any }>(key, outer as Envelope, `lume-secure-response-v1:${requestNonce}`);
    if (secure.status < 200 || secure.status >= 300) {
      if (secure.status === 401) await this.forget();
      throw new Error(secure.body?.error?.message || `Lume returned ${secure.status}`);
    }
    return secure.body;
  }

  async api(path: string, method = "GET", body: unknown = null): Promise<any> {
    const realtime = this.nativeCredentials ? realtimePlugin() : null;
    if (realtime && path !== "/api/v1/pair") return realtime.request({ method, path, body: body ?? {} });
    if (this.apiBase.startsWith("http://")) {
      return path === "/api/v1/pair" ? this.securePair(body as { code: string | null; deviceName: string }) : this.secureApi(path, method, body);
    }
    const headers: Record<string, string> = { "Content-Type": "application/json" };
    if (this.token) headers.Authorization = `Bearer ${this.token}`;
    const response = await fetch(`${this.apiBase}${path}`, { method, headers, cache: "no-store", body: body === null ? undefined : JSON.stringify(body) });
    const payload = await response.json().catch(() => ({}));
    if (!response.ok) {
      if (response.status === 401 && path !== "/api/v1/pair") await this.forget();
      throw new Error(payload.error?.message || `Lume returned ${response.status}`);
    }
    return payload;
  }

  async command(command: CommandInput, { refresh = true } = {}): Promise<HubCommandResponse> {
    const request = { requestId: crypto.randomUUID?.() || `${Date.now()}-${Math.random()}`, ...command };
    const realtime = this.realtimeConnected ? realtimePlugin() : null;
    const response: HubCommandResponse = realtime
      ? await realtime.sendCommand({ command: request })
      : await this.api("/api/v1/commands", "POST", request);
    if (!response.ok) throw new Error(response.error?.message || this.tr("The command failed", "O comando falhou"));
    if (refresh) await this.refresh();
    return response;
  }

  // ---------------------------------------------------------------- lifecycle

  async start() {
    window.addEventListener("beforeinstallprompt", (event) => event.preventDefault());
    document.addEventListener("visibilitychange", () => this.onVisible());
    this.registerServiceWorker();
    if (this.token && !this.deviceId) {
      write(keys.token, null);
      this.token = null;
      this.transportKey = undefined;
    }
    try {
      await this.restoreNativeCredentials();
      const handled = await this.initializeDeepLinks();
      void this.initializeUpdates();
      if (handled) return;
      if (this.paired) await this.showDashboard();
      else this.showEntry();
    } catch (error) {
      this.phase = "failed";
      this.connection = { state: "offline", label: this.tr("Setup needed", "Configuração necessária") };
      this.showBanner((error as Error)?.message || this.tr("Lume Mobile could not finish starting. Scan the pairing QR code again.", "O Lume Mobile não terminou de abrir. Escaneie o QR de pareamento de novo."), "error", "connection");
    }
  }

  private showEntry() {
    clearTimeout(this.pollTimer);
    this.phase = this.pairingCode ? "pair" : "empty";
    this.connection = { state: "idle", label: this.tr("Not paired", "Não pareado") };
  }

  private async forget() {
    const realtime = realtimePlugin();
    const cleared = realtime ? realtime.clearCredentials().catch(() => undefined) : Promise.resolve();
    for (const key of [keys.token, keys.device, keys.desktop]) write(key, null);
    this.token = null;
    this.deviceId = null;
    this.desktopId = null;
    this.nativeCredentials = false;
    this.realtimeConnected = false;
    this.transportKey = undefined;
    this.showEntry();
    await cleared;
  }

  async disconnect() {
    await this.forget();
    write(keys.gateway, null);
    this.apiBase = "";
    this.device = null;
    this.snapshot = null;
    this.showEntry();
  }

  private async restoreNativeCredentials() {
    const realtime = realtimePlugin();
    if (!realtime || this.token) return false;
    try {
      const stored = await realtime.getStoredConnection();
      if (!stored.paired) return false;
      this.apiBase = stored.gateway;
      this.deviceId = stored.deviceId;
      this.desktopId = stored.desktopId || null;
      this.nativeCredentials = true;
      write(keys.gateway, this.apiBase);
      write(keys.device, this.deviceId);
      if (this.desktopId) write(keys.desktop, this.desktopId);
      return true;
    } catch {
      return false;
    }
  }

  private async showDashboard() {
    this.phase = "loading";
    await Promise.race([
      this.syncNotificationPreference(),
      new Promise((_, reject) => setTimeout(() => reject(new Error("Native monitoring startup timed out")), 2_000)),
    ]).catch(() => undefined);
    await Promise.race([this.initializeRealtime(), new Promise((resolve) => setTimeout(() => resolve(false), 4_000))]);
    await this.refresh();
    this.phase = "app";
    void this.poll();
  }

  async refresh() {
    try {
      const [snapshot, device] = await Promise.all([this.api("/api/v1/snapshot"), this.api("/api/v1/me")]);
      this.device = device;
      this.applySnapshot(snapshot);
      this.cached = false;
      this.connection = { state: "online", label: this.tr("Connected", "Conectado") };
      this.hideBanner("connection");
    } catch (error) {
      this.connection = { state: "offline", label: this.tr("Offline", "Offline") };
      this.showBanner((error as Error).message, "error", "connection");
    }
  }

  private async poll() {
    if (!this.paired) return;
    if (this.realtimeConnected) {
      this.pollTimer = setTimeout(() => void this.poll(), 10_000);
      return;
    }
    try {
      const payload = await this.api(`/api/v1/events?since=${this.lastSequence}`);
      const events: Array<{ sequence?: number }> = payload.events || [];
      if (events.length) {
        this.lastSequence = Math.max(this.lastSequence, ...events.map((event) => event.sequence || 0));
        await this.refresh();
      }
    } catch (error) {
      this.connection = { ...this.connection, state: "offline" };
      this.showBanner((error as Error).message, "error", "connection");
    } finally {
      this.pollTimer = setTimeout(() => void this.poll(), 1400);
    }
  }

  /** New snapshot: notify about what changed since the last one, then keep it. */
  private applySnapshot(snapshot: HubSnapshot) {
    const sessions = snapshot.sessions || [];
    if (this.tracked && this.notifications) {
      for (const session of sessions) {
        const permissionId = session.pendingPermission?.id;
        const questionId = session.pendingQuestion?.id;
        const newPermission = session.status === "permission_required" && permissionId && this.previousPermissions.get(session.id) !== permissionId;
        const transition = session.status !== "permission_required" && this.previousStatuses.get(session.id) !== session.status;
        const newQuestion = questionId && this.previousQuestions.get(session.id) !== questionId;
        if (newPermission || newQuestion || transition) {
          void this.notify(session).catch((error) => this.showBanner(error?.message || this.tr("Could not deliver the notification.", "Não foi possível mostrar a notificação."), "error"));
        }
      }
    }
    this.previousStatuses = new Map(sessions.map((session) => [session.id, session.status]));
    const open = new Set(sessions.map((session) => session.id));
    for (const session of sessions) {
      if (session.pendingPermission?.id) this.previousPermissions.set(session.id, session.pendingPermission.id);
      if (session.pendingQuestion?.id) this.previousQuestions.set(session.id, session.pendingQuestion.id);
    }
    for (const map of [this.previousPermissions, this.previousQuestions]) {
      for (const id of map.keys()) if (!open.has(id)) map.delete(id);
    }
    this.tracked = true;
    this.snapshot = snapshot;
    void this.coordinateCompanionUpdates(snapshot);
  }

  // ---------------------------------------------------------------- Android realtime service

  private applyRealtimeState = (state: RealtimeState = {}) => {
    const connected = state.status === "connected";
    const changed = this.realtimeConnected !== connected;
    this.realtimeConnected = connected;
    if (connected) {
      this.connection = { state: "online", label: this.tr("Connected", "Conectado") };
      this.hideBanner("connection");
    } else if (state.status === "reconnecting") {
      this.connection = { state: "offline", label: this.tr("Reconnecting", "Reconectando") };
    }
    if (changed && this.paired) {
      clearTimeout(this.pollTimer);
      this.pollTimer = setTimeout(() => void this.poll(), connected ? 10_000 : 250);
    }
  };

  private applyRealtimeSnapshot(snapshot: HubSnapshot | undefined, sequence?: number, cached = false) {
    if (!snapshot?.sessions) return;
    if (Number.isFinite(Number(sequence))) this.lastSequence = Math.max(this.lastSequence, Number(sequence));
    this.applySnapshot(snapshot);
    this.cached = cached;
    if (cached) {
      this.connection = { state: "offline", label: this.tr("Cached", "Em cache") };
      this.showBanner(this.tr("Showing the last state saved on this phone.", "Mostrando o último estado salvo neste celular."), "info", "connection");
    } else {
      this.connection = { state: "online", label: this.tr("Connected", "Conectado") };
      this.hideBanner("connection");
    }
  }

  private async initializeRealtime() {
    const realtime = realtimePlugin();
    if (!realtime || !this.deviceId || !this.apiBase || (!this.token && !this.nativeCredentials)) return false;
    if (!this.realtimeListeners) {
      await realtime.addListener("connectionChanged", this.applyRealtimeState);
      await realtime.addListener("sessionSnapshot", ({ snapshot, sequence, cached }) => this.applyRealtimeSnapshot(snapshot, sequence, cached));
      await realtime.addListener("sessionDelta", ({ snapshot, events = [], cached }) => {
        for (const event of events || []) this.lastSequence = Math.max(this.lastSequence, Number(event.sequence || 0));
        this.applyRealtimeSnapshot(snapshot, undefined, cached);
      });
      await realtime.addListener("streamError", ({ code, message, retryable }) => {
        if (code === "authentication_failed") {
          this.nativeCredentials = false;
          this.realtimeConnected = false;
          write(keys.device, null);
          write(keys.desktop, null);
          this.deviceId = null;
          this.desktopId = null;
          this.showEntry();
        }
        if (code === "background_service_unavailable") {
          void realtime.getStatus().then((status) => this.applyMonitoringStatus(status)).catch(() => undefined);
          this.showBanner(message || this.tr("Android could not start background monitoring.", "O Android não conseguiu iniciar o monitoramento em segundo plano."), "error", "connection");
        } else if (!retryable) {
          this.showBanner(message || this.tr("The live connection failed.", "A conexão ao vivo falhou."), "error", "connection");
        }
      });
      this.realtimeListeners = true;
    }
    try {
      const state = this.token
        ? await realtime.connect({ gateway: this.apiBase, desktopId: this.desktopId, deviceId: this.deviceId, token: this.token })
        : await realtime.connect();
      this.nativeCredentials = Boolean(state.credentialsStored || this.nativeCredentials);
      // Once Android holds the credentials in its keystore, the page forgets the token.
      if (this.nativeCredentials && this.token) {
        write(keys.token, null);
        this.token = null;
        this.transportKey = undefined;
      }
      this.applyRealtimeState(state);
      return true;
    } catch {
      this.realtimeConnected = false;
      return false;
    }
  }

  // ---------------------------------------------------------------- pairing

  private async initializeDeepLinks() {
    let handled = this.pairingCode && location.protocol === "https:" ? this.handlePairingUrl(location.href) : false;
    const app = plugin<any>("App");
    if (!app) return handled;
    try { await app.addListener("appUrlOpen", ({ url }: { url: string }) => this.handlePairingUrl(url)); } catch { /* The dashboard still starts without the deep-link bridge. */ }
    try {
      const launch = await app.getLaunchUrl();
      handled = this.handlePairingUrl(launch?.url) || handled;
    } catch { /* The warm-start listener stays active. */ }
    return handled;
  }

  handlePairingUrl(url: string | null | undefined) {
    const target = parsePairingTarget(url);
    if (!target) return false;
    this.applyPairingTarget(target);
    return true;
  }

  private applyPairingTarget(target: PairingTarget) {
    this.apiBase = target.gateway;
    if (target.token && target.deviceId) {
      this.token = target.token;
      this.deviceId = target.deviceId;
      this.desktopId = target.desktopId || null;
      this.pairingCode = null;
      this.transportKey = undefined;
      write(keys.gateway, this.apiBase);
      write(keys.token, this.token);
      write(keys.device, this.deviceId);
      if (this.desktopId) write(keys.desktop, this.desktopId);
      history.replaceState({}, "", new URL("./", location.href).pathname);
      void this.showDashboard();
      return;
    }
    this.pairingCode = target.code;
    this.desktopId = target.desktopId || null;
    this.installPromptDismissed = sessionStorage.getItem(`lume-install-prompt-dismissed:${this.pairingCode}`) === "1";
    write(keys.gateway, this.apiBase);
    this.pairMessage = "";
    this.showEntry();
    this.prepareNativeLaunch(target);
    this.scheduleAutomaticPairing();
  }

  /** In Android's browser, a pairing link first tries to open the installed app on the same pairing. */
  private prepareNativeLaunch(target: PairingTarget) {
    if (!/Android/i.test(navigator.userAgent) || isNativeShell()) return;
    const attempt = `lume-pairing-launch-${target.code}`;
    if (sessionStorage.getItem(attempt)) return;
    sessionStorage.setItem(attempt, "1");
    setTimeout(() => { location.href = pairingIntentUrl(target); }, 120);
  }

  get appLink() {
    const target = parsePairingTarget(location.href);
    return target ? pairingIntentUrl(target) : "#";
  }

  private scheduleAutomaticPairing() {
    clearTimeout(this.autoPairTimer);
    this.autoPairTimer = setTimeout(() => {
      if (!this.pairingCode || this.paired || document.hidden || this.pairing) return;
      void this.pair();
    }, nativePlatform() === "web" ? 900 : 80);
  }

  dismissInstallPrompt() {
    this.installPromptDismissed = true;
    if (this.pairingCode) sessionStorage.setItem(`lume-install-prompt-dismissed:${this.pairingCode}`, "1");
  }

  private pairingFailure(error: unknown) {
    const detail = String((error as Error)?.message || error);
    if (/failed to fetch|networkerror|load failed/i.test(detail)) {
      return nativePlatform() === "ios"
        ? this.tr("Could not reach Lume. In iPhone Settings, allow Lume to access the local network and confirm that both devices use the same Wi-Fi.", "Não foi possível alcançar o Lume. Nos Ajustes do iPhone, permita que o Lume acesse a rede local e confira se os dois aparelhos estão no mesmo Wi-Fi.")
        : this.tr("Could not reach Lume. Allow local network access and confirm that both devices use the same Wi-Fi.", "Não foi possível alcançar o Lume. Permita o acesso à rede local e confira se os dois aparelhos estão no mesmo Wi-Fi.");
    }
    return /não está ativo|expirou|not active|expired/i.test(detail)
      ? this.tr("Pairing code expired — generate a new QR code in Lume Desktop.", "O código de pareamento expirou: gere um novo QR no Lume do computador.")
      : detail;
  }

  /** Pairs with the code from the link (or typed by hand, with its gateway). */
  async pair(manual?: { gateway: string; code: string }) {
    if (this.pairing) return;
    if (manual) {
      const gateway = manual.gateway.trim().replace(/\/+$/, "");
      if (!/^https?:\/\//.test(gateway) || !manual.code.trim()) return;
      this.apiBase = gateway;
      this.pairingCode = manual.code.trim();
    }
    this.pairing = true;
    this.pairMessage = this.tr("Pairing…", "Pareando…");
    try {
      const credentials = await this.api("/api/v1/pair", "POST", { code: this.pairingCode, deviceName: this.deviceName.trim() });
      this.token = credentials.token;
      this.deviceId = credentials.device.id;
      this.desktopId = credentials.desktopId || this.desktopId;
      this.pairingCode = null;
      write(keys.gateway, this.apiBase);
      write(keys.token, this.token);
      write(keys.device, this.deviceId);
      if (this.desktopId) write(keys.desktop, this.desktopId);
      history.replaceState({}, "", new URL("./", location.href).pathname);
      this.pairMessage = "";
      await this.showDashboard();
    } catch (error) {
      this.pairMessage = this.pairingFailure(error);
    } finally {
      this.pairing = false;
    }
  }

  async scanPairingCode() {
    const scanner = plugin<any>("CapacitorBarcodeScanner");
    if (!scanner) {
      this.showBanner(this.tr("QR scanner is unavailable. Restart the app and try again.", "O leitor de QR não está disponível. Reabra o app e tente de novo."), "error");
      return;
    }
    this.pairMessage = "";
    try {
      const result = await scanner.scanBarcode({
        hint: 0,
        scanInstructions: this.tr("Scan the QR code shown in Lume Desktop", "Escaneie o QR mostrado no Lume do computador"),
        scanButton: false,
        cameraDirection: 1,
        scanOrientation: 3,
        cancelButtonAccessibilityLabel: this.tr("Cancel", "Cancelar"),
        android: { scanningLibrary: "mlkit" },
      });
      if (!this.handlePairingUrl(result.ScanResult)) throw new Error(this.tr("This is not a valid Lume pairing QR code.", "Este não é um QR de pareamento do Lume."));
    } catch (error) {
      const detail = String((error as Error)?.message || error);
      if (/cancel/i.test(detail)) this.pairMessage = this.tr("Scan cancelled.", "Leitura cancelada.");
      else this.showBanner(detail, "error");
    }
  }

  // ---------------------------------------------------------------- notifications and verification

  private async notify(session: HubSession) {
    const name = sessionName(session);
    const messages: Record<string, string> = {
      permission_required: this.tr(`${name} needs a permission decision.`, `${name} precisa de uma decisão de permissão.`),
      completed: this.tr(`${name} finished a task.`, `${name} terminou uma tarefa.`),
      failed: this.tr(`${name} reported an error.`, `${name} relatou um erro.`),
    };
    const body = session.pendingQuestion ? this.tr(`${name} is waiting for your answer.`, `${name} está esperando a sua resposta.`) : messages[session.status];
    if (!body) return;
    const key = session.pendingQuestion?.id || session.status;
    // With background monitoring Android's own service notifies; the page would only repeat it.
    if (nativePlatform() === "android" && this.nativeCredentials && realtimePlugin()) return;
    const local = plugin<any>("LocalNotifications");
    if (nativePlatform() === "android" && local) {
      let identifier = 17;
      for (const character of `${session.id}-${key}`) identifier = ((identifier * 31) + character.charCodeAt(0)) & 0x7fffffff;
      await local.schedule({ notifications: [{ id: identifier || 1, title: "Lume", body, channelId: "lume-agent-events", smallIcon: "ic_lume_notification", extra: { sessionId: session.id, status: session.status } }] });
      return;
    }
    if (!("Notification" in window) || Notification.permission !== "granted") return;
    const registration = await navigator.serviceWorker?.ready.catch(() => null);
    if (registration) registration.showNotification("Lume", { body, tag: `lume-${session.id}-${key}`, icon: "./lume-mobile-icon.svg" });
    else new Notification("Lume", { body, tag: `lume-${session.id}-${key}` });
  }

  private async syncNotificationPreference() {
    const local = plugin<any>("LocalNotifications");
    if (nativePlatform() === "android" && local && this.notifications) {
      const permission = await local.checkPermissions();
      if (permission.display !== "granted") this.setMonitoring(false, false);
    }
    await this.syncMonitoring();
  }

  private setMonitoring(notifications: boolean, background: boolean) {
    this.notifications = notifications;
    this.background = background;
    write(keys.notifications, notifications ? "on" : null);
    write(keys.background, background ? "on" : null);
  }

  private async syncMonitoring() {
    const realtime = realtimePlugin();
    if (nativePlatform() !== "android" || !realtime?.setMonitoringPreferences) return undefined;
    const status = await realtime.setMonitoringPreferences({ notificationsEnabled: this.notifications, backgroundEnabled: this.background });
    return this.applyMonitoringStatus(status);
  }

  private applyMonitoringStatus(status: MonitoringStatus = {}) {
    this.setMonitoring(Boolean(status.notificationsEnabled), Boolean(status.backgroundEnabled));
    return status;
  }

  async toggleNotifications() {
    if (this.notifications) {
      const backgroundWas = this.background;
      this.setMonitoring(false, false);
      try { await this.syncMonitoring(); } catch (error) {
        this.setMonitoring(true, backgroundWas);
        this.showBanner((error as Error)?.message || this.tr("Could not turn off notifications.", "Não foi possível desligar as notificações."), "error");
      }
      return;
    }
    const local = plugin<any>("LocalNotifications");
    if (nativePlatform() === "android" && local) {
      try {
        let permission = await local.checkPermissions();
        if (permission.display !== "granted") permission = await local.requestPermissions();
        if (permission.display !== "granted") {
          this.setMonitoring(false, this.background);
          this.showBanner(this.tr("Notification access is blocked. Enable it in Android Settings → Apps → Lume → Notifications.", "As notificações estão bloqueadas. Ative em Configurações do Android → Apps → Lume → Notificações."), "error");
          return;
        }
        await local.createChannel({ id: "lume-agent-events", name: "Agent activity", description: "Task completion, errors and permission requests", importance: 4, visibility: 1 });
        this.setMonitoring(true, this.background);
        await this.syncMonitoring();
        this.showBanner(this.tr("Notifications enabled for tasks, errors and approvals.", "Notificações ativadas para tarefas, erros e aprovações."), "success");
      } catch (error) {
        this.setMonitoring(false, false);
        void this.syncMonitoring().catch(() => undefined);
        this.showBanner((error as Error)?.message || this.tr("Could not enable Android notifications.", "Não foi possível ativar as notificações do Android."), "error");
      }
      return;
    }
    if (!("Notification" in window)) {
      this.showBanner(this.tr("Notifications are not supported in this browser.", "Este navegador não tem notificações."), "error");
      return;
    }
    const permission = await Notification.requestPermission();
    if (permission === "granted") {
      this.setMonitoring(true, this.background);
      this.showBanner(this.tr("Notifications enabled while Lume Mobile is active.", "Notificações ativadas enquanto o Lume Mobile estiver aberto."), "success");
    } else {
      this.showBanner(this.tr("Notification permission was not granted.", "A permissão de notificação não foi concedida."), "error");
    }
  }

  async toggleBackground() {
    if (!this.nativeBackground) return;
    if (this.background) {
      this.setMonitoring(this.notifications, false);
      try {
        await this.syncMonitoring();
        this.showBanner(this.tr("Background monitoring turned off.", "Monitoramento em segundo plano desligado."), "success");
      } catch (error) {
        this.setMonitoring(this.notifications, true);
        this.showBanner((error as Error)?.message || this.tr("Could not turn off background monitoring.", "Não foi possível desligar o monitoramento em segundo plano."), "error");
      }
      return;
    }
    if (!this.notifications) {
      await this.toggleNotifications();
      if (!this.notifications) return;
    }
    this.setMonitoring(true, true);
    try {
      const status = await this.syncMonitoring();
      if (!status?.backgroundEnabled) throw new Error(this.tr("Android did not enable background monitoring.", "O Android não ativou o monitoramento em segundo plano."));
      this.showBanner(this.tr("Background monitoring is on. Android will keep a quiet Lume notification visible.", "Monitoramento em segundo plano ligado. O Android vai manter uma notificação discreta do Lume."), "success");
    } catch (error) {
      this.setMonitoring(this.notifications, false);
      this.showBanner((error as Error)?.message || this.tr("Could not enable background monitoring.", "Não foi possível ligar o monitoramento em segundo plano."), "error");
    }
  }

  async toggleBiometric() {
    if (this.biometric) {
      if (confirm(this.tr("Turn off device verification for remote actions?", "Desligar a verificação do aparelho para ações remotas?"))) {
        write(keys.biometric, null);
        this.biometric = false;
      }
      return;
    }
    if (!window.PublicKeyCredential) {
      this.showBanner(this.tr("Device verification is not supported in this browser.", "Este navegador não suporta a verificação do aparelho."), "error");
      return;
    }
    try {
      const credential = (await navigator.credentials.create({
        publicKey: {
          challenge: randomBytes(32),
          rp: { name: "Lume Local" },
          user: { id: randomBytes(16), name: "lume-mobile", displayName: this.device?.name || "Lume Mobile" },
          pubKeyCredParams: [{ type: "public-key", alg: -7 }, { type: "public-key", alg: -257 }],
          authenticatorSelection: { authenticatorAttachment: "platform", residentKey: "preferred", userVerification: "required" },
          timeout: 60000,
          attestation: "none",
        },
      })) as PublicKeyCredential;
      write(keys.biometric, encodeBytes(credential.rawId));
      this.biometric = true;
      this.showBanner(this.tr("Device verification enabled for remote actions.", "Verificação do aparelho ativada para ações remotas."), "success");
    } catch (error) {
      this.showBanner((error as Error).name === "NotAllowedError"
        ? this.tr("Device verification was cancelled.", "A verificação foi cancelada.")
        : this.tr("Could not enable device verification.", "Não foi possível ativar a verificação."), "error");
    }
  }

  /** Remote actions (prompts, approvals, stopping) ask for the fingerprint or face when verification is on. */
  async verify() {
    const encoded = read(keys.biometric);
    if (!encoded) return true;
    try {
      await navigator.credentials.get({
        publicKey: {
          challenge: randomBytes(32),
          allowCredentials: [{ type: "public-key", id: decodeBytes(encoded), transports: ["internal"] }],
          userVerification: "required",
          timeout: 60000,
        },
      });
      return true;
    } catch {
      this.showBanner(this.tr("Device verification was not completed.", "A verificação do aparelho não foi concluída."), "error");
      return false;
    }
  }

  // ---------------------------------------------------------------- updates of the phone app

  private async initializeUpdates() {
    const updater = updaterPlugin();
    if (nativePlatform() === "android" && updater) {
      try {
        const info = await updater.getInfo();
        this.installedVersion = info.version;
        if (info.openedFromUpdateNotification) this.requestedView = "settings";
        this.update.version = info.version ? this.tr(`Installed version ${info.version}`, `Versão instalada ${info.version}`) : "";
      } catch {
        this.update.version = this.tr("Android application", "Aplicativo Android");
      }
    }
    const last = Number(read(keys.updateCheck) || 0);
    if (Date.now() - last >= UPDATE_INTERVAL) await this.checkUpdate();
    else if (nativePlatform() === "web") {
      this.update.version = this.tr("Web app", "App web");
      this.update.detail = this.tr("The web app updates automatically.", "O app web se atualiza sozinho.");
    }
    setInterval(() => void this.checkUpdate(), UPDATE_INTERVAL);
    this.updatesReady = true;
    if (this.snapshot) void this.coordinateCompanionUpdates(this.snapshot);
  }

  async checkUpdate(manual = false): Promise<string> {
    if (this.update.busy) return "busy";
    const platform = nativePlatform();
    if (platform === "web") {
      this.update.busy = true;
      this.update.detail = this.tr("Checking the installed web app…", "Verificando o app web instalado…");
      try {
        const registration = await navigator.serviceWorker?.ready;
        await registration?.update();
        this.update.detail = this.tr("The web app updates automatically.", "O app web se atualiza sozinho.");
        this.update.version = this.tr("Web app", "App web");
        return "up_to_date";
      } catch {
        this.update.detail = this.tr("Could not check the web app right now.", "Não foi possível verificar o app web agora.");
        return "error";
      } finally {
        this.update.busy = false;
      }
    }
    if (platform === "ios") {
      this.update.detail = this.tr("Updates are managed securely by TestFlight or the App Store.", "As atualizações chegam pelo TestFlight ou pela App Store.");
      this.update.version = this.tr("iOS application", "Aplicativo iOS");
      this.update.managed = true;
      return "managed";
    }
    const updater = updaterPlugin();
    if (!updater) {
      this.update.detail = this.tr("The native updater is unavailable in this build.", "O atualizador nativo não está disponível nesta versão.");
      this.update.managed = true;
      return "unavailable";
    }
    this.update.busy = true;
    this.update.detail = this.tr("Checking for a new version…", "Procurando uma versão nova…");
    try {
      const info = await updater.getInfo();
      this.installedVersion = info.version;
      if (info.openedFromUpdateNotification) this.requestedView = "settings";
      if (info.version) this.update.version = this.tr(`Installed version ${info.version}`, `Versão instalada ${info.version}`);
      const manifest = await updater.getUpdateManifest();
      if (!manifest.version || !manifest.android?.url || !/^[a-f0-9]{64}$/i.test(manifest.android?.sha256 || "")) {
        throw new Error(this.tr("Invalid mobile update manifest", "Manifesto de atualização inválido"));
      }
      write(keys.updateCheck, String(Date.now()));
      if (compareVersions(manifest.version, info.version) > 0) {
        this.update.available = manifest.version;
        this.manifestUrl = manifest.android.url;
        this.manifestSha = manifest.android.sha256!;
        this.update.detail = this.tr(`Version ${manifest.version} is ready to install.`, `A versão ${manifest.version} está pronta para instalar.`);
        return "available";
      }
      this.update.available = null;
      this.update.detail = this.tr("You are using the latest version.", "Você está na versão mais recente.");
      return "up_to_date";
    } catch (error) {
      this.update.detail = manual
        ? (error as Error)?.message || this.tr("Could not check for updates right now.", "Não foi possível procurar atualizações agora.")
        : this.tr("Automatic update checks will retry later.", "A verificação automática tenta de novo mais tarde.");
      return "error";
    } finally {
      this.update.busy = false;
    }
  }

  async installUpdate() {
    const updater = updaterPlugin();
    if (!updater || !this.update.available || !this.manifestUrl || !this.manifestSha || this.update.busy) return;
    this.update.busy = true;
    this.update.detail = this.tr("Downloading and verifying the update…", "Baixando e verificando a atualização…");
    try {
      await updater.installUpdate({ url: this.manifestUrl, sha256: this.manifestSha });
      this.update.detail = this.tr("Complete the update in the Android installer.", "Conclua a atualização no instalador do Android.");
    } catch (error) {
      this.update.detail = (error as { code?: string }).code === "INSTALL_PERMISSION_REQUIRED"
        ? this.tr("Allow Lume to install updates, then return and tap Install again.", "Permita que o Lume instale atualizações, volte e toque em Instalar de novo.")
        : (error as Error).message || this.tr("The update could not be installed.", "Não foi possível instalar a atualização.");
    } finally {
      this.update.busy = false;
    }
  }

  /** The phone app follows the desktop: an older phone updates itself, a newer one tells the desktop. */
  private async coordinateCompanionUpdates(snapshot: HubSnapshot) {
    if (nativePlatform() !== "android" || !this.updatesReady || this.companionBusy || !snapshot.desktopVersion) return;
    const updater = updaterPlugin();
    if (!updater) return;
    try { this.installedVersion ||= (await updater.getInfo()).version; } catch { return; }
    const mobile = this.installedVersion;
    const desktop = snapshot.desktopVersion;
    if (!mobile) return;
    if (compareVersions(desktop, mobile) > 0) {
      const key = `mobile:${mobile}->${desktop}`;
      if (Date.now() - (this.companionAttempts.get(key) || 0) < 5 * 60_000) return;
      this.companionAttempts.set(key, Date.now());
      this.companionBusy = true;
      try {
        if ((await this.checkUpdate(true)) === "available") {
          this.requestedView = "settings";
          await this.installUpdate();
        }
      } finally {
        this.companionBusy = false;
      }
      return;
    }
    if (compareVersions(mobile, desktop) > 0) {
      const key = `${mobile}->${desktop}`;
      if (this.versionReport.key === key && Date.now() - this.versionReport.at < 60_000) return;
      this.versionReport = { key, at: Date.now() };
      try { await this.command({ type: "report_mobile_version", version: mobile }); } catch { /* The next snapshot retries. */ }
    }
  }

  private onVisible() {
    if (document.hidden) return;
    if (this.pairingCode && !this.paired) this.scheduleAutomaticPairing();
    if (nativePlatform() !== "android") return;
    const realtime = realtimePlugin();
    if (realtime?.getStatus) void realtime.getStatus().then((status) => this.applyMonitoringStatus(status)).catch(() => undefined);
    if (Date.now() - Number(read(keys.updateCheck) || 0) >= UPDATE_INTERVAL) {
      void this.checkUpdate();
      return;
    }
    void updaterPlugin()?.getInfo().then((info) => {
      if (!info.openedFromUpdateNotification) return;
      this.requestedView = "settings";
      void this.checkUpdate(true);
    }).catch(() => undefined);
  }

  private registerServiceWorker() {
    if (!("serviceWorker" in navigator)) return;
    let refreshing = false;
    navigator.serviceWorker.addEventListener("controllerchange", () => {
      if (refreshing || !navigator.serviceWorker.controller) return;
      refreshing = true;
      location.reload();
    });
    navigator.serviceWorker.register("./sw.js").then((registration) => registration.update()).catch(() => undefined);
  }

  // ---------------------------------------------------------------- session actions

  canPrompt(session: HubSession) {
    return this.scopes.includes("prompt") && (session.capabilities?.canPrompt || session.capabilities?.canTakeControl);
  }

  /** Sends the composer's prompt; a session open in someone else's terminal asks to take it over first. */
  async sendPrompt(session: HubSession, takeoverConfirmed = false): Promise<"sent" | "takeover" | "skipped"> {
    const canTakeControl = this.scopes.includes("prompt") && Boolean(session.capabilities?.canTakeControl);
    const steers = session.capabilities?.promptDeliveries?.includes("steer");
    if (session.status === "running" && !steers && !canTakeControl) return "skipped";
    const prompt = (this.drafts[session.id] || "").trim();
    const attachments = this.attachments[session.id] || [];
    if ((!prompt && !attachments.length) || this.sending[session.id]) return "skipped";
    if (canTakeControl && !takeoverConfirmed) return "takeover";
    if (!(await this.verify())) return "skipped";
    const delivery = session.status === "running" ? this.deliveries[session.id] || "queue" : "new_turn";
    this.sending = { ...this.sending, [session.id]: true };
    try {
      await this.command(canTakeControl
        ? { type: "take_control_session", sessionId: session.id, prompt, attachments }
        : { type: "submit_prompt", sessionId: session.id, prompt, attachments, delivery });
      this.drafts = { ...this.drafts, [session.id]: "" };
      this.attachments = { ...this.attachments, [session.id]: [] };
      return "sent";
    } catch (error) {
      this.showBanner((error as Error).message, "error");
      return "skipped";
    } finally {
      this.sending = { ...this.sending, [session.id]: false };
    }
  }

  async resolvePermission(session: HubSession, action: string) {
    if (!session.pendingPermission || !(await this.verify())) return;
    try {
      await this.command({ type: "resolve_permission", sessionId: session.id, permissionId: session.pendingPermission.id, action: action as never });
    } catch (error) {
      this.showBanner((error as Error).message, "error");
    }
  }

  async answerQuestion(session: HubSession, answers: Array<{ questionId: string; answers: string[] }>) {
    if (!session.pendingQuestion || !(await this.verify())) return false;
    try {
      await this.command({ type: "resolve_question", sessionId: session.id, questionId: session.pendingQuestion.id, answers });
      return true;
    } catch (error) {
      this.showBanner((error as Error).message, "error");
      return false;
    }
  }

  async stop(session: HubSession) {
    const interrupt = this.scopes.includes("terminate") && session.capabilities?.canInterrupt;
    if (!interrupt && !confirm(this.tr("Stop this agent and its commands?", "Parar este agente e os comandos dele?"))) return;
    if (!(await this.verify())) return;
    try {
      if (interrupt) {
        await this.command({ type: "interrupt_prompt", sessionId: session.id });
        this.showBanner(this.tr("Prompt interrupted.", "Prompt interrompido."), "success");
      } else {
        await this.command({ type: "terminate_session", sessionId: session.id });
        this.showBanner(this.tr("Agent stopped.", "Agente parado."), "success");
      }
    } catch (error) {
      this.showBanner((error as Error).message, "error");
    }
  }

  private async responseFile(sessionId: string, attachmentId: string) {
    const response = await this.command({ type: "download_response_file", sessionId, attachmentId }, { refresh: false });
    const data = response.data as { dataBase64?: string; mimeType?: string; name?: string } | undefined;
    if (!data?.dataBase64) throw new Error(this.tr("The response file is unavailable", "O arquivo da resposta não está disponível"));
    return data as { dataBase64: string; mimeType?: string; name?: string };
  }

  /** Images an agent attached to a reply are fetched once and kept as data URLs. */
  async loadResponseImages(session: HubSession) {
    const images = (session.activities || [])
      .filter((activity) => activity.kind === "message")
      .flatMap((activity) => activity.attachments || [])
      .filter((attachment) => attachment?.id && attachment.path && String(attachment.mimeType || "").startsWith("image/")
        && !(attachment.id in this.responseImages) && !this.responseLoads.has(attachment.id));
    for (const attachment of images) {
      this.responseLoads.add(attachment.id);
      try {
        const file = await this.responseFile(session.id, attachment.id);
        this.responseImages = { ...this.responseImages, [attachment.id]: `data:${file.mimeType || attachment.mimeType};base64,${file.dataBase64}` };
      } catch {
        this.responseImages = { ...this.responseImages, [attachment.id]: "" };
      } finally {
        this.responseLoads.delete(attachment.id);
      }
    }
  }

  async downloadResponseFile(sessionId: string, attachmentId: string) {
    try {
      const file = await this.responseFile(sessionId, attachmentId);
      const binary = atob(file.dataBase64);
      const bytes = Uint8Array.from(binary, (character) => character.charCodeAt(0));
      const url = URL.createObjectURL(new Blob([bytes], { type: file.mimeType || "application/octet-stream" }));
      const anchor = Object.assign(document.createElement("a"), { href: url, download: file.name || "lume-response-file" });
      document.body.append(anchor);
      anchor.click();
      anchor.remove();
      setTimeout(() => URL.revokeObjectURL(url), 1_000);
    } catch (error) {
      this.showBanner((error as Error)?.message || this.tr("Could not download the response file.", "Não foi possível baixar o arquivo da resposta."), "error");
    }
  }

  /** Codex reports its limits only when asked; the chat asks at most once a minute. */
  async refreshRateLimits(session: HubSession) {
    if (session.agent !== "codex") return;
    if (Date.now() - (this.rateLimitRefreshes.get(session.agent) || 0) < 60_000) return;
    this.rateLimitRefreshes.set(session.agent, Date.now());
    try { await this.command({ type: "refresh_rate_limits", agent: session.agent }); } catch { this.rateLimitRefreshes.delete(session.agent); }
  }

  async workflow(type: "approve_workflow_handoff" | "advance_workflow" | "pause_workflow" | "resume_workflow" | "retry_workflow_step" | "skip_workflow_step" | "cancel_workflow", workflowId: string) {
    if (!(await this.verify())) return;
    try { await this.command({ type, workflowId }); } catch (error) { this.showBanner((error as Error).message, "error"); }
  }
}

export function sessionName(session: HubSession) {
  const custom = String(session.sessionName || "").trim();
  return custom || String(session.project || "").trim() || String(session.agentLabel || "AI").trim();
}
