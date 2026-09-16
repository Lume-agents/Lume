# Workspace and integrations roadmap

Status: in progress. The shared conversation foundation and the first Workspace shell are implemented locally; GitHub and Jira remain intentionally out of scope for the current UI slice.

## Product outcome

Lume should support two complementary desktop experiences backed by the same local state:

- **Orb** remains the lightweight ambient monitor for notifications, approvals, and quick actions.
- **Workspace** becomes a normal, resizable application window for multi-agent work, large diffs, workflows, and connected work items.

GitHub and Jira should be operational context and automation layers, not clones of their full products. Agents can understand local branches and changes, prepare commits and pull requests, read work, propose remote actions, publish documentation, and complete approved flows through Lume's policies and audit trail.

## Non-negotiable principles

- The Orb remains usable without loading the Workspace renderer or Atlassian integration.
- Orb, Workspace, floating terminals, mobile monitoring, and workflows share one Rust backend and one session identity model.
- Jira is optional and disabled until the user connects an Atlassian account.
- GitHub is optional; local Git status remains available without a GitHub account or network connection.
- Jira and Confluence content is sent directly between the local Lume client and Atlassian; Lume does not add a hosted relay.
- Agents never receive the full chat, an unrestricted Jira token, or access to every Atlassian site automatically.
- Read operations and write operations have separate policies.
- Writes are proposed and previewed by default. Destructive operations are outside the initial scope.
- A prompt finishing is not sufficient evidence to close a work item.
- Every remote mutation is idempotent, attributable to a session or workflow step, and recorded locally.
- Existing terminal and workflow behavior must remain functional when Jira is disconnected or unavailable.
- Local commits, remote pushes, pull requests, merges, releases, and work-item mutations are distinct capabilities with distinct approval policies.

## Experience model

### Orb

- Starts quickly and remains the default compact experience.
- Shows agent state, linked work-item key, approvals, questions, errors, and completion.
- Opens the Workspace, a floating terminal, or the original source.
- Does not render Jira boards, full histories, or large diffs.

### Workspace

- Opens as a standard system window and is not always-on-top by default.
- Can be opened from the Orb, tray, global shortcut, or command palette.
- Can return to the Orb without stopping agents, workflows, or Jira actions.
- Restores its last project, layout, selected sessions, and inspector state.

Recommended information architecture:

1. **Sidebar** — projects, active agents, workflows, saved layouts, and connected Jira work.
2. **Workbench** — two or three resizable session panes with chat, composer, plan, notes, and changes.
3. **Inspector** — work-item details, full diff, files, checks, context preview, approvals, and workflow history.
4. **Activity shelf** — queued prompts, proposed Jira actions, permissions, and recoverable errors.

The Workspace must reuse shared session components. It must not fork a second chat implementation from `TerminalWindow`.

## Interface icon system

- Use the original **Lume UI Icons** family for navigation, actions, states, menus, settings, and empty-state affordances. Its visual grammar may follow established UI conventions, but its paths and proportions remain original.
- Standardize on the outline family at a 20 px optical size with `currentColor`; use filled counterparts only for selected, toggled, or attention states.
- Keep agent, browser, IDE, language, and integration logos in `BrandIcon`; they must remain the original brands rather than interface-icon approximations.
- Keep the Lume mascot and the custom Workflow role icons as product-owned artwork.
- Route product icons through one semantic `LumeIcon` component and a typed name map instead of pasting SVG markup into each Svelte component.
- Package only the icons used by Lume. Keep the vector source inside a small typed Svelte registry and split it only if bundle measurements justify the extra requests or modules.
- Preserve visible labels or accessible names for every actionable icon; decorative icons must remain hidden from assistive technology.
- Keep every path original and owned by the Lume project; do not trace or adapt paid icon-library assets.

Exit criteria: Orb, Workspace, terminals, settings, and mobile use one coherent icon vocabulary without replacing product marks or increasing the initial renderer cost materially.

## Git and GitHub product surface

Local Git provides the fast, offline source of truth for:

- Repository root, current branch, detached-head state, and active worktree.
- Staged, unstaged, untracked, conflicted, ahead, and behind counts.
- Files and commits attributable to a session or workflow step.
- Warnings when agents are editing the same branch, worktree, or file set.

