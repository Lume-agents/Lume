---
name: Lume Landing
description: Presença discreta e espaço completo sobre uma base mineral.
colors:
  ink: "#18231b"
  action-hover: "#344631"
  paper: "#e8ebdf"
  white: "#f4f5ee"
  muted: "#58624f"
  line: "#c8cebf"
  night: "#101d17"
  night-text: "#e4eadf"
  connected: "#d1dac8"
  focus: "#66834c"
typography:
  display:
    fontFamily: "Manrope, sans-serif"
    fontSize: "clamp(68px, 7vw, 104px)"
    fontWeight: 500
    lineHeight: 1
    letterSpacing: "-.04em"
  headline:
    fontFamily: "Manrope, sans-serif"
    fontSize: "clamp(44px, 5.2vw, 78px)"
    fontWeight: 500
    lineHeight: 1.06
    letterSpacing: "-.04em"
  title:
    fontFamily: "Manrope, sans-serif"
    fontSize: "25px"
    fontWeight: 500
    lineHeight: 1.2
    letterSpacing: "-.025em"
  body:
    fontFamily: "Manrope, sans-serif"
    fontSize: "17px"
    fontWeight: 400
    lineHeight: 1.7
    letterSpacing: "-.015em"
  label:
    fontFamily: "Manrope, sans-serif"
    fontSize: "13px"
    fontWeight: 600
  button:
    fontFamily: "Manrope, sans-serif"
    fontSize: "14px"
    fontWeight: 600
  button-small:
    fontFamily: "Manrope, sans-serif"
    fontSize: "12px"
    fontWeight: 600
  status:
    fontFamily: "Manrope, sans-serif"
    fontSize: "11px"
    fontWeight: 600
rounded:
  badge: "4px"
  button: "6px"
  window-mobile: "9px"
  window: "12px"
  circle: "50%"
spacing:
  inline: "16px"
  content: "24px"
  composition: "28px"
  page-mobile: "20px"
  page-medium: "32px"
  page-desktop: "56px"
  section-mobile: "75px"
components:
  button-primary:
    backgroundColor: "{colors.ink}"
    textColor: "{colors.white}"
    typography: "{typography.button}"
    rounded: "{rounded.button}"
    padding: "16px 24px"
  button-primary-hover:
    backgroundColor: "{colors.action-hover}"
  button-small:
    backgroundColor: "{colors.ink}"
    textColor: "{colors.white}"
    typography: "{typography.button-small}"
    rounded: "{rounded.button}"
    padding: "11px 17px"
  text-link:
    textColor: "{colors.ink}"
    typography: "{typography.label}"
    padding: "0 0 6px"
  navigation:
    textColor: "{colors.ink}"
    typography: "{typography.label}"
  development-label:
    textColor: "{colors.ink}"
    typography: "{typography.status}"
    rounded: "{rounded.badge}"
    padding: "6px 10px"
  scroll-link:
    textColor: "{colors.ink}"
  shared-features:
    backgroundColor: "{colors.night}"
    textColor: "{colors.night-text}"
  connection-map:
    backgroundColor: "{colors.connected}"
    textColor: "{colors.ink}"
---

# Design System: Lume Landing

## Overview

**Creative North Star: "Presença que se expande"**

Uma base mineral clara, tipografia ampla e uma abertura de lâminas metálicas tornam visível a mudança de escala. O Orb discreto com terminais independentes e o Workspace em uma janela apresentam duas organizações completas do mesmo monitor de agentes.

As imagens vêm dos componentes Svelte reais, com sessões ilustrativas identificadas. Luz e perspectiva acompanham as interfaces; texto curto, espaço livre e leitura do produto conduzem a apresentação.

**Key Characteristics:**

- Manrope ampla sobre superfícies minerais.
- Contraste noturno no capítulo do Workspace.
- Capturas reais, composição editorial e rolagem nativa.

Escopo: landing v2. Fontes: `styles.css`, `assets/fonts/fonts.css`, `index.html`, `app.js` e `src/light-sculpture.js`. `DIRECTION.md` registra a direção aprovada e `README.md` descreve a distribuição.

## Colors

A paleta combina papel esverdeado, tinta mineral e superfícies noturnas. O frontmatter contém os valores normativos.

- **Primary:** tinta mineral (`ink`) em texto e ações; o verde mais aberto (`action-hover`) sinaliza hover. `focus` identifica o contorno de teclado.
- **Neutral:** `paper` sustenta a página; `white` dá contraste às ações; `muted` e `line` organizam texto de apoio e separadores. `night` e `night-text` formam o capítulo do Workspace. `connected` distingue a arquitetura planejada.

