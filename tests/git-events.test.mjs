import assert from "node:assert/strict";
import {
  activityCommandAndOutput,
  gitActivityInfo,
  gitEventTitle,
  gitResultLabel,
  gitSequence,
  parseGitCommand,
  shellSegments,
} from "../src/lib/gitEvents.ts";

let counter = 0;
const activity = (kind, title, detail = "", status = "completed") => ({
  id: `git-test-${(counter += 1)}`,
  kind,
  title,
  detail,
  status,
  createdAt: 1,
  files: [],
  attachments: [],
  appendDetail: false,
});

// Claude reports the command as the event title and its response as the detail.
const claude = (command, stdout = "", stderr = "", status = "completed") =>
  activity("command", command, JSON.stringify({ stdout, stderr, interrupted: false }), status);
// Codex reports a generic title and the command in the detail's `cmd`.
const codex = (command, status = "completed") =>
  activity("tool", "functions · exec_command", JSON.stringify({ cmd: command }), status);

// ── shell reading ──
assert.deepEqual(
  shellSegments(`cd /work/lume && git add -A && git commit -m "fix: a && b" | tee out; echo done`).map((segment) => segment.tokens),
  [["cd", "/work/lume"], ["git", "add", "-A"], ["git", "commit", "-m", "fix: a && b"], ["tee", "out"], ["echo", "done"]],
);
assert.deepEqual(shellSegments("git status 2>&1 | head -5").map((segment) => segment.tokens), [["git", "status", "2>&1"], ["head", "-5"]]);
const heredoc = shellSegments("git commit -F - <<'EOF'\nfeat: add panel\n\ngit push now && rm -rf x\nEOF\necho after");
assert.equal(heredoc.length, 2, "the body of a heredoc is not a command");
assert.equal(heredoc[0].heredoc.split("\n")[0], "feat: add panel");

// ── what is and is not git ──
for (const command of ["rg -n git src", "cat .gitignore", "echo git commit", "npm test", "cargo build --release", "ls .git/hooks"]) {
  assert.equal(parseGitCommand(command), undefined, command);
  assert.equal(gitActivityInfo(codex(command)), null, command);
}
assert.equal(gitActivityInfo(activity("message", "git status")), null);
assert.equal(gitActivityInfo(activity("prompt", "git push")), null);

// ── commit ──
const commit = gitActivityInfo(claude(
  `git commit -m "$(cat <<'EOF'\nfix(workspace): keep the composer locked\n\nBody text.\nEOF\n)"`,
  "[fix/writer 9d2c1ab] fix(workspace): keep the composer locked\n 3 files changed, 42 insertions(+), 7 deletions(-)\n",
));
assert.equal(commit.operation, "commit");
assert.equal(commit.subject, "fix(workspace): keep the composer locked");
assert.equal(commit.branch, "fix/writer");
assert.equal(commit.hash, "9d2c1ab");
assert.deepEqual(commit.stats, { files: 3, added: 42, removed: 7 });
assert.equal(commit.result, "ok");
assert.equal(gitEventTitle(commit, "en"), "Commit: fix(workspace): keep the composer locked");
assert.equal(parseGitCommand(`git commit -am "chore: bump"`).flags.includes("all"), true);
assert.equal(parseGitCommand(`git commit -am "chore: bump"`).operation, "commit");
assert.equal(gitEventTitle({ ...commit, flags: ["amend"], subject: undefined }, "pt-BR"), "Commit (amend)");

// ── a chain is named by its most consequential step ──
const chain = gitActivityInfo(claude(
  `cd /home/user/lume && git add -A && git commit -m "feat: git view" && git push -u origin feat/git-view`,
  "[feat/git-view 1a2b3c4] feat: git view\n 5 files changed, 120 insertions(+)\n",
  "To github.com:Lume-agents/Lume.git\n * [new branch]      feat/git-view -> feat/git-view\n",
));
assert.deepEqual(chain.operations, ["add", "commit", "push"]);
assert.equal(chain.operation, "push");
assert.equal(chain.remote, "origin");
assert.equal(chain.branch, "feat/git-view");
assert.equal(chain.repo, "Lume-agents/Lume");
assert.equal(chain.directory, "lume");
assert.equal(chain.subject, "feat: git view", "the commit of the chain keeps its message");
assert.equal(chain.hash, "1a2b3c4");
assert.deepEqual(chain.stats, { files: 5, added: 120, removed: 0 });
assert.ok(chain.flags.includes("set-upstream"));
assert.equal(gitEventTitle(chain, "en"), "Push → origin/feat/git-view");
assert.deepEqual(gitSequence(chain, "pt-BR"), ["Preparar", "Commit", "Push"]);

// ── push outcomes ──
assert.equal(gitActivityInfo(claude("git push", "", "Everything up-to-date\n")).result, "noop");
const rejected = gitActivityInfo(claude("git push origin main", "", "To github.com:o/r.git\n ! [rejected]        main -> main (fetch first)\nerror: failed to push some refs\n"));
assert.equal(rejected.result, "failed");
assert.equal(gitResultLabel(rejected, "pt-BR"), "Falhou");
assert.equal(gitActivityInfo(codex("git push origin main", "failed")).result, "failed");

