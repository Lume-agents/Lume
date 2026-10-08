import type { Preferences } from "$lib/domain";
import { appearanceFontCss } from "$lib/fonts";

export type AppearanceTheme = "lume" | "forest" | "ocean" | "violet" | "ember";

export const appearanceThemes: Array<{
  value: AppearanceTheme;
  label: string;
  accent: string;
  lightSurface: string;
  darkSurface: string;
}> = [
  { value: "lume", label: "Lume", accent: "#43b47d", lightSurface: "#e9eee8", darkSurface: "#14231c" },
  { value: "forest", label: "Forest", accent: "#8cbf54", lightSurface: "#e9eee3", darkSurface: "#182116" },
  { value: "ocean", label: "Ocean", accent: "#4ea6d8", lightSurface: "#e5edf0", darkSurface: "#101f28" },
  { value: "violet", label: "Violet", accent: "#9b82df", lightSurface: "#ece9f1", darkSurface: "#1b1726" },
  { value: "ember", label: "Ember", accent: "#d58a4b", lightSurface: "#f0e9e2", darkSurface: "#261a13" },
];

export type DarkBase = Preferences["darkBase"];
export type LightBase = Preferences["lightBase"];

type BasePigments = { canvas: string; surface: string; sidebar: string; ink: string };

/** Neutral surfaces, independent of the accent. "theme" keeps the tinted surfaces of the base theme. */
export const darkBases: Array<{ value: DarkBase; label: string; labelPt: string; pigments?: BasePigments }> = [
  { value: "theme", label: "Theme", labelPt: "Tema" },
  { value: "graphite", label: "Graphite", labelPt: "Grafite", pigments: { canvas: "#0f1011", surface: "#1a1c1e", sidebar: "#141618", ink: "#f2f3f4" } },
  { value: "black", label: "Black", labelPt: "Preto", pigments: { canvas: "#000000", surface: "#0d0e0f", sidebar: "#060707", ink: "#f4f4f4" } },
  { value: "slate", label: "Slate", labelPt: "Ardósia", pigments: { canvas: "#0b1016", surface: "#151c25", sidebar: "#10161d", ink: "#eff3f8" } },
];
export const lightBases: Array<{ value: LightBase; label: string; labelPt: string; pigments?: BasePigments }> = [
  { value: "theme", label: "Theme", labelPt: "Tema" },
  { value: "white", label: "White", labelPt: "Branco", pigments: { canvas: "#f1f2f3", surface: "#ffffff", sidebar: "#eaecee", ink: "#0f1112" } },
  { value: "gray", label: "Gray", labelPt: "Cinza", pigments: { canvas: "#e3e5e8", surface: "#f3f4f5", sidebar: "#d8dbdf", ink: "#121416" } },
  { value: "beige", label: "Beige", labelPt: "Bege", pigments: { canvas: "#eee7da", surface: "#faf6ee", sidebar: "#e5dbc9", ink: "#17130e" } },
];

export function normalizeDarkBase(value: string | undefined): DarkBase {
  return darkBases.some((base) => base.value === value) ? value as DarkBase : "theme";
}
export function normalizeLightBase(value: string | undefined): LightBase {
  return lightBases.some((base) => base.value === value) ? value as LightBase : "theme";
}

/** Inline custom properties for a neutral base; empty when both modes follow the theme. */
export function appearanceBaseCss(dark: string | undefined, light: string | undefined): string {
  const declarations: string[] = [];
  const darkPigments = darkBases.find((base) => base.value === normalizeDarkBase(dark))?.pigments;
  const lightPigments = lightBases.find((base) => base.value === normalizeLightBase(light))?.pigments;
  if (darkPigments) declarations.push(`--lume-canvas-dark:${darkPigments.canvas}`, `--lume-surface-dark:${darkPigments.surface}`, `--lume-sidebar-dark:${darkPigments.sidebar}`, `--lume-ink-seed-dark:${darkPigments.ink}`);
  if (lightPigments) declarations.push(`--lume-canvas-light:${lightPigments.canvas}`, `--lume-surface-light:${lightPigments.surface}`, `--lume-sidebar-light:${lightPigments.sidebar}`, `--lume-ink-seed-light:${lightPigments.ink}`);
  return declarations.join(";");
}

export function normalizeAppearanceTheme(value: string | undefined): AppearanceTheme {
  return appearanceThemes.some((theme) => theme.value === value)
    ? value as AppearanceTheme
    : "lume";
}

export function normalizeAccentColor(value: string | undefined): string | undefined {
  const normalized = value?.trim();
  return normalized && /^#[0-9a-f]{6}$/i.test(normalized) ? normalized.toLowerCase() : undefined;
}

export function normalizeOpacity(value: number | undefined, fallback = 100): number {
  return Number.isFinite(value)
    ? Math.max(0, Math.min(100, Math.round(value as number)))
    : fallback;
}

export function colorWithOpacity(color: string | undefined, opacity: number): string | undefined {
  const normalized = normalizeAccentColor(color);
  if (!normalized) return undefined;
  const alpha = normalizeOpacity(opacity) / 100;
  const channels = [1, 3, 5].map((index) => Number.parseInt(normalized.slice(index, index + 2), 16));
  return `rgba(${channels.join(", ")}, ${alpha})`;
}

export function appearanceAttributes(preferences: Preferences) {
  return {
    theme: normalizeAppearanceTheme(preferences.appearanceTheme),
    accent: normalizeAccentColor(preferences.accentColor),
    accentCss: colorWithOpacity(preferences.accentColor, preferences.accentOpacity),
    baseCss: [appearanceBaseCss(preferences.darkBase, preferences.lightBase), appearanceFontCss(preferences.uiFont, preferences.codeFont)].filter(Boolean).join(";"),
  };
}
