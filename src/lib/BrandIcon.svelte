<script lang="ts">
  import OfflineIcon from "@iconify/svelte/dist/OfflineIcon.svelte";
  import brave from "@iconify-icons/logos/brave";
  import chrome from "@iconify-icons/logos/chrome";
  import edge from "@iconify-icons/logos/microsoft-edge";
  import openai from "@iconify-icons/logos/openai-icon";
  import vscode from "@iconify-icons/logos/visual-studio-code";
  import { siClaude, siDeepseek, siGithub, siGooglegemini, siOpencode } from "simple-icons";

  type Brand =
    | "codex"
    | "chatgpt"
    | "claude"
    | "claude_code"
    | "antigravity"
    | "opencode"
    | "grok"
    | "omp"
    | "deepseek"
    | "gemini"
    | "vscode"
    | "chrome"
    | "edge"
    | "brave"
    | "browsers"
    | "lume"
    | "github"
    | "terminal"
    | "unknown";

  let { name, size = 18 }: { name: Brand; size?: number } = $props();
</script>

<span class="brand-icon" class:openai={name === "codex" || name === "chatgpt"} class:browsers={name === "browsers"} class:line={name === "terminal" || name === "unknown"} style:--brand-size={`${size}px`}>
  {#if name === "github"}
    <svg viewBox="0 0 24 24" aria-hidden="true"><path d={siGithub.path} fill="currentColor" /></svg>
  {:else if name === "codex" || name === "chatgpt"}
    <OfflineIcon icon={openai} />
  {:else if name === "claude" || name === "claude_code"}
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d={siClaude.path} fill="#d97757" />
    </svg>
  {:else if name === "antigravity"}
    <svg viewBox="60 78 420 390" aria-hidden="true">
      <defs>
        <linearGradient id="lume-antigravity-base" x1="0.76" y1="0.08" x2="0.48" y2="0.9">
          <stop stop-color="#f04445" />
          <stop offset="0.4" stop-color="#8b62dd" />
          <stop offset="1" stop-color="#2f80ed" />
        </linearGradient>
        <radialGradient id="lume-antigravity-light" cx="0.26" cy="0.12" r="0.72">
          <stop stop-color="#fbbc04" />
          <stop offset="0.43" stop-color="#34a853" stop-opacity="0.94" />
          <stop offset="1" stop-color="#34a853" stop-opacity="0" />
        </radialGradient>
      </defs>
      <path d="M232 94 225 99 209 116 193 143 171 202 145 296 126 350 107 388 93 408 70 434 67 441 67 447 74 454 88 455 100 451 116 440 129 429 150 406 167 382 204 320 221 301 230 294 248 286 257 284 282 284 305 292 315 299 327 311 341 330 380 395 394 413 416 435 441 452 447 454 464 453 470 446 469 437 464 428 446 409 435 394 418 363 406 334 393 295 370 211 357 171 349 151 336 126 326 112 312 98 303 92 288 86 278 84 260 84 244 88Z" fill="url(#lume-antigravity-base)" />
      <path d="M232 94 225 99 209 116 193 143 171 202 145 296 126 350 107 388 93 408 70 434 67 441 67 447 74 454 88 455 100 451 116 440 129 429 150 406 167 382 204 320 221 301 230 294 248 286 257 284 282 284 305 292 315 299 327 311 341 330 380 395 394 413 416 435 441 452 447 454 464 453 470 446 469 437 464 428 446 409 435 394 418 363 406 334 393 295 370 211 357 171 349 151 336 126 326 112 312 98 303 92 288 86 278 84 260 84 244 88Z" fill="url(#lume-antigravity-light)" />
    </svg>
  {:else if name === "omp"}
    <svg viewBox="0 0 64 64" aria-hidden="true">
      <defs>
        <linearGradient id="lume-omp-mark" x1="0%" y1="0%" x2="100%" y2="100%">
          <stop stop-color="#ed4abf" />
          <stop offset="55%" stop-color="#9b4dff" />
          <stop offset="100%" stop-color="#5ad8e6" />
        </linearGradient>
      </defs>
      <path d="M10 14h44v9H43v33h-9V23h-9v22h-9V23H10z" fill="url(#lume-omp-mark)" />
    </svg>
  {:else if name === "deepseek"}
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d={siDeepseek.path} fill="#5786fe" />
    </svg>
  {:else if name === "opencode"}
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d={siOpencode.path} fill="currentColor" />
    </svg>
  {:else if name === "grok"}
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path fill="currentColor" fill-rule="evenodd" d="M9.27 15.29l7.978-5.897c.391-.29.95-.177 1.137.272.98 2.369.542 5.215-1.41 7.169-1.951 1.954-4.667 2.382-7.149 1.406l-2.711 1.257c3.889 2.661 8.611 2.003 11.562-.953 2.341-2.344 3.066-5.539 2.388-8.42l.006.007c-.983-4.232.242-5.924 2.75-9.383.06-.082.12-.164.179-.248l-3.301 3.305v-.01L9.267 15.292M7.623 16.723c-2.792-2.67-2.31-6.801.071-9.184 1.761-1.763 4.647-2.483 7.166-1.425l2.705-1.25a7.808 7.808 0 00-1.829-1A8.975 8.975 0 005.984 5.83c-2.533 2.536-3.33 6.436-1.962 9.764 1.022 2.487-.653 4.246-2.34 6.022-.599.63-1.199 1.259-1.682 1.925l7.62-6.815" />
    </svg>
  {:else if name === "gemini"}
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <defs>
        <linearGradient id="lume-gemini" x1="3" y1="21" x2="21" y2="3" gradientUnits="userSpaceOnUse">
          <stop stop-color="#4e82ee" />
          <stop offset="0.48" stop-color="#8e75d5" />
          <stop offset="1" stop-color="#d96570" />
        </linearGradient>
      </defs>
      <path d={siGooglegemini.path} fill="url(#lume-gemini)" />
    </svg>
  {:else if name === "vscode"}
    <OfflineIcon icon={vscode} />
  {:else if name === "chrome"}
    <OfflineIcon icon={chrome} />
  {:else if name === "edge"}
    <OfflineIcon icon={edge} />
  {:else if name === "brave"}
    <OfflineIcon icon={brave} />
  {:else if name === "browsers"}
    <i class="browser-icon chrome"><OfflineIcon icon={chrome} /></i>
    <i class="browser-icon edge"><OfflineIcon icon={edge} /></i>
    <i class="browser-icon brave"><OfflineIcon icon={brave} /></i>
  {:else if name === "lume"}
    <img class="lume" src="/lume.svg" alt="" />
  {:else if name === "terminal"}
    <svg class="terminal" viewBox="0 0 24 24" aria-hidden="true">
      <path d="m5 7 5 5-5 5M16 7v10" />
    </svg>
  {:else}
    <svg class="unknown" viewBox="0 0 24 24" aria-hidden="true">
      <path d="M12 3v18M3 12h18M5.6 5.6l12.8 12.8M18.4 5.6 5.6 18.4" />
    </svg>
  {/if}
</span>

<style>
  .brand-icon {
    position: relative;
    width: var(--brand-size);
    height: var(--brand-size);
    display: inline-grid;
    flex: 0 0 auto;
    place-items: center;
  }

  .lume {
    width: 100%;
    height: 100%;
    display: block;
    object-fit: contain;
  }

  .brand-icon :global(svg) {
    width: 100%;
    height: 100%;
    display: block;
  }

  .brand-icon.openai :global(svg) {
    fill: currentColor;
  }

  .brand-icon:not(.line) :global(svg),
  .brand-icon:not(.line) :global(svg *) {
    stroke: none !important;
    stroke-width: 0 !important;
  }

  .unknown {
    fill: none;
    stroke: currentColor;
    stroke-linecap: round;
    stroke-width: 1.5;
  }

  .terminal {
    fill: none;
    stroke: currentColor;
    stroke-linecap: round;
    stroke-linejoin: round;
    stroke-width: 2;
  }

  .browsers { width: calc(var(--brand-size) * 1.28); }
  .browser-icon { position: absolute; width: 64%; height: 64%; display: grid; filter: drop-shadow(0 1px 1px rgba(25, 37, 32, 0.18)); }
  .browser-icon.chrome { left: 0; top: 0; }
  .browser-icon.edge { right: 0; top: 10%; }
  .browser-icon.brave { left: 26%; bottom: 0; }
</style>