**The Same Lume Rule.** A mudança entre a base clara e o capítulo escuro apresenta a organização do espaço; Orb e Workspace preservam a mesma hierarquia de capacidades.

## Typography

Manrope local, variável entre pesos 400 e 700, com `font-display: swap` e fallback sans-serif. Títulos amplos têm peso 500 e espaçamento apertado; navegação e ações usam 600. A marca textual usa 700.

Display serve à abertura; headline aos capítulos; title aos blocos de conexão; body às introduções; label à navegação e links. O texto introdutório ocupa até 415px. Legendas e notas usam 11–13px. A hierarquia encolhe nos breakpoints sem transformar as capturas em texto ilegível.

## Layout

Contêiner central com máximo de 1440px e margens de 56px; passa a 32px até 1100px e 20px até 760px. Cabeçalhos usam duas colunas e os recursos compartilhados usam três; ambos passam a uma coluna até 760px.

A abertura acompanha a rolagem nativa com uma área sticky; o percurso principal tem 156svh, mínimo de 1180px. Até 480px, a composição do Orb vira uma sequência vertical: Orb, painel com 88% de largura, terminal com 94% e legenda. O segundo terminal e os fios são ocultados. Até 640px, o Workspace usa um recorte do mesmo componente. A navegação principal some até 480px; o download permanece no cabeçalho.

Movimento reduzido e ausência de JavaScript compactam a abertura. Não há captura da rolagem nem carregamento obrigatório antes do conteúdo.

## Elevation & Depth

Profundidade vem da luz da escultura, das sombras das capturas e de pequenos deslocamentos de perspectiva. Terminais usam sombra difusa; a janela do Workspace recebe uma sombra mais profunda sobre a superfície escura. Os valores exatos ficam nas extensões do sidecar.

**The Readable Product Rule.** Sombras e perspectiva acompanham as capturas, mantendo texto, legendas e partes essenciais da interface legíveis.

## Shapes

Cantos discretos distinguem etiquetas, ações e janelas pelos tokens de raio. Círculos ficam nas setas e no símbolo de Relay. Linhas finas conectam informações e dividem seções. A abertura usa lâminas metálicas de cantos suaves ao redor do Orb; sua alternativa estática mantém o contorno concêntrico.

## Components

- **Ações:** links preenchidos, altura mínima de 58px; a variante compacta tem 43px, passando a 39px no celular. Hover eleva 2px e desloca a seta 2px. Foco usa contorno de 2px com afastamento de 6px.
- **Navegação e links:** sublinhado animado no cabeçalho; links de documentação mantêm uma linha inferior e seta diagonal. A chamada de rolagem usa uma seta circular.
- **Etiqueta de estágio:** “In development” tem borda fina, fundo transparente e texto compacto. Não é um controle.
- **Capturas:** arquivos em `assets/product/`, com texto alternativo e identificação das sessões ilustrativas. O painel do Orb surge na rolagem; o Workspace resolve a perspectiva em uma vista frontal.
- **Recursos compartilhados:** lista de definição sobre a superfície escura, separada por linha fina, com títulos e descrições curtos.
- **Mapa conectado:** três pontos Lume → Relay → Node e uma rota LAN pontilhada. A trajetória acompanha a rolagem e descreve a arquitetura planejada. A primeira fase do Node informa identidade, pareamento e saúde em leitura.
- **Escultura:** lâminas instanciadas em Three.js, entrada de 1,6s e resposta suave ao ponteiro. A renderização para ao estabilizar e pausa fora da tela ou em aba oculta. Movimento reduzido, economia de dados e falha de WebGL usam a escultura estática; movimento reduzido também remove deslocamentos de conteúdo. Não há áudio.

O sidecar contém oito exemplos HTML/CSS independentes. Campos de conversa pertencem às capturas do aplicativo; a landing não implementa formulários ou controles de agentes.

## Do's and Don'ts

### Do:

- Apresentar Orb e Workspace como modos completos com as mesmas capacidades, respeitando os limites de cada integração.
- Usar capturas reais e identificar as sessões ilustrativas.
- Preservar foco visível, navegação por teclado, texto alternativo e conteúdo sem JavaScript.
- Manter GitHub, Jira e execução distribuída identificados como “In development”, conforme a documentação do produto.

### Don't:

- Não substituir a apresentação por controles fictícios de agentes, revisão ou docking.
- Não atribuir execução remota completa à primeira fase do Node.
- Não condicionar a leitura ao WebGL nem sobrepor legendas e interfaces nas telas pequenas.
