<script lang="ts">
  import { onMount } from "svelte";
  import { emit, listen } from "@tauri-apps/api/event";
  import { invoke } from "@tauri-apps/api/core";
  import type { Language } from "$lib/i18n";
  import TourCard from "$lib/TourCard.svelte";
  import { TOUR_ACTION_EVENT, TOUR_CARD_EVENT, TOUR_READY_EVENT, type TourAction, type TourStep } from "$lib/tours";

  // The Orb's tour card, in a window of its own beside the Orb (see tour_card.rs): the Orb sends each step
  // and moves the window next to what it highlights; the buttons go back to the Orb.
  type Shown = { step: TourStep; index: number; total: number; language: Language; dark: boolean };

  let shown = $state<Shown | null>(null);
  let side = $state<"left" | "right" | null>(null);
  let card = $state<HTMLElement | null>(null);

  function act(action: TourAction) {
    void emit(TOUR_ACTION_EVENT, action);
  }

  /** The window takes the card's height, plus the margin its shadow needs. */
  async function fit() {
    if (!card) return;
    const placed = await invoke<{ side: "left" | "right" }>("fit_tour_card", { height: card.offsetHeight + 36 }).catch(() => null);
    if (placed) side = placed.side;
  }

  function keydown(event: KeyboardEvent) {
    if (event.key === "Escape") act("skip");
    else if (event.key === "ArrowRight" || event.key === "Enter") act("next");
    else if (event.key === "ArrowLeft") act("back");
    else return;
    event.preventDefault();
  }

  $effect(() => {
    if (!card) return;
    const observer = new ResizeObserver(() => void fit());
    observer.observe(card);
    return () => observer.disconnect();
  });

  onMount(() => {
    const stop = listen<Shown>(TOUR_CARD_EVENT, (event) => { shown = event.payload; });
    // The Orb sent its first step before this window was listening: ask for it again.
    void stop.then(() => emit(TOUR_READY_EVENT));
    return () => void stop.then((unlisten) => unlisten());
  });
</script>

<svelte:head><title>Lume · Tour</title></svelte:head>
<svelte:window onkeydown={keydown} />

{#if shown}
  <main>
    <TourCard bind:element={card} step={shown.step} index={shown.index} total={shown.total} language={shown.language} dark={shown.dark} {side} style="width: 100%;" onBack={() => act("back")} onNext={() => act("next")} onSkip={() => act("skip")} onMediaReady={() => void fit()} />
  </main>
{/if}

<style>
  :global(html), :global(body) { margin: 0; overflow: hidden; background: transparent !important; }
  main { padding: 18px; box-sizing: border-box; }
</style>
