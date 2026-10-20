/** Moves a node to `document.body`, so a fixed overlay is never clipped or offset by a transformed ancestor. */
export function portal(node: HTMLElement) {
  node.ownerDocument.body.appendChild(node);
  return { destroy() { node.remove(); } };
}
