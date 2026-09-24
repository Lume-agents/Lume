# Lume — direção revisada da landing

Atualizada em 23/09/2026 a partir da correção do usuário.

## Produto e objetivo

Lume é um monitor de agentes de IA. Orb e Workspace são duas formas completas de usar o mesmo produto. O Orb oferece presença discreta, um painel de sessões e terminais independentes que podem ser organizados e acoplados. O Workspace reúne o trabalho em uma janela. Capacidades variam por integração, em ambos os modos.

A landing apresenta o produto e incentiva o download. As interações são de composição, navegação e rolagem. A primeira versão, centrada em operar demos inventadas, foi rejeitada pelo usuário e é a anti-referência desta revisão.

## Direction contract

**THESIS:** o mesmo trabalho pode ocupar apenas um canto ou todo o seu espaço. A mudança de escala é o gesto central.

**OWN-WORLD:** uma abertura de lâminas metálicas em verde mineral, iluminada como uma peça de estúdio, em torno do Orb real. Fundo claro e tipografia Manrope ampla contrastam com o Workspace escuro. As imagens do produto vêm dos próprios componentes Svelte.

**STORY:** presença discreta → Orb e terminais independentes → Workspace completo → capacidades compartilhadas → integrações e execução distribuída → download.

**FIRST VIEWPORT:** composição central de tela inteira. “Your agents.” acima da escultura, “Your way.” abaixo, o Orb real no centro. A primeira rolagem abre o painel do Orb enquanto as lâminas mudam orientação e escala. Navegação e download aparecem imediatamente.

**FORM:** implementação direta autorizada na conversa, com correções explícitas do usuário. Three.js com geometria instanciada, reflexos de ambiente e iluminação física. Capturas reais da interface permanecem em HTML. Não há seletor de sessões, decisão de revisão, jogo de docking ou aprovação fictícia.

**FINISH:** revisão de composição desktop/mobile, veredito independente, atualização do sistema visual e proveniência dos rasters publicados.

## Referências verificadas

| Referência | Evidência consultada | Aplicação no Lume |
| --- | --- | --- |
| [Lusion v3](https://www.awwwards.com/sites/lusion-v3) | Página Awwwards e captura editorial publicada; site atual ficou no loader na captura local | Uma composição tridimensional dominante, escala editorial e espaço negativo |
| [Devin por Lusion](https://lusion.co/projects/devin_ai/) | Case do estúdio e captura do [site arquivado](https://archive-devin-ai.lusion.co/) | Apresentação de um produto de IA com narrativa visual e interface reconhecível |
| [Unseen Studio](https://www.awwwards.com/sites/unseen-studio) | Página Awwwards e captura editorial publicada | Luz, materiais e continuidade de uma cena |
| [Igloo](https://www.igloo.inc/) | Referência localizada; execução atual e case Awwwards não carregaram no ambiente | Referência de ambição, sem alegar inspeção visual completa |

Capturas de referência ficam em `.impeccable/review/references-v2/`, fora da distribuição. Não são assets da landing. Não se afirma equivalência de acabamento ou premiação.

## Roteiro e movimento

1. **Abertura:** escultura instanciada de lâminas em torno do Orb. Luz, reflexos e resposta suave ao ponteiro. A rolagem expande a composição e revela o painel real.
2. **Orb:** painel e dois terminais do próprio Lume, em enquadramentos independentes. Deslocamentos pequenos de profundidade acompanham a rolagem. Texto curto explica monitoramento, abertura e organização.
3. **Workspace:** a janela real ocupa quase toda a largura. A perspectiva se resolve em uma vista frontal legível. No celular, um enquadramento do mesmo componente apresenta uma conversa.
4. **Mesmas capacidades:** monitorar, conversar, controlar permissões, fornecer contexto, revisar e conduzir workflows. Não sugerir níveis diferentes de produto.
5. **Conexões:** linha de arquitetura Lume → Relay → Node, com caminho LAN e descrições de GitHub/Jira. Movimento de trajetória discreto.
6. **Download:** chamada única, plataformas reais, privacidade e links de documentação.

Scroll nativo. Sem áudio. Sem telas de carregamento obrigatórias. Texto e links funcionam sem JavaScript. WebGL é opcional e deixa uma composição estática quando indisponível ou quando o visitante prefere movimento reduzido. Renderização termina quando estabiliza e pausa fora da tela.

## Fontes de verdade e estágio

- `docs/PRODUCT.md`: monitoramento e capacidades de cada integração.
- Correção do usuário: Orb e Workspace são modos completos sobre o mesmo produto, com terminais separados como experiência original.
- `docs/WORKSPACE_JIRA_ROADMAP.md`: GitHub/Jira ainda estão no roadmap do checkout.
- `docs/LUME_NODE.md`: a primeira fase do Node oferece identidade, pareamento e saúde em leitura.
- `docs/DISTRIBUTED_EXECUTION_ROADMAP.md`: execução remota e Relay ainda em desenvolvimento; internet via Relay por padrão, acesso remoto opcional.
- `src/routes/+page.svelte`, `src/lib/WorkspaceWindow.svelte`, `src/lib/TerminalWindow.svelte`: UI usada nas capturas.

O capítulo conectado apresenta a visão com o estágio “In development”. Não atribui execução remota completa à primeira fase do Node.

## Imagens do produto

Os arquivos em `assets/product/` são capturas dos componentes atuais, com sessões ilustrativas injetadas no processo de navegador. Nenhum agente foi executado e nenhum arquivo do app foi modificado para produzir as imagens. Não são reconstruções de interface feitas para marketing. Os exemplos são identificados na página.
