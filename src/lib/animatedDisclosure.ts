/** Keep native details/summary semantics while revealing its content smoothly. */
export function animatedDisclosure(details: HTMLDetailsElement) {
  const summary = details.querySelector<HTMLElement>(":scope > summary");
  const content = details.querySelector<HTMLElement>(":scope > .settings-section-content");
  if (!summary || !content) return;

  let targetOpen = details.open;
  let heightAnimation: Animation | undefined;
  let contentAnimation: Animation | undefined;
  let generation = 0;
  const originalOverflow = details.style.overflow;

  function settle(open: boolean) {
    details.open = open;
    heightAnimation?.cancel();
    contentAnimation?.cancel();
    heightAnimation = undefined;
    contentAnimation = undefined;
    details.style.overflow = originalOverflow;
  }

  function toggle(event: MouseEvent) {
    if (!summary || !content || event.defaultPrevented || event.button !== 0) return;
    event.preventDefault();
    const current = ++generation;
    const wasAnimating = Boolean(heightAnimation);
    const wasOpen = details.open;
    const startHeight = details.getBoundingClientRect().height;
    const contentStyle = getComputedStyle(content);
    const startOpacity = wasAnimating ? Number(contentStyle.opacity) : wasOpen ? 1 : 0;
    const startTransform = wasAnimating ? contentStyle.transform : wasOpen ? "translateY(0)" : "translateY(-5px)";
    targetOpen = !targetOpen;
    summary!.setAttribute("aria-expanded", String(targetOpen));
    heightAnimation?.cancel();
    contentAnimation?.cancel();

    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches || !details.animate) {
      settle(targetOpen);
      return;
    }

    details.open = true;
    const border = details.offsetHeight - details.clientHeight;
    const endHeight = summary!.getBoundingClientRect().height + border
      + (targetOpen ? content!.getBoundingClientRect().height : 0);
    details.style.overflow = "hidden";
    const duration = targetOpen ? 220 : 160;
    const easing = "cubic-bezier(.16, 1, .3, 1)";
    heightAnimation = details.animate(
      [{ height: `${startHeight}px` }, { height: `${endHeight}px` }],
      { duration, easing, fill: "both" },
    );
    contentAnimation = content!.animate(
      [
        { opacity: startOpacity, transform: startTransform },
        { opacity: targetOpen ? 1 : 0, transform: targetOpen ? "translateY(0)" : "translateY(-5px)" },
      ],
      { duration: targetOpen ? 200 : 120, easing, fill: "both" },
    );
    contentAnimation.finished.catch(() => { /* Cancelling the paired fade is intentional too. */ });
    heightAnimation.finished.then(() => {
      if (current === generation) settle(targetOpen);
    }).catch(() => { /* Reversing a disclosure intentionally cancels its animation. */ });
  }

  summary.addEventListener("click", toggle);
  function syncNativeState() {
    if (heightAnimation) return;
    targetOpen = details.open;
    summary!.setAttribute("aria-expanded", String(targetOpen));
  }
  details.addEventListener("toggle", syncNativeState);
  return {
    destroy() {
      generation += 1;
      summary.removeEventListener("click", toggle);
      details.removeEventListener("toggle", syncNativeState);
      settle(targetOpen);
      summary.removeAttribute("aria-expanded");
    },
  };
}
