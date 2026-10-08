<script lang="ts">
  import { getContext } from "svelte";
  import { crossfade, fade } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import LumeIcon from "$lib/LumeIcon.svelte";
  import { openGitHub, type ActivityDay, type ActivityRepository } from "$lib/repositories";
  import { SYSTEM_BANNER_CONTEXT, type SystemBannerReporter } from "$lib/systemBannerContext";

  let { days = [], language = "en", kind = "contributions", repositories = [], total, compact = false, rangeDays = 365 }: {
    days: ActivityDay[]; language?: "en" | "pt-BR"; kind?: "contributions" | "commits";
    repositories?: ActivityRepository[]; total?: number; compact?: boolean; rangeDays?: 90 | 365;
  } = $props();
  let expanded = $state(false);
  let linkError = $state("");
  let failedAvatarUrls = $state<Record<string, boolean>>({});
  const bannerReporter = getContext<SystemBannerReporter | undefined>(SYSTEM_BANNER_CONTEXT);
  let selected = $state<ActivityDay | null>(null);
  let activeIndex = $state(-1);
  let fittedColumns = $state(18);
  let reducedMotion = $state(false);
  let cardElement = $state<HTMLElement | null>(null);
  let calendarElement = $state<HTMLDivElement | null>(null);
  let hovered = $state<{ x: number; y: number; background: string; color: string } | null>(null);
  const [sendAvatar, receiveAvatar] = crossfade({ duration: 320, easing: cubicOut });
  const tr = (en: string, pt: string) => language === "pt-BR" ? pt : en;
  const visibleDays = $derived.by(() => {
    const today = new Date(); today.setUTCHours(0, 0, 0, 0);
    const start = new Date(today); start.setUTCDate(today.getUTCDate() - rangeDays + 1);
    const from = start.toISOString().slice(0, 10);
    const through = today.toISOString().slice(0, 10);
    return days.filter((day) => day.date >= from && day.date <= through);
  });
  const count = $derived(total ?? visibleDays.reduce((sum, day) => sum + day.count, 0));
  const topRepositories = $derived(repositories.slice(0, 3));
  const overlayOpen = $derived(expanded && topRepositories.length > 0);
  const dateFormatter = $derived(new Intl.DateTimeFormat(language, { day: "numeric", month: "short", year: "numeric", timeZone: "UTC" }));
  const periodFormatter = $derived(new Intl.DateTimeFormat(language, { day: "numeric", month: "short", timeZone: "UTC" }));
  const calendar = $derived.by(() => {
    const today = new Date(); today.setUTCHours(0, 0, 0, 0);
    const rangeStart = new Date(today); rangeStart.setUTCDate(today.getUTCDate() - rangeDays + 1);
    const start = new Date(rangeStart); start.setUTCDate(start.getUTCDate() - start.getUTCDay());
    const indexed = new Map(visibleDays.map((day) => [day.date, day]));
    const cells: Array<ActivityDay & { unavailable: boolean }> = [];
    for (let i = 0; i < rangeDays + 7; i += 1) {
      const date = new Date(start); date.setUTCDate(start.getUTCDate() + i);
      const key = date.toISOString().slice(0, 10);
      cells.push({ ...(indexed.get(key) ?? { date: key, count: 0, level: 0 }), unavailable: date > today || date < rangeStart });
      if (i % 7 === 6 && date >= today) break;
    }
    return cells.slice(-Math.min(Math.ceil(cells.length / 7), fittedColumns) * 7);
  });
  const weeks = $derived(Math.ceil(calendar.length / 7));
  const firstDayIndex = $derived(calendar.findIndex((day) => !day.unavailable));
  const lastDayIndex = $derived(calendar.findLastIndex((day) => !day.unavailable));
  const periodLabel = $derived(firstDayIndex < 0 ? "" : `${periodFormatter.format(new Date(`${calendar[firstDayIndex].date}T00:00:00Z`))} — ${periodFormatter.format(new Date(`${calendar[lastDayIndex].date}T00:00:00Z`))}`);
  const months = $derived.by(() => {
    const labels = Array.from({ length: weeks }, () => "");
    let start = 0;
    for (let i = 1; i <= weeks; i += 1) {
      if (i < weeks && calendar[i * 7]?.date.slice(0, 7) === calendar[start * 7]?.date.slice(0, 7)) continue;
      if (i - start >= 3) labels[start] = new Intl.DateTimeFormat(language, { month: "short", timeZone: "UTC" }).format(new Date(`${calendar[start * 7].date}T00:00:00Z`)).replace(".", "");
      start = i;
    }
    return labels;
  });

  $effect(() => {
    const media = window.matchMedia("(prefers-reduced-motion: reduce)");
    const update = () => { reducedMotion = media.matches; };
    update(); media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  });
  function fitCalendar(node: HTMLElement) {
    const measure = () => {
      const next = Math.max(1, Math.floor((node.clientWidth + 3) / 14));
      if (next !== fittedColumns) { fittedColumns = next; activeIndex = -1; selected = null; hovered = null; }
    };
    measure();
    const observer = new ResizeObserver(measure); observer.observe(node);
    return { destroy: () => observer.disconnect() };
  }
  function description(day: ActivityDay) {
    const noun = kind === "commits" ? tr(day.count === 1 ? "commit" : "commits", day.count === 1 ? "commit" : "commits") : tr(day.count === 1 ? "contribution" : "contributions", day.count === 1 ? "contribuição" : "contribuições");
    return `${day.count} ${noun} · ${dateFormatter.format(new Date(`${day.date}T00:00:00Z`))}`;
  }
  function showDay(day: ActivityDay, event: PointerEvent | FocusEvent) {
    selected = day;
    const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const palette = cardElement ? getComputedStyle(cardElement) : null;
    hovered = { x: box.left + box.width / 2, y: box.top, background: palette?.getPropertyValue("--workspace-strong") || "#20392a", color: palette?.getPropertyValue("--activity-surface") || "#f6faf7" };
  }
  function hideDay() { selected = null; hovered = null; }
  function positionTooltip(node: HTMLElement, point: NonNullable<typeof hovered>) {
    document.body.appendChild(node);
    const place = () => {
      const box = node.getBoundingClientRect();
      const left = Math.max(8, Math.min(point.x - box.width / 2, window.innerWidth - box.width - 8));
      const above = point.y - box.height - 8;
      node.style.left = `${left}px`; node.style.top = `${above >= 8 ? above : Math.min(window.innerHeight - box.height - 8, point.y + 20)}px`;
    };
    place();
    return { update(next: NonNullable<typeof hovered>) { point = next; place(); }, destroy() { node.remove(); } };
  }
  function navigate(event: KeyboardEvent, index: number) {
    const move = { ArrowLeft: -7, ArrowRight: 7, ArrowUp: -1, ArrowDown: 1, Home: firstDayIndex - index, End: lastDayIndex - index }[event.key];
    if (move === undefined) return;
    event.preventDefault();
    activeIndex = Math.max(firstDayIndex, Math.min(lastDayIndex, index + move));
    calendarElement?.querySelectorAll<HTMLButtonElement>("button")[activeIndex]?.focus();
  }
  function toggleRepositories() { expanded = !expanded; hideDay(); }
  async function openRepository(url: string) {
    linkError = "";
    try { await openGitHub(url); }
    catch (error) {
      linkError = tr("Could not open GitHub. Try the repository link again.", "Não foi possível abrir o GitHub. Tente o link do repositório novamente.") + ` ${String(error).replace(/^Error:\s*/, "")}`;
      const message = linkError;
      bannerReporter?.({ id: "github-activity-link-error", message, tone: "error", onDismiss: () => { if (linkError === message) linkError = ""; } });
    }
  }
  function repositoryName(name: string) { return name.split("/").at(-1) ?? name; }
