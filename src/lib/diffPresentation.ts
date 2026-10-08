// Presentation helpers for the review center's diff: word-level changes, collapsed unchanged
// regions and a small syntax colorizer. Pure functions, covered by tests/diff-presentation.test.mjs.
import type { ReviewDiffLine } from "./reviewDiffs.ts";

export type Range = [number, number];
export type TokenKind = "kw" | "str" | "com" | "num" | "fn" | "";
export interface Segment { text: string; kind: TokenKind; changed: boolean }

const tokenPattern = /\s+|[A-Za-z0-9_$]+|[^\sA-Za-z0-9_$]/g;

/** Character ranges that differ between two lines: the common head and tail are left out. */
export function changedRanges(before: string, after: string): { before: Range[]; after: Range[] } {
  if (before === after || before.length > 500 || after.length > 500) return { before: [], after: [] };
  const left = before.match(tokenPattern) ?? [];
  const right = after.match(tokenPattern) ?? [];
  let head = 0;
  while (head < left.length && head < right.length && left[head] === right[head]) head += 1;
  let tail = 0;
  while (tail < left.length - head && tail < right.length - head && left[left.length - 1 - tail] === right[right.length - 1 - tail]) tail += 1;
  const length = (tokens: string[], from: number, to: number) => tokens.slice(from, to).reduce((total, token) => total + token.length, 0);
  const beforeStart = length(left, 0, head);
  const afterStart = length(right, 0, head);
  const beforeEnd = before.length - length(left, left.length - tail, left.length);
  const afterEnd = after.length - length(right, right.length - tail, right.length);
  // Lines that share almost nothing read better as plain added/removed lines.
  const shared = Math.min(before.length, after.length) - Math.max(beforeEnd - beforeStart, afterEnd - afterStart);
  if (shared < Math.min(before.length, after.length) * 0.25) return { before: [], after: [] };
  return {
    before: beforeEnd > beforeStart ? [[beforeStart, beforeEnd]] : [],
    after: afterEnd > afterStart ? [[afterStart, afterEnd]] : [],
  };
}

export type FoldRow<T> = { type: "item"; item: T } | { type: "fold"; id: string; count: number };

/** Hides unchanged runs that sit more than `context` items away from a change. */
export function foldUnchanged<T>(items: T[], isChange: (item: T) => boolean, isFoldable: (item: T) => boolean, expanded: ReadonlySet<string>, context = 3): FoldRow<T>[] {
  const near = new Array<boolean>(items.length).fill(false);
  items.forEach((item, index) => {
    if (!isChange(item)) return;
    for (let step = -context; step <= context; step += 1) if (items[index + step]) near[index + step] = true;
  });
  const rows: FoldRow<T>[] = [];
  let index = 0;
  while (index < items.length) {
    if (!isFoldable(items[index]) || near[index]) { rows.push({ type: "item", item: items[index] }); index += 1; continue; }
    let end = index;
    while (end < items.length && isFoldable(items[end]) && !near[end]) end += 1;
    const id = `${index}-${end}`;
    if (end - index < 4 || expanded.has(id)) for (let at = index; at < end; at += 1) rows.push({ type: "item", item: items[at] });
    else rows.push({ type: "fold", id, count: end - index });
    index = end;
  }
  return rows;
}

const keywords = new Set(("as async await break case catch class const continue default def defer del do elif else enum export extends false fi finally fn for from func function if impl import in interface is let lambda loop match mod mut new nil none None null of pub raise readonly return self static struct super switch then this throw trait true try type typeof use var void where while with yield True False echo local").split(" "));

function commentMarker(path: string) {
  const extension = path.split(".").pop()?.toLowerCase() ?? "";
  if (["py", "sh", "bash", "zsh", "rb", "yml", "yaml", "toml", "ps1", "dockerfile", "conf", "env"].includes(extension)) return "#";
  if (["json", "md", "txt", "lock", "svg", "xml"].includes(extension)) return "";
  if (["html", "svelte", "vue"].includes(extension)) return "<!--";
  if (["sql", "lua"].includes(extension)) return "--";
  return "//";
}

