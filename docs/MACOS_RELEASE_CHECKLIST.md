# Validação de release no macOS

Este roteiro acompanha as issues #7–#14. As mudanças de código precisam de
compilação no runner macOS e uso em uma máquina real antes de encerrar as issues.
Um build Linux não valida AppKit, Terminal.app, Spaces ou permissões da Apple.

## Registro da execução

Preencher a cada candidato de release:

- Versão e commit:
- URL do workflow e do artefato:
- macOS e arquitetura da máquina:
- Instalação nova ou atualização:
- Resultado de cada item: passou, falhou ou pendente; anexar evidência da falha.

O workflow atual gera `aarch64-apple-darwin` (Apple Silicon), com mínimo macOS 15.
Máquinas Intel precisam de um artefato `x86_64-apple-darwin` e validação própria.

## Assinatura e notarização (#13)

Configurar os secrets do repositório usados em `.github/workflows/installers.yml`:

| Secret | Conteúdo |
| --- | --- |
| `APPLE_CERTIFICATE` | Certificado **Developer ID Application** e chave privada exportados como `.p12`, em base64 em uma linha |
| `APPLE_CERTIFICATE_PASSWORD` | Senha de exportação do `.p12` |
| `APPLE_SIGNING_IDENTITY` | Nome completo da identidade Developer ID Application |
| `APPLE_ID` | Apple ID autorizado para notarização |
| `APPLE_PASSWORD` | Senha específica de app desse Apple ID |
| `APPLE_TEAM_ID` | ID da equipe Apple Developer |

O preflight recusa configuração parcial. Se nenhum secret Apple estiver
configurado, o workflow continua e identifica o build como sem assinatura
Developer ID/notarização nos logs. Tauri importa o certificado, assina e envia
para notarização quando a configuração está completa. A chave
`TAURI_SIGNING_PRIVATE_KEY` continua sendo a assinatura do updater e tem uma
finalidade diferente da assinatura Apple.

