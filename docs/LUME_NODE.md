# Lume Node development guide

The Phase 1 Node is an opt-in headless process embedded in the Lume executable. It exposes its signed public identity, high-entropy pairing and authenticated read-only health/inventory over pinned TLS. Inventory includes detected built-in agent processes and installed/loaded Ollama models. Agent control, inference prompts, files, P2P and Relay transport remain under development.

```bash
lume node enable
# Select the actual private interface on PC1 before LAN pairing:
lume node listen 192.168.1.20
lume node run
lume node status
lume node inventory
lume node pair
lume node clients
lume node revoke DEVICE_ID
lume node discover
lume node connect 'lume://pair-node?...'
lume node remotes
lume node remote-health NODE_ID
lume node remote-inventory NODE_ID
lume node forget NODE_ID
lume node disable
```

`enable` persists consent but does not silently start a process. `run` starts the headless loop without loading Tauri or a WebView. `disable` is observed by a running Node within a few seconds and stops it. `status` prints the stable Node ID, Ed25519 identity fingerprint, signed identity attestation, lifecycle, protocol range, read-only capabilities and a machine snapshot as JSON.

The listener defaults to `127.0.0.1`, including existing configurations without `listenAddress`. `listen ADDRESS` selects one loopback/private interface and requires restarting a running Node. Wildcard and public addresses are rejected; an address not assigned to this computer fails to bind. Private LAN, IPv6 ULA and explicitly selected CGNAT/private-network interfaces are supported. Loopback does not advertise mDNS. Lume does not create firewall or router rules; selecting a private address does not override the network's existing forwarding policy.

For development and tests, `--state-dir PATH` isolates state and `run --once` validates startup without leaving a process running.

State is stored under `%LOCALAPPDATA%/Lume/node` on Windows or `$XDG_STATE_HOME/lume/node` / `~/.local/state/lume/node` on Linux. The directory and files are restricted to the current user on Unix. Logs rotate at 1 MiB, heartbeat writes happen at most every 30 seconds, and only one live process may hold the Node lock.

The Node creates a persistent Ed25519 identity and fails closed if its private or public identity becomes inconsistent. The private key is created owner-only on Unix and never appears in health output or logs. A separate persistent TLS certificate is bound to that identity by a signed attestation, so a client can verify both the QR fingerprint and the certificate it actually reached.

`pair` creates one five-minute QR payload backed by a 256-bit secret. Only its domain-separated SHA-256 hash is persisted. A client must prove possession of its own Ed25519 key, request explicit scopes and complete the offer before it expires or reaches five failed attempts. Phase 1 currently grants only the read-only `observe` scope. Offers are single-use; `clients` lists paired public identities and `revoke` removes one immediately.

While `run` is active, the Node listens with TLS on the selected address and port `43132` and advertises `_lume-node._tcp.local.` on a selected non-loopback interface when mDNS is available. Discovery records are hints, not trusted identity: clients must verify the Ed25519 fingerprint and signed TLS-certificate binding. `/v1/identity` is the only public metadata endpoint; `/v1/pair` requires the active offer. `/v1/health` and `/v1/inventory` require a paired key, `observe` scope, a fresh timestamp and a unique nonce signed for that Node, method and path. The current transport is HTTPS JSON (`lan_tls_http`), not a streaming WebSocket.

Connections are limited to eight globally and two per source IP, with a ten-second I/O deadline and at most two seconds of inactivity. Paired devices can make eight health/inventory requests per ten seconds. Both snapshots are cached for five seconds; authentication and Node-side revocation are checked before every cached response. Client `lastSeenAt` writes are debounced to thirty seconds unless the address changes. These implementation bounds still need hostile-client and native-platform validation before remote prompts ship.

The inventory reuses process detection without acquiring session ownership or publishing command lines, working directories, native thread IDs or conversation history. Each observed process has a Node-scoped ID derived from its PID and start time; it is metadata, not a controllable session. Full session monitoring and managed prompt capabilities are not advertised yet. Lists are bounded to 128 agents and 128 models and report truncation.

Ollama probes use only `127.0.0.1:11434` on PC1, with a combined two-second deadline and bounded response sizes. They query [installed models](https://docs.ollama.com/api/tags) and [loaded models](https://docs.ollama.com/api/ps); they never load a model or run inference. If the loaded-model query fails, `loaded` is unknown rather than incorrectly false. In `node.json`, `ollamaInventoryEnabled` can disable probing and `ollamaPort` can select another loopback port. There is no arbitrary URL, proxy, redirect or public inference exposure.

On PC2, `discover` lists compatible LAN Nodes and `connect` consumes the QR payload emitted by `pair`. The client pins the advertised TLS certificate, verifies the signed Node identity and certificate binding against the QR fingerprint, and only then sends the pairing secret. No bearer token or private key is transferred. The client stores its own Ed25519 key and the remote public identity locally; `remote-health` and `remote-inventory` authenticate every request with a fresh signed nonce. If the Node address changes, the client may update it only from an mDNS record matching both the paired identity and pinned certificate. Phase 1 grants read-only `observe` access only.

Orb and Workspace share **Settings → Remote computers** for discovery, pairing and manual health/inventory checks. A snapshot displays its query time; a failed check clears previously reachable state. **Forget** removes the local record. To remove authorization, use `clients` and `revoke DEVICE_ID` on PC1; client-side revocation is still pending. Update both Node and client for this development slice; older health-only Nodes do not expose `/v1/inventory`.

## Access through the Lume Relay (prototype)

For access from another network, a Node can connect **outbound** to a [Lume Relay](https://github.com/Lume-agents/lume-relay),
which only forwards end-to-end encrypted frames:

```bash
lume node relay https://relay.example.com   # or `off`; the address is saved in the Node config
lume node run                                # connects to the Relay and reconnects with backoff
lume node pair                               # also prints `relayPairingUri` (lume://pair-relay?...)
# on the other device:
lume node relay-pair 'lume://pair-relay?...'
lume node relay-remotes
lume node relay-health NODE_ID
lume node relay-inventory NODE_ID
```

The pairing secret travels only in the URI/QR code. The Relay learns a one-way derivative of it, and the pairing request
is encrypted with a key derived separately, so the Relay can neither read nor forge it. After pairing, every message runs
inside an Ed25519-authenticated X25519 session (ChaCha20-Poly1305, per-direction keys, increasing counters). Only the read-only
`health` and `inventory` commands exist so far, with the `observe` scope. `lume node revoke` removes a device from the Node; the
Relay drops its access the next time the Node connects.
