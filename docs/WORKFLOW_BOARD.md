# Workflow Board (Workspace)

Estado: implementado localmente em `src/lib/WorkflowBoard.svelte` e integrado ao `WorkspaceWindow.svelte`, com regras em `workflowBoard.ts` e fila de salvamento em `workflowBoardPersistence.ts`. Testes puros, check, build e smoke de navegador passaram; execução nativa com agentes reais e sincronização entre janelas ainda precisam de aceite runtime (ver validação abaixo).

## Objetivo

Uma segunda forma de **ver e conectar** o workflow que o orb já executa. A funcionalidade é a mesma: mesma definição (`WorkflowGroupDefinition`), mesmos comandos de execução, mesmas salvaguardas, mesmo histórico e recuperação. Muda só a visualização e o jeito de conectar: no orb, terminais acoplados e uma "ponte" entre vizinhos; no workspace, um canvas.

## Como é

- O board ocupa **toda a área onde ficam os chats** no workspace. Só a sidebar permanece ao lado.
- O fundo é um **grid de pontos**, apenas para orientar o olhar. Os cards ficam **livres**, sem encaixe nem reorganização automática.
- Cada **card** é uma etapa ligada a uma sessão: avatar, nome da sessão, agente e projeto, papel, e estado (da execução, quando houver; senão o da sessão).
- As **conexões** são setas desenhadas à mão de um card para outro. A direção é explícita. O fluxo existe apenas onde há setas.
- **Etapas intermediárias são opcionais**: basta não colocar uma, ou inserir uma sobre uma seta existente (A → B vira A → nova → B).
- Os chats continuam montados por baixo do board, então voltar aos chats não perde rolagem nem rascunhos.

## Uso

1. Abra pelo ícone **Workflow board** no cabeçalho da sidebar, ao lado do Inspector; escolha um workflow existente ou crie um novo.
2. Arraste uma sessão da sidebar para o canvas, ou use **Agente** para escolhê-la. Mova os cards livremente e arraste a alça de saída até outro card para desenhar uma seta.
3. Selecione um card para editar papel e contrato; o botão no meio de cada seta abre um menu com "Configurações da conexão" (contexto, aprovação, avanço, contexto exato) e "Adicionar agente intermediário"; os dois abrem logo abaixo do botão.
4. Informe o objetivo e execute; os controles de aprovação, próxima etapa, pausa, retomada, tentativa, ignorar e parada aparecem conforme o estado.
5. Use **Voltar aos chats** ou **Abrir chat** no card para sair do board. Os chats permanecem montados.

## Decisões

1. Posição é só layout. **A ordem vem das setas**, nunca de onde o card está.
2. Nada muda na execução. O board não introduz conceito que o runtime não execute.
3. Uma sessão aparece uma única vez por workflow (o backend já exige isso, e duas etapas na mesma conversa causariam conflito de escritor).
4. Conectar não compartilha a conversa inteira: cada seta define o que passa (contexto, aprovação, avanço).
5. Valores padrão de uma seta nova são os mesmos do orb: contexto padrão, aprovação ligada, avanço manual.
6. O papel padrão de uma etapa nova segue a ordem de inclusão (planejador, implementador, revisor, testador, pesquisador, depois personalizado), com o contrato do papel vindo de `loadWorkflowRoleContract`.

## Regras da cadeia (espelham `validate_manual_workflow` no runtime)

O runtime hoje só executa **uma cadeia linear**. Por isso o board é livre na posição e estrito nas conexões:

- cada card tem no máximo **uma seta saindo** e **uma entrando**;
- **sem laços** (uma seta que fecharia um laço é recusada);
- exatamente **uma primeira etapa** (a única sem seta entrando);
- todas as etapas na **mesma rota**; sem etapas soltas ao iniciar;
- papel personalizado precisa de nome; o contrato do papel (instrução, entrada esperada, saída produzida, condição de término) precisa estar completo;
- uma seta não liga um card a ele mesmo e não pode repetir o mesmo par.

Uma seta que viole essas regras é **recusada na hora**, com o motivo (`checkConnection` e `connectionRefusalMessage`). O botão de iniciar fica desabilitado enquanto houver problema, mostrando o primeiro (`analyzeChain` e `problemMessage`); tocar nele destaca os cards envolvidos. Ramificação, laços e etapas que não são agente seriam evolução do runtime e valeriam também para o orb.

## Interações

