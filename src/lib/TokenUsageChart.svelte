<script lang="ts">
  import type { PromptTokenUsage } from "$lib/domain";
  import type { Language } from "$lib/i18n";
  import { TOKEN_GRAPH_HEIGHT, TOKEN_GRAPH_WIDTH, contextPercent, tokenGraphGeometry } from "$lib/tokenUsage";

  /** The tokens of each prompt over time, as the Inspector and the chat header show it. */
  let { samples, language, loading = false, limit = 14 }: {
    samples: PromptTokenUsage[];
    language: Language;
    loading?: boolean;
    /** How many of the latest prompts are drawn. */
    limit?: number;
  } = $props();

  const shown = $derived(samples.slice(-limit));
  const graph = $derived(tokenGraphGeometry(shown));
  let plot = $state<HTMLDivElement | null>(null);
  let active = $state<number | null>(null);
  const point = $derived(active !== null ? graph.dots[active] ?? null : null);
  // The chart's drawn height; the tooltip is placed over it in pixels.
  const plotHeight = 48;

  function tr(english: string, portuguese: string) {
    return language === "pt-BR" ? portuguese : english;
  }
  const number = (value: number) => Math.round(value).toLocaleString(language);
  const time = (value: number) => new Date(value).toLocaleString(language, { hour: "2-digit", minute: "2-digit", day: "2-digit", month: "short" });

  /** The prompt closest to the pointer along the time axis. */
  function pick(event: PointerEvent) {
    if (!plot || !graph.dots.length) return;
    const bounds = plot.getBoundingClientRect();
    const x = ((event.clientX - bounds.left) / Math.max(1, bounds.width)) * TOKEN_GRAPH_WIDTH;
    let nearest = 0;
    graph.dots.forEach((dot, index) => { if (Math.abs(dot.x - x) < Math.abs(graph.dots[nearest].x - x)) nearest = index; });
    active = nearest;
  }
  function keydown(event: KeyboardEvent) {
    if (!graph.dots.length) return;
    const last = graph.dots.length - 1;
    const next = event.key === "ArrowLeft" ? Math.max(0, (active ?? last) - 1)
      : event.key === "ArrowRight" ? Math.min(last, (active ?? last) + 1)
      : event.key === "Home" ? 0 : event.key === "End" ? last : null;
    if (next === null) return;
    event.preventDefault();
    active = next;
  }
</script>

