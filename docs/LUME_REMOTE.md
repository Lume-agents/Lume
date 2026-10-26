# Lume Remote — hosted Relay design

Status: phase 1 (Relay MVP) running as a prototype, owner only. The Relay lives in
[Lume-agents/lume-relay](https://github.com/Lume-agents/lume-relay) (AGPL-3.0) and is deployed at `relay.lumeagents.dev`;
the Node side is `relay_link.rs` (connection, signed challenge, pairing, authorization), `relay_e2e.rs` (end-to-end
encryption) and `node_relay.rs` (the Node's outbound connection, the commands it answers and the client queries) in
`src-tauri`. The running Node connects to the Relay, devices pair through it from **Settings → Remote computers** or
`lume node relay-pair`, and a paired device can read the other computer's health, inventory and agent sessions, plus a
session's conversation when the owner allows it. Everything is read-only: there are no remote prompts, approvals or
interrupts yet. Accounts, entitlement tokens, billing and push do not exist, so the Relay has no per-account quota, and
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

## Self-hosting

The same Relay and control-plane code runs on the user's own Cloudflare account, or as a single container with SQLite
for the registry. Self-hosters skip Paddle and entitlement checks.

## Phases and gates

1. **Relay MVP (closed beta, owner only):** Durable Object relay, outbound Node transport, pairing, no billing. *Running, read-only.*
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
- Telling a joining client that its Node is offline, so a query fails at once instead of waiting and leaving a frame in
  the replay queue.
- Regions: Durable Objects are placed near the first client, which may add latency for the other peer.
- Whether an optional end-to-end encrypted backup (user-held key) is ever offered; it would be opt-in and
  unrecoverable if the key is lost.
