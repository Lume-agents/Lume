# Distributed execution roadmap

Status: Phase 0 implemented locally; Phase 1 in progress. This roadmap extends Lume from a local agent workspace into an optional distributed workspace without removing or weakening its fully local mode.

## Product outcome

A user can run Lume on a lightweight client, pair a trusted computer elsewhere, and use the models, agents, projects and compute available on that computer without streaming its screen.

The first target is a work laptop controlling a local model and coding agent running on a home PC. The same architecture later serves desktop, Android, iOS/PWA and hybrid cloud/local workflows.

```text
Lume Desktop / Mobile / Web
              |
      encrypted connection
              |
      direct P2P or Relay
              |
          Lume Node
        /      |       \
 local model  projects  Git and tools
```

Screen sharing, remote desktop and full IDE streaming remain complementary tools. They are not part of the Lume transport.

## Non-negotiable principles

- Local-only operation remains available and requires no Lume account, Relay or internet connection.
- Remote access is opt-in per Node and disabled by default until pairing is completed.
- Remote access must never be presented as risk-free. If zero residual network risk is required, keep Node and Relay disabled and use local-only Lume.
- Internet connections use an outbound-only, Relay-only privacy mode by default: no router port forwarding, UPnP, public inference port or direct peer-to-peer fallback without a separate explicit choice. The Relay still observes connection IPs and timing, which must be disclosed.
- The Node never replaces agent binaries, shadows commands in `PATH`, or intercepts an externally owned CLI.
- Lume controls only sessions it owns or sessions whose ownership was explicitly transferred.
- Ollama, llama.cpp and other inference ports are never exposed directly to the public internet by Lume.
- The Relay transports encrypted payloads and cannot read prompts, responses, files, diffs, commands or credentials.
- Every remote device receives explicit scopes and can be revoked independently.
- Project roots, tools and mutation permissions are allowlisted on the Node.
- A client disconnect does not terminate a running task, repeat a prompt or lose its execution identity.
- Cloud agents receive local source or context only after the existing context and approval policies permit it.
- Lume clearly shows which device and provider execute each task.
- Corporate source code must not be transferred to a personal Node without authorization from its owner or employer.
- Remote execution does not ship until independent security review, adversarial tests and representative performance benchmarks pass. Failure keeps the feature disabled without affecting local Lume.

## Product components

### Lume Client

Orb, Workspace, floating terminals and mobile remain interfaces over the same session model. A client can:

- list paired Nodes and their presence;
- inspect available models and agent capabilities;
- open remote chats and managed coding sessions;
- stream structured activity, output, diffs and approvals;
- queue, interrupt and resume supported work;
- download an explicitly shared result;
- revoke its own connection or request a new scope.

The Workspace groups sessions by execution device, for example `This device` and `Home PC`, while allowing local, cloud and remote-local agents in the same layout.

### Lume Node

The Node is a lightweight background service installed with Lume or explicitly enabled on a machine with compute and projects. It owns:

- model-runtime discovery and health;
- managed agent processes and durable execution state;
- project-root access and tool sandboxing;
- local Git status, diffs, checks and artifacts;
- permission requests and capability enforcement;
- encrypted client connections;
- bounded event history for reconnection;
- resource reporting for CPU, RAM, GPU, VRAM and queues;
- automatic updates compatible with the desktop release channel.

The Node remains useful over LAN without the hosted Relay.

### Rendezvous service

A small hosted control plane helps already paired devices find one another. It stores only the minimum required to route a connection:

- public device identity;
- online/offline presence with short expiry;
- connection candidates and protocol version;
- revoked-device state;
- short-lived pairing and connection metadata.

It never stores conversations, project contents or agent history.

### Lume Relay

The Relay is the encrypted fallback when LAN or direct peer-to-peer connectivity is unavailable because of NAT, CGNAT, firewall or corporate-network restrictions.

- Both peers create outbound connections, preferably over TLS on port 443.
- Direct LAN or peer-to-peer transport is preferred when available.
- The Relay forwards ciphertext without holding the session keys.
- Backpressure, size limits and per-device quotas prevent one transfer from exhausting the service.
- Large artifacts require explicit transfer and can use a separately bounded channel.

An advanced user may choose a private network such as Tailscale, a custom endpoint or a future self-hosted Relay instead of the hosted Lume Relay.

## Pairing and device lifecycle

QR codes and short codes remain the common pairing experience. Pairing authorizes a device; the selected transport provides reachability.

