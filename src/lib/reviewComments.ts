// Line comments made while reviewing a result, and the request they become for the agent.
export type CommentSide = "old" | "new";

export interface ReviewComment {
  id: string;
  path: string;
  side: CommentSide;
  line: number;
  body: string;
  /** The code the comment points at, so the agent sees it without opening the file. */
  excerpt: string;
  createdAt: number;
  sentAt?: number;
}

export const commentKey = (path: string, side: CommentSide, line: number) => `${path}|${side}|${line}`;

export function indexComments(comments: ReviewComment[]): Map<string, ReviewComment[]> {
  const index = new Map<string, ReviewComment[]>();
  for (const comment of comments) {
    const key = commentKey(comment.path, comment.side, comment.line);
    index.set(key, [...(index.get(key) ?? []), comment]);
  }
  return index;
}

export function parseStoredComments(raw: string | null): ReviewComment[] {
  try {
    const value = JSON.parse(raw ?? "[]");
    if (!Array.isArray(value)) return [];
    return value.filter((item): item is ReviewComment => Boolean(item) && typeof item.id === "string" && typeof item.path === "string" && typeof item.body === "string" && Number.isFinite(item.line) && (item.side === "old" || item.side === "new"));
  } catch {
    return [];
  }
}

/** Comments grouped by file and ordered by line, as they read in a review. */
export function groupByFile(comments: ReviewComment[]): Array<{ path: string; comments: ReviewComment[] }> {
  const groups = new Map<string, ReviewComment[]>();
  for (const comment of comments) groups.set(comment.path, [...(groups.get(comment.path) ?? []), comment]);
  return [...groups.entries()].map(([path, items]) => ({ path, comments: [...items].sort((left, right) => left.line - right.line || left.createdAt - right.createdAt) }));
}

/** The text appended to a correction request: one entry per comment with file, line and code. */
export function formatCommentsForPrompt(comments: ReviewComment[]): string {
  if (!comments.length) return "";
  const lines = ["Line comments:"];
  for (const { path, comments: items } of groupByFile(comments)) {
    for (const comment of items) {
      const where = `${path}:${comment.line}${comment.side === "old" ? " (before the change)" : ""}`;
      lines.push("", `- ${where}`);
      if (comment.excerpt.trim()) lines.push(`  Code: ${comment.excerpt.trim().slice(0, 200)}`);
      lines.push(`  Comment: ${comment.body.trim().replace(/\n+/g, "\n    ")}`);
    }
  }
  return lines.join("\n");
}
