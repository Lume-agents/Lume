/** One stop of a guided tour: what it points at and what it says, in English and Portuguese. */
export type TourStep = {
  id: string;
  /** `data-tour` names to highlight, the first one on screen wins; none centers the card. */
  targets: string[];
  title: [string, string];
  body: [string, string];
  /** A short loop of the gesture, under static/tour. */
  media?: string;
  /** Small controls in a row (the header buttons) take the card below, clear of their neighbours. */
  below?: boolean;
};

export const workspaceTourKey = "lume:tour:workspace:v1";
export const boardTourKey = "lume:tour:board:v1";
export const orbTourKey = "lume:tour:orb:v1";

export const workspaceTour: TourStep[] = [
  { id: "welcome", targets: [], title: ["This is the Workspace", "Este é o Workspace"], body: ["Chat with several coding agents at once and follow everything they do. A minute to see where things are.", "Converse com vários agentes de código ao mesmo tempo e acompanhe tudo o que eles fazem. Um minuto para ver onde fica cada coisa."] },
  { id: "agents", targets: ["agents"], title: ["Your agents", "Seus agentes"], body: ["Every chat with Claude Code, Codex, Gemini or OpenCode is listed here, including the ones you started in a terminal. The dot shows who is working, waiting or needs you.", "Cada chat com Claude Code, Codex, Gemini ou OpenCode aparece aqui, inclusive os que você abriu no terminal. O ponto mostra quem está trabalhando, esperando ou precisa de você."] },
  { id: "launcher", targets: ["new-chat"], below: true, title: ["Start a chat", "Comece um chat"], body: ["Pick an agent and a folder, or resume an earlier session.", "Escolha um agente e uma pasta, ou retome uma sessão anterior."] },
  { id: "split", targets: ["workbench"], media: "workspace-drag", title: ["Side by side", "Lado a lado"], body: ["Drag an agent from the list next to a chat to see up to three at once. Drag a header to swap them.", "Arraste um agente da lista para o lado de um chat para ver até três ao mesmo tempo. Arraste um cabeçalho para trocar a ordem."] },
  { id: "composer", targets: ["composer"], title: ["Talk to it", "Fale com ele"], body: ["Type @ to point at files and folders, / for the agent's commands and skills. Attach images by pasting or dropping them.", "Digite @ para citar arquivos e pastas, / para os comandos e skills do agente. Anexe imagens colando ou soltando."] },
  { id: "events", targets: ["events", "conversation"], media: "file-card", title: ["See every step", "Veja cada passo"], body: ["Commands, edits, skills and git operations show up as the agent works. Click an edited file to see its diff.", "Comandos, edições, skills e operações git aparecem enquanto o agente trabalha. Clique num arquivo editado para ver o diff."] },
  { id: "inspector", targets: ["inspector"], below: true, title: ["Inspector and review", "Inspector e revisão"], body: ["The inspector shows the session, its changed files and the repository. From there, the review center compares every change and lets you comment on lines.", "O inspector mostra a sessão, os arquivos alterados e o repositório. De lá, a central de revisão compara cada mudança e deixa você comentar linhas."] },
  { id: "board", targets: ["board"], below: true, media: "workflow-board", title: ["Workflows", "Workflows"], body: ["On the board, chain agents into steps (plan, build, review) and Lume hands the work from one to the next, with your approval when you want it.", "No board, encadeie agentes em etapas (planejar, implementar, revisar) e o Lume passa o trabalho de um para o outro, com a sua aprovação quando quiser."] },
  { id: "extensions", targets: ["extensions"], below: true, title: ["Extensions", "Extensões"], body: ["Each agent's plugins, skills, MCP servers and instructions in one place, with a catalog to install more.", "Plugins, skills, servidores MCP e instruções de cada agente num só lugar, com um catálogo para instalar mais."] },
  { id: "settings", targets: ["settings"], below: true, title: ["Make it yours", "Do seu jeito"], body: ["Appearance, notifications and this tour again live in Settings. The Lume head takes you back to the Orb.", "Aparência, notificações e este tour de novo ficam em Ajustes. A cabeça do Lume volta para o Orb."] },
];

