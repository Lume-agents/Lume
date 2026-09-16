export function copyResolvedColorTokens(
  source: HTMLElement,
  target: HTMLElement,
  tokens: Array<{ source: string; target?: string }>,
) {
  const sourceStyles = getComputedStyle(source);
  const probe = document.createElement("span");
  probe.style.position = "absolute";
  probe.style.width = "0";
  probe.style.height = "0";
  probe.style.visibility = "hidden";
  probe.style.pointerEvents = "none";
  source.appendChild(probe);
  try {
    for (const token of tokens) {
      if (!sourceStyles.getPropertyValue(token.source).trim()) continue;
      probe.style.color = "";
      probe.style.color = `var(${token.source})`;
      const resolved = getComputedStyle(probe).color;
      if (resolved) target.style.setProperty(token.target ?? token.source, resolved);
    }
  } finally {
    probe.remove();
  }
}
