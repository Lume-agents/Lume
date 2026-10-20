import type { Backdrop, Preferences, SurfacePalette } from "$lib/domain";
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
  { value: "black", label: "Black", labelPt: "Preto", pigments: { canvas: "#000000", surface: "#000000", sidebar: "#000000", ink: "#f4f4f4" } },
  { value: "slate", label: "Slate", labelPt: "Ardósia", pigments: { canvas: "#0b1016", surface: "#151c25", sidebar: "#10161d", ink: "#eff3f8" } },
  { value: "custom", label: "Custom", labelPt: "Personalizado" },
];
export const lightBases: Array<{ value: LightBase; label: string; labelPt: string; pigments?: BasePigments }> = [
  { value: "theme", label: "Theme", labelPt: "Tema" },
  { value: "white", label: "White", labelPt: "Branco", pigments: { canvas: "#f1f2f3", surface: "#ffffff", sidebar: "#eaecee", ink: "#0f1112" } },
  { value: "gray", label: "Gray", labelPt: "Cinza", pigments: { canvas: "#e3e5e8", surface: "#f3f4f5", sidebar: "#d8dbdf", ink: "#121416" } },
  { value: "beige", label: "Beige", labelPt: "Bege", pigments: { canvas: "#eee7da", surface: "#faf6ee", sidebar: "#e5dbc9", ink: "#17130e" } },
  { value: "custom", label: "Custom", labelPt: "Personalizado" },
];

export function normalizeDarkBase(value: string | undefined): DarkBase {
  return darkBases.some((base) => base.value === value) ? value as DarkBase : "theme";
}
export function normalizeLightBase(value: string | undefined): LightBase {
  return lightBases.some((base) => base.value === value) ? value as LightBase : "theme";
}

