<script lang="ts">
  import type { Snippet } from "svelte";
  import AccentColorPicker from "$lib/AccentColorPicker.svelte";
  import type { Backdrop, Preferences, SurfacePalette } from "$lib/domain";
  import type { Language } from "$lib/i18n";
  import {
    appearanceAttributes, appearanceThemes, backdropBackground, backdropEffects, backdropPresets, backdropStyles,
    darkBases, lightBases, normalizeBackdrop, workspaceCanvas,
  } from "$lib/appearance";

  let { preferences, language, dark, onpatch, disabled = false, typography, backdropImage }: {
    preferences: Preferences;
    language: Language;
    /** The color mode in effect now (a "system" choice already resolved). */
    dark: boolean;
    /** Saves a change; calls are queued, so a slider can commit freely. */
    onpatch: (patch: Partial<Preferences>) => unknown;
    disabled?: boolean;
    /** Extra rows for the Typography section (font pickers), when the window offers them. */
    typography?: Snippet;
    /** Extra rows for the backdrop section (the background image), when the window offers it. */
    backdropImage?: Snippet;
  } = $props();

  const tr = (english: string, portuguese: string) => (language === "pt-BR" ? portuguese : english);
  let chain: Promise<unknown> = Promise.resolve();
  function patch(next: Partial<Preferences>) { chain = chain.then(() => onpatch(next)).catch(() => undefined); }

  const appearance = $derived(appearanceAttributes(preferences));
  const theme = $derived(appearanceThemes.find((item) => item.value === appearance.theme) ?? appearanceThemes[0]);
  const accent = $derived(appearance.accent ?? theme.accent);
  const mode = $derived<"system" | "light" | "dark">(preferences.darkMode === undefined ? "system" : preferences.darkMode ? "dark" : "light");
  const baseValue = $derived(dark ? preferences.darkBase : preferences.lightBase);
  const bases = $derived(dark ? darkBases : lightBases);
  const palette = $derived<SurfacePalette>((dark ? preferences.customDarkSurfaces : preferences.customLightSurfaces) ?? {});
  const canvas = $derived(workspaceCanvas(preferences, dark));
  const savedBackdrop = $derived(normalizeBackdrop(preferences.backdrop));
  // Sliders show their value while dragged and save when released.
  let live = $state<Partial<Backdrop>>({});
  const backdrop = $derived({ ...savedBackdrop, ...live });
  const gradient = $derived(backdrop.style !== "solid");
  const effects = $derived(backdropEffects(backdrop, accent));
  const previewBackground = $derived(backdropBackground(backdrop, canvas.color, accent));

  function setBackdrop(change: Partial<Backdrop>) {
    live = {};
    patch({ backdrop: { ...savedBackdrop, ...change } });
  }
  function slider(key: "angle" | "grain" | "vignette" | "glow", event: Event, commit: boolean) {
    const value = Number((event.currentTarget as HTMLInputElement).value);
    if (commit) setBackdrop({ [key]: value }); else live = { ...live, [key]: value };
  }
  function pickBase(value: string) {
    // A chosen surface shows at once: a color picked earlier for the Workspace would otherwise hide it.
    if (dark) patch({ darkBase: value as Preferences["darkBase"], workspaceDarkBackgroundColor: undefined, workspaceBackgroundColor: undefined });
    else patch({ lightBase: value as Preferences["lightBase"], workspaceLightBackgroundColor: undefined });
  }
  function pickTheme(value: Preferences["appearanceTheme"]) {
    patch(dark
      ? { appearanceTheme: value, workspaceDarkBackgroundColor: undefined, workspaceBackgroundColor: undefined }
      : { appearanceTheme: value, workspaceLightBackgroundColor: undefined });
  }
  function setPalette(key: keyof SurfacePalette, color: string | undefined) {
    const next = { ...palette, [key]: color };
    patch(dark ? { customDarkSurfaces: next, darkBase: "custom" } : { customLightSurfaces: next, lightBase: "custom" });
  }
  const surfaceReady = $derived(dark
    ? ["#000000", "#0a0b0c", "#101214", "#16181b", "#1c1f24", "#14231c", "#101f28", "#1b1726"]
    : ["#ffffff", "#f6f7f8", "#eceef0", "#e3e5e8", "#f3efe6", "#e9eee8", "#e5edf0", "#ece9f1"]);
  const surfaceFields = $derived<Array<{ key: keyof SurfacePalette; label: string; hint: string }>>([
    { key: "canvas", label: tr("Window", "Janela"), hint: tr("Behind everything", "Atrás de tudo") },
    { key: "surface", label: tr("Panels", "Painéis"), hint: tr("Chats, cards and menus", "Chats, cartões e menus") },
    { key: "sidebar", label: tr("Sidebar", "Barra lateral"), hint: tr("Side panels", "Painéis laterais") },
  ]);
  const surfaceFallback = $derived((key: keyof SurfacePalette) => {
    const base = (dark ? darkBases : lightBases).find((item) => item.value === (dark ? "graphite" : "white"))?.pigments;
    return (key === "canvas" ? base?.canvas : key === "surface" ? base?.surface : base?.sidebar) ?? (dark ? "#14231c" : "#e9eee8");
  });

  const sections = $derived([
    { id: "mode", label: tr("Mode", "Modo") },
    { id: "surfaces", label: tr("Surfaces", "Superfícies") },
    { id: "accent", label: tr("Accent", "Destaque") },
    { id: "backdrop", label: tr("Background", "Fundo") },
    ...(typography ? [{ id: "type", label: tr("Typography", "Tipografia") }] : []),
  ]);
  let active = $state("mode");
