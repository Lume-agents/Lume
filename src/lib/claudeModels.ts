import type { CodexModelOption } from "$lib/lume";

export function claudeModelOptions(
  models: CodexModelOption[],
  translate: (english: string, portuguese: string) => string,
) {
  return models.map((option) => ({
    value: option.model,
    label: option.displayName,
    description: option.isDefault
      ? translate("Claude Code default", "Padrão do Claude Code")
      : option.description,
  }));
}

export function claudeEffortValues(models: CodexModelOption[], model: string): string[] {
  return models
    .find((option) => option.model === model)
    ?.supportedReasoningEfforts.map((effort) => effort.value) ?? [];
}

/** Keeps the chosen effort when the new model supports it, otherwise its default. */
export function claudeEffortForModel(models: CodexModelOption[], model: string, effort: string): string {
  const option = models.find((candidate) => candidate.model === model);
  if (!option) return effort;
  if (option.supportedReasoningEfforts.some((supported) => supported.value === effort)) return effort;
  return option.defaultReasoningEffort;
}