/** Colors one line of code; deliberately small (strings, comments, numbers, keywords, calls). */
export function colorize(content: string, path: string): Segment[] {
  const marker = commentMarker(path);
  const segments: Segment[] = [];
  const push = (text: string, kind: TokenKind) => { if (text) segments.push({ text, kind, changed: false }); };
  const pattern = /"(?:\\.|[^"\\])*"?|'(?:\\.|[^'\\])*'?|`(?:\\.|[^`\\])*`?|\b\d[\d_.]*\b|[A-Za-z_$][\w$]*(?=\()|[A-Za-z_$][\w$]*|\s+|./g;
  let position = 0;
  let match: RegExpExecArray | null;
  while ((match = pattern.exec(content))) {
    if (marker && content.startsWith(marker, match.index)) {
      push(content.slice(position, match.index), "");
      push(content.slice(match.index), "com");
      return segments;
    }
    const text = match[0];
    position = match.index + text.length;
    if (/^["'`]/.test(text)) push(text, "str");
    else if (/^\d/.test(text)) push(text, "num");
    else if (keywords.has(text)) push(text, "kw");
    else if (/^[A-Za-z_$]/.test(text) && content[position] === "(") push(text, "fn");
    else push(text, "");
  }
  return segments;
}

/** Splits colored segments so the changed ranges can be highlighted on top of the colors. */
export function withChanges(segments: Segment[], ranges: Range[]): Segment[] {
  if (!ranges.length) return segments;
  const result: Segment[] = [];
  let offset = 0;
  for (const segment of segments) {
    const end = offset + segment.text.length;
    const cuts = [offset, ...ranges.flatMap(([from, to]) => [from, to]).filter((cut) => cut > offset && cut < end).sort((a, b) => a - b), end];
    for (let index = 0; index < cuts.length - 1; index += 1) {
      const from = cuts[index];
      const to = cuts[index + 1];
      result.push({ text: segment.text.slice(from - offset, to - offset), kind: segment.kind, changed: ranges.some(([start, stop]) => from >= start && to <= stop) });
    }
    offset = end;
  }
  return result;
}

export interface DiffCell { line: ReviewDiffLine; segments: Segment[] }
export type SplitPair = { left?: DiffCell; right?: DiffCell };

/** Colored cells for every line; paired removed/added lines also get word-level highlights. */
export function presentLines(lines: ReviewDiffLine[], path: string): DiffCell[] {
  const cells: DiffCell[] = lines.map((line) => ({ line, segments: line.kind === "meta" || line.kind === "hunk" ? [{ text: line.content, kind: "" as TokenKind, changed: false }] : colorize(line.content, path) }));
  let index = 0;
  while (index < lines.length) {
    if (lines[index].kind !== "removed") { index += 1; continue; }
    let removedEnd = index;
    while (lines[removedEnd]?.kind === "removed") removedEnd += 1;
    let addedEnd = removedEnd;
    while (lines[addedEnd]?.kind === "added") addedEnd += 1;
    const pairs = Math.min(removedEnd - index, addedEnd - removedEnd);
    for (let pair = 0; pair < pairs; pair += 1) {
      const removed = cells[index + pair];
      const added = cells[removedEnd + pair];
      const ranges = changedRanges(removed.line.content, added.line.content);
      removed.segments = withChanges(removed.segments, ranges.before);
      added.segments = withChanges(added.segments, ranges.after);
    }
    index = addedEnd;
  }
  return cells;
}

export function pairForSplit(cells: DiffCell[]): SplitPair[] {
  const rows: SplitPair[] = [];
  let removed: DiffCell[] = [];
  let added: DiffCell[] = [];
  const flush = () => {
    const size = Math.max(removed.length, added.length);
    for (let index = 0; index < size; index += 1) rows.push({ left: removed[index], right: added[index] });
    removed = [];
    added = [];
  };
  for (const cell of cells) {
    if (cell.line.kind === "removed") removed.push(cell);
    else if (cell.line.kind === "added") added.push(cell);
    else { flush(); rows.push({ left: cell, right: cell }); }
  }
  flush();
  return rows;
}
