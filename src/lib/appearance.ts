import type { Preferences } from "$lib/domain";

export type AppearanceTheme = "lume" | "forest" | "ocean" | "violet" | "ember";

export const appearanceThemes: Array<{
  value: AppearanceTheme;
  label: string;
  accent: string;
  surface: string;
}> = [
  { value: "lume", label: "Lume", accent: "#43b47d", surface: "#14231c" },
  { value: "forest", label: "Forest", accent: "#8cbf54", surface: "#182116" },
  { value: "ocean", label: "Ocean", accent: "#4ea6d8", surface: "#101f28" },
  { value: "violet", label: "Violet", accent: "#9b82df", surface: "#1b1726" },
  { value: "ember", label: "Ember", accent: "#d58a4b", surface: "#261a13" },
];

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
  };
}
