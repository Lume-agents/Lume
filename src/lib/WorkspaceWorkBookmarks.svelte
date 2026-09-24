<script lang="ts">
  import { onDestroy } from "svelte";
  import { flip } from "svelte/animate";
  import { cubicOut } from "svelte/easing";
  import type { HubSession, WorkItem } from "$lib/hubProtocol";
  import type { Language } from "$lib/i18n";
  import GoalTargetIcon from "$lib/GoalTargetIcon.svelte";
  import WorkspaceBookmarkIcon from "$lib/WorkspaceBookmarkIcon.svelte";

  type WorkKind = "goal" | "plan" | "todo";
  type TodoEntry = { item: WorkItem; key: string };

  let { session, language = "en", hasSubagents = false } = $props<{
    session: HubSession;
    language?: Language;
    hasSubagents?: boolean;
  }>();

  let open = $state(false);
  let selected = $state<WorkKind>("todo");
  let pullProgress = $state(0);
  let pointerId = $state<number | null>(null);
  let pointerKind: WorkKind | null = null;
  let pointerStartX = 0;
  let pointerMoved = false;
  let suppressClick = false;
  let openedTodoSignature = "";
  let dismissedTodoSignature = "";
  let activeSessionId = "";
  let parkedTodoKeys = $state<string[]>([]);
  let settlingTodoKeys = $state<string[]>([]);
  let announcement = $state("");
  let containerWidth = $state(357);
  let observedTodoUpdatedAt = 0;
  const todoTimers = new Map<string, number>();

  const savedPlan = $derived(session.workSummary.plan?.content ? session.workSummary.plan : null);
  const todo = $derived(session.workSummary.todo ?? (!session.workSummary.plan?.content ? session.workSummary.plan : null));
  const goal = $derived(session.workSummary.goal ?? null);
  const todoItems: WorkItem[] = $derived(todo?.items ?? []);
  const todoEntries = $derived(todoItems.map((item, index, all): TodoEntry => ({
    item,
    key: `${item.label}:${all.slice(0, index).filter((candidate) => candidate.label === item.label).length}`,
  })));
  const orderedTodoEntries = $derived([
    ...todoEntries.filter((entry) => !parkedTodoKeys.includes(entry.key)),
    ...parkedTodoKeys
      .map((key) => todoEntries.find((entry) => entry.key === key))
      .filter((entry): entry is TodoEntry => Boolean(entry)),
  ]);
  const completedTodos = $derived(todoItems.filter((item: WorkItem) => item.status === "completed").length);
  const goalProgress = $derived(goal?.status === "complete"
    ? 100
    : todoItems.length
      ? Math.round((completedTodos / todoItems.length) * 100)
      : 0);
  const activeTodoSignature = $derived.by(() => {
    const active = todoItems.filter((item: WorkItem) => item.status === "in_progress");
    const visible = active.length
      ? active
      : session.status === "running"
        ? todoItems.filter((item: WorkItem) => item.status !== "completed").slice(0, 1)
        : [];
    return visible.map((item: WorkItem) => item.label).join("\n");
  });
  const available = $derived([
    goal ? "goal" as const : null,
    savedPlan ? "plan" as const : null,
    todoItems.length ? "todo" as const : null,
  ].filter((kind): kind is WorkKind => kind !== null));
  const drawerProgress = $derived(open ? 1 : pullProgress);
  const panelWidth = $derived(Math.max(0, Math.min(318, containerWidth - 39)));
  const panelReveal = $derived(drawerProgress * panelWidth);
  const selectedIndex = $derived(Math.max(0, available.indexOf(selected)));
  const panelTop = $derived(selectedIndex * 45);

  function sameKeys(left: string[], right: string[]) {
    return left.length === right.length && left.every((key, index) => key === right[index]);
  }

  $effect(() => {
    if (activeSessionId === session.id) return;
    for (const timer of todoTimers.values()) window.clearTimeout(timer);
    todoTimers.clear();
    activeSessionId = session.id;
    open = false;
    selected = available[0] ?? "todo";
    openedTodoSignature = "";
    dismissedTodoSignature = "";
    parkedTodoKeys = todoEntries.filter((entry) => entry.item.status === "completed").map((entry) => entry.key);
    settlingTodoKeys = [];
    observedTodoUpdatedAt = todo?.updatedAt ?? 0;
  });

  $effect(() => {
    const entries = todoEntries;
    const validKeys = new Set(entries.map((entry) => entry.key));
    const validParkedKeys = parkedTodoKeys.filter((key) => validKeys.has(key));
    const validSettlingKeys = settlingTodoKeys.filter((key) => validKeys.has(key));
    if (!sameKeys(validParkedKeys, parkedTodoKeys)) parkedTodoKeys = validParkedKeys;
    if (!sameKeys(validSettlingKeys, settlingTodoKeys)) settlingTodoKeys = validSettlingKeys;

    for (const entry of entries) {
      const done = entry.item.status === "completed";
      const parked = parkedTodoKeys.includes(entry.key);
      const pending = todoTimers.has(entry.key);
      if (done && !parked && !pending) {
        selected = "todo";
        open = true;
        dismissedTodoSignature = "";
        settlingTodoKeys = [...settlingTodoKeys, entry.key];
        announcement = tr(`${entry.item.label} completed`, `${entry.item.label} concluída`);
        const timer = window.setTimeout(() => {
          todoTimers.delete(entry.key);
          if (!parkedTodoKeys.includes(entry.key)) parkedTodoKeys = [...parkedTodoKeys, entry.key];
          settlingTodoKeys = settlingTodoKeys.filter((key) => key !== entry.key);
        }, 980);
        todoTimers.set(entry.key, timer);
      } else if (!done && parked && !pending) {
        announcement = tr(`${entry.item.label} reopened`, `${entry.item.label} reaberta`);
        const timer = window.setTimeout(() => {
          todoTimers.delete(entry.key);
          parkedTodoKeys = parkedTodoKeys.filter((key) => key !== entry.key);
        }, 520);
        todoTimers.set(entry.key, timer);
      }
    }
  });

  $effect(() => {
    const updatedAt = todo?.updatedAt ?? 0;
    if (!updatedAt || updatedAt <= observedTodoUpdatedAt) return;
    observedTodoUpdatedAt = updatedAt;
    selected = "todo";
    open = true;
    dismissedTodoSignature = "";
  });

  $effect(() => {
    const signature = activeTodoSignature;
    if (!signature || signature === openedTodoSignature || signature === dismissedTodoSignature) return;
    selected = "todo";
    open = true;
    openedTodoSignature = signature;
  });

  $effect(() => {
    if (!available.length) {
      open = false;
      return;
    }
    if (!available.includes(selected)) selected = available[0];
  });

  onDestroy(() => {
    for (const timer of todoTimers.values()) window.clearTimeout(timer);
  });

  function tr(english: string, portuguese: string) {
    return language === "pt-BR" ? portuguese : english;
  }

  function kindLabel(kind: WorkKind) {
    if (kind === "goal") return "GOAL";
    if (kind === "plan") return "PLAN";
    return "TODO";
  }

  function toggle(kind: WorkKind) {
    if (suppressClick) return;
    if (open && selected === kind) {
      closeDrawer();
      return;
    }
    selected = kind;
    open = true;
  }

  function closeDrawer() {
    if (selected === "todo" && activeTodoSignature) dismissedTodoSignature = activeTodoSignature;
    open = false;
    pullProgress = 0;
  }

  function startPull(event: PointerEvent, kind: WorkKind) {
    if (event.button !== 0) return;
    pointerKind = kind;
    pointerId = event.pointerId;
    pointerStartX = event.clientX;
    pointerMoved = false;
    event.currentTarget instanceof HTMLElement && event.currentTarget.setPointerCapture(event.pointerId);
  }

  function movePull(event: PointerEvent) {
    if (pointerId !== event.pointerId || open) return;
    const distance = Math.max(0, pointerStartX - event.clientX);
    pointerMoved ||= distance > 4;
    if (pointerMoved && pointerKind) selected = pointerKind;
    pullProgress = Math.min(1, distance / Math.max(panelWidth, 1));
  }

  function finishPull(event: PointerEvent) {
    if (pointerId !== event.pointerId) return;
    pointerId = null;
    if (!pointerMoved) {
      pointerKind = null;
      pullProgress = 0;
      return;
    }
    suppressClick = true;
    open = pullProgress >= .22;
    pullProgress = 0;
    pointerKind = null;
    window.setTimeout(() => (suppressClick = false), 0);
  }
