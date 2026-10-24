<script lang="ts">
  import WorkspaceHeaderIcon from "$lib/WorkspaceHeaderIcon.svelte";
  import type { MobileClient } from "./client.svelte";

  type Tab = "agents" | "workflows" | "settings";
  let { client, tab, onSelect }: { client: MobileClient; tab: Tab; onSelect: (tab: Tab) => void } = $props();

  const attention = $derived(client.sessions.filter((session) => session.status === "permission_required" || session.pendingQuestion).length);
  // Runs that wait on the person (an approval) or are under way.
  const running = $derived((client.snapshot?.workflowHistory ?? []).filter((record) => ["running", "waiting_for_approval"].includes(record.run.status)).length);
  const items = $derived([
    { id: "agents" as const, icon: "project" as const, label: client.tr("Agents", "Agentes"), badge: attention },
    { id: "workflows" as const, icon: "workflow" as const, label: client.tr("Workflows", "Workflows"), badge: running },
    { id: "settings" as const, icon: "settings" as const, label: client.tr("Settings", "Ajustes"), badge: client.update.available ? 1 : 0 },
  ]);
</script>

<nav class="mobile-nav" aria-label={client.tr("Main navigation", "Navegação principal")}>
  {#each items as item (item.id)}
    <button class:active={tab === item.id} type="button" aria-current={tab === item.id ? "page" : undefined} onclick={() => onSelect(item.id)}>
      <span class="icon"><WorkspaceHeaderIcon name={item.icon} size={22} />{#if item.badge}<i class="badge">{item.badge > 9 ? "9+" : item.badge}</i>{/if}</span>
      <span>{item.label}</span>
    </button>
  {/each}
</nav>

<style>
  .mobile-nav { padding: 6px 10px calc(var(--safe-bottom) + 6px); display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 4px; border-top: 1px solid var(--workspace-line); background: color-mix(in srgb, var(--workspace-sidebar) 92%, transparent); backdrop-filter: blur(14px); }
  button { min-height: 54px; padding: 6px 4px; display: grid; justify-items: center; align-content: center; gap: 3px; border: 0; border-radius: 14px; color: var(--workspace-muted); background: transparent; font-size: 11.5px; font-weight: 650; }
  button.active { color: var(--workspace-accent); background: var(--workspace-accent-soft); }
  .icon { position: relative; display: grid; place-items: center; }
  .badge { position: absolute; top: -5px; right: -9px; min-width: 17px; height: 17px; padding: 0 4px; display: grid; place-items: center; border: 2px solid var(--workspace-sidebar); border-radius: 999px; color: #1b1406; background: #e0a84a; font: 800 9.5px/1 var(--lume-font-ui, sans-serif); font-style: normal; }
</style>
