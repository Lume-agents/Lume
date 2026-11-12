<script lang="ts">
  import { onMount } from "svelte";
  import type { PromptTokenUsage } from "$lib/domain";
  import type { Language } from "$lib/i18n";
  import TokenUsageChart from "$lib/TokenUsageChart.svelte";
  import { compactTokens, tokenUsageSummary } from "$lib/tokenUsage";

  /** Token use in the chat header: the context left after the last prompt (or its tokens), and the chart on click. */
  let { usage, language }: { usage: PromptTokenUsage[]; language: Language } = $props();

  let open = $state(false);
  let root = $state<HTMLDivElement | null>(null);
  const summary = $derived(tokenUsageSummary(usage));
  const lastTokens = $derived(summary.last ? compactTokens(summary.last.totalTokens, language) : "");
  // The ring fills with the context used; r = 6 gives a circumference of about 37.7.
  const ring = 2 * Math.PI * 6;

  function tr(english: string, portuguese: string) {
    return language === "pt-BR" ? portuguese : english;
  }
  const number = (value: number) => Math.round(value).toLocaleString(language);
  const label = $derived(summary.context !== null
    ? tr(`Token usage: context ${summary.context}% full, last prompt ${lastTokens} tokens`, `Uso de tokens: contexto ${summary.context}% cheio, último prompt ${lastTokens} tokens`)
    : tr(`Token usage: last prompt ${lastTokens} tokens`, `Uso de tokens: último prompt ${lastTokens} tokens`));

  onMount(() => {
    const outside = (event: PointerEvent) => { if (open && root && !root.contains(event.target as Node)) open = false; };
    const escape = (event: KeyboardEvent) => { if (open && event.key === "Escape") { open = false; root?.querySelector<HTMLButtonElement>(".usage-trigger")?.focus(); } };
    document.addEventListener("pointerdown", outside);
    window.addEventListener("keydown", escape);
    return () => { document.removeEventListener("pointerdown", outside); window.removeEventListener("keydown", escape); };
  });
</script>

{#if usage.length}
  <div class="chat-usage" bind:this={root}>
    <button class:active={open} class="usage-trigger" type="button" title={label} aria-label={label} aria-expanded={open} onclick={() => (open = !open)}>
      {#if summary.context !== null}
        <svg class="usage-ring" class:high={summary.context >= 80} viewBox="0 0 16 16" aria-hidden="true">
          <circle cx="8" cy="8" r="6" />
          <circle class="used" cx="8" cy="8" r="6" stroke-dasharray="{(summary.context / 100) * ring} {ring}" />
        </svg>
        <span>{summary.context}%</span>
      {:else}
        <svg class="usage-bars" viewBox="0 0 16 16" aria-hidden="true"><path d="M3 13V9M8 13V4M13 13V7" /></svg>
        <span>{lastTokens}</span>
      {/if}
    </button>
    {#if open}
      <section class="usage-popover" aria-label={tr("Token usage", "Uso de tokens")}>
        <header><strong>{tr("Token usage", "Uso de tokens")}</strong><small>{summary.prompts === 1 ? tr("1 prompt", "1 prompt") : tr(`${summary.prompts} prompts`, `${summary.prompts} prompts`)}</small></header>
        <dl class="usage-totals">
          <div><dt>{tr("Total", "Total")}</dt><dd title={number(summary.totalTokens)}>{compactTokens(summary.totalTokens, language)}</dd></div>
          <div><dt>{tr("Input", "Entrada")}</dt><dd title={number(summary.inputTokens)}>{compactTokens(summary.inputTokens, language)}</dd></div>
          <div><dt>{tr("Output", "Saída")}</dt><dd title={number(summary.outputTokens)}>{compactTokens(summary.outputTokens, language)}</dd></div>
          <div><dt>{tr("Context", "Contexto")}</dt><dd>{summary.context !== null ? summary.context + "%" : "—"}</dd></div>
        </dl>
        <TokenUsageChart samples={usage} {language} />
      </section>
    {/if}
  </div>
{/if}

<style>
  .chat-usage { position: relative; }
  .usage-trigger { height: 29px; padding: 0 7px; display: flex; align-items: center; gap: 5px; border: 0; border-radius: 8px; color: var(--workspace-muted); background: transparent; font: inherit; font-size: 10px; font-weight: 720; font-variant-numeric: tabular-nums; cursor: pointer; transition: color 120ms ease, background 120ms ease; }
  .usage-trigger:hover, .usage-trigger.active { color: var(--workspace-accent); background: var(--workspace-subtle); }
  .usage-trigger:focus-visible { outline: 2px solid var(--workspace-accent); outline-offset: 1px; }
  .usage-ring, .usage-bars { width: 16px; height: 16px; flex: 0 0 auto; fill: none; }
  .usage-ring { transform: rotate(-90deg); }
  .usage-ring circle { stroke: color-mix(in srgb, var(--workspace-muted) 30%, transparent); stroke-width: 2.2; }
  .usage-ring circle.used { stroke: var(--workspace-accent); stroke-linecap: round; transition: stroke-dasharray 300ms ease; }
  .usage-ring.high circle.used { stroke: #c9963d; }
  .usage-bars path { stroke: currentColor; stroke-width: 2; stroke-linecap: round; }
  .usage-popover { position: absolute; z-index: 12; top: 36px; right: 0; width: 268px; padding: 12px 13px 10px; display: grid; gap: 10px; border: 1px solid var(--workspace-line); border-radius: 14px; color: var(--workspace-text); background: var(--workspace-raised); box-shadow: 0 16px 40px rgba(8, 18, 13, .18); animation: usage-arrive 180ms cubic-bezier(.16, 1, .3, 1) both; }
  .usage-popover header { display: flex; align-items: baseline; justify-content: space-between; gap: 8px; }
  .usage-popover header strong { color: var(--workspace-strong); font-size: 12px; font-weight: 720; }
  .usage-popover header small { color: var(--workspace-muted); font-size: 10px; }
  .usage-totals { margin: 0; display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 4px; }
  .usage-totals div { min-width: 0; padding: 6px 7px; border-radius: 8px; background: var(--workspace-subtle); }
  .usage-totals dt { color: var(--workspace-muted); font-size: 9px; font-weight: 700; }
  .usage-totals dd { margin: 2px 0 0; overflow: hidden; color: var(--workspace-strong); font-size: 11px; font-weight: 720; font-variant-numeric: tabular-nums; text-overflow: ellipsis; white-space: nowrap; }
  @keyframes usage-arrive { from { opacity: 0; transform: translateY(-4px); } }
  @media (prefers-reduced-motion: reduce) { .usage-popover, .usage-ring circle.used { animation: none; transition: none; } }
</style>
