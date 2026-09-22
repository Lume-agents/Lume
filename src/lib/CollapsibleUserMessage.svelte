<script lang="ts">
  import { cubicOut } from "svelte/easing";
  import { slide } from "svelte/transition";
  import type { Language } from "$lib/i18n";
  import LumeIcon from "$lib/LumeIcon.svelte";
  import { userMessagePresentation } from "$lib/userMessagePresentation";

  let { text, language = "en" } = $props<{ text: string; language?: Language }>();
  let expanded = $state(false);
  let previousText = $state("");
  const presentation = $derived(userMessagePresentation(text));
  const collapsible = $derived(presentation.kind !== "full");
  const visibleText = $derived(expanded ? text : presentation.preview);

  $effect(() => {
    if (text === previousText) return;
    previousText = text;
    expanded = false;
  });

  function tr(english: string, portuguese: string) {
    return language === "pt-BR" ? portuguese : english;
  }

  function formattedCharacterCount() {
    return new Intl.NumberFormat(language === "pt-BR" ? "pt-BR" : "en-US").format(presentation.characterCount);
  }
</script>

<div class:expanded class="collapsible-user-message">
  {#key expanded}
    {#if !expanded && presentation.kind === "pasted"}
      <div class="pasted-summary" transition:slide={{ duration: 220, easing: cubicOut }}>[{tr("Pasted content", "Conteúdo colado")} {formattedCharacterCount()} {tr("chars", "caracteres")}]</div>
    {:else}
      <div class="message-text-frame" transition:slide={{ duration: 220, easing: cubicOut }}>
        <pre>{visibleText}</pre>
        {#if !expanded && presentation.kind === "clamped"}
          <span class="message-fade" aria-hidden="true"></span>
        {/if}
      </div>
    {/if}
  {/key}

  {#if collapsible}
    <button type="button" aria-expanded={expanded} onclick={() => (expanded = !expanded)}>
      <span>{expanded ? tr("Show less", "Ver menos") : tr("Show more", "Ver mais")}</span>
      <LumeIcon name="chevron-down" size={12} />
    </button>
  {/if}
</div>

<style>
  .collapsible-user-message { min-width: 0; max-width: 100%; margin-top: var(--user-message-content-margin-top, 0); display: grid; gap: 5px; justify-items: end; }
  .message-text-frame { position: relative; min-width: 0; max-width: 100%; }
  pre { min-width: 0; max-width: 100%; margin: 0; overflow-x: hidden; color: var(--user-message-text, inherit); font: var(--user-message-font, inherit); line-height: var(--user-message-line-height, 1.55); overflow-wrap: anywhere; white-space: pre-wrap; word-break: break-word; animation: message-reveal 180ms cubic-bezier(.16, 1, .3, 1); }
  .message-fade { position: absolute; right: 0; bottom: -1px; left: 0; height: 38px; pointer-events: none; background: linear-gradient(180deg, transparent, var(--message-collapse-surface, var(--workspace-user))); -webkit-backdrop-filter: blur(2.5px); backdrop-filter: blur(2.5px); -webkit-mask-image: linear-gradient(transparent, #000 62%); mask-image: linear-gradient(transparent, #000 62%); }
  .pasted-summary { min-height: 30px; padding: 5px 9px; display: flex; align-items: center; border-radius: 8px; color: var(--user-message-muted, var(--workspace-muted)); background: color-mix(in srgb, currentColor 7%, transparent); font: 650 var(--user-message-summary-size, .88em)/1.35 var(--user-message-summary-font, inherit); letter-spacing: -.01em; }
  button { min-height: 24px; padding: 2px 5px 2px 7px; display: inline-flex; align-items: center; justify-content: center; gap: 3px; border: 0; border-radius: 7px; color: var(--user-message-muted, var(--workspace-muted)); background: transparent; font: 700 var(--user-message-action-size, .78em)/1 var(--user-message-action-font, inherit); cursor: pointer; transition: color 120ms ease, background-color 120ms ease; }
  button:hover, button:focus-visible { color: var(--user-message-accent, var(--workspace-accent)); background: color-mix(in srgb, currentColor 9%, transparent); }
  button:focus-visible { outline: 2px solid color-mix(in srgb, var(--user-message-accent, var(--workspace-accent)) 70%, transparent); outline-offset: 2px; }
  button :global(.lume-icon) { transition: transform 180ms cubic-bezier(.16, 1, .3, 1); }
  .expanded button :global(.lume-icon) { transform: rotate(180deg); }

  @keyframes message-reveal { from { opacity: .55; filter: blur(2px); } to { opacity: 1; filter: blur(0); } }
  @media (prefers-reduced-motion: reduce) { pre { animation: none; } button :global(.lume-icon) { transition: none; } }
</style>
