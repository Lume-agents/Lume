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

// A webview starts at 1, so the zoom is only touched when the setting actually changes it.
let appliedScale = 1;

/** Zooms this window, and `--lume-chat-unzoom` lets the chat's text keep the size of its own setting. */
export async function applyInterfaceScale(value: number | undefined) {
  if (typeof document === "undefined") return;
  const scale = normalizeInterfaceScale(value) / 100;
  document.documentElement.style.setProperty("--lume-chat-unzoom", String(1 / scale));
  if (scale === appliedScale) return;
  try {
    await getCurrentWebview().setZoom(scale);
    appliedScale = scale;
  } catch { /* Outside Tauri (tests, the browser) nothing is zoomed. */ }
}
