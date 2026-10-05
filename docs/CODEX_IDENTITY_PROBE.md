# Codex CLI identity observation prototype

This is an **opt-in, observation-only** diagnostic, not a new gateway. Normal
CLI identity capture is independent of this diagnostic's enable flag. Positive
startup ancestry supplies automatic display links for already detected CLIs.
It never creates extra sessions, assumes control, acquires a native writer, reads
a terminal, sends a prompt, restarts a process, or alters PATH/Codex settings.
Existing CLIs stay untouched.

**Safety update (2026-10-01):** the shared-daemon metadata observer is disabled in
production, including diagnostic reports. Codex's third-party `initialize`
handshake is not read-only even if all subsequent requests are metadata reads.
Reports return `read_only_initialize_unavailable`; the transport remains only in
isolated tests. Private hook/ancestry capture and local SQLite/index title recovery
are unchanged. This removes the observer's polling, not the identity limitation.

## Product target: ordinary `codex`, without extra flags

Automatic monitoring must work with ordinary `codex` and `codex resume`. Neither
`--no-daemon` nor manual conversation linking completes that requirement; they
remain compatibility/testing fallbacks. The shared-daemon case is **not solved**.

The missing contract is **connected CLI client -> attached conversation(s)**,
with connection/switch/disconnection lifecycle. A physical PID is not mandatory
for read-only conversation monitoring. Positive process identity remains required
for any action that terminates an external process; monitoring must not imply
writer ownership or permission to send prompts.

### Sources checked

