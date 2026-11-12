import type { SessionNote } from "$lib/domain";

/**
 * The Notes tab's filtering and the composer's attached notes. Pure: the components only draw this.
 */

export type NoteFilters = {
  /** "" for every agent. */
  agent?: string;
  /** "" for every tag. */
  tag?: string;
};

/** Notes that match the agent and tag chosen; the project, kind and text are narrowed by the backend. */
export function filterNotes(notes: SessionNote[], filters: NoteFilters): SessionNote[] {
  return notes.filter((note) =>
    (!filters.agent || note.agent === filters.agent)
    && (!filters.tag || (note.tags ?? []).includes(filters.tag)));
}

/** Every tag in use, most used first, then by name. */
export function noteTags(notes: SessionNote[]): string[] {
  const counts = new Map<string, number>();
  for (const note of notes) for (const tag of note.tags ?? []) counts.set(tag, (counts.get(tag) ?? 0) + 1);
  return [...counts].sort((left, right) => right[1] - left[1] || left[0].localeCompare(right[0])).map(([tag]) => tag);
}

/** `#release, ideas  todo` -> ["release", "ideas", "todo"]: the backend normalizes the same way. */
export function parseNoteTags(text: string): string[] {
  const tags: string[] = [];
  for (const part of text.split(/[\s,]+/)) {
    const tag = part.replace(/^#+/, "").trim().toLowerCase().slice(0, 32);
    if (tag && !tags.includes(tag) && tags.length < 12) tags.push(tag);
  }
  return tags;
}

/** The last folder of a project path, as the sidebar names projects. */
export function projectLabel(project: string | undefined): string {
  if (!project) return "";
  return project.split(/[\\/]/).filter(Boolean).at(-1) ?? project;
}

/** A first line to show in the list when the note has no distinct title. */
export function noteExcerpt(body: string, limit = 120): string {
  const line = body.replace(/\[[ xX]\]/g, " ").replace(/[#>*_`[\]-]+/g, " ").replace(/\s+/g, " ").trim();
  return line.length > limit ? `${line.slice(0, limit - 1).trimEnd()}…` : line;
}

/** A note's title from the start of what it saves, for "Save to note". */
export function noteTitleFrom(text: string, fallback: string): string {
  const line = text.split("\n").map((value) => value.replace(/^[#>*\-\s]+/, "").trim()).find(Boolean) ?? "";
  if (!line) return fallback;
  return line.length > 80 ? `${line.slice(0, 79).trimEnd()}…` : line;
}

/** Sent on `window` when a chat saves a note, so the Inspector's list reloads. */
export const NOTES_CHANGED_EVENT = "lume:notes-changed";
