<script lang="ts">
  import LumeIcon from "$lib/LumeIcon.svelte";
  import WorkflowRoleIcon from "$lib/WorkflowRoleIcon.svelte";
  import type { HubSession } from "$lib/hubProtocol";
  import type { Language } from "$lib/i18n";
  import { localize } from "$lib/i18n";
  import { presenceTone, type WorkflowPresence, type WorkflowStripAction } from "$lib/workflowPresence";

  /**
   * Where this chat stands in a running workflow, kept in view while the run lasts. It offers only the action that
   * fits the moment; everything else stays on the board.
   */
  let { presence, session, language = "en", onAction, onOpen } = $props<{
    presence: WorkflowPresence;
    session: HubSession;
    language?: Language;
    onAction?: (action: WorkflowStripAction) => Promise<unknown>;
    onOpen?: () => void;
  }>();

  let busy = $state(false);
  let failure = $state("");

  function tr(english: string, portuguese: string) { return localize(language, english, portuguese); }

  const tone = $derived(presenceTone(presence));
  const blocked = $derived(session.controlOrigin === "external" || !session.capabilities.canPrompt);
  const detail = $derived.by(() => {
    const next = presence.nextRoleLabel ?? tr("the next step", "a próxima etapa");
    const previous = presence.previousRoleLabel ?? tr("the previous step", "a etapa anterior");
    if (presence.outgoingPending) return tr(`Handoff to ${next} waiting for approval`, `Handoff para ${next} aguardando aprovação`);
    if (presence.incomingPending) return tr(`Handoff from ${previous} waiting for approval`, `Handoff de ${previous} aguardando aprovação`);
    if (presence.runStatus === "paused" && presence.isCurrent) return tr("Workflow paused", "Workflow pausado");
    if (presence.outgoingReady) return tr(`Ready to hand off to ${next}`, `Pronto para passar para ${next}`);
    if (presence.incomingReady) return tr(`${previous} handed off; this step can start`, `${previous} passou o resultado; esta etapa pode começar`);
    if (presence.state === "running") return tr("Working on this step", "Trabalhando nesta etapa");
    if (presence.state === "failed") return tr("This step failed", "Esta etapa falhou");
    if (presence.state === "completed") return tr("Step done", "Etapa concluída");
    if (presence.state === "skipped") return tr("Step skipped", "Etapa ignorada");
    return tr(`Waiting for ${previous}`, `Aguardando ${previous}`);
  });
  // Approving is offered on the step whose result goes out, the same rule as the review center.
  const actions = $derived.by((): { action: WorkflowStripAction; label: string; primary?: boolean }[] => {
    if (presence.outgoingPending) return [{ action: "approve", label: tr("Approve handoff", "Aprovar handoff"), primary: true }];
    if (presence.outgoingReady) return [{ action: "advance", label: tr("Run next", "Executar a próxima"), primary: true }];
    if (presence.incomingReady) return [{ action: "advance", label: tr("Start this step", "Iniciar esta etapa"), primary: true }];
    if (presence.isCurrent && presence.runStatus === "failed") return [
      { action: "retry", label: tr("Retry", "Tentar de novo"), primary: true },
      { action: "skip", label: tr("Skip", "Ignorar") },
    ];
    return [];
  });

  async function perform(action: WorkflowStripAction) {
    if (busy || !onAction) return;
    busy = true;
    failure = "";
    try { await onAction(action); }
    catch (error) { failure = String(error).replace(/^Error:\s*/, ""); }
    finally { busy = false; }
  }
</script>

