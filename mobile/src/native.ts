// The Capacitor bridges the Android and iOS shells add to the page. In a browser none of them exist.

type Listener = { remove: () => Promise<void> };

export type RealtimeState = { status?: "connected" | "reconnecting" | "disconnected" | string; credentialsStored?: boolean };
export type MonitoringStatus = { notificationsEnabled?: boolean; backgroundEnabled?: boolean };

export interface LumeNativePlugin {
  getStoredConnection(): Promise<{ paired: boolean; gateway: string; deviceId: string; desktopId?: string }>;
  connect(options?: { gateway: string; desktopId: string | null; deviceId: string; token: string }): Promise<RealtimeState>;
  request(options: { method: string; path: string; body: unknown }): Promise<any>;
  sendCommand(options: { command: unknown }): Promise<any>;
  clearCredentials(): Promise<void>;
  getStatus(): Promise<MonitoringStatus>;
  setMonitoringPreferences?(options: { notificationsEnabled: boolean; backgroundEnabled: boolean }): Promise<MonitoringStatus>;
  addListener(event: string, callback: (payload: any) => void): Promise<Listener>;
}

export interface LumeUpdaterPlugin {
  getInfo(): Promise<{ version?: string; openedFromUpdateNotification?: boolean }>;
  getUpdateManifest(): Promise<{ version?: string; android?: { url?: string; sha256?: string } }>;
  installUpdate(options: { url: string; sha256: string }): Promise<void>;
}

export interface LumeImagesPlugin {
  prepareImage?(options: { dataBase64: string }): Promise<{ mimeType: string; dataBase64: string; previewDataUrl: string }>;
  readClipboardImage?(): Promise<{ name: string; mimeType: string; dataBase64: string; previewDataUrl: string } | null>;
}

type CapacitorGlobal = {
  getPlatform?: () => string;
  isNativePlatform?: () => boolean;
  Plugins?: Record<string, any>;
};

const capacitor = () => (window as unknown as { Capacitor?: CapacitorGlobal }).Capacitor;

export function nativePlatform(): "android" | "ios" | "web" | string {
  return capacitor()?.getPlatform?.() || "web";
}

export function isNativeShell() {
  return Boolean(capacitor()?.isNativePlatform?.()) || nativePlatform() !== "web";
}

export function plugin<T = any>(name: string): T | null {
  return (capacitor()?.Plugins?.[name] as T | undefined) ?? null;
}

/** Android keeps the encrypted connection in a native service, so it survives the app going to the background. */
export function realtimePlugin(): LumeNativePlugin | null {
  return nativePlatform() === "android" ? plugin<LumeNativePlugin>("LumeNative") : null;
}

export function updaterPlugin(): LumeUpdaterPlugin | null {
  return nativePlatform() === "android" ? plugin<LumeUpdaterPlugin>("LumeUpdater") : null;
}

export function imagesPlugin(): LumeImagesPlugin | null {
  return nativePlatform() === "android" ? plugin<LumeImagesPlugin>("LumeImages") : null;
}