</script>

{#if available.length}
  <section
    bind:clientWidth={containerWidth}
    class:open
    class="work-bookmarks"
    style={`--work-bookmark-top:${hasSubagents ? 154 : 74}px;--panel-top:${panelTop}px;--panel-width:${panelWidth}px;--panel-reveal:${panelReveal}px;--panel-opacity:${drawerProgress}`}
    aria-label={tr("Agent work", "Trabalho do agente")}
  >
    <nav class="bookmark-rail" aria-label={tr("Work bookmarks", "Marcadores de trabalho")}>
      {#each available as kind (kind)}
        <button
          class:active={open && selected === kind}
          class="bookmark-trigger kind-{kind}"
          style={`--trigger-reveal:${available.indexOf(kind) >= selectedIndex ? panelReveal : 0}px`}
          type="button"
          aria-expanded={open && selected === kind}
          aria-controls={`work-bookmark-panel-${session.id}`}
          aria-label={kindLabel(kind)}
          title={kindLabel(kind)}
          onclick={() => toggle(kind)}
          onpointerdown={(event) => startPull(event, kind)}
          onpointermove={movePull}
          onpointerup={finishPull}
          onpointercancel={finishPull}
        ><WorkspaceBookmarkIcon name={kind} /></button>
      {/each}
    </nav>

    <aside id={`work-bookmark-panel-${session.id}`} class="bookmark-panel" aria-hidden={!open} inert={!open}>
      <div class="panel-content">
        {#if selected === "todo"}
          <ul class="task-list" aria-label={tr("Current tasks", "Tarefas atuais")}>
            {#each orderedTodoEntries as entry (entry.key)}
              <li class:done={entry.item.status === "completed"} class:doing={entry.item.status === "in_progress"} class:settling={settlingTodoKeys.includes(entry.key)} animate:flip={{ duration: 260, easing: cubicOut }}>
                <svg class="task-check" viewBox="0 0 24 24" aria-hidden="true">
                  <circle class="check-ring" cx="12" cy="12" r="11" />
                  <circle class="check-fill" cx="12" cy="12" r="12" />
                  <path class="check-tick" d="M7.4 12.4 10.6 15.5 16.6 8.9" pathLength="1" />
                </svg>
                <span><span class="task-label">{entry.item.label}</span></span>
              </li>
            {/each}
            <li class="sr-only" role="status" aria-live="polite">{announcement}</li>
          </ul>
        {:else if selected === "plan" && savedPlan}
          <ol class="plan-list">
            {#each savedPlan.items as item, index}
              <li class:done={item.status === "completed"}><em>{String(index + 1).padStart(2, "0")}</em><span>{item.label}</span></li>
            {/each}
          </ol>
        {:else if selected === "goal" && goal}
          <div class="goal-detail status-{goal.status}">
            <div class="goal-copy">
              <span class="goal-target" aria-hidden="true"><GoalTargetIcon size={40} /></span>
              <p>{goal.objective}</p>
            </div>
            <div class="goal-progress" aria-label={tr(`Goal ${goalProgress}% complete`, `Objetivo ${goalProgress}% concluído`)}>
              <span><i style={`transform:scaleX(${goalProgress / 100})`}></i></span>
              <strong>{goalProgress}%</strong>
            </div>
          </div>
        {/if}
      </div>
    </aside>
  </section>
{/if}

<style>
  .work-bookmarks { position: absolute; z-index: 6; top: var(--work-bookmark-top); right: 0; bottom: 82px; width: min(357px, calc(100% - 10px)); pointer-events: none; }
  .bookmark-rail { position: absolute; z-index: 4; top: 0; right: 0; display: grid; gap: 6px; pointer-events: auto; }
  .bookmark-trigger { width: 39px; height: 39px; padding: 0; display: grid; place-items: center; border: 1px solid var(--workspace-line); border-right: 0; border-radius: 10px 0 0 10px; color: var(--workspace-muted); background: var(--workspace-raised); box-shadow: -5px 6px 16px rgba(8, 22, 16, .08); cursor: grab; touch-action: none; transform: translateX(calc(0px - var(--trigger-reveal))); transition: transform 240ms cubic-bezier(.22, 1, .36, 1), color 100ms ease, border-color 100ms ease, background 100ms ease, box-shadow 140ms ease; will-change: transform; }
  .bookmark-trigger:hover, .bookmark-trigger:focus-visible { color: var(--bookmark-tone); border-color: color-mix(in srgb, var(--bookmark-tone) 46%, var(--workspace-line)); background: color-mix(in srgb, var(--bookmark-tone) 8%, var(--workspace-raised)); box-shadow: -7px 8px 20px rgba(8, 22, 16, .11); }
  .bookmark-trigger:focus-visible { outline: 2px solid var(--workspace-accent); outline-offset: 2px; }.bookmark-trigger:active { cursor: grabbing; }
  .kind-goal { --bookmark-tone: #c28a48; }.kind-plan { --bookmark-tone: #718fc8; }.kind-todo { --bookmark-tone: var(--workspace-accent); }
  .bookmark-trigger { color: var(--bookmark-tone); }

  .bookmark-panel { position: absolute; z-index: 3; top: 0; right: 0; width: var(--panel-width); max-height: min(470px, calc(100% - var(--panel-top))); display: flex; flex-direction: column; overflow: hidden; border-radius: 0 0 15px 15px; color: var(--workspace-text); background: var(--workspace-raised); box-shadow: -18px 18px 52px rgba(4, 18, 12, .2); opacity: var(--panel-opacity); pointer-events: none; clip-path: inset(0 0 0 calc(100% - var(--panel-reveal)) round 0 0 15px 15px); transform: translateY(var(--panel-top)); transition: transform 160ms cubic-bezier(.22, 1, .36, 1), clip-path 280ms cubic-bezier(.22, 1, .36, 1), opacity 140ms ease; will-change: transform, clip-path; }
  .work-bookmarks.open .bookmark-panel { pointer-events: auto; }
  .panel-content { min-height: 0; padding: 9px; overflow-y: auto; overscroll-behavior: contain; scrollbar-width: thin; scrollbar-color: var(--workspace-scroll-thumb) transparent; }

  .task-list { margin: 0; padding: 0; display: flex; flex-direction: column; align-items: flex-start; gap: 6px; list-style: none; }
  .task-list li:not(.sr-only) { --task-accent: var(--workspace-accent); width: fit-content; max-width: 100%; min-height: 36px; padding: 6px 9px; display: grid; grid-template-columns: 19px minmax(0, 1fr); align-items: start; gap: 8px; border-radius: 11px; color: var(--workspace-strong); background: color-mix(in srgb, var(--workspace-raised) 76%, var(--workspace-subtle)); font-size: 10px; font-weight: 650; line-height: 19px; transition: color 300ms ease, filter 160ms ease, box-shadow 300ms ease; }
  .task-list li:not(.sr-only):hover { filter: brightness(1.035); }.task-list li.doing { color: var(--workspace-strong); }
  .task-check { width: 19px; height: 19px; overflow: visible; color: color-mix(in srgb, var(--workspace-muted) 58%, transparent); }
  .check-ring { fill: none; stroke: currentColor; stroke-width: 2; stroke-linecap: round; stroke-dasharray: 1 4.316; transition: opacity 240ms cubic-bezier(.22, 1, .36, 1); }
  .check-fill { fill: var(--task-accent); transform: scale(0); transform-box: fill-box; transform-origin: center; transition: transform 240ms cubic-bezier(.22, 1, .36, 1); }
  .check-tick { fill: none; stroke: white; stroke-width: 2.2; stroke-linecap: round; stroke-linejoin: round; stroke-dasharray: 1; stroke-dashoffset: 1; opacity: 0; transition: stroke-dashoffset 220ms cubic-bezier(.22, 1, .36, 1) 60ms, opacity 120ms ease 60ms; }
  .task-label { color: inherit; background-image: linear-gradient(currentColor, currentColor); background-repeat: no-repeat; background-position: 0 52%; background-size: 0 1.5px; box-decoration-break: clone; -webkit-box-decoration-break: clone; transition: background-size 380ms cubic-bezier(.65, 0, .35, 1), color 300ms ease; }
  .task-list li.done { color: var(--workspace-faint); }.done .check-ring { opacity: 0; }.done .check-fill { transform: scale(1); }.done .check-tick { stroke-dashoffset: 0; opacity: 1; }.done .task-label { background-size: 100% 1.5px; }
  .task-list li.settling { animation: task-nudge 300ms cubic-bezier(.22, 1, .36, 1) 680ms both; }.settling .task-check { animation: task-pop 340ms cubic-bezier(.22, 1, .36, 1) both; }
  .doing .check-ring { color: var(--workspace-accent); animation: active-ring 1.7s linear infinite; }

  .plan-list { margin: 0; padding: 2px 1px; display: grid; list-style: none; }
  .plan-list li { position: relative; min-width: 0; min-height: 48px; padding: 8px 7px; display: grid; grid-template-columns: 27px minmax(0, 1fr); align-items: start; gap: 10px; color: var(--workspace-text); font-size: 11.5px; font-weight: 590; line-height: 1.5; }
  .plan-list li:not(:last-child)::after { position: absolute; top: 35px; bottom: -5px; left: 20px; width: 1px; background: color-mix(in srgb, #718fc8 34%, var(--workspace-line)); content: ""; }
  .plan-list em { width: 27px; height: 27px; display: grid; place-items: center; border: 1px solid color-mix(in srgb, #718fc8 50%, var(--workspace-line)); border-radius: 9px; color: color-mix(in srgb, #718fc8 86%, var(--workspace-strong)); background: color-mix(in srgb, #718fc8 10%, var(--workspace-raised)); font-size: 9.5px; font-style: normal; font-weight: 800; font-variant-numeric: tabular-nums; line-height: 1; }
  .plan-list li > span { min-width: 0; padding-top: 4px; overflow-wrap: anywhere; }
  .plan-list li.done { color: var(--workspace-faint); }.plan-list li.done em { opacity: .62; }.plan-list li.done span { text-decoration: line-through; text-decoration-thickness: 1px; }
  .goal-detail { min-height: 122px; padding: 16px 13px; display: grid; align-content: center; gap: 13px; }.goal-copy { min-width: 0; display: grid; justify-items: start; gap: 7px; }.goal-detail p { margin: 0; color: var(--workspace-strong); font-size: 12.5px; font-weight: 650; line-height: 1.48; overflow-wrap: anywhere; }.goal-target { width: 40px; height: 40px; display: grid; place-items: center; color: #c28a48; }.goal-detail.status-complete .goal-target { color: var(--workspace-accent); }.goal-detail.status-blocked .goal-target { color: #c66762; }
  .goal-progress { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: center; gap: 9px; }.goal-progress > span { height: 5px; overflow: hidden; border-radius: 999px; background: color-mix(in srgb, var(--workspace-line) 72%, transparent); }.goal-progress > span i { width: 100%; height: 100%; display: block; border-radius: inherit; background: var(--workspace-accent); transform-origin: left center; transition: transform 420ms cubic-bezier(.22, 1, .36, 1); }.goal-progress strong { min-width: 29px; color: var(--workspace-muted); font-size: 8px; font-variant-numeric: tabular-nums; text-align: right; }
  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; overflow: hidden; clip: rect(0, 0, 0, 0); white-space: nowrap; border: 0; }

  @keyframes task-pop { 0%, 100% { transform: scale(1); } 40% { transform: scale(1.08); } }
  @keyframes task-nudge { 0% { transform: translateX(0); } 35% { transform: translateX(8px); } 70% { transform: translateX(-2px); } 100% { transform: translateX(0); } }
  @keyframes active-ring { to { transform: rotate(360deg); transform-origin: center; } }
  @media (max-width: 420px) { .work-bookmarks { width: calc(100% - 10px); } }
  @media (prefers-reduced-motion: reduce) { .bookmark-panel { transition: opacity 100ms ease; }.bookmark-trigger { transition-duration: 80ms; }.task-list li.settling, .settling .task-check, .doing .check-ring { animation: none; }.task-list li:not(.sr-only), .check-ring, .check-fill, .check-tick, .task-label, .goal-progress > span i { transition-duration: 80ms; } }
</style>
