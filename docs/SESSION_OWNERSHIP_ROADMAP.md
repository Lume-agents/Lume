# Session ownership roadmap

The goal is to let Lume manage supported agent sessions without requiring a visible native CLI for every thread. The official agent executable remains the integration boundary; Lume does not replace or patch it.

## Invariants

- Never write to a thread that is still owned by an external client.
- Keep externally discovered CLIs read-only until the user explicitly transfers control.
- Preserve native CLI and VS Code as opt-in destinations.
- Keep one stable session identity across discovery, transfer, prompts and restarts.
- Do not weaken approval, sandbox or permission settings during a transfer.
- Authenticate local bridges, reject browser-originated bridge connections and keep the App Server on an ephemeral loopback port.
- Never replay a prompt after restart when delivery could not be confirmed.

## Phase 1 — Headless Codex sessions

Status: implemented locally; manual validation pending.

- Treat the existing `Auto` launch target as Lume-managed for Codex.
- Create and resume threads through `codex app-server` without opening a terminal window.
- Send prompts and observe lifecycle events through the existing Codex bridge.
- Identify these sessions as originating from Lume instead of CLI.
- Preserve explicit `Terminal` and `VS Code` launch targets.

Exit criteria: a new or resumed Codex session appears immediately in Lume, accepts prompts and completes a turn without a native terminal window.

## Phase 2 — Headless Take Control

Status: implemented locally for Codex with a durable, exclusive transfer journal; manual validation pending. Claude Code Take Control remains disabled until its managed path guarantees one writer and preserves permissions.

- Stop only the confirmed external agent process.
- Wait for the active writer to be released.
- Resume the same thread through the App Server.
- Deliver the pending prompt directly after ownership is confirmed.
- Do not open a replacement CLI unless explicitly requested.
- Roll back to an external/read-only state when transfer fails.

Exit criteria: taking control preserves the thread and pending prompt without opening or rapidly closing terminal windows.

## Phase 3 — Optional native client

- Add an explicit `Open in CLI` action for Lume-managed sessions.
- Transfer ownership safely or connect the CLI through the existing proxy.
- Make the ownership change visible in the session header.
- Prevent simultaneous independent writers.

Exit criteria: users can opt into the native TUI without losing Lume monitoring or creating duplicate sessions.

## Phase 4 — Claude managed execution

- Promote the existing `claude --print --resume` background path to a defined managed-session mode.
- Validate hooks, permissions, questions, cancellation and final-response capture without a visible terminal.
- Keep interactive Claude CLI sessions external/read-only until transferred.

Exit criteria: supported Claude Code prompts run in the background with complete status and permission behavior.

## Phase 5 — Recovery and performance

Status: core recovery safeguards implemented locally; cross-platform measurements and manual failure injection pending.

- Persist Lume-managed ownership as sanitized metadata, separately from conversation history.
- Reconcile managed sessions and interrupted Take Control operations after restart without repeating turns.
- Clean up orphaned monitors without deleting transcripts.
- Bound queues and reconnect attempts; recover uncertain queued prompts as user-visible failures instead of replaying them.
- Track active turns from live App Server events so queue polling does not repeatedly load complete histories.
- Cache mobile command results by request ID to prevent duplicate destructive retries.
- Measure idle and active CPU/RAM with multiple managed sessions.

Exit criteria: restart, suspension and process failure recover deterministically on Windows, Ubuntu, Pop!_OS and Fedora.

## Required validation

- New and resumed Codex sessions in headless, Terminal and VS Code modes.
- External active-writer rejection and successful transfer after release.
- New turn, queue, steer, interrupt, approval and question flows.
- Duplicate-session and missing-rollout recovery.
- Lume restart during idle and running turns.
- Manual smoke tests on Windows and the supported Linux display backends.
