<script lang="ts">
  import { slide } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import type { WorkflowGroupDefinition, WorkflowHistoryRecord, WorkflowRole, WorkflowRun, WorkflowStepDefinition } from "$lib/domain";
  import { analyzeChain } from "$lib/workflowBoard";
  import WorkflowTimeline from "$lib/WorkflowTimeline.svelte";
  import WorkflowRoleIcon from "$lib/WorkflowRoleIcon.svelte";
  import LumeIcon from "$lib/LumeIcon.svelte";
  import LumeMascot from "$lib/LumeMascot.svelte";
  import WorkspaceHeaderIcon from "$lib/WorkspaceHeaderIcon.svelte";
  import { sessionName, type MobileClient } from "./client.svelte";

  let { client, onOpenChat, onOpenBoard }: { client: MobileClient; onOpenChat: (id: string) => void; onOpenBoard: () => void } = $props();

  let open = $state<string[]>([]);

  // The newest run of each workflow, with the group it ran.
  const workflows = $derived.by(() => {
    const latest = new Map<string, WorkflowHistoryRecord>();
    for (const record of client.snapshot?.workflowHistory ?? []) {
      const current = latest.get(record.run.workflowId);
      if (!current || record.run.updatedAt > current.run.updatedAt) latest.set(record.run.workflowId, record);
    }
    const groups: WorkflowGroupDefinition[] = client.snapshot?.workflowGroups ?? [];
    const items = groups.filter((group) => group.steps.length).map((group) => ({ group: latest.get(group.id)?.group ?? group, run: latest.get(group.id)?.run ?? null }));
    for (const [id, record] of latest) if (!groups.some((group) => group.id === id)) items.push({ group: record.group, run: record.run });
    return items.sort((left, right) => (right.run?.updatedAt ?? 0) - (left.run?.updatedAt ?? 0));
  });

  function roleName(role: WorkflowRole) {
    return ({ planner: client.tr("Planner", "Planejador"), implementer: client.tr("Implementer", "Implementador"), reviewer: client.tr("Reviewer", "Revisor"), tester: client.tr("Tester", "Testador"), researcher: client.tr("Researcher", "Pesquisador"), custom: client.tr("Custom", "Personalizado") } as Record<WorkflowRole, string>)[role];
  }

  function runStatus(status: WorkflowRun["status"]) {
    return ({ draft: client.tr("Draft", "Rascunho"), ready: client.tr("Ready for next step", "Pronto para a próxima etapa"), running: client.tr("Running", "Executando"), waiting_for_approval: client.tr("Waiting for approval", "Aguardando aprovação"), paused: client.tr("Paused", "Pausado"), completed: client.tr("Completed", "Concluído"), failed: client.tr("Failed", "Falhou"), cancelled: client.tr("Cancelled", "Cancelado") } as Record<string, string>)[status] ?? status;
  }

  function sessionFor(step: WorkflowStepDefinition) {
    return client.sessions.find((session) => session.nativeSessionId === step.sessionNativeId) ?? null;
  }

  function stepTitle(step: WorkflowStepDefinition) {
    return step.role === "custom" ? step.customRoleLabel || roleName(step.role) : roleName(step.role);
  }

  function toggle(id: string) {
    open = open.includes(id) ? open.filter((value) => value !== id) : [...open, id];
  }

  function elapsed(run: WorkflowRun) {
    const seconds = Math.max(0, Math.round((run.updatedAt - run.createdAt) / 1000));
    if (seconds < 60) return `${seconds}s`;
    const minutes = Math.round(seconds / 60);
    return minutes < 60 ? `${minutes} min` : `${Math.floor(minutes / 60)}h ${minutes % 60}min`;
  }
</script>

