# Produto

## Objetivo

Lume acompanha sessões de agentes de IA que já estejam abertas ou que tenham sido iniciadas pelo próprio aplicativo. O monitoramento local funciona sem conta, Relay ou internet e mantém os dados no computador.

O acesso distribuído é opcional e está em desenvolvimento: o Lume Node no PC1 permite parear o Lume no PC2. A experiência planejada inclui monitorar agentes e controlar sessões autorizadas com modelos locais no PC1, inclusive de outra rede, mantendo a execução e os arquivos naquele computador. Conexões diretas terão preferência e um Relay transportará conteúdo cifrado quando necessário. O pareamento, a saúde e o inventário inicial de processos de agentes e modelos Ollama já têm implementação local; controle remoto de sessões e acesso pela internet ainda estão em desenvolvimento.

## Experiência principal

- Orb discreto e Workspace completo são duas interfaces sobre as mesmas sessões e funcionalidades.
- O Inspector conecta cada chat ao repositório local, às ferramentas do GitHub e à atividade da conta; veja o [guia de GitHub](./GITHUB.md).
- A cápsula inicia recolhida e pode ser arrastada e acoplada às bordas do monitor, adaptando seu formato.
- O usuário pode escolher outro monitor nas preferências.
- Por padrão, a cápsula não aparece sobre vídeos ou jogos em tela cheia.
- Uma preferência permite manter a sobreposição visível em tela cheia.
- Ao expandir, a cápsula mostra todos os agentes, cada sessão, projeto, origem e estado.
- Sons sutis de conclusão e erro podem ser desativados.
- A bandeja permite mostrar ou ocultar o painel e sair.
- Sessões podem ser abertas ou retomadas no terminal ou VS Code habitual.
- O Whiteboard abre cada sessão em seu próprio mini terminal; janelas próximas podem ser acopladas e movidas como um conjunto.
- O Whiteboard permite nomear, salvar e restaurar layouts para as sessões abertas correspondentes.
- O modo Workflow oferece execução manual ou automática, recuperação local e histórico consolidado por etapa. No mobile, a visualização é somente de monitoramento dos papéis e estados; os controles permanecem no desktop.
- Perfis por projeto podem definir destino, monitor, posição da cápsula, permissões de novas sessões, layout e agentes preferidos.
- `Ctrl+Shift+Space` abre uma paleta global para navegar entre telas, sessões e ações.
- Detectores externos declarativos podem ser instalados por manifesto JSON e entram em vigor sem reiniciar.

## Estados normalizados

- `running`: o agente está trabalhando.
- `permission_required`: existe uma decisão de segurança pendente.
- `waiting_for_input`: o agente aguarda uma resposta que não é uma permissão.
- `completed`: a tarefa terminou normalmente.
- `failed`: houve erro ou uma ação necessária falhou.

## Permissões por sessão

Cada conversa mantém seu próprio `PermissionProfile`. O perfil é obtido do agente e inclui:

- modo de acesso atual;
- política de aprovação atual;
- se o Lume pode responder por aquela integração;
- ações válidas para a solicitação atual.

A interface nunca cria um botão apenas porque outro chat do mesmo agente o oferece. Uma sessão pode ter acesso total enquanto outra está em modo de planejamento ou somente leitura.

O conteúdo detalhado da permissão permanece apenas na memória enquanto a decisão está pendente. Depois de permitir ou recusar, o histórico mantém um resumo sanitizado, sem comando completo, payload, caminho absoluto ou segredo.

## Plataformas iniciais

- Windows 10 e 11.
- Pop!_OS e outras distribuições Linux modernas.
- X11/XWayland pela janela Tauri.
- Wayland nativo por um backend de posicionamento `layer-shell` quando o compositor oferecer o protocolo.

## Integrações iniciais

- Codex CLI e extensão do VS Code.
- Claude CLI e extensão do VS Code.
- Antigravity CLI (`agy`) como integração principal do ecossistema Google, com status/atividades observáveis por hooks e retomada da conversa mais recente indexada por workspace. O Lume não importa a transcrição completa; para o histórico integral, use a CLI. Os hooks ficam no `settings.json` exclusivo da CLI, sem usar o registro compartilhado com Antigravity IDE/Gemini Code Assist. Ao conectar, o hook `PreToolUse` com matcher curinga responde `allow` e permite automaticamente todas as chamadas de ferramentas da CLI; essa consequência é informada antes da ativação. Se o executável do Lume estiver ausente ou o comando do hook falhar, o fallback também responde `allow` para não bloquear a CLI; as confirmações nativas não voltam até o hook ser desativado/removido. O hook não se aplica à IDE Antigravity nem ao Gemini Code Assist. O Lume ainda não oferece interrupção segura do prompt nem queue/steer para essa integração.
- Oh My Pi (`omp`): monitora processos CLI e arquivos de sessão; sessões controladas pelo Lume usam RPC, com prompts, steer/fila, interrupção, seleção de modelo e thinking, aprovações apenas por permitir uma vez/negar, perguntas e compactação. Não há modo de planejamento. Sessões externas só podem ser assumidas depois de fechar o omp no terminal.

- Na primeira inicialização após a migração, remover do registro compartilhado apenas os hooks Antigravity antigos do Lume, preservando hooks de terceiros e criando backup. Configurações antigas ficam desconectadas até o usuário reconectar e confirmar a permissão automática das ferramentas. A CLI e a IDE já abertas precisam ser reiniciadas para recarregar a configuração.
- DeepSeek Web pelo Companion; a CLI oficial `dsh` pode ser aberta pelo Lume quando o perfil TUI opcional estiver configurado.
- O Companion web aceita prompts somente quando a página está esperando entrada; a confirmação de envio exige o protocolo v2 do Companion, e versões antigas não consomem nem marcam prompts como enviados. Queue/steer durante uma resposta em execução não está disponível. O Lume bloqueia o envio enquanto ocupado em vez de exibir uma fila que a página não consegue garantir.
- Gemini CLI legado para ambientes empresariais, Google Cloud e uso por API; monitoramento por processo apenas, sem envio, retomada, encerramento ou hooks globais. Como `~/.gemini/settings.json` também é usado pelo Gemini Code Assist para ferramentas e MCP, a migração remove apenas hooks antigos do Lume desse arquivo, com backup e preservação das demais configurações.
- VS Code como IDE inicial.
- Chrome, Edge e Brave por uma extensão Chromium local.

Integrações profundas respondem permissões diretamente. Integrações somente observáveis mostram o pedido e levam o usuário à origem, sem simular suporte inexistente.

Gemini Code Assist no VS Code é distinto da Antigravity CLI e da Gemini CLI legada; o Lume não anuncia controle direto dos chats dessa extensão. O Lume não instala hooks na configuração compartilhada do Gemini Code Assist; a integração legada continua somente por detecção de processo.
