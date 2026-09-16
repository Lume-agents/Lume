<script lang="ts">
  import { onMount } from "svelte";
  import {
    MODE_FRAMES,
    paintFrame,
    resolvePreset,
    type Dot,
    type Line,
    type OrbFrame,
    type OrbSize,
    type OrbState,
  } from "thinking-orbs/engine";

  export type ThinkingOrbState = OrbState;

  let {
    state: orbState = "breathing",
    size = 40,
    speed = 1,
    paused = false,
    label = "Thinking",
  } = $props<{
    state?: ThinkingOrbState;
    size?: number;
    speed?: number;
    paused?: boolean;
    label?: string;
  }>();

  let canvas = $state<HTMLCanvasElement | null>(null);
  let requestedState: ThinkingOrbState = "breathing";
  let renderedFrame: OrbFrame | null = null;
  let transitionFrom: OrbFrame | null = null;
  let transitionStartedAt = 0;
  let syncAnimation: (() => void) | undefined;

  function presetFor(renderSize: number): OrbSize {
    return renderSize <= 24 ? 20 : 64;
  }

  function frameFor(state: ThinkingOrbState, presetSize: OrbSize, now: number): OrbFrame {
    const preset = resolvePreset(state, presetSize);
    return MODE_FRAMES[preset.mode](presetSize, now / 1_000 * preset.speed * speed, preset.opts);
  }

  function ease(value: number) {
    const clamped = Math.max(0, Math.min(1, value));
    return 1 - Math.pow(1 - clamped, 3);
  }

  function dotAt(dots: Dot[], index: number, length: number, center: number): Dot {
    if (!dots.length) return { x: center, y: center, z: 0, r: 0, white: 0.5, a: 0 };
    const sourceIndex = length <= 1 ? 0 : Math.round(index * (dots.length - 1) / (length - 1));
    return dots[sourceIndex];
  }

  function lineAt(lines: Line[], index: number, length: number, center: number): Line {
    if (!lines.length) return { x1: center, y1: center, x2: center, y2: center, white: 0.5, a: 0, w: 0 };
    const sourceIndex = length <= 1 ? 0 : Math.round(index * (lines.length - 1) / (length - 1));
    return lines[sourceIndex];
  }

  function interpolateFrame(from: OrbFrame, to: OrbFrame, amount: number, presetSize: number): OrbFrame {
    const center = presetSize / 2;
    const dotCount = Math.max(from.dots.length, to.dots.length);
    const lineCount = Math.max(from.lines.length, to.lines.length);
    const lerp = (start: number, end: number) => start + (end - start) * amount;
    return {
      dots: Array.from({ length: dotCount }, (_, index) => {
        const source = dotAt(from.dots, index, dotCount, center);
        const target = dotAt(to.dots, index, dotCount, center);
        return {
          x: lerp(source.x, target.x), y: lerp(source.y, target.y), z: lerp(source.z, target.z),
          r: lerp(source.r, target.r), white: lerp(source.white, target.white),
          a: lerp(source.a ?? 1, target.a ?? 1),
        };
      }),
      lines: Array.from({ length: lineCount }, (_, index) => {
        const source = lineAt(from.lines, index, lineCount, center);
        const target = lineAt(to.lines, index, lineCount, center);
        return {
          x1: lerp(source.x1, target.x1), y1: lerp(source.y1, target.y1),
          x2: lerp(source.x2, target.x2), y2: lerp(source.y2, target.y2),
          white: lerp(source.white, target.white), a: lerp(source.a ?? 1, target.a ?? 1),
          w: lerp(source.w, target.w),
        };
      }),
    };
  }

  $effect(() => {
    const nextState = orbState;
    if (nextState === requestedState) return;
    transitionFrom = renderedFrame
      ? { dots: renderedFrame.dots.map((dot) => ({ ...dot })), lines: renderedFrame.lines.map((line) => ({ ...line })) }
      : null;
    transitionStartedAt = typeof performance === "undefined" ? 0 : performance.now();
    requestedState = nextState;
    syncAnimation?.();
  });

  $effect(() => {
    paused;
    size;
    speed;
    syncAnimation?.();
  });

  onMount(() => {
    if (!canvas) return;
    const context = canvas.getContext("2d");
    if (!context) return;
    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
    let intersecting = true;
    let frameId = 0;
    let running = false;

    function draw(now: number) {
      if (!canvas || !context) return;
      const presetSize = presetFor(size);
      const dpr = Math.min(2, window.devicePixelRatio || 1);
      const pixelSize = Math.max(1, Math.round(size * dpr));
      if (canvas.width !== pixelSize || canvas.height !== pixelSize) {
        canvas.width = pixelSize;
        canvas.height = pixelSize;
      }
      const scale = size / presetSize;
      context.setTransform(dpr * scale, 0, 0, dpr * scale, 0, 0);
      context.clearRect(0, 0, presetSize, presetSize);

      const target = frameFor(requestedState, presetSize, reducedMotion.matches ? 420 : now);
      const progress = transitionFrom && !reducedMotion.matches
        ? ease((now - transitionStartedAt) / 320)
        : 1;
      renderedFrame = transitionFrom && progress < 1
        ? interpolateFrame(transitionFrom, target, progress, presetSize)
        : target;
      if (progress >= 1) transitionFrom = null;
      paintFrame(context, renderedFrame, Boolean(canvas.closest(".dark")));
    }

    function loop(now: number) {
      draw(now);
      if (running) frameId = requestAnimationFrame(loop);
    }

    syncAnimation = () => {
      const shouldRun = !paused && !reducedMotion.matches && intersecting && document.visibilityState !== "hidden";
      if (shouldRun && !running) {
        running = true;
        frameId = requestAnimationFrame(loop);
      } else if (!shouldRun && running) {
        running = false;
        cancelAnimationFrame(frameId);
        draw(performance.now());
      } else if (!shouldRun) {
        draw(performance.now());
      }
    };

    const observer = new IntersectionObserver(([entry]) => {
      intersecting = entry.isIntersecting;
      syncAnimation?.();
    });
    const onVisibilityChange = () => syncAnimation?.();
    const onMotionChange = () => syncAnimation?.();
    observer.observe(canvas);
    document.addEventListener("visibilitychange", onVisibilityChange);
    reducedMotion.addEventListener("change", onMotionChange);
    draw(performance.now());
    syncAnimation();

    return () => {
      running = false;
      cancelAnimationFrame(frameId);
      observer.disconnect();
      document.removeEventListener("visibilitychange", onVisibilityChange);
      reducedMotion.removeEventListener("change", onMotionChange);
      syncAnimation = undefined;
    };
  });
</script>

<span class="orb-accessibility">{label}</span>
<canvas bind:this={canvas} aria-hidden="true" style:width={`${size}px`} style:height={`${size}px`}></canvas>

<style>
  canvas { display: block; flex: 0 0 auto; }
  .orb-accessibility { position: absolute; width: 1px; height: 1px; padding: 0; overflow: hidden; clip: rect(0, 0, 0, 0); white-space: nowrap; border: 0; }
</style>
