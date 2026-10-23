<script lang="ts">
  import { onMount, tick } from "svelte";
  import { fade } from "svelte/transition";
  import type { Language } from "$lib/i18n";
  import LumeIcon from "$lib/LumeIcon.svelte";
  import type { TourStep } from "$lib/tours";


  let { steps, language = "en", dark = false, onPrepare, onDone } = $props<{
    steps: TourStep[];
    /** For surfaces without the Workspace's color tokens (the Orb): which fallback palette to use. */
    dark?: boolean;
    language?: Language;
    /** Gets the workspace ready for a step (closes the board, opens the sidebar…) before it is measured. */
    onPrepare?: (step: string) => void | Promise<void>;
    onDone: (completed: boolean) => void;
  }>();


  let index = $state(0);
  let rect = $state<{ x: number; y: number; width: number; height: number } | null>(null);
  let card = $state<HTMLElement | null>(null);
  let cardSize = $state({ width: 340, height: 220 });
  let viewport = $state({ width: 1200, height: 800 });
  let mediaFailed = $state<Record<string, boolean>>({});
  let frame = 0;

  const step: TourStep = $derived(steps[index]);
  const pt = $derived(language === "pt-BR");
  const last = $derived(index === steps.length - 1);
  const padding = 7;
  const gap = 16;

  /** Beside the target where it fits (right, left, below, above), else centered. */
  const position = $derived.by(() => {
    const { width, height } = cardSize;
    const clampX = (x: number) => Math.max(14, Math.min(viewport.width - width - 14, x));
    const clampY = (y: number) => Math.max(14, Math.min(viewport.height - height - 14, y));
    if (!rect) return { x: (viewport.width - width) / 2, y: (viewport.height - height) / 2 };
    const right = rect.x + rect.width + padding + gap;
    const left = rect.x - padding - gap - width;
    const below = rect.y + rect.height + padding + gap;
    const above = rect.y - padding - gap - height;
    const middleY = clampY(rect.y + rect.height / 2 - height / 2);
    const middleX = clampX(rect.x + rect.width / 2 - width / 2);
    if (step.below && below + height <= viewport.height - 14) return { x: clampX(rect.x - 10), y: below };
    if (right + width <= viewport.width - 14) return { x: right, y: middleY };
    if (left >= 14) return { x: left, y: middleY };
    if (below + height <= viewport.height - 14) return { x: middleX, y: below };
    if (above >= 14) return { x: middleX, y: above };
    // The target fills the screen (the chats): the card sits inside it, bottom right.
    return { x: clampX(rect.x + rect.width - width - 24), y: clampY(rect.y + rect.height - height - 24) };
  });

  function tr(pair: [string, string]) {
    return pt ? pair[1] : pair[0];
  }

  function target(): HTMLElement | null {
    for (const name of step.targets) {
      const found = [...document.querySelectorAll<HTMLElement>(`[data-tour="${name}"]`)].find((element) => {
        const box = element.getBoundingClientRect();
        return box.width > 0 && box.height > 0;
      });
      if (found) return found;
    }
    return null;
  }

  function measure() {
    viewport = { width: window.innerWidth, height: window.innerHeight };
    const element = target();
    if (!element) {
      rect = null;
    } else {
      const box = element.getBoundingClientRect();
      // A target taller than the screen (a long chat) is highlighted where it is visible.
      const top = Math.max(box.top, 8);
      const bottom = Math.min(box.bottom, viewport.height - 8);
      rect = { x: box.left, y: top, width: box.width, height: Math.max(0, bottom - top) };
    }
    if (card) cardSize = { width: card.offsetWidth, height: card.offsetHeight };
  }

  /** Follows the target for a moment after a step changes, while panels slide into place. */
  function follow(duration = 700) {
    cancelAnimationFrame(frame);
    const until = performance.now() + duration;
    const loop = () => {
      measure();
      if (performance.now() < until) frame = requestAnimationFrame(loop);
    };
    loop();
  }

  async function go(next: number) {
    if (next < 0) return;
    if (next >= steps.length) {
      onDone(true);
      return;
    }
    index = next;
    await onPrepare?.(steps[next].id);
    await tick();
    follow();
  }

  function keydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onDone(false);
    } else if (event.key === "ArrowRight" || event.key === "Enter") {
      event.preventDefault();
      void go(index + 1);
    } else if (event.key === "ArrowLeft") {
      event.preventDefault();
      void go(index - 1);
    }
  }

  onMount(() => {
    void go(0);
    const resize = () => follow(200);
    window.addEventListener("resize", resize);
    return () => {
      cancelAnimationFrame(frame);
      window.removeEventListener("resize", resize);
    };
  });
</script>

<svelte:window onkeydown={keydown} />

