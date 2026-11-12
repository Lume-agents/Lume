import type { PromptTokenUsage } from "$lib/domain";

/**
 * The per-prompt token chart shared by the Inspector and the chat header, and
 * the totals shown next to it. Pure: the chart component only draws this.
 */

export const TOKEN_GRAPH_WIDTH = 226;
export const TOKEN_GRAPH_HEIGHT = 54;

export type TokenGraphDot = { x: number; y: number; sample: PromptTokenUsage };
export type TokenGraph = { points: string; area: string; dots: TokenGraphDot[] };

/** The line, the filled area and one dot per prompt, spread over the time between the first and the last. */
export function tokenGraphGeometry(samples: PromptTokenUsage[], width = TOKEN_GRAPH_WIDTH, height = TOKEN_GRAPH_HEIGHT): TokenGraph {
  if (!samples.length) return { points: "", area: "", dots: [] };
  const insetX = 4;
  const insetTop = 12;
  const insetBottom = 10;
  const firstTime = samples[0]?.createdAt ?? 0;
  const lastTime = samples.at(-1)?.createdAt ?? firstTime;
  const timeSpan = Math.max(1, lastTime - firstTime);
  const maximum = Math.max(1, ...samples.map((sample) => sample.totalTokens));
  const dots = samples.map((sample, index) => ({
    x: samples.length === 1 ? width / 2 : insetX + ((sample.createdAt - firstTime) / timeSpan || index / (samples.length - 1)) * (width - insetX * 2),
    y: height - insetBottom - (sample.totalTokens / maximum) * (height - insetTop - insetBottom),
    sample,
  }));
  const points = dots.length === 1
    ? `${insetX},${dots[0].y.toFixed(1)} ${width - insetX},${dots[0].y.toFixed(1)}`
    : dots.map((dot) => `${dot.x.toFixed(1)},${dot.y.toFixed(1)}`).join(" ");
  const area = `${insetX},${height - insetBottom} ${points} ${width - insetX},${height - insetBottom}`;
  return { points, area, dots };
}

export type TokenBar = {
  x: number;
  width: number;
  /** Top of the input part; the output part sits on it. */
  inputY: number;
  inputHeight: number;
  outputY: number;
  outputHeight: number;
  sample: PromptTokenUsage;
};

/**
 * One bar per prompt, input at the bottom and output stacked on it, all scaled to the largest prompt.
 * Bars are spaced by order, not time, so a burst of short prompts stays readable.
 */
export function tokenBarsGeometry(samples: PromptTokenUsage[], width = TOKEN_GRAPH_WIDTH, height = TOKEN_GRAPH_HEIGHT): TokenBar[] {
  if (!samples.length) return [];
  const insetX = 4;
  const insetTop = 12;
  const insetBottom = 10;
  const slot = (width - insetX * 2) / samples.length;
  const barWidth = Math.max(1, Math.min(18, slot * 0.66));
  const maximum = Math.max(1, ...samples.map((sample) => Math.max(sample.totalTokens, sample.inputTokens + sample.outputTokens)));
  const scale = (height - insetTop - insetBottom) / maximum;
  const base = height - insetBottom;
  return samples.map((sample, index) => {
    const inputHeight = Math.max(0, sample.inputTokens) * scale;
    const outputHeight = Math.max(0, sample.outputTokens) * scale;
    return {
      x: insetX + slot * index + slot / 2,
      width: barWidth,
      inputY: base - inputHeight,
      inputHeight,
      outputY: base - inputHeight - outputHeight,
      outputHeight,
      sample,
    };
  });
}

export type ContextFillPoint = { x: number; y: number; percent: number; sample: PromptTokenUsage };

