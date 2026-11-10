<script lang="ts">
  import LumeSelect from "$lib/LumeSelect.svelte";
  import { loadModelControls, saveModelControls, supportsModelControls, type ModelControls } from "$lib/agentModelControls";
  import type { Language } from "$lib/i18n";
  import { effortName } from "$lib/interruptNotice";
  import { effortAfterModelChange, effortsFor } from "$lib/modelChoices";

  /** The model and reasoning effort of one conversation, changed where it is read (the workflow board's step panel). */
  let { sessionId, agent, controlledByLume, working = false, locked = false, language = "en" } = $props<{
    sessionId: string;
    agent: string;
    /** Codex and OpenCode only answer for conversations Lume controls. */
    controlledByLume: boolean;
    /** The agent is in the middle of a turn. */
    working?: boolean;
    /** The workflow is running, so nothing about its steps changes. */
    locked?: boolean;
    language?: Language;
  }>();
  const tr = (english: string, portuguese: string) => (language === "pt-BR" ? portuguese : english);

  let controls = $state<ModelControls | null>(null);
  let loading = $state(false);
  let saving = $state(false);
  let error = $state("");

  const supported = $derived(supportsModelControls(agent));
  // Claude Code reads its settings for any conversation; the others only for the ones Lume controls.
  const readable = $derived(supported && (agent === "claude_code" || controlledByLume));
  const blocked = $derived(!controlledByLume || locked || loading || saving || (agent === "claude_code" && working));
  const efforts = $derived(controls ? effortsFor(controls.models, controls.model) : []);

  $effect(() => {
    const id = sessionId;
    if (!readable) { controls = null; return; }
    let cancelled = false;
    loading = true;
    error = "";
    void loadModelControls(id, agent)
      .then((value) => { if (!cancelled) controls = value; })
      .catch((reason) => { if (!cancelled) error = String(reason).replace(/^Error:\s*/, ""); })
      .finally(() => { if (!cancelled) loading = false; });
    return () => { cancelled = true; };
  });

  async function change(model: string, effort: string) {
    if (!controls || blocked) return;
    saving = true;
    error = "";
    try {
      controls = await saveModelControls(sessionId, agent, model, effort);
    } catch (reason) {
      error = String(reason).replace(/^Error:\s*/, "");
    } finally {
      saving = false;
    }
  }
</script>

{#if supported}
  <div class="model-picker" aria-busy={loading || saving}>
    {#if controls}
      <div class="picker-row">
        <LumeSelect
          value={controls.model}
          options={controls.models.map((option) => ({ value: option.model, label: option.displayName, description: option.description }))}
          ariaLabel={tr("Model", "Modelo")}
          disabled={blocked}
          minWidth={150}
          menuMinWidth={230}
          onValueChange={(model) => void change(model, effortAfterModelChange(controls!.models, model, controls!.effort))}
        />
        {#if efforts.length}
          <LumeSelect
            value={controls.effort}
            options={efforts.map((value) => ({ value, label: effortName(value, tr) }))}
            ariaLabel={tr("Reasoning effort", "Esforço de raciocínio")}
            disabled={blocked}
            minWidth={96}
            onValueChange={(effort) => void change(controls!.model, effort)}
          />
        {/if}
      </div>
    {:else if loading}
      <p class="picker-note">{tr("Reading the model…", "Lendo o modelo…")}</p>
    {:else if !readable}
      <p class="picker-note">{tr("Take control of this conversation to change its model.", "Assuma o controle desta conversa para mudar o modelo.")}</p>
    {/if}
    {#if controls && agent === "claude_code" && working}
      <p class="picker-note">{tr("The model can be changed after the current task finishes.", "O modelo pode ser mudado depois que a tarefa atual terminar.")}</p>
    {/if}
    {#if error}<p class="picker-note error" role="alert">{error}</p>{/if}
  </div>
{/if}

<style>
  .model-picker { display: grid; gap: 6px; }
  .picker-row { display: flex; flex-wrap: wrap; gap: 6px; }
  .picker-note { margin: 0; color: var(--workspace-muted); font-size: 11px; line-height: 1.45; }
  .picker-note.error { color: #c0554f; }
</style>
