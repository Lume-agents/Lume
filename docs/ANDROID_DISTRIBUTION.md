# Android distribution checklist

What Lume Mobile needs to be distributed outside and inside Google Play. The Google requirements below come from memory
of the policies and must be confirmed in the Google Play Console before each step.

## What the app is

| | |
|---|---|
| Package name | `com.tulerws.lume.mobile` |
| Version | the Lume desktop version (`versionCode` derives from it) |
| Licence | MIT (source: <https://github.com/Lume-agents/Lume>) |
| Privacy policy | <https://lumeagents.dev/privacy/> |
| Contact | contact@lumeagents.dev |
| Website | <https://lumeagents.dev/> |

## 1. Accounts to open (you)

1. **Google Play Console developer account.** One-time US$ 25. Personal accounts need identity and address verification.
   A new personal account normally has to run a closed test with a minimum number of testers (about 12) for 14 days
   before it can publish to production. An organization account needs a D-U-N-S number instead.
2. **Android developer verification** (apps installed outside Google Play on certified devices). Announced for Brazil,
   Indonesia, Singapore and Thailand from September 2026. Register the package name and the fingerprint of the release
   signing key. Confirm the current scope and dates in the Android Developer Console.

## 2. What you provide

- A government ID and the address for the identity check.
- The **SHA-256 fingerprint of the release signing key** (from the keystore kept in the repository secrets:
  `keytool -list -v -keystore <file>`). Lume only keeps this key for the APK; never upload the keystore itself.
- Play App Signing: decide whether to let Google hold the app signing key. If so, the key people install from Play differs
  from the one of the direct download, and Android will not update one with the other.

## 3. What Lume prepares

- Store listing text (short and full description, in English and Portuguese), icon, feature graphic and screenshots.
- A privacy policy page (done: <https://lumeagents.dev/privacy/>).
- The **Data safety** form answers (below).
- A **Play build without the self updater** (below).

## 4. Play build: remove the self updater

The APK on GitHub installs its own updates, which needs `REQUEST_INSTALL_PACKAGES` and the `LumeUpdater` plugin. Google
Play does not allow an app to update itself outside of Play, so a Play build needs a separate flavor that:

- drops the `REQUEST_INSTALL_PACKAGES` permission and the update worker and notification;
- hides the "Check now / Install" row in Settings and shows "Updates come from Google Play";
- keeps everything else identical.

The direct download keeps the updater. A Play and a direct build cannot update one another (different signing keys unless
Play App Signing reuses the upload key), so the website should present them as two ways of installing the same app.

## 5. Permissions to justify in the console

| Permission | Why |
|---|---|
| `INTERNET` | Talks to the user's own Lume on the computer, on the local network or through the Relay |
| `CHANGE_WIFI_MULTICAST_STATE` | Finds the computer on the local network (mDNS) |
| `CAMERA` (optional hardware) | Scans the pairing QR code; the camera is not required |
| `POST_NOTIFICATIONS` | Tells the person that an agent needs them |
| `FOREGROUND_SERVICE`, `FOREGROUND_SERVICE_REMOTE_MESSAGING` | Keeps the connection to the computer alive for notifications, only when the person turns it on |
| `REQUEST_INSTALL_PACKAGES` | Direct download only (the self updater); not in the Play build |

## 6. Data safety form (draft answers)

- **Data collected or shared with us:** none. The app talks to the person's own computer; nothing goes to a Lume server
  (the optional hosted Relay carries encrypted data and is not available yet).
- **Data encrypted in transit:** yes (TLS on the local network, end-to-end encryption through the Relay).
- **Deletion:** removing the app removes its data. The phone keeps the pairing, the settings and a cached snapshot only on the device.
- **Account:** none.
- **Device or other identifiers:** a pairing identifier and the device name the person chooses, stored on the phone and on the person's computer.
- **Ads:** none. **Analytics:** none.

## 7. Content rating and category

- Category: Developer tools (or Productivity).
- Content rating questionnaire: no user generated content shared between people, no violence, no purchases (the future Lume Remote subscription would be handled by the web, not in the app, unless the policy changes).
- Target audience: adults (developers).

## 8. Listing text (draft)

**Short description (80 characters):**
- English: `Watch and answer your AI coding agents from your phone.`
- Portuguese: `Acompanhe e responda seus agentes de código pelo celular.`

**Full description (English):**
Lume Mobile is the companion of Lume, the desktop app that keeps your AI coding agents in sight. Pair your phone with your
computer on the same network and follow every agent: what it is doing, which ones need your approval or an answer, and what
it just said. Send prompts, approve permissions, open the workflow board and drag its cards, and get a notification when an
agent needs you. Everything stays between your phone and your computer: no account, no analytics, no ads. Lume is free and
open source.

**Descrição completa (português):**
O Lume Mobile é o companheiro do Lume, o app de desktop que mantém seus agentes de código à vista. Pareie o celular com o
computador na mesma rede e acompanhe cada agente: o que ele está fazendo, quais precisam da sua aprovação ou de uma
resposta e o que acabou de dizer. Envie prompts, aprove permissões, abra o board de workflows e arraste os cards, e receba
uma notificação quando um agente precisar de você. Tudo fica entre o celular e o computador: sem conta, sem análises, sem
anúncios. O Lume é gratuito e de código aberto.

## 9. Assets to produce

- Icon 512×512 (from the Lume dino).
- Feature graphic 1024×500.
- Phone screenshots (at least two, up to eight): chats, the workflow board, settings. The `Lume-video/tools/mobile-shot.mjs`
  recorder already produces them against a demo fixture.

## 10. Order

1. Create the Play Console account and verify identity.
2. Register the package and signing key for developer verification.
3. Build the Play flavor and publish it to a closed test with the required testers.
4. Fill the data safety form, rating and listing, then ask for production.
