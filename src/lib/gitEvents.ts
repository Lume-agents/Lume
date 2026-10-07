import type { SessionActivity } from "$lib/domain";
import type { Language } from "$lib/i18n";

/** What a git or GitHub CLI event did, read from the command and its output. */
export type GitOperation =
  | "commit" | "push" | "pull" | "fetch" | "clone" | "checkout" | "switch" | "branch" | "merge"
  | "rebase" | "cherry-pick" | "reset" | "restore" | "stash" | "add" | "status" | "diff" | "log"
  | "show" | "tag" | "remote" | "worktree" | "apply" | "config" | "credential" | "pull-request"
  | "issue" | "release" | "gh" | "other";

export type GitResult = "ok" | "failed" | "conflict" | "noop" | "running";

export type GitEventInfo = {
  tool: "git" | "gh";
  /** Operations of the command, in order (`git add && git commit` is add, commit). */
  operations: GitOperation[];
  /** The most consequential of them; it names the event. */
  operation: GitOperation;
  branch?: string;
  fromBranch?: string;
  remote?: string;
  repo?: string;
  directory?: string;
  hash?: string;
  subject?: string;
  scope?: string;
  stats?: { files?: number; added?: number; removed?: number };
  /** `git status`: commits ahead of / behind the upstream, and whether the tree is clean. */
  sync?: { ahead: number; behind: number };
  clean?: boolean;
  flags: string[];
  result: GitResult;
  note?: string;
  url?: string;
  command: string;
  output: string;
};

type Segment = { tokens: string[]; heredoc?: string };
type GitCall = {
  tool: "git" | "gh";
  operation: GitOperation;
  args: string[];
  flags: Set<string>;
  directory?: string;
  heredoc?: string;
};

// ───────────────────────── shell reading ─────────────────────────

function matchingParenthesis(source: string, open: number): number {
  let depth = 0;
  for (let index = open; index < source.length; index += 1) {
    const char = source[index];
    if (char === "'" || char === '"') {
      const end = source.indexOf(char, index + 1);
      index = end < 0 ? source.length : end;
    } else if (char === "(") depth += 1;
    else if (char === ")" && --depth === 0) return index;
  }
  return source.length - 1;
}

/**
 * Splits a shell command into the simple commands it chains (`&&`, `||`, `;`,
 * `|`, newlines) with their words unquoted. A quoted `$(cat <<'EOF' …)` stays
 * inside its word, and the body of a top-level heredoc is kept apart so text in
 * a commit message is never mistaken for another command.
 */
