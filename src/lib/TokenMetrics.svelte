<script lang="ts">
  import type { AgentSession, PromptTokenUsage } from "$lib/domain";
  import type { Language } from "$lib/i18n";
  import TokenUsageChart from "$lib/TokenUsageChart.svelte";
  import { sampleContextPercent, sessionContextUsage } from "$lib/contextUsage";
  import { TOKEN_GRAPH_WIDTH, compactDuration, compactTokens, contextFillGeometry, promptJumpForSample, tokenMetricStats, type PromptJumpTarget } from "$lib/tokenUsage";

  /** The Inspector's Metrics section: every prompt's tokens, the context over time and the session's figures. */
  let { samples, agent, language, onJump }: {
    samples: PromptTokenUsage[];
    agent: AgentSession["agent"];
    language: Language;
    /** Scrolls the chat to the prompt behind a sample. */
    onJump?: (target: PromptJumpTarget) => void;
  } = $props();

  let mode = $state<"line" | "bars">("bars");
  const stats = $derived(tokenMetricStats(samples));
  const context = $derived(sessionContextUsage({ agent, promptTokenUsage: samples }));
  const fillHeight = 34;
  const fill = $derived(contextFillGeometry(samples, TOKEN_GRAPH_WIDTH, fillHeight, (sample) => sampleContextPercent(agent, sample)));

  function tr(english: string, portuguese: string) {
    return language === "pt-BR" ? portuguese : english;
  }
  const number = (value: number) => Math.round(value).toLocaleString(language);
  const compact = (value: number) => compactTokens(value, language);
  function jump(index: number) {
    const target = promptJumpForSample(samples, index);
    if (target) onJump?.(target);
  }
</script>

