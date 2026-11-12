# Lume Mobile hybrid architecture

Lume Mobile keeps the existing Capacitor interface as the product UI. Kotlin is
an Android capability layer, not a second interface and not a second source of
business rules.

## Ownership

- `mobile/` (Svelte sources, sharing modules with the Workspace) owns navigation, chat, results, visual states and
  accessibility. `mobile-pwa/` is its build output (`npm run build:mobile`, see `vite.mobile.config.js`), published
  as the PWA and synced into the Capacitor shells; it is not edited by hand.
- `src-tauri/` owns sessions, permissions, commands and device scopes.
- `LumeNativePlugin` owns Android connectivity while the app is visible.
- `LumeRealtimeService` owns it while optional background monitoring is active.
- REST remains the browser/PWA fallback and the authoritative command API.

## Realtime contract

The desktop exposes `/api/v1/ws` with subprotocol `lume.hub.v1`. The connection
does not place credentials in the URL or WebSocket headers. Its first message is
an AES-256-GCM authentication envelope containing:

- `deviceId` in the outer envelope;
- `timestamp`, a unique request nonce and a unique stream nonce in the encrypted
  payload;
- AAD `lume-stream-auth-v1`.

Every server message is encrypted with the paired device key and serialized as:

```json
{
  "type": "secure_message",
  "sequence": 1,
  "nonce": "...",
  "ciphertext": "..."
}
```

The AAD is `lume-stream-message-v1:<streamNonce>:<sequence>`. Replayed messages,
messages copied from another connection and altered sequence numbers therefore
fail authentication.

After authentication the server sends:

1. `hello`, with protocol features and heartbeat interval;
2. `snapshot`, with the current sessions;
3. `update`, with event envelopes and a fresh authoritative snapshot.

## Capacitor plugin

The Android bridge is exposed as `LumeNative`:

- methods: `connect`, `disconnect`, `getStatus`, `getSnapshot`, `sendCommand`,
  secure REST `request`, credential management and monitoring preferences;
- events: `connectionChanged`, `sessionSnapshot`, `sessionDelta`, `streamError`.

The web layer activates it only in the Android app. If it is absent, connecting
or reconnecting, the existing 1.4-second event polling continues automatically.
Once the native stream is authenticated, polling becomes an idle watchdog.

## Credential ownership

On Android, the first successful native connection migrates an existing paired
token out of `localStorage`. The plugin stores one encrypted JSON blob in
DataStore; its AES-GCM key is generated inside Android Keystore and is not
exportable. Native REST and WebSocket requests then use that credential without
returning the token to JavaScript. Browser/PWA installations retain the existing
Web Crypto transport because they do not have Android Keystore.

## Offline cache

Android stores only the latest snapshot for the paired device in Room. The
snapshot is capped at 5 MB and encrypted with a separate non-exportable Keystore
key. It is restored immediately with a visible `Cached` state, then replaced by
the first live snapshot. Disconnecting the device clears both credentials and
cache.

## Local discovery

When the LAN gateway is enabled, the desktop advertises `_lume._tcp.local.` with
its persistent desktop identity. After repeated connection failures Android
uses NSD to find that exact identity. An mDNS result is never trusted by itself:
the app sends an encrypted `/api/v1/me` probe using the existing paired key and
only saves the new address after the response authenticates successfully.

Legacy pairings without a desktop identity continue using their saved address
and REST fallback until paired again.

## Native notifications and background monitoring

The Kotlin layer tracks authoritative snapshot transitions and emits native
notifications for a new permission request, completed task, failed task, or a
confirmed agent usage-window reset. Reset events carry only the agent, usage
window and remaining percentage; the phone deduplicates them across foreground
and background handling and distinguishes an already-exhausted new window.
Its last observed state is shared between the foreground bridge and background
service to prevent duplicates.

Background monitoring is disabled by default. Enabling it also requires Android
notification permission and starts a visible low-priority foreground service
with a secure WebSocket connection. It uses the same encrypted credentials,
cache, reconnection and authenticated local discovery as the foreground bridge.
The plugin closes its socket before the service takes ownership, and the service
is stopped before the plugin reconnects on resume, so only one realtime client
is active. If Android rejects the service start, the preference is turned off,
the foreground bridge remains the best-effort fallback and the UI receives the
error. Turning off notifications also turns off background monitoring.

These are native notifications delivered while the paired mobile has a live
connection to the desktop, not a cloud push service for offline devices. The
desktop detects resets from provider usage snapshots; it does not fabricate a
reset when a provider omits the next reset time or when no prior snapshot was
observed. Codex and Claude are polled, and a previously observed Antigravity
window is checked when its deadline arrives. Other providers need to expose
usage data before their resets can be detected.

## Updates

The existing native updater remains independent of the web interface. It checks
the release manifest periodically, validates the declared SHA-256, verifies the
APK package and signing identity against the installed app, and then hands the
verified file to Android's package installer. The PWA keeps its service-worker
update path.
