/** Time between reveal steps, in milliseconds. */
export const STREAM_STEP_MS = 90;
const MIN_STEPS = 6;
const MAX_STEPS = 30;
const CHARS_PER_STEP = 22;

/**
 * How many steps a backlog of text should take to reveal. It grows with the
 * text but is capped, so a long answer never keeps what comes after it waiting.
 * At 90 ms a step that is about 0.5 s for a short sentence and 2.7 s at most.
 */
export function stepsForBacklog(characters: number): number {
  return Math.min(MAX_STEPS, Math.max(MIN_STEPS, Math.ceil(characters / CHARS_PER_STEP)));
}

/** Words to reveal now so the backlog is done when the planned steps run out. */
export function wordsForStep(wordsLeft: number, stepsLeft: number): number {
  return Math.max(1, Math.ceil(wordsLeft / Math.max(1, stepsLeft)));
}

/** The longest anything below a streaming message is held back, in milliseconds. */
export const MAX_HOLD_MS = 4_000;