export function shellSegments(command: string): Segment[] {
  const segments: Segment[] = [];
  let tokens: string[] = [];
  let word = "";
  let hasWord = false;
  const pending: string[] = [];
  const endWord = () => {
    if (hasWord) tokens.push(word);
    word = "";
    hasWord = false;
  };
  const endSegment = (): Segment | undefined => {
    endWord();
    if (!tokens.length) return undefined;
    const segment = { tokens };
    segments.push(segment);
    tokens = [];
    return segment;
  };
  let index = 0;
  while (index < command.length) {
    const char = command[index];
    if (char === "\\") {
      if (command[index + 1] !== "\n") {
        word += command[index + 1] ?? "";
        hasWord = true;
      }
      index += 2;
    } else if (char === "'") {
      const end = command.indexOf("'", index + 1);
      const stop = end < 0 ? command.length : end;
      word += command.slice(index + 1, stop);
      hasWord = true;
      index = stop + 1;
    } else if (char === '"') {
      hasWord = true;
      index += 1;
      while (index < command.length && command[index] !== '"') {
        if (command[index] === "\\") {
          word += command[index + 1] ?? "";
          index += 2;
        } else if (command.startsWith("$(", index)) {
          const end = matchingParenthesis(command, index + 1);
          word += command.slice(index, end + 1);
          index = end + 1;
        } else {
          word += command[index];
          index += 1;
        }
      }
      index += 1;
    } else if (char === "$" && command[index + 1] === "(") {
      const end = matchingParenthesis(command, index + 1);
      word += command.slice(index, end + 1);
      hasWord = true;
      index = end + 1;
    } else if (char === "`") {
      const end = command.indexOf("`", index + 1);
      const stop = end < 0 ? command.length - 1 : end;
      word += command.slice(index, stop + 1);
      hasWord = true;
      index = stop + 1;
    } else if (char === "\n") {
      const segment = endSegment();
      index += 1;
      let body: string[] = [];
      for (const delimiter of pending.splice(0)) {
        body = [];
        while (index < command.length) {
          const end = command.indexOf("\n", index);
          const line = command.slice(index, end < 0 ? command.length : end);
          index = end < 0 ? command.length : end + 1;
          if (line.trim() === delimiter) break;
          body.push(line);
        }
        if (segment && !segment.heredoc) segment.heredoc = body.join("\n");
      }
    } else if (char === "<" && command[index + 1] === "<" && command[index + 2] !== "<") {
      const delimiter = /^<<-?\s*(['"]?)([A-Za-z0-9_]+)\1/.exec(command.slice(index));
      endWord();
      if (delimiter) {
        pending.push(delimiter[2]);
        index += delimiter[0].length;
      } else {
        index += 2;
      }
    } else if (char === " " || char === "\t") {
      endWord();
      index += 1;
    } else if (char === "#" && !hasWord) {
      const end = command.indexOf("\n", index);
      index = end < 0 ? command.length : end;
    } else if (char === "&" && (command[index - 1] === ">" || command[index + 1] === ">")) {
      word += char; // 2>&1 and &> are redirections, not separators
      hasWord = true;
      index += 1;
    } else if (char === "&" || char === "|") {
      endSegment();
      index += command[index + 1] === char ? 2 : 1;
    } else if (char === ";" || char === "(" || char === ")") {
      endSegment();
      index += 1;
    } else {
      word += char;
      hasWord = true;
      index += 1;
    }
  }
  endSegment();
  return segments;
}

const WRAPPERS = new Set(["sudo", "time", "command", "nohup", "exec", "env", "nice"]);

function executableName(token: string): string {
  return (token.split(/[\\/]/).pop() ?? token).replace(/\.exe$/i, "");
}

function withoutRedirections(tokens: string[]): string[] {
  const kept: string[] = [];
  for (let index = 0; index < tokens.length; index += 1) {
    const token = tokens[index];
    if (/^\d*(?:>>?|<)&?\S/.test(token) || /^&>/.test(token)) continue;
    if (/^\d*(?:>>?|<)$/.test(token)) {
      index += 1;
      continue;
    }
    kept.push(token);
  }
  return kept;
}

// ───────────────────────── command reading ─────────────────────────

const OPTIONS_WITH_VALUE = new Set(["-C", "-c", "--git-dir", "--work-tree", "--namespace", "--exec-path", "-m", "--message", "-F", "--file", "-b", "-B", "--set-upstream-to", "--author", "--date", "--format", "--pretty", "-n", "--max-count", "--since", "--until", "--grep"]);

function repoName(url: string): string | undefined {
  const cleaned = url.trim().replace(/\.git$/, "").replace(/\/+$/, "");
  const match = /(?:[:/])([^/:\s]+\/[^/:\s]+)$/.exec(cleaned) ?? /([^/:\s]+)$/.exec(cleaned);
  return match?.[1];
}

function operationOf(subcommand: string, args: string[], flags: Set<string>): GitOperation {
  switch (subcommand) {
    case "commit": case "push": case "pull": case "fetch": case "clone": case "merge": case "rebase":
    case "reset": case "stash": case "add": case "status": case "diff": case "log": case "show":
    case "tag": case "remote": case "worktree": case "apply": case "config": case "credential":
    case "switch": case "branch":
      return subcommand;
    case "cherry-pick": return "cherry-pick";
    case "restore": case "clean": case "rm": return "restore";
    case "checkout": return args.includes("--") && !flags.has("new-branch") ? "restore" : "checkout";
    case "am": return "apply";
    case "rev-parse": case "rev-list": case "ls-files": case "ls-remote": case "cat-file": case "blame":
    case "describe": case "shortlog": case "reflog": case "grep": case "show-ref": case "diff-tree":
    case "for-each-ref": case "merge-base": case "name-rev":
      return "show";
    default: return "other";
  }
}

function interpretSegment(segment: Segment, directory: string | undefined): GitCall | undefined {
  const tokens = withoutRedirections(segment.tokens);
  let index = 0;
  const skipAssignments = () => { while (index < tokens.length && /^[A-Za-z_][A-Za-z0-9_]*=/.test(tokens[index])) index += 1; };
  skipAssignments();
  while (index < tokens.length && WRAPPERS.has(executableName(tokens[index]))) {
    index += 1;
    skipAssignments();
  }
  const tool = executableName(tokens[index] ?? "");
  if (tool !== "git" && tool !== "gh") return undefined;
  index += 1;
  let workingDirectory = directory;
  if (tool === "gh") {
    const noun = tokens[index];
    const verb = tokens[index + 1];
    if (!noun) return { tool, operation: "gh", args: [], flags: new Set(), directory: workingDirectory };
    const operation: GitOperation = noun === "pr" ? "pull-request" : noun === "issue" ? "issue" : noun === "release" ? "release" : "gh";
    return { tool, operation, args: [noun, ...(verb ? [verb] : []), ...tokens.slice(index + 2)], flags: new Set(), directory: workingDirectory };
  }
  while (index < tokens.length && tokens[index].startsWith("-")) {
    const option = tokens[index];
    if (option === "-C" || option === "-c" || option === "--git-dir" || option === "--work-tree" || option === "--namespace" || option === "--exec-path") {
      if (option === "-C") workingDirectory = tokens[index + 1];
      index += 2;
    } else {
      index += 1;
    }
  }
  const subcommand = tokens[index];
  if (!subcommand) return { tool, operation: "other", args: [], flags: new Set(), directory: workingDirectory };
  const args = tokens.slice(index + 1);
  const flags = new Set<string>();
  const set = (flag: string, ...names: string[]) => { if (args.some((arg) => names.includes(arg))) flags.add(flag); };
  set("force", "-f", "--force", "--force-with-lease");
  set("amend", "--amend");
  set("set-upstream", "-u", "--set-upstream");
  set("new-branch", "-b", "-B", "-c", "-C", "--orphan");
  set("delete", "-d", "-D", "--delete");
  set("staged", "--cached", "--staged");
  set("hard", "--hard");
  set("squash", "--squash");
  set("no-ff", "--no-ff");
  set("tags", "--tags");
  set("all", "-A", "--all", "-a");
  set("abort", "--abort");
  set("continue", "--continue");
  if (subcommand === "commit" && args.some((arg) => /^-[a-zA-Z]*a[a-zA-Z]*$/.test(arg))) flags.add("all");
  const operation = operationOf(subcommand, args, flags);
  return { tool, operation, args, flags, directory: workingDirectory, heredoc: segment.heredoc };
}

function positionals(args: string[]): string[] {
  const result: string[] = [];
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (arg === "--") break;
    if (arg.startsWith("-")) {
      if (OPTIONS_WITH_VALUE.has(arg)) index += 1;
      continue;
    }
    result.push(arg);
  }
  return result;
}

function optionValues(args: string[], names: string[]): string[] {
  const values: string[] = [];
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (names.includes(arg) && args[index + 1] !== undefined) {
      values.push(args[index + 1]);
      index += 1;
      continue;
    }
    const joined = names.find((name) => name.length > 2 && arg.startsWith(`${name}=`));
    if (joined) values.push(arg.slice(joined.length + 1));
    // `-m"msg"` and the short flag cluster `-am msg`
    const cluster = /^-([a-zA-Z]+)$/.exec(arg);
    if (cluster && names.some((name) => name.length === 2 && cluster[1].endsWith(name[1])) && args[index + 1] !== undefined) {
      values.push(args[index + 1]);
      index += 1;
    }
  }
  return values;
}

