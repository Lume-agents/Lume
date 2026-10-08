import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { get, writable, type Readable } from "svelte/store";
import type { AgentSession } from "$lib/domain";

export interface ActivityDay { date: string; count: number; level: number }
export interface ActivityRepository { name: string; count: number; url?: string; avatarUrl?: string; fallbackAvatarUrl?: string }
export interface RepositoryFile { path: string; index: string; worktree: string; conflict: boolean; untracked: boolean }
export interface RepositoryCommit { oid: string; subject: string; author: string; date: string }
export interface RepositorySnapshot {
  root: string; name: string; branch: string; head: string | null; upstream: string | null;
  ahead: number; behind: number; staged: number; modified: number; untracked: number; conflicts: number;
  files: RepositoryFile[]; commits: RepositoryCommit[]; days: ActivityDay[];
  activityLimited: boolean; shallow: boolean; github: string | null; fetchedAt: number;
}
export interface RepositoryDiff { path: string; diff: string; binary: boolean; untracked: boolean }
export interface GitHubItem {
  number: number; title: string; url: string; author: { login: string } | null;
  isDraft?: boolean; headRefName?: string; baseRefName?: string; reviewDecision?: string | null;
  repository?: { nameWithOwner: string }; updatedAt?: string;
}
export interface GitHubWorkflow { id: number; title: string; name: string; url: string; branch: string; status: string; conclusion: string | null; updatedAt: string }
export interface GitHubRepository {
  nameWithOwner: string; url: string; description: string | null; isPrivate: boolean; isArchived?: boolean;
  updatedAt?: string; stargazerCount?: number; primaryLanguage?: { name: string; color: string } | null;
  defaultBranchRef: { name: string } | null;
  owner?: { avatarUrl: string };
  openGraphImageUrl?: string;
  usesCustomOpenGraphImage?: boolean;
  issues?: { totalCount: number; nodes: GitHubItem[] };
  pullRequests?: { totalCount: number; nodes: GitHubItem[] };
  object?: { statusCheckRollup: { state: string } | null } | null;
}
export interface GitHubRepoSnapshot {
  state: string; repository?: GitHubRepository; workflows?: GitHubWorkflow[]; workflowsError?: string; fetchedAt: number;
}
export interface GitHubAccountSnapshot {
  state: string; login?: string; name?: string | null; url?: string; avatarUrl?: string;
  totalContributions?: number; days?: ActivityDay[];
  topRepositories?: { repository: Pick<GitHubRepository, "nameWithOwner" | "url" | "isPrivate" | "owner" | "openGraphImageUrl" | "usesCustomOpenGraphImage">; contributions: { totalCount: number } }[];
  fetchedAt: number;
}
export interface RepositoryState { loading: boolean; value: RepositorySnapshot | null; error: string }
export const inDesktop = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

// Share within a chat, not between chats whose provisional process cwd may match.
// Only visible, mounted consumers keep the observer alive.
const observers = new Map<string, ReturnType<typeof createObserver>>();
function createObserver(sessionId: string) {
  const store = writable<RepositoryState>({ loading: true, value: null, error: "" });
  let consumers = 0;
  let timer: ReturnType<typeof setInterval> | undefined;
  let pending: Promise<void> | undefined;
  let currentSessionId = sessionId;
  const activeSessions = new Map<string, number>();
  let lastRead = 0;
  function refresh(force = false): Promise<void> {
    if (pending) return pending;
    if (!force && Date.now() - lastRead < 5_000) return Promise.resolve();
    if (!inDesktop()) {
      store.set({ loading: false, value: null, error: "desktop_required" });
      return Promise.resolve();
    }
    store.update((state) => ({ ...state, loading: true }));
    const liveSession = activeSessions.keys().next().value ?? currentSessionId;
    pending = invoke<RepositorySnapshot>("get_session_repository", { sessionId: liveSession, refresh: force })
      .then((value) => { lastRead = Date.now(); store.set({ loading: false, value, error: "" }); })
      .catch((error) => { lastRead = Date.now(); store.update((state) => ({ ...state, loading: false, error: String(error) })); })
      .finally(() => { pending = undefined; });
    return pending;
  }
  const visibleRefresh = () => { if (document.visibilityState === "visible") void refresh(); };
  return {
    subscribe: ((run, invalidate) => {
      consumers += 1;
      if (consumers === 1) {
        void refresh();
        timer = setInterval(visibleRefresh, 15_000);
        document.addEventListener("visibilitychange", visibleRefresh);
      }
      const unsubscribe = store.subscribe(run, invalidate);
      return () => {
        unsubscribe(); consumers -= 1;
        if (!consumers) { clearInterval(timer); document.removeEventListener("visibilitychange", visibleRefresh); }
      };
    }) as Readable<RepositoryState>["subscribe"],
    refresh,
    current: () => get(store),
    setSession: (id: string) => { currentSessionId = id; },
    register: (id: string) => { activeSessions.set(id, (activeSessions.get(id) ?? 0) + 1); },
    unregister: (id: string) => { const count = (activeSessions.get(id) ?? 1) - 1; if (count) activeSessions.set(id, count); else activeSessions.delete(id); },
    active: () => consumers > 0,
  };
}

