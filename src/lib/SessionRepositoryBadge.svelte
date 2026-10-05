<script lang="ts">
  import LumeIcon from "$lib/LumeIcon.svelte";
  import type { AgentSession } from "$lib/domain";
  import { observeRepository, type RepositoryState } from "$lib/repositories";
  let { session, language = "en", onOpen } = $props<{ session: AgentSession; language?: "en" | "pt-BR"; onOpen?: () => void }>();
  let repository = $state<RepositoryState>({ loading: true, value: null, error: "" });
  $effect(() => observeRepository({ id: session.id, nativeSessionId: session.nativeSessionId, workingDirectory: session.workingDirectory }).subscribe((value) => { repository = value; }));
</script>

{#if repository.value}
  <button class="session-repository" type="button" title={`${repository.value.root} · ${repository.value.branch}`} aria-label={`${language === "pt-BR" ? "Inspecionar repositório" : "Inspect repository"}: ${repository.value.name}`} onclick={onOpen}>
    <LumeIcon name="branch" size={12} /><span>{repository.value.github?.split("/").at(-1) ?? repository.value.name}</span><span class="branch-name">{repository.value.branch === "(detached)" ? repository.value.head?.slice(0, 7) : repository.value.branch}</span>{#if repository.value.files.length}<i title={language === "pt-BR" ? "Mudanças na árvore de trabalho" : "Working tree changes"}></i>{/if}
  </button>
{/if}

<style>
  .session-repository { min-width: 0; max-width: 100%; display: inline-flex; align-items: center; gap: 5px; align-self: start; padding: 0; border: 0; background: transparent; color: var(--workspace-muted); font: inherit; font-size: 9px; text-align: left; cursor: pointer; }
  .session-repository > span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .session-repository .branch-name { color: var(--workspace-accent); }
  .session-repository > i { width: 4px; height: 4px; flex: 0 0 auto; border-radius: 50%; background: var(--workspace-accent); }
  .session-repository:hover { color: var(--workspace-accent); }
  .session-repository:focus-visible { outline: 2px solid var(--workspace-accent); outline-offset: 3px; border-radius: 2px; }
</style>