/** How full the context was after each prompt that reports it, on a 0–100% scale, spaced by order. */
export function contextFillGeometry(
  samples: PromptTokenUsage[],
  width = TOKEN_GRAPH_WIDTH,
  height = 36,
  percentOf: (sample: PromptTokenUsage) => number | null = contextPercent,
): { points: string; dots: ContextFillPoint[] } {
  const known = samples
    .map((sample) => ({ sample, percent: percentOf(sample) }))
    .filter((item): item is { sample: PromptTokenUsage; percent: number } => item.percent !== null);
  if (!known.length) return { points: "", dots: [] };
  const insetX = 4;
  const insetY = 4;
  const dots = known.map(({ sample, percent }, index) => ({
    x: known.length === 1 ? width / 2 : insetX + (index / (known.length - 1)) * (width - insetX * 2),
    y: height - insetY - (percent / 100) * (height - insetY * 2),
    percent,
    sample,
  }));
  const points = dots.length === 1
    ? `${insetX},${dots[0].y.toFixed(1)} ${width - insetX},${dots[0].y.toFixed(1)}`
    : dots.map((dot) => `${dot.x.toFixed(1)},${dot.y.toFixed(1)}`).join(" ");
  return { points, dots };
}

/** How full the context was when the prompt ended, 0–100, or null when the agent does not report it. */
export function contextPercent(sample: PromptTokenUsage | undefined): number | null {
  if (!sample?.contextTokens || !sample.contextWindow) return null;
  return Math.max(0, Math.min(100, Math.round((sample.contextTokens / sample.contextWindow) * 100)));
}

export type TokenUsageSummary = {
  prompts: number;
  totalTokens: number;
  inputTokens: number;
  outputTokens: number;
  last: PromptTokenUsage | null;
  /** The context use after the latest prompt, when known. */
  context: number | null;
};

export function tokenUsageSummary(samples: PromptTokenUsage[]): TokenUsageSummary {
  const sum = (key: "totalTokens" | "inputTokens" | "outputTokens") => samples.reduce((total, sample) => total + (sample[key] || 0), 0);
  const last = samples.at(-1) ?? null;
  return { prompts: samples.length, totalTokens: sum("totalTokens"), inputTokens: sum("inputTokens"), outputTokens: sum("outputTokens"), last, context: contextPercent(last ?? undefined) };
}

export type TokenMetricStats = {
  prompts: number;
  totalTokens: number;
  averageTokens: number;
  largest: { sample: PromptTokenUsage; index: number } | null;
  /** Context use after the latest prompt, when reported. */
  context: number | null;
  /** Share of input read from the prompt cache, 0–100, over the prompts that report it; null when none do. */
  cacheHitPercent: number | null;
  /** Mean time from sending a prompt to its last response, over the prompts that know when they were sent. */
  averageDurationMs: number | null;
  /** The newest model named by a prompt. */
  model: string | null;
};

/** The Inspector's metric tiles. Every optional figure is null when no prompt reports it, never estimated. */
export function tokenMetricStats(samples: PromptTokenUsage[]): TokenMetricStats {
  const summary = tokenUsageSummary(samples);
  let largest: TokenMetricStats["largest"] = null;
  samples.forEach((sample, index) => { if (!largest || sample.totalTokens > largest.sample.totalTokens) largest = { sample, index }; });
  const cached = samples.filter((sample) => typeof sample.cacheReadTokens === "number" && sample.inputTokens > 0);
  const cachedInput = cached.reduce((total, sample) => total + sample.inputTokens, 0);
  const cacheHitPercent = cachedInput > 0
    ? Math.round((cached.reduce((total, sample) => total + (sample.cacheReadTokens ?? 0), 0) / cachedInput) * 100)
    : null;
  const durations = samples
    .filter((sample) => typeof sample.promptAt === "number" && sample.promptAt > 0 && sample.createdAt >= sample.promptAt)
    .map((sample) => sample.createdAt - (sample.promptAt ?? 0));
  return {
    prompts: samples.length,
    totalTokens: summary.totalTokens,
    averageTokens: samples.length ? Math.round(summary.totalTokens / samples.length) : 0,
    largest,
    context: summary.context,
    cacheHitPercent: cacheHitPercent === null ? null : Math.max(0, Math.min(100, cacheHitPercent)),
    averageDurationMs: durations.length ? Math.round(durations.reduce((total, value) => total + value, 0) / durations.length) : null,
    model: samples.findLast((sample) => sample.model?.trim())?.model?.trim() ?? null,
  };
}

