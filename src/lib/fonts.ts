import { invoke } from "@tauri-apps/api/core";
import type { Preferences } from "$lib/domain";

export type FontChoice = { value: string; label: string; labelPt: string; stack?: string };
export type CustomFont = { id: string; name: string; file: string };

const uiFallback = 'system-ui, -apple-system, "Segoe UI", sans-serif';
const codeFallback = 'ui-monospace, "SF Mono", "Cascadia Mono", Consolas, "Liberation Mono", monospace';

/** Interface fonts. "default" keeps the stack each surface already has. */
export const uiFonts: FontChoice[] = [
  { value: "default", label: "Default", labelPt: "Padrão" },
  { value: "inter", label: "Inter", labelPt: "Inter", stack: `"Lume Inter", ${uiFallback}` },
  { value: "plex-sans", label: "IBM Plex Sans", labelPt: "IBM Plex Sans", stack: `"Lume Plex Sans", ${uiFallback}` },
  { value: "source-serif", label: "Source Serif", labelPt: "Source Serif", stack: '"Lume Source Serif", Georgia, "Times New Roman", serif' },
  { value: "system", label: "System", labelPt: "Sistema", stack: uiFallback },
];

/** Fonts for code, diffs and terminal text. */
export const codeFonts: FontChoice[] = [
  { value: "default", label: "Default", labelPt: "Padrão" },
  { value: "jetbrains-mono", label: "JetBrains Mono", labelPt: "JetBrains Mono", stack: `"Lume JetBrains Mono", ${codeFallback}` },
  { value: "fira-code", label: "Fira Code", labelPt: "Fira Code", stack: `"Lume Fira Code", ${codeFallback}` },
  { value: "plex-mono", label: "IBM Plex Mono", labelPt: "IBM Plex Mono", stack: `"Lume Plex Mono", ${codeFallback}` },
  { value: "system", label: "System mono", labelPt: "Mono do sistema", stack: codeFallback },
];

export const customFamily = (id: string) => `Lume Custom ${id}`;

function stackFor(value: string | undefined, choices: FontChoice[], fallback: string): string | undefined {
  if (!value || value === "default") return undefined;
  if (value.startsWith("custom:")) {
    const id = value.slice(7);
    return /^[a-z0-9-]{1,48}$/.test(id) ? `"${customFamily(id)}", ${fallback}` : undefined;
  }
  return choices.find((choice) => choice.value === value)?.stack;
}

/** Inline custom properties for the chosen fonts; empty when both stay on their defaults. */
export function appearanceFontCss(ui: string | undefined, code: string | undefined): string {
  const declarations: string[] = [];
  const interfaceStack = stackFor(ui, uiFonts, uiFallback);
  const codeStack = stackFor(code, codeFonts, codeFallback);
  if (interfaceStack) declarations.push(`--lume-font-ui:${interfaceStack}`);
  if (codeStack) declarations.push(`--lume-font-code:${codeStack}`);
  return declarations.join(";");
}

export function normalizeFont(value: string | undefined): string {
  return typeof value === "string" && /^(default|system|[a-z0-9-]{1,32}|custom:[a-z0-9-]{1,48})$/.test(value) ? value : "default";
}

const inDesktop = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
const registered = new Set<string>();

export async function listCustomFonts(): Promise<CustomFont[]> {
  return inDesktop() ? invoke<CustomFont[]>("list_custom_fonts") : [];
}
export const importCustomFont = (path: string) => invoke<CustomFont>("import_custom_font", { path });
export const removeCustomFont = (id: string) => invoke<void>("remove_custom_font", { id });

/** Registers an imported font in this window, once. The bytes come from the app data folder. */
export async function registerCustomFont(id: string): Promise<boolean> {
  if (registered.has(id) || !inDesktop()) return registered.has(id);
  try {
    const bytes = await invoke<ArrayBuffer>("read_custom_font", { id });
    const face = new FontFace(customFamily(id), bytes);
    await face.load();
    document.fonts.add(face);
    registered.add(id);
    return true;
  } catch {
    return false;
  }
}

/** Makes sure the imported fonts the preferences point at are available to this window. */
export async function ensureCustomFonts(preferences: Pick<Preferences, "uiFont" | "codeFont">): Promise<void> {
  for (const value of [preferences.uiFont, preferences.codeFont]) {
    if (value?.startsWith("custom:")) await registerCustomFont(value.slice(7));
  }
}
