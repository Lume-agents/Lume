<script lang="ts">
  import { untrack } from "svelte";
  import { cubicOut } from "svelte/easing";
  import { fly } from "svelte/transition";
  import LumeIcon, { type LumeIconName } from "$lib/LumeIcon.svelte";

  type BannerTone = "info" | "success" | "warning" | "error";

  let {
    message,
    tone = "info",
    contained = false,
    inline = false,
    duration,
    dismissLabel = "Dismiss",
    detailsLabel = "View details",
    onDismiss,
    onInspect,
  } = $props<{
    message: string;
    tone?: BannerTone;
    contained?: boolean;
    inline?: boolean;
    duration?: number;
    dismissLabel?: string;
    detailsLabel?: string;
    onDismiss?: () => void;
    onInspect?: () => void;
  }>();

  const icon = $derived<LumeIconName>(tone === "success" ? "check" : tone === "error" ? "close" : tone === "warning" ? "warning" : "bolt");
  let timeout: ReturnType<typeof setTimeout> | null = null;
  let collapseTimer: ReturnType<typeof setTimeout> | null = null;
  // An error shows its whole message first, then shrinks to one line, then leaves.
  const COLLAPSE_AFTER = 4_000;
  const ERROR_LIFETIME = 12_000;
  let collapsed = $state(false);
  let clamped = $state(false);
  let text: HTMLParagraphElement | undefined = $state();
  let textHeight = $state<number | null>(null);
  let settleTimer: ReturnType<typeof setTimeout> | null = null;
  const reducedMotion = typeof window !== "undefined" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  function clearDismissTimer() {
    if (timeout) clearTimeout(timeout);
    if (collapseTimer) clearTimeout(collapseTimer);
    if (settleTimer) clearTimeout(settleTimer);
    timeout = collapseTimer = settleTimer = null;
  }

  /** Animates the text between its full height and one line; the ellipsis comes once it is one line. */
  /** The message's height with every line showing, measured on a copy so the clamp does not hide it. */
  function fullHeight() {
    const copy = text!.cloneNode(true) as HTMLElement;
    copy.style.cssText = `position:absolute;visibility:hidden;height:auto;width:${text!.clientWidth}px;white-space:normal`;
    copy.classList.remove("clamped");
    text!.parentElement!.appendChild(copy);
    const height = copy.scrollHeight;
    copy.remove();
    return height;
  }

  function setCollapsed(next: boolean) {
    if (!text || collapsed === next) return;
    if (!next && clamped) { clamped = false; }
    const full = next ? text.scrollHeight : fullHeight();
    const line = Math.ceil(parseFloat(getComputedStyle(text).lineHeight) || 15);
    if (full <= line + 1) { collapsed = next; return; }
    if (next) {
      textHeight = text.offsetHeight;
      requestAnimationFrame(() => { textHeight = line; });
      collapsed = true;
      settleTimer = setTimeout(() => { clamped = true; }, 240);
    } else {
      clamped = false;
      collapsed = false;
      textHeight = line;
      requestAnimationFrame(() => { textHeight = full; });
      settleTimer = setTimeout(() => { textHeight = null; }, 260);
    }
  }

  function scheduleDismiss() {
    clearDismissTimer();
    const expiring = tone === "error";
    const delay = expiring ? Math.max(duration ?? 0, ERROR_LIFETIME) : (duration ?? (tone === "warning" ? 5600 : 3600));
    if (expiring && !reducedMotion) collapseTimer = setTimeout(() => setCollapsed(true), COLLAPSE_AFTER);
    if (delay > 0 && onDismiss) timeout = setTimeout(() => onDismiss?.(), delay);
  }

  // Pointing at or focusing the banner reads it in full again and keeps it.
  function hold() {
    clearDismissTimer();
    setCollapsed(false);
  }

  $effect(() => {
    message;
    tone;
    duration;
    untrack(() => { collapsed = false; clamped = false; textHeight = null; scheduleDismiss(); });
    return clearDismissTimer;
  });
</script>

<aside
  class:contained
  class:inline
  class:inspectable={Boolean(onInspect)}
  class:collapsed
  class="system-banner tone-{tone}"
  role={tone === "error" ? "alert" : "status"}
  aria-live={tone === "error" ? "assertive" : "polite"}
  onmouseenter={hold}
  onmouseleave={scheduleDismiss}
  onfocusin={hold}
  onfocusout={scheduleDismiss}
  in:fly|global={{ x: reducedMotion ? 0 : -26, opacity: 0, duration: reducedMotion ? 110 : 240, easing: cubicOut }}
  out:fly|global={{ x: reducedMotion ? 0 : -26, opacity: 0, duration: reducedMotion ? 110 : 260, easing: cubicOut }}
>
  <span class="banner-icon" aria-hidden="true"><LumeIcon name={icon} size={13} strokeWidth={2} /></span>
  <p bind:this={text} class:clamped style:height={textHeight === null ? undefined : `${textHeight}px`}>{message}</p>
  {#if onInspect}
    <button type="button" aria-label={detailsLabel} title={detailsLabel} onclick={onInspect}>
      <LumeIcon name="inspector" size={13} />
    </button>
  {/if}
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
  p { min-width: 0; flex: 1; max-width: 48ch; margin: 0; overflow-wrap: anywhere; color: inherit; font: 650 10px/1.45 "Segoe UI Variable", "SF Pro Text", ui-sans-serif, system-ui, sans-serif; }
  p { transition: height 240ms cubic-bezier(.16, 1, .3, 1); overflow: hidden; }
  p.clamped { display: block; white-space: nowrap; text-overflow: ellipsis; }
  .system-banner.collapsed { align-items: center; }
  .system-banner.inspectable p { display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 3; line-clamp: 3; overflow: hidden; }
  button { width: 23px; height: 23px; padding: 0; display: grid; place-items: center; flex: 0 0 auto; border: 0; border-radius: 7px; color: color-mix(in srgb, currentColor 66%, transparent); background: transparent; cursor: pointer; }
  button:hover { color: var(--banner-tone); background: color-mix(in srgb, var(--banner-tone) 9%, transparent); }
  button:focus-visible { outline: 2px solid color-mix(in srgb, var(--banner-tone) 60%, transparent); outline-offset: 1px; }
  @media (prefers-reduced-motion: reduce) { .system-banner { transition: none; } }
</style>
