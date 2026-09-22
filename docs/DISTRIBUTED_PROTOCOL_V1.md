# Lume distributed protocol v1

Status: local design baseline. This contract does not enable remote access by itself.

## Purpose

The protocol lets a trusted Lume client observe and control work owned by a Lume Node without exposing an inference server, replacing an agent CLI or coupling the application contract to one network transport.

The current mobile hub is the starting point: it already has TLS identity, scoped paired devices, authenticated requests, AES-256-GCM envelopes, nonces, realtime sequence numbers and a bounded event journal. Distributed execution extends those guarantees with explicit Node identity, execution ownership, feature negotiation, command idempotency and resumable snapshots.

## Identities and ownership

- A `Node` is the machine that owns runtimes, projects, tools and managed executions.
- A `Client` is a paired desktop, mobile or web device acting for a user.
- Device identity is a persistent public-key fingerprint and does not depend on an IP address.
- A Node-managed execution has one stable `executionId`, one owning `nodeId`, a monotonically increasing ownership generation and at most one controlling client.
- An externally observed process can be monitored, but cannot receive remote prompts or mutations until ownership is explicitly transferred.
- Disconnecting a client never ends the execution. Reconnecting must reuse its execution ID and event cursor.

Scopes are granted per paired client: `observe`, `prompt`, `approve`, `interrupt`, `read_files`, `download_artifacts`, `run_tools` and `administer`. Every command declares the scope it requires; the Node remains the final authorization boundary.

## Pairing

1. The Node creates a single-use offer containing at least 128 bits of entropy with a five-minute maximum lifetime.
2. The client and Node exchange persistent public keys through the authenticated offer.
3. The Node displays the requesting device and scopes before approval.
4. Both sides store the peer fingerprint and encrypted credentials.
5. Offers are invalidated after success, expiry or repeated failed attempts.

A human-friendly short code cannot be used directly as a long-lived encryption key. It must be protected by a password-authenticated key exchange or confirmed with a visual short authentication string.

## Negotiation

Every connection starts with `ProtocolHello` containing:

- the minimum and maximum supported protocol versions;
- the device identity;
- a set of supported feature identifiers.

Peers select the highest shared version and the intersection of features. An incompatible version, invalid identity or unsupported mandatory feature fails closed. Version 1 defines capability advertisement, scoped devices, execution ownership, command idempotency, event cursors and resumable snapshots.

Signed identity statements carry the exact serialized payload bytes alongside their decoded JSON representation. Verification checks that both representations match before validating the Ed25519 signature, avoiding differences in JSON key ordering between Rust, JavaScript and future clients.

## Channels

The application protocol is transport-independent and separates traffic by purpose:

| Channel | Purpose | Maximum payload |
|---|---|---:|
| Control | Commands, acknowledgements and approvals | 64 KiB |
| State | Snapshots, session metadata and normalized results | 2 MiB |
| Stream | Text deltas and structured activity | 256 KiB |
| Artifact | Explicitly requested files in verified chunks | 100 MiB per artifact |

Artifact chunks are at most 256 KiB. Each manifest includes a stable ID, content type, total size and SHA-256 digest. Artifacts are never inferred from every changed file; they must be explicitly offered by the Node and requested by a client with the correct scope.

## Commands and idempotency

Every state-changing command includes:

- `commandId` and `idempotencyKey`;
- `nodeId`, `clientId` and optional `executionId`;
- required scope;
- issue and expiry timestamps;
- a bounded typed payload.

The maximum command lifetime is 60 seconds. A Node retains idempotency outcomes for 24 hours and returns the original outcome when a command is repeated. It never executes the same key twice. A client may keep at most 128 pending commands per Node; excess work is rejected with backpressure rather than buffered without limit.

## Events, reconnect and snapshots

Events have a stable ID and a monotonically increasing sequence scoped to a Node connection journal. Clients reject repeated IDs and sequences. A gap causes the client to stop applying incremental events and request a fresh snapshot; it must not guess or replay commands.

The baseline journal is bounded to 10,000 events and 24 hours. Older history belongs to the durable Lume history model rather than the realtime transport. A snapshot reports its final sequence so incremental delivery can resume at the next event.

## Transport decision

The same envelopes and authorization rules apply to every route:

1. **LAN:** authenticated TLS connection using the existing HTTPS/WebSocket server foundation and local discovery.
2. **Internet direct:** WebRTC DataChannel negotiated through a minimal rendezvous service. ICE provides LAN, STUN and TURN candidate handling without exposing the model API.
3. **Relay fallback:** outbound secure WebSockets over TLS on port 443 carrying application-layer end-to-end encrypted envelopes. The Relay sees routing metadata, timing and ciphertext sizes, but not content.
4. **Private network:** the same authenticated TLS protocol through a user-configured private address.

Native QUIC remains a future optimization. It is not the v1 primary route because a PWA cannot use a raw QUIC socket and restrictive networks commonly block arbitrary UDP while allowing TLS traffic on port 443.

## Local Node invariants

- The Node never shadows agent commands in `PATH` and never intercepts an external CLI.
- Model-runtime ports remain bound locally and are not published by Lume.
- Project operations resolve canonical paths inside explicit allowlisted roots and reject traversal and symlink escape.
- Prompt, approval and tool capabilities are advertised, not inferred from an agent name.
- Local-only Lume continues to work when Node, rendezvous and Relay features are disabled.

## Implementation source

`src-tauri/src/distributed_protocol.rs` contains the serializable v1 types, negotiation, envelope validation, artifact bounds and event-cursor behavior. It is intentionally separate from network listeners so Phase 1 can reuse it without prematurely exposing an endpoint.
