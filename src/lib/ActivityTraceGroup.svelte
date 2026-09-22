<script lang="ts">
  import type { SessionActivity } from "$lib/domain";
  import type { Language } from "$lib/i18n";
  import { slide } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import ActivityTypeIcon from "$lib/ActivityTypeIcon.svelte";
  import FileTypeIcon from "$lib/FileTypeIcon.svelte";
  import { displayFileChangePath } from "$lib/fileChanges";
  import { activityCategory, activityDisplayTitle, activityGroupSummary, activityPreview, activityRunFiles, activityRunTitle, groupConsecutiveTraceActivities, type ActivityRun } from "$lib/activityPresentation";

  let { activities, language = "en", active = false, plain = false } = $props<{
    activities: SessionActivity[];
    language?: Language;
    active?: boolean;
    plain?: boolean;
  }>();

  const summary = $derived(activityGroupSummary(activities, language));
  const isWorking = $derived(active || activities.some(
    (activity: SessionActivity) => activity.status === "running",
  ));
  let visibleActivityLimit = $state(40);
  const hiddenActivityCount = $derived(Math.max(0, activities.length - visibleActivityLimit));
  const visibleActivities = $derived(
    hiddenActivityCount > 0 ? activities.slice(-visibleActivityLimit) : activities,
  );
  const visibleRuns: ActivityRun[] = $derived(plain
    ? groupConsecutiveTraceActivities(visibleActivities)
    : visibleActivities.map((activity: SessionActivity) => ({ id: activity.id, category: activity.kind === "analysis" ? "analysis" : activityCategory(activity), activities: [activity] })));
  let expanded = $state(true);
  let expandedRuns = $state<string[]>([]);
  let collapsedFileRuns = $state<string[]>([]);
  let selectedFileKey = $state("");
  let initialized = false;
  let wasActive = false;

  $effect(() => {
    const isActive = isWorking;
    if (!initialized) {
      expanded = isActive;
      initialized = true;
    } else if (isActive) {
      expanded = true;
    } else if (wasActive) {
      expanded = false;
    }
    wasActive = isActive;
  });

  function tr(english: string, portuguese: string) {
    return language === "pt-BR" ? portuguese : english;
  }

  function toggleRun(id: string) {
    expandedRuns = expandedRuns.includes(id)
      ? expandedRuns.filter((value) => value !== id)
      : [...expandedRuns, id];
  }

  function toggleFileRun(id: string) {
    collapsedFileRuns = collapsedFileRuns.includes(id)
      ? collapsedFileRuns.filter((value) => value !== id)
      : [...collapsedFileRuns, id];
  }

  function fileDetail(run: ActivityRun, path: string): string {
    const activity = run.activities.find((item) => activityRunFiles({ ...run, activities: [item] }).includes(path));
    return activity?.detail ?? "";
  }

  function motionDuration(duration: number) {
    return typeof window !== "undefined" && window.matchMedia("(prefers-reduced-motion: reduce)").matches ? 0 : duration;
  }

  function runStatus(run: ActivityRun) {
    return run.activities.some((item) => item.status === "failed") ? "failed"
      : run.activities.some((item) => item.status === "running") ? "running"
      : run.activities.at(-1)?.status;
  }
</script>

