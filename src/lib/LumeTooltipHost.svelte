<script lang="ts">
  import { onMount, tick } from "svelte";

  const tooltipId = "lume-global-tooltip";
  const originalTitles = new Map<HTMLElement, string>();
  const originalDescriptions = new Map<HTMLElement, string | null>();
  let tooltip = $state<HTMLDivElement | null>(null);
  let anchor = $state<HTMLElement | null>(null);
  let text = $state("");
  let visible = $state(false);
  let placement = $state<"top" | "bottom">("top");
  let left = $state(0);
  let top = $state(0);
  let arrowLeft = $state(12);
  let pendingAnchor: HTMLElement | null = null;
  let showTimer: ReturnType<typeof setTimeout> | undefined;
  let hideTimer: ReturnType<typeof setTimeout> | undefined;

  function tooltipAnchor(target: EventTarget | null) {
    return target instanceof Element
      ? target.closest<HTMLElement>("[data-tooltip], [title], [data-lume-tooltip-anchor]")
      : null;
  }

  function tooltipText(target: HTMLElement) {
    const explicit = target.dataset.tooltip?.trim();
    const native = target.getAttribute("title")?.trim();
    if (native) {
      originalTitles.set(target, native);
      target.removeAttribute("title");
      target.dataset.lumeTooltipAnchor = "";
      if (!target.hasAttribute("aria-label")) target.setAttribute("aria-label", native);
    }
    return explicit || native || originalTitles.get(target) || "";
  }

  function clearTimers() {
    if (showTimer) clearTimeout(showTimer);
    if (hideTimer) clearTimeout(hideTimer);
    showTimer = undefined;
    hideTimer = undefined;
    pendingAnchor = null;
  }

  function restoreDescription(target: HTMLElement | null) {
    if (!target) return;
    const describedBy = originalDescriptions.get(target);
    if (describedBy) target.setAttribute("aria-describedby", describedBy);
    else target.removeAttribute("aria-describedby");
    originalDescriptions.delete(target);
  }

  async function place() {
    if (!anchor || !tooltip) return;
    const targetBounds = anchor.getBoundingClientRect();
    const tooltipWidth = tooltip.offsetWidth;
    const tooltipHeight = tooltip.offsetHeight;
    const viewportGap = 8;
    const gap = 9;
    const spaceAbove = targetBounds.top - viewportGap;
    const spaceBelow = window.innerHeight - targetBounds.bottom - viewportGap;
    placement = spaceAbove >= tooltipHeight + gap || spaceAbove >= spaceBelow ? "top" : "bottom";
    left = Math.max(
      viewportGap,
      Math.min(
        window.innerWidth - tooltipWidth - viewportGap,
        targetBounds.left + targetBounds.width / 2 - tooltipWidth / 2,
      ),
    );
    const desiredTop = placement === "top"
      ? targetBounds.top - tooltipHeight - gap
      : targetBounds.bottom + gap;
    top = Math.max(viewportGap, Math.min(window.innerHeight - tooltipHeight - viewportGap, desiredTop));
    arrowLeft = Math.max(10, Math.min(tooltipWidth - 10, targetBounds.left + targetBounds.width / 2 - left));
  }

  function show(target: HTMLElement, delay: number) {
    if (target === anchor && visible) {
      // Re-entering during the short hide delay must keep the current tooltip.
      if (hideTimer) clearTimeout(hideTimer);
      hideTimer = undefined;
      return;
    }
    if (target === pendingAnchor) return;
    const nextText = tooltipText(target);
    if (!nextText) return;
    clearTimers();
    if (anchor && anchor !== target) {
      restoreDescription(anchor);
      anchor = null;
      visible = false;
    }
    pendingAnchor = target;
    showTimer = setTimeout(async () => {
      pendingAnchor = null;
      if (!target.isConnected) return;
      anchor = target;
      text = nextText;
      visible = true;
      const describedBy = target.getAttribute("aria-describedby");
      originalDescriptions.set(target, describedBy);
      target.setAttribute("aria-describedby", [describedBy, tooltipId].filter(Boolean).join(" "));
      await tick();
      await place();
    }, delay);
  }

  function hide(delay = 60) {
    if (showTimer) clearTimeout(showTimer);
    showTimer = undefined;
    pendingAnchor = null;
    if (hideTimer) clearTimeout(hideTimer);
    hideTimer = setTimeout(() => {
      restoreDescription(anchor);
      anchor = null;
      visible = false;
      hideTimer = undefined;
    }, delay);
  }

  onMount(() => {
    const hoverDelay = 600;
    const focusDelay = 190;
    const pointerOver = (event: PointerEvent) => {
      const target = tooltipAnchor(event.target);
      if (!target) return;
      show(target, hoverDelay);
    };
    const pointerMove = (event: PointerEvent) => {
      const target = tooltipAnchor(event.target);
      if (target) show(target, hoverDelay);
    };
    const pointerOut = (event: PointerEvent) => {
      const target = tooltipAnchor(event.target);
      if (!target) return;
      const related = event.relatedTarget instanceof Node ? event.relatedTarget : null;
      if (related && target.contains(related)) return;
      hide();
    };
    const focusIn = (event: FocusEvent) => {
      const target = tooltipAnchor(event.target);
      if (target) show(target, focusDelay);
    };
    const focusOut = (event: FocusEvent) => {
      const target = tooltipAnchor(event.target);
      if (!target) return;
      const related = event.relatedTarget instanceof Node ? event.relatedTarget : null;
      if (!related || !target.contains(related)) hide(0);
    };
    const dismiss = () => hide(0);

    // Capture hover before controls with their own pointer handlers can stop bubbling.
    document.addEventListener("pointerover", pointerOver, true);
    document.addEventListener("pointermove", pointerMove, true);
    document.addEventListener("pointerout", pointerOut, true);
    document.addEventListener("pointerdown", dismiss);
    document.addEventListener("focusin", focusIn);
    document.addEventListener("focusout", focusOut);
    window.addEventListener("resize", dismiss);
    document.addEventListener("scroll", dismiss, true);
    return () => {
      clearTimers();
      document.removeEventListener("pointerover", pointerOver, true);
      document.removeEventListener("pointermove", pointerMove, true);
      document.removeEventListener("pointerout", pointerOut, true);
      document.removeEventListener("pointerdown", dismiss);
      document.removeEventListener("focusin", focusIn);
      document.removeEventListener("focusout", focusOut);
      window.removeEventListener("resize", dismiss);
      document.removeEventListener("scroll", dismiss, true);
      for (const [target, title] of originalTitles) {
        target.setAttribute("title", title);
        target.removeAttribute("data-lume-tooltip-anchor");
      }
      originalTitles.clear();
      originalDescriptions.clear();
    };
  });
