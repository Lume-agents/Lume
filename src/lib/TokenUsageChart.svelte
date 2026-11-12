<script lang="ts">
  import type { PromptTokenUsage } from "$lib/domain";
  import type { Language } from "$lib/i18n";
  import { TOKEN_GRAPH_WIDTH, contextPercent, tokenBarsGeometry, tokenGraphGeometry } from "$lib/tokenUsage";

  /** The tokens of each prompt over time, as the Inspector and the chat header show it. */
  let { samples, language, loading = false, limit = 14, height = 48, mode = "line", title, onSelect }: {
    samples: PromptTokenUsage[];
    language: Language;
    loading?: boolean;
    /** How many of the latest prompts are drawn. */
    limit?: number;
    /** The chart's drawn height in pixels; the tooltip is placed over it. */
    height?: number;
    /** A line over time, or one bar per prompt with its output stacked on its input. */
    mode?: "line" | "bars";
    title?: string;
    /** Called with the index in `samples` of the prompt clicked (or chosen with Enter). */
    onSelect?: (index: number) => void;
  } = $props();

  const shown = $derived(samples.slice(-limit));
  const offset = $derived(samples.length - shown.length);
  const viewHeight = $derived(height + 6);
  const graph = $derived(tokenGraphGeometry(shown, TOKEN_GRAPH_WIDTH, viewHeight));
  const bars = $derived(mode === "bars" ? tokenBarsGeometry(shown, TOKEN_GRAPH_WIDTH, viewHeight) : []);
  // Where each prompt sits, for the pointer, the cursor and the tooltip, whichever way it is drawn.
  const marks = $derived(mode === "bars" ? bars.map((bar) => ({ x: bar.x, y: bar.outputY, sample: bar.sample })) : graph.dots);
  const grid = $derived([0.25, 0.5, 0.75].map((share) => `M4 ${(viewHeight * share + 1).toFixed(1)}H222`).join(""));
  let plot = $state<HTMLDivElement | null>(null);
  let active = $state<number | null>(null);
  const point = $derived(active !== null ? marks[active] ?? null : null);

  function tr(english: string, portuguese: string) {
    return language === "pt-BR" ? portuguese : english;
  }
  const number = (value: number) => Math.round(value).toLocaleString(language);
  const time = (value: number) => new Date(value).toLocaleString(language, { hour: "2-digit", minute: "2-digit", day: "2-digit", month: "short" });

  /** The prompt closest to the pointer along the time axis. */
  function pick(event: MouseEvent) {
    if (!plot || !marks.length) return;
    const bounds = plot.getBoundingClientRect();
    const x = ((event.clientX - bounds.left) / Math.max(1, bounds.width)) * TOKEN_GRAPH_WIDTH;
    let nearest = 0;
    marks.forEach((mark, index) => { if (Math.abs(mark.x - x) < Math.abs(marks[nearest].x - x)) nearest = index; });
    active = nearest;
  }
  function select() {
    if (onSelect && active !== null && marks[active]) onSelect(offset + active);
  }
  function keydown(event: KeyboardEvent) {
    if (!marks.length) return;
    if (event.key === "Enter" && onSelect && active !== null) { event.preventDefault(); select(); return; }
    const last = marks.length - 1;
    const next = event.key === "ArrowLeft" ? Math.max(0, (active ?? last) - 1)
      : event.key === "ArrowRight" ? Math.min(last, (active ?? last) + 1)
      : event.key === "Home" ? 0 : event.key === "End" ? last : null;
    if (next === null) return;
    event.preventDefault();
    active = next;
  }
</script>

