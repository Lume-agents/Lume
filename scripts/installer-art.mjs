// Draws the Windows installer artwork (header and sidebar bitmaps) from the Lume dino, in the same pixel
// style as the website. Run `node scripts/installer-art.mjs`; the BMPs are committed.
import { writeFileSync } from "node:fs";

const BODY = "M4 16h3v-2h3V9h3V6h11v2h3v8h-9v2h4v4h-5v4h-4v-5h-3v5H7v-5H5v-2H3v-4h1z";
const BELLY = "M10 15h3v2h5v3h-8z";
const FEET = [[7, 25, 5, 2], [13, 25, 5, 2]];

/** Polygon of a rectilinear SVG path (M, h, v, H, V, z). */
function polygon(path) {
  const points = [];
  let x = 0, y = 0;
  for (const [, command, values] of path.matchAll(/([MhvHVz])([^MhvHVz]*)/g)) {
    const numbers = values.trim() ? values.trim().split(/[\s,]+/).map(Number) : [];
    if (command === "M") [x, y] = numbers;
    else if (command === "h") x += numbers[0];
    else if (command === "v") y += numbers[0];
    else if (command === "H") x = numbers[0];
    else if (command === "V") y = numbers[0];
    if (command !== "z") points.push([x, y]);
  }
  return points;
}

function inside(points, px, py) {
  let result = false;
  for (let i = 0, j = points.length - 1; i < points.length; j = i++) {
    const [xi, yi] = points[i], [xj, yj] = points[j];
    if ((yi > py) !== (yj > py) && px < ((xj - xi) * (py - yi)) / (yj - yi) + xi) result = !result;
  }
  return result;
}

class Canvas {
  constructor(width, height, color) {
    this.width = width; this.height = height;
    this.data = new Uint8Array(width * height * 3);
    for (let i = 0; i < width * height; i += 1) this.data.set(color, i * 3);
  }
  rect(x, y, w, h, color) {
    for (let j = Math.max(0, y); j < Math.min(this.height, y + h); j += 1)
      for (let i = Math.max(0, x); i < Math.min(this.width, x + w); i += 1) this.data.set(color, (j * this.width + i) * 3);
  }
  bmp() {
    const row = Math.ceil((this.width * 3) / 4) * 4;
    const out = Buffer.alloc(54 + row * this.height);
    out.write("BM"); out.writeUInt32LE(out.length, 2); out.writeUInt32LE(54, 10); out.writeUInt32LE(40, 14);
    out.writeInt32LE(this.width, 18); out.writeInt32LE(this.height, 22); out.writeUInt16LE(1, 26); out.writeUInt16LE(24, 28);
    out.writeUInt32LE(row * this.height, 34);
    for (let y = 0; y < this.height; y += 1)
      for (let x = 0; x < this.width; x += 1) {
        const [r, g, b] = this.data.subarray((y * this.width + x) * 3, (y * this.width + x) * 3 + 3);
        out.set([b, g, r], 54 + (this.height - 1 - y) * row + x * 3);
      }
    return out;
  }
}

const hex = (value) => [1, 3, 5].map((i) => parseInt(value.slice(i, i + 2), 16));
const mix = (a, b, t) => a.map((v, i) => Math.round(v + (b[i] - v) * t));

function dino(canvas, left, top, scale, { body = hex("#63a57d"), ink = hex("#20322c"), eye = hex("#eaf2ee") } = {}) {
  const outline = polygon(BODY), belly = polygon(BELLY);
  for (let cy = 0; cy < 32; cy += 1)
    for (let cx = 0; cx < 32; cx += 1) {
      if (!inside(outline, cx + 0.5, cy + 0.5)) continue;
      canvas.rect(left + cx * scale, top + cy * scale, scale, scale, inside(belly, cx + 0.5, cy + 0.5) ? mix(body, [255, 255, 255], 0.26) : body);
    }
  const cell = (x, y, w, h, color) => canvas.rect(left + x * scale, top + y * scale, w * scale, h * scale, color);
  cell(23, 13, 4, 2, mix(body, hex("#183027"), 0.3));
  cell(21, 9, 3, 3, eye);
  cell(23, 9, 1, 2, ink);
  for (const [x, y, w, h] of FEET) cell(x, y, w, h, ink);
}

const FONT = {
  l: ["01100", "00100", "00100", "00100", "00100", "00100", "01110"],
  u: ["00000", "00000", "10001", "10001", "10001", "10011", "01101"],
  m: ["00000", "00000", "11010", "10101", "10101", "10101", "10101"],
  e: ["00000", "00000", "01110", "10001", "11111", "10000", "01110"],
};

function text(canvas, word, left, top, scale, color) {
  [...word].forEach((letter, index) => FONT[letter].forEach((row, y) => [...row].forEach((on, x) => {
    if (on === "1") canvas.rect(left + (index * 6 + x) * scale, top + y * scale, scale, scale, color);
  })));
}

const DARK = hex("#101d17");
const PAPER = hex("#e8ebdf");

// Sidebar: 164 x 314, a night-green gradient with the dino and the name.
const sidebar = new Canvas(164, 314, DARK);
for (let y = 0; y < 314; y += 1) sidebar.rect(0, y, 164, 1, mix(hex("#172720"), hex("#0c1511"), y / 313));
let seed = 7;
const random = () => ((seed = (seed * 1103515245 + 12345) & 0x7fffffff) / 0x7fffffff);
for (let i = 0; i < 26; i += 1) sidebar.rect(Math.floor(random() * 162), Math.floor(random() * 300), 1, 1, mix(hex("#3b5a47"), hex("#8fb89c"), random()));
dino(sidebar, 18, 56, 4);
sidebar.rect(18, 190, 128, 1, hex("#2c4636"));
text(sidebar, "lume", 36, 212, 4, hex("#e4eadf"));
writeFileSync(new URL("../src-tauri/installer/sidebar.bmp", import.meta.url), sidebar.bmp());

// Header: 150 x 57, the dino and the name on the installer's light paper.
const header = new Canvas(150, 57, PAPER);
dino(header, 106, 12, 1, { body: hex("#4f8a67") });
text(header, "lume", 14, 19, 3, hex("#18231b"));
writeFileSync(new URL("../src-tauri/installer/header.bmp", import.meta.url), header.bmp());
console.log("installer artwork written");
