# Lume Node development guide

The Phase 1 Node is an opt-in headless process embedded in the Lume executable. It exposes only its signed public identity, high-entropy pairing and authenticated read-only health over TLS. It does not expose a model runtime, prompts, files or remote commands.

```bash
lume node enable
lume node run
lume node status
lume node pair
lume node clients
lume node revoke DEVICE_ID
lume node discover
lume node connect 'lume://pair-node?...'
lume node remotes
lume node remote-health NODE_ID
lume node forget NODE_ID
lume node disable
```

`enable` persists consent but does not silently start a process. `run` starts the headless loop without loading Tauri or a WebView. `disable` is observed by a running Node within a few seconds and stops it. `status` prints the stable Node ID, Ed25519 identity fingerprint, signed identity attestation, lifecycle, protocol range, read-only capabilities and a machine snapshot as JSON.

For development and tests, `--state-dir PATH` isolates state and `run --once` validates startup without leaving a process running.

State is stored under `%LOCALAPPDATA%/Lume/node` on Windows or `$XDG_STATE_HOME/lume/node` / `~/.local/state/lume/node` on Linux. The directory and files are restricted to the current user on Unix. Logs rotate at 1 MiB, heartbeat writes happen at most every 30 seconds, and only one live process may hold the Node lock.

The Node creates a persistent Ed25519 identity and fails closed if its private or public identity becomes inconsistent. The private key is created owner-only on Unix and never appears in health output or logs. A separate persistent TLS certificate is bound to that identity by a signed attestation, so a client can verify both the QR fingerprint and the certificate it actually reached.

`pair` creates one five-minute QR payload backed by a 256-bit secret. Only its domain-separated SHA-256 hash is persisted. A client must prove possession of its own Ed25519 key, request explicit scopes and complete the offer before it expires or reaches five failed attempts. Phase 1 currently grants only the read-only `observe` scope. Offers are single-use; `clients` lists paired public identities and `revoke` removes one immediately.

While `run` is active, the Node listens with TLS on port `43132` and advertises `_lume-node._tcp.local.` when a LAN address and mDNS are available. Discovery records are hints, not trusted identity: clients must verify the Ed25519 fingerprint and signed TLS-certificate binding. `/v1/identity` is the only public metadata endpoint; `/v1/pair` requires the active offer, and `/v1/health` requires a paired key, `observe` scope, a fresh timestamp and a unique signed nonce.

On another Lume installation, `discover` lists compatible LAN Nodes and `connect` consumes the QR payload emitted by `pair`. The client pins the advertised TLS certificate, verifies the signed Node identity and certificate binding against the QR fingerprint, and only then sends the pairing secret. No bearer token or private key is transferred. The client stores its own Ed25519 key and the remote public identity locally; `remote-health` authenticates every request with a fresh signed nonce. If the Node address changes, the client may update it only from an mDNS record matching both the paired identity and pinned certificate. Phase 1 grants read-only `observe` access only.
