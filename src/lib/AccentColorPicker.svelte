<script lang="ts">
  import { onMount, tick } from "svelte";
  import { cubicOut } from "svelte/easing";
  import { fly } from "svelte/transition";
  import LumeIcon from "$lib/LumeIcon.svelte";
  import { copyResolvedColorTokens } from "$lib/floatingTheme";
  import type { Language } from "$lib/i18n";

  const defaultColors = [
    "#43b47d", "#75b65a", "#4ea6d8", "#687fd2",
    "#9b82df", "#c873a5", "#d06f69", "#d58a4b",
  ];

  let {
    value,
    opacity = 100,
    fallback = "#43b47d",
    language = "en",
    label,
    readyColors = defaultColors,
    minimumOpacity = 20,
    onValueChange,
    onReset,
  } = $props<{
    value?: string;
    opacity?: number;
    fallback?: string;
    language?: Language;
    label?: string;
    readyColors?: string[];
    minimumOpacity?: number;
    onValueChange: (value: string, opacity: number) => void;
    onReset: () => void;
  }>();

  let root = $state<HTMLDivElement | null>(null);
  let trigger = $state<HTMLButtonElement | null>(null);
  let popoverNode: HTMLDivElement | null = null;
  let open = $state(false);
  let customOpen = $state(false);
  let left = $state(8);
  let top = $state(8);
  let above = $state(false);
  let draft = $state("#43b47d");
  let hue = $state(145);
  let lightness = $state(48);
  let draftOpacity = $state(100);
  let reducedMotion = $state(false);

  const tr = (english: string, portuguese: string) => language === "pt-BR" ? portuguese : english;
  const normalizedValue = $derived(normalizeHex(value) ?? normalizeHex(fallback) ?? "#43b47d");
  const displayOpacity = $derived(clamp(opacity, minimumOpacity, 100));
  const previewColor = $derived(hexToRgba(draft, draftOpacity));
  const triggerColor = $derived(hexToRgba(normalizedValue, displayOpacity));
  const dialogLabel = $derived(label ?? tr("Color", "Cor"));

  $effect(() => {
    if (open) return;
    resetDraft();
  });

  function clamp(value: number, min: number, max: number) {
    return Math.max(min, Math.min(max, Number.isFinite(value) ? Math.round(value) : max));
  }

  function normalizeHex(input?: string) {
    const raw = input?.trim();
    if (!raw) return null;
    const expanded = /^#[0-9a-f]{3}$/i.test(raw)
      ? `#${raw.slice(1).split("").map((part) => part + part).join("")}`
      : raw;
    return /^#[0-9a-f]{6}$/i.test(expanded) ? expanded.toLowerCase() : null;
  }

  function hslToHex(h: number, s: number, l: number) {
    const saturation = s / 100;
    const light = l / 100;
    const chroma = (1 - Math.abs(2 * light - 1)) * saturation;
    const segment = ((h % 360) + 360) % 360 / 60;
    const x = chroma * (1 - Math.abs(segment % 2 - 1));
    const [r1, g1, b1] = segment < 1 ? [chroma, x, 0]
      : segment < 2 ? [x, chroma, 0]
      : segment < 3 ? [0, chroma, x]
      : segment < 4 ? [0, x, chroma]
      : segment < 5 ? [x, 0, chroma]
      : [chroma, 0, x];
    const match = light - chroma / 2;
    return `#${[r1, g1, b1].map((channel) => Math.round((channel + match) * 255).toString(16).padStart(2, "0")).join("")}`;
  }

  function hexToHsl(hex: string) {
    const [r, g, b] = [1, 3, 5].map((index) => Number.parseInt(hex.slice(index, index + 2), 16) / 255);
    const max = Math.max(r, g, b);
    const min = Math.min(r, g, b);
    const delta = max - min;
    const light = (max + min) / 2;
    if (!delta) return { hue: 0, lightness: Math.round(light * 100) };
    const rawHue = max === r ? ((g - b) / delta) % 6 : max === g ? (b - r) / delta + 2 : (r - g) / delta + 4;
    return { hue: Math.round((rawHue * 60 + 360) % 360), lightness: Math.round(light * 100) };
  }

  function hexToRgba(hex: string, alpha: number) {
    const channels = [1, 3, 5].map((index) => Number.parseInt(hex.slice(index, index + 2), 16));
    return `rgba(${channels.join(", ")}, ${clamp(alpha, 0, 100) / 100})`;
  }

  function resetDraft() {
    draft = normalizedValue;
    const hsl = hexToHsl(normalizedValue);
    hue = hsl.hue;
    lightness = hsl.lightness;
    draftOpacity = displayOpacity;
  }

  function updateCustom(nextHue = hue, nextLightness = lightness) {
    hue = nextHue;
    lightness = nextLightness;
    draft = hslToHex(hue, 62, lightness);
  }

  function chooseReadyColor(color: string) {
    const normalized = normalizeHex(color);
    if (!normalized) return;
    onValueChange(normalized, 100);
    open = false;
    trigger?.focus();
  }

  function syncTheme() {
    if (!root || !popoverNode) return;
    copyResolvedColorTokens(root, popoverNode, ["surface", "pane", "text", "muted", "line", "accent", "subtle"].map((token) => ({ source: `--picker-${token}` })));
  }

  function placePopover() {
    if (!trigger || !popoverNode) return;
    const bounds = trigger.getBoundingClientRect();
    const width = Math.min(270, window.innerWidth - 16);
    const height = popoverNode.offsetHeight || (customOpen ? 350 : 190);
    above = window.innerHeight - bounds.bottom < height + 8 && bounds.top > height;
    left = Math.max(8, Math.min(window.innerWidth - width - 8, bounds.right - width));
    top = above
      ? Math.max(8, bounds.top - height - 6)
      : Math.max(8, Math.min(window.innerHeight - height - 8, bounds.bottom + 6));
    syncTheme();
  }

  function floatPopover(node: HTMLDivElement) {
    popoverNode = node;
    document.body.appendChild(node);
    void tick().then(placePopover);
    return { destroy() { if (popoverNode === node) popoverNode = null; node.remove(); } };
  }

  function toggle() {
    if (open) { open = false; return; }
    resetDraft();
    customOpen = false;
    open = true;
  }

  function toggleCustom() {
    customOpen = !customOpen;
    void tick().then(placePopover);
  }

  function apply() {
    const normalized = normalizeHex(draft);
    if (!normalized) return;
    onValueChange(normalized, draftOpacity);
    open = false;
    trigger?.focus();
  }

  function reset() {
    onReset();
    open = false;
    trigger?.focus();
  }

  onMount(() => {
    const motion = window.matchMedia("(prefers-reduced-motion: reduce)");
    const syncMotion = () => (reducedMotion = motion.matches);
    const closeOutside = (event: PointerEvent) => {
      const target = event.target as Node;
      if (open && !root?.contains(target) && !popoverNode?.contains(target)) open = false;
    };
    const reposition = () => open && placePopover();
    syncMotion();
    motion.addEventListener("change", syncMotion);
    document.addEventListener("pointerdown", closeOutside);
    window.addEventListener("resize", reposition);
    document.addEventListener("scroll", reposition, true);
    return () => {
      motion.removeEventListener("change", syncMotion);
      document.removeEventListener("pointerdown", closeOutside);
      window.removeEventListener("resize", reposition);
      document.removeEventListener("scroll", reposition, true);
    };
  });
