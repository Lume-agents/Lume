# Lume Remote — hosted Relay design

Status: phase 1 (Relay MVP) running as a prototype, owner only. The Relay lives in
[Lume-agents/lume-relay](https://github.com/Lume-agents/lume-relay) (AGPL-3.0) and is deployed at `relay.lumeagents.dev`;
the Node side is `relay_link.rs` (connection, signed challenge, pairing, authorization), `relay_e2e.rs` (end-to-end
encryption) and `node_relay.rs` (the Node's outbound connection, the commands it answers and the client queries) in
`src-tauri`. The running Node connects to the Relay, devices pair through it from **Settings → Remote computers** or
`lume node relay-pair`, and a paired device can read the other computer's health, inventory and agent sessions, plus a
session's conversation when the owner allows it. It can also send a prompt to a session the app controls and allow once or deny what a session waits for, each behind
its own owner switch, and stop a running turn. There is no streaming yet: views refresh every few seconds. Relay access
is between computers (the app and the `lume node relay-*` commands); the phone stays on the local network for now
([MOBILE.md](../MOBILE.md)). Accounts, entitlement tokens, billing and push do not exist, so the Relay has no per-account quota, and
the independent security review has not happened. It extends [LUME_NODE.md](./LUME_NODE.md) and must satisfy
[DISTRIBUTED_THREAT_MODEL.md](./DISTRIBUTED_THREAT_MODEL.md), including its security gates before any internet pairing.

## Product decisions

- **History stays on the user's computer.** The Node remains the authority for projects, executions, conversations,
  events and diffs. The hosted service never stores them in readable form.
- **The Relay is blind.** It routes end-to-end encrypted frames between paired devices. It cannot read prompts,
  responses, files or approvals, and it holds no session key.
- **Hosting:** Cloudflare (Workers, Durable Objects, D1). **Billing:** Paddle (merchant of record, handles sales tax).
- **Pricing (planned):** Lume Remote US$ 4 / R$ 14,90 per month. Lume Premium later, once there is more to add.
- **Licensing:** the desktop app stays MIT. The Relay and control plane are published in a separate repository under
  AGPL-3.0, so anyone can self-host but nobody can run a closed fork as a service. Contributors to that repository
  sign a CLA so a commercial license stays possible.

## What the service stores

| Data | Where | Retention |
|---|---|---|
| Conversations, events, diffs, files | The user's Node only | Local |
| Frames in transit | Relay (Durable Object memory) | Bounded replay queue, minutes, ciphertext only |
| Account, subscription state | Control plane (D1) | While the account exists |
| Paired devices: id, name, **public key**, scopes, last seen | Control plane (D1) | Until the device is revoked |
| Push tokens | Control plane (D1) | Until the device is revoked |
| Request logs | Cloudflare logs | Short retention, no content, no IP beyond abuse handling |

Metadata a hosted Relay can unavoidably observe: device ids, source IPs, connection timing and duration, ciphertext
size. This is disclosed in the privacy policy (LGPD/GDPR roles, region, retention, deletion on request).

## Components

```
 Phone / PC2 ──wss──▶  Relay (Durable Object, one per Node)  ◀──wss── Node on PC1 (outbound only)
                               │  ciphertext only
                               ▼
 Control plane (Worker + D1): accounts, entitlement tokens, device registry, push dispatch, Paddle webhooks
```

1. **Relay.** One Durable Object per Node, using WebSocket Hibernation so an idle Node costs almost nothing. The Node
   opens an outbound connection (no router ports). Clients attach with a short-lived token. The Relay forwards
   frames and keeps a bounded queue per peer for reconnection, using the protocol's `EventCursor`, stable event ids
   and command idempotency keys. It enforces per-connection quotas, frame size limits and backpressure.
2. **Control plane.** A Worker with a D1 database:
   - sign-in (magic link and GitHub/Google OAuth);
   - **entitlement tokens**: after verifying the subscription it issues short-lived signed tokens the Node and
     clients present to the Relay. Lapsed subscription means no new tokens; the local app is unaffected;
   - device registry (public keys, scopes, revocation) and the pairing handshake (PAKE or visual confirmation, per
     the threat model);
   - Paddle webhooks for subscription state.
3. **Push.** A Worker sends content-free pushes (FCM, APNs, Web Push): "an agent needs you". The Node, not the
   server, decides when to notify. Payloads carry no prompt or response text.
4. **Clients.** The Node gains a Relay transport (`TransportKind` already allows several). Mobile and a second Lume
   install connect through it. Everything else in the product keeps working offline and on the LAN.

## Keys and pairing

- Each device keeps its Ed25519 identity (the Node already does). Content is encrypted with keys derived between the
  two paired peers; the server only ever sees public keys.
- Pairing reuses the Node's single-use, short-lived offers with explicit scope confirmation. The control plane
  records the pairing; it cannot decrypt traffic.
- Revocation is independent: the user can remove a device from the Node, and the control plane drops its tokens.
- Losing the Node's key means re-pairing. There is no server-side recovery, by design.

## Phase 1 hardening (client and Node)

Everything here works with the current Relay; each new field is optional, and its absence keeps the earlier behavior.

- **Offline Node.** A device fails at once when the Relay's `ready` frame says `nodeOnline: false`, before it sends a
  handshake, and also on a `presence` frame for the Node going offline while it waits. A Relay that sends neither
  leaves the device waiting for its answer timeout.
- **Replayed frames.** The Node drops a handshake the Relay marks `replayed: true`, and a handshake identical to the
  one that opened the current session, so neither can replace a live session or end its authenticator grant. A sealed
  frame carries a counter the receiver requires to increase.
- **Reset.** A sealed frame for a session the Node does not have (it restarted or reconnected) is answered with a
  plaintext `0x04` frame holding 16 bytes of that frame's SHA-256. The device acts only on a reset naming the frame it
  just sent: it reconnects and retries once. A reset is not authenticated; a forged one can do no more than a Relay
  that drops the frame. Replayed frames never get a reset.
- **Idempotency.** Prompts, approvals and stops carry a request id and a send time (refused outside ±60 s). The Node
  remembers each request by device and id for 24 hours (at most 4096) in `relay-requests.json`, private to the owner,
  with its answer and a digest of the command, session and payload. A repeat gets the first answer without running
  again, a request cut short by a Node stop is not run again, and the same id with a different request is refused.
  The app receives `relay:<device>:<id>`. A device retries an action once, with the same id, only after a transport
  failure.
- **Audit.** Pairing, authenticating, prompts, approvals and stops through the Relay are appended to
  `remote-audit.jsonl` in the Node's data directory (owner only, rotated at 1 MiB with one previous file kept): time,
  device id, fingerprint and name, command, session, request id, route and outcome (`accepted`, `repeated` or
  `refused:<reason>`). A prompt contributes only its length. `lume node audit [N]` prints the recent records.
- **Limits per device.** About 30 commands per 10 s, 10 prompts, approvals, stops or authentications per minute, and
  one handshake per 2 s. Commands over the limit get a sealed `busy` error; handshakes over it are dropped. Frames
  larger than any command are dropped, and an unreadable frame from the Relay is logged without ending the
  connection.

### Relay-side follow-ups (in lume-relay)

- Send `nodeOnline` in `ready` for clients, so an offline Node is reported before the handshake.
- Keep marking queued frames `replayed: true`, and do not replay handshakes or frames older than the request window
  (60 s) at all.
- Per-account and per-key limits for opening rooms, and limits per peer in addition to per connection.
- Optionally answer a client's frame to an offline Node with a `presence` frame instead of queuing it.

## Self-hosting

The same Relay and control-plane code runs on the user's own Cloudflare account, or as a single container with SQLite
for the registry. Self-hosters skip Paddle and entitlement checks.

## Phases and gates

1. **Relay MVP (closed beta, owner only):** Durable Object relay, outbound Node transport, pairing, no billing. *Running, owner only; prompts, approvals and stops behind owner switches, with request memory, audit and per-device limits.*
2. **Accounts and push:** sign-in, entitlement tokens, push dispatch, device revocation.
3. **Billing:** Paddle subscriptions and webhooks, in USD and BRL. Only after measuring real usage and cost.
4. **Security review:** independent review of the handshake, key storage, revocation and downgrade behavior; replay,
   expiry, reconnect, scope-denial and malformed-frame tests; compromised-client and compromised-Relay scenarios.
   **No public launch before this passes.**
5. **Public launch** with a privacy policy and terms, then Lume Premium.

## Open questions

- Account recovery and what happens to paired devices after a subscription lapses.
- Replay queue size and TTL per plan; abuse limits per account. Today any key can open a room and the limits are per room
  and per connection only.
- Telling a joining client that its Node is offline: closed on the client side (it honors `nodeOnline` in `ready` and
  a Node `presence` frame); the Relay still has to send `nodeOnline` (see the follow-ups above).
- Regions: Durable Objects are placed near the first client, which may add latency for the other peer.
- Whether an optional end-to-end encrypted backup (user-held key) is ever offered; it would be opt-in and
  unrecoverable if the key is lost.
