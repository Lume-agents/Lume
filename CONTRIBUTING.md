# Contributing to Lume

Lume is developed in the public `Lume-agents/Lume` monorepo. Desktop, mobile,
Node, and shared contracts evolve together. The marketing website is maintained
in a separate private repository.

## Components

| Component | Current location | Responsibility |
| --- | --- | --- |
| Desktop | `src/`, `src-tauri/` | Orb, Workspace, native windows, and agent integrations |
| Mobile | `mobile-pwa/`, `android/`, `ios/` | Paired mobile clients and native shells |
| Node | `src-tauri/src/node_*.rs` | Headless device identity, pairing, and observation |
| Shared contracts | `src/lib/domain.ts`, `src-tauri/src/domain.rs`, `src-tauri/src/distributed_protocol.rs` | Session state and device communication |
| Extensions | `extensions/` | Browser and VS Code integrations |

Relay transport and remote agent control remain under development. Their
implementation belongs in this monorepo and must remain independently deployable.

## Local development

Use Node.js 22 or later and the Rust/Tauri prerequisites described in the
[README](README.md).

```bash
npm ci
npm run tauri dev
```

For the native mobile shells, see [MOBILE.md](MOBILE.md).

## Changes

- Open focused pull requests against `main`.
- Keep Orb and Workspace on the same session model and permission semantics.
- Update both clients and producers when a shared contract changes.
- Describe which platforms and integrations were actually validated.
- Keep local agent instructions, design critiques, and internal planning files
  outside version control; `.gitignore` contains the relevant rules.

Lume is licensed under [MIT](LICENSE). Third-party assets retain their own
license notices.