GitHub adds the remote delivery context:

- Linked repository, issue, pull request, review, check run, Actions workflow, artifact, release, and security state.
- Create a branch or pull request, request review, post an approved comment, and inspect failed CI.
- Associate session and workflow history with a branch, commit, pull request, or issue.
- Display branch, dirty state, linked PR, and CI compactly in the agent header; show full details in the Workspace Inspector.

Lume should use local Git for commits made from the working tree. GitHub MCP/API should handle remote repository entities rather than recreating a local commit from file contents.

Initial capability levels:

- **Observe** — local status plus read-only GitHub context.
- **Suggest** — propose staging, commit text, PR content, reviews, and issue updates.
- **Approved delivery** — execute explicitly approved commit, push, PR, comment, or workflow action.
- **Protected** — merge, release, branch deletion, force push, and security changes always require a dedicated confirmation; destructive operations stay out of the initial scope.

The first GitHub connection should enable only the required official MCP toolsets and begin in read-only mode. Credentials stay in the operating-system credential store and are never exposed to an agent.

## Jira product surface

The Jira surface should expose only the information needed to operate agent work:

- Site and project selection.
- Search by key, text, assignee, sprint, or saved filter.
- My work, current sprint, linked work, and recently viewed sections.
- Work-item summary, description, status, assignee, acceptance criteria, comments, attachments, links, and child items.
- Link or unlink a work item from a session, terminal group, or workflow run.
- Start a workflow from a work item.
- Preview pending comments, transitions, attachments, worklogs, and documentation changes.
- Open the full item in Jira when deeper administration is required.

The interface should not reproduce Jira administration, board configuration, permission management, or destructive tools.

## Automation model

Agents interact with Jira through Lume capabilities rather than receiving raw credentials.

```text
Agent or workflow step
        ↓ structured request
Lume action broker
        ↓ policy and approval
Atlassian MCP
        ↓ result and audit event
Session, workflow, and linked work item
```

Initial policy levels:

- **Observe** — read and search only.
- **Suggest** — the agent prepares actions; the user approves each one.
- **Approved writes** — predefined non-destructive actions may run after one approval for the workflow.
- **Custom** — allowlisted actions, projects, issue types, transitions, and documentation targets.

Examples:

- Planner creates proposed subtasks from an approved plan.
- Implementer optionally proposes `To Do → In Progress` when execution really starts.
- Tester publishes checks without closing the issue.
- Reviewer can approve completion, request corrections, reopen work, or create a bug.
- A completed workflow can draft a Jira comment and a Confluence page with the result, files, checks, and release notes.
- Attachments explicitly delivered by an agent can be proposed for upload.

Completion rules can require any combination of:

- A real final-turn completion event.
- Required checks passing.
- Acceptance criteria acknowledged by the Reviewer.
- No pending permission or question.
- A designated workflow role completing successfully.
- Explicit user approval.

## Technical architecture

### Shared application state

- Keep sessions, workflow runs, work-item links, action proposals, and connection state in the Rust backend.
- Publish bounded incremental events to Orb, Workspace, terminals, and mobile monitoring.
- Keep window-specific layout state in preferences while session truth remains global.
- Use stable native session IDs and workflow execution IDs as correlation keys.

### Workspace frontend

- Add a dedicated Tauri window and route for the Workspace.
- Extract reusable chat, composer, activity, plan, notes, changes, permission, and rate-limit components from the floating terminal.
- Use one Workspace WebView instead of one WebView per embedded pane.
- Mount only visible session panes and virtualize long histories.
- Load full diffs, Jira descriptions, comments, and attachments only when opened.
- Keep expensive resizing work in animation frames and avoid synchronous backend calls during pointer movement.

### Git and GitHub integration

- Read local repository state in Rust through bounded Git commands or a Git library; never poll once per terminal or once per rendered message.
- Correlate repositories by canonical root and share one debounced watcher across every session in that repository.
- Refresh immediately after known file, commit, branch, and workflow events, with a slow fallback refresh only while the repository is visible.
- Use the official GitHub MCP Server for scoped remote capabilities such as repositories, issues, pull requests, and Actions.
- Start read-only, enable only selected toolsets, and place every write behind Lume's action broker and policy preview.
- Keep local commits separate from remote pushes. A commit proposal contains an explicit file selection, diff summary, message, author, and base revision.
- Detect branch or index changes between preview and execution and require a new preview instead of committing stale content.
- Use worktrees to isolate concurrent implementing agents when the user enables managed branches.
- Treat pull-request comments and public repository content as untrusted context and sanitize it before agent delivery.