<section class="screen">
  <header class="top">
    <h1>{client.tr("Workflows", "Workflows")}</h1>
    <button class="open-board" type="button" onclick={onOpenBoard}><WorkspaceHeaderIcon name="workflow" size={18} />{client.tr("Open board", "Abrir board")}</button>
  </header>
  <div class="scroll">
    {#each workflows as { group, run } (group.id)}
      {@const order = analyzeChain(group).order}
      {@const steps = order.map((id) => group.steps.find((step) => step.id === id)).filter((step): step is WorkflowStepDefinition => Boolean(step))}
      {@const done = run ? run.steps.filter((step) => step.status === "completed").length : 0}
      <article class="workflow">
        <button class="summary" type="button" aria-expanded={open.includes(group.id)} onclick={() => toggle(group.id)}>
          <span class="chain">
            {#each steps as step, index (step.id)}
              {@const state = run?.steps.find((item) => item.stepId === step.id)?.status}
              {#if index}<i class="link" class:done={state && state !== "pending"}></i>{/if}
              <span class="fab role-{step.role} state-{state ?? 'pending'}" title={stepTitle(step)}><WorkflowRoleIcon role={step.role} /></span>
            {/each}
          </span>
          <span class="copy">
            <strong>{run?.objective || client.tr(`${steps.length} steps`, `${steps.length} etapas`)}</strong>
            <small>
              {#if run}<span class="status status-{run.status}"><i></i>{runStatus(run.status)}</span> · {done}/{steps.length} · {elapsed(run)}{:else}{client.tr("Not run yet", "Ainda não executado")}{/if}
            </small>
          </span>
          <LumeIcon name="chevron-down" size={16} />
        </button>

        {#if open.includes(group.id)}
          <div class="details" transition:slide={{ duration: 180, easing: cubicOut }}>
            {#if run && client.scopes.includes("prompt")}
              <div class="actions">
                {#if run.status === "waiting_for_approval"}<button class="primary" type="button" onclick={() => void client.workflow("approve_workflow_handoff", group.id)}>{client.tr("Approve handoff", "Aprovar passagem")}</button>{/if}
                {#if run.status === "ready"}<button class="primary" type="button" onclick={() => void client.workflow("advance_workflow", group.id)}>{client.tr("Next step", "Próxima etapa")}</button>{/if}
                {#if run.status === "running"}<button type="button" onclick={() => void client.workflow("pause_workflow", group.id)}>{client.tr("Pause", "Pausar")}</button>{/if}
                {#if run.status === "paused"}<button class="primary" type="button" onclick={() => void client.workflow("resume_workflow", group.id)}>{client.tr("Resume", "Retomar")}</button>{/if}
                {#if run.status === "failed"}
                  <button class="primary" type="button" onclick={() => void client.workflow("retry_workflow_step", group.id)}>{client.tr("Retry step", "Tentar de novo")}</button>
                  <button type="button" onclick={() => void client.workflow("skip_workflow_step", group.id)}>{client.tr("Skip step", "Pular etapa")}</button>
                {/if}
                {#if ["running", "paused", "waiting_for_approval", "ready"].includes(run.status)}<button class="danger" type="button" onclick={() => void client.workflow("cancel_workflow", group.id)}>{client.tr("Cancel", "Cancelar")}</button>{/if}
              </div>
            {/if}
            {#if run?.error}<p class="error">{run.error}</p>{/if}
            <div class="steps">
              {#each steps as step, index (step.id)}
                {@const session = sessionFor(step)}
                <button class="step" type="button" disabled={!session} onclick={() => session && onOpenChat(session.id)}>
                  <span class="fab small role-{step.role}"><WorkflowRoleIcon role={step.role} /></span>
                  <span class="step-copy"><strong>{index + 1}. {stepTitle(step)}</strong><small>{session ? `${sessionName(session)} · ${session.agentLabel}` : client.tr("Chat not open", "Chat não aberto")}</small></span>
                  {#if session}<LumeIcon name="arrow-right" size={14} />{/if}
                </button>
              {/each}
            </div>
            {#if run}
              <div class="timeline">
                <WorkflowTimeline
                  {run}
                  steps={group.steps}
                  {order}
                  connections={group.connections}
                  language={client.language}
                  activitiesFor={(step) => sessionFor(step)?.activities ?? []}
                  labelFor={(step) => {
                    const session = sessionFor(step);
                    return { title: stepTitle(step), subtitle: session ? `${sessionName(session)} · ${session.agentLabel}` : "", agent: session?.agentLabel };
                  }}
                  onOpenStep={(step) => { const session = sessionFor(step); if (session) onOpenChat(session.id); }}
                />
              </div>
            {/if}
          </div>
        {/if}
      </article>
    {:else}
      <div class="empty">
        <LumeMascot status="idle" size={56} />
        <strong>{client.tr("No workflows yet", "Nenhum workflow ainda")}</strong>
        <p>{client.tr("Build one on the board in Lume's Workspace: chain agents into steps and follow the run here.", "Monte um no board do Workspace do Lume: encadeie agentes em etapas e acompanhe a execução aqui.")}</p>
      </div>
    {/each}
  </div>
</section>

<style>
  .screen { --role-planner: #6aa0d6; --role-implementer: #56bd8a; --role-reviewer: #dba653; --role-tester: #a583d8; --role-researcher: #58b6c5; --role-custom: #93a39b; min-height: 0; display: grid; grid-template-columns: minmax(0, 1fr); grid-template-rows: auto minmax(0, 1fr); }
  .top { padding: calc(var(--safe-top) + 14px) 18px 8px; display: flex; align-items: center; justify-content: space-between; gap: 10px; }
  .open-board { height: 38px; padding: 0 14px; display: inline-flex; align-items: center; gap: 7px; border: 0; border-radius: 12px; color: #fff; background: var(--workspace-accent); font-size: 13.5px; font-weight: 700; }
  h1 { margin: 0; color: var(--workspace-strong); font-size: 24px; font-weight: 760; letter-spacing: -.02em; }
  .scroll { min-height: 0; padding: 4px 12px 24px; display: grid; align-content: start; gap: 10px; overflow-y: auto; }
  .workflow { border: 1px solid var(--workspace-line); border-radius: 18px; background: var(--workspace-raised); }
  .summary { width: 100%; padding: 14px; display: grid; grid-template-columns: minmax(0, 1fr) auto; grid-template-rows: auto auto; gap: 10px 8px; border: 0; color: inherit; background: transparent; text-align: left; }
  .summary > :global(svg) { grid-row: 1 / 3; grid-column: 2; align-self: center; color: var(--workspace-faint); }
  .chain { display: flex; align-items: center; }
  .link { width: 16px; height: 2px; border-radius: 2px; background: var(--workspace-line); }
  .link.done { background: var(--workspace-accent); }
  .fab { --role: var(--role-custom); width: 34px; height: 34px; display: grid; place-items: center; flex: 0 0 auto; border: 1px solid color-mix(in srgb, var(--role) 42%, transparent); border-radius: 50%; color: var(--role); background: radial-gradient(circle at 34% 24%, rgba(255, 255, 255, .22) 0 13%, transparent 46%), linear-gradient(145deg, color-mix(in srgb, var(--role) 20%, var(--workspace-raised)), var(--workspace-bg) 88%); box-shadow: 0 0 0 3px color-mix(in srgb, var(--role) 12%, transparent); }
  .fab.small { width: 30px; height: 30px; }
  .fab :global(.workflow-role-icon) { width: 17px; height: 17px; }
  .fab.state-running { box-shadow: 0 0 0 4px color-mix(in srgb, var(--role) 30%, transparent); }
  .fab.state-pending { opacity: .55; }
  .role-planner { --role: var(--role-planner); }
  .role-implementer { --role: var(--role-implementer); }
  .role-reviewer { --role: var(--role-reviewer); }
  .role-tester { --role: var(--role-tester); }
  .role-researcher { --role: var(--role-researcher); }
  .copy { display: grid; gap: 4px; }
  .copy strong { color: var(--workspace-strong); font-size: 15px; font-weight: 700; line-height: 1.35; }
  .copy small { display: flex; flex-wrap: wrap; align-items: center; gap: 4px; color: var(--workspace-muted); font-size: 12.5px; }
  .status { display: inline-flex; align-items: center; gap: 5px; }
  .status i { width: 6px; height: 6px; border-radius: 50%; background: currentColor; }
  .status-running { color: #6ea6d6; }
  .status-waiting_for_approval, .status-paused, .status-ready { color: #d39a4c; }
  .status-completed { color: #57b380; }
  .status-failed, .status-cancelled { color: #d06e66; }
  .details { padding: 0 14px 14px; display: grid; gap: 12px; }
  .actions { display: flex; flex-wrap: wrap; gap: 6px; }
  .actions button { height: 40px; padding: 0 14px; flex: 1 1 auto; border: 1px solid var(--workspace-line); border-radius: 12px; color: var(--workspace-strong); background: transparent; font-size: 13.5px; font-weight: 700; }
  .actions .primary { border-color: transparent; color: #fff; background: var(--workspace-accent); }
  .actions .danger { color: #d76e66; }
  .error { margin: 0; padding: 10px 12px; border-radius: 12px; color: #d76e66; background: color-mix(in srgb, #d0645c 10%, transparent); font-size: 13px; }
  .steps { display: grid; gap: 4px; }
  .step { min-height: 50px; padding: 6px 8px; display: flex; align-items: center; gap: 10px; border: 0; border-radius: 12px; color: inherit; background: var(--workspace-bg); text-align: left; }
  .step:disabled { opacity: .7; }
  .step-copy { min-width: 0; flex: 1; display: grid; gap: 2px; }
  .step-copy strong { color: var(--workspace-strong); font-size: 14px; }
  .step-copy small { overflow: hidden; color: var(--workspace-muted); font-size: 12px; text-overflow: ellipsis; white-space: nowrap; }
  .timeline { overflow-x: auto; }
  .empty { margin-top: 40px; display: grid; justify-items: center; gap: 8px; color: var(--workspace-muted); text-align: center; }
  .empty strong { color: var(--workspace-strong); font-size: 16px; }
  .empty p { margin: 0; max-width: 300px; font-size: 14px; line-height: 1.5; }
</style>
