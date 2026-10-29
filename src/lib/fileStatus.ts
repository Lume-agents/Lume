import type { RepositoryFile } from "$lib/repositories";

export type FileStatus = "added" | "modified" | "deleted" | "renamed" | "conflict" | "staged";

/** A working-tree file's state, from `git status --porcelain` columns. */
export function workingStatus(file: Pick<RepositoryFile, "index" | "worktree" | "conflict" | "untracked">): FileStatus {
  if (file.conflict) return "conflict";
  if (file.untracked || file.index === "A") return "added";
  const codes = `${file.index}${file.worktree}`;
  if (codes.includes("D")) return "deleted";
  if (codes.includes("R")) return "renamed";
  return file.worktree === "." ? "staged" : "modified";
}

/** A committed file's state, from `git show --name-status`. */
export function commitStatus(code: string): FileStatus {
  return ({ A: "added", D: "deleted", R: "renamed", C: "added" } as Record<string, FileStatus>)[code] ?? "modified";
}

export function statusLabel(status: FileStatus, portuguese: boolean) {
  const labels: Record<FileStatus, [string, string]> = {
    added: ["New", "Novo"], modified: ["Modified", "Modificado"], deleted: ["Deleted", "Removido"],
    renamed: ["Renamed", "Renomeado"], conflict: ["Conflict", "Conflito"], staged: ["Staged", "Preparado"],
  };
  return labels[status][Number(portuguese)];
}
