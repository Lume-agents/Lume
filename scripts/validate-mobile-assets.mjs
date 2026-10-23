import { access, readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const mobileRoot = resolve(root, "mobile-pwa");
const errors = [];

async function requireFile(path, context) {
  try {
    await access(path);
  } catch {
    errors.push(`${context}: missing ${path.slice(root.length + 1)}`);
  }
}

// mobile-pwa/ is the build of mobile/ (npm run build:mobile): the files the desktop gateway embeds and
// GitHub Pages and Capacitor ship. The shared modules (markdown, response dedup) are bundled into app.js.
for (const entry of ["index.html", "app.js", "styles.css", "sw.js", "manifest.webmanifest", "lume-mobile-icon.svg"]) {
  const path = resolve(mobileRoot, entry);
  await requireFile(path, entry);
  let source = "";
  try {
    source = await readFile(path, "utf8");
  } catch {
    continue;
  }
  const imports = [
    ...source.matchAll(/\bfrom\s+["'](\.\/[^"']+)["']/g),
    ...source.matchAll(/\bimport\s+["'](\.\/[^"']+)["']/g),
  ];
  for (const match of imports) {
    await requireFile(resolve(dirname(path), match[1]), `${entry} import`);
  }
}

const serviceWorker = await readFile(resolve(mobileRoot, "sw.js"), "utf8");
for (const match of serviceWorker.matchAll(/["'](\.\/[^"']+)["']/g)) {
  if (match[1] === "./") continue;
  await requireFile(resolve(mobileRoot, match[1]), "service worker cache");
}

if (errors.length) {
  console.error(errors.map((error) => `- ${error}`).join("\n"));
  process.exit(1);
}

console.log("Mobile assets are complete and synchronized.");