<div class="token-chart" class:selectable={Boolean(onSelect)} aria-busy={loading && !shown.length} style:--plot-height="{height}px">
  <span>{title ?? tr("Tokens / prompt", "Tokens / prompt")}</span>
  <!-- Pointer and arrow keys move between prompts; the details show in the tooltip, which is also announced. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div
    class="token-plot"
    bind:this={plot}
    role="group"
    tabindex={marks.length ? 0 : -1}
    aria-label={onSelect
      ? tr("Tokens used per prompt over time. Use the arrow keys to read each prompt and Enter to open it in the chat.", "Tokens usados por prompt ao longo do tempo. Use as setas para ler cada prompt e Enter para abri-lo no chat.")
      : tr("Tokens used per prompt over time. Use the arrow keys to read each prompt.", "Tokens usados por prompt ao longo do tempo. Use as setas para ler cada prompt.")}
    onpointermove={pick}
    onpointerleave={() => (active = null)}
    onclick={(event) => { pick(event); select(); }}
    onfocus={() => { if (marks.length) active = marks.length - 1; }}
    onblur={() => (active = null)}
    onkeydown={keydown}
  >
    <svg class:empty={!graph.points} class:loading={loading && !shown.length} class="token-graph" viewBox="0 0 {TOKEN_GRAPH_WIDTH} {viewHeight}" preserveAspectRatio="none" aria-hidden="true">
      <path class="graph-grid" d={grid} />
      {#if point}<path class="graph-cursor" d="M{point.x} 4V{viewHeight - 6}" />{/if}
      {#if mode === "bars" && bars.length}
        {#each bars as bar, index (bar.sample.turnId + index)}
          <g class="token-bar" class:active={index === active}>
            <rect class="bar-input" x={bar.x - bar.width / 2} y={bar.inputY} width={bar.width} height={Math.max(0.5, bar.inputHeight)} rx="1" />
            {#if bar.outputHeight > 0}<rect class="bar-output" x={bar.x - bar.width / 2} y={bar.outputY} width={bar.width} height={bar.outputHeight} rx="1" />{/if}
          </g>
        {/each}
      {:else}
        <polygon points={graph.area} />
        <polyline points={graph.points || `4,${viewHeight - 12} 222,${viewHeight - 12}`} />
        {#each graph.dots as dot, index (dot.sample.turnId + index)}
          <circle class:active={index === active} cx={dot.x} cy={dot.y} r={index === active ? 3 : 2} />
        {/each}
      {/if}
    </svg>
    {#if point}
      {@const context = contextPercent(point.sample)}
      <div class="token-tip" class:edge-left={point.x < 60} class:edge-right={point.x > TOKEN_GRAPH_WIDTH - 60} role="status" style:left={(point.x / TOKEN_GRAPH_WIDTH) * 100 + "%"} style:top={(point.y / viewHeight) * height + "px"}>
        <strong>{number(point.sample.totalTokens)} tokens</strong>
        <span>{tr("Input", "Entrada")} {number(point.sample.inputTokens)} · {tr("Output", "Saída")} {number(point.sample.outputTokens)}</span>
        {#if context !== null}<span>{tr("Context", "Contexto")} {context}%</span>{/if}
        {#if point.sample.model}<span>{point.sample.model}</span>{/if}
        <small>{time(point.sample.createdAt)}{onSelect ? " · " + tr("click to open", "clique para abrir") : ""}</small>
      </div>
    {/if}
  </div>
  <small>{loading && !shown.length ? tr("Loading…", "Carregando…") : mode === "bars" ? tr("Prompts →", "Prompts →") : tr("Time →", "Tempo →")}</small>
</div>

<style>
  .token-chart { --plot-height: 48px; min-width: 0; display: grid; grid-template-rows: auto var(--plot-height) auto; gap: 3px; }
  .token-chart > span, .token-chart > small { color: var(--workspace-muted); font-size: 10px; font-weight: 720; line-height: 1.2; }
  .token-chart > small { color: var(--workspace-faint); text-align: right; }
  .token-plot { position: relative; min-width: 0; height: var(--plot-height); border-radius: 6px; outline: none; }
  .token-chart.selectable .token-plot { cursor: pointer; }
  .token-plot:focus-visible { box-shadow: 0 0 0 2px color-mix(in srgb, var(--workspace-accent) 45%, transparent); }
  .token-graph { display: block; width: 100%; min-width: 0; height: var(--plot-height); overflow: visible; }
  .token-graph :is(path, polyline, circle, rect) { vector-effect: non-scaling-stroke; }
  /* Input below, output stacked on it; output uses the warm accent the context ring warns with. */
  .token-graph .bar-input { fill: color-mix(in srgb, var(--workspace-accent) 58%, transparent); }
  .token-graph .bar-output { fill: #c9963d; }
  .token-graph .token-bar.active rect { stroke: var(--workspace-strong); stroke-width: 1; }
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
