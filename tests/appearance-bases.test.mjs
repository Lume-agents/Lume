import assert from "node:assert/strict";
import { registerHooks } from "node:module";
import test from "node:test";

registerHooks({
  resolve(specifier, context, nextResolve) {
    if (specifier.startsWith("$lib/")) {
      const path = specifier.slice(5);
      return nextResolve(new URL(`../src/lib/${path}${/\.[jt]s$/.test(path) ? "" : ".ts"}`, import.meta.url).href, context);
    }
    return nextResolve(specifier, context);
  },
});
const { appearanceBaseCss, darkBases, lightBases, normalizeDarkBase, normalizeLightBase } = await import("../src/lib/appearance.ts");

const channel = (hex) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255).map((c) => (c <= .04045 ? c / 12.92 : ((c + .055) / 1.055) ** 2.4));
const luminance = (hex) => { const [r, g, b] = channel(hex); return r * .2126 + g * .7152 + b * .0722; };
const contrast = (a, b) => (Math.max(luminance(a), luminance(b)) + .05) / (Math.min(luminance(a), luminance(b)) + .05);

test("the theme base adds nothing and unknown values fall back to it", () => {
  assert.equal(appearanceBaseCss("theme", "theme"), "");
  assert.equal(appearanceBaseCss(undefined, "nope"), "");
  assert.equal(normalizeDarkBase("white"), "theme");
  assert.equal(normalizeLightBase("graphite"), "theme");
});

test("a neutral base overrides only its own mode", () => {
  const dark = appearanceBaseCss("graphite", "theme");
  assert.match(dark, /--lume-surface-dark:#1a1c1e/);
  assert.doesNotMatch(dark, /-light:/);
  const light = appearanceBaseCss("theme", "beige");
  assert.match(light, /--lume-surface-light:#faf6ee/);
  assert.doesNotMatch(light, /-dark:/);
});

test("every neutral base keeps its ink readable on its surface", () => {
  for (const base of [...darkBases, ...lightBases].filter((item) => item.pigments)) {
    for (const surface of [base.pigments.canvas, base.pigments.surface, base.pigments.sidebar]) {
      assert.ok(contrast(base.pigments.ink, surface) >= 7, `${base.value}: ink on ${surface}`);
    }
  }
});

const { appearanceFontCss, normalizeFont, codeFonts, uiFonts } = await import("../src/lib/fonts.ts");

test("fonts: defaults add nothing, bundled and imported fonts become stacks", () => {
  assert.equal(appearanceFontCss("default", undefined), "");
  assert.match(appearanceFontCss("source-serif", "jetbrains-mono"), /--lume-font-ui:"Lume Source Serif".*serif;--lume-font-code:"Lume JetBrains Mono".*monospace/);
  assert.match(appearanceFontCss("default", "custom:fira-retina"), /--lume-font-code:"Lume Custom fira-retina", /);
  assert.equal(appearanceFontCss("custom:../x", "nope"), "", "unsafe or unknown ids are ignored");
});

test("fonts: only safe ids survive normalization", () => {
  assert.equal(normalizeFont("inter"), "inter");
  assert.equal(normalizeFont("custom:my-font"), "custom:my-font");
  assert.equal(normalizeFont("Inter; color:red"), "default");
  assert.equal(normalizeFont(undefined), "default");
  for (const choice of [...uiFonts, ...codeFonts]) assert.equal(normalizeFont(choice.value), choice.value);
});