const hexColor = (value: string | undefined) => (value && /^#[0-9a-f]{6}$/i.test(value) ? value.toLowerCase() : undefined);

/** Pigments a base contributes in one mode: a preset's own, or the colors picked for "custom" (missing ones keep the theme's). */
function basePigments(value: string | undefined, dark: boolean, custom: SurfacePalette | undefined): Partial<BasePigments> | undefined {
  if (value === "custom") {
    const palette = custom ?? {};
    return { canvas: hexColor(palette.canvas), surface: hexColor(palette.surface), sidebar: hexColor(palette.sidebar), ink: dark ? "#f2f3f4" : "#0f1112" };
  }
  return (dark ? darkBases : lightBases).find((base) => base.value === value)?.pigments;
}

/** Inline custom properties for a neutral base; empty when both modes follow the theme. */
export function appearanceBaseCss(dark: string | undefined, light: string | undefined, customDark?: SurfacePalette, customLight?: SurfacePalette): string {
  const declarations: string[] = [];
  const add = (mode: "dark" | "light", pigments: Partial<BasePigments> | undefined) => {
    if (!pigments) return;
    if (pigments.canvas) declarations.push(`--lume-canvas-${mode}:${pigments.canvas}`);
    if (pigments.surface) declarations.push(`--lume-surface-${mode}:${pigments.surface}`);
    if (pigments.sidebar) declarations.push(`--lume-sidebar-${mode}:${pigments.sidebar}`);
    if (pigments.ink) declarations.push(`--lume-ink-seed-${mode}:${pigments.ink}`);
  };
  add("dark", basePigments(normalizeDarkBase(dark), true, customDark));
  add("light", basePigments(normalizeLightBase(light), false, customLight));
  return declarations.join(";");
}

/** The surface color the active base gives panes in this mode, or undefined while the theme's own applies. */
export function baseSurfaceColor(preferences: Preferences, dark: boolean): string | undefined {
  return basePigments(dark ? normalizeDarkBase(preferences.darkBase) : normalizeLightBase(preferences.lightBase), dark, dark ? preferences.customDarkSurfaces : preferences.customLightSurfaces)?.surface;
}

/** The solid color and opacity behind the Workspace chats: a color picked for it, else the surface the base or tint gives. */
export function workspaceCanvas(preferences: Preferences, dark: boolean): { color: string; opacity: number } {
  const theme = appearanceThemes.find((item) => item.value === normalizeAppearanceTheme(preferences.appearanceTheme)) ?? appearanceThemes[0];
  const fromBase = baseSurfaceColor(preferences, dark);
  if (!dark) return { color: preferences.workspaceLightBackgroundColor ?? fromBase ?? theme.lightSurface, opacity: preferences.workspaceLightBackgroundOpacity };
  const color = preferences.workspaceDarkBackgroundColor ?? preferences.workspaceBackgroundColor ?? fromBase ?? theme.darkSurface;
  const opacity = preferences.workspaceDarkBackgroundColor
    ? preferences.workspaceDarkBackgroundOpacity
    : preferences.workspaceBackgroundColor ? preferences.workspaceBackgroundOpacity : preferences.workspaceDarkBackgroundOpacity;
  return { color, opacity };
}

export type BackdropStyleOption = { value: Backdrop["style"]; label: string; labelPt: string };
export const backdropStyles: BackdropStyleOption[] = [
  { value: "solid", label: "Solid", labelPt: "Sólido" },
  { value: "linear", label: "Linear", labelPt: "Linear" },
  { value: "radial", label: "Radial", labelPt: "Radial" },
  { value: "mesh", label: "Mesh", labelPt: "Malha" },
];

export const backdropPresets: Array<{ id: string; label: string; labelPt: string; from: string; to: string; angle: number }> = [
  { id: "aurora", label: "Aurora", labelPt: "Aurora", from: "#0b2b26", to: "#241d4f", angle: 150 },
  { id: "dusk", label: "Dusk", labelPt: "Crepúsculo", from: "#2a1633", to: "#7a3b3b", angle: 165 },
  { id: "deep", label: "Deep sea", labelPt: "Mar profundo", from: "#04121c", to: "#0e4a5c", angle: 180 },
  { id: "ember", label: "Ember", labelPt: "Brasa", from: "#160c08", to: "#6a2f14", angle: 140 },
  { id: "dawn", label: "Dawn", labelPt: "Alvorada", from: "#f7e9dc", to: "#d7e6f2", angle: 160 },
  { id: "mint", label: "Mint", labelPt: "Menta", from: "#e8f3ec", to: "#cfe9e1", angle: 150 },
];

export function normalizeBackdrop(value: Partial<Backdrop> | undefined): Backdrop {
  const style = (["solid", "linear", "radial", "mesh"] as const).find((item) => item === value?.style) ?? "solid";
  const clamp = (input: number | undefined, fallback: number, max: number) => (Number.isFinite(input) ? Math.max(0, Math.min(max, Math.round(input as number))) : fallback);
  return { style, from: hexColor(value?.from), to: hexColor(value?.to), angle: clamp(value?.angle, 160, 360), grain: clamp(value?.grain, 0, 100), vignette: clamp(value?.vignette, 0, 100), glow: clamp(value?.glow, 0, 100) };
}

/** A pigment pulled toward another one, for gradient stops derived from the surface. */
const mix = (color: string, other: string, percent: number) => `color-mix(in srgb, ${color} ${100 - percent}%, ${other})`;

/** The CSS background of the Workspace backdrop, or "" for a solid one (the plain canvas color shows). */
export function backdropBackground(backdrop: Backdrop, base: string, accent: string): string {
  if (backdrop.style === "solid") return "";
  const from = backdrop.from ?? base;
  const to = backdrop.to ?? mix(base, accent, 35);
  if (backdrop.style === "linear") return `linear-gradient(${backdrop.angle}deg, ${from}, ${to})`;
  if (backdrop.style === "radial") return `radial-gradient(120% 95% at 50% 0%, ${to}, ${from} 72%)`;
  return [
    `radial-gradient(60% 55% at 12% 8%, ${to} 0%, transparent 70%)`,
    `radial-gradient(55% 50% at 90% 18%, ${mix(to, accent, 45)} 0%, transparent 68%)`,
    `radial-gradient(70% 60% at 55% 100%, ${mix(from, accent, 25)} 0%, transparent 72%)`,
    from,
  ].join(",");
}

let grainTile = "";
/** A small tile of noise, drawn once: cheap to repeat, unlike an SVG turbulence filter. */
function grainImage(): string {
  if (grainTile || typeof document === "undefined") return grainTile;
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = 128;
  const context = canvas.getContext("2d");
  if (!context) return "";
  const image = context.createImageData(128, 128);
  let seed = 7;
  for (let index = 0; index < image.data.length; index += 4) {
    seed = (seed * 1664525 + 1013904223) >>> 0;
    const value = (seed >>> 24) & 255;
    image.data[index] = image.data[index + 1] = image.data[index + 2] = value;
    image.data[index + 3] = 255;
  }
  context.putImageData(image, 0, 0);
  grainTile = `url("${canvas.toDataURL("image/png")}")`;
  return grainTile;
}

/** The decorative layers drawn over the backdrop: accent glow, vignette and film grain, each as a CSS background. */
export function backdropEffects(backdrop: Backdrop, accent: string): { glow: string; vignette: string; grain: string; grainOpacity: number } {
  return {
    glow: backdrop.glow ? `radial-gradient(55% 42% at 50% 0%, color-mix(in srgb, ${accent} ${Math.round(backdrop.glow * 0.5)}%, transparent), transparent 70%)` : "",
    vignette: backdrop.vignette ? `radial-gradient(ellipse at center, transparent 52%, rgba(0, 0, 0, ${(backdrop.vignette / 100 * 0.7).toFixed(2)}) 100%)` : "",
    grain: backdrop.grain ? grainImage() : "",
    grainOpacity: backdrop.grain / 100 * 0.2,
  };
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
    baseCss: [appearanceBaseCss(preferences.darkBase, preferences.lightBase, preferences.customDarkSurfaces, preferences.customLightSurfaces), appearanceFontCss(preferences.uiFont, preferences.codeFont)].filter(Boolean).join(";"),
  };
}
