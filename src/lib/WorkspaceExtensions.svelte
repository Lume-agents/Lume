<script lang="ts">
  import { onMount } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { fade, fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import type { Language } from "$lib/i18n";
  import BrandIcon from "$lib/BrandIcon.svelte";
  import LumeIcon from "$lib/LumeIcon.svelte";
  import ActivityTypeIcon from "$lib/ActivityTypeIcon.svelte";
  import LumeSelect from "$lib/LumeSelect.svelte";
  import PluginImage from "$lib/PluginImage.svelte";
  import { hasCatalogMetric, isOfficialMarketplace, sortExtensionCatalog, type CatalogSort } from "$lib/extensionCatalog";
  import { renderSafeMarkdown } from "$lib/markdown.js";
  import {
    listAgentExtensions,
    listExtensionCatalog,
    getExtensionGithubStars,
    manageAgentPlugin,
    readExtensionFile,
    revealExtensionPath,
    type AgentExtensions,
    type CatalogPlugin,
    type ExtensionItem,
    type ExtensionKind,
  } from "$lib/lume";

  let { language = "en", project, onClose } = $props<{
    language?: Language;
    /** The focused chat's folder, for the project's own skills, instructions and servers. */
    project?: string;
    onClose: () => void;
  }>();

  const kinds: ExtensionKind[] = ["plugin", "skill", "mcp", "instruction", "command", "subagent"];
  const catalogPage = 60;

  let tab = $state<"installed" | "catalog">("installed");
  let agents = $state<AgentExtensions[]>([]);
  let loading = $state(true);
  let loadError = $state("");
  let agentId = $state("");
  let kindFilter = $state<ExtensionKind | "all">("all");
  let query = $state("");
  let selected = $state<ExtensionItem | null>(null);
  let preview = $state<{ id: string; text: string; error?: string } | null>(null);
  let catalogs = $state<Record<string, CatalogPlugin[]>>({});
  let repositoryStars = $state<Record<string, number | null>>({});
  const requestedRepositories = new Set<string>();
  let catalogLoading = $state(false);
  let catalogError = $state("");
  let catalogLimit = $state(catalogPage);
  let catalogSort = $state<CatalogSort>("popular");
  let catalogRequest = 0;
  let busy = $state("");
  let notice = $state<{ tone: "ok" | "error"; text: string } | null>(null);
  let pending = $state<{ agent: string; plugin: string; command: string; sha256: string } | null>(null);
  let searchInput = $state<HTMLInputElement | null>(null);

  const pt = $derived(language === "pt-BR");
  const agent = $derived(agents.find((item) => item.agent === agentId) ?? agents[0] ?? null);
  const needle = $derived(query.trim().toLocaleLowerCase());
  const matches = (text: string) => !needle || text.toLocaleLowerCase().includes(needle);
  const visibleItems = $derived((agent?.items ?? []).filter((item) => (kindFilter === "all" || item.kind === kindFilter) && matches(`${item.name} ${item.description} ${item.source ?? ""}`)));
  const sections = $derived(kinds.map((kind) => ({ kind, items: visibleItems.filter((item) => item.kind === kind) })).filter((section) => section.items.length));
  const counts = $derived(Object.fromEntries(kinds.map((kind) => [kind, (agent?.items ?? []).filter((item) => item.kind === kind).length])) as Record<ExtensionKind, number>);
  const catalog = $derived(agent ? catalogs[agent.agent]?.map((plugin) => ({ ...plugin, githubStars: plugin.githubRepository ? repositoryStars[plugin.githubRepository] : undefined })) ?? null : null);
  const hasUsageCounts = $derived((catalog ?? []).some((plugin) => hasCatalogMetric(plugin.installs) || hasCatalogMetric(plugin.downloads)));
  const hasRepositories = $derived((catalog ?? []).some((plugin) => plugin.githubRepository));
  const catalogMatches = $derived(sortExtensionCatalog((catalog ?? []).filter((plugin) => matches(`${plugin.name} ${plugin.description} ${plugin.category ?? ""} ${plugin.marketplace}`)), catalogSort === "popular" && !hasUsageCounts && hasRepositories ? "stars" : catalogSort, language));
  const sortOptions = $derived([
    { value: "popular", label: !hasUsageCounts && hasRepositories ? tr("Most GitHub stars", "Mais estrelados no GitHub") : tr("Most installed", "Mais instalados"), description: hasUsageCounts ? undefined : hasRepositories ? tr("Repository stars, not plugin installs. Unknown counts are listed last.", "Estrelas do repositório, não instalações do plugin. Contagens indisponíveis ficam por último.") : tr("No counts provided; alphabetical order is used.", "Sem contagens informadas; a ordem alfabética é usada.") },
    ...((catalog ?? []).some((plugin) => hasCatalogMetric(plugin.downloads)) ? [{ value: "downloads", label: tr("Most downloaded", "Mais baixados") }] : []),
    ...(hasUsageCounts && hasRepositories ? [{ value: "stars", label: tr("Most GitHub stars", "Mais estrelados no GitHub") }] : []),
    { value: "name", label: tr("Name (A–Z)", "Nome (A–Z)") },
    ...((catalog ?? []).some((plugin) => hasCatalogMetric(plugin.updatedAt)) ? [{ value: "recent", label: tr("Most recent", "Mais recentes") }] : []),
    ...((catalog ?? []).some((plugin) => hasCatalogMetric(plugin.rating)) ? [{ value: "rating", label: tr("Best rated", "Melhor avaliados") }] : []),
  ]);
  const installedPluginIds = $derived(new Set((agent?.items ?? []).filter((item) => item.kind === "plugin").map((item) => item.id.replace(/^plugin:/, ""))));

  function tr(english: string, portuguese: string) {
    return pt ? portuguese : english;
  }

  function kindLabel(kind: ExtensionKind, plural = true) {
    const labels: Record<ExtensionKind, [string, string, string, string]> = {
      plugin: ["Plugins", "Plugins", "Plugin", "Plugin"],
      skill: ["Skills", "Skills", "Skill", "Skill"],
      mcp: ["MCP servers", "Servidores MCP", "MCP server", "Servidor MCP"],
      instruction: ["Instructions", "Instruções", "Instructions", "Instruções"],
      command: ["Commands", "Comandos", "Command", "Comando"],
      subagent: ["Subagents", "Subagentes", "Subagent", "Subagente"],
    };
    const [en, ptText, enOne, ptOne] = labels[kind];
    return plural ? tr(en, ptText) : tr(enOne, ptOne);
  }

  function scopeLabel(scope: string) {
    const labels: Record<string, [string, string]> = { user: ["User", "Usuário"], project: ["Project", "Projeto"], local: ["Local", "Local"], account: ["Account", "Conta"] };
    const label = labels[scope];
    return label ? tr(label[0], label[1]) : scope;
  }

  function installsLabel(count?: number | null) {
    if (count == null) return "";
    const short = count >= 1000 ? `${(count / 1000).toLocaleString(language, { maximumFractionDigits: 1 })}${pt ? " mil" : "k"}` : count.toLocaleString(language);
    return short;
  }

  function dateLabel(stamp: number) {
    return Number.isFinite(new Date(stamp).getTime()) ? new Intl.DateTimeFormat(language, { dateStyle: "medium" }).format(stamp) : "";
  }

  function pluginId(item: ExtensionItem) {
    return item.id.replace(/^plugin:/, "");
  }

  async function load() {
    loading = true;
    loadError = "";
    try {
      agents = await listAgentExtensions(project);
      if (!agents.some((item) => item.agent === agentId)) agentId = agents[0]?.agent ?? "";
      if (selected) selected = agents.flatMap((item) => item.items).find((item) => item.id === selected?.id) ?? null;
    } catch (error) {
      loadError = String(error);
    } finally {
      loading = false;
    }
  }

  async function loadCatalog(refresh = false) {
    if (!agent?.managesPlugins || (catalogs[agent.agent] && !refresh)) return;
    const target = agent.agent;
    const request = ++catalogRequest;
    catalogLoading = true;
    catalogError = "";
    try {
      const plugins = await listExtensionCatalog(target, refresh);
      catalogs = { ...catalogs, [target]: plugins };
      void loadRepositoryStars(plugins);
    } catch (error) {
      if (request === catalogRequest) catalogError = String(error);
    } finally {
      if (request === catalogRequest) catalogLoading = false;
    }
  }

  async function loadRepositoryStars(plugins: CatalogPlugin[]) {
    const repositories = [...new Set(plugins.flatMap((plugin) => plugin.githubRepository ? [plugin.githubRepository] : []))]
      .filter((repository) => !requestedRepositories.has(repository)).slice(0, 40);
    if (!repositories.length) return;
    repositories.forEach((repository) => requestedRepositories.add(repository));
    try {
      const counts = await getExtensionGithubStars(repositories);
      repositoryStars = { ...repositoryStars, ...Object.fromEntries(counts.map(({ repository, stars }) => [repository, stars])) };
    } catch { /* Optional public metadata: an offline GitHub must never hide the native catalog. */ }
  }

  async function openRepository(repository: string) {
    try { await openUrl(`https://github.com/${repository}`); }
    catch (error) { notice = { tone: "error", text: String(error) }; }
  }

  function chooseAgent(id: string) {
    catalogRequest += 1;
    catalogLoading = false;
    catalogError = "";
    agentId = id;
    selected = null;
    kindFilter = "all";
    catalogLimit = catalogPage;
    catalogSort = "popular";
    if (tab === "catalog") void loadCatalog();
  }

  function chooseTab(next: "installed" | "catalog") {
    catalogRequest += 1;
    catalogLoading = false;
    catalogError = "";
    tab = next;
    selected = null;
    catalogLimit = catalogPage;
    catalogSort = "popular";
    if (next === "catalog") void loadCatalog();
  }

  async function select(item: ExtensionItem) {
    selected = selected?.id === item.id ? null : item;
    if (!selected?.path || preview?.id === selected.id) return;
    const id = selected.id;
    preview = { id, text: "" };
    try {
      const text = await readExtensionFile(selected.path, project);
      if (preview?.id === id) preview = { id, text };
    } catch (error) {
      if (preview?.id === id) preview = { id, text: "", error: String(error) };
    }
  }

  /** The body of a markdown file without its frontmatter, which the card already shows. */
  function previewBody(text: string) {
    return text.replace(/^---\n[\s\S]*?\n---\n?/, "").trim();
  }

  async function act(agentName: string, action: "install" | "uninstall" | "enable" | "disable", plugin: string, acceptCommand?: string) {
    busy = `${action}:${plugin}`;
    notice = null;
    try {
      const result = await manageAgentPlugin(agentName, action, plugin, acceptCommand);
      if (result.confirm) {
        pending = { agent: agentName, plugin, ...result.confirm };
        return;
      }
      const done: Record<typeof action, [string, string]> = { install: ["Installed", "Instalado"], uninstall: ["Removed", "Removido"], enable: ["Enabled", "Ativado"], disable: ["Disabled", "Desativado"] };
      notice = result.ok
        ? { tone: "ok", text: `${tr(done[action][0], done[action][1])}: ${plugin.split("@")[0]}` }
        : { tone: "error", text: result.message || tr("The agent's CLI could not finish this.", "A CLI do agente não concluiu esta ação.") };
      if (result.ok) {
        if (action === "uninstall") selected = null;
        await load();
        if (tab === "catalog") await loadCatalog(true);
      }
    } catch (error) {
      notice = { tone: "error", text: String(error) };
    } finally {
      busy = "";
    }
  }

  function confirmPending() {
    if (!pending) return;
    const { agent: agentName, plugin, sha256 } = pending;
    pending = null;
    void act(agentName, "install", plugin, sha256);
  }

  function keydown(event: KeyboardEvent) {
    if (event.key !== "Escape") return;
    if (pending) pending = null;
    else if (selected) selected = null;
    else if (query) query = "";
    else onClose();
  }

  onMount(() => {
    void load();
    searchInput?.focus();
  });
</script>

<svelte:window onkeydown={keydown} />

{#snippet kindIcon(kind: ExtensionKind, size = 15)}
  {#if kind === "skill" || kind === "instruction" || kind === "command"}
    <ActivityTypeIcon category={kind} {size} />
  {:else if kind === "mcp"}
    <LumeIcon name="server" {size} />
  {:else if kind === "plugin"}
    <svg viewBox="0 0 24 24" width={size} height={size} fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 4.5a2 2 0 0 1 4 0V6h3.5a1.5 1.5 0 0 1 1.5 1.5V11h-1.5a2 2 0 0 0 0 4H18v3.5a1.5 1.5 0 0 1-1.5 1.5H13v-1.5a2 2 0 0 0-4 0V20H5.5A1.5 1.5 0 0 1 4 18.5V15h1.5a2 2 0 0 0 0-4H4V7.5A1.5 1.5 0 0 1 5.5 6H9z" /></svg>
  {:else}
    <svg viewBox="0 0 24 24" width={size} height={size} fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="5" y="8" width="14" height="11" rx="3" /><path d="M12 8V5M9.5 13h.01M14.5 13h.01M9 19v1.5M15 19v1.5" /><circle cx="12" cy="4" r="1" /></svg>
  {/if}
{/snippet}

{#snippet officialBadge(source: string)}
  <span class="official-badge" title={tr(`Official catalog: ${source}. This identifies the source, not a security review or plugin authorship.`, `Catálogo oficial: ${source}. Identifica a origem, não uma auditoria de segurança ou a autoria do plugin.`)}>
    {tr("Official", "Oficial")}
  </span>
{/snippet}

<section class="extensions" aria-label={tr("Extensions", "Extensões")} in:fade={{ duration: 160 }} out:fade={{ duration: 110 }}>
  <header class="extensions-bar">
    <span class="extensions-title">
      <span class="title-mark">{@render kindIcon("plugin", 17)}</span>
      <strong>{tr("Extensions", "Extensões")}</strong>
    </span>
    <span class="extensions-tabs" role="tablist">
      <button class:on={tab === "installed"} type="button" role="tab" aria-selected={tab === "installed"} onclick={() => chooseTab("installed")}>{tr("Installed", "Instalados")}</button>
      <button class:on={tab === "catalog"} type="button" role="tab" aria-selected={tab === "catalog"} onclick={() => chooseTab("catalog")}>{tr("Catalog", "Catálogo")}</button>
    </span>
    <label class="extensions-search">
      <LumeIcon name="search" size={14} />
      <input bind:this={searchInput} bind:value={query} type="search" placeholder={tab === "catalog" ? tr("Search plugins", "Buscar plugins") : tr("Search extensions", "Buscar extensões")} aria-label={tr("Search", "Buscar")} />
    </label>
    <button class="bar-icon" type="button" title={tr("Refresh", "Atualizar")} aria-label={tr("Refresh", "Atualizar")} disabled={loading || catalogLoading} onclick={() => { void load(); if (tab === "catalog") void loadCatalog(true); }}><LumeIcon name="refresh" size={15} /></button>
    <button class="bar-icon" type="button" title={tr("Close", "Fechar")} aria-label={tr("Close extensions", "Fechar extensões")} onclick={onClose}><LumeIcon name="close" size={15} /></button>
  </header>

  <nav class="agent-rail" aria-label={tr("Agents", "Agentes")}>
    {#each agents as item (item.agent)}
      <button class:on={item.agent === agent?.agent} type="button" onclick={() => chooseAgent(item.agent)}>
        <BrandIcon name={item.agent as never} size={18} />
        <span><strong>{item.label}</strong><small>{tab === "catalog" ? (item.managesPlugins ? tr("Plugin catalog", "Catálogo de plugins") : tr("No catalog", "Sem catálogo")) : tr(`${item.items.length} installed`, `${item.items.length} instalados`)}</small></span>
      </button>
    {/each}
    {#if project}
      <p class="rail-project" title={project}><LumeIcon name="folder" size={13} /><span>{tr("Including", "Incluindo")} <b>{project.split(/[\\/]/).filter(Boolean).at(-1)}</b></span></p>
    {/if}
  </nav>

  <div class="extensions-main">
    {#if notice}
      <div class="notice {notice.tone}" role="status" transition:fly={{ y: -4, duration: 140 }}>
        <span>{notice.text}</span>
        <button type="button" aria-label={tr("Dismiss", "Fechar")} onclick={() => (notice = null)}><LumeIcon name="close" size={12} /></button>
      </div>
    {/if}

    {#if loading && !agents.length}
      <div class="state"><span class="spinner" aria-hidden="true"></span>{tr("Reading what your agents have installed…", "Lendo o que seus agentes têm instalado…")}</div>
    {:else if loadError}
      <div class="state error">{loadError}</div>
    {:else if !agent}
      <div class="state">{tr("No coding agents were found on this machine.", "Nenhum agente de código foi encontrado nesta máquina.")}</div>
    {:else if tab === "installed"}
      <div class="kind-filter" role="group" aria-label={tr("Show", "Mostrar")}>
        <button class:on={kindFilter === "all"} type="button" onclick={() => (kindFilter = "all")}>{tr("All", "Tudo")}<small>{agent.items.length}</small></button>
        {#each kinds as kind (kind)}
          {#if counts[kind]}
            <button class:on={kindFilter === kind} type="button" onclick={() => (kindFilter = kindFilter === kind ? "all" : kind)}>{@render kindIcon(kind, 13)}{kindLabel(kind)}<small>{counts[kind]}</small></button>
          {/if}
        {/each}
      </div>
      {#if agent.problems.length}
        <p class="problems"><LumeIcon name="warning" size={13} />{agent.problems.join(" · ")}</p>
      {/if}
      {#if !sections.length}
        <div class="state">{needle ? tr("Nothing matches this search.", "Nada corresponde a esta busca.") : agent.managesPlugins ? tr(`${agent.label} has no extensions yet. The catalog has plugins to start with.`, `${agent.label} ainda não tem extensões. O catálogo tem plugins para começar.`) : tr(`No extensions detected for ${agent.label}.`, `Nenhuma extensão detectada para ${agent.label}.`)}</div>
      {/if}
      {#each sections as section (section.kind)}
        <section class="kind-section">
          <h3>{@render kindIcon(section.kind, 14)}{kindLabel(section.kind)}<small>{section.items.length}</small></h3>
          <div class="card-grid">
            {#each section.items as item (item.id)}
              <button class:selected={selected?.id === item.id} class:off={item.enabled === false} class="ext-card kind-{item.kind}" type="button" onclick={() => void select(item)}>
                <span class="card-head">
                  <span class="card-icon"><PluginImage url={item.iconUrl} darkUrl={item.iconDarkUrl}>{@render kindIcon(item.kind)}</PluginImage></span>
                  <strong title={item.name}>{item.name}</strong>
                  {#if item.kind === "plugin" && isOfficialMarketplace(agent.agent, item.source)}{@render officialBadge(item.source!)}{/if}
                  {#if item.enabled === false}<span class="chip muted">{tr("Off", "Desativado")}</span>{/if}
                </span>
                <span class="card-text">{item.description || (item.kind === "plugin" ? item.source : "") || tr("No description", "Sem descrição")}</span>
                <span class="card-meta">
                  <span class="chip">{scopeLabel(item.scope)}</span>
                  {#if item.version}<span>v{item.version}</span>{/if}
                  {#if item.kind === "plugin" && item.source}<span title={item.source}>{item.source}</span>{/if}
                  {#if item.kind === "mcp" && item.source}<span>{item.source}</span>{/if}
                </span>
              </button>
            {/each}
          </div>
        </section>
      {/each}
    {:else if !agent.managesPlugins}
      <div class="state">{agent.cli ? tr(`Lume does not yet manage a catalog for ${agent.label}. Its detected extensions are under Installed.`, `O Lume ainda não gerencia um catálogo para ${agent.label}. As extensões detectadas estão em Instalados.`) : tr(`Install the ${agent.label} CLI to browse its plugins.`, `Instale a CLI do ${agent.label} para ver os plugins dele.`)}</div>
    {:else if catalogLoading && !catalog}
      <div class="state"><span class="spinner" aria-hidden="true"></span>{tr(`Asking ${agent.label} for its marketplaces…`, `Pedindo ao ${agent.label} os marketplaces…`)}</div>
    {:else if catalogError}
      <div class="state error">{catalogError}</div>
    {:else if catalog}
      <div class="catalog-toolbar">
        <p class="catalog-summary">{catalogMatches.length} plugins · {tr(`installed with the ${agent.label} CLI`, `instalação pela CLI do ${agent.label}`)}</p>
        <LumeSelect value={catalogSort} options={sortOptions} ariaLabel={tr("Sort plugins", "Ordenar plugins")} variant="heading" minWidth={155} menuMinWidth={190} onValueChange={(value) => { catalogSort = value as CatalogSort; catalogLimit = catalogPage; }} />
      </div>
      {#if agent.agent === "antigravity"}
        <p class="catalog-summary">{tr("Official marketplace cached by Antigravity. Use /plugins in its CLI to synchronize new entries.", "Marketplace oficial em cache pelo Antigravity. Use /plugins na CLI para sincronizar novas opções.")}</p>
      {/if}
      {#if !catalogMatches.length}
        <div class="state">{needle ? tr("No plugin matches this search.", "Nenhum plugin corresponde a esta busca.") : tr("The marketplaces have no plugins to offer.", "Os marketplaces não têm plugins para oferecer.")}</div>
      {/if}
      <div class="card-grid">
        {#each catalogMatches.slice(0, catalogLimit) as plugin (plugin.id)}
          {@const installed = plugin.installed || installedPluginIds.has(plugin.id) || (agent.agent === "antigravity" && installedPluginIds.has(plugin.id.split("@")[0]))}
          <article class="ext-card catalog-card">
            <span class="card-head">
              <span class="card-icon"><PluginImage url={plugin.iconUrl} darkUrl={plugin.iconDarkUrl}>{@render kindIcon("plugin")}</PluginImage></span>
              <strong title={plugin.id}>{plugin.name}</strong>
              {#if isOfficialMarketplace(agent.agent, plugin.marketplace)}
                {@render officialBadge(plugin.marketplace)}
              {:else}
                <small class="marketplace" title={plugin.marketplace}>{plugin.marketplace}</small>
              {/if}
            </span>
            <span class="card-text">{plugin.description || tr("No description", "Sem descrição")}</span>
            <span class="card-meta">
              {#if plugin.category}<span class="chip">{plugin.category}</span>{/if}
              {#if hasCatalogMetric(plugin.downloads)}
                <span class="installs" title={tr(`${plugin.downloads.toLocaleString(language)} downloads`, `${plugin.downloads.toLocaleString(language)} downloads`)}><LumeIcon name="download" size={11} />{installsLabel(plugin.downloads)} downloads</span>
                {#if hasCatalogMetric(plugin.installs)}<span class="installs" title={tr(`${plugin.installs.toLocaleString(language)} installs`, `${plugin.installs.toLocaleString(language)} instalações`)}>{installsLabel(plugin.installs)} inst.</span>{/if}
              {:else if hasCatalogMetric(plugin.installs)}
                <span class="installs" title={tr(`${plugin.installs.toLocaleString(language)} installs`, `${plugin.installs.toLocaleString(language)} instalações`)}><LumeIcon name="download" size={11} />{installsLabel(plugin.installs)}</span>
              {/if}
              {#if hasCatalogMetric(plugin.rating)}<span class="plugin-rating" title={tr("Rating reported by the catalog", "Nota informada pelo catálogo")}>{tr("Rating", "Nota")} {plugin.rating.toLocaleString(language, { maximumFractionDigits: 2 })}</span>{/if}
              {#if plugin.githubRepository && hasCatalogMetric(plugin.githubStars)}
                <button class="repo-stars" type="button" title={tr(`${plugin.githubRepository}: ${plugin.githubStars.toLocaleString(language)} repository stars. A repository can contain several plugins.`, `${plugin.githubRepository}: ${plugin.githubStars.toLocaleString(language)} estrelas do repositório. Um repositório pode conter vários plugins.`)} onclick={() => void openRepository(plugin.githubRepository!)}>
                  <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linejoin="round" aria-hidden="true"><path d="m12 3 2.8 5.7 6.2.9-4.5 4.4 1.1 6.2-5.6-3-5.6 3 1.1-6.2L3 9.6l6.2-.9Z" /></svg>
                  {installsLabel(plugin.githubStars)} {tr("GitHub stars", "estrelas no GitHub")}
                </button>
              {/if}
              {#if plugin.updatedAt && dateLabel(plugin.updatedAt)}<time datetime={new Date(plugin.updatedAt).toISOString()} title={tr("Updated or published on this date", "Atualizado ou publicado nesta data")}>{dateLabel(plugin.updatedAt)}</time>{/if}
              {#if installed}
                <span class="installed-mark"><LumeIcon name="check" size={12} />{tr("Installed", "Instalado")}</span>
              {:else}
                <button class="install" type="button" disabled={Boolean(busy)} onclick={() => void act(agent.agent, "install", plugin.id)}>
                  {#if busy === `install:${plugin.id}`}<span class="spinner small" aria-hidden="true"></span>{tr("Installing", "Instalando")}{:else}<LumeIcon name="download" size={12} />{tr("Install", "Instalar")}{/if}
                </button>
              {/if}
            </span>
          </article>
        {/each}
      </div>
      {#if catalogMatches.length > catalogLimit}
        <button class="show-more" type="button" onclick={() => (catalogLimit += catalogPage)}>{tr(`Show more (${catalogMatches.length - catalogLimit})`, `Mostrar mais (${catalogMatches.length - catalogLimit})`)}</button>
      {/if}
    {/if}
  </div>

  {#if selected && agent}
    {@const item = selected}
    <aside class="detail" aria-label={item.name} in:fly={{ x: 16, duration: 180, easing: cubicOut }} out:fly={{ x: 12, duration: 120 }}>
      <header>
        <span class="card-icon"><PluginImage url={item.iconUrl} darkUrl={item.iconDarkUrl}>{@render kindIcon(item.kind, 17)}</PluginImage></span>
        <span class="detail-title"><strong>{item.name}</strong><small>{kindLabel(item.kind, false)} · {scopeLabel(item.scope)}{item.version ? ` · v${item.version}` : ""}</small></span>
        <button class="bar-icon" type="button" aria-label={tr("Close", "Fechar")} onclick={() => (selected = null)}><LumeIcon name="close" size={14} /></button>
      </header>
      {#if item.description}<p class="detail-description">{item.description}</p>{/if}
      {#if item.kind === "mcp"}
        <dl class="detail-facts"><dt>{tr("Transport", "Transporte")}</dt><dd>{item.source}</dd><dt>{tr("Target", "Destino")}</dt><dd><code>{item.description}</code></dd></dl>
      {/if}
      {#if item.path}
        <code class="detail-path" title={item.path}><bdi>{item.path}</bdi></code>
      {/if}
      <div class="detail-actions">
        {#if item.path}
          <button type="button" onclick={() => void revealExtensionPath(item.path ?? "", project).catch((error) => (notice = { tone: "error", text: String(error) }))}><LumeIcon name="folder" size={13} />{tr("Show in folder", "Mostrar na pasta")}</button>
        {/if}
        {#if item.kind === "plugin" && agent.togglesPlugins && item.enabled !== null && item.enabled !== undefined}
          <button type="button" disabled={Boolean(busy)} onclick={() => void act(agent.agent, item.enabled ? "disable" : "enable", agent.agent === "gemini" ? item.name : pluginId(item))}>{item.enabled ? tr("Disable", "Desativar") : tr("Enable", "Ativar")}</button>
        {/if}
        {#if item.kind === "plugin" && (agent.managesPlugins || agent.agent === "gemini") && agent.cli}
          <button class="danger" type="button" disabled={Boolean(busy)} onclick={() => void act(agent.agent, "uninstall", agent.agent === "gemini" ? item.name : pluginId(item))}>{#if busy === `uninstall:${agent.agent === "gemini" ? item.name : pluginId(item)}`}<span class="spinner small" aria-hidden="true"></span>{:else}<LumeIcon name="trash" size={13} />{/if}{tr("Remove", "Remover")}</button>
        {/if}
      </div>
      {#if item.path}
        <div class="detail-preview">
          {#if preview?.id !== item.id || (!preview.text && !preview.error)}
            <span class="spinner" aria-hidden="true"></span>
          {:else if preview.error}
            <p class="preview-error">{preview.error}</p>
          {:else if /\.md$/i.test(item.path)}
            <div class="markdown">{@html renderSafeMarkdown(previewBody(preview.text))}</div>
          {:else}
            <pre>{preview.text}</pre>
          {/if}
        </div>
      {/if}
    </aside>
  {/if}

  {#if pending}
    <div class="confirm-backdrop" transition:fade={{ duration: 120 }}>
      <div class="confirm" role="alertdialog" aria-modal="true" aria-labelledby="extension-confirm-title">
        <strong id="extension-confirm-title">{tr("This plugin installs by running a command", "Este plugin é instalado rodando um comando")}</strong>
        <p>{tr("The marketplace declares the command below. It runs on this machine as you; install only plugins you trust.", "O marketplace declara o comando abaixo. Ele roda nesta máquina com o seu usuário; instale apenas plugins em que você confia.")}</p>
        <pre>{pending.command}</pre>
        <footer>
          <button type="button" onclick={() => (pending = null)}>{tr("Cancel", "Cancelar")}</button>
          <button class="primary" type="button" onclick={confirmPending}>{tr("Run and install", "Rodar e instalar")}</button>
        </footer>
      </div>
    </div>
  {/if}
</section>

<style>
  .extensions { position: absolute; z-index: 31; inset: 0; display: grid; grid-template-columns: 216px minmax(0, 1fr) auto; grid-template-rows: auto minmax(0, 1fr); overflow: hidden; color: var(--workspace-text); background: var(--workspace-bg); isolation: isolate; container-type: inline-size; }
  .extensions-bar { grid-column: 1 / -1; min-width: 0; height: 56px; padding: 0 14px 0 18px; display: flex; align-items: center; gap: 12px; border-bottom: 1px solid var(--workspace-line); }
  .extensions-title { display: flex; align-items: center; gap: 9px; color: var(--workspace-strong); }
  .extensions-title strong { font-size: 14px; font-weight: 720; letter-spacing: -.01em; }
  .title-mark { width: 28px; height: 28px; display: grid; place-items: center; border-radius: 8px; color: var(--workspace-accent); background: var(--workspace-accent-soft); }
  .extensions-tabs { display: inline-flex; gap: 2px; padding: 2px; border-radius: 9px; background: var(--workspace-subtle); }
  .extensions-tabs button { height: 26px; padding: 0 11px; border: 0; border-radius: 7px; color: var(--workspace-muted); background: transparent; font: 650 11px/1 var(--lume-font-ui, Inter, sans-serif); cursor: pointer; }
  .extensions-tabs button.on { color: var(--workspace-strong); background: var(--workspace-raised); box-shadow: 0 1px 3px rgba(20, 38, 30, .12); }
  .extensions-search { min-width: 0; max-width: 340px; height: 32px; margin-left: auto; padding: 0 10px; display: flex; flex: 1; align-items: center; gap: 7px; border: 1px solid var(--workspace-line); border-radius: 9px; color: var(--workspace-faint); background: var(--workspace-raised); }
  .extensions-search:focus-within { border-color: color-mix(in srgb, var(--workspace-accent) 50%, var(--workspace-line)); }
  .extensions-search input { min-width: 0; flex: 1; border: 0; outline: 0; color: var(--workspace-strong); background: transparent; font: 12px var(--lume-font-ui, Inter, sans-serif); }
  .bar-icon { width: 30px; height: 30px; padding: 0; display: grid; place-items: center; flex: 0 0 auto; border: 0; border-radius: 8px; color: var(--workspace-muted); background: transparent; cursor: pointer; }
  .bar-icon:hover:not(:disabled) { color: var(--workspace-accent); background: var(--workspace-subtle); }
  .bar-icon:disabled { opacity: .4; cursor: default; }

  .agent-rail { min-height: 0; padding: 12px 10px; display: flex; flex-direction: column; gap: 3px; overflow-y: auto; border-right: 1px solid var(--workspace-line); }
  .agent-rail button { width: 100%; min-height: 46px; padding: 6px 9px; display: flex; align-items: center; gap: 10px; border: 0; border-radius: 10px; color: var(--workspace-text); background: transparent; text-align: left; cursor: pointer; transition: background 120ms ease; }
  .agent-rail button:hover { background: var(--workspace-subtle); }
  .agent-rail button.on { background: var(--workspace-accent-soft); }
  .agent-rail button span { min-width: 0; display: grid; gap: 2px; }
  .agent-rail strong { color: var(--workspace-strong); font: 680 12px/1.2 var(--lume-font-ui, Inter, sans-serif); }
  .agent-rail small { color: var(--workspace-faint); font: 600 10px/1.2 var(--lume-font-ui, Inter, sans-serif); }
  .rail-project { margin: auto 2px 0; padding: 9px 7px 2px; display: flex; align-items: center; gap: 7px; border-top: 1px solid var(--workspace-line); color: var(--workspace-faint); font: 600 10px/1.35 var(--lume-font-ui, Inter, sans-serif); }
  .rail-project span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .rail-project b { color: var(--workspace-muted); }

  .extensions-main { position: relative; min-width: 0; min-height: 0; padding: 16px 20px 28px; overflow-y: auto; scrollbar-width: thin; scrollbar-color: var(--workspace-scroll-thumb) transparent; }
  .kind-filter { margin-bottom: 14px; display: flex; flex-wrap: wrap; gap: 5px; }
  .kind-filter button { height: 28px; padding: 0 10px; display: inline-flex; align-items: center; gap: 6px; border: 1px solid var(--workspace-line); border-radius: 999px; color: var(--workspace-muted); background: transparent; font: 650 11px/1 var(--lume-font-ui, Inter, sans-serif); cursor: pointer; }
  .kind-filter button:hover { color: var(--workspace-strong); background: var(--workspace-subtle); }
  .kind-filter button.on { border-color: color-mix(in srgb, var(--workspace-accent) 45%, var(--workspace-line)); color: var(--workspace-accent); background: var(--workspace-accent-soft); }
  .kind-filter small { color: var(--workspace-faint); font-size: 10px; font-variant-numeric: tabular-nums; }
  .kind-section { margin-bottom: 20px; }
  .kind-section h3 { margin: 0 0 9px; display: flex; align-items: center; gap: 7px; color: var(--workspace-muted); font: 700 11px/1 var(--lume-font-ui, Inter, sans-serif); letter-spacing: .02em; }
  .kind-section h3 small { color: var(--workspace-faint); font-weight: 650; }
  .card-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(100%, 250px), 1fr)); gap: 8px; }
  .ext-card { min-width: 0; min-height: 104px; padding: 11px 12px 10px; display: grid; grid-template-rows: auto 1fr auto; gap: 6px; border: 1px solid var(--workspace-line); border-radius: 12px; color: var(--workspace-text); background: var(--workspace-raised); text-align: left; font: inherit; transition: border-color 120ms ease, box-shadow 160ms ease, transform 160ms cubic-bezier(.16, 1, .3, 1); }
  button.ext-card { cursor: pointer; }
  button.ext-card:hover { border-color: color-mix(in srgb, var(--workspace-accent) 30%, var(--workspace-line)); box-shadow: 0 6px 18px rgba(20, 38, 30, .08); transform: translateY(-1px); }
  .ext-card.selected { border-color: color-mix(in srgb, var(--workspace-accent) 60%, var(--workspace-line)); box-shadow: 0 0 0 1px color-mix(in srgb, var(--workspace-accent) 40%, transparent); }
  .ext-card.off { opacity: .66; }
  .card-head { min-width: 0; display: flex; align-items: center; gap: 8px; }
  .card-head strong { min-width: 0; flex: 1; overflow: hidden; color: var(--workspace-strong); font: 700 12px/1.25 var(--lume-font-ui, Inter, sans-serif); text-overflow: ellipsis; white-space: nowrap; }
  .card-icon { width: 26px; height: 26px; display: grid; place-items: center; flex: 0 0 auto; border-radius: 7px; color: var(--workspace-accent); background: var(--workspace-accent-soft); }
  .kind-plugin .card-icon, .catalog-card .card-icon { color: #8a63c9; background: color-mix(in srgb, #8a63c9 12%, transparent); }
  .kind-mcp .card-icon { color: #3c8fb0; background: color-mix(in srgb, #3c8fb0 12%, transparent); }
  .kind-instruction .card-icon { color: #b2843a; background: color-mix(in srgb, #b2843a 13%, transparent); }
  .kind-command .card-icon { color: #c06a45; background: color-mix(in srgb, #c06a45 12%, transparent); }
  .kind-subagent .card-icon { color: #4f7fc0; background: color-mix(in srgb, #4f7fc0 12%, transparent); }
  .card-text { display: -webkit-box; overflow: hidden; color: var(--workspace-muted); font: 500 11px/1.45 var(--lume-font-ui, Inter, sans-serif); -webkit-box-orient: vertical; -webkit-line-clamp: 2; line-clamp: 2; overflow-wrap: anywhere; }
  .card-meta { min-width: 0; display: flex; align-items: center; gap: 7px; overflow: hidden; color: var(--workspace-faint); font: 600 10px/1 var(--lume-font-ui, Inter, sans-serif); white-space: nowrap; }
  .card-meta > span:not(.chip):not(.installed-mark) { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .chip { flex: 0 0 auto; padding: 3px 6px; border-radius: 5px; color: var(--workspace-muted); background: var(--workspace-subtle); font: 650 9.5px/1 var(--lume-font-ui, Inter, sans-serif); }
  .chip.muted { color: var(--workspace-faint); }
  .install, .installed-mark { margin-left: auto; flex: 0 0 auto; }
  .installs { display: inline-flex; align-items: center; gap: 4px; }
  .repo-stars { min-width: 0; padding: 2px 0; display: inline-flex; align-items: center; gap: 4px; border: 0; border-radius: 4px; color: var(--workspace-muted); background: transparent; font: inherit; line-height: 1.3; white-space: nowrap; cursor: pointer; }
  .repo-stars svg { flex: 0 0 auto; }
  .repo-stars:hover { color: var(--workspace-accent); text-decoration: underline; text-underline-offset: 3px; }
  .repo-stars:focus-visible { outline: 2px solid var(--workspace-accent); outline-offset: 3px; }
  .marketplace { max-width: 45%; overflow: hidden; color: var(--workspace-faint); font: 600 9.5px/1 var(--lume-font-ui, Inter, sans-serif); text-overflow: ellipsis; white-space: nowrap; }
  .install { height: 24px; padding: 0 9px; display: inline-flex; align-items: center; gap: 5px; border: 0; border-radius: 7px; color: var(--workspace-accent); background: var(--workspace-accent-soft); font: 700 10.5px/1 var(--lume-font-ui, Inter, sans-serif); cursor: pointer; }
  .install:hover:not(:disabled) { background: color-mix(in srgb, var(--workspace-accent) 18%, transparent); }
  .install:disabled { opacity: .55; cursor: default; }
  .installed-mark { display: inline-flex; align-items: center; gap: 4px; color: var(--workspace-accent); }
  .catalog-summary { margin: 0 0 11px; color: var(--workspace-faint); font: 600 11px/1 var(--lume-font-ui, Inter, sans-serif); }
  .catalog-toolbar { min-width: 0; margin-bottom: 11px; display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 8px 12px; }
  .catalog-toolbar .catalog-summary { margin: 0; line-height: 1.4; }
  .official-badge { display: inline-flex; align-items: center; gap: 3px; flex: 0 0 auto; padding: 3px 5px; border-radius: 5px; color: var(--workspace-accent); background: var(--workspace-accent-soft); font: 650 9px/1 var(--lume-font-ui, Inter, sans-serif); white-space: nowrap; }
  .catalog-card .card-meta { min-height: 24px; flex-wrap: wrap; overflow: visible; row-gap: 6px; }
  .catalog-card .card-meta > span, .catalog-card time { white-space: nowrap; }
  .show-more { margin: 14px auto 0; height: 30px; padding: 0 14px; display: block; border: 1px solid var(--workspace-line); border-radius: 9px; color: var(--workspace-muted); background: var(--workspace-raised); font: 650 11px/1 var(--lume-font-ui, Inter, sans-serif); cursor: pointer; }
  .show-more:hover { color: var(--workspace-accent); }
  .state { min-height: 180px; display: flex; align-items: center; justify-content: center; gap: 10px; color: var(--workspace-muted); font: 600 12px/1.5 var(--lume-font-ui, Inter, sans-serif); text-align: center; }
  .state.error, .preview-error { color: #b4574f; }
  .problems { margin: -6px 0 12px; display: flex; align-items: center; gap: 6px; color: #a07a2c; font: 600 10.5px/1.4 var(--lume-font-ui, Inter, sans-serif); }
  .notice { position: sticky; z-index: 2; top: 0; margin: -4px 0 12px; padding: 8px 8px 8px 12px; display: flex; align-items: center; gap: 8px; border: 1px solid var(--workspace-line); border-radius: 10px; background: var(--workspace-raised); box-shadow: 0 6px 18px rgba(20, 38, 30, .1); font: 600 11.5px/1.4 var(--lume-font-ui, Inter, sans-serif); }
  .notice span { flex: 1; }
  .notice.ok { color: var(--workspace-accent); }
  .notice.error { color: #b4574f; }
  .notice button { width: 22px; height: 22px; padding: 0; display: grid; place-items: center; border: 0; border-radius: 6px; color: inherit; background: transparent; cursor: pointer; }
  .spinner { width: 14px; height: 14px; border: 2px solid color-mix(in srgb, var(--workspace-accent) 22%, transparent); border-top-color: var(--workspace-accent); border-radius: 50%; animation: ext-spin 700ms linear infinite; }
  .spinner.small { width: 10px; height: 10px; border-width: 1.5px; }

  .detail { width: min(400px, 38cqw); min-width: 300px; min-height: 0; padding: 14px 16px 18px; display: flex; flex-direction: column; gap: 11px; overflow-y: auto; border-left: 1px solid var(--workspace-line); background: color-mix(in srgb, var(--workspace-raised) 70%, var(--workspace-bg)); scrollbar-width: thin; }
  .detail header { display: flex; align-items: center; gap: 10px; }
  .detail-title { min-width: 0; flex: 1; display: grid; gap: 3px; }
  .detail-title strong { overflow: hidden; color: var(--workspace-strong); font: 720 14px/1.2 var(--lume-font-ui, Inter, sans-serif); text-overflow: ellipsis; white-space: nowrap; }
  .detail-title small { color: var(--workspace-faint); font: 600 10.5px/1.2 var(--lume-font-ui, Inter, sans-serif); }
  .detail-description { margin: 0; color: var(--workspace-text); font: 500 12px/1.55 var(--lume-font-ui, Inter, sans-serif); }
  .detail-facts { margin: 0; display: grid; grid-template-columns: max-content minmax(0, 1fr); gap: 5px 12px; font: 600 11px/1.4 var(--lume-font-ui, Inter, sans-serif); }
  .detail-facts dt { color: var(--workspace-faint); }
  .detail-facts dd { min-width: 0; margin: 0; color: var(--workspace-strong); overflow-wrap: anywhere; }
  .detail-path { padding: 6px 8px; overflow: hidden; border-radius: 7px; color: var(--workspace-muted); background: var(--workspace-subtle); font: 10.5px/1.4 var(--lume-font-code, "SFMono-Regular", Consolas, monospace); text-overflow: ellipsis; white-space: nowrap; }
  .detail-actions { display: flex; flex-wrap: wrap; gap: 6px; }
  .detail-actions button { height: 28px; padding: 0 10px; display: inline-flex; align-items: center; gap: 6px; border: 1px solid var(--workspace-line); border-radius: 8px; color: var(--workspace-strong); background: var(--workspace-raised); font: 650 11px/1 var(--lume-font-ui, Inter, sans-serif); cursor: pointer; }
  .detail-actions button:hover:not(:disabled) { border-color: color-mix(in srgb, var(--workspace-accent) 40%, var(--workspace-line)); color: var(--workspace-accent); }
  .detail-actions button.danger:hover:not(:disabled) { border-color: color-mix(in srgb, #b4574f 45%, var(--workspace-line)); color: #b4574f; }
  .detail-actions button:disabled { opacity: .5; cursor: default; }
  .detail-preview { min-height: 80px; padding: 12px 14px; border: 1px solid var(--workspace-line); border-radius: 11px; background: var(--workspace-raised); }
  .detail-preview pre { margin: 0; overflow-x: auto; color: var(--workspace-text); font: 10.5px/1.55 var(--lume-font-code, "SFMono-Regular", Consolas, monospace); white-space: pre-wrap; overflow-wrap: anywhere; }
  .markdown { color: var(--workspace-text); font: 500 12px/1.6 var(--lume-font-ui, Inter, sans-serif); overflow-wrap: anywhere; }
  .markdown :global(:is(h1, h2, h3, h4)) { margin: 14px 0 6px; color: var(--workspace-strong); font-size: 12.5px; line-height: 1.3; }
  .markdown :global(:is(h1, h2, h3, h4):first-child) { margin-top: 0; }
  .markdown :global(p), .markdown :global(ul), .markdown :global(ol) { margin: 0 0 8px; }
  .markdown :global(ul), .markdown :global(ol) { padding-left: 18px; }
  .markdown :global(code) { padding: 1px 4px; border-radius: 4px; background: var(--workspace-subtle); font: 10.5px var(--lume-font-code, "SFMono-Regular", Consolas, monospace); }
  .markdown :global(pre) { padding: 8px 10px; overflow-x: auto; border-radius: 8px; background: var(--workspace-subtle); }
  .markdown :global(pre code) { padding: 0; background: transparent; }

  .confirm-backdrop { position: absolute; z-index: 5; inset: 0; display: grid; place-items: center; padding: 20px; background: color-mix(in srgb, var(--workspace-bg) 60%, transparent); backdrop-filter: blur(3px); }
  .confirm { width: min(460px, 100%); padding: 18px; display: grid; gap: 10px; border: 1px solid var(--workspace-line); border-radius: 14px; background: var(--workspace-raised); box-shadow: 0 20px 50px rgba(15, 30, 22, .25); }
  .confirm strong { color: var(--workspace-strong); font: 720 13px/1.3 var(--lume-font-ui, Inter, sans-serif); }
  .confirm p { margin: 0; color: var(--workspace-muted); font: 500 12px/1.5 var(--lume-font-ui, Inter, sans-serif); }
  .confirm pre { max-height: 160px; margin: 0; padding: 9px 10px; overflow: auto; border-radius: 8px; color: var(--workspace-strong); background: var(--workspace-subtle); font: 11px/1.5 var(--lume-font-code, "SFMono-Regular", Consolas, monospace); white-space: pre-wrap; overflow-wrap: anywhere; }
  .confirm footer { display: flex; justify-content: flex-end; gap: 7px; }
  .confirm footer button { height: 30px; padding: 0 13px; border: 1px solid var(--workspace-line); border-radius: 8px; color: var(--workspace-strong); background: transparent; font: 650 11.5px/1 var(--lume-font-ui, Inter, sans-serif); cursor: pointer; }
  .confirm footer button.primary { border-color: transparent; color: #fff; background: var(--workspace-accent); }

  :global(.workspace.dark) .extensions-tabs button.on { box-shadow: 0 1px 3px rgba(0, 0, 0, .35); }
  :global(.workspace.dark) button.ext-card:hover { box-shadow: 0 8px 22px rgba(0, 0, 0, .3); }
  :global(.workspace.dark) .kind-plugin .card-icon, :global(.workspace.dark) .catalog-card .card-icon { color: #b394e6; }
  :global(.workspace.dark) .kind-mcp .card-icon { color: #6cb8d6; }
  :global(.workspace.dark) .kind-instruction .card-icon { color: #d9ab5f; }
  :global(.workspace.dark) .kind-command .card-icon { color: #e08c68; }
  :global(.workspace.dark) .kind-subagent .card-icon { color: #7ea6dd; }

  @container (max-width: 760px) {
    .extensions { grid-template-columns: 64px minmax(0, 1fr); }
    .agent-rail button { justify-content: center; padding: 6px; }
    .agent-rail button span, .rail-project span { display: none; }
    .detail { position: absolute; z-index: 3; top: 56px; right: 0; bottom: 0; width: min(380px, 100%); box-shadow: -12px 0 30px rgba(15, 30, 22, .18); }
  }
  @keyframes ext-spin { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) { .ext-card { transition: none; } .spinner { animation-duration: 2s; } }
</style>
