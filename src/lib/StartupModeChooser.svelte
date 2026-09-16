<script lang="ts">
  import BrandIcon from "$lib/BrandIcon.svelte";
  import type { Language } from "$lib/i18n";

  type StartupMode = "orb" | "workspace";

  let {
    language,
    onChoose,
  } = $props<{
    language: Language;
    onChoose: (mode: StartupMode, remember: boolean) => void | Promise<void>;
  }>();

  let remember = $state(false);
  let choosing = $state<StartupMode | null>(null);

  function tr(english: string, portuguese: string) {
    return language === "pt-BR" ? portuguese : english;
  }

  async function choose(mode: StartupMode) {
    if (choosing) return;
    choosing = mode;
    try { await onChoose(mode, remember); }
    finally { choosing = null; }
  }
</script>

<div class="startup-mode-chooser" role="dialog" aria-modal="true" aria-labelledby="startup-title">
  <header>
    <span class="chooser-mark"><BrandIcon name="lume" size={25} /></span>
    <span><strong id="startup-title">{tr("How should Lume open?", "Como o Lume deve abrir?")}</strong><small>{tr("Choose the view for this session.", "Escolha a visualização desta sessão.")}</small></span>
  </header>

  <div class="mode-options">
    <button class="mode-option orb-option" disabled={choosing !== null} type="button" onclick={() => void choose("orb")}>
      <span class="mode-preview orb-preview" aria-hidden="true">
        <span class="orb-compact"><BrandIcon name="lume" size={15} /></span>
        <span class="orb-expanded"><i></i><b></b><em></em><em></em></span>
        <span class="demo-cursor">
          <svg viewBox="0 0 22 28" aria-hidden="true">
            <path d="M3 2.5v20.2l5.1-5.1 4 8.1 3.8-1.9-4-8 7.1-.6L3 2.5Z"></path>
          </svg>
          <i></i>
        </span>
      </span>
      <span class="mode-copy"><strong>{choosing === "orb" ? tr("Opening…", "Abrindo…") : "Orb"}</strong><small>{tr("Quiet monitoring, always close by", "Monitoramento discreto, sempre por perto")}</small></span>
    </button>

    <button class="mode-option workspace-option" disabled={choosing !== null} type="button" onclick={() => void choose("workspace")}>
      <span class="mode-preview workspace-preview" aria-hidden="true">
        <span class="workspace-sidebar"><i></i><i></i><i></i></span>
        <span class="workspace-chat chat-left"><i></i><i></i><i></i></span>
        <span class="workspace-chat chat-right"><i></i><i></i><i></i></span>
      </span>
      <span class="mode-copy"><strong>{choosing === "workspace" ? tr("Opening…", "Abrindo…") : "Workspace"}</strong><small>{tr("Multiple agents in one focused view", "Vários agentes em uma visão focada")}</small></span>
    </button>
  </div>

  <label class="remember-choice">
    <input type="checkbox" bind:checked={remember} />
    <span aria-hidden="true"><i></i></span>
    <b>{tr("Use my choice as the default", "Usar minha escolha como padrão")}</b>
  </label>
</div>

