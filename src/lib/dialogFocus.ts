type DialogFocusOptions = {
  onDismiss: () => void;
  fallbackFocus?: () => HTMLElement | null;
};

/** Keep keyboard navigation in the active dialog, then return to its trigger. */
export function dialogFocus(node: HTMLElement, options: DialogFocusOptions) {
  const trigger = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  const originalTabIndex = node.getAttribute("tabindex");
  node.tabIndex = -1;
  node.focus({ preventScroll: true });

  const keydown = (event: KeyboardEvent) => {
    if (event.isComposing) return;
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      options.onDismiss();
      return;
    }
    if (event.key !== "Tab") return;
    const controls = Array.from(node.querySelectorAll<HTMLElement>(
      'a[href], button, input, select, textarea, [tabindex]',
    )).filter((element) => element.tabIndex >= 0
      && !element.matches(':disabled, [inert], [hidden]')
      && element.getClientRects().length > 0);
    const first = controls[0];
    const last = controls.at(-1);
    if (!first || !last) {
      event.preventDefault();
      node.focus();
    } else if (event.shiftKey && (document.activeElement === first || document.activeElement === node)) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && (document.activeElement === last || document.activeElement === node)) {
      event.preventDefault();
      first.focus();
    }
  };

  node.addEventListener("keydown", keydown);
  return {
    update(next: DialogFocusOptions) { options = next; },
    destroy() {
      node.removeEventListener("keydown", keydown);
      if (originalTabIndex === null) node.removeAttribute("tabindex");
      else node.setAttribute("tabindex", originalTabIndex);
      queueMicrotask(() => {
        const target = trigger?.isConnected && trigger !== document.body
          ? trigger
          : options.fallbackFocus?.();
        target?.focus({ preventScroll: true });
      });
    },
  };
}