// ── branches ──
const created = gitActivityInfo(claude("git switch -c fix/external-writer", "", "Switched to a new branch 'fix/external-writer'\n"));
assert.equal(created.operation, "switch");
assert.equal(created.branch, "fix/external-writer");
assert.ok(created.flags.includes("new-branch"));
assert.equal(gitEventTitle(created, "pt-BR"), "Nova branch · fix/external-writer");
const checkoutFrom = parseGitCommand("git checkout -b feat/x origin/main");
assert.equal(checkoutFrom.branch, "feat/x");
assert.equal(checkoutFrom.fromBranch, "origin/main");
assert.equal(parseGitCommand("git checkout -- src/app.ts").operation, "restore");
assert.equal(parseGitCommand("git branch -D old").scope, "delete");
assert.equal(parseGitCommand("git branch").scope, "list");

// ── merge, pull and conflicts ──
const merge = gitActivityInfo(claude("git merge feat/x", "Auto-merging a.ts\nCONFLICT (content): Merge conflict in a.ts\nAutomatic merge failed\n"));
assert.equal(merge.operation, "merge");
assert.equal(merge.fromBranch, "feat/x");
assert.equal(merge.result, "conflict");
assert.equal(gitResultLabel(merge, "en"), "1 conflict");
assert.equal(gitActivityInfo(claude("git pull --rebase origin main", "Already up to date.\n")).result, "noop");
assert.equal(gitEventTitle(gitActivityInfo(claude("git pull origin main", "")), "en"), "Pull ← origin/main");

// ── status and diff ──
const status = gitActivityInfo(claude("git status", "On branch main\nYour branch is ahead of 'origin/main' by 2 commits.\n\nnothing to commit, working tree clean\n"));
assert.equal(status.operation, "status");
assert.equal(status.branch, "main");
assert.deepEqual(status.sync, { ahead: 2, behind: 0 });
assert.equal(status.clean, true);
assert.equal(status.result, "ok", "a read-only command is never an up-to-date no-op");
const behind = gitActivityInfo(claude("git status -sb", "## main...origin/main [ahead 1, behind 3]\n M src/a.ts\n"));
assert.deepEqual(behind.sync, { ahead: 1, behind: 3 });
assert.equal(behind.branch, "main");
assert.equal(behind.clean, undefined);
assert.equal(behind.stats.files, 1);
const dirty = gitActivityInfo(claude("git status", "On branch dev\n\tmodified:   a.ts\n\tnew file:   b.ts\n"));
assert.equal(dirty.stats.files, 2);
const diff = gitActivityInfo(claude("git diff --staged HEAD~1", "diff --git a/a.ts b/a.ts\n--- a/a.ts\n+++ b/a.ts\n@@ -1 +1,2 @@\n-old\n+new\n+more\n"));
assert.equal(diff.operation, "diff");
assert.equal(diff.scope, "staged");
assert.deepEqual(diff.stats, { files: 1, added: 2, removed: 1 });
assert.equal(parseGitCommand("git diff src/lib").scope, undefined, "a path is not a scope");
assert.equal(parseGitCommand("git diff main..feature").scope, "main..feature");

// ── git -C, env prefixes and wrappers ──
const withDir = parseGitCommand("GIT_PAGER=cat git -C /srv/app --no-pager log --oneline -5");
assert.equal(withDir.operation, "log");
assert.equal(withDir.directory, "app");
assert.equal(parseGitCommand("sudo git -c user.name=x commit -m hi").operation, "commit");

// ── GitHub CLI ──
const pr = gitActivityInfo(claude(
  `gh pr create --title "feat: git events" --base main --head feat/git-events --body "text"`,
  "https://github.com/Lume-agents/Lume/pull/57\n",
));
assert.equal(pr.operation, "pull-request");
assert.equal(pr.subject, "feat: git events");
assert.equal(pr.fromBranch, "feat/git-events", "the head branch is where the changes come from");
assert.equal(pr.branch, "main");
assert.equal(pr.repo, "Lume-agents/Lume");
assert.equal(pr.hash, "#57");
assert.equal(gitEventTitle(pr, "en"), "Pull request · create · feat: git events");

// ── remote ──
const remote = gitActivityInfo(claude("git remote -v", "origin\tgit@github.com:Lume-agents/Lume.git (fetch)\norigin\tgit@github.com:Lume-agents/Lume.git (push)\n"));
assert.equal(remote.operation, "remote");
assert.equal(remote.repo, "Lume-agents/Lume");

// ── both agents' shapes ──
assert.equal(gitActivityInfo(codex("git log --oneline -3")).operation, "log");
const running = gitActivityInfo(activity("command", "git push origin main", JSON.stringify({ command: "git push origin main" }), "running"));
assert.equal(running.result, "running");
assert.equal(running.output, "");
const plain = activity("command", "Command", "git status");
assert.equal(activityCommandAndOutput(plain).command, "git status");
assert.equal(gitActivityInfo(plain).operation, "status");
assert.equal(gitActivityInfo(activity("tool", "Bash", JSON.stringify({ command: "git tag v1.2.0" }))).branch, "v1.2.0");