<style>
  .startup-mode-chooser { position: absolute; z-index: 45; inset: 0; padding: 16px 16px 12px; display: flex; flex-direction: column; gap: 12px; overflow: hidden; border-radius: inherit; color: #27342e; background: #f7faf7; }
  header { display: flex; align-items: center; gap: 10px; }
  .chooser-mark { width: 36px; height: 36px; display: grid; place-items: center; flex: 0 0 auto; color: var(--lume-accent-strong); }
  header > span:last-child { min-width: 0; display: grid; gap: 2px; }
  header strong { font-size: 15px; font-weight: 780; letter-spacing: -.025em; }
  header small { color: #6f7e76; font-size: 9px; }
  .mode-options { min-height: 0; display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 9px; flex: 1; }
  .mode-option { min-width: 0; padding: 8px; display: grid; grid-template-rows: 108px auto; gap: 8px; overflow: hidden; border: 1px solid rgba(74, 104, 89, .16); border-radius: 14px; color: inherit; background: rgba(255, 255, 252, .72); text-align: left; cursor: pointer; transition: border-color 160ms ease, background 160ms ease, box-shadow 220ms cubic-bezier(.16, 1, .3, 1), transform 220ms cubic-bezier(.16, 1, .3, 1); }
  .mode-option:hover, .mode-option:focus-visible { border-color: color-mix(in srgb, var(--lume-accent-strong) 45%, transparent); background: #fff; box-shadow: 0 12px 28px rgba(21, 45, 32, .09); outline: 0; transform: translateY(-2px); }
  .mode-option:active { transform: translateY(0) scale(.99); }
  .mode-option:disabled { cursor: wait; opacity: .72; }
  .mode-preview { position: relative; min-height: 0; height: 108px; overflow: hidden; border-radius: 10px; background: #eaf0eb; }
  .mode-copy { min-width: 0; padding: 0 3px 3px; display: grid; gap: 3px; }
  .mode-copy strong { color: #24312b; font-size: 11px; font-weight: 770; }
  .mode-copy small { min-height: 24px; color: #6a7a72; font-size: 8px; line-height: 1.45; }

  .orb-preview { background: linear-gradient(145deg, #e7eee9, #dfe9e3); }
  .orb-compact, .orb-expanded { position: absolute; top: 50%; left: 50%; color: #347d59; background: #f9fcf9; box-shadow: 0 7px 18px rgba(26, 55, 39, .12); }
  .orb-compact { width: 48px; height: 31px; display: grid; place-items: center; border: 1px solid rgba(61, 105, 82, .16); border-radius: 16px; transform: translate(-50%, -50%); animation: orb-compact-cycle 4.8s cubic-bezier(.16, 1, .3, 1) infinite; }
  .orb-expanded { width: 112px; height: 87px; padding: 11px; display: grid; grid-template-columns: 24px 1fr; grid-template-rows: 18px repeat(2, 1fr); gap: 6px; border: 1px solid rgba(61, 105, 82, .14); border-radius: 14px; clip-path: inset(36% 31% round 16px); opacity: 0; transform: translate(-50%, -50%); animation: orb-panel-cycle 4.8s cubic-bezier(.16, 1, .3, 1) infinite; }
  .orb-expanded i { grid-row: 1 / -1; border-radius: 7px; background: rgba(62, 142, 97, .18); }
  .orb-expanded b { border-radius: 4px; background: rgba(45, 82, 62, .18); }
  .orb-expanded em { border-radius: 5px; background: rgba(55, 102, 76, .1); }
  .demo-cursor { position: absolute; top: calc(50% + 42px); left: calc(50% + 58px); width: 18px; height: 23px; color: #1e2521; filter: drop-shadow(0 1px 1px rgba(20, 35, 27, .22)); animation: cursor-open-orb 4.8s cubic-bezier(.16, 1, .3, 1) infinite; }
  .demo-cursor svg { width: 18px; height: 23px; display: block; overflow: visible; }
  .demo-cursor path { fill: #fff; stroke: currentColor; stroke-width: 1.55; stroke-linejoin: round; }
  .demo-cursor i { position: absolute; top: -4px; left: -4px; width: 9px; height: 9px; border: 1px solid color-mix(in srgb, var(--lume-accent-strong) 65%, transparent); border-radius: 50%; opacity: 0; animation: cursor-click-ring 4.8s ease-out infinite; }

  .workspace-preview { display: grid; grid-template-columns: 27px 1fr 1fr; gap: 4px; padding: 8px; background: #e4ece6; }
  .workspace-sidebar, .workspace-chat { min-width: 0; padding: 6px 4px; display: flex; flex-direction: column; gap: 5px; overflow: hidden; border-radius: 6px; background: rgba(250, 252, 249, .88); }
  .workspace-sidebar { gap: 7px; background: rgba(210, 224, 215, .8); }
  .workspace-sidebar i { height: 10px; border-radius: 4px; background: rgba(46, 95, 68, .16); }
  .workspace-sidebar i:first-child { margin-bottom: 5px; background: rgba(56, 155, 102, .36); }
  .workspace-chat { justify-content: flex-end; }
  .workspace-chat i { width: 88%; height: 18px; flex: 0 0 auto; border-radius: 6px; background: rgba(62, 94, 76, .12); animation: message-rise 4.2s cubic-bezier(.16, 1, .3, 1) infinite; }
  .workspace-chat i:nth-child(2) { width: 68%; margin-left: auto; background: rgba(52, 148, 94, .18); animation-delay: .55s; }
  .workspace-chat i:nth-child(3) { width: 78%; animation-delay: 1.1s; }
  .chat-right i { animation-delay: .3s; }.chat-right i:nth-child(2) { animation-delay: .85s; }.chat-right i:nth-child(3) { animation-delay: 1.4s; }

  .remember-choice { min-height: 35px; padding: 0 3px; display: flex; align-items: center; gap: 9px; color: #53655b; cursor: pointer; }
  .remember-choice input { position: absolute; opacity: 0; pointer-events: none; }
  .remember-choice > span { width: 30px; height: 18px; padding: 2px; flex: 0 0 auto; border-radius: 9px; background: #dce5df; transition: background 160ms ease; }
  .remember-choice i { width: 14px; height: 14px; display: block; border-radius: 50%; background: #fff; box-shadow: 0 1px 3px rgba(25, 45, 34, .18); transition: transform 190ms cubic-bezier(.16, 1, .3, 1); }
  .remember-choice input:checked + span { background: var(--lume-accent-strong); }
  .remember-choice input:checked + span i { transform: translateX(12px); }
  .remember-choice input:focus-visible + span { outline: 2px solid color-mix(in srgb, var(--lume-accent-strong) 55%, transparent); outline-offset: 2px; }
  .remember-choice b { font-size: 9px; font-weight: 680; }

  :global(.dark) .startup-mode-chooser { color: #dce8e1; background: #111b16; }
  :global(.dark) .chooser-mark { color: var(--lume-accent); }
  :global(.dark) header small, :global(.dark) .mode-copy small, :global(.dark) .remember-choice { color: #98aaa0; }
  :global(.dark) .mode-option { border-color: rgba(201, 224, 211, .1); background: rgba(27, 41, 34, .8); }
  :global(.dark) .mode-option:hover, :global(.dark) .mode-option:focus-visible { border-color: color-mix(in srgb, var(--lume-accent) 45%, transparent); background: #1c2a23; box-shadow: 0 12px 28px rgba(0, 0, 0, .22); }
  :global(.dark) .mode-copy strong { color: #e1ece6; }
  :global(.dark) .mode-preview { background: #17241d; }
  :global(.dark) .orb-compact, :global(.dark) .orb-expanded, :global(.dark) .workspace-chat { color: var(--lume-accent); border-color: rgba(195, 224, 208, .1); background: #1d2c24; }
  :global(.dark) .workspace-sidebar { background: #1a2821; }
  :global(.dark) .remember-choice > span { background: #2b3a32; }

  @keyframes cursor-open-orb { 0%, 10% { opacity: 0; transform: translate(8px, 8px); } 22%, 30% { opacity: 1; transform: translate(-58px, -42px); } 33% { transform: translate(-58px, -42px) scale(.9); } 39%, 62% { opacity: 1; transform: translate(-58px, -42px); } 76%, 100% { opacity: 0; transform: translate(-34px, -24px); } }
  @keyframes cursor-click-ring { 0%, 30% { opacity: 0; transform: scale(.3); } 34% { opacity: 1; } 45%, 100% { opacity: 0; transform: scale(2.4); } }
  @keyframes orb-compact-cycle { 0%, 31% { opacity: 1; transform: translate(-50%, -50%) scale(1); } 38%, 78% { opacity: 0; transform: translate(-50%, -50%) scale(.86); } 88%, 100% { opacity: 1; transform: translate(-50%, -50%) scale(1); } }
  @keyframes orb-panel-cycle { 0%, 30% { clip-path: inset(36% 31% round 16px); opacity: 0; } 43%, 76% { clip-path: inset(0 round 14px); opacity: 1; } 86%, 100% { clip-path: inset(36% 31% round 16px); opacity: 0; } }
  @keyframes message-rise { 0%, 12% { opacity: 0; transform: translateY(16px) scale(.96); } 28%, 72% { opacity: 1; transform: translateY(0) scale(1); } 88%, 100% { opacity: 0; transform: translateY(-13px) scale(.98); } }

  @media (prefers-reduced-motion: reduce) { .orb-compact, .orb-expanded, .demo-cursor, .demo-cursor i, .workspace-chat i { animation: none; }.orb-compact { opacity: 0; }.orb-expanded { clip-path: inset(0 round 14px); opacity: 1; }.demo-cursor { display: none; }.workspace-chat i { opacity: 1; transform: none; } }
</style>
