/** What the model picker holds for a conversation: model, reasoning effort and Fast mode. */
export type ControlsSnapshot = { model: string; effort: string; fast: boolean };

/**
 * Whether the picker differs from the settings the conversation opened with, which is what
 * enables "restore the original". Fast only counts for agents that have it.
 */
export function controlsDiffer(
  original: ControlsSnapshot | null,
  current: ControlsSnapshot,
  hasFast: boolean,
): boolean {
  if (!original) return false;
  return original.model !== current.model
    || original.effort !== current.effort
    || (hasFast && original.fast !== current.fast);
}
