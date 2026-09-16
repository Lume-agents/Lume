<script lang="ts">
  import BrandIcon from "$lib/BrandIcon.svelte";
  import LumeIcon from "$lib/LumeIcon.svelte";
  import FileTypeIcon from "$lib/FileTypeIcon.svelte";
  import type { HubSession } from "$lib/hubProtocol";
  import type { Language } from "$lib/i18n";
  import { displayText } from "$lib/i18n";
  import { displayFileChangePath } from "$lib/fileChanges";
  import { buildReviewTurns } from "$lib/reviewDiffs";

  let { session, language = "en", onClose, onOpenReview } = $props<{
    session: HubSession | null;
    language?: Language;
    onClose: () => void;
    onOpenReview: (path?: string) => void;
  }>();

  const latestTurn = $derived(session ? buildReviewTurns(session.activities, session.results, session.workingDirectory)[0] : null);
  const changes = $derived(latestTurn?.files ?? []);
  const checks = $derived(latestTurn?.checks ?? []);
  const currentWork = $derived(session?.workSummary.plan ?? session?.workSummary.todo ?? null);

  function tr(english: string, portuguese: string) {
    return language === "pt-BR" ? portuguese : english;
  }

  function remainingRate(used: number) {
    return Math.max(0, Math.min(100, Math.round(100 - used)));
  }

  function formatReset(value?: number) {
    if (!value) return "";
    return new Intl.DateTimeFormat(language, { hour: "2-digit", minute: "2-digit" }).format(new Date(value));
  }
</script>