Referência: [assinatura macOS no Tauri](https://v2.tauri.app/distribute/sign/macos/).
Não registrar valores de secrets, senhas ou chaves em evidências.

- [ ] Compilação macOS concluída sem erros nos módulos específicos da plataforma.
- [ ] Baixar o DMG publicado pelo navegador e instalar em `/Applications`.
- [ ] Abrir pelo Finder com Gatekeeper ativo, sem remover quarantine nem criar exceção.
- [ ] Verificar assinatura, notarização e entitlement de Automation no app instalado:

```bash
codesign --verify --deep --strict --verbose=2 /Applications/Lume.app
codesign -d --entitlements :- /Applications/Lume.app
spctl --assess --type execute --verbose=2 /Applications/Lume.app
xcrun stapler validate /Applications/Lume.app
```

Assinatura ad hoc ou ausência de assinatura Developer ID não aprova este gate.
Repetir a instalação com a arquitetura realmente publicada; não validar só o
executável produzido por `tauri dev`.

## Terminal.app (#7)

- [ ] Iniciar uma sessão externa com Codex e com Claude quando instalados.
- [ ] Usar projeto cujo caminho contenha espaços, apóstrofo, aspas, `$` e caracteres acentuados.
- [ ] Confirmar diretório, argumentos e agente corretos; não substituir comandos de terminais já abertos.
- [ ] Autorizar Lume em **Privacidade e Segurança → Automação → Terminal** quando solicitado.
- [ ] Negar a autorização e confirmar que o Lume abre o diálogo de Automação; o botão
      **Abrir Ajustes de Automação** deve abrir Privacidade e Segurança → Automação. Depois autorizar e repetir.
- [ ] Deixar o pedido de permissão sem resposta por mais de 30 s e confirmar a mensagem
      "Responda ao pedido e tente de novo" (sem erro técnico do `osascript`).
- [ ] Confirmar que o payload temporário é removido após consumo ou falha de abertura.
- [ ] Com o Terminal fechado, iniciar uma sessão: é esperado que apareçam **duas** janelas,
      a da sessão e a janela vazia padrão do Terminal. Comportamento aceito, não é falha.

O app declara a finalidade de Apple Events no Info.plist e o entitlement de
Automation. Esse consentimento é específico do lançamento pelo Terminal.app.

## Orb, Dock, menu bar e fullscreen (#9)

- [ ] Arrastar, deformar e acoplar o Orb em lados, topo, base e cantos.
- [ ] Repetir com Dock embaixo, à esquerda e à direita; testar ocultação automática.
- [x] Repetir em dois monitores, com escala diferente e posições negativas.
- [x] Confirmar que o Orb e os mini terminais respeitam a área útil do monitor.
- [ ] Trocar entre Spaces normais: Orb, mini terminais e conectores acompanham; a janela de Workspace não.
- [x] Entrar/sair do fullscreen nativo de outro app com `show_over_fullscreen` desligado: o Orb não aparece no Space do app.
- [ ] Fullscreen não nativo (Keynote "Reproduzir" e um player com fullscreen não nativo) com
      `show_over_fullscreen` desligado: o Orb recua em até ~1 s e volta ao sair.
- [ ] Repetir os dois tipos de fullscreen com `show_over_fullscreen` ligado: o Orb aparece por cima.
- [ ] Alternar `show_over_fullscreen` com Orb, terminais e conectores abertos: todos atualizam sem reiniciar.
- [x] Abrir diálogo de arquivo e confirmar restauração da ordem das janelas ao fechar.
- [x] Observar renderização após vários encaixes, sem rastros, cortes ou saltos.

Resultado em macOS 27.0 (26A428), Mac17,3 com notch + monitor externo 1080p, commit `fa1d419`:

- Acoplar: borda esquerda, base (para no fim do `visibleFrame`, acima da Dock) e canto
  superior direito passaram; borda direita, topo e os outros cantos não foram testados à parte.
- Dock à esquerda/direita e ocultação automática: pendente (exige mudar Ajustes do Sistema).
- Dois monitores: o Orb vai para o externo (escala 1×, origem −175, −1080) ao recolher o painel;
  com o painel aberto ele fica no monitor anterior.
- Spaces normais: pendente (a UI do Mission Control não aparece nas capturas). Log confirmou
  `CanJoinAllSpaces` no Orb e no mini terminal.
- Fullscreen não nativo desligado: Keynote "Reproduzir" passou (layer 5 → 0 em ~1 s, volta em
  ~0,5–1 s). A apresentação do Preview cria um Space próprio. Player não nativo: não testado.
- **Flag ligada: falhou.** No fullscreen nativo o Orb não entra no Space do app, mesmo com
  `CanJoinAllSpaces | FullScreenAuxiliary` aplicado (também testado com política `Accessory` e
  `orderFrontRegardless`); provável necessidade de `NSPanel` não ativante. No Keynote o Orb
  mantém o layer 5, mas a apresentação usa layer 9 e o cobre.
- Alternar a flag em tempo de execução: o Orb atualizou sem reiniciar; terminais e conectores
  abertos durante a troca não foram testados.

A detecção usa `currentSystemPresentationOptions` no thread principal. A área
útil vem de `NSScreen.visibleFrame` por meio do Tauri. O comportamento em Spaces
e Stage Manager depende da validação nativa; não presumir que `always_on_top`
garanta presença em todos os Spaces.

A posição inicial do Orb (12 px do topo do monitor, perto da menu bar e do notch)
é intencional. Registrar o resultado também como comentário na #9.

**Hiato de testes:** a validação da #9 é feita somente no macOS 27. Não há
validação do macOS 15.0 (mínimo suportado) até o 26 (Tahoe).

## Identidade dos processos (#10)

- [ ] Executar duas CLIs Codex simultâneas em projetos diferentes; conferir associação ao chat correto.
- [ ] Repetir duas CLIs no mesmo projeto; nenhum chat deve ser escolhido por proximidade de diretório ou recência.
- [ ] Confirmar identidade de CLI com rollout aberto e identificador explícito de sessão retomada.
- [ ] Encerrar/reabrir uma CLI; o marcador de nascimento deve impedir associação com processo anterior.
- [ ] Conferir que subagentes e inventários incompletos/ambíguos não viram associação automática.

O marcador usa nascimento do processo em microssegundos via `libproc`. A leitura
de rollouts usa descritores realmente abertos na árvore da CLI. Se o rollout
existe apenas em daemon compartilhado, a CLI pode continuar sem correlação
automática; usar a associação explícita existente, sem inventar um UUID.

## Ciclo de vida do servidor Codex (#11)

- [ ] Iniciar sessão gerenciada pelo Lume e identificar seu supervisor e app-server.
- [ ] Sair normalmente; os dois processos devem terminar.
- [ ] Repetir encerrando à força **apenas o processo GUI do Lume**; o supervisor deve recolher seu app-server.
- [ ] Manter uma CLI externa com daemon compartilhado em execução e confirmar que ela não é encerrada.
- [ ] Reabrir o Lume após a falha e iniciar outra sessão sem conflito com servidor antigo.

O supervisor é pai do app-server e recebe EOF quando o pipe do Lume fecha.
Não sinaliza um PID salvo de outra execução. Encerrar à força somente o próprio
supervisor está fora dessa proteção e deve ser registrado separadamente.

## Atalhos e integração com o sistema (#8, #12, #14)

- [ ] Executar todos os atalhos padrão fora do foco do Lume e editar/restaurar um deles.
- [ ] Provocar falha real de registro, quando possível; confirmar aviso tanto no Orb quanto no Workspace.
- [ ] Recuperar o registro e confirmar remoção do aviso, sem recarregar a interface.
- [ ] Abrir Lume novamente por Finder/Dock e confirmar instância única.
- [ ] Ativar início automático, sair/entrar na conta e conferir a preferência.
- [ ] Autorizar/recusar notificações e conferir a entrega real pela Central de Notificações.
- [ ] Abrir links externos do inspector e confirmar destino no navegador.
- [ ] Instalar uma versão assinada anterior e atualizar para o candidato pelo updater.

A auditoria de #8 não identificou necessidade de ampliar capabilities: o opener
já inclui URLs padrão, e as chamadas Rust de autostart, notificações e atalhos
não passam pelo IPC de plugins no frontend. Os atalhos convencionais usam
`RegisterEventHotKey`; #12 trata a falha de registro ocultada no startup e não
solicita Acessibilidade para esses atalhos. #14 só está concluída depois da
execução deste roteiro, com evidências de uma máquina macOS.