1. The Node creates a single-use pairing offer with a short expiry.
2. The client scans the QR code or enters the short code.
3. Both devices authenticate the offer and exchange persistent device public keys.
4. The Node shows the requested scopes before accepting the client.
5. The client stores only the paired-device identity and encrypted credentials.
6. Future connections authenticate automatically and negotiate LAN, direct or Relay transport.
7. Either device can revoke the pairing, immediately invalidating future sessions.

Pairing must work without an account. An optional Lume account may later simplify device discovery and recovery, but it must not become mandatory for LAN-only operation.

## Connection strategy

For each connection, Lume selects the best available route without changing the application protocol:

1. authenticated LAN discovery and direct connection;
2. authenticated peer-to-peer connection after rendezvous;
3. end-to-end encrypted Lume Relay fallback;
4. user-configured private-network address.

Route changes are visible as diagnostics, not as different session types. Reconnection uses an event cursor and stable execution ID so a route change cannot duplicate a prompt.

The transport spike must compare WebRTC data channels, QUIC-based peer connectivity and a WebSocket-over-TLS fallback before the production protocol is selected.

## Protocol foundation

The distributed protocol should evolve the existing encrypted mobile hub contract rather than create an unrelated API.

Required properties:

- protocol and feature negotiation;
- device and Node identities independent of IP addresses;
- end-to-end authenticated encryption with session-key rotation;
- monotonically sequenced event envelopes and replay rejection;
- idempotency keys for every state-changing command;
- resumable snapshots plus event cursors;
- bounded queues, payload sizes and reconnect attempts;
- explicit capability advertisement instead of inferred controls;
- separate channels or priorities for state, streaming text and artifacts;
- cancellation and timeout propagation;
- client clock independence.

The Relay sees routing identifiers, connection timing and payload sizes. This metadata exposure must be documented even though content remains encrypted.

## Local model and agent integration

### Runtime adapters

The initial adapters target Ollama and llama.cpp through their local APIs. Later adapters may include LM Studio, vLLM and other OpenAI-compatible runtimes.

Each adapter reports:

- installed and loaded models;
- text, vision, reasoning, streaming and tool capabilities;
- context and generation settings supported by the runtime;
- current load, queue and sleep state;
- normalized usage and performance metrics when available.

Compatibility is capability-driven. An OpenAI-compatible route does not imply that every OpenAI feature is available.

### Managed local coding agent

An inference endpoint is not by itself a coding agent. Lume therefore needs a managed runner that provides:

- conversation and context management;
- repository-aware file tools;
- patch application and conflict handling;
- shell commands with approval and sandbox policies;
- Git, checks and changed-file attribution;
- plans, tasks, final results and structured events;
- interruption, queues and recovery;
- model and reasoning configuration per session.

This runner uses the same normalized session, approval, history and Workflow contracts as supported cloud agents.

## Hybrid workflows

Distributed execution extends the existing Workflow engine rather than creating a second orchestrator.

Examples:

- cloud Planner -> local Qwen Implementer -> cloud Reviewer;
- local Researcher -> local Implementer -> mobile approval;
- home Node implementation -> laptop review and approved GitHub delivery.

Every handoff records source device, destination device, provider, selected context policy and user approval. Full chats are never shared automatically. Workflow persistence remains authoritative on the execution host, with a recoverable summary mirrored to authorized clients.

## Security and privacy model

- Long-lived private device keys use OS-protected storage where available.
- Pairing secrets are single-use, short-lived and rate-limited.
- Session keys are ephemeral and rotated without requiring a new pairing.
- Permissions are granted by capability: observe, prompt, approve, interrupt, read files, download artifacts, run tools and administer the Node.
- Project access uses canonical allowlisted roots and rejects traversal or symlink escape.
- Sensitive-file and context redaction applies before remote transmission.
- Shell, filesystem, Git and integration actions retain their existing approval policies.
- Remote commands include actor, device, execution and idempotency identities in the local audit history.
- Device revocation, key rotation and compromise recovery are first-class flows.
- Remote access can be disabled immediately without disabling local Lume operation.

The current privacy documentation continues to describe the shipped local product until remote access is implemented and released. Before release, it must be expanded with Relay metadata, retention, infrastructure region, subprocessors and account behavior, if any.

## Delivery phases

### Phase 0 — Protocol and threat-model spike

Status: implemented locally. The typed contract is not connected to a listener and does not enable remote access. An independent security review remains a gate before internet connectivity ships.

- Inventory the existing mobile realtime protocol and session-ownership model.
- Define Node identity, client identity, scopes and execution ownership.
- Threat-model pairing theft, replay, malicious clients, compromised Relay, path traversal and duplicate commands.
- Compare direct-connection transports and define the TLS/443 fallback.
- Define payload, queue, artifact and event-retention limits.

