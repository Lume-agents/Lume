# Lume

Local-first command center for AI coding agents running on the user's computer.

## Language

**build macOS**:
Compilação do desktop Lume em runner macOS, unsigned, para teste.
_Avoid_: suporte Mac, versão Mac, build Apple

**instalador macOS**:
O `.dmg` publicado na Release, contendo o `.app` dentro.
_Avoid_: dmg, pacote Mac, setup Mac

**release macOS**:
Publicação por tag `v*` com entrada macOS no `latest.json` do updater.
_Avoid_: deploy Mac, publish Apple

**suporte macOS desktop**:
Runtime funcional no macOS (launcher de terminal, overlay, atalhos, testes manuais).
_Avoid_: build passou, CI verde no Mac