/** 4s, 1m 12s, 1h 5m. */
export function compactDuration(milliseconds: number): string {
  const seconds = Math.max(0, Math.round(milliseconds / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return seconds % 60 ? `${minutes}m ${seconds % 60}s` : `${minutes}m`;
  return minutes % 60 ? `${Math.floor(minutes / 60)}h ${minutes % 60}m` : `${Math.floor(minutes / 60)}h`;
}

/**
 * A request to scroll a chat to one of its prompts: by activity id (a note's link), or by the token sample
 * it produced, resolved against the prompts the chat can read.
 */
export type PromptJumpRequest = {
  sessionId: string;
  /** Changes on every request, so asking for the same prompt twice still scrolls. */
  nonce: number;
  activityId?: string;
  /** When the sample's turn ended. */
  sampleAt?: number;
  /** When the previous sample's turn ended: the prompt came after it. */
  after?: number;
  /** When the prompt was sent, if the agent reported it. */
  promptAt?: number;
};

export type PromptJumpTarget = Omit<PromptJumpRequest, "sessionId" | "nonce">;

/** The jump to the prompt behind a token sample, which follows the sample before it. */
export function promptJumpForSample(samples: PromptTokenUsage[], index: number): PromptJumpTarget | null {
  const sample = samples[index];
  if (!sample) return null;
  return { sampleAt: sample.createdAt, after: samples[index - 1]?.createdAt, promptAt: sample.promptAt };
}

/** A prompt can be logged a little before or after the time the agent reports for it. */
const PROMPT_AT_TOLERANCE = 120_000;

/**
 * The prompt a jump points at, among prompts in any order. With the prompt's own time, the closest prompt to it;
 * otherwise the last prompt before the turn ended and after the previous turn did. Null when none fits.
 */
export function resolvePromptJump<T extends { id: string; createdAt: number }>(request: PromptJumpTarget, prompts: T[]): T | null {
  if (request.activityId) return prompts.find((prompt) => prompt.id === request.activityId) ?? null;
  if (typeof request.promptAt === "number") {
    let best: T | null = null;
    for (const prompt of prompts) {
      const distance = Math.abs(prompt.createdAt - request.promptAt);
      if (distance <= PROMPT_AT_TOLERANCE && (!best || distance < Math.abs(best.createdAt - request.promptAt))) best = prompt;
    }
    if (best) return best;
  }
  if (typeof request.sampleAt !== "number") return null;
  let match: T | null = null;
  for (const prompt of prompts) {
    if (prompt.createdAt > request.sampleAt) continue;
    if (typeof request.after === "number" && prompt.createdAt <= request.after) continue;
    if (!match || prompt.createdAt > match.createdAt) match = prompt;
  }
  return match;
}

/** Whether older prompts could still hold the target: the oldest prompt read is not yet before its window. */
export function promptJumpNeedsOlder(request: PromptJumpTarget, oldestRead: number | undefined): boolean {
  if (oldestRead === undefined || request.activityId) return true;
  const floor = typeof request.promptAt === "number" ? request.promptAt - PROMPT_AT_TOLERANCE : request.after ?? -Infinity;
  return oldestRead > floor;
}

/** 1.2k, 34k, 1.1M: short enough for a header button. */
export function compactTokens(value: number, locale = "en"): string {
  return new Intl.NumberFormat(locale, { notation: "compact", maximumFractionDigits: value < 10_000 ? 1 : 0 }).format(value);
}
