/** Match the Orb's scroll feedback without keeping a visible track at rest. */
export function transientScrollbar(node: HTMLElement) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const show = () => {
    node.classList.add("is-scrolling");
    clearTimeout(timer);
    timer = setTimeout(() => node.classList.remove("is-scrolling"), 700);
  };
  node.addEventListener("scroll", show, { passive: true });
  return {
    destroy() {
      clearTimeout(timer);
      node.removeEventListener("scroll", show);
      node.classList.remove("is-scrolling");
    },
  };
}
