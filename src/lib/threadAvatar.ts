export type ThreadAvatarBody = "cube" | "capsule" | "gem" | "arch" | "sphere" | "stack";

export type ThreadAvatarEye = {
  cx: number;
  cy: number;
  rx: number;
  ry: number;
  rotate: number;
};

export type ThreadAvatarConfig = {
  body: ThreadAvatarBody;
  background: string;
  base: string;
  light: string;
  shade: string;
  deep: string;
  offsetX: number;
  offsetY: number;
  tilt: number;
  eyes: [ThreadAvatarEye, ThreadAvatarEye];
};

const palettes = [
  { background: "#dcefe5", base: "#61b789", light: "#9bd6b4", shade: "#398267", deep: "#245c4a" },
  { background: "#dcebf2", base: "#5ba6cf", light: "#a2d2e8", shade: "#3a739e", deep: "#28536f" },
  { background: "#f2e4d1", base: "#df9462", light: "#f2c299", shade: "#a65f42", deep: "#704231" },
  { background: "#ebe1f2", base: "#9876c6", light: "#c8b1e4", shade: "#675193", deep: "#473968" },
  { background: "#efe8c9", base: "#c8aa45", light: "#e5d68a", shade: "#8f7731", deep: "#655425" },
  { background: "#f0dfe2", base: "#cf7787", light: "#e7aeb8", shade: "#985263", deep: "#693a48" },
] as const;

const bodies: ThreadAvatarBody[] = ["cube", "capsule", "gem", "arch", "sphere", "stack"];

export function hashThreadAvatarSeed(value: string) {
  let hash = 2166136261;
  for (let index = 0; index < value.length; index += 1) {
    hash ^= value.charCodeAt(index);
    hash = Math.imul(hash, 16777619);
  }
  return hash >>> 0;
}

function eyeExpression(index: number): [ThreadAvatarEye, ThreadAvatarEye] {
  const expressions: [ThreadAvatarEye, ThreadAvatarEye][] = [
    [{ cx: 25, cy: 33, rx: 3.2, ry: 5.1, rotate: -4 }, { cx: 37, cy: 33, rx: 3.2, ry: 5.1, rotate: 4 }],
    [{ cx: 25, cy: 34, rx: 3.8, ry: 2.1, rotate: 5 }, { cx: 38, cy: 34, rx: 3.8, ry: 2.1, rotate: -5 }],
    [{ cx: 25, cy: 34, rx: 3.7, ry: 1.15, rotate: -7 }, { cx: 38, cy: 33, rx: 3.1, ry: 5, rotate: 5 }],
    [{ cx: 25, cy: 32, rx: 3.7, ry: 5.5, rotate: -8 }, { cx: 38, cy: 34, rx: 2.8, ry: 4.2, rotate: 8 }],
    [{ cx: 23.5, cy: 33, rx: 3.1, ry: 4.8, rotate: -3 }, { cx: 35.5, cy: 33, rx: 3.1, ry: 4.8, rotate: 3 }],
    [{ cx: 25, cy: 33, rx: 3.1, ry: 4.8, rotate: 14 }, { cx: 38, cy: 33, rx: 3.1, ry: 4.8, rotate: -14 }],
  ];
  return expressions[index % expressions.length] ?? expressions[0];
}

export function createThreadAvatarConfig(seed: string): ThreadAvatarConfig {
  const hash = hashThreadAvatarSeed(seed || "lume-thread");
  const palette = palettes[hash % palettes.length] ?? palettes[0];
  const body = bodies[(hash >>> 4) % bodies.length] ?? "cube";
  return {
    body,
    ...palette,
    offsetX: ((hash >>> 8) % 7) - 3,
    offsetY: ((hash >>> 11) % 5) - 2,
    tilt: ((hash >>> 14) % 9) - 4,
    eyes: eyeExpression((hash >>> 18) % 6),
  };
}