<aside class="workspace-inspector" aria-label={tr("Session inspector", "Inspector da sessão")}>
  <header>
    <span><strong>{tr("Inspector", "Inspector")}</strong><small>{session ? displayText(language, session.statusLabel) : tr("No session selected", "Nenhuma sessão selecionada")}</small></span>
    <button type="button" aria-label={tr("Close inspector", "Fechar inspector")} title={tr("Close inspector", "Fechar inspector")} onclick={onClose}>
      <LumeIcon name="close" size={16} />
    </button>
  </header>

  {#if session}
    <div class="inspector-scroll">
      <section class="session-summary">
        <span class="agent-icon"><BrandIcon name={session.agent} size={21} /></span>
        <span><strong>{session.sessionName?.trim() || session.project || session.agentLabel}</strong><small>{session.agentLabel} · {session.project}</small></span>
      </section>

      <details class="inspector-section" open>
        <summary>{tr("Current work", "Trabalho atual")}</summary>
        {#if session.workSummary.goal}
          <div class="goal-summary">
            <span class="goal-state state-{session.workSummary.goal.status}"></span>
            <span><strong>{session.workSummary.goal.objective}</strong><small>{displayText(language, session.workSummary.goal.status)}</small></span>
          </div>
        {/if}
        {#if currentWork?.items.length}
          <ul class="work-items">
            {#each currentWork.items as item}
              <li class:item-active={item.status === "in_progress"} class:item-done={item.status === "completed"}><i></i><span>{item.label}</span></li>
            {/each}
          </ul>
        {:else if !session.workSummary.goal}
          <p class="empty-section">{tr("No active plan or goal.", "Nenhum plano ou objetivo ativo.")}</p>
        {/if}
      </details>

      <details class="inspector-section" open>
        <summary><span>{tr("Changed files", "Arquivos alterados")}</span><em>{changes.length}</em></summary>
        {#if changes.length}
          <button class="open-review" type="button" onclick={() => onOpenReview()}><LumeIcon name="diff" size={13} /><span>{tr("Open review center", "Abrir central de revisão")}</span></button>
          <div class="inspector-files">
            {#each changes as file (file.path)}
              <button type="button" title={file.path} onclick={() => onOpenReview(file.path)}>
                <FileTypeIcon path={file.path} />
                <span>{displayFileChangePath(file.path)}</span>
                <b>+{file.added}</b><i>-{file.removed}</i>
              </button>
            {/each}
          </div>
        {:else}
          <p class="empty-section">{tr("No changed files in the latest turn.", "Nenhum arquivo alterado no último turno.")}</p>
        {/if}
      </details>

      <details class="inspector-section" open>
        <summary><span>{tr("Checks", "Validações")}</span><em>{checks.length}</em></summary>
        {#if checks.length}
          <ul class="check-list">{#each checks as check}<li><i></i><span>{check}</span></li>{/each}</ul>
        {:else}
          <p class="empty-section">{tr("No checks reported in the latest turn.", "Nenhuma validação informada no último turno.")}</p>
        {/if}
      </details>

      <details class="inspector-section" open>
        <summary>{tr("Usage", "Uso")}</summary>
        {#if session.rateLimits?.length}
          <div class="rate-limits">
            {#each session.rateLimits as limit (limit.id)}
              {@const remaining = remainingRate(Number(limit.usedPercent))}
              <div>
                <span><strong>{limit.label}</strong><small>{remaining}% {tr("left", "restante")}{limit.resetsAt ? ` · ${formatReset(limit.resetsAt)}` : ""}</small></span>
                <i><b style:width={`${remaining}%`}></b></i>
              </div>
            {/each}
          </div>
        {:else}
          <p class="empty-section">{tr("Rate limits are unavailable for this source.", "Limites indisponíveis para esta origem.")}</p>
        {/if}
      </details>

      <details class="inspector-section metadata-section">
        <summary>{tr("Session details", "Detalhes da sessão")}</summary>
        <dl>
          <div><dt>{tr("Source", "Origem")}</dt><dd>{session.source}</dd></div>
          <div><dt>{tr("Access", "Acesso")}</dt><dd>{displayText(language, session.permissionProfile.label)}</dd></div>
          <div><dt>{tr("Directory", "Diretório")}</dt><dd title={session.workingDirectory}>{session.workingDirectory || "—"}</dd></div>
          <div><dt>{tr("Thread", "Thread")}</dt><dd title={session.nativeSessionId}>{session.nativeSessionId || "—"}</dd></div>
        </dl>
      </details>
    </div>
  {:else}
    <div class="inspector-empty"><BrandIcon name="lume" size={30} /><span>{tr("Select an agent to inspect its work.", "Selecione um agente para inspecionar o trabalho.")}</span></div>
  {/if}
</aside>

<style>
  .workspace-inspector { min-width: 0; height: 100%; display: grid; grid-template-rows: auto minmax(0, 1fr); overflow: hidden; border-left: 1px solid var(--workspace-line); color: var(--workspace-text); background: var(--workspace-sidebar); }
  .workspace-inspector > header { min-height: 64px; padding: 11px 12px 11px 15px; display: flex; align-items: center; gap: 8px; border-bottom: 1px solid var(--workspace-line); }
  .workspace-inspector > header > span { min-width: 0; display: grid; gap: 2px; flex: 1; }.workspace-inspector > header strong { color: var(--workspace-strong); font-size: 11px; }.workspace-inspector > header small { color: var(--workspace-muted); font-size: 8px; }
  .workspace-inspector > header button { width: 29px; height: 29px; display: grid; place-items: center; border: 0; border-radius: 8px; color: var(--workspace-muted); background: transparent; cursor: pointer; }.workspace-inspector > header button:hover { color: var(--workspace-accent); background: var(--workspace-subtle); }
  .inspector-scroll { min-height: 0; padding: 0 14px 24px; overflow-y: auto; overscroll-behavior: contain; scrollbar-width: thin; scrollbar-color: var(--workspace-scroll-thumb) transparent; }
  .session-summary { min-height: 70px; display: flex; align-items: center; gap: 10px; border-bottom: 1px solid var(--workspace-line); }.session-summary > span:last-child { min-width: 0; display: grid; gap: 3px; }.session-summary strong, .session-summary small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }.session-summary strong { color: var(--workspace-strong); font-size: 10px; }.session-summary small { color: var(--workspace-muted); font-size: 8px; }.agent-icon { width: 31px; height: 31px; display: grid; place-items: center; flex: 0 0 auto; color: var(--workspace-accent); }
  .inspector-section { border-bottom: 1px solid var(--workspace-line); }.inspector-section > summary { min-height: 44px; display: flex; align-items: center; gap: 7px; color: var(--workspace-strong); font-size: 9px; font-weight: 750; list-style: none; cursor: pointer; }.inspector-section > summary::-webkit-details-marker { display: none; }.inspector-section > summary::after { width: 6px; height: 6px; margin-left: auto; border-right: 1.5px solid currentColor; border-bottom: 1.5px solid currentColor; content: ""; opacity: .5; transform: rotate(45deg); transition: transform 160ms ease; }.inspector-section[open] > summary::after { transform: rotate(225deg); }.inspector-section > summary em { min-width: 18px; height: 18px; display: grid; place-items: center; border-radius: 6px; color: var(--workspace-muted); background: var(--workspace-subtle); font-size: 7px; font-style: normal; }.inspector-section[open] { padding-bottom: 12px; }
  .goal-summary { padding: 4px 0 8px; display: flex; align-items: flex-start; gap: 8px; }.goal-summary > span:last-child { min-width: 0; display: grid; gap: 3px; }.goal-summary strong { color: var(--workspace-text); font-size: 9px; line-height: 1.4; }.goal-summary small { color: var(--workspace-muted); font-size: 7px; text-transform: capitalize; }.goal-state { width: 7px; height: 7px; margin-top: 3px; flex: 0 0 auto; border-radius: 50%; background: #4e98ca; }.goal-state.state-complete { background: #50aa79; }.goal-state.state-blocked { background: #c66762; }
  .work-items, .check-list { margin: 0; padding: 0; display: grid; gap: 6px; list-style: none; }.work-items li, .check-list li { min-width: 0; display: flex; align-items: flex-start; gap: 7px; color: var(--workspace-muted); font-size: 8px; line-height: 1.4; }.work-items i, .check-list i { width: 7px; height: 7px; margin-top: 2px; flex: 0 0 auto; border: 1px solid var(--workspace-faint); border-radius: 50%; }.work-items .item-active { color: var(--workspace-text); }.work-items .item-active i { border-color: #4e98ca; background: #4e98ca; }.work-items .item-done i, .check-list i { border-color: #50aa79; background: #50aa79; }.work-items .item-done span { text-decoration: line-through; opacity: .7; }
  .open-review { width: 100%; min-height: 30px; margin: 0 0 5px; padding: 0 7px; display: flex; align-items: center; justify-content: center; gap: 6px; border: 1px solid color-mix(in srgb, var(--workspace-accent) 22%, var(--workspace-line)); border-radius: 8px; color: var(--workspace-accent); background: var(--workspace-accent-soft); font-size: 8px; font-weight: 720; cursor: pointer; }.open-review:hover { border-color: color-mix(in srgb, var(--workspace-accent) 45%, var(--workspace-line)); }
  .inspector-files { display: grid; gap: 2px; }.inspector-files > button { width: 100%; min-width: 0; min-height: 27px; padding: 0 4px; display: flex; align-items: center; gap: 6px; border: 0; border-radius: 7px; color: var(--workspace-text); background: transparent; font-size: 8px; text-align: left; cursor: pointer; }.inspector-files > button:hover { background: var(--workspace-subtle); }.inspector-files span { min-width: 0; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }.inspector-files b { color: #438f67; font-size: 7px; }.inspector-files > button > i { color: #b96862; font-size: 7px; font-style: normal; }
  .empty-section { margin: 0; padding: 0 0 5px; color: var(--workspace-faint); font-size: 8px; line-height: 1.45; }
  .rate-limits { display: grid; gap: 9px; }.rate-limits > div { display: grid; gap: 5px; }.rate-limits span { display: flex; gap: 6px; }.rate-limits strong { min-width: 0; flex: 1; color: var(--workspace-text); font-size: 8px; }.rate-limits small { color: var(--workspace-muted); font-size: 7px; }.rate-limits > div > i { height: 3px; overflow: hidden; border-radius: 2px; background: var(--workspace-line); }.rate-limits b { height: 100%; display: block; border-radius: inherit; background: var(--workspace-accent); }
  .metadata-section dl { margin: 0; display: grid; gap: 8px; }.metadata-section dl > div { min-width: 0; display: grid; grid-template-columns: 55px minmax(0, 1fr); gap: 8px; }.metadata-section dt { color: var(--workspace-faint); font-size: 7px; }.metadata-section dd { margin: 0; overflow: hidden; color: var(--workspace-text); font-size: 8px; text-overflow: ellipsis; text-transform: capitalize; white-space: nowrap; }
  .inspector-empty { margin: auto; padding: 24px; display: grid; justify-items: center; gap: 10px; color: var(--workspace-faint); font-size: 9px; text-align: center; }
  @media (prefers-reduced-motion: reduce) { .inspector-section > summary::after { transition: none; } }
</style>