</script>

{#snippet repositoryAvatar(repo: ActivityRepository)}
  {@const avatarUrl = [repo.avatarUrl, repo.fallbackAvatarUrl].find((url) => url && !failedAvatarUrls[url])}
  {#if avatarUrl}
    <img src={avatarUrl} alt="" width="25" height="25" loading="lazy" decoding="async" referrerpolicy="no-referrer" onerror={() => (failedAvatarUrls[avatarUrl] = true)} />
  {:else}
    {repositoryName(repo.name).slice(0, 1).toUpperCase()}
  {/if}
{/snippet}

<section class="github-activity" class:compact class:has-repositories={topRepositories.length > 0} bind:this={cardElement} aria-label={kind === "commits" ? tr("Local repository commit calendar", "Calendário de commits locais do repositório") : tr("GitHub contribution calendar", "Calendário de contribuições do GitHub")}>
  <div class="activity-content" inert={overlayOpen} aria-hidden={overlayOpen}>
    <div class="activity-heading"><strong>{count.toLocaleString(language)} {kind === "commits" ? "commits" : tr("contributions", "contribuições")}</strong><span>{rangeDays === 90 ? tr("Last 90 days", "Últimos 90 dias") : tr("Last year", "Último ano")}</span></div>
    <div class="calendar-shell" use:fitCalendar>
      <div class="months" style:grid-template-columns={`repeat(${weeks}, 11px)`} aria-hidden="true">{#each months as month}<span>{month}</span>{/each}</div>
      <div class="calendar" role="group" aria-label={tr("Activity by day; use arrow keys to navigate", "Atividade por dia; use as setas para navegar")} bind:this={calendarElement} style:grid-template-columns={`repeat(${weeks}, 11px)`} onpointerleave={hideDay}>
        {#each calendar as day, index (day.date)}
          <button type="button" class="day level-{day.level}" class:unavailable={day.unavailable} tabindex={index === (activeIndex < 0 ? lastDayIndex : Math.min(activeIndex, lastDayIndex)) ? 0 : -1} disabled={day.unavailable} aria-label={description(day)} onpointerenter={(event) => showDay(day, event)} onfocus={(event) => { activeIndex = index; showDay(day, event); }} onblur={hideDay} onkeydown={(event) => navigate(event, index)} onclick={(event) => { selected = day; (event.currentTarget as HTMLElement).focus(); }}></button>
        {/each}
      </div>
    </div>
    <p class="calendar-caption" aria-live="polite">{selected ? description(selected) : `${kind === "commits" ? tr("Local branch ·", "Branch local ·") : ""} ${periodLabel}`}</p>
  </div>
  {#if topRepositories.length}
    <div class="repository-overlay" class:expanded>
      <button class="expand-activity" type="button" aria-expanded={expanded} aria-label={expanded ? tr("Hide top repositories", "Ocultar principais repositórios") : tr("Show top repositories", "Ver principais repositórios")} onclick={toggleRepositories}>
        <span>{tr("Top contributions in", "Mais contribuições em")}</span>
        {#if !expanded}<span class="repository-initials" aria-hidden="true">{#each topRepositories as repo (repo.name)}<span class="repo-avatar" in:receiveAvatar={{ key: repo.name, duration: reducedMotion ? 0 : 320 }} out:sendAvatar={{ key: repo.name, duration: reducedMotion ? 0 : 320 }}>{@render repositoryAvatar(repo)}</span>{/each}</span>{/if}
        <span class="expand-chevron"><LumeIcon name="chevron-down" size={15} /></span>
      </button>
      {#if expanded}
        <div class="top-repositories" transition:fade={{ duration: reducedMotion ? 0 : 160 }}>{#each topRepositories as repo (repo.name)}<button type="button" disabled={!repo.url} onclick={() => repo.url && void openRepository(repo.url)}><span class="repo-avatar" aria-hidden="true" in:receiveAvatar={{ key: repo.name, duration: reducedMotion ? 0 : 320 }} out:sendAvatar={{ key: repo.name, duration: reducedMotion ? 0 : 320 }}>{@render repositoryAvatar(repo)}</span><span class="contribution-repo" title={repo.name}>{repositoryName(repo.name)}<small>{repo.name.split("/").slice(0, -1).join("/")}</small></span><strong>{repo.count.toLocaleString(language)}<small>commits</small></strong></button>{/each}</div>
      {/if}
    </div>
  {/if}
  {#if !bannerReporter && linkError}<p class="activity-error" role="alert">{linkError}</p>{/if}
</section>
{#if hovered && selected && !overlayOpen}<div class="activity-tooltip" role="tooltip" use:positionTooltip={hovered} style:background={hovered.background} style:color={hovered.color} transition:fade={{ duration: reducedMotion ? 0 : 120 }}>{description(selected)}</div>{/if}

<style>
  .github-activity { position: relative; min-width: 0; padding: 16px 14px 0; border: 1px solid var(--workspace-line); border-radius: 14px; background: var(--workspace-pane); --activity-surface: var(--lume-surface-light, #f6faf7); }
  :global(.workspace.dark) .github-activity, :global(.overlay-shell.dark) .github-activity { --activity-surface: var(--lume-surface-dark, #12221a); }
  .activity-heading { display: flex; justify-content: space-between; align-items: baseline; gap: 8px; margin-bottom: 16px; }
  .activity-heading strong { color: var(--workspace-strong); font-size: 12px; font-weight: 600; font-variant-numeric: tabular-nums; }
  .activity-heading > span { color: var(--workspace-muted); font-size: 10px; white-space: nowrap; }
  .calendar-shell { min-width: 0; }
  .months { display: grid; justify-content: center; height: 20px; gap: 3px; color: var(--workspace-muted); font-size: 10px; }
  .months > span { overflow: visible; white-space: nowrap; }
  .calendar { display: grid; justify-content: center; grid-template-rows: repeat(7, 11px); grid-auto-flow: column; gap: 3px; }
  .day { width: 11px; height: 11px; padding: 0; border: 0; border-radius: 3px; cursor: pointer; }
  .day:hover { outline: 1px solid var(--workspace-accent); outline-offset: 2px; }
  button:focus-visible { outline: 2px solid var(--workspace-accent); outline-offset: 2px; }
  .day.unavailable { visibility: hidden; }
  .level-0 { background: var(--workspace-subtle); }
  .level-1 { background: color-mix(in srgb, var(--workspace-accent) 30%, var(--workspace-pane)); }
  .level-2 { background: color-mix(in srgb, var(--workspace-accent) 52%, var(--workspace-pane)); }
  .level-3 { background: color-mix(in srgb, var(--workspace-accent) 76%, var(--workspace-pane)); }
  .level-4 { background: var(--workspace-accent); }
  .calendar-caption { min-height: 39px; display: flex; align-items: center; margin: 0; color: var(--workspace-muted); font-size: 9px; line-height: 1.5; }
  .has-repositories { padding-bottom: 64px; }
  .repository-overlay { position: absolute; top: calc(100% - 62px); bottom: 8px; left: 8px; right: 8px; border-radius: 10px; background: var(--activity-surface); overflow: hidden; transition: top 380ms cubic-bezier(.22, 1, .36, 1); }
  .repository-overlay.expanded { top: 8px; display: flex; flex-direction: column; }
  .expand-activity { width: 100%; min-height: 52px; padding: 0 10px; display: flex; align-items: center; gap: 8px; border: 0; color: var(--workspace-text); background: var(--workspace-subtle); text-align: left; font: inherit; font-size: 11px; cursor: pointer; }
  .expand-activity > span:first-child { min-width: 0; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .expand-activity:hover { color: var(--workspace-strong); }
  .expanded .expand-activity { flex: 0 0 auto; background: transparent; }
  .expand-chevron { width: 24px; height: 24px; display: grid; place-items: center; flex: 0 0 auto; color: var(--workspace-muted); border-radius: 50%; background: var(--activity-surface); }
  .expand-chevron :global(svg) { transition: transform 380ms cubic-bezier(.22, 1, .36, 1); }
  .expanded .expand-chevron :global(svg) { transform: rotate(180deg); }
  .repository-initials { display: flex; padding-left: 6px; }
  .repo-avatar { width: 25px; height: 25px; display: grid; place-items: center; flex: 0 0 auto; border: 2px solid var(--activity-surface); border-radius: 50%; overflow: hidden; background: var(--workspace-subtle); color: var(--workspace-accent); font-size: 10px; font-weight: 600; }
  .repo-avatar :global(img) { width: 100%; height: 100%; display: block; object-fit: cover; }
  .repository-initials > .repo-avatar { margin-left: -6px; }
  .top-repositories { min-height: 0; padding: 0 5px 7px; overflow: auto; scrollbar-width: thin; scrollbar-color: var(--workspace-scroll-thumb) transparent; }
  .top-repositories > button { width: 100%; min-width: 0; min-height: 47px; padding: 6px; display: flex; align-items: center; gap: 8px; border: 0; border-radius: 8px; color: var(--workspace-text); background: transparent; text-align: left; font: inherit; font-size: 12px; cursor: pointer; }
  .top-repositories > button:hover { background: var(--workspace-subtle); }
  .top-repositories > button:disabled { cursor: default; opacity: .5; }
  .contribution-repo { min-width: 0; flex: 1; display: grid; gap: 3px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .contribution-repo > small { color: var(--workspace-muted); font-size: 9px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .top-repositories strong { display: grid; gap: 3px; color: var(--workspace-strong); font-size: 12px; font-weight: 550; font-variant-numeric: tabular-nums; text-align: right; }
  .top-repositories strong > small { color: var(--workspace-muted); font-size: 8px; font-weight: 400; }
  .activity-error { position: relative; z-index: 1; margin: 7px 0 10px; color: var(--workspace-text); font-size: 10px; line-height: 1.5; }
  .activity-tooltip { position: fixed; z-index: 1000; pointer-events: none; max-width: calc(100vw - 16px); padding: 7px 9px; border-radius: 7px; font: 500 11px/1.4 var(--lume-font-ui, Inter, sans-serif); box-shadow: 0 4px 14px rgba(0, 0, 0, .18); box-sizing: border-box; }
  @media (prefers-reduced-motion: reduce) { .repository-overlay, .expand-chevron :global(svg) { transition: none; } }
</style>