### Atlassian connection

- Implement the Atlassian client in Rust using the official Atlassian Rovo MCP v2 endpoint.
- Prefer OAuth 2.1; allow API-token authentication only where required and clearly identified.
- Store refresh tokens or API tokens in the operating-system credential store, never in plain SQLite preferences.
- Persist only non-secret connection metadata: account label, site/cloud ID, selected projects, scopes, and credential reference.
- Discover capabilities at connection time because site permissions and enabled MCP groups can differ.
- Treat expired credentials, revoked scopes, unavailable sites, rate limits, and offline mode as recoverable states.

### Agent tool broker

- Start with Lume executing structured action proposals after the agent completes a turn.
- Later expose a scoped local MCP tool broker to sessions launched and owned by Lume.
- The broker must not replace the `codex` or `claude` executable, modify `PATH`, proxy session ownership, or interfere with native CLIs.
- Each request carries session, workflow, work-item, and idempotency identifiers.
- External/read-only sessions can link Jira context but do not receive injected tools unless explicitly configured by the user.

### Proposed data model

```text
RepositoryContext
├── id
├── canonicalRoot
├── remoteProvider?
├── remoteRepository?
├── branch
├── worktree
├── headSha
├── aheadBehind
└── statusSummary

DevelopmentLink
├── id
├── repositoryId
├── sessionId?
├── workflowRunId?
├── workItemLinkId?
├── branch?
├── commitSha?
└── pullRequestNumber?

GitHubConnection
├── id
├── accountLabel
├── credentialRef
├── grantedScopes[]
├── enabledToolsets[]
├── readOnly
└── status

AtlassianConnection
├── id
├── accountLabel
├── cloudId
├── siteUrl
├── authKind
├── credentialRef
├── grantedScopes[]
└── status

WorkItemLink
├── id
├── connectionId
├── issueKey
├── sessionId?
├── terminalGroupId?
├── workflowRunId?
└── linkedAt

ProposedRemoteAction
├── id
├── sourceSessionId
├── workflowRunId?
├── stepId?
├── issueKey
├── kind
├── sanitizedPreview
├── idempotencyKey
├── policyDecision
├── status
└── timestamps

ProposedGitAction
├── id
├── sourceSessionId
├── workflowRunId?
├── repositoryId
├── kind
├── baseRevision
├── selectedFiles[]
├── sanitizedPreview
├── idempotencyKey
├── policyDecision
├── status
└── timestamps

AutomationPolicy
├── scope
├── allowedProjects[]
├── allowedActions[]
├── allowedTransitions[]
├── requiresApproval
├── completionRules[]
└── limits
```

## Delivery phases

### Phase 0 — Stability gate

- Complete manual validation of headless Codex creation, resume, Take Control, prompt delivery, interruption, queue, and approval.
- Confirm the same session identity survives transfer and Lume restart.
- Record current Orb and terminal performance as the comparison baseline.

Exit criteria: the new Workspace is not built on an unstable session-ownership path.

### Phase 1 — Shared UI foundation

- Extract reusable session presentation and action components from floating terminals.
- Establish shared stores/selectors for bounded chat, activities, changes, plans, notes, permissions, and rate limits.
- Preserve current floating-terminal behavior and styling.
- Add component-level tests before changing the window structure.
- Add a typed semantic `LumeIcon` wrapper backed by the original Lume UI Icons family before replacing icons surface by surface.

Exit criteria: Orb and terminals still behave identically while shared components can render outside `TerminalWindow`.

Current checkpoint:

- Conversation aggregation and deduplication now live in a shared module used by the floating terminal and Workspace.
- A bounded session pane renders chat, activity groups, attachments, and changed files outside `TerminalWindow`.
- Frontend checks and the production build pass with the existing terminal behavior preserved.

### Phase 2 — Workspace shell

