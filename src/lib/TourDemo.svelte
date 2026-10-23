<script lang="ts" module>
  export type TourDemoName = "orb-approve" | "orb-dock" | "orb-capsule";
</script>

<script lang="ts">
  import type { Language } from "$lib/i18n";

  // Small looping drawings of a gesture, for surfaces too small for a recorded clip (the Orb).
  let { name, language = "en" } = $props<{ name: TourDemoName; language?: Language }>();
  const pt = $derived(language === "pt-BR");
</script>

<div class="demo demo-{name}" aria-hidden="true">
  {#if name === "orb-approve"}
    <div class="panel">
      <div class="bar"><i class="dino"></i><b></b></div>
      {#each [0, 1, 2] as row (row)}
        <div class="row row-{row}"><i class="avatar"></i><span><b></b><small></small></span><em class="state"></em></div>
      {/each}
      <div class="ask">
        <strong>{pt ? "Claude Code quer executar" : "Claude Code wants to run"}</strong>
        <code>$ npm test -- auth</code>
        <span class="buttons"><i>{pt ? "Negar" : "Deny"}</i><i class="allow">{pt ? "Permitir" : "Allow"}</i></span>
      </div>
    </div>
    <i class="cursor"></i>
  {:else if name === "orb-dock"}
    {#each [0, 1, 2] as pane (pane)}
      <div class="term term-{pane}">
        <div class="term-bar"><i></i><b></b></div>
        <span class="line l1"></span><span class="line l2"></span><span class="line l3"></span>
      </div>
    {/each}
    <i class="link link-0"></i><i class="link link-1"></i>
    <i class="baton"></i>
  {:else}
    <div class="shell">
      <div class="panel-lines"><b></b><b></b><b></b></div>
      <i class="dino"></i>
      <i class="badge">1</i>
    </div>
    <span class="caption">{pt ? "Precisa de você" : "Needs you"}</span>
  {/if}
</div>

<style>
  .demo { --c-accent: var(--tour-accent, #4fae7e); --c-line: var(--tour-line, rgba(205, 222, 213, .14)); --c-raised: var(--tour-raised, #1f2924); --c-muted: var(--tour-muted, #9aaba2); --c-strong: var(--tour-strong, #eef5f1); --c-amber: #d8a24a;
    position: relative; width: 100%; aspect-ratio: 16 / 9; overflow: hidden; border-radius: 15px 15px 0 0; border-bottom: 1px solid var(--c-line);
    background: radial-gradient(circle at 30% 20%, color-mix(in srgb, var(--c-accent) 16%, transparent), transparent 55%), radial-gradient(circle at 80% 90%, color-mix(in srgb, var(--c-amber) 12%, transparent), transparent 50%), color-mix(in srgb, var(--c-raised) 55%, #0b120e); }
  .demo :global(*) { box-sizing: border-box; }
  b, small, i, em { display: block; }

  /* Approving a permission from the Orb. */
  .panel { position: absolute; left: 50%; top: 9%; width: 46%; height: 82%; padding: 6px; border: 1px solid var(--c-line); border-radius: 12px; background: var(--c-raised); box-shadow: 0 10px 26px rgba(0, 0, 0, .35); transform: translateX(-50%); }
  .bar { height: 14%; display: flex; align-items: center; gap: 6px; padding: 0 4px; border-bottom: 1px solid var(--c-line); }
  .bar b { width: 34%; height: 5px; border-radius: 3px; background: var(--c-muted); opacity: .5; }
  .dino { width: 12px; height: 10px; border-radius: 3px 5px 2px 2px; background: #5d93c4; }
  .row { height: 22%; margin-top: 4%; display: flex; align-items: center; gap: 6px; padding: 0 4px; border-radius: 7px; }
  .avatar { width: 18px; height: 18px; flex: 0 0 auto; border-radius: 6px; background: color-mix(in srgb, var(--c-accent) 40%, #e8efe9); }
  .row-1 .avatar { background: #e2c66d; }
  .row-2 .avatar { background: #b49ae8; }
  .row span { flex: 1; display: grid; gap: 4px; }
  .row span b { width: 70%; height: 5px; border-radius: 3px; background: var(--c-strong); opacity: .7; }
  .row span small { width: 45%; height: 4px; border-radius: 3px; background: var(--c-muted); opacity: .5; }
  .state { width: 7px; height: 7px; border-radius: 50%; background: var(--c-accent); }
  .row-2 { animation: needs 6s infinite; }
  .row-2 .state { animation: state 6s infinite; }
  .ask { position: absolute; left: 6%; right: 6%; bottom: 7%; padding: 7px 8px; display: grid; gap: 5px; border: 1px solid color-mix(in srgb, var(--c-amber) 50%, var(--c-line)); border-radius: 9px; background: color-mix(in srgb, var(--c-raised) 92%, var(--c-amber)); box-shadow: 0 8px 20px rgba(0, 0, 0, .4); opacity: 0; transform: translateY(8px); animation: ask 6s infinite; }
  .ask strong { color: var(--c-amber); font: 700 7.5px/1.2 var(--lume-font-ui, Inter, sans-serif); }
  .ask code { padding: 3px 5px; border-radius: 4px; color: var(--c-strong); background: rgba(0, 0, 0, .28); font: 7px/1.3 var(--lume-font-code, monospace); }
  .buttons { display: flex; gap: 4px; }
  .buttons i { padding: 3px 6px; border-radius: 5px; color: var(--c-muted); background: rgba(255, 255, 255, .06); font: 700 6.5px/1 var(--lume-font-ui, Inter, sans-serif); font-style: normal; }
  .buttons .allow { color: #0d1a13; background: var(--c-accent); animation: press 6s infinite; }
  .cursor { position: absolute; left: 70%; top: 92%; width: 14px; height: 14px; clip-path: polygon(0 0, 100% 62%, 58% 66%, 78% 100%, 62% 100%, 42% 70%, 0 92%); background: #f7faf5; filter: drop-shadow(0 1px 1px #000); animation: cursor 6s infinite; }
  @keyframes needs { 0%, 18% { background: transparent; } 24%, 62% { background: color-mix(in srgb, var(--c-amber) 16%, transparent); } 70%, 100% { background: transparent; } }
  @keyframes state { 0%, 18% { background: #7aa7d8; } 24%, 64% { background: var(--c-amber); box-shadow: 0 0 0 3px color-mix(in srgb, var(--c-amber) 25%, transparent); } 70%, 100% { background: var(--c-accent); } }
  @keyframes ask { 0%, 26% { opacity: 0; transform: translateY(8px); } 32%, 62% { opacity: 1; transform: none; } 68%, 100% { opacity: 0; transform: translateY(6px); } }
  @keyframes cursor { 0%, 34% { left: 76%; top: 96%; } 52% { left: 50%; top: 82%; } 56% { left: 50%; top: 82%; transform: scale(.82); } 60%, 100% { left: 50%; top: 82%; transform: scale(1); opacity: 1; } 72%, 100% { opacity: 0; } }
  @keyframes press { 0%, 54% { filter: none; } 56% { filter: brightness(1.35); transform: scale(.94); } 60%, 100% { filter: none; transform: none; } }

  /* Terminals docking into a workflow, the work passing along. */
  .term { position: absolute; top: 24%; width: 26%; height: 54%; padding: 5px; border: 1px solid var(--c-line); border-radius: 9px; background: var(--c-raised); box-shadow: 0 8px 20px rgba(0, 0, 0, .35); animation: 6.5s infinite cubic-bezier(.2, .8, .2, 1); }
  .term-0 { left: 11%; animation-name: dock-0; }
  .term-1 { left: 37%; animation-name: dock-1; }
  .term-2 { left: 63%; animation-name: dock-2; }
  .term-bar { height: 16%; display: flex; align-items: center; gap: 4px; border-bottom: 1px solid var(--c-line); }
  .term-bar i { width: 9px; height: 9px; border-radius: 3px; background: color-mix(in srgb, var(--c-accent) 50%, #e8efe9); }
  .term-1 .term-bar i { background: #8fc0ea; }
  .term-2 .term-bar i { background: #e2c66d; }
  .term-bar b { width: 46%; height: 4px; border-radius: 2px; background: var(--c-muted); opacity: .5; }
  .line { display: block; height: 4px; margin-top: 9%; border-radius: 2px; background: var(--c-muted); opacity: .35; }
  .l1 { width: 80%; } .l2 { width: 60%; } .l3 { width: 70%; }
  .link { position: absolute; top: calc(51% - 3px); width: 6px; height: 6px; border-radius: 50%; background: var(--c-accent); box-shadow: 0 0 0 3px color-mix(in srgb, var(--c-accent) 25%, transparent); opacity: 0; animation: link 6.5s infinite; }
  .link-0 { left: calc(37% - 3px); }
  .link-1 { left: calc(63% - 3px); }
  .baton { position: absolute; top: calc(51% - 4px); left: 24%; width: 8px; height: 8px; border-radius: 50%; background: #f2f7f4; box-shadow: 0 0 10px var(--c-accent); opacity: 0; animation: baton 6.5s infinite; }
  @keyframes dock-0 { 0%, 8% { transform: translate(-14%, -22%) rotate(-7deg); } 30%, 100% { transform: none; } }
  @keyframes dock-1 { 0%, 8% { transform: translate(6%, 26%) rotate(5deg); } 30%, 100% { transform: none; } }
  @keyframes dock-2 { 0%, 8% { transform: translate(18%, -16%) rotate(8deg); } 30%, 100% { transform: none; } }
  @keyframes link { 0%, 30% { opacity: 0; } 36%, 92% { opacity: 1; } 100% { opacity: 0; } }
  @keyframes baton { 0%, 40% { left: 24%; opacity: 0; } 44% { opacity: 1; left: 24%; } 60% { left: 50%; } 66% { left: 50%; } 82% { left: 76%; opacity: 1; } 92%, 100% { left: 76%; opacity: 0; } }

  /* The panel tucked into the capsule, which speaks up when someone needs you. */
  .shell { position: absolute; left: 50%; top: 50%; width: 40%; height: 74%; border: 1px solid var(--c-line); border-radius: 12px; background: var(--c-raised); box-shadow: 0 10px 26px rgba(0, 0, 0, .35); transform: translate(-50%, -50%); animation: shell 6s infinite cubic-bezier(.2, .8, .2, 1); }
  .panel-lines { position: absolute; inset: 22% 10% 10%; display: grid; align-content: start; gap: 12%; animation: lines 6s infinite; }
  .panel-lines b { height: 6px; border-radius: 3px; background: var(--c-muted); opacity: .4; }
  .shell .dino { position: absolute; left: 10px; top: 8px; transition: none; animation: dino 6s infinite; }
  .badge { position: absolute; right: -6px; top: -6px; width: 16px; height: 16px; display: grid; place-items: center; border-radius: 50%; color: #1a1306; background: var(--c-amber); font: 800 8px/1 var(--lume-font-ui, Inter, sans-serif); font-style: normal; opacity: 0; animation: badge 6s infinite; }
  .caption { position: absolute; left: 50%; top: 72%; color: var(--c-amber); font: 700 9px/1 var(--lume-font-ui, Inter, sans-serif); white-space: nowrap; opacity: 0; transform: translateX(-50%); animation: caption 6s infinite; }
  @keyframes shell { 0%, 14% { width: 40%; height: 74%; border-radius: 12px; } 30%, 78% { width: 15%; height: 15%; border-radius: 16px; } 92%, 100% { width: 40%; height: 74%; border-radius: 12px; } }
  @keyframes lines { 0%, 12% { opacity: 1; } 20%, 84% { opacity: 0; } 94%, 100% { opacity: 1; } }
  @keyframes dino { 0%, 14% { left: 10px; top: 8px; } 30%, 78% { left: calc(50% - 6px); top: calc(50% - 5px); } 92%, 100% { left: 10px; top: 8px; } }
  @keyframes badge { 0%, 40% { opacity: 0; transform: scale(.4); } 46% { opacity: 1; transform: scale(1.15); } 50%, 76% { opacity: 1; transform: none; } 82%, 100% { opacity: 0; } }
  @keyframes caption { 0%, 44% { opacity: 0; } 50%, 74% { opacity: 1; } 80%, 100% { opacity: 0; } }

  @media (prefers-reduced-motion: reduce) { .demo :global(*) { animation: none !important; } .ask, .link, .badge, .caption { opacity: 1; transform: none; } }
</style>
