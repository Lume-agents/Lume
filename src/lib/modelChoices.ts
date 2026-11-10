import type { CodexModelOption } from "$lib/lume";

/** The reasoning efforts a model accepts, in the order its agent lists them. */
export function effortsFor(models: CodexModelOption[], model: string): string[] {
  return models.find((option) => option.model === model)?.supportedReasoningEfforts.map((effort) => effort.value) ?? [];
}

/** The effort to use after the model changed: the one already chosen when the new model has it, else the model's default. */
export function effortAfterModelChange(models: CodexModelOption[], model: string, effort: string): string {
  const option = models.find((candidate) => candidate.model === model);
  if (!option) return effort;
  if (option.supportedReasoningEfforts.some((supported) => supported.value === effort)) return effort;
  return option.defaultReasoningEffort || option.supportedReasoningEfforts[0]?.value || "";
}