Artifacts: [distributed protocol v1](./DISTRIBUTED_PROTOCOL_V1.md), [threat model](./DISTRIBUTED_THREAT_MODEL.md) and `src-tauri/src/distributed_protocol.rs`.

Exit criteria: reviewed protocol contract, threat model and transport decision with no dependency on CLI interception or public inference ports.

### Phase 1 — Local Lume Node

Status: in progress. The backend now includes an opt-in headless process, persistent Ed25519 identities, signed TLS binding, single-use scoped pairing, revocation, authenticated LAN discovery, bounded logs, lifecycle configuration, client-side discovery, verified pairing, reconnection after address changes and authenticated read-only health. Desktop settings can discover, pair, verify and remove remote computers. Local Node setup, startup and update controls are still pending. Development commands are documented in [the Node guide](./LUME_NODE.md).

- Package a Node process that can run with or without the desktop UI.
- Add enable/disable, startup, health, logs and safe update controls.
- Advertise Node identity over authenticated LAN discovery.
- Expose read-only machine, runtime and capability information.
- Keep resource use bounded while idle.

Exit criteria: another Lume client on the LAN can pair, reconnect and monitor the Node without affecting native CLIs or the desktop UI.

#### Phase 1 hardening TODO — blocking before remote prompts

- [ ] Restrict the Node listener to an explicitly selected local interface; test public-interface and port-forwarding scenarios instead of assuming `0.0.0.0` means LAN-only.
- [ ] Add a total request deadline, minimum read progress and per-peer connection limits so slow unauthenticated clients cannot occupy all connection slots.
- [ ] Rate-limit authenticated health requests per device; cache expensive machine/process snapshots and debounce `last_seen` persistence before adding any automatic polling.
- [ ] Distinguish **Forget on this client** from actual **Revoke access on the Node** in both API and UI; verify revocation immediately blocks an already paired key.
- [ ] Protect Node private keys and custom state directories with explicit Windows ACL checks; fail closed when permissions are unsafe.
- [ ] Reconcile every direct/P2P passage in this roadmap with the Relay-only internet default; require a separate explicit opt-in for any direct path.
- [ ] Benchmark idle Node overhead and model first-token latency/tokens-per-second against Node disabled on representative hardware before enabling remote inference.

### Phase 2 — Local inference sessions

- Add Ollama and llama.cpp discovery and adapters.
- List models and verified capabilities.
- Support encrypted streaming chat, cancellation and attachments over LAN.
- Display model, device, latency, queue and resource use in Workspace.
- Recover the UI after disconnect without losing the Node-side turn.

Exit criteria: a laptop can use a model on another LAN computer through Lume without screen sharing or a public model port.

### Phase 3 — Managed coding runner

- Add project allowlists and per-project policies.
- Implement file, patch, terminal, Git and validation tools.
- Stream normalized events, diffs, approvals and final results.
- Persist execution identity, queue and recovery state on the Node.
- Integrate model and effort controls with supported runtimes.

Exit criteria: a remote client can complete and review a controlled coding task while all file operations occur on the Node.

### Phase 4 — Durable device pairing

- Generalize the mobile pairing flow for desktop-to-Node pairing.
- Add named devices, scopes, last-seen status, key rotation and revocation.
- Add QR code and short-code onboarding.
- Unify paired-device settings across Orb, Workspace and mobile.
- Add clear diagnostics for authorization versus reachability failures.

Exit criteria: pairing survives address changes and restarts without requiring a shared Wi-Fi after initial authorization.

### Phase 5 — Rendezvous and direct connectivity

- Deploy the minimum rendezvous service.
- Negotiate direct peer connectivity after authentication.
- Prefer LAN or direct paths and migrate routes without duplicating commands.
- Add retry budgets, heartbeat and offline presence expiry.
- Verify behavior behind common NAT and CGNAT configurations.

Exit criteria: paired devices on different networks connect directly when their networks permit it.

### Phase 6 — Encrypted Relay fallback

- Deploy the TLS/443 Relay data plane.
- Keep payload encryption exclusively between client and Node.
- Implement backpressure, quotas, abuse prevention and regional health checks.
- Route state and prompt streams separately from explicit artifact transfers.
- Add optional private-network and future self-hosted Relay configuration.

Exit criteria: a paired laptop reaches the home Node through restrictive networks without port forwarding, VPN installation or Relay access to plaintext.