- Add the normal resizable Workspace window.
- Implement sidebar, workbench, inspector, layout persistence, and project switching.
- Open and close Workspace from Orb, tray, shortcut, and command palette.
- Keep agents running while either interface is hidden.

Exit criteria: users can inspect every active agent and open selected sessions without spawning floating terminal windows.

Current checkpoint:

- A normal resizable Workspace window can be opened from the Orb header and replaces the Orb while it remains open.
- The first shell includes agent search and filters, live status, a primary chat pane, one optional side-by-side pane, and prompt/queue delivery for sessions already controlled by Lume.
- The side-by-side workbench has an adjustable, keyboard-accessible divider and restores the selected sessions and pane proportion locally.
- Workspace composers support file selection, clipboard paste, previews, removal, queue delivery, and Take Control with the same bounded attachment contract as floating terminals.
- Workspace and Orb now behave as exclusive visual modes while sharing the same running backend.
- Project switching filters the active workspace without changing global session state.
- The persistent inspector summarizes the selected agent's plan, files, checks, rate limits, and source metadata without loading an additional terminal WebView.
- Orb and Workspace expose the same appearance, agent, mobile, project-profile, update, and shortcut settings.
- A configurable global shortcut opens Workspace directly, including the GNOME and COSMIC desktop-shortcut paths.
- The remaining Take Control recovery states are still pending and remain part of the Phase 0 stability gate.

### Phase 3 — Multi-session workbench

- Support two or three visible chat panes with independent composers.
- Add drag-to-reorder, resize, focus, maximize-pane, and restore-layout actions.
- Preserve queue, steer, interrupt, model, effort, plan mode, attachments, and Take Control.
- Add keyboard navigation between agents and panes.

Exit criteria: normal agent work can be completed entirely inside Workspace with no regression in floating terminals.

Current checkpoint:

- Up to three bounded session panes can be opened in one Workspace without spawning additional WebViews.
- Each side pane can be closed independently; selecting an already-open session promotes it without duplicating the chat.
- Both dividers are pointer- and keyboard-resizable, and their proportions survive a Workspace restart.
- Clicking a pane focuses it for the Inspector; `Alt+1`, `Alt+2`, and `Alt+3` move focus between visible panes.
- Any visible pane can be maximized and restored without losing the underlying three-pane layout.
- Each pane now preserves the operational controls of the floating terminal: queued prompts are visible, can be steered into the running turn, and the primary action becomes Interrupt while work is active.
- Codex mode, model, and reasoning effort, plus Claude Code model and effort overrides, are available from one compact pane-level control surface and reuse the existing backend contracts.
- Dragging any non-interactive area of a session header reorders the two or three open panes in place, with an explicit drop target and persisted ordering; sessions are not recreated.
- Named layouts save and restore pane order, proportions, project filter, and Inspector visibility using stable session keys; unavailable sessions are omitted without launching or duplicating agents.

Phase 3 exit criteria are now implemented. Cross-platform visual and interaction checks remain part of the release gate.

### Phase 4 — Review center

- Add a large per-file diff viewer with added/removed counts and file-type icons.
- Support side-by-side and unified diff modes.
- Connect changes to the prompt, workflow step, and final response that produced them.
- Add check results, downloadable deliverables, context preview, and review notes.
- Keep changed files distinct from files explicitly offered for download.

Exit criteria: a Reviewer can understand and approve a multi-file result without opening the original CLI.

Current checkpoint:

- The Inspector opens a large Review Center for the focused session and can jump directly to a changed file.
- Changes, checks, and final responses are now grouped by prompt/result turn, with a task selector; old results with truncated activity history are shown without falsely attributed files.
- Latest-turn changes are grouped per file with file-type icons and individual added/removed totals.
- Captured unified diffs can be reviewed in unified or side-by-side mode without opening the source CLI.
- Latest-turn checks and the final response remain visible as review context; changed files are not presented as downloadable deliverables.
- Where a persisted workflow run references the exact result ID, the Review Center identifies its role and objective on demand.
- Review notes are stored locally against the exact session and result, and remain available after reopening the Review Center or restarting Lume.
- Explicit deliverables, context preview, and richer non-unified diff sources remain pending.

### Phase 5 — Local Git and GitHub context

