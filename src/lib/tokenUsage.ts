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
export function tokenGraphGeometry(samples: PromptTokenUsage[]): TokenGraph {
  if (!samples.length) return { points: "", area: "", dots: [] };
  const width = TOKEN_GRAPH_WIDTH;
  const height = TOKEN_GRAPH_HEIGHT;
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

/** 1.2k, 34k, 1.1M: short enough for a header button. */
export function compactTokens(value: number, locale = "en"): string {
  return new Intl.NumberFormat(locale, { notation: "compact", maximumFractionDigits: value < 10_000 ? 1 : 0 }).format(value);
}
