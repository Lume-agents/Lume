# Build macOS CI (unsigned, arm64, piso 15.0)

Primeiro passo do desktop macOS: pipeline verde em `macos-15` arm64 (`aarch64-apple-darwin`), `dmg` + updater no `latest.json`, unsigned para teste, piso `15.0` e categoria `DeveloperTool`.

Considered Options: (a) arm64-only vs +Intel/universal — escolhemos arm64-only porque `macos-14` deprecia em nov/2026, Intel é minoria e universal tem bug de cross-build no `tauri-action#430`; (b) piso `11.0` (piso real dos Apple Silicon / Claude Code pede `13.0+`) vs `15.0` — escolhemos `15.0` por decisão explícita de escopo de teste, reavaliar `13.0` no runtime; (c) dmg com updater vs sem — escolhemos com updater reaproveitando `TAURI_SIGNING_PRIVATE_KEY`.

Consequences: usuários abaixo do Sequoia não instalam; Gatekeeper bloqueia o `.dmg` unsigned em outra máquina (bypass documentado); README/landing seguem `Windows · Linux` até o runtime funcionar; `app.macosPrivateApi=true` (exigido para compilar `.transparent()` no Mac — sem isso o CI nem compila; impede App Store, irrelevante pois distribuímos via `dmg`).
