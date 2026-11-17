<script lang="ts">
  import { slide } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import type { AgentAlert } from "$lib/agentAlerts";
  import BrandIcon from "$lib/BrandIcon.svelte";
  import LumeIcon from "$lib/LumeIcon.svelte";
  import type { Language } from "$lib/i18n";

  let { alerts, language, now, reducedMotion = false, attached = false, onDismiss }: {
    alerts: AgentAlert[];
    language: Language;
    now?: number;
    reducedMotion?: boolean;
    attached?: boolean;
    onDismiss: (id: string) => void;
  } = $props();

  let clock = $state(Date.now());
  let needsClock = $derived(now === undefined && alerts.some((alert) => alert.usageInfo?.resetsAt != null));

  $effect(() => {
    if (!needsClock) return;
    clock = Date.now();
    const timer = setInterval(() => { clock = Date.now(); }, 10_000);
    return () => clearInterval(timer);
  });

  const tr = (english: string, portuguese: string) => (language === "pt-BR" ? portuguese : english);

  function resetIn(resetsAt: number | undefined) {
    if (!resetsAt) return "";
    const minutes = Math.max(0, Math.round((resetsAt - (now ?? clock)) / 60_000));
    if (minutes < 1) return tr("resets now", "redefine agora");
    const days = Math.floor(minutes / 1_440);
    const hours = Math.floor((minutes % 1_440) / 60);
    const rest = minutes % 60;
    const span = days ? `${days}d ${hours}h` : hours ? `${hours}h ${rest}min` : `${rest}min`;
    return tr(`resets in ${span}`, `redefine em ${span}`);
  }
</script>

{#if alerts.length}
  <div class:attached class="usage-banner" role="status" aria-live="polite" transition:slide={{ duration: reducedMotion ? 0 : 180, easing: cubicOut }}>
    {#each alerts as alert (alert.id)}
      {#if alert.usageInfo}
        {@const info = alert.usageInfo}
        <div class="usage-row tone-{alert.tone}" style:--usage-fill={Math.max(info.remaining, 4) / 100}>
          <BrandIcon name={info.agent} size={14} />
          <span class="usage-text">
            <strong>{info.remaining === 0 ? tr("Limit reached", "Limite atingido") : tr(`${info.remaining}% left`, `${info.remaining}% restante`)}</strong>
            <small>{info.agentLabel} · {info.windowLabel}{#if resetIn(info.resetsAt)} · {resetIn(info.resetsAt)}{/if}</small>
          </span>
          <span class="usage-meter" aria-hidden="true"><i></i></span>
          {#if !alert.critical}
            <button type="button" class="usage-close" title={tr("Dismiss", "Fechar")} aria-label={tr(`Dismiss ${info.windowLabel} notice`, `Fechar aviso de ${info.windowLabel}`)} onclick={() => onDismiss(alert.id)}><LumeIcon name="close" size={11} /></button>
          {/if}
        </div>
      {/if}
    {/each}
  </div>
{/if}

<style>
  .usage-banner { box-sizing: border-box; width: min(440px, 100%); min-width: 0; margin: 0 auto 8px; align-self: center; flex: 0 0 auto; display: flex; flex-direction: column; overflow: hidden; border: 1px solid color-mix(in srgb, #c78d35 28%, var(--workspace-line, transparent)); border-radius: 10px; background: color-mix(in srgb, #c78d35 8%, var(--workspace-raised, var(--dropdown-bg, transparent))); }
  .usage-banner.attached { width: auto; align-self: stretch; margin: calc(-1 * var(--field-inset)) calc(-1 * var(--field-inset)) 3px; border: 0; border-radius: calc(var(--field-radius) - 1px) calc(var(--field-radius) - 1px) 0 0; }
  .usage-row { --tone: #c78d35; position: relative; min-width: 0; padding: 8px 10px; display: flex; align-items: center; gap: 9px; color: var(--workspace-strong, var(--dropdown-text, inherit)); }
  .usage-row + .usage-row { border-top: 1px solid color-mix(in srgb, #c78d35 14%, transparent); }
  .usage-row.tone-error { --tone: #c45f5b; }
  .usage-row > :global(.lume-icon:last-child) { color: var(--tone); }
  .usage-text { min-width: 0; flex: 1 1 auto; display: grid; gap: 1px; }
  .usage-text strong { font-size: 12px; font-weight: 700; line-height: 1.2; }
  .usage-text small { overflow: hidden; color: var(--workspace-muted, inherit); opacity: .85; font-size: 10.5px; text-overflow: ellipsis; white-space: nowrap; }
  .usage-meter { position: relative; width: 54px; height: 5px; flex: 0 1 54px; overflow: hidden; border-radius: 999px; background: color-mix(in srgb, var(--tone) 22%, transparent); }
  .usage-meter i { position: absolute; inset: 0; border-radius: inherit; background: var(--tone); transform: scaleX(var(--usage-fill)); transform-origin: left; transition: transform 360ms cubic-bezier(.16, 1, .3, 1); }
  .usage-close { width: 20px; height: 20px; padding: 0; display: grid; flex: 0 0 auto; place-items: center; border: 0; border-radius: 6px; color: inherit; background: transparent; opacity: .6; }
  .usage-close:hover { opacity: 1; background: color-mix(in srgb, var(--tone) 18%, transparent); }
  @media (prefers-reduced-motion: reduce) { .usage-meter i { transition: none; } }
</style>
