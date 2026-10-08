/** The agent groups of the Workspace sidebar, stored on this computer and shared by every Lume window. */
export type SidebarGroup = { id: string; name: string; collapsed: boolean };
export type SidebarGroupState = { groups: SidebarGroup[]; assignments: Record<string, string>; order: string[] };

export const sidebarGroupsKey = "lume:sidebar-groups:v1";

export function readSidebarGroups(): SidebarGroupState {
  try {
    const raw = JSON.parse(localStorage.getItem(sidebarGroupsKey) || "{}");
    return {
      groups: Array.isArray(raw.groups)
        ? raw.groups
          .filter((group: SidebarGroup) => typeof group?.id === "string" && typeof group?.name === "string")
          .map((group: SidebarGroup) => ({ id: group.id, name: group.name, collapsed: Boolean(group.collapsed) }))
        : [],
      assignments: raw.assignments && typeof raw.assignments === "object" ? raw.assignments : {},
      order: Array.isArray(raw.order) ? raw.order.filter((id: unknown) => typeof id === "string") : [],
    };
  } catch {
    return { groups: [], assignments: {}, order: [] };
  }
}

export function writeSidebarGroups(state: SidebarGroupState) {
  try { localStorage.setItem(sidebarGroupsKey, JSON.stringify(state)); }
  catch { /* Groups still work for this run when storage is unavailable. */ }
}

/** Group ids and `none` (the ungrouped list) in the order the Workspace shows them. */
export function orderedSections(state: SidebarGroupState): string[] {
  const valid = new Set(["none", ...state.groups.map((group) => group.id)]);
  const order = state.order.filter((id) => valid.has(id));
  const next = [...order, ...state.groups.map((group) => group.id).filter((id) => !order.includes(id))];
  if (!next.includes("none")) next.unshift("none");
  return next;
}

export type SessionSection<T> = { key: string; group: SidebarGroup | null; sessions: T[] };

/**
 * Splits sessions into the sidebar's sections. Without groups there is one plain section.
 * Empty groups are left out, and "No group" only appears next to at least one group.
 */
export function groupSessions<T extends { id: string; nativeSessionId?: string | null }>(
  sessions: T[],
  state: SidebarGroupState,
): SessionSection<T>[] {
  if (!state.groups.length) return [{ key: "all", group: null, sessions }];
  const members = new Map<string, T[]>(state.groups.map((group) => [group.id, []]));
  const loose: T[] = [];
  for (const session of sessions) {
    const list = members.get(state.assignments[session.nativeSessionId || session.id]);
    if (list) list.push(session); else loose.push(session);
  }
  const sections: SessionSection<T>[] = [];
  for (const id of orderedSections(state)) {
    if (id === "none") {
      if (loose.length) sections.push({ key: "none", group: null, sessions: loose });
      continue;
    }
    const group = state.groups.find((item) => item.id === id);
    const list = members.get(id) ?? [];
    if (group && list.length) sections.push({ key: id, group, sessions: list });
  }
  return sections;
}
