const MARGIN = 8;

/**
 * Keeps a `position: fixed` menu inside the window. The menu is placed where it was asked for (a click), then
 * measured, because its height is not known beforehand (text size row, submenus, a confirmation), and moved
 * back in when it would run past an edge. It is placed again when its size or the window's changes.
 */
export function keepInView(node: HTMLElement, position: { x: number; y: number }) {
  let wanted = position;
  const place = () => {
    const { width, height } = node.getBoundingClientRect();
    const left = Math.max(MARGIN, Math.min(wanted.x, window.innerWidth - width - MARGIN));
    const top = Math.max(MARGIN, Math.min(wanted.y, window.innerHeight - height - MARGIN));
    node.style.left = `${left}px`;
    node.style.top = `${top}px`;
  };
  place();
  const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(place);
  observer?.observe(node);
  window.addEventListener("resize", place);
  return {
    update(next: { x: number; y: number }) {
      wanted = next;
      place();
    },
    destroy() {
      observer?.disconnect();
      window.removeEventListener("resize", place);
    },
  };
}
