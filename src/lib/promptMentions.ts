/** The `@` token being typed at the caret, as agent CLIs complete file paths. */
export type MentionQuery = { start: number; query: string };

export function mentionAtCaret(text: string, caret: number): MentionQuery | null {
  const before = text.slice(0, caret);
  const match = /(^|\s)@([^\s@]*)$/.exec(before);
  if (!match) return null;
  return { start: before.length - match[2].length - 1, query: match[2] };
}

/**
 * Goes up one folder inside the `@` token: `@src/lib/` becomes `@src/`, and `@src/` the top level `@`.
 * Null when the token has no folder to leave, so the arrow key keeps moving the caret.
 */
export function parentMention(
  text: string,
  caret: number,
  mention: MentionQuery,
): { text: string; caret: number } | null {
  const folder = mention.query.replace(/\/+$/, "");
  if (!mention.query.includes("/")) return null;
  const parent = folder.includes("/") ? folder.slice(0, folder.lastIndexOf("/") + 1) : "";
  const tokenEnd = mention.start + 1 + mention.query.length;
  if (caret !== tokenEnd) return null;
  const value = `${text.slice(0, mention.start + 1)}${parent}${text.slice(tokenEnd)}`;
  return { text: value, caret: mention.start + 1 + parent.length };
}

/** Replaces the typed token with `@path`; files get a trailing space, folders keep completing. */
export function applyMention(
  text: string,
  caret: number,
  mention: MentionQuery,
  path: string,
  isDirectory: boolean,
): { text: string; caret: number } {
  const after = text.slice(caret).replace(/^[^\s]*/, "");
  const inserted = `@${path}${isDirectory || after.startsWith(" ") ? "" : " "}`;
  const value = `${text.slice(0, mention.start)}${inserted}${after}`;
  return { text: value, caret: mention.start + inserted.length };
}