</script>

<div class="appearance" class:dark style={appearance.baseCss} data-appearance={appearance.theme} style:--ap-accent={accent}>
  <div class="ap-preview" class:gradient style:--ap-canvas={canvas.color} aria-hidden="true">
    <i class="ap-backdrop" style:background={previewBackground || undefined}></i>
    {#if effects.glow}<i class="ap-layer" style:background={effects.glow}></i>{/if}
    {#if effects.grain}<i class="ap-layer ap-grain" style:background-image={effects.grain} style:opacity={effects.grainOpacity}></i>{/if}
    <div class="ap-window">
      <b class="ap-side"><em></em><em></em><em></em></b>
      <div class="ap-chat"><span class="ap-agent"></span><span class="ap-agent short"></span><span class="ap-user"></span><span class="ap-input"></span></div>
    </div>
    {#if effects.vignette}<i class="ap-layer" style:background={effects.vignette}></i>{/if}
  </div>

  <div class="ap-tabs" role="tablist" aria-label={tr("Appearance sections", "Seções da aparência")}>
    {#each sections as section (section.id)}
      <button class:on={active === section.id} type="button" role="tab" aria-selected={active === section.id} onclick={() => (active = section.id)}>{section.label}</button>
    {/each}
  </div>

  {#if active === "mode"}
    <section class="ap-section" aria-label={tr("Mode", "Modo")}>
      <div class="ap-segment" role="group" aria-label={tr("Color mode", "Modo de cores")}>
        {#each [["system", tr("System", "Sistema")], ["light", tr("Light", "Claro")], ["dark", tr("Dark", "Escuro")]] as [id, label] (id)}
          <button class:on={mode === id} type="button" aria-pressed={mode === id} {disabled} onclick={() => patch({ darkMode: id === "system" ? undefined : id === "dark" })}>{label}</button>
        {/each}
      </div>
      <p class="ap-hint">{tr("System follows your computer. The surfaces below are set separately for light and dark.", "Sistema acompanha o computador. As superfícies abaixo são definidas separadamente para claro e escuro.")}</p>
    </section>
  {:else if active === "surfaces"}
    <section class="ap-section" aria-label={tr("Surfaces", "Superfícies")}>
      <p class="ap-label">{tr("Tint", "Tom")}{#if baseValue !== "theme"}<small>{tr("Replaced by the surface below.", "Substituído pela superfície abaixo.")}</small>{/if}</p>
      <div class="ap-swatches" class:muted={baseValue !== "theme"}>
        {#each appearanceThemes as item (item.value)}
          <button class:on={appearance.theme === item.value} type="button" aria-pressed={appearance.theme === item.value} onclick={() => pickTheme(item.value)}>
            <i style:--a={item.accent} style:--s={dark ? item.darkSurface : item.lightSurface}></i>{item.label}
          </button>
        {/each}
      </div>
      <p class="ap-label">{tr("Surface", "Superfície")}</p>
      <div class="ap-swatches">
        {#each bases as base (base.value)}
          <button class:on={baseValue === base.value} type="button" aria-pressed={baseValue === base.value} onclick={() => pickBase(base.value)}>
            <i style:--a={accent} style:--s={base.pigments?.surface ?? (base.value === "custom" ? (palette.surface ?? surfaceFallback("surface")) : dark ? theme.darkSurface : theme.lightSurface)}></i>{tr(base.label, base.labelPt)}
          </button>
        {/each}
      </div>
      {#if baseValue === "custom"}
        <div class="ap-rows">
          {#each surfaceFields as field (field.key)}
            <div class="ap-row">
              <span><strong>{field.label}</strong><small>{palette[field.key] ?? field.hint}</small></span>
              <AccentColorPicker value={palette[field.key]} fallback={surfaceFallback(field.key)} readyColors={surfaceReady} showOpacity={false} fullRange {language} label={field.label}
                onValueChange={(color) => setPalette(field.key, color)} onReset={() => setPalette(field.key, undefined)} />
            </div>
          {/each}
        </div>
      {/if}
    </section>
  {:else if active === "accent"}
    <section class="ap-section" aria-label={tr("Accent", "Destaque")}>
      <div class="ap-rows">
        <div class="ap-row">
          <span><strong>{tr("Accent color", "Cor de destaque")}</strong><small>{appearance.accent ?? tr("Following the tint", "Seguindo o tom")}</small></span>
          <AccentColorPicker value={appearance.accent} opacity={preferences.accentOpacity} readyColors={appearanceThemes.map((item) => item.accent)} fallback={theme.accent} {language} label={tr("Accent color", "Cor de destaque")}
            onValueChange={(color, opacity) => patch({ accentColor: color, accentOpacity: opacity })} onReset={() => patch({ accentColor: undefined, accentOpacity: 100 })} />
        </div>
      </div>
    </section>
  {:else if active === "backdrop"}
    <section class="ap-section" aria-label={tr("Workspace background", "Fundo do Workspace")}>
      <p class="ap-label">{tr("Style", "Estilo")}</p>
      <div class="ap-segment" role="group" aria-label={tr("Background style", "Estilo do fundo")}>
        {#each backdropStyles as option (option.value)}
          <button class:on={backdrop.style === option.value} type="button" aria-pressed={backdrop.style === option.value} {disabled} onclick={() => setBackdrop({ style: option.value })}>{tr(option.label, option.labelPt)}</button>
        {/each}
      </div>
      {#if gradient}
        <p class="ap-label">{tr("Gradients", "Gradientes")}</p>
        <div class="ap-presets">
          {#each backdropPresets as preset (preset.id)}
            <button type="button" title={tr(preset.label, preset.labelPt)} aria-label={tr(preset.label, preset.labelPt)} class:on={backdrop.from === preset.from && backdrop.to === preset.to}
              style:background={`linear-gradient(${preset.angle}deg, ${preset.from}, ${preset.to})`} onclick={() => setBackdrop({ from: preset.from, to: preset.to, angle: preset.angle })}></button>
          {/each}
        </div>
        <div class="ap-rows">
          <div class="ap-row">
            <span><strong>{tr("From", "De")}</strong><small>{backdrop.from ?? tr("The surface color", "A cor da superfície")}</small></span>
            <AccentColorPicker value={backdrop.from} fallback={canvas.color} readyColors={surfaceReady} showOpacity={false} fullRange {language} label={tr("Gradient start", "Início do gradiente")}
              onValueChange={(color) => setBackdrop({ from: color })} onReset={() => setBackdrop({ from: undefined })} />
          </div>
          <div class="ap-row">
            <span><strong>{tr("To", "Até")}</strong><small>{backdrop.to ?? tr("The surface, warmed by the accent", "A superfície, com o destaque")}</small></span>
            <AccentColorPicker value={backdrop.to} fallback={accent} readyColors={[...surfaceReady, ...appearanceThemes.map((item) => item.accent)]} showOpacity={false} fullRange {language} label={tr("Gradient end", "Fim do gradiente")}
              onValueChange={(color) => setBackdrop({ to: color })} onReset={() => setBackdrop({ to: undefined })} />
          </div>
          {#if backdrop.style === "linear"}
            <label class="ap-row ap-range">
              <span><strong>{tr("Angle", "Ângulo")}</strong><small>{backdrop.angle}°</small></span>
              <input type="range" min="0" max="360" step="5" value={backdrop.angle} aria-label={tr("Gradient angle", "Ângulo do gradiente")} oninput={(event) => slider("angle", event, false)} onchange={(event) => slider("angle", event, true)} />
            </label>
          {/if}
        </div>
      {:else}
        <div class="ap-rows">
          <div class="ap-row">
            <span><strong>{tr("Solid color", "Cor sólida")}</strong><small>{(dark ? preferences.workspaceDarkBackgroundColor ?? preferences.workspaceBackgroundColor : preferences.workspaceLightBackgroundColor) ?? tr("Following the surface", "Seguindo a superfície")} · {canvas.opacity}%</small></span>
            <AccentColorPicker value={dark ? preferences.workspaceDarkBackgroundColor ?? preferences.workspaceBackgroundColor : preferences.workspaceLightBackgroundColor} opacity={canvas.opacity} fallback={canvas.color}
              readyColors={surfaceReady} minimumOpacity={35} fullRange {language} label={tr("Workspace background", "Fundo do Workspace")}
              onValueChange={(color, opacity) => patch(dark ? { workspaceDarkBackgroundColor: color, workspaceDarkBackgroundOpacity: opacity } : { workspaceLightBackgroundColor: color, workspaceLightBackgroundOpacity: opacity })}
              onReset={() => patch(dark ? { workspaceDarkBackgroundColor: undefined, workspaceDarkBackgroundOpacity: 96, workspaceBackgroundColor: undefined } : { workspaceLightBackgroundColor: undefined, workspaceLightBackgroundOpacity: 96 })} />
          </div>
        </div>
      {/if}
      <p class="ap-label">{tr("Effects", "Efeitos")}</p>
      <div class="ap-rows">
        {#each [["glow", tr("Glow", "Brilho"), tr("A soft accent light from the top", "Uma luz suave do destaque, vinda de cima")], ["grain", tr("Grain", "Granulado"), tr("Fine film grain", "Granulado fino de filme")], ["vignette", tr("Vignette", "Vinheta"), tr("Darkens the edges", "Escurece as bordas")]] as [key, label, hint] (key)}
          <label class="ap-row ap-range">
            <span><strong>{label}</strong><small>{backdrop[key as "glow" | "grain" | "vignette"] ? `${backdrop[key as "glow" | "grain" | "vignette"]}%` : hint}</small></span>
            <input type="range" min="0" max="100" step="5" value={backdrop[key as "glow" | "grain" | "vignette"]} aria-label={label} oninput={(event) => slider(key as "glow", event, false)} onchange={(event) => slider(key as "glow", event, true)} />
          </label>
        {/each}
      </div>
      {#if backdropImage}<div class="ap-extra">{@render backdropImage()}</div>{/if}
    </section>
  {:else if active === "type" && typography}
    <section class="ap-section ap-extra" aria-label={tr("Typography", "Tipografia")}>{@render typography()}</section>
  {/if}
</div>

<style>
  .appearance {
    --ap-text: var(--lume-ink-strong-light); --ap-muted: var(--lume-ink-muted-light); --ap-line: var(--lume-line-light);
    --ap-surface: var(--lume-surface-light); --ap-sidebar: var(--lume-sidebar-light); --ap-raised: var(--lume-raised-light);
    --ap-user: var(--lume-user-light); --ap-canvas-fallback: var(--lume-canvas-light);
    min-width: 0; display: grid; gap: 12px; color: var(--ap-text); font: 12px/1.4 var(--lume-font-ui, system-ui, sans-serif);
  }
  .appearance.dark {
    --ap-text: var(--lume-ink-strong-dark); --ap-muted: var(--lume-ink-muted-dark); --ap-line: var(--lume-line-dark);
    --ap-surface: var(--lume-surface-dark); --ap-sidebar: var(--lume-sidebar-dark); --ap-raised: var(--lume-raised-dark);
    --ap-user: var(--lume-user-dark); --ap-canvas-fallback: var(--lume-canvas-dark);
  }
  .ap-preview { position: relative; height: 128px; overflow: hidden; border: 1px solid var(--ap-line); border-radius: 12px; background: var(--ap-canvas, var(--ap-canvas-fallback)); isolation: isolate; }
  .ap-backdrop, .ap-layer { position: absolute; inset: 0; z-index: 0; pointer-events: none; }
  .ap-window { position: absolute; inset: 10px; z-index: 1; display: grid; grid-template-columns: 26% 1fr; gap: 8px; }
  .ap-side { padding: 8px 7px; display: grid; align-content: start; gap: 5px; border-radius: 8px; background: color-mix(in srgb, var(--ap-sidebar) 92%, transparent); }
  .ap-side em { height: 8px; border-radius: 4px; background: color-mix(in srgb, var(--ap-muted) 38%, transparent); }
  .ap-side em:first-child { background: color-mix(in srgb, var(--ap-accent) 55%, transparent); }
  .ap-chat { padding: 9px; display: grid; align-content: start; gap: 6px; border-radius: 8px; background: color-mix(in srgb, var(--ap-surface) 88%, transparent); }
  .ap-agent, .ap-user, .ap-input { height: 9px; border-radius: 5px; background: color-mix(in srgb, var(--ap-muted) 32%, transparent); }
  .ap-agent.short { width: 62%; }
  .ap-user { width: 48%; margin-left: auto; background: color-mix(in srgb, var(--ap-accent) 48%, var(--ap-surface)); }
  .ap-input { height: 14px; margin-top: 4px; border: 1px solid var(--ap-line); background: transparent; }
  .ap-tabs { display: flex; gap: 2px; padding: 3px; overflow-x: auto; border-radius: 10px; background: color-mix(in srgb, var(--ap-muted) 10%, transparent); scrollbar-width: none; }
  .ap-tabs button { flex: 1 0 auto; padding: 6px 10px; border: 0; border-radius: 8px; color: var(--ap-muted); background: transparent; font: inherit; font-weight: 650; cursor: pointer; transition: color 140ms ease, background 140ms ease; }
  .ap-tabs button:hover { color: var(--ap-text); }
  .ap-tabs button.on { color: var(--ap-text); background: var(--ap-raised); box-shadow: 0 1px 3px rgba(0, 0, 0, .14); }
  .ap-section { display: grid; gap: 10px; min-width: 0; }
  .ap-label { margin: 4px 0 -2px; display: flex; align-items: baseline; gap: 8px; color: var(--ap-muted); font-size: 10px; font-weight: 700; letter-spacing: .06em; text-transform: uppercase; }
  .ap-label small { font-size: 10px; font-weight: 500; letter-spacing: 0; text-transform: none; }
  .ap-hint { margin: 0; color: var(--ap-muted); font-size: 11px; }
  .ap-segment { display: grid; grid-auto-flow: column; grid-auto-columns: 1fr; gap: 2px; padding: 3px; border-radius: 10px; background: color-mix(in srgb, var(--ap-muted) 10%, transparent); }
  .ap-segment button { padding: 7px 6px; border: 0; border-radius: 8px; color: var(--ap-muted); background: transparent; font: inherit; font-weight: 650; cursor: pointer; transition: color 140ms ease, background 140ms ease; }
  .ap-segment button.on { color: var(--ap-text); background: var(--ap-raised); box-shadow: 0 1px 3px rgba(0, 0, 0, .14); }
  .ap-segment button:disabled { opacity: .55; cursor: default; }
  .ap-swatches { display: grid; grid-template-columns: repeat(auto-fill, minmax(64px, 1fr)); gap: 6px; }
  .ap-swatches.muted { opacity: .55; }
  .ap-swatches button { min-width: 0; min-height: 62px; padding: 6px; display: grid; grid-template-rows: 30px minmax(1.2em, auto); justify-items: center; align-content: start; gap: 5px; border: 1px solid transparent; border-radius: 9px; color: var(--ap-muted); background: transparent; font: inherit; font-size: 10px; font-weight: 650; line-height: 1.2; text-align: center; cursor: pointer; transition: border-color 140ms ease, color 140ms ease, background 140ms ease; }
  .ap-swatches button:hover { color: var(--ap-text); background: color-mix(in srgb, var(--ap-muted) 6%, transparent); }
  .ap-swatches button.on { color: var(--ap-text); border-color: color-mix(in srgb, var(--ap-accent) 68%, var(--ap-line)); background: color-mix(in srgb, var(--ap-accent) 6%, transparent); }
  .ap-swatches i { display: block; box-sizing: border-box; width: 100%; max-width: 72px; height: 30px; overflow: hidden; border: 0; border-radius: 6px; background: linear-gradient(135deg, var(--s) 0 60%, var(--a) 60% 100%); box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--ap-text) 16%, transparent); }
  .ap-presets { display: flex; flex-wrap: wrap; gap: 8px; }
  .ap-presets button { box-sizing: border-box; width: 40px; height: 28px; padding: 0; border: 1px solid color-mix(in srgb, var(--ap-line) 78%, transparent); border-radius: 7px; background-clip: padding-box; cursor: pointer; transition: border-color 140ms ease; }
  .ap-presets button.on { border-color: var(--ap-accent); }
  .ap-rows { display: grid; }
  .ap-row { min-height: 52px; display: flex; align-items: center; justify-content: space-between; gap: 14px; border-top: 1px solid var(--ap-line); }
  .ap-rows > :first-child { border-top: 0; }
  .ap-row > span { min-width: 0; display: grid; gap: 2px; }
  .ap-row strong { font-size: 12px; font-weight: 650; }
  .ap-row small { overflow: hidden; color: var(--ap-muted); font-size: 11px; text-overflow: ellipsis; white-space: nowrap; }
  .ap-range input { width: 46%; min-width: 96px; accent-color: var(--ap-accent); }
  .ap-extra { display: grid; min-width: 0; }
  button:focus-visible, input:focus-visible { outline: 2px solid var(--ap-accent); outline-offset: 2px; }
  @media (prefers-reduced-motion: reduce) { * { transition: none !important; } }
</style>
