<script lang="ts">
  import { cubicOut } from "svelte/easing";
  import { fade, fly } from "svelte/transition";
  import LumeIcon, { type LumeIconName } from "$lib/LumeIcon.svelte";

  type BannerTone = "info" | "success" | "warning" | "error";

  let {
    message,
    tone = "info",
    contained = false,
    inline = false,
    duration,
    dismissLabel = "Dismiss",
    onDismiss,
  } = $props<{
    message: string;
    tone?: BannerTone;
    contained?: boolean;
    inline?: boolean;
    duration?: number;
    dismissLabel?: string;
    onDismiss?: () => void;
  }>();

  const icon = $derived<LumeIconName>(tone === "success" ? "check" : tone === "error" ? "close" : tone === "warning" ? "warning" : "bolt");
  let timeout: ReturnType<typeof setTimeout> | null = null;

  function clearDismissTimer() {
    if (timeout) clearTimeout(timeout);
    timeout = null;
  }

  function scheduleDismiss() {
    clearDismissTimer();
    const delay = duration ?? (tone === "error" ? 7200 : tone === "warning" ? 5600 : 3600);
    if (delay > 0 && onDismiss) timeout = setTimeout(onDismiss, delay);
  }

  $effect(() => {
    message;
    tone;
    scheduleDismiss();
    return clearDismissTimer;
  });
</script>

<aside
  class:contained
  class:inline
  class="system-banner tone-{tone}"
  role={tone === "error" ? "alert" : "status"}
  aria-live={tone === "error" ? "assertive" : "polite"}
  onmouseenter={clearDismissTimer}
  onmouseleave={scheduleDismiss}
  in:fly={{ y: -7, duration: 190, easing: cubicOut }}
  out:fade={{ duration: 110 }}
>
  <span class="banner-icon" aria-hidden="true"><LumeIcon name={icon} size={13} strokeWidth={2} /></span>
  <p>{message}</p>
  {#if onDismiss}
    <button type="button" aria-label={dismissLabel} title={dismissLabel} onclick={onDismiss}>
      <LumeIcon name="close" size={12} />
    </button>
  {/if}
</aside>

<style>
  .system-banner {
    --banner-tone: var(--workspace-accent, var(--lume-accent-strong, #3f9e72));
    --banner-surface: var(--workspace-raised, var(--lume-raised-light, #fbfcf8));
    --banner-line: var(--workspace-line, var(--lume-line-light, rgba(90, 110, 99, .25)));
    --banner-text: var(--workspace-text, var(--lume-ink-light, #26332d));
    position: fixed;
    z-index: 80;
    top: max(10px, env(safe-area-inset-top));
    left: 50%;
    width: max-content;
    max-width: min(420px, calc(100vw - 22px));
    min-height: 35px;
    padding: 6px 7px 6px 8px;
    display: flex;
    align-items: center;
    gap: 8px;
    border: 1px solid color-mix(in srgb, var(--banner-tone) 30%, var(--banner-line));
    border-radius: 11px;
    color: var(--banner-text);
    background: color-mix(in srgb, var(--banner-surface) 94%, transparent);
    box-shadow: 0 9px 28px rgba(7, 22, 14, .16);
    backdrop-filter: blur(12px);
    transform: translateX(-50%);
  }
  .system-banner.contained { position: absolute; top: 72px; }
  .system-banner.inline { position: relative; top: auto; left: auto; width: 100%; max-width: none; transform: none; }
  :global(.dark) .system-banner { --banner-surface: var(--workspace-raised, var(--lume-raised-dark, #17221d)); --banner-line: var(--workspace-line, var(--lume-line-dark, rgba(210, 230, 220, .12))); --banner-text: var(--workspace-text, var(--lume-ink-dark, #dce7e1)); }
  .tone-success { --banner-tone: #42a878; }
  .tone-warning { --banner-tone: #c78d35; }
  .tone-error { --banner-tone: #c45f5b; }
  .banner-icon { width: 22px; height: 22px; display: grid; place-items: center; flex: 0 0 auto; border-radius: 7px; color: var(--banner-tone); background: color-mix(in srgb, var(--banner-tone) 12%, transparent); }
  p { min-width: 0; max-width: 48ch; margin: 0; overflow-wrap: anywhere; color: inherit; font: 650 9px/1.4 "Segoe UI Variable", "SF Pro Text", ui-sans-serif, system-ui, sans-serif; }
  button { width: 23px; height: 23px; padding: 0; display: grid; place-items: center; flex: 0 0 auto; border: 0; border-radius: 7px; color: color-mix(in srgb, currentColor 66%, transparent); background: transparent; cursor: pointer; }
  button:hover { color: var(--banner-tone); background: color-mix(in srgb, var(--banner-tone) 9%, transparent); }
  button:focus-visible { outline: 2px solid color-mix(in srgb, var(--banner-tone) 60%, transparent); outline-offset: 1px; }
  @media (prefers-reduced-motion: reduce) { .system-banner { transition: none; } }
</style>
