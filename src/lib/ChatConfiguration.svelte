<script lang="ts">
  import LumeIcon, { type LumeIconName } from "$lib/LumeIcon.svelte";
  import {
    getClaudeSessionModelSettings, getSessionCollaborationMode, getSessionFastMode, getSessionModelSettings,
    getSessionPermissionMode, listAgentExtensions, type ExtensionItem, type ExtensionKind,
  } from "$lib/lume";
  import { permissionLabel } from "$lib/sessionPermissions";
  import type { AgentSession } from "$lib/domain";
  import type { Language } from "$lib/i18n";

  // Everything this chat runs with: its settings, and the plugins, skills, instructions, commands and subagents the
  // agent has for this folder. What the agent does not report is left out rather than guessed.
  let { session, language = "en" }: { session: AgentSession; language?: Language } = $props();
  const tr = (english: string, portuguese: string) => (language === "pt-BR" ? portuguese : english);

  type Row = { label: string; value: string; title?: string };
  let settings = $state<Row[]>([]);
  let items = $state<ExtensionItem[]>([]);
  let loadedKey = "";
  let loading = $state(true);
  let expanded = $state<Record<string, boolean>>({});
  const LIMIT = 6;

  const agentId = $derived(({ claude_code: "claude" } as Record<string, string>)[session.agent] ?? session.agent);
  const kinds: Array<{ kind: ExtensionKind; icon: LumeIconName; label: [string, string] }> = [
    { kind: "plugin", icon: "layout", label: ["Plugins", "Plugins"] },
    { kind: "skill", icon: "bolt", label: ["Skills", "Skills"] },
    { kind: "instruction", icon: "note", label: ["Instructions", "Instruções"] },
    { kind: "command", icon: "task-list", label: ["Commands", "Comandos"] },
    { kind: "subagent", icon: "fork", label: ["Subagents", "Subagentes"] },
  ];
  const grouped = $derived(kinds
    .map((entry) => ({ ...entry, items: items.filter((item) => item.kind === entry.kind) }))
    .filter((entry) => entry.items.length));

  const extensionCache = new Map<string, { at: number; value: Awaited<ReturnType<typeof listAgentExtensions>> }>();
  async function extensionsFor(project: string | undefined) {
    const key = project ?? "";
    const hit = extensionCache.get(key);
    if (hit && Date.now() - hit.at < 60_000) return hit.value;
    const value = await listAgentExtensions(project);
    extensionCache.set(key, { at: Date.now(), value });
    return value;
  }

  function efforts(value?: string | null) {
    return value ? value.charAt(0).toUpperCase() + value.slice(1) : tr("Default", "Padrão");
  }

  async function load(id: string, agent: string, folder: string | undefined) {
    const rows: Row[] = [{ label: tr("Agent", "Agente"), value: session.agentLabel }];
    const asks: Array<Promise<void>> = [];
    if (agent === "codex") {
      asks.push(getSessionModelSettings(id).then((value) => {
        const model = value.models.find((entry) => entry.model === value.model);
        rows.push({ label: tr("Model", "Modelo"), value: model?.displayName || value.model });
        rows.push({ label: tr("Reasoning", "Raciocínio"), value: efforts(value.reasoningEffort) });
      }).catch(() => {}));
      asks.push(getSessionFastMode(id).then((value) => { rows.push({ label: tr("Fast mode", "Modo rápido"), value: value ? tr("On", "Ligado") : tr("Off", "Desligado") }); }).catch(() => {}));
      asks.push(getSessionCollaborationMode(id).then((value) => { rows.push({ label: tr("Collaboration", "Colaboração"), value: value === "plan" ? tr("Plan", "Planejar") : tr("Default", "Padrão") }); }).catch(() => {}));
    } else if (agent === "claude_code") {
      asks.push(getClaudeSessionModelSettings(id).then((value) => {
        const model = value.models.find((entry) => entry.model === value.model);
        rows.push({ label: tr("Model", "Modelo"), value: model?.displayName || value.model });
        rows.push({ label: tr("Reasoning", "Raciocínio"), value: efforts(value.reasoningEffort) });
      }).catch(() => {}));
    }
    if (["claude_code", "codex", "antigravity", "omp"].includes(agent)) {
      asks.push(getSessionPermissionMode(id).then((value) => { rows.push({ label: tr("Permissions", "Permissões"), value: permissionLabel(value.mode, tr, agent) }); }).catch(() => {}));
    }
    const extension = extensionsFor(folder).then((all) => all.find((entry) => entry.agent === agentId)?.items ?? []).catch(() => [] as ExtensionItem[]);
    await Promise.all(asks);
    const order = [tr("Agent", "Agente"), tr("Model", "Modelo"), tr("Reasoning", "Raciocínio"), tr("Fast mode", "Modo rápido"), tr("Permissions", "Permissões"), tr("Collaboration", "Colaboração")];
    rows.sort((left, right) => order.indexOf(left.label) - order.indexOf(right.label));
    if (folder) rows.push({ label: tr("Folder", "Pasta"), value: folder.split(/[\\/]/).filter(Boolean).at(-1) ?? folder, title: folder });
    const found = await extension;
    if (loadedKey !== `${id}|${folder ?? ""}`) return;
    settings = rows;
    items = found;
    loading = false;
  }

  // Read again when the chat or its folder changes, not on every streamed update of the session.
  const key = $derived(`${session.id}|${session.workingDirectory ?? ""}`);
  $effect(() => {
    const current = key;
    if (loadedKey === current) return;
    loadedKey = current;
    settings = []; items = []; expanded = {};
    if (session.remote) { settings = [{ label: tr("Agent", "Agente"), value: session.agentLabel }, { label: tr("Computer", "Computador"), value: session.remote.computer }]; loading = false; return; }
    loading = true;
    void load(session.id, session.agent, session.workingDirectory ?? undefined);
  });