<div class="workflow-strip tone-{tone}" role="status" aria-label={tr("Workflow step", "Etapa do workflow")} data-workflow-strip={presence.workflowId}>
  <span class="strip-role" aria-hidden="true"><WorkflowRoleIcon role={presence.role} /></span>
  <span class="strip-copy">
    <strong title={presence.objective || undefined}>{tr(`Step ${presence.index}/${presence.total}`, `Etapa ${presence.index}/${presence.total}`)} · {presence.roleLabel} · {presence.name}</strong>
    <small><i aria-hidden="true"></i>{detail}</small>
    {#if blocked && (presence.isCurrent || presence.incomingPending || presence.incomingReady)}
      <small class="strip-warning"><LumeIcon name="warning" size={12} />{session.controlOrigin === "external" ? tr("Take control in the chat before running this workflow.", "Assuma o controle no chat antes de executar este workflow.") : tr("Connect this session before running the workflow.", "Conecte esta sessão antes de executar o workflow.")}</small>
    {/if}
    {#if failure}<small class="strip-error" role="alert">{failure}</small>{/if}
  </span>
  <span class="strip-actions">
    {#each actions as item (item.action)}
      <button class:primary={item.primary} type="button" disabled={busy} data-workflow-action={item.action} onclick={() => void perform(item.action)}>{item.label}</button>
    {/each}
    {#if onOpen}
      <button class="strip-open" type="button" title={tr("Open in board", "Abrir no board")} aria-label={tr("Open in board", "Abrir no board")} onclick={() => onOpen?.()}><LumeIcon name="external" size={13} /></button>
    {/if}
  </span>
</div>

<style>
  .workflow-strip { position: relative; z-index: 1; min-width: 0; margin: 6px 10px 0; padding: 7px 8px 7px 9px; display: flex; align-items: center; gap: 9px; flex: 0 0 auto; border: 1px solid color-mix(in srgb, var(--strip-tone) 30%, var(--workspace-line)); border-radius: 12px; color: var(--workspace-text); background: color-mix(in srgb, var(--strip-tone) 8%, var(--workspace-pane)); --strip-tone: var(--workspace-accent); }
  .workflow-strip.tone-running { --strip-tone: #4d99cc; }
  .workflow-strip.tone-approval, .workflow-strip.tone-paused { --strip-tone: #d6a441; }
  .workflow-strip.tone-failed { --strip-tone: #c86662; }
  .workflow-strip.tone-done { --strip-tone: #4daa77; }
  .workflow-strip.tone-idle { --strip-tone: #8a9891; }
  .strip-role { width: 26px; height: 26px; display: grid; place-items: center; flex: 0 0 auto; border-radius: 8px; color: var(--strip-tone); background: color-mix(in srgb, var(--strip-tone) 14%, transparent); }
  .strip-copy { min-width: 0; display: grid; gap: 1px; flex: 1; }
  .strip-copy strong, .strip-copy small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .strip-copy strong { color: var(--workspace-strong); font-size: 11px; font-weight: 720; }
  .strip-copy small { display: flex; align-items: center; gap: 5px; color: var(--workspace-muted); font-size: 10px; font-weight: 600; }
  .strip-copy small i { width: 6px; height: 6px; flex: 0 0 auto; border-radius: 50%; background: var(--strip-tone); }
  .workflow-strip.tone-running .strip-copy small i { animation: strip-pulse 1.8s ease-out infinite; }
  .strip-copy .strip-warning { color: #b9852f; }
  .strip-copy .strip-error { color: #c86662; white-space: normal; }
  .strip-actions { display: flex; align-items: center; gap: 5px; flex: 0 0 auto; }
  .strip-actions button { min-height: 26px; padding: 0 10px; display: inline-flex; align-items: center; gap: 5px; border: 1px solid var(--workspace-line); border-radius: 8px; color: var(--workspace-text); background: var(--workspace-raised); font: inherit; font-size: 11px; font-weight: 680; cursor: pointer; }
  .strip-actions button:hover:not(:disabled) { border-color: color-mix(in srgb, var(--strip-tone) 45%, var(--workspace-line)); }
  .strip-actions button.primary { border-color: transparent; color: var(--workspace-raised); background: var(--workspace-accent); }
  .strip-actions button:disabled { opacity: .55; cursor: default; }
  .strip-actions .strip-open { width: 26px; padding: 0; justify-content: center; color: var(--workspace-muted); }
  @keyframes strip-pulse { 0% { box-shadow: 0 0 0 0 color-mix(in srgb, var(--strip-tone) 55%, transparent); } 100% { box-shadow: 0 0 0 6px transparent; } }
  @media (prefers-reduced-motion: reduce) { .workflow-strip.tone-running .strip-copy small i { animation: none; } }
  @container (max-width: 420px) { .strip-role { display: none; } .strip-actions button:not(.strip-open) { padding: 0 7px; } }
</style>
