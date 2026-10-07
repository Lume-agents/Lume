import { cubicOut } from "svelte/easing";

type LauncherTransitionOptions = {
  duration?: number;
  radius?: number;
};

type TransitionDirection = "in" | "out" | "both";

export function sessionLauncherTransition(
  node: Element,
  { duration = 190, radius = 14 }: LauncherTransitionOptions = {},
  { direction }: { direction: TransitionDirection },
) {
  const reduceMotion =
    typeof window !== "undefined" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  const opensAbove = node.getAttribute("data-placement") === "above";

  return {
    duration: reduceMotion ? 110 : direction === "out" ? 125 : duration,
    easing: cubicOut,
    css: (progress: number) => {
      if (reduceMotion) return `opacity: ${progress};`;

      const reveal = ((1 - progress) * 14).toFixed(2);
      const clip = opensAbove
        ? `${reveal}% 0 0 0`
        : `0 0 ${reveal}% 0`;
      const offset = (1 - progress) * (opensAbove ? 6 : -6);
      const origin = opensAbove ? "bottom" : "top";

      return `opacity: ${progress}; transform: translate3d(0, ${offset}px, 0) scale(${0.985 + progress * 0.015}); clip-path: inset(${clip} round ${radius}px); transform-origin: ${origin} right;`;
    },
  };
}