<section class:active={isWorking} class:open={expanded} class:plain class="activity-cluster">
  <button
    class="cluster-summary"
    type="button"
    aria-expanded={expanded}
    onclick={() => (expanded = !expanded)}
  >
    <span class="cluster-mark" aria-hidden="true">
      <svg viewBox="0 0 20 20"><path d="M5 5.5h7M5 10h10M5 14.5h6" /><path d="m13.5 4 1 1 2-2" /></svg>
    </span>
    <strong>{summary}</strong>
    <small>{activities.length}</small>
    <svg class="cluster-chevron" viewBox="0 0 20 20" aria-hidden="true"><path d="m6 8 4 4 4-4" /></svg>
  </button>
  {#if expanded}
    <div class="activity-list" in:slide={{ duration: motionDuration(190), easing: cubicOut }} out:slide={{ duration: motionDuration(140), easing: cubicOut }}>
      {#if hiddenActivityCount > 0}
        <button class="load-earlier-activities" type="button" onclick={() => (visibleActivityLimit += 40)}>
          {tr(`Show ${Math.min(40, hiddenActivityCount)} earlier events`, `Mostrar ${Math.min(40, hiddenActivityCount)} eventos anteriores`)}
        </button>
      {/if}
      {#each visibleRuns as run, index (run.id)}
        {@const activity = run.activities[0]}
        {@const preview = activityPreview(activity)}
        {@const runFiles = plain ? activityRunFiles(run) : []}
        {#if plain && runFiles.length > 0}
          {@const status = runStatus(run)}
          <div
            class:failed={status === "failed"}
            class:interrupted={status === "interrupted"}
            class:running={status === "running"}
            class:waiting={status === "waiting"}
            class:open={!collapsedFileRuns.includes(run.id)}
            class="activity-row file-tree-row"
            style={`--step-delay: ${Math.min(index, 4) * 22}ms`}
          >
            <button class="event-trigger" type="button" aria-expanded={!collapsedFileRuns.includes(run.id)} onclick={() => toggleFileRun(run.id)}>
              <span class="activity-status" aria-label={status}>
                {#if status === "completed"}<svg viewBox="0 0 16 16" aria-hidden="true"><path d="m4 8 2.5 2.5L12 5" /></svg>
                {:else}<i></i>{/if}
              </span>
              <span class="activity-type"><ActivityTypeIcon category={run.category} /></span>
              <span class="activity-copy"><strong>{activityRunTitle(run, language)}</strong></span>
              <svg class="row-chevron" viewBox="0 0 20 20" aria-hidden="true"><path d="m6 8 4 4 4-4" /></svg>
            </button>
            {#if !collapsedFileRuns.includes(run.id)}
              <div class="file-tree" in:slide={{ duration: motionDuration(190), easing: cubicOut }} out:slide={{ duration: motionDuration(130), easing: cubicOut }}>
                <div class="file-tree-files" role="list">
                  {#each runFiles as path, fileIndex (path)}
                    {@const detail = fileDetail(run, path)}
                    {@const fileKey = `${run.id}:${path}`}
                    <div class="file-node" role="listitem" style={`--file-delay: ${Math.min(fileIndex, 6) * 30}ms`}>
                      <button class="file-node-trigger" type="button" title={path} disabled={!detail} aria-expanded={detail ? selectedFileKey === fileKey : undefined} onclick={() => (selectedFileKey = selectedFileKey === fileKey ? "" : fileKey)}>
                        <span class="file-node-action">{run.category === "read" ? tr("Read", "Leu") : tr("Edited", "Editou")}</span>
                        <FileTypeIcon {path} />
                        <span class="file-node-name">{displayFileChangePath(path)}</span>
                      </button>
                      {#if detail && selectedFileKey === fileKey}
                        <div class="file-node-detail" in:slide={{ duration: motionDuration(180), easing: cubicOut }} out:slide={{ duration: motionDuration(120), easing: cubicOut }}>
                          <pre>{detail}</pre>
                        </div>
                      {/if}
                    </div>
                  {/each}
                </div>
              </div>
            {/if}
          </div>
        {:else if run.activities.length > 1}
          {@const status = runStatus(run)}
          <div
            class:failed={status === "failed"}
            class:interrupted={status === "interrupted"}
            class:running={status === "running"}
            class:waiting={status === "waiting"}
            class:open={expandedRuns.includes(run.id)}
            class="activity-row grouped-row"
            style={`--step-delay: ${Math.min(index, 4) * 22}ms`}
          >
            <button class="event-trigger" type="button" aria-expanded={expandedRuns.includes(run.id)} onclick={() => toggleRun(run.id)}>
              <span class="activity-status" aria-label={status}>
                {#if status === "completed"}<svg viewBox="0 0 16 16" aria-hidden="true"><path d="m4 8 2.5 2.5L12 5" /></svg>
                {:else}<i></i>{/if}
              </span>
              <span class="activity-type"><ActivityTypeIcon category={run.category} /></span>
              <span class="activity-copy"><strong>{activityRunTitle(run, language)}</strong></span>
              <svg class="row-chevron" viewBox="0 0 20 20" aria-hidden="true"><path d="m6 8 4 4 4-4" /></svg>
            </button>
            {#if expandedRuns.includes(run.id)}
            <div class="grouped-activity-details" in:slide={{ duration: motionDuration(180), easing: cubicOut }} out:slide={{ duration: motionDuration(120), easing: cubicOut }}>
              {#each run.activities as grouped, detailIndex (grouped.id)}
                <div class="grouped-activity-detail">
                  <span aria-hidden="true">{detailIndex + 1}</span>
                  <pre>{grouped.detail || activityDisplayTitle(grouped, language)}</pre>
                </div>
              {/each}
            </div>
            {/if}
          </div>
        {:else if activity.kind === "analysis"}
          <div
            class:failed={activity.status === "failed"}
            class:interrupted={activity.status === "interrupted"}
            class:running={activity.status === "running"}
            class:waiting={activity.status === "waiting"}
            class="activity-row reasoning-row"
            style={`--step-delay: ${Math.min(index, 4) * 22}ms`}
          >
            <div class="reasoning-summary">
              <span class="activity-status" aria-label={activity.status}>
                {#if activity.status === "completed"}
                  <svg viewBox="0 0 16 16" aria-hidden="true"><path d="m4 8 2.5 2.5L12 5" /></svg>
                {:else if activity.status === "failed"}
                  <svg viewBox="0 0 16 16" aria-hidden="true"><path d="m5 5 6 6m0-6-6 6" /></svg>
                {:else}
                  <i></i>
                {/if}
              </span>
              <span class="activity-type"><ActivityTypeIcon category="analysis" /></span>
              <span class="activity-copy"><strong>{activityDisplayTitle(activity, language)}</strong></span>
            </div>
            {#if activity.detail}<p class="reasoning-text">{activity.detail}</p>{/if}
          </div>
        {:else}
          <details
            class:failed={activity.status === "failed"}
            class:interrupted={activity.status === "interrupted"}
            class:running={activity.status === "running"}
            class:waiting={activity.status === "waiting"}
            class="activity-row"
            style={`--step-delay: ${Math.min(index, 4) * 22}ms`}
          >
            <summary>
              <span class="activity-status" aria-label={activity.status}>
                {#if activity.status === "completed"}
                  <svg viewBox="0 0 16 16" aria-hidden="true"><path d="m4 8 2.5 2.5L12 5" /></svg>
                {:else if activity.status === "failed"}
                  <svg viewBox="0 0 16 16" aria-hidden="true"><path d="m5 5 6 6m0-6-6 6" /></svg>
                {:else}
                  <i></i>
                {/if}
              </span>
              <span class="activity-type"><ActivityTypeIcon category={activityCategory(activity)} /></span>
              <span class="activity-copy">
                <strong>{activityDisplayTitle(activity, language)}</strong>
                {#if !plain && preview}<code title={preview}>{preview}</code>{/if}
              </span>
              {#if activity.detail}<svg class="row-chevron" viewBox="0 0 20 20" aria-hidden="true"><path d="m6 8 4 4 4-4" /></svg>{/if}
            </summary>
            {#if activity.detail}<pre>{activity.detail}</pre>{/if}
          </details>
        {/if}
      {/each}
    </div>
  {/if}
</section>

<style>
  svg { width: 14px; height: 14px; fill: none; stroke: currentColor; stroke-width: 1.55; stroke-linecap: round; stroke-linejoin: round; }
  .activity-cluster { box-sizing: border-box; width: 100%; min-width: 0; max-width: 100%; overflow: hidden; border: 1px solid var(--workspace-line, rgba(65, 94, 80, .18)); border-radius: 10px; background: color-mix(in srgb, var(--workspace-subtle, #eef4f0) 62%, transparent); transition: border-color 80ms ease, background 80ms ease; }
  .activity-cluster.active { border-color: color-mix(in srgb, #4e91bf 24%, transparent); }
  .activity-cluster.plain, .activity-cluster.plain.active { border: 0; border-radius: 0; background: transparent; }
  .activity-cluster.plain .cluster-summary { padding-inline: 6px; border-radius: 7px; background: transparent; }
  .activity-cluster.plain .cluster-mark, .activity-cluster.plain .cluster-summary > small { background: transparent; }
  .activity-cluster.plain .activity-list { padding-left: 13px; }
  .activity-cluster.plain .activity-row pre { border: 0; border-radius: 0; background: transparent; }
  .cluster-summary { width: 100%; min-height: var(--activity-summary-height, 39px); padding: 6px 8px; display: flex; align-items: center; gap: 8px; border: 0; color: var(--workspace-muted, #65776e); background: transparent; cursor: pointer; text-align: left; }
  .activity-row > summary::-webkit-details-marker { display: none; }
  .cluster-mark { width: 24px; height: 24px; display: grid; place-items: center; flex: 0 0 auto; border-radius: 7px; color: var(--workspace-accent, #428066); background: color-mix(in srgb, var(--workspace-accent, #428066) 8%, transparent); }
  .cluster-mark svg { width: 15px; height: 15px; }
  .cluster-summary > strong { min-width: 0; flex: 1; overflow: hidden; font: 730 var(--chat-small-font-size, 9px)/1.35 Inter, sans-serif; text-overflow: ellipsis; white-space: nowrap; }
  .cluster-summary > small { min-width: 17px; height: 17px; padding: 0 4px; display: grid; place-items: center; border-radius: 9px; color: #778980; background: rgba(76, 105, 91, .07); font: 700 var(--chat-tiny-font-size, 7px) Inter, sans-serif; }
  .cluster-chevron, .row-chevron { flex: 0 0 auto; color: #89968f; transition: transform 140ms ease; }
  .activity-cluster.open .cluster-chevron, .activity-row[open] > summary .row-chevron, .grouped-row.open .row-chevron { transform: rotate(180deg); }
  .activity-list { min-height: 0; height: auto; padding: 2px 9px 8px 21px; display: flex; flex-direction: column; align-items: stretch; overflow: visible; }
  .load-earlier-activities { min-height: 27px; margin: 2px 4px 3px 0; border: 0; border-bottom: 1px solid rgba(75, 114, 94, .1); color: #778a80; background: transparent; font: 700 var(--chat-tiny-font-size, 7px) Inter, sans-serif; cursor: pointer; text-align: left; }
  .load-earlier-activities:hover { color: #3f7f61; }
  .activity-row { position: relative; min-width: 0; min-height: var(--activity-row-height, 34px); height: auto; display: block; flex: 0 0 auto; overflow: visible; border-left: 1px solid color-mix(in srgb, var(--workspace-accent, #428066) 23%, transparent); }
  .activity-row:last-child { border-left-color: transparent; }
  .activity-row > summary, .reasoning-summary, .event-trigger { box-sizing: border-box; width: 100%; min-width: 0; min-height: var(--activity-row-height, 34px); height: auto; padding: 4px 2px 4px 16px; display: flex; align-items: center; gap: 8px; color: var(--workspace-muted, #61736a); list-style: none; }
  .activity-row > summary, .event-trigger { cursor: pointer; }
  .event-trigger { border: 0; background: transparent; text-align: left; }
  .activity-type { width: 15px; height: 15px; display: grid; place-items: center; flex: 0 0 auto; color: var(--workspace-accent, #428066); }
  .activity-copy { min-width: 0; flex: 1; display: grid; gap: 2px; }
  .activity-copy strong { overflow: hidden; color: var(--workspace-strong, #52665c); font: 700 var(--activity-title-size, var(--chat-small-font-size, 9px))/1.25 Inter, sans-serif; text-overflow: ellipsis; white-space: nowrap; }
  .activity-copy code { overflow: hidden; color: var(--workspace-faint, #89958f); font: var(--activity-detail-size, var(--chat-tiny-font-size, 7px))/1.3 "SFMono-Regular", Consolas, monospace; text-overflow: ellipsis; white-space: nowrap; }
  .activity-status { position: absolute; z-index: 1; top: calc((var(--activity-row-height, 34px) - 16px) / 2); left: -8px; width: 16px; height: 16px; display: grid; place-items: center; border: 1px solid color-mix(in srgb, var(--workspace-accent, #428066) 28%, transparent); border-radius: 50%; color: #4a956b; background: var(--workspace-pane, #f8fbf9); }
  .activity-status svg { width: 11px; height: 11px; stroke-width: 1.9; }
  .activity-status i { width: 5px; height: 5px; border-radius: 50%; background: currentColor; }
  .activity-row.running .activity-status { color: #4e91bf; border-color: rgba(78, 145, 191, .3); }
  .activity-row.running .activity-status i { will-change: transform, opacity; animation: activity-pulse 1s ease-in-out infinite; }
  .activity-row.running .activity-copy strong { color: #4e7fa6; }
  .activity-row.failed .activity-status { color: #b85d59; border-color: rgba(184, 93, 89, .28); }
  .activity-row.waiting .activity-status { color: #c2943f; border-color: rgba(194, 148, 63, .28); }
  .activity-row.interrupted .activity-status { color: #839088; border-color: rgba(131, 144, 136, .25); }
  .activity-row pre { min-width: 0; max-width: calc(100% - 20px); max-height: 180px; margin: 0 0 8px 20px; padding: 7px 8px; overflow: auto; border: 1px solid var(--workspace-line, rgba(65, 94, 80, .12)); border-radius: 7px; color: var(--workspace-text, #4f6258); background: var(--workspace-pane, #eaf0ed); font: var(--chat-tiny-font-size, 7px)/1.5 "SFMono-Regular", Consolas, monospace; overflow-wrap: anywhere; white-space: pre-wrap; word-break: break-word; }
  .grouped-activity-details { min-width: 0; margin: 0 0 8px 17px; display: grid; gap: 5px; }
  .file-tree { min-width: 0; padding: 0 0 7px 17px; display: grid; gap: 1px; }
  .file-tree-files { min-width: 0; display: grid; gap: 1px; }
  .file-node { position: relative; min-width: 0; padding-left: 10px; color: var(--workspace-muted, #61736a); animation: file-node-arrive 280ms cubic-bezier(.16, 1, .3, 1) both; animation-delay: var(--file-delay); }
  .file-node::before { position: absolute; top: 0; left: -17px; width: 20px; height: 50%; border-bottom: 1px solid color-mix(in srgb, var(--workspace-accent, #428066) 23%, transparent); border-left: 1px solid color-mix(in srgb, var(--workspace-accent, #428066) 23%, transparent); border-bottom-left-radius: 6px; content: ""; }
  .file-node-trigger { min-width: 0; min-height: 26px; padding: 3px; display: flex; align-items: center; gap: 7px; border: 0; border-radius: 6px; color: inherit; background: transparent; text-align: left; cursor: pointer; }
  .file-node-trigger:hover:not(:disabled), .file-node-trigger:focus-visible { color: var(--workspace-accent, #428066); background: var(--workspace-subtle, #eef4f0); }
  .file-node-trigger:disabled { cursor: default; }
  .file-node-detail { min-width: 0; margin: 3px 0 0 8px; }
  .file-node-detail pre { max-width: 100%; margin: 0 0 5px; }
  .file-node-action { flex: 0 0 auto; color: var(--workspace-muted, #61736a); font: 600 var(--activity-detail-size, var(--chat-tiny-font-size, 7px))/1.3 Inter, sans-serif; }
  .file-node :global(.file-type-icon) { flex: 0 0 auto; }
  .file-node-name { min-width: 0; padding: 3px 6px; overflow: hidden; border-radius: 5px; color: var(--workspace-strong, #52665c); background: color-mix(in srgb, var(--workspace-subtle, #eef4f0) 75%, transparent); font: 650 var(--activity-detail-size, var(--chat-tiny-font-size, 7px))/1.3 "SFMono-Regular", Consolas, monospace; text-overflow: ellipsis; white-space: nowrap; }
  .grouped-activity-detail { min-width: 0; display: grid; grid-template-columns: 15px minmax(0, 1fr); align-items: start; gap: 5px; }
  .grouped-activity-detail > span { padding-top: 2px; color: var(--workspace-faint, #89958f); font: 650 var(--chat-tiny-font-size, 7px)/1.5 "SFMono-Regular", Consolas, monospace; font-variant-numeric: tabular-nums; }
  .grouped-activity-detail pre { max-width: 100%; max-height: 120px; margin: 0; padding: 0; border: 0; border-radius: 0; background: transparent; }
  .reasoning-row { padding-bottom: 8px; }
  .reasoning-text { max-width: 72ch; margin: -1px 8px 0 16px; padding: 0 0 0 8px; overflow-wrap: anywhere; color: var(--workspace-text, #4f6258); font: var(--activity-detail-size, var(--chat-small-font-size, 9px))/1.58 Inter, sans-serif; white-space: pre-wrap; }
  :global(.terminal-window.dark) .activity-cluster { border-color: rgba(205, 222, 213, .075); background: rgba(218, 234, 226, .018); }
  :global(.terminal-window.dark) .activity-cluster.active { border-color: rgba(114, 184, 230, .18); }
  :global(.terminal-window.dark) .cluster-summary { color: #a1b3aa; }
  :global(.terminal-window.dark) .cluster-mark { color: var(--workspace-accent, #8bc5a8); }
  :global(.terminal-window.dark) .cluster-summary > small { color: #8fa198; background: rgba(205, 222, 213, .055); }
  :global(.terminal-window.dark) .activity-row { border-color: rgba(177, 207, 191, .1); }
  :global(.terminal-window.dark) .activity-row:last-child { border-left-color: transparent; }
  :global(.terminal-window.dark) .activity-status { background: var(--workspace-pane, #141d19); }
  :global(.terminal-window.dark) .activity-copy strong { color: #bccdc4; }
  :global(.terminal-window.dark) .activity-row.running .activity-copy strong { color: #72a9cf; }
  :global(.terminal-window.dark) .activity-copy code { color: #7f9188; }
  :global(.terminal-window.dark) .activity-row pre { color: #adbbb4; background: rgba(4, 12, 8, .18); }
  :global(.terminal-window.dark) .activity-cluster.plain, :global(.terminal-window.dark) .activity-cluster.plain.active { border: 0; background: transparent; }
  :global(.terminal-window.dark) .activity-cluster.plain .cluster-summary { background: transparent; }
  :global(.terminal-window.dark) .activity-cluster.plain .cluster-mark, :global(.terminal-window.dark) .activity-cluster.plain .cluster-summary > small, :global(.terminal-window.dark) .activity-cluster.plain .activity-row pre { background: transparent; }
  :global(.terminal-window.dark) .load-earlier-activities { color: #8fa198; border-color: rgba(177, 207, 191, .08); }
  :global(.terminal-window.dark) .load-earlier-activities:hover { color: #8bc5a8; }
  @keyframes activity-pulse { 50% { opacity: .38; transform: scale(.62); } }
  @keyframes step-arrive { from { opacity: 0; transform: translateY(5px); } }
  @keyframes file-node-arrive { from { opacity: 0; transform: translateY(3px); } }
  .activity-row { animation: step-arrive 240ms cubic-bezier(.16, 1, .3, 1) both; animation-delay: var(--step-delay); }
  @media (prefers-reduced-motion: reduce) {
    .activity-cluster, .cluster-chevron, .row-chevron { transition: none; }
    .activity-row { animation: none; }
    .file-node { animation: none; }
    .activity-row.running .activity-copy strong { color: #4e7fa6; }
    :global(.terminal-window.dark) .activity-row.running .activity-copy strong { color: #72a9cf; }
    .activity-row.running .activity-status i { animation: none; }
  }
</style>
