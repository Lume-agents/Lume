<script lang="ts">
  import type { Language } from "$lib/i18n";
  import { parseQuestionAnswers } from "$lib/questionAnswers";

  // The person's reply to an agent's question: each question with the answer chosen for it.
  let { text, language = "en" } = $props<{ text: string; language?: Language }>();
  const pt = $derived(language === "pt-BR");
  const answers = $derived(parseQuestionAnswers(text));
</script>

<div class="answer-card">
  <span class="answer-title">
    <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true"><path d="M3 8.5l3 3 7-7" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" /></svg>
    {answers.length > 1 ? (pt ? `${answers.length} respostas` : `${answers.length} answers`) : (pt ? "Resposta" : "Answer")}
  </span>
  <ol>
    {#each answers as item, index (index)}
      <li>
        {#if item.header || item.question}
          <span class="answer-question">{#if item.header}<b>{item.header}</b>{/if}{#if item.question}<span>{item.question}</span>{/if}</span>
        {/if}
        <strong class="answer-value">{item.answer}</strong>
      </li>
    {/each}
  </ol>
</div>

<style>
  .answer-card { min-width: 0; display: grid; gap: 7px; }
  .answer-title { display: inline-flex; align-items: center; gap: 5px; color: var(--workspace-accent, #3f9b69); font-size: var(--workspace-chat-tiny-size, 10px); font-weight: 800; letter-spacing: .05em; text-transform: uppercase; }
  ol { margin: 0; padding: 0; display: grid; gap: 6px; list-style: none; }
  li { min-width: 0; display: grid; gap: 3px; padding: 7px 9px; border-radius: 9px; background: color-mix(in srgb, var(--workspace-accent, #3f9b69) 8%, transparent); border-left: 2px solid color-mix(in srgb, var(--workspace-accent, #3f9b69) 55%, transparent); }
  .answer-question { min-width: 0; display: flex; flex-wrap: wrap; align-items: baseline; gap: 2px 7px; color: var(--workspace-muted, #61736a); font-size: var(--workspace-chat-small-size, 11px); line-height: 1.4; overflow-wrap: anywhere; }
  .answer-question b { color: var(--workspace-faint, #89958f); font-size: var(--workspace-chat-tiny-size, 10px); font-weight: 800; letter-spacing: .05em; text-transform: uppercase; }
  .answer-value { color: var(--workspace-strong, #52665c); font-size: var(--workspace-chat-font-size, 12.5px); font-weight: 700; line-height: 1.4; overflow-wrap: anywhere; white-space: pre-wrap; }
</style>