export const boardTour: TourStep[] = [
  { id: "board-welcome", targets: [], title: ["Workflows", "Workflows"], body: ["Chain agents into steps and Lume hands the work from one to the next. Here is how to build one.", "Encadeie agentes em etapas e o Lume passa o trabalho de um para o outro. Veja como montar um."] },
  { id: "board-add", targets: ["agents"], media: "board-build", title: ["Drag agents in", "Arraste os agentes"], body: ["Drag a chat from the list onto the canvas: each card is a step, done by that agent. Pull the dot on a card's right edge to another card to connect them.", "Arraste um chat da lista para o canvas: cada card é uma etapa, feita por aquele agente. Puxe a bolinha da direita de um card até outro card para ligá-los."] },
  { id: "board-role", targets: ["board-card", "board-canvas"], title: ["Give each step a role", "Dê um papel a cada etapa"], body: ["Click a card to pick its role (planner, implementer, reviewer, tester…) and say what it should do and hand over.", "Clique num card para escolher o papel (planejador, implementador, revisor, testador…) e dizer o que ele deve fazer e entregar."] },
  { id: "board-connect", targets: ["board-link", "board-port", "board-canvas"], title: ["Decide what passes on", "Decida o que passa adiante"], body: ["Click a connection to choose what the next step receives (the reply, the files, the checks) and whether it waits for your approval.", "Clique numa ligação para escolher o que a próxima etapa recebe (a resposta, os arquivos, os testes) e se ela espera a sua aprovação."] },
  { id: "board-run", targets: ["board-run"], media: "workflow-board", title: ["Run it", "Execute"], body: ["Write the goal and run. The steps go one after another; you can pause, stop or approve each handoff here.", "Escreva o objetivo e execute. As etapas rodam uma depois da outra; aqui você pausa, para ou aprova cada passagem."] },
  { id: "board-timeline", targets: ["board-mode"], below: true, title: ["Follow the run", "Acompanhe a execução"], body: ["The timeline shows who did what, for how long, and where the work changed hands.", "A linha do tempo mostra quem fez o quê, por quanto tempo e onde o trabalho trocou de mãos."] },
];

export const orbTour: TourStep[] = [
  { id: "orb-welcome", targets: [], media: "orb-intro", title: ["This is the Orb", "Este é o Orb"], body: ["It sits over your windows and follows every coding agent on this computer. When one needs you, approve or reply right here.", "Ele fica sobre as suas janelas e acompanha todos os agentes de código do computador. Quando um precisa de você, aprove ou responda daqui mesmo."] },
  { id: "orb-sessions", targets: ["orb-sessions"], title: ["Your agents", "Seus agentes"], body: ["Each row is a session. Its color says whether it is working, ready for review or waiting for your approval. Click it to read the reply and answer.", "Cada linha é uma sessão. A cor diz se ela está trabalhando, pronta para revisão ou esperando a sua aprovação. Clique para ler a resposta e responder."] },
  { id: "orb-new", targets: ["orb-new"], below: true, title: ["Start a session", "Comece uma sessão"], body: ["Open an agent in a folder, or resume an earlier conversation.", "Abra um agente numa pasta ou retome uma conversa anterior."] },
  { id: "orb-search", targets: ["orb-search"], below: true, title: ["Command palette", "Paleta de comandos"], body: ["Find sessions and actions from the keyboard. The global shortcut brings the Orb up from anywhere.", "Encontre sessões e ações pelo teclado. O atalho global abre o Orb de qualquer lugar."] },
  { id: "orb-terminals", targets: ["orb-terminals"], media: "orb-dock", title: ["Terminals and workflows", "Terminais e workflows"], body: ["Open sessions as terminal windows. In Workflow mode they dock side by side and the work passes from one agent to the next.", "Abra as sessões como janelas de terminal. No modo Workflow elas se encaixam lado a lado e o trabalho passa de um agente para o próximo."] },
  { id: "orb-inspector", targets: ["orb-inspector"], title: ["Inspector", "Inspector"], body: ["The focused session's context: its reply, changed files and repository.", "O contexto da sessão em foco: a resposta, os arquivos alterados e o repositório."] },
  { id: "orb-workspace", targets: ["orb-workspace"], below: true, title: ["Need more room?", "Precisa de mais espaço?"], body: ["The Workspace opens chats side by side, with the workflow board and extensions.", "O Workspace abre os chats lado a lado, com o board de workflow e as extensões."] },
  { id: "orb-collapse", targets: ["orb-collapse"], below: true, title: ["Tuck it away", "Recolha"], body: ["Collapse the Orb into a small capsule. It lets you know when someone needs you, and you can drag it anywhere.", "Recolha o Orb numa cápsula pequena. Ela avisa quando alguém precisa de você, e dá para arrastá-la para qualquer lugar."] },
  { id: "orb-settings", targets: ["orb-settings"], title: ["Settings", "Ajustes"], body: ["Agents, appearance, shortcuts and this tour again.", "Agentes, aparência, atalhos e este tour de novo."] },
];