- `thread/loaded/list` enumerates in-memory threads, not connected CLIs. The
  [official thread API](https://learn.chatgpt.com/docs/app-server#threads) documents
  that a thread can remain loaded for 30 minutes after its last subscriber leaves.
- `thread/read(includeTurns=false)` supplies names and runtime status, but not
  local subscribers. `originator` is historical, and `sessionId` groups a thread
  tree rather than identifying the client currently viewing it.
- The installed protocol schemas expose no snapshot/events for local client
  subscribers. `remoteControl/client/list` is for remote devices, not local TUIs.
- Technical logs contain literal `process_uuid=pid:<pid>:<uuid>`, but observed
  startup/dispatch records lack a current-thread field. Recap/overview references
  are not selection evidence; arbitrary UUIDs in logs can come from user text.
- Empty `tui-thread-reference-capabilities` files are not a verified presence
  lease. File existence, a historical log PID, cwd, recency, or matching counts
  must not supply a binding. The installed binary calls their persistence a
  task-reference capability; they do not cover all currently open conversations
  and no disconnect/heartbeat contract was verified.
- The live CLI's log database has no `codex_app_server_client::protocol_requests`
  records. A generic typed-request literal in a binary is not a verified log
  envelope and cannot justify extracting IDs from arbitrary message bodies.
- Native MCP integration is also unproven as a presence source: the
  [official changelog](https://learn.chatgpt.com/docs/changelog) mentions session
  and originating-window request metadata, but the
  [MCP documentation](https://learn.chatgpt.com/docs/extend/mcp) does not establish
  a live TUI owner, selected thread before the first prompt, or conversation-switch
  lifecycle. Dynamic tool calls during a turn are not startup presence events.

### Correction work and acceptance gate

1. Obtain an explicit, supported snapshot/event source for connected native CLI
   clients and their attached thread IDs. The official server already tracks
   subscriptions, but exposing them to Lume is a producer-side integration need,
   not a field currently available in its public protocol. Do not invent an RPC.
2. Use connection/client identity for monitoring. A disconnect removes that
   client's attachment immediately, regardless of the daemon's loaded-thread
   grace period. Handle multiple CLI clients attached to the same conversation.
3. Key conversation cards and names by native **thread ID**, not tree session ID.
   Merge confirmed client observations with the existing card; never add a
   second process card for the same confirmed conversation.
4. Keep process termination, native writer ownership, prompt sending, and manual
   display links as separate authorities. Metadata alone enables none of them.
5. Bound snapshots, caches, retries, and I/O. Do not load turns, parse prompt text
   for identity, read PTYs, intercept traffic, install a gateway, or change PATH.
6. Validate two ordinary CLIs in the same directory, opened before Lume; detect
   both without a first prompt; rename/resume/fork one; close only one; open the
   same conversation in two clients; restart Lume; reject old/stale evidence.

A producer-side proposal or additional native integration needs separate approval
before modifying/distributing Codex, registering new global integrations, or
publishing an upstream issue/PR. No such change is authorized or installed by this
prototype. Until a supported source is verified, the automatic shared-daemon
acceptance gate remains open rather than being replaced by a heuristic.

## Verified native-source audit and upstream proposal

This is a proposal, **not an existing Codex API**, and is not installed or posted
upstream. Sources were checked against TUI `rust-v0.159.2` and daemon
`rust-v0.159.3`; the current default-branch protocol was also checked for a new
presence interface.

### What the native code establishes

- The daemon already owns live connections and a connection-to-thread subscription
  map in [thread_state.rs](https://github.com/openai/codex/blob/01fc69f4026735edfdf6789820549727a4867b11/codex-rs/app-server/src/thread_state.rs#L342).
  These are internal memory, not public snapshots or a local presence database.
- A subscription is not necessarily the conversation displayed by the TUI. The
  TUI can keep other threads subscribed while its active thread changes in
  [session_lifecycle.rs](https://github.com/openai/codex/blob/rust-v0.159.2/codex-rs/tui/src/app/session_lifecycle.rs#L692).
  Selection therefore needs an explicit native signal, not inference from the set
  of subscribers.
- Transport closure is detected before outstanding requests finish draining in
  [app-server/lib.rs](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/app-server/src/lib.rs#L1146).
  Presence must expire at transport loss, not wait for subscription-manager
  cleanup or the loaded-thread inactivity grace period.
- Ordinary `initialize` with the current Lume observer name attempts to set the
  global originator, updates the global user-agent suffix and can affect initial
  gateway-login policy. Only two reserved internal client names bypass originating
  behavior in [initialize_processor.rs](https://github.com/openai/codex/blob/01fc69f4026735edfdf6789820549727a4867b11/codex-rs/app-server/src/request_processors/initialize_processor.rs#L130).
  Do not impersonate one of those clients as a workaround.
- The capability files are persistent task-reference caches, not TUI leases;
  shutdown does not remove them. TUI-hosted dynamic MCP receives the thread ID on
  tool calls, not a passive selection snapshot. See
  [app_server_session.rs](https://github.com/openai/codex/blob/rust-v0.159.2/codex-rs/tui/src/app_server_session.rs#L535)
  and [dynamic_tools_mcp.rs](https://github.com/openai/codex/blob/rust-v0.159.2/codex-rs/tui/src/dynamic_tools_mcp.rs#L107).
- The apparent `protocol_requests` identity fields were a misleading source hint:
  [chatwidget/protocol_requests.rs](https://github.com/openai/codex/blob/rust-v0.159.2/codex-rs/tui/src/chatwidget/protocol_requests.rs#L168)
  logs a file diff, not a typed RPC envelope. Never derive bindings from that body.
  Analytics is best-effort and lacks a complete disconnect lifecycle; log SQLite
  fields also do not provide a structured client-to-thread presence map.

### Minimal native change to propose

1. Introduce a genuinely non-originating observer handshake. It must not alter
   originator, user-agent, login policy, shared config, subscriptions, or writer
   state. An observer's own connection must never appear as an agent session.
2. Mark authenticated local transports distinctly. Today both local Unix sockets
   and TCP WebSockets share `ConnectionOrigin::WebSocket`, so that enum alone
   cannot authorize a local-only presence export. Preserve verified OS user/peer
   information without exporting credentials or environments.
3. Publish an opt-in, metadata-only snapshot of live native client connections.
   Keep active displayed thread, subscribed threads, and writer owner separate.
4. Have the TUI announce selection after opening/resuming/forking and when switching
   conversations, before any prompt. Publish clearing/closure through its native
   connection lifecycle; do not ask the model to invoke a tool or hook for this.
5. Provide ordered deltas plus a race-free initial snapshot. Register the observer
   and take its snapshot atomically; otherwise a disconnect between those steps
   can leave a ghost client. Invalidate all old observations on daemon restart.
6. Send only to authorized opt-in local observers. An empty recipient set must do
   nothing: the existing
   [send_server_notification_to_connections](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/app-server/src/outgoing_message.rs#L761)
   treats an empty list as broadcast, which is unsuitable here.

Proposed minimal metadata (names are conceptual, not implemented wire fields):

| Field | Purpose |
| --- | --- |
| Daemon instance + snapshot/event revision | Reject observations from an old server and detect gaps. |
| Connection ID + native client kind | Distinguish live TUI instances from Lume observers and server jobs. |
| Selected native thread ID or explicit no-selection | Identify the visible conversation without requiring a first prompt. |
| Attached/subscribed native thread IDs | Describe background subscriptions without promoting them to open CLI cards. |
| Optional OS-verified PID + process incarnation | Separate authority for process actions; never trust a client-asserted PID. |

No prompts, turns, previews, paths, environment variables, capability tokens,
permission answers or user content belong in presence records. Keep existing
private-socket authentication; bound clients, subscriptions, frame size, cache,
event backlog and retry budget. Presence must not create a cloud telemetry path.

### Lume consumer and completion gate

Lume would discover the existing daemon, negotiate this supported capability,
consume the initial snapshot and deltas, and fetch names by the exact native
thread ID. It would merge verified CLI and native observations into a single
conversation card, retain a separate set of attached clients, and remove an
external card when its last displayed-client attachment ends. User-started Lume
chats remain governed by Lume's existing managed-session lifecycle.

Missing capability means **unknown**, not historical-thread promotion. Never send
an invented method to existing Codex releases or infer a selected conversation
from directory, timestamps, subscriptions, or matching client/thread counts.
Metadata cannot grant prompt sending, writer takeover, or process termination.

Before runtime integration, use owned fake-client fixtures for snapshot/delta
ordering, missed disconnect, restart/connection-ID reuse, multiple displayed
clients on one thread, background-only subscriptions, an observer connecting,
unauthorized network clients and empty observer-recipient sets. Then run the
ordinary-CLI acceptance gate above on Linux, Windows and macOS. Builds alone do
not establish that gate.

**Status:** native investigation and local observer safety correction complete;
exact shared-daemon identification is still pending native capability support.
An isolated native-source patch is now available for evaluation (below); no
installed binary, global integration or public issue/PR has been changed.
Distributing a patched Codex would introduce a separate update and compatibility
responsibility and requires an explicit product decision.

### Isolated TUI producer prototype

The evaluation patch takes a smaller path than the proposed daemon RPC: the
local TUI publishes its **displayed** conversation directly. Lume need not connect
to the daemon, register an originator, infer subscriptions, inspect prompts or
wait for model activity. This is an alternative implementation to evaluate, not
an API shipped by the official Codex CLI.

Artifacts live in `prototypes/codex-client-presence/`:

- `0001-local-tui-presence.patch`, then `0002-foreground-lifecycle.patch`:
  native producer and foreground-lifecycle wiring against official
  `rust-v0.159.2`, commit `ff6aec96948b70d94983af2641a6b67c94faeff5`. Apply
  both, in that order, only to a disposable source checkout for evaluation.
- `validation/`: standalone producer/reader tests, including the same producer
  source and unit tests carried by the patch; no desktop integration or binary
  installation step.
- `evaluation.json`: pinned source, artifact hashes, validation results and
  uncompleted gates.

This base is **not** the separately audited `rust-v0.159.3` daemon source, nor a
claim of compatibility with every installed Codex version. Future rebases need
their own native lifecycle and runtime checks.

On Unix, publication requires an existing opt-in marker:

```text
<CODEX_HOME>/client-presence-v1/ENABLED  (0600, exact bytes: version=1\n)
<CODEX_HOME>/client-presence-v1/         (0700, owned by the current user)
  <client-instance-uuid>/              (0700)
    lease                             (0600, exclusive OS file lock)
    state.json                        (0600, atomic metadata snapshot)
```

The producer does not create the marker or repair permissions. The Codex home
must be owned by the current user and not writable by group/others; a home with
mode `0775` fails closed when opt-in is present. The user's existing permissions
were **not** changed. Remote TUI targets do not publish local presence. Windows
publication is deliberately disabled pending ACL, lock and native-runtime work.

Snapshot fields are limited to version, client instance UUID, asserted process
ID, monotonic revision, selected thread UUID (explicitly nullable), and `tui`
client kind. They contain no title, directory, prompt, history, environment or
token. **An asserted PID or a busy lease never grants process-control authority.**

Each TUI instance has its own stable lease inode. A normal close releases the
lease and removes only its own record; abrupt process exit leaves files but
releases the lock. The reader accepts a sample only when the same lease remains
busy before and after a bounded read, and ownership, permissions, inode paths
and schema remain valid. UUID instances prevent PID reuse from becoming client
identity. Closing a TUI can race a sample: presence is an observation at sampling
time, not a promise of continued liveness.

Selection changes write a bounded snapshot via atomic rename; unchanged selection
does not write again. There is no heartbeat, background publisher, network path
or per-token write. Snapshots are ephemeral and are not fsynced. This limits the
mechanism's work, but is not a measured end-to-end performance guarantee.

Remaining product gates:

1. Compile and execute the complete patched TUI against the pinned upstream
   toolchain/dependencies, including start/resume/fork/switch/reconnect/exit.
2. Test ordinary CLIs before the first prompt and after rename, in both embedded
   and shared-daemon modes, using disposable conversations rather than real
   ongoing chats. Windows and macOS require their own native validation.
3. Integrate the Lume reader and deduplicate by exact native thread ID while
   retaining separate client attachments; names remain a read-only metadata
   lookup, never a conversation-count heuristic.
4. Decide explicit opt-in UX, ownership checks on all platforms, and bounded
   cleanup of crash leftovers. The prototype reader rejects scans beyond 256
   directory entries or 128 clients rather than returning a misleading partial
   set. It never removes another process's records.
5. Handle fork-without-exec descendants that can temporarily retain a lease;
   close-on-exec prevents ordinary exec children from retaining descriptors but
   is not proof that the original asserted PID is alive. Do not authorize
   takeover/termination from presence records.

Validation on Linux: **22 standalone test cases passed**, including two owned
fake processes before any prompt, independent close, abrupt exit without Drop,
close-on-exec flags/inode checks, malformed data, unsafe permissions, symlinks,
FIFO records and replaced leases. Standalone Clippy with warnings denied and
format checking passed. Both patches applied cleanly to a fresh pinned checkout;
the producer and unit-test copies in `validation/` were byte-identical to the
applied native source.

Reproduce from `prototypes/codex-client-presence/validation`:

```sh
cargo +stable test --locked --offline -- --test-threads=2
cargo +stable clippy --locked --offline --all-targets -- -D warnings
cargo +stable fmt --all -- --check
```

The standalone run used Rust `1.97.1`; the native upstream checkout pins `1.95.0`.
`--offline` requires the locked crates to be available in the local Cargo cache;
an initial `cargo fetch --locked` is needed in a fresh evaluation environment.
No test creates a real Codex conversation, connects to its daemon, enables a
user-home marker, or uses an asserted PID for process actions. Owned subprocesses
are controlled solely through their test-created child handles.

**Not validated:** the App lifecycle tests and complete native TUI build/run.
The offline native test stopped at an uncached pinned `tungstenite` Git
dependency, and the native Clippy/fix attempt at `crossterm`. The whole upstream
`just fmt` script also could not finish its non-Rust tooling stages. No
pass is claimed for those gates. Real CLI acceptance, Windows and macOS remain
pending; no current Lume build consumes this proposed signal.

## Enable and inspect

From a source checkout after building the native binary:

```bash
./src-tauri/target/debug/lume identity-probe enable
./src-tauri/target/debug/lume identity-probe status
./src-tauri/target/debug/lume identity-probe disable
```

On Windows use `src-tauri\target\debug\lume.exe`. The report is JSON. No desktop
or WebView is started by these commands. `disable` keeps the diagnostic records.
An optional `--state-dir <absolute-private-directory>` selects isolated test
storage; hooks use `LUME_IDENTITY_PROBE_STATE_DIR` if explicitly provided.

The default store is a private `identity-probe` directory beside the existing
Lume Node state directory. Unix permissions are `0700` / `0600`; Windows uses the
current user's local application-data directory and its inherited ACLs. A custom
directory on Windows must likewise be private to the current user.

The **existing** `lume hook codex` handler captures `SessionStart`,
`UserPromptSubmit`, `Stop`, and `SessionEnd` if delivered. Nothing is reinstalled:
the currently configured hook command must point at this build. `SessionEnd`
is not automatically added to the configured hooks.

Codex requires trust of the exact hook definition. Review `/hooks` in the CLI;
the probe's `enabled` field means only that local recording is enabled, not that
Codex has trusted/run its hooks. Existing sessions may not send another startup
event. Lack of a record is **not** proof that no CLI is open.

## Evidence and limits

### Automatic recovery

When an existing trusted lifecycle hook reports a genuine CLI ancestor, Lume
recovers that CLI's thread and name automatically. A captured `SessionStart` is
sufficient: no first prompt or manual link is required, including when the CLI
started before the desktop. The hook command must already point at this build,
but diagnostic recording need not be enabled. Cached startup evidence cannot
identify a CLI that never supplied it. These automatic links are transient, keyed by exact
PID/process birth/OS boot, and never grant native control capabilities.

An earlier Unix observer attempted metadata RPCs on the existing official daemon
socket, restricted to `initialize`, `thread/loaded/list` and
`thread/read(includeTurns=false)`. Native-source inspection showed that even the
initialization could mutate daemon policy. **It is now disabled in production,
including diagnostics:** no worker, socket or RPC is started. Transport/parser
code is retained only for synthetic tests. Read-only local title lookup and
already verified lifecycle ancestry are independent of this disabled observer.

Loaded daemon threads are **not** open CLIs: Codex retains inactive threads for
up to 30 minutes after losing subscribers. The current protocol does not expose
the local CLI PID-to-thread mapping. Such observations remain unbound, even if
only one loaded thread seems likely. Shared-daemon automatic association is not
solved by this prototype.

### Display fallback

The application now separates **CLI presence**, **conversation identity**, and
**writer ownership**. Loaded-only native CLI observations stay internal. Actual
root prompt/activity hooks restore the named conversation, messages and events
as a **read-only monitor**, even when the shared server cannot identify its CLI.
That monitor carries no guessed PID and grants no prompt, question/approval
response, Take Control, interruption or termination capability. A confirmed link
deduplicates it with the CLI card; unresolved CLI cards may still coexist with
monitors because their correspondence cannot be guessed safely.
An unresolved CLI is labeled
"CLI não identificada", not named after its directory. Historical `source=vscode`
is not proof that the current client is the VS Code extension.

Use **Vincular conversa** in the Orb or Workspace session context menu
to select the conversation actually open in that CLI. This is a display and
monitoring association only: it does not resume a thread, change its permissions,
unlock prompts/Take Control, or terminate anything. The source remains CLI.
Titles are refreshed from the local Codex index; old rollout failure/running
statuses do not become the new CLI's status without fresh activity.

Links are stored as small metadata in the existing Lume database, separately
from managed sessions. PID, process birth marker and OS boot must match, including
when confirming the dialog. Closing/restarting the CLI invalidates its link.
No association is inferred from directory, recent timestamps, or loaded daemon
threads on Linux or Windows. If a shared-daemon CLI switches conversations without
providing a verified process/thread event, its manual link must be updated by the
user. This fallback does not remove that Codex protocol limitation.

The normal recorder stores verified lifecycle ancestry and real root-turn
activity/end metadata in the
private `cli-identity` directory alongside `identity-probe`. It has the same
128-record, 7-day, 2 MiB limits and 25 ms database wait, and captures no prompts,
commands, transcript, environment, or credentials. Unbound root activity is used
only to restore monitoring eligibility (24-hour limit; SessionEnd revokes it),
never CLI liveness or ownership. SessionStart alone, including compaction, cannot
create monitoring eligibility. Recovery does not create sessions from history;
the conversation must also arrive through the event pipeline. Explicit subagent
hooks are excluded before normalization. The diagnostic recorder remains optional;
disabling it does not disable normal identification. Previously captured probe
evidence is reused only while that probe remains enabled. Neither recorder changes
manual-link permissions.
The default diagnostic report includes normal capture when explaining CLI
bindings; an isolated `--state-dir` report does not read the user's normal store.

- `verified_hook_ancestry`: a delivered lifecycle hook has a genuine Codex CLI
  ancestor whose PID, process birth and OS boot still match the live process.
  The current thread name is read from Codex's index/database read-only.
- `shared_server_unbound`: the hook came through shared daemon infrastructure;
  the thread is known but its physical CLI is **not** assigned.
- `ambiguous_cli_ancestry`: multiple nested CLIs/wrappers were seen; the probe
  does not choose one. Precise process-birth verification is implemented on
  Linux and Windows; other systems remain unverified in this initial prototype.
- `lume_host_unbound` / `editor_host_unbound`: a managed app/extension host was
  reached before any CLI. Its launching terminal is not borrowed as identity.
- `launch_argument_only` / `detector_candidate_only`: candidates, not proof of
  the currently selected thread. `/resume` can make launch arguments stale.
- `unresolved` / `ambiguous_*`: no trustworthy unique association. No fallback
  to the directory name, latest chat, loaded-thread list or temporal proximity.
- `process_not_visible` / `pid_reused_or_new_boot`: the stored process evidence
  no longer matches. A stored record is never treated as CLI liveness.

Native thread-only records never become agents or conversation cards. This
prototype replaces unresolved directory labels with "CLI não identificada" and
recovers titles automatically only when exact ancestry is available. Display
confidence is not authorization to stop a process or take control.

## Privacy and cost

Only the native thread ID, whitelisted event/start type, observation time, and
process/boot metadata are stored. There are no prompts, commands, file contents,
transcript paths, working directories, model credentials or permission answers.
Tool and permission hooks are not recorded. The recorder has no background loop
or network connection and performs at most ten ancestor refreshes per eligible
hook. SQLite has a 25 ms busy timeout, a 2 MiB database cap, at most 128 deduplicated
records and seven-day retention. Recorder errors fail open with no hook output.
Retention filters old records immediately in reports; physical pruning occurs
on the next successful recording. The daemon observer makes no production
connections and starts no polling worker. Its isolated socket fixtures retain
the original limits of 64 metadata records, 512 KiB messages, 1.5-second I/O and
12-second cache expiry. Diagnostics never grant ownership.

## Desktop pipeline diagnostics

While the probe is enabled, `identity-probe status` also includes
`desktopRuntime`: the latest PID-only snapshot for discovery, internal state,
the Orb response, and the Workspace response. Each includes the desktop host
PID, observation time, unique Codex PID count, up to 128 PIDs, and a failure
flag. A missing or old UI-stage record means that view has not recently queried
the desktop; it is not evidence that the CLI closed.

Only four records are retained, replacing the previous record for each stage.
Writes are throttled to once per two seconds per stage in the existing private,
size-limited probe database. No titles, prompts, commands, environment values,
or credentials are recorded. Disabling the probe stops these writes.

Process discovery refreshes command arguments for agent and CLI-launcher
processes before its filtering rules. `exec` can change those arguments while
keeping a PID and birth time; unrelated process arguments remain cached.

## Manual validation

1. Enable the probe. Review the existing Lume hooks in Codex's `/hooks` if needed.
2. Open/resume CLIs both before and after opening Lume, including several in the
   same directory. Inspect `identity-probe status` **before sending a prompt**.
3. Compare startup records for normal Codex and `codex --no-daemon` rather than
   assuming the latter necessarily isolates every installed Codex version.
4. Switch threads inside one CLI. Check that newer hook evidence supersedes the
   initial launch UUID without creating another endpoint.
5. Close one CLI and run status again: its stored record must not count as live.
6. Open Lume again: records should survive, but dead PIDs must remain unbound.
7. Confirm normal typing and unchanged permission behavior throughout.

If the shared daemon exposes no exact CLI identity, keep the report unbound. The
remaining product step is an upstream per-client ownership signal or another
verifiable association source, **not** another PID/time-based heuristic.

Official contracts: [Codex hook inputs and lifecycle](https://learn.chatgpt.com/docs/hooks#common-input-fields),
[hook trust review](https://learn.chatgpt.com/docs/hooks#review-and-trust-hooks).
[App Server metadata and lifecycle](https://developers.openai.com/codex/app-server).