- Add one shared, debounced repository observer per canonical project root.
- Show branch, worktree, dirty files, conflicts, ahead/behind, head commit, linked PR, and CI state in Workspace.
- Attribute changed files and commits to the session or workflow step that produced them.
- Detect concurrent agents on the same branch or overlapping file sets and surface a warning before execution.
- Add GitHub connection setup with credentials in the operating-system store.
- Connect the official GitHub MCP Server in read-only mode with only the repository, issue, pull-request, and Actions capabilities selected by the user.
- Add explicit proposals for staging, committing, pushing, creating a PR, and requesting review; no write executes silently.
- Keep repository observation unloaded in Orb-only mode except for projects associated with a visible active session.

Exit criteria: Lume can explain where each agent is working, what changed, whether CI is healthy, and what will be committed or sent remotely before approval.

### Phase 6 — Atlassian connection

- Add connection setup, OAuth callback, secure credential storage, site selection, scope review, disconnect, and re-authentication.
- Detect available MCP permission groups and disabled operations.
- Add connection diagnostics without exposing tokens.
- Keep Jira completely absent from normal use until connected.

Exit criteria: a user can connect and disconnect a Jira Cloud account safely on every supported desktop OS.

### Phase 7 — Read-only Jira context

- Implement issue search, project selection, assigned work, current sprint, issue detail, comments, attachments, and links.
- Link an issue to a session, group, or workflow.
- Add selected Jira content to Context Builder with exact preview and size limits.
- Show the linked key and status in Workspace, floating terminals, and a compact Orb state.

Exit criteria: agents can receive approved issue context without manual copy and paste or write access.

### Phase 8 — Unified action proposals

- Define structured proposals for Git staging, commits, pushes, pull requests, reviews, Jira create/edit/comment/transition/worklog/attachment, and Confluence documentation.
- Render human-readable previews and field-level diffs.
- Add approve, edit, reject, and open-in-Jira actions.
- Persist proposal state and idempotency keys before execution.

Exit criteria: agents can prepare useful GitHub and Jira changes without being able to execute them silently.

### Phase 9 — Gated writes and documentation

- Execute approved local commits only when the index and base revision still match their preview.
- Execute approved pushes, pull requests, reviews, and Actions operations through the scoped GitHub connection.
- Execute approved proposals through Atlassian MCP.
- Support Jira issue/subtask creation, edits, comments, non-destructive transitions, worklogs, and attachments.
- Support Confluence page creation and updates for plans, technical notes, reports, and release documentation.
- Record sanitized request/result metadata in the consolidated workflow history.
- Reconcile uncertain outcomes after timeout or restart before retrying.

Exit criteria: each supported write is safe to retry, visible in history, and never duplicated after recovery.

### Phase 10 — Workflow automation

- Add GitHub- and Jira-aware workflow templates.
- Evaluate completion rules before proposing or executing a transition.
- Allow project-level policies for selected comments, labels, assignments, and transitions.
- Pause on permissions, questions, policy violations, missing scopes, rate limits, and Jira conflicts.
- Prevent Jira updates from recursively triggering the same workflow.
- Prevent GitHub webhook or status updates from recursively triggering the same workflow.

Initial templates:

- Jira issue → Plan → Implement → Test → Review.
- Jira bug → Reproduce → Fix → Verify → Comment.
- Jira epic → Plan → Create proposed subtasks.
- Implementation result → Reviewer → Confluence technical note.
- Branch ready → Reviewer → checks → proposed pull request.

Exit criteria: an approved policy can advance a work item through a complete workflow without loops or duplicate writes.

### Phase 11 — Scoped agent tools

- Expose read tools to Lume-owned agents through the local broker.
- Add write tools only when policy enforcement and approval callbacks are reliable.
- Use deferred tool discovery to avoid inflating every agent context.
- Display tool activity as normal Lume events with issue keys and outcomes.
- Support Codex first, then Claude Code after its managed-session path reaches parity.

Exit criteria: a managed agent can use scoped GitHub and Jira tools during a turn without receiving credentials or bypassing Lume policies.

### Phase 12 — Hardening and release

