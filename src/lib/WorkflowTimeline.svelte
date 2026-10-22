<script lang="ts">
  import { onMount } from "svelte";
  import type { SessionActivity, WorkflowConnectionDefinition, WorkflowRun, WorkflowStepDefinition } from "$lib/domain";
  import type { Language } from "$lib/i18n";
  import { localize } from "$lib/i18n";
  import { activityCategory, activityDisplayTitle, activityPreview, formatAgentDuration, type ActivityCategory } from "$lib/activityPresentation";
  import ActivityTypeIcon from "$lib/ActivityTypeIcon.svelte";
  import { buildWorkflowTimeline, timelineTicks, type TimelineSpan } from "$lib/workflowTimeline";
  import LumeIcon from "$lib/LumeIcon.svelte";

  let { run, steps, order, connections, activitiesFor, labelFor, language, onOpenStep }: {
    run: WorkflowRun | null;
    steps: WorkflowStepDefinition[];
    order: string[];
    connections: WorkflowConnectionDefinition[];
    activitiesFor: (step: WorkflowStepDefinition) => SessionActivity[];
    labelFor: (step: WorkflowStepDefinition) => { title: string; subtitle: string; agent?: string };
    language: Language;
    onOpenStep?: (step: WorkflowStepDefinition) => void;
  } = $props();

  const tr = (english: string, portuguese: string) => localize(language, english, portuguese);
  // Each step keeps one color across its spans and connectors.
  const palette = ["#5aa9f0", "#b083ee", "#ef8a3f", "#3fc48c", "#e9c04c", "#e46f8f"];
  let now = $state(Date.now());
  const model = $derived(buildWorkflowTimeline({ run, steps, order, connections, activitiesFor, now }));
  const duration = $derived(model ? Math.max(1_000, model.end - model.start) : 1_000);
  const ticks = $derived(timelineTicks(duration));
  const rowIndex = $derived(new Map((model?.rows ?? []).map((row, index) => [row.id, index])));
  let hovered = $state<TimelineSpan | null>(null);

  onMount(() => {
    // A live run grows every second; a finished one stays still.
    const timer = window.setInterval(() => { if (model?.live) now = Date.now(); }, 1_000);
    return () => window.clearInterval(timer);
  });

  const x = (at: number) => `${((at - (model?.start ?? 0)) / duration) * 100}%`;
  const w = (span: TimelineSpan) => `${Math.max(0.35, ((span.end - span.start) / duration) * 100)}%`;
  const stepOf = (stepId: string) => steps.find((step) => step.id === stepId);
  const colorOf = (rowId: string) => palette[(rowIndex.get(rowId) ?? 0) % palette.length];
  function seconds(ms: number) { return formatAgentDuration(Math.max(0, ms)); }
  function tick(ms: number) {
    const total = Math.round(ms / 1000);
    return total < 60 ? `${total}s` : `${Math.floor(total / 60)}m${total % 60 ? ` ${total % 60}s` : ""}`;
  }
  function kindLabel(span: TimelineSpan) {
    if (span.kind === "waiting") return span.label === "approval" ? tr("Waiting for your approval", "Aguardando sua aprovação") : span.label === "handoff" ? tr("Waiting for its turn", "Aguardando a vez") : tr("Waiting for you", "Aguardando você");
    return span.kind === "tool" ? tr("Tool call", "Ferramenta") : tr("Thinking", "Pensando");
  }
  /** The first sentence of an agent's message, without Markdown, short enough for a card. */
  function sentence(text: string | undefined) {
    const plain = (text ?? "").replace(/```[\s\S]*?```/g, " ").replace(/[*_`#>]+/g, "").replace(/\[([^\]]+)\]\([^)]*\)/g, "$1").replace(/\s+/g, " ").trim();
    const first = plain.match(/^.{12,}?[.!?](?=\s|$)/)?.[0] ?? plain;
    return first.length > 120 ? `${first.slice(0, 117).trimEnd()}…` : first;
  }
  function roleOf(rowId: string | undefined) {
    const row = model?.rows.find((item) => item.id === rowId);
    const step = row ? stepOf(row.stepId) : undefined;
    return step ? labelFor(step).title : "";
  }
  function lastMessageOf(rowId: string) {
    const row = model?.rows.find((item) => item.id === rowId);
    const step = row ? stepOf(row.stepId) : undefined;
    return step ? sentence(activitiesFor(step).findLast((activity) => activity.kind === "message")?.detail) : "";
  }
  /** What the hover card says about a span: the headline, the tool line and how it ended or connected. */
  function describe(span: TimelineSpan) {
    const activity = span.activity;
    const category: ActivityCategory | "analysis" = activity ? activityCategory(activity) : span.kind === "waiting" ? "tool" : "analysis";
    let headline: string;
    let meta: string;
    if (span.kind === "waiting") {
      headline = kindLabel(span);
      meta = tr("Waiting", "Aguardando");
    } else if (span.kind === "tool" && activity) {
      const files = activity.files.map((path) => path.split(/[\\/]/).at(-1)).filter(Boolean);
      headline = activityDisplayTitle(activity, language);
      meta = files.length ? files.slice(0, 3).join(", ") : activityPreview(activity) || headline;
    } else if (activity?.detail) {
      headline = sentence(activity.detail) || tr("Thought it through", "Pensou no próximo passo");
      meta = tr("Thinking", "Pensando");
    } else {
      headline = span.receivedFrom ? tr("Read the handoff and planned the work", "Leu a passagem e planejou o trabalho") : tr("Read the request and planned the work", "Leu o pedido e planejou o trabalho");
      meta = tr("Thinking", "Pensando");
    }
    const row = model?.rows.find((item) => item.id === span.rowId);
    let outcome: { kind: "failed" | "link"; text: string } | null = null;
    if (span.failed) {
      const reason = (span.kind === "tool" ? activityPreview(activity!) : "") || row?.error || "";
      outcome = { kind: "failed", text: reason ? tr(`Failed: ${reason}`, `Falhou: ${reason}`) : tr("Failed", "Falhou") };
    } else if (span.kind === "waiting") {
      const source = model?.handoffs.find((handoff) => handoff.to === span.rowId);
      const summary = source ? lastMessageOf(source.from) : "";
      if (source) outcome = { kind: "link", text: summary ? `${roleOf(source.from)}: ${summary}` : tr(`${roleOf(source.from)} finished`, `${roleOf(source.from)} terminou`) };
    } else if (span.receivedFrom) {
      const summary = lastMessageOf(span.receivedFrom);
      outcome = { kind: "link", text: tr(`From ${roleOf(span.receivedFrom)}`, `De ${roleOf(span.receivedFrom)}`) + (summary ? `: ${summary}` : "") };
    } else if (span.handedTo) {
      outcome = { kind: "link", text: tr(`Handed over to ${roleOf(span.handedTo)}`, `Passado para ${roleOf(span.handedTo)}`) };
    }
    return { headline, meta, category, outcome };
  }
  function statusLine() {
    if (!model || !run) return "";
    const state = { running: tr("Running", "Executando"), waiting_for_approval: tr("Waiting for approval", "Aguardando aprovação"), paused: tr("Paused", "Pausado"), ready: tr("Ready", "Pronto"), completed: tr("Finished", "Concluído"), failed: tr("Failed", "Falhou"), cancelled: tr("Cancelled", "Cancelado"), draft: tr("Draft", "Rascunho") }[run.status];
    const parts = [state, seconds(model.end - model.start), tr(`${model.rows.length} steps`, `${model.rows.length} etapas`)];
    const waited = model.end - model.start - model.workMs;
    if (!model.live && waited > 1_000) parts.push(tr(`${seconds(waited)} waiting`, `${seconds(waited)} esperando`));
    if (model.retries) parts.push(tr(`${model.retries} retr${model.retries === 1 ? "y" : "ies"}`, `${model.retries} nova${model.retries === 1 ? "" : "s"} tentativa${model.retries === 1 ? "" : "s"}`));
    return parts.join(" · ");
  }
</script>

<section class="timeline" aria-label={tr("Workflow timeline", "Linha do tempo do workflow")}>
  {#if !model}
    <div class="timeline-empty">
      <LumeIcon name="task-list" size={22} />
      <strong>{tr("No run yet", "Nenhuma execução ainda")}</strong>
      <p>{tr("Start the workflow to follow each step here, with the time it worked, waited and handed over.", "Inicie o workflow para acompanhar cada etapa aqui: quanto tempo trabalhou, esperou e passou adiante.")}</p>
    </div>
  {:else}
    <header>
      <strong>{run?.objective?.trim() || tr("Workflow run", "Execução do workflow")}</strong>
      <span class:live={model.live}>{#if model.live}<i aria-hidden="true"></i>{:else if run?.status === "completed"}<LumeIcon name="check" size={12} />{/if}{statusLine()}</span>
    </header>
    <div class="grid" style:--rows={model.rows.length}>
      <ol class="labels">
        {#each model.rows as row (row.id)}
          {@const step = stepOf(row.stepId)}
          {@const label = step ? labelFor(step) : { title: "", subtitle: "" }}
          <li>
            <button type="button" disabled={!step || !onOpenStep} title={tr("Open this chat", "Abrir este chat")} onclick={() => step && onOpenStep?.(step)}>
              <i style:--c={colorOf(row.id)} class:failed={row.status === "failed"} class:running={row.status === "running"}></i>
              <span><strong>{label.title}</strong><small>{label.subtitle}</small>{#if row.attempt > 1}<small class="retry"><LumeIcon name="refresh" size={9} />{tr(`attempt ${row.attempt}`, `tentativa ${row.attempt}`)}</small>{/if}</span>
            </button>
          </li>
        {/each}
      </ol>
      <div class="plot" role="group" aria-label={tr("Steps over time", "Etapas ao longo do tempo")}>
        {#each ticks as at (at)}<span class="tick" style:left={`${(at / duration) * 100}%`}><b>{tick(at)}</b></span>{/each}
        {#each model.handoffs as handoff (handoff.id)}
          {@const from = rowIndex.get(handoff.from) ?? 0}
          {@const to = rowIndex.get(handoff.to) ?? 0}
          <span class="handoff" class:pending={!handoff.done} style:left={x(handoff.at)} style:--from={Math.min(from, to)} style:--to={Math.max(from, to)} style:--c={colorOf(handoff.from)} title={handoff.approval ? tr("Handed over after approval", "Passado adiante após aprovação") : tr("Handed over automatically", "Passado adiante automaticamente")}>
            <i class="dot top"></i><i class="dot bottom"></i>
          </span>
        {/each}
        {#each model.rows as row, index (row.id)}
          <div class="lane" style:--i={index} style:--c={colorOf(row.id)}>
            {#each model.spans.filter((span) => span.rowId === row.id) as span (span.id)}
              <button type="button" class="span kind-{span.kind}" class:open={span.open} class:failed={span.failed}
                style:left={x(span.start)} style:width={w(span)}
                aria-label={`${kindLabel(span)}${span.label && span.kind !== "waiting" ? `: ${span.label}` : ""} · ${seconds(span.end - span.start)}`}
                onmouseenter={() => (hovered = span)} onmouseleave={() => (hovered = null)} onfocus={() => (hovered = span)} onblur={() => (hovered = null)}
                onclick={() => { const step = stepOf(row.stepId); if (step) onOpenStep?.(step); }}>
                {#if span.failed}<LumeIcon name="close" size={10} />{/if}
              </button>
            {/each}
          </div>
        {/each}
        {#if model.live}<span class="now" style:left={x(now)} aria-hidden="true"></span>{/if}
        {#if hovered}
          {@const index = rowIndex.get(hovered.rowId) ?? 0}
          {@const row = model.rows.find((item) => item.id === hovered?.rowId)}
          {@const step = row ? stepOf(row.stepId) : undefined}
          {@const who = step ? labelFor(step) : { title: "", subtitle: "", agent: "" }}
          {@const card = describe(hovered)}
          <div class="card" class:below={index === 0} style:left={`clamp(0px, calc(${x(hovered.start)} - 24px), calc(100% - 290px))`} style:--i={index} style:--c={colorOf(hovered.rowId)} aria-hidden="true">
            <small class="who"><i></i><b>{who.title}</b>{#if who.agent}<em>{who.agent}</em>{/if}</small>
            <strong>{card.headline}</strong>
            <span class="meta">{#if hovered.kind !== "waiting"}<ActivityTypeIcon category={card.category} size={13} /><code>{card.meta}</code><b>·</b>{/if}{seconds(hovered.end - hovered.start)}{#if hovered.open}<b>·</b>{tr("in progress", "em andamento")}{/if}</span>
            {#if card.outcome}<span class="outcome {card.outcome.kind}">{#if card.outcome.kind === "failed"}<LumeIcon name="close" size={11} />{:else}<svg viewBox="0 0 16 16" width="12" height="12" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M4 3v6.5a2 2 0 0 0 2 2h6M9.5 8.5 12 11.5 9.5 14.5" /></svg>{/if}<span>{card.outcome.text}</span></span>{/if}
          </div>
        {/if}
      </div>
    </div>
    <footer aria-label={tr("Legend", "Legenda")}>
      <span><i class="key thinking"></i>{tr("Thinking", "Pensando")}</span>
      <span><i class="key tool"></i>{tr("Tool call", "Ferramenta")}</span>
      <span><i class="key waiting"></i>{tr("Waiting", "Aguardando")}</span>
      <span><i class="key handoff"></i>{tr("Handoff", "Passagem")}</span>
      <span><LumeIcon name="close" size={10} />{tr("Failed", "Falhou")}</span>
    </footer>
  {/if}
</section>

<style>
  .timeline { --row: 46px; --label: 190px; min-width: 0; padding: 18px 20px 14px; display: grid; gap: 14px; border: 1px solid var(--workspace-line); border-radius: 18px; color: var(--workspace-text); background: var(--workspace-raised); box-shadow: 0 18px 50px rgba(0, 0, 0, .14); }
  header { display: grid; gap: 4px; }
  header strong { overflow: hidden; color: var(--workspace-strong); font-size: 14px; font-weight: 700; text-overflow: ellipsis; white-space: nowrap; }
  header span { display: flex; align-items: center; gap: 6px; color: var(--workspace-muted); font-size: 11.5px; font-variant-numeric: tabular-nums; }
  header span.live i { width: 7px; height: 7px; border-radius: 50%; background: var(--workspace-accent); box-shadow: 0 0 0 0 color-mix(in srgb, var(--workspace-accent) 50%, transparent); animation: live-pulse 1.6s ease-out infinite; }
  .grid { display: grid; grid-template-columns: var(--label) minmax(0, 1fr); }
  .labels { margin: 0; padding: 0; list-style: none; }
  .labels li { height: var(--row); display: flex; align-items: center; }
  .labels button { min-width: 0; width: 100%; padding: 4px 8px 4px 0; display: flex; align-items: center; gap: 9px; border: 0; color: inherit; background: none; font: inherit; text-align: left; cursor: pointer; }
  .labels button:disabled { cursor: default; }
  .labels button:hover:not(:disabled) strong { color: var(--workspace-accent); }
  .labels i { width: 9px; height: 9px; flex: 0 0 auto; border-radius: 50%; background: var(--c); }
  .labels i.running { box-shadow: 0 0 0 3px color-mix(in srgb, var(--c) 28%, transparent); }
  .labels i.failed { background: #d9645c; }
  .labels span { min-width: 0; display: grid; gap: 1px; }
  .labels strong { overflow: hidden; color: var(--workspace-strong); font-size: 12px; font-weight: 680; text-overflow: ellipsis; white-space: nowrap; transition: color 140ms ease; }
  .labels small { display: flex; align-items: center; gap: 3px; overflow: hidden; color: var(--workspace-muted); font-size: 10.5px; text-overflow: ellipsis; white-space: nowrap; }
  .labels small.retry { color: var(--workspace-accent); }
  .plot { position: relative; height: calc(var(--rows) * var(--row) + 26px); margin-right: 6px; }
  .tick { position: absolute; top: 0; bottom: 20px; width: 0; border-left: 1px solid color-mix(in srgb, var(--workspace-line) 70%, transparent); }
  .tick b { position: absolute; top: calc(100% + 6px); left: 0; color: var(--workspace-faint); font-size: 10px; font-weight: 600; font-variant-numeric: tabular-nums; transform: translateX(-50%); white-space: nowrap; }
  .tick:first-child b { transform: none; }
  .lane { position: absolute; left: 0; right: 0; top: calc(var(--i) * var(--row) + (var(--row) - 18px) / 2); height: 18px; }
  .span { position: absolute; top: 0; height: 18px; padding: 0; display: grid; place-items: center; border: 0; border-radius: 6px; color: #fff; cursor: pointer; transition: filter 140ms ease, box-shadow 140ms ease; }
  .span + .span { margin-left: 2px; }
  .span:hover, .span:focus-visible { filter: brightness(1.15); box-shadow: 0 0 0 2px color-mix(in srgb, var(--c) 45%, transparent); outline: none; }
  .kind-thinking { background: color-mix(in srgb, var(--c) 34%, var(--workspace-raised)); }
  .kind-tool { background: var(--c); }
  .kind-waiting { background: repeating-linear-gradient(135deg, color-mix(in srgb, var(--workspace-muted) 26%, transparent) 0 3px, transparent 3px 7px); box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--workspace-muted) 30%, transparent); }
  .span.open { border-top-right-radius: 1px; border-bottom-right-radius: 1px; overflow: hidden; }
  .span.open::after { position: absolute; inset: 0; content: ""; background: linear-gradient(90deg, transparent 0 60%, rgba(255, 255, 255, .35) 85%, transparent); animation: span-shine 1.8s ease-in-out infinite; }
  .span.failed { background: color-mix(in srgb, #d9645c 22%, var(--workspace-raised)); box-shadow: inset 0 0 0 1px #d9645c; color: #d9645c; }
  .handoff { position: absolute; z-index: 1; top: calc(var(--from) * var(--row) + var(--row) / 2 + 9px); height: calc((var(--to) - var(--from)) * var(--row) - 18px); width: 0; border-left: 1.5px solid color-mix(in srgb, var(--c) 75%, transparent); pointer-events: auto; }
  .handoff.pending { border-left-style: dashed; }
  .handoff .dot { position: absolute; left: -4px; width: 7px; height: 7px; border-radius: 50%; background: var(--c); box-shadow: 0 0 0 2px var(--workspace-raised); }
  .handoff .dot.top { top: -4px; }
  .handoff .dot.bottom { bottom: -4px; }
  .now { position: absolute; z-index: 2; top: -4px; bottom: 20px; width: 0; border-left: 1.5px solid var(--workspace-strong); }
  .now::after { position: absolute; bottom: -5px; left: -5px; width: 9px; height: 9px; border-radius: 50%; content: ""; background: var(--workspace-strong); }
  .card { position: absolute; z-index: 5; bottom: calc(100% - var(--i) * var(--row) - 2px); width: 290px; padding: 11px 13px 12px; display: grid; gap: 5px; border: 1px solid var(--workspace-line); border-radius: 14px; background: color-mix(in srgb, var(--workspace-raised) 96%, var(--workspace-strong)); box-shadow: 0 16px 40px rgba(0, 0, 0, .28); pointer-events: none; }
  .card.below { top: calc(var(--row) - 2px); bottom: auto; }
  .card .who { display: flex; align-items: center; gap: 6px; min-width: 0; font-size: 11px; }
  .card .who i { width: 7px; height: 7px; flex: 0 0 auto; border-radius: 50%; background: var(--c); }
  .card .who b { color: var(--workspace-strong); font-weight: 700; }
  .card .who em { overflow: hidden; color: var(--workspace-muted); font-style: normal; text-overflow: ellipsis; white-space: nowrap; }
  .card strong { display: -webkit-box; overflow: hidden; color: var(--workspace-strong); font-size: 13px; font-weight: 650; line-height: 1.35; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; }
  .card .meta { min-width: 0; display: flex; align-items: center; gap: 6px; color: var(--workspace-muted); font-size: 11px; font-variant-numeric: tabular-nums; white-space: nowrap; }
  .card .meta code { min-width: 0; overflow: hidden; font: 10.5px/1.4 var(--lume-font-code, ui-monospace, monospace); text-overflow: ellipsis; }
  .card .meta b { color: var(--workspace-faint); font-weight: 400; }
  .card .outcome { display: flex; align-items: flex-start; gap: 6px; color: var(--workspace-muted); font-size: 11px; line-height: 1.45; }
  .card .outcome > :first-child { flex: 0 0 auto; margin-top: 2px; }
  .card .outcome span { display: -webkit-box; overflow: hidden; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; }
  .card .outcome.failed { color: #e0736b; }
  footer { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 14px; color: var(--workspace-muted); font-size: 10.5px; }
  footer span { display: inline-flex; align-items: center; gap: 6px; }
  .key { display: inline-block; width: 16px; height: 9px; border-radius: 3px; --c: var(--workspace-accent); }
  .key.thinking { background: color-mix(in srgb, var(--c) 34%, var(--workspace-raised)); }
  .key.tool { background: var(--c); }
  .key.waiting { background: repeating-linear-gradient(135deg, color-mix(in srgb, var(--workspace-muted) 40%, transparent) 0 2px, transparent 2px 5px); box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--workspace-muted) 30%, transparent); }
  .key.handoff { margin-right: 2px; width: 9px; height: 12px; border-left: 1.5px solid var(--c); border-bottom: 1.5px solid var(--c); border-radius: 0 0 0 3px; }
  .timeline-empty { padding: 30px 12px; display: grid; justify-items: center; gap: 7px; color: var(--workspace-muted); text-align: center; }
  .timeline-empty strong { color: var(--workspace-strong); font-size: 13px; }
  .timeline-empty p { max-width: 340px; margin: 0; font-size: 11.5px; line-height: 1.5; }
  @keyframes span-shine { from { transform: translateX(-100%); } to { transform: translateX(100%); } }
  @keyframes live-pulse { 70% { box-shadow: 0 0 0 6px transparent; } 100% { box-shadow: 0 0 0 0 transparent; } }
  @media (max-width: 640px) { .timeline { --label: 120px; } }
  @media (prefers-reduced-motion: reduce) { .span.open::after, header span.live i { animation: none; } .span, .labels strong { transition: none; } }
</style>