- **Adicionar:** soltar uma sessão da sidebar no canvas (payload de arrastar `text/x-lume-session` com o id da sessão) mostra o card do agente sob o cursor assim que o arrasto entra no board (com ícone do tipo de agente) e o fixa onde for solto; também há um seletor de sessões. Sessões sem identificador nativo (ainda não conectadas) não podem entrar.
- **Mover:** arrastar o card. **Navegar:** arrastar o fundo para mover a vista; a roda do mouse (ou os botões) dá zoom em torno do cursor; botão de ajustar à tela.
- **Conectar:** arrastar da alça do card até outro card. Enquanto arrasta, o card sob o cursor mostra se a seta seria aceita ou recusada.
- **Botão da seta:** abre um menu; as configurações abrem num popover ancorado ao botão, com contexto (mínimo, padrão, detalhado; "personalizado" só é editado no orb), pedir aprovação, avanço manual ou automático, instrução extra, **ver o contexto exato** (`previewWorkflowContext`, com tokens estimados) e remover. Um botão "+" no meio da seta insere uma etapa ali.
- **Selecionar um card:** painel com papel, nome do papel personalizado e os quatro campos do contrato, "abrir chat" e remover. Ao trocar o papel, campos que diferem do padrão anterior são preservados; só os que ainda seguem aquele padrão recebem o padrão do novo papel (`preserveRoleOverrides`). A sessão e o agente não são trocados.
- **Remover um card no meio da cadeia** religa os vizinhos (A → B → C vira A → C, mantendo o que A passava).
- Selo no meio da seta: cadeado (pede aprovação) e raio (avança sozinha). Setas esperando aprovação destacadas. Cards numerados pela ordem da cadeia e o primeiro marcado.
- Cards de CLI externa mostram aviso: é preciso assumir o controle antes de executar.
- Teclado: `Delete`/`Backspace` remove o selecionado; `Esc` fecha painel, cancela conexão ou limpa a seleção. Sem seleção ou gesto, `Esc` volta aos chats. Esses atalhos não atuam em campos de edição nem quando um overlay do workspace, como Ajustes, possui o teclado.

## Execução

Mesmos comandos do orb, por estado da execução:

| Estado | Ações |
|---|---|
| sem execução | objetivo + executar (conclui os salvamentos pendentes antes) |
| `running` | pausar, parar |
| `waiting_for_approval` | aprovar handoff, pausar, parar |
| `ready` | executar próxima, pausar, parar |
| `paused` | retomar, parar |
| `failed` | tentar novamente, ignorar, parar |
| `completed` / `cancelled` | executar novamente |
| recuperação (`run.recovering`) | aviso "aguardando o agente original"; avanço bloqueado |

O estado dos cards e das setas vem de `WorkflowRun` e do evento `lume://workflow-run-changed`; `acceptRun` descarta eventos antigos comparando identidade e timestamps do run. Iniciar ou executar novamente chama `queue.flush()` antes de `startWorkflowRun`: se o salvamento falhar, a execução não começa.

## Dados e persistência

- A **definição** do workflow fica em `preferences.workflowGroups` e é salva por `saveWorkflowBoardGroup`, via `updatePreference("workflowGroups", …)` do workspace. `WorkflowBoardSaveQueue` serializa as gravações e reúne edições rápidas do mesmo grupo na versão mais recente, sem descartar outros grupos; a integração aguarda gravações de ajustes em andamento.
- Se uma gravação falhar, a edição mais recente permanece no rascunho e na fila em memória, com estado **Não salvo** e botão **Tentar salvar novamente**. Não há descarte silencioso nem repetição automática ilimitada; a fila também é concluída antes de iniciar a execução. Rascunhos não salvos não têm garantia de recuperação após fechar o aplicativo.
- Grupos criados no board usam ids próprios (`newGroup()`); o backend exige um identificador estável e não vazio em `terminalGroupId`. Grupos existentes do Orb são editáveis no board porque compartilham a definição de etapas e conexões. Criar um workflow no board **não cria automaticamente um grupo de janelas de terminal no Orb**.
- O **layout** (posição dos cards, vista, nome do workflow), os objetivos por grupo e a seleção do grupo ativo ficam em `localStorage`, chave `lume:workflow-board:v1`; layouts são lidos com `normalizeLayout`. O estado aberto/fechado é salvo separadamente em `lume:workflow-board:open`. Esses dados locais não alteram a definição nem o backend. Grupos do Orb sem layout salvo recebem posições em linha, na ordem da cadeia.

## Integração no `WorkspaceWindow`