- Validate Windows, Ubuntu, Pop!_OS, Fedora GNOME, and Fedora KDE.
- Test expired OAuth, revoked access, changed Jira permissions, unavailable MCP, offline recovery, and server rate limits.
- Test Git repository replacement, detached HEAD, conflicts, stale commit previews, protected branches, failed pushes, and changed GitHub scopes.
- Test restart during a pending proposal and during an uncertain remote write.
- Test three visible chats with at least 300 messages each, an active workflow, Jira context, and a large diff.
- Measure Orb-only and Workspace idle/active CPU, memory, resize smoothness, and event latency against the Phase 0 baseline.
- Complete keyboard, screen-reader, reduced-motion, scaling, and light/dark theme checks.
- Run a manual end-to-end test against a disposable Jira/Confluence sandbox before release.

Exit criteria: GitHub and Jira remain optional and failure-isolated, Workspace meets the performance baseline, and all local or remote mutations are recoverable and auditable.

## Safety and loop protection

- Maximum remote actions per workflow and per issue.
- Maximum retries with exponential backoff and jitter.
- No automatic delete, project administration, public-link creation, or permission replacement.
- No transition based only on a command ending or an unreviewed agent claim.
- Explicit confirmation when context contains sensitive paths, secrets, personal data, or large attachments.
- Per-project allowlists and per-action approval rules.
- Global emergency stop for local workflows and pending remote actions.
- Remote action IDs recorded before sending and reconciled before retry.
- Never stage unrelated files automatically, rewrite published history, force-push, merge, or publish a release from an agent claim alone.

## Performance constraints

- Orb-only mode must not mount Workspace or initialize GitHub or Atlassian until needed.
- Sidebar lists may show all agents, but only visible panes render full conversations.
- Historical messages and events must be virtualized or incrementally paged.
- Diffs, attachments, GitHub reviews, Jira comments, and documentation are lazy-loaded.
- Pointer movement and window resizing must not trigger synchronous database, MCP, or filesystem work.
- Repository, GitHub, and Jira refresh loops must be shared, bounded, debounced, and suspended when disconnected or unused.
- The Lume icon registry contains only semantic icons used by the product and is loaded once per renderer.
- Performance acceptance uses measured comparison with the Phase 0 baseline rather than unverified universal hardware claims.

## Validation matrix

Automated coverage:

- OAuth state and callback validation.
- Secure credential-reference persistence without token leakage.
- MCP capability discovery and permission-group changes.
- Context selection, sanitization, and size bounds.
- Action-policy evaluation and approval enforcement.
- Idempotent create, edit, comment, transition, attachment, and documentation flows.
- Timeout, retry, uncertain outcome, restart, and offline recovery.
- Session/workflow/work-item correlation and duplicate prevention.
- Repository identity, branch/worktree attribution, stale-preview rejection, and overlapping-agent detection.
- GitHub read-only capability filtering and approval enforcement for every write class.
- Long-chat virtualization and bounded event rendering.

Manual coverage:

- Orb ↔ Workspace transitions without visual or session-state loss.
- Workspace layouts across resolutions, scaling, and multiple monitors.
- Lume UI Icons at compact, normal, selected, disabled, and high-contrast states across desktop and mobile.
- Managed and external Codex/Claude sessions with linked work.
- Local Git without GitHub, GitHub read-only, and explicitly approved delivery flows.
- Jira Observe, Suggest, Approved writes, and Custom policies.
- Workflow completion, rejection, correction, and re-run behavior.
- Real Jira and Confluence sandbox verification with least-privilege accounts.

## Initial out of scope

- Replacing Jira's full board, backlog, reporting, or administration interface.
- Jira Server or Data Center integration; the first implementation targets Atlassian Cloud.
- Automatic destructive operations.
- Autonomous processing of every new issue by default.
- Giving agents direct OAuth/API credentials.
- Injecting tools into externally owned sessions without explicit configuration.
- Replacing or wrapping official agent executables to provide Jira access.

## Recommended release sequence

- **Workspace preview** — Phases 0–2 behind an opt-in preference.
- **Workspace beta** — Phases 3–4 with performance telemetry kept local.
- **Jira preview** — Phases 5–6, read-only.
- **Jira beta** — Phases 7–8, approval-gated writes.
- **Automation preview** — Phase 9 with conservative project policies.
- **Integrated tools** — Phase 10 only after managed-agent parity and Phase 11 hardening.