<div class="token-chart" aria-busy={loading && !shown.length}>
  <span>{tr("Tokens / prompt", "Tokens / prompt")}</span>
  <!-- Pointer and arrow keys move between prompts; the details show in the tooltip, which is also announced. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div
    class="token-plot"
    bind:this={plot}
    role="group"
    tabindex={graph.dots.length ? 0 : -1}
    aria-label={tr("Tokens used per prompt over time. Use the arrow keys to read each prompt.", "Tokens usados por prompt ao longo do tempo. Use as setas para ler cada prompt.")}
    onpointermove={pick}
    onpointerleave={() => (active = null)}
    onfocus={() => { if (graph.dots.length) active = graph.dots.length - 1; }}
    onblur={() => (active = null)}
    onkeydown={keydown}
  >
    <svg class:empty={!graph.points} class:loading={loading && !shown.length} class="token-graph" viewBox="0 0 {TOKEN_GRAPH_WIDTH} {TOKEN_GRAPH_HEIGHT}" preserveAspectRatio="none" aria-hidden="true">
      <path class="graph-grid" d="M4 14H222M4 28H222M4 42H222" />
      <polygon points={graph.area} />
      <polyline points={graph.points || "4,42 222,42"} />
      {#if point}<path class="graph-cursor" d="M{point.x} 4V{TOKEN_GRAPH_HEIGHT - 6}" />{/if}
      {#each graph.dots as dot, index (dot.sample.turnId + index)}
        <circle class:active={index === active} cx={dot.x} cy={dot.y} r={index === active ? 3 : 2} />
      {/each}
    </svg>
    {#if point}
      {@const context = contextPercent(point.sample)}
      <div class="token-tip" class:edge-left={point.x < 60} class:edge-right={point.x > TOKEN_GRAPH_WIDTH - 60} role="status" style:left={(point.x / TOKEN_GRAPH_WIDTH) * 100 + "%"} style:top={(point.y / TOKEN_GRAPH_HEIGHT) * plotHeight + "px"}>
        <strong>{number(point.sample.totalTokens)} tokens</strong>
        <span>{tr("Input", "Entrada")} {number(point.sample.inputTokens)} · {tr("Output", "Saída")} {number(point.sample.outputTokens)}</span>
        {#if context !== null}<span>{tr("Context", "Contexto")} {context}%</span>{/if}
        <small>{time(point.sample.createdAt)}</small>
      </div>
    {/if}
  </div>
  <small>{loading && !shown.length ? tr("Loading…", "Carregando…") : tr("Time →", "Tempo →")}</small>
</div>

<style>
  .token-chart { min-width: 0; display: grid; grid-template-rows: auto 48px auto; gap: 3px; }
  .token-chart > span, .token-chart > small { color: var(--workspace-muted); font-size: 10px; font-weight: 720; line-height: 1.2; }
  .token-chart > small { color: var(--workspace-faint); text-align: right; }
  .token-plot { position: relative; min-width: 0; height: 48px; border-radius: 6px; outline: none; }
  .token-plot:focus-visible { box-shadow: 0 0 0 2px color-mix(in srgb, var(--workspace-accent) 45%, transparent); }
  .token-graph { display: block; width: 100%; min-width: 0; height: 48px; overflow: visible; }
  .token-graph :is(path, polyline, circle) { vector-effect: non-scaling-stroke; }
  .token-graph .graph-grid { fill: none; stroke: color-mix(in srgb, var(--workspace-line) 54%, transparent); stroke-width: .7; }
  .token-graph polygon { fill: color-mix(in srgb, var(--workspace-accent) 8%, transparent); }
  .token-graph polyline { fill: none; stroke: var(--workspace-accent); stroke-width: 1.4; stroke-linecap: round; stroke-linejoin: round; }
  .token-graph.empty polyline { stroke: var(--workspace-line); stroke-dasharray: 3 4; }
  .token-graph circle { fill: var(--workspace-raised); stroke: var(--workspace-accent); stroke-width: 1.3; }
  .token-graph circle.active { fill: var(--workspace-accent); }
  .token-graph .graph-cursor { fill: none; stroke: color-mix(in srgb, var(--workspace-accent) 40%, transparent); stroke-width: 1; stroke-dasharray: 2 3; }
  .token-graph.loading { animation: token-pulse 1.2s ease-in-out infinite alternate; }
  /* The tooltip sits above the prompt and slides inward at the edges so it stays over the chart. */
  .token-tip { position: absolute; z-index: 20; bottom: auto; min-width: max-content; padding: 6px 9px; display: grid; gap: 2px; border: 1px solid var(--workspace-line); border-radius: 9px; color: var(--workspace-text); background: var(--workspace-raised); box-shadow: 0 10px 26px rgba(8, 18, 13, .18); font-size: 10px; line-height: 1.35; pointer-events: none; transform: translate(-50%, calc(-100% - 9px)); }
  .token-tip.edge-left { transform: translate(-14px, calc(-100% - 9px)); }
  .token-tip.edge-right { transform: translate(calc(-100% + 14px), calc(-100% - 9px)); }
  .token-tip strong { color: var(--workspace-strong); font-size: 11px; font-weight: 750; font-variant-numeric: tabular-nums; }
  .token-tip span { color: var(--workspace-text); font-variant-numeric: tabular-nums; }
  .token-tip small { color: var(--workspace-muted); font-size: 9px; }
  @keyframes token-pulse { from { opacity: .45; } to { opacity: 1; } }
  @media (prefers-reduced-motion: reduce) { .token-graph.loading { animation: none; } }
</style>
