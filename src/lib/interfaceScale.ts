import { getCurrentWebview } from "@tauri-apps/api/webview";

/** Interface size in percent; the chat keeps its own text size. */
export const interfaceScales = [80, 90, 100, 110, 120, 130, 140] as const;

export function normalizeInterfaceScale(value: number | undefined) {
  const number = Number(value);
  return Number.isFinite(number) ? Math.min(140, Math.max(80, Math.round(number))) : 100;
}

export function interfaceScaleOptions(language: "en" | "pt-BR") {
  return interfaceScales.map((value) => ({ value: String(value), label: value === 100 ? `100% · ${language === "pt-BR" ? "padrão" : "default"}` : `${value}%` }));
}

/** Zooms this window and undoes it on the chat content, which `--lume-chat-unzoom` keeps at its own size. */
export async function applyInterfaceScale(value: number | undefined) {
  if (typeof document === "undefined") return;
  const scale = normalizeInterfaceScale(value) / 100;
  document.documentElement.style.setProperty("--lume-chat-unzoom", String(1 / scale));
  try { await getCurrentWebview().setZoom(scale); } catch { /* Outside Tauri (tests, the browser) nothing is zoomed. */ }
}