</script>

<div class="accent-color-picker" bind:this={root}>
  <button bind:this={trigger} class:open class="color-trigger checkerboard" type="button" aria-label={tr(`Choose ${dialogLabel.toLocaleLowerCase()}`, `Escolher ${dialogLabel.toLocaleLowerCase()}`)} aria-haspopup="dialog" aria-expanded={open} onclick={toggle}>
    <span style:background={triggerColor}></span>
  </button>

  {#if open}
    <div class="color-popover" use:floatPopover role="dialog" aria-label={dialogLabel} style:left="{left}px" style:top="{top}px" style:--picker-hue={hue} style:--picker-draft={draft} style:--picker-preview={previewColor} in:fly={{ y: reducedMotion ? 0 : above ? 6 : -6, duration: reducedMotion ? 0 : 170, easing: cubicOut }} out:fly={{ y: reducedMotion ? 0 : above ? 5 : -5, duration: reducedMotion ? 0 : 110, easing: cubicOut }}>
      <header><strong>{dialogLabel}</strong><button type="button" aria-label={tr("Close color picker", "Fechar seletor de cor")} onclick={() => (open = false)}><LumeIcon name="close" size={14} /></button></header>

      <section class="ready-section">
        <span>{tr("Ready colors", "Cores prontas")}</span>
        <div class="ready-colors">
          {#each readyColors as color (color)}
            <button class:active={normalizedValue === color.toLocaleLowerCase() && displayOpacity === 100} type="button" style:--ready-color={color} aria-label={color} onclick={() => chooseReadyColor(color)}></button>
          {/each}
        </div>
      </section>

      <button class:open={customOpen} class="custom-toggle" type="button" aria-expanded={customOpen} onclick={toggleCustom}>
        <span>{tr("Custom color", "Cor personalizada")}</span><LumeIcon name="chevron-down" size={13} />
      </button>

      {#if customOpen}
        <section class="custom-controls">
          <div class="color-preview checkerboard"><i style:background={previewColor}></i></div>
          <label><span>{tr("Hue", "Matiz")}<b>{hue}°</b></span><input class="hue" type="range" min="0" max="359" value={hue} oninput={(event) => updateCustom(Number(event.currentTarget.value), lightness)} /></label>
          <label><span>{tr("Lightness", "Luminosidade")}<b>{lightness}%</b></span><input class="lightness" type="range" min="12" max="88" value={lightness} oninput={(event) => updateCustom(hue, Number(event.currentTarget.value))} /></label>
          <label><span>{tr("Opacity", "Opacidade")}<b>{draftOpacity}%</b></span><input class="opacity" type="range" min={minimumOpacity} max="100" value={draftOpacity} oninput={(event) => (draftOpacity = Number(event.currentTarget.value))} /></label>
        </section>
      {/if}

      <footer>{#if value}<button type="button" onclick={reset}>{tr("Use theme color", "Usar cor do tema")}</button>{/if}{#if customOpen}<button class="apply" type="button" onclick={apply}>{tr("Apply", "Aplicar")}</button>{/if}</footer>
    </div>
  {/if}
</div>

<style>
  .accent-color-picker { --picker-surface: var(--workspace-raised, var(--lume-raised-light)); --picker-pane: var(--workspace-pane, var(--lume-surface-light)); --picker-text: var(--workspace-strong, var(--lume-ink-strong-light)); --picker-muted: var(--workspace-muted, var(--lume-ink-muted-light)); --picker-line: var(--workspace-line, var(--lume-line-light)); --picker-accent: var(--workspace-accent, var(--lume-accent-strong)); --picker-subtle: var(--workspace-subtle, var(--lume-subtle-light)); position: relative; flex: 0 0 auto; }
  :global(.overlay-shell.dark) .accent-color-picker, :global(.terminal-window.dark) .accent-color-picker, :global(.bridge-window.dark) .accent-color-picker { --picker-surface: var(--lume-raised-dark); --picker-pane: var(--lume-surface-dark); --picker-text: var(--lume-ink-strong-dark); --picker-muted: var(--lume-ink-muted-dark); --picker-line: var(--lume-line-dark); --picker-accent: var(--lume-accent); --picker-subtle: var(--lume-subtle-dark); }
  .checkerboard { background-color: var(--picker-pane); background-image: linear-gradient(45deg, color-mix(in srgb, var(--picker-text) 10%, transparent) 25%, transparent 25%), linear-gradient(-45deg, color-mix(in srgb, var(--picker-text) 10%, transparent) 25%, transparent 25%), linear-gradient(45deg, transparent 75%, color-mix(in srgb, var(--picker-text) 10%, transparent) 75%), linear-gradient(-45deg, transparent 75%, color-mix(in srgb, var(--picker-text) 10%, transparent) 75%); background-position: 0 0, 0 4px, 4px -4px, -4px 0; background-size: 8px 8px; }
  .color-trigger { width: 35px; height: 29px; padding: 3px; display: grid; place-items: stretch; border: 1px solid var(--picker-line); border-radius: 8px; cursor: pointer; }
  .color-trigger span { border-radius: 5px; box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--picker-text) 13%, transparent); }
  .color-trigger:hover, .color-trigger.open { border-color: color-mix(in srgb, var(--picker-accent) 48%, var(--picker-line)); }
  .color-trigger:focus-visible { outline: 2px solid color-mix(in srgb, var(--picker-accent) 45%, transparent); outline-offset: 2px; }
  .color-popover { position: fixed; z-index: 1001; width: min(270px, calc(100vw - 16px)); box-sizing: border-box; padding: 12px; display: grid; gap: 10px; border: 1px solid var(--picker-line); border-radius: 14px; color: var(--picker-text); background: var(--picker-surface); box-shadow: 0 18px 48px rgba(7, 17, 12, .22); }
  .color-popover header { display: flex; align-items: center; gap: 8px; }
  .color-popover header strong { min-width: 0; flex: 1; font-size: 10px; }
  .color-popover header button { width: 27px; height: 27px; padding: 0; display: grid; place-items: center; border: 0; border-radius: 7px; color: var(--picker-muted); background: transparent; cursor: pointer; }
  .color-popover header button:hover { color: var(--picker-text); background: var(--picker-subtle); }
  .ready-section { display: grid; gap: 7px; }
  .ready-section > span, .custom-controls label > span { display: flex; justify-content: space-between; color: var(--picker-muted); font-size: 8px; font-weight: 720; }
  .ready-colors { display: grid; grid-template-columns: repeat(8, minmax(0, 1fr)); gap: 6px; }
  .ready-colors button { aspect-ratio: 1; min-width: 0; padding: 0; border: 2px solid var(--picker-surface); border-radius: 8px; background: var(--ready-color); box-shadow: 0 0 0 1px color-mix(in srgb, var(--picker-text) 12%, transparent); cursor: pointer; transition: transform 150ms cubic-bezier(.16, 1, .3, 1), box-shadow 150ms ease; }
  .ready-colors button:hover { transform: translateY(-2px) scale(1.05); }
  .ready-colors button.active { box-shadow: 0 0 0 2px var(--picker-text); }
  .custom-toggle { min-height: 32px; padding: 0 9px; display: flex; align-items: center; justify-content: space-between; border: 1px solid var(--picker-line); border-radius: 9px; color: var(--picker-text); background: var(--picker-pane); font-size: 8px; font-weight: 740; cursor: pointer; }
  .custom-toggle :global(svg) { transition: transform 180ms cubic-bezier(.16, 1, .3, 1); }.custom-toggle.open :global(svg) { transform: rotate(180deg); }
  .custom-controls { display: grid; gap: 10px; }
  .color-preview { height: 34px; overflow: hidden; border: 1px solid var(--picker-line); border-radius: 9px; }.color-preview i { display: block; width: 100%; height: 100%; }
  .custom-controls label { display: grid; gap: 6px; }
  .custom-controls label b { color: var(--picker-text); font-variant-numeric: tabular-nums; }
  .custom-controls input { width: 100%; height: 7px; margin: 0; appearance: none; border: 0; border-radius: 999px; outline: 0; cursor: pointer; }
  .custom-controls input.hue { background: linear-gradient(90deg, #e34b4b, #dfd84d, #48c96b, #43c8c9, #4b72dc, #a654d8, #e34b4b); }
  .custom-controls input.lightness { background: linear-gradient(90deg, #050706, hsl(var(--picker-hue) 62% 48%), #fff); }
  .custom-controls input.opacity { background: linear-gradient(90deg, transparent, var(--picker-draft)), repeating-conic-gradient(color-mix(in srgb, var(--picker-text) 12%, transparent) 0 25%, transparent 0 50%) 0 / 8px 8px; }
  .custom-controls input::-webkit-slider-thumb { width: 15px; height: 15px; appearance: none; border: 3px solid var(--picker-surface); border-radius: 50%; background: var(--picker-preview); box-shadow: 0 0 0 1px color-mix(in srgb, var(--picker-text) 40%, transparent); }
  .custom-controls input::-moz-range-thumb { width: 9px; height: 9px; border: 3px solid var(--picker-surface); border-radius: 50%; background: var(--picker-preview); }
  .color-popover footer { min-height: 30px; display: flex; justify-content: flex-end; gap: 6px; }
  .color-popover footer:empty { display: none; }
  .color-popover footer button { min-height: 30px; padding: 0 9px; border: 1px solid var(--picker-line); border-radius: 8px; color: var(--picker-muted); background: transparent; font-size: 8px; font-weight: 730; cursor: pointer; }
  .color-popover footer .apply { margin-left: auto; color: var(--picker-surface); border-color: transparent; background: var(--picker-accent); }
  .color-popover footer button:hover { color: var(--picker-text); background: var(--picker-subtle); }
  .color-popover footer .apply:hover { color: var(--picker-surface); background: var(--picker-accent); filter: brightness(1.05); }
</style>
