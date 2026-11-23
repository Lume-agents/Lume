<script lang="ts">
  import type { Language } from "$lib/i18n";
  import { promptIntentLabel, promptIntents, type PromptIntent } from "$lib/promptIntent";

  // The next prompt asks the agent for a to-do list, a goal or a plan first; one at a time, and only for that prompt.
  let { intent = $bindable(null), language = "en", disabled = false } = $props<{ intent?: PromptIntent | null; language?: Language; disabled?: boolean }>();
  const pt = $derived(language === "pt-BR");
  const hints: Record<PromptIntent, [string, string]> = {
    todo: ["Ask for a to-do list before the work starts", "Pedir uma lista de tarefas antes de começar"],
    goal: ["Make this prompt the chat's goal", "Tornar este prompt o objetivo do chat"],
    planning: ["Ask for a plan and wait for approval", "Pedir um plano e aguardar aprovação"],
  };
</script>

<div class="intent-chips" role="group" aria-label={pt ? "Pedir antes de trabalhar" : "Ask before working"}>
  {#each promptIntents as option (option)}
    <button type="button" class:active={intent === option} aria-pressed={intent === option} {disabled}
      title={hints[option][Number(pt)]} onclick={() => (intent = intent === option ? null : option)}>
      {promptIntentLabel(option, pt)}
    </button>
  {/each}
</div>

<style>
  .intent-chips { display: inline-flex; align-items: center; gap: 2px; }
  button { height: 22px; padding: 0 7px; border: 1px solid transparent; border-radius: 7px; color: var(--workspace-muted, #61736a); background: transparent; font: 680 10px/1 var(--lume-font-ui, Inter, sans-serif); cursor: pointer; transition: color 140ms ease, background 140ms ease, border-color 140ms ease; }
  button:hover:not(:disabled) { color: var(--workspace-strong, #52665c); background: var(--workspace-subtle, rgba(65, 94, 80, .08)); }
  button.active { color: var(--workspace-accent, #3f9b69); border-color: color-mix(in srgb, var(--workspace-accent, #3f9b69) 40%, transparent); background: color-mix(in srgb, var(--workspace-accent, #3f9b69) 12%, transparent); }
  button:focus-visible { outline: 2px solid var(--workspace-accent, #3f9b69); outline-offset: 1px; }
  button:disabled { opacity: .45; cursor: default; }
</style>