function firstLine(text: string): string {
  return text.split("\n").map((line) => line.trim()).find(Boolean) ?? "";
}

function commitSubject(call: GitCall): string | undefined {
  const [message] = optionValues(call.args, ["-m", "--message"]);
  if (message !== undefined) {
    const heredoc = /<<-?\s*(['"]?)([A-Za-z0-9_]+)\1[^\n]*\n([\s\S]*?)\n\s*\2\b/.exec(message);
    return firstLine(heredoc ? heredoc[3] : message) || undefined;
  }
  if (call.heredoc !== undefined) return firstLine(call.heredoc) || undefined;
  return undefined;
}

const PRIORITY: GitOperation[] = [
  "push", "commit", "merge", "rebase", "cherry-pick", "pull-request", "pull", "checkout", "switch", "branch",
  "tag", "reset", "stash", "clone", "fetch", "release", "issue", "restore", "add", "worktree", "remote", "apply",
  "status", "diff", "show", "log", "config", "gh", "credential", "other",
];

function describeCalls(calls: GitCall[]): Omit<GitEventInfo, "result" | "note" | "command" | "output" | "url"> {
  const operations = calls.map((call) => call.operation).filter((operation, index, all) => operation !== all[index - 1]);
  const operation = [...operations].sort((a, b) => PRIORITY.indexOf(a) - PRIORITY.indexOf(b))[0] ?? "other";
  const lead = calls.find((call) => call.operation === operation) ?? calls[0];
  const info: Omit<GitEventInfo, "result" | "note" | "command" | "output" | "url"> = {
    tool: lead.tool,
    operations,
    operation,
    directory: lead.directory ? executableName(lead.directory.replace(/\/+$/, "")) || undefined : undefined,
    flags: [...new Set(calls.flatMap((call) => [...call.flags]))],
  };
  // A commit inside a longer chain (add, commit, push) still names the event's subject.
  const commitCall = calls.find((call) => call.operation === "commit");
  if (commitCall && operation !== "commit") info.subject = commitSubject(commitCall);
  const places = positionals(lead.args);
  switch (operation) {
    case "commit":
      info.subject = commitSubject(lead);
      break;
    case "push": {
      const [remote, ...refs] = places;
      info.remote = remote;
      const ref = refs.at(-1);
      info.branch = ref?.includes(":") ? ref.split(":").pop() : ref;
      if (info.branch === "HEAD") info.branch = undefined;
      break;
    }
    case "pull": case "fetch":
      [info.remote] = places;
      info.branch = places[1];
      break;
    case "clone": {
      const url = places[0];
      info.repo = url ? repoName(url) : undefined;
      break;
    }
    case "checkout": case "switch": {
      const names = optionValues(lead.args, ["-b", "-B", "-c", "-C"]);
      info.branch = names[0] ?? places[0];
      if (names[0] && places[0]) info.fromBranch = places[0];
      break;
    }
    case "branch": {
      const names = optionValues(lead.args, ["-m", "-M"]);
      info.branch = places[0];
      info.scope = lead.flags.has("delete") ? "delete" : names.length ? "rename" : places.length ? "create" : "list";
      break;
    }
    case "merge": case "rebase":
      info.fromBranch = places[0];
      break;
    case "cherry-pick":
      info.hash = places[0];
      break;
    case "tag":
      info.branch = places[0];
      break;
    case "stash":
      info.scope = places[0] ?? "push";
      break;
    case "worktree":
      info.scope = places[0];
      info.branch = optionValues(lead.args, ["-b", "-B"])[0];
      break;
    case "remote":
      info.scope = places[0];
      info.remote = places[1];
      info.repo = places[2] ? repoName(places[2]) : undefined;
      break;
    case "diff": {
      // Only refs and ranges name a scope; anything else is a path.
      const ref = places.find((place) => /\.\.|^HEAD|^[0-9a-f]{7,40}$|^origin\/|^@|~\d*$|\^\d*$/.test(place));
      info.scope = lead.flags.has("staged") ? "staged" : ref;
      break;
    }
    case "pull-request": case "issue": case "release": {
      info.scope = [lead.args[0], lead.args[1]].filter(Boolean).join(" ");
      const title = optionValues(lead.args, ["--title", "-t"])[0];
      info.subject = title;
      // Changes flow from the head branch into the base branch.
      info.fromBranch = optionValues(lead.args, ["--head", "-H"])[0];
      info.branch = optionValues(lead.args, ["--base", "-B"])[0];
      break;
    }
    case "reset": case "show": case "log":
      info.scope = places[0];
      break;
    default:
      break;
  }
  return info;
}

/** Reads a shell command; `undefined` when it runs no git or GitHub CLI command. */
export function parseGitCommand(command: string): Omit<GitEventInfo, "result" | "note" | "command" | "output" | "url"> | undefined {
  if (!/\b(?:git|gh)\b/.test(command)) return undefined;
  let directory: string | undefined;
  const calls: GitCall[] = [];
  for (const segment of shellSegments(command)) {
    const tokens = withoutRedirections(segment.tokens);
    if (tokens[0] === "cd" && tokens[1]) {
      directory = tokens[1];
      continue;
    }
    const call = interpretSegment(segment, directory);
    if (call) calls.push(call);
  }
  return calls.length ? describeCalls(calls) : undefined;
}

// ───────────────────────── output reading ─────────────────────────

function applyOutput(info: GitEventInfo, output: string, failedStatus: boolean) {
  const text = output.replace(/\r/g, "");
  const stats = /(\d+) files? changed(?:, (\d+) insertions?\(\+\))?(?:, (\d+) deletions?\(-\))?/.exec(text);
  if (stats) {
    info.stats = { files: Number(stats[1]), added: Number(stats[2] ?? 0), removed: Number(stats[3] ?? 0) };
  }
  const to = /^To\s+(\S+)/m.exec(text);
  if (to) info.repo ??= repoName(to[1]);
  const committed = /^\[([^\]\s]+)(?: \(root-commit\))? ([0-9a-f]{7,40})\]\s*(.*)$/m.exec(text);
  if (committed && info.operations.includes("commit")) {
    if (info.operation === "commit") info.branch ??= committed[1];
    info.hash ??= committed[2];
    info.subject ??= committed[3] || undefined;
  }
  const pushed = /^\s*(?:[+*\- ]\s*)?(?:\[new (?:branch|tag)\]|([0-9a-f]{7,40}\.\.\.?[0-9a-f]{7,40}))\s+(\S+) -> (\S+)/m.exec(text);
  if (pushed && info.operation === "push") {
    info.branch ??= pushed[3];
    if (pushed[1] && !info.operations.includes("commit")) info.hash ??= pushed[1].split(/\.\.\.?/).pop();
    if (/\[new branch\]/.test(text)) info.scope ??= "new";
  }
  const switched = /Switched to (?:a new )?branch '([^']+)'|Already on '([^']+)'/.exec(text);
  if (switched && (info.operation === "checkout" || info.operation === "switch")) info.branch ??= switched[1] ?? switched[2];
  const onBranch = /^On branch (\S+)/m.exec(text) ?? /^## ([^.\s]+)/m.exec(text);
  if (onBranch && info.operation === "status") info.branch ??= onBranch[1];
  if (info.operation === "status") {
    const upstream = /Your branch is (ahead of|behind) '[^']+' by (\d+) commits?/.exec(text);
    const diverged = /and have (\d+) and (\d+) different commits each/.exec(text);
    const short = /^## \S+?(?:\.\.\.\S+)?(?: \[(?:ahead (\d+))?(?:, )?(?:behind (\d+))?\])?$/m.exec(text);
    const ahead = upstream?.[1] === "ahead of" ? Number(upstream[2]) : diverged ? Number(diverged[1]) : Number(short?.[1] ?? 0);
    const behind = upstream?.[1] === "behind" ? Number(upstream[2]) : diverged ? Number(diverged[2]) : Number(short?.[2] ?? 0);
    if (ahead || behind) info.sync = { ahead, behind };
    const entries = text.split("\n").filter((line) => /^\s+(?:modified|new file|deleted|renamed|typechange):/.test(line) || /^[ MADRCU?!]{2} \S/.test(line)).length;
    if (/working tree clean|nothing to commit/.test(text) || (short && !entries)) info.clean = true;
    else if (entries) info.stats ??= { files: entries };
  }
  if (info.operation === "diff" || info.operation === "show") {
    const headers = (text.match(/^diff --git /gm) ?? []).length;
    if (headers && !info.stats) {
      const body = text.split("\n");
      info.stats = {
        files: headers,
        added: body.filter((line) => line.startsWith("+") && !line.startsWith("+++")).length,
        removed: body.filter((line) => line.startsWith("-") && !line.startsWith("---")).length,
      };
    }
  }
  const pullRequest = /https:\/\/github\.com\/([^/\s]+\/[^/\s]+)\/(?:pull|issues)\/(\d+)/.exec(text);
  if (pullRequest) {
    info.url = pullRequest[0];
    info.repo ??= pullRequest[1];
    info.hash ??= `#${pullRequest[2]}`;
  }
  const origin = /^origin\s+(\S+)\s+\((?:fetch|push)\)/m.exec(text);
  if (origin) info.repo ??= repoName(origin[1]);
  const conflicts = (text.match(/^CONFLICT \(/gm) ?? []).length;
  if (conflicts) {
    info.result = "conflict";
    info.note = `${conflicts}`;
  } else if (failedStatus || /^(?:fatal|error):/m.test(text) || /^ ! \[(?:rejected|remote rejected)\]/m.test(text)) {
    info.result = "failed";
    info.note = firstLine(text.split("\n").filter((line) => /^(?:fatal|error|hint):|rejected/.test(line.trim())).join("\n")) || undefined;
  } else if (!["status", "diff", "log", "show"].includes(info.operation) && /Everything up-to-date|Already up to date|nothing to commit|nothing added to commit|no changes added/i.test(text)) {
    info.result = "noop";
    info.note ??= /Everything up-to-date/.test(text) ? "up-to-date" : /nothing to commit|nothing added|no changes added/i.test(text) ? "nothing-to-commit" : "up-to-date";
  }
}

// ───────────────────────── activity reading ─────────────────────────

function jsonObject(text: string | undefined): Record<string, unknown> | undefined {
  const trimmed = text?.trim() ?? "";
  if (!trimmed.startsWith("{")) return undefined;
  try {
    const value = JSON.parse(trimmed);
    return value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : undefined;
  } catch {
    return undefined;
  }
}

function textOf(value: unknown): string {
  if (typeof value === "string") return value;
  if (Array.isArray(value)) return value.map((part) => String(part)).join(" ");
  return "";
}

function normalizedTitle(title: string): string {
  return title.replace(/^functions\s*[·:]\s*/i, "").replace(/^functions[.:/]/i, "").trim();
}

/** The command line and its output, whichever agent produced the event. */
export function activityCommandAndOutput(activity: SessionActivity): { command: string; output: string } {
  const title = normalizedTitle(activity.title);
  const detail = activity.detail ?? "";
  const object = jsonObject(detail);
  const fromObject = textOf(object?.cmd) || textOf(object?.command);
  // An input object (`cmd`/`command`) has no output yet; a response object carries it.
  const output = object
    ? [object.stdout, object.stderr, object.output, object.aggregated_output, object.result]
        .map(textOf).filter(Boolean).join("\n")
    : detail;
  if (/\b(?:git|gh)\b/.test(title) && parseGitCommand(title)) return { command: title, output };
  if (fromObject) return { command: fromObject, output };
  const lead = firstLine(detail);
  return parseGitCommand(lead) ? { command: lead, output: detail.slice(detail.indexOf(lead) + lead.length).trim() } : { command: title, output };
}

const cache = new Map<string, GitEventInfo | null>();

/** Git details for an event, or null when it is not a git or GitHub CLI command. */
export function gitActivityInfo(activity: SessionActivity): GitEventInfo | null {
  if (["prompt", "message", "analysis", "plan", "file", "permission", "question", "subagent", "task"].includes(activity.kind)) return null;
  const probe = `${activity.title} ${activity.detail?.slice(0, 1_500) ?? ""}`;
  if (!/\b(?:git|gh)\b/.test(probe)) return null;
  const key = `${activity.id}\u0000${activity.status}\u0000${activity.title}\u0000${activity.detail?.length ?? 0}`;
  const known = cache.get(key);
  if (known !== undefined) return known;
  const { command, output } = activityCommandAndOutput(activity);
  const parsed = parseGitCommand(command);
  let info: GitEventInfo | null = null;
  if (parsed) {
    info = { ...parsed, result: activity.status === "running" ? "running" : "ok", command, output: output.trim() };
    if (activity.status !== "running") applyOutput(info, output, activity.status === "failed");
  }
  if (cache.size > 800) cache.clear();
  cache.set(key, info);
  return info;
}

// ───────────────────────── wording ─────────────────────────

const LABELS: Record<GitOperation, [string, string]> = {
  commit: ["Commit", "Commit"], push: ["Push", "Push"], pull: ["Pull", "Pull"], fetch: ["Fetch", "Fetch"],
  clone: ["Clone", "Clone"], checkout: ["Checkout", "Checkout"], switch: ["Switch branch", "Troca de branch"],
  branch: ["Branch", "Branch"], merge: ["Merge", "Merge"], rebase: ["Rebase", "Rebase"],
  "cherry-pick": ["Cherry-pick", "Cherry-pick"], reset: ["Reset", "Reset"], restore: ["Restore", "Restauração"],
  stash: ["Stash", "Stash"], add: ["Stage", "Preparar"], status: ["Status", "Status"], diff: ["Diff", "Diff"],
  log: ["History", "Histórico"], show: ["Inspect", "Inspeção"], tag: ["Tag", "Tag"], remote: ["Remote", "Remoto"],
  worktree: ["Worktree", "Worktree"], apply: ["Apply patch", "Aplicar patch"], config: ["Config", "Config"],
  credential: ["Credentials", "Credenciais"], "pull-request": ["Pull request", "Pull request"],
  issue: ["Issue", "Issue"], release: ["Release", "Release"], gh: ["GitHub CLI", "GitHub CLI"], other: ["Git", "Git"],
};

export function gitOperationLabel(operation: GitOperation, language: Language): string {
  return LABELS[operation][language === "pt-BR" ? 1 : 0];
}

/** Short headline of the event, e.g. `Push → origin/feat/x`. */
export function gitEventTitle(info: GitEventInfo, language: Language): string {
  const pt = language === "pt-BR";
  const label = gitOperationLabel(info.operation, language);
  const target = [info.remote, info.branch].filter(Boolean).join("/");
  switch (info.operation) {
    case "commit":
      return info.subject ? `${info.flags.includes("amend") ? (pt ? "Commit (amend)" : "Amend commit") : label}: ${info.subject}` : info.flags.includes("amend") ? (pt ? "Commit (amend)" : "Amend commit") : label;
    case "push": return target ? `${label} → ${target}` : label;
    case "pull": case "fetch": return target ? `${label} ← ${target}` : label;
    case "checkout": case "switch":
      return info.branch ? `${info.flags.includes("new-branch") ? (pt ? "Nova branch" : "New branch") : label} · ${info.branch}` : label;
    case "branch": {
      const verb = info.scope === "delete" ? (pt ? "removida" : "deleted") : info.scope === "create" ? (pt ? "criada" : "created") : info.scope === "rename" ? (pt ? "renomeada" : "renamed") : "";
      return info.branch ? `${label} · ${info.branch}${verb ? ` ${verb}` : ""}` : pt ? "Branches" : "Branches";
    }
    case "merge": case "rebase":
      return info.fromBranch ? `${label} · ${info.fromBranch}${info.branch ? ` → ${info.branch}` : ""}` : label;
    case "tag": return info.branch ? `${label} · ${info.branch}` : label;
    case "clone": return info.repo ? `${label} · ${info.repo}` : label;
    case "status": return info.branch ? `${label} · ${info.branch}` : label;
    case "pull-request": case "issue": case "release":
      return [label, info.scope?.split(" ")[1], info.subject].filter(Boolean).join(" · ");
    case "diff": return info.scope ? `${label} · ${info.scope === "staged" ? (pt ? "preparado" : "staged") : info.scope}` : label;
    default: return label;
  }
}

/** `add › commit › push` for a command that chains several git operations. */
export function gitSequence(info: GitEventInfo, language: Language): string[] {
  return info.operations.length > 1 ? info.operations.map((operation) => gitOperationLabel(operation, language)) : [];
}

export function gitResultLabel(info: GitEventInfo, language: Language): string | null {
  const pt = language === "pt-BR";
  switch (info.result) {
    case "failed": return pt ? "Falhou" : "Failed";
    case "conflict": return info.note ? (pt ? `${info.note} conflito${info.note === "1" ? "" : "s"}` : `${info.note} conflict${info.note === "1" ? "" : "s"}`) : pt ? "Conflito" : "Conflict";
    case "noop": return info.note === "nothing-to-commit" ? (pt ? "Nada a commitar" : "Nothing to commit") : pt ? "Já atualizado" : "Up to date";
    default: return null;
  }
}