</script>

{#if visible}
  <div
    bind:this={tooltip}
    id={tooltipId}
    class:bottom={placement === "bottom"}
    class="lume-tooltip"
    role="tooltip"
    style:left="{left}px"
    style:top="{top}px"
    style:--tooltip-arrow-left="{arrowLeft}px"
  >{text}</div>
{/if}

<style>
  .lume-tooltip {
    --tooltip-surface: var(--lume-raised-light);
    --tooltip-ink: var(--lume-ink-strong-light);
    --tooltip-line: color-mix(in srgb, var(--lume-accent-strong) 24%, var(--lume-line-light));
    position: fixed;
    z-index: 2147483647;
    max-width: min(260px, calc(100vw - 16px));
    padding: 6px 8px;
    pointer-events: none;
    border: 1px solid var(--tooltip-line);
    border-radius: 7px;
    color: var(--tooltip-ink);
    background: var(--tooltip-surface);
    box-shadow: 0 8px 24px color-mix(in srgb, var(--lume-canvas-dark) 18%, transparent);
    font: 650 10px/1.35 "Segoe UI Variable", "SF Pro Text", ui-sans-serif, system-ui, sans-serif;
    letter-spacing: .005em;
    overflow-wrap: anywhere;
    animation: tooltip-arrive 140ms cubic-bezier(.16, 1, .3, 1) both;
  }
  .lume-tooltip::after {
    position: absolute;
    top: 100%;
    left: var(--tooltip-arrow-left);
    width: 7px;
    height: 7px;
    border-right: 1px solid var(--tooltip-line);
    border-bottom: 1px solid var(--tooltip-line);
    background: var(--tooltip-surface);
    content: "";
    transform: translate(-50%, -4px) rotate(45deg);
  }
  .lume-tooltip.bottom::after {
    top: auto;
    bottom: 100%;
    transform: translate(-50%, 4px) rotate(225deg);
  }
  :global(:root[data-theme="dark"]) .lume-tooltip {
    --tooltip-surface: var(--lume-raised-dark);
    --tooltip-ink: var(--lume-ink-strong-dark);
    --tooltip-line: color-mix(in srgb, var(--lume-accent) 26%, var(--lume-line-dark));
  }
  @keyframes tooltip-arrive {
    from { opacity: 0; transform: translateY(2px) scale(.98); }
  }
  @media (prefers-reduced-motion: reduce) {
    .lume-tooltip { animation: none; }
  }
</style>
