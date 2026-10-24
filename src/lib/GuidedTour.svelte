<script lang="ts">
  import { onMount, tick } from "svelte";
  import { fade } from "svelte/transition";
  import { emit, listen } from "@tauri-apps/api/event";
  import { invoke } from "@tauri-apps/api/core";
  import type { Language } from "$lib/i18n";
  import TourCard from "$lib/TourCard.svelte";
  import { TOUR_ACTION_EVENT, TOUR_CARD_EVENT, TOUR_READY_EVENT, type TourAction, type TourStep } from "$lib/tours";

  let { steps, language = "en", dark = false, detached = false, onPrepare, onDone } = $props<{
    steps: TourStep[];
    /** The card goes in its own window beside this one (the Orb is too small to hold it); only the highlight stays here. */
    detached?: boolean;
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
  let frame = 0;
  // A separate card window only exists in the desktop app, and only counts once it has answered: if it never
  // does (a desktop that will not stack it above the Orb, or one that cannot place it), the card is drawn here.
  let gaveUp = $state(false);
  const external = $derived(detached && !gaveUp && typeof window !== "undefined" && "__TAURI_INTERNALS__" in window);

  const step: TourStep = $derived(steps[index]);
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
      else if (external) void placeCard();
    };
    loop();
  }

  function giveUp() {
    if (gaveUp) return;
    gaveUp = true;
    void invoke("close_tour_card").catch(() => {});
    void tick().then(() => follow(120));
  }

  /** Lines the card window up with the highlight and hands it the step to show. */
  async function placeCard() {
    // The card window adds a margin around the card for its shadow.
    const width = (step.media || step.demo ? 400 : 340) + 36;
    // A desktop that cannot make or place the window says so right away, and the card is drawn here instead.
    const placed = await invoke("place_tour_card", { anchorY: rect ? rect.y + rect.height / 2 : null, width }).then(() => true, () => false);
    if (!placed) {
      giveUp();
      return;
    }
    await emit(TOUR_CARD_EVENT, { step, index, total: steps.length, language, dark });
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
    if (external) void emit(TOUR_CARD_EVENT, { step: steps[next], index: next, total: steps.length, language, dark });
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

  function act(action: TourAction) {
    if (action === "next") void go(index + 1);
    else if (action === "back") void go(index - 1);
    else onDone(false);
  }

  onMount(() => {
    void go(0);
    const resize = () => follow(200);
    window.addEventListener("resize", resize);
    let cardAlive = false;
    const unlisten = external
      ? [listen<TourAction>(TOUR_ACTION_EVENT, (event) => act(event.payload)), listen(TOUR_READY_EVENT, () => { cardAlive = true; void placeCard(); })]
      : [];
    // The window is made as the tour opens, not after the first step settles: a webview can take a few seconds to
    // start (Windows especially), and the card gives up on it only after that.
    if (external) void Promise.all(unlisten).then(() => placeCard());
    const waiting = external ? window.setTimeout(() => {
      if (!cardAlive) giveUp();
    }, (window as unknown as { __lumeTourCardWait?: number }).__lumeTourCardWait ?? 8000) : 0;
    return () => {
      window.clearTimeout(waiting);
      cancelAnimationFrame(frame);
      window.removeEventListener("resize", resize);
      for (const pending of unlisten) void pending.then((stop) => stop());
      if (external) void invoke("close_tour_card").catch(() => {});
    };
  });
</script>

<svelte:window onkeydown={keydown} />

<div class:dark class="tour" role="dialog" aria-modal="true" aria-labelledby="tour-title" transition:fade={{ duration: 160 }}>
  {#if rect}
    <div class:soft={external} class="spotlight" style:left={`${rect.x - padding}px`} style:top={`${rect.y - padding}px`} style:width={`${rect.width + padding * 2}px`} style:height={`${rect.height + padding * 2}px`}></div>
  {:else}
    <div class:soft={external} class="scrim"></div>
  {/if}
  {#if !external}
    <TourCard bind:element={card} {step} {index} total={steps.length} {language} {dark} style={`position: absolute; left: ${position.x}px; top: ${position.y}px; transition: left 380ms cubic-bezier(.16, 1, .3, 1), top 380ms cubic-bezier(.16, 1, .3, 1);`} onBack={() => act("back")} onNext={() => act("next")} onSkip={() => act("skip")} onMediaReady={() => follow(120)} />
  {/if}
</div>

<style>
  .tour { --tour-accent: var(--workspace-accent, #428066); --tour-bg: var(--workspace-bg, #0f1512); --tour-line: var(--workspace-line, rgba(65, 94, 80, .16)); --tour-muted: var(--workspace-muted, #61736a); --tour-raised: var(--workspace-raised, #fff); --tour-strong: var(--workspace-strong, #1f2d26); --tour-subtle: var(--workspace-subtle, #eef4f0); --tour-text: var(--workspace-text, #4f6258); position: fixed; z-index: 400; inset: 0; pointer-events: auto; }
  .tour.dark { --tour-accent: var(--workspace-accent, #4fae7e); --tour-bg: var(--workspace-bg, #0f1512); --tour-line: var(--workspace-line, rgba(205, 222, 213, .12)); --tour-muted: var(--workspace-muted, #9aaba2); --tour-raised: var(--workspace-raised, #1f2924); --tour-strong: var(--workspace-strong, #eef5f1); --tour-subtle: var(--workspace-subtle, rgba(141, 206, 176, .09)); --tour-text: var(--workspace-text, #c3d2ca); }
  .scrim { position: absolute; inset: 0; background: rgba(6, 12, 9, .56); }
  .scrim.soft { background: rgba(6, 12, 9, .22); }
  .spotlight.soft { box-shadow: 0 0 0 1.5px color-mix(in srgb, var(--tour-accent) 75%, transparent), 0 0 0 9999px rgba(6, 12, 9, .34); }
  .spotlight { position: absolute; border-radius: 14px; box-shadow: 0 0 0 1.5px color-mix(in srgb, var(--tour-accent) 75%, transparent), 0 0 0 9999px rgba(6, 12, 9, .56); pointer-events: none; transition: left 380ms cubic-bezier(.16, 1, .3, 1), top 380ms cubic-bezier(.16, 1, .3, 1), width 380ms cubic-bezier(.16, 1, .3, 1), height 380ms cubic-bezier(.16, 1, .3, 1); }
  .spotlight::after { position: absolute; inset: -1.5px; border-radius: inherit; box-shadow: 0 0 0 0 color-mix(in srgb, var(--tour-accent) 45%, transparent); content: ""; animation: tour-pulse 1.8s ease-out infinite; }
  @keyframes tour-pulse { 0% { box-shadow: 0 0 0 0 color-mix(in srgb, var(--tour-accent) 45%, transparent); } 100% { box-shadow: 0 0 0 12px transparent; } }
  @media (prefers-reduced-motion: reduce) { .spotlight { transition: none; } .spotlight::after { animation: none; } }
</style>