<div class="token-metrics">
  <div class="metrics-toolbar">
    <div class="mode-toggle" role="group" aria-label={tr("Chart style", "Estilo do gráfico")}>
      <button type="button" class:active={mode === "bars"} aria-pressed={mode === "bars"} onclick={() => (mode = "bars")}>{tr("Bars", "Barras")}</button>
      <button type="button" class:active={mode === "line"} aria-pressed={mode === "line"} onclick={() => (mode = "line")}>{tr("Line", "Linha")}</button>
    </div>
    {#if mode === "bars"}
      <span class="legend" aria-hidden="true"><i class="input"></i>{tr("Input", "Entrada")}<i class="output"></i>{tr("Output", "Saída")}</span>
    {/if}
  </div>
  <TokenUsageChart {samples} {language} {mode} limit={Math.max(1, samples.length)} height={108} title={tr(`All ${samples.length} prompts`, `Todos os ${samples.length} prompts`)} onSelect={onJump ? jump : undefined} />

  {#if fill.dots.length > 1}
    <div class="context-fill">
      <span>{tr("Context fill", "Uso do contexto")}</span>
      <svg viewBox="0 0 {TOKEN_GRAPH_WIDTH} {fillHeight}" preserveAspectRatio="none" role="img" aria-label={tr(`Context use after each prompt, now ${context?.percent ?? fill.dots.at(-1)?.percent}%`, `Uso do contexto após cada prompt, agora ${context?.percent ?? fill.dots.at(-1)?.percent}%`)}>
        <path class="limit" d="M4 4H222" />
        <path class="half" d="M4 {fillHeight / 2}H222" />
        <polyline points={fill.points} />
      </svg>
    </div>
  {/if}

  <dl class="metric-tiles">
    <div><dt>{tr("Session total", "Total da sessão")}</dt><dd title={number(stats.totalTokens)}>{compact(stats.totalTokens)}</dd></div>
    <div><dt>{tr("Per turn", "Por turno")}</dt><dd title={number(stats.averageTokens)}>{compact(stats.averageTokens)}</dd></div>
    {#if stats.largest}
      {@const largest = stats.largest}
      <div>
        <dt>{tr("Largest turn", "Maior turno")}</dt>
        <dd title={number(largest.sample.totalTokens)}>
          {#if onJump}<button type="button" title={tr("Open this prompt in the chat", "Abrir este prompt no chat")} onclick={() => jump(largest.index)}>{compact(largest.sample.totalTokens)}</button>{:else}{compact(largest.sample.totalTokens)}{/if}
        </dd>
      </div>
    {/if}
    {#if context}
      <div class="level-{context.level}"><dt>{tr("Context now", "Contexto agora")}</dt><dd title={`${number(context.tokens)} / ${number(context.window)}`}>{context.percent}%</dd></div>
    {/if}
    {#if stats.cacheHitPercent !== null}
      <div><dt>{tr("Cache hits", "Acerto de cache")}</dt><dd title={tr("Share of input read from the prompt cache", "Parte da entrada lida do cache de prompt")}>{stats.cacheHitPercent}%</dd></div>
    {/if}
    {#if stats.averageDurationMs !== null}
      <div><dt>{tr("Time per turn", "Tempo por turno")}</dt><dd>{compactDuration(stats.averageDurationMs)}</dd></div>
    {/if}
    {#if stats.model}
      <div class="wide"><dt>{tr("Model", "Modelo")}</dt><dd title={stats.model}>{stats.model}</dd></div>
    {/if}
  </dl>
</div>

<style>
  .token-metrics { min-width: 0; display: grid; gap: 10px; }
  .metrics-toolbar { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  .mode-toggle { display: inline-flex; gap: 2px; padding: 2px; border-radius: 7px; background: var(--workspace-subtle); }
  .mode-toggle button { min-height: 22px; padding: 0 8px; border: 0; border-radius: 5px; color: var(--workspace-muted); background: transparent; font: inherit; font-size: 10px; font-weight: 700; cursor: pointer; }
  .mode-toggle button.active { color: var(--workspace-strong); background: var(--workspace-pane); }
  .mode-toggle button:focus-visible { outline: 2px solid var(--workspace-accent); outline-offset: -2px; }
  .legend { display: inline-flex; align-items: center; gap: 4px; color: var(--workspace-muted); font-size: 10px; }
  .legend i { width: 8px; height: 8px; margin-left: 4px; border-radius: 2px; }
  .legend i.input { background: color-mix(in srgb, var(--workspace-accent) 58%, transparent); }
  .legend i.output { background: #c9963d; }
  .context-fill { display: grid; gap: 3px; }
  .context-fill > span { color: var(--workspace-muted); font-size: 10px; font-weight: 720; }
  .context-fill svg { display: block; width: 100%; height: 34px; overflow: visible; }
  .context-fill path, .context-fill polyline { vector-effect: non-scaling-stroke; fill: none; }
  .context-fill .limit { stroke: color-mix(in srgb, #c9963d 55%, transparent); stroke-width: 1; stroke-dasharray: 3 3; }
  .context-fill .half { stroke: color-mix(in srgb, var(--workspace-line) 60%, transparent); stroke-width: .7; }
  .context-fill polyline { stroke: var(--workspace-accent); stroke-width: 1.4; stroke-linejoin: round; stroke-linecap: round; }
  .metric-tiles { margin: 0; display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 4px; }
  .metric-tiles > div { min-width: 0; padding: 6px 8px; border-radius: 8px; background: var(--workspace-subtle); }
  .metric-tiles > div.wide { grid-column: 1 / -1; }
  .metric-tiles dt { color: var(--workspace-muted); font-size: 9px; font-weight: 700; }
  .metric-tiles dd { margin: 2px 0 0; overflow: hidden; color: var(--workspace-strong); font-size: 12px; font-weight: 720; font-variant-numeric: tabular-nums; text-overflow: ellipsis; white-space: nowrap; }
  .metric-tiles dd button { padding: 0; border: 0; color: var(--workspace-accent); background: transparent; font: inherit; text-decoration: underline dotted; text-underline-offset: 3px; cursor: pointer; }
  .metric-tiles .level-warn dd { color: #b8862f; }
  .metric-tiles .level-danger dd { color: #c0554f; }
</style>
