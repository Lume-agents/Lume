<script lang="ts">
  import { fade } from "svelte/transition";
  import type { Language } from "$lib/i18n";
  import LumeIcon from "$lib/LumeIcon.svelte";
  import TourDemo from "$lib/TourDemo.svelte";
  import type { TourStep } from "$lib/tours";

  let {
    step,
    index,
    total,
    language = "en",
    dark = false,
    side = null,
    style = "",
    element = $bindable(null),
    onBack,
    onNext,
    onSkip,
    onMediaReady,
  } = $props<{
    step: TourStep;
    index: number;
    total: number;
    language?: Language;
    /** For surfaces without the Workspace's color tokens (the Orb and its card window). */
    dark?: boolean;
    /** In its own window beside the Orb: the side the Orb is on, where the card points. */
    side?: "left" | "right" | null;
    style?: string;
    element?: HTMLElement | null;
    onBack: () => void;
    onNext: () => void;
    onSkip: () => void;
    onMediaReady?: () => void;
  }>();

  let mediaFailed = $state<Record<string, boolean>>({});
  const pt = $derived(language === "pt-BR");
  const last = $derived(index === total - 1);
  const media = $derived(step.media && !mediaFailed[step.media] ? `/tour/${step.media}-${pt ? "pt" : "en"}` : "");

  function tr(pair: [string, string]) {
    return pt ? pair[1] : pair[0];
  }
</script>

<div bind:this={element} class:dark class:with-media={Boolean(media || step.demo)} class:points-left={side === "right"} class:points-right={side === "left"} class="tour-card" {style}>
  {#key step.id}
    {#if step.demo}
      <TourDemo name={step.demo} {language} />
    {:else if media}
      <!-- WebM for WebKitGTK, H.264 for the macOS and Windows webviews; when neither plays, the card goes without. -->
      <video class="tour-media" autoplay loop muted playsinline onloadeddata={() => onMediaReady?.()}>
        <source src={`${media}.webm`} type="video/webm" />
        <source src={`${media}.mp4`} type="video/mp4" onerror={() => (mediaFailed = { ...mediaFailed, [step.media ?? ""]: true })} />
      </video>
    {/if}
    <div class="tour-copy" in:fade={{ duration: 160 }}>
      <span class="tour-count">{index + 1} / {total}</span>
      <strong id="tour-title">{tr(step.title)}</strong>
      <p>{tr(step.body)}</p>
    </div>
  {/key}
  <footer>
    <span class="tour-dots" aria-hidden="true">
      {#each { length: total } as _, dot (dot)}<i class:on={dot === index}></i>{/each}
    </span>
    {#if index === 0}
      <button class="quiet" type="button" onclick={onSkip}>{pt ? "Pular" : "Skip"}</button>
    {:else}
      <button class="quiet" type="button" onclick={onBack}>{pt ? "Voltar" : "Back"}</button>
    {/if}
    <button class="primary" type="button" onclick={onNext}>
      {#if last}{pt ? "Começar" : "Done"}{:else if index === 0}{pt ? "Mostrar" : "Show me"}{:else}{pt ? "Próximo" : "Next"}{/if}
      {#if !last}<LumeIcon name="arrow-right" size={13} />{/if}
    </button>
  </footer>
</div>

<style>
  .tour-card { --tour-accent: var(--workspace-accent, #428066); --tour-bg: var(--workspace-bg, #0f1512); --tour-line: var(--workspace-line, rgba(65, 94, 80, .16)); --tour-muted: var(--workspace-muted, #61736a); --tour-raised: var(--workspace-raised, #fff); --tour-strong: var(--workspace-strong, #1f2d26); --tour-subtle: var(--workspace-subtle, #eef4f0); --tour-text: var(--workspace-text, #4f6258);
    position: relative; width: min(340px, calc(100vw - 28px)); box-sizing: border-box; border: 1px solid var(--tour-line); border-radius: 16px; color: var(--tour-text); background: var(--tour-raised); box-shadow: 0 24px 60px rgba(5, 12, 8, .34), 0 2px 8px rgba(5, 12, 8, .14); }
  .tour-card.dark { --tour-accent: var(--workspace-accent, #4fae7e); --tour-line: var(--workspace-line, rgba(205, 222, 213, .12)); --tour-muted: var(--workspace-muted, #9aaba2); --tour-raised: var(--workspace-raised, #1f2924); --tour-strong: var(--workspace-strong, #eef5f1); --tour-subtle: var(--workspace-subtle, rgba(141, 206, 176, .09)); --tour-text: var(--workspace-text, #c3d2ca); }
  .tour-card.with-media { width: min(400px, calc(100vw - 28px)); }
  /* In its own window the card points back at the Orb, on whichever side it is. */
  .tour-card.points-left::before, .tour-card.points-right::before { position: absolute; top: calc(50% - 7px); width: 14px; height: 14px; box-sizing: border-box; border: 1px solid var(--tour-line); background: var(--tour-raised); content: ""; transform: rotate(45deg); }
  .tour-card.points-left::before { left: -8px; border-top-color: transparent; border-right-color: transparent; }
  .tour-card.points-right::before { right: -8px; border-bottom-color: transparent; border-left-color: transparent; }
  .tour-media { width: 100%; aspect-ratio: 16 / 9; display: block; object-fit: cover; border-bottom: 1px solid var(--tour-line); border-radius: 15px 15px 0 0; background: var(--tour-bg); }
  .tour-copy { padding: 16px 18px 4px; display: grid; gap: 6px; }
  .tour-count { color: var(--tour-accent); font: 700 10.5px/1 var(--lume-font-ui, Inter, sans-serif); letter-spacing: .04em; }
  .tour-copy strong { color: var(--tour-strong); font: 720 15px/1.25 var(--lume-font-ui, Inter, sans-serif); letter-spacing: -.01em; }
  .tour-copy p { margin: 0; color: var(--tour-muted); font: 500 12.5px/1.55 var(--lume-font-ui, Inter, sans-serif); }
  footer { padding: 12px 14px 14px 18px; display: flex; align-items: center; gap: 6px; }
  .tour-dots { margin-right: auto; display: flex; gap: 4px; }
  .tour-dots i { width: 5px; height: 5px; border-radius: 999px; background: color-mix(in srgb, var(--tour-muted) 30%, transparent); transition: width 220ms cubic-bezier(.16, 1, .3, 1), background 220ms ease; }
  .tour-dots i.on { width: 14px; background: var(--tour-accent); }
  footer button { height: 30px; padding: 0 12px; display: inline-flex; align-items: center; gap: 6px; border: 0; border-radius: 9px; font: 680 12px/1 var(--lume-font-ui, Inter, sans-serif); cursor: pointer; }
  footer .quiet { color: var(--tour-muted); background: transparent; }
  footer .quiet:hover { color: var(--tour-strong); background: var(--tour-subtle); }
  footer .primary { color: #fff; background: var(--tour-accent); }
  footer .primary:hover { filter: brightness(1.08); }
</style>
