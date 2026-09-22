# Distributed Lume threat model

Status: Phase 0 baseline. Revisit before enabling any internet route or public beta.

## Assets and trust boundaries

No networked design can guarantee zero compromise probability. The safe fallback for users who require zero residual network exposure is to leave remote access disabled. Internet access must be opt-in and Relay-only by default; direct peer-to-peer routing, router port mapping and public model-server listeners are not implicit fallbacks. The Relay can still observe source IPs and connection metadata even when content is end-to-end encrypted.

Protected assets include prompts, responses, project files, diffs, artifacts, credentials, model access, tool permissions, execution state and device identities.

The Node is the authority for projects and executions on its machine. A paired client is trusted only for its granted scopes. Rendezvous and Relay infrastructure is outside the content trust boundary. Local model servers, agent CLIs and project directories are not made network services by pairing.

## Threats and controls

| Threat | Required control |
|---|---|
| Pairing offer theft | High-entropy, single-use, short-lived offers; visible peer identity and scope confirmation; failed-attempt limit |
| Short-code guessing | PAKE or visual short-authentication confirmation; never derive a persistent key directly from the code |
| Captured command replay | Authenticated encryption, command expiry, stable idempotency key and retained original outcome |
| Captured event replay or reordering | Monotonic sequence, stable event ID, duplicate rejection and snapshot recovery on gaps |
| Protocol downgrade | Authenticated version/feature negotiation and fail-closed mandatory capabilities |
| Malicious paired client | Least-privilege scopes, project allowlists, existing approvals, rate limits, audit history and independent revocation |
| Compromised Relay | End-to-end content encryption with peer keys; no session key at the Relay; document unavoidable routing metadata |
| Path traversal or symlink escape | Canonicalize both root and target, verify containment after resolution and reject special or sensitive paths |
| Duplicate prompt after reconnect | Stable execution ID, command idempotency and resume from an event cursor rather than resubmission |
| Oversized payload or queue exhaustion | Per-channel limits, bounded pending commands, artifact chunking, backpressure and connection quotas |
| Stale or stolen device key | Independent device revocation, key rotation, short-lived session keys and immediate remote-access kill switch |
| Compromised Node | Treat all Node-readable data as exposed; keep project roots narrow and never reuse service credentials across Nodes |
| Compromised client | Limit scopes, avoid storing project data by default and allow the Node to revoke the device immediately |
| Resource abuse by local inference | Advertised capacity, bounded queues, task timeout, explicit cancellation and CPU/RAM/GPU limits where supported |
| Node slows model inference | Benchmark against the same model with Node disabled; bound polling, queues and worker priority; suspend nonessential telemetry under load; block remote-inference release if measured contention exceeds the agreed budget |

## Execution safety

Only Node-managed executions accept remote mutation. Observed external sessions remain read-only until an explicit ownership transfer succeeds. Ownership generation prevents a stale controller from issuing commands after takeover or reconnect.

Interrupting an execution and terminating its owning process are distinct capabilities. Neither is inferred from a dropped socket. A disconnect changes presence, not execution status.

## File and artifact safety

- Read and mutation roots are configured independently.
- Relative paths containing traversal are rejected before access, then canonical containment is checked again.
- Symlinks, junctions and Windows path prefixes are evaluated at the Node, not trusted from a client.
- Sensitive-file filtering and context redaction run before serialization.
- Downloads use explicit manifests, size bounds and digest verification.
- Upload and extraction support must reject archive traversal and decompression bombs before it is introduced.

## Metadata and privacy

Even with end-to-end encryption, a hosted service may observe paired routing identifiers, source IP addresses, connection timing, duration and ciphertext size. Logs must avoid content and use short retention. Product privacy documentation must disclose region, retention, subprocessors and account behavior before the hosted service ships.

## Security gates for later phases

Before any remote prompt or local-model capability is enabled, perform an independent security review of both endpoints, fuzz malformed inputs, verify per-request authorization and revocation, test compromised-client and compromised-Relay scenarios, and benchmark inference with Node disabled and enabled. A failed gate leaves the capability disabled.

Before Phase 5 internet pairing:

- independently review the handshake, key storage, revocation and downgrade behavior;
- test replay, expiry, reconnect, scope denial and malformed-frame cases;
- document key recovery and device-loss behavior.

Before Phase 6 Relay:

- prove the Relay cannot derive content keys;
- add per-device quotas, backpressure and abuse controls;
- test that direct-to-Relay route changes do not duplicate a command.

Before remote project mutation:

- test canonical path containment on Linux and Windows;
- retain human approval for privileged tools and sensitive context;
- record actor, device, execution and idempotency identities in the local audit history.