</script>

{#if loading}
  <section class="config-loading" role="status" aria-label={tr("Loading this chat's settings", "Carregando os ajustes deste chat")}>
    <div class="bar head"></div>
    {#each [62, 78, 54, 70, 48] as width}<div class="bar" style:width="{width}%"></div>{/each}
    <div class="bar head"></div>
    <div class="bar head"></div>
  </section>
{:else if settings.length}
  <details class="inspector-section config-section" open>
    <summary><LumeIcon name="settings" size={14} /><span>{tr("Settings", "Ajustes")}</span><em>{settings.length}</em></summary>
    <dl class="config-rows">
      {#each settings as row (row.label)}<dt>{row.label}</dt><dd title={row.title ?? row.value}>{row.value}</dd>{/each}
    </dl>
  </details>
{/if}

{#if !loading}
{#each grouped as group (group.kind)}
  <details class="inspector-section config-section">
    <summary><LumeIcon name={group.icon} size={14} /><span>{tr(group.label[0], group.label[1])}</span><em>{group.items.length}</em></summary>
    <ul class="config-list">
      {#each expanded[group.kind] ? group.items : group.items.slice(0, LIMIT) as item (item.id)}
        <li class:off={item.enabled === false} title={item.description || item.name}>
          <strong>{item.name}</strong>
          <small>{item.enabled === false ? tr("Disabled", "Desativado") : item.scope}{item.version ? ` · ${item.version}` : ""}</small>
        </li>
      {/each}
    </ul>
    {#if group.items.length > LIMIT}
      <button class="config-more" type="button" onclick={() => (expanded = { ...expanded, [group.kind]: !expanded[group.kind] })}>
        {expanded[group.kind] ? tr("Show fewer", "Mostrar menos") : tr(`Show ${group.items.length - LIMIT} more`, `Mostrar mais ${group.items.length - LIMIT}`)}
      </button>
    {/if}
  </details>
{/each}
{/if}

<style>
  .config-loading { padding: 14px 0 16px; display: grid; gap: 10px; border-bottom: 1px solid var(--workspace-line); }
  .config-loading .bar { height: 12px; border-radius: 6px; background: var(--workspace-line); opacity: .5; animation: config-pulse 1.2s ease-in-out infinite alternate; }
  .config-loading .bar.head { width: 40%; height: 14px; margin-top: 4px; }
  @keyframes config-pulse { from { opacity: .22; } to { opacity: .5; } }
  @media (prefers-reduced-motion: reduce) { .config-loading .bar { animation: none; } }
  .config-rows { margin: 0 0 8px; display: grid; grid-template-columns: auto minmax(0, 1fr); gap: 6px 12px; font-size: 10.5px; }
  .config-rows dt { color: var(--workspace-muted); }
  .config-rows dd { min-width: 0; margin: 0; overflow: hidden; color: var(--workspace-strong); font-weight: 560; text-align: right; text-overflow: ellipsis; white-space: nowrap; }
  .config-list { margin: 0; padding: 0; list-style: none; display: grid; gap: 1px; }
  .config-list li { min-width: 0; padding: 5px 0; display: flex; align-items: baseline; justify-content: space-between; gap: 10px; }
  .config-list li + li { border-top: 1px solid var(--workspace-line); }
  .config-list li.off { opacity: .55; }
  .config-list strong { min-width: 0; overflow: hidden; color: var(--workspace-text); font-size: 10.5px; font-weight: 600; text-overflow: ellipsis; white-space: nowrap; }
  .config-list small { flex: 0 0 auto; max-width: 45%; overflow: hidden; color: var(--workspace-muted); font-size: 9px; text-overflow: ellipsis; white-space: nowrap; }
  .config-more { padding: 6px 0 2px; border: 0; color: var(--workspace-accent); background: transparent; font: inherit; font-size: 10px; cursor: pointer; }
  .config-more:focus-visible { outline: 2px solid var(--workspace-accent); outline-offset: 2px; }
</style>