- `boardOpen` controla a abertura e `boardMounted` mantém o componente montado depois do primeiro uso. O botão usa `WorkspaceHeaderIcon` com `name="workflow"`, no cabeçalho da sidebar ao lado do Inspector; a abertura é restaurada do storage local.
- O board é uma **camada sobre a área dos chats**, dentro de `.workbench`, cobrindo-a; os painéis continuam montados em `.board-chat-layer`, com `inert` e `aria-hidden` enquanto o board está aberto. Inspector e central de revisão também ficam ocultos e inertes nesse estado.
- O drop no board para a propagação, não aciona o encaixe de painéis do workspace e encerra o gesto por `onFinishSidebarDrag`/`finishSidebarSessionDrag`.
- `onOpenChat` fecha o board e foca a sessão (`selectSession`); `onClose` apenas volta aos chats. `keyboardEnabled` cede o teclado aos overlays do workspace.
- Segue o tema do workspace (variáveis `--workspace-*`), claro e escuro, e respeita `prefers-reduced-motion`. Usa os comandos existentes de workflow em `lume.ts`; não há mudança no backend.

## O que já existe

`src/lib/workflowBoard.ts` (puro, testado em `tests/workflow-board.test.mjs`):

- regras: `analyzeChain`, `checkConnection`, `connectionRefusalMessage`, `problemMessage`;
- edições (devolvem um grupo novo): `addStep`, `updateStep`, `removeStep`, `addConnection`, `updateConnection`, `removeConnection`, `insertStep`; criação: `newGroup`, `newStep`, `newConnection`, `defaultRole`, `defaultContextSelection`;
- geometria: `cardRect`, `edgeGeometry` (caminho, pontos de início e fim e o ponto médio), `cardAt`, `placeCard`, `toBoardPoint`, `zoomAround`, `fitView`, `clampZoom`;
- layout e execução: `normalizeLayout`, `stepVisualState`, `connectionWaitsForApproval`.

`src/lib/workflowBoardPersistence.ts` fornece `WorkflowBoardSaveQueue`, testada em `tests/workflow-board-persistence.test.mjs` (serialização, coalescimento, retenção após falha e retry). `tests/workflow-order.test.mjs` inclui os dois conjuntos de testes puros.

`src/lib/WorkflowBoard.svelte` reúne canvas, cards, setas, editores, preview de contexto e controles de execução. `src/lib/WorkspaceWindow.svelte` integra abertura, salvamento, sidebar e retorno aos chats. `tests/workflow-board-ui.test.mjs` exercita essa interface em navegador.

## Fora do escopo

Ramificação, laços, etapas que não são agente, cards sem sessão e qualquer mudança no runtime. Se forem desejados, entram como evolução do runtime para o orb e o board juntos.

## Critérios de aceite

- Um workflow do orb abre no board com os mesmos cards, setas e papéis.
- Não é possível desenhar uma seta que o runtime recusaria ao iniciar; a recusa diz o motivo.
- Executar, aprovar, avançar, pausar, retomar, tentar de novo, ignorar e parar funcionam como no orb, com o estado refletido ao vivo.
- Editar no board e abrir o orb (e o contrário) mostra o mesmo workflow.
- Reabrir o workspace restaura posições, zoom e o workflow ativo.
- Claro e escuro legíveis; sem rolagem horizontal da janela; teclado cobre selecionar, remover e cancelar.

## Validação local e aceite pendente

- **Confirmado:** testes puros de regras/geometria e persistência passaram; `npm run check` terminou sem erros ou avisos e `npm run build` concluiu com exit 0.
- **Confirmado:** `npm run test:workflow-board-ui` monta o Workspace e o Board reais em Chromium, mas usa **IPC Tauri simulado e conteúdo dos chats substituído por um stub**. Não envia prompts a agentes reais.
- Esse smoke cobre importação de grupo existente do Orb, drop da sidebar, arraste livre, desenho de setas e recusa de ciclo, preview, inserção/remoção intermediária, zoom e ajustar à tela responsivo, controles de execução e recuperação, descarte de eventos antigos, retenção de rascunho de chat e restauração do layout.
- A última execução também confirmou isolamento do teclado dos Ajustes e contraste de avisos/erros sobre a superfície do painel de pelo menos **4,5:1**, nos temas claro e escuro, medido com estilos computados. As capturas desktop/compacto foram atualizadas em `.impeccable/review/workflow-board/`.
- **Ainda pendente:** aceitar execução e recuperação na janela Tauri nativa com agentes reais e verificar sincronização bidirecional entre board e Orb em múltiplas janelas. A cobertura simulada não prova integração nativa, comportamento de plataforma ou sincronização multi-janela; os critérios acima continuam sendo critérios de aceite, não declarações incondicionais de conclusão.
