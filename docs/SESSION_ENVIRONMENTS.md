# Ambientes por sessão

No Workspace, o FAB à esquerda acima do composer abre os ambientes detectados do chat. O mesmo menu está disponível pelo ícone de servidor no card da sidebar; bancos de dados detectados usam o ícone de database. O indicador da sidebar acompanha o serviço, independentemente do estado do agente, e some após o último serviço parar. O FAB mantém os processos parados disponíveis durante esta execução do Lume.

O primeiro incremento mostra processo, portas abertas, tempo de execução, endereço HTTP para frameworks reconhecidos e a ação de parar o processo. Uma porta aberta não prova que a aplicação passou em um health check. Não há reinício automático, importação de comandos do histórico nem captura retroativa de stdout.

## Associação e parada

- `CODEX_THREAD_ID` nos subprocessos do Codex identifica a conversa, inclusive após o servidor se desvincular do processo pai. A identidade precisa corresponder exatamente a uma sessão conhecida.
- Nas outras integrações, uma árvore com um único proprietário confirmado associa o serviço à sessão. Registros nativos do Claude, runtimes Antigravity e sessões carregadas pelo OpenCode complementam os PIDs de CLI.
- Processos compartilhados entre conversas, sem identidade explícita, permanecem sem associação. Caminho de projeto, nome e proximidade temporal nunca decidem o proprietário. Processos do agente, editor e do próprio Lume não são ambientes encerráveis.
- Parar verifica sessão, PID e instante de início. No Linux/macOS solicita SIGTERM; no Windows encerra o processo identificado. Não encerra o agente nem o pai do serviço, e não força parada por timeout.
- O monitor mantém até 256 registros recentes; os processos parados mais antigos são descartados primeiro. O inventário é local e fica na memória, sem persistir variáveis de ambiente, comandos ou segredos.

## Consulta

A interface compartilha uma consulta a cada 4,5 segundos, suspensa enquanto a janela está oculta. O backend guarda o resultado por quatro segundos e executa a inspeção fora da thread da interface. Linux usa `ss` com fallback `/proc`; Windows consulta as tabelas TCP nativas IPv4/IPv6, independentes do idioma do sistema; macOS usa `/usr/sbin/lsof`. As saídas têm limite de tamanho e tempo. Uma falha de inspeção mostra estado indisponível; não afirma que o serviço parou.

## Próximos incrementos

Supervisor de lançamentos com identidade durável, logs e reinício explícito; associação por execução para runtimes compartilhados; serviços sem porta (workers/watchers); health checks configuráveis; persistência e ambientes de Nodes remotos. Windows e macOS precisam de validação nativa, além dos testes dos parsers.