### Phase 7 — Unified desktop and mobile remote access

- Reuse the Node connection from Workspace, Orb, Android and iOS/PWA.
- Show remote device, route, latency and stale/offline state consistently.
- Add remote notifications without duplicating local notifications.
- Preserve read-only mobile Workflow monitoring unless a separate control scope is explicitly introduced.
- Cache only bounded, encrypted snapshots on clients.

Exit criteria: authorized clients observe the same stable session identities and recover consistently after network changes.

### Phase 8 — Hybrid Workflow execution

- Allow Workflow steps to target local or remote execution hosts.
- Apply Context Builder policy before cross-device and cloud handoffs.
- Persist handoff and execution IDs across both peers.
- Pause safely when a required Node is offline or permissions are pending.
- Prevent loops, duplicate turns and silent provider/device changes.

Exit criteria: a cloud/local multi-device workflow survives client disconnect and produces one auditable result per step.

### Phase 9 — Reliability and performance hardening

Performance baselines and release gates for remote execution start before Phase 2, not only in this phase. Phase 9 broadens that validation to long-running and multi-device workloads.

- Measure idle CPU/RAM and active overhead with multiple sessions.
- Bound chat history, event rendering, queues and artifact memory.
- Test large diffs, long turns, slow links, packet loss and reconnect storms.
- Recover from client, Node and Relay restarts without repeating prompts.
- Test sleep, wake, model unloading and optional Wake-on-LAN assistance.
- Validate Windows, Ubuntu, Pop!_OS and Fedora Nodes plus supported clients.
- Compare model first-token latency, sustained token throughput and peak CPU/RAM/GPU contention with Node disabled versus connected and active. Set measurable budgets from the baseline before release; if exceeded, keep remote inference disabled until redesigned.

Exit criteria: the Node remains lightweight at idle, long sessions stay responsive, and failure recovery is deterministic.

### Phase 10 — Distribution and operations

- Include the Node in desktop installers with an explicit setup choice.
- Add signed automatic updates and protocol compatibility windows.
- Publish Relay availability, incident and retention policies.
- Add capacity planning based on P2P success rate and relayed bandwidth.
- Provide export, device removal and complete remote-access deletion flows.
- Update README, product documentation and privacy disclosures.

Exit criteria: remote execution can be released without making the local product dependent on Lume infrastructure.

## Validation matrix

### Connectivity

- Same LAN with and without internet access.
- Different residential networks.
- One or both peers behind CGNAT.
- Restrictive guest or corporate networks permitting HTTPS only.
- Route change between Wi-Fi, Ethernet and mobile hotspot.
- Relay unavailable while direct connectivity works, and the inverse.

### Lifecycle

- Pair, reject, expire, revoke and re-pair.
- Client restart, Node restart and Relay restart.
- Laptop disconnect during streaming generation and during a tool approval.
- Node sleep/wake and model unload/reload.
- Upgrade with one peer temporarily on the previous protocol version.

### Correctness and safety

- Exactly-once prompt submission across reconnects.
- No duplicate completion, notification or Workflow transition.
- Scope enforcement for every command.
- Project-root and sensitive-file escape attempts.
- Malformed, replayed, oversized and out-of-order envelopes.
- Relay compromise simulation demonstrating ciphertext-only exposure.

### Performance

- Idle Node CPU and resident memory.
- First-token latency over LAN, direct internet and Relay.
- Sustained text/event throughput and backpressure.
- Multiple concurrent sessions and one loaded 20B- to 30B-class model.
- Large histories and diffs without unbounded client rendering.

## Initial out of scope

- Screen, audio or full desktop streaming.
- A complete browser IDE or replacement for Remote SSH.
- Public unauthenticated access to local inference servers.
- General-purpose file synchronization or cloud drive storage.
- Training or hosting user models on Lume infrastructure.
- Automatic transfer of corporate repositories to personal devices.
- Transparent interception, wrapping or replacement of third-party CLIs.
- Relay-side decryption, indexing, analytics or conversation storage.

## Recommended release sequence

- **Node developer preview** — Phases 0–2, LAN only and explicitly enabled.
- **Remote coding preview** — Phases 3–4 with trusted devices and project allowlists.
- **Remote access beta** — Phases 5–7 with P2P and Relay fallback.
- **Hybrid Workflow preview** — Phase 8 after workflow and ownership recovery are stable.
- **Remote access stable** — Phases 9–10 after cross-platform and hostile-network validation.

Tailscale remains useful for private development and advanced deployments, but it is not a mandatory end-user dependency or the default product experience.