export function observeRepository(session: Pick<AgentSession, "id" | "workingDirectory" | "nativeSessionId">) {
  const key = JSON.stringify([session.id, session.nativeSessionId, session.workingDirectory]);
  let observer = observers.get(key);
  if (!observer) {
    if (observers.size >= 64) { for (const [cachedKey, cached] of observers) { if (!cached.active()) observers.delete(cachedKey); if (observers.size < 64) break; } }
    observer = createObserver(session.id); observers.set(key, observer);
  }
  observer.setSession(session.id);
  const shared = observer;
  return {
    subscribe: ((run, invalidate) => {
      shared.register(session.id);
      const unsubscribe = shared.subscribe(run, invalidate);
      return () => { shared.unregister(session.id); unsubscribe(); };
    }) as Readable<RepositoryState>["subscribe"],
    refresh: (force = false) => { shared.setSession(session.id); return shared.refresh(force); },
    current: shared.current,
  };
}

export const getGitHubAccount = (refresh = false) => invoke<GitHubAccountSnapshot>("get_github_account", { refresh });
export const getSessionGitHub = (sessionId: string, refresh = false) => invoke<GitHubRepoSnapshot>("get_session_github", { sessionId, refresh });
export const getRepositorySnapshot = (sessionId: string, refresh = true) => invoke<RepositorySnapshot>("get_session_repository", { sessionId, refresh });
export const getRepositoryDiff = (sessionId: string, path: string) => invoke<RepositoryDiff>("get_session_repository_diff", { sessionId, path });

export async function openGitHub(url: string) {
  const parsed = new URL(url);
  if (parsed.protocol !== "https:" || parsed.hostname !== "github.com" || parsed.username || parsed.password) throw new Error("invalid_github_url");
  if (inDesktop()) await openUrl(parsed.href);
  else window.open(parsed.href, "_blank", "noopener,noreferrer");
}

export function repositoryError(error: string, portuguese: boolean) {
  const descriptions: Record<string, [string, string]> = {
    not_a_repository: ["This chat's directory is not a Git repository.", "O diretório deste chat não é um repositório Git."],
    no_working_directory: ["This chat has not reported its project directory.", "Este chat ainda não informou o diretório do projeto."],
    directory_unavailable: ["The project directory is not available on this computer.", "O diretório do projeto não está disponível neste computador."],
    git_missing: ["Install Git to inspect this repository.", "Instale o Git para inspecionar este repositório."],
    cli_missing: ["Install GitHub CLI and sign in to load GitHub data.", "Instale o GitHub CLI e conecte sua conta para carregar os dados."],
    auth_required: ["Sign in to GitHub CLI, then refresh.", "Conecte sua conta no GitHub CLI e atualize."],
    access_limited: ["GitHub access is limited. Check repository permissions or try again later.", "O acesso ao GitHub está limitado. Confira as permissões do repositório ou tente mais tarde."],
    not_found: ["GitHub could not find this repository with the current account.", "O GitHub não encontrou este repositório com a conta atual."],
    command_timeout: ["The repository took too long to respond. Try refreshing.", "O repositório demorou a responder. Tente atualizar."],
    command_output_limit: ["This result exceeds the preview limit. Open it in your editor.", "Este resultado excede o limite de visualização. Abra no seu editor."],
    file_no_longer_changed: ["This file changed since the last refresh. Refresh the repository.", "Este arquivo mudou desde a última atualização. Atualize o repositório."],
    desktop_required: ["Repository tools are available in the Lume desktop app.", "As ferramentas de repositório estão disponíveis no aplicativo desktop do Lume."],
    no_github_remote: ["This repository has no GitHub remote.", "Este repositório não tem um remote do GitHub."],
  };
  return descriptions[error]?.[Number(portuguese)] ?? (portuguese ? "Não foi possível carregar os dados. Confira sua conexão e tente atualizar." : "Could not load the data. Check your connection and try refreshing.");
}