<div class:dark class="tour" role="dialog" aria-modal="true" aria-labelledby="tour-title" transition:fade={{ duration: 160 }}>
  {#if rect}
    <div class="spotlight" style:left={`${rect.x - padding}px`} style:top={`${rect.y - padding}px`} style:width={`${rect.width + padding * 2}px`} style:height={`${rect.height + padding * 2}px`}></div>
  {:else}
    <div class="scrim"></div>
  {/if}
  <div bind:this={card} class:with-media={step.media && !mediaFailed[step.media]} class="tour-card" style:left={`${position.x}px`} style:top={`${position.y}px`}>
    {#key step.id}
      {#if step.media && !mediaFailed[step.media]}
        {@const clip = `/tour/${step.media}-${pt ? "pt" : "en"}`}
        <!-- WebM for WebKitGTK, H.264 for the macOS and Windows webviews; when neither plays, the card goes without. -->
        <video class="tour-media" autoplay loop muted playsinline onloadeddata={() => follow(120)}>
          <source src={`${clip}.webm`} type="video/webm" />
          <source src={`${clip}.mp4`} type="video/mp4" onerror={() => (mediaFailed = { ...mediaFailed, [step.media ?? ""]: true })} />
        </video>
      {/if}
      <div class="tour-copy" in:fade={{ duration: 160 }}>
        <span class="tour-count">{index + 1} / {steps.length}</span>
        <strong id="tour-title">{tr(step.title)}</strong>
        <p>{tr(step.body)}</p>
      </div>
    {/key}
    <footer>
      <span class="tour-dots" aria-hidden="true">
        {#each steps as item, dot (item.id)}<i class:on={dot === index}></i>{/each}
      </span>
      {#if index === 0}
        <button class="quiet" type="button" onclick={() => onDone(false)}>{pt ? "Pular" : "Skip"}</button>
      {:else}
        <button class="quiet" type="button" onclick={() => void go(index - 1)}>{pt ? "Voltar" : "Back"}</button>
      {/if}
      <button class="primary" type="button" onclick={() => void go(index + 1)}>
        {#if last}{pt ? "Começar" : "Done"}{:else if index === 0}{pt ? "Mostrar" : "Show me"}{:else}{pt ? "Próximo" : "Next"}{/if}
        {#if !last}<LumeIcon name="arrow-right" size={13} />{/if}
      </button>
    </footer>
  </div>
</div>

<style>
  .tour { --tour-accent: var(--workspace-accent, #428066); --tour-bg: var(--workspace-bg, #0f1512); --tour-line: var(--workspace-line, rgba(65, 94, 80, .16)); --tour-muted: var(--workspace-muted, #61736a); --tour-raised: var(--workspace-raised, #fff); --tour-strong: var(--workspace-strong, #1f2d26); --tour-subtle: var(--workspace-subtle, #eef4f0); --tour-text: var(--workspace-text, #4f6258); position: fixed; z-index: 400; inset: 0; pointer-events: auto; }
  .tour.dark { --tour-accent: var(--workspace-accent, #4fae7e); --tour-bg: var(--workspace-bg, #0f1512); --tour-line: var(--workspace-line, rgba(205, 222, 213, .12)); --tour-muted: var(--workspace-muted, #9aaba2); --tour-raised: var(--workspace-raised, #1f2924); --tour-strong: var(--workspace-strong, #eef5f1); --tour-subtle: var(--workspace-subtle, rgba(141, 206, 176, .09)); --tour-text: var(--workspace-text, #c3d2ca); }
  .scrim { position: absolute; inset: 0; background: rgba(6, 12, 9, .56); }
  .spotlight { position: absolute; border-radius: 14px; box-shadow: 0 0 0 1.5px color-mix(in srgb, var(--tour-accent) 75%, transparent), 0 0 0 9999px rgba(6, 12, 9, .56); pointer-events: none; transition: left 380ms cubic-bezier(.16, 1, .3, 1), top 380ms cubic-bezier(.16, 1, .3, 1), width 380ms cubic-bezier(.16, 1, .3, 1), height 380ms cubic-bezier(.16, 1, .3, 1); }
  .spotlight::after { position: absolute; inset: -1.5px; border-radius: inherit; box-shadow: 0 0 0 0 color-mix(in srgb, var(--tour-accent) 45%, transparent); content: ""; animation: tour-pulse 1.8s ease-out infinite; }
  .tour-card { position: absolute; width: min(340px, calc(100vw - 28px)); overflow: hidden; border: 1px solid var(--tour-line); border-radius: 16px; color: var(--tour-text); background: var(--tour-raised); box-shadow: 0 24px 60px rgba(5, 12, 8, .34), 0 2px 8px rgba(5, 12, 8, .14); transition: left 380ms cubic-bezier(.16, 1, .3, 1), top 380ms cubic-bezier(.16, 1, .3, 1); }
  .tour-card.with-media { width: min(400px, calc(100vw - 28px)); }
  .tour-media { width: 100%; aspect-ratio: 16 / 9; display: block; object-fit: cover; border-bottom: 1px solid var(--tour-line); background: var(--tour-bg); }
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
  @keyframes tour-pulse { 0% { box-shadow: 0 0 0 0 color-mix(in srgb, var(--tour-accent) 45%, transparent); } 100% { box-shadow: 0 0 0 12px transparent; } }
  @media (prefers-reduced-motion: reduce) { .spotlight, .tour-card { transition: none; } .spotlight::after { animation: none; } }
</style>
